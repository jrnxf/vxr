//! Test 2 measurements, shared by the native binary and the browser page.
//!
//! Every frame is the full editor frame at the configured resolution (engine march + blit into
//! an offscreen target), plus, when the preview is on, the preview's reset/trace slice, denoise,
//! resolve and corner composite. A scripted radius-16 brush edits every frame in the editing runs.

use crate::preview::{EditBox, PixelRec, Preview, PreviewTs, PREVIEW_H, PREVIEW_W};
use glam::Vec3;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use vxr_engine_spike::bench::{stats, Stats};
use vxr_engine_spike::camera::OrbitCamera;
use vxr_engine_spike::gpu::{self, now_ms, Gpu, GpuBrickMap, RenderOptions, Renderer};
use vxr_engine_spike::scene::Scene;

pub const TARGET_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Bgra8Unorm;
const Q: u32 = 8; // timestamps per frame
const BATCH: u32 = 8; // frames submitted back to back between reads

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Config {
    pub width: u32,
    pub height: u32,
    /// Measured frames per editing run.
    pub frames: u32,
    /// Preview budget per frame while editing: GPU ms for all preview work (reset, trace,
    /// denoise, resolve, composite). The trace slice is this minus the measured overhead.
    pub slice_ms: f64,
    /// Budget once the input has been idle for `idle_after_ms`.
    pub idle_slice_ms: f64,
    pub idle_after_ms: f64,
    /// Target length of one trace dispatch.
    pub dispatch_ms: f64,
    /// Display refresh interval: a frame never takes less than this.
    pub vsync_ms: f64,
    /// Brush radius (voxels).
    pub brush: f32,
    /// PSNR (dB, sRGB 8-bit, against the 1024-spp reference) that counts as "clean at 1x".
    pub psnr_clean: f64,
    pub max_clean_s: f64,
    pub reference_spp: u32,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            width: 3024,
            height: 1964,
            frames: 240,
            slice_ms: 3.0,
            idle_slice_ms: 12.0,
            idle_after_ms: 300.0,
            dispatch_ms: 1.0,
            vsync_ms: 1000.0 / 120.0,
            brush: 16.0,
            psnr_clean: 38.0,
            max_clean_s: 12.0,
            reference_spp: 1024,
        }
    }
}

pub struct Timer {
    set: wgpu::QuerySet,
    resolve: wgpu::Buffer,
    read: wgpu::Buffer,
    period_ns: f64,
}

impl Timer {
    fn new(device: &wgpu::Device, queue: &wgpu::Queue) -> Self {
        let count = BATCH * Q + 64;
        let set = device.create_query_set(&wgpu::QuerySetDescriptor { label: Some("preview frames"), ty: wgpu::QueryType::Timestamp, count });
        let size = count as u64 * 8;
        let mk = |label: &str, usage| device.create_buffer(&wgpu::BufferDescriptor { label: Some(label), size, usage, mapped_at_creation: false });
        let resolve = mk("ts resolve", wgpu::BufferUsages::QUERY_RESOLVE | wgpu::BufferUsages::COPY_SRC);
        let read = mk("ts read", wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ);
        Self { set, resolve, read, period_ns: queue.get_timestamp_period() as f64 }
    }

    async fn read(&self, device: &wgpu::Device, queue: &wgpu::Queue, first: u32, n: u32) -> Vec<u64> {
        let mut enc = device.create_command_encoder(&Default::default());
        enc.resolve_query_set(&self.set, first..first + n, &self.resolve, 0);
        enc.copy_buffer_to_buffer(&self.resolve, 0, &self.read, 0, Some(n as u64 * 8));
        queue.submit([enc.finish()]);
        let data = gpu::map_read(device, &self.read).await;
        bytemuck::cast_slice::<u8, u64>(&data[..n as usize * 8]).to_vec()
    }

    fn ms(&self, a: u64, b: u64) -> f64 {
        b.wrapping_sub(a) as f64 * self.period_ns / 1e6
    }
}

#[derive(Clone, Copy, Debug, Default, Serialize)]
pub struct FrameRec {
    /// GPU time of the whole frame (editor + preview), from timestamps.
    pub total_ms: f64,
    pub editor_ms: f64,
    pub trace_ms: f64,
    pub denoise_ms: f64,
    /// Writing the preview into the editor's colour texture (compute).
    pub composite_ms: f64,
    /// Preview passes: start of its first pass to end of denoise, plus the composite. The
    /// editor's march may run alongside, so the frame time on vs off is the reliable measure of
    /// what the preview adds.
    pub preview_ms: f64,
    /// Wall time per frame of its batch (CPU edit + submit + GPU, back to back).
    pub wall_ms: f64,
    pub rows: u32,
    pub dispatches: u32,
    pub reset_pass: bool,
    pub edit_cpu_ms: f64,
}

