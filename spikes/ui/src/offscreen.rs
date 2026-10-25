//! Headless native frames: the test 1 renderer plus the egui Hybrid UI drawn into an offscreen
//! texture, read back for screenshots and timed with GPU timestamps. No window, so it works
//! with the screen locked.

use crate::hybrid::{FontBytes, Hybrid, Stats};
use vxr_engine_spike::{bench::TARGET_FORMAT, camera::OrbitCamera, gpu, scene::Scene};

pub fn load_fonts(dir: &std::path::Path) -> FontBytes {
    let read = |n: &str| std::fs::read(dir.join(n)).unwrap_or_else(|e| panic!("{n}: {e} (run spikes/ui/fetch-fonts)"));
    FontBytes {
        inter_regular: read("Inter-Regular.ttf"),
        inter_medium: read("Inter-Medium.ttf"),
        inter_semibold: read("Inter-SemiBold.ttf"),
        mono_regular: read("JetBrainsMono-Regular.ttf"),
        mono_medium: read("JetBrainsMono-Medium.ttf"),
        icons: read("lucide.ttf"),
    }
}

/// Frames whose GPU times can be read back at once.
pub const MAX_FRAMES: u32 = 1000;

/// GPU and CPU cost of one frame, in milliseconds.
#[derive(Clone, Copy, Debug, Default)]
pub struct FrameTimes {
    /// egui run + tessellate + buffer upload on the CPU.
    pub ui_cpu: f64,
}

pub struct Offscreen {
    pub gpu: gpu::Gpu,
    pub scene: Scene,
    pub map: gpu::GpuBrickMap,
    pub renderer: gpu::Renderer,
    pub cam: OrbitCamera,
    pub ctx: egui::Context,
    egui: egui_wgpu::Renderer,
    target: wgpu::Texture,
    pub width: u32,
    pub height: u32,
    pub ppp: f32,
    queries: wgpu::QuerySet,
    resolve: wgpu::Buffer,
    read: wgpu::Buffer,
    period_ns: f32,
}

