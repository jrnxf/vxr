use std::cell::RefCell;
use std::rc::Rc;
use vxr_app::Editor;
use vxr_app::bridge::{Out, Page};
use vxr_app::editor::{Button, Finger, NEW_SIZE};
use vxr_app::files::{obj_files, slices_png, zip};
use vxr_app::theme::ThemeChoice;
use vxr_app::ui::{Action, FontBytes};
use vxr_core::Scene;
use vxr_render::Gpu;

use vxr_app::assistant::{Assistant, Entry, Progress};
use wasm_bindgen::prelude::*;

use crate::net::{Fetch, KeyStore, WebKeys};

fn err(e: impl ToString) -> JsValue {
    JsValue::from_str(&e.to_string())
}

/// The editor on a canvas. Positions from the page are in CSS pixels; the canvas is sized in
/// device pixels (`ppp` device pixels per CSS pixel).
#[wasm_bindgen]
pub struct App {
    editor: Editor,
    /// The assistant's keys in this tab (shared with the assistant's `WebKeys`).
    keys: Rc<RefCell<KeyStore>>,
    surface: wgpu::Surface<'static>,
    config: wgpu::SurfaceConfiguration,
    events: Vec<egui::Event>,
    modifiers: egui::Modifiers,
    cursor: egui::CursorIcon,
    /// Text a field copied or cut in the last frame, for the page to put on the clipboard.
    copied: Option<String>,
    /// The system is in dark mode (`prefers-color-scheme`), for the System theme.
    dark: bool,
    /// Terminal agents through `vxr bridge`; the page's script owns the WebSocket.
    bridge: Page,
}

/// Start the editor on `canvas` with the `.vox` bytes (an empty array opens a new scene).
/// Each font is the file's bytes; if any is empty the UI falls back to egui's fonts.
#[wasm_bindgen]
#[allow(clippy::too_many_arguments)]
pub async fn start(
    canvas: web_sys::HtmlCanvasElement,
    vox: Vec<u8>,
    name: String,
    ppp: f32,
    sans_regular: Vec<u8>,
    sans_medium: Vec<u8>,
    sans_semibold: Vec<u8>,
    mono_regular: Vec<u8>,
    mono_medium: Vec<u8>,
    symbols: Vec<u8>,
    icons: Vec<u8>,
) -> Result<App, JsValue> {
    console_error_panic_hook::set_once();
    let scene = if vox.is_empty() { Scene::new(NEW_SIZE) } else { Scene::from_vox(&vox).map_err(err)? };
    let (width, height) = (canvas.width().max(1), canvas.height().max(1));
    let instance = wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle());
    let surface = instance.create_surface(wgpu::SurfaceTarget::Canvas(canvas)).map_err(err)?;
    let gpu = Gpu::new(instance, Some(&surface)).await?;
    let caps = surface.get_capabilities(&gpu.adapter);
    // As on desktop: the march writes sRGB-encoded colour and egui blends in gamma space.
    let format = caps.formats.iter().copied().find(|f| !f.is_srgb()).unwrap_or(caps.formats[0]);
    let config = wgpu::SurfaceConfiguration {
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
        format,
        width,
        height,
        present_mode: wgpu::PresentMode::Fifo,
        desired_maximum_frame_latency: 1,
        alpha_mode: wgpu::CompositeAlphaMode::Auto,
        view_formats: vec![],
        color_space: wgpu::SurfaceColorSpace::Auto,
    };
    surface.configure(&gpu.device, &config);
    let fonts = [&sans_regular, &sans_medium, &sans_semibold, &mono_regular, &mono_medium, &symbols, &icons]
        .iter()
        .all(|f| !f.is_empty())
        .then_some(FontBytes { sans_regular, sans_medium, sans_semibold, mono_regular, mono_medium, symbols, icons });
    let mut editor = Editor::new(gpu, scene, &name, format, (width, height), ppp, fonts)?;
    // The assistant calls its provider straight from this page, with keys in this tab.
    let keys = Rc::new(RefCell::new(KeyStore::default()));
    editor.assistant = Assistant::new(Box::new(Fetch), Box::new(WebKeys(keys.clone())), true);
    let bridge = Page::new();
    bridge.sync(&mut editor);
    Ok(App {
        editor,
        keys,
        surface,
        config,
        events: Vec::new(),
        modifiers: egui::Modifiers::default(),
        cursor: egui::CursorIcon::Default,
        copied: None,
        dark: true,
        bridge,
    })
}

