//! Headless run for evidence and smoke testing: no window, so it works with the screen
//! locked. It renders the editor offscreen, drives every tool through the same pointer path
//! the window uses, saves and reopens the scene, and writes screenshots.

use std::path::Path;

use vxr_app::Editor;
use vxr_app::editor::{Button, NEW_SIZE};
use vxr_app::ui::{Action, Region, Shape, Tool};
use vxr_core::Scene;
use vxr_render::Gpu;
use vxr_render::gpu::read_texture;

const FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Bgra8Unorm;
/// Logical size, the mockups' 1280x800.
const POINTS: (u32, u32) = (1280, 800);

struct Shot {
    ed: Editor,
    target: wgpu::Texture,
    /// egui time in seconds, so double clicks and drags see time move forward.
    time: f64,
    /// What the editor asked its host to do (open, save, export), oldest first.
    host: Vec<Action>,
    /// The system's light or dark setting, as a window reports it (none: unknown).
    system: Option<egui::Theme>,
}

impl Shot {
    fn new(scene: Scene, name: &str, ppp: f32) -> Result<Self, String> {
        Self::sized(scene, name, ppp, POINTS)
    }

    fn sized(scene: Scene, name: &str, ppp: f32, points: (u32, u32)) -> Result<Self, String> {
        let instance = wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle());
        let gpu = pollster::block_on(Gpu::new(instance, None))?;
        let (w, h) = ((points.0 as f32 * ppp) as u32, (points.1 as f32 * ppp) as u32);
        let target = gpu.device.create_texture(&wgpu::TextureDescriptor {
            label: Some("shot"),
            size: wgpu::Extent3d { width: w, height: h, depth_or_array_layers: 1 },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: FORMAT,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
            view_formats: &[],
        });
        let ed = Editor::new(gpu, scene, name, FORMAT, (w, h), ppp, crate::native::fonts())?;
        Ok(Self { ed, target, time: 0.0, host: Vec::new(), system: None })
    }

    /// Draw `n` frames (egui needs a couple to lay out and upload fonts).
    fn frames(&mut self, n: usize) {
        self.frames_with(n, Vec::new());
    }

    /// Draw `n` frames, the first with `events` as egui input.
    fn frames_with(&mut self, n: usize, mut events: Vec<egui::Event>) {
        let view = self.target.create_view(&Default::default());
        for _ in 0..n {
            let size = egui::vec2(self.ed.width as f32 / self.ed.ppp, self.ed.height as f32 / self.ed.ppp);
            self.time += 1.0 / 60.0;
            let mut input = egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, size)),
                time: Some(self.time),
                events: std::mem::take(&mut events),
                system_theme: self.system,
                ..Default::default()
            };
            input.viewports.entry(egui::ViewportId::ROOT).or_default().native_pixels_per_point =
                Some(self.ed.native_ppp);
            let (host, _) = self.ed.frame(input, &view);
            self.host.extend(host);
        }
    }

    fn save_png(&self, path: &Path) -> Result<(), String> {
        let (dev, queue) = (&self.ed.gpu.device, &self.ed.gpu.queue);
        let mut px = pollster::block_on(read_texture(dev, queue, &self.target));
        for p in px.as_chunks_mut::<4>().0 {
            *p = [p[2], p[1], p[0], 255];
        }
        let f = std::fs::File::create(path).map_err(|e| format!("{}: {e}", path.display()))?;
        let mut e = png::Encoder::new(std::io::BufWriter::new(f), self.target.width(), self.target.height());
        e.set_color(png::ColorType::Rgba);
        e.set_depth(png::BitDepth::Eight);
        e.write_header().and_then(|mut w| w.write_image_data(&px)).map_err(|e| e.to_string())?;
        println!("wrote {}", path.display());
        Ok(())
    }

    /// A pixel near the middle of the view whose pointer lands on the top face of a voxel.
    fn top_face_pixel(&mut self, from: [f32; 2]) -> Result<[f32; 2], String> {
        let step = 6.0 * self.ed.ppp;
        for r in 0..200 {
            for k in 0..8 {
                let a = k as f32 * std::f32::consts::FRAC_PI_4;
                let p = [from[0] + a.cos() * r as f32 * step, from[1] + a.sin() * r as f32 * step];
                self.ed.pointer_moved(Some(p));
                if self.ed.cursor().is_some_and(|t| !t.ground && t.normal == [0, 1, 0]) {
                    return Ok(p);
                }
            }
        }
        Err("no top face found near the middle of the view".into())
    }

    /// Press at `at`, move through `path` (pixel offsets), release.
    fn stroke(&mut self, at: [f32; 2], path: &[[f32; 2]]) {
        self.ed.pointer_moved(Some(at));
        self.ed.button(Button::Left, true, false);
        for d in path {
            self.ed.pointer_moved(Some([at[0] + d[0], at[1] + d[1]]));
        }
        self.ed.button(Button::Left, false, false);
    }

    /// The middle of a named part of the UI, in points (see `Hybrid::hits`).
    fn at(&self, name: &str) -> Result<egui::Pos2, String> {
        self.ed.ui.hits.get(name).map(|r| r.center()).ok_or_else(|| format!("{name} is not on screen"))
    }

    /// Press the left button at `from` (points), move to `to` in a few steps, release, with
    /// `modifiers` held, all as egui input.
    fn ui_drag(&mut self, from: egui::Pos2, to: egui::Pos2, modifiers: egui::Modifiers) {
        let button =
            |pos, pressed| egui::Event::PointerButton { pos, button: egui::PointerButton::Primary, pressed, modifiers };
        self.frames_with(1, vec![egui::Event::ModifiersChanged(modifiers), egui::Event::PointerMoved(from)]);
        self.frames_with(1, vec![button(from, true)]);
        if from != to {
            for k in 1..=6 {
                self.frames_with(1, vec![egui::Event::PointerMoved(from + (to - from) * k as f32 / 6.0)]);
            }
        }
        self.frames_with(1, vec![button(to, false)]);
        self.frames_with(2, vec![egui::Event::ModifiersChanged(egui::Modifiers::NONE)]);
    }

    fn ui_click(&mut self, name: &str) -> Result<(), String> {
        let p = self.at(name)?;
        self.ui_drag(p, p, egui::Modifiers::NONE);
        Ok(())
    }

    /// Click a field, replace its text with `text` and press Enter.
    fn type_into(&mut self, name: &str, text: &str) -> Result<(), String> {
        self.ui_click(name)?;
        let key =
            |key, modifiers| egui::Event::Key { key, physical_key: None, pressed: true, repeat: false, modifiers };
        self.frames_with(1, vec![key(egui::Key::A, egui::Modifiers::COMMAND)]);
        self.frames_with(1, vec![egui::Event::Text(text.into())]);
        self.frames_with(2, vec![key(egui::Key::Enter, egui::Modifiers::NONE)]);
        Ok(())
    }

    /// Hold modifier keys from the next frame on, as the window would report them.
    fn hold(&mut self, m: egui::Modifiers) {
        self.frames_with(1, vec![egui::Event::ModifiersChanged(m)]);
    }

    fn tool(&mut self, tool: Tool, shape: Shape) {
        (self.ed.ui.tool, self.ed.ui.shape) = (tool, shape);
    }

    fn voxels(&self) -> u64 {
        self.ed.scene.map.voxels
    }
}

fn check(ok: bool, what: String) -> Result<(), String> {
    println!("{} {what}", if ok { "ok  " } else { "FAIL" });
    if ok { Ok(()) } else { Err(what) }
}

pub fn run(dir: &Path, scene: Option<&Path>) -> Result<(), String> {
    let path = scene;
    let source = scene;
    std::fs::create_dir_all(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    let load = || match scene {
        Some(p) => crate::native::open(p),
        None => Ok((Scene::new(NEW_SIZE), "Untitled".into())),
    };
    // The opened scene as it first looks, at 1x and 2x.
    for (ppp, tag) in [(1.0, "1x"), (2.0, "2x")] {
        let (scene, name) = load()?;
        let mut s = Shot::new(scene, &name, ppp)?;
        s.frames(3);
        s.save_png(&dir.join(format!("app-{tag}.png")))?;
    }

    themes(dir, &load)?;
    touch(dir, &load)?;
    phone(dir, &load)?;

    // Every tool at 2x through the pointer path.
    let (scene, name) = load()?;
    let mut s = Shot::new(scene, &name, 2.0)?;
    s.frames(2);
    let mid = [s.ed.width as f32 / 2.0, s.ed.height as f32 / 2.0];
    let v0 = s.voxels();
    println!("opened {name}: {v0} voxels");
    let file = path.map(std::fs::read).transpose().map_err(|e| e.to_string())?;
    if let Some(file) = &file {
        check(s.ed.scene.to_vox() == *file, format!("save without edits: byte-identical ({} bytes)", file.len()))?;
    }

    s.tool(Tool::Add, Shape::Voxel);
    let p = if v0 == 0 { mid } else { s.top_face_pixel(mid)? };
    s.stroke(p, &[[30.0, 0.0], [60.0, 0.0], [90.0, 10.0]]);
    let v1 = s.voxels();
    check(v1 > v0, format!("add voxel drag: {v0} -> {v1}"))?;
    s.ed.undo();
    check(s.voxels() == v0, format!("undo: back to {}", s.voxels()))?;
    s.ed.redo();
    check(s.voxels() == v1, format!("redo: {}", s.voxels()))?;

    s.tool(Tool::Add, Shape::Box);
    let p = s.top_face_pixel([mid[0] - 200.0, mid[1] + 100.0])?;
    s.stroke(p, &[[60.0, 20.0], [120.0, 40.0]]);
    let v2 = s.voxels();
    check(v2 > v1, format!("add box drag: {v1} -> {v2}"))?;
    s.ed.place_box();

    s.tool(Tool::Paint, Shape::Face);
    s.ed.ui.slot = 12;
    let before = s.ed.scene.map.counts[12];
    let p = s.top_face_pixel(p)?;
    s.stroke(p, &[]);
    let after = s.ed.scene.map.counts[12];
    check(after > before && s.voxels() == v2, format!("paint face: slot 12 has {before} -> {after} voxels"))?;

    s.tool(Tool::Erase, Shape::Voxel);
    let p = s.top_face_pixel(mid)?;
    s.stroke(p, &[]);
    let v3 = s.voxels();
    check(v3 == v2 - 1, format!("erase voxel: {v2} -> {v3}"))?;

    s.tool(Tool::Add, Shape::Face);
    s.ed.ui.slot = 40;
    let p = s.top_face_pixel([mid[0] + 150.0, mid[1]])?;
    s.stroke(p, &[]);
    let v4 = s.voxels();
    check(v4 > v3, format!("extrude face: {v3} -> {v4}"))?;

    s.tool(Tool::Pick, Shape::Voxel);
    s.ed.ui.slot = 1;
    s.ed.pointer_moved(Some(p));
    let under = s.ed.cursor().map(|t| t.value).unwrap_or(0);
    s.stroke(p, &[]);
    check(
        s.ed.ui.slot == under && under == 40,
        format!("pick: slot {} (voxel under the pointer is {under})", s.ed.ui.slot),
    )?;

    // "?" (as the coach card says) opens the command list, which lists every key.
    let key = |key, modifiers| egui::Event::Key { key, physical_key: None, pressed: true, repeat: false, modifiers };
    s.frames_with(2, vec![key(egui::Key::Questionmark, egui::Modifiers::SHIFT)]);
    check(s.ed.ui.cmdk.is_some(), "? opens the command list".into())?;
    s.frames_with(2, vec![key(egui::Key::Escape, egui::Modifiers::NONE)]);
    check(s.ed.ui.cmdk.is_none(), "Escape closes it".into())?;

    palette(&mut s, dir)?;
    brushes(dir)?;
    brush_size(dir)?;
    box_height(dir)?;
    selection(dir)?;
    model_tools(dir)?;
    views(dir)?;
    camera(dir, source)?;
    world(dir)?;
    new_scene_save(dir)?;
    new_scene_command(dir)?;
    scene_tree(dir)?;
    render(&mut s, dir)?;
    materials(&mut s, dir)?;
    export(&mut s, dir)?;
    review(dir)?;
    review_phone(dir)?;
    assistant_column(dir)?;
    agent_chip_squeezed(dir)?;
    palette_beside_flyouts()?;
    column_short_window(dir)?;
    agents_states(dir)?;
    assistant(dir)?;
    ai_refused()?;
    ai_new_chat()?;
    ai_bar_failed()?;
    #[cfg(unix)]
    agents_live(dir)?;
    #[cfg(unix)]
    agents_bridge(dir)?;

    // Save, reopen and compare.
    let bytes = s.ed.scene.to_vox();
    let path = dir.join("edited.vox");
    std::fs::write(&path, &bytes).map_err(|e| e.to_string())?;
    let (reopened, _) = crate::native::open(&path)?;
    check(
        reopened.map.voxels == v4 && reopened.map.counts == s.ed.scene.map.counts,
        format!("save and reopen: {} voxels, same per-colour counts ({} bytes)", reopened.map.voxels, bytes.len()),
    )?;
    if let Some(file) = &file {
        // Lossless save: only model and scene graph chunks may change; the rest (palette,
        // materials, layers, cameras, notes, unknown chunks) keep their bytes and order.
        let other = |b: &[u8]| -> Result<Vec<vxr_vox::Chunk>, String> {
            let raw = vxr_vox::RawFile::parse(b).map_err(|e| e.to_string())?;
            let geometry = [b"SIZE", b"XYZI", b"TDCZ", b"nTRN", b"nGRP", b"nSHP"];
            Ok(raw.chunks.into_iter().filter(|c| !geometry.iter().any(|id| c.is(id))).collect())
        };
        let kept = other(file)?;
        check(other(&bytes)? == kept, format!("lossless save: {} other chunks byte-identical", kept.len()))?;
    }
    s.ed.load(reopened, "edited.vox")?;
    s.tool(Tool::Add, Shape::Voxel);
    s.frames(2);
    let p = s.top_face_pixel(mid)?;
    s.ed.pointer_moved(Some(p));
    s.frames(12);
    s.save_png(&dir.join("app-2x-edited.png"))?;
    if let Some(p) = source {
        frame_times(p)?;
        review_times(p)?;
    }
    Ok(())
}

/// The touch layout on an iPad-sized screen (1180x820 points, 2x): the dock in place of the
/// rail with 44-point targets, in landscape and portrait, Neutral and Paper, and the gestures
/// through `Editor::touch_at`: a tap adds one voxel, two fingers orbit and pinch, three pan, a
/// two-finger tap undoes, and a finger on the dock is the UI's.
fn touch(dir: &Path, load: &dyn Fn() -> Result<(Scene, String), String>) -> Result<(), String> {
    use vxr_app::editor::{Finger, TOUCH_SCALE};
    use vxr_app::theme::ThemeChoice;
    for (points, tag) in [((1180, 820), "land"), ((820, 1180), "port")] {
        for theme in [ThemeChoice::Neutral, ThemeChoice::Paper] {
            let (scene, name) = load()?;
            let mut s = Shot::sized(scene, &name, 2.0, points)?;
            s.ed.ui.theme = theme;
            s.ed.set_touch(true);
            s.frames(3);
            let suffix = if theme == ThemeChoice::Paper { "-paper" } else { "" };
            s.save_png(&dir.join(format!("app-2x-touch-{tag}{suffix}.png")))?;
            if theme != ThemeChoice::Neutral {
                continue;
            }
            let h = &s.ed.ui.hits;
            let targets = ["tool-add", "select", "dock-color", "undo", "voxel", "center", "mirror", "lock"];
            let small =
                targets.iter().find(|n| h.get(**n).is_none_or(|r| r.width().min(r.height()) * TOUCH_SCALE < 44.0));
            check(
                s.ed.ui.touch && s.ed.ppp == 2.0 * TOUCH_SCALE && small.is_none() && !h.contains_key("hint"),
                format!(
                    "{tag}: the touch layout has the dock with 44-point targets and no hint bar (short: {small:?})"
                ),
            )?;
            let corner = h.contains_key("render");
            check(corner == (tag == "land"), format!("{tag}: the render corner shows in landscape only ({corner})"))?;
        }
    }

    let (scene, name) = load()?;
    let mut s = Shot::sized(scene, &name, 2.0, (1180, 820))?;
    s.ed.ui.coach = false;
    s.ed.set_touch(true);
    s.frames(3);
    let mid = [s.ed.width as f32 / 2.0, s.ed.height as f32 / 2.0];
    let p = s.top_face_pixel(mid)?;
    s.ed.pointer_moved(None);
    let mut t = 1000.0;
    let mut at = |s: &mut Shot, id, phase, p: [f32; 2], dt: f64| {
        t += dt;
        s.ed.touch_at(id, phase, p, t)
    };
    let v0 = s.voxels();
    at(&mut s, 1, Finger::Down, p, 0.0);
    at(&mut s, 1, Finger::Up, p, 60.0);
    check(
        s.voxels() == v0 + 1 && s.ed.cursor().is_none(),
        format!("a tap adds one voxel ({} added)", s.voxels() - v0),
    )?;
    let near = [p[0] + 60.0, p[1]];
    at(&mut s, 1, Finger::Down, p, 500.0);
    at(&mut s, 2, Finger::Down, near, 20.0);
    at(&mut s, 1, Finger::Up, p, 60.0);
    at(&mut s, 2, Finger::Up, near, 10.0);
    check(s.voxels() == v0, "a two-finger tap undoes it".into())?;

    // A resize mid-stroke keeps the finger: its lift ends the stroke as one undo step.
    at(&mut s, 1, Finger::Down, p, 500.0);
    at(&mut s, 1, Finger::Move, p, 200.0);
    let (w, h, ppp) = (s.ed.width, s.ed.height, s.ed.native_ppp);
    s.ed.resize(w, h, ppp);
    at(&mut s, 1, Finger::Up, p, 20.0);
    let drawn = s.voxels() == v0 + 1 && s.ed.cursor().is_none();
    at(&mut s, 1, Finger::Down, p, 500.0);
    at(&mut s, 2, Finger::Down, near, 20.0);
    at(&mut s, 1, Finger::Up, p, 60.0);
    at(&mut s, 2, Finger::Up, near, 10.0);
    check(drawn && s.voxels() == v0, "a resize mid-stroke keeps the finger, and its stroke undoes in one step".into())?;

    // Two fingers landing together and moving together orbit; nothing is drawn.
    let (cam0, b) = (s.ed.cam, [mid[0] + 200.0, mid[1]]);
    at(&mut s, 1, Finger::Down, mid, 500.0);
    at(&mut s, 2, Finger::Down, b, 40.0);
    for k in 1..=20 {
        let d = 8.0 * k as f32;
        at(&mut s, 1, Finger::Move, [mid[0] + d, mid[1]], 8.0);
        at(&mut s, 2, Finger::Move, [b[0] + d, b[1]], 0.0);
    }
    at(&mut s, 1, Finger::Up, mid, 30.0);
    at(&mut s, 2, Finger::Up, b, 0.0);
    let cam1 = s.ed.cam;
    check(
        (cam1.yaw - cam0.yaw).abs() > 0.2 && cam1.distance == cam0.distance && s.voxels() == v0,
        format!("two fingers orbit ({:.2} rad) and draw nothing", cam1.yaw - cam0.yaw),
    )?;
    at(&mut s, 1, Finger::Down, mid, 500.0);
    at(&mut s, 2, Finger::Down, b, 10.0);
    for k in 1..=20 {
        let d = 6.0 * k as f32;
        at(&mut s, 1, Finger::Move, [mid[0] - d, mid[1]], 8.0);
        at(&mut s, 2, Finger::Move, [b[0] + d, b[1]], 0.0);
    }
    at(&mut s, 1, Finger::Up, mid, 30.0);
    at(&mut s, 2, Finger::Up, b, 0.0);
    let cam2 = s.ed.cam;
    check(
        cam2.distance < cam1.distance * 0.8 && cam2.yaw == cam1.yaw,
        format!("spreading two fingers zooms in ({:.0} to {:.0}) without orbiting", cam1.distance, cam2.distance),
    )?;
    let fingers = [mid, [mid[0] + 120.0, mid[1]], [mid[0] + 240.0, mid[1]]];
    for (i, f) in fingers.iter().enumerate() {
        at(&mut s, i as u64, Finger::Down, *f, 500.0 + i as f64);
    }
    for k in 1..=15 {
        for (i, f) in fingers.iter().enumerate() {
            at(&mut s, i as u64, Finger::Move, [f[0], f[1] + 10.0 * k as f32], 4.0);
        }
    }
    for (i, f) in fingers.iter().enumerate() {
        at(&mut s, i as u64, Finger::Up, *f, 10.0);
    }
    let cam3 = s.ed.cam;
    check(
        (cam3.target - cam2.target).length() > 1.0 && cam3.yaw == cam2.yaw && cam3.pitch == cam2.pitch,
        format!("three fingers pan ({:.1} voxels) without orbiting", (cam3.target - cam2.target).length()),
    )?;

    // A finger on the dock is the UI's: the host gives it to egui.
    s.frames(1);
    let c = s.at("dock-color")?;
    let c = [c.x * s.ed.ppp, c.y * s.ed.ppp];
    let on_ui = !at(&mut s, 7, Finger::Down, c, 500.0);
    at(&mut s, 7, Finger::Up, c, 50.0);
    s.ui_click("dock-color")?;
    let pal = s.ed.ui.hits.get("palette").copied();
    let dock = s.ed.ui.hits.get("dock-brushes").copied();
    check(
        on_ui && s.ed.ui.palette_open && pal.zip(dock).is_some_and(|(p, d)| p.bottom() <= d.top()),
        "a finger on the dock is the UI's, and its colour button opens the palette over the dock".into(),
    )?;
    s.ui_click("select")?;
    s.ui_click("dock-color")?;
    s.frames(2);
    let sel = s.ed.ui.hits.get("selection").copied();
    check(
        s.ed.ui.tool == Tool::Select && sel.zip(dock).is_some_and(|(r, d)| r.bottom() <= d.top()),
        "the select button opens its flyout over the dock".into(),
    )?;
    s.save_png(&dir.join("app-2x-touch-select.png"))?;
    s.ed.set_touch(false);
    s.frames(2);
    check(
        !s.ed.ui.touch && s.ed.ppp == 2.0 && s.ed.ui.hits.contains_key("hint"),
        "a mouse brings back the mouse layout".into(),
    )?;

    // The finger that brings the touch layout lands on the mouse layout egui drew.
    let rail = s.at("mirror")?;
    let rail = [rail.x * s.ed.ppp, rail.y * s.ed.ppp];
    let v = s.voxels();
    s.ed.set_touch(true);
    let on_ui = !at(&mut s, 8, Finger::Down, rail, 500.0);
    at(&mut s, 8, Finger::Up, rail, 50.0);
    s.frames(2);
    check(
        on_ui && s.voxels() == v,
        "the finger that brings the touch layout is the UI's where the mouse layout drew a panel".into(),
    )?;
    Ok(())
}

/// The touch layout on phones (360, 390 and 430 CSS px wide upright, 844 and 932 sideways, 3x):
/// every dock and top bar target, and every panel, stays on screen and reachable with the
/// assistant, the palette, the selection flyout, the Export menu and the colour editor open,
/// by pointer clicks on the drawn UI. Writes `app-3x-phone-<width>-<state>.png`.
fn phone(dir: &Path, load: &dyn Fn() -> Result<(Scene, String), String>) -> Result<(), String> {
    use vxr_app::editor::TOUCH_SCALE;
    let targets = [
        "tool-add",
        "tool-erase",
        "tool-paint",
        "tool-pick",
        "select",
        "dock-color",
        "undo",
        "redo",
        "voxel",
        "brush-box",
        "brush-face",
        "brush-line",
        "center",
        "mirror",
        "lock",
        "open",
        "save",
        "export",
        "theme",
        "ai-toggle",
        "keys",
    ];
    for css in [(360u32, 780u32), (390, 844), (430, 932), (844, 390), (932, 430)] {
        let tag = format!("{}x{}", css.0, css.1);
        let (scene, name) = load()?;
        let mut s = Shot::sized(scene, &name, 3.0, css)?;
        s.ed.set_touch(true);
        s.frames(3);
        let (w, h) = (s.ed.width as f32 / s.ed.ppp, s.ed.height as f32 / s.ed.ppp);
        // Every hit of the UI on screen, and the dock and top bar targets finger-sized.
        let fits = |s: &Shot, what: &str| -> Result<(), String> {
            let screen = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(w, h)).expand(0.5);
            let off: Vec<_> = s
                .ed
                .ui
                .hits
                .iter()
                .filter(|(n, r)| !(n.starts_with("ai-") && !n.starts_with("ai-toggle")) && !screen.contains_rect(**r))
                .map(|(n, r)| format!("{n} {r:?}"))
                .collect();
            let small: Vec<_> = targets[..15]
                .iter()
                .filter(|n| s.ed.ui.hits.get(**n).is_none_or(|r| r.width().min(r.height()) * TOUCH_SCALE < 44.0))
                .collect();
            check(
                off.is_empty() && small.is_empty(),
                format!(
                    "{tag} {what}: the UI is on screen (off: {off:?}) and the targets are there (missing: {small:?})"
                ),
            )
        };
        fits(&s, "plain")?;
        s.save_png(&dir.join(format!("app-3x-phone-{tag}.png")))?;

        // The assistant open: the dock stays reachable, so the colour button still opens the
        // palette over the assistant. The tips card hides under it rather than show through.
        let welcome = |s: &Shot| {
            let id = egui::LayerId::new(egui::Order::Middle, egui::Id::unique("welcome"));
            s.ed.ctx.memory(|m| m.areas().visible_last_frame(&id))
        };
        check(s.ed.ui.coach && welcome(&s), format!("{tag}: the tips card shows"))?;
        s.ui_click("ai-toggle")?;
        s.frames(2);
        check(!welcome(&s), format!("{tag}: the tips card hides under the assistant"))?;
        s.ed.ui.coach = false;
        fits(&s, "assistant open")?;
        s.save_png(&dir.join(format!("app-3x-phone-{tag}-assistant.png")))?;
        s.ui_click("dock-color")?;
        s.frames(2);
        check(s.ed.ui.palette_open, format!("{tag}: the colour button opens the palette with the assistant open"))?;
        fits(&s, "assistant and palette open")?;
        s.save_png(&dir.join(format!("app-3x-phone-{tag}-palette.png")))?;
        s.ui_click("dock-color")?;
        s.ui_click("ai-toggle")?;
        s.frames(2);
        check(!s.ed.ui.palette_open && !s.ed.ui.ai_open, format!("{tag}: the palette and the assistant close"))?;

        // The select button opens its flyout beside or over the dock, fully on screen, and the
        // buttons on it work.
        s.ui_click("select")?;
        s.frames(2);
        check(s.ed.ui.tool == Tool::Select, format!("{tag}: the select button picks Select"))?;
        fits(&s, "selection flyout open")?;
        s.save_png(&dir.join(format!("app-3x-phone-{tag}-select.png")))?;
        s.ui_click("tool-add")?;

        // The Export menu from the top bar.
        s.ui_click("export")?;
        s.frames(2);
        check(s.ed.ui.export_menu, format!("{tag}: the export button opens the Export menu"))?;
        fits(&s, "Export menu open")?;
        s.save_png(&dir.join(format!("app-3x-phone-{tag}-export.png")))?;
        s.ui_click("export")?;
        s.frames(2);
        check(!s.ed.ui.export_menu, format!("{tag}: the export button closes the Export menu"))?;

        // The colour editor over the palette.
        s.ui_click("dock-color")?;
        s.ui_click("edit-color")?;
        s.frames(2);
        check(s.ed.ui.color_edit.is_some(), format!("{tag}: Edit opens the colour editor"))?;
        fits(&s, "colour editor open")?;
        s.save_png(&dir.join(format!("app-3x-phone-{tag}-colour.png")))?;

        // The keyboard button opens the command list, the only way to it without a keyboard.
        s.ui_click("keys")?;
        s.frames(2);
        check(s.ed.ui.cmdk.is_some(), format!("{tag}: the keyboard button opens the command list"))?;
        fits(&s, "command list open")?;
        s.save_png(&dir.join(format!("app-3x-phone-{tag}-commands.png")))?;
    }
    Ok(())
}

/// The themes: the Theme menu in the top bar and Cmd-K pick one and ask the host to keep it,
/// System follows the system's light or dark setting, and each theme recolours the UI and the
/// view's background, ground, grid, bounds frame and selection while the voxels keep their
/// colours. Writes `theme-<id>.png` for each theme at 1280x800 with a colour selected and the
/// menu open.
fn themes(dir: &Path, load: &dyn Fn() -> Result<(Scene, String), String>) -> Result<(), String> {
    use vxr_app::theme::ThemeChoice;
    let key = |key, modifiers| egui::Event::Key { key, physical_key: None, pressed: true, repeat: false, modifiers };
    let (scene, name) = load()?;
    let mut s = Shot::new(scene, &name, 1.0)?;
    s.ed.ui.coach = false;
    s.frames(3);
    let neutral = vxr_render::ViewColors::NEUTRAL;
    check(
        s.ed.ui.theme == ThemeChoice::System && s.ed.renderer.colors == neutral,
        "the theme starts on System, which is Neutral when the system does not say".into(),
    )?;
    // A pixel of the view's background, and of a panel's fill.
    let corner = |s: &mut Shot| {
        let px = view_pixels(s);
        let at = (s.ed.width as usize * (s.ed.height as usize - 4) + s.ed.width as usize / 2) * 4;
        [px[at], px[at + 1], px[at + 2]]
    };
    let dark_bg = corner(&mut s);

    s.system = Some(egui::Theme::Light);
    s.frames(3);
    let light_bg = corner(&mut s);
    check(
        s.ed.ui.shown_theme(&s.ed.ctx).name == "Paper" && s.ed.renderer.colors == vxr_app::theme::PAPER.view,
        "System is Paper while the system is light".into(),
    )?;
    check(
        light_bg.iter().all(|&c| c > 200) && dark_bg.iter().all(|&c| c < 40),
        format!("the view's background follows: {dark_bg:?} dark, {light_bg:?} light"),
    )?;
    s.system = Some(egui::Theme::Dark);
    s.frames(3);
    check(s.ed.ui.shown_theme(&s.ed.ctx).name == "Neutral", "and Neutral again once it is dark".into())?;

    s.ui_click("theme")?;
    s.frames(2);
    let rows = ThemeChoice::ALL.iter().all(|c| s.ed.ui.hits.contains_key(&format!("theme-{}", c.id())));
    check(s.ed.ui.theme_menu && rows, "the top bar's theme button opens the Theme menu with every theme".into())?;
    s.frames_with(2, vec![key(egui::Key::Escape, egui::Modifiers::NONE)]);
    let escaped = !s.ed.ui.theme_menu;
    s.ui_click("theme")?;
    let voxels = s.voxels();
    let mid = egui::pos2(s.ed.width as f32 / s.ed.ppp / 2.0, s.ed.height as f32 / s.ed.ppp / 2.0);
    s.ui_drag(mid, mid, egui::Modifiers::NONE);
    check(
        escaped && !s.ed.ui.theme_menu && s.voxels() == voxels,
        "Escape closes it, and so does a click outside it, which edits nothing".into(),
    )?;
    if s.voxels() != voxels {
        s.ed.undo();
    }

    // Each theme from the menu, with a colour selected, as the evidence screenshots.
    s.tool(Tool::Select, Shape::Voxel);
    let mid_px = [s.ed.width as f32 / 2.0, s.ed.height as f32 / 2.0];
    if s.voxels() > 0 {
        let p = s.top_face_pixel(mid_px)?;
        s.stroke(p, &[]);
    }
    let palette = s.ed.scene.palette;
    let mut seen = Vec::new();
    for choice in ThemeChoice::ALL.into_iter().skip(1) {
        s.host.clear();
        if !s.ed.ui.theme_menu {
            s.ui_click("theme")?;
        }
        s.ui_click(&format!("theme-{}", choice.id()))?;
        s.frames(3);
        let t = choice.resolve(true);
        let bg = corner(&mut s);
        check(
            s.ed.ui.theme == choice
                && s.host.contains(&Action::Theme)
                && s.ed.ui.theme_menu
                && s.ed.renderer.colors == t.view
                && s.ed.scene.palette == palette
                && !seen.contains(&bg),
            format!("Theme menu: {} recolours the view ({bg:?} at its foot) and keeps the voxels' colours", t.name),
        )?;
        seen.push(bg);
        s.ed.pointer_moved(None);
        s.frames(2);
        s.save_png(&dir.join(format!("theme-{}.png", choice.id())))?;
    }
    s.frames_with(2, vec![key(egui::Key::Escape, egui::Modifiers::NONE)]);

    // Cmd-K picks one too, and System from there follows the system again.
    s.host.clear();
    s.frames_with(2, vec![key(egui::Key::K, egui::Modifiers::COMMAND)]);
    s.frames_with(1, vec![egui::Event::Text("theme: tokyo".into())]);
    s.frames_with(3, vec![key(egui::Key::Enter, egui::Modifiers::NONE)]);
    check(
        s.ed.ui.theme == ThemeChoice::TokyoNight && s.host.contains(&Action::Theme),
        "Cmd-K Theme: Tokyo Night picks it and asks the host to keep it".into(),
    )?;
    s.system = Some(egui::Theme::Light);
    s.frames(3);
    check(s.ed.ui.shown_theme(&s.ed.ctx).name == "Tokyo Night", "a picked theme ignores the system's setting".into())?;
    s.frames_with(2, vec![key(egui::Key::K, egui::Modifiers::COMMAND)]);
    s.frames_with(1, vec![egui::Event::Text("theme: system".into())]);
    s.frames_with(3, vec![key(egui::Key::Enter, egui::Modifiers::NONE)]);
    check(
        s.ed.ui.theme == ThemeChoice::System && s.ed.renderer.colors == vxr_app::theme::PAPER.view,
        "Cmd-K Theme: System goes back to following it (Paper while light)".into(),
    )?;

    // A Render PNG export keeps Neutral's dark backdrop even while the light Paper theme shows.
    s.ed.ui.render = vxr_app::render::RenderSettings { spp: 0, size: 0, ..Default::default() };
    s.frames_with(2, vec![key(egui::Key::K, egui::Modifiers::COMMAND)]);
    s.frames_with(1, vec![egui::Event::Text("export png".into())]);
    s.frames_with(2, vec![key(egui::Key::Enter, egui::Modifiers::NONE)]);
    let mut n = 0;
    while s.ed.exported.is_none() && n < 3000 {
        s.frames(1);
        let _ = s.ed.gpu.device.poll(wgpu::PollType::wait_indefinitely());
        n += 1;
    }
    let Some((png, _)) = s.ed.exported.take() else {
        return check(false, format!("the themed export did not finish in {n} frames"));
    };
    let mut r = png::Decoder::new(std::io::Cursor::new(&png)).read_info().map_err(|e| e.to_string())?;
    let mut buf = vec![0; r.output_buffer_size().unwrap_or(0)];
    let info = r.next_frame(&mut buf).map_err(|e| e.to_string())?;
    let px = |x: u32, y: u32| {
        let i = ((y * info.width + x) * 4) as usize;
        [buf[i], buf[i + 1], buf[i + 2]]
    };
    let corners = [px(1, 1), px(info.width - 2, 1)];
    check(
        s.ed.ui.shown_theme(&s.ed.ctx).name == "Paper"
            && corners
                .iter()
                .all(|c| c.iter().all(|&v| v < 40) && c.iter().max().unwrap() - c.iter().min().unwrap() <= 2),
        format!("Render PNG under Paper keeps Neutral's dark grey backdrop: top corners {corners:?}"),
    )?;
    std::fs::write(dir.join("theme-paper-export.png"), &png).map_err(|e| e.to_string())?;
    Ok(())
}