impl Offscreen {
    /// `points` is the logical size; the texture is `points * ppp` pixels.
    pub fn new(scene: Scene, points: (u32, u32), ppp: f32, fonts: FontBytes) -> Self {
        let instance = wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle());
        let g = pollster::block_on(gpu::Gpu::new(instance, None)).expect("gpu");
        assert!(g.timestamps, "adapter has no timestamp queries");
        let (width, height) = ((points.0 as f32 * ppp) as u32, (points.1 as f32 * ppp) as u32);
        let layout = gpu::brickmap_layout(&g.device);
        let map = gpu::GpuBrickMap::new_with_layout(&g.device, &g.queue, &layout, &scene.map, &scene.palette, gpu::MapLayout::Compact).expect("upload");
        let renderer = gpu::Renderer::new(&g.device, &layout, TARGET_FORMAT, width, height, gpu::RenderOptions { gbuffer: false, levels: 4 });
        let (lo, hi) = scene.map.occupied_bounds().unwrap_or(([0; 3], [8; 3]));
        let v = |a: [u32; 3]| glam::Vec3::new(a[0] as f32, a[1] as f32, a[2] as f32);
        let cam = OrbitCamera::framing_aspect(v(lo), v(hi), width as f32 / height as f32);
        let target = g.device.create_texture(&wgpu::TextureDescriptor {
            label: Some("frame"),
            size: wgpu::Extent3d { width, height, depth_or_array_layers: 1 },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: TARGET_FORMAT,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
            view_formats: &[],
        });
        let egui = egui_wgpu::Renderer::new(&g.device, TARGET_FORMAT, egui_wgpu::RendererOptions::default());
        let ctx = egui::Context::default();
        crate::hybrid::install(&ctx, fonts);
        let queries = g.device.create_query_set(&wgpu::QuerySetDescriptor { label: Some("frame ts"), ty: wgpu::QueryType::Timestamp, count: MAX_FRAMES * 4 });
        let buf = |usage| g.device.create_buffer(&wgpu::BufferDescriptor { label: Some("ts"), size: MAX_FRAMES as u64 * 32, usage, mapped_at_creation: false });
        let resolve = buf(wgpu::BufferUsages::QUERY_RESOLVE | wgpu::BufferUsages::COPY_SRC);
        let read = buf(wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ);
        let period_ns = g.queue.get_timestamp_period();
        Self { gpu: g, scene, map, renderer, cam, ctx, egui, target, width, height, ppp, queries, resolve, read, period_ns }
    }

    pub fn raw_input(&self, events: Vec<egui::Event>) -> egui::RawInput {
        let size = egui::vec2(self.width as f32 / self.ppp, self.height as f32 / self.ppp);
        let mut input = egui::RawInput { screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, size)), events, ..Default::default() };
        input.viewports.entry(egui::ViewportId::ROOT).or_default().native_pixels_per_point = Some(self.ppp);
        input
    }

    /// Draw one frame (with the UI unless `ui` is None) and wait for it. Returns its timings.
    pub fn frame(&mut self, slot: u32, ui: Option<(&mut Hybrid, &Stats, egui::RawInput)>) -> FrameTimes {
        assert!(slot < MAX_FRAMES);
        let mut times = FrameTimes::default();
        let dev = &self.gpu.device;
        let queue = &self.gpu.queue;
        self.renderer.set_camera(queue, &self.cam);
        let view = self.target.create_view(&Default::default());
        let mut enc = dev.create_command_encoder(&Default::default());
        let begin = wgpu::ComputePassTimestampWrites { query_set: &self.queries, beginning_of_pass_write_index: Some(slot * 4), end_of_pass_write_index: None };
        self.renderer.march(&mut enc, &self.map, Some(begin));
        let end = wgpu::RenderPassTimestampWrites { query_set: &self.queries, beginning_of_pass_write_index: None, end_of_pass_write_index: Some(slot * 4 + 1) };
        self.renderer.blit(&mut enc, &view, Some(end));
        if let Some((hybrid, stats, input)) = ui {
            let t0 = gpu::now_ms();
            let out = self.ctx.run_ui(input, |ui| {
                hybrid.ui(ui.ctx(), stats);
            });
            let jobs = self.ctx.tessellate(out.shapes, out.pixels_per_point);
            for (id, deltas) in &out.textures_delta.set {
                for delta in deltas {
                    self.egui.update_texture(dev, queue, *id, delta);
                }
            }
            let screen = egui_wgpu::ScreenDescriptor { size_in_pixels: [self.width, self.height], pixels_per_point: self.ppp };
            let extra = self.egui.update_buffers(dev, queue, &mut enc, &jobs, &screen);
            times.ui_cpu = gpu::now_ms() - t0;
            {
                let pass = enc.begin_render_pass(&wgpu::RenderPassDescriptor {
                    label: Some("egui"),
                    color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                        view: &view,
                        depth_slice: None,
                        resolve_target: None,
                        ops: wgpu::Operations { load: wgpu::LoadOp::Load, store: wgpu::StoreOp::Store },
                    })],
                    depth_stencil_attachment: None,
                    timestamp_writes: Some(wgpu::RenderPassTimestampWrites { query_set: &self.queries, beginning_of_pass_write_index: Some(slot * 4 + 2), end_of_pass_write_index: Some(slot * 4 + 3) }),
                    occlusion_query_set: None,
                    multiview_mask: None,
                });
                if std::env::var("VXR_UI_EMPTY").is_err() {
                    self.egui.render(&mut pass.forget_lifetime(), &jobs, &screen);
                }
            }
            for id in &out.textures_delta.free {
                self.egui.free_texture(id);
            }
            queue.submit(extra);
        }
        queue.submit([enc.finish()]);
        let _ = dev.poll(wgpu::PollType::wait_indefinitely());
        times
    }

    /// GPU milliseconds for the frames drawn with `slot` 0..n: (march start to blit end, march
    /// start to UI pass end). The second is 0 for frames drawn without the UI. Resolved after all of
    /// them: on Metal, resolving right after a frame read some end-of-pass stamps one frame late.
    pub fn gpu_times(&self, n: u32) -> Vec<(f64, f64)> {
        let (dev, queue) = (&self.gpu.device, &self.gpu.queue);
        pollster::block_on(gpu::wait_idle(dev, queue));
        let mut enc = dev.create_command_encoder(&Default::default());
        enc.resolve_query_set(&self.queries, 0..n * 4, &self.resolve, 0);
        enc.copy_buffer_to_buffer(&self.resolve, 0, &self.read, 0, Some(n as u64 * 32));
        queue.submit([enc.finish()]);
        let data = pollster::block_on(gpu::map_read(dev, &self.read));
        let ts: Vec<u64> = data.as_chunks::<8>().0.iter().map(|c| u64::from_le_bytes(*c)).collect();
        let ms = |a: u64, b: u64| b.wrapping_sub(a) as f64 * self.period_ns as f64 / 1e6;
        (0..n as usize).map(|f| (ms(ts[f * 4], ts[f * 4 + 1]), if ts[f * 4 + 3] > ts[f * 4] { ms(ts[f * 4], ts[f * 4 + 3]) } else { 0.0 })).collect()
    }

    /// The last frame as tightly packed RGBA8.
    pub fn read_rgba(&self) -> Vec<u8> {
        let (dev, queue) = (&self.gpu.device, &self.gpu.queue);
        let row = self.width * 4;
        let padded = row.div_ceil(256) * 256;
        let buf = dev.create_buffer(&wgpu::BufferDescriptor {
            label: Some("readback"),
            size: padded as u64 * self.height as u64,
            usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
            mapped_at_creation: false,
        });
        let mut enc = dev.create_command_encoder(&Default::default());
        enc.copy_texture_to_buffer(
            wgpu::TexelCopyTextureInfo { texture: &self.target, mip_level: 0, origin: wgpu::Origin3d::ZERO, aspect: wgpu::TextureAspect::All },
            wgpu::TexelCopyBufferInfo { buffer: &buf, layout: wgpu::TexelCopyBufferLayout { offset: 0, bytes_per_row: Some(padded), rows_per_image: Some(self.height) } },
            wgpu::Extent3d { width: self.width, height: self.height, depth_or_array_layers: 1 },
        );
        queue.submit([enc.finish()]);
        let data = pollster::block_on(gpu::map_read(dev, &buf));
        let mut out = Vec::with_capacity((row * self.height) as usize);
        for y in 0..self.height as usize {
            for px in data[y * padded as usize..y * padded as usize + row as usize].as_chunks::<4>().0 {
                out.extend_from_slice(&[px[2], px[1], px[0], 255]);
            }
        }
        out
    }
}

pub fn write_png(path: &std::path::Path, w: u32, h: u32, rgba: &[u8]) {
    let f = std::fs::File::create(path).expect("create png");
    let mut e = png::Encoder::new(std::io::BufWriter::new(f), w, h);
    e.set_color(png::ColorType::Rgba);
    e.set_depth(png::BitDepth::Eight);
    e.write_header().unwrap().write_image_data(rgba).unwrap();
}
