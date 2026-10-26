//! Phase AI test 6: a 100k-cell staged change drawn as a ghost on the island at 3024x1964.
//! Measures the frame (CPU + GPU, waiting for the GPU, as the app's `--shot` frame time does)
//! without and with the ghost, the march pass alone by GPU timestamps, and Apply, Discard and
//! the undo of an Apply, each through to the next frame on screen.
//!
//! cargo run --release -- <scene.vox> [shots dir]

mod ghost;
mod staged;

use std::time::Instant;

use glam::Vec3;
use staged::{Staged, plan};
use vxr_core::Scene;
use vxr_render::{Gpu, GpuMap, Guides, OrbitCamera, View, brickmap_layout};

const W: u32 = 3024;
const H: u32 = 1964;
/// The app's surface format on macOS.
const FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Bgra8UnormSrgb;
const RUNS: usize = 5;
const FRAMES: usize = 120;
const REPEATS: usize = 21;

struct Ctx {
    gpu: Gpu,
    layout: wgpu::BindGroupLayout,
    target: wgpu::TextureView,
}

impl Ctx {
    fn wait(&self) {
        let _ = self.gpu.device.poll(wgpu::PollType::wait_indefinitely());
    }
}

/// Which renderer draws: the real one from `vxr_render`, or the spike's copy with the ghost.
enum Draw<'a> {
    Real(&'a vxr_render::Renderer),
    Ghost(&'a ghost::Renderer),
}

impl Draw<'_> {
    /// One editor frame: camera uniform, march, blit onto the window-sized target.
    fn frame(&self, cx: &Ctx, map: &GpuMap, cam: &OrbitCamera, ts: Option<&Stamps>) {
        let q = &cx.gpu.queue;
        let mut enc = cx.gpu.device.create_command_encoder(&Default::default());
        let tw = ts.map(|t| wgpu::ComputePassTimestampWrites {
            query_set: &t.set,
            beginning_of_pass_write_index: Some(0),
            end_of_pass_write_index: Some(1),
        });
        match self {
            Draw::Real(r) => {
                r.set_camera(q, cam, None);
                r.march(&mut enc, map, tw);
                r.blit(&mut enc, &cx.target, None);
            }
            Draw::Ghost(r) => {
                r.set_camera(q, cam, None);
                r.march(&mut enc, map, tw);
                r.blit(&mut enc, &cx.target, None);
            }
        }
        if let Some(t) = ts {
            enc.resolve_query_set(&t.set, 0..2, &t.resolve, 0);
            enc.copy_buffer_to_buffer(&t.resolve, 0, &t.read, 0, 16);
        }
        q.submit([enc.finish()]);
    }
}

struct Stamps {
    set: wgpu::QuerySet,
    resolve: wgpu::Buffer,
    read: wgpu::Buffer,
    period_ns: f64,
}

impl Stamps {
    fn new(gpu: &Gpu) -> Option<Self> {
        if !gpu.timestamps {
            return None;
        }
        let d = &gpu.device;
        let set = d.create_query_set(&wgpu::QuerySetDescriptor {
            label: Some("march"),
            ty: wgpu::QueryType::Timestamp,
            count: 2,
        });
        let buf = |label, usage| {
            d.create_buffer(&wgpu::BufferDescriptor { label: Some(label), size: 16, usage, mapped_at_creation: false })
        };
        let resolve = buf("resolve", wgpu::BufferUsages::QUERY_RESOLVE | wgpu::BufferUsages::COPY_SRC);
        let read = buf("read", wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST);
        Some(Self { set, resolve, read, period_ns: gpu.queue.get_timestamp_period() as f64 })
    }

    fn ms(&self, gpu: &Gpu) -> f64 {
        let b = pollster::block_on(vxr_render::gpu::map_read(&gpu.device, &self.read));
        let t: &[u64] = bytemuck::cast_slice(&b);
        t[1].saturating_sub(t[0]) as f64 * self.period_ns / 1e6
    }
}

#[derive(Clone, Copy)]
struct Stats {
    median: f64,
    p95: f64,
    min_run: f64,
    max_run: f64,
}

