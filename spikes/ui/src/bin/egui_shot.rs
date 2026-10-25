//! Headless egui Hybrid screenshots and UI cost: `egui_shot [scene.vox] [out dir]`.
//! Writes `egui-1x.png` and `egui-2x.png` (1280x800 points, like the mockup), plus a Cmd-K shot,
//! then times 300 frames with and without the UI at 2x and prints medians and p95s.

use std::path::PathBuf;
use vxr_engine_spike::scene;
use vxr_ui_spike::hybrid::{Hybrid, Stats};
use vxr_ui_spike::offscreen::{Offscreen, load_fonts, write_png};

fn pct(v: &mut [f64], p: f64) -> f64 {
    v.sort_by(|a, b| a.partial_cmp(b).unwrap());
    v[((v.len() - 1) as f64 * p).round() as usize]
}

fn main() {
    let here = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let args: Vec<String> = std::env::args().collect();
    let scene_path = args.get(1).map(PathBuf::from).unwrap_or_else(|| here.join("../../prototype/demo/scene.vox"));
    let out = args.get(2).map(PathBuf::from).unwrap_or_else(|| here.join("shots"));
    std::fs::create_dir_all(&out).unwrap();
    let bytes = std::fs::read(&scene_path).expect("read scene (run node prototype/src/build.mjs)");
    for ppp in [1.0f32, 2.0] {
        let sc = scene::from_vox("island", &bytes).unwrap();
        let mut used = vec![false; 256];
        let s = sc.map.size_voxels();
        for z in 0..s[2] as i32 {
            for y in 0..s[1] as i32 {
                for x in 0..s[0] as i32 {
                    used[sc.map.get(x, y, z) as usize] = true;
                }
            }
        }
        used[0] = false;
        let mut hybrid = Hybrid::new(sc.palette, used, "Island diorama");
        let stats = Stats { voxels: sc.map.voxels, fps: 120.0, frame_ms: 0.0, cursor: None };
        let mut off = Offscreen::new(sc, (1280, 800), ppp, load_fonts(&here.join("fetched")));
        let tag = if ppp == 1.0 { "1x" } else { "2x" };
        // A few frames so fonts are uploaded and layout settles.
        for _ in 0..3 {
            let input = off.raw_input(vec![]);
            off.frame(0, Some((&mut hybrid, &stats, input)));
        }
        write_png(&out.join(format!("egui-{tag}.png")), off.width, off.height, &off.read_rgba());
        hybrid.coach = false;
        hybrid.cmdk = Some("pa".into());
        for _ in 0..2 {
            let input = off.raw_input(vec![]);
            off.frame(0, Some((&mut hybrid, &stats, input)));
        }
        write_png(&out.join(format!("egui-cmdk-{tag}.png")), off.width, off.height, &off.read_rgba());
        hybrid.cmdk = None;
        hybrid.coach = true;
        if ppp != 2.0 {
            continue;
        }
        let mut cpu = vec![];
        for i in 0..600u32 {
            off.cam.orbit(2.0, 0.0);
            if i % 2 == 0 {
                let stats = Stats { frame_ms: i as f32 * 0.01, ..stats.clone() };
                let input = off.raw_input(vec![]);
                cpu.push(off.frame(i, Some((&mut hybrid, &stats, input))).ui_cpu);
            } else {
                off.frame(i, None);
            }
        }
        let t = off.gpu_times(600);
        let mut with: Vec<f64> = t.iter().step_by(2).map(|t| t.1).collect();
        let mut without: Vec<f64> = t.iter().skip(1).step_by(2).map(|t| t.0).collect();
        let empty = if std::env::var("VXR_UI_EMPTY").is_ok() { " (UI pass left empty)" } else { "" };
        println!("egui native{empty}, 1280x800 points at 2x ({}x{} px), 300 frames each, alternating:", off.width, off.height);
        println!("  UI CPU (run + tessellate + upload): median {:.3} ms, p95 {:.3} ms", pct(&mut cpu.clone(), 0.5), pct(&mut cpu, 0.95));
        let (wm, wp) = (pct(&mut with.clone(), 0.5), pct(&mut with, 0.95));
        let (om, op) = (pct(&mut without.clone(), 0.5), pct(&mut without, 0.95));
        println!("  GPU frame without UI: median {om:.3} ms, p95 {op:.3} ms");
        println!("  GPU frame with UI:    median {wm:.3} ms, p95 {wp:.3} ms");
        println!("  UI adds (median): {:.3} ms GPU, {:.3} ms GPU + CPU", wm - om, wm - om + pct(&mut cpu, 0.5));
    }
}
