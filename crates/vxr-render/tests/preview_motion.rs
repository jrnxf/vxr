//! The path-traced preview under its GPU time budget, which the other preview tests replace with
//! a fixed sample count (a process-wide switch, so this file is its own test binary): when the
//! camera moves and the budget does not reach every row, the first frame still shows the whole
//! new view, never the rows the old view left. Skips (passes with a note) when the machine has
//! no GPU adapter, unless `VXR_REQUIRE_GPU` is set.

use vxr_core::{BrickMap, Scene};
use vxr_render::preview::{PREVIEW_H, PREVIEW_W};
use vxr_render::{Gpu, GpuMap, OrbitCamera, Preview, brickmap_layout};

fn gpu() -> Option<Gpu> {
    vxr_render::preview::fix_samples_per_frame(None);
    let instance = wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle());
    match pollster::block_on(Gpu::new(instance, None)) {
        Ok(g) => Some(g),
        Err(e) => {
            assert!(
                std::env::var_os("VXR_REQUIRE_GPU").is_none(),
                "VXR_REQUIRE_GPU is set but there is no GPU adapter: {e}"
            );
            eprintln!("skipping GPU test: {e}");
            None
        }
    }
}

/// A slab in slot 1 (red) with a tower in slot 9 (green) on one side, so the view changes a
/// lot as the camera goes round it.
fn scene() -> Scene {
    let mut s = Scene::new([48, 32, 48]);
    let mut cells = Vec::new();
    for x in 4..44 {
        for z in 4..44 {
            for y in 0..3 {
                cells.push(([x, y, z], 1u8));
            }
        }
    }
    for x in 6..16 {
        for z in 6..16 {
            for y in 3..24 {
                cells.push(([x, y, z], 9u8));
            }
        }
    }
    s.edit(cells);
    s.set_color(1, [255, 0, 0, 255]);
    s.set_color(9, [0, 255, 0, 255]);
    s
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

/// Fraction of pixels whose colour class disagrees with the CPU: red where slot 1 is hit,
/// green where slot 9 is, the dark editor background on a miss.
fn mismatch(img: &[u8], cpu: &[u8]) -> f64 {
    let bad = img
        .as_chunks::<4>()
        .0
        .iter()
        .zip(cpu)
        .filter(|(p, v)| {
            let [r, g, b, _] = **p;
            match v {
                0 => !(r < 60 && r.abs_diff(g) <= 6 && r.abs_diff(b) <= 6),
                1 => !(r > g && r > b),
                9 => !(g > r && g > b),
                _ => true,
            }
        })
        .count();
    bad as f64 / cpu.len() as f64
}

#[test]
fn a_camera_move_shows_the_whole_new_view_on_its_first_frame() {
    let Some(g) = gpu() else { return };
    let scene = scene();
    let layout = brickmap_layout(&g.device);
    let map = GpuMap::new(&g.device, &g.queue, &layout, &scene.map, &scene.palette).unwrap();
    let mut preview = Preview::new(&g.device, &g.queue, &layout, &scene.palette, g.timestamps);
    let s = scene.map.size_voxels();
    let mut cam = OrbitCamera::framing_aspect(
        glam::Vec3::ZERO,
        glam::Vec3::new(s[0] as f32, s[1] as f32, s[2] as f32),
        PREVIEW_W as f32 / PREVIEW_H as f32,
    );
    // A budget far below one full sample on any GPU: the rows traced each frame cover only a
    // band of the image (at least 8 rows).
    let frame = |preview: &mut Preview, cam: &OrbitCamera| {
        let mut enc = g.device.create_command_encoder(&Default::default());
        preview.encode_budget(&g.queue, &mut enc, &map, cam, None, 0.01);
        g.queue.submit([enc.finish()]);
        preview.after_submit(&g.device);
        let _ = g.device.poll(wgpu::PollType::wait_indefinitely());
    };
    for _ in 0..8 {
        frame(&mut preview, &cam);
    }
    assert!(preview.samples() < 1.0, "the budget covered every row: {} samples", preview.samples());
    for turn in 1..=3 {
        cam.yaw += 0.8;
        frame(&mut preview, &cam);
        assert_eq!(preview.current(), 1.0, "turn {turn}: part of the image still shows an older view");
        let img = pollster::block_on(preview.read(&g.device, &g.queue));
        let m = mismatch(&img, &cpu_values(&scene.map, &cam));
        assert!(m < 0.02, "turn {turn}: {:.2}% of pixels disagree with the new view", m * 100.0);
    }
    // Once the camera stops, the rows sharpen it and sampling goes on as before.
    for _ in 0..200 {
        frame(&mut preview, &cam);
    }
    assert!(preview.samples() >= 1.0, "only {} samples 200 frames after the camera stopped", preview.samples());
    let img = pollster::block_on(preview.read(&g.device, &g.queue));
    let m = mismatch(&img, &cpu_values(&scene.map, &cam));
    assert!(m < 0.01, "{:.2}% of pixels disagree once sampled", m * 100.0);
}