#[derive(Clone, Copy)]
struct Spec {
    preview: bool,
    edit: bool,
    slice_ms: f64,
}

pub struct Session<'a> {
    pub gpu: &'a Gpu,
    pub scene: Scene,
    pub map: GpuBrickMap,
    pub editor: Renderer,
    pub preview: Preview,
    pub target: wgpu::Texture,
    pub timer: Option<Timer>,
    pub cfg: Config,
    pub cam: OrbitCamera,
    /// Budget controller: GPU ms per preview row (reset pass included), EMA.
    pub ms_per_row: f64,
    /// Budget controller: preview work that is not tracing (denoise, resolve, composite), EMA.
    pub overhead_ms: f64,
    brush_i: u32,
    pub notes: Vec<String>,
}

fn make_target(device: &wgpu::Device, w: u32, h: u32) -> wgpu::Texture {
    device.create_texture(&wgpu::TextureDescriptor {
        label: Some("editor target"),
        size: wgpu::Extent3d { width: w, height: h, depth_or_array_layers: 1 },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: TARGET_FORMAT,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
        view_formats: &[],
    })
}

/// PSNR in dB over the pixels where `mask` is true (all pixels if `mask` is empty).
pub fn psnr(a: &[u8], b: &[u8], mask: &[bool]) -> f64 {
    let mut se = 0f64;
    let mut n = 0f64;
    for (i, (pa, pb)) in a.chunks_exact(4).zip(b.chunks_exact(4)).enumerate() {
        if !mask.is_empty() && !mask[i] {
            continue;
        }
        for c in 0..3 {
            let d = pa[c] as f64 - pb[c] as f64;
            se += d * d;
            n += 1.0;
        }
    }
    let mse = se / n;
    if mse <= 0.0 { 99.0 } else { 10.0 * (255.0f64 * 255.0 / mse).log10() }
}

/// Mean absolute error in 8-bit units, and the 99th percentile per-pixel error.
pub fn errors(a: &[u8], b: &[u8], mask: &[bool]) -> (f64, f64) {
    let mut e: Vec<f64> = a
        .chunks_exact(4)
        .zip(b.chunks_exact(4))
        .enumerate()
        .filter(|(i, _)| mask.is_empty() || mask[*i])
        .map(|(_, (pa, pb))| (0..3).map(|c| (pa[c] as f64 - pb[c] as f64).abs()).sum::<f64>() / 3.0)
        .collect();
    let mean = e.iter().sum::<f64>() / e.len() as f64;
    e.sort_by(|x, y| x.partial_cmp(y).unwrap());
    (mean, e[(e.len() as f64 * 0.99) as usize])
}

impl<'a> Session<'a> {
    pub async fn new(gpu: &'a Gpu, scene: Scene, cfg: Config) -> Result<Self, String> {
        let layout = gpu::brickmap_layout(&gpu.device);
        let map = GpuBrickMap::new(&gpu.device, &gpu.queue, &layout, &scene.map, &scene.palette)?;
        let editor = Renderer::new(&gpu.device, &layout, TARGET_FORMAT, cfg.width, cfg.height, RenderOptions { gbuffer: false, levels: 4 });
        let preview = Preview::new(&gpu.device, &layout, TARGET_FORMAT, &scene.palette);
        preview.set_palette(&gpu.queue, &scene.palette);
        let target = make_target(&gpu.device, cfg.width, cfg.height);
        let timer = gpu.timestamps.then(|| Timer::new(&gpu.device, &gpu.queue));
        let (lo, hi) = scene.map.occupied_bounds().unwrap_or(([0; 3], [8; 3]));
        let lo = Vec3::new(lo[0] as f32, lo[1] as f32, lo[2] as f32);
        let hi = Vec3::new(hi[0] as f32, hi[1] as f32, hi[2] as f32);
        let cam = OrbitCamera::framing_aspect(lo, hi, cfg.width as f32 / cfg.height as f32);
        gpu.queue.submit([]);
        gpu::wait_idle(&gpu.device, &gpu.queue).await;
        let mut s = Self { gpu, scene, map, editor, preview, target, timer, cfg, cam, ms_per_row: 0.02, overhead_ms: 0.8, brush_i: 0, notes: Vec::new() };
        s.scene.map.track_dirty(true);
        s.calibrate().await;
        Ok(s)
    }

