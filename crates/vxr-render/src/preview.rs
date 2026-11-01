//! The live path-traced preview (from spikes/preview): a progressive path tracer at 640x360
//! over the editor's brick map, run in slices of each frame inside a GPU time budget, with an
//! albedo-demodulated A-trous denoise, written into a corner of the editor image.
//!
//! - Budget: 3 ms of GPU per frame while editing, 12 ms once input has been idle for 300 ms
//!   (spikes/preview/RESULTS.md). With `TIMESTAMP_QUERY` the trace and denoise passes are timed
//!   (read back a few frames later, never waited on) and the rows traced per frame follow the
//!   measured cost per row. Without timestamps a conservative fixed cost per row is assumed.
//! - Edits reset only the pixels whose primary or sun ray crosses the edited box; a camera move
//!   restarts the whole image. Recolours need no reset (radiance is stored demodulated),
//!   unless a slot is metal, plastic or glass: those pixels are stored in colour, so they restart.
//! - Motion: a restart shows the whole new view on its first frame. When the budget does not
//!   reach every row, a coarse sample (one pixel of each block) is traced instead and the
//!   denoiser fills the rest of each block, so the corner follows the camera every frame
//!   instead of updating the old view in place.
//! - Materials per palette slot ([`Preview::set_materials`]): diffuse, metal (glossy, with a
//!   sun highlight), plastic (diffuse under a clear glossy coat), glass (Fresnel reflection, or
//!   straight through with a tint that deepens with the distance crossed: thin glass, no
//!   bending) and emissive.

use std::sync::Arc;
use std::sync::atomic::{AtomicU8, AtomicU32, Ordering};

use glam::Vec3;

use vxr_core::material::{Kind, Material};

use crate::camera::OrbitCamera;
use crate::map::{GpuMap, palette_linear};
use crate::renderer::BRICKMAP_WGSL;

const COMMON_WGSL: &str = include_str!("preview_common.wgsl");
const TRACE_WGSL: &str = include_str!("preview_trace.wgsl");
const DENOISE_WGSL: &str = include_str!("preview_denoise.wgsl");
const COMPOSITE_WGSL: &str = include_str!("preview_composite.wgsl");

pub const PREVIEW_W: u32 = 640;
pub const PREVIEW_H: u32 = 360;
/// GPU budget per frame for all preview work, while editing and once idle (ms).
pub const BUDGET_EDITING_MS: f64 = 3.0;
pub const BUDGET_IDLE_MS: f64 = 12.0;
/// Cost of one full 640x360 sample assumed before (or without) timestamps: above the spike's
/// measured 1.0 to 2.9 ms natively, so browsers without timestamps stay inside the budget.
const GUESS_SAMPLE_MS: f64 = 4.0;
/// Sampling stops here (the spike's reference image used 1,024) until something changes.
pub const MAX_SPP: u32 = 1024;
/// At most this many full samples per frame.
const MAX_SAMPLES_PER_FRAME: u32 = 4;
/// Samples per frame in place of the GPU time budget, 0 when the budget rules.
static FIXED_SAMPLES_PER_FRAME: AtomicU32 = AtomicU32::new(0);

/// Make every preview trace exactly `n` full samples a frame instead of what a GPU time budget
/// allows, so what a test or a headless run sees counts frames and does not depend on how fast
/// the GPU is (a software GPU is orders of magnitude slower than a real one). `None` goes back
/// to the budget, which is what the editor uses. At most `MAX_SAMPLES_PER_FRAME`. Process-wide,
/// as it is set once up front.
pub fn fix_samples_per_frame(n: Option<u32>) {
    FIXED_SAMPLES_PER_FRAME.store(n.unwrap_or(0).min(MAX_SAMPLES_PER_FRAME), Ordering::Relaxed);
}

/// The largest block a coarse sample takes one pixel of (the denoiser fills the rest of each
/// block from the samples around it, up to about 30 pixels away).
const MAX_BLOCK: u32 = 8;
/// Trace dispatches are cut to about this much GPU time each.
const DISPATCH_MS: f64 = 1.0;
const SLOT: u64 = 256;
const MAX_SLOTS: u64 = 64;
const PIXEL_BYTES: u64 = 32;
const ATROUS_ITERS: u32 = 4;
/// Samples a pixel near an edit keeps (indirect light there changed), and how far "near" is.
const HIST_CAP: u32 = 16;
const CAP_MARGIN: f32 = 32.0;
/// Timestamp readbacks in flight.
const RING: usize = 4;

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
    cam_r: [f32; 4],
    cam_u: [f32; 4],
    cam_f: [f32; 4],
    post: [f32; 4],
    backdrop: [[f32; 4]; 2],
}

#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable, Default)]
struct DispatchU {
    row0: u32,
    rows: u32,
    seed: u32,
    step: u32,
}

#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
struct RectU {
    origin: [f32; 2],
    size: [f32; 2],
    radius: f32,
    pad: [f32; 3],
}

/// The voxel box an edit changed (voxel units, max exclusive).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct EditBox {
    pub lo: Vec3,
    pub hi: Vec3,
}

impl EditBox {
    /// The box of voxels `lo..=hi`, grown by one voxel (faces next to an edit change too).
    pub fn voxels(lo: [i32; 3], hi: [i32; 3]) -> Self {
        let v = |a: [i32; 3]| Vec3::new(a[0] as f32, a[1] as f32, a[2] as f32);
        Self { lo: v(lo) - Vec3::ONE, hi: v(hi) + Vec3::splat(2.0) }
    }
}

