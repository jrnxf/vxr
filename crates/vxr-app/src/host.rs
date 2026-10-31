//! What the native window and the browser page share around the [`Editor`]: the surface and
//! how a frame is drawn into it, where a pointer press goes (the UI or the view), and what
//! each [`Action`] that touches files does ([`run_action`]).
//!
//! A host (`native.rs`, `vxr-web`) only translates its events into calls here and provides a
//! [`Platform`]: how this machine picks a file to open, writes one out and remembers the
//! theme. Open, Save and the exports are written once, so they cannot drift apart: a save
//! is refused when the scene would not read back whole before any dialog or download starts.

use std::path::{Path, PathBuf};

use vxr_core::Scene;
use vxr_render::Gpu;

use crate::Editor;
use crate::editor::Button;
use crate::files::{File, obj_files, slices_png, vox_bytes};
use crate::ui::{Action, FontBytes};

/// A scene file the user picked to open.
pub struct Opened {
    pub name: String,
    pub bytes: Vec<u8>,
    /// Where it came from, when the host has files (not in the browser).
    pub path: Option<PathBuf>,
}

/// A `.vox` file the platform wrote.
pub struct Saved {
    /// The file's name, which the scene takes.
    pub name: String,
    /// Where it went, when the host has files: Save then replaces it without asking.
    pub path: Option<PathBuf>,
}

/// What differs between this machine's hosts. Every method that writes returns `Ok(None)`
/// when the user cancelled.
pub trait Platform {
    /// "Saved" where files are written, "Downloaded" where the browser takes them.
    fn verb(&self) -> &'static str;

    /// Ask for a scene to open. A browser cannot answer at once: it opens its file picker,
    /// returns `Ok(None)`, and the page calls [`open`] with the file later.
    fn pick_open(&mut self) -> Result<Option<Opened>, String>;

    /// Write the scene's `.vox` bytes: over `current` when there is one, else wherever the
    /// user chooses (suggesting `name`).
    fn save_vox(&mut self, current: Option<&Path>, name: &str, bytes: &[u8]) -> Result<Option<Saved>, String>;

    /// Write one PNG, suggesting `name`; returns the name it was given.
    fn save_png(&mut self, name: &str, bytes: &[u8]) -> Result<Option<String>, String>;

    /// Write several files built from a name stem (suggesting `stem`): `make` builds them
    /// for the stem the user ended up choosing. Returns the main file's name.
    fn save_all(
        &mut self,
        stem: &str,
        make: &mut dyn FnMut(&str) -> Result<Vec<File>, String>,
    ) -> Result<Option<String>, String>;

    /// Keep the picked theme (by its id) for the next start.
    fn persist_theme(&mut self, id: &str) -> Result<(), String>;

    /// The scene now has this name (the window's title).
    fn named(&mut self, _name: &str) {}

    /// A request only this platform answers (an agent's), run before the action itself;
    /// true when it took the action.
    fn intercept(&mut self, _action: &Action, _editor: &mut Editor) -> bool {
        false
    }
}

/// Run what the UI asked of the host. Actions the editor already ran itself do nothing here.
pub fn run_action(ed: &mut Editor, p: &mut dyn Platform, action: Action) {
    if p.intercept(&action, ed) {
        return;
    }
    let ctx = ed.ctx.clone();
    let toast = |ed: &mut Editor, text: String| ed.ui.toast(&ctx, text);
    match action {
        Action::New => p.named("Untitled"),
        Action::Theme => {
            if let Err(e) = p.persist_theme(ed.ui.theme.id()) {
                toast(ed, format!("Could not keep the theme: {e}"));
            }
        }
        Action::Open => match p.pick_open() {
            Ok(Some(file)) => open(ed, p, file),
            Ok(None) => {}
            Err(e) => toast(ed, format!("Could not open: {e}")),
        },
        Action::Save => {
            // Checked before any dialog or download: a scene that would not read back whole
            // is never offered for saving.
            let bytes = match vox_bytes(&ed.scene) {
                Ok(b) => b,
                Err(e) => return toast(ed, format!("Could not save: {e}")),
            };
            let name = ed.ui.scene_name.clone();
            match p.save_vox(ed.path.as_deref(), &name, &bytes) {
                Ok(Some(saved)) => {
                    ed.ui.scene_name = saved.name.clone();
                    p.named(&saved.name);
                    toast(ed, format!("{} {}", p.verb(), saved.name));
                    ed.path = saved.path;
                    ed.mark_saved();
                }
                Ok(None) => {}
                Err(e) => toast(ed, format!("Could not save: {e}")),
            }
        }
        Action::Embed => toast(ed, "The embed export is not built yet".into()),
        Action::ExportObj => {
            let mut quads = 0;
            let scene = &ed.scene;
            let made = p.save_all(&ed.export_stem(), &mut |stem| {
                obj_files(scene, stem).map(|(files, q)| {
                    quads = q;
                    files
                })
            });
            match made {
                Ok(Some(name)) => toast(ed, format!("{} {name} ({quads} quads)", p.verb())),
                Ok(None) => {}
                Err(e) => toast(ed, format!("Could not export: {e}")),
            }
        }
        Action::ExportSlices => {
            let written = slices_png(&ed.scene)
                .and_then(|(png, size, layers)| Ok((p.save_png(&ed.slices_name(), &png)?, size, layers)));
            match written {
                Ok((Some(name), [w, h], layers)) => {
                    toast(ed, format!("{} {name} ({layers} layers, {w}×{h})", p.verb()))
                }
                Ok((None, ..)) => {}
                Err(e) => toast(ed, format!("Could not export: {e}")),
            }
        }
        Action::Exported => {
            let Some((png, (w, h))) = ed.exported.take() else { return };
            match p.save_png(&ed.export_name(), &png) {
                Ok(Some(name)) => toast(ed, format!("{} {name} ({w}×{h})", p.verb())),
                Ok(None) => {}
                Err(e) => toast(ed, format!("Could not save: {e}")),
            }
        }
        _ => {}
    }
}