#[wasm_bindgen]
impl App {
    /// The canvas was resized to `width` x `height` device pixels.
    pub fn resize(&mut self, width: u32, height: u32, ppp: f32) {
        self.config.width = width.max(1);
        self.config.height = height.max(1);
        self.surface.configure(&self.editor.gpu.device, &self.config);
        self.editor.resize(self.config.width, self.config.height, ppp);
    }

    pub fn set_modifiers(&mut self, shift: bool, alt: bool, ctrl: bool, meta: bool) {
        let m = egui::Modifiers { alt, ctrl, shift, mac_cmd: meta, command: meta || ctrl };
        if m != self.modifiers {
            self.modifiers = m;
            self.events.push(egui::Event::ModifiersChanged(m));
        }
    }

    /// The system went dark (true) or light: the System theme follows it.
    pub fn set_system_dark(&mut self, dark: bool) {
        self.dark = dark;
    }

    /// Pick a theme by its id (`system`, `neutral`, `paper`, `gruvbox`, `tokyo-night`,
    /// `kanagawa`, `nord`), as the page kept it. False for an unknown id.
    pub fn set_theme(&mut self, id: &str) -> bool {
        ThemeChoice::from_id(id).map(|c| self.editor.ui.theme = c).is_some()
    }

    /// The picked theme's id, for the page to keep after a "theme" action.
    pub fn theme(&self) -> String {
        self.editor.ui.theme.id().into()
    }

    /// The window lost focus: release held keys so nothing sticks.
    pub fn blur(&mut self) {
        self.set_modifiers(false, false, false, false);
        self.editor.space = false;
        self.events.push(egui::Event::WindowFocused(false));
    }

    /// A point in CSS pixels in egui's points: the same, but in the touch layout's larger ones.
    fn pt(&self, x: f32, y: f32) -> egui::Pos2 {
        let k = self.editor.native_ppp / self.editor.ppp;
        egui::pos2(x * k, y * k)
    }

    pub fn pointer_move(&mut self, x: f32, y: f32) {
        self.events.push(egui::Event::PointerMoved(self.pt(x, y)));
        let ppp = self.editor.native_ppp;
        self.editor.pointer_moved(Some([x * ppp, y * ppp]));
    }

    /// Use the touch layout (a bottom dock, larger targets and touch gestures) or the mouse
    /// layout: the page asks for it on a touch-first screen, a finger, or a mouse press.
    pub fn set_touch(&mut self, on: bool) {
        self.editor.set_touch(on);
    }

    /// Finger `id` touched down (`phase` 0), moved (1) or lifted (2) at (x, y). The view takes
    /// it as a gesture, or egui as the pointer when it landed on the UI.
    pub fn touch(&mut self, id: u32, phase: u8, x: f32, y: f32) {
        self.editor.set_touch(true);
        let phase = match phase {
            0 => Finger::Down,
            1 => Finger::Move,
            _ => Finger::Up,
        };
        let ppp = self.editor.native_ppp;
        if self.editor.touch(id.into(), phase, [x * ppp, y * ppp]) {
            return;
        }
        let pos = self.pt(x, y);
        let button = |pressed| egui::Event::PointerButton {
            pos,
            button: egui::PointerButton::Primary,
            pressed,
            modifiers: egui::Modifiers::NONE,
        };
        self.events.push(egui::Event::PointerMoved(pos));
        match phase {
            Finger::Down => self.events.push(button(true)),
            Finger::Move => {}
            // No hover after a finger lifts.
            Finger::Up => self.events.extend([button(false), egui::Event::PointerGone]),
        }
    }