    /// "framed": the whole scene in view (the editor's default). "close": a quarter of the
    /// framing distance, 25 degrees down, so terrain fills the preview.
    pub fn set_view(&mut self, view: &str) {
        let (lo, hi) = self.scene.map.occupied_bounds().unwrap_or(([0; 3], [8; 3]));
        let lo = Vec3::new(lo[0] as f32, lo[1] as f32, lo[2] as f32);
        let hi = Vec3::new(hi[0] as f32, hi[1] as f32, hi[2] as f32);
        let mut cam = OrbitCamera::framing_aspect(lo, hi, self.cfg.width as f32 / self.cfg.height as f32);
        if view == "close" {
            cam.distance *= 0.3;
            cam.pitch = 25f32.to_radians();
            cam.target.y = (lo.y + hi.y) * 0.25;
        }
        self.cam = cam;
        self.preview.restart();
    }

    pub fn resize(&mut self, w: u32, h: u32) {
        self.editor.resize(&self.gpu.device, w, h);
        self.target = make_target(&self.gpu.device, w, h);
        self.cfg.width = w;
        self.cfg.height = h;
    }

    /// Where the preview sits: top-right corner, 640x360 points at 2x (Retina), scaled with
    /// the editor size so the half-resolution run keeps the same layout.
    fn corner(&self) -> [f32; 4] {
        let scale = self.cfg.width as f32 / 3024.0;
        let (w, h) = (1280.0 * scale, 720.0 * scale);
        let m = 32.0 * scale;
        [self.cfg.width as f32 - w - m, m, w, h]
    }

    fn rows_for(&self, slice_ms: f64) -> (u32, u32) {
        let h = PREVIEW_H;
        let round8 = |x: f64| ((x / 8.0).floor().max(1.0) * 8.0) as u32;
        let rows = round8((slice_ms - self.overhead_ms).max(0.25) / self.ms_per_row).min(h * 16);
        let rpd = round8(self.cfg.dispatch_ms / self.ms_per_row).min(h);
        (rows, rpd)
    }

    /// Scripted brush: walk across the middle of the editor view, alternating add and erase.
    fn brush(&mut self) -> (Option<EditBox>, f64) {
        let (w, h) = (self.cfg.width as f32, self.cfg.height as f32);
        let e = self.brush_i;
        self.brush_i += 1;
        let fx = 0.3 + 0.4 * ((e / 2) as f32 * 0.137).fract();
        let fy = 0.4 + 0.2 * ((e / 2) as f32 * 0.291).fract();
        let (ro, dir) = self.cam.ray(fx * w, fy * h, w, h);
        let t0 = now_ms();
        let Some(hit) = self.scene.map.raycast(ro.into(), dir.into(), 1e6) else { return (None, 0.0) };
        let c = [hit.voxel[0] as f32 + 0.5, hit.voxel[1] as f32 + 0.5, hit.voxel[2] as f32 + 0.5];
        let add = e % 2 == 0;
        self.scene.map.sphere(c, self.cfg.brush, if add { 2 } else { 0 }, false);
        let g = self.gpu;
        self.map.upload_dirty(&g.device, &g.queue, &mut self.scene.map, &self.scene.palette).expect("upload");
        (Some(EditBox::sphere(c, self.cfg.brush)), now_ms() - t0)
    }

    /// Encode and submit one frame into timestamp slot `slot` (0..BATCH).
    fn submit_frame(&mut self, spec: Spec, slot: u32) -> FrameRec {
        let g = self.gpu;
        let (edit, edit_ms) = if spec.edit { self.brush() } else { (None, 0.0) };
        let cam = self.cam;
        self.editor.set_camera(&g.queue, &cam);
        let mut enc = g.device.create_command_encoder(&Default::default());
        let view = self.target.create_view(&Default::default());
        let base = slot * Q;
        let t = self.timer.as_ref();
        // Order: preview (reset, trace slice, denoise, resolve), editor march, preview written
        // into the editor's colour texture, editor blit. The preview goes first so its first
        // pass starts the frame: GPUs may run passes with no data dependency side by side, and
        // a preview pass that overlaps the march cannot be timed on its own.
        let mut rec = FrameRec { edit_cpu_ms: edit_ms, ..Default::default() };
        let mut info = None;
        if spec.preview {
            let (rows, rpd) = self.rows_for(spec.slice_ms);
            let pts = t.map(|t| PreviewTs { set: &t.set, trace_begin: base + 2, trace_end: base + 3, denoise_begin: base + 4, denoise_end: base + 5 });
            info = Some(self.preview.encode(&g.queue, &mut enc, &self.map, &cam, edit, rows, rpd, pts));
        }
        self.editor.march(
            &mut enc,
            &self.map,
            t.map(|t| wgpu::ComputePassTimestampWrites { query_set: &t.set, beginning_of_pass_write_index: Some(base), end_of_pass_write_index: None }),
        );
        if let Some(info) = info {
            let rect = self.corner();
            self.preview.composite_into(
                &g.device,
                &g.queue,
                &mut enc,
                &self.editor.color,
                rect,
                t.map(|t| wgpu::ComputePassTimestampWrites { query_set: &t.set, beginning_of_pass_write_index: Some(base + 6), end_of_pass_write_index: Some(base + 7) }),
            );
            rec.rows = info.rows;
            rec.dispatches = info.dispatches;
            rec.reset_pass = info.reset_pass;
        }
        self.editor.blit(
            &mut enc,
            &view,
            t.map(|t| wgpu::RenderPassTimestampWrites { query_set: &t.set, beginning_of_pass_write_index: None, end_of_pass_write_index: Some(base + 1) }),
        );
        g.queue.submit([enc.finish()]);
        rec
    }

