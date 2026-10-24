//! Interactive native viewer: `view [scene]` (same scene specs as `bench`).
//! Right-drag orbits, Shift+right-drag or middle-drag pans, wheel zooms toward the cursor,
//! left click adds a radius-8 sphere, Shift+left click erases, `.` frames the scene, H toggles
//! the traversal heat map. The title shows frame time and the last edit's CPU cost.

use std::sync::Arc;
use vxr_engine_spike::{bench, brickmap::BrickMap, camera::OrbitCamera, gpu, scene};
use winit::application::ApplicationHandler;
use winit::event::{ElementState, MouseButton, MouseScrollDelta, WindowEvent};
use winit::event_loop::{ActiveEventLoop, EventLoop};
use winit::keyboard::{Key, ModifiersState, NamedKey};
use winit::window::{Window, WindowId};

struct State {
    window: Arc<Window>,
    surface: wgpu::Surface<'static>,
    gpu: gpu::Gpu,
    map: gpu::GpuBrickMap,
    renderer: gpu::Renderer,
    scene: scene::Scene,
    cam: OrbitCamera,
    cursor: (f32, f32),
    drag: Option<MouseButton>,
    mods: ModifiersState,
    last_frame: f64,
    frame_ms: f64,
    edit_ms: f64,
}

struct App {
    spec: String,
    state: Option<State>,
}

fn load(spec: &str) -> scene::Scene {
    if let Some(n) = spec.strip_prefix("terrain:") {
        let n = n.to_lowercase();
        let count = n.strip_suffix('m').map(|m| m.parse::<f64>().unwrap() * 1e6).unwrap_or_else(|| n.parse().unwrap());
        scene::terrain(scene::terrain_side_for(count as u64))
    } else {
        let bytes = std::fs::read(spec).expect("read .vox");
        scene::from_vox(spec, &bytes).expect("parse .vox")
    }
}

fn bounds(map: &BrickMap) -> (glam::Vec3, glam::Vec3) {
    let (lo, hi) = map.occupied_bounds().unwrap_or(([0; 3], [8; 3]));
    (glam::Vec3::new(lo[0] as f32, lo[1] as f32, lo[2] as f32), glam::Vec3::new(hi[0] as f32, hi[1] as f32, hi[2] as f32))
}

