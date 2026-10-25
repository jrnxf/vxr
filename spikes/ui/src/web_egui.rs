//! Browser build of the egui Hybrid UI: the same `Viewer` canvas, with `hybrid::Hybrid` drawn by
//! egui-wgpu into the canvas frame after the blit. The page (`web/egui.html`) only forwards DOM
//! input events here; every panel, label and Cmd-K is egui.

use crate::hybrid::{Action, FontBytes, Hybrid, Stats, Tool};
use crate::web::Viewer;
use vxr_engine_spike::{bench::TARGET_FORMAT, gpu};
use wasm_bindgen::prelude::*;

#[wasm_bindgen]
pub struct EguiApp {
    v: Viewer,
    ctx: egui::Context,
    egui: egui_wgpu::Renderer,
    hybrid: Hybrid,
    events: Vec<egui::Event>,
    modifiers: egui::Modifiers,
    ppp: f32,
    stats: Stats,
    /// egui run + tessellate + upload of the last frame, ms.
    ui_cpu: f64,
    /// Freeze the stats corner for screenshots.
    frozen: bool,
}

fn used_slots(v: &Viewer) -> Vec<bool> {
    v.used().into_iter().map(|u| u != 0).collect()
}

/// Open the `.vox` bytes on `canvas` (sized in device pixels, `ppp` device pixels per CSS px).
#[wasm_bindgen]
#[allow(clippy::too_many_arguments)]
pub async fn create_egui_app(
    canvas: web_sys::HtmlCanvasElement,
    vox: Vec<u8>,
    ppp: f32,
    inter_regular: Vec<u8>,
    inter_medium: Vec<u8>,
    inter_semibold: Vec<u8>,
    mono_regular: Vec<u8>,
    mono_medium: Vec<u8>,
    icons: Vec<u8>,
) -> Result<EguiApp, JsValue> {
    let v = crate::web::create_viewer(canvas, vox).await?;
    let ctx = egui::Context::default();
    crate::hybrid::install(&ctx, FontBytes { inter_regular, inter_medium, inter_semibold, mono_regular, mono_medium, icons });
    let egui = egui_wgpu::Renderer::new(&v.gpu.device, TARGET_FORMAT, egui_wgpu::RendererOptions::default());
    let mut palette = [0u32; 256];
    palette.copy_from_slice(&v.scene.palette[..256]);
    let hybrid = Hybrid::new(palette, used_slots(&v), "Island diorama");
    let stats = Stats { voxels: v.scene.map.voxels, ..Default::default() };
    Ok(EguiApp { v, ctx, egui, hybrid, events: vec![], modifiers: Default::default(), ppp, stats, ui_cpu: 0.0, frozen: false })
}

#[wasm_bindgen]
impl EguiApp {
    /// Canvas size in device pixels.
    pub fn resize(&mut self, width: u32, height: u32, ppp: f32) {
        self.ppp = ppp;
        self.v.resize(width, height);
    }

    pub fn set_modifiers(&mut self, shift: bool, alt: bool, ctrl: bool, meta: bool) {
        self.modifiers = egui::Modifiers { alt, ctrl, shift, mac_cmd: meta, command: meta || ctrl };
        self.events.push(egui::Event::ModifiersChanged(self.modifiers));
    }

    /// Pointer moved to CSS px (x, y).
    pub fn pointer_move(&mut self, x: f32, y: f32) {
        self.events.push(egui::Event::PointerMoved(egui::pos2(x, y)));
        if !self.frozen {
            self.stats.cursor = self.v.hit(x * self.ppp, y * self.ppp).map(|h| h.voxel);
        }
    }

    /// A button press or release at CSS px (x, y). Returns true when egui takes it (the pointer is
    /// over a panel or egui is dragging), false when the page should treat it as a scene click.
    pub fn pointer_button(&mut self, x: f32, y: f32, button: u32, pressed: bool) -> bool {
        let pos = egui::pos2(x, y);
        let over_ui = self.ctx.layer_id_at(pos).is_some_and(|l| l.order != egui::Order::Background);
        let button = match button {
            1 => egui::PointerButton::Middle,
            2 => egui::PointerButton::Secondary,
            _ => egui::PointerButton::Primary,
        };
        self.events.push(egui::Event::PointerMoved(pos));
        self.events.push(egui::Event::PointerButton { pos, button, pressed, modifiers: self.modifiers });
        over_ui || self.ctx.egui_is_using_pointer()
    }