/// Replace the scene with a file the user picked, or say why not.
pub fn open(ed: &mut Editor, p: &mut dyn Platform, file: Opened) {
    let ctx = ed.ctx.clone();
    let Opened { name, bytes, path } = file;
    match Scene::from_vox(&bytes).map_err(|e| format!("{name}: {e}")).and_then(|s| ed.load(s, &name)) {
        Ok(()) => {
            p.named(&name);
            ed.ui.toast(&ctx, format!("Opened {name}"));
            ed.path = path;
        }
        Err(e) => ed.ui.toast(&ctx, format!("Could not open: {e}")),
    }
}

/// How a frame ended.
pub enum Drawn {
    /// A frame was presented; these are what the UI asked of the host.
    Yes(Vec<Action>, Box<egui::PlatformOutput>),
    /// The window has nothing to draw into (minimised or covered): the next draw waits for
    /// the window to be shown again.
    Hidden,
    /// The surface was not ready: try again shortly.
    Retry,
}

/// The editor and the surface it draws to.
pub struct Host {
    pub editor: Editor,
    surface: wgpu::Surface<'static>,
    config: wgpu::SurfaceConfiguration,
}

impl Host {
    /// Configure `surface` (`gpu` was made for it) at `size` pixels and start the editor on
    /// `scene`. The scene is drawn as sRGB-encoded colour and egui blends in gamma space, as
    /// CSS does, so the surface is a non-sRGB format.
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        gpu: Gpu,
        surface: wgpu::Surface<'static>,
        size: (u32, u32),
        present_mode: wgpu::PresentMode,
        scene: Scene,
        name: &str,
        ppp: f32,
        fonts: Option<FontBytes>,
    ) -> Result<Self, String> {
        let caps = surface.get_capabilities(&gpu.adapter);
        let format = caps.formats.iter().copied().find(|f| !f.is_srgb()).unwrap_or(caps.formats[0]);
        let config = wgpu::SurfaceConfiguration {
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
            format,
            width: size.0.max(1),
            height: size.1.max(1),
            present_mode,
            desired_maximum_frame_latency: 1,
            alpha_mode: caps.alpha_modes[0],
            view_formats: vec![],
            color_space: wgpu::SurfaceColorSpace::Auto,
        };
        surface.configure(&gpu.device, &config);
        gpu.report_problems();
        let editor = Editor::new(gpu, scene, name, format, (config.width, config.height), ppp, fonts)?;
        Ok(Self { editor, surface, config })
    }

    /// The surface is `width` x `height` pixels, at `ppp` pixels per point.
    pub fn resize(&mut self, width: u32, height: u32, ppp: f32) {
        self.config.width = width.max(1);
        self.config.height = height.max(1);
        self.surface.configure(&self.editor.gpu.device, &self.config);
        self.editor.resize(self.config.width, self.config.height, ppp);
    }

    /// The display's pixels per point changed; the surface did not.
    pub fn rescale(&mut self, ppp: f32) {
        self.editor.resize(self.config.width, self.config.height, ppp);
    }

    /// The surface's size in pixels.
    pub fn size(&self) -> (u32, u32) {
        (self.config.width, self.config.height)
    }

    /// Draw a frame. `input` runs once a texture to draw into is in hand, so input waiting
    /// for a frame is not lost when none is; it gets the editor for what must happen just
    /// before the frame. `before_present` runs between the draw and the present.
    pub fn frame(&mut self, input: impl FnOnce(&mut Editor) -> egui::RawInput, before_present: impl FnOnce()) -> Drawn {
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
        let input = input(&mut self.editor);
        let (actions, output) = self.editor.frame(input, &view);
        before_present();
        self.editor.gpu.queue.present(frame);
        Drawn::Yes(actions, Box::new(output))
    }

    /// The pointer is over something egui draws, so a wheel or pinch there is the UI's.
    pub fn over_ui(&self) -> bool {
        self.editor.ctx.is_pointer_over_egui()
    }

    /// A press or release goes to the UI when the pointer is over it or egui is holding the
    /// pointer (a drag that began there), else to the view.
    pub fn button(&mut self, button: Button, pressed: bool) {
        let ctx = &self.editor.ctx;
        let over_ui = ctx.is_pointer_over_egui() || ctx.egui_wants_pointer_input();
        self.editor.button(button, pressed, over_ui);
    }

    /// Zoom by wheel `lines`, unless the pointer is over the UI.
    pub fn scroll(&mut self, lines: f32) {
        if !self.over_ui() {
            self.editor.scroll(lines);
        }
    }

    /// Zoom by a trackpad pinch's `delta`, unless the pointer is over the UI.
    pub fn pinch(&mut self, delta: f32) {
        if !self.over_ui() {
            self.editor.pinch(delta);
        }
    }
}
