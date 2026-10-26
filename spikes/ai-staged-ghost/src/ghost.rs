//! Spike copy of `vxr_render::renderer` with one addition: the staged-change ghost, a second
//! mask beside the selection (2 bits per voxel: added, repainted, removed) that `march.wgsl`
//! tints. Everything else is the real renderer's code, so a frame with no ghost costs what the
//! editor's does.

// The copy keeps the real renderer whole, used or not.
#![allow(dead_code)]

use vxr_render::renderer::BRICKMAP_WGSL;
use vxr_render::{GpuMap, Guides, OrbitCamera};

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
    pix: [f32; 4],
    bounds: [u32; 4],
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
    /// Which bounds edges pass near each FRAME_TILE square of the view (see `frame_tiles`).
    tiles: wgpu::Buffer,
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
    /// The staged-change ghost (2 bits per voxel, 32 words per brick slot) and its words in use.
    ghost: wgpu::Buffer,
    ghost_words: u32,
    pub guides: Guides,
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
                wgpu::BindGroupLayoutEntry {
                    binding: 4,
                    visibility: wgpu::ShaderStages::COMPUTE,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Storage { read_only: true },
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                },
            ],
        });
        let cam = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("camera"),
            size: std::mem::size_of::<CameraUniform>() as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let sel_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("selection and ghost"),
            entries: &[storage_buf(0), storage_buf(1)],
        });
        let sel = mask_buffer(device, 16);
        let ghost = mask_buffer(device, 16);
        let sel_group = mask_group(device, &sel_layout, &sel, &ghost);
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
            tiles: t.tiles,
            march,
            blit,
            blit_layout,
            blit_group: t.blit_group,
            sampler,
            sel,
            sel_layout,
            sel_group,
            sel_words: 0,
            ghost,
            ghost_words: 0,
            guides: Guides::default(),
        }
    }

    /// Highlight the selection: one bit per voxel in the map's mask layout (16 words per
    /// brick slot), as `vxr_core::Scene::selection_mask` builds it. Empty clears it.
    pub fn set_selection(&mut self, device: &wgpu::Device, queue: &wgpu::Queue, words: &[u32]) {
        if words.len() as u64 * 4 > self.sel.size() {
            self.sel = mask_buffer(device, (words.len() as u64 * 4).next_power_of_two());
            self.sel_group = mask_group(device, &self.sel_layout, &self.sel, &self.ghost);
        }
        if !words.is_empty() {
            queue.write_buffer(&self.sel, 0, bytemuck::cast_slice(words));
        }
        self.sel_words = words.len() as u32;
    }

    /// Draw the staged change as a ghost: 2 bits per voxel in 32 words per brick slot, as
    /// `Staged::ghost_mask` builds it (0 none, 1 added, 2 repainted, 3 removed). Empty clears it.
    pub fn set_ghost(&mut self, device: &wgpu::Device, queue: &wgpu::Queue, words: &[u32]) {
        if words.len() as u64 * 4 > self.ghost.size() {
            self.ghost = mask_buffer(device, (words.len() as u64 * 4).next_power_of_two());
            self.sel_group = mask_group(device, &self.sel_layout, &self.sel, &self.ghost);
        }
        if !words.is_empty() {
            queue.write_buffer(&self.ghost, 0, bytemuck::cast_slice(words));
        }
        self.ghost_words = words.len() as u32;
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
        (self.color, self.depth, self.info, self.tiles, self.out_group, self.blit_group) =
            (t.color, t.depth, t.info, t.tiles, t.out_group, t.blit_group);
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
        // World size of a pixel at ray distance t: constant in orthographic, growing with t in
        // perspective.
        let h = self.height as f32;
        let pix = if cam.ortho { [2.0 * cam.half_height() / h, 0.0] } else { [0.0, 2.0 * (cam.fov_y * 0.5).tan() / h] };
        let g = &self.guides;
        let (w, h) = (self.width as f32, self.height as f32);
        if g.frame {
            let edges = bounds_edges(vp, g.bounds, w, h);
            let reach = g.line.max(1.0) * 0.5 + 1.5;
            queue.write_buffer(
                &self.tiles,
                0,
                bytemuck::cast_slice(&frame_tiles(&edges, self.width, self.height, reach)),
            );
        }
        let flags = g.grid as u32 | (g.ground as u32) << 1 | (g.frame as u32) << 2;
        let u = CameraUniform {
            inv_vp: vp.inverse().to_cols_array_2d(),
            eye: [eye.x, eye.y, eye.z, if cam.ortho { 0.0 } else { 1.0 }],
            size: [self.width as f32, self.height as f32, 1.0 / self.width as f32, 1.0 / self.height as f32],
            sun: [sun.x, sun.y, sun.z, 0.0],
            cursor: [c[0], c[1], c[2], cursor.is_some() as i32],
            select: [self.sel_words, self.width.div_ceil(FRAME_TILE), self.ghost_words, 0],
            pix: [pix[0], pix[1], g.floor as f32, g.line.max(1.0)],
            bounds: [g.bounds[0], g.bounds[1], g.bounds[2], flags],
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
        vxr_render::gpu::read_texture(device, queue, &self.color).await
    }
}

/// Side of the square tiles the bounds edges are binned into, in pixels.
const FRAME_TILE: u32 = 32;