fn stats(runs: &[Vec<f64>]) -> Stats {
    let pct = |v: &[f64], p: f64| {
        let mut v = v.to_vec();
        v.sort_by(f64::total_cmp);
        v[((v.len() - 1) as f64 * p).round() as usize]
    };
    let all: Vec<f64> = runs.concat();
    let medians: Vec<f64> = runs.iter().map(|r| pct(r, 0.5)).collect();
    Stats {
        median: pct(&all, 0.5),
        p95: pct(&all, 0.95),
        min_run: medians.iter().copied().fold(f64::MAX, f64::min),
        max_run: medians.iter().copied().fold(0.0, f64::max),
    }
}

/// One run of frame times (CPU + GPU, each frame waited for): FRAMES frames after a warm-up.
fn frame_time(cx: &Ctx, d: &Draw, map: &GpuMap, cam: &OrbitCamera) -> Vec<f64> {
    for _ in 0..4 {
        d.frame(cx, map, cam, None);
        cx.wait();
    }
    (0..FRAMES)
        .map(|_| {
            let t0 = Instant::now();
            d.frame(cx, map, cam, None);
            cx.wait();
            ms(t0)
        })
        .collect()
}

/// One run of the march pass alone, by GPU timestamps.
fn march_time(cx: &Ctx, d: &Draw, map: &GpuMap, cam: &OrbitCamera, ts: &Stamps) -> Vec<f64> {
    (0..FRAMES / 2)
        .map(|_| {
            d.frame(cx, map, cam, Some(ts));
            ts.ms(&cx.gpu)
        })
        .collect()
}

fn ms(t0: Instant) -> f64 {
    t0.elapsed().as_secs_f64() * 1e3
}

/// Every voxel and the palette counts, to check that Discard and undo restore the scene.
fn fingerprint(scene: &Scene) -> (u64, [u32; 256], u64) {
    let mut h = 0xcbf29ce484222325u64;
    scene.for_each_voxel(|p, v| {
        for b in p.iter().flat_map(|c| c.to_le_bytes()).chain([v]) {
            h = (h ^ b as u64).wrapping_mul(0x100000001b3);
        }
    });
    (scene.map.voxels, scene.map.counts, h)
}

struct Editor {
    scene: Scene,
    map: GpuMap,
    ghost: ghost::Renderer,
}

impl Editor {
    /// What the editor does at the start of a frame after an edit: upload the dirty bricks.
    fn upload(&mut self, cx: &Ctx) -> vxr_render::map::UploadStats {
        self.map.upload_dirty(&cx.gpu.device, &cx.gpu.queue, &mut self.scene.map, &self.scene.palette).unwrap()
    }

    /// After a scene operation that started at `t0`: upload, clear the ghost, draw the next
    /// frame and wait for it. Returns [total, scene operation, upload on the CPU, the frame
    /// with its wait, a second frame] in ms and whether the upload rebuilt the GPU map.
    fn finish(&mut self, cx: &Ctx, cam: &OrbitCamera, t0: Instant) -> ([f64; 5], bool) {
        let scene = ms(t0);
        let t1 = Instant::now();
        let st = self.upload(cx);
        let upload = ms(t1);
        self.set_ghost(cx, &[]);
        let t2 = Instant::now();
        self.frame(cx, cam);
        let total = ms(t0);
        let frame = ms(t2);
        let t3 = Instant::now();
        self.frame(cx, cam);
        ([total, scene, upload, frame, ms(t3)], st.rebuilt)
    }

    fn set_ghost(&mut self, cx: &Ctx, words: &[u32]) {
        self.ghost.set_ghost(&cx.gpu.device, &cx.gpu.queue, words);
    }

    fn frame(&self, cx: &Ctx, cam: &OrbitCamera) {
        Draw::Ghost(&self.ghost).frame(cx, &self.map, cam, None);
        cx.wait();
    }

