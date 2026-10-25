//! The path-traced preview on the real GPU: it sees the same geometry as the CPU ray cast,
//! converges, follows edits and recolours without a restart, and restarts on light changes.
//! Skips (passes with a note) when the machine has no GPU adapter.

use vxr_core::{BrickMap, Scene};
use vxr_render::preview::{PREVIEW_H, PREVIEW_W};
use vxr_render::{EditBox, Gpu, GpuMap, OrbitCamera, Preview, brickmap_layout};

fn gpu() -> Option<Gpu> {
    let instance = wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle());
    match pollster::block_on(Gpu::new(instance, None)) {
        Ok(g) => Some(g),
        Err(e) => {
            eprintln!("skipping GPU test: {e}");
            None
        }
    }
}

/// A ground slab with steps, all in slot 1, which is pure red.
fn red_scene() -> Scene {
    let mut s = Scene::new([48, 32, 48]);
    let mut cells = Vec::new();
    for x in 4..44 {
        for z in 4..44 {
            for y in 0..4 + (x / 6 + z / 9) % 5 {
                cells.push(([x, y, z], 1u8));
            }
        }
    }
    s.edit(cells);
    s.set_color(1, [255, 0, 0, 255]);
    s.set_color(9, [0, 255, 0, 255]);
    s.map.track_dirty(true);
    s
}

fn camera_for(map: &BrickMap) -> OrbitCamera {
    let s = map.size_voxels();
    OrbitCamera::framing_aspect(
        glam::Vec3::ZERO,
        glam::Vec3::new(s[0] as f32, s[1] as f32, s[2] as f32),
        PREVIEW_W as f32 / PREVIEW_H as f32,
    )
}

/// The CPU ray cast's palette index per preview pixel (0 = miss).
fn cpu_values(map: &BrickMap, cam: &OrbitCamera) -> Vec<u8> {
    let (w, h) = (PREVIEW_W, PREVIEW_H);
    let mut out = Vec::with_capacity((w * h) as usize);
    for y in 0..h {
        for x in 0..w {
            let (o, d) = cam.ray(x as f32 + 0.5, y as f32 + 0.5, w as f32, h as f32);
            out.push(map.raycast(o.to_array(), d.to_array(), 1e6).map_or(0, |h| h.value));
        }
    }
    out
}

struct Rig {
    g: Gpu,
    map: GpuMap,
    preview: Preview,
    cam: OrbitCamera,
}

impl Rig {
    fn new(g: Gpu, scene: &Scene) -> Self {
        let layout = brickmap_layout(&g.device);
        let map = GpuMap::new(&g.device, &g.queue, &layout, &scene.map, &scene.palette).unwrap();
        let preview = Preview::new(&g.device, &g.queue, &layout, &scene.palette, g.timestamps);
        Self { g, map, preview, cam: camera_for(&scene.map) }
    }

    fn frames(&mut self, n: u32, mut edit: Option<EditBox>) {
        for _ in 0..n {
            let mut enc = self.g.device.create_command_encoder(&Default::default());
            self.preview.encode(&self.g.queue, &mut enc, &self.map, &self.cam, edit.take(), true);
            self.g.queue.submit([enc.finish()]);
            self.preview.after_submit(&self.g.device);
        }
    }

    fn image(&self) -> Vec<u8> {
        pollster::block_on(self.preview.read(&self.g.device, &self.g.queue))
    }
}

/// Fraction of pixels whose colour class disagrees with the CPU: red where slot 1 is hit,
/// green where slot 9 is, the dark editor background (blue over red, dim) on a miss.
fn mismatch(img: &[u8], cpu: &[u8]) -> f64 {
    let bad = img
        .as_chunks::<4>()
        .0
        .iter()
        .zip(cpu)
        .filter(|(p, v)| {
            let [r, g, b, _] = **p;
            match v {
                0 => !(b > r && r < 60),
                1 => !(r > g && r > b),
                9 => !(g > r && g > b),
                _ => true,
            }
        })
        .count();
    bad as f64 / cpu.len() as f64
}

