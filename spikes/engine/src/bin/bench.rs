//! Native measurements: `bench <scene> [--width W --height H --frames N --edits N --levels 2|3
//! --no-half --no-upload --load-only --shot out.png --heatmap shot.png]`
//! Scenes: `terrain:<voxels>` (e.g. terrain:10m, terrain:100m), `terrain-side:<n>`, or a `.vox` path.

use vxr_engine_spike::{bench, gpu, scene};

fn parse_count(s: &str) -> u64 {
    let s = s.to_lowercase();
    let (n, mul) = if let Some(n) = s.strip_suffix('m') { (n, 1_000_000.0) } else if let Some(n) = s.strip_suffix('k') { (n, 1000.0) } else { (s.as_str(), 1.0) };
    (n.parse::<f64>().expect("count") * mul) as u64
}

fn load(spec: &str) -> scene::Scene {
    if let Some(n) = spec.strip_prefix("terrain:") {
        let side = scene::terrain_side_for(parse_count(n));
        scene::terrain(side)
    } else if let Some(n) = spec.strip_prefix("terrain-side:") {
        scene::terrain(n.parse().expect("side"))
    } else {
        let bytes = std::fs::read(spec).expect("read .vox");
        let name = std::path::Path::new(spec).file_name().unwrap().to_string_lossy().to_string();
        scene::from_vox(&name, &bytes).expect("parse .vox")
    }
}

fn write_png(path: &str, w: u32, h: u32, rgba: &[u8]) {
    let f = std::fs::File::create(path).expect("create png");
    let mut e = png::Encoder::new(std::io::BufWriter::new(f), w, h);
    e.set_color(png::ColorType::Rgba);
    e.set_depth(png::BitDepth::Eight);
    e.write_header().unwrap().write_image_data(rgba).unwrap();
}