/// Light, sky and lens of the preview (the Render panel's settings).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Lighting {
    /// Toward the sun (normalized on use).
    pub sun_dir: Vec3,
    /// Sun irradiance, linear RGB.
    pub sun_color: Vec3,
    /// Sky light, times the built-in sky (the gradient or the physical sky).
    pub sky: f32,
    /// Show the sky behind the scene instead of the editor's backdrop.
    pub sky_backdrop: bool,
    /// The physical sky (scattered sunlight that follows the sun's height, with the sun disk
    /// shown behind the scene) instead of the fixed gradient.
    pub physical_sky: bool,
    /// Linear exposure. Changing it, the gamma or the vignette alone re-resolves the image
    /// without a restart.
    pub exposure: f32,
    /// Display gamma: 2.2 is the plain sRGB image, higher is brighter in the mid-tones.
    pub gamma: f32,
    /// How much the corners darken: 0 is none, 1 takes the corners to black.
    pub vignette: f32,
    /// Lens radius in voxels; 0 is a pinhole (no depth of field).
    pub aperture: f32,
    /// Distance in voxels from the eye to the plane in focus; 0 or less focuses on the orbit
    /// target.
    pub focus: f32,
}

impl Default for Lighting {
    fn default() -> Self {
        Self {
            sun_dir: Vec3::new(0.45, 0.8, 0.35),
            sun_color: Vec3::new(3.0, 2.85, 2.6),
            sky: 1.0,
            sky_backdrop: false,
            physical_sky: false,
            exposure: 1.0,
            gamma: 2.2,
            vignette: 0.0,
            aperture: 0.0,
            focus: 0.0,
        }
    }
}

impl Lighting {
    /// Depth of field is on. The image is then stored with the colour of each sample's own
    /// hit (blurred edges mix colours), so recolours and edits restart it.
    pub fn dof(&self) -> bool {
        self.aperture > 0.0
    }
}

/// GPU time per traced row, learnt from timestamps.
struct Budget {
    ms_per_row: f64,
    denoise_ms: f64,
    learnt: bool,
}

impl Budget {
    fn rows(&self, budget_ms: f64, height: u32) -> u32 {
        let fixed = FIXED_SAMPLES_PER_FRAME.load(Ordering::Relaxed);
        if fixed > 0 {
            return height * fixed;
        }
        let r = ((budget_ms - self.denoise_ms).max(0.25) / self.ms_per_row) as u32;
        r.clamp(8, height * MAX_SAMPLES_PER_FRAME)
    }

    fn learn(&mut self, rows: u32, trace_ms: f64, denoise_ms: f64) {
        if rows == 0 || !(trace_ms > 0.0 && trace_ms < 1000.0) || !(0.0..1000.0).contains(&denoise_ms) {
            return;
        }
        let per_row = trace_ms / rows as f64;
        if self.learnt {
            self.ms_per_row = self.ms_per_row * 0.8 + per_row * 0.2;
            self.denoise_ms = self.denoise_ms * 0.8 + denoise_ms * 0.2;
        } else {
            (self.ms_per_row, self.denoise_ms, self.learnt) = (per_row, denoise_ms, true);
        }
    }
}

const FREE: u8 = 0;
const ENCODED: u8 = 1;
const MAPPING: u8 = 2;
const READY: u8 = 3;

/// Timestamps of the trace and denoise passes, read back without waiting.
struct Timer {
    set: wgpu::QuerySet,
    resolve: wgpu::Buffer,
    reads: Vec<(wgpu::Buffer, Arc<AtomicU8>, u32)>,
    period_ns: f64,
}

impl Timer {
    fn new(device: &wgpu::Device, queue: &wgpu::Queue) -> Self {
        let set = device.create_query_set(&wgpu::QuerySetDescriptor {
            label: Some("preview timestamps"),
            ty: wgpu::QueryType::Timestamp,
            count: 4 * RING as u32,
        });
        let resolve = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("preview timestamps resolve"),
            size: wgpu::QUERY_RESOLVE_BUFFER_ALIGNMENT * RING as u64,
            usage: wgpu::BufferUsages::QUERY_RESOLVE | wgpu::BufferUsages::COPY_SRC,
            mapped_at_creation: false,
        });
        let reads = (0..RING)
            .map(|_| {
                let b = device.create_buffer(&wgpu::BufferDescriptor {
                    label: Some("preview timestamps read"),
                    size: 32,
                    usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
                    mapped_at_creation: false,
                });
                (b, Arc::new(AtomicU8::new(FREE)), 0)
            })
            .collect();
        Self { set, resolve, reads, period_ns: queue.get_timestamp_period() as f64 }
    }
}