    /// Run `n` frames in batches; returns per-frame records and feeds the budget controller.
    async fn run(&mut self, n: u32, mut spec: impl FnMut(u32, f64) -> Spec) -> Vec<FrameRec> {
        let g = self.gpu;
        let mut out = Vec::with_capacity(n as usize);
        let mut sim_ms = 0.0;
        let mut i = 0;
        while i < n {
            let b = BATCH.min(n - i);
            let t0 = now_ms();
            let mut recs = Vec::new();
            for k in 0..b {
                let s = spec(i + k, sim_ms);
                let mut r = self.submit_frame(s, k);
                r.wall_ms = f64::NAN;
                recs.push((r, s));
            }
            gpu::wait_idle(&g.device, &g.queue).await;
            let wall = (now_ms() - t0) / b as f64;
            if let Some(t) = &self.timer {
                let ts = t.read(&g.device, &g.queue, 0, b * Q).await;
                for (k, (r, s)) in recs.iter_mut().enumerate() {
                    let q = &ts[k * Q as usize..(k + 1) * Q as usize];
                    if s.preview {
                        r.total_ms = t.ms(q[0].min(q[2]), q[1]);
                        r.trace_ms = t.ms(q[2], q[3]);
                        r.denoise_ms = t.ms(q[4], q[5]);
                        r.composite_ms = t.ms(q[6], q[7]);
                        r.preview_ms = t.ms(q[2], q[5]) + r.composite_ms;
                        r.editor_ms = t.ms(q[0], q[1]);
                    } else {
                        r.total_ms = t.ms(q[0], q[1]);
                        r.editor_ms = r.total_ms;
                    }
                }
            }
            for (mut r, s) in recs {
                r.wall_ms = wall;
                if s.preview && r.rows > 0 && self.timer.is_some() && r.trace_ms > 0.0 {
                    let m = r.trace_ms / r.rows as f64;
                    self.ms_per_row = 0.7 * self.ms_per_row + 0.3 * m;
                    // Denoise and composite depend on the trace output, so their timestamps are
                    // clean even when the editor's march runs alongside.
                    self.overhead_ms = 0.8 * self.overhead_ms + 0.2 * (r.denoise_ms + r.composite_ms);
                }
                let frame = if self.timer.is_some() { r.total_ms } else { r.wall_ms };
                sim_ms += frame.max(self.cfg.vsync_ms);
                out.push(r);
            }
            i += b;
        }
        out
    }

    /// First guess of the controller: time full-image trace frames (no editor), by timestamps or,
    /// without them, by wall clock (trace frames minus empty frames).
    async fn calibrate(&mut self) {
        let g = self.gpu;
        let cam = self.cam;
        let (w, h) = (PREVIEW_W, PREVIEW_H);
        let _ = w;
        let encode = |s: &mut Self, rows: u32, ts: bool| {
            let mut enc = g.device.create_command_encoder(&Default::default());
            let pts = if ts { s.timer.as_ref().map(|t| PreviewTs { set: &t.set, trace_begin: 0, trace_end: 1, denoise_begin: 2, denoise_end: 3 }) } else { None };
            s.preview.encode(&g.queue, &mut enc, &s.map, &cam, None, rows, h, pts);
            g.queue.submit([enc.finish()]);
        };
        self.preview.restart();
        encode(self, 0, false);
        gpu::wait_idle(&g.device, &g.queue).await;
        if self.timer.is_some() {
            let mut v = Vec::new();
            let mut dn = Vec::new();
            for _ in 0..8 {
                encode(self, h, true);
                gpu::wait_idle(&g.device, &g.queue).await;
                let t = self.timer.as_ref().unwrap();
                let ts = t.read(&g.device, &g.queue, 0, 4).await;
                v.push(t.ms(ts[0], ts[1]));
                dn.push(t.ms(ts[2], ts[3]));
            }
            let s = stats(&v);
            self.ms_per_row = s.median / h as f64;
            self.overhead_ms = stats(&dn).median + 0.1;
            self.notes.push(format!("calibration: full 640x360 sample {:.3} ms median (GPU timestamps)", s.median));
        } else {
            let timed = |s: &mut Self, rows: u32| {
                let t0 = now_ms();
                for _ in 0..16 {
                    encode(s, rows, false);
                }
                t0
            };
            let mut full = Vec::new();
            let mut empty = Vec::new();
            for _ in 0..3 {
                let t0 = timed(self, h);
                gpu::wait_idle(&g.device, &g.queue).await;
                full.push((now_ms() - t0) / 16.0);
                let t0 = timed(self, 0);
                gpu::wait_idle(&g.device, &g.queue).await;
                empty.push((now_ms() - t0) / 16.0);
            }
            let d = (stats(&full).median - stats(&empty).median).max(0.05);
            self.ms_per_row = d / h as f64;
            self.notes.push(format!("calibration: full 640x360 sample {:.3} ms by wall clock (no timestamps)", d));
        }
    }