/// GPU busy % from other apps right before the run (macOS IOAccelerator statistics).
fn gpu_busy_pct() -> Option<f64> {
    let mut v = Vec::new();
    for _ in 0..3 {
        let out = std::process::Command::new("ioreg").args(["-r", "-d", "1", "-c", "IOAccelerator"]).output().ok()?;
        let s = String::from_utf8_lossy(&out.stdout);
        let i = s.find("\"Device Utilization %\"=")?;
        let rest = &s[i + 23..];
        let n: String = rest.chars().take_while(|c| c.is_ascii_digit()).collect();
        v.push(n.parse::<f64>().ok()?);
        std::thread::sleep(std::time::Duration::from_millis(200));
    }
    Some(v.iter().sum::<f64>() / v.len() as f64)
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let spec = args.first().filter(|a| !a.starts_with("--")).cloned().unwrap_or_else(|| "terrain:10m".into());
    let mut cfg = bench::Config::default();
    let mut shot: Option<String> = None;
    let mut heat: Option<String> = None;
    let mut limits_only = false;
    let mut check = false;
    let mut drift = 0u32;
    let mut secondary = false;
    let mut close_shot = false;
    let mut i = if args.first().is_some_and(|a| !a.starts_with("--")) { 1 } else { 0 };
    while i < args.len() {
        let next = |i: &mut usize| {
            *i += 1;
            args[*i].clone()
        };
        match args[i].as_str() {
            "--width" => cfg.width = next(&mut i).parse().unwrap(),
            "--height" => cfg.height = next(&mut i).parse().unwrap(),
            "--frames" => cfg.frames = next(&mut i).parse().unwrap(),
            "--edits" => cfg.edits = next(&mut i).parse().unwrap(),
            "--levels" => cfg.levels = next(&mut i).parse().unwrap(),
            "--no-half" => cfg.half = false,
            "--no-upload" => cfg.upload_test = false,
            "--load-only" => cfg.load_only = true,
            "--shot" => shot = Some(next(&mut i)),
            "--close-shot" => close_shot = true,
            "--heatmap" => heat = Some(next(&mut i)),
            "--limits" => limits_only = true,
            "--check" => check = true,
            "--drift" => drift = next(&mut i).parse().unwrap(),
            "--secondary" => secondary = true,
            "--layout" => cfg.layout = serde_json::from_value(serde_json::json!(next(&mut i))).expect("layout dense|compact"),
            "--add-only" => cfg.add_only = true,
            "--fill" => cfg.fill_edits = next(&mut i).parse().unwrap(),
            a => panic!("unknown arg {a}"),
        }
        i += 1;
    }
    let instance = wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle());
    let g = pollster::block_on(gpu::Gpu::new(instance, None)).expect("gpu");
    let limits = bench::limits_json(&g);
    if limits_only {
        println!("{}", serde_json::to_string_pretty(&limits).unwrap());
        return;
    }
    let busy = gpu_busy_pct();
    let t0 = gpu::now_ms();
    let sc = load(&spec);
    let gen_ms = gpu::now_ms() - t0;
    eprintln!("scene {}: {} voxels, {} bricks, built in {:.0} ms", sc.name, sc.map.voxels, sc.map.live_bricks(), gen_ms);

    if check {
        check_layouts(&g, sc, &cfg);
        return;
    }
    if drift > 0 {
        let (mut b, _) = pollster::block_on(bench::Bench::new(&g, sc, cfg.clone())).expect("bench");
        b.renderer.secondary = secondary;
        let mut out = pollster::block_on(b.drift(drift));
        out["secondary"] = serde_json::json!(secondary);
        if let Some(path) = &heat {
            b.renderer.heatmap = true;
            b.frame(&b.framed_camera(), None);
            pollster::block_on(gpu::wait_idle(&g.device, &g.queue));
            let px = pollster::block_on(b.renderer.read_color(&g.device, &g.queue));
            write_png(path, b.renderer.width, b.renderer.height, &px);
        }
        out["layout"] = serde_json::json!(cfg.layout);
        out["levels"] = serde_json::json!(cfg.levels);
        out["gpu_busy_before_pct"] = serde_json::json!(busy);
        for k in ["before", "after"] {
            if let Some(o) = out[k].as_object_mut() {
                o.remove("gpu_ms_all");
                o.remove("wall_ms_all");
            }
        }
        println!("{}", serde_json::to_string(&out).unwrap());
        return;
    }

    if shot.is_some() || heat.is_some() {
        let (mut b, up) = pollster::block_on(bench::Bench::new(&g, sc, cfg.clone())).expect("bench");
        eprintln!("{}", up);
        let cam = if close_shot { b.close_camera() } else { b.framed_camera() };
        for (path, hm) in [(shot.clone(), false), (heat.clone(), true)] {
            let Some(path) = path else { continue };
            b.renderer.heatmap = hm;
            b.frame(&cam, None);
            pollster::block_on(gpu::wait_idle(&g.device, &g.queue));
            let px = pollster::block_on(b.renderer.read_color(&g.device, &g.queue));
            write_png(&path, b.renderer.width, b.renderer.height, &px);
            eprintln!("wrote {path}");
        }
        return;
    }

    let mut out = pollster::block_on(bench::run(&g, sc, cfg.clone())).expect("run");
    out["scene_build_ms"] = serde_json::json!(gen_ms);
    out["limits"] = limits;
    out["platform"] = serde_json::json!("native");
    out["gpu_busy_before_pct"] = serde_json::json!(busy);
    out["load_avg"] = serde_json::json!(std::process::Command::new("sysctl").args(["-n", "vm.loadavg"]).output().map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string()).unwrap_or_default());
    if cfg.upload_test && !cfg.load_only {
        out["upload_256mb"] = pollster::block_on(bench::upload_throughput(&g));
    }
    println!("{}", serde_json::to_string_pretty(&out).unwrap());
}

