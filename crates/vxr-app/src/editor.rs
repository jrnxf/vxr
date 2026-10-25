//! The editor, independent of the window: the scene on the GPU, the camera, the Hybrid UI
//! and the editing gestures. A host (the native window, or a headless run for screenshots)
//! feeds it pointer events in physical pixels and egui input, and gives it a target to draw to.

use glam::Vec3;
use vxr_core::Scene;
use vxr_core::material::Material;
use vxr_core::tools::{Brush, Gesture, Press, Shape, Target, plane_cell};
use vxr_render::{EditBox, Gpu, GpuMap, OrbitCamera, Preview, Renderer, brickmap_layout, gpu::now_ms};

use crate::render::{EXPORT_SIZES, Export};
use crate::ui::{Action, FontBytes, Hybrid, Stats};

/// Input quiet for this long raises the preview's budget (spikes/preview/RESULTS.md).
const IDLE_MS: f64 = 300.0;

/// Size of a new, empty scene.
pub const NEW_SIZE: [u32; 3] = [128, 128, 128];

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Button {
    Left,
    Right,
    Middle,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum CamDrag {
    Orbit,
    Pan,
}

pub struct Editor {
    pub gpu: Gpu,
    pub scene: Scene,
    layout: wgpu::BindGroupLayout,
    map: GpuMap,
    renderer: Renderer,
    /// The live path-traced preview in the corner.
    pub preview: Preview,
    /// Whether the preview drew last frame (showing it again restarts it).
    preview_shown: bool,
    /// When input last arrived (ms, `now_ms`).
    last_input: f64,
    pub cam: OrbitCamera,
    pub ctx: egui::Context,
    egui: egui_wgpu::Renderer,
    pub ui: Hybrid,
    /// Target size in pixels and pixels per point.
    pub width: u32,
    pub height: u32,
    pub ppp: f32,
    gesture: Option<Gesture>,
    cam_drag: Option<CamDrag>,
    /// Pointer position in pixels, if it is over the view.
    pointer: Option<[f32; 2]>,
    cursor: Option<Target>,
    /// Space held: left-drag orbits.
    pub space: bool,
    palette: [[u8; 4]; 256],
    /// The materials the GPU has, to notice changes.
    materials: [Material; 256],
    frames: Vec<f64>,
    /// Where the scene was opened from or last saved to.
    pub path: Option<std::path::PathBuf>,
    /// A PNG render export in progress (the corner pauses meanwhile).
    pub export: Option<Export>,
    /// The last finished export's PNG and size, for the host to save or download.
    pub exported: Option<(Vec<u8>, (u32, u32))>,
    /// Export sizes this GPU can render.
    export_fits: [bool; 4],
}

impl Editor {
    pub fn new(
        gpu: Gpu,
        scene: Scene,
        name: &str,
        format: wgpu::TextureFormat,
        size: (u32, u32),
        ppp: f32,
        fonts: Option<FontBytes>,
    ) -> Result<Self, String> {
        let layout = brickmap_layout(&gpu.device);
        let mut scene = scene;
        scene.map.track_dirty(true);
        let map = GpuMap::new(&gpu.device, &gpu.queue, &layout, &scene.map, &scene.palette)?;
        let (width, height) = (size.0.max(1), size.1.max(1));
        let renderer = Renderer::new(&gpu.device, &layout, format, width, height, false);
        let mut preview = Preview::new(&gpu.device, &gpu.queue, &layout, &scene.palette, gpu.timestamps);
        preview.set_materials(&gpu.queue, &scene.materials);
        let egui = egui_wgpu::Renderer::new(&gpu.device, format, egui_wgpu::RendererOptions::default());
        let ctx = egui::Context::default();
        crate::ui::install(&ctx, fonts);
        let ui = Hybrid::new(scene.palette, used(&scene), name);
        let (palette, materials) = (scene.palette, scene.materials);
        let mut e = Self {
            gpu,
            scene,
            layout,
            map,
            renderer,
            preview,
            preview_shown: false,
            last_input: 0.0,
            cam: OrbitCamera::framing(Vec3::ZERO, Vec3::ONE),
            ctx,
            egui,
            ui,
            width,
            height,
            ppp,
            gesture: None,
            cam_drag: None,
            pointer: None,
            cursor: None,
            space: false,
            palette,
            materials,
            frames: Vec::new(),
            path: None,
            export: None,
            exported: None,
            export_fits: [false; 4],
        };
        e.export_fits = EXPORT_SIZES.map(|(w, h)| Preview::fits(&e.gpu.device, w, h));
        e.frame_scene();
        Ok(e)
    }

    /// Replace the scene (after Open), reframing the camera.
    pub fn load(&mut self, scene: Scene, name: &str) -> Result<(), String> {
        let mut scene = scene;
        scene.map.track_dirty(true);
        self.map = GpuMap::new(&self.gpu.device, &self.gpu.queue, &self.layout, &scene.map, &scene.palette)?;
        self.palette = scene.palette;
        self.preview.set_palette(&self.gpu.queue, &self.palette);
        self.materials = scene.materials;
        self.preview.set_materials(&self.gpu.queue, &self.materials);
        self.preview.restart();
        self.export = None;
        self.scene = scene;
        self.ui.scene_name = name.into();
        self.gesture = None;
        self.cursor = None;
        self.frame_scene();
        Ok(())
    }

    pub fn resize(&mut self, width: u32, height: u32, ppp: f32) {
        let (width, height) = (width.max(1), height.max(1));
        self.renderer.resize(&self.gpu.device, width, height);
        (self.width, self.height, self.ppp) = (width, height, ppp);
    }

    /// Frame the model (or the floor of an empty scene), like MagicaVoxel's `.`.
    pub fn frame_scene(&mut self) {
        let (lo, hi) = match self.scene.map.occupied_bounds() {
            Some(b) => b,
            None => {
                let s = self.scene.map.size_voxels();
                let f = self.scene.floor.max(0) as u32;
                ([0, f, 0], [s[0], f + 1, s[2]])
            }
        };
        let v = |a: [u32; 3]| Vec3::new(a[0] as f32, a[1] as f32, a[2] as f32);
        self.cam = OrbitCamera::framing_aspect(v(lo), v(hi), self.width as f32 / self.height as f32);
    }

    /// What the pointer is over, if anything.
    pub fn cursor(&self) -> Option<Target> {
        self.cursor
    }

    fn ray(&self, p: [f32; 2]) -> ([f32; 3], [f32; 3]) {
        let (o, d) = self.cam.ray(p[0], p[1], self.width as f32, self.height as f32);
        (o.to_array(), d.to_array())
    }

    fn brush(&self) -> Brush {
        Brush {
            tool: self.ui.effective_tool(),
            shape: self.ui.shape,
            mirror: self.ui.mirror_axes(),
            lock: self.ui.lock,
            color: self.ui.slot,
        }
    }

    /// The pointer moved to `p` (pixels), or left the view (`None`).
    pub fn pointer_moved(&mut self, p: Option<[f32; 2]>) {
        self.last_input = now_ms();
        let last = self.pointer;
        self.pointer = p;
        let Some(p) = p else {
            self.cursor = None;
            return;
        };
        if let (Some(drag), Some(last)) = (self.cam_drag, last) {
            let (dx, dy) = (p[0] - last[0], p[1] - last[1]);
            match drag {
                CamDrag::Orbit => self.cam.orbit(dx, dy),
                CamDrag::Pan => self.cam.pan(dx, dy, self.height as f32),
            }
            return;
        }
        let (o, d) = self.ray(p);
        if let Some(g) = &mut self.gesture {
            let cell = if g.on_plane() {
                plane_cell(o, d, g.axis, g.layer)
            } else {
                self.scene.target(o, d).filter(|t| !t.ground).map(|t| t.voxel)
            };
            if let Some(c) = cell {
                g.move_to(&mut self.scene, c);
            }
            return;
        }
        self.cursor = self.scene.target(o, d);
    }

    /// A mouse button changed. `over_ui` is true when egui has the pointer.
    pub fn button(&mut self, b: Button, pressed: bool, over_ui: bool) {
        self.last_input = now_ms();
        if !pressed {
            if let Some(g) = self.gesture.take()
                && b == Button::Left
            {
                g.release(&mut self.scene);
            }
            self.cam_drag = None;
            return;
        }
        if over_ui || self.gesture.is_some() || self.cam_drag.is_some() {
            return;
        }
        let (shift, _) = self.ui.modifiers();
        self.cam_drag = match b {
            Button::Right if shift => Some(CamDrag::Pan),
            Button::Right => Some(CamDrag::Orbit),
            Button::Middle => Some(CamDrag::Pan),
            Button::Left if self.space => Some(CamDrag::Orbit),
            Button::Left => None,
        };
        if b != Button::Left || self.space {
            return;
        }
        self.ui.coach = false;
        // A colour edit still open (a field not yet applied) is its own undo step.
        self.scene.commit();
        let Some(p) = self.pointer else { return };
        let (o, d) = self.ray(p);
        let Some(t) = self.scene.target(o, d) else { return };
        match self.scene.press(self.brush(), t) {
            Press::Picked(v) => {
                self.ui.slot = v;
                let c = self.scene.palette[v as usize];
                self.ui.toast(&self.ctx, format!("Picked #{:02X}{:02X}{:02X}", c[0], c[1], c[2]));
            }
            Press::Drag(g) => self.gesture = Some(g),
            Press::Done(_) | Press::Nothing => {}
        }
    }

    /// Wheel zoom toward the point under the pointer, in notches (positive zooms in).
    pub fn scroll(&mut self, notches: f32) {
        self.last_input = now_ms();
        let hit = self.pointer.and_then(|p| {
            let (o, d) = self.ray(p);
            self.scene.map.raycast(o, d, 1e5).map(|h| (Vec3::from(d), h.t))
        });
        self.cam.zoom(notches, hit);
    }

    pub fn undo(&mut self) {
        self.gesture = None;
        self.scene.undo();
    }

    pub fn redo(&mut self) {
        self.gesture = None;
        self.scene.redo();
    }

    /// What a shape drag in progress spans, for the stats panel.
    pub fn span(&self) -> Option<(&'static str, String)> {
        self.gesture.as_ref().and_then(|g| {
            let (lo, hi) = g.extent();
            let d: [i32; 3] = std::array::from_fn(|a| hi[a] - lo[a] + 1);
            match g.brush().shape {
                Shape::Box => Some(("BOX", format!("{}×{}×{}", d[0], d[1], d[2]))),
                Shape::Line => Some(("LINE", format!("{}", d.into_iter().max().unwrap_or(1)))),
                Shape::Circle | Shape::Sphere => Some(("RADIUS", format!("{}", g.radius()))),
                Shape::Voxel | Shape::Face => None,
            }
        })
    }

    fn stats(&self) -> Stats {
        let n = self.frames.len();
        let (fps, frame_ms) = if n >= 2 {
            let span = self.frames[n - 1] - self.frames[0];
            ((n - 1) as f64 * 1000.0 / span, span / (n - 1) as f64)
        } else {
            (0.0, 0.0)
        };
        let cursor = self.cursor.filter(|t| !t.ground).map(|t| self.scene.to_world(t.voxel));
        let span = self.span();
        let render = self.ui.show_render.then(|| (self.preview.samples(), self.preview.last_rows > 0));
        let export = self.export.as_ref().map(|x| {
            let (done, of) = x.progress();
            (x.size(), done, of)
        });
        Stats {
            voxels: self.scene.map.voxels,
            fps: fps as f32,
            frame_ms: frame_ms as f32,
            cursor,
            span,
            render,
            export,
            export_fits: self.export_fits,
        }
    }

    /// Run the UI, apply what it asked for, and draw a frame onto `view`. Returns the actions
    /// the host handles (open, save, embed) and egui's platform output.
    pub fn frame(&mut self, input: egui::RawInput, view: &wgpu::TextureView) -> (Vec<Action>, egui::PlatformOutput) {
        let now = now_ms();
        if !input.events.is_empty() {
            self.last_input = now;
        }
        self.frames.push(now);
        let keep = self.frames.iter().position(|&t| now - t <= 500.0).unwrap_or(0);
        self.frames.drain(..keep);

        self.ui.palette = self.scene.palette;
        self.ui.materials = self.scene.materials;
        self.ui.used = used(&self.scene);
        self.ui.can_undo = self.scene.history().can_undo();
        self.ui.can_redo = self.scene.history().can_redo();
        let stats = self.stats();
        let mut actions = Vec::new();
        let ui = &mut self.ui;
        let out = self.ctx.run_ui(input, |u| actions = ui.ui(u.ctx(), &stats));
        let mut host = Vec::new();
        for a in actions {
            match a {
                Action::Undo => self.undo(),
                Action::Redo => self.redo(),
                Action::Frame => self.frame_scene(),
                Action::Recolor { slot, rgba } => self.scene.put_color(slot, rgba),
                Action::SetMaterial { slot, material } => self.scene.put_material(slot, material),
                Action::CommitColor => {
                    self.scene.commit();
                }
                Action::SwapColors(a, b) => {
                    self.gesture = None;
                    self.scene.swap_colors(a, b);
                    self.ui.toast(&self.ctx, format!("Swapped slots {a} and {b}"));
                }
                Action::CopyColor { from, to } => {
                    self.gesture = None;
                    self.scene.copy_color(from, to);
                    self.ui.toast(&self.ctx, format!("Copied slot {from} to slot {to}"));
                }
                Action::ExportPng => self.start_export(),
                Action::CancelExport => {
                    self.export = None;
                    self.ui.toast(&self.ctx, "Export cancelled");
                }
                other => host.push(other),
            }
        }
        self.preview.lighting = self.ui.render.lighting(self.cam.distance);
        self.preview.max_spp = self.ui.render.samples();
        if let Some(done) = self.draw(out.shapes, out.pixels_per_point, out.textures_delta, view) {
            host.push(done);
        }
        (host, out.platform_output)
    }

    /// The file name for a render export: the scene's name with `-render.png`.
    pub fn export_name(&self) -> String {
        let n = self.ui.scene_name.trim_end_matches(".vox").trim_end_matches(".VOX");
        format!("{}-render.png", if n.is_empty() { "vxr" } else { n })
    }

    /// Start a PNG export of the corner's view with the Render panel's settings.
    pub fn start_export(&mut self) {
        let r = Export::start(
            &self.gpu,
            &self.layout,
            &self.scene.palette,
            &self.scene.materials,
            self.cam,
            &self.ui.render,
        );
        match r {
            Ok(x) => {
                let (w, h) = x.size();
                self.export = Some(x);
                self.ui.show_render = true;
                self.ui.toast(&self.ctx, format!("Rendering {w}×{h} at {} spp", self.ui.render.samples()));
            }
            Err(e) => self.ui.toast(&self.ctx, format!("Could not export: {e}")),
        }
    }

    /// Draw a frame; returns `Action::Exported` when an export finished this frame.
    fn draw(
        &mut self,
        shapes: Vec<egui::epaint::ClippedShape>,
        ppp: f32,
        textures: egui::TexturesDelta,
        view: &wgpu::TextureView,
    ) -> Option<Action> {
        let (dev, queue) = (&self.gpu.device, &self.gpu.queue);
        if self.scene.palette != self.palette {
            self.palette = self.scene.palette;
            self.map.set_palette(queue, &self.palette);
            self.preview.set_palette(queue, &self.palette);
            if let Some(x) = &mut self.export {
                x.set_palette(queue, &self.palette);
            }
        }
        if self.scene.materials != self.materials {
            self.materials = self.scene.materials;
            self.preview.set_materials(queue, &self.materials);
            if let Some(x) = &mut self.export {
                x.set_materials(queue, &self.materials);
            }
        }
        let edit = self.scene.map.take_dirty_box().map(|(lo, hi)| EditBox::voxels(lo, hi));
        if let Err(e) = self.map.upload_dirty(dev, queue, &mut self.scene.map, &self.scene.palette) {
            eprintln!("upload failed: {e}");
        }
        let tint = if self.gesture.is_none() { self.cursor.filter(|t| !t.ground).map(|t| t.voxel) } else { None };
        self.renderer.set_camera(queue, &self.cam, tint);
        let mut enc = dev.create_command_encoder(&Default::default());
        // The preview's passes go first so they can be timed on their own (see preview.rs).
        let corner = self.ui.render_rect.filter(|_| self.ui.show_render);
        if let Some(x) = &mut self.export {
            x.encode(queue, &mut enc, &self.map, edit);
        }
        // An export takes the corner's GPU time; the corner keeps its last image meanwhile.
        if corner.is_some() && self.export.is_none() {
            if !self.preview_shown {
                self.preview.restart();
            }
            let idle = now_ms() - self.last_input > IDLE_MS;
            self.preview.encode(queue, &mut enc, &self.map, &self.cam, edit, idle);
        }
        self.preview_shown = corner.is_some();
        self.renderer.march(&mut enc, &self.map, None);
        if let Some(r) = corner {
            let px = [r.left() * ppp, r.top() * ppp, r.width() * ppp, r.height() * ppp];
            self.preview.composite_into(dev, queue, &mut enc, &self.renderer.color, px, crate::ui::R as f32 * ppp);
        }
        self.renderer.blit(&mut enc, view, None);
        let jobs = self.ctx.tessellate(shapes, ppp);
        for (id, deltas) in &textures.set {
            for delta in deltas {
                self.egui.update_texture(dev, queue, *id, delta);
            }
        }
        let screen = egui_wgpu::ScreenDescriptor { size_in_pixels: [self.width, self.height], pixels_per_point: ppp };
        let extra = self.egui.update_buffers(dev, queue, &mut enc, &jobs, &screen);
        {
            let pass = enc.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("egui"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view,
                    depth_slice: None,
                    resolve_target: None,
                    ops: wgpu::Operations { load: wgpu::LoadOp::Load, store: wgpu::StoreOp::Store },
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            });
            self.egui.render(&mut pass.forget_lifetime(), &jobs, &screen);
        }
        for id in &textures.free {
            self.egui.free_texture(id);
        }
        queue.submit(extra.into_iter().chain([enc.finish()]));
        if corner.is_some() {
            self.preview.after_submit(dev);
        }
        let done = self.export.as_mut().and_then(|x| x.after_submit(&self.gpu).map(|r| (r, x.size())));
        match done {
            Some((Ok(png), size)) => {
                self.export = None;
                self.exported = Some((png, size));
                Some(Action::Exported)
            }
            Some((Err(e), _)) => {
                self.export = None;
                self.ui.toast(&self.ctx, format!("Could not export: {e}"));
                None
            }
            None => None,
        }
    }
}

/// Palette indices with voxels in the scene, from the map's per-colour counts.
fn used(scene: &Scene) -> [bool; 256] {
    let mut u = [false; 256];
    for (i, c) in scene.map.counts.iter().enumerate().skip(1) {
        u[i] = *c > 0;
    }
    u
}