    fn summarize(&self, recs: &[FrameRec]) -> Value {
        let col = |f: &dyn Fn(&FrameRec) -> f64| -> Stats { stats(&recs.iter().map(f).filter(|x| x.is_finite()).collect::<Vec<_>>()) };
        let px = (PREVIEW_W * PREVIEW_H) as f64;
        let spp_frame: Vec<f64> = recs.iter().map(|r| r.rows as f64 * PREVIEW_W as f64 / px).collect();
        let total = col(&|r| if self.timer.is_some() { r.total_ms } else { r.wall_ms });
        let frame_interval = total.median.max(self.cfg.vsync_ms);
        let spp = stats(&spp_frame);
        json!({
            "frame_gpu_ms": col(&|r| r.total_ms),
            "editor_gpu_ms": col(&|r| r.editor_ms),
            "trace_gpu_ms": col(&|r| r.trace_ms),
            "denoise_gpu_ms": col(&|r| r.denoise_ms),
            "composite_gpu_ms": col(&|r| r.composite_ms),
            "preview_gpu_ms": col(&|r| r.preview_ms),
            "wall_ms_per_frame": col(&|r| r.wall_ms),
            "edit_cpu_ms": col(&|r| r.edit_cpu_ms),
            "rows_per_frame": col(&|r| r.rows as f64),
            "dispatches_per_frame": col(&|r| r.dispatches as f64),
            "spp_per_frame": spp,
            "spp_per_s_at_vsync": spp.median * 1000.0 / frame_interval,
            "frame_interval_ms": frame_interval,
        })
    }

    /// Editing run: the brush edits every frame, camera still. Preview on or off.
    pub async fn edit_run(&mut self, preview: bool) -> Value {
        self.edit_run_with(preview, true).await
    }

    pub async fn edit_run_with(&mut self, preview: bool, edit: bool) -> Value {
        let slice = self.cfg.slice_ms;
        self.preview.restart();
        let warm = self.run(48, |_, _| Spec { preview, edit, slice_ms: slice }).await;
        drop(warm);
        let n = self.cfg.frames;
        let recs = self.run(n, |_, _| Spec { preview, edit, slice_ms: slice }).await;
        let mut v = self.summarize(&recs);
        v["preview"] = json!(preview);
        v["frames"] = json!(n);
        v["ms_per_row_end"] = json!(self.ms_per_row);
        v["overhead_ms_end"] = json!(self.overhead_ms);
        v
    }

    /// Flat-out tracer throughput: full-image samples back to back (no editor), spp per GPU second.
    pub async fn flat_out(&mut self) -> Value {
        let g = self.gpu;
        let cam = self.cam;
        self.preview.restart();
        let mut ms = Vec::new();
        let mut dn = Vec::new();
        for i in 0..24 {
            let mut enc = g.device.create_command_encoder(&Default::default());
            let pts = self.timer.as_ref().map(|t| PreviewTs { set: &t.set, trace_begin: 0, trace_end: 1, denoise_begin: 2, denoise_end: 3 });
            self.preview.encode(&g.queue, &mut enc, &self.map, &cam, None, PREVIEW_H, PREVIEW_H, pts);
            g.queue.submit([enc.finish()]);
            gpu::wait_idle(&g.device, &g.queue).await;
            if let Some(t) = &self.timer {
                let ts = t.read(&g.device, &g.queue, 0, 4).await;
                if i > 0 {
                    ms.push(t.ms(ts[0], ts[1]));
                    dn.push(t.ms(ts[2], ts[3]));
                }
            }
        }
        let s = stats(&ms);
        json!({ "full_sample_gpu_ms": s, "spp_per_gpu_second": 1000.0 / s.median, "denoise_resolve_gpu_ms": stats(&dn) })
    }