pub struct Preview {
    pub width: u32,
    pub height: u32,
    pub lighting: Lighting,
    frame_buf: wgpu::Buffer,
    disp_buf: wgpu::Buffer,
    pal_buf: wgpu::Buffer,
    mat_buf: wgpu::Buffer,
    /// Some slot is metal or glass, so recolours restart.
    has_specular: bool,
    pub out_tex: wgpu::Texture,
    out_view: wgpu::TextureView,
    trace_layout: wgpu::BindGroupLayout,
    pixels: wgpu::Buffer,
    trace_group: wgpu::BindGroup,
    /// A staged change's ghost words, whose removed cells the trace passes through, and
    /// whether any are set (see `set_hidden`).
    ghost: wgpu::Buffer,
    hide: bool,
    dn_ab: wgpu::BindGroup,
    dn_ba: wgpu::BindGroup,
    reset_pipe: wgpu::ComputePipeline,
    trace_pipe: wgpu::ComputePipeline,
    init_pipe: wgpu::ComputePipeline,
    atrous_pipe: wgpu::ComputePipeline,
    fill_pipe: wgpu::ComputePipeline,
    resolve_pipe: wgpu::ComputePipeline,
    cs_pipe: wgpu::ComputePipeline,
    cs_layout: wgpu::BindGroupLayout,
    cs_rect: wgpu::Buffer,
    sampler: wgpu::Sampler,
    last_key: Option<[f32; 19]>,
    /// Exposure, gamma and vignette of the last resolve.
    last_post: [f32; 3],
    /// Sampling stops at this many samples per pixel until something changes.
    pub max_spp: u32,
    /// A restart shows the whole new view on its first frame, with a coarse sample when the
    /// budget does not reach every row (the live corner). Off, a restart traces rows from the
    /// top as the budget allows, as a render export wants: it shows only the finished image.
    pub follow: bool,
    /// The editor view's background gradient behind the scene (`ViewColors::sky`).
    backdrop: [[u8; 3]; 2],
    cursor: u32,
    frame_counter: u32,
    force_restart: bool,
    budget: Budget,
    timer: Option<Timer>,
    /// Rows traced since the last restart or edit (samples per pixel of the least-sampled
    /// pixels = rows / height).
    rows_since_restart: u64,
    /// Full restarts so far (camera or light changes, new scenes).
    pub restarts: u32,
    /// Rows traced in the last frame (a coarse sample counts as the rows it costs).
    pub last_rows: u32,
    /// Rows that show the current view since the last restart: every row after a coarse
    /// sample, else the rows traced so far.
    shown_rows: u32,
}

/// One slot's material as the trace shader reads it: two vec4s (`mats` in preview_trace.wgsl).
fn material_params(m: &Material) -> [[f32; 4]; 2] {
    match m.kind {
        Kind::Diffuse => [[0.0; 4]; 2],
        Kind::Metal => [[1.0, m.rough, m.metal, 0.0], [0.0; 4]],
        Kind::Glass => [[2.0, m.rough, m.ior, m.trans], [m.att, 0.0, 0.0, 0.0]],
        Kind::Emit => [[3.0, 0.0, m.radiance(), 0.0], [0.0; 4]],
        Kind::Plastic => [[4.0, m.rough, m.spec, 0.0], [0.0; 4]],
    }
}

fn entry(binding: u32, ty: wgpu::BindingType) -> wgpu::BindGroupLayoutEntry {
    wgpu::BindGroupLayoutEntry { binding, visibility: wgpu::ShaderStages::COMPUTE, ty, count: None }
}

fn uniform(dynamic: bool) -> wgpu::BindingType {
    wgpu::BindingType::Buffer {
        ty: wgpu::BufferBindingType::Uniform,
        has_dynamic_offset: dynamic,
        min_binding_size: None,
    }
}

fn storage(read_only: bool) -> wgpu::BindingType {
    wgpu::BindingType::Buffer {
        ty: wgpu::BufferBindingType::Storage { read_only },
        has_dynamic_offset: false,
        min_binding_size: None,
    }
}

/// The trace pass's group: the frame, dispatch, pixels, materials and ghost buffers.
fn trace_group(device: &wgpu::Device, layout: &wgpu::BindGroupLayout, b: [&wgpu::Buffer; 5]) -> wgpu::BindGroup {
    let disp =
        wgpu::BindingResource::Buffer(wgpu::BufferBinding { buffer: b[1], offset: 0, size: wgpu::BufferSize::new(16) });
    device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some("preview trace"),
        layout,
        entries: &[
            wgpu::BindGroupEntry { binding: 0, resource: b[0].as_entire_binding() },
            wgpu::BindGroupEntry { binding: 1, resource: disp },
            wgpu::BindGroupEntry { binding: 2, resource: b[2].as_entire_binding() },
            wgpu::BindGroupEntry { binding: 3, resource: b[3].as_entire_binding() },
            wgpu::BindGroupEntry { binding: 4, resource: b[4].as_entire_binding() },
        ],
    })
}

fn storage_tex() -> wgpu::BindingType {
    wgpu::BindingType::StorageTexture {
        access: wgpu::StorageTextureAccess::WriteOnly,
        format: wgpu::TextureFormat::Rgba8Unorm,
        view_dimension: wgpu::TextureViewDimension::D2,
    }
}

impl Preview {
    /// The corner preview, 640x360. `timestamps`: the device has `TIMESTAMP_QUERY` (see
    /// `Gpu::timestamps`).
    pub fn new(
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        map_layout: &wgpu::BindGroupLayout,
        palette: &[[u8; 4]; 256],
        timestamps: bool,
    ) -> Self {
        Self::sized(device, queue, map_layout, palette, timestamps, (PREVIEW_W, PREVIEW_H))
    }