#[test]
fn preview_matches_geometry_and_converges() {
    let Some(g) = gpu() else { return };
    let scene = red_scene();
    let mut rig = Rig::new(g, &scene);
    rig.frames(60, None);
    assert!(rig.preview.samples() >= 10.0, "only {} samples after 60 idle frames", rig.preview.samples());
    let (ms, measured) = rig.preview.sample_ms();
    eprintln!("one 640x360 sample: {ms:.2} ms ({})", if measured { "GPU timestamps" } else { "assumed" });
    assert!(measured || !rig.g.timestamps, "timestamps are on but the budget never learnt from them");
    let cpu = cpu_values(&scene.map, &rig.cam);
    assert!(cpu.iter().filter(|&&v| v != 0).count() > cpu.len() / 10, "the scene does not fill the view");
    let a = rig.image();
    let m = mismatch(&a, &cpu);
    assert!(m < 0.01, "{:.2}% of pixels disagree with the CPU ray cast", m * 100.0);
    // Converged: more samples barely change the image.
    rig.frames(60, None);
    let b = rig.image();
    let diff = a.iter().zip(&b).map(|(x, y)| x.abs_diff(*y) as f64).sum::<f64>() / a.len() as f64;
    assert!(diff < 2.0, "mean change {diff:.2}/255 after 60 more frames: not converging");
}

#[test]
fn edits_and_recolours_update_without_a_restart() {
    let Some(g) = gpu() else { return };
    let mut scene = red_scene();
    let mut rig = Rig::new(g, &scene);
    rig.frames(30, None);
    let restarts = rig.preview.restarts;

    // A green tower: only pixels whose rays cross it restart, so the sample count keeps going.
    let (lo, hi) = ([20, 8, 20], [25, 20, 25]);
    scene.edit(
        (lo[0]..=hi[0]).flat_map(|x| (lo[1]..=hi[1]).flat_map(move |y| (lo[2]..=hi[2]).map(move |z| ([x, y, z], 9u8)))),
    );
    rig.map.upload_dirty(&rig.g.device, &rig.g.queue, &mut scene.map, &scene.palette).unwrap();
    rig.frames(8, Some(EditBox::voxels(lo, hi)));
    assert_eq!(rig.preview.restarts, restarts, "the edit restarted the whole preview");
    let cpu = cpu_values(&scene.map, &rig.cam);
    assert!(cpu.contains(&9), "the tower is not in view");
    let m = mismatch(&rig.image(), &cpu);
    assert!(m < 0.01, "after the edit {:.2}% of pixels disagree with the CPU ray cast", m * 100.0);

    // Recolour slot 1 to blue: the next frame shows it, without a restart.
    let spp = rig.preview.samples();
    assert!(spp > 0.0);
    scene.set_color(1, [0, 0, 255, 255]);
    rig.preview.set_palette(&rig.g.queue, &scene.palette);
    rig.frames(1, None);
    assert!(rig.preview.samples() > spp, "the recolour restarted the preview");
    let img = rig.image();
    let blue = cpu
        .iter()
        .zip(img.as_chunks::<4>().0)
        .filter(|(v, _)| **v == 1)
        .filter(|(_, p)| p[2] > p[0] && p[2] > p[1])
        .count();
    let ones = cpu.iter().filter(|&&v| v == 1).count();
    assert!(blue * 100 >= ones * 99, "{blue} of {ones} slot-1 pixels are blue after the recolour");
}

#[test]
fn light_changes_restart() {
    let Some(g) = gpu() else { return };
    let scene = red_scene();
    let mut rig = Rig::new(g, &scene);
    rig.frames(20, None);
    let spp = rig.preview.samples();
    rig.preview.lighting.sun_dir = glam::Vec3::new(-0.6, 0.5, 0.2);
    rig.frames(1, None);
    assert!(rig.preview.samples() < spp, "a new sun direction kept {} old samples", rig.preview.samples());
    assert_eq!(rig.preview.restarts, 2);
}

#[test]
fn sampling_stops_when_converged_and_resumes_on_an_edit() {
    let Some(g) = gpu() else { return };
    let mut scene = red_scene();
    let mut rig = Rig::new(g, &scene);
    let max = vxr_render::preview::MAX_SPP as f32;
    let mut frames = 0;
    while rig.preview.samples() < max && frames < 20_000 {
        rig.frames(1, None);
        frames += 1;
    }
    rig.frames(1, None);
    assert_eq!(rig.preview.last_rows, 0, "still tracing at {} spp", rig.preview.samples());
    scene.edit([([10, 12, 10], 9u8)]);
    let b = scene.map.take_dirty_box().unwrap();
    rig.map.upload_dirty(&rig.g.device, &rig.g.queue, &mut scene.map, &scene.palette).unwrap();
    rig.frames(1, Some(EditBox::voxels(b.0, b.1)));
    assert!(rig.preview.last_rows > 0, "an edit did not resume sampling");
}