    pub fn pointer_gone(&mut self) {
        self.events.push(egui::Event::PointerGone);
    }

    /// A DOM key event: `key` is `KeyboardEvent.key`. Returns true when the page should
    /// preventDefault (egui knows the key or is typing).
    pub fn key(&mut self, key: &str, pressed: bool, repeat: bool) -> bool {
        let typing = self.ctx.egui_wants_keyboard_input();
        let mut known = false;
        if let Some(k) = egui::Key::from_name(key) {
            self.events.push(egui::Event::Key { key: k, physical_key: None, pressed, repeat, modifiers: self.modifiers });
            known = true;
        }
        if pressed && !self.modifiers.command && key.chars().count() == 1 {
            self.events.push(egui::Event::Text(key.into()));
        }
        known || typing
    }

    /// IME composition from the page's hidden text field: `commit` false is a preedit update.
    pub fn ime(&mut self, text: &str, commit: bool) {
        let ev = if commit { egui::ImeEvent::Commit(text.into()) } else { egui::ImeEvent::Preedit { text: text.into(), active_range_chars: None } };
        self.events.push(egui::Event::Ime(ev));
    }

    /// True while an egui text field has focus (the page then routes typing and IME to it).
    pub fn wants_keyboard(&self) -> bool {
        self.ctx.egui_wants_keyboard_input()
    }

    pub fn orbit(&mut self, dx: f32, dy: f32) {
        self.v.orbit(dx, dy);
    }

    pub fn pan(&mut self, dx: f32, dy: f32) {
        self.v.pan(dx, dy);
    }

    /// Zoom toward CSS px (x, y).
    pub fn zoom(&mut self, steps: f32, x: f32, y: f32) {
        self.v.zoom(steps, x * self.ppp, y * self.ppp);
    }

    /// A scene click at CSS px (x, y) with the UI's current tool and colour. The edit is uploaded
    /// before this returns, so the next `frame` draws it. Returns `Viewer::edit`'s result.
    pub fn click(&mut self, x: f32, y: f32) -> i32 {
        let tool = match self.hybrid.effective_tool() {
            Tool::Add => 0,
            Tool::Erase => 1,
            Tool::Paint => 2,
            Tool::Pick => 3,
        };
        let r = self.v.edit(x * self.ppp, y * self.ppp, tool, self.hybrid.slot);
        if tool == 3 && r > 0 {
            self.hybrid.slot = r as u8;
        } else if r == 1 {
            self.hybrid.used = used_slots(&self.v);
            self.stats.voxels = self.v.scene.map.voxels;
        }
        r
    }

    /// Fix the stats corner at 120 fps and 0.0 ms, like the native screenshots.
    pub fn freeze_stats(&mut self) {
        self.frozen = true;
        self.stats = Stats { voxels: self.v.scene.map.voxels, fps: 120.0, frame_ms: 0.0, cursor: None };
    }

    pub fn set_fps(&mut self, fps: f32, frame_ms: f32) {
        if !self.frozen {
            self.stats.fps = fps;
            self.stats.frame_ms = frame_ms;
        }
    }

    /// Open Cmd-K with `text` and hide the coach card (screenshots).
    pub fn open_cmdk(&mut self, text: &str) {
        self.hybrid.coach = false;
        self.hybrid.cmdk = Some(text.into());
    }

    /// The UI's CPU cost of the last frame (egui run, tessellate, upload), ms.
    pub fn ui_cpu_ms(&self) -> f64 {
        self.ui_cpu
    }