    pub fn pointer_leave(&mut self) {
        self.events.push(egui::Event::PointerGone);
        self.editor.pointer_moved(None);
    }

    /// A DOM `button` (0 left, 1 middle, 2 right) went down or up at (x, y).
    pub fn pointer_button(&mut self, x: f32, y: f32, button: i16, pressed: bool) {
        let (b, eb) = match button {
            0 => (Button::Left, egui::PointerButton::Primary),
            1 => (Button::Middle, egui::PointerButton::Middle),
            2 => (Button::Right, egui::PointerButton::Secondary),
            _ => return,
        };
        let pos = self.pt(x, y);
        self.events.push(egui::Event::PointerButton { pos, button: eb, pressed, modifiers: self.modifiers });
        let ctx = &self.editor.ctx;
        let over_ui = ctx.is_pointer_over_egui() || ctx.egui_wants_pointer_input();
        self.editor.button(b, pressed, over_ui);
    }

    /// Wheel in pixels (`dy` positive scrolls down). Scrolls a panel under the pointer, or zooms.
    pub fn wheel(&mut self, dx: f32, dy: f32) {
        if self.editor.ctx.is_pointer_over_egui() {
            self.events.push(egui::Event::MouseWheel {
                unit: egui::MouseWheelUnit::Point,
                delta: egui::vec2(-dx, -dy),
                phase: egui::TouchPhase::Move,
                source: egui::MouseWheelSource::Unknown,
                modifiers: self.modifiers,
            });
        } else {
            self.editor.scroll(-dy / 50.0);
        }
    }

    /// A trackpad pinch: `delta` is the change in magnification (positive zooms in). Chrome
    /// and Firefox send it as a wheel with Ctrl held, Safari as gesture events.
    pub fn pinch(&mut self, delta: f32) {
        if !self.editor.ctx.is_pointer_over_egui() {
            self.editor.pinch(delta);
        }
    }

    /// A DOM key event (`KeyboardEvent.key`). Returns true when the page should call
    /// `preventDefault`: egui knows the key, or a text field has focus. While a field has focus
    /// Cmd-C, Cmd-X and Cmd-V return false and reach egui as the browser's copy, cut and paste
    /// events instead (`copy`, `cut`, `paste`): a prevented keydown never fires them.
    pub fn key(&mut self, key: &str, pressed: bool, repeat: bool) -> bool {
        let typing = self.editor.ctx.egui_wants_keyboard_input();
        if typing && self.modifiers.command && ["c", "x", "v"].iter().any(|k| key.eq_ignore_ascii_case(k)) {
            return false;
        }
        if key == " " && !typing {
            self.editor.space = pressed;
        }
        let k = egui::Key::from_name(key);
        if let Some(key) = k {
            self.events.push(egui::Event::Key { key, physical_key: None, pressed, repeat, modifiers: self.modifiers });
        }
        if pressed && !self.modifiers.command && key.chars().count() == 1 {
            self.events.push(egui::Event::Text(key.into()));
        }
        k.is_some() || typing
    }

    /// Text pasted while a text field has focus.
    pub fn paste(&mut self, text: &str) {
        self.events.push(egui::Event::Paste(text.into()));
    }

    /// Copy (`cut` false) or cut the focused text field's selection. The text comes back from
    /// `take_copied` after the next frame.
    pub fn copy(&mut self, cut: bool) {
        self.events.push(if cut { egui::Event::Cut } else { egui::Event::Copy });
    }

    /// The text a field copied or cut in the last frame, for the page's clipboard.
    pub fn take_copied(&mut self) -> Option<String> {
        self.copied.take()
    }

