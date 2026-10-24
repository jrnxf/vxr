//! vxr embed viewer spike: `.vox` in, ray-marched brick map out. WebGPU only.
//! Errors are small integer codes so no formatting code ends up in the binary.

use std::cell::RefCell;
use wasm_bindgen::prelude::*;

mod vox;

const SHADER: &str = include_str!("shader.wgsl");

pub const E_ADAPTER: u32 = 1;
pub const E_DEVICE: u32 = 2;
pub const E_SURFACE: u32 = 3;
pub const E_NO_GPU: u32 = 4;
pub const E_FRAME: u32 = 5;

struct Gpu {
    instance: wgpu::Instance,
    adapter: wgpu::Adapter,
    device: wgpu::Device,
    queue: wgpu::Queue,
    march: Option<wgpu::ComputePipeline>,
    blits: Vec<(wgpu::TextureFormat, wgpu::RenderPipeline)>,
    module: Option<wgpu::ShaderModule>,
}

thread_local! {
    // One device for every <voxel-scene> on the page.
    static GPU: RefCell<Option<Gpu>> = const { RefCell::new(None) };
    static WAITERS: RefCell<Vec<Option<js_sys::Function>>> = const { RefCell::new(Vec::new()) };
}

/// Creates the shared adapter and device. Resolves to 0 or an error code.
#[wasm_bindgen]
pub async fn gpu_init() -> u32 {
    if GPU.with(|g| g.borrow().is_some()) {
        return 0;
    }
    let instance = wgpu::Instance::new(wgpu::InstanceDescriptor {
        backends: wgpu::Backends::BROWSER_WEBGPU,
        ..wgpu::InstanceDescriptor::new_without_display_handle()
    });
    let Ok(adapter) = instance.request_adapter(&wgpu::RequestAdapterOptions::default()).await else {
        return E_ADAPTER;
    };
    let Ok((device, queue)) = adapter
        .request_device(&wgpu::DeviceDescriptor { required_limits: wgpu::Limits::default(), ..Default::default() })
        .await
    else {
        return E_DEVICE;
    };
    GPU.with(|g| {
        *g.borrow_mut() = Some(Gpu { instance, adapter, device, queue, march: None, blits: Vec::new(), module: None })
    });
    0
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

fn digits(mut v: u32) -> String {
    let mut b = [0u8; 10];
    let mut i = b.len();
    loop {
        i -= 1;
        b[i] = b'0' + (v % 10) as u8;
        v /= 10;
        if v == 0 {
            break;
        }
    }
    b[i..].iter().map(|&c| c as char).collect()
}

struct SceneGpu {
    bufs: [wgpu::Buffer; 4],
    dims: [u32; 3],
    ext: [f32; 3],
}

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
    cam: wgpu::Buffer,
    scene: Option<SceneGpu>,
    target: Option<Target>,
}