/// Cells within the circle or sphere brush's reach (r^2 + r/2, see `Gesture::draw`) at or
/// above the centre's layer: `dims` 2 for a circle, 3 for a ball cut by the floor.
fn round_cells(r: i32, dims: usize) -> u64 {
    let r2 = (r * r) as f32 + r as f32 / 2.0;
    let mut n = 0;
    for a in -r..=r {
        for b in -r..=r {
            for c in if dims == 3 { 0..=r } else { 0..=0 } {
                n += u64::from(((a * a + b * b + c * c) as f32) <= r2);
            }
        }
    }
    n
}

/// A new scene saved and reopened as Save and Open do it: a box across the origin stays
/// one object, which opens in model mode with every voxel where it was.
fn new_scene_save(dir: &Path) -> Result<(), String> {
    use vxr_app::editor::tree_rows;
    use vxr_core::history::At;
    let mut s = Shot::new(Scene::new(NEW_SIZE), "Untitled", 2.0)?;
    s.frames(3);
    s.ed.ui.coach = false;
    s.tool(Tool::Add, Shape::Box);
    // A box on the ground from 10 cells before the world origin to 10 after it.
    let o = s.ed.scene.to_map([0, 0, 0]).map(|c| c as f32);
    let a = project(&s.ed, [o[0] - 9.5, 0.0, o[2] - 9.5]);
    let b = project(&s.ed, [o[0] + 9.5, 0.0, o[2] + 9.5]);
    s.stroke(a, &[[(b[0] - a[0]) / 2.0, (b[1] - a[1]) / 2.0], [b[0] - a[0], b[1] - a[1]]]);
    s.ed.place_box();
    let world = s.ed.scene.world_voxels(false);
    let (mut lo, mut hi) = ([i32::MAX; 3], [i32::MIN; 3]);
    for p in world.keys() {
        for a in 0..3 {
            (lo[a], hi[a]) = (lo[a].min(p[a]), hi[a].max(p[a]));
        }
    }
    check(
        lo[0] < 0 && hi[0] >= 0 && lo[2] < 0 && hi[2] >= 0,
        format!("a box across the origin on a new scene: {} voxels, {lo:?} to {hi:?}", world.len()),
    )?;
    let path = dir.join("new.vox");
    std::fs::write(&path, s.ed.scene.to_vox()).map_err(|e| e.to_string())?;
    let (reopened, name) = crate::native::open(&path)?;
    let objects = reopened.world().map_or(0, |w| w.objects().len());
    let same = reopened.world_voxels(false) == world;
    s.ed.load(reopened, &name)?;
    s.frames(2);
    check(
        objects == 1 && matches!(s.ed.scene.at(), At::Object(_)) && same && tree_rows(&s.ed.scene).is_empty(),
        format!("the saved new scene reopens in model mode, every voxel in place ({objects} object(s))"),
    )
}

/// New scene (Cmd-N, Cmd-K, the top bar's New): with nothing unsaved it starts a blank scene
/// at once; after an edit (or an undo past a save) it asks first, the confirm takes the keys
/// and the pointer, Cancel, Escape and an Open keep the scene, Start new and Enter replace it
/// with what launching with no file gives, and a save of it is one model.
fn new_scene_command(dir: &Path) -> Result<(), String> {
    use egui::{Event, Key, Modifiers};
    use vxr_core::history::At;
    let mut s = Shot::new(Scene::new(NEW_SIZE), "Untitled", 2.0)?;
    s.frames(3);
    s.ed.ui.coach = false;
    s.tool(Tool::Add, Shape::Voxel);
    let key = |key, modifiers| Event::Key { key, physical_key: None, pressed: true, repeat: false, modifiers };
    let fresh = format!("{:?}", s.ed.cam);
    let mid = [s.ed.width as f32 / 2.0, s.ed.height as f32 / 2.0];

    s.frames_with(2, vec![key(Key::N, Modifiers::COMMAND)]);
    check(
        s.host.contains(&Action::New) && !s.ed.ui.confirm_new && s.ed.ui.scene_name == "Untitled",
        "New scene with no edits starts at once, no confirm".into(),
    )?;
    s.host.clear();

    // An edit, then New: the confirm shows and nothing is lost yet.
    s.stroke(mid, &[]);
    let edited = s.voxels();
    s.ed.cam.orbit(0.4, 0.2);
    s.frames_with(2, vec![key(Key::N, Modifiers::COMMAND)]);
    check(
        edited > 0 && s.ed.ui.confirm_new && s.host.is_empty() && s.voxels() == edited,
        format!("New scene after an edit ({edited} voxel) asks first and keeps the scene"),
    )?;
    s.save_png(&dir.join("app-2x-new.png"))?;

    // The pointer waits for an answer too: the top bar's Open and the view are behind it.
    s.ui_click("open")?;
    let view = egui::pos2(mid[0] / s.ed.ppp, mid[1] / s.ed.ppp + 120.0);
    s.ui_drag(view, view + egui::vec2(40.0, 0.0), Modifiers::NONE);
    check(
        s.ed.ui.confirm_new && s.host.is_empty() && s.ed.ctx.is_pointer_over_egui(),
        "the confirm takes the pointer (the top bar's Open and the view do nothing)".into(),
    )?;
    s.ui_click("new-cancel")?;
    check(!s.ed.ui.confirm_new && s.voxels() == edited && s.host.is_empty(), "Cancel keeps the scene".into())?;
    s.frames_with(2, vec![key(Key::N, Modifiers::COMMAND)]);
    s.frames_with(2, vec![key(Key::Escape, Modifiers::NONE)]);
    check(!s.ed.ui.confirm_new && s.voxels() == edited && s.host.is_empty(), "Escape keeps the scene".into())?;

    // Keys do nothing behind the confirm; Start new clears the scene.
    s.frames_with(2, vec![key(Key::N, Modifiers::COMMAND)]);
    s.frames_with(2, vec![key(Key::Z, Modifiers::COMMAND)]);
    check(s.ed.ui.confirm_new && s.voxels() == edited, "the confirm takes the keys (Cmd-Z does nothing)".into())?;
    s.ui_click("new-discard")?;
    check(
        !s.ed.ui.confirm_new
            && s.voxels() == 0
            && s.host.contains(&Action::New)
            && !s.ed.scene.history().can_undo()
            && s.ed.scene.selected_count() == 0
            && s.ed.path.is_none()
            && s.ed.ui.scene_name == "Untitled"
            && s.ed.scene.bounds().size == NEW_SIZE
            && format!("{:?}", s.ed.cam) == fresh,
        "Start new gives a blank scene: default size and camera, no history, selection or path".into(),
    )?;
    s.host.clear();

    // Cmd-K lists it, and Enter starts the confirm; so does the top bar's New.
    s.stroke(mid, &[]);
    s.frames_with(2, vec![key(Key::K, Modifiers::COMMAND)]);
    s.frames_with(1, vec![Event::Text("new scene".into())]);
    s.frames_with(2, vec![key(Key::Enter, Modifiers::NONE)]);
    check(s.ed.ui.confirm_new && s.ed.ui.cmdk.is_none(), "Cmd-K New scene asks first".into())?;
    s.frames_with(2, vec![key(Key::Enter, Modifiers::NONE)]);
    check(
        !s.ed.ui.confirm_new && s.voxels() == 0 && s.host.contains(&Action::New),
        "Enter starts the new scene".into(),
    )?;
    s.host.clear();
    s.stroke(mid, &[]);
    s.ui_click("new")?;
    check(s.ed.ui.confirm_new, "the top bar's New asks first".into())?;
    s.ed.load(Scene::new(NEW_SIZE), "opened.vox")?;
    s.frames(2);
    check(!s.ed.ui.confirm_new && s.host.is_empty(), "Open behind the confirm closes it".into())?;

    // Saved edits are not lost: New starts at once. An undo past the save is, so it asks.
    let path = dir.join("before-new.vox");
    s.stroke(mid, &[]);
    std::fs::write(&path, s.ed.scene.to_vox()).map_err(|e| e.to_string())?;
    s.ed.mark_saved();
    s.frames_with(2, vec![key(Key::N, Modifiers::COMMAND)]);
    check(
        !s.ed.ui.confirm_new && s.host.contains(&Action::New) && s.voxels() == 0,
        "New scene after an edit and a save starts at once".into(),
    )?;
    s.host.clear();
    s.stroke(mid, &[]);
    std::fs::write(&path, s.ed.scene.to_vox()).map_err(|e| e.to_string())?;
    s.ed.mark_saved();
    s.frames_with(2, vec![key(Key::Z, Modifiers::COMMAND)]);
    s.frames_with(2, vec![key(Key::N, Modifiers::COMMAND)]);
    check(
        s.ed.ui.confirm_new && s.host.is_empty() && s.voxels() == 0,
        "New scene after a save and an undo asks first".into(),
    )?;
    s.ui_click("new-discard")?;

    // Save after New: one model.
    s.stroke(mid, &[]);
    let voxels = s.voxels();
    let path = dir.join("after-new.vox");
    std::fs::write(&path, s.ed.scene.to_vox()).map_err(|e| e.to_string())?;
    let (reopened, _) = crate::native::open(&path)?;
    let objects = reopened.world().map_or(0, |w| w.objects().len());
    check(
        voxels > 0 && objects == 1 && matches!(reopened.at(), At::Object(_)) && reopened.map.voxels == voxels,
        format!("Save after New writes one model ({objects} object, {voxels} voxel)"),
    )
}

/// Line and center brushes, mirror on X, Y and Z and the axis lock, on an empty scene so
/// every count is exact. Drives the keys and the rail and flyout buttons through egui.
fn brushes(dir: &Path) -> Result<(), String> {
    let mut s = Shot::new(Scene::new(NEW_SIZE), "Untitled", 2.0)?;
    s.frames(3);
    let mid = [s.ed.width as f32 / 2.0, s.ed.height as f32 / 2.0];
    let key = |key| egui::Event::Key {
        key,
        physical_key: None,
        pressed: true,
        repeat: false,
        modifiers: egui::Modifiers::NONE,
    };
    s.ed.ui.tool = Tool::Add;
    s.ed.ui.coach = false;

    // L, then a drag that shows the line live and shrinks it again before the release.
    s.frames_with(2, vec![key(egui::Key::L)]);
    s.ed.pointer_moved(Some(mid));
    s.ed.button(Button::Left, true, false);
    s.ed.pointer_moved(Some([mid[0] + 240.0, mid[1] + 60.0]));
    let long = s.voxels();
    s.ed.pointer_moved(Some([mid[0] + 160.0, mid[1] + 40.0]));
    let span = s.ed.span();
    let n = s.voxels();
    s.ed.button(Button::Left, false, false);
    check(
        s.ed.ui.shape == Shape::Line && long > n && n > 3 && span == Some(("LINE", n.to_string())) && s.voxels() == n,
        format!("L line drag: shown live ({long} voxels, then {n} after moving back), {span:?}"),
    )?;
    s.ed.undo();
    check(s.voxels() == 0, "line: one undo step".into())?;

    // C: a circle from its centre; C again: a sphere, cut by the floor.
    s.frames_with(2, vec![key(egui::Key::C)]);
    s.stroke(mid, &[[40.0, 0.0], [90.0, 0.0]]);
    let r_circle = s.voxels();
    s.ed.undo();
    s.ed.pointer_moved(Some(mid));
    s.ed.button(Button::Left, true, false);
    s.ed.pointer_moved(Some([mid[0] + 90.0, mid[1]]));
    let r: i32 = s.ed.span().and_then(|(_, v)| v.parse().ok()).unwrap_or(0);
    s.ed.button(Button::Left, false, false);
    check(
        s.ed.ui.shape == Shape::Circle && r >= 2 && r_circle == round_cells(r, 2),
        format!("C circle drag: radius {r}, {r_circle} voxels on one layer"),
    )?;
    s.ed.undo();
    s.frames_with(2, vec![key(egui::Key::C)]);
    s.stroke(mid, &[[90.0, 0.0]]);
    let ball = s.voxels();
    check(
        s.ed.ui.shape == Shape::Sphere && ball == round_cells(r, 3),
        format!("C again: sphere drag of radius {r}, {ball} voxels above the floor"),
    )?;

    // Mirror through the rail button and the flyout's axis chips: one click adds 8 voxels.
    s.ed.ui.shape = Shape::Voxel;
    s.ui_click("mirror")?;
    s.ui_click("mirror-y")?;
    s.ui_click("mirror-z")?;
    let v = s.voxels();
    s.stroke([mid[0] - 220.0, mid[1] + 80.0], &[]);
    check(
        s.ed.ui.mirror_axes() == [true; 3] && s.voxels() == v + 8,
        format!("mirror X, Y and Z from the flyout: one click adds {} voxels", s.voxels() - v),
    )?;
    s.ui_click("mirror-x")?;
    s.ui_click("mirror-y")?;
    check(s.ed.ui.mirror_axes() == [false, false, true], "flyout chips toggle single axes".into())?;
    s.ui_click("mirror-z")?;
    check(!s.ed.ui.mirror && !s.ed.ui.hits.contains_key("mirror-x"), "the last axis off turns mirror off".into())?;

    // K locks X: a box drag on the ground builds a wall across x instead of a floor tile.
    s.frames_with(2, vec![key(egui::Key::K)]);
    s.frames(1);
    s.ed.ui.shape = Shape::Box;
    s.ed.undo();
    s.ed.undo();
    s.stroke([mid[0] + 40.0, mid[1] + 40.0], &[[-60.0, -60.0], [-120.0, -100.0]]);
    let (mut lo, mut hi) = ([i32::MAX; 3], [i32::MIN; 3]);
    s.ed.scene.for_each_voxel(|p, _| {
        for a in 0..3 {
            (lo[a], hi[a]) = (lo[a].min(p[a]), hi[a].max(p[a]));
        }
    });
    let b = (lo, hi);
    s.ed.place_box();
    check(
        s.ed.ui.lock == Some(0) && s.ed.ui.hits.contains_key("lock-x") && lo[0] == hi[0] && hi[1] > lo[1],
        format!("K locks X: a box drag builds a wall across x ({b:?})"),
    )?;
    s.ed.undo();
    let k = |key, modifiers| egui::Event::Key { key, physical_key: None, pressed: true, repeat: false, modifiers };
    s.frames_with(2, vec![k(egui::Key::K, egui::Modifiers::COMMAND)]);
    s.frames_with(1, vec![egui::Event::Text("lock axis z".into())]);
    s.frames_with(2, vec![k(egui::Key::Enter, egui::Modifiers::NONE)]);
    check(s.ed.ui.lock == Some(2), "Cmd-K 'Lock axis Z'".into())?;

    // A scene for the screenshot: a sphere, a mirrored line and the flyout open.
    s.ed.ui.lock = None;
    s.ed.ui.shape = Shape::Sphere;
    s.stroke(mid, &[[70.0, 0.0]]);
    s.ed.ui.shape = Shape::Line;
    s.ed.ui.mirror = true;
    s.ed.ui.mirror_on = [true, false, true];
    s.ed.ui.slot = 12;
    s.stroke([mid[0] - 300.0, mid[1] + 120.0], &[[200.0, -40.0]]);
    s.ed.ui.lock = Some(1);
    s.ed.pointer_moved(Some([mid[0] + 300.0, mid[1] + 100.0]));
    s.frames(4);
    s.save_png(&dir.join("app-2x-brushes.png"))
}

/// The voxel brush's size: V again opens the size flyout, its buttons, - and =, Cmd-K, a
/// cube added on the ground and one erased into a face, each one undo step.
fn brush_size(dir: &Path) -> Result<(), String> {
    let mut s = Shot::new(Scene::new(NEW_SIZE), "Untitled", 2.0)?;
    s.frames(3);
    let mid = [s.ed.width as f32 / 2.0, s.ed.height as f32 / 2.0];
    let k = |key, modifiers| egui::Event::Key { key, physical_key: None, pressed: true, repeat: false, modifiers };
    let none = egui::Modifiers::NONE;
    s.ed.ui.coach = false;
    s.tool(Tool::Add, Shape::Box);
    s.frames_with(2, vec![k(egui::Key::V, none)]);
    let closed = !s.ed.ui.hits.contains_key("size-plus");
    s.frames_with(2, vec![k(egui::Key::V, none)]);
    check(
        s.ed.ui.shape == Shape::Voxel && closed && s.ed.ui.hits.contains_key("size-plus"),
        "V picks the voxel brush, V again opens its size flyout".into(),
    )?;
    s.ui_click("size-plus")?;
    s.ui_click("size-plus")?;
    check(s.ed.ui.size == 3, format!("the flyout's + steps the size to {}", s.ed.ui.size))?;
    s.frames_with(2, vec![k(egui::Key::Minus, none)]);
    let down = s.ed.ui.size;
    s.frames_with(2, vec![k(egui::Key::Equals, none)]);
    check(down == 2 && s.ed.ui.size == 3, format!("- and = step it ({down}, then {})", s.ed.ui.size))?;
    s.frames_with(2, vec![k(egui::Key::K, egui::Modifiers::COMMAND)]);
    s.frames_with(1, vec![egui::Event::Text("smaller brush".into())]);
    s.frames_with(2, vec![k(egui::Key::Enter, none)]);
    let cmdk = s.ed.ui.size;
    s.frames_with(2, vec![k(egui::Key::Equals, none)]);
    check(cmdk == 2 && s.ed.ui.size == 3, "Cmd-K 'Smaller brush'".into())?;

    // Size 3 on the ground: a 3x3x3 cube standing on it, one undo step.
    s.stroke(mid, &[]);
    let (mut lo, mut hi) = ([i32::MAX; 3], [i32::MIN; 3]);
    s.ed.scene.for_each_voxel(|p, _| {
        for a in 0..3 {
            (lo[a], hi[a]) = (lo[a].min(p[a]), hi[a].max(p[a]));
        }
    });
    let cube = (0..3).all(|a| hi[a] - lo[a] == 2) && lo[1] == 0;
    check(s.voxels() == 27 && cube, format!("a size 3 click on the ground adds a 3x3x3 cube ({lo:?} to {hi:?})"))?;
    s.ed.undo();
    let undone = s.voxels();
    s.ed.redo();
    check(undone == 0 && s.voxels() == 27, "one undo step, and redo".into())?;

    // Size 2 erase into the cube's top face takes the 2x2x2 cells there that are filled.
    s.frames_with(2, vec![k(egui::Key::Minus, none)]);
    s.ed.ui.tool = Tool::Erase;
    // The middle of the top face of the cube's middle column: x and z reach one cell further
    // on the high side, so all 8 cells are inside the cube.
    let c = [lo[0] + 1, hi[1], lo[2] + 1];
    let at = project(&s.ed, [c[0] as f32 + 0.5, c[1] as f32 + 1.0, c[2] as f32 + 0.5]);
    s.ed.pointer_moved(Some(at));
    let aimed = s.ed.cursor().is_some_and(|t| t.voxel == c && t.normal == [0, 1, 0]);
    s.stroke(at, &[]);
    let gone = |x: i32, y: i32, z: i32| s.ed.scene.map.get(x, y, z) == 0;
    check(
        aimed && s.voxels() == 27 - 8 && gone(c[0] + 1, c[1] - 1, c[2] + 1) && !gone(c[0] - 1, c[1], c[2]),
        format!("a size 2 erase on the top face takes the 2x2x2 cells under it ({} left)", s.voxels()),
    )?;
    s.ed.undo();
    check(s.voxels() == 27, "erase: one undo step".into())?;

    // The screenshot: the cube with the flyout open at size 3.
    s.ed.ui.tool = Tool::Add;
    s.frames_with(2, vec![k(egui::Key::Equals, none)]);
    s.ed.pointer_moved(Some([mid[0] + 200.0, mid[1] + 60.0]));
    s.frames(4);
    s.save_png(&dir.join("app-2x-size.png"))?;

    // - back to 1 (- stops there), then Escape closes the flyout.
    s.ui_click("size-minus")?;
    s.ui_click("size-minus")?;
    s.frames_with(2, vec![k(egui::Key::Minus, none)]);
    let open = s.ed.ui.hits.contains_key("size-plus");
    s.frames_with(2, vec![k(egui::Key::Escape, none)]);
    check(
        s.ed.ui.size == 1 && open && !s.ed.ui.hits.contains_key("size-plus"),
        "- steps back to 1 and stops; Escape closes the flyout".into(),
    )
}

/// The bounds of every voxel, inclusive, and whether they fill that box exactly.
fn solid_box(s: &Shot) -> ([i32; 3], [i32; 3], bool) {
    let (mut lo, mut hi) = ([i32::MAX; 3], [i32::MIN; 3]);
    s.ed.scene.for_each_voxel(|p, _| {
        for a in 0..3 {
            (lo[a], hi[a]) = (lo[a].min(p[a]), hi[a].max(p[a]));
        }
    });
    let cells: i64 = (0..3).map(|a| (hi[a] - lo[a] + 1).max(0) as i64).product();
    (lo, hi, s.voxels() > 0 && cells == s.voxels() as i64)
}

/// The box brush's height: a drag draws the base, then the pointer lifts it live until a
/// click, Enter or Cmd-K places it (one undo step with its base) or Escape drops it; erase
/// pushes a base down into a box.
fn box_height(dir: &Path) -> Result<(), String> {
    let mut s = Shot::new(Scene::new(NEW_SIZE), "Untitled", 2.0)?;
    s.frames(3);
    let mid = [s.ed.width as f32 / 2.0, s.ed.height as f32 / 2.0];
    let k = |key, modifiers| egui::Event::Key { key, physical_key: None, pressed: true, repeat: false, modifiers };
    let none = egui::Modifiers::NONE;
    s.ed.ui.coach = false;
    s.tool(Tool::Add, Shape::Voxel);
    s.frames_with(2, vec![k(egui::Key::B, none)]);
    let end = [mid[0] + 160.0, mid[1] + 40.0];
    s.stroke(mid, &[[80.0, 20.0], [160.0, 40.0]]);
    s.frames(1);
    let (lo, hi, solid) = solid_box(&s);
    let base = s.voxels();
    check(
        s.ed.ui.shape == Shape::Box && s.ed.ui.lifting && solid && lo[1] == 0 && hi[1] == 0 && base > 4,
        format!("B: a drag draws the base and lets go into its height ({base} voxels, {lo:?} to {hi:?})"),
    )?;
    s.ed.pointer_moved(Some([end[0], end[1] - 160.0]));
    s.frames(1);
    let (lo2, hi2, solid) = solid_box(&s);
    let high = hi2[1] + 1;
    let span = s.ed.span().map(|(_, v)| v).unwrap_or_default();
    check(
        solid
            && lo2 == lo
            && high > 1
            && s.voxels() == base * high as u64
            && span.split('×').nth(1) == Some(&*high.to_string()),
        format!("moving up lifts it live: {high} layers, {} voxels, BOX {span}", s.voxels()),
    )?;
    s.save_png(&dir.join("app-2x-box.png"))?;
    s.ed.pointer_moved(Some(end));
    check(s.voxels() == base, format!("back down to where it let go: one layer ({})", s.voxels()))?;
    s.ed.pointer_moved(Some([end[0], end[1] - 160.0]));
    s.stroke([end[0], end[1] - 160.0], &[]);
    let placed = s.voxels();
    check(!s.ed.lifting() && placed == base * high as u64, format!("a click places it ({placed} voxels)"))?;
    s.ed.undo();
    let undone = s.voxels();
    s.ed.redo();
    check(undone == 0 && s.voxels() == placed, "base and height are one undo step, and redo".into())?;

    // Escape drops a box, base and all; Enter and Cmd-K 'Place box' place one.
    let beside = [mid[0] - 300.0, mid[1] + 160.0];
    s.stroke(beside, &[[60.0, 20.0]]);
    s.ed.pointer_moved(Some([beside[0] + 60.0, beside[1] - 100.0]));
    s.frames(1);
    let lifted = s.voxels() > placed;
    s.frames_with(2, vec![k(egui::Key::Escape, none)]);
    s.ed.undo();
    check(
        lifted && !s.ed.lifting() && s.voxels() == 0,
        format!("Escape drops a lifted box, leaving no undo step ({} after one undo)", s.voxels()),
    )?;
    s.ed.redo();
    s.stroke(beside, &[[60.0, 20.0]]);
    s.ed.pointer_moved(Some([beside[0] + 60.0, beside[1] - 100.0]));
    s.frames_with(2, vec![k(egui::Key::Enter, none)]);
    let entered = s.voxels();
    s.ed.undo();
    check(
        entered > placed && s.voxels() == placed && !s.ed.lifting(),
        format!("Enter places it, one undo step ({entered} voxels)"),
    )?;
    s.stroke(beside, &[[60.0, 20.0]]);
    s.frames_with(2, vec![k(egui::Key::K, egui::Modifiers::COMMAND)]);
    s.frames_with(1, vec![egui::Event::Text("place box".into())]);
    s.frames_with(2, vec![k(egui::Key::Enter, none)]);
    let flat = s.voxels() - placed;
    s.ed.undo();
    check(
        !s.ed.lifting() && flat > 0 && s.voxels() == placed,
        format!("Cmd-K 'Place box' places it one layer high ({flat} voxels)"),
    )?;

    // Erase: a base on the placed box's top, pushed down to the ground, clears that column.
    s.ed.ui.tool = Tool::Erase;
    let (lo, hi, _) = solid_box(&s);
    let a = [lo[0] + 1, hi[1], lo[2]];
    let b = [lo[0] + 2, hi[1], lo[2] + 1];
    let at = |c: [i32; 3]| [c[0] as f32 + 0.5, c[1] as f32 + 1.0, c[2] as f32 + 0.5];
    // The far corner is aimed at its cell's middle, where the drag plane crosses it.
    let pa = project(&s.ed, at(a));
    let pb = project(&s.ed, [b[0] as f32 + 0.5, b[1] as f32 + 0.5, b[2] as f32 + 0.5]);
    s.stroke(pa, &[[pb[0] - pa[0], pb[1] - pa[1]]]);
    let top_only = placed - s.voxels();
    let floor = project(&s.ed, [b[0] as f32 + 0.5, 0.2, b[2] as f32 + 0.5]);
    s.ed.pointer_moved(Some(floor));
    s.stroke(floor, &[]);
    let gone = placed - s.voxels();
    let hole = (0..=hi[1]).all(|y| s.ed.scene.map.get(a[0], y, a[2]) == 0 && s.ed.scene.map.get(b[0], y, b[2]) == 0);
    check(
        top_only == 4 && gone == 4 * (hi[1] + 1) as u64 && hole,
        format!("erase: a 2x2 base on top ({top_only}), pushed down, clears the {gone} cells under it"),
    )?;
    s.ed.undo();
    check(s.voxels() == placed, "erase: one undo step".into())
}

/// Where a point in map space lands on screen, in pixels.
fn project(ed: &Editor, p: [f32; 3]) -> [f32; 2] {
    let (w, h) = (ed.width as f32, ed.height as f32);
    let c = ed.cam.view_proj(w / h) * glam::Vec4::new(p[0], p[1], p[2], 1.0);
    [(c.x / c.w + 1.0) * 0.5 * w, (1.0 - c.y / c.w) * 0.5 * h]
}

/// Selection and transform on an empty scene with two blocks, so every count is exact: S,
/// colour click with Shift and Alt, Escape, box select, a live move drag, the flyout's
/// rotate, flip and loop, copy and paste at the pointer, fill, delete, Cmd-A, arrows, cut,
/// and a whole-model flip, all through the pointer path, keys and egui clicks.
fn selection(dir: &Path) -> Result<(), String> {
    let mut s = Shot::new(Scene::new(NEW_SIZE), "Untitled", 2.0)?;
    s.ed.ui.coach = false;
    // Block A: 4 x 2 x 3 of slot 12 with one marker voxel of slot 13 in a top corner, so
    // flips and loops show. Block B: 2 x 2 x 2 of slot 40, 12 cells along +x.
    let mut cells = Vec::new();
    for (x, y, z) in (50..54).flat_map(|x| (0..2).flat_map(move |y| (60..63).map(move |z| (x, y, z)))) {
        cells.push(([x, y, z], if [x, y, z] == [50, 1, 60] { 13 } else { 12 }));
    }
    for (x, y, z) in (66..68).flat_map(|x| (0..2).flat_map(move |y| (60..62).map(move |z| (x, y, z)))) {
        cells.push(([x, y, z], 40));
    }
    s.ed.scene.edit(cells);
    s.ed.frame_scene();
    s.frames(3);
    let key = |key, modifiers| egui::Event::Key { key, physical_key: None, pressed: true, repeat: false, modifiers };
    let none = egui::Modifiers::NONE;
    let count = |s: &Shot| s.ed.scene.selected_count();
    let bounds = |s: &Shot| s.ed.scene.selection_bounds().map(|(lo, hi)| (lo, [0, 1, 2].map(|a| hi[a] - lo[a] + 1)));

    s.frames_with(2, vec![key(egui::Key::S, none)]);
    check(
        s.ed.ui.tool == Tool::Select && s.ed.ui.hits.contains_key("rotate-y"),
        "S: select tool, selection flyout shown".into(),
    )?;
    let a_top = project(&s.ed, [51.5, 1.99, 61.5]);
    let b_top = project(&s.ed, [66.5, 1.99, 60.5]);
    s.stroke(a_top, &[]);
    let n_a = count(&s);
    s.hold(egui::Modifiers::SHIFT);
    s.stroke(b_top, &[]);
    let n_ab = count(&s);
    s.hold(egui::Modifiers::ALT);
    s.stroke(b_top, &[]);
    let n_alt = count(&s);
    s.hold(none);
    check(
        n_a == 23 && n_ab == 31 && n_alt == 23,
        format!("click selects a colour ({n_a}), Shift adds ({n_ab}), Alt subtracts ({n_alt})"),
    )?;
    s.frames_with(2, vec![key(egui::Key::Escape, none)]);
    check(count(&s) == 0, "Escape deselects".into())?;

    // Box select around block A only.
    let corners = [
        project(&s.ed, [49.0, 2.5, 59.0]),
        project(&s.ed, [55.0, 2.5, 59.0]),
        project(&s.ed, [49.0, -0.5, 64.0]),
        project(&s.ed, [55.0, -0.5, 64.0]),
    ];
    let x0 = corners.iter().map(|c| c[0]).fold(f32::MAX, f32::min);
    let x1 = corners.iter().map(|c| c[0]).fold(f32::MIN, f32::max);
    let y0 = corners.iter().map(|c| c[1]).fold(f32::MAX, f32::min);
    let y1 = corners.iter().map(|c| c[1]).fold(f32::MIN, f32::max);
    s.ed.pointer_moved(Some([x0, y0]));
    s.ed.button(Button::Left, true, false);
    s.ed.pointer_moved(Some([(x0 + x1) / 2.0, (y0 + y1) / 2.0]));
    s.ed.pointer_moved(Some([x1, y1]));
    s.frames(1);
    let shown = s.ed.ui.marquee.is_some();
    s.ed.button(Button::Left, false, false);
    check(shown && count(&s) == 24, format!("box select: the box shows while dragging, {} selected", count(&s)))?;

    // Drag the selection 3 cells along +x on its top layer: live, then one undo step.
    let v = s.voxels();
    s.ed.pointer_moved(Some(a_top));
    s.ed.button(Button::Left, true, false);
    s.ed.pointer_moved(Some(project(&s.ed, [53.5, 1.5, 61.5])));
    s.ed.pointer_moved(Some(project(&s.ed, [54.5, 1.5, 61.5])));
    let live = (s.ed.span(), s.ed.scene.map.get(56, 0, 60));
    s.ed.button(Button::Left, false, false);
    let moved = bounds(&s);
    check(
        live == (Some(("MOVE", "3,0,0".into())), 12) && s.voxels() == v && moved == Some(([53, 0, 60], [4, 2, 3])),
        format!("move drag: live {live:?}, selection now at {moved:?}"),
    )?;
    s.ed.undo();
    check(bounds(&s) == Some(([50, 0, 60], [4, 2, 3])), "move: one undo step, selection back".into())?;

    // The flyout: rotate Y turns 4 x 2 x 3 to 3 x 2 x 4 and takes the marker from
    // (50, 1, 60) to (50, 1, 63); flip X takes it to (52, 1, 63), loop Z wraps it to (52, 1, 60).
    let marker = |s: &Shot, p: [i32; 3]| s.ed.scene.map.get(p[0], p[1], p[2]) == 13;
    s.ui_click("rotate-y")?;
    let r = bounds(&s);
    let turned = marker(&s, [50, 1, 63]);
    s.ui_click("flip-x")?;
    let flipped = marker(&s, [52, 1, 63]);
    s.ui_click("loop-z")?;
    let looped = marker(&s, [52, 1, 60]);
    check(
        r.map(|b| b.1) == Some([3, 2, 4]) && bounds(&s) == r && turned && flipped && looped && s.voxels() == v,
        format!("flyout rotate Y {r:?}, flip X and loop Z move the marker: {turned} {flipped} {looped}"),
    )?;
    for _ in 0..3 {
        s.ed.undo();
    }

    // Copy (the native clipboard event), then Cmd-V with the pointer over empty ground.
    s.frames_with(2, vec![egui::Event::Copy]);
    let ground = project(&s.ed, [52.5, 0.0, 75.5]);
    s.ed.pointer_moved(Some(ground));
    s.frames_with(2, vec![key(egui::Key::V, egui::Modifiers::COMMAND)]);
    let pasted = bounds(&s);
    check(
        s.voxels() == v + 24 && count(&s) == 24 && pasted.is_some_and(|b| b.0[2] >= 70 && b.0[1] == 0),
        format!("copy, then Cmd-V pastes at the pointer: {pasted:?}"),
    )?;
    s.ed.ui.slot = 5;
    s.ui_click("fill")?;
    check(s.ed.scene.map.counts[5] == 24, "fill paints the selection with the current colour".into())?;
    s.frames_with(2, vec![key(egui::Key::Backspace, none)]);
    check(s.voxels() == v && count(&s) == 0, "Backspace deletes the selection".into())?;

    s.frames_with(2, vec![key(egui::Key::A, egui::Modifiers::COMMAND)]);
    let all = count(&s);
    s.ui_click("deselect")?;
    check(all == 32 && count(&s) == 0, format!("Cmd-A selects all ({all}); the flyout's x deselects"))?;

    // Invert: nothing selected inverts to every voxel, a colour's voxels to the rest, and
    // the flyout's All selects every voxel again. None of it is an undo step.
    let saved = s.ed.scene.to_vox();
    s.ui_click("invert")?;
    let whole = count(&s);
    s.frames_with(2, vec![key(egui::Key::I, egui::Modifiers::COMMAND | egui::Modifiers::SHIFT)]);
    check(
        whole == 32 && count(&s) == 0,
        format!("the flyout's Invert selects every voxel ({whole}), Cmd-Shift-I none"),
    )?;
    s.stroke(a_top, &[]);
    let color = count(&s);
    s.frames_with(2, vec![key(egui::Key::K, egui::Modifiers::COMMAND)]);
    s.frames_with(1, vec![egui::Event::Text("invert selection".into())]);
    s.frames_with(2, vec![key(egui::Key::Enter, none)]);
    let rest = count(&s);
    s.ui_click("select-all")?;
    check(
        color > 0 && rest == 32 - color && count(&s) == 32 && s.ed.scene.to_vox() == saved,
        format!(
            "Cmd-K Invert selection turns one colour's {color} voxels into the other {rest}; All selects 32; no edit"
        ),
    )?;
    s.ui_click("deselect")?;

    // Region select: the flyout's Face takes block A's top layer of its colour (12 cells,
    // less the marker), Cmd-K Click selects volume all of block A and not B (24), Shift adds
    // B's volume, and Color goes back to the colour (23). None of it is an edit.
    s.ui_click("click-face")?;
    s.stroke(a_top, &[]);
    let face = (s.ed.ui.region, count(&s), bounds(&s));
    s.frames_with(2, vec![key(egui::Key::K, egui::Modifiers::COMMAND)]);
    s.frames_with(1, vec![egui::Event::Text("click selects volume".into())]);
    s.frames_with(2, vec![key(egui::Key::Enter, none)]);
    s.stroke(a_top, &[]);
    let volume = (s.ed.ui.region, count(&s));
    s.hold(egui::Modifiers::SHIFT);
    s.stroke(b_top, &[]);
    s.hold(none);
    let both = count(&s);
    s.ui_click("click-color")?;
    s.stroke(a_top, &[]);
    check(
        face == (Region::Face, 11, Some(([50, 1, 60], [4, 1, 3])))
            && volume == (Region::Volume, 24)
            && both == 32
            && count(&s) == 23
            && s.ed.scene.to_vox() == saved,
        format!(
            "a click selects the face {face:?}, Cmd-K the volume {volume:?}, Shift adds ({both}), Color the colour ({}); no edit",
            count(&s)
        ),
    )?;
    s.ui_click("deselect")?;

    s.stroke(a_top, &[]);
    let before = bounds(&s);
    s.frames_with(2, vec![key(egui::Key::ArrowRight, none)]);
    let after = bounds(&s);
    let step = before.zip(after).map(|(b, a)| [0, 1, 2].map(|k| a.0[k] - b.0[k]));
    check(
        step.is_some_and(|d| d.iter().map(|c| c.abs()).sum::<i32>() == 1 && d[1] == 0),
        format!("arrow nudges the selection one cell along the ground ({step:?})"),
    )?;
    s.frames_with(2, vec![key(egui::Key::ArrowUp, egui::Modifiers::SHIFT)]);
    let up = bounds(&s).map(|b| b.0[1]);
    check(up == Some(1), "Shift-Up lifts it one cell".into())?;
    s.ed.undo();
    s.ed.undo();

    s.ui_click("cut")?;
    let cut = s.voxels();
    s.frames_with(2, vec![key(egui::Key::Z, egui::Modifiers::COMMAND)]);
    check(cut == v - 23 && s.voxels() == v && count(&s) == 23, "cut, and Cmd-Z puts it back selected".into())?;

    // Nothing selected: the flyout transforms the whole model.
    s.frames_with(2, vec![key(egui::Key::Escape, none)]);
    let mut before = Vec::new();
    s.ed.scene.for_each_voxel(|p, c| before.push((p, c)));
    s.ui_click("flip-x")?;
    let a_now = s.ed.scene.map.get(66, 0, 60);
    check(count(&s) == 0 && s.voxels() == v && a_now == 12, "whole-model flip X: block A lands where B was".into())?;
    s.ed.undo();
    let mut back = Vec::new();
    s.ed.scene.for_each_voxel(|p, c| back.push((p, c)));
    check(back == before, "undo restores the whole model".into())?;

    // The screenshot: block A selected, a box being drawn over B.
    s.stroke(a_top, &[]);
    let (b0, b1) = (project(&s.ed, [64.0, 3.0, 58.0]), project(&s.ed, [70.0, -0.5, 64.0]));
    s.ed.ui.slot = 40;
    s.hold(egui::Modifiers::SHIFT);
    s.ed.pointer_moved(Some(b0));
    s.ed.button(Button::Left, true, false);
    s.ed.pointer_moved(Some(b1));
    s.frames(4);
    s.save_png(&dir.join("app-2x-select.png"))?;
    s.ed.button(Button::Left, false, false);
    s.hold(none);
    Ok(())
}