    /// Draw one frame with the UI at time `time` (seconds). Returns true if egui wants another
    /// frame soon (an animation or a blinking cursor).
    pub fn frame(&mut self, time: f64) -> bool {
        let input = self.raw_input(time);
        let t0 = gpu::now_ms();
        let out = self.ctx.run_ui(input, |ui| {
            let actions = self.hybrid.ui(ui.ctx(), &self.stats);
            for a in actions {
                if a == Action::Frame {
                    self.v.frame_scene();
                }
            }
        });
        let jobs = self.ctx.tessellate(out.shapes, out.pixels_per_point);
        let screen = egui_wgpu::ScreenDescriptor { size_in_pixels: [self.v.width, self.v.height], pixels_per_point: self.ppp };
        let (egui, ui_cpu) = (&mut self.egui, &mut self.ui_cpu);
        self.v.frame_with(|g, enc, view| {
            for (id, deltas) in &out.textures_delta.set {
                for delta in deltas {
                    egui.update_texture(&g.device, &g.queue, *id, delta);
                }
            }
            let extra = egui.update_buffers(&g.device, &g.queue, enc, &jobs, &screen);
            *ui_cpu = gpu::now_ms() - t0;
            ui_pass(egui, enc, view, &jobs, &screen, None);
            g.queue.submit(extra);
        });
        for id in &out.textures_delta.free {
            self.egui.free_texture(id);
        }
        out.viewport_output.get(&egui::ViewportId::ROOT).is_some_and(|v| v.repaint_delay.as_secs_f32() < 0.5)
    }

    /// Time `n` frames with the UI and `n` without, alternating, orbiting, one frame in flight.
    /// Returns [gpu_without_median, gpu_without_p95, gpu_with_median, gpu_with_p95,
    /// ui_cpu_median, ui_cpu_p95] in ms, or the CPU numbers only (GPU 0) without timestamps.
    pub async fn bench(&mut self, n: u32) -> Vec<f64> {
        let g = &self.v.gpu;
        let ts = g.timestamps.then(|| {
            let q = g.device.create_query_set(&wgpu::QuerySetDescriptor { label: Some("ui ts"), ty: wgpu::QueryType::Timestamp, count: n * 2 * 3 });
            let buf = |usage| g.device.create_buffer(&wgpu::BufferDescriptor { label: Some("ts"), size: n as u64 * 2 * 3 * 8, usage, mapped_at_creation: false });
            (q, buf(wgpu::BufferUsages::QUERY_RESOLVE | wgpu::BufferUsages::COPY_SRC), buf(wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ))
        });
        let target = g.device.create_texture(&wgpu::TextureDescriptor {
            label: Some("bench"),
            size: wgpu::Extent3d { width: self.v.width, height: self.v.height, depth_or_array_layers: 1 },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: TARGET_FORMAT,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
            view_formats: &[],
        });
        let view = target.create_view(&Default::default());
        let mut cpu = vec![];
        for i in 0..n * 2 {
            self.v.cam.orbit(2.0, 0.0);
            let with_ui = i % 2 == 0;
            let input = self.raw_input(i as f64 / 60.0);
            let g = &self.v.gpu;
            self.v.renderer.set_camera(&g.queue, &self.v.cam);
            let mut enc = g.device.create_command_encoder(&Default::default());
            let slot = i * 3;
            let q = ts.as_ref().map(|t| &t.0);
            self.v.renderer.march(&mut enc, &self.v.map, q.map(|q| wgpu::ComputePassTimestampWrites { query_set: q, beginning_of_pass_write_index: Some(slot), end_of_pass_write_index: None }));
            self.v.renderer.blit(&mut enc, &view, q.map(|q| wgpu::RenderPassTimestampWrites { query_set: q, beginning_of_pass_write_index: None, end_of_pass_write_index: Some(slot + 1) }));
            if with_ui {
                let t0 = gpu::now_ms();
                let hybrid = &mut self.hybrid;
                let out = self.ctx.run_ui(input, |ui| {
                    hybrid.ui(ui.ctx(), &self.stats);
                });
                let jobs = self.ctx.tessellate(out.shapes, out.pixels_per_point);
                for (id, deltas) in &out.textures_delta.set {
                    for delta in deltas {
                        self.egui.update_texture(&g.device, &g.queue, *id, delta);
                    }
                }
                let screen = egui_wgpu::ScreenDescriptor { size_in_pixels: [self.v.width, self.v.height], pixels_per_point: self.ppp };
                let extra = self.egui.update_buffers(&g.device, &g.queue, &mut enc, &jobs, &screen);
                cpu.push(gpu::now_ms() - t0);
                ui_pass(&mut self.egui, &mut enc, &view, &jobs, &screen, q.map(|q| (q, slot + 2)));
                g.queue.submit(extra);
            }
            g.queue.submit([enc.finish()]);
            gpu::wait_idle(&g.device, &g.queue).await;
        }
        let pct = |v: &mut Vec<f64>, p: f64| {
            v.sort_by(|a, b| a.partial_cmp(b).unwrap());
            v.get(((v.len().max(1) - 1) as f64 * p).round() as usize).copied().unwrap_or(0.0)
        };
        let (mut with, mut without) = (vec![], vec![]);
        if let Some((q, resolve, read)) = &ts {
            let g = &self.v.gpu;
            let mut enc = g.device.create_command_encoder(&Default::default());
            enc.resolve_query_set(q, 0..n * 2 * 3, resolve, 0);
            enc.copy_buffer_to_buffer(resolve, 0, read, 0, Some(n as u64 * 2 * 3 * 8));
            g.queue.submit([enc.finish()]);
            let data = gpu::map_read(&g.device, read).await;
            let t: Vec<u64> = data.as_chunks::<8>().0.iter().map(|c| u64::from_le_bytes(*c)).collect();
            let period = g.queue.get_timestamp_period() as f64;
            let ms = |a: u64, b: u64| b.wrapping_sub(a) as f64 * period / 1e6;
            for i in 0..(n * 2) as usize {
                if i % 2 == 0 {
                    with.push(ms(t[i * 3], t[i * 3 + 2]));
                } else {
                    without.push(ms(t[i * 3], t[i * 3 + 1]));
                }
            }
        }
        vec![pct(&mut without, 0.5), pct(&mut without, 0.95), pct(&mut with, 0.5), pct(&mut with, 0.95), pct(&mut cpu, 0.5), pct(&mut cpu, 0.95)]
    }
}

