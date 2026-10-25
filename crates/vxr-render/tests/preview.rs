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

fn mean_luma(img: &[u8]) -> f64 {
    img.as_chunks::<4>()
        .0
        .iter()
        .map(|p| p[0] as f64 * 0.2126 + p[1] as f64 * 0.7152 + p[2] as f64 * 0.0722)
        .sum::<f64>()
        / (img.len() / 4) as f64
}

#[test]
fn exposure_re_resolves_without_a_restart_and_stops_at_max_spp() {
    let Some(g) = gpu() else { return };
    let scene = red_scene();
    let mut rig = Rig::new(g, &scene);
    rig.preview.max_spp = 8;
    rig.frames(40, None);
    assert!(rig.preview.converged(), "not converged at {} spp", rig.preview.samples());
    rig.frames(1, None);
    assert_eq!(rig.preview.last_rows, 0, "tracing past max_spp");
    // Hits only: the editor backdrop does not follow exposure.
    let cpu = cpu_values(&scene.map, &rig.cam);
    let hits = |img: Vec<u8>| -> Vec<u8> {
        img.as_chunks::<4>().0.iter().zip(&cpu).filter(|(_, v)| **v != 0).flat_map(|(p, _)| *p).collect()
    };
    let a = mean_luma(&hits(rig.image()));
    let restarts = rig.preview.restarts;
    rig.preview.lighting.exposure = 0.25;
    rig.frames(1, None);
    let b = mean_luma(&hits(rig.image()));
    assert_eq!(rig.preview.restarts, restarts, "an exposure change restarted the preview");
    assert!(b < a * 0.8, "two stops down did not darken the image: {a:.1} -> {b:.1}");
}

#[test]
fn sky_backdrop_and_depth_of_field() {
    let Some(g) = gpu() else { return };
    let scene = red_scene();
    let mut rig = Rig::new(g, &scene);
    rig.frames(30, None);
    let cpu = cpu_values(&scene.map, &rig.cam);
    let sharp = rig.image();
    // The sky backdrop: miss pixels show the sky (and its ground below the horizon) instead
    // of the dark editor backdrop.
    rig.preview.lighting.sky_backdrop = true;
    rig.frames(30, None);
    let img = rig.image();
    let px = img.as_chunks::<4>().0;
    let sky = cpu.iter().zip(px).filter(|(v, p)| **v == 0 && p[0].max(p[1]).max(p[2]) > 90).count();
    let misses = cpu.iter().filter(|&&v| v == 0).count();
    assert!(sky * 100 >= misses * 95, "{sky} of {misses} miss pixels show the sky");
    rig.preview.lighting.sky_backdrop = false;
    // Depth of field focused far behind the scene: edges blur, so neighbouring pixels differ
    // less than in the pinhole image (sum of squared steps), and recolours restart.
    rig.preview.lighting.aperture = 4.0;
    rig.preview.lighting.focus = rig.cam.distance * 4.0;
    rig.frames(60, None);
    let blurred = rig.image();
    let edges = |img: &[u8]| -> f64 {
        let w = vxr_render::preview::PREVIEW_W as usize;
        let p = img.as_chunks::<4>().0;
        // Squared steps: a blur spreads an edge over several smaller steps.
        (1..p.len()).filter(|i| i % w != 0).map(|i| (p[i][0].abs_diff(p[i - 1][0]) as f64).powi(2)).sum::<f64>()
    };
    let (e0, e1) = (edges(&sharp), edges(&blurred));
    assert!(e1 < e0 * 0.8, "depth of field did not soften edges: {e0:.0} -> {e1:.0}");
    let restarts = rig.preview.restarts;
    rig.preview.set_palette(&rig.g.queue, &scene.palette);
    rig.frames(1, None);
    assert_eq!(rig.preview.restarts, restarts + 1, "a recolour with depth of field kept stale colours");
}

