//! The viewer the loader drives: one shared device for every `<voxel-scene>` on the page,
//! and one [`Viewer`] per element (its canvas, its scene on the GPU and its picture).

use std::cell::RefCell;
use std::sync::atomic::{AtomicBool, Ordering};

use vxr_render::{GpuMap, brickmap_layout};
use wasm_bindgen::prelude::*;

const EMBED_WGSL: &str = include_str!("embed.wgsl");
const BLIT_WGSL: &str = include_str!("blit.wgsl");

/// Half the vertical field of view, radians.
const HALF_FOV: f32 = vxr_core::bricks::VIEW_HALF_FOV;

struct Gpu {
    instance: wgpu::Instance,
    adapter: wgpu::Adapter,
    device: wgpu::Device,
    queue: wgpu::Queue,
    map_layout: wgpu::BindGroupLayout,
    view_layout: wgpu::BindGroupLayout,
    march: wgpu::ComputePipeline,
    blit_module: wgpu::ShaderModule,
    /// One blit pipeline per canvas format seen.
    blits: Vec<(wgpu::TextureFormat, wgpu::RenderPipeline)>,
}

thread_local! {
    static GPU: RefCell<Option<Gpu>> = const { RefCell::new(None) };
    static WAITERS: RefCell<Vec<Option<js_sys::Function>>> = const { RefCell::new(Vec::new()) };
}

/// The device reported an error nothing handled (out of memory, a lost device): every
/// viewer stops drawing and its element goes back to the poster.
static BROKEN: AtomicBool = AtomicBool::new(false);

fn buffer_entry(binding: u32, ty: wgpu::BufferBindingType) -> wgpu::BindGroupLayoutEntry {
    wgpu::BindGroupLayoutEntry {
        binding,
        visibility: wgpu::ShaderStages::COMPUTE,
        ty: wgpu::BindingType::Buffer { ty, has_dynamic_offset: false, min_binding_size: None },
        count: None,
    }
}

/// Make the page's one adapter and device and start compiling the march. Resolves to an
/// empty string, or to what went wrong.
#[wasm_bindgen]
pub async fn gpu_init() -> String {
    if GPU.with(|g| g.borrow().is_some()) {
        return String::new();
    }
    let instance = wgpu::Instance::new(wgpu::InstanceDescriptor {
        backends: wgpu::Backends::BROWSER_WEBGPU,
        ..wgpu::InstanceDescriptor::new_without_display_handle()
    });
    let Ok(adapter) = instance.request_adapter(&wgpu::RequestAdapterOptions::default()).await else {
        return "this browser's WebGPU has no adapter".into();
    };
    // The default limits: what every WebGPU browser offers, phones included.
    let Ok((device, queue)) = adapter.request_device(&wgpu::DeviceDescriptor::default()).await else {
        return "this browser's WebGPU gave no device".into();
    };
    device.on_uncaptured_error(std::sync::Arc::new(|_| BROKEN.store(true, Ordering::Relaxed)));
    let map_layout = brickmap_layout(&device);
    let view_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
        label: None,
        entries: &[
            buffer_entry(0, wgpu::BufferBindingType::Uniform),
            buffer_entry(1, wgpu::BufferBindingType::Storage { read_only: false }),
        ],
    });
    let mut source = String::with_capacity(vxr_render::renderer::BRICKMAP_WGSL.len() + EMBED_WGSL.len() + 1);
    source.push_str(vxr_render::renderer::BRICKMAP_WGSL);
    source.push('\n');
    source.push_str(EMBED_WGSL);
    let module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
        label: None,
        source: wgpu::ShaderSource::Wgsl(source.into()),
    });
    let layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
        label: None,
        bind_group_layouts: &[Some(&view_layout), Some(&map_layout)],
        immediate_size: 0,
    });
    let march = device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
        label: None,
        layout: Some(&layout),
        module: &module,
        entry_point: Some("main"),
        compilation_options: Default::default(),
        cache: None,
    });
    let blit_module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
        label: None,
        source: wgpu::ShaderSource::Wgsl(BLIT_WGSL.into()),
    });
    GPU.with(|g| {
        *g.borrow_mut() = Some(Gpu {
            instance,
            adapter,
            device,
            queue,
            map_layout,
            view_layout,
            march,
            blit_module,
            blits: Vec::new(),
        })
    });
    String::new()
}

