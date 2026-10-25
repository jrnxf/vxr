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
}

impl Shot {
    fn new(scene: Scene, name: &str, ppp: f32) -> Result<Self, String> {
        let instance = wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle());
        let gpu = pollster::block_on(Gpu::new(instance, None))?;
        let (w, h) = ((POINTS.0 as f32 * ppp) as u32, (POINTS.1 as f32 * ppp) as u32);
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
        Ok(Self { ed, target })
    }

    /// Draw `n` frames (egui needs a couple to lay out and upload fonts).
    fn frames(&mut self, n: usize) {
        self.frames_with(n, Vec::new());
    }

    /// Draw `n` frames, the first with `events` as egui input.
    fn frames_with(&mut self, n: usize, mut events: Vec<egui::Event>) {
        let view = self.target.create_view(&Default::default());
        for i in 0..n {
            let size = egui::vec2(self.ed.width as f32 / self.ed.ppp, self.ed.height as f32 / self.ed.ppp);
            let mut input = egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, size)),
                time: Some(i as f64 / 60.0),
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
    s.save_png(&dir.join("app-2x-edited.png"))
}