#[wasm_bindgen]
impl Viewer {
    /// Creates the canvas surface and, once per page, the pipelines.
    /// `nonce` other than 0 makes the shader unique so no shader cache can help.
    #[wasm_bindgen(constructor)]
    pub fn new(canvas: web_sys::HtmlCanvasElement, nonce: u32) -> Result<Viewer, JsValue> {
        GPU.with(|g| {
            let mut g = g.borrow_mut();
            let g = g.as_mut().ok_or(JsValue::from(E_NO_GPU))?;
            let surface = g
                .instance
                .create_surface(wgpu::SurfaceTarget::Canvas(canvas))
                .map_err(|_| JsValue::from(E_SURFACE))?;
            let format = *surface.get_capabilities(&g.adapter).formats.first().ok_or(JsValue::from(E_SURFACE))?;
            if g.module.is_none() {
                let src = if nonce != 0 { SHADER.replacen("4294967295u", &(digits(nonce) + "u"), 1) } else { SHADER.into() };
                g.module = Some(g.device.create_shader_module(wgpu::ShaderModuleDescriptor {
                    label: None,
                    source: wgpu::ShaderSource::Wgsl(src.into()),
                }));
            }
            let module = g.module.as_ref().unwrap();
            if g.march.is_none() {
                g.march = Some(g.device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
                    label: None,
                    layout: None,
                    module,
                    entry_point: Some("march"),
                    compilation_options: Default::default(),
                    cache: None,
                }));
            }
            if !g.blits.iter().any(|(f, _)| *f == format) {
                let p = g.device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                    label: None,
                    layout: None,
                    vertex: wgpu::VertexState {
                        module,
                        entry_point: Some("vs"),
                        compilation_options: Default::default(),
                        buffers: &[],
                    },
                    primitive: Default::default(),
                    depth_stencil: None,
                    multisample: Default::default(),
                    fragment: Some(wgpu::FragmentState {
                        module,
                        entry_point: Some("fs"),
                        compilation_options: Default::default(),
                        targets: &[Some(format.into())],
                    }),
                    multiview_mask: None,
                    cache: None,
                });
                g.blits.push((format, p));
            }
            let cam = g.device.create_buffer(&wgpu::BufferDescriptor {
                label: None,
                size: 112,
                usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            });
            Ok(Viewer { surface, format, cam, scene: None, target: None })
        })
    }

    /// Parses a `.vox` file and uploads its brick map. Returns the voxel count, or an error code
    /// with the top bit set.
    pub fn load(&mut self, bytes: &[u8]) -> u32 {
        let s = match vox::read(bytes) {
            Ok(s) => s,
            Err(e) => return e | 0x8000_0000,
        };
        GPU.with(|g| {
            let g = g.borrow();
            let g = g.as_ref().unwrap();
            let mk = |data: &[u32]| {
                let b = g.device.create_buffer(&wgpu::BufferDescriptor {
                    label: None,
                    size: (data.len().max(1) * 4) as u64,
                    usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST,
                    mapped_at_creation: false,
                });
                if !data.is_empty() {
                    g.queue.write_buffer(&b, 0, words(data));
                }
                b
            };
            self.scene = Some(SceneGpu {
                bufs: [mk(&s.grid), mk(&s.masks), mk(&s.colors), mk(&s.palette)],
                dims: s.dims,
                ext: [s.ext[0] as f32, s.ext[1] as f32, s.ext[2] as f32],
            });
            self.target = None;
        });
        s.voxels
    }

    /// Draws one frame. `yaw`/`pitch` in radians, `zoom` 1.0 frames the scene.
    pub fn render(&mut self, yaw: f32, pitch: f32, zoom: f32, w: u32, h: u32) -> u32 {
        let Some(sc) = self.scene.as_ref() else { return E_FRAME };
        if w == 0 || h == 0 {
            return E_FRAME;
        }
        GPU.with(|g| {
            let g = g.borrow();
            let g = g.as_ref().unwrap();
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
                // Pixels go through a storage buffer, not a storage texture: see shader.wgsl.
                let pix = g.device.create_buffer(&wgpu::BufferDescriptor {
                    label: None,
                    size: w as u64 * h as u64 * 4,
                    usage: wgpu::BufferUsages::STORAGE,
                    mapped_at_creation: false,
                });
                let march = g.march.as_ref().unwrap();
                let blit = &g.blits.iter().find(|(f, _)| *f == self.format).unwrap().1;
                let mbg = g.device.create_bind_group(&wgpu::BindGroupDescriptor {
                    label: None,
                    layout: &march.get_bind_group_layout(0),
                    entries: &[
                        e(0, self.cam.as_entire_binding()),
                        e(1, sc.bufs[0].as_entire_binding()),
                        e(2, sc.bufs[1].as_entire_binding()),
                        e(3, sc.bufs[2].as_entire_binding()),
                        e(4, sc.bufs[3].as_entire_binding()),
                        e(5, pix.as_entire_binding()),
                    ],
                });
                let bbg = g.device.create_bind_group(&wgpu::BindGroupDescriptor {
                    label: None,
                    layout: &blit.get_bind_group_layout(0),
                    entries: &[e(0, self.cam.as_entire_binding()), e(6, pix.as_entire_binding())],
                });
                self.target = Some(Target { w, h, march: mbg, blit: bbg });
            }
            let t = self.target.as_ref().unwrap();

            // Orbit camera around the scene centre, Z up.
            let c = [sc.ext[0] * 0.5, sc.ext[1] * 0.5, sc.ext[2] * 0.5];
            let r = 0.5 * (sc.ext[0] * sc.ext[0] + sc.ext[1] * sc.ext[1] + sc.ext[2] * sc.ext[2]).sqrt();
            let half_fov = 0.35f32;
            let aspect = w as f32 / h as f32;
            let fit = if aspect < 1.0 { 1.0 / aspect } else { 1.0 };
            let dist = r / half_fov.sin() * zoom * fit;
            let (sy, cy) = yaw.sin_cos();
            let (sp, cp) = pitch.sin_cos();
            let eye = [c[0] + dist * cp * cy, c[1] + dist * cp * sy, c[2] + dist * sp];
            let fwd = [-cp * cy, -cp * sy, -sp];
            let right = norm([fwd[1], -fwd[0], 0.0]);
            let up = [
                right[1] * fwd[2] - right[2] * fwd[1],
                right[2] * fwd[0] - right[0] * fwd[2],
                right[0] * fwd[1] - right[1] * fwd[0],
            ];
            let sun = norm([0.45, 0.3, 0.84]);
            let f = |v: [f32; 3], w: f32| [v[0].to_bits(), v[1].to_bits(), v[2].to_bits(), w.to_bits()];
            let mut u = [0u32; 28];
            u[0..4].copy_from_slice(&f(eye, half_fov.tan()));
            u[4..8].copy_from_slice(&f(fwd, aspect));
            u[8..12].copy_from_slice(&f(right, 0.0));
            u[12..16].copy_from_slice(&f(up, 0.0));
            u[16..20].copy_from_slice(&f(sun, 0.0));
            u[20..24].copy_from_slice(&[sc.dims[0], sc.dims[1], sc.dims[2], 0]);
            u[24..28].copy_from_slice(&[w, h, 0, 0]);
            g.queue.write_buffer(&self.cam, 0, words(&u));

            let frame = match self.surface.get_current_texture() {
                wgpu::CurrentSurfaceTexture::Success(f) | wgpu::CurrentSurfaceTexture::Suboptimal(f) => f,
                _ => return E_FRAME,
            };
            let view = frame.texture.create_view(&Default::default());
            let mut enc = g.device.create_command_encoder(&Default::default());
            {
                let mut cp = enc.begin_compute_pass(&Default::default());
                cp.set_pipeline(g.march.as_ref().unwrap());
                cp.set_bind_group(0, &t.march, &[]);
                cp.dispatch_workgroups(w.div_ceil(8), h.div_ceil(8), 1);
            }
            {
                let mut rp = enc.begin_render_pass(&wgpu::RenderPassDescriptor {
                    label: None,
                    color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                        view: &view,
                        depth_slice: None,
                        resolve_target: None,
                        ops: wgpu::Operations { load: wgpu::LoadOp::Clear(wgpu::Color::BLACK), store: wgpu::StoreOp::Store },
                    })],
                    ..Default::default()
                });
                rp.set_pipeline(&g.blits.iter().find(|(f, _)| *f == self.format).unwrap().1);
                rp.set_bind_group(0, &t.blit, &[]);
                rp.draw(0..3, 0..1);
            }
            g.queue.submit([enc.finish()]);
            g.queue.present(frame);
            0
        })
    }
}

fn e(binding: u32, resource: wgpu::BindingResource<'_>) -> wgpu::BindGroupEntry<'_> {
    wgpu::BindGroupEntry { binding, resource }
}

fn norm(v: [f32; 3]) -> [f32; 3] {
    let l = (v[0] * v[0] + v[1] * v[1] + v[2] * v[2]).sqrt().max(1e-9);
    [v[0] / l, v[1] / l, v[2] / l]
}

fn words(d: &[u32]) -> &[u8] {
    // SAFETY: u32 has no padding and any byte pattern is a valid u8.
    unsafe { core::slice::from_raw_parts(d.as_ptr() as *const u8, d.len() * 4) }
}
