//! The live preview: a progressive path tracer at 640x360 over the engine's brick map, run in
//! slices of each frame, with an albedo-demodulated A-trous denoise and a corner composite.

use glam::Vec3;
use vxr_engine_spike::camera::OrbitCamera;
use vxr_engine_spike::gpu::{GpuBrickMap, BRICKMAP_WGSL};

const COMMON_WGSL: &str = include_str!("common.wgsl");
const TRACE_WGSL: &str = include_str!("trace.wgsl");
const DENOISE_WGSL: &str = include_str!("denoise.wgsl");
const COMPOSITE_WGSL: &str = include_str!("composite.wgsl");
const COMPOSITE_CS_WGSL: &str = include_str!("composite_cs.wgsl");

pub const PREVIEW_W: u32 = 640;
pub const PREVIEW_H: u32 = 360;
const SLOT: u64 = 256;
const MAX_SLOTS: u64 = 256;
const PIXEL_BYTES: u64 = 32;

#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
struct FrameU {
    inv_vp: [[f32; 4]; 4],
    eye: [f32; 4],
    size: [f32; 4],
    sun: [f32; 4],
    sun_col: [f32; 4],
    edit_lo: [f32; 4],
    edit_hi: [f32; 4],
    cap_lo: [f32; 4],
    cap_hi: [f32; 4],
    flags: [u32; 4],
}

#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable, Default)]
struct DispatchU {
    row0: u32,
    rows: u32,
    seed: u32,
    step: u32,
}

/// The voxel box an edit changed (voxel units, max exclusive).
#[derive(Clone, Copy, Debug)]
pub struct EditBox {
    pub lo: Vec3,
    pub hi: Vec3,
}

impl EditBox {
    pub fn sphere(c: [f32; 3], r: f32) -> Self {
        let c = Vec3::from(c);
        Self { lo: c - Vec3::splat(r + 1.0), hi: c + Vec3::splat(r + 1.0) }
    }
}

/// Timestamp indices for the preview's passes in a caller-owned query set.
#[derive(Clone, Copy)]
pub struct PreviewTs<'a> {
    pub set: &'a wgpu::QuerySet,
    pub trace_begin: u32,
    pub trace_end: u32,
    pub denoise_begin: u32,
    pub denoise_end: u32,
}

#[derive(Clone, Copy, Debug, Default)]
pub struct EncodeInfo {
    pub restart: bool,
    pub reset_pass: bool,
    pub rows: u32,
    pub dispatches: u32,
}

pub struct Preview {
    pub width: u32,
    pub height: u32,
    pub denoise: bool,
    pub atrous_iters: u32,
    /// Samples a pixel near an edit keeps (indirect light there changed).
    pub hist_cap: u32,
    /// Cap box = edit box grown by this many voxels.
    pub cap_margin: f32,
    pub exposure: f32,
    frame_buf: wgpu::Buffer,
    disp_buf: wgpu::Buffer,
    pub pixels: wgpu::Buffer,
    pal_buf: wgpu::Buffer,
    pub out_tex: wgpu::Texture,
    trace_group: wgpu::BindGroup,
    dn_ab: wgpu::BindGroup,
    dn_ba: wgpu::BindGroup,
    reset_pipe: wgpu::ComputePipeline,
    trace_pipe: wgpu::ComputePipeline,
    prim_pipe: wgpu::ComputePipeline,
    bounce_pipe: wgpu::ComputePipeline,
    init_pipe: wgpu::ComputePipeline,
    atrous_pipe: wgpu::ComputePipeline,
    resolve_pipe: wgpu::ComputePipeline,
    comp_pipe: wgpu::RenderPipeline,
    comp_group: wgpu::BindGroup,
    cs_pipe: wgpu::ComputePipeline,
    cs_layout: wgpu::BindGroupLayout,
    cs_rect: wgpu::Buffer,
    sampler: wgpu::Sampler,
    out_view: wgpu::TextureView,
    last_cam: Option<[f32; 7]>,
    cursor: u32,
    pub frame_counter: u32,
    force_restart: bool,
}

fn srgb_to_linear(c: u8) -> f32 {
    let c = c as f32 / 255.0;
    if c <= 0.04045 { c / 12.92 } else { ((c + 0.055) / 1.055).powf(2.4) }
}

