//! The native window: winit and egui-winit around the [`Editor`], with file dialogs for
//! open and save.

use std::path::PathBuf;
use std::sync::Arc;

use vxr_app::Editor;
use vxr_app::editor::{Button, NEW_SIZE};
use vxr_app::files::{obj_files, slices_png};
use vxr_app::ui::Action;
use vxr_core::Scene;
use vxr_render::Gpu;
use winit::application::ApplicationHandler;
use winit::event::{ElementState, MouseButton, MouseScrollDelta, WindowEvent};
use winit::event_loop::{ActiveEventLoop, EventLoop};
use winit::keyboard::{KeyCode, PhysicalKey};
use winit::window::{Window, WindowId};

/// Open the window on `path` (or a new scene). `stage` is a review script (see
/// [`vxr_app::review::Script`]) run as an AI request once the window is up.
pub fn run(path: Option<PathBuf>, stage: Option<PathBuf>) -> Result<(), String> {
    let stage = match stage {
        Some(p) => {
            let text = std::fs::read_to_string(&p).map_err(|e| format!("{}: {e}", p.display()))?;
            Some(vxr_app::review::Script::parse(&text).map_err(|e| format!("{}: {e}", p.display()))?)
        }
        None => None,
    };
    let event_loop = EventLoop::new().map_err(|e| e.to_string())?;
    let mut app = App { path, stage, state: None, error: None };
    event_loop.run_app(&mut app).map_err(|e| e.to_string())?;
    app.error.map_or(Ok(()), Err)
}

/// Open a `.vox` file and name the scene after it.
pub fn open(path: &std::path::Path) -> Result<(Scene, String), String> {
    let bytes = std::fs::read(path).map_err(|e| format!("{}: {e}", path.display()))?;
    let scene = Scene::from_vox(&bytes).map_err(|e| format!("{}: {e}", path.display()))?;
    Ok((scene, file_name(path)))
}

fn file_name(path: &std::path::Path) -> String {
    path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default()
}

pub fn fonts() -> Option<vxr_app::ui::FontBytes> {
    vxr_app::fonts::load(&vxr_app::fonts::dir())
        .map_err(|e| eprintln!("vxr: no UI fonts ({e}); run crates/vxr-app/fetch-fonts. Using egui's fonts."))
        .ok()
}

struct App {
    path: Option<PathBuf>,
    stage: Option<vxr_app::review::Script>,
    state: Option<State>,
    error: Option<String>,
}

struct State {
    window: Arc<Window>,
    surface: wgpu::Surface<'static>,
    config: wgpu::SurfaceConfiguration,
    egui: egui_winit::State,
    editor: Editor,
}

impl App {
    fn start(&mut self, event_loop: &ActiveEventLoop) -> Result<State, String> {
        let (scene, name) = match &self.path {
            Some(p) => open(p)?,
            None => (Scene::new(NEW_SIZE), "Untitled".into()),
        };
        let attrs = Window::default_attributes()
            .with_title(format!("{name} - vxr"))
            .with_inner_size(winit::dpi::LogicalSize::new(1280.0, 800.0))
            .with_min_inner_size(winit::dpi::LogicalSize::new(720.0, 560.0));
        let window = Arc::new(event_loop.create_window(attrs).map_err(|e| e.to_string())?);
        let instance = wgpu::Instance::new(wgpu::InstanceDescriptor::new_with_display_handle(Box::new(
            event_loop.owned_display_handle(),
        )));
        let surface = instance.create_surface(window.clone()).map_err(|e| e.to_string())?;
        let gpu = pollster::block_on(Gpu::new(instance, Some(&surface)))?;
        let caps = surface.get_capabilities(&gpu.adapter);
        // The march writes sRGB-encoded colour and egui blends in gamma space, as CSS does:
        // present to a non-sRGB format.
        let format = caps.formats.iter().copied().find(|f| !f.is_srgb()).unwrap_or(caps.formats[0]);
        let size = window.inner_size();
        let config = wgpu::SurfaceConfiguration {
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
            format,
            width: size.width.max(1),
            height: size.height.max(1),
            present_mode: wgpu::PresentMode::AutoVsync,
            desired_maximum_frame_latency: 1,
            alpha_mode: caps.alpha_modes[0],
            view_formats: vec![],
            color_space: wgpu::SurfaceColorSpace::Auto,
        };
        surface.configure(&gpu.device, &config);
        let ppp = window.scale_factor() as f32;
        let max_tex = gpu.device.limits().max_texture_dimension_2d as usize;
        let mut editor = Editor::new(gpu, scene, &name, format, (config.width, config.height), ppp, fonts())?;
        if let Some(script) = self.stage.take() {
            editor.stage(script);
        }
        let egui =
            egui_winit::State::new(editor.ctx.clone(), egui::ViewportId::ROOT, &window, Some(ppp), None, Some(max_tex));
        Ok(State { window, surface, config, egui, editor })
    }
}