/// Model tools on an empty scene with one 5^3 cube, so every count is exact: the selection
/// flyout's hollow and fill (whole model), Cmd-K 'Fill inside', crop to content (flyout and
/// Cmd-K) and resize through the Bounds panel's fields, one undo step each. Writes
/// `app-2x-bounds.png` with the Bounds panel open.
fn model_tools(dir: &Path) -> Result<(), String> {
    let mut s = Shot::new(Scene::new(NEW_SIZE), "Untitled", 2.0)?;
    s.ed.ui.coach = false;
    let mut cells = Vec::new();
    for (x, y, z) in (60..65).flat_map(|x| (0..5).flat_map(move |y| (60..65).map(move |z| (x, y, z)))) {
        cells.push(([x, y, z], 12));
    }
    s.ed.scene.edit(cells);
    s.ed.frame_scene();
    s.frames(3);
    let key = |key, modifiers| egui::Event::Key { key, physical_key: None, pressed: true, repeat: false, modifiers };
    s.frames_with(2, vec![key(egui::Key::S, egui::Modifiers::NONE)]);
    s.ui_click("hollow")?;
    let hollowed = (s.voxels(), s.ed.scene.map.get(62, 2, 62));
    check(hollowed == (98, 0), format!("flyout hollow: the 3^3 core goes ({hollowed:?})"))?;
    s.frames_with(2, vec![key(egui::Key::Z, egui::Modifiers::COMMAND)]);
    check(s.voxels() == 125, "hollow is one undo step".into())?;
    s.ui_click("hollow")?;
    s.ed.ui.slot = 7;
    s.ui_click("fill-in")?;
    check(
        s.voxels() == 125 && s.ed.scene.map.counts[7] == 27,
        format!("flyout fill: the enclosed core fills with the current colour ({})", s.ed.scene.map.counts[7]),
    )?;
    s.ed.undo();
    s.frames_with(2, vec![key(egui::Key::K, egui::Modifiers::COMMAND)]);
    s.frames_with(1, vec![egui::Event::Text("fill inside".into())]);
    s.frames_with(2, vec![key(egui::Key::Enter, egui::Modifiers::NONE)]);
    let filled = s.ed.scene.map.counts[7];
    s.ed.undo();
    s.ed.undo();
    check(
        filled == 27 && s.voxels() == 125 && s.ed.scene.map.counts[12] == 125,
        "Cmd-K 'Fill inside'; undo back to the solid cube".into(),
    )?;

    // Crop: the bounds fit the cube, the cube stays where it is in the world, and the view
    // does not jump (the voxel under the middle of the view is the same world cell).
    let mid = [s.ed.width as f32 / 2.0, s.ed.height as f32 / 2.0];
    let under = |s: &mut Shot| {
        s.ed.pointer_moved(Some(mid));
        s.ed.cursor().filter(|t| !t.ground).map(|t| s.ed.scene.to_world(t.voxel))
    };
    let (before, world) = (under(&mut s), s.ed.scene.to_world([60, 0, 60]));
    s.ui_click("crop")?;
    let b = s.ed.scene.bounds();
    check(
        b.size == [5, 5, 5] && b.lo == world && s.voxels() == 125,
        format!("flyout crop: the bounds fit the cube ({:?} at {:?})", b.size, b.lo),
    )?;
    check(before.is_some() && under(&mut s) == before, format!("the view holds still across the crop ({before:?})"))?;
    s.frames_with(2, vec![key(egui::Key::Z, egui::Modifiers::COMMAND)]);
    let b = s.ed.scene.bounds();
    check(b.size == [128, 128, 128] && s.voxels() == 125 && under(&mut s) == before, "crop is one undo step".into())?;
    // Resize through the Bounds panel: 3 wide keeps the middle three columns of the cube.
    s.ui_click("resize")?;
    check(
        s.ed.ui.bounds_panel.is_some() && s.ed.ui.hits.contains_key("bounds-x"),
        "Resize… opens the Bounds panel".into(),
    )?;
    s.type_into("bounds-x", "3")?;
    let b = s.ed.scene.bounds();
    check(
        b.size == [3, 128, 128] && s.voxels() == 75,
        format!("the X field resizes to 3, erasing what falls outside ({:?}, {} voxels)", b.size, s.voxels()),
    )?;
    s.frames(2);
    s.save_png(&dir.join("app-2x-bounds.png"))?;
    s.frames_with(2, vec![key(egui::Key::Z, egui::Modifiers::COMMAND)]);
    check(s.ed.scene.bounds().size == [128; 3] && s.voxels() == 125, "resize is one undo step".into())?;
    s.frames_with(2, vec![key(egui::Key::Escape, egui::Modifiers::NONE)]);
    check(s.ed.ui.bounds_panel.is_none(), "Escape closes the Bounds panel".into())?;
    s.frames_with(2, vec![key(egui::Key::K, egui::Modifiers::COMMAND)]);
    s.frames_with(1, vec![egui::Event::Text("crop to content".into())]);
    s.frames_with(2, vec![key(egui::Key::Enter, egui::Modifiers::NONE)]);
    let cropped = s.ed.scene.bounds().size;
    s.ed.undo();
    s.frames(1);
    check(
        cropped == [5, 5, 5] && s.ed.scene.bounds().size == [128; 3] && s.voxels() == 125,
        "Cmd-K 'Crop to content' and its undo".into(),
    )?;
    Ok(())
}

/// The world: a file of several objects opens in world mode, where voxels cannot be edited;
/// Tab edits an object's model and goes back, Cmd-K does the same, and undo goes back into
/// the model it edits.
fn world(dir: &Path) -> Result<(), String> {
    use vxr_core::history::At;
    let bytes = vxr_vox::fixtures::all()
        .into_iter()
        .find(|f| f.name == "22-world")
        .ok_or("no 22-world fixture")?
        .builder
        .build()
        .write();
    let mut s = Shot::new(Scene::from_vox(&bytes)?, "22-world.vox", 2.0)?;
    s.ed.ui.coach = false;
    s.frames(3);
    let key = |key, modifiers| egui::Event::Key { key, physical_key: None, pressed: true, repeat: false, modifiers };
    let tab = |s: &mut Shot| s.frames_with(2, vec![key(egui::Key::Tab, egui::Modifiers::NONE)]);
    let world = s.ed.scene.world().ok_or("no world")?;
    let (objects, shown) = (world.objects(), world.world_voxels(true).len() as u64);
    check(
        s.ed.scene.at() == At::World && s.voxels() == shown,
        format!("a file of {} objects opens in world mode, showing {shown} voxels", objects.len()),
    )?;
    s.tool(Tool::Add, Shape::Voxel);
    let mid = [s.ed.width as f32 / 2.0, s.ed.height as f32 / 2.0];
    let p = s.top_face_pixel(mid)?;
    s.stroke(p, &[]);
    check(s.voxels() == shown && !s.ed.scene.history().can_undo(), "a click in world mode edits no voxel".into())?;
    s.frames(1);
    let picked = s.ed.scene.picked();
    check(
        picked.is_some() && s.ed.scene.selected_count() > 0 && s.ed.ui.picked.is_some(),
        format!("the click picks {} and shows it selected", s.ed.ui.picked.clone().unwrap_or_default()),
    )?;
    s.frames(2);
    s.save_png(&dir.join("app-2x-world.png"))?;

    tab(&mut s);
    let first = picked.ok_or("nothing picked")?;
    let w = s.ed.scene.world().ok_or("no world")?;
    let m = w.model_of(first).ok_or("not an object")?;
    let model = w.models[m].voxels;
    // A linked copy's edit shows on every shown copy of its model.
    let copies = w.objects().into_iter().filter(|&i| w.model_of(i) == Some(m) && w.shown(i)).count() as u64;
    check(
        s.ed.scene.at() == At::Object(first) && s.voxels() == model,
        format!("Tab edits the picked object's model ({model} voxels)"),
    )?;
    s.ed.frame_scene();
    s.frames(2);
    // The slab is one voxel tall and its bounds are its model's size, so erase rather than add.
    s.tool(Tool::Erase, Shape::Voxel);
    let p = s.top_face_pixel(mid)?;
    s.stroke(p, &[]);
    check(s.voxels() == model - 1, "an erase in model mode".into())?;
    tab(&mut s);
    check(
        s.ed.scene.at() == At::World && s.voxels() == shown - copies,
        format!("Tab shows the world with the edit ({} voxels, {:?})", s.voxels(), s.ed.scene.at()),
    )?;
    tab(&mut s);
    check(s.ed.scene.at() == At::Object(first), "Tab goes back to the model last edited".into())?;
    s.frames_with(2, vec![key(egui::Key::K, egui::Modifiers::COMMAND)]);
    s.frames_with(1, vec![egui::Event::Text("world mode".into())]);
    s.frames_with(2, vec![key(egui::Key::Enter, egui::Modifiers::NONE)]);
    check(s.ed.scene.at() == At::World, "Cmd-K 'World mode / edit model'".into())?;
    s.frames_with(2, vec![key(egui::Key::Z, egui::Modifiers::COMMAND)]);
    check(
        s.ed.scene.at() == At::Object(first) && s.voxels() == model && s.ed.scene.to_vox() == bytes,
        "undo goes back into the model and the save is byte-identical again".into(),
    )?;

    // World mode: pick, drag, nudge, duplicate, linked copy, delete, all through the
    // pointer path and keys.
    tab(&mut s);
    check(
        s.ed.scene.at() == At::World && s.ed.scene.picked() == Some(first),
        "leaving a model picks it in the world".into(),
    )?;
    s.frames(2);
    let p = s.top_face_pixel(mid)?;
    s.stroke(p, &[]);
    let o = s.ed.scene.picked().ok_or("the click picked nothing")?;
    let place = |s: &Shot, i| s.ed.scene.world().and_then(|w| w.placement(i)).ok_or("no placement");
    let before = place(&s, o)?;
    s.stroke(p, &[[20.0, 0.0], [40.0, 4.0], [70.0, 10.0]]);
    let after = place(&s, o)?;
    check(
        after.1 == before.1 && after.0[1] == before.0[1] && after.0 != before.0 && s.ed.scene.picked() == Some(o),
        format!("a drag moves the object on its top face's plane ({:?} to {:?})", before.0, after.0),
    )?;
    s.frames_with(2, vec![key(egui::Key::Z, egui::Modifiers::COMMAND)]);
    let undone = place(&s, o)? == before;
    s.frames_with(2, vec![key(egui::Key::Z, egui::Modifiers::COMMAND | egui::Modifiers::SHIFT)]);
    check(undone && place(&s, o)? == after, "the drag is one undo step, and redo puts it back".into())?;
    s.frames_with(2, vec![key(egui::Key::ArrowRight, egui::Modifiers::NONE)]);
    let n = place(&s, o)?;
    let d: i32 = (0..3).map(|a| (n.0[a] - after.0[a]).abs()).sum();
    check(d == 1 && n.0[1] == after.0[1], "an arrow nudges the picked object one cell along the ground".into())?;
    s.frames_with(2, vec![key(egui::Key::ArrowUp, egui::Modifiers::SHIFT)]);
    check(place(&s, o)?.0[1] == n.0[1] + 1, "Shift-Up raises it one cell".into())?;
    let counts = |s: &Shot| s.ed.scene.world().map_or((0, 0), |w| (w.objects().len(), w.models.len()));
    let (n0, m0) = counts(&s);
    s.frames_with(2, vec![key(egui::Key::D, egui::Modifiers::COMMAND)]);
    let dup = s.ed.scene.picked().ok_or("no pick after duplicate")?;
    let (pd, po) = (place(&s, dup)?, place(&s, o)?);
    check(
        counts(&s) == (n0 + 1, m0 + 1) && dup != o && pd.0[0] == po.0[0] + po.1[0] as i32 + 1 && pd.1 == po.1,
        "Cmd-D duplicates the object beside itself with its own model, and picks the copy".into(),
    )?;
    s.frames_with(2, vec![key(egui::Key::D, egui::Modifiers::COMMAND | egui::Modifiers::SHIFT)]);
    let link = s.ed.scene.picked().ok_or("no pick after linked copy")?;
    let w = s.ed.scene.world().ok_or("no world")?;
    check(
        counts(&s) == (n0 + 2, m0 + 1) && link != dup && w.model_of(link) == w.model_of(dup),
        "Cmd-Shift-D makes a linked copy sharing the model".into(),
    )?;
    tab(&mut s);
    check(s.ed.scene.at() == At::Object(link), "Tab edits the picked object's model".into())?;
    tab(&mut s);
    s.frames_with(2, vec![key(egui::Key::Backspace, egui::Modifiers::NONE)]);
    s.frames(1);
    check(
        counts(&s).0 == n0 + 1 && s.ed.scene.picked().is_none() && s.ed.ui.picked.is_none(),
        "Backspace deletes the picked object".into(),
    )?;
    s.stroke(p, &[]);
    let had = s.ed.scene.picked().is_some();
    s.frames_with(2, vec![key(egui::Key::Escape, egui::Modifiers::NONE)]);
    check(had && s.ed.scene.picked().is_none() && s.ed.scene.selected_count() == 0, "Escape picks nothing".into())?;
    s.stroke(p, &[]);
    s.frames_with(2, vec![key(egui::Key::K, egui::Modifiers::COMMAND)]);
    s.frames_with(1, vec![egui::Event::Text("duplicate object".into())]);
    s.frames_with(2, vec![key(egui::Key::Enter, egui::Modifiers::NONE)]);
    check(counts(&s).0 == n0 + 2, "Cmd-K 'Duplicate object'".into())?;
    s.frames(2);
    s.save_png(&dir.join("app-2x-world-edit.png"))?;
    let saved = s.ed.scene.to_vox();
    let again = Scene::from_vox(&saved)?;
    check(
        again.world_voxels(false) == s.ed.scene.world_voxels(false),
        "the edited world saves and reopens the same".into(),
    )?;
    while s.ed.scene.undo() {}
    check(s.ed.scene.to_vox() == bytes, "every world edit undone: byte-identical save".into())?;
    Ok(())
}

/// The scene tree on the generated `22-world` file: its rows and indentation, flyouts moved
/// beside it, a row click picks, the eye and H hide and show (one undo step each), a
/// double-click and Cmd-K rename (Escape cancels), and in model mode a row click edits that
/// object. A single model shows no tree.
fn scene_tree(dir: &Path) -> Result<(), String> {
    use vxr_app::editor::{node_name, tree_rows};
    use vxr_core::history::At;
    let fixture =
        |name: &str| vxr_vox::fixtures::all().into_iter().find(|f| f.name == name).map(|f| f.builder.build().write());
    let bytes = fixture("22-world").ok_or("no 22-world fixture")?;
    let single = Scene::from_vox(&fixture("01-single-voxel").ok_or("no 01-single-voxel fixture")?)?;
    check(
        tree_rows(&single).is_empty() && tree_rows(&Scene::new(NEW_SIZE)).is_empty(),
        "a single model or a new scene shows no scene tree".into(),
    )?;
    let mut s = Shot::new(Scene::from_vox(&bytes)?, "22-world.vox", 2.0)?;
    s.ed.ui.coach = false;
    s.frames(3);
    let key = |key, modifiers| egui::Event::Key { key, physical_key: None, pressed: true, repeat: false, modifiers };
    let node = |s: &Shot, name: &str| {
        let w = s.ed.scene.world().unwrap();
        (0..w.tree.nodes.len()).find(|&i| node_name(w, i) == name).ok_or(format!("no node {name}"))
    };
    let rows = s.ed.ui.tree.clone();
    let (tree_c, grove) = (node(&s, "tree c")?, node(&s, "grove")?);
    let row = |n| rows.iter().find(|r| r.node == n);
    check(
        rows.len() == 8
            && row(tree_c).is_some_and(|r| r.depth == 2 && r.linked == 3)
            && row(grove).is_some_and(|r| r.group && r.depth == 1)
            && row(node(&s, "lost gem")?).is_some_and(|r| r.hidden && !r.shown)
            && row(node(&s, "gem")?).is_some_and(|r| !r.hidden && !r.shown)
            && rows.iter().all(|r| s.ed.ui.hits.contains_key(&format!("tree-{}", r.node))),
        format!("22-world shows a scene tree of {} rows: groups indented, linked copies, hidden rows", rows.len()),
    )?;
    let tree_right = rows.iter().map(|r| s.ed.ui.hits[&format!("tree-{}", r.node)].right()).fold(0.0, f32::max);
    s.ui_click("nav-views")?;
    let flyout = s.ed.ui.hits.get("view-persp").map(|r| r.left()).unwrap_or(0.0);
    s.ui_click("nav-views")?;
    check(flyout > tree_right, "the View flyout opens clear of the scene tree".into())?;

    let slab = node(&s, "slab")?;
    let slab_voxels = s.ed.scene.world().unwrap().extent(slab).is_some();
    s.ui_click(&format!("tree-{slab}"))?;
    check(
        slab_voxels && s.ed.scene.picked() == Some(slab) && s.ed.ui.tree_on == Some(slab),
        "a row click picks its object".into(),
    )?;
    let v0 = s.voxels();
    s.ui_click(&format!("tree-eye-{slab}"))?;
    let hidden = s.ed.scene.world().unwrap().tree.nodes[slab].hidden;
    check(hidden && s.voxels() == v0 - 64, format!("the row's eye hides the slab ({v0} to {} voxels)", s.voxels()))?;
    s.frames(2);
    s.save_png(&dir.join("app-2x-tree.png"))?;
    s.frames_with(2, vec![key(egui::Key::Z, egui::Modifiers::COMMAND)]);
    let undone = s.voxels() == v0;
    s.frames_with(2, vec![key(egui::Key::Z, egui::Modifiers::COMMAND | egui::Modifiers::SHIFT)]);
    check(undone && s.voxels() == v0 - 64, "hiding is one undo step, and redo hides it again".into())?;
    s.ui_click(&format!("tree-eye-{slab}"))?;
    check(s.voxels() == v0, "the eye shows it again".into())?;
    s.frames_with(2, vec![key(egui::Key::H, egui::Modifiers::NONE)]);
    let h_hid = s.voxels() == v0 - 64;
    s.frames_with(2, vec![key(egui::Key::H, egui::Modifiers::NONE)]);
    check(h_hid && s.voxels() == v0, "H hides and shows the picked object".into())?;

    // Rename: double-click, type, Enter; Escape cancels; Cmd-K opens the field too.
    let name_of = |s: &Shot, n: usize| node_name(s.ed.scene.world().unwrap(), n);
    // Half a second after the last click, so egui does not count a triple click.
    s.frames(30);
    s.ui_click(&format!("tree-{slab}"))?;
    s.ui_click(&format!("tree-{slab}"))?;
    let field = s.ed.ui.hits.contains_key("tree-rename");
    s.frames_with(1, vec![key(egui::Key::A, egui::Modifiers::COMMAND)]);
    s.frames_with(1, vec![egui::Event::Text("floor".into())]);
    s.frames_with(2, vec![key(egui::Key::Enter, egui::Modifiers::NONE)]);
    check(
        field && name_of(&s, slab) == "floor" && !s.ed.ui.hits.contains_key("tree-rename"),
        format!("a double-click renames the row ({})", name_of(&s, slab)),
    )?;
    s.frames_with(2, vec![key(egui::Key::Z, egui::Modifiers::COMMAND)]);
    check(name_of(&s, slab) == "slab", "renaming is one undo step".into())?;
    s.frames_with(2, vec![key(egui::Key::K, egui::Modifiers::COMMAND)]);
    s.frames_with(1, vec![egui::Event::Text("rename object".into())]);
    s.frames_with(2, vec![key(egui::Key::Enter, egui::Modifiers::NONE)]);
    let opened = s.ed.ui.hits.contains_key("tree-rename");
    s.frames_with(1, vec![egui::Event::Text(" x".into())]);
    s.frames_with(2, vec![key(egui::Key::Escape, egui::Modifiers::NONE)]);
    check(
        opened
            && name_of(&s, slab) == "slab"
            && !s.ed.ui.hits.contains_key("tree-rename")
            && s.ed.scene.picked() == Some(slab),
        "Cmd-K 'Rename object' opens the field, and Escape cancels it".into(),
    )?;

    groups(&mut s, &bytes, dir)?;
    layers(&mut s, &bytes, dir)?;
    s.frames(30);
    s.ui_click(&format!("tree-{slab}"))?;

    // Model mode: the edited object's row is on, and a row click edits another object.
    s.frames_with(2, vec![key(egui::Key::Tab, egui::Modifiers::NONE)]);
    let editing = s.ed.scene.at() == At::Object(slab) && s.ed.ui.tree_on == Some(slab);
    let tree_a = node(&s, "tree a")?;
    s.ui_click(&format!("tree-{tree_a}"))?;
    check(
        editing && s.ed.scene.at() == At::Object(tree_a) && s.ed.ui.tree_on == Some(tree_a),
        "in model mode a row click edits that object's model".into(),
    )?;
    while s.ed.scene.undo() {}
    check(s.ed.scene.to_vox() == bytes, "every tree edit undone: byte-identical save".into())?;
    Ok(())
}

/// Groups in world mode on 22-world: a group row picks the group, a drag on one of its
/// objects and the arrows move the whole group, Cmd-D copies it, Cmd-Shift-G ungroups it,
/// Cmd-G and Cmd-K "Ungroup" wrap an object in a group and take it out again, and Tab edits
/// the group's first object. Leaves the world with every group edit undone.
fn groups(s: &mut Shot, bytes: &[u8], dir: &Path) -> Result<(), String> {
    use vxr_app::editor::node_name;
    use vxr_core::history::At;
    let key = |key, modifiers| egui::Event::Key { key, physical_key: None, pressed: true, repeat: false, modifiers };
    let cmd = egui::Modifiers::COMMAND;
    let node = |s: &Shot, name: &str| {
        let w = s.ed.scene.world().unwrap();
        (0..w.tree.nodes.len()).find(|&i| node_name(w, i) == name).ok_or(format!("no node {name}"))
    };
    let trees = node(s, "trees")?;
    let members = s.ed.scene.world().unwrap().objects_under(trees);
    let places = |s: &Shot| -> Vec<[i32; 3]> {
        let w = s.ed.scene.world().unwrap();
        members.iter().map(|&i| w.placement(i).map_or([0; 3], |p| p.0)).collect()
    };
    let shifted = |a: &[[i32; 3]], b: &[[i32; 3]]| -> Option<[i32; 3]> {
        let d: [i32; 3] = std::array::from_fn(|k| b[0][k] - a[0][k]);
        a.iter().zip(b).all(|(p, q)| (0..3).all(|k| q[k] - p[k] == d[k])).then_some(d)
    };
    let shown: usize = {
        let w = s.ed.scene.world().unwrap();
        let mut n = 0;
        for &i in members.iter().filter(|&&i| w.shown(i)) {
            w.for_each_object_voxel(i, |_, _| n += 1);
        }
        n
    };

    s.frames(30);
    s.ui_click(&format!("tree-{trees}"))?;
    s.frames(1);
    check(
        s.ed.scene.picked() == Some(trees)
            && s.ed.ui.tree_on == Some(trees)
            && s.ed.scene.selected_count() == shown
            && members.len() == 3,
        format!("a group row picks the group: its {} objects' {shown} voxels show selected", members.len()),
    )?;
    s.frames(2);
    s.save_png(&dir.join("app-2x-group.png"))?;

    // A drag on one of the group's objects moves the whole group, as one undo step.
    let (w, h) = (s.ed.width as f32, s.ed.height as f32);
    let mut at = None;
    'find: for y in (0..40).map(|k| h * (0.15 + k as f32 * 0.0175)) {
        for x in (0..60).map(|k| w * (0.2 + k as f32 * 0.01)) {
            s.ed.pointer_moved(Some([x, y]));
            let hit = s.ed.cursor().filter(|t| !t.ground && t.normal == [0, 1, 0]);
            if hit.and_then(|t| s.ed.scene.object_at(t.voxel)).is_some_and(|o| members.contains(&o)) {
                at = Some([x, y]);
                break 'find;
            }
        }
    }
    let at = at.ok_or("no top face of the trees group in view")?;
    let before = places(s);
    s.stroke(at, &[[20.0, 0.0], [40.0, 4.0], [70.0, 10.0]]);
    let after = places(s);
    let d = shifted(&before, &after);
    check(
        s.ed.scene.picked() == Some(trees) && d.is_some_and(|d| d != [0; 3] && d[1] == 0),
        format!("a drag on a tree of the picked group moves the whole group ({d:?})"),
    )?;
    s.frames_with(2, vec![key(egui::Key::Z, cmd)]);
    check(places(s) == before, "the group drag is one undo step".into())?;
    s.frames_with(2, vec![key(egui::Key::ArrowRight, egui::Modifiers::NONE)]);
    let d = shifted(&before, &places(s));
    check(
        d.is_some_and(|d| d.iter().map(|k| k.abs()).sum::<i32>() == 1 && d[1] == 0),
        "an arrow nudges every object of the picked group one cell".into(),
    )?;
    s.frames_with(2, vec![key(egui::Key::Z, cmd)]);

    // Cmd-D copies the group with all its objects.
    let objects = |s: &Shot| s.ed.scene.world().unwrap().objects().len();
    let n0 = objects(s);
    s.frames_with(2, vec![key(egui::Key::D, cmd)]);
    let copy = s.ed.scene.picked().filter(|&c| c != trees);
    let copied = copy.is_some_and(|c| s.ed.scene.world().unwrap().objects_under(c).len() == members.len());
    check(copied && objects(s) == n0 + members.len(), "Cmd-D copies the picked group and every object in it".into())?;
    s.frames_with(2, vec![key(egui::Key::Z, cmd)]);

    // Cmd-Shift-G ungroups: one row fewer, the objects stay where they show.
    let world = s.ed.scene.world_voxels(false);
    s.frames(30);
    s.ui_click(&format!("tree-{trees}"))?;
    let rows = s.ed.ui.tree.len();
    s.frames_with(2, vec![key(egui::Key::G, cmd | egui::Modifiers::SHIFT)]);
    check(
        s.ed.ui.tree.len() == rows - 1
            && !s.ed.ui.tree.iter().any(|r| r.node == trees)
            && s.ed.scene.picked() == Some(members[0])
            && s.ed.scene.world_voxels(false) == world,
        "Cmd-Shift-G ungroups the picked group in place and picks its first object".into(),
    )?;
    s.frames_with(2, vec![key(egui::Key::Z, cmd)]);
    check(
        s.ed.ui.tree.len() == rows && s.ed.ui.tree.iter().any(|r| r.node == trees && r.group),
        "ungrouping is one undo step".into(),
    )?;

    // Cmd-G wraps the slab in a group of its own; Cmd-K "Ungroup" takes it out again.
    let slab = node(s, "slab")?;
    s.frames(30);
    s.ui_click(&format!("tree-{slab}"))?;
    s.frames_with(2, vec![key(egui::Key::G, cmd)]);
    let g = s.ed.scene.picked().filter(|&g| g != slab);
    let wrapped = g.is_some_and(|g| s.ed.scene.world().unwrap().tree.children(g) == [slab]);
    let row = g.and_then(|g| s.ed.ui.tree.iter().find(|r| r.node == g).cloned());
    let slab_row = s.ed.ui.tree.iter().find(|r| r.node == slab).cloned();
    check(
        wrapped
            && s.ed.ui.tree.len() == rows + 1
            && row.is_some_and(|r| r.group && r.depth == 0)
            && slab_row.is_some_and(|r| r.depth == 1)
            && s.ed.scene.world_voxels(false) == world,
        "Cmd-G puts the picked object into a new group, in place, and picks the group".into(),
    )?;
    s.frames_with(2, vec![key(egui::Key::K, cmd)]);
    s.frames_with(1, vec![egui::Event::Text("ungroup".into())]);
    s.frames_with(2, vec![key(egui::Key::Enter, egui::Modifiers::NONE)]);
    check(
        s.ed.ui.tree.len() == rows && s.ed.scene.picked() == Some(slab),
        "Cmd-K 'Ungroup' takes it out again".into(),
    )?;
    s.frames_with(2, vec![key(egui::Key::Z, cmd)]);
    s.frames_with(2, vec![key(egui::Key::Z, cmd)]);

    // Tab with a group picked edits the group's first object.
    s.frames(30);
    s.ui_click(&format!("tree-{trees}"))?;
    s.frames_with(2, vec![key(egui::Key::Tab, egui::Modifiers::NONE)]);
    let editing = s.ed.scene.at() == At::Object(members[0]);
    s.frames_with(2, vec![key(egui::Key::Tab, egui::Modifiers::NONE)]);
    check(
        editing && s.ed.scene.at() == At::World && s.ed.scene.to_vox() == bytes,
        "Tab with a group picked edits its first object; every group edit undone gives the file".into(),
    )?;
    Ok(())
}

/// Layers in world mode on 22-world: the LAYERS list under the tree, a layer's eye, a layer
/// row click putting the picked object on it, a double-click rename, + and Cmd-K "New layer",
/// Cmd-K "Hide or show layer", one undo step each, and no list in model mode. Leaves the
/// world with every layer edit undone.
fn layers(s: &mut Shot, bytes: &[u8], dir: &Path) -> Result<(), String> {
    use vxr_app::editor::node_name;
    let key = |key, modifiers| egui::Event::Key { key, physical_key: None, pressed: true, repeat: false, modifiers };
    let cmd = egui::Modifiers::COMMAND;
    let node = |s: &Shot, name: &str| {
        let w = s.ed.scene.world().unwrap();
        (0..w.tree.nodes.len()).find(|&i| node_name(w, i) == name).ok_or(format!("no node {name}"))
    };
    let list = |s: &Shot| -> Vec<(i32, String, usize, bool)> {
        s.ed.ui.layers.iter().map(|l| (l.id, l.name.clone(), l.objects, l.hidden)).collect()
    };
    let cmdk = |s: &mut Shot, text: &str| {
        s.frames_with(2, vec![key(egui::Key::K, cmd)]);
        s.frames_with(1, vec![egui::Event::Text(text.into())]);
        s.frames_with(2, vec![key(egui::Key::Enter, egui::Modifiers::NONE)]);
    };
    s.frames(2);
    let want = [(0, "ground".into(), 2, false), (1, "trees".into(), 3, false), (2, "secret".into(), 1, true)];
    let tree_bottom =
        s.ed.ui.tree.iter().map(|r| s.ed.ui.hits[&format!("tree-{}", r.node)].bottom()).fold(0.0, f32::max);
    check(
        list(s) == want
            && (0..3).all(|id| s.ed.ui.hits.get(&format!("layer-{id}")).is_some_and(|r| r.top() > tree_bottom))
            && s.ed.ui.hits.contains_key("layer-add"),
        format!("the LAYERS list under the tree shows 22-world's layers ({:?})", list(s)),
    )?;

    // The eye shows the hidden secret layer and its gem, as one undo step.
    let v0 = s.voxels();
    s.ui_click("layer-eye-2")?;
    let gem = node(s, "gem")?;
    check(
        s.voxels() == v0 + 1 && s.ed.scene.world().unwrap().shown(gem) && !s.ed.ui.layers[2].hidden,
        "a layer's eye shows the hidden secret layer and its gem".into(),
    )?;
    s.frames_with(2, vec![key(egui::Key::Z, cmd)]);
    let undone = s.voxels() == v0;
    s.frames_with(2, vec![key(egui::Key::Z, cmd | egui::Modifiers::SHIFT)]);
    check(undone && s.voxels() == v0 + 1, "showing a layer is one undo step, and redo shows it again".into())?;

    // A layer row click puts the picked object on that layer; hiding the layer hides it.
    let slab = node(s, "slab")?;
    s.frames(30);
    s.ui_click(&format!("tree-{slab}"))?;
    let on_ground = s.ed.ui.layer_on == Some(0);
    s.ui_click("layer-1")?;
    let row = s.ed.ui.tree.iter().find(|r| r.node == slab).cloned();
    check(
        on_ground
            && s.ed.ui.layer_on == Some(1)
            && row.is_some_and(|r| r.layer == 1)
            && s.ed.ui.layers[1].objects == 4
            && s.ed.ui.layers[0].objects == 1,
        "a layer row click puts the picked slab on the trees layer".into(),
    )?;
    s.frames(2);
    s.save_png(&dir.join("app-2x-layers.png"))?;
    let v1 = s.voxels();
    s.ui_click("layer-eye-1")?;
    let w = s.ed.scene.world().unwrap();
    let hidden_all = w.objects().iter().filter(|&&i| w.tree.nodes[i].layer == 1).all(|&i| !w.shown(i));
    check(
        hidden_all && s.voxels() < v1 - 64,
        format!("hiding the trees layer hides the slab and the trees ({v1} to {} voxels)", s.voxels()),
    )?;
    s.frames_with(2, vec![key(egui::Key::Z, cmd)]);
    s.frames_with(2, vec![key(egui::Key::Z, cmd)]);
    check(
        s.voxels() == v1 && s.ed.ui.layer_on == Some(0) && list(s)[1].2 == 3,
        "hiding a layer and moving an object to it are one undo step each".into(),
    )?;

    // Cmd-K "Hide or show layer" hides the picked object's layer.
    cmdk(s, "hide or show layer");
    let hid = s.ed.ui.layers[0].hidden && !s.ed.scene.world().unwrap().shown(slab);
    s.frames_with(2, vec![key(egui::Key::Z, cmd)]);
    check(hid && !s.ed.ui.layers[0].hidden, "Cmd-K 'Hide or show layer' hides the picked object's layer".into())?;

    // A double-click renames a layer; Enter keeps the name.
    s.frames(30);
    s.ui_click("layer-0")?;
    s.ui_click("layer-0")?;
    let field = s.ed.ui.hits.contains_key("tree-rename");
    s.frames_with(1, vec![key(egui::Key::A, cmd)]);
    s.frames_with(1, vec![egui::Event::Text("floor".into())]);
    s.frames_with(2, vec![key(egui::Key::Enter, egui::Modifiers::NONE)]);
    check(
        field && list(s)[0].1 == "floor" && s.ed.ui.layer_on == Some(0),
        format!("a double-click renames the layer ({}), and leaves the slab on it", list(s)[0].1),
    )?;
    s.frames_with(2, vec![key(egui::Key::Z, cmd)]);
    check(list(s)[0].1 == "ground", "renaming a layer is one undo step".into())?;

    // + and Cmd-K "New layer" add layers.
    s.ui_click("layer-add")?;
    let added = list(s).last().is_some_and(|l| l.0 == 3 && l.1 == "layer 3" && l.2 == 0);
    cmdk(s, "new layer");
    check(
        added && list(s).len() == 5 && list(s)[4].1 == "layer 4",
        "+ and Cmd-K 'New layer' add layer 3 and layer 4".into(),
    )?;
    s.frames_with(2, vec![key(egui::Key::Z, cmd)]);
    s.frames_with(2, vec![key(egui::Key::Z, cmd)]);
    check(list(s).len() == 3, "adding a layer is one undo step".into())?;

    // Model mode lists no layers.
    s.frames_with(2, vec![key(egui::Key::Tab, egui::Modifiers::NONE)]);
    let none = !s.ed.ui.hits.contains_key("layer-0") && !s.ed.ui.hits.contains_key("layer-add");
    s.frames_with(2, vec![key(egui::Key::Tab, egui::Modifiers::NONE)]);
    s.frames_with(2, vec![key(egui::Key::Z, cmd)]);
    check(
        none && s.ed.ui.hits.contains_key("layer-0") && s.ed.scene.to_vox() == bytes,
        "model mode lists no layers; every layer edit undone gives the file".into(),
    )?;
    Ok(())
}

