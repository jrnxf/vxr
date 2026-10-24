//! Browser entry points (WebGPU through wgpu). Called from web/index.html.

use crate::{bench, gpu, scene};
use serde_json::{json, Value};
use wasm_bindgen::prelude::*;

fn load(spec: &str, vox: Option<Vec<u8>>) -> Result<scene::Scene, String> {
    if let Some(n) = spec.strip_prefix("terrain:") {
        let n = n.to_lowercase();
        let count = if let Some(m) = n.strip_suffix('m') { m.parse::<f64>().map_err(|e| e.to_string())? * 1e6 } else { n.parse::<f64>().map_err(|e| e.to_string())? };
        Ok(scene::terrain(scene::terrain_side_for(count as u64)))
    } else {
        let bytes = vox.ok_or("no .vox bytes")?;
        scene::from_vox(spec, &bytes)
    }
}

async fn device(canvas: Option<web_sys::HtmlCanvasElement>) -> Result<(gpu::Gpu, Option<wgpu::Surface<'static>>), String> {
    let instance = wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle());
    let surface = match canvas {
        Some(c) => Some(instance.create_surface(wgpu::SurfaceTarget::Canvas(c)).map_err(|e| e.to_string())?),
        None => None,
    };
    let g = gpu::Gpu::new(instance, surface.as_ref()).await?;
    Ok((g, surface))
}

/// Run the test 1 measurements for one scene. `config` is a JSON `bench::Config` (fields may be
/// omitted). Returns the results as JSON text.
#[wasm_bindgen]
pub async fn run_bench(spec: String, config: String, vox: Option<Vec<u8>>, canvas: Option<web_sys::HtmlCanvasElement>) -> Result<String, JsValue> {
    console_error_panic_hook::set_once();
    let mut cfg: Value = serde_json::to_value(bench::Config { budget_ms: 16.7, ..Default::default() }).unwrap();
    let over: Value = serde_json::from_str(&config).unwrap_or(json!({}));
    if let (Some(base), Some(o)) = (cfg.as_object_mut(), over.as_object()) {
        for (k, v) in o {
            base.insert(k.clone(), v.clone());
        }
    }
    let cfg: bench::Config = serde_json::from_value(cfg).map_err(|e| e.to_string())?;
    let (g, surface) = device(canvas.clone()).await?;
    let limits = bench::limits_json(&g);
    let t0 = gpu::now_ms();
    let sc = load(&spec, vox)?;
    let build_ms = gpu::now_ms() - t0;
    let (mut out, bench) = match bench::run_keep(&g, sc, cfg.clone()).await {
        Ok((v, b)) => (v, Some(b)),
        Err(e) => (json!({ "error": e }), None),
    };
    out["scene_build_ms"] = json!(build_ms);
    out["limits"] = limits;
    out["platform"] = json!("browser");
    // Show a frame on the page so a screenshot proves the browser renders the scene.
    if let (Some(surface), Some(canvas), Some(b)) = (surface, canvas, bench.as_ref()) {
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
            let cam = b.framed_camera();
            b.renderer.set_camera(&g.queue, &cam);
            let mut enc = g.device.create_command_encoder(&Default::default());
            b.renderer.march(&mut enc, &b.map, None);
            b.renderer.blit(&mut enc, &frame.texture.create_view(&Default::default()), None);
            g.queue.submit([enc.finish()]);
            g.queue.present(frame);
        }
    }
    drop(bench);
    if cfg.upload_test && !cfg.load_only {
        out["upload_256mb"] = bench::upload_throughput(&g).await;
    }
    Ok(out.to_string())
}

/// Allocate wasm heap in 128 MiB steps until the allocator fails or 4 GiB; returns MiB reached.
#[wasm_bindgen]
pub fn heap_probe(max_mib: u32) -> u32 {
    let mut held: Vec<Vec<u8>> = Vec::new();
    let mut total = 0u32;
    while total + 128 <= max_mib {
        let mut v: Vec<u8> = Vec::new();
        if v.try_reserve_exact(128 << 20).is_err() {
            break;
        }
        v.resize(128 << 20, 1);
        held.push(v);
        total += 128;
    }
    total
}