impl State {
    fn redraw(&mut self) {
        let frame = match self.surface.get_current_texture() {
            wgpu::CurrentSurfaceTexture::Success(f) | wgpu::CurrentSurfaceTexture::Suboptimal(f) => f,
            wgpu::CurrentSurfaceTexture::Outdated | wgpu::CurrentSurfaceTexture::Lost => {
                self.surface.configure(&self.editor.gpu.device, &self.config);
                return;
            }
            _ => return,
        };
        let view = frame.texture.create_view(&Default::default());
        let input = self.egui.take_egui_input(&self.window);
        let (actions, platform) = self.editor.frame(input, &view);
        self.window.pre_present_notify();
        self.editor.gpu.queue.present(frame);
        self.egui.handle_platform_output(&self.window, platform);
        for a in actions {
            self.act(a);
        }
    }

    fn act(&mut self, a: Action) {
        let ctx = self.editor.ctx.clone();
        match a {
            Action::New => self.window.set_title("Untitled - vxr"),
            Action::Open => {
                let Some(path) = rfd::FileDialog::new().add_filter("MagicaVoxel", &["vox"]).pick_file() else { return };
                match open(&path).and_then(|(scene, name)| self.editor.load(scene, &name).map(|_| name)) {
                    Ok(name) => {
                        self.window.set_title(&format!("{name} - vxr"));
                        self.editor.ui.toast(&ctx, format!("Opened {name}"));
                        self.editor.path = Some(path);
                    }
                    Err(e) => self.editor.ui.toast(&ctx, format!("Could not open: {e}")),
                }
            }
            Action::Save => {
                let path = match self.editor.path.clone() {
                    Some(p) => p,
                    None => match rfd::FileDialog::new()
                        .add_filter("MagicaVoxel", &["vox"])
                        .set_file_name("scene.vox")
                        .save_file()
                    {
                        Some(p) => p,
                        None => return,
                    },
                };
                match std::fs::write(&path, self.editor.scene.to_vox()) {
                    Ok(()) => {
                        let name = file_name(&path);
                        self.editor.ui.scene_name = name.clone();
                        self.window.set_title(&format!("{name} - vxr"));
                        self.editor.ui.toast(&ctx, format!("Saved {name}"));
                        self.editor.path = Some(path);
                        self.editor.mark_saved();
                    }
                    Err(e) => self.editor.ui.toast(&ctx, format!("Could not save: {e}")),
                }
            }
            Action::Embed => self.editor.ui.toast(&ctx, "The embed export is not built yet"),
            Action::ExportObj => {
                let Some(path) = rfd::FileDialog::new()
                    .add_filter("OBJ", &["obj"])
                    .set_file_name(format!("{}.obj", self.editor.export_stem()))
                    .save_file()
                else {
                    return;
                };
                let stem = path.file_stem().map_or("vxr".into(), |s| s.to_string_lossy().into_owned());
                let dir = path.parent().map(|d| d.to_path_buf()).unwrap_or_default();
                let written = obj_files(&self.editor.scene, &stem).and_then(|(files, quads)| {
                    for (name, bytes) in files {
                        std::fs::write(dir.join(name), bytes).map_err(|e| e.to_string())?;
                    }
                    Ok(quads)
                });
                match written {
                    Ok(quads) => {
                        self.editor.ui.toast(&ctx, format!("Exported {stem}.obj, .mtl and .png ({quads} quads)"))
                    }
                    Err(e) => self.editor.ui.toast(&ctx, format!("Could not export: {e}")),
                }
            }
            Action::ExportSlices => {
                let Some(path) = rfd::FileDialog::new()
                    .add_filter("PNG", &["png"])
                    .set_file_name(self.editor.slices_name())
                    .save_file()
                else {
                    return;
                };
                let written = slices_png(&self.editor.scene).and_then(|(png, size, layers)| {
                    std::fs::write(&path, png).map(|_| (size, layers)).map_err(|e| e.to_string())
                });
                match written {
                    Ok(([w, h], layers)) => {
                        self.editor.ui.toast(&ctx, format!("Exported {} ({layers} layers, {w}×{h})", file_name(&path)))
                    }
                    Err(e) => self.editor.ui.toast(&ctx, format!("Could not export: {e}")),
                }
            }
            Action::Exported => {
                let Some((png, (w, h))) = self.editor.exported.take() else { return };
                let Some(path) = rfd::FileDialog::new()
                    .add_filter("PNG", &["png"])
                    .set_file_name(self.editor.export_name())
                    .save_file()
                else {
                    return;
                };
                match std::fs::write(&path, png) {
                    Ok(()) => self.editor.ui.toast(&ctx, format!("Saved {} ({w}×{h})", file_name(&path))),
                    Err(e) => self.editor.ui.toast(&ctx, format!("Could not save: {e}")),
                }
            }
            _ => {}
        }
    }
}

