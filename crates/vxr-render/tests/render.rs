//! Renders small scenes on the real GPU and checks every pixel against the CPU ray cast.
//! Skips (passes with a note) when the machine has no GPU adapter.

use vxr_core::{BrickMap, Scene};
use vxr_render::{Gpu, GpuMap, OrbitCamera, Renderer, brickmap_layout, gpu::read_texture};

const W: u32 = 160;
const H: u32 = 100;

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

struct View {
    layout: wgpu::BindGroupLayout,
    renderer: Renderer,
}

impl View {
    fn new(g: &Gpu) -> Self {
        let layout = brickmap_layout(&g.device);
        let renderer = Renderer::new(&g.device, &layout, wgpu::TextureFormat::Rgba8Unorm, W, H, true);
        Self { layout, renderer }
    }

    /// March once and return the palette index per pixel (0 = miss).
    fn values(&self, g: &Gpu, map: &GpuMap, cam: &OrbitCamera) -> Vec<u8> {
        self.renderer.set_camera(&g.queue, cam, None);
        let mut enc = g.device.create_command_encoder(&Default::default());
        self.renderer.march(&mut enc, map, None);
        g.queue.submit([enc.finish()]);
        let info = pollster::block_on(read_texture(&g.device, &g.queue, &self.renderer.info));
        info.as_chunks::<4>()
            .0
            .iter()
            .map(|&p| {
                let v = u32::from_le_bytes(p);
                if v & (1 << 11) != 0 { (v & 0xff) as u8 } else { 0 }
            })
            .collect()
    }
}

fn camera_for(map: &BrickMap) -> OrbitCamera {
    let s = map.size_voxels();
    OrbitCamera::framing_aspect(
        glam::Vec3::ZERO,
        glam::Vec3::new(s[0] as f32, s[1] as f32, s[2] as f32),
        W as f32 / H as f32,
    )
}

/// The CPU ray cast's palette index per pixel (0 = miss).
fn cpu_values(map: &BrickMap, cam: &OrbitCamera) -> Vec<u8> {
    let mut out = Vec::with_capacity((W * H) as usize);
    for y in 0..H {
        for x in 0..W {
            let (o, d) = cam.ray(x as f32 + 0.5, y as f32 + 0.5, W as f32, H as f32);
            out.push(map.raycast(o.to_array(), d.to_array(), 1e6).map_or(0, |h| h.value));
        }
    }
    out
}

/// GPU and CPU agree on all but a few pixels on voxel edges.
fn assert_matches(gpu: &[u8], cpu: &[u8], what: &str) {
    let differ = gpu.iter().zip(cpu).filter(|(a, b)| a != b).count();
    let hits = cpu.iter().filter(|&&v| v != 0).count();
    assert!(hits > 0, "{what}: the CPU sees nothing, so the test proves nothing");
    assert!(differ * 1000 <= gpu.len(), "{what}: {differ} of {} pixels differ from the CPU ray cast", gpu.len());
}

/// A terrain-like test scene: a ground slab with steps and a few colours.
fn test_scene() -> Scene {
    let mut s = Scene::new([48, 32, 48]);
    let mut cells = Vec::new();
    for x in 4..44 {
        for z in 4..44 {
            let top = 4 + (x / 6 + z / 9) % 5;
            for y in 0..top {
                cells.push(([x, y, z], 1 + ((x / 8 + z / 8) % 6) as u8));
            }
        }
    }
    s.edit(cells);
    s
}

#[test]
fn march_matches_cpu_raycast() {
    let Some(g) = gpu() else { return };
    let v = View::new(&g);
    let scene = test_scene();
    let map = GpuMap::new(&g.device, &g.queue, &v.layout, &scene.map, &scene.palette).unwrap();
    let mut cam = camera_for(&scene.map);
    for (yaw, pitch) in [(0.6, 0.6), (2.5, 0.3), (-1.2, 1.2), (4.0, -0.2)] {
        cam.yaw = yaw;
        cam.pitch = pitch;
        assert_matches(&v.values(&g, &map, &cam), &cpu_values(&scene.map, &cam), &format!("yaw {yaw} pitch {pitch}"));
    }
}

