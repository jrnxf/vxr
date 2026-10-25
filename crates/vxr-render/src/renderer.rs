//! The editor view: a compute pass that ray marches the brick map into a color texture
//! (and optionally a G-buffer), then a blit of that texture to any target.

use crate::camera::OrbitCamera;
use crate::map::GpuMap;

pub const BRICKMAP_WGSL: &str = include_str!("brickmap.wgsl");
const MARCH_WGSL: &str = include_str!("march.wgsl");
const BLIT_WGSL: &str = include_str!("blit.wgsl");
const WORKGROUP: (u32, u32) = (8, 8);

#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
struct CameraUniform {
    inv_vp: [[f32; 4]; 4],
    eye: [f32; 4],
    size: [f32; 4],
    sun: [f32; 4],
    cursor: [i32; 4],
    select: [u32; 4],
}

pub struct Renderer {
    pub width: u32,
    pub height: u32,
    /// Also write the depth and info G-buffer (for the path-traced preview).
    gbuffer: bool,
    cam: wgpu::Buffer,
    out_layout: wgpu::BindGroupLayout,
    out_group: wgpu::BindGroup,
    pub color: wgpu::Texture,
    pub depth: wgpu::Texture,
    pub info: wgpu::Texture,
    march: wgpu::ComputePipeline,
    blit: wgpu::RenderPipeline,
    blit_layout: wgpu::BindGroupLayout,
    blit_group: wgpu::BindGroup,
    sampler: wgpu::Sampler,
    /// The selection mask (see `march.wgsl`) and how many of its words are in use.
    sel: wgpu::Buffer,
    sel_layout: wgpu::BindGroupLayout,
    sel_group: wgpu::BindGroup,
    sel_words: u32,
}

