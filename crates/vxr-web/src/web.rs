use vxr_app::Editor;
use vxr_app::editor::{Button, NEW_SIZE};
use vxr_app::ui::{Action, FontBytes};
use vxr_core::Scene;
use vxr_render::Gpu;
use wasm_bindgen::prelude::*;

fn err(e: impl ToString) -> JsValue {
    JsValue::from_str(&e.to_string())
}

/// The editor on a canvas. Positions from the page are in CSS pixels; the canvas is sized in
/// device pixels (`ppp` device pixels per CSS pixel).
#[wasm_bindgen]
pub struct App {
    editor: Editor,
    surface: wgpu::Surface<'static>,
    config: wgpu::SurfaceConfiguration,
    events: Vec<egui::Event>,
    modifiers: egui::Modifiers,
    cursor: egui::CursorIcon,
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
    inter_regular: Vec<u8>,
    inter_medium: Vec<u8>,
    inter_semibold: Vec<u8>,
    mono_regular: Vec<u8>,
    mono_medium: Vec<u8>,
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
    let fonts = [&inter_regular, &inter_medium, &inter_semibold, &mono_regular, &mono_medium, &icons]
        .iter()
        .all(|f| !f.is_empty())
        .then_some(FontBytes { inter_regular, inter_medium, inter_semibold, mono_regular, mono_medium, icons });
    let editor = Editor::new(gpu, scene, &name, format, (width, height), ppp, fonts)?;
    Ok(App {
        editor,
        surface,
        config,
        events: Vec::new(),
        modifiers: egui::Modifiers::default(),
        cursor: egui::CursorIcon::Default,
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

    /// The window lost focus: release held keys so nothing sticks.
    pub fn blur(&mut self) {
        self.set_modifiers(false, false, false, false);
        self.editor.space = false;
        self.events.push(egui::Event::WindowFocused(false));
    }

    pub fn pointer_move(&mut self, x: f32, y: f32) {
        self.events.push(egui::Event::PointerMoved(egui::pos2(x, y)));
        let ppp = self.editor.ppp;
        self.editor.pointer_moved(Some([x * ppp, y * ppp]));
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
        let pos = egui::pos2(x, y);
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

    /// A DOM key event (`KeyboardEvent.key`). Returns true when the page should call
    /// `preventDefault`: egui knows the key, or a text field has focus.
    pub fn key(&mut self, key: &str, pressed: bool, repeat: bool) -> bool {
        let typing = self.editor.ctx.egui_wants_keyboard_input();
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

    /// Draw a frame at `time` seconds. Returns what the page must do: "open", "save", "embed"
    /// or "exported" (a render export is ready: `take_export`).
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
            ..Default::default()
        };
        input.viewports.entry(egui::ViewportId::ROOT).or_default().native_pixels_per_point = Some(ppp);
        let (actions, platform) = self.editor.frame(input, &view);
        self.editor.gpu.queue.present(frame);
        self.cursor = platform.cursor_icon;
        actions
            .into_iter()
            .filter_map(|a| match a {
                Action::Open => Some("open".into()),
                Action::Save => Some("save".into()),
                Action::Embed => Some("embed".into()),
                Action::Exported => Some("exported".into()),
                _ => None,
            })
            .collect()
    }

    /// Replace the scene with the `.vox` bytes the page read from a file.
    pub fn open(&mut self, vox: &[u8], name: &str) {
        let ctx = self.editor.ctx.clone();
        match Scene::from_vox(vox).map_err(|e| e.to_string()).and_then(|s| self.editor.load(s, name)) {
            Ok(()) => self.editor.ui.toast(&ctx, format!("Opened {name}")),
            Err(e) => self.editor.ui.toast(&ctx, format!("Could not open: {e}")),
        }
    }

    /// The scene as a `.vox` file, for the page to download.
    pub fn save(&mut self) -> Vec<u8> {
        self.editor.scene.to_vox()
    }

    /// The finished render export's PNG (empty if none), for the page to download.
    pub fn take_export(&mut self) -> Vec<u8> {
        self.editor.exported.take().map(|(png, _)| png).unwrap_or_default()
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
            r#"{{"voxels":{},"tool":"{:?}","shape":"{:?}","mirror":{},"mirrorAxes":[{},{},{}],"lock":{},"span":{},"selected":{},"hasClip":{},"bounds":[{},{},{}],"boundsPanel":{},"slot":{},"colorEdit":{},"slotOp":"{:?}","undo":{},"redo":{},"cmdk":{},"coach":{},"name":{:?},"size":[{},{}],"ppp":{},"cam":[{},{},{},{},{},{}],"ortho":{},"view":{},"viewPanel":{},"guides":[{},{},{}],"mode":"{}","objects":{},"models":{},"picked":{},"render":{{"shown":{},"spp":{},"rows":{},"restarts":{},"timestamps":{},"panel":{},"maxSpp":{},"exposure":{},"aperture":{},"sky":{},"export":{}}}}}"#,
            e.scene.map.voxels,
            e.ui.tool,
            e.ui.shape,
            e.ui.mirror,
            e.ui.mirror_on[0],
            e.ui.mirror_on[1],
            e.ui.mirror_on[2],
            e.ui.lock.map_or("null".into(), |a| format!("\"{}\"", ["X", "Y", "Z"][a])),
            e.span().map_or("null".into(), |(l, v)| format!("[\"{l}\",\"{v}\"]")),
            e.scene.selected_count(),
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
            c.view().map_or("null".into(), |v| format!("\"{}\"", v.name())),
            e.ui.view_panel,
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
                .and_then(|i| e.scene.world().map(|w| vxr_app::editor::object_name(w, i)))
                .map_or("null".into(), |n| format!("{n:?}")),
            e.ui.render_rect.is_some() && e.ui.show_render,
            e.preview.samples(),
            e.preview.last_rows,
            e.preview.restarts,
            e.gpu.timestamps,
            e.ui.render_panel,
            e.preview.max_spp,
            e.preview.lighting.exposure,
            e.preview.lighting.aperture,
            e.preview.lighting.sky_backdrop,
            e.export.as_ref().map_or("null".into(), |x| format!("{:?}", x.progress().0)),
        )
    }

    /// The RGBA colour of palette slot `slot` and how many voxels use it.
    pub fn color(&self, slot: u8) -> Vec<u32> {
        let c = self.editor.scene.palette[slot as usize];
        vec![c[0] as u32, c[1] as u32, c[2] as u32, c[3] as u32, self.editor.scene.map.counts[slot as usize]]
    }

    /// The material of palette slot `slot`: type (0 diffuse, 1 metal, 2 glass, 3 emit),
    /// roughness, metalness, refractive index, transparency, emission and power.
    pub fn material(&self, slot: u8) -> Vec<f32> {
        let m = self.editor.scene.materials[slot as usize];
        let kind = vxr_core::material::Kind::ALL.iter().position(|&k| k == m.kind).unwrap_or(0);
        vec![kind as f32, m.rough, m.metal, m.ior, m.trans, m.emit, m.flux]
    }

    /// The middle of a named part of the UI in CSS pixels (see `Hybrid::hits`), or an empty
    /// array if it is not on screen. For the browser check.
    pub fn hit(&self, name: &str) -> Vec<f32> {
        self.editor.ui.hits.get(name).map(|r| vec![r.center().x, r.center().y]).unwrap_or_default()
    }

    /// Show a short message in the editor.
    pub fn toast(&mut self, msg: &str) {
        let ctx = self.editor.ctx.clone();
        self.editor.ui.toast(&ctx, msg);
    }
}