#[test]
fn orthographic_and_snap_views_match_cpu_raycast() {
    let Some(g) = gpu() else { return };
    let v = View::new(&g);
    let scene = test_scene();
    let map = GpuMap::new(&g.device, &g.queue, &v.layout, &scene.map, &scene.palette).unwrap();
    for ortho in [false, true] {
        for view in vxr_render::View::ALL {
            let mut cam = camera_for(&scene.map);
            cam.ortho = ortho;
            cam.snap(view);
            let gpu = v.values(&g, &map, &cam);
            let label = format!("{view:?} ortho {}", cam.ortho);
            assert_matches(&gpu, &cpu_values(&scene.map, &cam), &label);
            assert!(gpu.iter().any(|&c| c != 0), "{label}: nothing in view");
        }
    }
}

#[test]
fn partial_uploads_track_edits_undo_and_redo() {
    let Some(g) = gpu() else { return };
    let v = View::new(&g);
    let mut scene = test_scene();
    scene.map.track_dirty(true);
    let mut map = GpuMap::new(&g.device, &g.queue, &v.layout, &scene.map, &scene.palette).unwrap();
    let cam = camera_for(&scene.map);
    // A tower in a new colour, a hole through the ground, then a repaint.
    scene.edit((0..20).flat_map(|y| (20..26).map(move |x| ([x, 8 + y, 22], 9))));
    scene.edit((10..30).flat_map(|x| (26..34).flat_map(move |z| (0..12).map(move |y| ([x, y, z], 0)))));
    scene.edit((4..44).map(|x| ([x, 3, 4], 12)));
    let mut last = v.values(&g, &map, &cam);
    for step in ["edits", "undo", "undo again", "redo"] {
        match step {
            "undo" | "undo again" => assert!(scene.undo()),
            "redo" => assert!(scene.redo()),
            _ => {}
        }
        let st = map.upload_dirty(&g.device, &g.queue, &mut scene.map, &scene.palette).unwrap();
        assert!(st.bricks > 0 || st.rebuilt, "{step}: nothing uploaded");
        let now = v.values(&g, &map, &cam);
        assert_matches(&now, &cpu_values(&scene.map, &cam), step);
        let changed = now.iter().zip(&last).filter(|(a, b)| a != b).count();
        assert!(changed > 0, "{step}: the image did not change");
        last = now;
    }
}

#[test]
fn edits_into_an_empty_map_grow_the_pool() {
    let Some(g) = gpu() else { return };
    let v = View::new(&g);
    let mut scene = Scene::new([96, 96, 96]);
    scene.map.track_dirty(true);
    let mut map = GpuMap::new(&g.device, &g.queue, &v.layout, &scene.map, &scene.palette).unwrap();
    let cam = camera_for(&scene.map);
    assert!(v.values(&g, &map, &cam).iter().all(|&p| p == 0), "an empty map draws only background");
    // Over 512 new bricks: more than the empty map's pool headroom, so it must rebuild.
    let mut rebuilt = false;
    for layer in 0..4 {
        let y0 = layer * 24;
        scene.edit(
            (0..96).flat_map(|x| (0..96).flat_map(move |z| (y0..y0 + 2).map(move |y| ([x, y, z], 2 + layer as u8)))),
        );
        rebuilt |= map.upload_dirty(&g.device, &g.queue, &mut scene.map, &scene.palette).unwrap().rebuilt;
        assert_matches(&v.values(&g, &map, &cam), &cpu_values(&scene.map, &cam), &format!("layer {layer}"));
    }
    assert!(rebuilt, "the pool never had to grow");
}