/// Camera and view: the View flyout, orthographic and isometric, the six side views by key
/// (the middle of the view lands on the matching face of a cube), Cmd-K and the O key,
/// framing that keeps the view, and an edit made in orthographic.
fn views(dir: &Path) -> Result<(), String> {
    use vxr_render::View;
    let mut s = Shot::new(Scene::new(NEW_SIZE), "Untitled", 2.0)?;
    s.ed.ui.coach = false;
    let mut cells = Vec::new();
    for (x, y, z) in (60..68).flat_map(|x| (0..8).flat_map(move |y| (60..68).map(move |z| (x, y, z)))) {
        cells.push(([x, y, z], 12));
    }
    s.ed.scene.edit(cells);
    s.ed.frame_scene();
    s.frames(3);
    let key = |key, modifiers| egui::Event::Key { key, physical_key: None, pressed: true, repeat: false, modifiers };
    let press = |s: &mut Shot, k| s.frames_with(2, vec![key(k, egui::Modifiers::NONE)]);
    s.ui_click("nav-views")?;
    check(
        s.ed.ui.view_panel && s.ed.ui.hits.contains_key("view-front"),
        "the camera bar's Views button opens the View flyout".into(),
    )?;
    check(
        !s.ed.ui.hits.contains_key("view") && s.ed.ui.hits.contains_key("nav-frame"),
        "the rail has no view or frame button: the camera bar holds them".into(),
    )?;
    // Guides: on by default; the flyout's buttons, G and Cmd-K turn each off and on, and each
    // changes the view.
    check(s.ed.ui.grid && s.ed.ui.ground && s.ed.ui.frame, "the grid, ground and bounds show by default".into())?;
    let view_px = |s: &mut Shot| {
        s.frames(2);
        pollster::block_on(read_texture(&s.ed.gpu.device, &s.ed.gpu.queue, &s.ed.renderer.color))
    };
    let changed =
        |a: &[u8], b: &[u8]| a.as_chunks::<4>().0.iter().zip(b.as_chunks::<4>().0).filter(|(p, q)| p != q).count();
    // The render corner off (it keeps sampling), and the pointer over the flyout, as during
    // the clicks, so no voxel is highlighted.
    let corner = std::mem::replace(&mut s.ed.ui.show_render, false);
    let c = s.ed.ui.hits["view-close"].center() * s.ed.ppp;
    s.ed.pointer_moved(Some([c.x, c.y]));
    let all = view_px(&mut s);
    for (hit, what) in [("view-bounds", "bounds frame"), ("view-grid", "grid"), ("view-ground", "ground")] {
        s.ui_click(hit)?;
        let off = view_px(&mut s);
        let on = [s.ed.ui.grid, s.ed.ui.ground, s.ed.ui.frame];
        s.ui_click(hit)?;
        let back = view_px(&mut s);
        let n = changed(&all, &off);
        check(
            on.iter().filter(|o| !**o).count() == 1 && n > 500 && back == all,
            format!("the flyout's {what} button hides it ({n} pixels change) and shows it again"),
        )?;
    }
    s.ed.ui.show_render = corner;
    press(&mut s, egui::Key::G);
    let g_off = !s.ed.ui.grid;
    press(&mut s, egui::Key::G);
    check(g_off && s.ed.ui.grid, "G toggles the grid".into())?;
    for (q, flag) in [("show ground", 1), ("show bounds", 2), ("show grid", 0)] {
        let get = |s: &Shot| [s.ed.ui.grid, s.ed.ui.ground, s.ed.ui.frame][flag];
        let before = get(&s);
        s.frames_with(2, vec![key(egui::Key::K, egui::Modifiers::COMMAND)]);
        s.frames_with(1, vec![egui::Event::Text(q.into())]);
        s.frames_with(2, vec![key(egui::Key::Enter, egui::Modifiers::NONE)]);
        let after = get(&s);
        s.frames_with(2, vec![key(egui::Key::K, egui::Modifiers::COMMAND)]);
        s.frames_with(1, vec![egui::Event::Text(q.into())]);
        s.frames_with(2, vec![key(egui::Key::Enter, egui::Modifiers::NONE)]);
        check(before && !after && get(&s), format!("Cmd-K '{q}' hides it and shows it again"))?;
    }
    if !s.ed.ui.view_panel {
        s.ui_click("nav-views")?;
    }
    s.ui_click("view-ortho")?;
    let (o0, d0) = s.ed.cam.ray(10.0, 10.0, 400.0, 300.0);
    let (o1, d1) = s.ed.cam.ray(390.0, 290.0, 400.0, 300.0);
    check(
        s.ed.cam.ortho && (d0 - d1).length() < 1e-5 && (o0 - o1).length() > 1.0,
        "Ortho: orthographic, every ray parallel".into(),
    )?;
    let mid = [s.ed.width as f32 / 2.0, s.ed.height as f32 / 2.0];
    let normal_at_mid = |s: &mut Shot| {
        s.ed.pointer_moved(Some(mid));
        s.ed.cursor().filter(|t| !t.ground).map(|t| t.normal)
    };
    for (k, v, n) in [
        (egui::Key::Num1, View::Front, [0, 0, 1]),
        (egui::Key::Num2, View::Back, [0, 0, -1]),
        (egui::Key::Num3, View::Left, [-1, 0, 0]),
        (egui::Key::Num4, View::Right, [1, 0, 0]),
        (egui::Key::Num6, View::Bottom, [0, -1, 0]),
        (egui::Key::Num5, View::Top, [0, 1, 0]),
    ] {
        press(&mut s, k);
        let got = normal_at_mid(&mut s);
        check(
            s.ed.ui.view == Some(v) && got == Some(n),
            format!("key {:?}: {} view, the middle of the view is on the cube's {n:?} face ({got:?})", k, v.name()),
        )?;
    }
    // Add in the top orthographic view: the click lands on the top face.
    s.tool(Tool::Add, Shape::Voxel);
    s.stroke(mid, &[]);
    let added = s.voxels();
    s.ed.undo();
    check(added == 513 && s.voxels() == 512, format!("add from the top orthographic view: {added}, undo 512"))?;
    press(&mut s, egui::Key::Period);
    check(s.ed.cam.ortho && s.ed.ui.view == Some(View::Top), "frame (.) keeps the projection and the view".into())?;
    s.ui_click("view-iso")?;
    // Isometric: the cube's three edges from its top corner are the same length on screen.
    let (w, h) = (s.ed.width as f32, s.ed.height as f32);
    let vp = s.ed.cam.view_proj(w / h);
    let at = |p: [f32; 3]| {
        let c = vp.project_point3(glam::Vec3::from(p));
        glam::Vec2::new(c.x * w, c.y * h) / 2.0
    };
    let o = at([68.0, 8.0, 68.0]);
    let edges = [at([60.0, 8.0, 68.0]), at([68.0, 0.0, 68.0]), at([68.0, 8.0, 60.0])].map(|e| (e - o).length());
    let even = edges.iter().all(|e| (e - edges[0]).abs() < 0.5);
    check(
        s.ed.cam.ortho && s.ed.ui.view == Some(View::Iso) && even,
        format!("Iso: orthographic isometric, the cube's edges even on screen ({edges:.1?} px)"),
    )?;
    s.frames(3);
    s.save_png(&dir.join("app-2x-view.png"))?;
    s.ui_click("view-persp")?;
    check(!s.ed.cam.ortho && s.ed.ui.view.is_none(), "Persp: back to perspective".into())?;
    press(&mut s, egui::Key::O);
    let o_on = s.ed.cam.ortho;
    press(&mut s, egui::Key::O);
    check(o_on && !s.ed.cam.ortho, "O toggles orthographic".into())?;
    for (q, want) in [("orthographic", true), ("perspective", false)] {
        s.frames_with(2, vec![key(egui::Key::K, egui::Modifiers::COMMAND)]);
        s.frames_with(1, vec![egui::Event::Text(q.into())]);
        s.frames_with(2, vec![key(egui::Key::Enter, egui::Modifiers::NONE)]);
        check(s.ed.cam.ortho == want, format!("Cmd-K '{q}'"))?;
    }
    s.frames_with(2, vec![key(egui::Key::K, egui::Modifiers::COMMAND)]);
    s.frames_with(1, vec![egui::Event::Text("right view".into())]);
    s.frames_with(2, vec![key(egui::Key::Enter, egui::Modifiers::NONE)]);
    check(s.ed.ui.view == Some(View::Right), "Cmd-K 'Right view'".into())?;
    s.ui_click("view-front")?;
    check(s.ed.ui.view == Some(View::Front), "the flyout's Front button".into())?;
    press(&mut s, egui::Key::Escape);
    check(!s.ed.ui.view_panel, "Escape closes the View flyout".into())?;
    Ok(())
}

/// The camera: pan and zoom cannot lose the scene, every way back (`.`, Home, the camera
/// bar's buttons, Cmd-K) brings it into view from a lost camera, and the bar's zoom,
/// projection and views, the trackpad pinch, Space-drag pan, X-click and the hint bar work.
fn camera(dir: &Path, scene: Option<&Path>) -> Result<(), String> {
    use vxr_render::{
        View,
        camera::{HOME, MAX_ZOOM_OUT},
    };
    let (scene, name) = match scene {
        Some(p) => crate::native::open(p)?,
        None => {
            let mut s = Scene::new(NEW_SIZE);
            s.edit(
                (40..88)
                    .flat_map(|x| (0..6).flat_map(move |y| (40..88).map(move |z| ([x, y, z], 12))))
                    .collect::<Vec<_>>(),
            );
            (s, "Untitled".into())
        }
    };
    let mut s = Shot::new(scene, &name, 2.0)?;
    s.ed.ui.coach = false;
    s.frames(3);
    let key = |key, modifiers| egui::Event::Key { key, physical_key: None, pressed: true, repeat: false, modifiers };
    let press = |s: &mut Shot, k, m| s.frames_with(2, vec![key(k, m)]);
    let cmdk = |s: &mut Shot, q: &str| {
        s.frames_with(2, vec![key(egui::Key::K, egui::Modifiers::COMMAND)]);
        s.frames_with(1, vec![egui::Event::Text(q.into())]);
        s.frames_with(2, vec![key(egui::Key::Enter, egui::Modifiers::NONE)]);
    };
    let (shown, tall) = s.ed.scene_in_view();
    check(
        shown > 0.999 && tall > 0.3,
        format!("opened: the scene is in view ({shown:.2} shown, {tall:.2} of the height)"),
    )?;
    let framed = s.ed.cam.distance;
    let (lo, hi) = s.ed.scene.map.occupied_bounds().ok_or("no voxels")?;
    let (lo, hi) = (glam::Vec3::from(lo.map(|v| v as f32)), glam::Vec3::from(hi.map(|v| v as f32)));
    // The camera stays inside the model's bounds (with any voxels outside them), at most
    // MAX_ZOOM_OUT times the distance whose view holds their bounding sphere.
    let size = glam::UVec3::from(s.ed.scene.map.size_voxels()).as_vec3();
    let (blo, bhi) = (lo.min(glam::Vec3::ZERO), hi.max(size));
    let far = (bhi - blo).length() * 0.5 / (s.ed.cam.fov_y * 0.5).sin() * MAX_ZOOM_OUT;
    let mid = [s.ed.width as f32 / 2.0, s.ed.height as f32 / 2.0];
    let drag = |s: &mut Shot, b: Button, d: [f32; 2], steps: usize| {
        s.ed.pointer_moved(Some(mid));
        s.ed.button(b, true, false);
        for k in 1..=steps {
            s.ed.pointer_moved(Some([mid[0] + d[0] * k as f32, mid[1] + d[1] * k as f32]));
        }
        s.ed.button(b, false, false);
    };

    // What used to lose the scene: pan far away, zoom far out, orbit, pan and zoom again.
    let v0 = s.voxels();
    for _ in 0..6 {
        drag(&mut s, Button::Middle, [60.0, 25.0], 40);
        s.ed.pointer_moved(Some([mid[0] * 0.2, mid[1] * 0.3]));
        s.ed.scroll(-40.0);
        drag(&mut s, Button::Right, [35.0, 6.0], 10);
        s.hold(egui::Modifiers::SHIFT);
        drag(&mut s, Button::Right, [-50.0, 40.0], 40);
        s.hold(egui::Modifiers::NONE);
    }
    s.frames(2);
    let (shown, tall) = s.ed.box_in_view(blo, bhi);
    let t = s.ed.cam.target;
    check(
        t.cmpge(blo).all() && t.cmple(bhi).all() && s.voxels() == v0,
        format!("pan and zoom keep the orbit point inside the model's bounds ({t:.0})"),
    )?;
    check(
        s.ed.cam.distance <= far * 1.001 && shown > 0.999 && tall > 0.15,
        format!(
            "zooming out stops at {:.1}x the framing: the bounds stay in view ({shown:.2} shown, {tall:.2} of the height)",
            s.ed.cam.distance / framed
        ),
    )?;

    // A lost camera, as panning and zooming reached before the bounds: every way back works.
    let lose = |s: &mut Shot| {
        s.ed.cam.target += glam::Vec3::new(9000.0, 2500.0, -7000.0);
        s.ed.cam.distance *= 60.0;
        s.ed.cam.yaw += 2.0;
        s.frames(1);
    };
    lose(&mut s);
    let (shown, tall) = s.ed.scene_in_view();
    check(shown < 0.01 || tall < 0.02, format!("a lost camera: {shown:.2} shown, {tall:.2} of the height"))?;
    s.save_png(&dir.join("app-2x-camera-lost.png"))?;
    // Panning and zooming further out from there is held, never jumps back.
    let (t, d) = (s.ed.cam.target, s.ed.cam.distance);
    drag(&mut s, Button::Middle, [80.0, 0.0], 10);
    s.ed.scroll(-10.0);
    check(
        (s.ed.cam.target - t).length() <= (t - (blo + bhi) * 0.5).length() && s.ed.cam.distance == d,
        "from a lost camera pan and zoom do not go further out".into(),
    )?;
    let back = |s: &Shot, what: &str| {
        let (shown, tall) = s.ed.scene_in_view();
        check(
            shown > 0.999 && tall > 0.3,
            format!("{what} brings the scene back ({shown:.2} shown, {tall:.2} of the height)"),
        )
    };
    let yaw = s.ed.cam.yaw;
    press(&mut s, egui::Key::Period, egui::Modifiers::NONE);
    back(&s, ".")?;
    check((s.ed.cam.yaw - yaw).abs() < 1e-6, "framing (.) keeps the angle".into())?;
    lose(&mut s);
    s.ui_click("nav-frame")?;
    back(&s, "the camera bar's Frame scene")?;
    lose(&mut s);
    cmdk(&mut s, "frame scene");
    back(&s, "Cmd-K 'Frame scene'")?;
    lose(&mut s);
    s.ed.cam.ortho = true;
    press(&mut s, egui::Key::Home, egui::Modifiers::NONE);
    back(&s, "Home (reset camera)")?;
    check(
        (s.ed.cam.yaw, s.ed.cam.pitch) == HOME && !s.ed.cam.ortho,
        "Reset camera: the starting angle in perspective".into(),
    )?;
    lose(&mut s);
    s.ui_click("nav-reset")?;
    back(&s, "the camera bar's Reset camera")?;
    lose(&mut s);
    cmdk(&mut s, "reset camera");
    back(&s, "Cmd-K 'Reset camera'")?;

    // Frame selection: the bar's button is off until something is selected.
    check(s.ed.ui.selected == 0, "nothing selected".into())?;
    let c = ((lo + hi) * 0.5).as_ivec3();
    let cells: Vec<[i32; 3]> = (-12..12)
        .flat_map(|x| (-12..12).flat_map(move |y| (-12..12).map(move |z| [c.x + x, c.y + y, c.z + z])))
        .collect();
    s.ed.scene.select(cells, vxr_core::select::Mode::Replace);
    s.frames(1);
    let (slo, shi) = s.ed.scene.selection_bounds().ok_or("nothing selected")?;
    lose(&mut s);
    s.ui_click("nav-frame-sel")?;
    let (slo, shi) = (glam::IVec3::from(slo).as_vec3(), glam::IVec3::from(shi).as_vec3() + 1.0);
    let (shown, tall) = s.ed.box_in_view(slo, shi);
    check(
        shown > 0.999 && tall > 0.5,
        format!("the camera bar's Frame selection fills the view with it ({shown:.2} shown, {tall:.2} of the height)"),
    )?;
    lose(&mut s);
    cmdk(&mut s, "frame selection");
    let (shown, tall) = s.ed.box_in_view(slo, shi);
    check(shown > 0.999 && tall > 0.5, "Cmd-K 'Frame selection'".into())?;
    s.ed.scene.select(Vec::new(), vxr_core::select::Mode::Replace);
    press(&mut s, egui::Key::Home, egui::Modifiers::NONE);

    // Zoom: Cmd-= / Cmd-- step about the middle (the bar has no zoom buttons: the wheel and a
    // pinch zoom); a pinch zooms too.
    let d = s.ed.cam.distance;
    check(
        !s.ed.ui.hits.contains_key("nav-zoom-in") && !s.ed.ui.hits.contains_key("nav-frame-sel"),
        "the camera bar has no zoom buttons, and Frame selection only with a selection".into(),
    )?;
    press(&mut s, egui::Key::Equals, egui::Modifiers::COMMAND);
    let d1 = s.ed.cam.distance;
    press(&mut s, egui::Key::Minus, egui::Modifiers::COMMAND);
    check(
        d1 < d * 0.8 && (s.ed.cam.distance - d).abs() < d * 1e-3 && s.ed.ui.size == 1,
        "Cmd-= and Cmd-- zoom (the brush size stays)".into(),
    )?;
    cmdk(&mut s, "zoom in");
    check(s.ed.cam.distance < d * 0.8, "Cmd-K 'Zoom in'".into())?;
    cmdk(&mut s, "zoom out");
    s.ed.pointer_moved(Some(mid));
    s.ed.pinch(0.5);
    check(s.ed.cam.distance < d * 0.8, format!("a trackpad pinch zooms in ({:.0})", s.ed.cam.distance))?;
    s.ed.pinch(-0.9);
    check(s.ed.cam.distance <= far * 1.001, "a pinch out stops at the same limit".into())?;
    press(&mut s, egui::Key::Home, egui::Modifiers::NONE);

    // Projection and views from the bar.
    s.ui_click("nav-proj")?;
    check(s.ed.cam.ortho, "the bar's Persp button turns orthographic on".into())?;
    s.ui_click("nav-proj")?;
    check(!s.ed.cam.ortho, "and back to perspective".into())?;
    // Where the flyout opens: below the bar (over the render corner) if it ends on screen, else
    // above the bar if it clears the top bar, else left of the bar; never over the top bar or the
    // palette, always on screen.
    fn flyout_at(s: &mut Shot, corner: bool) -> Result<&'static str, String> {
        let points = (s.ed.width as f32 / s.ed.ppp, s.ed.height as f32 / s.ed.ppp);
        let hit = |s: &Shot, n: &str| s.ed.ui.hits.get(n).copied().ok_or(format!("no {n} at {points:?}"));
        let (flyout, nav, palette, top) = (
            hit(s, "view-flyout")?,
            hit(s, "nav")?,
            s.ed.ui.hits.get("palette").copied().unwrap_or(egui::Rect::NOTHING),
            hit(s, "undo")?,
        );
        let screen = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(points.0, points.1));
        let at = if flyout.top() > nav.bottom() {
            "below"
        } else if flyout.bottom() < nav.top() {
            "above"
        } else {
            "beside"
        };
        check(
            s.ed.ui.view_panel
                && s.ed.ui.hits.contains_key("render") == corner
                && screen.contains_rect(flyout)
                && !flyout.intersects(nav)
                && !flyout.intersects(palette)
                && !flyout.intersects(top)
                && (at != "beside" || flyout.right() < nav.left()),
            format!(
                "at {}x{} (render corner {}) the View flyout opens {at} the bar, on screen, clear of the palette and the top bar",
                points.0,
                points.1,
                if corner { "shown" } else { "hidden" }
            ),
        )?;
        Ok(at)
    }
    s.ui_click("nav-views")?;
    check(
        flyout_at(&mut s, true)? == "above",
        "at the default 1280x800 with the render corner shown it opens above the bar".into(),
    )?;
    for (points, corner, want) in [
        ((1440, 1100), true, "above"),
        ((1280, 1000), false, "above"),
        ((1024, 700), true, "above"),
        ((900, 520), true, "above"),
        ((900, 420), true, "beside"),
    ] {
        let mut t = Shot::sized(Scene::new(NEW_SIZE), "Untitled", 1.0, points)?;
        t.ed.ui.coach = false;
        t.ed.ui.show_render = corner;
        t.frames(3);
        t.ui_click("nav-views")?;
        let at = flyout_at(&mut t, corner)?;
        check(at == want, format!("at {}x{} it opens {want} the bar ({at})", points.0, points.1))?;
    }
    s.ui_click("view-front")?;
    check(s.ed.ui.view == Some(View::Front), "its Front button".into())?;
    s.ui_click("view-iso")?;
    s.frames(2);
    s.save_png(&dir.join("app-2x-camera.png"))?;
    press(&mut s, egui::Key::Escape, egui::Modifiers::NONE);
    check(!s.ed.ui.view_panel, "Escape closes it".into())?;
    // Hover: a tooltip with the shortcut (shown in the screenshot), above the button.
    let at = s.at("nav-frame")?;
    s.frames_with(3, vec![egui::Event::PointerMoved(at)]);
    let tip = s.ed.ui.hits.get("nav-tip").copied().ok_or("no tooltip")?;
    check(tip.bottom() < at.y, "the Frame scene tooltip shows above its button".into())?;
    s.save_png(&dir.join("app-2x-camera-tip.png"))?;
    s.frames_with(1, vec![egui::Event::PointerGone]);
    press(&mut s, egui::Key::Home, egui::Modifiers::NONE);

    // Space-drag pans as in MagicaVoxel (no edit), and the hint bar shows the camera moves.
    let (t, yaw, v) = (s.ed.cam.target, s.ed.cam.yaw, s.voxels());
    s.ed.space = true;
    s.frames(1);
    check(s.ed.ui.camera_hints, "holding Space shows the camera moves in the hint bar".into())?;
    s.save_png(&dir.join("app-2x-camera-hints.png"))?;
    drag(&mut s, Button::Left, [-4.0, 3.0], 20);
    s.ed.space = false;
    s.frames(1);
    check(
        (s.ed.cam.target - t).length() > 1.0 && s.ed.cam.yaw == yaw && s.voxels() == v && !s.ed.ui.camera_hints,
        "Space-drag pans, keeping the angle and the voxels".into(),
    )?;

    // X-click: orbit around the voxel clicked, which moves to the middle of the view.
    press(&mut s, egui::Key::Home, egui::Modifiers::NONE);
    let p = s.top_face_pixel([mid[0] + 300.0, mid[1] + 120.0])?;
    s.frames_with(
        1,
        vec![egui::Event::Key {
            key: egui::Key::X,
            physical_key: None,
            pressed: true,
            repeat: false,
            modifiers: egui::Modifiers::NONE,
        }],
    );
    s.stroke(p, &[]);
    s.frames_with(
        1,
        vec![egui::Event::Key {
            key: egui::Key::X,
            physical_key: None,
            pressed: false,
            repeat: false,
            modifiers: egui::Modifiers::NONE,
        }],
    );
    let (w, h) = (s.ed.width as f32, s.ed.height as f32);
    let c = s.ed.cam.view_proj(w / h).project_point3(s.ed.cam.target);
    let moved = (s.ed.cam.target - ((lo + hi) * 0.5)).length();
    check(
        s.voxels() == v && c.x.abs() < 1e-3 && c.y.abs() < 1e-3 && moved > 4.0,
        format!("X-click orbits around the voxel clicked ({:.0}), no edit", s.ed.cam.target),
    )?;

    // One voxel in large bounds: framing fits the voxel, but zooming in toward it does not
    // trap the camera there; zoom-out and pan still reach the whole bounds.
    let mut one = Scene::new(NEW_SIZE);
    let at = glam::Vec3::new(100.5, 0.5, 100.5);
    one.edit(vec![([100, 0, 100], 12)]);
    let mut s = Shot::new(one, "Untitled", 2.0)?;
    s.ed.ui.coach = false;
    s.frames(2);
    let framed = s.ed.cam.distance;
    s.ed.pointer_moved(Some(mid));
    s.ed.scroll(3.0);
    let near = s.ed.cam.distance;
    for _ in 0..10 {
        s.ed.scroll(-10.0);
    }
    let out = s.ed.cam.distance;
    drag(&mut s, Button::Middle, [-40.0, -10.0], 30);
    let t = s.ed.cam.target;
    let size = glam::UVec3::from(NEW_SIZE).as_vec3();
    check(
        near < framed && out > 9.0 * 10.0,
        format!("one voxel: framed at {framed:.0}, zoomed in to {near:.0}, out to {out:.0} voxels"),
    )?;
    check(
        (t - at).length() > 20.0 && t.cmpge(glam::Vec3::ZERO).all() && t.cmple(size).all(),
        format!("one voxel: a pan moves over the bounds, away from it ({t:.0})"),
    )?;
    Ok(())
}

/// The live render corner: it shows, converges, follows an edit without restarting, and
/// Cmd-K hides it.
fn render(s: &mut Shot, dir: &Path) -> Result<(), String> {
    s.frames(2);
    let rect = s.ed.ui.hits.get("render").copied();
    check(rect.is_some(), "the render corner is on screen".into())?;
    let restarts = s.ed.preview.restarts;
    for _ in 0..200 {
        s.frames(1);
        if s.ed.preview.samples() >= 16.0 {
            break;
        }
    }
    let spp = s.ed.preview.samples();
    check(spp >= 16.0, format!("the render corner reaches {spp:.0} spp"))?;
    let img = pollster::block_on(s.ed.preview.read(&s.ed.gpu.device, &s.ed.gpu.queue));
    let lum: Vec<u32> = img.as_chunks::<4>().0.iter().map(|p| p[0] as u32 + p[1] as u32 + p[2] as u32).collect();
    let (lo, hi) = (lum.iter().min().unwrap(), lum.iter().max().unwrap());
    check(hi - lo > 200, format!("the path-traced image has light and shadow (sum of RGB {lo} to {hi})"))?;
    s.save_png(&dir.join("app-2x-render.png"))?;

    s.tool(Tool::Add, Shape::Voxel);
    let mid = [s.ed.width as f32 / 2.0, s.ed.height as f32 / 2.0];
    let p = s.top_face_pixel(mid)?;
    s.stroke(p, &[]);
    s.frames(1);
    check(
        s.ed.preview.restarts == restarts && s.ed.preview.last_rows > 0,
        format!("an edit resets only nearby pixels ({} full restarts)", s.ed.preview.restarts - restarts),
    )?;
    s.ed.undo();

    full_render(s, dir)?;
    render_panel(s, dir)?;

    let key = |key, modifiers| egui::Event::Key { key, physical_key: None, pressed: true, repeat: false, modifiers };
    s.frames_with(2, vec![key(egui::Key::K, egui::Modifiers::COMMAND)]);
    s.frames_with(1, vec![egui::Event::Text("render preview".into())]);
    s.frames_with(2, vec![key(egui::Key::Enter, egui::Modifiers::NONE)]);
    check(!s.ed.ui.show_render && !s.ed.ui.hits.contains_key("render"), "Cmd-K hides the render corner".into())?;
    s.ed.ui.show_render = true;
    s.frames(2);
    Ok(())
}

/// The full render view: a click on the corner swaps the path-traced image into the whole
/// view at one pixel per point, edits still reach it without a restart, and its chip, Cmd-K
/// and Escape swap it back to the 640x360 corner.
fn full_render(s: &mut Shot, dir: &Path) -> Result<(), String> {
    let key = |key, modifiers| egui::Event::Key { key, physical_key: None, pressed: true, repeat: false, modifiers };
    let points = (s.ed.width as f32 / s.ed.ppp, s.ed.height as f32 / s.ed.ppp);
    let size = (points.0.round() as u32, points.1.round() as u32);
    s.ui_click("render")?;
    let rect = s.ed.ui.hits.get("render").copied().unwrap_or(egui::Rect::NOTHING);
    check(
        s.ed.ui.full_render
            && (s.ed.preview.width, s.ed.preview.height) == size
            && rect.width() == points.0
            && rect.height() == points.1,
        format!(
            "a click on the corner fills the view: a {}x{} preview over {:.0}x{:.0} points",
            s.ed.preview.width,
            s.ed.preview.height,
            rect.width(),
            rect.height()
        ),
    )?;
    for _ in 0..400 {
        s.frames(1);
        if s.ed.preview.samples() >= 8.0 {
            break;
        }
    }
    let spp = s.ed.preview.samples();
    // The middle of the view on screen is the middle of the preview image.
    let (dev, queue) = (&s.ed.gpu.device, &s.ed.gpu.queue);
    let img = pollster::block_on(s.ed.preview.read(dev, queue));
    let screen = pollster::block_on(read_texture(dev, queue, &s.target));
    // Mean colour of a 32-point square at the middle, in the image and on screen (BGRA).
    let mean = |px: &[u8], w: u32, h: u32, scale: u32, bgr: bool| {
        let mut sum = [0u64; 3];
        let n = 32 * scale;
        for y in h / 2 - n / 2..h / 2 + n / 2 {
            for x in w / 2 - n / 2..w / 2 + n / 2 {
                let i = ((y * w + x) * 4) as usize;
                for c in 0..3 {
                    sum[c] += px[i + if bgr { 2 - c } else { c }] as u64;
                }
            }
        }
        sum.map(|v| (v / (n * n) as u64) as u8)
    };
    let ppp = s.ed.ppp.round() as u32;
    let (rgb, bgr) = (mean(&img, size.0, size.1, 1, false), mean(&screen, s.ed.width, s.ed.height, ppp, true));
    let near = (0..3).all(|i| (rgb[i] as i32 - bgr[i] as i32).abs() <= 4);
    check(
        spp >= 8.0 && near,
        format!(
            "the full view converges ({spp:.0} spp) and shows the preview: {rgb:?} in the image, {bgr:?} on screen"
        ),
    )?;
    s.save_png(&dir.join("app-2x-render-full.png"))?;
    let restarts = s.ed.preview.restarts;
    let before = s.voxels();
    s.tool(Tool::Add, Shape::Voxel);
    let mid = [s.ed.width as f32 / 2.0, s.ed.height as f32 / 2.0];
    let p = s.top_face_pixel(mid)?;
    s.stroke(p, &[]);
    s.frames(1);
    check(
        s.voxels() == before + 1 && s.ed.preview.restarts == restarts && s.ed.preview.last_rows > 0,
        "an edit in the full view adds its voxel and resets only nearby pixels".into(),
    )?;
    s.ed.undo();
    s.ui_click("render-full")?;
    s.frames(1);
    let corner = (s.ed.preview.width, s.ed.preview.height);
    let back = s.ed.ui.hits.get("render").map(|r| r.width());
    check(
        !s.ed.ui.full_render && corner == (640, 360) && back == Some(208.0),
        format!("its chip swaps back to the corner: a {}x{} preview", corner.0, corner.1),
    )?;
    s.frames_with(2, vec![key(egui::Key::K, egui::Modifiers::COMMAND)]);
    s.frames_with(1, vec![egui::Event::Text("full render".into())]);
    s.frames_with(2, vec![key(egui::Key::Enter, egui::Modifiers::NONE)]);
    let on = s.ed.ui.full_render && s.ed.preview.width == size.0;
    s.frames_with(2, vec![key(egui::Key::Escape, egui::Modifiers::NONE)]);
    check(
        on && !s.ed.ui.full_render && s.ed.preview.width == 640,
        "Cmd-K Full render view fills the view and Escape swaps back".into(),
    )?;
    Ok(())
}