    /// Mrays/s with the other traversal (`trace` instead of `trace_h`), for comparison.
    pub async fn rays_with_tracer(&mut self, tracer: u32) -> Value {
        let layout = gpu::brickmap_layout(&self.gpu.device);
        let keep = std::mem::replace(&mut self.preview, Preview::with_tracer(&self.gpu.device, &layout, TARGET_FORMAT, tracer));
        self.preview.set_palette(&self.gpu.queue, &self.scene.palette);
        let v = self.rays().await;
        self.preview = keep;
        v
    }

    /// Swap the preview's traversal (3 = `trace`, 4 = `trace_h`); accumulation restarts.
    pub fn set_tracer(&mut self, tracer: u32) {
        let layout = gpu::brickmap_layout(&self.gpu.device);
        self.preview = Preview::with_tracer(&self.gpu.device, &layout, TARGET_FORMAT, tracer);
        self.preview.set_palette(&self.gpu.queue, &self.scene.palette);
    }

    /// Mrays/s for primary rays and one-bounce rays at 640x360 (pixel-centre primaries, cosine
    /// bounce rays from the primary hits).
    pub async fn rays(&mut self) -> Value {
        let g = self.gpu;
        let cam = self.cam;
        // One frame to fill the pixel-centre hits.
        self.preview.restart();
        let mut enc = g.device.create_command_encoder(&Default::default());
        self.preview.encode(&g.queue, &mut enc, &self.map, &cam, None, 8, 8, None);
        let pbuf = self.preview.copy_pixels(&g.device, &mut enc);
        g.queue.submit([enc.finish()]);
        let data = gpu::map_read(&g.device, &pbuf).await;
        let recs: &[PixelRec] = bytemuck::cast_slice(&data);
        let hits = recs.iter().filter(|p| p.t >= 0.0).count() as f64;
        let px = (PREVIEW_W * PREVIEW_H) as f64;
        let mut out = json!({ "pixels": px, "primary_hits": hits });
        // Dispatches are submitted back to back (one submit each, so each gets its own random
        // seed) and timed with timestamps, keeping the GPU busy so clocks stay up.
        for (mode, name, rays) in [(1u32, "primary", px), (2u32, "bounce", hits)] {
            let mut ms = Vec::new();
            for round in 0..3 {
                let n = 24u32;
                let t0 = now_ms();
                for i in 0..n {
                    let mut enc = g.device.create_command_encoder(&Default::default());
                    let tsw = self.timer.as_ref().map(|t| wgpu::ComputePassTimestampWrites { query_set: &t.set, beginning_of_pass_write_index: Some(i * 2), end_of_pass_write_index: Some(i * 2 + 1) });
                    self.preview.encode_rays(&g.queue, &mut enc, &self.map, &cam, mode, tsw);
                    g.queue.submit([enc.finish()]);
                }
                gpu::wait_idle(&g.device, &g.queue).await;
                let wall = (now_ms() - t0) / n as f64;
                if let Some(t) = &self.timer {
                    let ts = t.read(&g.device, &g.queue, 0, n * 2).await;
                    for i in 2..n as usize {
                        ms.push(t.ms(ts[i * 2], ts[i * 2 + 1]));
                    }
                } else if round > 0 {
                    ms.push(wall);
                }
            }
            let s = stats(&ms);
            out[name] = json!({ "rays": rays, "gpu_ms": s, "mrays_per_s": rays / (s.median / 1000.0) / 1e6, "timing": if self.timer.is_some() { "timestamps" } else { "wall clock per dispatch, back to back" } });
        }
        out
    }

    async fn read_preview(&self) -> Vec<u8> {
        let g = self.gpu;
        let mut enc = g.device.create_command_encoder(&Default::default());
        let buf = self.preview.copy_out(&g.device, &mut enc);
        g.queue.submit([enc.finish()]);
        let data = gpu::map_read(&g.device, &buf).await;
        self.preview.unpack(&data)
    }

    async fn read_pixels(&self) -> Vec<PixelRec> {
        let g = self.gpu;
        let mut enc = g.device.create_command_encoder(&Default::default());
        let buf = self.preview.copy_pixels(&g.device, &mut enc);
        g.queue.submit([enc.finish()]);
        let data = gpu::map_read(&g.device, &buf).await;
        bytemuck::cast_slice(&data).to_vec()
    }