#[test]
fn an_export_sized_preview_matches_geometry() {
    let Some(g) = gpu() else { return };
    let scene = red_scene();
    let layout = brickmap_layout(&g.device);
    let map = GpuMap::new(&g.device, &g.queue, &layout, &scene.map, &scene.palette).unwrap();
    let (w, h) = (1280, 720);
    assert!(Preview::fits(&g.device, w, h));
    let mut p = Preview::sized(&g.device, &g.queue, &layout, &scene.palette, g.timestamps, (w, h));
    p.max_spp = 4;
    let cam = camera_for(&scene.map);
    for _ in 0..400 {
        let mut enc = g.device.create_command_encoder(&Default::default());
        p.encode_budget(&g.queue, &mut enc, &map, &cam, None, 12.0);
        g.queue.submit([enc.finish()]);
        p.after_submit(&g.device);
        if p.converged() {
            break;
        }
    }
    assert!(p.converged(), "{} spp after 400 frames", p.samples());
    let img = pollster::block_on(p.read(&g.device, &g.queue));
    assert_eq!(img.len(), (w * h * 4) as usize);
    let mut cpu = Vec::with_capacity((w * h) as usize);
    for y in 0..h {
        for x in 0..w {
            let (o, d) = cam.ray(x as f32 + 0.5, y as f32 + 0.5, w as f32, h as f32);
            cpu.push(scene.map.raycast(o.to_array(), d.to_array(), 1e6).map_or(0, |h| h.value));
        }
    }
    let m = mismatch(&img, &cpu);
    assert!(m < 0.01, "{:.2}% of export pixels disagree with the CPU ray cast", m * 100.0);
}

/// Mean colour of the pixels the CPU ray cast says show slot `v`.
fn mean_of(img: &[u8], cpu: &[u8], v: u8) -> [f64; 3] {
    let px: Vec<&[u8; 4]> = img.as_chunks::<4>().0.iter().zip(cpu).filter(|(_, c)| **c == v).map(|(p, _)| p).collect();
    assert!(!px.is_empty(), "slot {v} is not in view");
    [0, 1, 2].map(|k| px.iter().map(|p| p[k] as f64).sum::<f64>() / px.len() as f64)
}

#[test]
fn materials_emit_reflect_and_let_light_through() {
    use vxr_core::material::{Kind, Material};
    let Some(g) = gpu() else { return };
    // The red steps, with a slab of slot 2 (pale blue) on top of part of them.
    let mut scene = red_scene();
    let mut cells = Vec::new();
    for x in 10..30 {
        for z in 10..30 {
            cells.push(([x, 12, z], 2u8));
            cells.push(([x, 13, z], 2u8));
        }
    }
    scene.edit(cells);
    scene.set_color(2, [150, 180, 255, 255]);
    let mut rig = Rig::new(g, &scene);
    let cpu = cpu_values(&scene.map, &rig.cam);
    rig.frames(60, None);
    let diffuse = (mean_of(&rig.image(), &cpu, 1), mean_of(&rig.image(), &cpu, 2));
    assert!(diffuse.1[2] > diffuse.1[0], "the diffuse slab shows its own pale blue: {:?}", diffuse.1);

    // Clear glass: the slab shows the red steps under it, and a recolour now restarts.
    let mut mats = [Material::default(); 256];
    mats[2] = Material { kind: Kind::Glass, rough: 0.0, ior: 1.3, trans: 1.0, ..Material::default() };
    let restarts = rig.preview.restarts;
    rig.preview.set_materials(&rig.g.queue, &mats);
    rig.frames(60, None);
    assert_eq!(rig.preview.restarts, restarts + 1, "a material change restarts");
    let glass = mean_of(&rig.image(), &cpu, 2);
    assert!(glass[0] > glass[2] * 1.3, "clear glass shows the red under it: {glass:?}");
    let restarts = rig.preview.restarts;
    rig.preview.set_palette(&rig.g.queue, &scene.palette);
    rig.frames(1, None);
    assert_eq!(rig.preview.restarts, restarts + 1, "with glass in the scene a recolour restarts");

    // A mirror: slot 2 as smooth white metal reflects the blue sky, not its own colour.
    scene.set_color(2, [255, 255, 255, 255]);
    rig.preview.set_palette(&rig.g.queue, &scene.palette);
    mats[2] = Material { kind: Kind::Metal, rough: 0.0, metal: 1.0, ..Material::default() };
    rig.preview.set_materials(&rig.g.queue, &mats);
    rig.frames(60, None);
    let metal = mean_of(&rig.image(), &cpu, 2);
    assert!(metal[2] > metal[0] * 1.1, "the mirror reflects the sky: {metal:?}");

    // Emissive: slot 1 glows far brighter than when lit by the sun alone.
    mats[2] = Material::default();
    mats[1] = Material { kind: Kind::Emit, emit: 1.0, flux: 3.0, ..Material::default() };
    rig.preview.set_materials(&rig.g.queue, &mats);
    rig.frames(60, None);
    let emit = mean_of(&rig.image(), &cpu, 1);
    eprintln!("mean colours: diffuse {diffuse:?}, glass {glass:?}, metal {metal:?}, emissive {emit:?}");
    assert!(emit[0] > diffuse.0[0] + 20.0, "emissive red is brighter: {emit:?} vs {:?}", diffuse.0);
}