impl EguiApp {
    fn raw_input(&mut self, time: f64) -> egui::RawInput {
        let size = egui::vec2(self.v.width as f32 / self.ppp, self.v.height as f32 / self.ppp);
        let mut input = egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, size)),
            time: Some(time),
            events: std::mem::take(&mut self.events),
            ..Default::default()
        };
        input.viewports.entry(egui::ViewportId::ROOT).or_default().native_pixels_per_point = Some(self.ppp);
        input
    }
}

/// The egui pass, loading the frame the march and blit drew. `ts` writes the end-of-pass stamp.
fn ui_pass(
    egui: &mut egui_wgpu::Renderer,
    enc: &mut wgpu::CommandEncoder,
    view: &wgpu::TextureView,
    jobs: &[egui::ClippedPrimitive],
    screen: &egui_wgpu::ScreenDescriptor,
    ts: Option<(&wgpu::QuerySet, u32)>,
) {
    let pass = enc.begin_render_pass(&wgpu::RenderPassDescriptor {
        label: Some("egui"),
        color_attachments: &[Some(wgpu::RenderPassColorAttachment {
            view,
            depth_slice: None,
            resolve_target: None,
            ops: wgpu::Operations { load: wgpu::LoadOp::Load, store: wgpu::StoreOp::Store },
        })],
        depth_stencil_attachment: None,
        timestamp_writes: ts.map(|(q, i)| wgpu::RenderPassTimestampWrites { query_set: q, beginning_of_pass_write_index: None, end_of_pass_write_index: Some(i) }),
        occlusion_query_set: None,
        multiview_mask: None,
    });
    egui.render(&mut pass.forget_lifetime(), jobs, screen);
}