/// Materials through the colour editor's Material tab: Cmd-K opens it, the type buttons and
/// sliders set the current slot's material as one undo step each, the render corner restarts
/// and shows it, it saves to MATL chunks and reopens, and undo restores the file's materials.
fn materials(s: &mut Shot, dir: &Path) -> Result<(), String> {
    use vxr_core::material::Kind;
    let key = |key, modifiers| egui::Event::Key { key, physical_key: None, pressed: true, repeat: false, modifiers };
    let start = s.ed.scene.materials;
    // The most used slot, so the change shows.
    let slot = (1..=255u8).max_by_key(|&i| s.ed.scene.map.counts[i as usize]).unwrap_or(1);
    s.ed.ui.slot = slot;
    s.frames_with(2, vec![key(egui::Key::K, egui::Modifiers::COMMAND)]);
    s.frames_with(1, vec![egui::Event::Text("edit material".into())]);
    s.frames_with(2, vec![key(egui::Key::Enter, egui::Modifiers::NONE)]);
    check(s.ed.ui.hits.contains_key("mt1"), "Cmd-K Edit material opens the Material tab".into())?;
    let restarts = s.ed.preview.restarts;
    s.ui_click("mt1")?;
    let m = s.ed.scene.materials[slot as usize];
    check(
        m.kind == Kind::Metal && s.ed.preview.restarts > restarts && s.ed.ui.hits.contains_key("mat-rough"),
        format!("Metal: slot {slot} is metal and the render corner restarted"),
    )?;
    let r = s.ed.ui.hits["mat-rough"];
    s.ui_drag(r.center(), egui::pos2(r.left() - 20.0, r.center().y), egui::Modifiers::NONE);
    let rough = s.ed.scene.materials[slot as usize].rough;
    s.ed.undo();
    let undone = s.ed.scene.materials[slot as usize];
    s.ed.undo();
    let diffuse = s.ed.scene.materials[slot as usize].kind;
    s.ed.redo();
    s.ed.redo();
    check(
        rough == 0.0
            && undone == m
            && diffuse == start[slot as usize].kind
            && s.ed.scene.materials[slot as usize].rough == 0.0,
        format!("roughness slider drag: {rough}, one undo step (back to {:.2}), the type one more", undone.rough),
    )?;
    // Glass and emissive on two other used slots.
    let mut used: Vec<u8> = (1..=255u8).filter(|&i| i != slot && s.ed.scene.map.counts[i as usize] > 0).collect();
    used.sort_by_key(|&i| std::cmp::Reverse(s.ed.scene.map.counts[i as usize]));
    for (k, name, want) in [(0, "mt2", Kind::Glass), (1, "mt3", Kind::Emit)] {
        let Some(&other) = used.get(k) else { break };
        s.ed.ui.slot = other;
        s.frames(1);
        s.ui_click(name)?;
        let got = s.ed.scene.materials[other as usize].kind;
        check(got == want, format!("{}: slot {other} is {:?}", want.name(), got))?;
    }
    // Glass attenuation dragged full; plastic on a third slot, its specular dragged off and undone.
    if let Some(&glass) = used.first() {
        s.ed.ui.slot = glass;
        s.frames(1);
        let r = *s.ed.ui.hits.get("mat-att").ok_or("no Attenuation slider on glass")?;
        let restarts = s.ed.preview.restarts;
        s.ui_drag(r.center(), egui::pos2(r.right() + 20.0, r.center().y), egui::Modifiers::NONE);
        let att = s.ed.scene.materials[glass as usize].att;
        check(
            att == 1.0 && s.ed.preview.restarts > restarts,
            format!("glass attenuation slider drag: {att}, the render corner restarted"),
        )?;
    }
    if let Some(&other) = used.get(2) {
        s.ed.ui.slot = other;
        s.frames(1);
        s.ui_click("mt4")?;
        let m = s.ed.scene.materials[other as usize];
        let r = *s.ed.ui.hits.get("mat-spec").ok_or("no Specular slider on plastic")?;
        s.ui_drag(r.center(), egui::pos2(r.left() - 20.0, r.center().y), egui::Modifiers::NONE);
        let spec = s.ed.scene.materials[other as usize].spec;
        s.ed.undo();
        let undone = s.ed.scene.materials[other as usize];
        check(
            m.kind == Kind::Plastic && spec == 0.0 && undone == m && m.spec == 1.0,
            format!(
                "Plastic: slot {other} is plastic, specular drag to {spec} and one undo step back to {}",
                undone.spec
            ),
        )?;
    }
    for _ in 0..90 {
        s.frames(1);
    }
    s.save_png(&dir.join("app-2x-material.png"))?;
    // Saved as MATL chunks, read back the same.
    let bytes = s.ed.scene.to_vox();
    let back = Scene::from_vox(&bytes)?;
    let raw = vxr_vox::RawFile::parse(&bytes).map_err(|e| e.to_string())?;
    let n = raw.chunks.iter().filter(|c| c.is(b"MATL")).count();
    let changed = (1..256).filter(|&i| back.materials[i] != start[i]).count();
    check(
        back.materials == s.ed.scene.materials && changed >= 1,
        format!("save and reopen: {changed} changed materials in {n} MATL chunks"),
    )?;
    while s.ed.scene.materials != start && s.ed.scene.history().can_undo() {
        s.ed.undo();
    }
    s.frames_with(2, vec![key(egui::Key::Escape, egui::Modifiers::NONE)]);
    check(
        s.ed.scene.materials == start && s.ed.ui.color_edit.is_none(),
        "undo restores every material; Escape closes the editor".into(),
    )?;
    s.ed.ui.slot = 1;
    s.frames(2);
    Ok(())
}

/// The Render panel through egui input on the drawn UI: R opens it, each setting reaches the
/// path tracer (exposure, gamma and vignette without a restart), Export PNG renders and reads back a PNG at the
/// chosen size, Cmd-K starts an export and the panel's button cancels it, Escape closes it.
fn render_panel(s: &mut Shot, dir: &Path) -> Result<(), String> {
    let key = |key, modifiers| egui::Event::Key { key, physical_key: None, pressed: true, repeat: false, modifiers };
    s.frames_with(2, vec![key(egui::Key::R, egui::Modifiers::NONE)]);
    check(s.ed.ui.render_panel && s.ed.ui.hits.contains_key("rs-export"), "R opens the Render panel".into())?;
    let left = |s: &Shot, name: &str| s.ed.ui.hits.get(name).map(|r| egui::pos2(r.left(), r.center().y));
    let right = |s: &Shot, name: &str| s.ed.ui.hits.get(name).map(|r| egui::pos2(r.right(), r.center().y));
    // Let the corner converge first so the exposure change hits a converged image.
    s.ed.ui.render.spp = 0;
    s.frames(40);
    let restarts = s.ed.preview.restarts;
    let (from, to) = (s.at("rs-exposure")?, left(s, "rs-exposure").unwrap());
    s.ui_drag(from, to, egui::Modifiers::NONE);
    let l = s.ed.preview.lighting;
    check(
        (l.exposure - 0.125).abs() < 1e-3 && s.ed.preview.restarts == restarts && s.ed.preview.max_spp == 64,
        format!(
            "exposure slider: x{:.3}, no restart; Samples 64 stops the corner at {}",
            l.exposure, s.ed.preview.max_spp
        ),
    )?;
    s.ed.ui.render.exposure_ev = 0.0;
    s.frames(2);
    let (from, to) = (s.at("rs-gamma")?, right(s, "rs-gamma").unwrap());
    s.ui_drag(from, to, egui::Modifiers::NONE);
    let (from, to) = (s.at("rs-vignette")?, right(s, "rs-vignette").unwrap());
    s.ui_drag(from, to, egui::Modifiers::NONE);
    let l = s.ed.preview.lighting;
    check(
        (l.gamma - 3.0).abs() < 1e-3 && (l.vignette - 1.0).abs() < 1e-3 && s.ed.preview.restarts == restarts,
        format!("gamma and vignette sliders: gamma {:.2}, vignette {:.2}, no restart", l.gamma, l.vignette),
    )?;
    s.ed.ui.render.gamma = 2.2;
    s.ed.ui.render.vignette = 0.0;
    let sun = s.ed.preview.lighting.sun_dir;
    let (from, to) = (s.at("rs-sun-angle")?, right(s, "rs-sun-angle").unwrap());
    s.ui_drag(from, to, egui::Modifiers::NONE);
    let moved = s.ed.preview.lighting.sun_dir.angle_between(sun).to_degrees();
    check(
        moved > 15.0 && s.ed.preview.restarts > restarts,
        format!("sun direction slider: the sun moved {moved:.0}° and the corner restarted"),
    )?;
    s.type_into("rs-sun-color", "FF8040")?;
    let c = s.ed.preview.lighting.sun_color;
    check(c.x > 4.0 * c.z, format!("sun colour field: #FF8040 gives linear sun ({:.2}, {:.2}, {:.2})", c.x, c.y, c.z))?;
    s.ui_click("rs-sky-backdrop")?;
    let sky = s.ed.preview.lighting.sky_backdrop;
    let (from, to) = (s.at("rs-blur")?, right(s, "rs-blur").unwrap());
    s.ui_drag(from, to, egui::Modifiers::NONE);
    let ap = s.ed.preview.lighting.aperture;
    check(sky && ap > 0.0, format!("sky backdrop toggle on, depth of field slider: lens radius {ap:.1} voxels"))?;
    let restarts = s.ed.preview.restarts;
    s.ui_click("rs-physical-sky")?;
    s.frames(1);
    check(
        s.ed.preview.lighting.physical_sky && s.ed.preview.restarts > restarts,
        "physical sky toggle on: the sky follows the sun and the corner restarted".into(),
    )?;
    for _ in 0..60 {
        s.frames(1);
    }
    s.save_png(&dir.join("app-2x-render-panel.png"))?;
    s.ed.ui.render = vxr_app::render::RenderSettings { spp: 0, size: 0, ..Default::default() };
    s.frames(2);

    // Export at 1280x720, 64 spp.
    s.ui_click("rs-export")?;
    check(s.ed.export.is_some(), "Export PNG starts a 1280×720 render".into())?;
    let t0 = std::time::Instant::now();
    let (mut n, mut sample_ms) = (0, 0.0);
    while s.ed.exported.is_none() && n < 3000 {
        s.frames(1);
        // As a window's vsync would, keep the CPU at most a frame ahead of the GPU.
        let _ = s.ed.gpu.device.poll(wgpu::PollType::wait_indefinitely());
        n += 1;
        if let Some(x) = &s.ed.export {
            sample_ms = x.sample_ms().0;
        }
    }
    let Some((png, (w, h))) = s.ed.exported.take() else {
        return check(false, format!("the export did not finish in {n} frames"));
    };
    let mut r = png::Decoder::new(std::io::Cursor::new(&png)).read_info().map_err(|e| e.to_string())?;
    let mut buf = vec![0; r.output_buffer_size().unwrap_or(0)];
    let info = r.next_frame(&mut buf).map_err(|e| e.to_string())?;
    let lum: Vec<u32> = buf.as_chunks::<4>().0.iter().map(|p| p[0] as u32 + p[1] as u32 + p[2] as u32).collect();
    let (lo, hi) = (lum.iter().min().unwrap(), lum.iter().max().unwrap());
    check(
        (info.width, info.height) == (1280, 720) && (w, h) == (1280, 720) && hi - lo > 200,
        format!(
            "export PNG: {}×{}, {} KB, light and shadow ({lo} to {hi}), 64 spp in {n} frames, {:.2} s \
             ({sample_ms:.2} ms per sample)",
            info.width,
            info.height,
            png.len() / 1024,
            t0.elapsed().as_secs_f64()
        ),
    )?;
    std::fs::write(dir.join("render-export.png"), &png).map_err(|e| e.to_string())?;
    println!("wrote {}", dir.join("render-export.png").display());

    // Cmd-K starts one, the panel's button cancels it.
    s.frames_with(2, vec![key(egui::Key::K, egui::Modifiers::COMMAND)]);
    s.frames_with(1, vec![egui::Event::Text("export png".into())]);
    s.frames_with(2, vec![key(egui::Key::Enter, egui::Modifiers::NONE)]);
    let started = s.ed.export.is_some();
    // An edit during the export reaches the corner once the export ends.
    let restarts = s.ed.preview.restarts;
    s.tool(Tool::Add, Shape::Voxel);
    let mid = [s.ed.width as f32 / 2.0, s.ed.height as f32 / 2.0];
    let p = s.top_face_pixel(mid)?;
    s.stroke(p, &[]);
    s.frames(1);
    s.ui_click("rs-export")?;
    s.frames(1);
    check(
        started && s.ed.export.is_none() && s.ed.preview.restarts > restarts,
        "Cmd-K Export PNG starts an export; the panel's button cancels it; an edit meanwhile restarts the corner"
            .into(),
    )?;
    s.ed.undo();
    s.frames_with(2, vec![key(egui::Key::Escape, egui::Modifiers::NONE)]);
    check(!s.ed.ui.render_panel, "Escape closes the Render panel".into())?;
    s.ed.ui.render = Default::default();
    s.frames(2);
    Ok(())
}

/// The OBJ text's vertex and face counts and its faces' total area (each face a rectangle).
fn obj_stats(obj: &str) -> Result<(usize, usize, f64), String> {
    let v: Vec<[f64; 3]> = obj
        .lines()
        .filter_map(|l| l.strip_prefix("v "))
        .map(|l| {
            let n: Vec<f64> = l.split(' ').filter_map(|x| x.parse().ok()).collect();
            [n[0], n[1], n[2]]
        })
        .collect();
    let (mut faces, mut area) = (0, 0.0);
    for f in obj.lines().filter_map(|l| l.strip_prefix("f ")) {
        let c: Vec<usize> = f.split(' ').filter_map(|c| c.split('/').next()?.parse().ok()).collect();
        let p = |k: usize| v.get(c.get(k)?.checked_sub(1)?).copied();
        let (Some(a), Some(b), Some(d)) = (p(0), p(1), p(3)) else { return Err(format!("bad face: {f}")) };
        let (e1, e2) = ([b[0] - a[0], b[1] - a[1], b[2] - a[2]], [d[0] - a[0], d[1] - a[1], d[2] - a[2]]);
        let n = [e1[1] * e2[2] - e1[2] * e2[1], e1[2] * e2[0] - e1[0] * e2[2], e1[0] * e2[1] - e1[1] * e2[0]];
        area += (n[0] * n[0] + n[1] * n[1] + n[2] * n[2]).sqrt();
        faces += 1;
    }
    Ok((v.len(), faces, area))
}

/// Faces of `voxels` with an empty cell beside them.
fn visible_faces(voxels: &std::collections::BTreeMap<[i32; 3], u8>) -> usize {
    let mut n = 0;
    for p in voxels.keys() {
        for a in 0..3 {
            for d in [-1, 1] {
                let mut q = *p;
                q[a] += d;
                n += !voxels.contains_key(&q) as usize;
            }
        }
    }
    n
}

/// The top bar's Export menu and the OBJ export: the menu opens, closes and opens the Render
/// panel, its OBJ row and Cmd-K ask the host to export, and the files (written as the native
/// host does) cover the model's visible faces, natively in model mode and on 22-world's
/// shown objects in world mode.
fn export(s: &mut Shot, dir: &Path) -> Result<(), String> {
    use vxr_app::files::{obj_files, slices_png};
    let key = |key, modifiers| egui::Event::Key { key, physical_key: None, pressed: true, repeat: false, modifiers };
    s.ed.ui.coach = false;
    s.ui_click("export")?;
    s.frames(2);
    check(
        s.ed.ui.export_menu && s.ed.ui.hits.contains_key("export-obj") && s.ed.ui.hits.contains_key("export-render"),
        "the top bar's Export button opens the Export menu".into(),
    )?;
    s.save_png(&dir.join("app-2x-export.png"))?;
    s.frames_with(2, vec![key(egui::Key::Escape, egui::Modifiers::NONE)]);
    let escaped = !s.ed.ui.export_menu;
    s.ui_click("export")?;
    let mid = egui::pos2(s.ed.width as f32 / s.ed.ppp / 2.0, s.ed.height as f32 / s.ed.ppp / 2.0);
    let voxels = s.voxels();
    s.ui_drag(mid, mid, egui::Modifiers::NONE);
    check(
        escaped && !s.ed.ui.export_menu && s.voxels() == voxels,
        "Escape closes it, and so does a click outside it, which edits nothing".into(),
    )?;
    if s.voxels() != voxels {
        s.ed.undo();
    }
    s.ui_click("export")?;
    s.ui_click("export-render")?;
    check(s.ed.ui.render_panel && !s.ed.ui.export_menu, "its Render PNG row opens the Render panel".into())?;
    s.frames_with(2, vec![key(egui::Key::Escape, egui::Modifiers::NONE)]);

    s.host.clear();
    s.ui_click("export")?;
    s.ui_click("export-obj")?;
    check(
        s.host.contains(&Action::ExportObj) && !s.ed.ui.export_menu,
        "its OBJ row asks the host to export and closes the menu".into(),
    )?;
    s.host.clear();
    s.frames_with(2, vec![key(egui::Key::K, egui::Modifiers::COMMAND)]);
    s.frames_with(1, vec![egui::Event::Text("export obj".into())]);
    s.frames_with(2, vec![key(egui::Key::Enter, egui::Modifiers::NONE)]);
    check(s.host.contains(&Action::ExportObj), "Cmd-K Export OBJ asks the host too".into())?;
    s.host.clear();
    s.ui_click("export")?;
    s.ui_click("export-slices")?;
    check(
        s.host.contains(&Action::ExportSlices) && !s.ed.ui.export_menu,
        "its PNG slices row asks the host to export and closes the menu".into(),
    )?;
    s.host.clear();
    s.frames_with(2, vec![key(egui::Key::K, egui::Modifiers::COMMAND)]);
    s.frames_with(1, vec![egui::Event::Text("export png slices".into())]);
    s.frames_with(2, vec![key(egui::Key::Enter, egui::Modifiers::NONE)]);
    check(s.host.contains(&Action::ExportSlices), "Cmd-K Export PNG slices asks the host too".into())?;

    // The slices the native host writes: one tile per layer, every voxel at its cell.
    let decode = |bytes: &[u8]| -> Result<(u32, u32, Vec<u8>), String> {
        let mut r = png::Decoder::new(std::io::Cursor::new(bytes)).read_info().map_err(|e| e.to_string())?;
        let mut px = vec![0; r.output_buffer_size().unwrap_or(0)];
        let info = r.next_frame(&mut px).map_err(|e| e.to_string())?;
        Ok((info.width, info.height, px))
    };
    let (png_bytes, _, layers) = slices_png(&s.ed.scene)?;
    std::fs::write(dir.join(s.ed.slices_name()), &png_bytes).map_err(|e| e.to_string())?;
    let (w, h, px) = decode(&png_bytes)?;
    let size = s.ed.scene.bounds().size;
    let (mut n, mut right) = (0usize, 0usize);
    s.ed.scene.for_each_voxel(|p, c| {
        let at = (p[2] as usize * w as usize + p[1] as usize * size[0] as usize + p[0] as usize) * 4;
        let [r, g, b, _] = s.ed.scene.palette[c as usize];
        n += 1;
        right += (px[at..at + 4] == [r, g, b, 255]) as usize;
    });
    let filled = px.chunks(4).filter(|p| p[3] != 0).count();
    check(
        (w, h, layers) == (size[0] * size[1], size[2], size[1]) && right == n && filled == n,
        format!(
            "{}: {layers} layers of {}×{} side by side ({w}×{h}), all {n} voxels at their cells",
            s.ed.slices_name(),
            size[0],
            size[2]
        ),
    )?;

    // What the native host writes, and what it holds.
    let stem = s.ed.export_stem();
    let (files, quads) = obj_files(&s.ed.scene, &stem)?;
    let out = dir.join("obj");
    std::fs::create_dir_all(&out).map_err(|e| e.to_string())?;
    for (name, bytes) in &files {
        std::fs::write(out.join(name), bytes).map_err(|e| e.to_string())?;
    }
    let obj = String::from_utf8_lossy(&files[0].1).into_owned();
    let mtl = String::from_utf8_lossy(&files[1].1).into_owned();
    let (nv, nf, area) = obj_stats(&obj)?;
    let mut shown = std::collections::BTreeMap::new();
    s.ed.scene.for_each_voxel(|p, c| {
        shown.insert(p, c);
    });
    let faces = visible_faces(&shown);
    check(
        nf == quads && area as usize == faces && quads * 2 < faces,
        format!(
            "{stem}.obj: {nv} vertices, {quads} quads covering all {faces} visible faces of {} voxels",
            shown.len()
        ),
    )?;
    let mut r = png::Decoder::new(std::io::Cursor::new(&files[2].1)).read_info().map_err(|e| e.to_string())?;
    let mut tex = vec![0; r.output_buffer_size().unwrap_or(0)];
    let info = r.next_frame(&mut tex).map_err(|e| e.to_string())?;
    let slot = (1..256).find(|&i| s.ed.scene.map.counts[i] > 0).unwrap_or(1);
    check(
        (info.width, info.height) == (256, 1)
            && tex[(slot - 1) * 4..slot * 4] == s.ed.scene.palette[slot]
            && obj.contains(&format!("mtllib {stem}.mtl"))
            && mtl.contains(&format!("map_Kd {stem}.png")),
        format!("{stem}.png is the 256×1 palette (texel {} is slot {slot}), named by {stem}.mtl", slot - 1),
    )?;
    println!("wrote {}", out.display());

    // World mode exports every shown object; a hidden one is left out.
    let bytes = vxr_vox::fixtures::all()
        .into_iter()
        .find(|f| f.name == "22-world")
        .ok_or("no 22-world fixture")?
        .builder
        .build()
        .write();
    let mut w = Shot::new(Scene::from_vox(&bytes)?, "22-world.vox", 1.0)?;
    w.frames(2);
    let all = w.ed.scene.world_voxels(false);
    let shown = w.ed.scene.world_voxels(true);
    let (files, quads) = obj_files(&w.ed.scene, &w.ed.export_stem())?;
    let (_, nf, area) = obj_stats(&String::from_utf8_lossy(&files[0].1))?;
    check(
        nf == quads && area as usize == visible_faces(&shown) && shown.len() < all.len(),
        format!(
            "in world mode 22-world.obj covers its {} shown voxels' {} faces ({} hidden left out)",
            shown.len(),
            visible_faces(&shown),
            all.len() - shown.len()
        ),
    )?;
    let (png_bytes, _, _) = slices_png(&w.ed.scene)?;
    let (_, _, px) = decode(&png_bytes)?;
    let filled = px.chunks(4).filter(|p| p[3] != 0).count();
    check(filled == shown.len(), format!("in world mode 22-world-slices.png holds its {filled} shown voxels"))?;
    Ok(())
}

/// A small stone house with a two-layer roof (slot 20) on hollow walls (slot 10), in the
/// middle of a 32^3 model: world x and z from -16, y from 0.
fn house() -> Scene {
    let mut s = Scene::new([32, 32, 32]);
    let mut cells = Vec::new();
    for x in -6..=5 {
        for z in -6..=5 {
            for y in 0..=5 {
                if x == -6 || x == 5 || z == -6 || z == 5 {
                    cells.push(([x, y, z], 10u8));
                }
            }
        }
    }
    for x in -7..=6 {
        for z in -7..=6 {
            cells.extend([([x, 6, z], 20u8), ([x, 7, z], 20u8)]);
        }
    }
    let cells: Vec<_> = cells.into_iter().map(|(p, v)| (s.to_map(p), v)).collect();
    s.edit(cells);
    s
}

/// The house request: repaint the roof (392 cells) into slot 3 and make it red, add a flag on
/// a pole (10 + 12 cells) and open a door (6 cells), then done.
const HOUSE_SCRIPT: &str = r##"{"prompt": "make the roof red, add a flag and open a door", "calls": [
    {"name": "paint", "input": {"slot": 3, "from_slot": 20}},
    {"name": "palette_set", "input": {"slot": 3, "hex": "#D94A3A"}},
    {"name": "line", "input": {"from": [8, 0, 8], "to": [8, 9, 8], "slot": 40, "name": "flag pole"}},
    {"name": "box", "input": {"from": [9, 7, 8], "to": [12, 9, 8], "slot": 50, "name": "flag"}},
    {"name": "box", "input": {"from": [-1, 0, 5], "to": [0, 2, 5], "mode": "erase"}},
    {"name": "done", "input": {"summary": "Painted the roof red, added a flag on a pole and opened a door"}}
]}"##;

/// The palette opens below the rail's flyouts showing beside it (the mirror and lock axes, the
/// selection) and never over them, and its colour editor opens right of it, clear of the rail
/// and the scene tree on 22-world. At 1280x800.
fn palette_beside_flyouts() -> Result<(), String> {
    let rect = |s: &Shot, name: &str| s.ed.ui.hits.get(name).copied().ok_or(format!("no {name} on screen"));
    let on_screen = |s: &Shot, r: egui::Rect| r.top() >= 0.0 && r.bottom() <= s.ed.height as f32 / 2.0;
    let mut s = Shot::new(house(), "house.vox", 2.0)?;
    s.ed.ui.coach = false;
    s.frames(2);
    s.ui_click("mirror")?;
    s.ui_click("rail-color")?;
    s.frames(2);
    let (axes, pal) = (rect(&s, "axes")?, rect(&s, "palette")?);
    check(
        s.ed.ui.mirror && !pal.intersects(axes) && on_screen(&s, pal),
        format!("with mirror on the palette opens clear of the axes flyout: {pal:?}, {axes:?}"),
    )?;
    s.ui_click("mirror")?;
    s.tool(Tool::Select, Shape::Voxel);
    s.frames(2);
    let (sel, pal) = (rect(&s, "selection")?, rect(&s, "palette")?);
    check(
        !pal.intersects(sel) && on_screen(&s, pal),
        format!("with the Select tool the palette opens clear of the selection flyout: {pal:?}, {sel:?}"),
    )?;
    let bytes = vxr_vox::fixtures::all()
        .into_iter()
        .find(|f| f.name == "22-world")
        .ok_or("no 22-world fixture")?
        .builder
        .build()
        .write();
    let mut s = Shot::new(Scene::from_vox(&bytes)?, "22-world.vox", 2.0)?;
    s.ed.ui.coach = false;
    s.frames(3);
    s.ui_click("rail-color")?;
    s.ui_click("edit-color")?;
    s.frames(2);
    let (pal, ed, color) = (rect(&s, "palette")?, rect(&s, "color-edit")?, rect(&s, "rail-color")?);
    let tree: Vec<_> =
        s.ed.ui.tree.iter().filter_map(|r| s.ed.ui.hits.get(&format!("tree-{}", r.node)).copied()).collect();
    check(
        !tree.is_empty()
            && (ed.left() - pal.right() - 8.0).abs() < 0.5
            && ed.left() > color.right()
            && tree.iter().all(|r| !ed.intersects(*r) && !pal.intersects(*r)),
        format!("with the scene tree the colour editor opens right of the palette, clear of the rail and tree: {ed:?}"),
    )?;
    Ok(())
}

/// The palette is a flyout from the rail's colour button and the assistant a column of its own:
/// closed by default, the palette opens beside the rail level with its colour, its colour editor
/// beside it; the assistant runs the right column's whole height with nothing of the palette in
/// it, the change under review in its thread, clear of the camera bar and the render corner.
/// Escape closes the editor, then the palette. At 1280x800.
fn assistant_column(dir: &Path) -> Result<(), String> {
    let mut s = Shot::new(house(), "house.vox", 2.0)?;
    s.ed.ui.coach = false;
    s.ed.cam.distance *= 1.9;
    let h = s.ed.height as f32;
    s.ed.cam.pan(-420.0, 0.0, h);
    s.frames(2);
    let window = s.ed.width as f32 / 2.0;
    let rect = |s: &Shot, name: &str| s.ed.ui.hits.get(name).copied().ok_or(format!("no {name} on screen"));
    let (lock, color) = (rect(&s, "lock")?, rect(&s, "rail-color")?);
    check(
        !s.ed.ui.palette_open && !s.ed.ui.hits.contains_key("palette"),
        "closed: the palette is not on screen by default".into(),
    )?;
    check(
        (color.left() - lock.left()).abs() < 0.5 && color.top() > lock.bottom(),
        format!("the rail's colour button sits under the axis lock: {color:?}"),
    )?;
    s.ui_click("rail-color")?;
    s.frames(2);
    let pal = rect(&s, "palette")?;
    check(
        s.ed.ui.palette_open
            && pal.left() > color.right()
            && pal.left() - color.right() < 20.0
            && pal.top() <= color.top(),
        format!("the colour button opens the palette beside the rail: {pal:?}"),
    )?;
    let key = |key, modifiers| egui::Event::Key { key, physical_key: None, pressed: true, repeat: false, modifiers };
    s.ui_click("edit-color")?;
    s.frames(2);
    let ed = rect(&s, "color-edit")?;
    check(
        (ed.left() - pal.right() - 8.0).abs() < 0.5
            && ed.top() <= pal.top()
            && ed.top() >= 60.0
            && ed.bottom() <= s.ed.height as f32 / 2.0 - 12.0,
        format!("the colour editor opens beside the palette, level with it as far as the window lets: {ed:?}"),
    )?;
    let hint = s.ed.ui.hits.get("hint").copied();
    check(
        hint.is_none_or(|h| !h.intersects(ed) && !h.intersects(pal)),
        format!("the hint bar keeps clear of the palette and the colour editor: {hint:?}"),
    )?;
    let at = s.at("edit-color")?;
    s.frames_with(3, vec![egui::Event::PointerMoved(at)]);
    let (tip, all) = (rect(&s, "edit-tip")?, rect(&s, "palette-all")?);
    check(
        !tip.intersects(all) && !tip.intersects(rect(&s, "palette-used")?),
        format!("the Edit color tooltip keeps off the palette's Used / All toggle: {tip:?}"),
    )?;
    s.save_png(&dir.join("app-2x-palette-rail.png"))?;
    s.frames_with(1, vec![egui::Event::PointerGone]);
    s.frames_with(2, vec![key(egui::Key::Escape, egui::Modifiers::NONE)]);
    check(s.ed.ui.color_edit.is_none() && s.ed.ui.palette_open, "Escape closes the editor first".into())?;
    s.frames_with(2, vec![key(egui::Key::Escape, egui::Modifiers::NONE)]);
    check(!s.ed.ui.palette_open && !s.ed.ui.hits.contains_key("palette"), "then the palette".into())?;
    s.ed.ui.ai_open = true;
    s.ui_click("rail-color")?;
    s.frames(4);
    let (pal, ai) = (rect(&s, "palette")?, rect(&s, "assistant")?);
    s.save_png(&dir.join("app-2x-column.png"))?;
    check(
        (ai.right() - (window - 12.0)).abs() < 0.5
            && (ai.top() - 60.0).abs() < 0.5
            && (ai.bottom() - (s.ed.height as f32 / 2.0 - 12.0)).abs() < 0.5,
        format!("open: the assistant runs the column's whole height at the right edge: {ai:?}"),
    )?;
    check(
        pal.right() < ai.left() - 100.0 && pal.left() < 120.0,
        format!("the palette stays beside the rail, apart from the assistant: {pal:?}, {ai:?}"),
    )?;
    let (nav, render) = (rect(&s, "nav")?, rect(&s, "render")?);
    check(
        nav.right() < ai.left() && render.right() < ai.left(),
        "open: the camera bar and the render corner sit left of the column".into(),
    )?;
    // A change under review sits in the thread.
    let answers = s.ed.stage_json(HOUSE_SCRIPT)?;
    check(answers[0].len() == 6, "a scripted request stages the card for the layout check".into())?;
    s.frames(40);
    let (pal, ai, card) = (rect(&s, "palette")?, rect(&s, "assistant")?, rect(&s, "review")?);
    s.save_png(&dir.join("app-2x-column-review.png"))?;
    check(
        ai.contains_rect(card) || (card.left() >= ai.left() && card.right() <= ai.right()),
        format!("the card sits in the assistant's thread: {card:?} in {ai:?}"),
    )?;
    check(
        !card.intersects(pal) && s.ed.ui.hits.contains_key("review-apply"),
        "the card clears the palette and shows Apply".into(),
    )?;
    s.ui_click("rail-color")?;
    s.frames(2);
    check(!s.ed.ui.palette_open, "the colour button closes the palette again".into())?;
    Ok(())
}

/// With an agent connected, the palette and its colour editor open on a narrow window squeeze
/// the hint row: the hints hide and the agent's chip stays, its Stop on screen, on top and clear
/// of the palette and the colour editor. At 900x560, and at 900x420 with every slot shown, the
/// palette down to the bottom.
fn agent_chip_squeezed(dir: &Path) -> Result<(), String> {
    for (h, all) in [(560, false), (420, true)] {
        let mut many = house();
        let cells: Vec<_> =
            (1..=96u8).map(|v| (many.to_map([i32::from(v % 12) - 6, 8 + i32::from(v / 12), 0]), v)).collect();
        many.edit(cells);
        let mut s = Shot::sized(many, "house.vox", 2.0, (900, h))?;
        s.ed.ui.coach = false;
        s.frames(2);
        let now = s.ed.ctx.input(|i| i.time);
        let a = &mut s.ed.agents;
        (a.available, a.allowed, a.since) = (true, true, Some(now - 240.0));
        a.clients = vec![(1, "Claude Code".to_string())];
        a.note_clients(now - 240.0);
        s.ui_click("rail-color")?;
        if all {
            s.ui_click("palette-all")?;
        }
        s.ui_click("edit-color")?;
        s.frames(3);
        let rect = |s: &Shot, name: &str| s.ed.ui.hits.get(name).copied().ok_or(format!("no {name} on screen"));
        let (pal, ed, stop, chip) =
            (rect(&s, "palette")?, rect(&s, "color-edit")?, rect(&s, "agent-stop")?, rect(&s, "agent-chip")?);
        let screen = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(900.0, h as f32));
        let top = s.ed.ctx.layer_id_at(stop.center()).map(|l| l.id);
        s.save_png(&dir.join(format!("app-2x-agent-squeezed-{h}.png")))?;
        check(
            (!all || pal.bottom() > screen.bottom() - 12.0 - 32.8 - 4.0 - 28.0)
                && screen.contains_rect(stop)
                && top == Some(egui::Id::unique("hint"))
                && [pal, ed].iter().all(|r| !r.intersects(stop) && !r.intersects(chip)),
            format!(
                "at 900x{h}, squeezed by the colour editor, the agent's Stop stays on screen, on top and clear: \
                 {stop:?}, {pal:?}, {ed:?}"
            ),
        )?;
    }
    Ok(())
}