/// Resolves once all GPU work submitted so far has finished.
#[wasm_bindgen]
pub fn gpu_done() -> js_sys::Promise {
    js_sys::Promise::new(&mut |resolve, _| {
        let id = WAITERS.with(|w| {
            let mut w = w.borrow_mut();
            w.push(Some(resolve));
            w.len() - 1
        });
        GPU.with(|g| {
            if let Some(g) = g.borrow().as_ref() {
                g.queue.on_submitted_work_done(move || {
                    let f = WAITERS.with(|w| w.borrow_mut().get_mut(id).and_then(|f| f.take()));
                    if let Some(f) = f {
                        let _ = f.call0(&JsValue::NULL);
                    }
                });
            }
        });
    })
}

/// A scene on the GPU and the ball around its voxels, which the camera frames.
struct Scene {
    map: GpuMap,
    centre: [f32; 3],
    radius: f32,
}

/// The picture at one canvas size.
struct Target {
    w: u32,
    h: u32,
    march: wgpu::BindGroup,
    blit: wgpu::BindGroup,
}

#[wasm_bindgen]
pub struct Viewer {
    surface: wgpu::Surface<'static>,
    format: wgpu::TextureFormat,
    view: wgpu::Buffer,
    res: wgpu::Buffer,
    scene: Option<Scene>,
    target: Option<Target>,
}

