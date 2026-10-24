//! Native measurements for test 2.
//! `pbench [--width W --height H --frames N --slice MS --psnr DB --only edit|rays|clean|recolor|shots]`

use serde_json::json;
use vxr_engine_spike::{gpu, scene};
use vxr_preview_spike::bench::{self, Config, Session};

fn write_png(path: &str, w: u32, h: u32, rgba: &[u8]) {
    let f = std::fs::File::create(path).expect("create png");
    let mut e = png::Encoder::new(std::io::BufWriter::new(f), w, h);
    e.set_color(png::ColorType::Rgba);
    e.set_depth(png::BitDepth::Eight);
    e.write_header().unwrap().write_image_data(rgba).unwrap();
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let mut cfg = Config::default();
    let mut only: Vec<String> = Vec::new();
    let mut shots = "shots".to_string();
    let mut voxels = 10_000_000u64;
    let mut view = "framed".to_string();
    let mut it = args.into_iter();
    while let Some(a) = it.next() {
        let mut next = || it.next().expect("missing value");
        match a.as_str() {
            "--width" => cfg.width = next().parse().unwrap(),
            "--height" => cfg.height = next().parse().unwrap(),
            "--frames" => cfg.frames = next().parse().unwrap(),
            "--slice" => cfg.slice_ms = next().parse().unwrap(),
            "--idle-slice" => cfg.idle_slice_ms = next().parse().unwrap(),
            "--psnr" => cfg.psnr_clean = next().parse().unwrap(),
            "--ref-spp" => cfg.reference_spp = next().parse().unwrap(),
            "--voxels" => voxels = next().parse().unwrap(),
            "--only" => only.push(next()),
            "--shots" => shots = next(),
            "--view" => view = next(),
            a => panic!("unknown arg {a}"),
        }
    }
    let want = |s: &str| only.is_empty() || only.iter().any(|o| o == s);
    let instance = wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle());
    let g = pollster::block_on(gpu::Gpu::new(instance, None)).expect("gpu");
    let t0 = gpu::now_ms();
    let sc = scene::terrain(scene::terrain_side_for(voxels));
    eprintln!("scene {}: {} voxels in {:.0} ms", sc.name, sc.map.voxels, gpu::now_ms() - t0);
    let voxel_count = sc.map.voxels;
    let mut s = pollster::block_on(Session::new(&g, sc, cfg.clone())).expect("session");
    eprintln!("calibration: {:?}, ms/row {:.5}", s.notes, s.ms_per_row);
    s.set_view(&view);
    let mut out = json!({
        "platform": "native",
        "adapter": pollster::block_on(bench::adapter_json(&g)),
        "config": cfg,
        "voxels": voxel_count,
        "view": view,
    });
    if want("editshot") {
        std::fs::create_dir_all(&shots).ok();
        pollster::block_on(s.edit_frames(90));
        let img = pollster::block_on(s.grab_preview());
        write_png(&format!("{shots}/preview-while-editing-{view}.png"), 640, 360, &img);
        let (w, h, img) = pollster::block_on(s.grab_editor());
        write_png(&format!("{shots}/editor-while-editing-{view}-{w}x{h}.png"), w, h, &img);
        pollster::block_on(s.settle(1, 0.0));
        pollster::block_on(s.settle(200, 12.0));
        let img = pollster::block_on(s.grab_preview());
        write_png(&format!("{shots}/preview-after-editing-settled-{view}.png"), 640, 360, &img);
    }

    if want("edit") {
        let off = pollster::block_on(s.edit_run(false));
        eprintln!("edit off: frame {}", off["frame_gpu_ms"]["median"]);
        let on = pollster::block_on(s.edit_run(true));
        eprintln!("edit on: frame {} trace {} denoise {} spp/frame {}", on["frame_gpu_ms"]["median"], on["trace_gpu_ms"]["median"], on["denoise_gpu_ms"]["median"], on["spp_per_frame"]["median"]);
        let added = on["frame_gpu_ms"]["median"].as_f64().unwrap() - off["frame_gpu_ms"]["median"].as_f64().unwrap();
        let added_p95 = on["frame_gpu_ms"]["p95"].as_f64().unwrap() - off["frame_gpu_ms"]["p95"].as_f64().unwrap();
        out["edit"] = json!({ "off": off, "on": on, "added_ms_median": added, "added_ms_p95": added_p95 });
        out["flat_out"] = pollster::block_on(s.flat_out());
    }
    if only.iter().any(|o| o == "diag") {
        let f = |v: &serde_json::Value| format!("frame {:.3} p95 {:.3} trace {:.3} denoise {:.3} spp {:.2}", v["frame_gpu_ms"]["median"].as_f64().unwrap(), v["frame_gpu_ms"]["p95"].as_f64().unwrap(), v["trace_gpu_ms"]["median"].as_f64().unwrap(), v["denoise_gpu_ms"]["median"].as_f64().unwrap(), v["spp_per_frame"]["median"].as_f64().unwrap());
        let off = pollster::block_on(s.edit_run_with(false, true));
        eprintln!("off, edits:            {}", f(&off));
        let off_ne = pollster::block_on(s.edit_run_with(false, false));
        eprintln!("off, no edits:         {}", f(&off_ne));
        let on = pollster::block_on(s.edit_run_with(true, true));
        eprintln!("on, edits:             {}", f(&on));
        let on_ne = pollster::block_on(s.edit_run_with(true, false));
        eprintln!("on, no edits:          {}", f(&on_ne));
        let keep = s.cfg.slice_ms;
        s.cfg.slice_ms = 0.0;
        let tiny = pollster::block_on(s.edit_run_with(true, true));
        eprintln!("on, edits, 8 rows:     {}", f(&tiny));
        s.preview.denoise = false;
        let nodn = pollster::block_on(s.edit_run_with(true, true));
        eprintln!("on, 8 rows, no denoise:{}", f(&nodn));
        s.preview.denoise = true;
        s.cfg.slice_ms = keep;
    }
    if only.iter().any(|o| o == "rays-after-edits") {
        let r0 = pollster::block_on(s.rays());
        let r0b = pollster::block_on(s.rays_with_tracer(3));
        pollster::block_on(s.edit_frames(240));
        let r1 = pollster::block_on(s.rays());
        let r1b = pollster::block_on(s.rays_with_tracer(3));
        for k in 0..3 {
            let r = pollster::block_on(s.rays());
            eprintln!("after edits, trace_h again {k}: {:.0}/{:.0} primary p95 {:.3} min {:.3} max {:.3}", r["primary"]["mrays_per_s"].as_f64().unwrap(), r["bounce"]["mrays_per_s"].as_f64().unwrap(), r["primary"]["gpu_ms"]["p95"].as_f64().unwrap(), r["primary"]["gpu_ms"]["min"].as_f64().unwrap(), r["primary"]["gpu_ms"]["max"].as_f64().unwrap());
        }
        eprintln!("before edits: trace_h {:.0}/{:.0} trace {:.0}/{:.0}", r0["primary"]["mrays_per_s"].as_f64().unwrap(), r0["bounce"]["mrays_per_s"].as_f64().unwrap(), r0b["primary"]["mrays_per_s"].as_f64().unwrap(), r0b["bounce"]["mrays_per_s"].as_f64().unwrap());
        eprintln!("after edits:  trace_h {:.0}/{:.0} trace {:.0}/{:.0}", r1["primary"]["mrays_per_s"].as_f64().unwrap(), r1["bounce"]["mrays_per_s"].as_f64().unwrap(), r1b["primary"]["mrays_per_s"].as_f64().unwrap(), r1b["bounce"]["mrays_per_s"].as_f64().unwrap());
    }
    if only.iter().any(|o| o == "tracer-ab") {
        // Full-sample cost with each traversal, before and after the brush edits.
        for (tag, edits) in [("before edits", 0u32), ("after edits", 240)] {
            if edits > 0 {
                pollster::block_on(s.edit_frames(edits));
            }
            for tr in [3u32, 4] {
                s.set_tracer(tr);
                let f = pollster::block_on(s.flat_out());
                eprintln!("{tag}, tracer {tr}: full sample {:.3} ms ({:.0} spp/GPU-s)", f["full_sample_gpu_ms"]["median"].as_f64().unwrap(), f["spp_per_gpu_second"].as_f64().unwrap());
            }
        }
        s.set_tracer(4);
    }
    if only.iter().any(|o| o == "heat") {
        // Editor step heat map (engine levels 4 = trace_h) before and after the brush edits.
        std::fs::create_dir_all(&shots).ok();
        s.editor.heatmap = true;
        for (tag, edits) in [("before", 0u32), ("after", 240)] {
            if edits > 0 {
                s.editor.heatmap = false;
                pollster::block_on(s.edit_frames(edits));
                s.editor.heatmap = true;
            }
            pollster::block_on(s.settle(2, 0.0));
            let (w, h, img) = pollster::block_on(s.grab_editor());
            let mean_r: f64 = img.chunks_exact(4).map(|p| p[0] as f64).sum::<f64>() / (w * h) as f64;
            eprintln!("heat {tag}: mean red {mean_r:.1} (steps / 256, gamma encoded)");
            write_png(&format!("{shots}/editor-heat-{tag}-edits.png"), w, h, &img);
        }
        s.editor.heatmap = false;
    }
    if want("rays") {
        out["rays_trace3"] = pollster::block_on(s.rays_with_tracer(3));
        out["rays"] = pollster::block_on(s.rays());
        eprintln!("rays: {}", out["rays"]);
    }
    if want("clean") || want("shots") {
        std::fs::create_dir_all(&shots).ok();
        let reference = pollster::block_on(s.reference());
        write_png(&format!("{shots}/reference-{}spp.png", cfg.reference_spp), 640, 360, &reference);
        if want("shots") {
            // By-eye calibration of the PSNR line: keep the first frame past each level.
            let levels = [26.0, 30.0, 33.0, 35.0, 36.0, 37.0, 38.0, 39.0, 40.0, 42.0];
            let mut saved = vec![false; levels.len()];
            let shots2 = shots.clone();
            let r = pollster::block_on(s.clean_time(&reference, true, |t, p, img| {
                for (k, l) in levels.iter().enumerate() {
                    if !saved[k] && p >= *l {
                        saved[k] = true;
                        write_png(&format!("{shots2}/calib-psnr{:.0}-t{:.0}ms.png", l, t), 640, 360, img);
                    }
                }
            }));
            out["calib_curve"] = r;
        }
        if want("clean") {
            let mut keep_img: Option<Vec<u8>> = None;
            let mut pass_t = None;
            let line = cfg.psnr_clean;
            let boosted = pollster::block_on(s.clean_time(&reference, true, |t, p, img| {
                if p >= line && pass_t.is_none() {
                    pass_t = Some(t);
                    keep_img = Some(img.to_vec());
                } else if p < line {
                    pass_t = None;
                    keep_img = None;
                }
            }));
            if let Some(img) = &keep_img {
                write_png(&format!("{shots}/denoised-at-pass.png"), 640, 360, img);
            }
            let fixed = pollster::block_on(s.clean_time(&reference, false, |_, _, _| {}));
            let (spp, noisy) = pollster::block_on(s.noisy(1));
            write_png(&format!("{shots}/noisy-raw-{:.1}spp.png", spp), 640, 360, &noisy);
            out["clean"] = json!({ "idle_boost": boosted, "fixed_slice": fixed, "noisy_frame_spp": spp });
            eprintln!("clean: boosted {:?} fixed {:?}", out["clean"]["idle_boost"]["clean_at_ms"], out["clean"]["fixed_slice"]["clean_at_ms"]);
        }
    }
    if want("recolor") {
        out["recolor"] = pollster::block_on(s.recolor(9, 0xff3c32dc));
        eprintln!("recolor: {}", out["recolor"]);
    }
    if want("shots") {
        pollster::block_on(s.settle(240, cfg.idle_slice_ms));
        let (w, h, img) = pollster::block_on(s.grab_editor());
        write_png(&format!("{shots}/editor-with-preview-{w}x{h}.png"), w, h, &img);
    }
    out["notes"] = json!(s.notes);
    println!("{}", serde_json::to_string_pretty(&out).unwrap());
}