    /// Bytes of GPU buffers a preview of `w` x `h` needs, and the largest single buffer.
    pub fn memory(w: u32, h: u32) -> (u64, u64) {
        let n = w as u64 * h as u64;
        (n * (PIXEL_BYTES + 3 * 16), n * PIXEL_BYTES)
    }

    /// Whether a preview of `w` x `h` fits the device's buffer limits (for render export).
    pub fn fits(device: &wgpu::Device, w: u32, h: u32) -> bool {
        let l = device.limits();
        let (_, largest) = Self::memory(w, h);
        largest <= l.max_storage_buffer_binding_size
            && largest <= l.max_buffer_size
            && w.max(h) <= l.max_texture_dimension_2d
    }

    /// A preview of `size` pixels (render export uses the export size).
    pub fn sized(
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        map_layout: &wgpu::BindGroupLayout,
        palette: &[[u8; 4]; 256],
        timestamps: bool,
        size: (u32, u32),
    ) -> Self {
        let (w, h) = size;
        let n = (w * h) as u64;
        let buf = |label: &str, size: u64, usage: wgpu::BufferUsages| {
            device.create_buffer(&wgpu::BufferDescriptor { label: Some(label), size, usage, mapped_at_creation: false })
        };
        let st = wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST;
        let un = wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST;
        let frame_buf = buf("preview frame", std::mem::size_of::<FrameU>() as u64, un);
        let disp_buf = buf("preview dispatch", SLOT * MAX_SLOTS, un);
        let pixels = buf("preview pixels", n * PIXEL_BYTES, st);
        let aux = buf("preview aux", n * 16, st);
        let fa = buf("preview filter a", n * 16, st);
        let fb = buf("preview filter b", n * 16, st);
        let pal_buf = buf("preview palette", 256 * 16, un);
        let mat_buf = buf("preview materials", 256 * 32, un);
        let out_tex = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("preview out"),
            size: wgpu::Extent3d { width: w, height: h, depth_or_array_layers: 1 },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba8Unorm,
            usage: wgpu::TextureUsages::STORAGE_BINDING
                | wgpu::TextureUsages::TEXTURE_BINDING
                | wgpu::TextureUsages::COPY_SRC,
            view_formats: &[],
        });
        let out_view = out_tex.create_view(&Default::default());

        let trace_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("preview trace"),
            entries: &[
                entry(0, uniform(false)),
                entry(1, uniform(true)),
                entry(2, storage(false)),
                entry(3, uniform(false)),
                entry(4, storage(true)),
            ],
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
                entry(7, storage_tex()),
            ],
        });
        let disp_binding = wgpu::BindingResource::Buffer(wgpu::BufferBinding {
            buffer: &disp_buf,
            offset: 0,
            size: wgpu::BufferSize::new(16),
        });
        let ghost = buf("preview ghost", 16, st);
        let trace_group = trace_group(device, &trace_layout, [&frame_buf, &disp_buf, &pixels, &mat_buf, &ghost]);
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

        let module = |label: &str, src: String| {
            device.create_shader_module(wgpu::ShaderModuleDescriptor {
                label: Some(label),
                source: wgpu::ShaderSource::Wgsl(src.into()),
            })
        };
        let trace_mod = module("preview trace", format!("{BRICKMAP_WGSL}\n{COMMON_WGSL}\n{TRACE_WGSL}"));
        let dn_mod = module("preview denoise", format!("{COMMON_WGSL}\n{DENOISE_WGSL}"));
        let cs_mod = module("preview composite", COMPOSITE_WGSL.to_string());
        let layout = |label: &str, groups: &[Option<&wgpu::BindGroupLayout>]| {
            device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                label: Some(label),
                bind_group_layouts: groups,
                immediate_size: 0,
            })
        };
        let trace_pl = layout("preview trace", &[Some(&trace_layout), Some(map_layout)]);
        let dn_pl = layout("preview denoise", &[Some(&dn_layout)]);
        let compute = |label: &str, layout: &wgpu::PipelineLayout, module: &wgpu::ShaderModule, entry: &str| {
            device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
                label: Some(label),
                layout: Some(layout),
                module,
                entry_point: Some(entry),
                compilation_options: wgpu::PipelineCompilationOptions {
                    constants: &[],
                    zero_initialize_workgroup_memory: false,
                },
                cache: None,
            })
        };
        let reset_pipe = compute("preview reset", &trace_pl, &trace_mod, "reset_main");
        let trace_pipe = compute("preview trace", &trace_pl, &trace_mod, "trace_main");
        let init_pipe = compute("preview denoise init", &dn_pl, &dn_mod, "init_main");
        let atrous_pipe = compute("preview atrous", &dn_pl, &dn_mod, "atrous_main");
        let fill_pipe = compute("preview fill", &dn_pl, &dn_mod, "fill_main");
        let resolve_pipe = compute("preview resolve", &dn_pl, &dn_mod, "resolve_main");

        let cs_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("preview composite"),
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
                entry(2, storage_tex()),
                entry(3, uniform(false)),
            ],
        });
        let cs_pl = layout("preview composite", &[Some(&cs_layout)]);
        let cs_pipe = compute("preview composite", &cs_pl, &cs_mod, "main");
        let cs_rect = buf("preview composite rect", std::mem::size_of::<RectU>() as u64, un);
        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("preview composite"),
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            ..Default::default()
        });
        let me = Self {
            width: w,
            height: h,
            lighting: Lighting::default(),
            frame_buf,
            disp_buf,
            pal_buf,
            mat_buf,
            has_specular: false,
            out_tex,
            out_view,
            trace_layout,
            pixels,
            trace_group,
            ghost,
            hide: false,
            dn_ab,
            dn_ba,
            reset_pipe,
            trace_pipe,
            init_pipe,
            atrous_pipe,
            fill_pipe,
            resolve_pipe,
            cs_pipe,
            cs_layout,
            cs_rect,
            sampler,
            last_key: None,
            last_post: [1.0, 2.2, 0.0],
            max_spp: MAX_SPP,
            follow: true,
            backdrop: crate::ViewColors::NEUTRAL.sky,
            cursor: 0,
            frame_counter: 0,
            force_restart: true,
            budget: Budget {
                ms_per_row: GUESS_SAMPLE_MS * (w * h) as f64 / (PREVIEW_W * PREVIEW_H) as f64 / h as f64,
                denoise_ms: 0.5 * (w * h) as f64 / (PREVIEW_W * PREVIEW_H) as f64,
                learnt: false,
            },
            timer: timestamps.then(|| Timer::new(device, queue)),
            rows_since_restart: 0,
            restarts: 0,
            last_rows: 0,
            shown_rows: 0,
        };
        queue.write_buffer(&me.pal_buf, 0, bytemuck::cast_slice(&palette_linear(palette)));
        queue.write_buffer(&me.mat_buf, 0, &[0u8; 256 * 32]);
        me
    }

    /// Set the material of every palette slot (index 0 is unused). Restarts sampling.
    pub fn set_materials(&mut self, queue: &wgpu::Queue, materials: &[Material; 256]) {
        let params: Vec<[[f32; 4]; 2]> = materials.iter().map(material_params).collect();
        queue.write_buffer(&self.mat_buf, 0, bytemuck::cast_slice(&params));
        self.has_specular = materials[1..].iter().any(|m| matches!(m.kind, Kind::Metal | Kind::Glass | Kind::Plastic));
        self.force_restart = true;
    }

    /// Recolor: rewrite the preview's palette copy (the map's palette is written separately).
    /// Instant, except with depth of field or a metal, plastic or glass slot, which restart.
    pub fn set_palette(&mut self, queue: &wgpu::Queue, palette: &[[u8; 4]; 256]) {
        queue.write_buffer(&self.pal_buf, 0, bytemuck::cast_slice(&palette_linear(palette)));
        if self.lighting.dof() || self.has_specular {
            self.force_restart = true;
        }
    }

    /// Pass through a staged change's removals, which stay in the map until Apply: `words` is
    /// the editor view's ghost (`vxr_core::Scene::ghost_mask`), and its cells marked removed
    /// are hidden. Empty hides nothing. Restarts sampling.
    pub fn set_hidden(&mut self, device: &wgpu::Device, queue: &wgpu::Queue, words: &[u32]) {
        self.force_restart = true;
        self.hide = !words.is_empty();
        if words.is_empty() {
            return;
        }
        // Sized exactly: the shader bounds its reads by the buffer's length.
        let size = words.len() as u64 * 4;
        if size != self.ghost.size() {
            self.ghost = device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("preview ghost"),
                size,
                usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            });
            self.trace_group = trace_group(
                device,
                &self.trace_layout,
                [&self.frame_buf, &self.disp_buf, &self.pixels, &self.mat_buf, &self.ghost],
            );
        }
        queue.write_buffer(&self.ghost, 0, bytemuck::cast_slice(words));
    }

    /// Show the editor view's background gradient (the theme's `ViewColors::sky`) behind the
    /// scene. Restarts sampling when it changes.
    pub fn set_backdrop(&mut self, sky: [[u8; 3]; 2]) {
        if sky != self.backdrop {
            self.backdrop = sky;
            self.force_restart = true;
        }
    }

    /// Drop all accumulated samples on the next frame (a new scene).
    pub fn restart(&mut self) {
        self.force_restart = true;
    }

    /// Samples per pixel since the last restart or edit (pixels away from an edit have more).
    pub fn samples(&self) -> f32 {
        self.rows_since_restart as f32 / self.height as f32
    }

    /// Share of the image that shows the current view (the camera, light and scene of the
    /// last frame) rather than what an older one left: 1 from a restart's first frame on.
    pub fn current(&self) -> f32 {
        self.shown_rows as f32 / self.height as f32
    }

    /// Sampling reached `max_spp` and stopped.
    pub fn converged(&self) -> bool {
        self.rows_since_restart >= self.max_spp as u64 * self.height as u64
    }

    /// GPU time of one full sample, as measured (or assumed without timestamps), and whether
    /// it was measured.
    pub fn sample_ms(&self) -> (f64, bool) {
        (self.budget.ms_per_row * self.height as f64, self.budget.learnt)
    }

    fn key(&self, cam: &OrbitCamera) -> [f32; 19] {
        let l = &self.lighting;
        [
            cam.target.x,
            cam.target.y,
            cam.target.z,
            cam.yaw,
            cam.pitch,
            cam.distance,
            cam.fov_y,
            cam.ortho as u8 as f32,
            l.sun_dir.x,
            l.sun_dir.y,
            l.sun_dir.z,
            l.sun_color.x,
            l.sun_color.y,
            l.sun_color.z,
            l.sky,
            l.sky_backdrop as u8 as f32,
            l.physical_sky as u8 as f32,
            l.aperture,
            l.focus,
        ]
    }

    fn write_frame(&self, queue: &wgpu::Queue, cam: &OrbitCamera, flags: u32, edit: Option<EditBox>) {
        let vp = cam.view_proj(self.width as f32 / self.height as f32);
        let eye = cam.eye();
        let sun = self.lighting.sun_dir.normalize_or(Vec3::Y);
        let c = self.lighting.sun_color;
        let (lo, hi) = edit.map(|e| (e.lo, e.hi)).unwrap_or((Vec3::ZERO, Vec3::ZERO));
        let m = Vec3::splat(CAP_MARGIN);
        let v4 = |v: Vec3| [v.x, v.y, v.z, 0.0];
        let (fwd, right, up) = cam.basis();
        let l = &self.lighting;
        // The focus distance is from the orbit eye; orthographic rays start further back.
        let behind = if cam.ortho { (cam.view_eye() - eye).length() } else { 0.0 };
        let focus = if l.focus > 0.0 { l.focus } else { cam.distance } + behind;
        let u = FrameU {
            inv_vp: vp.inverse().to_cols_array_2d(),
            eye: [eye.x, eye.y, eye.z, if cam.ortho { 0.0 } else { 1.0 }],
            size: [self.width as f32, self.height as f32, 1.0 / self.width as f32, 1.0 / self.height as f32],
            sun: [sun.x, sun.y, sun.z, 0.9995],
            sun_col: [c.x, c.y, c.z, self.lighting.exposure],
            edit_lo: v4(lo),
            edit_hi: v4(hi),
            cap_lo: v4(lo - m),
            cap_hi: v4(hi + m),
            flags: [
                flags,
                HIST_CAP,
                self.frame_counter,
                1 | (l.sky_backdrop as u32) << 1
                    | (l.dof() as u32) << 2
                    | (l.physical_sky as u32) << 3
                    | (self.hide as u32) << 4,
            ],
            cam_r: [right.x, right.y, right.z, l.aperture.max(0.0)],
            cam_u: [up.x, up.y, up.z, focus],
            cam_f: [fwd.x, fwd.y, fwd.z, l.sky.max(0.0)],
            post: [2.2 / l.gamma.max(0.1), l.vignette.clamp(0.0, 1.0), 0.0, 0.0],
            backdrop: self.backdrop.map(|c| [c[0] as f32 / 255.0, c[1] as f32 / 255.0, c[2] as f32 / 255.0, 0.0]),
        };
        queue.write_buffer(&self.frame_buf, 0, bytemuck::bytes_of(&u));
    }

    /// Fold in timestamp readbacks that have arrived.
    fn read_timestamps(&mut self) {
        let Some(t) = &mut self.timer else { return };
        for (buf, state, rows) in &mut t.reads {
            if state.load(Ordering::Acquire) != READY {
                continue;
            }
            let ts: Vec<u64> = {
                let data = buf.slice(..).get_mapped_range().expect("mapped range");
                bytemuck::cast_slice(&data).to_vec()
            };
            buf.unmap();
            state.store(FREE, Ordering::Release);
            let ms = |a: u64, b: u64| b.saturating_sub(a) as f64 * t.period_ns / 1e6;
            self.budget.learn(*rows, ms(ts[0], ts[1]), ms(ts[2], ts[3]));
        }
    }

    /// Encode one frame of preview work: reset (camera or light change, or an edit), as many
    /// rows of samples as the budget allows, then denoise and resolve into `out_tex`. `idle`:
    /// no input for 300 ms, so the bigger budget applies. Call `after_submit` once the encoder
    /// is submitted.
    pub fn encode(
        &mut self,
        queue: &wgpu::Queue,
        enc: &mut wgpu::CommandEncoder,
        map: &GpuMap,
        cam: &OrbitCamera,
        edit: Option<EditBox>,
        idle: bool,
    ) {
        let budget = if idle { BUDGET_IDLE_MS } else { BUDGET_EDITING_MS };
        self.encode_budget(queue, enc, map, cam, edit, budget);
    }

    /// `encode` with a GPU budget of `budget_ms` for this frame (render export uses its own).
    pub fn encode_budget(
        &mut self,
        queue: &wgpu::Queue,
        enc: &mut wgpu::CommandEncoder,
        map: &GpuMap,
        cam: &OrbitCamera,
        edit: Option<EditBox>,
        budget_ms: f64,
    ) {
        self.read_timestamps();
        let key = self.key(cam);
        // With depth of field the reset pass cannot tell which pixels an edit touched.
        let restart = self.force_restart || self.last_key != Some(key) || (edit.is_some() && self.lighting.dof());
        self.force_restart = false;
        self.last_key = Some(key);
        let l = &self.lighting;
        let post = [l.exposure, l.gamma, l.vignette];
        let exposure = post != self.last_post;
        self.last_post = post;
        if restart {
            self.cursor = 0;
            self.restarts += 1;
        }
        if restart || edit.is_some() {
            self.rows_since_restart = 0;
        }
        let reset_pass = restart || edit.is_some();
        // Converged and nothing changed: keep the image, skip all work.
        // A new exposure, gamma or vignette alone only needs the resolve.
        let converged = self.converged();
        if !reset_pass && converged && !exposure {
            self.last_rows = 0;
            return;
        }
        let flags = (restart as u32) | if edit.is_some() { 2 } else { 0 };
        self.write_frame(queue, cam, flags, edit);

        let rows = if converged && !reset_pass { 0 } else { self.budget.rows(budget_ms, self.height) };
        // Dispatch slots: trace bands first, then A-trous steps, the fill, then resolve.
        let trace_slots = MAX_SLOTS as u32 - ATROUS_ITERS - 3;
        // A fixed count traces every row it asks for, whatever the height: the bands are sized to
        // fit the slots, with one more band for each time the cursor wraps to the top.
        let fixed = FIXED_SAMPLES_PER_FRAME.load(Ordering::Relaxed);
        let rpd = if fixed > 0 {
            rows.div_ceil(trace_slots - fixed).max(8)
        } else {
            ((DISPATCH_MS / self.budget.ms_per_row) as u32).max(8)
        };
        let mut slots: Vec<DispatchU> = Vec::new();
        // A restart (the camera, light or scene moved) shows only the new view this frame: when
        // the budget does not reach every row, one pixel of each k x k block is traced instead,
        // the denoiser fills the rest of each block from those samples, and the rows sharpen
        // the image from the next frame on. Never part new view and part old.
        let (mut coarse, mut block) = (0, 0);
        if self.follow && restart && rows < self.height {
            let k = ((self.height as f64 / rows as f64).sqrt().ceil() as u32).clamp(2, MAX_BLOCK);
            let blocks = self.height.div_ceil(k);
            // A row of blocks costs 1/k of a row of pixels.
            (coarse, block) = (blocks.div_ceil(k), k);
            let mut b0 = 0;
            while b0 < blocks {
                let n = (blocks - b0).min(rpd * k);
                let seed = self.frame_counter.wrapping_mul(64).wrapping_add(slots.len() as u32);
                slots.push(DispatchU { row0: b0, rows: n, seed, step: k });
                b0 += n;
            }
        }
        // The fill reads only the block samples: a coarse frame traces nothing else.
        let mut left = if block > 0 { 0 } else { rows };
        while left > 0 && (slots.len() as u32) < trace_slots {
            let n = left.min(rpd).min(self.height - self.cursor);
            let seed = self.frame_counter.wrapping_mul(64).wrapping_add(slots.len() as u32);
            slots.push(DispatchU { row0: self.cursor, rows: n, seed, step: 0 });
            self.cursor = (self.cursor + n) % self.height;
            left -= n;
        }
        let traced: u32 = slots.iter().filter(|s| s.step == 0).map(|s| s.rows).sum();
        let n_trace = slots.len();
        let first_atrous = slots.len();
        for k in 0..ATROUS_ITERS {
            slots.push(DispatchU { step: 1 << k, ..Default::default() });
        }
        let fill_slot = slots.len();
        slots.push(DispatchU { step: block, ..Default::default() });
        let resolve_slot = slots.len();
        slots.push(DispatchU { step: 1, ..Default::default() });
        let mut bytes = vec![0u8; slots.len() * SLOT as usize];
        for (i, s) in slots.iter().enumerate() {
            bytes[i * SLOT as usize..i * SLOT as usize + 16].copy_from_slice(bytemuck::bytes_of(s));
        }
        queue.write_buffer(&self.disp_buf, 0, &bytes);
        let off = |i: usize| (i as u64 * SLOT) as u32;
        let (gx, gy) = (self.width.div_ceil(8), self.height.div_ceil(8));

        // A free timestamp slot, if timestamps are on: trace begin/end, denoise begin/end.
        let q0 = self
            .timer
            .as_ref()
            .and_then(|t| t.reads.iter().position(|(_, s, _)| s.load(Ordering::Acquire) == FREE).map(|i| i as u32 * 4));
        let slot = self.timer.as_ref().zip(q0).map(|(t, q)| (&t.set, q));
        let n_passes = reset_pass as usize + n_trace;
        for i in 0..n_passes {
            let tsw = slot.filter(|_| i == 0 || i + 1 == n_passes).map(|(set, q)| wgpu::ComputePassTimestampWrites {
                query_set: set,
                beginning_of_pass_write_index: (i == 0).then_some(q),
                end_of_pass_write_index: (i + 1 == n_passes).then_some(q + 1),
            });
            let mut p = enc.begin_compute_pass(&wgpu::ComputePassDescriptor {
                label: Some("preview trace"),
                timestamp_writes: tsw,
            });
            p.set_bind_group(1, &map.bind_group, &[]);
            if reset_pass && i == 0 {
                p.set_pipeline(&self.reset_pipe);
                p.set_bind_group(0, &self.trace_group, &[0]);
                p.dispatch_workgroups(gx, gy, 1);
            } else {
                let k = i - reset_pass as usize;
                p.set_pipeline(&self.trace_pipe);
                p.set_bind_group(0, &self.trace_group, &[off(k)]);
                let blk = slots[k].step.max(1);
                p.dispatch_workgroups(self.width.div_ceil(blk).div_ceil(8), slots[k].rows.div_ceil(8), 1);
            }
        }

        let tsw = slot.map(|(set, q)| wgpu::ComputePassTimestampWrites {
            query_set: set,
            beginning_of_pass_write_index: Some(q + 2),
            end_of_pass_write_index: Some(q + 3),
        });
        {
            let mut p = enc.begin_compute_pass(&wgpu::ComputePassDescriptor {
                label: Some("preview denoise"),
                timestamp_writes: tsw,
            });
            // init writes filter a (dn_ba: dst = a); steps alternate a->b, b->a.
            p.set_pipeline(&self.init_pipe);
            p.set_bind_group(0, &self.dn_ba, &[off(first_atrous)]);
            p.dispatch_workgroups(gx, gy, 1);
            // A coarse sample's blocks are filled in one pass instead (a -> b).
            let steps = if block > 0 {
                p.set_pipeline(&self.fill_pipe);
                p.set_bind_group(0, &self.dn_ab, &[off(fill_slot)]);
                p.dispatch_workgroups(gx, gy, 1);
                1
            } else {
                p.set_pipeline(&self.atrous_pipe);
                for k in 0..ATROUS_ITERS as usize {
                    let g = if k % 2 == 0 { &self.dn_ab } else { &self.dn_ba };
                    p.set_bind_group(0, g, &[off(first_atrous + k)]);
                    p.dispatch_workgroups(gx, gy, 1);
                }
                ATROUS_ITERS
            };
            // The final filter result is in a when the step count is even, else in b.
            let g = if steps.is_multiple_of(2) { &self.dn_ab } else { &self.dn_ba };
            p.set_pipeline(&self.resolve_pipe);
            p.set_bind_group(0, g, &[off(resolve_slot)]);
            p.dispatch_workgroups(gx, gy, 1);
        }
        if let (Some(t), Some(q)) = (&mut self.timer, q0) {
            let i = q as usize / 4;
            let at = i as u64 * wgpu::QUERY_RESOLVE_BUFFER_ALIGNMENT;
            enc.resolve_query_set(&t.set, q..q + 4, &t.resolve, at);
            enc.copy_buffer_to_buffer(&t.resolve, at, &t.reads[i].0, 0, 32);
            t.reads[i].1.store(ENCODED, Ordering::Release);
            t.reads[i].2 = traced + coarse;
        }
        self.frame_counter = self.frame_counter.wrapping_add(1);
        self.rows_since_restart += traced as u64;
        self.last_rows = traced + coarse;
        let before = if restart { 0 } else { self.shown_rows };
        self.shown_rows = if block > 0 { self.height } else { (before + traced).min(self.height) };
    }

    /// Start reading back the timestamps of the frame just submitted.
    pub fn after_submit(&mut self, device: &wgpu::Device) {
        let Some(t) = &self.timer else { return };
        for (buf, state, _) in &t.reads {
            if state.load(Ordering::Acquire) != ENCODED {
                continue;
            }
            state.store(MAPPING, Ordering::Release);
            let s = state.clone();
            buf.slice(..).map_async(wgpu::MapMode::Read, move |r| {
                s.store(if r.is_ok() { READY } else { FREE }, Ordering::Release);
            });
        }
        #[cfg(not(target_arch = "wasm32"))]
        let _ = device.poll(wgpu::PollType::Poll);
        #[cfg(target_arch = "wasm32")]
        let _ = device;
    }

    /// Write the preview into `rect` (x, y, w, h in pixels) of the editor's colour texture
    /// (rgba8unorm, storage binding) with rounded corners of `radius` pixels, before the
    /// editor's blit draws it to the screen.
    #[allow(clippy::too_many_arguments)]
    pub fn composite_into(
        &self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        enc: &mut wgpu::CommandEncoder,
        color: &wgpu::Texture,
        rect: [f32; 4],
        radius: f32,
    ) {
        let r = rect.map(f32::round);
        let (cw, ch) = (color.width() as f32, color.height() as f32);
        // Clip to the target.
        let size = [(cw - r[0]).min(r[2]), (ch - r[1]).min(r[3])];
        if r[0] < 0.0 || r[1] < 0.0 || size[0] < 1.0 || size[1] < 1.0 {
            return;
        }
        let u = RectU { origin: [r[0], r[1]], size: [r[2], r[3]], radius, pad: [0.0; 3] };
        queue.write_buffer(&self.cs_rect, 0, bytemuck::bytes_of(&u));
        let dst = color.create_view(&Default::default());
        let group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("preview composite"),
            layout: &self.cs_layout,
            entries: &[
                wgpu::BindGroupEntry { binding: 0, resource: wgpu::BindingResource::TextureView(&self.out_view) },
                wgpu::BindGroupEntry { binding: 1, resource: wgpu::BindingResource::Sampler(&self.sampler) },
                wgpu::BindGroupEntry { binding: 2, resource: wgpu::BindingResource::TextureView(&dst) },
                wgpu::BindGroupEntry { binding: 3, resource: self.cs_rect.as_entire_binding() },
            ],
        });
        let mut p = enc.begin_compute_pass(&wgpu::ComputePassDescriptor {
            label: Some("preview composite"),
            timestamp_writes: None,
        });
        p.set_pipeline(&self.cs_pipe);
        p.set_bind_group(0, &group, &[]);
        p.dispatch_workgroups((size[0] as u32).div_ceil(8), (size[1] as u32).div_ceil(8), 1);
    }

    /// Read the preview image back as tightly packed RGBA8 rows.
    pub async fn read(&self, device: &wgpu::Device, queue: &wgpu::Queue) -> Vec<u8> {
        crate::gpu::read_texture(device, queue, &self.out_tex).await
    }
}