    /// True when (x, y) is on the focused text field, where a right click opens the
    /// browser's menu with Paste.
    pub fn over_field(&self, x: f32, y: f32) -> bool {
        let ctx = &self.editor.ctx;
        ctx.egui_wants_keyboard_input()
            && ctx
                .memory(|m| m.focused())
                .and_then(|id| ctx.read_response(id))
                .is_some_and(|r| r.rect.contains(self.pt(x, y)))
    }

    /// IME composition from the page's hidden text field: `commit` false is a preedit update.
    pub fn ime(&mut self, text: &str, commit: bool) {
        let ev = if commit {
            egui::ImeEvent::Commit(text.into())
        } else {
            egui::ImeEvent::Preedit { text: text.into(), active_range_chars: None }
        };
        self.events.push(egui::Event::Ime(ev));
    }

    /// True while an egui text field has focus (the page then routes typing and IME to it).
    pub fn wants_keyboard(&self) -> bool {
        self.editor.ctx.egui_wants_keyboard_input()
    }

    /// The CSS cursor egui asked for.
    pub fn cursor(&self) -> String {
        use egui::CursorIcon as C;
        match self.cursor {
            C::PointingHand => "pointer",
            C::Text => "text",
            C::Grab => "grab",
            C::Grabbing => "grabbing",
            C::NotAllowed => "not-allowed",
            C::ResizeHorizontal => "ew-resize",
            C::ResizeVertical => "ns-resize",
            _ => "default",
        }
        .into()
    }