    /// Converged reference: `reference_spp` samples per pixel, no denoise.
    pub async fn reference(&mut self) -> Vec<u8> {
        let g = self.gpu;
        let cam = self.cam;
        let was = self.preview.denoise;
        self.preview.denoise = false;
        self.preview.restart();
        let per_frame = 8u32;
        for f in 0..self.cfg.reference_spp / per_frame {
            let mut enc = g.device.create_command_encoder(&Default::default());
            self.preview.encode(&g.queue, &mut enc, &self.map, &cam, None, PREVIEW_H * per_frame, PREVIEW_H, None);
            g.queue.submit([enc.finish()]);
            if f % 8 == 7 {
                gpu::wait_idle(&g.device, &g.queue).await;
            }
        }
        gpu::wait_idle(&g.device, &g.queue).await;
        let img = self.read_preview().await;
        self.preview.denoise = was;
        img
    }

    /// Camera stops at t = 0 (accumulation restarts), nothing else changes. Frames run with the
    /// budget policy (`slice_ms`, then `idle_slice_ms` after `idle_after_ms`), on a simulated
    /// 120 Hz clock: each frame takes max(its GPU time, 8.33 ms). Every ~100 ms the preview is
    /// read back and compared with the reference.
    pub async fn clean_time(&mut self, reference: &[u8], idle_boost: bool, mut keep: impl FnMut(f64, f64, &[u8])) -> Value {
        self.preview.restart();
        self.preview.clear_out(&self.gpu.queue);
        let cfg = self.cfg.clone();
        let mut sim = 0.0;
        let mut curve = Vec::new();
        let mut pass_at: Option<f64> = None;
        let mut streak = 0;
        let mut next_check = 0.0;
        let mut spp_total = 0.0;
        let mut mask: Vec<bool> = Vec::new();
        let mut first = true;
        while sim < cfg.max_clean_s * 1000.0 {
            let slice = if idle_boost && sim >= cfg.idle_after_ms { cfg.idle_slice_ms } else { cfg.slice_ms };
            let recs = self.run(1, |_, _| Spec { preview: true, edit: false, slice_ms: slice }).await;
            if first {
                first = false;
                mask = self.read_pixels().await.iter().map(|p| p.t >= 0.0).collect();
            }
            let r = recs[0];
            let ft = if self.timer.is_some() { r.total_ms } else { r.wall_ms };
            sim += ft.max(cfg.vsync_ms);
            spp_total += r.rows as f64 * PREVIEW_W as f64 / (PREVIEW_W * PREVIEW_H) as f64;
            if sim >= next_check {
                next_check = sim + 100.0;
                let img = self.read_preview().await;
                let p = psnr(&img, reference, &mask);
                let p_all = psnr(&img, reference, &[]);
                let (mae, p99) = errors(&img, reference, &mask);
                curve.push(json!({
                    "t_ms": sim, "spp": spp_total, "psnr": p, "psnr_all_pixels": p_all, "mae": mae, "p99_err": p99,
                    "frame_ms": ft, "slice_ms": slice, "rows": r.rows, "trace_ms": r.trace_ms, "preview_ms": r.preview_ms,
                    "ms_per_row": self.ms_per_row, "overhead_ms": self.overhead_ms,
                }));
                keep(sim, p, &img);
                if p >= cfg.psnr_clean {
                    streak += 1;
                    if streak == 1 {
                        pass_at = Some(sim);
                    }
                    if streak >= 5 {
                        break;
                    }
                } else {
                    streak = 0;
                    pass_at = None;
                }
            }
        }
        json!({
            "idle_boost": idle_boost,
            "geometry_pixels": mask.iter().filter(|m| **m).count(),
            "psnr_clean": cfg.psnr_clean,
            "clean_at_ms": pass_at,
            "curve": curve,
        })
    }

    /// Recolor palette slot `slot` while the preview is converged; count frames until the
    /// preview shows it on the pixels whose primary hit uses that slot.
    pub async fn recolor(&mut self, slot: u8, color: u32) -> Value {
        let g = self.gpu;
        let slice = self.cfg.slice_ms;
        self.preview.restart();
        self.run(60, |_, _| Spec { preview: true, edit: false, slice_ms: slice }).await;
        let pixels = self.read_pixels().await;
        let before = self.read_preview().await;
        let mine: Vec<usize> = pixels.iter().enumerate().filter(|(_, p)| p.t >= 0.0 && (p.info & 0xff) == slot as u32).map(|(i, _)| i).collect();
        let mut pal = self.scene.palette;
        pal[slot as usize] = color;
        let t0 = now_ms();
        self.map.set_palette(&g.queue, &pal);
        self.preview.set_palette(&g.queue, &pal);
        let write_ms = now_ms() - t0;
        let mut frames = None;
        let mut fractions = Vec::new();
        let [r, gg, b, _] = color.to_le_bytes();
        let target_red_dominant = r as i32 > gg as i32 && r as i32 > b as i32;
        for f in 1..=4 {
            self.run(1, |_, _| Spec { preview: true, edit: false, slice_ms: slice }).await;
            let img = self.read_preview().await;
            let ok = mine
                .iter()
                .filter(|&&i| {
                    let p = &img[i * 4..i * 4 + 3];
                    let changed = p != &before[i * 4..i * 4 + 3];
                    changed && (!target_red_dominant || (p[0] > p[1] && p[0] > p[2]))
                })
                .count();
            let frac = ok as f64 / mine.len().max(1) as f64;
            fractions.push(frac);
            if frac >= 0.99 && frames.is_none() {
                frames = Some(f);
            }
        }
        self.map.set_palette(&g.queue, &self.scene.palette);
        self.preview.set_palette(&g.queue, &self.scene.palette);
        json!({
            "slot": slot,
            "pixels_with_slot": mine.len(),
            "palette_write_ms": write_ms,
            "frames_until_99pct_updated": frames,
            "fraction_updated_per_frame": fractions,
            "data_rebuilt": false,
        })
    }