impl ApplicationHandler for App {
    fn resumed(&mut self, el: &ActiveEventLoop) {
        if self.state.is_some() {
            return;
        }
        let window = Arc::new(
            el.create_window(Window::default_attributes().with_title("vxr engine spike").with_inner_size(winit::dpi::LogicalSize::new(1512.0, 982.0)))
                .unwrap(),
        );
        let instance = wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle());
        let surface = instance.create_surface(window.clone()).unwrap();
        let g = pollster::block_on(gpu::Gpu::new(instance, Some(&surface))).unwrap();
        let size = window.inner_size();
        surface.configure(
            &g.device,
            &wgpu::SurfaceConfiguration {
                usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
                format: bench::TARGET_FORMAT,
                color_space: Default::default(),
                width: size.width,
                height: size.height,
                present_mode: wgpu::PresentMode::Fifo,
                desired_maximum_frame_latency: 1,
                alpha_mode: wgpu::CompositeAlphaMode::Auto,
                view_formats: vec![],
            },
        );
        let mut sc = load(&self.spec);
        sc.map.track_dirty(true);
        let layout = gpu::brickmap_layout(&g.device);
        let map = gpu::GpuBrickMap::new(&g.device, &g.queue, &layout, &sc.map, &sc.palette).unwrap();
        let renderer = gpu::Renderer::new(&g.device, &layout, bench::TARGET_FORMAT, size.width, size.height, gpu::RenderOptions { gbuffer: false, levels: 4 });
        let (lo, hi) = bounds(&sc.map);
        let cam = OrbitCamera::framing_aspect(lo, hi, size.width as f32 / size.height as f32);
        self.state = Some(State {
            window,
            surface,
            gpu: g,
            map,
            renderer,
            scene: sc,
            cam,
            cursor: (0.0, 0.0),
            drag: None,
            mods: ModifiersState::empty(),
            last_frame: gpu::now_ms(),
            frame_ms: 0.0,
            edit_ms: 0.0,
        });
    }

    fn window_event(&mut self, el: &ActiveEventLoop, _id: WindowId, event: WindowEvent) {
        let Some(s) = self.state.as_mut() else { return };
        let (w, h) = (s.renderer.width as f32, s.renderer.height as f32);
        match event {
            WindowEvent::CloseRequested => el.exit(),
            WindowEvent::Resized(size) if size.width > 0 && size.height > 0 => {
                let mut cfg = s.surface.get_configuration().unwrap();
                cfg.width = size.width;
                cfg.height = size.height;
                s.surface.configure(&s.gpu.device, &cfg);
                s.renderer.resize(&s.gpu.device, size.width, size.height);
            }
            WindowEvent::ModifiersChanged(m) => s.mods = m.state(),
            WindowEvent::KeyboardInput { event, .. } if event.state == ElementState::Pressed => match event.logical_key {
                Key::Character(c) if c == "." => {
                    let (lo, hi) = bounds(&s.scene.map);
                    s.cam = OrbitCamera::framing_aspect(lo, hi, w / h);
                }
                Key::Character(c) if c == "h" => s.renderer.heatmap = !s.renderer.heatmap,
                Key::Named(NamedKey::Escape) => el.exit(),
                _ => {}
            },
            WindowEvent::CursorMoved { position, .. } => {
                let (x, y) = (position.x as f32, position.y as f32);
                let (dx, dy) = (x - s.cursor.0, y - s.cursor.1);
                s.cursor = (x, y);
                match s.drag {
                    Some(MouseButton::Right) if s.mods.shift_key() => s.cam.pan(dx, dy, h),
                    Some(MouseButton::Right) => s.cam.orbit(dx, dy),
                    Some(MouseButton::Middle) => s.cam.pan(dx, dy, h),
                    _ => {}
                }
            }
            WindowEvent::MouseInput { state, button, .. } => {
                if state == ElementState::Pressed {
                    s.drag = Some(button);
                    if button == MouseButton::Left {
                        let (ro, dir) = s.cam.ray(s.cursor.0, s.cursor.1, w, h);
                        if let Some(hit) = s.scene.map.raycast(ro.into(), dir.into(), 1e6) {
                            let erase = s.mods.shift_key();
                            let v = hit.voxel;
                            let n = hit.normal;
                            let c = if erase { v } else { [v[0] + n[0], v[1] + n[1], v[2] + n[2]] };
                            let t0 = gpu::now_ms();
                            s.scene.map.sphere([c[0] as f32 + 0.5, c[1] as f32 + 0.5, c[2] as f32 + 0.5], 8.0, if erase { 0 } else { 200 }, false);
                            s.map.upload_dirty(&s.gpu.device, &s.gpu.queue, &mut s.scene.map, &s.scene.palette).unwrap();
                            s.edit_ms = gpu::now_ms() - t0;
                        }
                    }
                } else {
                    s.drag = None;
                }
            }
            WindowEvent::MouseWheel { delta, .. } => {
                let steps = match delta {
                    MouseScrollDelta::LineDelta(_, y) => y,
                    MouseScrollDelta::PixelDelta(p) => p.y as f32 / 40.0,
                };
                let (ro, dir) = s.cam.ray(s.cursor.0, s.cursor.1, w, h);
                let hit = s.scene.map.raycast(ro.into(), dir.into(), 1e6).map(|h| (dir, h.t));
                s.cam.zoom(steps, hit);
            }
            WindowEvent::RedrawRequested => {
                let now = gpu::now_ms();
                s.frame_ms = s.frame_ms * 0.9 + (now - s.last_frame) * 0.1;
                s.last_frame = now;
                let frame = match s.surface.get_current_texture() {
                    wgpu::CurrentSurfaceTexture::Success(f) | wgpu::CurrentSurfaceTexture::Suboptimal(f) => f,
                    _ => return,
                };
                s.renderer.set_camera(&s.gpu.queue, &s.cam);
                let mut enc = s.gpu.device.create_command_encoder(&Default::default());
                s.renderer.march(&mut enc, &s.map, None);
                s.renderer.blit(&mut enc, &frame.texture.create_view(&Default::default()), None);
                s.gpu.queue.submit([enc.finish()]);
                s.gpu.queue.present(frame);
                s.window.set_title(&format!(
                    "vxr engine spike - {} voxels - {:.1} ms/frame - last edit {:.2} ms",
                    s.scene.map.voxels, s.frame_ms, s.edit_ms
                ));
                s.window.request_redraw();
            }
            _ => {}
        }
    }
}

fn main() {
    let spec = std::env::args().nth(1).unwrap_or_else(|| "terrain:10m".into());
    let el = EventLoop::new().unwrap();
    let mut app = App { spec, state: None };
    el.run_app(&mut app).unwrap();
}