/// The Agents tab's checklist in every state it has: the browser's twelve (not paired, typing, connecting,
/// a wrong code, paired with no agent, one agent editing, several, the bridge stopped, local access
/// refused, Safari or Brave blocked) and the desktop app's two, each with the hits it offers and its
/// tab label, in Neutral, and three of them in Paper. The code field's typing, pasting and refusing.
fn agents_states(dir: &Path) -> Result<(), String> {
    use vxr_app::agents::Fail;
    use vxr_app::theme::ThemeChoice;
    let has = |s: &Shot, ks: &[&str]| ks.iter().all(|k| s.ed.ui.hits.contains_key(*k));
    let lacks = |s: &Shot, ks: &[&str]| ks.iter().all(|k| !s.ed.ui.hits.contains_key(*k));
    // (name, web, allowed, connecting, fail, agents (name, last call seconds ago), shot name)
    type State = (&'static str, bool, bool, bool, Option<Fail>, &'static [(&'static str, Option<f64>)]);
    let states: [State; 12] = [
        ("not-paired", true, false, false, None, &[]),
        ("typing", true, false, false, None, &[]),
        ("connecting", true, false, true, None, &[]),
        ("wrong-code", true, false, false, Some(Fail::Wrong), &[]),
        ("paired", true, true, false, None, &[]),
        ("editing", true, true, false, None, &[("Claude Code", Some(1.0))]),
        ("several", true, true, false, None, &[("Claude Code", Some(1.0)), ("Codex", Some(38.0))]),
        ("bridge-stopped", true, false, false, Some(Fail::Lost), &[]),
        ("denied", true, false, false, Some(Fail::Denied), &[]),
        ("safari", true, false, false, Some(Fail::Safari), &[]),
        ("desktop-off", false, false, false, None, &[]),
        ("desktop-live", false, true, false, None, &[("Claude Code", Some(1.0))]),
    ];
    for theme in [ThemeChoice::Neutral, ThemeChoice::Paper] {
        for (name, web, allowed, connecting, fail, agents) in states {
            if theme == ThemeChoice::Paper && !["several", "not-paired", "wrong-code"].contains(&name) {
                continue;
            }
            let mut s = Shot::new(house(), "house.vox", 2.0)?;
            s.ed.ui.theme = theme;
            s.ed.ui.coach = false;
            s.frames(2);
            let now = s.ed.ctx.input(|i| i.time);
            let a = &mut s.ed.agents;
            (a.available, a.web, a.allowed, a.connecting, a.fail) = (true, web, allowed, connecting, fail);
            a.error = fail.map(|_| "the link failed".to_string());
            a.since = allowed.then_some(now - 240.0);
            a.clients = agents.iter().enumerate().map(|(i, (n, _))| (i as u64 + 1, n.to_string())).collect();
            a.note_clients(now - 240.0);
            for (i, (_, last)) in agents.iter().enumerate() {
                if let Some(ago) = last {
                    a.note_call(i as u64 + 1, now - ago, "box");
                }
            }
            s.ed.ui.open_agents();
            s.frames(6);
            if name == "typing" {
                s.type_into("agents-code", "k7qf3")?;
                s.frames(4);
            }
            let ok = match name {
                "not-paired" | "typing" => {
                    has(&s, &["agents-copy-bridge", "agents-code"]) && lacks(&s, &["agents-disconnect", "agents-retry"])
                }
                "connecting" => has(&s, &["agents-cancel"]) && lacks(&s, &["agents-retry", "agents-disconnect"]),
                "wrong-code" | "denied" => has(&s, &["agents-code", "agents-retry"]) && lacks(&s, &["agents-desktop"]),
                "bridge-stopped" => has(&s, &["agents-copy-bridge", "agents-retry", "agents-code"]),
                "safari" => has(&s, &["agents-desktop", "agents-code"]) && lacks(&s, &["agents-retry"]),
                "paired" => {
                    has(
                        &s,
                        &["agents-copy", "agents-copy-live", "agents-review", "agents-disconnect", "agents-regenerate"],
                    ) && lacks(&s, &["agents-code"])
                }
                "editing" => {
                    has(&s, &["agents-stop-1", "agents-review", "agents-disconnect"]) && lacks(&s, &["agents-copy"])
                }
                "several" => has(&s, &["agents-stop-1", "agents-stop-2", "agents-review", "agents-disconnect"]),
                "desktop-off" => {
                    has(&s, &["agents-allow"]) && lacks(&s, &["agents-code", "agents-copy", "agents-review"])
                }
                _ => {
                    has(&s, &["agents-allow", "agents-stop-1", "agents-review", "agents-regenerate"])
                        && lacks(&s, &["agents-code", "agents-disconnect"])
                }
            };
            check(ok, format!("{name}: the checklist offers its own controls"))?;
            let hits_ok = has(&s, &["ai-tab-agents", "agents-panel"]);
            check(hits_ok, format!("{name}: the Agents tab shows"))?;
            let suffix = if theme == ThemeChoice::Paper { "-paper" } else { "" };
            s.save_png(&dir.join(format!("app-2x-agents-{name}{suffix}.png")))?;
            if name == "typing" {
                check(s.ed.ui.agents_code == "K7QF3", "typing: upper case as typed".into())?;
            }
        }
    }
    // The code field: a paste in any form fills every cell, and the eighth character connects.
    let mut s = Shot::new(house(), "house.vox", 2.0)?;
    s.ed.ui.coach = false;
    s.frames(2);
    let a = &mut s.ed.agents;
    (a.available, a.web) = (true, true);
    s.ed.ui.open_agents();
    s.frames(4);
    s.ui_click("agents-code")?;
    s.frames_with(2, vec![egui::Event::Paste("k7qf 3mxp".into())]);
    s.frames(2);
    check(s.ed.ui.agents_code == "K7QF3MXP", format!("a paste in any form fills all cells: {}", s.ed.ui.agents_code))?;
    check(s.host.contains(&Action::PairBridge), "the eighth character connects".into())?;
    s.host.clear();
    s.ed.ui.agents_code.clear();
    s.frames_with(2, vec![egui::Event::Paste("o1l".into())]);
    check(s.ed.ui.agents_code == "011", format!("O reads as 0, I and L as 1: {}", s.ed.ui.agents_code))?;
    s.frames_with(2, vec![egui::Event::Text("u".into())]);
    check(s.ed.ui.agents_code == "011", "a character no code has is refused".into())?;
    s.save_png(&dir.join("app-2x-agents-code.png"))?;
    Ok(())
}

/// A palette of eight swatch rows (over 104 colours), opened, in a 1280x800 window with the
/// assistant open and a change under review: the palette stays on screen beside the rail and the
/// column holds, clear of the camera bar and the render corner. In a 1280x520 window the rail
/// goes on in a second column above the stats, and the palette still opens on screen.
fn column_short_window(dir: &Path) -> Result<(), String> {
    let mut scene = house();
    let cells: Vec<_> =
        (0..120u8).map(|i| (scene.to_map([i as i32 % 32 - 16, 0, -10 - i as i32 / 32]), 100 + i)).collect();
    scene.edit(cells.clone());
    let mut s = Shot::new(scene, "house.vox", 2.0)?;
    s.ed.ui.coach = false;
    s.ed.ui.palette_open = true;
    s.ed.ui.ai_open = true;
    s.frames(2);
    let answers = s.ed.stage_json(HOUSE_SCRIPT)?;
    check(answers[0].len() == 6, "a scripted request stages the card for the short window check".into())?;
    s.frames(40);
    let rect = |s: &Shot, name: &str| s.ed.ui.hits.get(name).copied().ok_or(format!("no {name} on screen"));
    let (palette, ai, card, nav, render) =
        (rect(&s, "palette")?, rect(&s, "assistant")?, rect(&s, "review")?, rect(&s, "nav")?, rect(&s, "render")?);
    s.save_png(&dir.join("app-2x-column-short.png"))?;
    check(palette.height() > 270.0, format!("the palette shows eight swatch rows ({:.0} tall)", palette.height()))?;
    let screen =
        egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(s.ed.width as f32 / 2.0, s.ed.height as f32 / 2.0));
    check(
        screen.contains_rect(ai)
            && screen.contains_rect(palette)
            && !ai.intersects(palette)
            && card.left() >= ai.left()
            && card.right() <= ai.right(),
        format!(
            "the palette stays on screen apart from the column and the card in it: {palette:?}, {card:?} in {ai:?}"
        ),
    )?;
    check(
        [nav, render].iter().all(|r| !ai.intersects(*r)),
        "the camera bar and the render corner stay clear of the column".into(),
    )?;
    let mut scene = house();
    scene.edit(cells);
    let mut s = Shot::sized(scene, "house.vox", 2.0, (1280, 520))?;
    s.ed.ui.coach = false;
    s.frames(2);
    let (tool, lock, color) = (rect(&s, "select")?, rect(&s, "lock")?, rect(&s, "rail-color")?);
    let stats_top = 520.0 - 12.0 - 28.0;
    check(
        color.bottom() < stats_top && lock.left() > tool.right() && (color.left() - lock.left()).abs() < 0.5,
        format!("short: the rail's last section goes on in a column beside, above the stats: {lock:?} {color:?}"),
    )?;
    s.ui_click("rail-color")?;
    s.frames(2);
    let palette = rect(&s, "palette")?;
    check(
        palette.left() > color.right() && palette.top() >= 60.0 && palette.bottom() <= 520.0 - 12.0,
        format!("short: the palette opens beside the wider rail, on screen: {palette:?}"),
    )?;
    s.save_png(&dir.join("app-2x-rail-short.png"))?;
    Ok(())
}

/// The review step of an AI change, driven by scripted tool calls through the same session,
/// card and ghost the assistant will use: exact counts, the ghost in the view and its
/// removals hidden from the render corner, Before and After, a dropped step, every other edit
/// kept out, Discard (button and Escape) leaving no step, Apply (Enter) as one undo step named
/// after the request, a change over 10,000 cells held until Allow or Stop, and the ask field.
fn review(dir: &Path) -> Result<(), String> {
    let mut s = Shot::new(house(), "house.vox", 2.0)?;
    s.ed.ui.coach = false;
    // Back off so the house, its flag and the card all show.
    s.ed.cam.distance *= 1.9;
    let h = s.ed.height as f32;
    s.ed.cam.pan(-420.0, 0.0, h);
    s.frames(2);
    let (v0, counts0, palette0) = (s.voxels(), s.ed.scene.map.counts, s.ed.scene.palette);
    let position = s.ed.scene.history().position();
    let same = |s: &Shot| s.voxels() == v0 && s.ed.scene.map.counts == counts0 && s.ed.scene.palette == palette0;
    let key = |key, modifiers| egui::Event::Key { key, physical_key: None, pressed: true, repeat: false, modifiers };
    // A change under review opens the assistant, which narrows the view: start with it open.
    s.ed.ui.ai_open = true;
    s.frames(2);
    let plain = view_pixels(&mut s);
    let answers = s.ed.stage_json(HOUSE_SCRIPT)?;
    let ok = answers[0].iter().all(|a| matches!(a, vxr_app::review::Answer::Ok(_)));
    check(ok && answers[0].len() == 6, format!("a scripted request stages its 6 calls: {answers:?}"))?;
    s.frames(3);
    let card = s.ed.ui.review.clone().ok_or("no review card")?;
    check(
        card.counts == [22, 6, 392] && card.slots.len() == 1 && card.slots[0].slot == 3,
        format!(
            "card counts: +{} added, {} removed, {} repainted, slots {:?}",
            card.counts[0],
            card.counts[1],
            card.counts[2],
            card.slots.iter().map(|c| c.slot).collect::<Vec<_>>()
        ),
    )?;
    check(
        card.steps.len() == 5 && card.summary.is_some() && s.ed.ui.hits.contains_key("review-apply"),
        format!("card: {} steps, the model's summary, Apply on screen", card.steps.len()),
    )?;
    // Staged, not applied: removals stay in the map, the history has no new step.
    check(
        s.voxels() == v0 + 22 && s.ed.scene.history().position() == position,
        format!("staged on the scene: {} voxels in the map (removals kept), no undo step", s.voxels()),
    )?;
    let ghost = view_pixels(&mut s);
    let changed = differ(&ghost, &plain);
    check(changed > 2000, format!("the ghost and the red roof change {changed} pixels of the view"))?;
    s.frames(40);
    s.save_png(&dir.join("app-2x-review.png"))?;
    // Every other edit waits: a click in the view, Cmd-Z, Tab, Backspace, Cmd-S.
    let p = s.top_face_pixel([s.ed.width as f32 / 2.0, s.ed.height as f32 / 2.0])?;
    s.tool(Tool::Add, Shape::Voxel);
    s.stroke(p, &[]);
    s.frames_with(2, vec![key(egui::Key::Z, egui::Modifiers::COMMAND)]);
    s.frames_with(2, vec![key(egui::Key::Tab, egui::Modifiers::NONE)]);
    s.frames_with(2, vec![key(egui::Key::S, egui::Modifiers::COMMAND)]);
    check(
        s.ed.review.is_some() && s.voxels() == v0 + 22 && !s.host.contains(&Action::Save),
        "a click, Cmd-Z, Tab and Cmd-S wait while the change is under review".into(),
    )?;
    // Before shows the scene as it was, with no ghost; After shows the change again.
    s.ui_click("review-before")?;
    s.frames(3);
    check(same(&s) && s.ed.ui.review.as_ref().is_some_and(|c| c.before), "Before: the scene as it was".into())?;
    let off = differ(&view_pixels(&mut s), &plain);
    check(off == 0, format!("Before draws the view as the scene was: {off} pixels differ"))?;
    s.frames(40);
    s.save_png(&dir.join("app-2x-review-before.png"))?;
    s.ui_click("review-after")?;
    let back = differ(&view_pixels(&mut s), &ghost);
    check(s.voxels() == v0 + 22 && back == 0, format!("After: the change again ({back} pixels differ)"))?;
    // Dropping the door step takes its 6 removals out, and back. The card stops above the
    // render corner, so its step list scrolls to the door.
    s.frames(2);
    if !s.ed.ui.hits.contains_key("review-step-4") {
        let over = s.at("review-step-0")?;
        let wheel = egui::Event::MouseWheel {
            unit: egui::MouseWheelUnit::Point,
            delta: egui::vec2(0.0, -200.0),
            phase: egui::TouchPhase::Move,
            source: egui::MouseWheelSource::Unknown,
            modifiers: egui::Modifiers::NONE,
        };
        s.frames_with(1, vec![egui::Event::PointerMoved(over)]);
        s.frames_with(20, vec![wheel]);
    }
    s.ui_click("review-step-4")?;
    let c = s.ed.ui.review.clone().ok_or("no card")?;
    check(c.counts == [22, 0, 392] && c.steps[4].dropped, format!("dropping the door step: {:?}", c.counts))?;
    s.ui_click("review-step-4")?;
    check(s.ed.ui.review.as_ref().is_some_and(|c| c.counts == [22, 6, 392]), "keeping it again".into())?;
    // The ask field takes text; Escape in it leaves the field, not the review.
    s.type_into("review-ask", "make the flag blue")?;
    check(s.ed.asks == ["make the flag blue"], format!("ask for changes reaches the assistant hook: {:?}", s.ed.asks))?;
    // The ask above opened the assistant panel (no key yet): the card moved left of it.
    s.frames(4);
    s.ui_click("review-ask")?;
    s.frames_with(2, vec![key(egui::Key::Escape, egui::Modifiers::NONE)]);
    check(s.ed.review.is_some(), "Escape in the ask field keeps the review".into())?;
    // Discard (the button) and Escape leave the scene as it was, with no undo step.
    s.ui_click("review-discard")?;
    check(
        s.ed.review.is_none() && same(&s) && s.ed.scene.history().position() == position,
        "Discard: the scene as it was, no undo step".into(),
    )?;
    s.ed.stage_json(HOUSE_SCRIPT)?;
    s.frames(2);
    s.frames_with(2, vec![key(egui::Key::Escape, egui::Modifiers::NONE)]);
    check(s.ed.review.is_none() && same(&s) && s.ed.scene.history().position() == position, "Escape discards".into())?;
    // Apply (Enter): one undo step named after the request.
    s.ed.stage_json(HOUSE_SCRIPT)?;
    s.frames(2);
    s.frames_with(2, vec![key(egui::Key::Enter, egui::Modifiers::NONE)]);
    let label = s.ed.scene.history().undo_label().map(str::to_string);
    check(
        s.ed.review.is_none()
            && s.voxels() == v0 + 22 - 6
            && s.ed.scene.history().position() == position + 1
            && label.as_deref() == Some("AI: make the roof red, add a flag and open a door"),
        format!("Enter applies as one undo step: {label:?}"),
    )?;
    s.frames(2);
    s.save_png(&dir.join("app-2x-review-applied.png"))?;
    s.ed.undo();
    check(same(&s), "undo puts the house back".into())?;
    s.ed.redo();
    check(s.voxels() == v0 + 16 && s.ed.scene.palette[3] == [0xD9, 0x4A, 0x3A, 255], "redo applies it again".into())?;
    s.ed.undo();
    // Over 10,000 cells the card asks before staging more: Stop keeps what is staged, Allow runs it.
    let big = r#"{"prompt": "fill the sky", "calls": [
        {"name": "place", "input": {"cells": [[-16, 31, -16]], "slot": 3}},
        {"name": "box", "input": {"from": [-16, 10, -16], "to": [15, 30, 15], "slot": 60}},
        {"name": "done", "input": {"summary": "a block of sky"}}]}"#;
    s.ed.stage_json(big)?;
    s.frames(3);
    let held = s.ed.ui.review.as_ref().and_then(|c| c.held);
    check(
        held == Some((10_000, 200_000)) && s.ed.ui.hits.contains_key("review-allow"),
        format!("a big change is held: {held:?}"),
    )?;
    s.frames(20);
    s.save_png(&dir.join("app-2x-review-held.png"))?;
    s.ui_click("review-stop")?;
    let c = s.ed.ui.review.clone().ok_or("no card")?;
    check(c.held.is_none() && c.counts == [1, 0, 0], format!("Stop keeps what is staged: {:?}", c.counts))?;
    s.ui_click("review-discard")?;
    s.ed.stage_json(big)?;
    s.frames(2);
    s.ui_click("review-allow")?;
    let c = s.ed.ui.review.clone().ok_or("no card")?;
    check(c.held.is_none() && c.counts[0] == 1 + 32 * 21 * 32, format!("Allow stages it: +{} added", c.counts[0]))?;
    s.frames_with(2, vec![key(egui::Key::Escape, egui::Modifiers::NONE)]);
    check(same(&s), "and Escape drops it all".into())?;
    Ok(())
}

/// A change under review on a phone held upright (390x844, 3x): the assistant opens as a sheet
/// over the lower half only, so the scene and the change's ghost show above it, with Before,
/// Discard and Apply pinned at the sheet's bottom and the card's top scrolling above them, open
/// on its top under the tabs. Closing the sheet sticks while the change waits, and the
/// sparkles button opens it again on the card. Writes `app-3x-phone-390x844-review.png`.
fn review_phone(dir: &Path) -> Result<(), String> {
    let mut s = Shot::sized(house(), "house.vox", 3.0, (390, 844))?;
    s.ed.set_touch(true);
    s.ed.ui.coach = false;
    s.frames(3);
    let plain = view_pixels(&mut s);
    let position = s.ed.scene.history().position();
    s.ed.stage_json(HOUSE_SCRIPT)?;
    s.frames(3);
    let ghost = view_pixels(&mut s);
    // The view's pixels the ghost changes where no UI covers them, below the top bar (the
    // card's parts count as the sheet that clips them).
    let shown = |s: &Shot| {
        let screen = s.ed.ctx.content_rect();
        let (w, ppp) = (s.ed.width as usize, s.ed.width as f32 / screen.width());
        let top = s.ed.ui.hits.get("ai-toggle").map_or(0.0, |r| r.bottom());
        let a = plain.as_chunks::<4>().0.iter();
        a.zip(ghost.as_chunks::<4>().0)
            .enumerate()
            .filter(|(i, (x, y))| {
                let p = egui::pos2((i % w) as f32 / ppp, (i / w) as f32 / ppp);
                x != y && p.y > top && !s.ed.ui.hits.iter().any(|(n, r)| !n.starts_with("review") && r.contains(p))
            })
            .count()
    };
    // In the sheet's thread: under its tabs, above its bottom.
    let in_sheet = |s: &Shot| {
        let (sheet, tabs) = (s.ed.ui.hits.get("assistant"), s.ed.ui.hits.get("ai-tab-chat"));
        let thread = sheet.zip(tabs).map(|(sh, t)| egui::Rect::from_min_max(egui::pos2(sh.left(), t.bottom()), sh.max));
        ["review-before", "review-discard", "review-apply"]
            .iter()
            .all(|n| s.ed.ui.hits.get(*n).zip(thread).is_some_and(|(r, th)| th.contains_rect(*r)))
    };
    let half = s.ed.ctx.content_rect().height() / 2.0;
    let sheet = s.ed.ui.hits.get("assistant").copied().ok_or("the review opens no sheet")?;
    let seen = shown(&s);
    check(
        s.ed.ui.ai_open && sheet.height() <= half && in_sheet(&s) && seen > 2000,
        format!(
            "upright phone review: the sheet ({sheet:?}) takes at most half the screen, {seen} pixels of the ghost show above it, Before, Discard and Apply are in it ({})",
            in_sheet(&s)
        ),
    )?;
    // The card opens on its top under the tabs, its footer pinned under the thread.
    let (card, tabs, apply) = (
        s.ed.ui.hits.get("review").copied().ok_or("no review card")?,
        s.ed.ui.hits.get("ai-tab-chat").copied().ok_or("no tabs")?,
        s.ed.ui.hits.get("review-apply").copied().ok_or("no Apply")?,
    );
    check(
        card.top() >= tabs.bottom() && card.top() - tabs.bottom() < 20.0 && apply.bottom() <= sheet.bottom(),
        format!("the card opens on its top under the tabs ({card:?}), Apply pinned in the sheet ({apply:?})"),
    )?;
    s.frames(40);
    s.save_png(&dir.join("app-3x-phone-390x844-review.png"))?;
    s.ui_click("ai-close")?;
    s.frames(3);
    let all = shown(&s);
    check(
        !s.ed.ui.ai_open && s.ed.review.is_some() && !s.ed.ui.hits.contains_key("assistant") && all > seen,
        format!("closing the sheet sticks while the change waits: {all} pixels of the ghost show"),
    )?;
    s.ui_click("ai-toggle")?;
    s.frames(3);
    check(s.ed.ui.ai_open && in_sheet(&s), "the sparkles button opens the sheet on the card again".into())?;
    // A request still running keeps the composer's Stop in the sheet, to keep what it staged.
    s.ed.ui.chat.running = true;
    s.frames(2);
    let stop =
        s.ed.ui.hits.get("ai-stop").zip(s.ed.ui.hits.get("assistant")).is_some_and(|(r, sh)| sh.contains_rect(*r));
    s.ed.ui.chat.running = false;
    s.frames(2);
    check(stop, "a running request shows Stop in the review sheet".into())?;
    s.ui_click("review-apply")?;
    check(
        s.ed.review.is_none() && s.ed.scene.history().position() == position + 1,
        "Apply in the sheet lands the change as one undo step".into(),
    )?;
    Ok(())
}

/// The voxel view alone (the march, under the UI), without the render corner or a cursor.
fn view_pixels(s: &mut Shot) -> Vec<u8> {
    let shown = s.ed.ui.show_render;
    s.ed.ui.show_render = false;
    s.ed.pointer_moved(None);
    s.frames(2);
    let px = pollster::block_on(read_texture(&s.ed.gpu.device, &s.ed.gpu.queue, &s.ed.renderer.color));
    s.ed.ui.show_render = shown;
    px
}

fn differ(a: &[u8], b: &[u8]) -> usize {
    a.as_chunks::<4>().0.iter().zip(b.as_chunks::<4>().0).filter(|(x, y)| x != y).count()
}

/// A change of about 100,000 cells on the island at 3024x1964, as Phase AI test 6 measured
/// it: the frame with its ghost against the frame without, and Apply and Discard each from
/// the call until the next frame is drawn (the bar is 50 ms).
fn review_times(scene: &Path) -> Result<(), String> {
    let (sc, name) = crate::native::open(scene)?;
    let mut s = Shot::sized(sc, &name, 2.0, (1512, 982))?;
    s.ed.ui.coach = false;
    s.ed.ui.show_render = false;
    s.frames(4);
    let script = r#"{"prompt": "change a lot", "calls": [
        {"name": "box", "input": {"from": [20, 14, -64], "to": [63, 30, 63], "slot": 3}},
        {"name": "box", "input": {"from": [-64, 6, -64], "to": [-24, 30, 63], "mode": "erase"}},
        {"name": "box", "input": {"from": [-23, 0, -64], "to": [19, 30, 63], "slot": 12, "mode": "paint"}}]}"#;
    let median = |mut t: Vec<f64>| {
        t.sort_by(f64::total_cmp);
        t[t.len() / 2]
    };
    let frame = |s: &mut Shot| {
        let mut t = Vec::new();
        for _ in 0..60 {
            let t0 = std::time::Instant::now();
            s.frames(1);
            let _ = s.ed.gpu.device.poll(wgpu::PollType::wait_indefinitely());
            t.push(t0.elapsed().as_secs_f64() * 1000.0);
        }
        median(t)
    };
    let stage = |s: &mut Shot| -> Result<[usize; 3], String> {
        s.ed.stage_json(script)?;
        if s.ed.review.as_ref().is_some_and(|r| !r.held.is_empty()) {
            let r = s.ed.review.as_mut().unwrap();
            r.allow(&mut s.ed.scene);
        }
        s.frames(2);
        Ok(s.ed.ui.review.as_ref().map(|c| c.counts).unwrap_or_default())
    };
    let plain = frame(&mut s);
    let counts = stage(&mut s)?;
    let staged = frame(&mut s);
    let (mut apply, mut discard) = (Vec::new(), Vec::new());
    for _ in 0..5 {
        let t0 = std::time::Instant::now();
        s.ed.apply_review();
        s.frames(1);
        let _ = s.ed.gpu.device.poll(wgpu::PollType::wait_indefinitely());
        apply.push(t0.elapsed().as_secs_f64() * 1000.0);
        s.ed.undo();
        s.frames(1);
        stage(&mut s)?;
        let t0 = std::time::Instant::now();
        s.ed.discard_review();
        s.frames(1);
        let _ = s.ed.gpu.device.poll(wgpu::PollType::wait_indefinitely());
        discard.push(t0.elapsed().as_secs_f64() * 1000.0);
        stage(&mut s)?;
    }
    s.ed.discard_review();
    let (apply, discard) = (median(apply), median(discard));
    let total: usize = counts.iter().sum();
    println!(
        "info review on the island at 3024x1964 (median): a change of {total} cells (+{} added, {} removed, {} repainted), \
         frame {plain:.2} ms without it, {staged:.2} ms staged with its ghost; Apply {apply:.1} ms, Discard {discard:.1} ms \
         (each until the next frame is drawn)",
        counts[0], counts[1], counts[2]
    );
    check(total > 80_000, format!("the timed change is big: {total} cells"))?;
    check(apply < 50.0 && discard < 50.0, format!("Apply {apply:.1} ms and Discard {discard:.1} ms are under 50 ms"))
}

/// Frame time of the island at full Retina (3024x1964), without and with the render corner,
/// while editing (pointer moving every frame) and idle. Each frame waits for the GPU.
fn frame_times(scene: &Path) -> Result<(), String> {
    let (sc, name) = crate::native::open(scene)?;
    let mut s = Shot::sized(sc, &name, 2.0, (1512, 982))?;
    s.ed.ui.coach = false;
    s.frames(4);
    let mid = [s.ed.width as f32 / 2.0, s.ed.height as f32 / 2.0];
    let run = |s: &mut Shot, render: bool, editing: bool| {
        s.ed.ui.show_render = render;
        s.frames(4);
        if !editing {
            std::thread::sleep(std::time::Duration::from_millis(350));
        }
        let mut t = Vec::new();
        for i in 0..90 {
            if editing {
                s.ed.pointer_moved(Some([mid[0] + (i % 7) as f32, mid[1]]));
            }
            let t0 = std::time::Instant::now();
            s.frames(1);
            let _ = s.ed.gpu.device.poll(wgpu::PollType::wait_indefinitely());
            t.push(t0.elapsed().as_secs_f64() * 1000.0);
        }
        t.sort_by(f64::total_cmp);
        t[t.len() / 2]
    };
    let off = run(&mut s, false, true);
    let on = run(&mut s, true, true);
    let idle = run(&mut s, true, false);
    let (ms, measured) = s.ed.preview.sample_ms();
    // Every voxel selected, so every hit pixel looks up the selection mask.
    s.ed.scene.select_all();
    let selected = run(&mut s, false, true);
    s.ed.scene.deselect();
    s.ed.cam.snap(vxr_render::View::Iso);
    let iso = run(&mut s, true, true);
    s.ed.cam.ortho = false;
    s.ed.ui.full_render = true;
    let full = run(&mut s, true, true);
    s.ed.ui.full_render = false;
    println!(
        "info frame time, island at 3024x1964 (median, CPU + GPU): {off:.2} ms without the render corner, \
         {on:.2} ms with it while editing, {idle:.2} ms idle, {selected:.2} ms with every voxel selected, \
         {iso:.2} ms isometric with the render corner, {full:.2} ms in the full render view while editing; one 640x360 sample {ms:.2} ms ({})",
        if measured { "timestamps" } else { "assumed" }
    );
    Ok(())
}

/// The palette editor, through egui input on the drawn UI: live recolour, one undo step per
/// edit, hex and RGB fields, swap and copy by button and by Cmd-drag, Escape.
fn palette(s: &mut Shot, dir: &Path) -> Result<(), String> {
    s.frames(2);
    let counts = s.ed.scene.map.counts;
    let used = (1..256).max_by_key(|&i| counts[i]).unwrap() as u8;
    s.ed.ui.slot = used;
    s.frames(2);
    s.ui_click("rail-color")?;
    check(s.ed.ui.palette_open, "the rail's colour button opens the palette".into())?;
    s.ui_click("edit-color")?;
    check(s.ed.ui.color_edit.is_some(), format!("Edit opens the color editor on slot {used}"))?;
    let before = s.ed.scene.palette[used as usize];
    let voxels = s.voxels();

    // Drag across the saturation/value square: the colour shows while dragging, one undo step.
    let sv = s.ed.ui.hits["sv"];
    let undo_before = s.ed.scene.history().can_undo();
    s.ui_drag(sv.left_top() + egui::vec2(4.0, 4.0), sv.center() + egui::vec2(30.0, 10.0), egui::Modifiers::NONE);
    let dragged = s.ed.scene.palette[used as usize];
    check(
        dragged != before && s.voxels() == voxels && s.ed.scene.map.counts == counts,
        format!("dragging in the color square recolors slot {used} live: {before:?} -> {dragged:?}, no voxel changed"),
    )?;
    s.save_png(&dir.join("app-2x-palette.png"))?;
    s.ed.undo();
    check(
        s.ed.scene.palette[used as usize] == before && s.ed.scene.history().can_undo() == undo_before,
        "one undo returns the color from before the drag".into(),
    )?;
    s.ed.redo();
    s.frames(2);

    s.type_into("field-hex", "#ff8800")?;
    let c = s.ed.scene.palette[used as usize];
    check(c[..3] == [0xff, 0x88, 0x00], format!("hex field sets {c:?}"))?;
    s.type_into("field-g", "200")?;
    let c = s.ed.scene.palette[used as usize];
    check(c[..3] == [0xff, 200, 0x00], format!("G field sets {c:?}"))?;
    s.type_into("field-h", "240")?;
    let c = s.ed.scene.palette[used as usize];
    check(c[2] > c[0] && c[2] > c[1], format!("H field turns it blue: {c:?}"))?;
    s.ed.undo();
    check(s.ed.scene.palette[used as usize][..3] == [0xff, 200, 0x00], "each field edit is one undo step".into())?;

    // Swap with an unused slot: the voxels move with the colour, so the scene looks the same.
    let free = (1..=64u8).find(|&i| counts[i as usize] == 0).ok_or("no unused slot in the first 64")?;
    let (cu, cf) = (s.ed.scene.palette[used as usize], s.ed.scene.palette[free as usize]);
    s.ui_click("swap")?;
    check(s.ed.ui.slot_op.is_some(), "Swap with… arms the next swatch click".into())?;
    s.ui_click(&format!("sw{free}"))?;
    let m = &s.ed.scene.map;
    check(
        s.ed.scene.palette[free as usize] == cu
            && s.ed.scene.palette[used as usize] == cf
            && m.counts[free as usize] == counts[used as usize]
            && m.counts[used as usize] == 0
            && s.ed.ui.slot == free,
        format!("swap slots {used} and {free}: colors and {} voxels trade places", counts[used as usize]),
    )?;
    s.ed.undo();
    check(
        s.ed.scene.map.counts == counts && s.ed.scene.palette[used as usize] == cu,
        "one undo reverts the swap".into(),
    )?;

    // Copy the colour to another slot.
    s.ed.ui.slot = used;
    s.frames(2);
    s.ui_click("copy")?;
    s.ui_click(&format!("sw{}", free + 1))?;
    check(s.ed.scene.palette[free as usize + 1] == cu, format!("copy slot {used} to slot {}", free + 1))?;
    s.ed.undo();

    // The palette shows the colours in use (and the current one); All shows every slot.
    s.ed.ui.slot = used;
    s.frames(2);
    let shown = |s: &Shot| (1..=255u8).filter(|i| s.ed.ui.hits.contains_key(&format!("sw{i}"))).count();
    let in_use = (1..=255).filter(|&i| counts[i] > 0).count();
    check(
        !s.ed.ui.palette_all && !s.ed.ui.hits.contains_key(&format!("sw{free}")) && shown(s) == in_use,
        format!("the palette shows the {in_use} colours in use"),
    )?;
    s.ui_click("palette-all")?;
    check(
        s.ed.ui.palette_all && s.ed.ui.hits.contains_key(&format!("sw{free}")) && shown(s) > in_use,
        format!("its All button shows every slot ({} on screen)", shown(s)),
    )?;

    // Cmd-drag a swatch onto another swaps them, as Ctrl+drag does in MagicaVoxel.
    let (a, b) = (s.at(&format!("sw{used}"))?, s.at(&format!("sw{free}"))?);
    s.ui_drag(a, b, egui::Modifiers::COMMAND);
    check(
        s.ed.scene.map.counts[free as usize] == counts[used as usize] && s.ed.scene.palette[free as usize] == cu,
        "Cmd-drag a swatch onto another swaps them".into(),
    )?;
    s.ed.undo();

    let key = |key| egui::Event::Key {
        key,
        physical_key: None,
        pressed: true,
        repeat: false,
        modifiers: egui::Modifiers::NONE,
    };
    s.frames_with(2, vec![key(egui::Key::Escape)]);
    check(s.ed.ui.color_edit.is_none(), "Escape closes the color editor".into())?;
    s.frames_with(2, vec![key(egui::Key::Escape)]);
    check(!s.ed.ui.palette_open, "and again the palette".into())?;
    s.ed.undo();
    s.ed.undo();
    s.ed.undo();
    check(s.ed.scene.palette[used as usize] == before, "undoing every palette edit restores the color".into())
}

fn key_event(key: egui::Key, modifiers: egui::Modifiers) -> egui::Event {
    egui::Event::Key { key, physical_key: None, pressed: true, repeat: false, modifiers }
}

impl Shot {
    /// The assistant on a scripted provider, with no key kept yet.
    fn fake_assistant(&mut self, chunk: usize) -> vxr_app::assistant::fake::Scripted {
        let fake = vxr_app::assistant::fake::Scripted::new(chunk);
        self.ed.assistant =
            vxr_app::assistant::Assistant::new(Box::new(fake.clone()), Box::new(vxr_ai::MemoryKeys::default()), false);
        fake
    }

    /// Draw frames until the assistant's request ends or waits for the user (at most `n`).
    fn settle(&mut self, n: usize) -> usize {
        for k in 0..n {
            if !self.ed.assistant.running() || self.ed.assistant.held() {
                return k;
            }
            self.frames(1);
        }
        n
    }

    fn last_answer(&self) -> Option<vxr_app::assistant::AnswerView> {
        match self.ed.assistant.entries.last() {
            Some(vxr_app::assistant::Entry::Answer(a)) => Some(a.clone()),
            _ => None,
        }
    }
}

