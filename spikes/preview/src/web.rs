//! Browser entry point (WebGPU through wgpu). Called from web/index.html.

use crate::bench::{self, Config, Session};
use serde_json::{json, Value};
use vxr_engine_spike::{gpu, scene};
use wasm_bindgen::prelude::*;

fn log(s: &str) {
    web_sys::console::log_1(&s.into());
}

/// Run the test 2 measurements on the 10M-voxel terrain. `config` is a JSON `bench::Config`
/// (fields may be omitted); `steps` is a comma list of edit, rays, clean, recolor, half.
/// Returns the results as JSON text and leaves a frame with the preview on `canvas`.
#[wasm_bindgen]
pub async fn run_preview(config: String, steps: String, canvas: Option<web_sys::HtmlCanvasElement>) -> Result<String, JsValue> {
    console_error_panic_hook::set_once();
    let mut cfg: Value = serde_json::to_value(Config::default()).unwrap();
    let over: Value = serde_json::from_str(&config).unwrap_or(json!({}));
    if let (Some(base), Some(o)) = (cfg.as_object_mut(), over.as_object()) {
        for (k, v) in o {
            base.insert(k.clone(), v.clone());
        }
    }
    let cfg: Config = serde_json::from_value(cfg).map_err(|e| e.to_string())?;
    let want = |s: &str| steps.split(',').any(|x| x.trim() == s);
    let instance = wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle());
    let surface = match &canvas {
        Some(c) => Some(instance.create_surface(wgpu::SurfaceTarget::Canvas(c.clone())).map_err(|e| e.to_string())?),
        None => None,
    };
    let g = gpu::Gpu::new(instance, surface.as_ref()).await?;
    let mut out = json!({ "platform": "browser", "adapter": bench::adapter_json(&g).await, "config": cfg });
    let t0 = gpu::now_ms();
    let sc = scene::terrain(scene::terrain_side_for(10_000_000));
    out["voxels"] = json!(sc.map.voxels);
    out["scene_build_ms"] = json!(gpu::now_ms() - t0);
    log(&format!("scene built: {} voxels", sc.map.voxels));
    let mut s = Session::new(&g, sc, cfg.clone()).await?;
    out["calibration"] = json!(s.notes.clone());
    log(&format!("calibrated: {:?}", s.notes));
    for view in ["framed", "close"] {
        s.set_view(view);
        let mut v = json!({});
        if want("edit") {
            let off = s.edit_run(false).await;
            let on = s.edit_run(true).await;
            let added = on["frame_gpu_ms"]["median"].as_f64().unwrap_or(f64::NAN) - off["frame_gpu_ms"]["median"].as_f64().unwrap_or(f64::NAN);
            let added_wall = on["wall_ms_per_frame"]["median"].as_f64().unwrap_or(f64::NAN) - off["wall_ms_per_frame"]["median"].as_f64().unwrap_or(f64::NAN);
            log(&format!("{view} edit: added {added:.2} ms (gpu), {added_wall:.2} ms (wall)"));
            v["edit"] = json!({ "off": off, "on": on, "added_ms_median": added, "added_wall_ms_median": added_wall });
            if view == "framed" {
                v["flat_out"] = s.flat_out().await;
            }
        }
        if want("rays") {
            v["rays"] = s.rays().await;
            log(&format!("{view} rays: {}", v["rays"]));
        }
        if want("clean") {
            let reference = s.reference().await;
            let boosted = s.clean_time(&reference, true, |_, _, _| {}).await;
            let fixed = s.clean_time(&reference, false, |_, _, _| {}).await;
            log(&format!("{view} clean at {:?} / {:?}", boosted["clean_at_ms"], fixed["clean_at_ms"]));
            v["clean"] = json!({ "idle_boost": boosted, "fixed_slice": fixed });
        }
        if want("recolor") && view == "framed" {
            v["recolor"] = s.recolor(9, 0xff3c32dc).await;
            log(&format!("recolor: {}", v["recolor"]));
        }
        out[view] = v;
    }
    if want("half") {
        s.set_view("framed");
        s.resize(cfg.width / 2, cfg.height / 2);
        let off = s.edit_run(false).await;
        let on = s.edit_run(true).await;
        let added = on["frame_gpu_ms"]["median"].as_f64().unwrap_or(f64::NAN) - off["frame_gpu_ms"]["median"].as_f64().unwrap_or(f64::NAN);
        let added_wall = on["wall_ms_per_frame"]["median"].as_f64().unwrap_or(f64::NAN) - off["wall_ms_per_frame"]["median"].as_f64().unwrap_or(f64::NAN);
        out["half_res"] = json!({ "off": off, "on": on, "added_ms_median": added, "added_wall_ms_median": added_wall });
        s.resize(cfg.width, cfg.height);
    }
    out["notes"] = json!(s.notes.clone());
    // Leave a frame on the page as visual proof.
    if let (Some(surface), Some(canvas)) = (surface, canvas) {
        s.set_view("framed");
        s.settle(60, cfg.idle_slice_ms).await;
        surface.configure(
            &g.device,
            &wgpu::SurfaceConfiguration {
                usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
                format: bench::TARGET_FORMAT,
                color_space: Default::default(),
                width: canvas.width().max(1),
                height: canvas.height().max(1),
                present_mode: wgpu::PresentMode::Fifo,
                desired_maximum_frame_latency: 2,
                alpha_mode: wgpu::CompositeAlphaMode::Auto,
                view_formats: vec![],
            },
        );
        if let wgpu::CurrentSurfaceTexture::Success(frame) | wgpu::CurrentSurfaceTexture::Suboptimal(frame) = surface.get_current_texture() {
            let view = frame.texture.create_view(&Default::default());
            s.present(&view);
            g.queue.present(frame);
        }
    }
    Ok(serde_json::to_string(&out).unwrap())
}