#[test]
fn background_and_recolor() {
    let Some(g) = gpu() else { return };
    let v = View::new(&g);
    let mut scene = test_scene();
    let map = GpuMap::new(&g.device, &g.queue, &v.layout, &scene.map, &scene.palette).unwrap();
    let cam = camera_for(&scene.map);
    let shot = |map: &GpuMap| {
        v.renderer.set_camera(&g.queue, &cam, None);
        let mut enc = g.device.create_command_encoder(&Default::default());
        v.renderer.march(&mut enc, map, None);
        g.queue.submit([enc.finish()]);
        pollster::block_on(v.renderer.read_color(&g.device, &g.queue))
    };
    let before = shot(&map);
    // Top left corner is background: in the dark neutral #0a0a0a to #1c1c1c range, with no hue.
    let px = &before[..4];
    assert!(
        px[0] < 40 && px[0] == px[1] && px[1] == px[2],
        "background {px:?} is not the dark neutral editor gradient"
    );
    // Recolor index 1 to pure red: pixels showing index 1 change, nothing else does.
    let values = v.values(&g, &map, &cam);
    scene.set_color(1, [255, 0, 0, 255]);
    map.set_palette(&g.queue, &scene.palette);
    let after = shot(&map);
    let mut changed = 0;
    for (i, &val) in values.iter().enumerate() {
        let (a, b) = (&before[i * 4..i * 4 + 4], &after[i * 4..i * 4 + 4]);
        if val == 1 {
            changed += (a != b) as usize;
            assert!(b[0] > b[1] && b[0] > b[2], "pixel {i} of index 1 is not red after the recolor");
        } else {
            assert_eq!(a, b, "pixel {i} (index {val}) changed");
        }
    }
    assert!(changed > 0);
}

#[test]
fn island_matches_cpu_raycast() {
    let path = concat!(env!("CARGO_MANIFEST_DIR"), "/../../prototype/demo/scene.vox");
    let Ok(bytes) = std::fs::read(path) else {
        eprintln!("skipping: run `node prototype/src/build.mjs` to create the island");
        return;
    };
    let Some(g) = gpu() else { return };
    let v = View::new(&g);
    let scene = Scene::from_vox(&bytes).unwrap();
    let map = GpuMap::new(&g.device, &g.queue, &v.layout, &scene.map, &scene.palette).unwrap();
    let (lo, hi) = scene.map.occupied_bounds().unwrap();
    let f = |p: [u32; 3]| glam::Vec3::new(p[0] as f32, p[1] as f32, p[2] as f32);
    let cam = OrbitCamera::framing_aspect(f(lo), f(hi), W as f32 / H as f32);
    assert_matches(&v.values(&g, &map, &cam), &cpu_values(&scene.map, &cam), "island");
}

/// The selection mask tints the selected voxels (lighter, edged in near black, no hue) and leaves
/// the rest as they were.
#[test]
fn selection_tints_only_selected_voxels() {
    let Some(g) = gpu() else { return };
    let v = View::new(&g);
    let mut renderer = v.renderer;
    // Two grey 4^3 blocks side by side; select the left one.
    let mut s = Scene::new([32, 16, 32]);
    let block =
        |x0: i32| (x0..x0 + 4).flat_map(move |x| (0..4).flat_map(move |y| (12..16).map(move |z| ([x, y, z], 8u8))));
    s.edit(block(8).chain(block(18)));
    s.select(block(8).map(|c| c.0), vxr_core::select::Mode::Replace);
    let mut palette = [[128, 128, 128, 255]; 256];
    palette[0] = [0; 4];
    let map = GpuMap::new(&g.device, &g.queue, &v.layout, &s.map, &palette).unwrap();
    let cam = camera_for(&s.map);
    let draw = |r: &Renderer| {
        r.set_camera(&g.queue, &cam, None);
        let mut enc = g.device.create_command_encoder(&Default::default());
        r.march(&mut enc, &map, None);
        g.queue.submit([enc.finish()]);
        pollster::block_on(read_texture(&g.device, &g.queue, &r.color))
    };
    let plain = draw(&renderer);
    renderer.set_selection(&g.device, &g.queue, &s.selection_mask());
    let lit = draw(&renderer);
    // Which block each pixel shows, from the CPU ray cast.
    let mut tinted = [0usize; 2];
    let mut hue = 0usize;
    let mut changed = [0usize; 2];
    let mut seen = [0usize; 2];
    for y in 0..H {
        for x in 0..W {
            let (o, d) = cam.ray(x as f32 + 0.5, y as f32 + 0.5, W as f32, H as f32);
            let Some(h) = s.map.raycast(o.to_array(), d.to_array(), 1e6) else { continue };
            let k = usize::from(h.voxel[0] >= 18);
            let i = ((y * W + x) * 4) as usize;
            seen[k] += 1;
            changed[k] += usize::from(plain[i..i + 3] != lit[i..i + 3]);
            tinted[k] += usize::from(lit[i..i + 3] != plain[i..i + 3]);
            let (lo, hi) = (lit[i..i + 3].iter().min().unwrap(), lit[i..i + 3].iter().max().unwrap());
            hue += usize::from(k == 0 && hi - lo > 4);
        }
    }
    assert!(seen[0] > 50 && seen[1] > 50, "both blocks on screen: {seen:?}");
    assert!(tinted[0] * 10 >= seen[0] * 9, "the selected block is tinted: {tinted:?} of {seen:?}");
    assert_eq!(hue, 0, "the grey selected block stays grey: the tint has no hue");
    assert!(changed[1] * 100 <= seen[1], "the other block is unchanged: {changed:?} of {seen:?}");
    renderer.set_selection(&g.device, &g.queue, &[]);
    assert_eq!(draw(&renderer), plain, "an empty mask clears the highlight");
}