/// The assistant end to end on a scripted provider, through the same pointer and key path
/// as the window: Cmd-J opens the panel on its key settings, a key typed into the masked
/// field is kept and the panel turns to the conversation, a request typed into the composer
/// streams its answer, runs its calls through the review card and sends their answers back,
/// "Ask for changes" on the card continues the same change and conversation, Enter applies
/// it as one undo step, Stop mid-answer asks to keep or drop what was staged (both), a big
/// change waits for Allow, Cmd-K "Ask AI" (Tab, and the row when no command matches) opens
/// the prompt bar whose request lands in the same conversation and card, and the model
/// picker opens the key settings for a provider without a key.
fn assistant(dir: &Path) -> Result<(), String> {
    use serde_json::json;
    use vxr_ai::Kind;
    use vxr_app::assistant::fake::anthropic;
    use vxr_app::assistant::{CallState, Progress};
    let mut s = Shot::new(house(), "house.vox", 2.0)?;
    s.ed.ui.coach = false;
    // Back off and move the house left, clear of the card, the palette and the panel.
    s.ed.cam.distance *= 2.6;
    let h = s.ed.height as f32;
    s.ed.cam.pan(-640.0, 40.0, h);
    let fake = s.fake_assistant(24);
    s.frames(2);
    let (v0, counts0, palette0) = (s.voxels(), s.ed.scene.map.counts, s.ed.scene.palette);
    let same = |s: &Shot| s.voxels() == v0 && s.ed.scene.map.counts == counts0 && s.ed.scene.palette == palette0;
    let position = s.ed.scene.history().position();

    // Cmd-J: the panel, on its key settings while there is no key.
    s.frames_with(2, vec![key_event(egui::Key::J, egui::Modifiers::COMMAND)]);
    check(
        s.ed.ui.ai_open && s.ed.ui.hits.contains_key("ai-key") && s.ed.ui.hits.contains_key("ai-pick-0"),
        "Cmd-J opens the assistant panel on its key settings".into(),
    )?;
    s.frames(2);
    s.save_png(&dir.join("app-2x-assistant-keys.png"))?;
    // The compatible endpoint needs no key, but its URL, model and key stay to be filled.
    s.ui_click("ai-pick-3")?;
    s.frames(2);
    check(
        s.ed.ui.ai_setup && s.ed.ui.hits.contains_key("ai-base") && s.ed.ui.hits.contains_key("ai-key"),
        "picking the compatible endpoint keeps its settings open".into(),
    )?;
    s.ui_click("ai-pick-0")?;
    s.frames(2);
    s.type_into("ai-key", "sk-ant-test-not-a-real-key")?;
    s.frames(2);
    check(
        s.ed.assistant.keys.get(Kind::Anthropic).as_deref() == Some("sk-ant-test-not-a-real-key")
            && !s.ed.ui.ai_setup
            && s.ed.ui.hits.contains_key("ai-draft")
            && s.ed.ui.hits.contains_key("ai-example-0"),
        "a key typed into the masked field is kept, and the panel turns to the conversation".into(),
    )?;
    s.frames(2);
    s.save_png(&dir.join("app-2x-assistant-empty.png"))?;

    // A request from the composer: streamed, its calls staged under the card, answered back.
    fake.answer(anthropic(
        "I'll repaint the roof into a new red slot.",
        &[("paint", json!({"slot": 3, "from_slot": 20})), ("palette_set", json!({"slot": 3, "hex": "#D94A3A"}))],
    ));
    fake.answer(anthropic("The roof is red now.", &[("done", json!({"summary": "Painted the roof red"}))]));
    s.type_into("ai-draft", "make the roof red")?;
    s.frames(6);
    let mid = s.last_answer().ok_or("no answer")?;
    check(
        matches!(mid.progress, Progress::Working(_)) && s.ed.review.is_some() && s.ed.ui.hits.contains_key("ai-stop"),
        format!("Enter sends it: the answer streams ({:?}), the card opens, Stop shows", mid.progress),
    )?;
    s.save_png(&dir.join("app-2x-assistant-working.png"))?;
    let frames = s.settle(600);
    let a = s.last_answer().ok_or("no answer")?;
    let card = s.ed.ui.review.clone().ok_or("no card")?;
    check(
        a.progress == Progress::Done
            && a.calls.len() == 3
            && a.calls.iter().all(|c| c.state == CallState::Ok)
            && a.text == "I'll repaint the roof into a new red slot.\n\nThe roof is red now."
            && a.cost.is_some_and(|c| c > 0.0)
            && card.counts == [0, 0, 392]
            && card.summary.as_deref() == Some("Painted the roof red")
            && card.working.is_none()
            && fake.requests().len() == 2,
        format!(
            "the request runs in {frames} frames: {} calls, {:?}, card {:?}, {} requests",
            a.calls.len(),
            a.progress,
            card.counts,
            fake.requests().len()
        ),
    )?;
    let req = &fake.requests()[1];
    check(
        !req.body.contains("sk-ant-test")
            && req.headers.iter().any(|(k, v)| k == "x-api-key" && v.starts_with("sk-ant-test")),
        "the key goes in the header only".into(),
    )?;
    s.frames(20);
    s.save_png(&dir.join("app-2x-assistant.png"))?;

    // Ask for changes: the same change and conversation take the next request.
    fake.answer(anthropic(
        "",
        &[
            ("line", json!({"from": [8, 0, 8], "to": [8, 9, 8], "slot": 40, "name": "flag pole"})),
            ("done", json!({"summary": "Added a flag pole"})),
        ],
    ));
    s.type_into("review-ask", "add a flag pole")?;
    s.settle(600);
    let card = s.ed.ui.review.clone().ok_or("no card")?;
    check(
        card.counts == [10, 0, 392] && s.ed.assistant.entries.len() == 4 && fake.requests().len() == 3,
        format!("Ask for changes continues the change ({:?}) and the conversation", card.counts),
    )?;
    let body: serde_json::Value = serde_json::from_str(&fake.requests()[2].body).map_err(|e| e.to_string())?;
    check(
        body["messages"].as_array().is_some_and(|m| m.len() == 5),
        "the follow-up request carries the whole conversation".into(),
    )?;
    s.frames_with(2, vec![key_event(egui::Key::Enter, egui::Modifiers::NONE)]);
    let label = s.ed.scene.history().undo_label().map(str::to_string);
    check(
        s.ed.review.is_none()
            && s.ed.scene.history().position() == position + 1
            && label.as_deref() == Some("AI: make the roof red"),
        format!("Enter applies it as one undo step: {label:?}"),
    )?;
    s.ed.undo();
    check(same(&s), "undo takes it all back".into())?;

    // Stop mid-answer with a change staged: Drop takes it away, Keep leaves it for review.
    for keep in [false, true] {
        fake.answer(anthropic("", &[("paint", json!({"slot": 5, "from_slot": 10}))]));
        fake.answer(anthropic(&"Now I will look at every wall in turn and think about it. ".repeat(12), &[]));
        let sent = fake.requests().len();
        s.type_into("ai-draft", "make the walls green")?;
        // Until the second answer is on its way, the first one's paint staged.
        for _ in 0..200 {
            if fake.requests().len() >= sent + 2 {
                break;
            }
            s.frames(1);
        }
        s.frames(4);
        check(s.ed.assistant.running(), "a second answer is streaming".into())?;
        s.ui_click("ai-stop")?;
        check(
            !s.ed.assistant.running() && s.ed.ui.chat.stop_ask && s.ed.ui.hits.contains_key("ai-keep"),
            "Stop ends the request and asks to keep or drop the staged change".into(),
        )?;
        if !keep {
            s.frames(2);
            s.save_png(&dir.join("app-2x-assistant-stop.png"))?;
            s.ui_click("ai-drop")?;
            check(s.ed.review.is_none() && same(&s), "Drop: the scene as it was".into())?;
        } else {
            s.ui_click("ai-keep")?;
            check(
                s.ed.review.as_ref().is_some_and(|r| r.card(&s.ed.scene).counts[2] > 0) && !s.ed.ui.chat.stop_ask,
                "Keep: the change stays under review".into(),
            )?;
            s.ui_click("review-discard")?;
        }
    }

    // A big change waits for Allow, then the request carries on.
    fake.answer(anthropic("", &[("box", json!({"from": [-16, 12, -16], "to": [15, 30, 15], "slot": 60}))]));
    fake.answer(anthropic("", &[("done", json!({"summary": "A block of sky"}))]));
    s.type_into("ai-draft", "fill the sky")?;
    s.settle(600);
    check(
        s.ed.assistant.held() && s.ed.ui.review.as_ref().is_some_and(|c| c.held.is_some()),
        "a change past 10,000 cells waits on the card".into(),
    )?;
    s.frames(2);
    s.ui_click("review-allow")?;
    s.settle(600);
    check(
        !s.ed.assistant.running() && s.ed.ui.review.as_ref().is_some_and(|c| c.counts[0] == 32 * 19 * 32),
        "Allow runs it and the request finishes".into(),
    )?;
    // The thread scrolls to the card, which takes a few frames after it changes size.
    s.frames(4);
    s.ui_click("review-discard")?;
    check(same(&s), "Discard drops it".into())?;

    // Cmd-K: Tab asks AI from any row, with what was typed, in the prompt bar.
    s.ed.ui.ai_open = false;
    s.frames_with(2, vec![key_event(egui::Key::K, egui::Modifiers::COMMAND)]);
    s.frames_with(2, vec![egui::Event::Text("open a door".into())]);
    s.frames_with(3, vec![key_event(egui::Key::Tab, egui::Modifiers::NONE)]);
    check(
        s.ed.ui.cmdk.is_none() && s.ed.ui.ask_bar.as_ref().is_some_and(|b| b.text == "open a door" && !b.sent),
        "Tab in Cmd-K opens the Ask AI bar with the query".into(),
    )?;
    s.frames(2);
    s.save_png(&dir.join("app-2x-ask-bar.png"))?;
    fake.answer(anthropic(
        "Opening a door in the front wall.",
        &[
            ("box", json!({"from": [-1, 0, 5], "to": [0, 2, 5], "mode": "erase"})),
            ("done", json!({"summary": "Opened a door"})),
        ],
    ));
    s.frames_with(2, vec![key_event(egui::Key::Enter, egui::Modifiers::NONE)]);
    check(s.ed.ui.ask_bar.as_ref().is_some_and(|b| b.sent) && s.ed.assistant.running(), "Enter sends it".into())?;
    s.frames(3);
    s.save_png(&dir.join("app-2x-ask-bar-working.png"))?;
    s.settle(600);
    s.frames(3);
    let card = s.ed.ui.review.clone().ok_or("no card")?;
    check(
        card.counts == [0, 6, 0] && s.ed.ui.ask_bar.is_none() && s.ed.assistant.entries.len() == 12,
        format!("its change lands in the card ({:?}), the bar closes, the panel's conversation has it", card.counts),
    )?;
    s.frames(20);
    s.save_png(&dir.join("app-2x-ask-card.png"))?;
    s.frames(4);
    s.ui_click("review-discard")?;
    // The change opened the panel; close it again.
    s.ed.ui.ai_open = false;
    s.frames(2);
    // With no command matching, the one row asks AI.
    s.frames_with(2, vec![key_event(egui::Key::K, egui::Modifiers::COMMAND)]);
    s.frames_with(2, vec![egui::Event::Text("put a chimney on the roof".into())]);
    check(s.ed.ui.hits.contains_key("cmd-ask-ai"), "no command matches: the Ask AI row".into())?;
    s.frames(2);
    s.save_png(&dir.join("app-2x-cmdk-ask.png"))?;
    s.frames_with(2, vec![key_event(egui::Key::Enter, egui::Modifiers::NONE)]);
    check(
        s.ed.ui.ask_bar.as_ref().is_some_and(|b| b.text == "put a chimney on the roof"),
        "Enter on it opens the bar".into(),
    )?;
    s.frames_with(2, vec![key_event(egui::Key::Escape, egui::Modifiers::NONE)]);
    check(s.ed.ui.ask_bar.is_none(), "Escape closes the bar".into())?;

    // The model picker: a provider without a key opens its key settings.
    s.frames_with(2, vec![key_event(egui::Key::J, egui::Modifiers::COMMAND)]);
    s.ui_click("ai-model-chip")?;
    check(s.ed.ui.hits.contains_key("ai-menu-1"), "the model chip opens the picker".into())?;
    s.frames(2);
    s.save_png(&dir.join("app-2x-assistant-models.png"))?;
    s.ui_click("ai-menu-1")?;
    check(
        s.ed.assistant.settings.kind == Kind::OpenAi
            && s.ed.assistant.settings.model == "gpt-6.1-sol"
            && s.ed.ui.ai_setup
            && s.ed.ui.hits.contains_key("ai-key"),
        "gpt-6.1-sol without an OpenAI key opens its key settings".into(),
    )?;
    s.ui_click("ai-pick-2")?;
    check(
        s.ed.assistant.settings.model == "claude-sonnet-5-5" && s.ed.ui.hits.contains_key("ai-key-remove"),
        "Sonnet uses the Anthropic key already kept".into(),
    )?;
    // A compatible endpoint the browser cannot reach says so under its URL, in the browser only.
    s.ui_click("ai-pick-3")?;
    s.type_into("ai-base", "http://192.168.1.20:11434/v1")?;
    s.frames(2);
    check(!s.ed.ui.hits.contains_key("ai-base-limit"), "the desktop app reaches any compatible host".into())?;
    s.ed.assistant.settings.browser = true;
    s.ed.assistant.version += 1;
    s.frames(2);
    check(
        s.ed.ui.hits.contains_key("ai-base-limit"),
        "the browser cannot reach a host on the LAN: the line says so".into(),
    )?;
    s.type_into("ai-base", "http://localhost:8080/v1")?;
    s.frames(2);
    check(!s.ed.ui.hits.contains_key("ai-base-limit"), "a local server on any port needs no line".into())?;
    s.ed.assistant.settings.browser = false;
    s.ed.assistant.version += 1;
    s.ui_click("ai-pick-0")?;
    s.ui_click("ai-done")?;
    check(
        !s.ed.ui.ai_setup && s.ed.assistant.settings.model == "claude-opus-5-5",
        "Done goes back to the conversation".into(),
    )?;
    // Escape in the composer only leaves it.
    s.ui_click("ai-draft")?;
    s.frames_with(2, vec![key_event(egui::Key::Escape, egui::Modifiers::NONE)]);
    check(s.ed.ui.ai_open && !s.ed.ctx.egui_wants_keyboard_input(), "Escape in the composer leaves it".into())?;
    s.frames_with(2, vec![key_event(egui::Key::J, egui::Modifiers::COMMAND)]);
    check(!s.ed.ui.ai_open, "Cmd-J closes the panel".into())?;
    Ok(())
}

/// A request the editor turns down (the world is not a model) keeps its words where they were
/// typed: the bar stays open and unsent, the composer gets its draft back, and nothing is sent.
fn ai_refused() -> Result<(), String> {
    use vxr_core::history::At;
    let mut s = Shot::new(Scene::from_vox(&house().to_vox())?, "house.vox", 1.0)?;
    s.ed.ui.coach = false;
    let fake = s.fake_assistant(0);
    s.ed.assistant.set_key(vxr_ai::Kind::Anthropic, "sk-ant-test-not-a-real-key", false)?;
    s.frames(2);
    s.frames_with(2, vec![key_event(egui::Key::Tab, egui::Modifiers::NONE)]);
    check(s.ed.scene.at() == At::World, "the house opens in the world after Tab".into())?;
    s.ed.ui.open_ask_bar("open a door");
    s.frames(2);
    s.frames_with(2, vec![key_event(egui::Key::Enter, egui::Modifiers::NONE)]);
    check(
        fake.requests().is_empty() && s.ed.ui.ask_bar.as_ref().is_some_and(|b| b.text == "open a door" && !b.sent),
        "a request turned down in the world leaves the bar open and unsent with its words".into(),
    )?;
    s.frames_with(2, vec![key_event(egui::Key::Escape, egui::Modifiers::NONE)]);
    s.ed.ui.ai_open = true;
    s.frames(2);
    s.type_into("ai-draft", "open a door")?;
    s.frames(2);
    check(
        fake.requests().is_empty() && s.ed.ui.ai_draft() == "open a door",
        "a request turned down in the world goes back into the composer".into(),
    )?;
    Ok(())
}

/// New conversation ends a request the way Stop does: held calls leave the card, an empty
/// review closes, and a staged change asks Keep or Drop; the next request then runs.
fn ai_new_chat() -> Result<(), String> {
    use serde_json::json;
    use vxr_app::assistant::Progress;
    use vxr_app::assistant::fake::anthropic;
    let mut s = Shot::new(house(), "house.vox", 1.0)?;
    s.ed.ui.coach = false;
    let fake = s.fake_assistant(0);
    s.ed.assistant.set_key(vxr_ai::Kind::Anthropic, "sk-ant-test-not-a-real-key", false)?;
    s.ed.ui.ai_open = true;
    s.frames(2);
    let v0 = s.voxels();

    // A small change staged, then a big one held: New conversation stops it.
    fake.answer(anthropic("", &[("paint", json!({"slot": 5, "from_slot": 10}))]));
    fake.answer(anthropic("", &[("box", json!({"from": [-16, 12, -16], "to": [15, 30, 15], "slot": 60}))]));
    s.type_into("ai-draft", "make the walls green")?;
    s.settle(600);
    check(s.ed.assistant.held(), "a big change waits on the card".into())?;
    s.ui_click("ai-new")?;
    s.frames(2);
    check(
        !s.ed.assistant.running()
            && s.ed.assistant.entries.is_empty()
            && s.ed.review.as_ref().is_some_and(|r| r.held.is_empty())
            && s.ed.ui.review.as_ref().is_some_and(|c| c.held.is_none())
            && s.ed.ui.chat.stop_ask
            && s.ed.ui.hits.contains_key("ai-keep"),
        "New conversation drops the held calls and asks to keep or drop the staged change".into(),
    )?;
    s.ui_click("ai-keep")?;
    // The kept change takes the next request, whose calls run.
    fake.answer(anthropic(
        "",
        &[
            ("line", json!({"from": [8, 0, 8], "to": [8, 9, 8], "slot": 40, "name": "flag pole"})),
            ("done", json!({"summary": "Added a flag pole"})),
        ],
    ));
    s.type_into("review-ask", "add a flag pole")?;
    s.settle(600);
    let a = s.last_answer().ok_or("no answer")?;
    let card = s.ed.ui.review.clone().ok_or("no card")?;
    check(
        a.progress == Progress::Done && card.counts[0] == 10 && card.held.is_none(),
        format!("the next request runs its calls: {:?}, card {:?}", a.progress, card.counts),
    )?;
    s.ui_click("review-discard")?;

    // New conversation while a request runs on an empty review closes the review.
    fake.hang();
    s.type_into("ai-draft", "make the roof red")?;
    s.frames(4);
    check(s.ed.assistant.running() && s.ed.review.is_some(), "a request runs on a new review".into())?;
    s.ui_click("ai-new")?;
    s.frames(2);
    check(
        !s.ed.assistant.running() && s.ed.review.is_none() && !s.ed.ui.chat.stop_ask && s.voxels() == v0,
        "New conversation closes the empty review".into(),
    )?;
    Ok(())
}