    /// Save-friendly frame grabs.
    pub async fn grab_preview(&self) -> Vec<u8> {
        self.read_preview().await
    }

    pub async fn grab_editor(&self) -> (u32, u32, Vec<u8>) {
        let g = self.gpu;
        let (w, h) = (self.cfg.width, self.cfg.height);
        let row = w * 4;
        let padded = row.div_ceil(256) * 256;
        let buf = g.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("editor readback"),
            size: padded as u64 * h as u64,
            usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
            mapped_at_creation: false,
        });
        let mut enc = g.device.create_command_encoder(&Default::default());
        enc.copy_texture_to_buffer(
            wgpu::TexelCopyTextureInfo { texture: &self.target, mip_level: 0, origin: wgpu::Origin3d::ZERO, aspect: wgpu::TextureAspect::All },
            wgpu::TexelCopyBufferInfo { buffer: &buf, layout: wgpu::TexelCopyBufferLayout { offset: 0, bytes_per_row: Some(padded), rows_per_image: Some(h) } },
            wgpu::Extent3d { width: w, height: h, depth_or_array_layers: 1 },
        );
        g.queue.submit([enc.finish()]);
        let data = gpu::map_read(&g.device, &buf).await;
        let mut out = Vec::with_capacity((row * h) as usize);
        for y in 0..h as usize {
            let line = &data[y * padded as usize..y * padded as usize + row as usize];
            // BGRA to RGBA.
            for px in line.chunks_exact(4) {
                out.extend_from_slice(&[px[2], px[1], px[0], px[3]]);
            }
        }
        (w, h, out)
    }

    /// `n` editing frames with the preview on (for screenshots).
    pub async fn edit_frames(&mut self, n: u32) {
        let slice = self.cfg.slice_ms;
        self.run(n, |_, _| Spec { preview: true, edit: true, slice_ms: slice }).await;
    }

    /// Draw the editor view with the preview corner onto `view` (a canvas or window frame).
    pub fn present(&mut self, view: &wgpu::TextureView) {
        let g = self.gpu;
        self.editor.set_camera(&g.queue, &self.cam);
        let mut enc = g.device.create_command_encoder(&Default::default());
        self.editor.march(&mut enc, &self.map, None);
        let rect = self.corner();
        self.preview.composite_into(&g.device, &g.queue, &mut enc, &self.editor.color, rect, None);
        self.editor.blit(&mut enc, view, None);
        g.queue.submit([enc.finish()]);
    }

    /// A few preview frames with the given slice, no edits (for screenshots).
    pub async fn settle(&mut self, frames: u32, slice_ms: f64) {
        self.run(frames, |_, _| Spec { preview: true, edit: false, slice_ms }).await;
    }

    /// Raw accumulation at `spp` samples per pixel, no denoise (the "noisy" image).
    pub async fn noisy(&mut self, spp: u32) -> (f64, Vec<u8>) {
        let g = self.gpu;
        let cam = self.cam;
        let was = self.preview.denoise;
        self.preview.denoise = false;
        self.preview.restart();
        let mut enc = g.device.create_command_encoder(&Default::default());
        self.preview.encode(&g.queue, &mut enc, &self.map, &cam, None, PREVIEW_H * spp, PREVIEW_H, None);
        g.queue.submit([enc.finish()]);
        gpu::wait_idle(&g.device, &g.queue).await;
        let img = self.read_preview().await;
        self.preview.denoise = was;
        (spp as f64, img)
    }
}

pub async fn adapter_json(gpu: &Gpu) -> Value {
    vxr_engine_spike::bench::limits_json(gpu)
}