/// One bit per bounds edge for each FRAME_TILE square of a `width` x `height` view: set when
/// the edge's segment comes within `reach` pixels of the tile, so a pixel tests only the
/// edges that can cover it (usually none).
fn frame_tiles(edges: &[[f32; 4]; 12], width: u32, height: u32, reach: f32) -> Vec<u32> {
    let (tw, th) = (width.div_ceil(FRAME_TILE), height.div_ceil(FRAME_TILE));
    let mut tiles = vec![0u32; (tw * th) as usize];
    let t = FRAME_TILE as f32;
    // Samples every half tile are within a quarter tile of every point of the segment.
    let r = reach + t * 0.25;
    let (w, h) = (width as f32, height as f32);
    for (e, s) in edges.iter().enumerate() {
        let (a, b) = (glam::Vec2::new(s[0], s[1]), glam::Vec2::new(s[2], s[3]));
        // Clip to the view grown by `r` (Liang-Barsky), so far-off segments cost nothing.
        let d = b - a;
        let (mut t0, mut t1) = (0f32, 1f32);
        for (p, q) in [(-d.x, a.x + r), (d.x, w + r - a.x), (-d.y, a.y + r), (d.y, h + r - a.y)] {
            if p == 0.0 {
                if q < 0.0 {
                    t1 = -1.0;
                }
            } else if p < 0.0 {
                t0 = t0.max(q / p);
            } else {
                t1 = t1.min(q / p);
            }
        }
        if t0 > t1 {
            continue;
        }
        let (a, b) = (a + d * t0, a + d * t1);
        let steps = ((b - a).length() / (t * 0.5)).ceil().max(1.0) as u32;
        for i in 0..=steps {
            let p = a.lerp(b, i as f32 / steps as f32);
            let x0 = ((p.x - r) / t).floor().max(0.0) as u32;
            let y0 = ((p.y - r) / t).floor().max(0.0) as u32;
            let x1 = (((p.x + r) / t).floor().max(0.0) as u32).min(tw - 1);
            let y1 = (((p.y + r) / t).floor().max(0.0) as u32).min(th - 1);
            for y in y0..=y1 {
                for x in x0..=x1 {
                    tiles[(y * tw + x) as usize] |= 1 << e;
                }
            }
        }
    }
    tiles
}

/// The bounds box's twelve edges in pixels, in the order `march.wgsl` numbers them: edge 3n + k
/// runs along axis k, at the corner (n & 1, n >> 1) of the other two axes (k + 1, k + 2).
/// Edges are clipped to the near plane; one wholly behind the eye lands far off screen.
fn bounds_edges(vp: glam::Mat4, size: [u32; 3], w: f32, h: f32) -> [[f32; 4]; 12] {
    const NEAR: f32 = 1e-3;
    let s = glam::Vec3::new(size[0] as f32, size[1] as f32, size[2] as f32);
    let screen = |c: glam::Vec4| [(c.x / c.w * 0.5 + 0.5) * w, (0.5 - c.y / c.w * 0.5) * h];
    std::array::from_fn(|e| {
        let (n, k) = (e / 3, e % 3);
        let mut p0 = glam::Vec3::ZERO;
        p0[(k + 1) % 3] = if n & 1 != 0 { s[(k + 1) % 3] } else { 0.0 };
        p0[(k + 2) % 3] = if n & 2 != 0 { s[(k + 2) % 3] } else { 0.0 };
        let mut p1 = p0;
        p1[k] = s[k];
        let (mut c0, mut c1) = (vp * p0.extend(1.0), vp * p1.extend(1.0));
        if c0.w < NEAR && c1.w < NEAR {
            return [-1e6; 4];
        }
        if c0.w < NEAR {
            c0 = c0.lerp(c1, (NEAR - c0.w) / (c1.w - c0.w));
        } else if c1.w < NEAR {
            c1 = c1.lerp(c0, (NEAR - c1.w) / (c0.w - c1.w));
        }
        let (a, b) = (screen(c0), screen(c1));
        [a[0], a[1], b[0], b[1]]
    })
}

struct Targets {
    color: wgpu::Texture,
    depth: wgpu::Texture,
    info: wgpu::Texture,
    tiles: wgpu::Buffer,
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
        let tiles = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("frame tiles"),
            size: (w.div_ceil(FRAME_TILE) * h.div_ceil(FRAME_TILE)) as u64 * 4,
            usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let out_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("march out"),
            layout: out_layout,
            entries: &[
                wgpu::BindGroupEntry { binding: 0, resource: cam.as_entire_binding() },
                wgpu::BindGroupEntry { binding: 1, resource: wgpu::BindingResource::TextureView(&cv) },
                wgpu::BindGroupEntry { binding: 2, resource: wgpu::BindingResource::TextureView(&dv) },
                wgpu::BindGroupEntry { binding: 3, resource: wgpu::BindingResource::TextureView(&iv) },
                wgpu::BindGroupEntry { binding: 4, resource: tiles.as_entire_binding() },
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
        Self { color, depth, info, tiles, out_group, blit_group }
    }
}

fn mask_buffer(device: &wgpu::Device, size: u64) -> wgpu::Buffer {
    device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("mask"),
        size,
        usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
    })
}

fn mask_group(
    device: &wgpu::Device,
    layout: &wgpu::BindGroupLayout,
    sel: &wgpu::Buffer,
    ghost: &wgpu::Buffer,
) -> wgpu::BindGroup {
    device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some("selection and ghost"),
        layout,
        entries: &[
            wgpu::BindGroupEntry { binding: 0, resource: sel.as_entire_binding() },
            wgpu::BindGroupEntry { binding: 1, resource: ghost.as_entire_binding() },
        ],
    })
}

fn storage_buf(binding: u32) -> wgpu::BindGroupLayoutEntry {
    wgpu::BindGroupLayoutEntry {
        binding,
        visibility: wgpu::ShaderStages::COMPUTE,
        ty: wgpu::BindingType::Buffer {
            ty: wgpu::BufferBindingType::Storage { read_only: true },
            has_dynamic_offset: false,
            min_binding_size: None,
        },
        count: None,
    }
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
