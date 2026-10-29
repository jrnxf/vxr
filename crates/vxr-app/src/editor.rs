//! The editor, independent of the window: the scene on the GPU, the camera, the Hybrid UI
//! and the editing gestures. A host (the native window, or a headless run for screenshots)
//! feeds it pointer events in physical pixels and egui input, and gives it a target to draw to.

use glam::Vec3;
use vxr_core::Scene;
use vxr_core::history::At;
use vxr_core::material::Material;
use vxr_core::select::{Mode, MoveDrag, Transform};
use vxr_core::tools::{Brush, Gesture, Press, Shape, Target, Tool, plane_cell};
use vxr_render::{EditBox, Gpu, GpuMap, Guides, OrbitCamera, Preview, Renderer, brickmap_layout, gpu::now_ms};

use crate::agents::Agents;
use crate::assistant::{Assistant, Entry, Progress};
use crate::render::{EXPORT_SIZES, Export};
use crate::review::{Answer, Review, Script};
use crate::touch::Touches;
use crate::ui::{Action, AiAction, ChatView, EditRow, FontBytes, Hybrid, LayerRow, Named, Stats, TreeRow};

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

/// A select tool drag that is not on the selection: a box in pixels from the press.
#[derive(Clone, Copy, Debug)]
struct Marquee {
    start: [f32; 2],
    end: [f32; 2],
    /// What was under the press, for a click (no drag): select that colour.
    target: Option<Target>,
    mode: Mode,
}

/// Pointer travel, in pixels, below which a select press is a click.
const CLICK_PX: f32 = 4.0;

/// An object being dragged in world mode, on the plane across `axis` through the pressed
/// voxel. In world cells, since the world map moves under it as the object does.
#[derive(Clone, Copy, Debug)]
struct WorldDrag {
    /// The node moved: the object pressed, or the picked group holding it.
    object: usize,
    axis: usize,
    start: [i32; 3],
    /// How far the object has been moved so far.
    offset: [i32; 3],
}

/// The touch layout's scale: its points are this many of the mouse layout's, so the rail's
/// 34-point buttons become 44-point targets.
pub const TOUCH_SCALE: f32 = 1.3;

/// Movement below this many points is a finger resting (see `Touches`).
const TOUCH_SLOP: f32 = 10.0;

/// A finger's change, from the host's touch events.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Finger {
    Down,
    Move,
    /// Lifted, or taken by the system.
    Up,
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
    pub renderer: Renderer,
    /// The live path-traced preview in the corner.
    pub preview: Preview,
    /// Whether the preview drew last frame and is up to date (showing it again restarts it).
    preview_shown: bool,
    /// When input last arrived (ms, `now_ms`).
    last_input: f64,
    pub cam: OrbitCamera,
    pub ctx: egui::Context,
    egui: egui_wgpu::Renderer,
    pub ui: Hybrid,
    /// Target size in pixels and pixels per point: the display's (`native_ppp`, what the host
    /// gives egui as its native pixels per point), times `TOUCH_SCALE` in the touch layout
    /// (egui's zoom factor).
    pub width: u32,
    pub height: u32,
    pub ppp: f32,
    pub native_ppp: f32,
    /// The fingers on the view, and the one pressing the UI (the host gives it to egui).
    touches: Touches,
    ui_touch: Option<u64>,
    /// A box brush was lifting when the current finger landed: its release places the box.
    touch_places: bool,
    /// The width in points the docked assistant takes from the right of the view, as the
    /// camera last allowed for: the scene stays centred in the space left over.
    ai_dock: f32,
    gesture: Option<Gesture>,
    /// The selection being dragged, or a box being drawn, with the select tool.
    moving: Option<MoveDrag>,
    /// Where the selection drag was pressed and on what: released without travel it is a
    /// click, which selects that colour.
    move_press: Option<([f32; 2], Target)>,
    marquee: Option<Marquee>,
    world_drag: Option<WorldDrag>,
    /// The selection version and `map.fills` the renderer's selection mask was built at.
    sel_shown: (u64, u64),
    cam_drag: Option<CamDrag>,
    /// Pointer position in pixels, if it is over the view.
    pointer: Option<[f32; 2]>,
    cursor: Option<Target>,
    /// Space held: a drag pans.
    pub space: bool,
    palette: [[u8; 4]; 256],
    /// The materials the GPU has, to notice changes.
    materials: [Material; 256],
    frames: Vec<f64>,
    /// Where the scene was opened from or last saved to.
    pub path: Option<std::path::PathBuf>,
    /// The history position at the last open, new scene or save (see `History::position`).
    saved_at: u64,
    /// A PNG render export in progress (the corner pauses meanwhile).
    pub export: Option<Export>,
    /// The last finished export's PNG and size, for the host to save or download.
    pub exported: Option<(Vec<u8>, (u32, u32))>,
    /// Export sizes this GPU can render.
    export_fits: [bool; 4],
    /// The scene's map replacements and origin the GPU map was built at (see `Scene::reshapes`).
    shape: (u64, [i32; 3]),
    /// The object last edited, where Tab goes back to from world mode.
    last_object: Option<usize>,
    /// The AI change under review: staged on the scene, drawn as a ghost, with its card.
    /// While it is open nothing else edits the scene.
    pub review: Option<Review>,
    /// The review version and `map.fills` the ghost was built at, and the card's version
    /// with the assistant's.
    ghost_shown: Option<(u64, u64)>,
    card_shown: Option<(u64, u64)>,
    /// "Ask for changes" requests from the review card, which go to the assistant.
    pub asks: Vec<String>,
    /// The AI assistant: its conversation and the request in progress, through the review step.
    pub assistant: Assistant,
    /// The assistant version the panel shows.
    chat_shown: Option<u64>,
    /// Terminal agents editing the window through `vxr mcp --live`, and what they did.
    pub agents: Agents,
    /// The agents version and history position the Agents tab's Agent edits list shows.
    agents_shown: Option<(u64, u64)>,
}

/// Whether a review has nothing in it: no step and no change.
fn review_empty(r: &Review, scene: &Scene) -> bool {
    r.session.steps().is_empty() && r.session.changes(scene) == Default::default()
}

/// What may run while an AI change is under review: the view, and the review itself.
fn review_allows(a: &Action) -> bool {
    matches!(
        a,
        Action::Frame
            | Action::FrameSelection
            | Action::ResetCamera
            | Action::Zoom(_)
            | Action::Snap(_)
            | Action::Ortho(_)
            | Action::CancelExport
            | Action::ApplyReview
            | Action::DiscardReview
            | Action::DropStep(..)
            | Action::ReviewBefore(_)
            | Action::AllowMore
            | Action::StopHeld
            | Action::AllowAgents(_)
            | Action::ReviewAgents(_)
            | Action::PairBridge
            | Action::DismissAgentsError
            | Action::RegenerateToken
            | Action::StopAgent(_)
            | Action::StopAgents
    )
}

const REVIEW_AGENT: &str = "The user is reviewing an AI change in vxr: call again once they apply or discard it.";

const REVIEW_FIRST: &str = "Apply or discard the AI change first";

