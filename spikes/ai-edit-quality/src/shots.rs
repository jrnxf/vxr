//! Screenshots for the owner's blind judging: the real editor (`vxr_app::Editor`, the same
//! code `vxr --shot` draws its first `app-1x.png` with), headless, from the default angle and
//! from the opposite side, with the render corner hidden.

use std::path::Path;

use vxr_app::Editor;
use vxr_core::Scene;
use vxr_render::Gpu;
use vxr_render::gpu::read_texture;

const FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Bgra8Unorm;
const POINTS: (u32, u32) = (1280, 800);

/// Write `<prefix>-a.png` (the default view), `<prefix>-b.png` (turned half way round) and,
/// with `focus` (world cells), `<prefix>-c.png` framed on that box from the default angle.
pub fn shoot(scene: Scene, name: &str, prefix: &Path, focus: Option<([i32; 3], [i32; 3])>) -> Result<(), String> {
    let instance = wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle());
    let gpu = pollster::block_on(Gpu::new(instance, None))?;
    let target = gpu.device.create_texture(&wgpu::TextureDescriptor {
        label: Some("shot"),
        size: wgpu::Extent3d { width: POINTS.0, height: POINTS.1, depth_or_array_layers: 1 },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: FORMAT,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
        view_formats: &[],
    });
    let fonts = vxr_app::fonts::load(&vxr_app::fonts::dir()).ok();
    let mut ed = Editor::new(gpu, scene, name, FORMAT, POINTS, 1.0, fonts)?;
    ed.ui.show_render = false;
    ed.ui.coach = false;
    let home = ed.cam;
    let mut time = 0.0;
    let mut shots = vec![("a", 0.0), ("b", std::f32::consts::PI)];
    if focus.is_some() {
        shots.push(("c", 0.0));
    }
    for (tag, turn) in shots {
        ed.cam = home;
        ed.cam.yaw += turn;
        if let (true, Some((a, b))) = (tag == "c", focus) {
            // The box with a margin, so the change shows in its surroundings.
            let (a, b) = (ed.scene.to_map(a), ed.scene.to_map(b));
            let pad = (0..3).map(|k| (b[k] - a[k]).abs()).max().unwrap_or(0).clamp(10, 24) as f32;
            let v = |p: [i32; 3]| glam::Vec3::new(p[0] as f32, p[1] as f32, p[2] as f32);
            let lo = v(std::array::from_fn(|k| a[k].min(b[k]))) - pad;
            let hi = v(std::array::from_fn(|k| a[k].max(b[k]) + 1)) + pad;
            ed.cam.fit(lo, hi, POINTS.0 as f32 / POINTS.1 as f32, 0.9);
        }
        let view = target.create_view(&Default::default());
        for _ in 0..3 {
            time += 1.0 / 60.0;
            let size = egui::vec2(POINTS.0 as f32, POINTS.1 as f32);
            let mut input = egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, size)),
                time: Some(time),
                ..Default::default()
            };
            input.viewports.entry(egui::ViewportId::ROOT).or_default().native_pixels_per_point = Some(1.0);
            ed.frame(input, &view);
        }
        let mut px = pollster::block_on(read_texture(&ed.gpu.device, &ed.gpu.queue, &target));
        for p in px.as_chunks_mut::<4>().0 {
            *p = [p[2], p[1], p[0], 255];
        }
        let png = crate::view::encode_png(&px, POINTS.0, POINTS.1)?;
        let mut path = prefix.as_os_str().to_owned();
        path.push(format!("-{tag}.png"));
        std::fs::write(&path, png).map_err(|e| format!("{}: {e}", Path::new(&path).display()))?;
    }
    Ok(())
}
