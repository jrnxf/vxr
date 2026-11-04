//! The native window: winit and egui-winit around the [`Editor`], with file dialogs for
//! open and save, the projects list and drafts kept in the config folder, and the account's
//! library on the vxr Worker (`VXR_SERVER`, else [`SERVER`]) with its session in the OS
//! keychain.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};

use vxr_app::editor::{Button, Finger, NEW_SIZE};
use vxr_app::files::{File, save_atomic};
use vxr_app::host::{Drawn, Host, Opened, Platform, Saved, run_action};
use vxr_app::projects::{Folder, Project, Projects, modified};
use vxr_app::sync::Library;
use vxr_app::theme::ThemeChoice;
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

fn save_theme(id: &str) -> Result<(), String> {
    let dir = config_dir().ok_or("no config folder")?;
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    std::fs::write(dir.join("theme"), format!("{id}\n")).map_err(|e| e.to_string())
}

/// The vxr Worker the account and its library are on.
pub const SERVER: &str = "https://vxr.jrnxf.co";

/// How often the library is asked again while the window stays open (it is also asked at
/// launch and whenever the window comes to the front).
const CHECK_EVERY: Duration = Duration::from_secs(5 * 60);

/// The Worker to sign in to: `VXR_SERVER` (a local `wrangler dev`, say), else [`SERVER`].
fn server() -> String {
    std::env::var("VXR_SERVER").ok().filter(|s| !s.is_empty()).unwrap_or_else(|| SERVER.into())
}

/// Sent to the event loop by the live host's and the library's threads when an agent's
/// request or a library reply arrives, and by a timer for the next library check.
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
    egui: egui_winit::State,
    host: Host,
    desktop: Desktop,
    /// Sends the library's calls to the Worker.
    courier: vxr_app::courier::Courier,
    /// When the library was last asked who is signed in.
    checked: Instant,
}

/// What a desktop does for [`run_action`]: file dialogs and files, the window's title, and
/// the account's session in the keychain.
struct Desktop {
    window: Arc<Window>,
    session: crate::net::Session,
    /// Terminal agents' socket, while the user allows them (`vxr mcp --live`).
    #[cfg(unix)]
    live: vxr_app::live::Live,
}

impl Platform for Desktop {
    fn verb(&self) -> &'static str {
        "Saved"
    }

    fn pick_open(&mut self) -> Result<Option<Opened>, String> {
        let Some(path) = rfd::FileDialog::new().add_filter("MagicaVoxel", &["vox"]).pick_file() else {
            return Ok(None);
        };
        read(&path).map(Some)
    }

    fn open_project(&mut self, project: &Project) -> Result<Option<Opened>, String> {
        read(project.path.as_deref().ok_or("it has no file")?).map(Some)
    }

    fn save_vox(
        &mut self,
        current: Option<&Path>,
        _project: &str,
        _name: &str,
        bytes: &[u8],
    ) -> Result<Option<Saved>, String> {
        let path = match current {
            Some(p) => p.to_path_buf(),
            None => {
                match rfd::FileDialog::new().add_filter("MagicaVoxel", &["vox"]).set_file_name("scene.vox").save_file()
                {
                    Some(p) => p,
                    None => return Ok(None),
                }
            }
        };
        // Through a symbolic link, the file it points at is the one replaced.
        let target = std::fs::canonicalize(&path).unwrap_or_else(|_| path.clone());
        save_atomic(&target, bytes)?;
        Ok(Some(Saved { name: file_name(&path), path: Some(path), ..Default::default() }))
    }

    fn export_vox(&mut self, name: &str, bytes: &[u8]) -> Result<Option<String>, String> {
        let Some(path) = rfd::FileDialog::new().add_filter("MagicaVoxel", &["vox"]).set_file_name(name).save_file()
        else {
            return Ok(None);
        };
        save_atomic(&path, bytes)?;
        Ok(Some(file_name(&path)))
    }

    fn save_png(&mut self, name: &str, bytes: &[u8]) -> Result<Option<String>, String> {
        let Some(path) = rfd::FileDialog::new().add_filter("PNG", &["png"]).set_file_name(name).save_file() else {
            return Ok(None);
        };
        std::fs::write(&path, bytes).map_err(|e| e.to_string())?;
        Ok(Some(file_name(&path)))
    }

    fn save_all(
        &mut self,
        stem: &str,
        make: &mut dyn FnMut(&str) -> Result<Vec<File>, String>,
    ) -> Result<Option<String>, String> {
        let Some(path) =
            rfd::FileDialog::new().add_filter("OBJ", &["obj"]).set_file_name(format!("{stem}.obj")).save_file()
        else {
            return Ok(None);
        };
        let stem = path.file_stem().map_or("vxr".into(), |s| s.to_string_lossy().into_owned());
        let dir = path.parent().map(|d| d.to_path_buf()).unwrap_or_default();
        let files = make(&stem)?;
        for (name, bytes) in &files {
            std::fs::write(dir.join(name), bytes).map_err(|e| e.to_string())?;
        }
        Ok(files.first().map(|f| f.0.clone()))
    }

    fn persist_theme(&mut self, id: &str) -> Result<(), String> {
        save_theme(id)
    }

    fn named(&mut self, name: &str) {
        self.window.set_title(&format!("{name} - vxr"));
    }

    fn keep_session(&mut self, token: Option<&str>) {
        if let Err(e) = self.session.set(token) {
            eprintln!("vxr: could not keep the sign-in in the keychain: {e}");
        }
    }

    #[cfg(unix)]
    fn intercept(&mut self, action: &vxr_app::ui::Action, editor: &mut vxr_app::Editor) -> bool {
        self.live.act(action, editor)
    }
}