const AGENTS_FIRST: &str = "Apply or discard the agents' change first";

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
            native_ppp: ppp,
            touches: Touches::new(TOUCH_SLOP * ppp),
            ui_touch: None,
            touch_places: false,
            ai_dock: 0.0,
            gesture: None,
            moving: None,
            move_press: None,
            marquee: None,
            world_drag: None,
            sel_shown: (u64::MAX, 0),
            cam_drag: None,
            pointer: None,
            cursor: None,
            space: false,
            palette,
            materials,
            frames: Vec::new(),
            path: None,
            saved_at: 0,
            export: None,
            exported: None,
            export_fits: [false; 4],
            shape: (0, [0; 3]),
            last_object: None,
            review: None,
            ghost_shown: None,
            card_shown: None,
            asks: Vec::new(),
            assistant: Assistant::default(),
            chat_shown: None,
            agents: Agents::default(),
            agents_shown: None,
        };
        e.shape = (e.scene.reshapes(), e.scene.origin);
        e.mark_saved();
        e.export_fits = EXPORT_SIZES.map(|(w, h)| Preview::fits(&e.gpu.device, w, h));
        e.frame_scene();
        Ok(e)
    }

    /// Replace the scene (after Open), reframing the camera.
    pub fn load(&mut self, scene: Scene, name: &str) -> Result<(), String> {
        // A change under review belongs to the scene it was staged on.
        self.assistant.stop(now_ms() / 1000.0);
        self.assistant.stop_ask = false;
        if let Some(r) = self.review.take().filter(|r| r.agents.is_some()) {
            // The agents hear that their edits that waited are gone.
            self.agents.discard_review(&mut self.scene, r);
        }
        // Agent edits name steps of the scene they were made on.
        self.agents.clear();
        let mut scene = scene;
        scene.map.track_dirty(true);
        self.map = GpuMap::new(&self.gpu.device, &self.gpu.queue, &self.layout, &scene.map, &scene.palette)?;
        self.palette = scene.palette;
        self.preview.set_palette(&self.gpu.queue, &self.palette);
        self.materials = scene.materials;
        self.preview.set_materials(&self.gpu.queue, &self.materials);
        self.preview.restart();
        self.export = None;
        self.shape = (scene.reshapes(), scene.origin);
        self.scene = scene;
        self.last_object = None;
        self.ui.scene_name = name.into();
        self.gesture = None;
        self.moving = None;
        self.marquee = None;
        self.world_drag = None;
        self.sel_shown = (u64::MAX, 0);
        self.cursor = None;
        self.ui.confirm_new = false;
        self.mark_saved();
        self.frame_scene();
        Ok(())
    }

    /// The scene is saved as it stands now: New asks again only after another edit or undo.
    pub fn mark_saved(&mut self) {
        self.saved_at = self.scene.history().position();
    }

    /// Start a blank scene exactly as launching with no file does: the default size, palette
    /// and camera, no history, selection or path.
    pub fn new_scene(&mut self) {
        // Cannot fail for an empty map of the default size.
        let _ = self.load(Scene::new(NEW_SIZE), "Untitled");
        self.cam = OrbitCamera::framing(Vec3::ZERO, Vec3::ONE);
        self.frame_scene();
        self.path = None;
        self.ui.bounds_panel = None;
        self.ui.toast(&self.ctx, "New scene");
    }

    /// The target's new size in pixels, with the display's pixels per point.
    pub fn resize(&mut self, width: u32, height: u32, ppp: f32) {
        let (width, height) = (width.max(1), height.max(1));
        self.renderer.resize(&self.gpu.device, width, height);
        (self.width, self.height, self.native_ppp) = (width, height, ppp);
        self.ppp = ppp * if self.ui.touch { TOUCH_SCALE } else { 1.0 };
        self.touches.set_slop(TOUCH_SLOP * self.ppp);
    }

    /// Switch to the touch layout (a bottom dock, larger targets and touch gestures) or back
    /// to the mouse layout. The host calls it when the screen is touch-first or a finger
    /// lands, and the mouse layout comes back with a mouse press.
    pub fn set_touch(&mut self, on: bool) {
        if self.ui.touch != on {
            self.ui.touch = on;
            self.ctx.set_zoom_factor(if on { TOUCH_SCALE } else { 1.0 });
            self.resize(self.width, self.height, self.native_ppp);
        }
    }

    /// Finger `id` touched, moved or lifted at `p` (pixels). True when the view takes it as a
    /// gesture; false when it is the UI's, and the host passes it to egui as the pointer.
    pub fn touch(&mut self, id: u64, phase: Finger, p: [f32; 2]) -> bool {
        self.touch_at(id, phase, p, now_ms())
    }

    /// `touch` at time `t` (ms).
    pub fn touch_at(&mut self, id: u64, phase: Finger, p: [f32; 2], t: f64) -> bool {
        self.last_input = t;
        if self.ui_touch == Some(id) {
            if phase == Finger::Up {
                self.ui_touch = None;
            }
            return false;
        }
        if phase == Finger::Down && self.ui_touch.is_none() && self.touches.count() == 0 {
            let at = egui::pos2(p[0], p[1]) / self.ctx.pixels_per_point();
            if self.ctx.layer_id_at(at).is_some_and(|l| l.order != egui::Order::Background) {
                self.ui_touch = Some(id);
                return false;
            }
        }
        let gestures = match phase {
            Finger::Down => self.touches.start(id, p, t),
            Finger::Move => self.touches.moved(id, p, t),
            Finger::Up => self.touches.end(id, p, t),
        };
        for g in gestures {
            self.gesture_input(g);
        }
        true
    }

    fn gesture_input(&mut self, g: crate::touch::Gesture) {
        use crate::touch::Gesture as G;
        match g {
            G::Press(p) => {
                self.pointer_moved(Some(p));
                // A lifting box's height follows the finger, and its release places it.
                self.touch_places = self.lifting();
                if !self.touch_places {
                    self.button(Button::Left, true, false);
                }
            }
            // A finger that has not moved does not move the drag: a mouse sends no move then
            // either, and the drag's plane (through the middle of the first cell) can round a
            // repeated move into the next cell.
            G::Drag(p) => {
                if self.pointer != Some(p) {
                    self.pointer_moved(Some(p));
                }
            }
            G::Release(p) => {
                if self.pointer != Some(p) {
                    self.pointer_moved(Some(p));
                }
                if std::mem::take(&mut self.touch_places) {
                    self.place_box();
                } else {
                    self.button(Button::Left, false, false);
                }
                // No hover on a touch screen: the cursor goes with the finger, unless a box's
                // height still follows it.
                if !self.lifting() {
                    self.pointer_moved(None);
                }
            }
            G::Orbit(d) => self.cam.orbit(d[0], d[1]),
            G::Pan(d) => {
                let from = self.cam;
                self.cam.pan(d[0], d[1], self.height as f32);
                self.bound_camera(&from);
            }
            G::Pinch { scale, at } => {
                let (o, d) = self.ray(at);
                let hit = self.scene.map.raycast(o, d, 1e5).map(|h| Vec3::from(o) + Vec3::from(d) * h.t);
                self.zoom(scale.max(0.01).ln() / -0.88f32.ln(), hit);
            }
            G::Undo => self.undo(),
        }
    }

    /// The box the camera frames: the voxels, or the floor of an empty scene.
    fn scene_box(&self) -> (Vec3, Vec3) {
        let (lo, hi) = match self.scene.map.occupied_bounds() {
            Some(b) => b,
            None => {
                let s = self.scene.map.size_voxels();
                let f = self.scene.floor.max(0) as u32;
                ([0, f, 0], [s[0], f + 1, s[2]])
            }
        };
        let v = |a: [u32; 3]| Vec3::new(a[0] as f32, a[1] as f32, a[2] as f32);
        (v(lo), v(hi))
    }

    /// Keep the camera near the model after a pan or zoom from `from`: inside its bounds (the
    /// box the bounds frame draws) and whatever `scene_box` frames, so a small model in large
    /// bounds still leaves all of them to move over.
    fn bound_camera(&mut self, from: &OrbitCamera) {
        let (lo, hi) = self.scene_box();
        let s = self.scene.map.size_voxels();
        let size = Vec3::new(s[0] as f32, s[1] as f32, s[2] as f32);
        self.cam.bound(lo.min(Vec3::ZERO), hi.max(size), from);
    }

    /// The view's width in pixels left of the docked assistant.
    fn free_width(&self) -> f32 {
        (self.width as f32 - self.ai_dock * self.ppp).max(1.0)
    }

    /// Move the camera sideways so the view's middle sits `px` pixels further left: the
    /// scene's centre stays in the middle of the space the docked assistant leaves.
    fn shift_left(&mut self, px: f32) {
        self.cam.pan(-px, 0.0, self.height as f32);
    }

    /// Frame a box from the current angle and projection, in the space left of the docked
    /// assistant.
    fn frame_box(&mut self, lo: Vec3, hi: Vec3) {
        let aspect = self.free_width() / self.height as f32;
        self.cam.target = (lo + hi) * 0.5;
        self.cam.fit(lo, hi, aspect, 0.9);
        self.shift_left(self.ai_dock * self.ppp / 2.0);
    }

    /// Frame the model (or the floor of an empty scene), like MagicaVoxel's recenter. Keeps
    /// the angle and the projection.
    pub fn frame_scene(&mut self) {
        let (lo, hi) = self.scene_box();
        self.frame_box(lo, hi);
    }

    /// Frame the selected voxels, or the scene when nothing is selected.
    pub fn frame_selection(&mut self) {
        match self.scene.selection_bounds() {
            Some((lo, hi)) => {
                let v = |a: [i32; 3]| Vec3::new(a[0] as f32, a[1] as f32, a[2] as f32);
                self.frame_box(v(lo), v(hi) + Vec3::ONE);
            }
            None => self.frame_scene(),
        }
    }

    /// Orbit around the voxel under `p` (pixels) from now on: the camera turns to look at it
    /// from where it is, so the view centres on it. Nothing happens off the voxels.
    pub fn orbit_around(&mut self, p: [f32; 2]) {
        let (o, d) = self.ray(p);
        let Some(h) = self.scene.map.raycast(o, d, 1e5) else { return };
        let point = Vec3::from(o) + Vec3::from(d) * h.t;
        let eye = self.cam.eye();
        let back = eye - point;
        self.cam.target = point;
        self.cam.distance = back.length().max(1.0);
        self.cam.yaw = back.x.atan2(back.z);
        self.cam.pitch = (back.y / self.cam.distance).clamp(-1.0, 1.0).asin();
    }

    /// How much of the scene's box the view shows: the share of its projected rectangle on
    /// screen, and its height as a share of the view's (for the checks).
    pub fn scene_in_view(&self) -> (f32, f32) {
        let (lo, hi) = self.scene_box();
        self.box_in_view(lo, hi)
    }

    /// How much of the box `lo`..`hi` the view shows, as `scene_in_view`.
    pub fn box_in_view(&self, lo: Vec3, hi: Vec3) -> (f32, f32) {
        let vp = self.cam.view_proj(self.width as f32 / self.height as f32);
        let (mut x0, mut x1, mut y0, mut y1) = (f32::MAX, f32::MIN, f32::MAX, f32::MIN);
        for i in 0..8 {
            let c = Vec3::new(
                if i & 1 == 0 { lo.x } else { hi.x },
                if i & 2 == 0 { lo.y } else { hi.y },
                if i & 4 == 0 { lo.z } else { hi.z },
            );
            let p = vp * c.extend(1.0);
            if p.w <= 0.0 {
                return (0.0, 0.0);
            }
            let (x, y) = (p.x / p.w, p.y / p.w);
            (x0, x1, y0, y1) = (x0.min(x), x1.max(x), y0.min(y), y1.max(y));
        }
        let area = (x1 - x0) * (y1 - y0);
        let shown = (x1.min(1.0) - x0.max(-1.0)).max(0.0) * (y1.min(1.0) - y0.max(-1.0)).max(0.0);
        (if area > 0.0 { shown / area } else { 0.0 }, (y1.min(1.0) - y0.max(-1.0)).max(0.0) / 2.0)
    }

    /// Back to the starting view: the home angle in perspective, framing the scene.
    pub fn reset_camera(&mut self) {
        let (lo, hi) = self.scene_box();
        self.cam = OrbitCamera::framing_aspect(lo, hi, self.free_width() / self.height as f32);
        self.shift_left(self.ai_dock * self.ppp / 2.0);
    }

    /// Zoom a step about the middle of the view (the on-screen buttons, Cmd-= and Cmd--).
    pub fn zoom_step(&mut self, zoom_in: bool) {
        self.zoom(if zoom_in { 2.0 } else { -2.0 }, None);
    }

    /// Zoom by `notches` toward `toward`, kept near the model.
    fn zoom(&mut self, notches: f32, toward: Option<Vec3>) {
        let from = self.cam;
        self.cam.zoom(notches, toward);
        self.bound_camera(&from);
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
            size: self.ui.size,
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
                CamDrag::Pan => {
                    let from = self.cam;
                    self.cam.pan(dx, dy, self.height as f32);
                    self.bound_camera(&from);
                }
            }
            return;
        }
        let (o, d) = self.ray(p);
        if let Some(m) = &mut self.marquee {
            m.end = p;
            return;
        }
        if let Some(g) = self.world_drag {
            self.drag_object(g, o, d);
            return;
        }
        if let Some(g) = &mut self.moving {
            if let Some(c) = plane_cell(o, d, g.axis, g.layer) {
                g.move_to(&mut self.scene, c);
            }
            return;
        }
        if let Some(g) = &mut self.gesture {
            if g.lifting {
                if let Some(h) = g.lift_cell(o, d) {
                    g.lift_to(&mut self.scene, h);
                }
                return;
            }
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
            if b == Button::Left {
                if let Some(mut g) = self.gesture.take() {
                    // A box's base is drawn: its height follows the pointer until a click.
                    g.lift();
                    if g.lifting {
                        self.gesture = Some(g);
                    } else {
                        g.release(&mut self.scene);
                    }
                }
                if let Some(g) = self.moving.take() {
                    let still = g.offset == [0; 3];
                    g.release(&mut self.scene);
                    if let (true, Some((at, t)), Some(p)) = (still, self.move_press.take(), self.pointer)
                        && (p[0] - at[0]).abs() < CLICK_PX
                        && (p[1] - at[1]).abs() < CLICK_PX
                    {
                        self.scene.select_region(&t, self.ui.region, Mode::Replace);
                    }
                }
                if let Some(m) = self.marquee.take() {
                    self.finish_marquee(m);
                }
                if self.world_drag.take().is_some() {
                    self.scene.commit();
                }
            }
            self.cam_drag = None;
            return;
        }
        if b == Button::Left && self.lifting() {
            self.place_box();
            return;
        }
        if over_ui
            || self.gesture.as_ref().is_some_and(|g| !g.lifting)
            || self.moving.is_some()
            || self.marquee.is_some()
            || self.world_drag.is_some()
            || self.cam_drag.is_some()
        {
            return;
        }
        // As MagicaVoxel: X-click orbits around the voxel clicked from then on.
        if b == Button::Left && self.ctx.input(|i| i.key_down(egui::Key::X)) {
            if let Some(p) = self.pointer {
                self.orbit_around(p);
            }
            return;
        }
        let (shift, _) = self.ui.modifiers();
        self.cam_drag = match b {
            // As MagicaVoxel: right-drag orbits, middle-drag and Space with either button pan.
            Button::Right if shift || self.space => Some(CamDrag::Pan),
            Button::Right => Some(CamDrag::Orbit),
            Button::Middle => Some(CamDrag::Pan),
            Button::Left if self.space => Some(CamDrag::Pan),
            Button::Left => None,
        };
        if b != Button::Left || self.space {
            return;
        }
        if self.review.is_some() {
            return self.ui.toast(&self.ctx, REVIEW_FIRST);
        }
        self.ui.coach = false;
        // A colour edit still open (a field not yet applied) is its own undo step.
        self.scene.commit();
        let Some(p) = self.pointer else { return };
        let (o, d) = self.ray(p);
        if self.scene.at() == At::World {
            self.press_world(self.scene.target(o, d));
            return;
        }
        if self.ui.tool == Tool::Select {
            self.press_select(p, self.scene.target(o, d));
            return;
        }
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

    /// Whether a box brush's height follows the pointer, waiting for a click or Enter.
    pub fn lifting(&self) -> bool {
        self.gesture.as_ref().is_some_and(|g| g.lifting)
    }

    /// Place the box whose height is being set, as one undo step with its base.
    pub fn place_box(&mut self) {
        if let Some(g) = self.gesture.take_if(|g| g.lifting) {
            g.release(&mut self.scene);
        }
    }

    /// Drop the box whose height is being set, base and all (Escape).
    pub fn cancel_box(&mut self) {
        if self.gesture.take_if(|g| g.lifting).is_some() {
            self.scene.revert_voxels();
            self.scene.commit();
        }
    }

    /// A press in world mode, with any tool: pick the object under it and start dragging it
    /// on the plane of the face pressed, or pick nothing. A press on an object of the picked
    /// group keeps the group picked and drags the whole group.
    fn press_world(&mut self, t: Option<Target>) {
        let t = t.filter(|t| !t.ground);
        let object = t.and_then(|t| self.scene.object_at(t.voxel)).map(|o| {
            let group = self.scene.picked().filter(|&p| self.scene.world().unwrap().model_of(p).is_none());
            group.filter(|&g| self.scene.world().unwrap().objects_under(g).contains(&o)).unwrap_or(o)
        });
        self.scene.pick(object);
        if let (Some(t), Some(object)) = (t, object) {
            let axis = t.normal.iter().position(|&n| n != 0).unwrap_or(1);
            self.world_drag = Some(WorldDrag { object, axis, start: self.scene.to_world(t.voxel), offset: [0; 3] });
        }
    }

    /// Move the object being dragged to where the ray crosses its drag plane, as part of one
    /// undo step, and keep the camera on the same world point as the world map moves.
    fn drag_object(&mut self, g: WorldDrag, o: [f32; 3], d: [f32; 3]) {
        let layer = g.start[g.axis] - self.scene.origin[g.axis];
        let Some(c) = plane_cell(o, d, g.axis, layer) else { return };
        let q = self.scene.to_world(c);
        let mut offset: [i32; 3] = std::array::from_fn(|a| q[a] - g.start[a]);
        offset[g.axis] = 0;
        let step: [i32; 3] = std::array::from_fn(|a| offset[a] - g.offset[a]);
        if step == [0; 3] || self.scene.put_world(|w| w.move_by(g.object, step)).is_none() {
            return;
        }
        self.world_drag = Some(WorldDrag { offset, ..g });
        if self.scene.reshapes() != self.shape.0 {
            self.reshaped();
        }
    }

    /// A selection action in world mode, where it acts on the picked object: arrows and
    /// Move nudge it, Delete removes it, Deselect and Escape pick nothing. Voxel actions
    /// say to Tab into an object first.
    fn world_action(&mut self, a: Action) {
        let picked = self.scene.picked();
        let s = &mut self.scene;
        match (a, picked) {
            (Action::Deselect, _) => {
                s.pick(None);
            }
            (Action::Transform(Transform::Move(d)), Some(i)) => {
                if s.edit_world(|w| w.move_by(i, d)).is_none() {
                    self.ui.toast(&self.ctx, "No room in the scene for that");
                }
            }
            (Action::DeleteSelection, Some(i)) => {
                let name = node_name(s.world().unwrap(), i);
                if s.edit_world(|w| w.remove(i)) == Some(true) {
                    self.ui.toast(&self.ctx, format!("Deleted {name}"));
                }
            }
            (Action::Duplicate { linked }, Some(i)) => match s.edit_world(|w| w.duplicate(i, linked)).flatten() {
                Some(c) => {
                    s.pick(Some(c));
                    let name = node_name(s.world().unwrap(), i);
                    let what = if linked { "Linked copy of" } else { "Duplicated" };
                    self.ui.toast(&self.ctx, format!("{what} {name}"));
                }
                None => self.ui.toast(&self.ctx, "No room in the scene for that"),
            },
            (Action::Group, Some(i)) => {
                if let Some(g) = s.edit_world(|w| w.group(&[i])).flatten() {
                    s.pick(Some(g));
                    let msg =
                        format!("Grouped {} as {}", node_name(s.world().unwrap(), i), node_name(s.world().unwrap(), g));
                    self.ui.toast(&self.ctx, msg);
                }
            }
            (Action::Ungroup, Some(g)) => {
                let w = s.world().unwrap();
                let (name, first) = (node_name(w, g), w.tree.children(g).first().copied());
                if w.model_of(g).is_some() {
                    self.ui.toast(&self.ctx, "Pick a group to ungroup it");
                } else if s.edit_world(|w| w.ungroup(g)) == Some(true) {
                    s.pick(first);
                    self.ui.toast(&self.ctx, format!("Ungrouped {name}"));
                }
            }
            (
                Action::Duplicate { .. }
                | Action::Transform(Transform::Move(_))
                | Action::DeleteSelection
                | Action::Group
                | Action::Ungroup,
                None,
            ) => self.ui.toast(&self.ctx, "Click an object to pick it first"),
            _ => self.ui.toast(&self.ctx, "Tab into an object to edit its voxels"),
        }
    }

    /// How Shift and Alt combine a select press with the selection.
    fn select_mode(&self) -> Mode {
        match self.ui.modifiers() {
            (true, _) => Mode::Add,
            (_, true) => Mode::Subtract,
            _ => Mode::Replace,
        }
    }

    /// A select tool press: on a selected voxel it starts moving the selection on the plane
    /// of the face pressed; anywhere else it starts a box (or, without a drag, selects the
    /// colour, face or volume clicked).
    fn press_select(&mut self, p: [f32; 2], t: Option<Target>) {
        let mode = self.select_mode();
        if mode == Mode::Replace
            && let Some(t) = t.filter(|t| !t.ground && self.scene.is_selected(t.voxel))
        {
            let axis = t.normal.iter().position(|&n| n != 0).unwrap_or(1);
            self.moving = self.scene.start_move(axis, t.voxel);
            self.move_press = Some((p, t));
            return;
        }
        self.marquee = Some(Marquee { start: p, end: p, target: t, mode });
    }

    fn finish_marquee(&mut self, m: Marquee) {
        let (dx, dy) = (m.end[0] - m.start[0], m.end[1] - m.start[1]);
        if dx.abs() < CLICK_PX && dy.abs() < CLICK_PX {
            match m.target.filter(|t| !t.ground) {
                Some(t) => self.scene.select_region(&t, self.ui.region, m.mode),
                None if m.mode == Mode::Replace => self.scene.deselect(),
                None => {}
            }
            return;
        }
        let cells = self.cells_in_rect(m.start, m.end);
        self.scene.select(cells, m.mode);
    }

    /// Every voxel whose centre projects inside the screen box from `a` to `b` (pixels),
    /// hidden ones included, as MagicaVoxel's box select does.
    pub fn cells_in_rect(&self, a: [f32; 2], b: [f32; 2]) -> Vec<[i32; 3]> {
        let (w, h) = (self.width as f32, self.height as f32);
        let vp = self.cam.view_proj(w / h);
        let (x0, x1) = (a[0].min(b[0]), a[0].max(b[0]));
        let (y0, y1) = (a[1].min(b[1]), a[1].max(b[1]));
        let mut out = Vec::new();
        self.scene.for_each_voxel(|p, _| {
            let c = vp * glam::Vec4::new(p[0] as f32 + 0.5, p[1] as f32 + 0.5, p[2] as f32 + 0.5, 1.0);
            if c.w <= 0.0 {
                return;
            }
            let (sx, sy) = ((c.x / c.w + 1.0) * 0.5 * w, (1.0 - c.y / c.w) * 0.5 * h);
            if sx >= x0 && sx <= x1 && sy >= y0 && sy <= y1 {
                out.push(p);
            }
        });
        out
    }

    /// Where an arrow key moves the selection: one cell along the ground axis nearest to the
    /// view's right (left and right arrows) or forward (up and down) direction.
    fn arrow_offset(&self, key: egui::Key) -> [i32; 3] {
        // From the yaw, so a view straight down still has a forward direction.
        let (sy, cy) = self.cam.yaw.sin_cos();
        let fwd = Vec3::new(-sy, 0.0, -cy);
        let right = fwd.cross(Vec3::Y);
        let snap = |v: Vec3| -> [i32; 3] {
            if v.x.abs() >= v.z.abs() { [v.x.signum() as i32, 0, 0] } else { [0, 0, v.z.signum() as i32] }
        };
        let neg = |d: [i32; 3]| d.map(|c| -c);
        match key {
            egui::Key::ArrowRight => snap(right),
            egui::Key::ArrowLeft => neg(snap(right)),
            egui::Key::ArrowUp => snap(fwd),
            _ => neg(snap(fwd)),
        }
    }

    /// Run a selection action from the UI.
    fn selection_action(&mut self, a: Action) {
        self.gesture = None;
        self.moving = None;
        if self.scene.at() == At::World {
            self.world_drag = None;
            self.scene.commit();
            return self.world_action(a);
        }
        let s = &mut self.scene;
        match a {
            Action::Transform(t) => {
                if s.transform(t).is_none() {
                    self.ui.toast(&self.ctx, "No room in the scene for that");
                }
            }
            Action::SelectAll => s.select_all(),
            Action::InvertSelection => s.invert_selection(),
            Action::SelectColor => {
                s.select_color(self.ui.slot, Mode::Replace);
                self.ui.tool = Tool::Select;
            }
            Action::Deselect => s.deselect(),
            Action::DeleteSelection => {
                s.delete_selection();
            }
            Action::FillSelection => {
                s.fill_selection(self.ui.slot);
            }
            Action::Hollow => {
                let n = s.hollow();
                let msg = if n > 0 { format!("Hollowed: removed {n} voxels") } else { "Nothing to hollow".into() };
                self.ui.toast(&self.ctx, msg);
            }
            Action::FillInterior => {
                let n = s.fill_interior(self.ui.slot);
                let msg = if n > 0 { format!("Filled {n} enclosed cells") } else { "No enclosed space to fill".into() };
                self.ui.toast(&self.ctx, msg);
            }
            Action::Resize(size) => match s.resize(size) {
                Some(n) => {
                    let msg = format!("Bounds {}×{}×{}", size[0], size[1], size[2]);
                    let msg = if n > 0 { format!("{msg}: erased {n} voxels outside") } else { msg };
                    self.ui.toast(&self.ctx, msg);
                }
                None => self
                    .ui
                    .toast(&self.ctx, format!("Bounds go from 1 to {} on each axis", vxr_core::scene::MAX_EXTENT)),
            },
            Action::Crop => {
                let msg = if s.crop() {
                    let b = s.bounds().size;
                    format!("Cropped to {}×{}×{}", b[0], b[1], b[2])
                } else if s.map.voxels == 0 {
                    "Nothing to crop to".into()
                } else {
                    "Already cropped".into()
                };
                self.ui.toast(&self.ctx, msg);
            }
            Action::Copy | Action::Cut => {
                let n = if a == Action::Copy { s.copy_selection() } else { s.cut_selection() };
                if n > 0 {
                    // A text stand-in on the system clipboard, so Cmd-V reaches the app natively.
                    self.ctx.copy_text(format!("vxr: {n} voxels"));
                    let verb = if a == Action::Copy { "Copied" } else { "Cut" };
                    self.ui.toast(&self.ctx, format!("{verb} {n} voxels"));
                }
            }
            Action::Paste => {
                let at = self.cursor.map(|t| t.front());
                if let Some(c) = s.paste_corner(at) {
                    let n = s.paste(c);
                    self.ui.tool = Tool::Select;
                    self.ui.toast(&self.ctx, format!("Pasted {n} voxels"));
                }
            }
            Action::Group | Action::Ungroup => self.ui.toast(&self.ctx, "Tab to the world to group objects"),
            _ => {}
        }
    }

    /// Wheel zoom toward the point under the pointer, in notches (positive zooms in).
    pub fn scroll(&mut self, notches: f32) {
        self.last_input = now_ms();
        let hit = self.pointer.and_then(|p| {
            let (o, d) = self.ray(p);
            self.scene.map.raycast(o, d, 1e5).map(|h| Vec3::from(o) + Vec3::from(d) * h.t)
        });
        self.zoom(notches, hit);
    }

    /// A trackpad pinch: `delta` is the change in magnification (positive zooms in), toward
    /// the point under the pointer as the wheel does.
    pub fn pinch(&mut self, delta: f32) {
        self.scroll((1.0 + delta).max(0.01).ln() / -0.88f32.ln());
    }

    pub fn undo(&mut self) {
        if self.review.is_some() {
            return self.ui.toast(&self.ctx, REVIEW_FIRST);
        }
        self.gesture = None;
        self.moving = None;
        self.world_drag = None;
        let label = self.scene.history().undo_label().map(str::to_string);
        if self.scene.undo()
            && let Some(l) = label
        {
            self.ui.toast(&self.ctx, format!("Undid {l}"));
        }
    }

    pub fn redo(&mut self) {
        if self.review.is_some() {
            return self.ui.toast(&self.ctx, REVIEW_FIRST);
        }
        self.gesture = None;
        self.moving = None;
        self.world_drag = None;
        let label = self.scene.history().redo_label().map(str::to_string);
        if self.scene.redo()
            && let Some(l) = label
        {
            self.ui.toast(&self.ctx, format!("Redid {l}"));
        }
    }

    /// Run a request's tool calls through the review step, as the assistant will: the first
    /// opens a review (an open review takes more calls, as "Ask for changes" will), each turn
    /// is one model answer. Returns each call's answer, by turn.
    pub fn stage(&mut self, script: Script) -> Vec<Vec<Answer>> {
        if self.review.as_ref().is_some_and(|r| r.agents.is_some()) {
            self.ui.toast(&self.ctx, AGENTS_FIRST);
            return Vec::new();
        }
        if self.review.is_none() {
            self.open_review(Some(&script.prompt));
        }
        let r = self.review.as_mut().unwrap();
        let mut out = Vec::new();
        for calls in script.turns {
            out.push(r.turn(&mut self.scene, calls));
        }
        out
    }

    /// Open a review for a request, or for terminal agents' edits (None): whatever the user
    /// had going ends first, as its own step.
    fn open_review(&mut self, prompt: Option<&str>) {
        self.place_box();
        self.gesture = None;
        self.moving = None;
        self.marquee = None;
        self.world_drag = None;
        self.ui.color_edit = None;
        self.ui.slot_op = None;
        self.ui.bounds_panel = None;
        self.ui.coach = false;
        self.scene.commit();
        self.review = Some(match prompt {
            Some(p) => Review::new(&self.scene, p),
            None => Review::for_agents(&self.scene),
        });
    }

    /// Send a request to the assistant: on the change under review if one is open (the
    /// card's "Ask for changes" continues it), else on a new one. Without a key the panel
    /// opens on its key settings. Returns whether the request started; when it did not, a
    /// toast says why.
    pub fn ask(&mut self, text: &str) -> bool {
        let text = text.trim();
        if text.is_empty() {
            return false;
        }
        if self.assistant.running() {
            self.ui.toast(&self.ctx, "Wait for the answer, or stop it");
            return false;
        }
        if self.review.as_ref().is_some_and(|r| r.agents.is_some()) {
            self.ui.toast(&self.ctx, AGENTS_FIRST);
            return false;
        }
        if !self.assistant.ready() {
            self.ui.open_ai_setup(None);
            self.ui.toast(&self.ctx, crate::assistant::NO_KEY);
            return false;
        }
        if self.scene.at() == At::World {
            self.ui.toast(&self.ctx, "Open an object's model (Tab) to change it with AI");
            return false;
        }
        let opened = self.review.is_none();
        if opened {
            self.open_review(Some(text));
        } else if let Some(r) = &mut self.review {
            r.session.begin_request();
            r.show_before(&mut self.scene, false);
        }
        let r = self.review.as_mut().unwrap();
        if let Err(e) = self.assistant.send(text, r, &self.scene, now_ms() / 1000.0) {
            if opened && let Some(r) = self.review.take() {
                r.session.discard(&mut self.scene);
            }
            self.ui.toast(&self.ctx, e);
            return false;
        }
        true
    }

    /// What the panel and the prompt bar asked for.
    fn ai_action(&mut self, a: AiAction) {
        let now = now_ms() / 1000.0;
        match a {
            AiAction::Send(text) => {
                if !self.ask(&text) {
                    self.ui.ai_refused(&text);
                }
            }
            AiAction::Stop => self.stop_request(now),
            AiAction::Keep => self.assistant.stop_ask = false,
            AiAction::Drop => {
                self.assistant.stop_ask = false;
                self.discard_review();
            }
            AiAction::SaveKey { kind, key, remember } => match self.assistant.set_key(kind, &key, remember) {
                Ok(()) => self.ui.toast(&self.ctx, format!("Kept your {} key", kind.name())),
                Err(e) => self.ui.toast(&self.ctx, format!("Could not keep the key: {e}")),
            },
            AiAction::RemoveKey(kind) => match self.assistant.remove_key(kind) {
                Ok(()) => self.ui.toast(&self.ctx, format!("Removed the {} key", kind.name())),
                Err(e) => self.ui.toast(&self.ctx, format!("Could not remove the key: {e}")),
            },
            AiAction::Settings(s) => self.assistant.set_settings(s),
            AiAction::NewChat => {
                self.assistant.stop_ask = false;
                self.stop_request(now);
                self.assistant.clear();
            }
        }
        self.assistant.version += 1;
    }

    /// Stop the request: held calls are dropped from the review, an empty review closes, and
    /// a staged change asks Keep or Drop.
    fn stop_request(&mut self, now: f64) {
        let staged = self.review.as_ref().is_some_and(|r| !review_empty(r, &self.scene));
        if self.assistant.stop(now) {
            if let Some(r) = &mut self.review {
                r.stop();
            }
            if staged {
                self.assistant.stop_ask = true;
            } else {
                self.close_empty_review();
            }
        }
    }

    /// A review the assistant opened and left without a step or a change closes by itself.
    fn close_empty_review(&mut self) {
        if self.review.as_ref().is_some_and(|r| review_empty(r, &self.scene))
            && let Some(r) = self.review.take()
        {
            r.session.discard(&mut self.scene);
        }
    }

    /// What the panel shows of the assistant.
    fn chat_view(&mut self) -> ChatView {
        let a = &mut self.assistant;
        let kinds = vxr_ai::Kind::ALL;
        ChatView {
            entries: a.entries.clone(),
            running: a.running(),
            held: a.held(),
            stop_ask: a.stop_ask,
            settings: a.settings.clone(),
            has_key: kinds.map(|k| a.has_key(k)),
            place: a.key_place(),
            remembered: kinds.map(|k| a.keys.remembered(k)),
            spent: a.spent,
        }
    }

    /// [`Editor::stage`] from a script's JSON (see [`Script`]).
    pub fn stage_json(&mut self, json: &str) -> Result<Vec<Vec<Answer>>, String> {
        Ok(self.stage(Script::parse(json)?))
    }

    /// Apply the change under review as one undo step named after the request (a request
    /// still running stops first).
    pub fn apply_review(&mut self) {
        self.assistant.stop(now_ms() / 1000.0);
        self.assistant.stop_ask = false;
        let Some(r) = self.review.take() else { return };
        if r.agents.is_some() {
            let n = self.agents.apply_review(&mut self.scene, r);
            let msg = match n {
                0 => "Nothing to apply".to_string(),
                1 => "Applied 1 agent edit".to_string(),
                n => format!("Applied {n} agent edits"),
            };
            return self.ui.toast(&self.ctx, msg);
        }
        let label = r.label();
        let n = r.session.apply_named(&mut self.scene, &label);
        let msg = if n > 0 || self.scene.history().undo_label() == Some(label.as_str()) {
            format!("Applied {label}")
        } else {
            "Nothing to apply".to_string()
        };
        self.ui.toast(&self.ctx, msg);
    }

    /// Drop the change under review: the scene is as it was, with no undo step.
    pub fn discard_review(&mut self) {
        self.assistant.stop(now_ms() / 1000.0);
        self.assistant.stop_ask = false;
        let Some(r) = self.review.take() else { return };
        if r.agents.is_some() {
            self.agents.discard_review(&mut self.scene, r);
            return self.ui.toast(&self.ctx, "Discarded the agents' change");
        }
        r.session.discard(&mut self.scene);
        self.ui.toast(&self.ctx, "Discarded the AI change");
    }

    fn review_action(&mut self, a: Action) {
        let Some(r) = &mut self.review else { return };
        match a {
            Action::ApplyReview => self.apply_review(),
            Action::DiscardReview => self.discard_review(),
            Action::DropStep(i, dropped) => r.set_dropped(&mut self.scene, i, dropped),
            Action::ReviewBefore(on) => r.show_before(&mut self.scene, on),
            Action::AllowMore if self.assistant.held() => self.assistant.allow(r, &mut self.scene, now_ms() / 1000.0),
            Action::AllowMore => {
                r.allow(&mut self.scene);
            }
            Action::StopHeld => {
                self.assistant.stop_held(now_ms() / 1000.0);
                r.stop();
            }
            _ => {}
        }
    }

    /// What a shape drag in progress spans, for the stats panel.
    pub fn span(&self) -> Option<(&'static str, String)> {
        if let Some(g) = &self.world_drag {
            let o = g.offset;
            return Some(("MOVE", format!("{},{},{}", o[0], o[1], o[2])));
        }
        if let Some(m) = &self.moving {
            let o = m.offset;
            return Some(("MOVE", format!("{},{},{}", o[0], o[1], o[2])));
        }
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
            selected: self.ui.selected,
            render,
            export,
            export_fits: self.export_fits,
        }
    }

    /// Run the UI, apply what it asked for, and draw a frame onto `view`. Returns the actions
    /// the host handles (open, save, embed) and egui's platform output.
    pub fn frame(&mut self, input: egui::RawInput, view: &wgpu::TextureView) -> (Vec<Action>, egui::PlatformOutput) {
        let mut input = input;
        // Tab switches between the world and a model. egui would move keyboard focus with it
        // before the UI could consume it, so it never reaches egui unless a field is typing.
        let mut tabs = 0;
        if !self.ctx.egui_wants_keyboard_input() {
            input.events.retain(|e| match e {
                egui::Event::Key { key: egui::Key::Tab, pressed, modifiers, .. } if modifiers.is_none() => {
                    tabs += *pressed as usize;
                    false
                }
                _ => true,
            });
        }
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
        self.ui.unsaved = self.scene.history().position() != self.saved_at;
        self.ui.selected = self.scene.selected_count();
        self.ui.has_clip = self.scene.clipboard.is_some();
        self.ui.bounds = self.scene.bounds().size;
        self.ui.world = self.scene.at() == At::World;
        self.ui.lifting = self.lifting();
        self.ui.camera_hints = self.space || self.cam_drag.is_some();
        self.ui.picked = self.scene.picked().map(|i| node_name(self.scene.world().unwrap(), i));
        self.ui.picked_node = self.scene.picked();
        self.ui.tree = tree_rows(&self.scene);
        self.ui.layers = layer_rows(&self.scene);
        self.ui.layer_on = self.scene.picked().zip(self.scene.world()).map(|(i, w)| w.tree.nodes[i].layer);
        self.ui.tree_on = match self.scene.at() {
            At::Object(i) => Some(i),
            _ => self.scene.picked(),
        };
        (self.ui.ortho, self.ui.view) = (self.cam.ortho, self.cam.view());
        self.ui.marquee = self.marquee.map(|m| {
            let (a, b) = (m.start, m.end);
            egui::Rect::from_two_pos(egui::pos2(a[0], a[1]), egui::pos2(b[0], b[1])) / self.ppp
        });
        // The assistant's request moves on: its stream, its calls through the review.
        if self.assistant.tick(self.review.as_mut(), &mut self.scene, now / 1000.0) {
            self.close_empty_review();
        }
        if self.chat_shown != Some(self.assistant.version) {
            self.chat_shown = Some(self.assistant.version);
            self.ui.chat = self.chat_view();
        }
        let working = match self.assistant.entries.last() {
            Some(Entry::Answer(a)) if self.assistant.running() => match &a.progress {
                Progress::Working(w) => Some(w.clone()),
                _ => None,
            },
            _ => None,
        };
        match &self.review {
            Some(r) if self.card_shown != Some((r.version, self.assistant.version)) => {
                self.card_shown = Some((r.version, self.assistant.version));
                let mut card = r.card(&self.scene);
                card.working = working;
                self.ui.review = Some(card);
            }
            Some(_) => {}
            None => {
                self.card_shown = None;
                self.ui.review = None;
            }
        }
        let key = (self.agents.version, self.scene.history().position());
        if self.agents_shown != Some(key) {
            self.agents_shown = Some(key);
            self.ui.agents.edits = (0..self.agents.edits.len())
                .map(|i| EditRow {
                    conn: self.agents.edits[i].conn,
                    label: self.agents.edits[i].label.clone(),
                    applied: self.agents.in_effect(&self.scene, i),
                })
                .collect();
        }
        let v = &mut self.ui.agents;
        (v.available, v.allowed, v.review) = (self.agents.available, self.agents.allowed, self.agents.review);
        (v.web, v.connecting) = (self.agents.web, self.agents.connecting);
        if v.clients != self.agents.clients || v.error != self.agents.error {
            v.clients = self.agents.clients.clone();
            v.error = self.agents.error.clone();
        }
        (v.fail, v.since) = (self.agents.fail, self.agents.since);
        v.times = self
            .agents
            .clients
            .iter()
            .map(|(c, _)| {
                let (seen, last) = self.agents.times(*c);
                (seen.unwrap_or(0.0), last)
            })
            .collect();
        let stats = self.stats();
        let mut actions = Vec::new();
        let ui = &mut self.ui;
        let out = self.ctx.run_ui(input, |u| actions = ui.ui(u.ctx(), &stats));
        // The assistant docked or left: keep the scene centred in the space beside it.
        let dock = self.ui.ai_width();
        if dock != self.ai_dock {
            self.shift_left((dock - self.ai_dock) * self.ppp / 2.0);
            self.ai_dock = dock;
        }
        let mut host = Vec::new();
        actions.extend(std::iter::repeat_n(Action::ToggleWorld, tabs));
        if let Some(text) = self.ui.asked.take() {
            self.asks.push(text.clone());
            if !self.ask(&text) {
                self.ui.review_ask_refused(&text);
            }
        }
        for a in std::mem::take(&mut self.ui.ai_out) {
            self.ai_action(a);
        }
        if self.review.is_some() {
            // Nothing else edits the scene while a change is under review: the view still moves.
            let n = actions.len();
            actions.retain(review_allows);
            let blocked = n > actions.len() || !self.ui.arrows.is_empty() || self.ui.renamed.is_some();
            self.ui.arrows.clear();
            self.ui.renamed = None;
            if blocked {
                self.ui.toast(&self.ctx, REVIEW_FIRST);
            }
        }
        // Any other command places a box whose height is being set first.
        if actions.iter().any(|a| *a != Action::CancelBox) {
            self.place_box();
        }
        for a in actions {
            match a {
                Action::New => {
                    self.new_scene();
                    host.push(Action::New);
                }
                Action::Undo => self.undo(),
                Action::Redo => self.redo(),
                Action::Frame => self.frame_scene(),
                Action::FrameSelection => self.frame_selection(),
                Action::ResetCamera => self.reset_camera(),
                Action::Zoom(zoom_in) => self.zoom_step(zoom_in),
                Action::Snap(v) => self.cam.snap(v),
                Action::Ortho(on) => self.cam.ortho = on,
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
                Action::Transform(_)
                | Action::SelectAll
                | Action::InvertSelection
                | Action::SelectColor
                | Action::Deselect
                | Action::DeleteSelection
                | Action::FillSelection
                | Action::Hollow
                | Action::FillInterior
                | Action::Resize(_)
                | Action::Crop
                | Action::Copy
                | Action::Cut
                | Action::Paste
                | Action::Duplicate { .. }
                | Action::Group
                | Action::Ungroup => self.selection_action(a),
                Action::ToggleWorld => self.toggle_world(),
                Action::PickNode(i) => self.pick_node(i),
                Action::SetHidden(i, hidden) => self.set_hidden(i, hidden),
                Action::ToggleHidden => match (self.scene.picked(), self.scene.world()) {
                    (Some(i), Some(w)) => self.set_hidden(i, !w.tree.nodes[i].hidden),
                    _ => self.ui.toast(&self.ctx, "Pick an object in the world to hide it"),
                },
                Action::SetLayer(id) => self.put_on_layer(id),
                Action::SetLayerHidden(id, hidden) => self.set_layer_hidden(id, hidden),
                Action::ToggleLayer => match self.ui.layer_on.and_then(|id| self.ui.layers.iter().find(|l| l.id == id))
                {
                    Some(l) => self.set_layer_hidden(l.id, !l.hidden),
                    None => self.ui.toast(&self.ctx, "Pick an object in the world to hide its layer"),
                },
                Action::AddLayer => self.add_layer(),
                Action::PlaceBox => self.place_box(),
                Action::CancelBox => self.cancel_box(),
                Action::ApplyReview
                | Action::DiscardReview
                | Action::DropStep(..)
                | Action::ReviewBefore(_)
                | Action::AllowMore
                | Action::StopHeld => self.review_action(a),
                Action::UndoAgentEdit(i) => self.undo_agent_edit(i),
                Action::ReviewAgents(on) => self.review_agents(on),
                Action::ExportPng => self.start_export(),
                Action::CancelExport => {
                    self.export = None;
                    self.ui.toast(&self.ctx, "Export cancelled");
                }
                other => host.push(other),
            }
        }
        match self.ui.renamed.take() {
            Some((Named::Node(i), name)) => self.rename(i, &name),
            Some((Named::Layer(id), name)) => self.rename_layer(id, &name),
            None => {}
        }
        for key in std::mem::take(&mut self.ui.arrows) {
            let d = self.arrow_offset(key);
            self.selection_action(Action::Transform(Transform::Move(d)));
        }
        self.preview.lighting = self.ui.render.lighting(self.cam.distance);
        self.preview.max_spp = self.ui.render.samples();
        // The theme's own colours for the view: its background, guides and selection.
        self.renderer.colors = self.ui.shown_theme(&self.ctx).view;
        self.preview.set_backdrop(self.renderer.colors.sky);
        if let Some(done) = self.draw(out.shapes, out.pixels_per_point, out.textures_delta, view) {
            host.push(done);
        }
        (host, out.platform_output)
    }

    /// Whether an agent's call has to wait: the user is in the middle of a drag, a box or a
    /// colour or material slider.
    pub fn agent_busy(&self) -> bool {
        self.gesture.is_some()
            || self.moving.is_some()
            || self.marquee.is_some()
            || self.world_drag.is_some()
            || self.lifting()
            // A colour or material slider being dragged is one open edit until release.
            || (self.ui.color_edit.is_some() && self.ctx.input(|i| i.pointer.any_down()))
    }

    /// Run a terminal agent's tool call on the scene (see [`Agents::call`]): an edit applies
    /// at once as one undo step named after the agent, or with "Review every agent change"
    /// on, waits in the agents' change under review ([`Agents::stage`]), which every later
    /// call goes into until the user applies or discards it. Refused while an AI change is
    /// under review, since nothing else edits the scene then. What the agent has not heard
    /// yet (the user applied or discarded its edits) comes first.
    pub fn agent_call(
        &mut self,
        conn: u64,
        client: &str,
        name: &str,
        args: &serde_json::Value,
    ) -> Result<String, String> {
        let agents_review = self.review.as_ref().filter(|r| r.agents.is_some());
        if name == vxr_mcp::live::REVIEW_STATUS {
            return Ok(self.agents.status(conn, agents_review));
        }
        if self.review.is_some() && agents_review.is_none() {
            return Err(REVIEW_AGENT.into());
        }
        let notice = self.agents.take_notices(conn);
        let now = self.ctx.input(|i| i.time);
        self.agents.note_call(conn, now, name);
        let r = if self.review.is_some() || (self.agents.review && crate::agents::edits(name, args)) {
            if self.review.is_none() {
                self.open_review(None);
            }
            let review = self.review.as_mut().unwrap();
            let r = self.agents.stage(&mut self.scene, review, conn, client, name, args);
            // A change whose every call failed is no change: it closes.
            if !review.session.steps().iter().any(|s| s.ok)
                && let Some(review) = self.review.take()
            {
                review.session.discard(&mut self.scene);
            }
            r
        } else {
            self.place_box();
            self.agents.call(&mut self.scene, conn, client, name, args)
        };
        if let At::Object(i) = self.scene.at() {
            self.last_object = Some(i);
        }
        match notice {
            Some(n) => r.map(|t| format!("{n}\n{t}")).map_err(|e| format!("{n}\n{e}")),
            None => r,
        }
    }

    /// A resource a terminal agent reads, or None for a uri that names nothing. Under the
    /// agents' review it reads the scene with their change.
    pub fn agent_read(&mut self, uri: &str) -> Option<Result<String, String>> {
        match &mut self.review {
            Some(r) if r.agents.is_some() => match uri {
                "vxr://scene/summary" => {
                    Some(r.session.call(&mut self.scene, "scene_summary", serde_json::json!({})).map_err(|e| e.message))
                }
                _ => self.agents.read(&mut self.scene, uri),
            },
            Some(_) => Some(Err(REVIEW_AGENT.into())),
            None => self.agents.read(&mut self.scene, uri),
        }
    }

    /// The Agents tab's "Review every agent change" switch. Turned off, a change
    /// already waiting stays for the user to apply or discard.
    fn review_agents(&mut self, on: bool) {
        self.agents.review = on;
        let msg = match (on, self.review.as_ref().is_some_and(|r| r.agents.is_some())) {
            (true, _) => "Agents' edits now wait for your review",
            (false, true) => "Agents' edits apply at once after this change under review",
            (false, false) => "Agents' edits apply at once",
        };
        self.ui.toast(&self.ctx, msg);
    }

    /// Undo on edit `i` in the Agents tab's Agent edits list.
    pub fn undo_agent_edit(&mut self, i: usize) {
        self.gesture = None;
        self.moving = None;
        self.world_drag = None;
        let msg = match self.agents.undo_edit(&mut self.scene, i) {
            Ok(label) => format!("Undid {label}"),
            Err(e) => format!("Could not undo: {e}"),
        };
        self.ui.toast(&self.ctx, msg);
    }

    /// Tab: show the world of every object, or go back to editing the last object's model
    /// (the first object's if none was edited yet).
    pub fn toggle_world(&mut self) {
        self.gesture = None;
        self.moving = None;
        self.marquee = None;
        self.world_drag = None;
        let s = &mut self.scene;
        let msg = match (s.at(), s.world()) {
            (_, None) => "A new scene is one model".to_string(),
            (At::World, Some(w)) => {
                let objects = w.objects();
                // A picked group edits its first object.
                let picked = s.picked().and_then(|p| w.objects_under(p).first().copied());
                let i = picked.or(self.last_object.filter(|i| objects.contains(i))).or(objects.first().copied());
                match i {
                    Some(i) if s.edit_object(i) => format!("Editing {}", object_name(s.world().unwrap(), i)),
                    _ => "No object to edit".into(),
                }
            }
            (at, Some(_)) => {
                if s.show_world() {
                    if let At::Object(i) = at {
                        self.last_object = Some(i);
                        s.pick(Some(i));
                    }
                    let n = s.world().unwrap().objects().len();
                    format!("World: {n} object{}", if n == 1 { "" } else { "s" })
                } else {
                    format!("The world is over {} voxels on an axis", vxr_core::scene::MAX_EXTENT)
                }
            }
        };
        self.ui.toast(&self.ctx, msg);
    }

    /// A scene tree row was clicked: pick the object in world mode, or edit its model.
    pub fn pick_node(&mut self, i: usize) {
        let s = &mut self.scene;
        match s.at() {
            At::World => {
                s.pick(Some(i));
            }
            At::Object(k) if k != i => {
                self.gesture = None;
                self.moving = None;
                self.marquee = None;
                if s.edit_object(i) {
                    self.last_object = Some(i);
                    let msg = format!("Editing {}", object_name(s.world().unwrap(), i));
                    self.ui.toast(&self.ctx, msg);
                }
            }
            _ => {}
        }
    }

    /// Hide or show tree node `i` in world mode, as one undo step.
    pub fn set_hidden(&mut self, i: usize, hidden: bool) {
        let Some(w) = self.scene.world() else { return };
        let name = node_name(w, i);
        let msg = if self.scene.edit_world(|w| w.set_hidden(i, hidden)).is_none() {
            "Tab to the world to hide objects".to_string()
        } else if hidden {
            format!("Hid {name}")
        } else {
            format!("Showing {name}")
        };
        self.ui.toast(&self.ctx, msg);
    }

    /// Name tree node `i` (an empty name clears it), as one undo step, in world mode.
    pub fn rename(&mut self, i: usize, name: &str) {
        let name = name.trim();
        let Some(w) = self.scene.world() else { return };
        let new = (!name.is_empty()).then_some(name);
        if w.tree.nodes.get(i).is_none_or(|n| n.name.as_deref() == new.map(str::as_bytes)) {
            return;
        }
        if self.scene.edit_world(|w| w.set_name(i, new)).is_some() {
            let msg = format!("Renamed to {}", node_name(self.scene.world().unwrap(), i));
            self.ui.toast(&self.ctx, msg);
        }
    }

    /// Put the picked object or group on layer `id`, as one undo step, in world mode.
    pub fn put_on_layer(&mut self, id: i32) {
        let Some(w) = self.scene.world() else { return };
        let msg = match self.scene.picked() {
            Some(i) if w.tree.nodes[i].layer == id => return,
            Some(i) => {
                let name = node_name(w, i);
                self.scene.edit_world(|w| w.put_on_layer(i, id));
                format!("Moved {name} to {}", layer_name(self.scene.world().unwrap(), id))
            }
            None => "Pick an object to move it to a layer".into(),
        };
        self.ui.toast(&self.ctx, msg);
    }

    /// Hide or show layer `id`, as one undo step, in world mode.
    pub fn set_layer_hidden(&mut self, id: i32, hidden: bool) {
        let Some(w) = self.scene.world() else { return };
        let name = layer_name(w, id);
        let msg = if self.scene.edit_world(|w| w.set_layer_hidden(id, hidden)).is_none() {
            "Tab to the world to hide layers".to_string()
        } else if hidden {
            format!("Hid layer {name}")
        } else {
            format!("Showing layer {name}")
        };
        self.ui.toast(&self.ctx, msg);
    }

    /// Add a layer named "layer N", as one undo step, in world mode.
    pub fn add_layer(&mut self) {
        let Some(w) = self.scene.world() else { return };
        let name = format!("layer {}", w.layers().iter().map(|l| l.id + 1).max().unwrap_or(0));
        let msg = match self.scene.edit_world(|w| w.add_layer(&name)) {
            Some(_) => format!("Added {name}"),
            None => "Tab to the world to add layers".into(),
        };
        self.ui.toast(&self.ctx, msg);
    }

    /// Name layer `id` (an empty name clears it), as one undo step, in world mode.
    pub fn rename_layer(&mut self, id: i32, name: &str) {
        let name = name.trim();
        let Some(w) = self.scene.world() else { return };
        let new = (!name.is_empty()).then_some(name);
        if w.layers().iter().find(|l| l.id == id).is_none_or(|l| l.name.as_deref() == new.map(str::as_bytes)) {
            return;
        }
        if self.scene.edit_world(|w| w.set_layer_name(id, new)).is_some() {
            let msg = format!("Renamed layer to {}", layer_name(self.scene.world().unwrap(), id));
            self.ui.toast(&self.ctx, msg);
        }
    }

    /// The scene's name without `.vox`, to name exports by.
    pub fn export_stem(&self) -> String {
        let n = self.ui.scene_name.trim_end_matches(".vox").trim_end_matches(".VOX");
        if n.is_empty() { "vxr" } else { n }.to_string()
    }

    /// The file name for the PNG slices export: the scene's name with `-slices.png`.
    pub fn slices_name(&self) -> String {
        format!("{}-slices.png", self.export_stem())
    }

    /// The file name for a render export: the scene's name with `-render.png`.
    pub fn export_name(&self) -> String {
        format!("{}-render.png", self.export_stem())
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
            vxr_render::ViewColors::NEUTRAL.sky,
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

    /// The scene replaced its map (a resize or crop, or their undo): upload the new one and
    /// keep the camera on the same world point, so the view does not jump.
    fn reshaped(&mut self) {
        let old = self.shape.1;
        self.shape = (self.scene.reshapes(), self.scene.origin);
        let d: [i32; 3] = std::array::from_fn(|a| old[a] - self.scene.origin[a]);
        self.cam.target += Vec3::new(d[0] as f32, d[1] as f32, d[2] as f32);
        let (dev, queue) = (&self.gpu.device, &self.gpu.queue);
        match GpuMap::new(dev, queue, &self.layout, &self.scene.map, &self.scene.palette) {
            Ok(m) => self.map = m,
            Err(e) => eprintln!("upload failed: {e}"),
        }
        self.scene.map.take_dirty();
        self.scene.map.take_dirty_box();
        self.sel_shown = (u64::MAX, 0);
        self.cursor = None;
        self.preview.restart();
        if self.export.take().is_some() {
            self.ui.toast(&self.ctx, "Export cancelled: the scene changed");
        }
    }

    /// The preview's size: 640x360 in the corner, one pixel per point of the view in the full
    /// render view (half the pixels at 2x, so it converges in about the same time as the
    /// corner per area of screen), or the corner's size when that would not fit the device.
    fn preview_size(&self, full: bool) -> (u32, u32) {
        let corner = (vxr_render::preview::PREVIEW_W, vxr_render::preview::PREVIEW_H);
        if !full {
            return corner;
        }
        let pt = |px: u32| ((px as f32 / self.ppp.max(1.0)).round() as u32).max(1);
        let size = (pt(self.width), pt(self.height));
        if Preview::fits(&self.gpu.device, size.0, size.1) { size } else { corner }
    }

    /// Make the preview again at `size`, keeping its settings; it restarts.
    fn resize_preview(&mut self, size: (u32, u32)) {
        let (dev, queue) = (&self.gpu.device, &self.gpu.queue);
        let mut p = Preview::sized(dev, queue, &self.layout, &self.palette, self.gpu.timestamps, size);
        p.set_materials(queue, &self.materials);
        p.lighting = self.preview.lighting;
        p.max_spp = self.preview.max_spp;
        p.set_backdrop(self.renderer.colors.sky);
        p.restarts = self.preview.restarts + 1;
        if let Some(r) = &self.review {
            p.set_hidden(dev, queue, &r.ghost_mask(&self.scene));
        }
        self.preview = p;
        self.preview_shown = false;
    }

    /// Draw a frame; returns `Action::Exported` when an export finished this frame.
    fn draw(
        &mut self,
        shapes: Vec<egui::epaint::ClippedShape>,
        ppp: f32,
        textures: egui::TexturesDelta,
        view: &wgpu::TextureView,
    ) -> Option<Action> {
        if self.scene.reshapes() != self.shape.0 {
            self.reshaped();
        }
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
        // The selection mask follows brick slots, which only move when cells fill or empty:
        // build it again then while something is selected, not on a repaint.
        let sel = (self.scene.selection_version(), self.scene.map.fills);
        if sel.0 != self.sel_shown.0 || (sel != self.sel_shown && self.ui.selected > 0) {
            self.sel_shown = sel;
            self.renderer.set_selection(dev, queue, &self.scene.selection_mask());
        }
        // The ghost of a change under review follows brick slots too; the render corner hides
        // its removals, which stay in the map until Apply.
        let ghost = self.review.as_ref().map(|r| (r.version, self.scene.map.fills));
        if ghost != self.ghost_shown {
            self.ghost_shown = ghost;
            let words = self.review.as_ref().map(|r| r.ghost_mask(&self.scene)).unwrap_or_default();
            self.renderer.set_ghost(dev, queue, &words);
            self.preview.set_hidden(dev, queue, &words);
        }
        let busy =
            self.gesture.is_some() || self.moving.is_some() || self.marquee.is_some() || self.world_drag.is_some();
        let tint = if busy { None } else { self.cursor.filter(|t| !t.ground).map(|t| t.voxel) };
        self.renderer.guides = Guides {
            bounds: self.scene.map.size_voxels(),
            floor: self.scene.floor,
            grid: self.ui.grid,
            ground: self.ui.ground,
            frame: self.ui.frame,
            line: ppp,
        };
        self.renderer.set_camera(queue, &self.cam, tint);
        let mut enc = dev.create_command_encoder(&Default::default());
        // The preview's passes go first so they can be timed on their own (see preview.rs).
        let corner = self.ui.render_rect.filter(|_| self.ui.show_render);
        let want = self.preview_size(corner.is_some() && self.ui.full_render);
        if (self.preview.width, self.preview.height) != want {
            self.resize_preview(want);
        }
        let (dev, queue) = (&self.gpu.device, &self.gpu.queue);
        if let Some(x) = &mut self.export {
            x.encode(queue, &mut enc, &self.map, edit);
        }
        // An export takes the corner's GPU time; the corner keeps its last image meanwhile.
        let paused = corner.is_some() && self.export.is_some();
        if corner.is_some() && !paused {
            if !self.preview_shown {
                self.preview.restart();
            }
            let idle = now_ms() - self.last_input > IDLE_MS;
            self.preview.encode(queue, &mut enc, &self.map, &self.cam, edit, idle);
        }
        // This frame's edit went only to the export, so an edit while paused leaves the
        // corner stale until it restarts.
        self.preview_shown = corner.is_some() && !(paused && (edit.is_some() || !self.preview_shown));
        // The full render view covers every pixel, so the voxel view under it is not drawn.
        let full = corner.is_some() && self.ui.full_render;
        if !full {
            self.renderer.march(&mut enc, &self.map, None);
        }
        if let Some(r) = corner {
            let px = [r.left() * ppp, r.top() * ppp, r.width() * ppp, r.height() * ppp];
            let radius = if full { 0.0 } else { crate::ui::R as f32 * ppp };
            self.preview.composite_into(dev, queue, &mut enc, &self.renderer.color, px, radius);
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

/// An object's name, or "object N" (its place among the objects) when it has none.
pub fn object_name(w: &vxr_core::World, i: usize) -> String {
    match &w.tree.nodes[i].name {
        Some(n) => String::from_utf8_lossy(n).into_owned(),
        None => format!("object {}", w.objects().iter().position(|&k| k == i).map_or(i, |k| k + 1)),
    }
}

/// A tree node's name: its own, or "object N" / "group N" by its place among its kind.
pub fn node_name(w: &vxr_core::World, i: usize) -> String {
    match (&w.tree.nodes[i].name, w.model_of(i)) {
        (Some(n), _) => String::from_utf8_lossy(n).into_owned(),
        (None, Some(_)) => object_name(w, i),
        (None, None) => {
            let groups: Vec<usize> = w.tree.order().into_iter().skip(1).filter(|&k| w.model_of(k).is_none()).collect();
            format!("group {}", groups.iter().position(|&k| k == i).map_or(i, |k| k + 1))
        }
    }
}

/// A layer's name: its own, or "layer N" by its id.
pub fn layer_name(w: &vxr_core::World, id: i32) -> String {
    match w.layers().into_iter().find(|l| l.id == id).and_then(|l| l.name) {
        Some(n) => String::from_utf8_lossy(&n).into_owned(),
        None => format!("layer {id}"),
    }
}

/// The LAYERS list under the scene tree: every layer by id with its object count, whenever
/// the tree shows.
pub fn layer_rows(s: &Scene) -> Vec<LayerRow> {
    let Some(w) = s.world() else { return Vec::new() };
    let objects = w.objects();
    if objects.len() < 2 {
        return Vec::new();
    }
    w.layers()
        .into_iter()
        .map(|l| LayerRow {
            id: l.id,
            name: layer_name(w, l.id),
            objects: objects.iter().filter(|&&i| w.tree.nodes[i].layer == l.id).count(),
            hidden: l.hidden,
        })
        .collect()
}

/// The scene tree's rows: every node under the root in tree order, once there are two
/// objects or more (none otherwise, so a single model stays clean).
pub fn tree_rows(s: &Scene) -> Vec<TreeRow> {
    let Some(w) = s.world() else { return Vec::new() };
    let objects = w.objects();
    if objects.len() < 2 {
        return Vec::new();
    }
    let parents = w.tree.parents();
    let mut users = vec![0; w.models.len()];
    for &i in &objects {
        if let Some(m) = w.model_of(i) {
            users[m] += 1;
        }
    }
    w.tree
        .order()
        .into_iter()
        .skip(1)
        .map(|i| {
            let (mut depth, mut k) = (0, parents[i]);
            while let Some(p) = k {
                depth += parents[p].is_some() as usize;
                k = parents[p];
            }
            let model = w.model_of(i);
            // The model being edited lives in the scene's map until it is stored back.
            let size = match (model, s.at()) {
                (Some(_), At::Object(e)) if w.model_of(e) == model => s.bounds().size,
                (Some(m), _) => w.models[m].size_voxels(),
                (None, _) => [0; 3],
            };
            TreeRow {
                node: i,
                depth,
                name: node_name(w, i),
                group: model.is_none(),
                linked: model.map_or(0, |m| users[m]),
                size,
                hidden: w.tree.nodes[i].hidden,
                shown: w.shown(i),
                layer: w.tree.nodes[i].layer,
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture(name: &str) -> Scene {
        let f = vxr_vox::fixtures::all().into_iter().find(|f| f.name == name).unwrap();
        Scene::from_vox(&f.builder.build().write()).unwrap()
    }

    #[test]
    fn tree_rows_show_from_the_second_object() {
        assert!(tree_rows(&Scene::new(NEW_SIZE)).is_empty());
        assert!(tree_rows(&fixture("01-single-voxel")).is_empty());
        let mut s = fixture("22-world");
        let rows = tree_rows(&s);
        let names: Vec<_> = rows.iter().map(|r| (r.name.as_str(), r.depth, r.group)).collect();
        assert_eq!(
            names,
            [
                ("slab", 0, false),
                ("trees", 0, true),
                ("tree a", 1, false),
                ("tree b", 1, false),
                ("grove", 1, true),
                ("tree c", 2, false),
                ("gem", 0, false),
                ("lost gem", 0, false),
            ]
        );
        let row = |rows: &[TreeRow], n: &str| rows.iter().find(|r| r.name == n).unwrap().clone();
        assert_eq!(row(&rows, "tree c").linked, 3);
        assert_eq!(row(&rows, "slab").size, [8, 1, 8]);
        assert!(row(&rows, "lost gem").hidden && !row(&rows, "gem").hidden && !row(&rows, "gem").shown);
        // Hiding a group hides the rows under it; unnamed nodes get names by their kind.
        let (trees, slab) = (row(&rows, "trees").node, row(&rows, "slab").node);
        s.edit_world(|w| {
            w.set_hidden(trees, true);
            w.set_name(trees, None);
            w.set_name(slab, None);
        });
        let rows = tree_rows(&s);
        assert!(!row(&rows, "tree a").shown && !row(&rows, "group 1").shown);
        assert_eq!(rows[0].name, "object 1");
        // The model being edited reports the scene's bounds.
        assert!(s.edit_object(slab));
        s.set_bounds([0, 0, 0], [8, 3, 8]);
        assert_eq!(tree_rows(&s)[0].size, [8, 3, 8]);
    }

    #[test]
    fn layer_rows_list_every_layer_with_its_objects() {
        assert!(layer_rows(&fixture("01-single-voxel")).is_empty());
        let mut s = fixture("22-world");
        let rows: Vec<_> = layer_rows(&s).into_iter().map(|l| (l.id, l.name, l.objects, l.hidden)).collect();
        assert_eq!(
            rows,
            [(0, "ground".into(), 2, false), (1, "trees".into(), 3, false), (2, "secret".into(), 1, true)]
        );
        // A layer an object is on without a LAYR entry is listed by its id.
        let gem = tree_rows(&s).into_iter().find(|r| r.name == "gem").unwrap().node;
        s.edit_world(|w| w.put_on_layer(gem, 5));
        let last = layer_rows(&s).pop().unwrap();
        assert_eq!((last.id, last.name.as_str(), last.objects), (5, "layer 5", 1));
        assert_eq!(tree_rows(&s).iter().find(|r| r.node == gem).unwrap().layer, 5);
    }
}
