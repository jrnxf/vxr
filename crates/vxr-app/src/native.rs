//! The native window: winit and egui-winit around the [`Editor`], with file dialogs for
//! open and save.

use std::path::PathBuf;
use std::sync::Arc;

use vxr_app::Editor;
use vxr_app::editor::{Button, Finger, NEW_SIZE};
use vxr_app::files::{obj_files, save_atomic, slices_png, vox_bytes};
use vxr_app::theme::ThemeChoice;
use vxr_app::ui::Action;
use vxr_core::Scene;
use vxr_render::Gpu;
use winit::application::ApplicationHandler;
use winit::event::StartCause;
use winit::event::{ElementState, MouseButton, MouseScrollDelta, TouchPhase, WindowEvent};
use winit::event_loop::{ActiveEventLoop, ControlFlow, EventLoop, EventLoopProxy};
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
    let event_loop = EventLoop::<Wake>::with_user_event().build().map_err(|e| e.to_string())?;
    let mut app = App { path, stage, state: None, error: None, proxy: event_loop.create_proxy() };
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

/// Where the app keeps the user's preferences: `VXR_CONFIG_DIR`, else the platform's config
/// folder (Application Support on macOS, `%APPDATA%` on Windows, `$XDG_CONFIG_HOME` or
/// `~/.config` elsewhere), in `vxr`.
fn config_dir() -> Option<PathBuf> {
    let var = |k| std::env::var_os(k).filter(|v| !v.is_empty()).map(PathBuf::from);
    if let Some(d) = var("VXR_CONFIG_DIR") {
        return Some(d);
    }
    let base = if cfg!(target_os = "macos") {
        var("HOME")?.join("Library/Application Support")
    } else if cfg!(windows) {
        var("APPDATA")?
    } else {
        var("XDG_CONFIG_HOME").or_else(|| Some(var("HOME")?.join(".config")))?
    };
    Some(base.join("vxr"))
}

/// The theme the user picked last time (System when none was saved).
fn load_theme() -> ThemeChoice {
    config_dir()
        .and_then(|d| std::fs::read_to_string(d.join("theme")).ok())
        .and_then(|id| ThemeChoice::from_id(&id))
        .unwrap_or_default()
}

fn save_theme(theme: ThemeChoice) -> Result<(), String> {
    let dir = config_dir().ok_or("no config folder")?;
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    std::fs::write(dir.join("theme"), format!("{}\n", theme.id())).map_err(|e| e.to_string())
}

/// Sent to the event loop by the live host's threads when an agent's request arrives.
struct Wake;

struct App {
    proxy: EventLoopProxy<Wake>,
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
    /// Terminal agents' socket, while the user allows them (`vxr mcp --live`).
    #[cfg(unix)]
    live: vxr_app::live::Live,
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
        gpu.report_problems();
        let ppp = window.scale_factor() as f32;
        let max_tex = gpu.device.limits().max_texture_dimension_2d as usize;
        let mut editor = Editor::new(gpu, scene, &name, format, (config.width, config.height), ppp, fonts())?;
        editor.ui.theme = load_theme();
        // The assistant reaches its provider from here, with keys from the OS keychain.
        editor.assistant =
            vxr_app::assistant::Assistant::new(Box::new(crate::net::Net::new()), Box::new(crate::net::Keychain), false);
        if let Some(script) = self.stage.take() {
            editor.stage(script);
        }
        let egui = egui_winit::State::new(
            editor.ctx.clone(),
            egui::ViewportId::ROOT,
            &window,
            Some(ppp),
            window.theme(),
            Some(max_tex),
        );
        Ok(State {
            window,
            surface,
            config,
            egui,
            editor,
            #[cfg(unix)]
            live: {
                let proxy = self.proxy.clone();
                vxr_app::live::Live::new(vxr_mcp::live::dir()).waking(Arc::new(move || _ = proxy.send_event(Wake)))
            },
        })
    }
}

/// How a redraw ended.
enum Drawn {
    /// A frame was presented.
    Yes,
    /// The window has nothing to draw into (minimised or covered): the next draw waits for
    /// the window to be shown again, which arrives as an event.
    Hidden,
    /// The surface was not ready: try again shortly.
    Retry,
}

impl State {
    /// Run what agents asked. Apart from [`State::redraw`], which needs a surface to draw
    /// into, so a hidden window still answers them.
    fn serve_agents(&mut self) {
        loop {
            #[cfg(unix)]
            self.live.tick(&mut self.editor);
            let host = self.editor.take_host_actions();
            if host.is_empty() {
                break;
            }
            for a in host {
                self.act(a);
            }
        }
    }