/// A request from the Cmd-K bar that fails while a review is open leaves the bar open on the
/// failure, until Escape.
fn ai_bar_failed() -> Result<(), String> {
    use serde_json::json;
    use vxr_app::assistant::Progress;
    use vxr_app::assistant::fake::anthropic;
    let mut s = Shot::new(house(), "house.vox", 1.0)?;
    s.ed.ui.coach = false;
    let fake = s.fake_assistant(0);
    s.ed.assistant.set_key(vxr_ai::Kind::Anthropic, "sk-ant-test-not-a-real-key", false)?;
    s.frames(2);
    fake.answer(anthropic(
        "",
        &[("paint", json!({"slot": 5, "from_slot": 10})), ("done", json!({"summary": "Green walls"}))],
    ));
    s.ed.ui.open_ask_bar("make the walls green");
    s.frames(2);
    s.frames_with(2, vec![key_event(egui::Key::Enter, egui::Modifiers::NONE)]);
    s.settle(600);
    s.frames(3);
    check(s.ed.review.is_some() && s.ed.ui.ask_bar.is_none(), "a change lands in the card and the bar closes".into())?;

    fake.fail(400, r#"{"error":{"message":"bad request"}}"#);
    s.ed.ui.open_ask_bar("add a flag pole");
    s.frames(2);
    s.frames_with(2, vec![key_event(egui::Key::Enter, egui::Modifiers::NONE)]);
    s.settle(600);
    s.frames(3);
    let a = s.last_answer().ok_or("no answer")?;
    check(
        matches!(a.progress, Progress::Failed(_))
            && s.ed.review.is_some()
            && s.ed.ui.ask_bar.as_ref().is_some_and(|b| b.sent)
            && s.ed.ui.hits.contains_key("ask-bar-close"),
        format!("a failed request keeps the bar open on its failure: {:?}", a.progress),
    )?;
    s.frames_with(2, vec![key_event(egui::Key::Escape, egui::Modifiers::NONE)]);
    check(s.ed.ui.ask_bar.is_none() && s.ed.review.is_some(), "Escape closes the bar, the card stays".into())?;
    Ok(())
}

/// The assistant against the real providers whose keys are in the environment: one request on
/// the house through the panel, then "Ask for changes" on its card (the conversation replayed
/// with the provider's thinking or reasoning), checked and applied. Keys stay in memory.
#[cfg(feature = "live")]
pub fn live(dir: &Path) -> Result<(), String> {
    use vxr_ai::Kind;
    use vxr_app::assistant::Progress;
    std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    let mut ran = 0;
    for (kind, var) in [(Kind::Anthropic, "ANTHROPIC_API_KEY"), (Kind::OpenAi, "OPENAI_API_KEY")] {
        let Some(key) = std::env::var(var).ok().filter(|k| !k.is_empty()) else {
            println!("skip {} ({var} is not set)", kind.name());
            continue;
        };
        ran += 1;
        let mut s = Shot::new(house(), "house.vox", 2.0)?;
        s.ed.ui.coach = false;
        s.ed.cam.distance *= 2.6;
        let h = s.ed.height as f32;
        s.ed.cam.pan(-640.0, 40.0, h);
        let rec = dir.join(format!("{}-streams", kind.id()));
        std::fs::create_dir_all(&rec).map_err(|e| e.to_string())?;
        let mut net = crate::net::Net::new();
        net.record = Some(rec);
        let mut keys = vxr_ai::MemoryKeys::default();
        vxr_ai::Keys::set(&mut keys, kind, &key, false)?;
        drop(key);
        s.ed.assistant = vxr_app::assistant::Assistant::new(Box::new(net), Box::new(keys), false);
        s.ed.ui.ai_open = true;
        s.frames(2);
        let mut cost = 0.0;
        // `VXR_LIVE_PROMPTS` ("first|then") swaps in other requests, such as harder ones that
        // make the model reason.
        let custom = std::env::var("VXR_LIVE_PROMPTS").ok();
        let prompts: Vec<&str> = match &custom {
            Some(p) => p.split('|').collect(),
            None => vec!["make the roof red", "add a small flag on a pole on top of the roof"],
        };
        for (k, prompt) in prompts.into_iter().enumerate() {
            let before = s.ed.assistant.entries.len();
            if k == 0 {
                s.type_into("ai-draft", prompt)?;
            } else {
                s.type_into("review-ask", prompt)?;
            }
            let t0 = std::time::Instant::now();
            while s.ed.assistant.running() && t0.elapsed().as_secs() < 300 {
                if s.ed.assistant.held() {
                    s.ui_click("review-allow")?;
                }
                s.frames(1);
                std::thread::sleep(std::time::Duration::from_millis(15));
            }
            let a = s.last_answer().ok_or("no answer")?;
            let card = s.ed.ui.review.clone();
            cost += a.cost.unwrap_or(0.0);
            check(
                a.progress == Progress::Done
                    && s.ed.assistant.entries.len() == before + 2
                    && card.as_ref().is_some_and(|c| c.counts.iter().sum::<usize>() > 0 || !c.slots.is_empty()),
                format!(
                    "{} {}: \"{prompt}\" -> {:?}, {} calls ({}), card {:?} slots {:?}, ${:.3}, {:.1} s",
                    kind.name(),
                    a.model,
                    a.progress,
                    a.calls.len(),
                    a.calls.iter().map(|c| c.tool.as_str()).collect::<Vec<_>>().join(" "),
                    card.as_ref().map(|c| c.counts),
                    card.as_ref().map(|c| c.slots.iter().map(|s| s.slot).collect::<Vec<_>>()),
                    a.cost.unwrap_or(0.0),
                    a.secs
                ),
            )?;
        }
        s.frames(30);
        s.save_png(&dir.join(format!("live-{}.png", kind.id())))?;
        s.frames_with(2, vec![key_event(egui::Key::Enter, egui::Modifiers::NONE)]);
        check(
            s.ed.review.is_none() && s.ed.scene.history().undo_label().is_some_and(|l| l.starts_with("AI: ")),
            format!("{}: applied as one undo step; ${cost:.3} for both requests", kind.name()),
        )?;
    }
    check(ran > 0, format!("{ran} providers checked live"))
}

/// `vxr mcp --live` as an agent's MCP client sees it, against the window's socket.
#[cfg(unix)]
struct LiveAgent(vxr_mcp::Server<vxr_mcp::live::Live<vxr_mcp::live::Remote>>);

#[cfg(unix)]
impl LiveAgent {
    fn new(dir: &Path, client: &str) -> LiveAgent {
        let remote = vxr_mcp::live::Remote::new(dir.to_path_buf());
        let mut server = vxr_mcp::Server::with(vxr_mcp::live::Live(remote));
        let init = serde_json::json!({"jsonrpc": "2.0", "id": 0, "method": "initialize",
            "params": {"protocolVersion": "2025-06-18", "capabilities": {}, "clientInfo": {"name": client, "version": "1"}}});
        server.handle(&init);
        LiveAgent(server)
    }
}

#[cfg(unix)]
impl Shot {
    /// One frame with live mode: the agents' requests run, then what the UI asked of live
    /// mode goes to it, as the native window does.
    fn live_frames(&mut self, live: &mut vxr_app::live::Live, n: usize) {
        for _ in 0..n {
            live.tick(&mut self.ed);
            self.frames(1);
            for a in std::mem::take(&mut self.host) {
                if !live.act(&a, &mut self.ed) {
                    self.host.push(a);
                }
            }
        }
    }

    /// An agent's tool call, answered by the window while frames run: its text and whether
    /// it was an error.
    fn agent_call(
        &mut self,
        live: &mut vxr_app::live::Live,
        agent: &mut LiveAgent,
        name: &str,
        args: serde_json::Value,
    ) -> (String, bool) {
        let r = self.agent_result(live, agent, name, args);
        (r["content"][0]["text"].as_str().unwrap_or("").to_string(), r["isError"].as_bool().unwrap_or(true))
    }

    /// An agent's tool call as the MCP client gets it: the `tools/call` result.
    fn agent_result(
        &mut self,
        live: &mut vxr_app::live::Live,
        agent: &mut LiveAgent,
        name: &str,
        args: serde_json::Value,
    ) -> serde_json::Value {
        let msg = serde_json::json!({"jsonrpc": "2.0", "id": 1, "method": "tools/call",
            "params": {"name": name, "arguments": args}});
        let reply = std::thread::scope(|sc| {
            let h = sc.spawn(|| agent.0.handle(&msg));
            while !h.is_finished() {
                self.live_frames(live, 1);
            }
            h.join().unwrap().unwrap()
        });
        self.live_frames(live, 2);
        reply["result"].clone()
    }

    /// An agent's tool call that opens the New scene confirm, answered by `answer` once it shows: its text, whether it was an error, and the voxels while the confirm waited.
    fn agent_call_confirmed(
        &mut self,
        live: &mut vxr_app::live::Live,
        agent: &mut LiveAgent,
        name: &str,
        args: serde_json::Value,
        answer: impl FnOnce(&mut Shot) -> Result<(), String>,
    ) -> Result<(String, bool, u64), String> {
        let msg = serde_json::json!({"jsonrpc": "2.0", "id": 1, "method": "tools/call",
            "params": {"name": name, "arguments": args}});
        let mut waiting = None;
        let mut answer = Some(answer);
        let reply = std::thread::scope(|sc| {
            let h = sc.spawn(|| agent.0.handle(&msg));
            while !h.is_finished() {
                self.live_frames(live, 1);
                if waiting.is_none() && self.ed.ui.confirm_new {
                    self.live_frames(live, 5);
                    waiting = Some((h.is_finished(), self.voxels()));
                    answer.take().map_or(Ok(()), |a| a(self))?;
                }
            }
            Ok::<_, String>(h.join().unwrap().unwrap())
        })?;
        self.live_frames(live, 2);
        let Some((false, voxels)) = waiting else { return Err(format!("{name} did not wait for the confirm")) };
        let r = &reply["result"];
        Ok((r["content"][0]["text"].as_str().unwrap_or("").to_string(), r["isError"].as_bool().unwrap_or(true), voxels))
    }
}

/// The live agent's look, render settings and exports, on the house: a picture of the
/// user's view and a named one, the Render panel's settings without an undo step, and the OBJ,
/// the PNG slices and a render into the downloads folder, which never replaces a file.
#[cfg(unix)]
fn agents_look_render(
    s: &mut Shot,
    live: &mut vxr_app::live::Live,
    claude: &mut LiveAgent,
    downloads: &Path,
    dir: &Path,
) -> Result<(), String> {
    use serde_json::{Value, json};
    let position = s.ed.scene.history().position();
    let png_size = |b64: &str| -> Result<(u32, u32), String> {
        let png = unbase64(b64);
        let r = png::Decoder::new(std::io::Cursor::new(png)).read_info().map_err(|e| e.to_string())?;
        Ok((r.info().width, r.info().height))
    };
    let r = s.agent_result(live, claude, "look", json!({"view": "current", "size": 300}));
    let text: Value = serde_json::from_str(r["content"][0]["text"].as_str().unwrap_or("")).unwrap_or_default();
    let (w, h) = png_size(r["content"][1]["data"].as_str().unwrap_or(""))?;
    let aspect = s.ed.width as f32 / s.ed.height as f32;
    check(
        r["isError"] == false
            && r["content"][1]["mimeType"] == "image/png"
            && text["projection"] == "perspective"
            && w == 300
            && (h as f32 - 300.0 / aspect).abs() <= 1.0,
        format!("look from the user's view is a PNG of the view's shape ({w} x {h}): {text}"),
    )?;
    let r = s.agent_result(live, claude, "look", json!({"view": "top"}));
    if let Some(data) = r["content"][1]["data"].as_str() {
        let _ = std::fs::write(dir.join("agent-look-top.png"), unbase64(data));
    }
    let text: Value = serde_json::from_str(r["content"][0]["text"].as_str().unwrap_or("")).unwrap_or_default();
    check(
        text["view"] == "top" && text["across"]["axis"] == "x" && text["down"]["axis"] == "z",
        format!("look from the top spans x across and z down: {text}"),
    )?;
    check(s.ed.scene.history().position() == position, "looking is no undo step".into())?;

    // Render settings: as the panel sets them, no undo step; a bad value changes nothing.
    let (t, e) = s.agent_call(
        live,
        claude,
        "render_settings",
        json!({"gamma": 1.8, "physical_sky": true, "samples": 64, "export_size": "720p"}),
    );
    let v: Value = serde_json::from_str(&t).unwrap_or_default();
    check(
        !e && v["gamma"] == 1.8 && v["physical_sky"] == true && v["export_pixels"] == json!([1280, 720]),
        format!("render_settings answers with every setting: {t}"),
    )?;
    check(
        (s.ed.ui.render.gamma - 1.8).abs() < 1e-6
            && s.ed.ui.render.physical_sky
            && (s.ed.preview.lighting.gamma - 1.8).abs() < 1e-6
            && s.ed.preview.lighting.physical_sky,
        "the panel and the path tracer take them".into(),
    )?;
    let (t, e) = s.agent_call(live, claude, "render_settings", json!({"gamma": 1.5, "vignette": 3}));
    check(e && t.contains("vignette") && (s.ed.ui.render.gamma - 1.8).abs() < 1e-6, format!("all or nothing: {t}"))?;
    check(s.ed.scene.history().position() == position, "render settings are no undo step".into())?;

    // Exports into the downloads folder, under the scene's name.
    let (t, e) = s.agent_call(live, claude, "export", json!({"kind": "obj"}));
    let names = |d: &Path| -> Vec<String> {
        let mut v: Vec<String> = std::fs::read_dir(d)
            .map(|r| r.flatten().map(|e| e.file_name().to_string_lossy().into_owned()).collect())
            .unwrap_or_default();
        v.sort();
        v
    };
    check(
        !e && names(downloads) == ["house.mtl", "house.obj", "house.png"] && t.contains("house.obj"),
        format!("the OBJ lands in Downloads: {:?} {t}", names(downloads)),
    )?;
    let obj = std::fs::read_to_string(downloads.join("house.obj")).unwrap_or_default();
    check(obj.contains("mtllib house.mtl"), "the OBJ names its .mtl".into())?;
    let (t, e) = s.agent_call(live, claude, "export", json!({"kind": "obj"}));
    let again = std::fs::read_to_string(downloads.join("house-1.obj")).unwrap_or_default();
    let mtl = std::fs::read_to_string(downloads.join("house-1.mtl")).unwrap_or_default();
    check(
        !e && again.contains("mtllib house-1.mtl\n")
            && mtl.contains("map_Kd house-1.png\n")
            && downloads.join("house-1.png").is_file(),
        format!("again it replaces nothing: {t}"),
    )?;
    let (t, e) = s.agent_call(live, claude, "export", json!({"kind": "slices"}));
    check(!e && downloads.join("house-slices.png").is_file(), format!("the PNG slices land in Downloads: {t}"))?;
    let (t, e) = s.agent_call(live, claude, "export", json!({"kind": "render"}));
    let png = std::fs::read(downloads.join("house-render.png")).unwrap_or_default();
    let size = png::Decoder::new(std::io::Cursor::new(png))
        .read_info()
        .map(|r| (r.info().width, r.info().height))
        .unwrap_or((0, 0));
    check(
        !e && t.contains("rendered 1280 x 720 at 64 samples") && size == (1280, 720),
        format!("a render export answers once traced, its PNG in Downloads ({size:?}): {t}"),
    )?;
    check(s.ed.exported.is_none(), "the agent's render asks the user nothing".into())?;
    check(s.ed.scene.history().position() == position, "exports are no undo step".into())?;
    s.ed.ui.render = vxr_app::render::RenderSettings::default();
    Ok(())
}

/// Standard base64 back to bytes.
fn unbase64(s: &str) -> Vec<u8> {
    let val = |c: u8| match c {
        b'A'..=b'Z' => c - b'A',
        b'a'..=b'z' => c - b'a' + 26,
        b'0'..=b'9' => c - b'0' + 52,
        b'+' => 62,
        _ => 63,
    } as u32;
    let mut out = Vec::new();
    for q in s.as_bytes().chunks(4) {
        let pad = q.iter().filter(|&&c| c == b'=').count();
        let n = q.iter().take(4 - pad).enumerate().fold(0u32, |n, (i, &c)| n | val(c) << (18 - 6 * i));
        out.extend(&n.to_be_bytes()[1..4 - pad]);
    }
    out
}

/// Live mode end to end through a real socket in a temporary folder: Cmd-K opens the
/// assistant's Agents tab, its switch opens the socket with a token only the user can read, an
/// agent's calls apply at once as undo steps named after it, the chip and the Agent edits list
/// show, the list's Undo takes back an earlier edit and the agent's `undo` its last, a call waits
/// while the user drags and is refused under review, Stop ends an agent for good, a new token
/// turns old agents away, and the switch closes the socket.
#[cfg(unix)]
fn agents_live(dir: &Path) -> Result<(), String> {
    use std::os::unix::fs::PermissionsExt;
    use vxr_mcp::live::{NOT_LIVE, REFUSED, SOCKET, STOPPED, TOKEN};
    let key = |key, modifiers| egui::Event::Key { key, physical_key: None, pressed: true, repeat: false, modifiers };
    let base = std::env::temp_dir().join(format!("vxs-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&base);
    std::fs::create_dir_all(&base).map_err(|e| e.to_string())?;
    let sock_dir = base.join("live");
    let downloads = base.join("downloads");
    let mut live = vxr_app::live::Live::new(sock_dir.clone()).downloads_to(downloads.clone());
    let r = (|| -> Result<(), String> {
        let mut s = Shot::new(house(), "house.vox", 2.0)?;
        s.ed.ui.coach = false;
        s.ed.cam.distance *= 1.9;
        let h = s.ed.height as f32;
        s.ed.cam.pan(-420.0, 0.0, h);
        s.live_frames(&mut live, 2);
        let mut claude = LiveAgent::new(&sock_dir, "claude-code");
        let (t, e) = s.agent_call(&mut live, &mut claude, "scene_summary", serde_json::json!({}));
        check(e && t == NOT_LIVE, "off by default: an agent is told how to turn it on".into())?;
        check(!sock_dir.join(SOCKET).exists(), "no socket while off".into())?;

        s.frames_with(2, vec![key(egui::Key::K, egui::Modifiers::COMMAND)]);
        s.frames_with(1, vec![egui::Event::Text("terminal agents".into())]);
        s.live_frames(&mut live, 1);
        s.frames_with(2, vec![key(egui::Key::Enter, egui::Modifiers::NONE)]);
        check(s.ed.ui.agents_panel, "Cmd-K Terminal agents opens the panel".into())?;
        check(
            s.ed.ui.hits.contains_key("agents-allow")
                && !s.ed.ui.hits.contains_key("agents-copy")
                && !s.ed.ui.agents.allowed,
            "the tab shows the allow step, switched off, and no setup lines yet".into(),
        )?;
        s.ui_click("agents-allow")?;
        s.live_frames(&mut live, 2);
        s.live_frames(&mut live, 4);
        check(
            s.ed.ui.hits.contains_key("agents-copy") && s.ed.ui.hits.contains_key("agents-copy-live"),
            "once it listens the connect step shows the setup lines".into(),
        )?;
        let mode = |p: std::path::PathBuf| std::fs::metadata(p).map(|m| m.permissions().mode() & 0o777).unwrap_or(0);
        check(
            live.on() && s.ed.ui.agents.allowed && sock_dir.join(SOCKET).exists(),
            "its switch opens the socket".into(),
        )?;
        check(
            mode(sock_dir.clone()) == 0o700 && mode(sock_dir.join(TOKEN)) == 0o600,
            "the folder and the token file are the user's only (700, 600)".into(),
        )?;

        // Each call applies at once, as one undo step named after the agent.
        let roof = s.ed.scene.map.counts[20];
        let calls = [
            ("paint", serde_json::json!({"slot": 3, "from_slot": 20}), "Claude Code: paint 392 voxels"),
            ("palette_set", serde_json::json!({"slot": 3, "hex": "#D94A3A"}), "Claude Code: palette_set slot 3"),
            (
                "line",
                serde_json::json!({"from": [8, 0, 8], "to": [8, 9, 8], "slot": 40}),
                "Claude Code: line 10 voxels",
            ),
            ("box", serde_json::json!({"from": [9, 7, 8], "to": [12, 9, 8], "slot": 50}), "Claude Code: box 12 voxels"),
        ];
        for (name, args, label) in calls {
            let (t, e) = s.agent_call(&mut live, &mut claude, name, args);
            check(
                !e && s.ed.scene.history().undo_label() == Some(label),
                format!("{name} applies at once as \"{label}\" ({})", t.chars().take(60).collect::<String>()),
            )?;
        }
        check(s.ed.scene.map.counts[20] == 0 && s.ed.scene.map.counts[3] == roof, "the roof is slot 3 now".into())?;
        agents_look_render(&mut s, &mut live, &mut claude, &downloads, dir)?;
        let names: Vec<&str> = s.ed.ui.agents.clients.iter().map(|c| c.1.as_str()).collect();
        check(names == ["Claude Code"], format!("the panel lists the agent: {names:?}"))?;
        s.frames_with(2, vec![key(egui::Key::Escape, egui::Modifiers::NONE)]);
        s.live_frames(&mut live, 2);
        check(!s.ed.ui.agents_panel, "Escape leaves the Agents tab".into())?;
        check(
            s.ed.ui.hits.contains_key("agent-stop") && s.ed.ui.hits.contains_key("agent-chip"),
            "the hint bar shows the agent with Stop".into(),
        )?;
        s.ui_click("ai-tab-agents")?;
        s.live_frames(&mut live, 4);
        check(
            s.ed.ui.agents_panel && (0..4).all(|i| s.ed.ui.hits.contains_key(&format!("agent-undo-{i}"))),
            "the Agents tab lists the 4 edits, each with Undo".into(),
        )?;
        s.live_frames(&mut live, 40);
        s.save_png(&dir.join("app-2x-agents.png"))?;

        // The list's Undo on the first edit: reverted under the three after it.
        s.ui_click("agent-undo-0")?;
        s.live_frames(&mut live, 2);
        check(
            s.ed.scene.map.counts[20] == roof
                && s.ed.scene.history().undo_label() == Some("Undo Claude Code: paint 392 voxels")
                && !s.ed.ui.agents.edits[0].applied
                && s.ed.ui.agents.edits[1..].iter().all(|e| e.applied),
            "the list's Undo takes back the paint alone, as its own step".into(),
        )?;
        check(!s.ed.ui.hits.contains_key("agent-undo-0"), "its row says Undone".into())?;
        // The agent's undo takes back its own last edit still in effect: the flag.
        let flag = s.voxels();
        let (t, e) = s.agent_call(&mut live, &mut claude, "undo", serde_json::json!({}));
        check(
            !e && t == "took back Claude Code: box 12 voxels" && s.voxels() == flag - 12,
            format!("the agent's undo: {t}"),
        )?;
        // It was not the last step, so it was taken back as a step of its own: Cmd-Z undoes that.
        s.frames_with(2, vec![key(egui::Key::Z, egui::Modifiers::COMMAND)]);
        check(s.voxels() == flag && s.ed.ui.agents.edits[3].applied, "Cmd-Z brings the flag back".into())?;

        // A call waits while the user drags, and runs when the drag ends.
        s.tool(Tool::Add, Shape::Voxel);
        let p = s.top_face_pixel([s.ed.width as f32 / 2.0, s.ed.height as f32 / 2.0])?;
        s.ed.pointer_moved(Some(p));
        s.ed.button(Button::Left, true, false);
        let msg = serde_json::json!({"jsonrpc": "2.0", "id": 1, "method": "tools/call",
            "params": {"name": "place", "arguments": {"cells": [[0, 12, 0]], "slot": 9}}});
        let (waited, reply) = std::thread::scope(|sc| {
            let h = sc.spawn(|| claude.0.handle(&msg));
            for _ in 0..30 {
                s.live_frames(&mut live, 1);
                std::thread::sleep(std::time::Duration::from_millis(2));
            }
            let waited = !h.is_finished();
            s.ed.button(Button::Left, false, false);
            while !h.is_finished() {
                s.live_frames(&mut live, 1);
            }
            (waited, h.join().unwrap().unwrap())
        });
        check(
            waited
                && reply["result"]["isError"] == false
                && s.ed.scene.history().undo_label() == Some("Claude Code: place 1 voxel"),
            "a call waits while the user drags, then applies".into(),
        )?;

        // Under review nothing else edits the scene: the agent is told to wait.
        s.ed.stage_json(HOUSE_SCRIPT)?;
        let (t, e) = s.agent_call(&mut live, &mut claude, "erase", serde_json::json!({"cells": [[0, 12, 0]]}));
        check(e && t.contains("reviewing"), "a call under review is refused".into())?;
        s.ed.discard_review();

        // Review every agent change: the panel's second switch, off by default.
        use vxr_app::agents::WAITING;
        s.ed.ui.open_agents();
        s.live_frames(&mut live, 2);
        check(!s.ed.ui.agents.review && s.ed.ui.hits.contains_key("agents-review"), "review is off by default".into())?;
        s.ui_click("agents-review")?;
        s.live_frames(&mut live, 2);
        check(s.ed.agents.review && s.ed.ui.agents.review, "its switch turns review on".into())?;
        s.live_frames(&mut live, 40);
        s.save_png(&dir.join("app-2x-agents-review-panel.png"))?;
        s.frames_with(2, vec![key(egui::Key::Escape, egui::Modifiers::NONE)]);
        s.live_frames(&mut live, 2);
        // Edits wait as a preview: no undo step, drawn as the AI change is, one card for every agent.
        let (voxels, label) = (s.voxels(), s.ed.scene.history().undo_label().map(str::to_string));
        let undo_label = |s: &Shot| s.ed.scene.history().undo_label().map(str::to_string);
        let (t, e) = s.agent_call(
            &mut live,
            &mut claude,
            "box",
            serde_json::json!({"from": [9, 10, 12], "to": [12, 11, 12], "slot": 50}),
        );
        check(
            !e && t.ends_with(WAITING),
            format!("with review on an edit waits ({})", t.chars().take(60).collect::<String>()),
        )?;
        let mut codex = LiveAgent::new(&sock_dir, "codex");
        let (t, e) =
            s.agent_call(&mut live, &mut codex, "place", serde_json::json!({"cells": [[14, 10, 8]], "slot": 9}));
        check(!e && t.ends_with(WAITING), "another agent's edit joins the same change".into())?;
        s.live_frames(&mut live, 2);
        let card = s.ed.ui.review.clone();
        check(
            card.as_ref().is_some_and(|c| c.agents && c.prompt == "Claude Code and Codex" && c.counts == [9, 0, 0])
                && s.ed.ui.hits.contains_key("review-apply")
                && !s.ed.ui.hits.contains_key("review-ask"),
            format!("the agent change card lists both agents' edits: {:?}", card.map(|c| (c.prompt, c.counts))),
        )?;
        check(
            s.voxels() == voxels + 9 && undo_label(&s) == label,
            "staged on the scene as a ghost, with no undo step yet".into(),
        )?;
        let (t, _) = s.agent_call(&mut live, &mut claude, "review_status", serde_json::json!({}));
        check(t.starts_with("1 edit of yours wait"), format!("review_status says it waits: {t}"))?;
        let (t, e) = s.agent_call(&mut live, &mut claude, "export", serde_json::json!({"kind": "obj"}));
        check(e && t.contains("waits for the user's review"), format!("no export under review: {t}"))?;
        let r = s.agent_result(&mut live, &mut claude, "look", serde_json::json!({"size": 64}));
        check(
            r["isError"] == false && r["content"][1]["type"] == "image",
            "look shows the change under review".into(),
        )?;
        s.live_frames(&mut live, 40);
        s.save_png(&dir.join("app-2x-agent-review.png"))?;
        // Apply: one undo step per edit, named after its agent; each agent hears it.
        s.ui_click("review-apply")?;
        s.live_frames(&mut live, 2);
        let n = s.ed.agents.edits.len();
        let last: Vec<&str> = s.ed.agents.edits[n - 2..].iter().map(|e| e.label.as_str()).collect();
        check(
            s.ed.review.is_none()
                && undo_label(&s).as_deref() == Some("Codex: place 1 voxel")
                && last == ["Claude Code: box 8 voxels", "Codex: place 1 voxel"],
            format!("Apply lands each edit as an undo step named after its agent: {last:?}"),
        )?;
        let (t, _) = s.agent_call(&mut live, &mut claude, "review_status", serde_json::json!({}));
        check(t.starts_with("vxr: the user applied your 1 edit"), format!("review_status tells the agent: {t}"))?;
        let (t, _) = s.agent_call(&mut live, &mut codex, "scene_summary", serde_json::json!({}));
        check(t.starts_with("vxr: the user applied your 1 edit"), "the other hears it with its next call".into())?;
        // Discard: the scene is as it was, and the agent hears it.
        let (voxels, label) = (s.voxels(), undo_label(&s));
        s.agent_call(&mut live, &mut claude, "erase", serde_json::json!({"cells": [[14, 10, 8]]}));
        check(s.ed.ui.review.as_ref().is_some_and(|c| c.counts == [0, 1, 0]), "an erase waits too".into())?;
        s.ui_click("review-discard")?;
        s.live_frames(&mut live, 2);
        check(
            s.ed.review.is_none() && s.voxels() == voxels && undo_label(&s) == label,
            "Discard leaves the scene and its undo steps as they were".into(),
        )?;
        let (t, _) = s.agent_call(&mut live, &mut claude, "scene_summary", serde_json::json!({}));
        check(t.starts_with("vxr: the user discarded"), "the agent hears it was discarded".into())?;
        // The assistant's request waits for the agents' change, and Stop leaves the change for the user.
        s.agent_call(&mut live, &mut codex, "place", serde_json::json!({"cells": [[8, 12, 8]], "slot": 9}));
        check(!s.ed.ask("make it red"), "the assistant waits for the agents' change".into())?;
        let conn = s.ed.agents.clients.iter().find(|c| c.1 == "Codex").map(|c| c.0).unwrap_or(0);
        s.ed.ui.open_agents();
        s.live_frames(&mut live, 2);
        s.ui_click(&format!("agents-stop-{conn}"))?;
        s.live_frames(&mut live, 3);
        let (t, e) = s.agent_call(&mut live, &mut codex, "scene_summary", serde_json::json!({}));
        check(
            e && t == STOPPED && s.ed.ui.review.as_ref().is_some_and(|c| c.counts == [1, 0, 0]),
            "Stop during a review cuts the agent off; its edit waits for the user".into(),
        )?;
        s.ui_click("review-discard")?;
        s.ui_click("agents-review")?;
        s.live_frames(&mut live, 2);
        check(s.ed.review.is_none() && !s.ed.agents.review, "review off again".into())?;
        s.frames_with(2, vec![key(egui::Key::Escape, egui::Modifiers::NONE)]);
        s.live_frames(&mut live, 2);

        // Stop on the chip ends the agent for good; another can still connect.
        s.live_frames(&mut live, 2);
        s.ui_click("agent-stop")?;
        s.live_frames(&mut live, 3);
        check(
            s.ed.ui.agents.clients.is_empty() && !s.ed.ui.hits.contains_key("agent-chip"),
            "Stop disconnects it".into(),
        )?;
        let (t, e) = s.agent_call(&mut live, &mut claude, "scene_summary", serde_json::json!({}));
        let (t2, _) = s.agent_call(&mut live, &mut claude, "scene_summary", serde_json::json!({}));
        check(e && t == STOPPED && t2 == STOPPED, "the stopped agent is told so, and stays stopped".into())?;
        let mut codex = LiveAgent::new(&sock_dir, "codex");
        let (_, e) =
            s.agent_call(&mut live, &mut codex, "place", serde_json::json!({"cells": [[1, 12, 1]], "slot": 9}));
        check(!e && s.ed.scene.history().undo_label() == Some("Codex: place 1 voxel"), "another agent edits".into())?;

        // The agent's redo brings back what its undo took; Save and New scene are the window's own.
        let v = s.voxels();
        let (t, e) = s.agent_call(&mut live, &mut codex, "undo", serde_json::json!({}));
        check(!e && s.voxels() == v - 1, format!("the agent's undo: {t}"))?;
        let (t, e) = s.agent_call(&mut live, &mut codex, "redo", serde_json::json!({}));
        check(
            !e && t == "brought back Codex: place 1 voxel" && s.voxels() == v,
            format!("the agent's redo brings it back: {t}"),
        )?;
        s.host.clear();
        let (t, e) = s.agent_call(&mut live, &mut codex, "save", serde_json::json!({}));
        check(
            !e && t.contains("as Cmd-S does") && s.host.contains(&Action::Save),
            format!("the agent's save is the window's Cmd-S: {t}"),
        )?;
        // With unsaved edits New scene asks the user first, as the UI's New does; no flag skips it.
        s.host.clear();
        let (t, e, waited) =
            s.agent_call_confirmed(&mut live, &mut codex, "new_scene", serde_json::json!({"discard": true}), |s| {
                s.ui_click("new-cancel")
            })?;
        check(
            e && t.contains("kept") && waited == v && s.voxels() == v && !s.ed.ui.confirm_new && s.host.is_empty(),
            format!("new_scene with unsaved edits asks first, and Cancel keeps the scene: {t}"),
        )?;
        let (t, e, waited) =
            s.agent_call_confirmed(&mut live, &mut codex, "new_scene", serde_json::json!({}), |s| {
                s.ui_click("new-discard")
            })?;
        check(
            !e && t.starts_with("new scene")
                && waited == v
                && s.voxels() == 0
                && s.host.contains(&Action::New)
                && s.ed.ui.scene_name == "Untitled",
            format!(
                "Start new in the confirm starts the window's New scene ({})",
                t.chars().take(40).collect::<String>()
            ),
        )?;
        s.host.clear();
        let (t, e) = s.agent_call(&mut live, &mut codex, "new_scene", serde_json::json!({}));
        check(
            !e && t.starts_with("new scene") && !s.ed.ui.confirm_new && s.host.contains(&Action::New),
            format!("new_scene on a saved scene starts at once ({})", t.chars().take(40).collect::<String>()),
        )?;
        // A file opened while the confirm is up (the browser's picker answers late) replaces the scene.
        s.agent_call(&mut live, &mut codex, "place", serde_json::json!({"cells": [[0, 0, 0]], "slot": 9}));
        let (t, e, waited) =
            s.agent_call_confirmed(&mut live, &mut codex, "new_scene", serde_json::json!({}), |s| {
                s.ed.load(house(), "other.vox")
            })?;
        check(
            e && t.contains("opened other.vox") && waited == 1 && s.voxels() > 1 && !s.ed.ui.confirm_new,
            format!("a file opened behind the agent's confirm is reported as opened: {t}"),
        )?;

        // A new token from the panel disconnects every agent and turns old ones away.
        s.ed.ui.open_agents();
        s.live_frames(&mut live, 2);
        s.live_frames(&mut live, 40);
        s.save_png(&dir.join("app-2x-agents-panel.png"))?;
        let old = std::fs::read_to_string(sock_dir.join(TOKEN)).map_err(|e| e.to_string())?;
        s.ui_click("agents-regenerate")?;
        s.live_frames(&mut live, 3);
        let new = std::fs::read_to_string(sock_dir.join(TOKEN)).map_err(|e| e.to_string())?;
        check(old != new && s.ed.ui.agents.clients.is_empty(), "Regenerate writes a new token and disconnects".into())?;
        let (t, e) = s.agent_call(&mut live, &mut codex, "scene_summary", serde_json::json!({}));
        check(e && t == REFUSED, "an agent with the old token is refused".into())?;

        // The switch closes the socket.
        s.ui_click("agents-allow")?;
        s.live_frames(&mut live, 2);
        check(!live.on() && !sock_dir.join(SOCKET).exists(), "the switch turns it off and removes the socket".into())?;
        Ok(())
    })();
    drop(live);
    let _ = std::fs::remove_dir_all(&base);
    r
}

/// The page's script in the browser, natively: the WebSocket to `vxr bridge` that
/// `vxr_app::bridge::Page` asks for, opened as Chrome would (with the editor's origin).
#[cfg(unix)]
struct PageScript {
    addr: std::net::SocketAddr,
    ws: Option<tungstenite::WebSocket<std::net::TcpStream>>,
}

#[cfg(unix)]
impl PageScript {
    /// Do what the page asked, then hand it what the bridge sent; a socket that closed is
    /// reported as the browser would (no reason).
    fn pump(&mut self, page: &mut vxr_app::bridge::Page, ed: &mut Editor) {
        use tungstenite::client::IntoClientRequest;
        use vxr_app::bridge::Out;
        for o in page.take_out() {
            match o {
                Out::Connect => {
                    let mut req = format!("ws://{}/", self.addr).into_client_request().unwrap();
                    let origin = tungstenite::http::HeaderValue::from_static(vxr_mcp::bridge::ORIGIN);
                    req.headers_mut().insert("origin", origin);
                    let ws = std::net::TcpStream::connect(self.addr)
                        .ok()
                        .and_then(|t| tungstenite::client(req, t).ok())
                        .map(|(ws, _)| ws);
                    match ws {
                        Some(ws) => {
                            let _ = ws.get_ref().set_read_timeout(Some(std::time::Duration::from_millis(1)));
                            self.ws = Some(ws);
                            page.opened();
                        }
                        None => page.closed("", ed),
                    }
                }
                Out::Send(m) => {
                    if let Some(ws) = &mut self.ws {
                        let _ = ws.send(tungstenite::Message::text(m));
                    }
                }
                Out::Close => {
                    if let Some(mut ws) = self.ws.take() {
                        let _ = ws.close(None);
                        let _ = ws.flush();
                    }
                }
            }
        }
        let mut closed = false;
        while let Some(ws) = &mut self.ws {
            match ws.read() {
                Ok(tungstenite::Message::Text(t)) => page.message(&t, ed),
                Ok(tungstenite::Message::Close(_)) => closed = true,
                Ok(_) => continue,
                Err(tungstenite::Error::Io(e))
                    if matches!(e.kind(), std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut) => {}
                Err(_) => closed = true,
            }
            if closed {
                self.ws = None;
                page.closed("", ed);
            }
            break;
        }
        // The page's script hands it the next message at once, not a frame later.
        if self.ws.as_ref().is_some_and(|_| !closed) {
            page.tick(ed);
        }
    }
}

#[cfg(unix)]
impl Shot {
    /// One frame of the browser editor with its link to `vxr bridge`, as `vxr-web` runs it:
    /// the page's script, the agents' requests, the frame, then what the UI asked of live mode.
    fn bridge_frames(&mut self, page: &mut vxr_app::bridge::Page, script: &mut PageScript, n: usize) {
        for _ in 0..n {
            script.pump(page, &mut self.ed);
            page.tick(&mut self.ed);
            self.frames(1);
            for a in std::mem::take(&mut self.host) {
                if !page.act(&a, &mut self.ed) {
                    self.host.push(a);
                }
            }
            script.pump(page, &mut self.ed);
        }
    }

    /// Frames until `done`, at most 2 s.
    fn bridge_until(
        &mut self,
        page: &mut vxr_app::bridge::Page,
        script: &mut PageScript,
        done: impl Fn(&Self) -> bool,
    ) -> bool {
        let start = std::time::Instant::now();
        while !done(self) {
            if start.elapsed() > std::time::Duration::from_secs(2) {
                return false;
            }
            self.bridge_frames(page, script, 1);
            std::thread::sleep(std::time::Duration::from_millis(2));
        }
        true
    }

    /// An agent's tool call, answered by the tab through the bridge while frames run.
    fn bridge_call(
        &mut self,
        page: &mut vxr_app::bridge::Page,
        script: &mut PageScript,
        agent: &mut LiveAgent,
        name: &str,
        args: serde_json::Value,
    ) -> (String, bool) {
        let msg = serde_json::json!({"jsonrpc": "2.0", "id": 1, "method": "tools/call",
            "params": {"name": name, "arguments": args}});
        let reply = std::thread::scope(|sc| {
            let h = sc.spawn(|| agent.0.handle(&msg));
            while !h.is_finished() {
                self.bridge_frames(page, script, 1);
                std::thread::sleep(std::time::Duration::from_millis(1));
            }
            h.join().unwrap().unwrap()
        });
        self.bridge_frames(page, script, 3);
        let r = &reply["result"];
        (r["content"][0]["text"].as_str().unwrap_or("").to_string(), r["isError"].as_bool().unwrap_or(true))
    }
}

/// Live mode in the browser end to end, natively: a real `vxr bridge` on a loopback port with
/// its socket in a temporary folder, the editor with the browser's panel and the page's link
/// ([`vxr_app::bridge::Page`]), and real `vxr mcp --live` servers. The panel says to run
/// `vxr bridge` and takes its code; a malformed or wrong code is refused with nothing for
/// agents; the right one pairs, and an agent's calls apply as undo steps named after it; Stop
/// and Regenerate in the panel behave as on desktop; the switch and the bridge stopping end it.
#[cfg(unix)]
fn agents_bridge(dir: &Path) -> Result<(), String> {
    use vxr_app::bridge::{LOST, Page};
    use vxr_mcp::bridge::{Bridge, Config, WRONG_CODE, show_code};
    use vxr_mcp::live::{NOT_LIVE, REFUSED, SOCKET, STOPPED, TOKEN};
    let base = std::env::temp_dir().join(format!("vxw-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&base);
    std::fs::create_dir_all(&base).map_err(|e| e.to_string())?;
    let sock_dir = base.join("live");
    let r = (|| -> Result<(), String> {
        let cfg = Config { addr: ([127, 0, 0, 1], 0).into(), dir: sock_dir.clone(), dev_origins: false };
        let bridge = Bridge::bind(cfg, |_| {})?;
        let code = bridge.code();
        let bridge = bridge.spawn();
        let mut script = PageScript { addr: bridge.addr(), ws: None };
        let mut page = Page::new();
        let mut s = Shot::new(house(), "house.vox", 2.0)?;
        s.ed.ui.coach = false;
        s.ed.cam.distance *= 1.9;
        let h = s.ed.height as f32;
        s.ed.cam.pan(-420.0, 0.0, h);
        page.sync(&mut s.ed);
        s.ed.ui.open_agents();
        s.bridge_frames(&mut page, &mut script, 2);
        let v = &s.ed.ui.agents;
        check(
            v.web
                && v.available
                && !v.allowed
                && ["agents-copy-bridge", "agents-code"].iter().all(|k| s.ed.ui.hits.contains_key(*k))
                && !s.ed.ui.hits.contains_key("agents-disconnect"),
            "the browser's tab says to run vxr bridge and takes its code, not paired".into(),
        )?;
        let mut claude = LiveAgent::new(&sock_dir, "claude-code");
        let (t, e) = s.bridge_call(&mut page, &mut script, &mut claude, "scene_summary", serde_json::json!({}));
        check(e && t == NOT_LIVE && !sock_dir.join(SOCKET).exists(), "before pairing an agent finds nothing".into())?;

        // Fewer than eight characters open nothing; a character no code has is refused; a wrong
        // code is refused by the bridge.
        s.type_into("agents-code", "K7Q")?;
        s.bridge_frames(&mut page, &mut script, 2);
        check(
            s.ed.ui.agents_code == "K7Q" && script.ws.is_none() && s.ed.ui.agents.error.is_none(),
            "three characters: nothing opens".into(),
        )?;
        s.type_into("agents-code", "k7u")?;
        check(s.ed.ui.agents_code == "K7", "a character no code has (U) is refused".into())?;
        s.type_into("agents-code", "0000-0000")?;
        let refused = s.bridge_until(&mut page, &mut script, |s| s.ed.ui.agents.error.as_deref() == Some(WRONG_CODE));
        check(
            refused
                && s.ed.ui.agents.fail == Some(vxr_app::agents::Fail::Wrong)
                && !s.ed.ui.agents.allowed
                && !sock_dir.join(SOCKET).exists()
                && s.ed.ui.hits.contains_key("agents-retry"),
            "the eighth character connects, a wrong code is refused, with no socket for agents".into(),
        )?;
        s.bridge_frames(&mut page, &mut script, 40);
        s.save_png(&dir.join("app-2x-agents-web-wrong.png"))?;
        s.ui_click("agents-retry")?;
        s.bridge_frames(&mut page, &mut script, 3);
        check(
            s.ed.ui.agents.fail.is_none() && s.ed.ui.agents_code.is_empty() && s.ed.ui.agents.error.is_none(),
            "Try again clears the error and the code".into(),
        )?;

        // The code vxr bridge printed, typed loosely: spaces, dashes, case and O for 0 are read.
        s.type_into("agents-code", &show_code(&code).to_lowercase())?;
        let paired = s.bridge_until(&mut page, &mut script, |s| s.ed.ui.agents.allowed);
        check(
            paired && s.ed.ui.agents.error.is_none() && sock_dir.join(SOCKET).exists() && bridge.paired(),
            "the right code pairs the tab, and the bridge opens the agents' socket".into(),
        )?;
        let calls = [
            ("box", serde_json::json!({"from": [9, 7, 8], "to": [12, 9, 8], "slot": 50}), "Claude Code: box 12 voxels"),
            ("palette_set", serde_json::json!({"slot": 50, "hex": "#3A7BD9"}), "Claude Code: palette_set slot 50"),
        ];
        for (name, args, label) in calls {
            let (t, e) = s.bridge_call(&mut page, &mut script, &mut claude, name, args);
            check(
                !e && s.ed.scene.history().undo_label() == Some(label),
                format!(
                    "through the bridge, {name} applies at once as \"{label}\" ({})",
                    t.chars().take(60).collect::<String>()
                ),
            )?;
        }
        s.bridge_until(&mut page, &mut script, |s| !s.ed.ui.agents.clients.is_empty());
        let names: Vec<String> = s.ed.ui.agents.clients.iter().map(|c| c.1.clone()).collect();
        check(names == ["Claude Code"], format!("the panel lists the agent: {names:?}"))?;
        s.bridge_frames(&mut page, &mut script, 40);
        s.save_png(&dir.join("app-2x-agents-web.png"))?;

        // Stop in the panel: that agent is told so, for good.
        let conn = s.ed.ui.agents.clients[0].0;
        s.ui_click(&format!("agents-stop-{conn}"))?;
        for a in std::mem::take(&mut s.host) {
            if !page.act(&a, &mut s.ed) {
                s.host.push(a);
            }
        }
        // A listing and a request the bridge sent before it read the stop change nothing.
        let label = s.ed.scene.history().undo_label().map(str::to_string);
        let stale = serde_json::json!({"id": 1_000_000, "conn": conn, "client": "Claude Code", "call": "place",
            "args": {"cells": [[2, 12, 2]], "slot": 9}});
        page.message(&serde_json::json!({"clients": [[conn, "Claude Code"]]}).to_string(), &mut s.ed);
        page.message(&stale.to_string(), &mut s.ed);
        page.tick(&mut s.ed);
        check(
            s.ed.agents.clients.is_empty() && s.ed.scene.history().undo_label().map(str::to_string) == label,
            "a stopped agent's request already on its way does not run".into(),
        )?;
        s.bridge_until(&mut page, &mut script, |s| s.ed.ui.agents.clients.is_empty());
        let (t, e) = s.bridge_call(&mut page, &mut script, &mut claude, "scene_summary", serde_json::json!({}));
        check(e && t == STOPPED, "Stop in the panel ends that agent for good".into())?;
        let mut codex = LiveAgent::new(&sock_dir, "codex");
        let (_, e) = s.bridge_call(
            &mut page,
            &mut script,
            &mut codex,
            "place",
            serde_json::json!({"cells": [[1, 12, 1]], "slot": 9}),
        );
        check(!e && s.ed.scene.history().undo_label() == Some("Codex: place 1 voxel"), "another agent edits".into())?;

        // Regenerate: a new token, every agent disconnected, old ones turned away.
        let old = std::fs::read_to_string(sock_dir.join(TOKEN)).map_err(|e| e.to_string())?;
        s.bridge_until(&mut page, &mut script, |s| !s.ed.ui.agents.clients.is_empty());
        let conn = s.ed.ui.agents.clients[0].0;
        s.ui_click("agents-regenerate")?;
        for a in std::mem::take(&mut s.host) {
            if !page.act(&a, &mut s.ed) {
                s.host.push(a);
            }
        }
        // A request on its way when Regenerate was pressed waits for the answer, then is dropped.
        let label = s.ed.scene.history().undo_label().map(str::to_string);
        let stale = serde_json::json!({"id": 1_000_001, "conn": conn, "client": "Codex", "call": "place",
            "args": {"cells": [[2, 12, 2]], "slot": 9}});
        page.message(&stale.to_string(), &mut s.ed);
        page.tick(&mut s.ed);
        let held = s.ed.scene.history().undo_label().map(str::to_string) == label;
        s.bridge_until(&mut page, &mut script, |s| s.ed.ui.agents.clients.is_empty());
        check(
            held && s.ed.scene.history().undo_label().map(str::to_string) == label,
            "a request on its way at Regenerate does not run".into(),
        )?;
        let new = std::fs::read_to_string(sock_dir.join(TOKEN)).map_err(|e| e.to_string())?;
        let (t, e) = s.bridge_call(&mut page, &mut script, &mut codex, "scene_summary", serde_json::json!({}));
        check(old != new && e && t == REFUSED, "Regenerate makes a new token and turns old agents away".into())?;

        // Disconnect ends the link: the bridge closes the agents' socket.
        s.ui_click("agents-disconnect")?;
        let off = s.bridge_until(&mut page, &mut script, |_| !sock_dir.join(SOCKET).exists() && !bridge.paired());
        s.bridge_frames(&mut page, &mut script, 2);
        check(off && !s.ed.ui.agents.allowed, "Disconnect unpairs the tab and the socket closes".into())?;
        let mut fresh = LiveAgent::new(&sock_dir, "claude-code");
        let (t, e) = s.bridge_call(&mut page, &mut script, &mut fresh, "scene_summary", serde_json::json!({}));
        check(e && t == NOT_LIVE, "an agent then finds the tab off".into())?;

        // The same code pairs again; the bridge stopping is shown.
        s.type_into("agents-code", &show_code(&code))?;
        check(s.bridge_until(&mut page, &mut script, |s| s.ed.ui.agents.allowed), "the code pairs again".into())?;
        drop(bridge);
        let lost = s.bridge_until(&mut page, &mut script, |s| !s.ed.ui.agents.allowed);
        check(
            lost && s.ed.ui.agents.error.as_deref() == Some(LOST)
                && s.ed.ui.agents.fail == Some(vxr_app::agents::Fail::Lost),
            "the bridge stopping turns the tab off".into(),
        )?;
        s.bridge_frames(&mut page, &mut script, 4);
        check(
            ["agents-copy-bridge", "agents-retry", "agents-code"].iter().all(|k| s.ed.ui.hits.contains_key(*k)),
            "its step turns red with the command, Try again and the code field for the new code".into(),
        )?;
        s.bridge_frames(&mut page, &mut script, 40);
        s.save_png(&dir.join("app-2x-agents-web-lost.png"))?;
        Ok(())
    })();
    let _ = std::fs::remove_dir_all(&base);
    r
}

/// `vxr --agents <dir> [scene.vox]`, only in a build with the `live` feature: the live mode
/// check from a real terminal agent. The editor runs headless on the scene (the house if none
/// is given) with "Allow agents to edit this window" switched on through the Agents tab, at the
/// default socket, as the window would; while an agent is connected each applied or undone edit
/// writes `agents-live.png` (chip and card), and once `<dir>/done` appears (or after 15 minutes) it
/// writes `agents-live-done.png` and the scene as `agents-live.vox` and stops.
#[cfg(all(feature = "live", unix))]
pub fn agents(dir: &Path, scene: Option<&Path>) -> Result<(), String> {
    std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    let (scene, name) = match scene {
        Some(p) => crate::native::open(p)?,
        None => (house(), "house.vox".into()),
    };
    let mut s = Shot::new(scene, &name, 2.0)?;
    s.ed.ui.coach = false;
    s.ed.cam.distance *= 1.9;
    let h = s.ed.height as f32;
    s.ed.cam.pan(-420.0, 0.0, h);
    let mut live = vxr_app::live::Live::new(vxr_mcp::live::dir());
    s.ed.ui.open_agents();
    s.live_frames(&mut live, 2);
    s.ui_click("agents-allow")?;
    s.live_frames(&mut live, 2);
    if !live.on() {
        return Err(s.ed.agents.error.clone().unwrap_or("live mode did not start".into()));
    }
    s.ed.ui.agents_panel = false;
    println!("listening; touch {} to stop", dir.join("done").display());
    let start = std::time::Instant::now();
    let mut shown = (0, s.ed.agents.version);
    while !dir.join("done").exists() && start.elapsed() < std::time::Duration::from_secs(900) {
        s.live_frames(&mut live, 1);
        let n = s.ed.agents.edits.len();
        if s.ed.agents.version != shown.1 && !s.ed.agents.clients.is_empty() {
            match s.ed.agents.edits.last() {
                Some(e) if n != shown.0 => println!("edit {n}: {}", e.label),
                _ => println!("undo: {:?}", s.ed.scene.history().undo_label()),
            }
            shown = (n, s.ed.agents.version);
            s.live_frames(&mut live, 30);
            s.save_png(&dir.join("agents-live.png"))?;
        }
        std::thread::sleep(std::time::Duration::from_millis(8));
    }
    s.live_frames(&mut live, 40);
    s.save_png(&dir.join("agents-live-done.png"))?;
    std::fs::write(dir.join("agents-live.vox"), s.ed.scene.to_vox()).map_err(|e| e.to_string())?;
    let labels: Vec<String> = s.ed.agents.edits.iter().map(|e| e.label.clone()).collect();
    println!("{} edits: {labels:?}", labels.len());
    Ok(())
}