/// The file at `path` to open, with when it last changed.
fn read(path: &Path) -> Result<Opened, String> {
    let bytes = std::fs::read(path).map_err(|e| format!("{}: {e}", path.display()))?;
    Ok(Opened { name: file_name(path), bytes, path: Some(path.into()), modified: modified(path), ..Default::default() })
}

impl App {
    fn start(&mut self, event_loop: &ActiveEventLoop) -> Result<State, String> {
        // The projects list knows a file by where it is, wherever it was opened from.
        self.path = self.path.take().map(|p| std::path::absolute(&p).unwrap_or(p));
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
        let max_tex = gpu.device.limits().max_texture_dimension_2d as usize;
        let ppp = window.scale_factor() as f32;
        let size = window.inner_size();
        let mut host = Host::new(
            gpu,
            surface,
            (size.width, size.height),
            wgpu::PresentMode::AutoVsync,
            scene,
            &name,
            ppp,
            fonts(),
        )?;
        let editor = &mut host.editor;
        editor.ui.theme = load_theme();
        if let Some(dir) = config_dir() {
            editor.projects = Projects::load(Box::new(Folder::new(dir.join("projects"))));
        }
        editor.path = self.path.take();
        match &editor.path {
            Some(p) => _ = editor.projects.opened(&name, Some(p), None, modified(p), false, 0),
            None => vxr_app::host::launch(editor),
        }
        // The account: signed in with the session kept in the keychain, if any. Who is signed
        // in, and the library, come with the first answer.
        let server = server();
        let session = crate::net::Session::new(&server, SERVER);
        editor.library = Library::native(session.get());
        editor.library.check();
        let courier = {
            let proxy = self.proxy.clone();
            vxr_app::courier::Courier::new(&server, Arc::new(move || _ = proxy.send_event(Wake)))
        };
        {
            let proxy = self.proxy.clone();
            std::thread::spawn(move || {
                loop {
                    std::thread::sleep(CHECK_EVERY);
                    if proxy.send_event(Wake).is_err() {
                        return;
                    }
                }
            });
        }
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
        let desktop = Desktop {
            window: window.clone(),
            session,
            #[cfg(unix)]
            live: {
                let proxy = self.proxy.clone();
                vxr_app::live::Live::new(vxr_mcp::live::dir()).waking(Arc::new(move || _ = proxy.send_event(Wake)))
            },
        };
        Ok(State { window, egui, host, desktop, courier, checked: Instant::now() })
    }
}