    fn redraw(&mut self) -> Drawn {
        let frame = match self.surface.get_current_texture() {
            wgpu::CurrentSurfaceTexture::Success(f) | wgpu::CurrentSurfaceTexture::Suboptimal(f) => f,
            wgpu::CurrentSurfaceTexture::Outdated | wgpu::CurrentSurfaceTexture::Lost => {
                self.surface.configure(&self.editor.gpu.device, &self.config);
                return Drawn::Retry;
            }
            wgpu::CurrentSurfaceTexture::Occluded => return Drawn::Hidden,
            _ => return Drawn::Retry,
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
        Drawn::Yes
    }

    fn act(&mut self, a: Action) {
        #[cfg(unix)]
        if self.live.act(&a, &mut self.editor) {
            return;
        }
        let ctx = self.editor.ctx.clone();
        match a {
            Action::New => self.window.set_title("Untitled - vxr"),
            Action::Theme => {
                if let Err(e) = save_theme(self.editor.ui.theme) {
                    self.editor.ui.toast(&ctx, format!("Could not keep the theme: {e}"));
                }
            }
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
                // Through a symbolic link, the file it points at is the one replaced.
                let target = std::fs::canonicalize(&path).unwrap_or_else(|_| path.clone());
                match vox_bytes(&self.editor.scene).and_then(|bytes| save_atomic(&target, &bytes)) {
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
                    for (name, bytes) in &files {
                        std::fs::write(dir.join(name), bytes).map_err(|e| e.to_string())?;
                    }
                    Ok((files[0].0.clone(), quads))
                });
                match written {
                    Ok((obj, quads)) => {
                        self.editor.ui.toast(&ctx, format!("Exported {obj}, .mtl and .png ({quads} quads)"))
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

impl ApplicationHandler<Wake> for App {
    /// Sleep between events, waking for the next one, for an agent's request, or for the
    /// moment [`Editor::repaint_after`] names after a draw.
    fn new_events(&mut self, event_loop: &ActiveEventLoop, cause: StartCause) {
        match cause {
            // Woken early by an event that may not draw: keep the wake already owed.
            StartCause::WaitCancelled { requested_resume: Some(t), .. } => {
                event_loop.set_control_flow(ControlFlow::WaitUntil(t));
            }
            StartCause::ResumeTimeReached { .. } => {
                event_loop.set_control_flow(ControlFlow::Wait);
                if let Some(s) = &self.state {
                    s.window.request_redraw();
                }
            }
            _ => event_loop.set_control_flow(ControlFlow::Wait),
        }
    }

    /// An agent's request arrived: answer it now, whether or not the window can draw.
    fn user_event(&mut self, _: &ActiveEventLoop, _: Wake) {
        let Some(s) = &mut self.state else { return };
        s.serve_agents();
        s.window.request_redraw();
    }

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
        // Input, a resize or the window coming back changes what to draw; an idle window draws
        // nothing (see `RedrawRequested`).
        if !matches!(event, WindowEvent::RedrawRequested) {
            s.window.request_redraw();
        }
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
            // A finger brings the touch layout and its gestures; egui-winit gives one on the UI
            // to egui as the pointer.
            WindowEvent::Touch(t) => {
                ed.set_touch(true);
                let phase = match t.phase {
                    TouchPhase::Started => Finger::Down,
                    TouchPhase::Moved => Finger::Move,
                    TouchPhase::Ended | TouchPhase::Cancelled => Finger::Up,
                };
                ed.touch(t.id, phase, [t.location.x as f32, t.location.y as f32]);
            }
            WindowEvent::MouseInput { state, button, .. } => {
                if state == ElementState::Pressed {
                    ed.set_touch(false);
                }
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
                s.serve_agents();
                let wait = match s.redraw() {
                    Drawn::Yes => s.editor.repaint_after(),
                    Drawn::Hidden => None,
                    Drawn::Retry => Some(std::time::Duration::from_millis(50)),
                };
                match wait {
                    Some(d) if d.is_zero() => s.window.request_redraw(),
                    Some(d) => event_loop.set_control_flow(ControlFlow::WaitUntil(std::time::Instant::now() + d)),
                    None => event_loop.set_control_flow(ControlFlow::Wait),
                }
            }
            _ => {}
        }
    }
}