pub fn palette_linear(p: &[u32; 256]) -> Vec<[f32; 4]> {
    p.iter()
        .map(|&c| {
            let b = c.to_le_bytes();
            [srgb_to_linear(b[0]), srgb_to_linear(b[1]), srgb_to_linear(b[2]), b[3] as f32 / 255.0]
        })
        .collect()
}

fn entry(binding: u32, ty: wgpu::BindingType) -> wgpu::BindGroupLayoutEntry {
    wgpu::BindGroupLayoutEntry { binding, visibility: wgpu::ShaderStages::COMPUTE, ty, count: None }
}
fn uniform(dynamic: bool) -> wgpu::BindingType {
    wgpu::BindingType::Buffer { ty: wgpu::BufferBindingType::Uniform, has_dynamic_offset: dynamic, min_binding_size: None }
}
fn storage(read_only: bool) -> wgpu::BindingType {
    wgpu::BindingType::Buffer { ty: wgpu::BufferBindingType::Storage { read_only }, has_dynamic_offset: false, min_binding_size: None }
}

impl Preview {
    pub fn new(device: &wgpu::Device, map_layout: &wgpu::BindGroupLayout, target_format: wgpu::TextureFormat, _palette: &[u32; 256]) -> Self {
        Self::with_tracer(device, map_layout, target_format, 4)
    }