    /// Stage `s` and show it: the edit, the ghost mask, the upload and one frame. Returns the
    /// times of (stage, ghost mask build, upload + ghost + frame).
    fn stage(&mut self, cx: &Ctx, cam: &OrbitCamera, s: &Staged) -> [f64; 3] {
        let t0 = Instant::now();
        s.stage(&mut self.scene);
        let a = ms(t0);
        let t1 = Instant::now();
        let words = s.ghost_mask(&self.scene);
        let b = ms(t1);
        let t2 = Instant::now();
        self.upload(cx);
        self.set_ghost(cx, &words);
        self.frame(cx, cam);
        [a, b, ms(t2)]
    }
}

fn guides(scene: &Scene) -> Guides {
    // The editor's defaults: ground, grid and bounds frame on, at 2 pixels per point.
    Guides { bounds: scene.map.size_voxels(), floor: scene.floor, grid: true, ground: true, frame: true, line: 2.0 }
}

fn save_png(cx: &Ctx, r: &ghost::Renderer, path: &std::path::Path) {
    let px = pollster::block_on(r.read_color(&cx.gpu.device, &cx.gpu.queue));
    let f = std::fs::File::create(path).unwrap();
    let mut e = png::Encoder::new(std::io::BufWriter::new(f), r.width, r.height);
    e.set_color(png::ColorType::Rgba);
    e.set_depth(png::BitDepth::Eight);
    e.write_header().unwrap().write_image_data(&px).unwrap();
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let path = args.get(1).expect("usage: ai-staged-ghost <scene.vox> [shots dir]");
    let shots = args.get(2).map(std::path::PathBuf::from);
    if let Some(d) = &shots {
        std::fs::create_dir_all(d).unwrap();
    }

    let instance = wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle());
    let gpu = pollster::block_on(Gpu::new(instance, None)).expect("GPU");
    let info = gpu.adapter.get_info();
    println!("adapter: {} ({:?}), timestamps: {}", info.name, info.backend, gpu.timestamps);
    let layout = brickmap_layout(&gpu.device);
    let target = gpu.device.create_texture(&wgpu::TextureDescriptor {
        label: Some("window"),
        size: wgpu::Extent3d { width: W, height: H, depth_or_array_layers: 1 },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: FORMAT,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
        view_formats: &[],
    });
    let cx = Ctx { target: target.create_view(&Default::default()), gpu, layout };
    let ts = Stamps::new(&cx.gpu);

    let bytes = std::fs::read(path).expect("read scene");
    let mut scene = Scene::from_vox(&bytes).expect("open scene");
    scene.map.track_dirty(true);
    let size = scene.map.size_voxels();
    let (lo, hi) = scene.map.occupied_bounds().unwrap();
    println!(
        "scene: {} voxels, bounds {}x{}x{}, {} brick slots ({} live)",
        scene.map.voxels,
        size[0],
        size[1],
        size[2],
        scene.map.brick_slots(),
        scene.map.live_bricks()
    );
    let original = fingerprint(&scene);

    let d = &cx.gpu.device;
    let map = GpuMap::new(d, &cx.gpu.queue, &cx.layout, &scene.map, &scene.palette).unwrap();
    let mut real = vxr_render::Renderer::new(d, &cx.layout, FORMAT, W, H, false);
    let mut ghost = ghost::Renderer::new(d, &cx.layout, FORMAT, W, H, false);
    real.guides = guides(&scene);
    ghost.guides = guides(&scene);
    let mut ed = Editor { scene, map, ghost };

    // Cameras: the editor's framing (`.`), isometric, and a close view over the staged area.
    let v = |a: [u32; 3]| Vec3::new(a[0] as f32, a[1] as f32, a[2] as f32);
    let aspect = W as f32 / H as f32;
    let framed = OrbitCamera::framing_aspect(v(lo), v(hi), aspect);
    let mut iso = framed;
    iso.snap(View::Iso);
    iso.fit(v(lo), v(hi), aspect, 0.9);

    // The change: 100k cells, 60% added, 20% repainted, 20% removed.
    let mix = plan(&ed.scene, 60_000, 20_000, 20_000);
    println!(
        "staged: {} cells ({} added, {} repainted, {} removed)",
        mix.cells(),
        mix.added.len(),
        mix.repainted.len(),
        mix.removed.len()
    );
    assert_eq!(mix.cells(), 100_000, "the island has room for the whole change");
    let centre = {
        let n = mix.added.len().max(1) as f32;
        mix.added.iter().fold(Vec3::ZERO, |a, (p, _)| a + v(p.map(|c| c as u32))) / n
    };
    let mut close = framed;
    close.target = centre;
    close.distance *= 0.45;
    let views = [("framed", &framed), ("iso", &iso), ("close", &close)];

    // 1. Frame time without and with the ghost. The four states take turns run by run, so
    // drift in GPU clocks lands on all of them alike.
    println!("\nframe time, CPU + GPU per frame, {RUNS} runs x {FRAMES} frames (median, p95, run medians min-max):");
    const STATES: [&str; 4] = [
        "before, real renderer",
        "before, spike renderer (no ghost)",
        "staged, ghost off (after state)",
        "staged, ghost on",
    ];
    let mut table = Vec::new();
    let mut march = Vec::new();
    for (name, cam) in views {
        let mut wall: [Vec<Vec<f64>>; 4] = Default::default();
        let mut gpu: [Vec<Vec<f64>>; 4] = Default::default();
        let words = {
            ed.stage(&cx, cam, &mix);
            let w = mix.ghost_mask(&ed.scene);
            Staged::discard(&mut ed.scene);
            ed.upload(&cx);
            ed.set_ghost(&cx, &[]);
            w
        };
        for _ in 0..RUNS {
            for k in 0..4 {
                if k == 2 {
                    ed.stage(&cx, cam, &mix);
                    ed.set_ghost(&cx, &[]);
                }
                if k == 3 {
                    ed.set_ghost(&cx, &words);
                }
                let d = if k == 0 { Draw::Real(&real) } else { Draw::Ghost(&ed.ghost) };
                wall[k].push(frame_time(&cx, &d, &ed.map, cam));
                if let Some(ts) = &ts {
                    gpu[k].push(march_time(&cx, &d, &ed.map, cam, ts));
                }
            }
            if let Some(dir) = &shots {
                ed.frame(&cx, cam);
                save_png(&cx, &ed.ghost, &dir.join(format!("ghost-{name}.png")));
            }
            Staged::discard(&mut ed.scene);
            ed.upload(&cx);
            ed.set_ghost(&cx, &[]);
            assert_eq!(fingerprint(&ed.scene), original, "discard restores the island");
            if let (Some(dir), "framed") = (&shots, name) {
                ed.frame(&cx, cam);
                save_png(&cx, &ed.ghost, &dir.join("before-framed.png"));
            }
        }
        let wall = wall.map(|r| stats(&r));
        for (k, s) in wall.iter().enumerate() {
            println!(
                "  {name:<7} {:<36} {:>5.2} ms  p95 {:>5.2} ms  runs {:.2}-{:.2}",
                STATES[k], s.median, s.p95, s.min_run, s.max_run
            );
        }
        if ts.is_some() {
            march.push((name, gpu.map(|r| stats(&r))));
        }
        table.push((name, wall));
    }
    if !march.is_empty() {
        println!("\nmarch pass alone, GPU timestamps (median, p95):");
        for (view, g) in &march {
            for (k, s) in g.iter().enumerate() {
                println!("  {view:<7} {:<36} {:>5.2} ms  p95 {:>5.2} ms", STATES[k], s.median, s.p95);
            }
        }
    }

    // 2. Apply, Discard and the undo of an Apply, each until the next frame is on screen.
    let cam = &framed;
    let pure = [
        ("100k mixed", mix.clone()),
        ("100k added", plan(&ed.scene, 100_000, 0, 0)),
        ("100k repainted", plan(&ed.scene, 0, 100_000, 0)),
        ("100k removed", plan(&ed.scene, 0, 0, 100_000)),
    ];
    // Control: a frame with no edit after 10 ms of CPU-only work, as Apply follows the
    // staging work. The GPU clocks down while it idles, so this frame is slower than one in a
    // steady stream.
    let idle: Vec<f64> = (0..REPEATS)
        .map(|_| {
            let t = Instant::now();
            while ms(t) < 10.0 {
                std::hint::spin_loop();
            }
            let t0 = Instant::now();
            ed.frame(&cx, cam);
            ms(t0)
        })
        .collect();
    let mut sorted = idle.clone();
    sorted.sort_by(f64::total_cmp);
    println!(
        "\ncontrol: one frame after 10 ms of CPU work, no edit: {:.2} ms median, {:.2} ms max",
        sorted[REPEATS / 2],
        sorted[REPEATS - 1]
    );
    println!(
        "\nstage, apply, discard and undo, {REPEATS} repeats each (median / p95 / max), until the next frame is drawn:"
    );
    for (label, s) in &pure {
        assert_eq!(s.cells(), 100_000, "{label}: the island has room");
        let (mut stage, mut mask, mut show_t) = (vec![], vec![], vec![]);
        let mut ops: [Vec<[f64; 5]>; 3] = Default::default();
        let mut rebuilt = [false; 3];
        for _ in 0..REPEATS {
            // Discard.
            let [a, b, c] = ed.stage(&cx, cam, s);
            stage.push(a);
            mask.push(b);
            show_t.push(c);
            let t0 = Instant::now();
            Staged::discard(&mut ed.scene);
            let (t, r) = ed.finish(&cx, cam, t0);
            ops[1].push(t);
            rebuilt[1] |= r;
            assert_eq!(fingerprint(&ed.scene), original, "{label}: discard restores the island");
            assert!(!ed.scene.history().can_undo(), "{label}: discard leaves no undo step");

            // Apply, then undo it.
            ed.stage(&cx, cam, s);
            let t0 = Instant::now();
            let n = s.apply(&mut ed.scene);
            let (t, r) = ed.finish(&cx, cam, t0);
            ops[0].push(t);
            rebuilt[0] |= r;
            assert_eq!(n, s.cells(), "{label}: apply changes every staged cell");
            assert!(ed.scene.history().can_undo(), "{label}: apply is one undo step");
            let t0 = Instant::now();
            ed.scene.undo();
            let (t, r) = ed.finish(&cx, cam, t0);
            ops[2].push(t);
            rebuilt[2] |= r;
            assert_eq!(fingerprint(&ed.scene), original, "{label}: one undo restores the island");
            assert!(!ed.scene.history().can_undo(), "{label}: apply was exactly one undo step");
        }
        let mm = |v: &[f64]| {
            let mut v = v.to_vec();
            v.sort_by(f64::total_cmp);
            let p95 = v[((v.len() - 1) as f64 * 0.95).round() as usize];
            format!("{:>6.2} / {:>6.2} / {:>6.2}", v[v.len() / 2], p95, v[v.len() - 1])
        };
        println!("  {label}:");
        println!("    stage (open edit, put cells)   {} ms", mm(&stage));
        println!("    ghost mask build (CPU)         {} ms", mm(&mask));
        println!("    upload + ghost + first frame   {} ms", mm(&show_t));
        for (k, op) in ["apply", "discard", "undo"].iter().enumerate() {
            let col = |c: usize| mm(&ops[k].iter().map(|t| t[c]).collect::<Vec<_>>());
            println!(
                "    {op:<8} {} ms  (scene {} ms, upload CPU {} ms{})",
                col(0),
                col(1),
                col(2),
                if rebuilt[k] { ", GPU map rebuilt" } else { "" }
            );
            let raw: Vec<String> =
                ops[k].iter().map(|t| format!("{:.1}/{:.1}/{:.1}/{:.1}", t[0], t[1], t[3], t[4])).collect();
            println!("             each repeat, total/scene/frame/next frame: {}", raw.join(" "));
        }
    }

    println!("\nsummary (framed view is the editor's default):");
    for (name, w) in &table {
        let (a, off, g) = (w[0], w[2], w[3]);
        println!(
            "  {name:<7} {:.2} ms before -> {:.2} ms staged with the ghost (p95 {:.2} -> {:.2}); the overlay alone {:+.2} ms",
            a.median,
            g.median,
            a.p95,
            g.p95,
            g.median - off.median
        );
    }
}