    /// Draw a frame at `time` seconds. Returns what the page must do: "open", "save", "embed",
    /// "obj" (download `export_obj`), "slices" (download `export_slices`), "exported" (a render export is ready: `take_export`)
    /// or "theme" (a theme was picked: keep `theme`).
    pub fn frame(&mut self, time: f64) -> Vec<String> {
        let frame = match self.surface.get_current_texture() {
            wgpu::CurrentSurfaceTexture::Success(f) | wgpu::CurrentSurfaceTexture::Suboptimal(f) => f,
            wgpu::CurrentSurfaceTexture::Outdated | wgpu::CurrentSurfaceTexture::Lost => {
                self.surface.configure(&self.editor.gpu.device, &self.config);
                return vec![];
            }
            _ => return vec![],
        };
        let view = frame.texture.create_view(&Default::default());
        let ppp = self.editor.ppp;
        let size = egui::vec2(self.config.width as f32 / ppp, self.config.height as f32 / ppp);
        let mut input = egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, size)),
            time: Some(time),
            focused: true,
            events: std::mem::take(&mut self.events),
            system_theme: Some(if self.dark { egui::Theme::Dark } else { egui::Theme::Light }),
            ..Default::default()
        };
        input.viewports.entry(egui::ViewportId::ROOT).or_default().native_pixels_per_point =
            Some(self.editor.native_ppp);
        self.bridge.tick(&mut self.editor);
        let (actions, platform) = self.editor.frame(input, &view);
        self.editor.gpu.queue.present(frame);
        self.cursor = platform.cursor_icon;
        self.copied = platform.commands.into_iter().rev().find_map(|c| match c {
            egui::OutputCommand::CopyText(text) => Some(text),
            _ => None,
        });
        actions
            .into_iter()
            .filter(|a| !self.bridge.act(a, &mut self.editor))
            .filter_map(|a| match a {
                Action::Open => Some("open".into()),
                Action::Save => Some("save".into()),
                Action::Embed => Some("embed".into()),
                Action::ExportObj => Some("obj".into()),
                Action::ExportSlices => Some("slices".into()),
                Action::Exported => Some("exported".into()),
                Action::Theme => Some("theme".into()),
                _ => None,
            })
            .collect()
    }

    /// Where the page's WebSocket to `vxr bridge` connects.
    pub fn bridge_url(&self) -> String {
        vxr_mcp::bridge::url()
    }

    /// What the page must do with its WebSocket to `vxr bridge`, oldest first: "connect" (open
    /// one at `bridge_url`), "close", or a message to send (a JSON object).
    pub fn bridge_out(&mut self) -> Vec<String> {
        self.bridge
            .take_out()
            .into_iter()
            .map(|o| match o {
                Out::Connect => "connect".into(),
                Out::Close => "close".into(),
                Out::Send(m) => m,
            })
            .collect()
    }

    /// The WebSocket to `vxr bridge` opened.
    pub fn bridge_open(&mut self) {
        self.bridge.opened();
    }

    /// A message from `vxr bridge`. An agent's request runs at once, unless the user is in
    /// the middle of a drag (then the next frame runs it).
    pub fn bridge_message(&mut self, text: &str) {
        self.bridge.message(text, &mut self.editor);
        self.bridge.tick(&mut self.editor);
    }

    /// The WebSocket to `vxr bridge` closed or never opened: `why` is "denied" (the browser's
    /// local network permission is denied), "blocked" (the browser refused to open it),
    /// "safari", "brave" or "" (nothing answered), as the page could tell.
    pub fn bridge_closed(&mut self, why: &str) {
        self.bridge.closed(why, &mut self.editor);
    }

    /// Replace the scene with the `.vox` bytes the page read from a file.
    pub fn open(&mut self, vox: &[u8], name: &str) {
        let ctx = self.editor.ctx.clone();
        match Scene::from_vox(vox).map_err(|e| e.to_string()).and_then(|s| self.editor.load(s, name)) {
            Ok(()) => self.editor.ui.toast(&ctx, format!("Opened {name}")),
            Err(e) => self.editor.ui.toast(&ctx, format!("Could not open: {e}")),
        }
    }

    /// The scene as a `.vox` file, for the page to download; New then has nothing to lose.
    pub fn save(&mut self) -> Vec<u8> {
        self.editor.mark_saved();
        self.editor.scene.to_vox()
    }

    /// The finished render export's PNG (empty if none), for the page to download.
    pub fn take_export(&mut self) -> Vec<u8> {
        self.editor.exported.take().map(|(png, _)| png).unwrap_or_default()
    }

    /// The OBJ export (`.obj`, `.mtl` and palette `.png`) zipped into one download, or empty
    /// with a toast when there is nothing to export.
    pub fn export_obj(&mut self) -> Vec<u8> {
        let ctx = self.editor.ctx.clone();
        match obj_files(&self.editor.scene, &self.editor.export_stem()) {
            Ok((files, _)) => zip(&files),
            Err(e) => {
                self.editor.ui.toast(&ctx, format!("Could not export: {e}"));
                Vec::new()
            }
        }
    }

    /// The file name for the OBJ export's zip.
    pub fn obj_name(&self) -> String {
        format!("{}-obj.zip", self.editor.export_stem())
    }

    /// The PNG slices export, or empty with a toast when there is nothing to export.
    pub fn export_slices(&mut self) -> Vec<u8> {
        let ctx = self.editor.ctx.clone();
        match slices_png(&self.editor.scene) {
            Ok((png, _, _)) => png,
            Err(e) => {
                self.editor.ui.toast(&ctx, format!("Could not export: {e}"));
                Vec::new()
            }
        }
    }

    /// The file name for the PNG slices export.
    pub fn slices_name(&self) -> String {
        self.editor.slices_name()
    }

    /// The file name for the render export.
    pub fn export_name(&self) -> String {
        self.editor.export_name()
    }

    /// The scene's file name.
    pub fn name(&self) -> String {
        self.editor.ui.scene_name.clone()
    }

    /// The editor's state as JSON, for the headless browser check (`crates/vxr-web/check`)
    /// and the console.
    pub fn state(&self) -> String {
        let e = &self.editor;
        let (c, h) = (&e.cam, e.scene.history());
        format!(
            r#"{{"voxels":{},"tool":"{:?}","shape":"{:?}","mirror":{},"mirrorAxes":[{},{},{}],"lock":{},"brushSize":{},"sizePanel":{},"lifting":{},"span":{},"selected":{},"region":"{:?}","hasClip":{},"bounds":[{},{},{}],"boundsPanel":{},"slot":{},"colorEdit":{},"slotOp":"{:?}","undo":{},"redo":{},"cmdk":{},"coach":{},"name":{:?},"size":[{},{}],"ppp":{},"cam":[{},{},{},{},{},{}],"ortho":{},"inView":[{},{}],"cameraHints":{},"view":{},"viewPanel":{},"exportMenu":{},"confirmNew":{},"theme":"{}","themeMenu":{},"touch":{},"docked":{},"paletteOpen":{},"themeShown":{:?},"guides":[{},{},{}],"mode":"{}","objects":{},"models":{},"picked":{},"tree":[{}],"treeOn":{},"layers":[{}],"layerOn":{},"render":{{"shown":{},"spp":{},"rows":{},"restarts":{},"timestamps":{},"panel":{},"full":{},"size":[{},{}],"maxSpp":{},"exposure":{},"gamma":{},"vignette":{},"aperture":{},"sky":{},"physicalSky":{},"export":{}}},"review":{},"undoLabel":{},"asks":{:?},"agents":{{"panel":{},"available":{},"web":{},"allowed":{},"connecting":{},"error":{},"clients":[{}],"edits":[{}]}}}}"#,
            e.scene.map.voxels,
            e.ui.tool,
            e.ui.shape,
            e.ui.mirror,
            e.ui.mirror_on[0],
            e.ui.mirror_on[1],
            e.ui.mirror_on[2],
            e.ui.lock.map_or("null".into(), |a| format!("\"{}\"", ["X", "Y", "Z"][a])),
            e.ui.size,
            e.ui.size_panel,
            e.lifting(),
            e.span().map_or("null".into(), |(l, v)| format!("[\"{l}\",\"{v}\"]")),
            e.scene.selected_count(),
            e.ui.region,
            e.scene.clipboard.is_some(),
            e.scene.bounds().size[0],
            e.scene.bounds().size[1],
            e.scene.bounds().size[2],
            e.ui.bounds_panel.is_some(),
            e.ui.slot,
            e.ui.color_edit.is_some(),
            e.ui.slot_op,
            h.can_undo(),
            h.can_redo(),
            e.ui.cmdk.is_some(),
            e.ui.coach,
            e.ui.scene_name,
            self.config.width,
            self.config.height,
            e.ppp,
            c.target.x,
            c.target.y,
            c.target.z,
            c.yaw,
            c.pitch,
            c.distance,
            c.ortho,
            e.scene_in_view().0,
            e.scene_in_view().1,
            e.ui.camera_hints,
            c.view().map_or("null".into(), |v| format!("\"{}\"", v.name())),
            e.ui.view_panel,
            e.ui.export_menu,
            e.ui.confirm_new,
            e.ui.theme.id(),
            e.ui.theme_menu,
            e.ui.touch,
            e.ui.docked,
            e.ui.palette_open,
            e.ui.shown_theme(&e.ctx).name,
            e.ui.grid,
            e.ui.ground,
            e.ui.frame,
            match e.scene.at() {
                vxr_core::history::At::World => "world",
                vxr_core::history::At::Object(_) => "object",
                vxr_core::history::At::Map => "model",
            },
            e.scene.world().map_or(1, |w| w.objects().len()),
            e.scene.world().map_or(1, |w| w.models.len()),
            e.scene
                .picked()
                .and_then(|i| e.scene.world().map(|w| vxr_app::editor::node_name(w, i)))
                .map_or("null".into(), |n| format!("{n:?}")),
            e.ui.tree
                .iter()
                .map(|r| format!(
                    r#"{{"node":{},"name":{:?},"depth":{},"group":{},"linked":{},"hidden":{},"shown":{},"layer":{}}}"#,
                    r.node, r.name, r.depth, r.group, r.linked, r.hidden, r.shown, r.layer
                ))
                .collect::<Vec<_>>()
                .join(","),
            e.ui.tree_on.map_or("null".into(), |n| n.to_string()),
            e.ui.layers
                .iter()
                .map(|l| format!(
                    r#"{{"id":{},"name":{:?},"objects":{},"hidden":{}}}"#,
                    l.id, l.name, l.objects, l.hidden
                ))
                .collect::<Vec<_>>()
                .join(","),
            e.ui.layer_on.map_or("null".into(), |n| n.to_string()),
            e.ui.render_rect.is_some() && e.ui.show_render,
            e.preview.samples(),
            e.preview.last_rows,
            e.preview.restarts,
            e.gpu.timestamps,
            e.ui.render_panel,
            e.ui.full_render && e.ui.show_render,
            e.preview.width,
            e.preview.height,
            e.preview.max_spp,
            e.preview.lighting.exposure,
            e.preview.lighting.gamma,
            e.preview.lighting.vignette,
            e.preview.lighting.aperture,
            e.preview.lighting.sky_backdrop,
            e.preview.lighting.physical_sky,
            e.export.as_ref().map_or("null".into(), |x| format!("{:?}", x.progress().0)),
            e.ui.review.as_ref().map_or("null".into(), |c| format!(
                r#"{{"prompt":{:?},"summary":{},"counts":[{},{},{}],"slots":[{}],"steps":[{}],"held":{},"before":{}}}"#,
                c.prompt,
                c.summary.as_ref().map_or("null".into(), |s| format!("{s:?}")),
                c.counts[0],
                c.counts[1],
                c.counts[2],
                c.slots.iter().map(|s| s.slot.to_string()).collect::<Vec<_>>().join(","),
                c.steps
                    .iter()
                    .map(|s| format!(r#"{{"tool":"{}","ok":{},"dropped":{}}}"#, s.tool, s.ok, s.dropped))
                    .collect::<Vec<_>>()
                    .join(","),
                c.held.map_or("null".into(), |(a, m)| format!("[{a},{m}]")),
                c.before,
            )),
            h.undo_label().map_or("null".into(), |l| format!("{l:?}")),
            e.asks,
            e.ui.agents_panel,
            e.ui.agents.available,
            e.ui.agents.web,
            e.ui.agents.allowed,
            e.ui.agents.connecting,
            e.ui.agents.error.as_ref().map_or("null".into(), |m| format!("{m:?}")),
            e.ui.agents.clients.iter().map(|(c, n)| format!("[{c},{n:?}]")).collect::<Vec<_>>().join(","),
            e.ui.agents
                .edits
                .iter()
                .map(|r| format!(r#"{{"label":{:?},"applied":{}}}"#, r.label, r.applied))
                .collect::<Vec<_>>()
                .join(","),
        )
    }

    /// Run a scripted AI request through the review step (see `vxr_app::review::Script`):
    /// the card and the ghost show it until Apply or Discard. Returns each call's answer by
    /// turn as JSON (`{"ok": text}`, `{"error": text}` or `"held"`), or the script's error.
    pub fn stage(&mut self, script: &str) -> Result<String, JsValue> {
        use vxr_app::review::Answer;
        let turns = self.editor.stage_json(script).map_err(err)?;
        let answer = |a: &Answer| match a {
            Answer::Ok(t) => format!(r#"{{"ok":{t:?}}}"#),
            Answer::Err(t) => format!(r#"{{"error":{t:?}}}"#),
            Answer::Held => r#""held""#.into(),
        };
        let turns: Vec<String> =
            turns.iter().map(|t| format!("[{}]", t.iter().map(answer).collect::<Vec<_>>().join(","))).collect();
        Ok(format!("[{}]", turns.join(",")))
    }

    /// The RGBA colour of palette slot `slot` and how many voxels use it.
    pub fn color(&self, slot: u8) -> Vec<u32> {
        let c = self.editor.scene.palette[slot as usize];
        vec![c[0] as u32, c[1] as u32, c[2] as u32, c[3] as u32, self.editor.scene.map.counts[slot as usize]]
    }

    /// The material of palette slot `slot`: type (0 diffuse, 1 metal, 2 glass, 3 emit,
    /// 4 plastic), roughness, metalness, refractive index, transparency, emission, power,
    /// specular and attenuation.
    pub fn material(&self, slot: u8) -> Vec<f32> {
        let m = self.editor.scene.materials[slot as usize];
        let kind = vxr_core::material::Kind::ALL.iter().position(|&k| k == m.kind).unwrap_or(0);
        vec![kind as f32, m.rough, m.metal, m.ior, m.trans, m.emit, m.flux, m.spec, m.att]
    }

    /// The middle of a named part of the UI in CSS pixels (see `Hybrid::hits`), or an empty
    /// array if it is not on screen. For the browser check.
    pub fn hit(&self, name: &str) -> Vec<f32> {
        let k = self.editor.ppp / self.editor.native_ppp;
        self.editor.ui.hits.get(name).map(|r| vec![r.center().x * k, r.center().y * k]).unwrap_or_default()
    }

    /// A key the page read back from the browser's encrypted store (`vxrKeys.load`) at start:
    /// `kind` is "anthropic", "openai" or "compatible".
    pub fn restore_key(&mut self, kind: &str, key: &str) {
        if let Some(k) = vxr_ai::Kind::from_id(kind) {
            self.keys.borrow_mut().restore(k, key);
            self.editor.assistant.keys_changed();
        }
    }

    /// The assistant's state as JSON, for the browser check: the panel, the conversation's
    /// answers, the model and which keys are kept (never the keys).
    pub fn ai(&mut self) -> String {
        let e = &mut self.editor;
        let a = &mut e.assistant;
        let kinds = vxr_ai::Kind::ALL;
        let has: Vec<String> = kinds.iter().map(|&k| a.has_key(k).to_string()).collect();
        let remembered: Vec<String> = kinds.iter().map(|&k| a.keys.remembered(k).to_string()).collect();
        let answers: Vec<String> = a
            .entries
            .iter()
            .filter_map(|en| match en {
                Entry::Answer(v) => Some(format!(
                    r#"{{"progress":{:?},"text":{:?},"calls":[{}],"model":{:?},"cost":{}}}"#,
                    match &v.progress {
                        Progress::Working(w) => format!("working: {w}"),
                        Progress::Done => "done".into(),
                        Progress::Stopped => "stopped".into(),
                        Progress::Failed(f) => format!("failed: {f}"),
                    },
                    v.text,
                    v.calls.iter().map(|c| format!("{:?}", c.tool)).collect::<Vec<_>>().join(","),
                    v.model,
                    v.cost.map_or("null".into(), |c| format!("{c}")),
                )),
                Entry::User(_) => None,
            })
            .collect();
        format!(
            r#"{{"open":{},"setup":{},"running":{},"held":{},"stopAsk":{},"kind":{:?},"model":{:?},"base":{:?},"hasKey":[{}],"remembered":[{}],"entries":{},"answers":[{}],"askBar":{},"spent":{}}}"#,
            e.ui.ai_open,
            e.ui.ai_setup,
            a.running(),
            a.held(),
            a.stop_ask,
            a.settings.kind.id(),
            a.settings.model,
            a.settings.base,
            has.join(","),
            remembered.join(","),
            a.entries.len(),
            answers.join(","),
            e.ui.ask_bar.as_ref().map_or("null".into(), |b| format!(r#"{{"text":{:?},"sent":{}}}"#, b.text, b.sent)),
            a.spent,
        )
    }

    /// Show a short message in the editor.
    pub fn toast(&mut self, msg: &str) {
        let ctx = self.editor.ctx.clone();
        self.editor.ui.toast(&ctx, msg);
    }
}