impl Renderer {
    pub fn new(
        device: &wgpu::Device,
        map_layout: &wgpu::BindGroupLayout,
        target_format: wgpu::TextureFormat,
        width: u32,
        height: u32,
        gbuffer: bool,
    ) -> Self {
        let out_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("march out"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::COMPUTE,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                },
                storage_tex(1, wgpu::TextureFormat::Rgba8Unorm),
                storage_tex(2, wgpu::TextureFormat::R32Float),
                storage_tex(3, wgpu::TextureFormat::R32Uint),
            ],
        });
        let cam = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("camera"),
            size: std::mem::size_of::<CameraUniform>() as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let sel_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("selection"),
            entries: &[wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::COMPUTE,
                ty: wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Storage { read_only: true },
                    has_dynamic_offset: false,
                    min_binding_size: None,
                },
                count: None,
            }],
        });
        let (sel, sel_group) = selection_buffer(device, &sel_layout, 16);
        let module = march_module(device, format!("{BRICKMAP_WGSL}\n{MARCH_WGSL}"));
        let pl = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("march"),
            bind_group_layouts: &[Some(&out_layout), Some(map_layout), Some(&sel_layout)],
            immediate_size: 0,
        });
        let constants = [
            ("WRITE_GBUFFER", if gbuffer { 1.0 } else { 0.0 }),
            ("WG_X", WORKGROUP.0 as f64),
            ("WG_Y", WORKGROUP.1 as f64),
        ];
        let march = device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
            label: Some("march"),
            layout: Some(&pl),
            module: &module,
            entry_point: Some("main"),
            compilation_options: wgpu::PipelineCompilationOptions {
                constants: &constants,
                zero_initialize_workgroup_memory: false,
            },
            cache: None,
        });
        let blit_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("blit"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: true },
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                    count: None,
                },
            ],
        });
        let blit_module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("blit"),
            source: wgpu::ShaderSource::Wgsl(BLIT_WGSL.into()),
        });
        let blit_pl = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("blit"),
            bind_group_layouts: &[Some(&blit_layout)],
            immediate_size: 0,
        });
        let blit = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("blit"),
            layout: Some(&blit_pl),
            vertex: wgpu::VertexState {
                module: &blit_module,
                entry_point: Some("vs"),
                compilation_options: Default::default(),
                buffers: &[],
            },
            primitive: wgpu::PrimitiveState::default(),
            depth_stencil: None,
            multisample: wgpu::MultisampleState::default(),
            fragment: Some(wgpu::FragmentState {
                module: &blit_module,
                entry_point: Some("fs"),
                compilation_options: Default::default(),
                targets: &[Some(wgpu::ColorTargetState {
                    format: target_format,
                    blend: None,
                    write_mask: wgpu::ColorWrites::ALL,
                })],
            }),
            multiview_mask: None,
            cache: None,
        });
        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("blit"),
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            ..Default::default()
        });
        let t = Targets::new(device, &out_layout, &blit_layout, &sampler, &cam, width, height, gbuffer);
        Self {
            width,
            height,
            gbuffer,
            cam,
            out_layout,
            out_group: t.out_group,
            color: t.color,
            depth: t.depth,
            info: t.info,
            march,
            blit,
            blit_layout,
            blit_group: t.blit_group,
            sampler,
            sel,
            sel_layout,
            sel_group,
            sel_words: 0,
        }
    }

    /// Highlight the selection: one bit per voxel in the map's mask layout (16 words per
    /// brick slot), as `vxr_core::Scene::selection_mask` builds it. Empty clears it.
    pub fn set_selection(&mut self, device: &wgpu::Device, queue: &wgpu::Queue, words: &[u32]) {
        if words.len() as u64 * 4 > self.sel.size() {
            let size = (words.len() as u64 * 4).next_power_of_two();
            (self.sel, self.sel_group) = selection_buffer(device, &self.sel_layout, size);
        }
        if !words.is_empty() {
            queue.write_buffer(&self.sel, 0, bytemuck::cast_slice(words));
        }
        self.sel_words = words.len() as u32;
    }

    pub fn resize(&mut self, device: &wgpu::Device, width: u32, height: u32) {
        if (width, height) == (self.width, self.height) {
            return;
        }
        let t = Targets::new(
            device,
            &self.out_layout,
            &self.blit_layout,
            &self.sampler,
            &self.cam,
            width,
            height,
            self.gbuffer,
        );
        (self.color, self.depth, self.info, self.out_group, self.blit_group) =
            (t.color, t.depth, t.info, t.out_group, t.blit_group);
        self.width = width;
        self.height = height;
    }

    /// Set the view for the next march. `cursor` is a map cell to highlight, if any.
    pub fn set_camera(&self, queue: &wgpu::Queue, cam: &OrbitCamera, cursor: Option<[i32; 3]>) {
        let vp = cam.view_proj(self.width as f32 / self.height as f32);
        let eye = cam.eye();
        let sun = glam::Vec3::new(0.45, 0.8, 0.35).normalize();
        // eye.w = 0: orthographic, rays start on the near plane (see `primary_ray`).
        let c = cursor.unwrap_or([0; 3]);
        let u = CameraUniform {
            inv_vp: vp.inverse().to_cols_array_2d(),
            eye: [eye.x, eye.y, eye.z, if cam.ortho { 0.0 } else { 1.0 }],
            size: [self.width as f32, self.height as f32, 1.0 / self.width as f32, 1.0 / self.height as f32],
            sun: [sun.x, sun.y, sun.z, 0.0],
            cursor: [c[0], c[1], c[2], cursor.is_some() as i32],
            select: [self.sel_words, 0, 0, 0],
        };
        queue.write_buffer(&self.cam, 0, bytemuck::bytes_of(&u));
    }

    /// Ray march into the color (and G-buffer) textures.
    pub fn march(
        &self,
        encoder: &mut wgpu::CommandEncoder,
        map: &GpuMap,
        timestamps: Option<wgpu::ComputePassTimestampWrites<'_>>,
    ) {
        let mut pass = encoder
            .begin_compute_pass(&wgpu::ComputePassDescriptor { label: Some("march"), timestamp_writes: timestamps });
        pass.set_pipeline(&self.march);
        pass.set_bind_group(0, &self.out_group, &[]);
        pass.set_bind_group(1, &map.bind_group, &[]);
        pass.set_bind_group(2, &self.sel_group, &[]);
        pass.dispatch_workgroups(self.width.div_ceil(WORKGROUP.0), self.height.div_ceil(WORKGROUP.1), 1);
    }

    /// Draw the color texture onto `view` (window, canvas or offscreen), scaled to fit.
    pub fn blit(
        &self,
        encoder: &mut wgpu::CommandEncoder,
        view: &wgpu::TextureView,
        timestamps: Option<wgpu::RenderPassTimestampWrites<'_>>,
    ) {
        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("blit"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view,
                depth_slice: None,
                resolve_target: None,
                ops: wgpu::Operations { load: wgpu::LoadOp::Clear(wgpu::Color::BLACK), store: wgpu::StoreOp::Store },
            })],
            depth_stencil_attachment: None,
            timestamp_writes: timestamps,
            occlusion_query_set: None,
            multiview_mask: None,
        });
        pass.set_pipeline(&self.blit);
        pass.set_bind_group(0, &self.blit_group, &[]);
        pass.draw(0..3, 0..1);
    }

    /// Read the color texture back as tightly packed RGBA8 rows.
    pub async fn read_color(&self, device: &wgpu::Device, queue: &wgpu::Queue) -> Vec<u8> {
        crate::gpu::read_texture(device, queue, &self.color).await
    }
}