/// Render the same scene through the dense and compacted layouts, after a run of mixed edits
/// (add, erase, paint, radius 2 to 16, plus whole-brick fills), and count differing pixels.
fn check_layouts(g: &gpu::Gpu, sc: scene::Scene, cfg: &bench::Config) {
    use vxr_engine_spike::{camera::OrbitCamera, gpu::MapLayout};
    let layout = gpu::brickmap_layout(&g.device);
    let (mut ma, mut mb) = (sc.map.clone(), sc.map.clone());
    let mut da = gpu::GpuBrickMap::new_with_layout(&g.device, &g.queue, &layout, &ma, &sc.palette, MapLayout::Dense).unwrap();
    let mut db = gpu::GpuBrickMap::new_with_layout(&g.device, &g.queue, &layout, &mb, &sc.palette, MapLayout::Compact).unwrap();
    let r = gpu::Renderer::new(&g.device, &layout, bench::TARGET_FORMAT, cfg.width, cfg.height, gpu::RenderOptions { gbuffer: false, levels: 4 });
    let r3 = gpu::Renderer::new(&g.device, &layout, bench::TARGET_FORMAT, cfg.width, cfg.height, gpu::RenderOptions { gbuffer: false, levels: 3 });
    let (lo, hi) = sc.map.occupied_bounds().unwrap();
    let cam = OrbitCamera::framing_aspect(glam::Vec3::new(lo[0] as f32, lo[1] as f32, lo[2] as f32), glam::Vec3::new(hi[0] as f32, hi[1] as f32, hi[2] as f32), cfg.width as f32 / cfg.height as f32);
    let draw = |m: &gpu::GpuBrickMap| {
        r.set_camera(&g.queue, &cam);
        let mut enc = g.device.create_command_encoder(&Default::default());
        r.march(&mut enc, m, None);
        g.queue.submit([enc.finish()]);
        pollster::block_on(r.read_color(&g.device, &g.queue))
    };
    let draw3 = |m: &gpu::GpuBrickMap| {
        r3.set_camera(&g.queue, &cam);
        let mut enc = g.device.create_command_encoder(&Default::default());
        r3.march(&mut enc, m, None);
        g.queue.submit([enc.finish()]);
        pollster::block_on(r3.read_color(&g.device, &g.queue))
    };
    let diff = |a: &[u8], b: &[u8]| a.chunks_exact(4).zip(b.chunks_exact(4)).filter(|(x, y)| x != y).count();
    println!("before edits: {} differing pixels (dense vs compacted), {} (trace vs trace_h)", diff(&draw(&da), &draw(&db)), diff(&draw(&da), &draw3(&da)));
    ma.track_dirty(true);
    mb.track_dirty(true);
    let (w, h) = (cfg.width as f32, cfg.height as f32);
    let mut seed = 12345u32;
    let mut rnd = || {
        seed ^= seed << 13;
        seed ^= seed >> 17;
        seed ^= seed << 5;
        (seed % 10000) as f32 / 10000.0
    };
    let mut reallocs = 0;
    let mut rebuilds = 0;
    for e in 0..300 {
        let (ro, dir) = cam.ray((0.2 + 0.6 * rnd()) * w, (0.2 + 0.6 * rnd()) * h, w, h);
        let Some(hit) = ma.raycast(ro.into(), dir.into(), 1e6) else { continue };
        let c = [hit.voxel[0] as f32 + 0.5, hit.voxel[1] as f32 + 0.5, hit.voxel[2] as f32 + 0.5];
        let radius = 2.0 + 14.0 * rnd();
        let (v, paint) = match e % 3 {
            0 => (200, false),
            1 => (0, false),
            _ => (201, true),
        };
        for m in [&mut ma, &mut mb] {
            if e % 25 == 0 {
                let b = [hit.voxel[0] / 8 * 8, hit.voxel[1] / 8 * 8, hit.voxel[2] / 8 * 8];
                for z in 0..8 {
                    for y in 0..8 {
                        for x in 0..8 {
                            m.set(b[0] + x, b[1] + y, b[2] + z, 202);
                        }
                    }
                }
            } else {
                m.sphere(c, radius, v, paint);
            }
        }
        da.upload_dirty(&g.device, &g.queue, &mut ma, &sc.palette).unwrap();
        let st = db.upload_dirty(&g.device, &g.queue, &mut mb, &sc.palette).unwrap();
        reallocs += st.reallocs;
        rebuilds += st.rebuilt as u32;
    }
    println!(
        "after 300 edits ({} reallocs, {} rebuilds): {} differing pixels (dense vs compacted), {} (trace vs trace_h), voxels {} vs {}",
        reallocs,
        rebuilds,
        diff(&draw(&da), &draw(&db)),
        diff(&draw(&da), &draw3(&da)),
        ma.voxels,
        mb.voxels
    );
    println!("heap {:?}", db.heap_stats());
}