impl ApplicationHandler for App {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if self.state.is_some() {
            return;
        }
        match self.start(event_loop) {
            Ok(mut s) => {
                s.editor.path = self.path.take();
                s.window.request_redraw();
                self.state = Some(s);
            }
            Err(e) => {
                self.error = Some(e);
                event_loop.exit();
            }
        }
    }

    fn window_event(&mut self, event_loop: &ActiveEventLoop, _id: WindowId, event: WindowEvent) {
        let Some(s) = &mut self.state else { return };
        let response = s.egui.on_window_event(&s.window, &event);
        let ed = &mut s.editor;
        match event {
            WindowEvent::CloseRequested => event_loop.exit(),
            WindowEvent::Resized(size) => {
                s.config.width = size.width.max(1);
                s.config.height = size.height.max(1);
                s.surface.configure(&ed.gpu.device, &s.config);
                ed.resize(s.config.width, s.config.height, s.window.scale_factor() as f32);
            }
            WindowEvent::ScaleFactorChanged { scale_factor, .. } => {
                ed.resize(s.config.width, s.config.height, scale_factor as f32);
            }
            WindowEvent::CursorMoved { position, .. } => ed.pointer_moved(Some([position.x as f32, position.y as f32])),
            WindowEvent::CursorLeft { .. } => ed.pointer_moved(None),
            WindowEvent::MouseInput { state, button, .. } => {
                let b = match button {
                    MouseButton::Left => Button::Left,
                    MouseButton::Right => Button::Right,
                    MouseButton::Middle => Button::Middle,
                    _ => return,
                };
                let over_ui = ed.ctx.is_pointer_over_egui() || ed.ctx.egui_wants_pointer_input();
                ed.button(b, state == ElementState::Pressed, over_ui);
            }
            WindowEvent::MouseWheel { delta, .. } => {
                if !ed.ctx.is_pointer_over_egui() {
                    ed.scroll(match delta {
                        MouseScrollDelta::LineDelta(_, y) => y,
                        MouseScrollDelta::PixelDelta(p) => p.y as f32 / 40.0,
                    });
                }
            }
            // A trackpad pinch (macOS) zooms like the wheel, toward the pointer.
            WindowEvent::PinchGesture { delta, .. } => {
                if !ed.ctx.is_pointer_over_egui() {
                    ed.pinch(delta as f32);
                }
            }
            WindowEvent::KeyboardInput { event, .. } => {
                if event.physical_key == PhysicalKey::Code(KeyCode::Space) && !response.consumed {
                    ed.space = event.state == ElementState::Pressed;
                }
            }
            WindowEvent::RedrawRequested => {
                s.redraw();
                s.window.request_redraw();
            }
            _ => {}
        }
    }
}
