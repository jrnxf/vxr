//! Headless run for evidence and smoke testing: no window, so it works with the screen
//! locked. It renders the editor offscreen, drives every tool through the same pointer path
//! the window uses, saves and reopens the scene, and writes screenshots.

use std::path::Path;

use vxr_app::Editor;
use vxr_app::editor::{Button, NEW_SIZE};
use vxr_app::ui::{Shape, Tool};
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
        Ok(Self { ed, target, time: 0.0 })
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
                ..Default::default()
            };
            input.viewports.entry(egui::ViewportId::ROOT).or_default().native_pixels_per_point = Some(self.ed.ppp);
            self.ed.frame(input, &view);
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
    render(&mut s, dir)?;

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
    }
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

/// The Render panel through egui input on the drawn UI: R opens it, each setting reaches the
/// path tracer (exposure without a restart), Export PNG renders and reads back a PNG at the
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
    s.ui_click("rs-export")?;
    check(started && s.ed.export.is_none(), "Cmd-K Export PNG starts an export; the panel's button cancels it".into())?;
    s.frames_with(2, vec![key(egui::Key::Escape, egui::Modifiers::NONE)]);
    check(!s.ed.ui.render_panel, "Escape closes the Render panel".into())?;
    s.ed.ui.render = Default::default();
    s.frames(2);
    Ok(())
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
    println!(
        "info frame time, island at 3024x1964 (median, CPU + GPU): {off:.2} ms without the render corner, \
         {on:.2} ms with it while editing, {idle:.2} ms idle; one 640x360 sample {ms:.2} ms ({})",
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

    // Cmd-drag a swatch onto another swaps them, as Ctrl+drag does in MagicaVoxel.
    s.ed.ui.slot = used;
    s.frames(2);
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
    s.ed.undo();
    s.ed.undo();
    s.ed.undo();
    check(s.ed.scene.palette[used as usize] == before, "undoing every palette edit restores the color".into())
}