impl State {
    /// Hand the library's replies to the editor and send its new calls; ask who is signed in
    /// again once [`CHECK_EVERY`] has passed.
    fn sync(&mut self) {
        let ed = &mut self.host.editor;
        for (id, reply) in self.courier.replies() {
            vxr_app::host::fetched(ed, &mut self.desktop, id, reply);
        }
        if self.checked.elapsed() >= CHECK_EVERY {
            self.check();
        }
        self.courier.send(self.host.editor.library.take_out());
    }

    /// Ask who is signed in, and so the library's list.
    fn check(&mut self) {
        self.checked = Instant::now();
        self.host.editor.library.check();
    }

    /// Run what agents asked. Apart from [`State::redraw`], which needs a surface to draw
    /// into, so a hidden window still answers them.
    fn serve_agents(&mut self) {
        loop {
            #[cfg(unix)]
            self.desktop.live.tick(&mut self.host.editor);
            let host = self.host.editor.take_host_actions();
            if host.is_empty() {
                break;
            }
            for a in host {
                run_action(&mut self.host.editor, &mut self.desktop, a);
            }
        }
    }

    /// Draw a frame and run what it asked of the host. Returns how long to wait before the
    /// next draw: none when only an event can change the view.
    fn redraw(&mut self) -> Option<std::time::Duration> {
        let Self { window, egui, host, desktop, .. } = self;
        match host.frame(|_| egui.take_egui_input(window), || window.pre_present_notify()) {
            Drawn::Yes(actions, platform) => {
                egui.handle_platform_output(window, *platform);
                for a in actions {
                    run_action(&mut host.editor, desktop, a);
                }
                host.editor.repaint_after()
            }
            // Nothing to draw into: the next draw waits for the window to be shown again,
            // which arrives as an event.
            Drawn::Hidden => None,
            Drawn::Retry => Some(std::time::Duration::from_millis(50)),
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

    /// An agent's request or a library reply arrived (or the next library check is due):
    /// answer it now, whether or not the window can draw.
    fn user_event(&mut self, _: &ActiveEventLoop, _: Wake) {
        let Some(s) = &mut self.state else { return };
        s.sync();
        s.serve_agents();
        s.window.request_redraw();
    }

    /// Closing by any way the close confirm did not see (Cmd-Q, the app menu's Quit, a
    /// logout) keeps the newest edits as a draft, as a crash would leave them.
    fn exiting(&mut self, _: &ActiveEventLoop) {
        if let Some(s) = &mut self.state {
            s.host.editor.flush_draft();
        }
    }

    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if self.state.is_some() {
            return;
        }
        match self.start(event_loop) {
            Ok(s) => {
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
        let ed = &mut s.host.editor;
        match event {
            // With edits not saved the close confirm asks first; its Save or Don't save set
            // `quit`, which the next draw sees.
            WindowEvent::CloseRequested if ed.is_unsaved() => {
                ed.ui.cmdk = None;
                ed.ui.confirm_close = true;
            }
            WindowEvent::CloseRequested => event_loop.exit(),
            // Back at the front: another device may have changed the library meanwhile.
            WindowEvent::Focused(true) => s.check(),
            WindowEvent::Resized(size) => s.host.resize(size.width, size.height, s.window.scale_factor() as f32),
            WindowEvent::ScaleFactorChanged { scale_factor, .. } => s.host.rescale(scale_factor as f32),
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
                s.host.button(b, state == ElementState::Pressed);
            }
            WindowEvent::MouseWheel { delta, .. } => s.host.scroll(match delta {
                MouseScrollDelta::LineDelta(_, y) => y,
                MouseScrollDelta::PixelDelta(p) => p.y as f32 / 40.0,
            }),
            // A trackpad pinch (macOS) zooms like the wheel, toward the pointer.
            WindowEvent::PinchGesture { delta, .. } => s.host.pinch(delta as f32),
            WindowEvent::KeyboardInput { event, .. } => {
                if event.physical_key == PhysicalKey::Code(KeyCode::Space) && !response.consumed {
                    s.host.editor.space = event.state == ElementState::Pressed;
                }
            }
            WindowEvent::RedrawRequested => {
                s.serve_agents();
                let wait = s.redraw();
                // What the frame asked of the library (a save, a sign-in) goes now.
                s.sync();
                if s.host.editor.quit {
                    return event_loop.exit();
                }
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