    /// `tracer`: 3 = the engine's `trace`, 4 = `trace_h`.
    pub fn with_tracer(device: &wgpu::Device, map_layout: &wgpu::BindGroupLayout, target_format: wgpu::TextureFormat, tracer: u32) -> Self {
        let tr = tracer as f64;
        let (w, h) = (PREVIEW_W, PREVIEW_H);
        let n = (w * h) as u64;
        let buf = |label: &str, size: u64, usage: wgpu::BufferUsages| device.create_buffer(&wgpu::BufferDescriptor { label: Some(label), size, usage, mapped_at_creation: false });
        let st = wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::COPY_SRC;
        let frame_buf = buf("preview frame", std::mem::size_of::<FrameU>() as u64, wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST);
        let disp_buf = buf("preview dispatch", SLOT * MAX_SLOTS, wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST);
        let pixels = buf("preview pixels", n * PIXEL_BYTES, st);
        let aux = buf("preview aux", n * 16, st);
        let fa = buf("preview filter a", n * 16, st);
        let fb = buf("preview filter b", n * 16, st);
        let pal_buf = buf("preview palette", 256 * 16, wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST);
        let out_tex = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("preview out"),
            size: wgpu::Extent3d { width: w, height: h, depth_or_array_layers: 1 },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba8Unorm,
            usage: wgpu::TextureUsages::STORAGE_BINDING | wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_SRC | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        });
        let out_view = out_tex.create_view(&Default::default());

        let trace_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("preview trace"),
            entries: &[entry(0, uniform(false)), entry(1, uniform(true)), entry(2, storage(false))],
        });
        let dn_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("preview denoise"),
            entries: &[
                entry(0, uniform(false)),
                entry(1, uniform(true)),
                entry(2, storage(true)),
                entry(3, storage(false)),
                entry(4, storage(true)),
                entry(5, storage(false)),
                entry(6, uniform(false)),
                entry(
                    7,
                    wgpu::BindingType::StorageTexture {
                        access: wgpu::StorageTextureAccess::WriteOnly,
                        format: wgpu::TextureFormat::Rgba8Unorm,
                        view_dimension: wgpu::TextureViewDimension::D2,
                    },
                ),
            ],
        });
        let disp_binding = wgpu::BindingResource::Buffer(wgpu::BufferBinding { buffer: &disp_buf, offset: 0, size: wgpu::BufferSize::new(16) });
        let trace_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("preview trace"),
            layout: &trace_layout,
            entries: &[
                wgpu::BindGroupEntry { binding: 0, resource: frame_buf.as_entire_binding() },
                wgpu::BindGroupEntry { binding: 1, resource: disp_binding.clone() },
                wgpu::BindGroupEntry { binding: 2, resource: pixels.as_entire_binding() },
            ],
        });
        let dn = |src: &wgpu::Buffer, dst: &wgpu::Buffer| {
            device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("preview denoise"),
                layout: &dn_layout,
                entries: &[
                    wgpu::BindGroupEntry { binding: 0, resource: frame_buf.as_entire_binding() },
                    wgpu::BindGroupEntry { binding: 1, resource: disp_binding.clone() },
                    wgpu::BindGroupEntry { binding: 2, resource: pixels.as_entire_binding() },
                    wgpu::BindGroupEntry { binding: 3, resource: aux.as_entire_binding() },
                    wgpu::BindGroupEntry { binding: 4, resource: src.as_entire_binding() },
                    wgpu::BindGroupEntry { binding: 5, resource: dst.as_entire_binding() },
                    wgpu::BindGroupEntry { binding: 6, resource: pal_buf.as_entire_binding() },
                    wgpu::BindGroupEntry { binding: 7, resource: wgpu::BindingResource::TextureView(&out_view) },
                ],
            })
        };
        let dn_ab = dn(&fa, &fb);
        let dn_ba = dn(&fb, &fa);

        let trace_src = format!("{BRICKMAP_WGSL}\n{COMMON_WGSL}\n{TRACE_WGSL}");
        let trace_mod = device.create_shader_module(wgpu::ShaderModuleDescriptor { label: Some("preview trace"), source: wgpu::ShaderSource::Wgsl(trace_src.into()) });
        let dn_src = format!("{COMMON_WGSL}\n{DENOISE_WGSL}");
        let dn_mod = device.create_shader_module(wgpu::ShaderModuleDescriptor { label: Some("preview denoise"), source: wgpu::ShaderSource::Wgsl(dn_src.into()) });
        let trace_pl = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("preview trace"),
            bind_group_layouts: &[Some(&trace_layout), Some(map_layout)],
            immediate_size: 0,
        });
        let dn_pl = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor { label: Some("preview denoise"), bind_group_layouts: &[Some(&dn_layout)], immediate_size: 0 });
        let compute = |label: &str, layout: &wgpu::PipelineLayout, module: &wgpu::ShaderModule, entry: &str, constants: &[(&str, f64)]| {
            device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
                label: Some(label),
                layout: Some(layout),
                module,
                entry_point: Some(entry),
                compilation_options: wgpu::PipelineCompilationOptions { constants, zero_initialize_workgroup_memory: false },
                cache: None,
            })
        };
        let reset_pipe = compute("preview reset", &trace_pl, &trace_mod, "reset_main", &[("TRACER", tr)]);
        let trace_pipe = compute("preview trace", &trace_pl, &trace_mod, "trace_main", &[("MODE", 0.0), ("TRACER", tr)]);
        let prim_pipe = compute("preview primary rays", &trace_pl, &trace_mod, "trace_main", &[("MODE", 1.0), ("TRACER", tr)]);
        let bounce_pipe = compute("preview bounce rays", &trace_pl, &trace_mod, "trace_main", &[("MODE", 2.0), ("TRACER", tr)]);
        let init_pipe = compute("preview denoise init", &dn_pl, &dn_mod, "init_main", &[]);
        let atrous_pipe = compute("preview atrous", &dn_pl, &dn_mod, "atrous_main", &[]);
        let resolve_pipe = compute("preview resolve", &dn_pl, &dn_mod, "resolve_main", &[]);

        let comp_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("preview composite"),
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
        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("preview composite"),
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            ..Default::default()
        });
        let comp_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("preview composite"),
            layout: &comp_layout,
            entries: &[
                wgpu::BindGroupEntry { binding: 0, resource: wgpu::BindingResource::TextureView(&out_view) },
                wgpu::BindGroupEntry { binding: 1, resource: wgpu::BindingResource::Sampler(&sampler) },
            ],
        });
        let comp_mod = device.create_shader_module(wgpu::ShaderModuleDescriptor { label: Some("preview composite"), source: wgpu::ShaderSource::Wgsl(COMPOSITE_WGSL.into()) });
        let comp_pl = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor { label: Some("preview composite"), bind_group_layouts: &[Some(&comp_layout)], immediate_size: 0 });
        let comp_pipe = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("preview composite"),
            layout: Some(&comp_pl),
            vertex: wgpu::VertexState { module: &comp_mod, entry_point: Some("vs"), compilation_options: Default::default(), buffers: &[] },
            primitive: wgpu::PrimitiveState::default(),
            depth_stencil: None,
            multisample: wgpu::MultisampleState::default(),
            fragment: Some(wgpu::FragmentState {
                module: &comp_mod,
                entry_point: Some("fs"),
                compilation_options: Default::default(),
                targets: &[Some(wgpu::ColorTargetState { format: target_format, blend: None, write_mask: wgpu::ColorWrites::ALL })],
            }),
            multiview_mask: None,
            cache: None,
        });
        let cs_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("preview composite cs"),
            entries: &[
                entry(
                    0,
                    wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: true },
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                ),
                entry(1, wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering)),
                entry(
                    2,
                    wgpu::BindingType::StorageTexture {
                        access: wgpu::StorageTextureAccess::WriteOnly,
                        format: wgpu::TextureFormat::Rgba8Unorm,
                        view_dimension: wgpu::TextureViewDimension::D2,
                    },
                ),
                entry(3, uniform(false)),
            ],
        });
        let cs_mod = device.create_shader_module(wgpu::ShaderModuleDescriptor { label: Some("preview composite cs"), source: wgpu::ShaderSource::Wgsl(COMPOSITE_CS_WGSL.into()) });
        let cs_pl = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor { label: Some("preview composite cs"), bind_group_layouts: &[Some(&cs_layout)], immediate_size: 0 });
        let cs_pipe = compute("preview composite cs", &cs_pl, &cs_mod, "main", &[]);
        let cs_rect = buf("preview composite rect", 16, wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST);
        let me = Self {
            width: w,
            height: h,
            denoise: true,
            atrous_iters: 4,
            hist_cap: 16,
            cap_margin: 32.0,
            exposure: 1.0,
            frame_buf,
            disp_buf,
            pixels,
            pal_buf,
            out_tex,
            trace_group,
            dn_ab,
            dn_ba,
            reset_pipe,
            trace_pipe,
            prim_pipe,
            bounce_pipe,
            init_pipe,
            atrous_pipe,
            resolve_pipe,
            comp_pipe,
            comp_group,
            cs_pipe,
            cs_layout,
            cs_rect,
            sampler,
            out_view,
            last_cam: None,
            cursor: 0,
            frame_counter: 0,
            force_restart: true,
        };
        me
    }

    /// Recolor: rewrite the preview's palette copy (the engine palette is written separately).
    pub fn set_palette(&self, queue: &wgpu::Queue, palette: &[u32; 256]) {
        queue.write_buffer(&self.pal_buf, 0, bytemuck::cast_slice(&palette_linear(palette)));
    }

    /// Blank the preview image (black), so pixels without a sample yet cannot show an older
    /// image. Used by the clean-image test.
    pub fn clear_out(&self, queue: &wgpu::Queue) {
        let zeros = vec![0u8; (self.width * self.height * 4) as usize];
        queue.write_texture(
            wgpu::TexelCopyTextureInfo { texture: &self.out_tex, mip_level: 0, origin: wgpu::Origin3d::ZERO, aspect: wgpu::TextureAspect::All },
            &zeros,
            wgpu::TexelCopyBufferLayout { offset: 0, bytes_per_row: Some(self.width * 4), rows_per_image: Some(self.height) },
            wgpu::Extent3d { width: self.width, height: self.height, depth_or_array_layers: 1 },
        );
    }

    /// Drop all accumulated samples on the next frame.
    pub fn restart(&mut self) {
        self.force_restart = true;
    }

    fn write_frame(&self, queue: &wgpu::Queue, cam: &OrbitCamera, flags: u32, edit: Option<EditBox>) {
        let vp = cam.view_proj(self.width as f32 / self.height as f32);
        let eye = cam.eye();
        let sun = Vec3::new(0.45, 0.8, 0.35).normalize();
        let (lo, hi) = edit.map(|e| (e.lo, e.hi)).unwrap_or((Vec3::ZERO, Vec3::ZERO));
        let m = Vec3::splat(self.cap_margin);
        let v4 = |v: Vec3| [v.x, v.y, v.z, 0.0];
        let u = FrameU {
            inv_vp: vp.inverse().to_cols_array_2d(),
            eye: [eye.x, eye.y, eye.z, 1.0],
            size: [self.width as f32, self.height as f32, 1.0 / self.width as f32, 1.0 / self.height as f32],
            sun: [sun.x, sun.y, sun.z, 0.9995f32],
            sun_col: [3.0, 2.85, 2.6, self.exposure],
            edit_lo: v4(lo),
            edit_hi: v4(hi),
            cap_lo: v4(lo - m),
            cap_hi: v4(hi + m),
            flags: [flags, self.hist_cap, self.frame_counter, self.denoise as u32],
        };
        queue.write_buffer(&self.frame_buf, 0, bytemuck::bytes_of(&u));
    }

    fn cam_key(cam: &OrbitCamera) -> [f32; 7] {
        [cam.target.x, cam.target.y, cam.target.z, cam.yaw, cam.pitch, cam.distance, cam.fov_y]
    }

    /// Encode one frame of preview work: reset (camera move or edit), `rows` rows of samples in
    /// dispatches of at most `rows_per_dispatch` rows, then denoise and resolve into `out_tex`.
    pub fn encode(
        &mut self,
        queue: &wgpu::Queue,
        enc: &mut wgpu::CommandEncoder,
        map: &GpuBrickMap,
        cam: &OrbitCamera,
        edit: Option<EditBox>,
        rows: u32,
        rows_per_dispatch: u32,
        ts: Option<PreviewTs<'_>>,
    ) -> EncodeInfo {
        let key = Self::cam_key(cam);
        let restart = self.force_restart || self.last_cam != Some(key);
        self.force_restart = false;
        self.last_cam = Some(key);
        if restart {
            self.cursor = 0;
        }
        let flags = (restart as u32) | if edit.is_some() { 2 } else { 0 };
        self.write_frame(queue, cam, flags, edit);

        // Dispatch slots: trace bands first, then A-trous steps, then resolve.
        let mut slots: Vec<DispatchU> = Vec::new();
        let rpd = rows_per_dispatch.max(1);
        let mut left = rows;
        while left > 0 {
            let n = left.min(rpd).min(self.height - self.cursor);
            slots.push(DispatchU { row0: self.cursor, rows: n, seed: self.frame_counter.wrapping_mul(64).wrapping_add(slots.len() as u32), step: 0 });
            self.cursor = (self.cursor + n) % self.height;
            left -= n;
        }
        let n_trace = slots.len();
        let first_atrous = slots.len();
        for k in 0..self.atrous_iters {
            slots.push(DispatchU { step: 1 << k, ..Default::default() });
        }
        let resolve_slot = slots.len();
        slots.push(DispatchU { step: self.denoise as u32, ..Default::default() });
        assert!(slots.len() as u64 <= MAX_SLOTS, "too many preview dispatches");
        let mut bytes = vec![0u8; slots.len() * SLOT as usize];
        for (i, s) in slots.iter().enumerate() {
            bytes[i * SLOT as usize..i * SLOT as usize + 16].copy_from_slice(bytemuck::bytes_of(s));
        }
        queue.write_buffer(&self.disp_buf, 0, &bytes);
        let off = |i: usize| (i as u64 * SLOT) as u32;
        let (gx, gy) = (self.width.div_ceil(8), self.height.div_ceil(8));

        let reset_pass = restart || edit.is_some();
        let n_passes = reset_pass as usize + n_trace.max(1);
        let mut pass_i = 0;
        let mut tw = |enc: &mut wgpu::CommandEncoder, label: &str| {
            let first = pass_i == 0;
            let last = pass_i + 1 == n_passes;
            pass_i += 1;
            let tsw = ts.map(|t| wgpu::ComputePassTimestampWrites {
                query_set: t.set,
                beginning_of_pass_write_index: first.then_some(t.trace_begin),
                end_of_pass_write_index: last.then_some(t.trace_end),
            });
            let tsw = tsw.filter(|t| t.beginning_of_pass_write_index.is_some() || t.end_of_pass_write_index.is_some());
            enc.begin_compute_pass(&wgpu::ComputePassDescriptor { label: Some(label), timestamp_writes: tsw }).forget_lifetime()
        };
        if reset_pass {
            let mut p = tw(enc, "preview reset");
            p.set_pipeline(&self.reset_pipe);
            p.set_bind_group(0, &self.trace_group, &[0]);
            p.set_bind_group(1, &map.bind_group, &[]);
            p.dispatch_workgroups(gx, gy, 1);
        }
        if n_trace == 0 {
            let _p = tw(enc, "preview trace (none)");
        }
        for (i, s) in slots[..n_trace].iter().enumerate() {
            let mut p = tw(enc, "preview trace");
            p.set_pipeline(&self.trace_pipe);
            p.set_bind_group(0, &self.trace_group, &[off(i)]);
            p.set_bind_group(1, &map.bind_group, &[]);
            p.dispatch_workgroups(gx, s.rows.div_ceil(8), 1);
        }

        let tsw = ts.map(|t| wgpu::ComputePassTimestampWrites {
            query_set: t.set,
            beginning_of_pass_write_index: Some(t.denoise_begin),
            end_of_pass_write_index: Some(t.denoise_end),
        });
        let mut p = enc.begin_compute_pass(&wgpu::ComputePassDescriptor { label: Some("preview denoise"), timestamp_writes: tsw });
        if self.denoise {
            // init writes filter a (dn_ba: dst = a); steps alternate a->b, b->a.
            p.set_pipeline(&self.init_pipe);
            p.set_bind_group(0, &self.dn_ba, &[off(first_atrous)]);
            p.dispatch_workgroups(gx, gy, 1);
            p.set_pipeline(&self.atrous_pipe);
            for k in 0..self.atrous_iters as usize {
                let g = if k % 2 == 0 { &self.dn_ab } else { &self.dn_ba };
                p.set_bind_group(0, g, &[off(first_atrous + k)]);
                p.dispatch_workgroups(gx, gy, 1);
            }
        }
        // The final filter result is in a when the step count is even, else in b.
        let g = if self.atrous_iters % 2 == 0 { &self.dn_ab } else { &self.dn_ba };
        p.set_pipeline(&self.resolve_pipe);
        p.set_bind_group(0, g, &[off(resolve_slot)]);
        p.dispatch_workgroups(gx, gy, 1);
        drop(p);
        self.frame_counter = self.frame_counter.wrapping_add(1);
        EncodeInfo { restart, reset_pass, rows, dispatches: n_trace as u32 }
    }

    /// Write the preview into `rect` (x, y, w, h in pixels) of the editor's colour texture
    /// (rgba8unorm, storage binding), before the editor's blit draws it to the screen.
    pub fn composite_into(&self, device: &wgpu::Device, queue: &wgpu::Queue, enc: &mut wgpu::CommandEncoder, color: &wgpu::Texture, rect: [f32; 4], ts: Option<wgpu::ComputePassTimestampWrites<'_>>) {
        let rect = [rect[0].round(), rect[1].round(), rect[2].round(), rect[3].round()];
        queue.write_buffer(&self.cs_rect, 0, bytemuck::cast_slice(&rect));
        let dst = color.create_view(&Default::default());
        let group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("preview composite cs"),
            layout: &self.cs_layout,
            entries: &[
                wgpu::BindGroupEntry { binding: 0, resource: wgpu::BindingResource::TextureView(&self.out_view) },
                wgpu::BindGroupEntry { binding: 1, resource: wgpu::BindingResource::Sampler(&self.sampler) },
                wgpu::BindGroupEntry { binding: 2, resource: wgpu::BindingResource::TextureView(&dst) },
                wgpu::BindGroupEntry { binding: 3, resource: self.cs_rect.as_entire_binding() },
            ],
        });
        let mut p = enc.begin_compute_pass(&wgpu::ComputePassDescriptor { label: Some("preview composite"), timestamp_writes: ts });
        p.set_pipeline(&self.cs_pipe);
        p.set_bind_group(0, &group, &[]);
        p.dispatch_workgroups((rect[2] as u32).div_ceil(8), (rect[3] as u32).div_ceil(8), 1);
    }

    /// Draw the preview into `rect` (x, y, w, h in target pixels) of `view`, keeping the rest.
    /// A second render pass over the whole target: measured at about 2 ms extra on a 3024x1964
    /// target, so the benchmarks use `composite_into` instead.
    pub fn composite(&self, enc: &mut wgpu::CommandEncoder, view: &wgpu::TextureView, rect: [f32; 4], ts: Option<wgpu::RenderPassTimestampWrites<'_>>) {
        let mut pass = enc.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("preview composite"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view,
                depth_slice: None,
                resolve_target: None,
                ops: wgpu::Operations { load: wgpu::LoadOp::Load, store: wgpu::StoreOp::Store },
            })],
            depth_stencil_attachment: None,
            timestamp_writes: ts,
            occlusion_query_set: None,
            multiview_mask: None,
        });
        pass.set_viewport(rect[0], rect[1], rect[2], rect[3], 0.0, 1.0);
        pass.set_pipeline(&self.comp_pipe);
        pass.set_bind_group(0, &self.comp_group, &[]);
        pass.draw(0..3, 0..1);
    }

    /// Ray-throughput benchmark: one full-image dispatch of primary (mode 1) or bounce (mode 2)
    /// rays. Bounce rays start from the pixel-centre hits, so a frame must have run first.
    pub fn encode_rays(&mut self, queue: &wgpu::Queue, enc: &mut wgpu::CommandEncoder, map: &GpuBrickMap, cam: &OrbitCamera, mode: u32, ts: Option<wgpu::ComputePassTimestampWrites<'_>>) {
        self.write_frame(queue, cam, 0, None);
        queue.write_buffer(&self.disp_buf, 0, bytemuck::bytes_of(&DispatchU { row0: 0, rows: self.height, seed: self.frame_counter, step: 0 }));
        self.frame_counter = self.frame_counter.wrapping_add(1);
        let mut p = enc.begin_compute_pass(&wgpu::ComputePassDescriptor { label: Some("preview rays"), timestamp_writes: ts });
        p.set_pipeline(if mode == 1 { &self.prim_pipe } else { &self.bounce_pipe });
        p.set_bind_group(0, &self.trace_group, &[0]);
        p.set_bind_group(1, &map.bind_group, &[]);
        p.dispatch_workgroups(self.width.div_ceil(8), self.height.div_ceil(8), 1);
    }

    /// Copy the preview texture into a mappable buffer (tightly packed RGBA8 after `unpack`).
    pub fn copy_out(&self, device: &wgpu::Device, enc: &mut wgpu::CommandEncoder) -> wgpu::Buffer {
        let padded = (self.width * 4).div_ceil(256) * 256;
        let buf = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("preview readback"),
            size: padded as u64 * self.height as u64,
            usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
            mapped_at_creation: false,
        });
        enc.copy_texture_to_buffer(
            wgpu::TexelCopyTextureInfo { texture: &self.out_tex, mip_level: 0, origin: wgpu::Origin3d::ZERO, aspect: wgpu::TextureAspect::All },
            wgpu::TexelCopyBufferInfo { buffer: &buf, layout: wgpu::TexelCopyBufferLayout { offset: 0, bytes_per_row: Some(padded), rows_per_image: Some(self.height) } },
            wgpu::Extent3d { width: self.width, height: self.height, depth_or_array_layers: 1 },
        );
        buf
    }

    pub fn unpack(&self, data: &[u8]) -> Vec<u8> {
        let row = (self.width * 4) as usize;
        let padded = row.div_ceil(256) * 256;
        let mut out = Vec::with_capacity(row * self.height as usize);
        for y in 0..self.height as usize {
            out.extend_from_slice(&data[y * padded..y * padded + row]);
        }
        out
    }

    pub fn copy_pixels(&self, device: &wgpu::Device, enc: &mut wgpu::CommandEncoder) -> wgpu::Buffer {
        let buf = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("preview pixels readback"),
            size: self.pixels.size(),
            usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
            mapped_at_creation: false,
        });
        enc.copy_buffer_to_buffer(&self.pixels, 0, &buf, 0, None);
        buf
    }
}

/// Per-pixel record as read back from the GPU.
#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable, Debug)]
pub struct PixelRec {
    pub sum: [f32; 3],
    pub lum2: f32,
    pub t: f32,
    pub info: u32,
    pub n: f32,
    pub pad: f32,
}