/// The staged-change ghost: added cells tinted green, removed ones red, repainted ones keep
/// their colour inside the face with an accent edge, unmarked cells unchanged; an empty mask
/// clears it.
#[test]
fn ghost_tints_staged_cells_by_kind() {
    let Some(g) = gpu() else { return };
    let v = View::new(&g);
    let mut renderer = v.renderer;
    // Four grey 4^3 blocks side by side: added, repainted, removed and untouched.
    let mut s = Scene::new([40, 16, 32]);
    let block =
        |x0: i32| (x0..x0 + 4).flat_map(move |x| (0..4).flat_map(move |y| (12..16).map(move |z| ([x, y, z], 8u8))));
    s.edit(block(4).chain(block(13)).chain(block(22)).chain(block(31)));
    let kinds = [1u32, 2, 3];
    let mask =
        s.ghost_mask(kinds.iter().enumerate().flat_map(|(k, &kind)| block(4 + 9 * k as i32).map(move |c| (c.0, kind))));
    let mut palette = [[128, 128, 128, 255]; 256];
    palette[0] = [0; 4];
    let map = GpuMap::new(&g.device, &g.queue, &v.layout, &s.map, &palette).unwrap();
    let cam = camera_for(&s.map);
    let draw = |r: &Renderer| {
        r.set_camera(&g.queue, &cam, None);
        let mut enc = g.device.create_command_encoder(&Default::default());
        r.march(&mut enc, &map, None);
        g.queue.submit([enc.finish()]);
        pollster::block_on(read_texture(&g.device, &g.queue, &r.color))
    };
    let plain = draw(&renderer);
    renderer.set_ghost(&g.device, &g.queue, &mask);
    let lit = draw(&renderer);
    let (mut seen, mut green, mut red, mut changed) = ([0usize; 4], [0usize; 4], [0usize; 4], [0usize; 4]);
    for y in 0..H {
        for x in 0..W {
            let (o, d) = cam.ray(x as f32 + 0.5, y as f32 + 0.5, W as f32, H as f32);
            let Some(h) = s.map.raycast(o.to_array(), d.to_array(), 1e6) else { continue };
            let k = ((h.voxel[0] - 4) / 9) as usize;
            let i = ((y * W + x) * 4) as usize;
            let [r, gr, b] = [lit[i] as i32, lit[i + 1] as i32, lit[i + 2] as i32];
            seen[k] += 1;
            green[k] += usize::from(gr > r + 30 && gr > b);
            red[k] += usize::from(r > gr + 30 && r > b + 30);
            changed[k] += usize::from(plain[i..i + 3] != lit[i..i + 3]);
        }
    }
    assert!(seen.iter().all(|&n| n > 50), "every block on screen: {seen:?}");
    assert!(green[0] * 10 >= seen[0] * 9, "added cells are green: {green:?} of {seen:?}");
    assert!(red[2] * 10 >= seen[2] * 9, "removed cells are red: {red:?} of {seen:?}");
    assert!(
        changed[1] * 10 >= seen[1] && changed[1] * 2 <= seen[1],
        "repainted cells change only along their edges: {changed:?} of {seen:?}"
    );
    assert!(changed[3] * 100 <= seen[3], "the untouched block is unchanged: {changed:?} of {seen:?}");
    renderer.set_ghost(&g.device, &g.queue, &[]);
    assert_eq!(draw(&renderer), plain, "an empty mask clears the ghost");
}