struct Targets {
    color: wgpu::Texture,
    depth: wgpu::Texture,
    info: wgpu::Texture,
    out_group: wgpu::BindGroup,
    blit_group: wgpu::BindGroup,
}

impl Targets {
    #[allow(clippy::too_many_arguments)]
    fn new(
        device: &wgpu::Device,
        out_layout: &wgpu::BindGroupLayout,
        blit_layout: &wgpu::BindGroupLayout,
        sampler: &wgpu::Sampler,
        cam: &wgpu::Buffer,
        w: u32,
        h: u32,
        gbuffer: bool,
    ) -> Self {
        let tex = |label, format, w: u32, h: u32| {
            device.create_texture(&wgpu::TextureDescriptor {
                label: Some(label),
                size: wgpu::Extent3d { width: w, height: h, depth_or_array_layers: 1 },
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format,
                usage: wgpu::TextureUsages::STORAGE_BINDING
                    | wgpu::TextureUsages::TEXTURE_BINDING
                    | wgpu::TextureUsages::COPY_SRC,
                view_formats: &[],
            })
        };
        // Without a G-buffer the depth and info bindings still need a texture: 1x1.
        let (gw, gh) = if gbuffer { (w, h) } else { (1, 1) };
        let color = tex("color", wgpu::TextureFormat::Rgba8Unorm, w, h);
        let depth = tex("depth", wgpu::TextureFormat::R32Float, gw, gh);
        let info = tex("info", wgpu::TextureFormat::R32Uint, gw, gh);
        let v = |t: &wgpu::Texture| t.create_view(&Default::default());
        let (cv, dv, iv) = (v(&color), v(&depth), v(&info));
        let out_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("march out"),
            layout: out_layout,
            entries: &[
                wgpu::BindGroupEntry { binding: 0, resource: cam.as_entire_binding() },
                wgpu::BindGroupEntry { binding: 1, resource: wgpu::BindingResource::TextureView(&cv) },
                wgpu::BindGroupEntry { binding: 2, resource: wgpu::BindingResource::TextureView(&dv) },
                wgpu::BindGroupEntry { binding: 3, resource: wgpu::BindingResource::TextureView(&iv) },
            ],
        });
        let blit_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("blit"),
            layout: blit_layout,
            entries: &[
                wgpu::BindGroupEntry { binding: 0, resource: wgpu::BindingResource::TextureView(&cv) },
                wgpu::BindGroupEntry { binding: 1, resource: wgpu::BindingResource::Sampler(sampler) },
            ],
        });
        Self { color, depth, info, out_group, blit_group }
    }
}

fn selection_buffer(
    device: &wgpu::Device,
    layout: &wgpu::BindGroupLayout,
    size: u64,
) -> (wgpu::Buffer, wgpu::BindGroup) {
    let buf = device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("selection"),
        size,
        usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
    });
    let group = device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some("selection"),
        layout,
        entries: &[wgpu::BindGroupEntry { binding: 0, resource: buf.as_entire_binding() }],
    });
    (buf, group)
}

/// The march shader. Natively, wgpu's runtime checks (buffer bounds checks and the counter
/// naga adds to every loop) are off unless `VXR_CHECKED=1`: the shader only indexes data the
/// renderer wrote, and every loop is bounded by MAX_STEPS or the 8^3 brick. Browsers always
/// apply their own checks.
fn march_module(device: &wgpu::Device, src: String) -> wgpu::ShaderModule {
    let desc = wgpu::ShaderModuleDescriptor { label: Some("march"), source: wgpu::ShaderSource::Wgsl(src.into()) };
    #[cfg(not(target_arch = "wasm32"))]
    if std::env::var("VXR_CHECKED").is_err() {
        // SAFETY: see above; indices come from the renderer's own grid, masks and pages.
        return unsafe { device.create_shader_module_trusted(desc, wgpu::ShaderRuntimeChecks::unchecked()) };
    }
    device.create_shader_module(desc)
}

fn storage_tex(binding: u32, format: wgpu::TextureFormat) -> wgpu::BindGroupLayoutEntry {
    wgpu::BindGroupLayoutEntry {
        binding,
        visibility: wgpu::ShaderStages::COMPUTE,
        ty: wgpu::BindingType::StorageTexture {
            access: wgpu::StorageTextureAccess::WriteOnly,
            format,
            view_dimension: wgpu::TextureViewDimension::D2,
        },
        count: None,
    }
}