#[wasm_bindgen]
impl Viewer {
    /// A viewer drawing on `canvas`. Call [`gpu_init`] first.
    #[wasm_bindgen(constructor)]
    pub fn new(canvas: web_sys::HtmlCanvasElement) -> Result<Viewer, String> {
        GPU.with(|g| {
            let mut g = g.borrow_mut();
            let g = g.as_mut().ok_or("the viewer has no GPU device")?;
            let surface = g
                .instance
                .create_surface(wgpu::SurfaceTarget::Canvas(canvas))
                .map_err(|_| "the canvas cannot show WebGPU")?;
            let format =
                *surface.get_capabilities(&g.adapter).formats.first().ok_or("the canvas cannot show WebGPU")?;
            if !g.blits.iter().any(|(f, _)| *f == format) {
                let blit = g.device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                    label: None,
                    layout: None,
                    vertex: wgpu::VertexState {
                        module: &g.blit_module,
                        entry_point: Some("vs"),
                        compilation_options: Default::default(),
                        buffers: &[],
                    },
                    primitive: Default::default(),
                    depth_stencil: None,
                    multisample: Default::default(),
                    fragment: Some(wgpu::FragmentState {
                        module: &g.blit_module,
                        entry_point: Some("fs"),
                        compilation_options: Default::default(),
                        targets: &[Some(format.into())],
                    }),
                    multiview_mask: None,
                    cache: None,
                });
                g.blits.push((format, blit));
            }
            let uniform = |size| {
                g.device.create_buffer(&wgpu::BufferDescriptor {
                    label: None,
                    size,
                    usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
                    mapped_at_creation: false,
                })
            };
            Ok(Viewer { surface, format, view: uniform(96), res: uniform(16), scene: None, target: None })
        })
    }

    /// Read a brick scene file or a `.vox` file and upload it. Returns an empty string, or
    /// what is wrong with the file. Bytes from anywhere are safe to pass.
    pub fn load(&mut self, bytes: &[u8]) -> String {
        let (map, palette) = match vxr_core::bricks::open(bytes) {
            Ok(scene) => scene,
            Err(e) => return e,
        };
        let Some((centre, radius)) = vxr_core::bricks::framing(&map) else {
            return "the scene has no voxels to show".into();
        };
        GPU.with(|g| {
            let g = g.borrow();
            let Some(g) = g.as_ref() else { return "the viewer has no GPU device".into() };
            match GpuMap::new(&g.device, &g.queue, &g.map_layout, &map, &palette) {
                Ok(map) => {
                    self.scene = Some(Scene { map, centre, radius });
                    String::new()
                }
                Err(e) => e,
            }
        })
    }

    /// Draw one frame of `w` by `h` pixels from an orbit around the scene: `yaw` and
    /// `pitch` in radians, `zoom` 1 fitting the whole scene. False when nothing was drawn
    /// (no scene yet, no size, or the device failed): the element then keeps its poster.
    pub fn render(&mut self, yaw: f32, pitch: f32, zoom: f32, w: u32, h: u32) -> bool {
        let Some(scene) = self.scene.as_ref() else { return false };
        if w == 0 || h == 0 || BROKEN.load(Ordering::Relaxed) {
            return false;
        }
        GPU.with(|g| {
            let g = g.borrow();
            let Some(g) = g.as_ref() else { return false };
            let Some(blit) = g.blits.iter().find(|(f, _)| *f == self.format).map(|(_, p)| p) else { return false };
            if self.target.as_ref().is_none_or(|t| t.w != w || t.h != h) {
                self.surface.configure(
                    &g.device,
                    &wgpu::SurfaceConfiguration {
                        usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
                        format: self.format,
                        color_space: wgpu::SurfaceColorSpace::Auto,
                        width: w,
                        height: h,
                        desired_maximum_frame_latency: 2,
                        present_mode: wgpu::PresentMode::Fifo,
                        alpha_mode: wgpu::CompositeAlphaMode::Opaque,
                        view_formats: Vec::new(),
                    },
                );
                let img = g.device.create_buffer(&wgpu::BufferDescriptor {
                    label: None,
                    size: w as u64 * h as u64 * 4,
                    usage: wgpu::BufferUsages::STORAGE,
                    mapped_at_creation: false,
                });
                let group = |layout: &wgpu::BindGroupLayout, first: &wgpu::Buffer| {
                    g.device.create_bind_group(&wgpu::BindGroupDescriptor {
                        label: None,
                        layout,
                        entries: &[
                            wgpu::BindGroupEntry { binding: 0, resource: first.as_entire_binding() },
                            wgpu::BindGroupEntry { binding: 1, resource: img.as_entire_binding() },
                        ],
                    })
                };
                g.queue.write_buffer(&self.res, 0, words(&[w, h, 0, 0]));
                self.target = Some(Target {
                    w,
                    h,
                    march: group(&g.view_layout, &self.view),
                    blit: group(&blit.get_bind_group_layout(0), &self.res),
                });
            }
            let Some(target) = self.target.as_ref() else { return false };

            // An orbit around the middle of the voxels, y up, far enough to fit their ball
            // in the shorter side of the picture.
            let aspect = w as f32 / h as f32;
            let dist = vxr_core::bricks::view_distance(scene.radius, aspect) * zoom;
            let (sy, cy) = yaw.sin_cos();
            let (sp, cp) = pitch.sin_cos();
            let fwd = [-cp * sy, -sp, -cp * cy];
            let eye: [f32; 3] = std::array::from_fn(|a| scene.centre[a] - fwd[a] * dist);
            let right = norm([-fwd[2], 0.0, fwd[0]]);
            let up = [
                right[1] * fwd[2] - right[2] * fwd[1],
                right[2] * fwd[0] - right[0] * fwd[2],
                right[0] * fwd[1] - right[1] * fwd[0],
            ];
            let sun = norm([0.45, 0.84, 0.3]);
            let row = |v: [f32; 3], w: f32| [v[0].to_bits(), v[1].to_bits(), v[2].to_bits(), w.to_bits()];
            let mut u = [0u32; 24];
            u[0..4].copy_from_slice(&row(eye, HALF_FOV.tan()));
            u[4..8].copy_from_slice(&row(fwd, aspect));
            u[8..12].copy_from_slice(&row(right, 0.0));
            u[12..16].copy_from_slice(&row(up, 0.0));
            u[16..20].copy_from_slice(&row(sun, 0.0));
            u[20..24].copy_from_slice(&[w, h, 0, 0]);
            g.queue.write_buffer(&self.view, 0, words(&u));

            let frame = match self.surface.get_current_texture() {
                wgpu::CurrentSurfaceTexture::Success(f) | wgpu::CurrentSurfaceTexture::Suboptimal(f) => f,
                _ => return false,
            };
            let view = frame.texture.create_view(&Default::default());
            let mut enc = g.device.create_command_encoder(&Default::default());
            {
                let mut pass = enc.begin_compute_pass(&Default::default());
                pass.set_pipeline(&g.march);
                pass.set_bind_group(0, &target.march, &[]);
                pass.set_bind_group(1, &scene.map.bind_group, &[]);
                pass.dispatch_workgroups(w.div_ceil(8), h.div_ceil(8), 1);
            }
            {
                let mut pass = enc.begin_render_pass(&wgpu::RenderPassDescriptor {
                    label: None,
                    color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                        view: &view,
                        depth_slice: None,
                        resolve_target: None,
                        ops: wgpu::Operations {
                            load: wgpu::LoadOp::Clear(wgpu::Color::BLACK),
                            store: wgpu::StoreOp::Store,
                        },
                    })],
                    ..Default::default()
                });
                pass.set_pipeline(blit);
                pass.set_bind_group(0, &target.blit, &[]);
                pass.draw(0..3, 0..1);
            }
            g.queue.submit([enc.finish()]);
            g.queue.present(frame);
            true
        })
    }
}

fn norm(v: [f32; 3]) -> [f32; 3] {
    let l = (v[0] * v[0] + v[1] * v[1] + v[2] * v[2]).sqrt().max(1e-9);
    [v[0] / l, v[1] / l, v[2] / l]
}

fn words(d: &[u32]) -> &[u8] {
    bytemuck::cast_slice(d)
}