/// The guides: the ground lights the bounds footprint, the grid draws a line on each voxel
/// edge of it, and the frame draws the bounds box's edges; with every guide off the view
/// is the plain background.
#[test]
fn guides_draw_ground_grid_and_frame() {
    let Some(g) = gpu() else { return };
    let v = View::new(&g);
    let mut renderer = v.renderer;
    let s = Scene::new([32, 16, 32]);
    let map = GpuMap::new(&g.device, &g.queue, &v.layout, &s.map, &s.palette).unwrap();
    let draw = |r: &Renderer, cam: &OrbitCamera| {
        r.set_camera(&g.queue, cam, None);
        let mut enc = g.device.create_command_encoder(&Default::default());
        r.march(&mut enc, &map, None);
        g.queue.submit([enc.finish()]);
        pollster::block_on(read_texture(&g.device, &g.queue, &r.color))
    };
    let at = |img: &[u8], cam: &OrbitCamera, p: [f32; 3]| {
        let c = cam.view_proj(W as f32 / H as f32).project_point3(glam::Vec3::from(p));
        let (x, y) = (((c.x * 0.5 + 0.5) * W as f32) as u32, ((0.5 - c.y * 0.5) * H as f32) as u32);
        let i = ((y * W + x) * 4) as usize;
        [img[i] as i32, img[i + 1] as i32, img[i + 2] as i32]
    };
    let cam = camera_for(&s.map);
    let plain = draw(&renderer, &cam);
    let guides = vxr_render::Guides { bounds: [32, 16, 32], floor: 0, line: 1.0, ..Default::default() };

    // Ground: the footprint is lighter than the background there; the sky above is not.
    renderer.guides = vxr_render::Guides { ground: true, ..guides };
    let ground = draw(&renderer, &cam);
    let (bg, lit) = (at(&plain, &cam, [16.0, 0.0, 16.0]), at(&ground, &cam, [16.0, 0.0, 16.0]));
    assert!(lit.iter().zip(bg).all(|(l, b)| *l > b + 3), "the footprint {lit:?} is lighter than {bg:?}");
    assert_eq!(&ground[..4], &plain[..4], "the top left corner stays background");

    // Grid, from straight above in orthographic at 10 pixels per voxel: lines on the voxel
    // edges, none in the middle of a cell.
    let mut top = cam;
    top.snap(vxr_render::View::Top);
    top.ortho = true;
    top.target = glam::Vec3::new(16.0, 0.0, 16.0);
    top.distance = 5.0 / (top.fov_y * 0.5).tan();
    renderer.guides = vxr_render::Guides { ground: true, grid: true, ..guides };
    let grid = draw(&renderer, &top);
    let (line, cell) = (at(&grid, &top, [16.02, 0.0, 16.5]), at(&grid, &top, [16.5, 0.0, 16.5]));
    assert!(line[2] > cell[2] + 10, "a grid line {line:?} is brighter than the cell middle {cell:?}");

    // Frame: an edge of the box shows as a light neutral line; the middle of the view does not.
    renderer.guides = vxr_render::Guides { frame: true, ..guides };
    let frame = draw(&renderer, &cam);
    let (on, off) = (at(&frame, &cam, [16.0, 16.0, 32.0]), at(&plain, &cam, [16.0, 16.0, 32.0]));
    assert!(
        on.iter().zip(off).all(|(o, b)| *o > b + 30) && on[0] == on[2],
        "the top front edge {on:?} is drawn over {off:?}"
    );
    assert_eq!(at(&frame, &cam, [16.0, 8.0, 16.0]), at(&plain, &cam, [16.0, 8.0, 16.0]), "inside the box");

    renderer.guides = vxr_render::Guides::default();
    assert_eq!(draw(&renderer, &cam), plain, "with every guide off the view is the background");
}
