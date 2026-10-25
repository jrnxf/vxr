//! The Hybrid UI (direction D) in egui, moved from the test 3 spike. Every size and colour below comes from the prototype's CSS
//! (`prototype/src/editor.html`); the CSS selector each block copies is named in its comment.

use egui::text::{LayoutJob, TextFormat};
use egui::{
    Align, Align2, Area, Color32, Context, CornerRadius, FontData, FontDefinitions, FontFamily, FontId, Id, Key,
    Modifiers, Order, Pos2, Rect, Sense, Shadow, Stroke, StrokeKind, Ui, pos2, vec2,
};
use std::sync::Arc;

// :root custom properties.
pub const PANEL: Color32 = Color32::from_rgba_unmultiplied_const(20, 23, 30, 199);
pub const PANEL_SOLID: Color32 = Color32::from_rgb(0x16, 0x19, 0x20);
pub const LINE: Color32 = Color32::from_rgba_unmultiplied_const(255, 255, 255, 20);
pub const LINE2: Color32 = Color32::from_rgba_unmultiplied_const(255, 255, 255, 36);
pub const TEXT: Color32 = Color32::from_rgb(0xe8, 0xea, 0xf0);
pub const MUTED: Color32 = Color32::from_rgb(0x8a, 0x90, 0xa0);
pub const FAINT: Color32 = Color32::from_rgb(0x5b, 0x60, 0x70);
pub const ACCENT: Color32 = Color32::from_rgb(0x8e, 0xa2, 0xff);
pub const ACCENT_BG: Color32 = Color32::from_rgba_unmultiplied_const(142, 162, 255, 41);
pub const DANGER: Color32 = Color32::from_rgb(0xff, 0x6b, 0x7a);
pub const OK: Color32 = Color32::from_rgb(0x6e, 0xe7, 0xa8);
pub const MID: Color32 = Color32::from_rgb(0xff, 0xd1, 0x66);
const HOVER: Color32 = Color32::from_rgba_unmultiplied_const(255, 255, 255, 15);
const R: u8 = 12;

/// Font bytes, loaded by the caller (see `crate::fonts`).
pub struct FontBytes {
    pub inter_regular: Vec<u8>,
    pub inter_medium: Vec<u8>,
    pub inter_semibold: Vec<u8>,
    pub mono_regular: Vec<u8>,
    pub mono_medium: Vec<u8>,
    pub icons: Vec<u8>,
}

fn family(name: &str) -> FontFamily {
    FontFamily::Name(name.into())
}
fn inter(size: f32) -> FontId {
    FontId::new(size, FontFamily::Proportional)
}
fn inter_medium(size: f32) -> FontId {
    FontId::new(size, family("medium"))
}
fn inter_semibold(size: f32) -> FontId {
    FontId::new(size, family("semibold"))
}
fn mono(size: f32) -> FontId {
    FontId::new(size, FontFamily::Monospace)
}
fn mono_medium(size: f32) -> FontId {
    FontId::new(size, family("mono-medium"))
}
fn icon_font(size: f32) -> FontId {
    FontId::new(size, family("icons"))
}

/// Install Inter, JetBrains Mono and the Lucide icon font, and the dark style. Without the
/// fonts, egui's built-in ones stand in (and the icons show as boxes).
pub fn install(ctx: &Context, fonts: Option<FontBytes>) {
    let defs = match fonts {
        Some(f) => font_definitions(f),
        None => {
            let mut defs = FontDefinitions::default();
            let prop = defs.families[&FontFamily::Proportional].clone();
            let mono = defs.families[&FontFamily::Monospace].clone();
            for (name, list) in [("medium", &prop), ("semibold", &prop), ("icons", &prop), ("mono-medium", &mono)] {
                defs.families.insert(family(name), list.clone());
            }
            defs
        }
    };
    ctx.set_fonts(defs);
    style(ctx);
}

fn font_definitions(fonts: FontBytes) -> FontDefinitions {
    let mut defs = FontDefinitions::empty();
    let mut add = |name: &str, bytes: Vec<u8>, families: &[FontFamily]| {
        defs.font_data.insert(name.into(), Arc::new(FontData::from_owned(bytes)));
        for f in families {
            defs.families.entry(f.clone()).or_default().push(name.into());
        }
    };
    add("inter", fonts.inter_regular, &[FontFamily::Proportional]);
    add("inter-medium", fonts.inter_medium, &[family("medium")]);
    add("inter-semibold", fonts.inter_semibold, &[family("semibold")]);
    add("mono", fonts.mono_regular, &[FontFamily::Monospace]);
    add("mono-medium", fonts.mono_medium, &[family("mono-medium")]);
    add("lucide", fonts.icons, &[family("icons")]);
    // Inter covers the symbols the labels use; keep it as every family's fallback.
    for f in [family("medium"), family("semibold"), FontFamily::Monospace, family("mono-medium")] {
        defs.families.get_mut(&f).unwrap().push("inter".into());
    }
    defs
}

fn style(ctx: &Context) {
    let mut visuals = egui::Visuals::dark();
    visuals.selection.bg_fill = Color32::from_rgba_unmultiplied(142, 162, 255, 90);
    visuals.text_cursor.stroke = Stroke::new(1.5, ACCENT);
    visuals.extreme_bg_color = Color32::TRANSPARENT;
    visuals.override_text_color = Some(TEXT);
    ctx.set_visuals(visuals);
}

// Lucide 1.61.0 codepoints.
mod icon {
    pub const BOX: char = '\u{e061}';
    pub const ERASER: char = '\u{e28f}';
    pub const PAINTBRUSH: char = '\u{e2e7}';
    pub const PIPETTE: char = '\u{e13b}';
    pub const POINTER: char = '\u{e1c3}';
    pub const SQUARE_DASHED: char = '\u{e1cb}';
    pub const LAYERS: char = '\u{e529}';
    pub const FLIP: char = '\u{e35e}';
    pub const SCAN: char = '\u{e257}';
    pub const FOLDER_OPEN: char = '\u{e247}';
    pub const DOWNLOAD: char = '\u{e0b2}';
    pub const KEYBOARD: char = '\u{e284}';
    pub const CODE: char = '\u{e206}';
    pub const UNDO: char = '\u{e2a1}';
    pub const REDO: char = '\u{e2a0}';
    pub const SEARCH: char = '\u{e151}';
    pub const CHEVRON_DOWN: char = '\u{e06d}';
}

pub use vxr_core::tools::{Shape, Tool};

/// What the UI asks the app to do this frame.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Action {
    Open,
    Save,
    Undo,
    Redo,
    Frame,
    Embed,
}

/// Readouts for the stats corner.
#[derive(Clone, Debug, Default)]
pub struct Stats {
    pub voxels: u64,
    pub fps: f32,
    pub frame_ms: f32,
    pub cursor: Option<[i32; 3]>,
    /// Size of the box being dragged, shown in place of the cursor.
    pub box_size: Option<[i32; 3]>,
}

pub struct Hybrid {
    pub tool: Tool,
    pub shape: Shape,
    pub mirror: bool,
    pub slot: u8,
    /// RGBA by voxel index, as the scene stores it.
    pub palette: [[u8; 4]; 256],
    /// Palette indices with voxels in the scene.
    pub used: [bool; 256],
    pub scene_name: String,
    pub coach: bool,
    /// The Cmd-K query while the palette is open.
    pub cmdk: Option<String>,
    pub cmdk_sel: usize,
    pub can_undo: bool,
    pub can_redo: bool,
    shift: bool,
    alt: bool,
    actions: Vec<Action>,
    toast: Option<(String, f64)>,
}

struct Command {
    label: &'static str,
    keys: &'static [&'static str],
    run: fn(&mut Hybrid),
}

const COMMANDS: &[Command] = &[
    Command { label: "Add", keys: &["A"], run: |h| h.tool = Tool::Add },
    Command { label: "Erase", keys: &["E"], run: |h| h.tool = Tool::Erase },
    Command { label: "Paint", keys: &["P"], run: |h| h.tool = Tool::Paint },
    Command { label: "Pick color", keys: &["I"], run: |h| h.tool = Tool::Pick },
    Command { label: "Voxel brush", keys: &["V"], run: |h| h.shape = Shape::Voxel },
    Command { label: "Box brush", keys: &["B"], run: |h| h.shape = Shape::Box },
    Command { label: "Face brush", keys: &["F"], run: |h| h.shape = Shape::Face },
    Command { label: "Mirror X", keys: &["M"], run: |h| h.mirror = !h.mirror },
    Command { label: "Frame scene", keys: &["."], run: |h| h.actions.push(Action::Frame) },
    Command { label: "Undo", keys: &["⌘", "Z"], run: |h| h.actions.push(Action::Undo) },
    Command { label: "Redo", keys: &["⌘", "⇧", "Z"], run: |h| h.actions.push(Action::Redo) },
    Command { label: "Open .vox", keys: &["⌘", "O"], run: |h| h.actions.push(Action::Open) },
    Command { label: "Save .vox", keys: &["⌘", "S"], run: |h| h.actions.push(Action::Save) },
    Command { label: "Embed", keys: &[], run: |h| h.actions.push(Action::Embed) },
];

impl Hybrid {
    pub fn new(palette: [[u8; 4]; 256], used: [bool; 256], scene_name: &str) -> Self {
        Self {
            tool: Tool::Add,
            shape: Shape::Voxel,
            mirror: false,
            slot: 28,
            palette,
            used,
            scene_name: scene_name.into(),
            coach: true,
            cmdk: None,
            cmdk_sel: 0,
            can_undo: false,
            can_redo: false,
            shift: false,
            alt: false,
            actions: Vec::new(),
            toast: None,
        }
    }

    /// Show a short message under the top bar for 1.8 s (#toast).
    pub fn toast(&mut self, ctx: &Context, msg: impl Into<String>) {
        self.toast = Some((msg.into(), ctx.input(|i| i.time) + 1.8));
    }

    /// Whether Shift or Alt is held, as last seen by the UI.
    pub fn modifiers(&self) -> (bool, bool) {
        (self.shift, self.alt)
    }

    /// The tool a click uses now: Alt picks, Shift erases.
    pub fn effective_tool(&self) -> Tool {
        if self.alt {
            Tool::Pick
        } else if self.shift && self.tool != Tool::Pick {
            Tool::Erase
        } else {
            self.tool
        }
    }

    fn color(&self, slot: u8) -> Color32 {
        let c = self.palette[slot as usize];
        Color32::from_rgb(c[0], c[1], c[2])
    }

    /// Draw the whole UI and return what the app should do.
    pub fn ui(&mut self, ctx: &Context, stats: &Stats) -> Vec<Action> {
        self.keys(ctx);
        let screen = ctx.content_rect();
        self.top_bar(ctx, screen);
        self.rail(ctx);
        self.palette_panel(ctx, screen);
        if self.coach {
            self.coach_card(ctx, screen);
        }
        self.stats(ctx, screen, stats);
        self.hint_bar(ctx, screen);
        self.history(ctx, screen);
        self.toast_panel(ctx, screen);
        if self.cmdk.is_some() {
            self.cmdk(ctx, screen);
        }
        std::mem::take(&mut self.actions)
    }

    fn keys(&mut self, ctx: &Context) {
        let typing = ctx.memory(|m| m.focused().is_some());
        ctx.input_mut(|i| {
            self.shift = i.modifiers.shift;
            self.alt = i.modifiers.alt;
            if i.consume_key(Modifiers::COMMAND, Key::K) {
                self.cmdk = if self.cmdk.is_some() { None } else { Some(String::new()) };
                self.cmdk_sel = 0;
            }
            if self.cmdk.is_some() || typing {
                return;
            }
            if i.consume_key(Modifiers::COMMAND | Modifiers::SHIFT, Key::Z) {
                self.actions.push(Action::Redo);
            }
            if i.consume_key(Modifiers::COMMAND, Key::Z) {
                self.actions.push(Action::Undo);
            }
            if i.consume_key(Modifiers::COMMAND, Key::O) {
                self.actions.push(Action::Open);
            }
            if i.consume_key(Modifiers::COMMAND, Key::S) {
                self.actions.push(Action::Save);
            }
            for (key, f) in [
                (Key::A, (|h: &mut Hybrid| h.tool = Tool::Add) as fn(&mut Hybrid)),
                (Key::E, |h| h.tool = Tool::Erase),
                (Key::P, |h| h.tool = Tool::Paint),
                (Key::I, |h| h.tool = Tool::Pick),
                (Key::V, |h| h.shape = Shape::Voxel),
                (Key::B, |h| h.shape = Shape::Box),
                (Key::F, |h| h.shape = Shape::Face),
                (Key::M, |h| h.mirror = !h.mirror),
                (Key::Escape, |h| h.coach = false),
                (Key::Period, |h| h.actions.push(Action::Frame)),
                (Key::OpenBracket, |h| h.slot = h.slot.saturating_sub(1).max(1)),
                (Key::CloseBracket, |h| h.slot = h.slot.saturating_add(1)),
            ] {
                if i.consume_key(Modifiers::NONE, key) {
                    f(self);
                }
            }
        });
    }

    // .panel
    fn panel(ctx: &Context, id: &str, rect: Rect, order: Order, contents: impl FnOnce(&mut Ui, Rect)) {
        Area::new(Id::unique(id)).fade_in(false).order(order).fixed_pos(rect.min).constrain(false).show(ctx, |ui| {
            let p = ui.painter();
            p.add(
                Shadow { offset: [0, 10], blur: 30, spread: 0, color: Color32::from_black_alpha(89) }
                    .as_shape(rect, CornerRadius::same(R)),
            );
            p.rect(rect, CornerRadius::same(R), PANEL, Stroke::new(1.0, LINE), StrokeKind::Inside);
            ui.allocate_rect(rect, Sense::hover());
            contents(ui, rect);
        });
    }

    // #top
    fn top_bar(&mut self, ctx: &Context, screen: Rect) {
        let rect = Rect::from_min_max(pos2(12.0, 12.0), pos2(screen.right() - 12.0, 56.0));
        Self::panel(ctx, "top", rect, Order::Middle, |ui, rect| {
            let cy = rect.center().y;
            let mut x = rect.left() + 12.0;
            logo(ui, pos2(x, cy - 9.0), 18.0);
            x += 26.0;
            let g = ui.painter().layout_no_wrap("vxr".into(), inter_semibold(13.0), TEXT);
            ui.painter().galley(pos2(x, cy - g.size().y / 2.0), g.clone(), TEXT);
            x += g.size().x + 10.0 + 4.0;
            // #scene-select
            let name = ui.painter().layout_no_wrap(self.scene_name.clone(), inter(13.0), TEXT);
            let sel = Rect::from_min_size(pos2(x, cy - 16.0), vec2(name.size().x + 36.0, 32.0));
            let resp = ui.interact(sel, Id::unique("scene-select"), Sense::click());
            let p = ui.painter();
            p.rect(
                sel,
                CornerRadius::same(8),
                Color32::from_white_alpha(10),
                Stroke::new(1.0, LINE),
                StrokeKind::Inside,
            );
            if resp.hovered() {
                p.rect_filled(sel, CornerRadius::same(8), HOVER);
            }
            p.galley(pos2(sel.left() + 10.0, cy - name.size().y / 2.0), name, TEXT);
            p.text(pos2(sel.right() - 14.0, cy), Align2::CENTER_CENTER, icon::CHEVRON_DOWN, icon_font(12.0), MUTED);
            x = sel.right() + 4.0 + 6.0;
            p.line_segment([pos2(x, cy - 10.0), pos2(x, cy + 10.0)], Stroke::new(1.0, LINE));
            x += 1.0 + 6.0 + 4.0;
            if tbtn(ui, &mut x, cy, Some(icon::FOLDER_OPEN), "Open .vox", "open", true).clicked() {
                self.actions.push(Action::Open);
            }
            x += 4.0;
            if tbtn(ui, &mut x, cy, Some(icon::DOWNLOAD), "Save .vox", "save", true).clicked() {
                self.actions.push(Action::Save);
            }
            // Right side, laid out from the right edge.
            let embed_label =
                ui.painter().layout_no_wrap("Embed".into(), inter_semibold(13.0), Color32::from_rgb(0x0d, 0x10, 0x20));
            let ew = 10.0 + 16.0 + 6.0 + embed_label.size().x + 10.0;
            let embed = Rect::from_min_size(pos2(rect.right() - 6.0 - ew, cy - 16.0), vec2(ew, 32.0));
            let resp = ui.interact(embed, Id::unique("embed"), Sense::click());
            let fill = if resp.hovered() { Color32::from_rgb(0xa6, 0xb6, 0xff) } else { ACCENT };
            let p = ui.painter();
            p.rect_filled(embed, CornerRadius::same(8), fill);
            let ink = Color32::from_rgb(0x0d, 0x10, 0x20);
            p.text(pos2(embed.left() + 18.0, cy), Align2::CENTER_CENTER, icon::CODE, icon_font(16.0), ink);
            p.galley(pos2(embed.left() + 32.0, cy - embed_label.size().y / 2.0), embed_label, ink);
            if resp.clicked() {
                self.actions.push(Action::Embed);
            }
            let mut kx = embed.left() - 4.0 - 36.0;
            if tbtn(ui, &mut kx, cy, Some(icon::KEYBOARD), "", "keys", true).clicked() {
                self.cmdk = Some(String::new());
            }
        });
    }

    // #rail
    fn rail(&mut self, ctx: &Context) {
        let h = 6.0 + rail_label_h() + 4.0 * 42.0 + 11.0 + rail_label_h() + 3.0 * 42.0 + 11.0 + 2.0 * 42.0 + 6.0 - 2.0;
        let rect = Rect::from_min_size(pos2(12.0, 68.0), vec2(52.0, h));
        Self::panel(ctx, "rail", rect, Order::Middle, |ui, rect| {
            let x = rect.left() + 6.0;
            let mut y = rect.top() + 6.0;
            rail_label(ui, rect, &mut y, "TOOL");
            let tool = self.tool;
            for (t, ic, k) in [
                (Tool::Add, icon::BOX, "A"),
                (Tool::Erase, icon::ERASER, "E"),
                (Tool::Paint, icon::PAINTBRUSH, "P"),
                (Tool::Pick, icon::PIPETTE, "I"),
            ] {
                if tool_button(ui, pos2(x, y), ic, k, tool == t).clicked() {
                    self.tool = t;
                }
                y += 42.0;
            }
            rail_sep(ui, rect, &mut y);
            rail_label(ui, rect, &mut y, "SHAPE");
            let shape = self.shape;
            for (s, ic, k) in [
                (Shape::Voxel, icon::POINTER, "V"),
                (Shape::Box, icon::SQUARE_DASHED, "B"),
                (Shape::Face, icon::LAYERS, "F"),
            ] {
                if tool_button(ui, pos2(x, y), ic, k, shape == s).clicked() {
                    self.shape = s;
                }
                y += 42.0;
            }
            rail_sep(ui, rect, &mut y);
            if tool_button(ui, pos2(x, y), icon::FLIP, "M", self.mirror).clicked() {
                self.mirror = !self.mirror;
            }
            y += 42.0;
            if tool_button(ui, pos2(x, y), icon::SCAN, ".", false).clicked() {
                self.actions.push(Action::Frame);
            }
        });
    }

    // #palette
    fn palette_panel(&mut self, ctx: &Context, screen: Rect) {
        let used_count = self.used.iter().skip(1).filter(|u| **u).count();
        let rect = Rect::from_min_size(
            pos2(screen.right() - 12.0 - 232.0, 68.0),
            vec2(232.0, 12.0 + 15.0 + 10.0 + 52.0 + 10.0 + 208.0 + 10.0 + 48.0 + 12.0),
        );
        Self::panel(ctx, "palette", rect, Order::Middle, |ui, rect| {
            let inner = rect.shrink(12.0);
            let p = ui.painter().clone();
            // .ph
            let hy = inner.top() + 7.5;
            let g = ui.painter().layout_job(spaced("PALETTE", inter_semibold(11.0), MUTED, 0.88));
            p.galley(pos2(inner.left(), hy - g.size().y / 2.0), g, MUTED);
            p.text(pos2(inner.right(), hy), Align2::RIGHT_CENTER, format!("{used_count} used"), mono(11.0), FAINT);
            // #current
            let cur = Rect::from_min_size(pos2(inner.left(), inner.top() + 25.0), vec2(inner.width(), 52.0));
            p.rect(
                cur,
                CornerRadius::same(9),
                Color32::from_white_alpha(9),
                Stroke::new(1.0, LINE),
                StrokeKind::Inside,
            );
            let chip = Rect::from_min_size(cur.min + vec2(9.0, 9.0), vec2(34.0, 34.0));
            p.add(
                Shadow { offset: [0, 4], blur: 12, spread: 0, color: Color32::from_black_alpha(77) }
                    .as_shape(chip, CornerRadius::same(8)),
            );
            p.rect(
                chip,
                CornerRadius::same(8),
                self.color(self.slot),
                Stroke::new(1.0, Color32::from_white_alpha(38)),
                StrokeKind::Inside,
            );
            let c = self.palette[self.slot as usize];
            let hex = format!("#{:02X}{:02X}{:02X}", c[0], c[1], c[2]);
            let mx = chip.right() + 10.0;
            p.text(pos2(mx, cur.center().y - 1.0), Align2::LEFT_BOTTOM, hex, mono(13.0), TEXT);
            let sub = if self.used[self.slot as usize] {
                format!("Slot {} · in scene", self.slot)
            } else {
                format!("Slot {}", self.slot)
            };
            p.text(pos2(mx, cur.center().y + 1.0), Align2::LEFT_TOP, sub, inter(11.0), FAINT);
            let eg = p.layout_no_wrap("Edit".into(), inter(12.0), MUTED);
            let edit = Rect::from_min_size(
                pos2(cur.right() - 9.0 - eg.size().x - 18.0, cur.center().y - 14.0),
                vec2(eg.size().x + 18.0, 28.0),
            );
            let er = ui.interact(edit, Id::unique("edit-color"), Sense::click());
            let (ec, eb) = if er.hovered() { (TEXT, Color32::from_white_alpha(64)) } else { (MUTED, LINE2) };
            p.rect_stroke(edit, CornerRadius::same(7), Stroke::new(1.0, eb), StrokeKind::Inside);
            p.galley(edit.center() - eg.size() / 2.0, eg, ec);
            // #swatches: 8 columns of 22 px with 4 px gaps, 2 px padding.
            let grid = pos2(inner.left() + 2.0, cur.bottom() + 10.0 + 2.0);
            for i in 0..64u8 {
                let slot = i + 1;
                let (col, row) = ((i % 8) as f32, (i / 8) as f32);
                let r = Rect::from_min_size(grid + vec2(col * 26.0, row * 26.0), vec2(22.0, 22.0));
                let resp = ui.interact(r, Id::unique(("sw", slot)), Sense::click());
                let on = self.slot == slot;
                let scale = if on {
                    1.05
                } else if resp.hovered() {
                    1.12
                } else {
                    1.0
                };
                let r = Rect::from_center_size(r.center(), r.size() * scale);
                if on {
                    p.rect_filled(r.expand(3.5), CornerRadius::same(9), Color32::WHITE);
                    p.rect_filled(r.expand(2.0), CornerRadius::same(7), PANEL_SOLID);
                }
                p.rect(
                    r,
                    CornerRadius::same(5),
                    self.color(slot),
                    Stroke::new(1.0, Color32::from_white_alpha(20)),
                    StrokeKind::Inside,
                );
                if self.used[slot as usize] {
                    // mix-blend-mode: difference against white at 75%.
                    let bg = self.color(slot);
                    let d = |v: u8| (v as f32 * 0.25 + (255.0 - v as f32) * 0.75) as u8;
                    p.circle_filled(r.center(), 2.0, Color32::from_rgb(d(bg.r()), d(bg.g()), d(bg.b())));
                }
                if resp.clicked() {
                    self.slot = slot;
                }
            }
            // .note
            let ny = grid.y + 8.0 * 26.0 - 4.0 + 2.0 + 10.0;
            let mut job = LayoutJob::default();
            let fmt =
                |color| TextFormat { font_id: inter(11.0), color, line_height: Some(15.95), ..Default::default() };
            job.append(
                "Editing a color recolors every voxel that uses it, instantly. Dots mark colors in the scene. ",
                0.0,
                fmt(FAINT),
            );
            job.append("[", 5.0, TextFormat { font_id: mono_medium(10.5), ..fmt(MUTED) });
            job.append(" ", 5.0, fmt(FAINT));
            job.append("]", 5.0, TextFormat { font_id: mono_medium(10.5), ..fmt(MUTED) });
            job.append(" step colors.", 5.0, fmt(FAINT));
            job.wrap.max_width = inner.width();
            let g = p.layout_job(job);
            kbd_frames(&p, pos2(inner.left(), ny), &g, &['[', ']']);
            p.galley(pos2(inner.left(), ny), g, FAINT);
        });
    }

    // #welcome
    fn coach_card(&mut self, ctx: &Context, screen: Rect) {
        let w = 440.0_f32.min(screen.width() - 32.0);
        let text_w = w - 32.0 - 14.0 - 19.0;
        let mut jobs = Vec::new();
        let lines: [&[(&str, bool)]; 3] = [
            &[
                ("Click", true),
                (" a surface to add a voxel. ", false),
                ("Drag", true),
                (" to draw along that surface.", false),
            ],
            &[
                ("Right-drag", true),
                (" to orbit, ", false),
                ("scroll", true),
                (" to zoom, ", false),
                ("Shift+right-drag", true),
                (" to pan.", false),
            ],
            &[
                ("Hold ", false),
                ("Shift", true),
                (" to erase, ", false),
                ("Alt", true),
                (" to pick a color. ", false),
                ("?", true),
                (" for every key.", false),
            ],
        ];
        for line in lines {
            let mut job = LayoutJob::default();
            for (t, b) in line {
                if *t == "?" {
                    job.append(
                        "?",
                        5.0,
                        TextFormat {
                            font_id: mono_medium(10.5),
                            color: MUTED,
                            line_height: Some(17.5),
                            ..Default::default()
                        },
                    );
                    continue;
                }
                let lead = if t.starts_with(" for every") { 5.0 } else { 0.0 };
                let f = if *b {
                    TextFormat {
                        font_id: inter_medium(12.5),
                        color: TEXT,
                        line_height: Some(17.5),
                        ..Default::default()
                    }
                } else {
                    TextFormat { font_id: inter(12.5), color: MUTED, line_height: Some(17.5), ..Default::default() }
                };
                job.append(t, lead, f);
            }
            job.wrap.max_width = text_w - 18.0;
            jobs.push(job);
        }
        let galleys: Vec<_> = jobs.into_iter().map(|j| ctx.fonts_mut(|f| f.layout_job(j))).collect();
        let body: f32 = galleys.iter().map(|g| g.size().y + 4.0).sum();
        let h = 14.0 + 19.6 + 4.0 + 8.0 - 4.0 + body + 14.0;
        let rect = Rect::from_min_size(pos2(screen.center().x - w / 2.0, 70.0), vec2(w, h));
        Self::panel(ctx, "welcome", rect, Order::Middle, |ui, rect| {
            let p = ui.painter().clone();
            let left = rect.left() + 16.0;
            p.text(
                pos2(left, rect.top() + 14.0 + 9.8),
                Align2::LEFT_CENTER,
                "Sculpt in under a minute",
                inter_semibold(14.0),
                TEXT,
            );
            let mut y = rect.top() + 14.0 + 19.6 + 4.0 + 8.0;
            for (i, g) in galleys.into_iter().enumerate() {
                y += 2.0;
                p.text(pos2(left + 14.0, y + 8.75), Align2::RIGHT_CENTER, format!("{}.", i + 1), inter(12.5), MUTED);
                let h = g.size().y;
                kbd_frames(&p, pos2(left + 18.0, y), &g, &['?']);
                p.galley(pos2(left + 18.0, y), g, MUTED);
                y += h + 2.0;
            }
            let x = Rect::from_center_size(pos2(rect.right() - 16.0 - 8.0, rect.top() + 14.0 + 11.0), vec2(16.0, 22.0));
            let resp = ui.interact(x, Id::unique("welcome-x"), Sense::click());
            p.text(x.center(), Align2::CENTER_CENTER, "×", inter(18.0), if resp.hovered() { TEXT } else { FAINT });
            if resp.clicked() {
                self.coach = false;
            }
        });
    }

    // #stats
    fn stats(&self, ctx: &Context, screen: Rect, s: &Stats) {
        let fps_color = if s.fps >= 55.0 {
            OK
        } else if s.fps >= 30.0 {
            MID
        } else {
            DANGER
        };
        let cols = [
            ("VOXELS", group_thousands(s.voxels), TEXT),
            ("FPS", if s.fps > 0.0 { format!("{:.0}", s.fps) } else { "-".into() }, fps_color),
            ("FRAME", format!("{:.1}ms", s.frame_ms), TEXT),
            match s.box_size {
                Some(b) => ("BOX", format!("{}×{}×{}", b[0], b[1], b[2]), TEXT),
                None => (
                    "CURSOR",
                    s.cursor.map(|c| format!("{},{},{}", c[0], c[1], c[2])).unwrap_or_else(|| "-".into()),
                    TEXT,
                ),
            },
        ];
        let galleys: Vec<_> = cols
            .iter()
            .map(|(l, v, c)| {
                let lg = ctx.fonts_mut(|f| f.layout_job(spaced(l, mono(9.5), FAINT, 0.76)));
                let vg = ctx.fonts_mut(|f| f.layout_no_wrap(v.clone(), mono(13.0), *c));
                (lg, vg)
            })
            .collect();
        let w: f32 = 24.0
            + galleys.iter().map(|(l, v)| l.size().x.max(v.size().x)).sum::<f32>()
            + 16.0 * (cols.len() - 1) as f32;
        let h = 16.0 + 13.3 + 18.2;
        let rect = Rect::from_min_size(pos2(12.0, screen.bottom() - 12.0 - h), vec2(w, h));
        Self::panel(ctx, "stats", rect, Order::Middle, |ui, rect| {
            let p = ui.painter();
            let mut x = rect.left() + 12.0;
            for (lg, vg) in galleys {
                let cw = lg.size().x.max(vg.size().x);
                p.galley(pos2(x, rect.top() + 8.0 + (13.3 - lg.size().y) / 2.0), lg, FAINT);
                p.galley(pos2(x, rect.top() + 8.0 + 13.3 + (18.2 - vg.size().y) / 2.0), vg, TEXT);
                x += cw + 16.0;
            }
        });
    }

    // #hint
    fn hint_bar(&self, ctx: &Context, screen: Rect) {
        let t = self.effective_tool();
        let mut items: Vec<(&str, &str)> = match (t, self.shape) {
            (Tool::Add, Shape::Voxel) => vec![("Click", "add"), ("Drag", "draw on that plane")],
            (Tool::Add, Shape::Box) => vec![("Drag", "fill a rectangle")],
            (Tool::Add, Shape::Face) => vec![("Click", "extrude the whole face")],
            (Tool::Erase, Shape::Voxel) => vec![("Click", "erase"), ("Drag", "erase as you go")],
            (Tool::Erase, Shape::Box) => vec![("Drag", "clear a rectangle")],
            (Tool::Erase, Shape::Face) => vec![("Click", "peel the whole face")],
            (Tool::Paint, Shape::Voxel) => vec![("Click", "paint"), ("Drag", "paint as you go")],
            (Tool::Paint, Shape::Box) => vec![("Drag", "paint a rectangle")],
            (Tool::Paint, Shape::Face) => vec![("Click", "fill the connected face")],
            (Tool::Pick, _) => vec![("Click", "take that color")],
        };
        items.extend([("Shift", "erase"), ("Alt", "pick"), ("Right-drag", "orbit")]);
        if t == Tool::Erase && self.shift {
            let n = items.len();
            items.remove(n - 3);
        }
        let mut job = LayoutJob::default();
        for (i, (k, v)) in items.iter().enumerate() {
            let lead = if i == 0 { 0.0 } else { 14.0 };
            job.append(
                k,
                lead,
                TextFormat { font_id: inter_medium(12.0), color: TEXT, line_height: Some(16.8), ..Default::default() },
            );
            job.append(
                v,
                5.0,
                TextFormat { font_id: inter(12.0), color: MUTED, line_height: Some(16.8), ..Default::default() },
            );
        }
        let g = ctx.fonts_mut(|f| f.layout_job(job));
        let size = vec2(g.size().x + 28.0, 16.8 + 16.0);
        let rect = Rect::from_min_size(
            pos2((screen.center().x - size.x / 2.0).round(), screen.bottom() - 12.0 - size.y),
            size,
        );
        Self::panel(ctx, "hint", rect, Order::Middle, |ui, rect| {
            ui.painter().galley(pos2(rect.left() + 14.0, rect.top() + 8.0), g, MUTED);
        });
    }

    // #history
    fn history(&mut self, ctx: &Context, screen: Rect) {
        let rect =
            Rect::from_min_size(pos2(screen.right() - 12.0 - 82.0, screen.bottom() - 12.0 - 40.0), vec2(82.0, 40.0));
        Self::panel(ctx, "history", rect, Order::Middle, |ui, rect| {
            let cy = rect.center().y;
            let mut x = rect.left() + 4.0;
            if tbtn(ui, &mut x, cy, Some(icon::UNDO), "", "undo", self.can_undo).clicked() {
                self.actions.push(Action::Undo);
            }
            x += 2.0;
            if tbtn(ui, &mut x, cy, Some(icon::REDO), "", "redo", self.can_redo).clicked() {
                self.actions.push(Action::Redo);
            }
        });
    }

    // #toast
    fn toast_panel(&mut self, ctx: &Context, screen: Rect) {
        let Some((msg, until)) = &self.toast else { return };
        let now = ctx.input(|i| i.time);
        if now > *until {
            self.toast = None;
            return;
        }
        ctx.request_repaint_after(std::time::Duration::from_secs_f64(until - now));
        let g = ctx.fonts_mut(|f| f.layout_no_wrap(msg.clone(), inter(12.5), TEXT));
        let size = vec2(g.size().x + 28.0, g.size().y + 18.0);
        let rect = Rect::from_min_size(pos2((screen.center().x - size.x / 2.0).round(), 68.0), size);
        Self::panel(ctx, "toast", rect, Order::Foreground, |ui, rect| {
            ui.painter().galley(pos2(rect.left() + 14.0, rect.top() + 9.0), g, TEXT);
        });
    }

    // Cmd-K: not in the mockups; styled like #help (a centred .panel).
    fn cmdk(&mut self, ctx: &Context, screen: Rect) {
        let query = self.cmdk.clone().unwrap_or_default().to_lowercase();
        let hits: Vec<usize> =
            (0..COMMANDS.len()).filter(|&i| COMMANDS[i].label.to_lowercase().contains(&query)).collect();
        self.cmdk_sel = self.cmdk_sel.min(hits.len().saturating_sub(1));
        let (mut run, mut close) = (None, false);
        ctx.input_mut(|i| {
            if i.consume_key(Modifiers::NONE, Key::Escape) {
                close = true;
            }
            if i.consume_key(Modifiers::NONE, Key::ArrowDown) {
                self.cmdk_sel = (self.cmdk_sel + 1).min(hits.len().saturating_sub(1));
            }
            if i.consume_key(Modifiers::NONE, Key::ArrowUp) {
                self.cmdk_sel = self.cmdk_sel.saturating_sub(1);
            }
            if i.consume_key(Modifiers::NONE, Key::Enter) {
                run = hits.get(self.cmdk_sel).copied();
            }
        });
        let shown = hits.len().min(8);
        let w = 560.0_f32.min(screen.width() - 32.0);
        let h = 8.0 + 40.0 + 9.0 + shown.max(1) as f32 * 34.0 + 8.0;
        let rect =
            Rect::from_min_size(pos2(screen.center().x - w / 2.0, (screen.height() * 0.18).max(70.0)), vec2(w, h));
        Self::panel(ctx, "cmdk", rect, Order::Foreground, |ui, rect| {
            let field = Rect::from_min_size(rect.min + vec2(8.0, 8.0), vec2(rect.width() - 16.0, 40.0));
            ui.painter().text(
                pos2(field.left() + 18.0, field.center().y),
                Align2::CENTER_CENTER,
                icon::SEARCH,
                icon_font(16.0),
                MUTED,
            );
            let text_rect = Rect::from_min_max(
                pos2(field.left() + 36.0, field.top() + 10.0),
                pos2(field.right() - 8.0, field.bottom() - 10.0),
            );
            let q = self.cmdk.as_mut().unwrap();
            let edit = egui::TextEdit::singleline(q)
                .id(Id::unique("cmdk-field"))
                .font(inter(14.0))
                .frame(egui::Frame::NONE)
                .hint_text(egui::RichText::new("Type a command").color(FAINT).font(inter(14.0)))
                .desired_width(text_rect.width());
            let resp = ui.put(text_rect, edit);
            resp.request_focus();
            let sep_y = field.bottom() + 4.5;
            ui.painter().line_segment(
                [pos2(rect.left() + 1.0, sep_y), pos2(rect.right() - 1.0, sep_y)],
                Stroke::new(1.0, LINE),
            );
            let mut y = sep_y + 4.5;
            if hits.is_empty() {
                ui.painter().text(
                    pos2(rect.left() + 18.0, y + 17.0),
                    Align2::LEFT_CENTER,
                    "No command matches",
                    inter(13.0),
                    FAINT,
                );
            }
            for (n, &i) in hits.iter().take(shown).enumerate() {
                let row = Rect::from_min_size(pos2(rect.left() + 8.0, y), vec2(rect.width() - 16.0, 34.0));
                let r = ui.interact(row, Id::unique(("cmd", i)), Sense::click());
                if r.hovered() {
                    self.cmdk_sel = n;
                }
                let p = ui.painter();
                if n == self.cmdk_sel {
                    p.rect_filled(row, CornerRadius::same(7), HOVER);
                }
                p.text(
                    pos2(row.left() + 10.0, row.center().y),
                    Align2::LEFT_CENTER,
                    COMMANDS[i].label,
                    inter(13.0),
                    TEXT,
                );
                let mut kx = row.right() - 10.0;
                for k in COMMANDS[i].keys.iter().rev() {
                    kx -= kbd(ui, pos2(kx, row.center().y), k, true) + 4.0;
                }
                if r.clicked() {
                    run = Some(i);
                }
                y += 34.0;
            }
        });
        if let Some(i) = run {
            (COMMANDS[i].run)(self);
            close = true;
        }
        if close {
            self.cmdk = None;
            ctx.memory_mut(|m| m.surrender_focus(Id::unique("cmdk-field")));
        }
    }
}

fn rail_label_h() -> f32 {
    4.0 + 12.6 + 2.0
}

// .rail-label
fn rail_label(ui: &Ui, rail: Rect, y: &mut f32, text: &str) {
    let g = ui.painter().layout_job(spaced(text, inter(9.0), FAINT, 0.72));
    ui.painter().galley(
        pos2(rail.center().x - g.size().x / 2.0 + 0.36, *y + 4.0 + (12.6 - g.size().y) / 2.0),
        g,
        FAINT,
    );
    *y += rail_label_h();
}

// .rail-sep
fn rail_sep(ui: &Ui, rail: Rect, y: &mut f32) {
    let sy = *y - 2.0 + 5.0 + 0.5;
    ui.painter().line_segment([pos2(rail.left() + 10.0, sy), pos2(rail.right() - 10.0, sy)], Stroke::new(1.0, LINE));
    *y += 11.0 - 2.0 + 2.0;
}

// .tool, .tool.on, .tool .k
fn tool_button(ui: &mut Ui, at: Pos2, glyph: char, key: &str, on: bool) -> egui::Response {
    let r = Rect::from_min_size(at, vec2(40.0, 40.0));
    let resp = ui.interact(r, Id::unique(("tool", key)), Sense::click());
    let p = ui.painter();
    let mut color = MUTED;
    if on {
        p.rect(
            r,
            CornerRadius::same(9),
            ACCENT_BG,
            Stroke::new(1.0, Color32::from_rgba_unmultiplied(142, 162, 255, 89)),
            StrokeKind::Inside,
        );
        color = ACCENT;
    } else if resp.hovered() {
        p.rect_filled(r, CornerRadius::same(9), HOVER);
        color = TEXT;
    }
    p.text(r.center(), Align2::CENTER_CENTER, glyph, icon_font(19.0), color);
    let kc = if on { ACCENT.gamma_multiply(0.8) } else { FAINT };
    p.text(pos2(r.right() - 3.0, r.bottom() - 2.0), Align2::RIGHT_BOTTOM, key, mono_medium(8.5), kc);
    resp
}

// .tbtn: 32 px high, 10 px side padding, 16 px icon, 6 px gap.
fn tbtn(
    ui: &mut Ui,
    x: &mut f32,
    cy: f32,
    glyph: Option<char>,
    label: &str,
    id: &str,
    enabled: bool,
) -> egui::Response {
    let g = (!label.is_empty()).then(|| ui.painter().layout_no_wrap(label.into(), inter(13.0), MUTED));
    let w = 20.0
        + glyph.map_or(0.0, |_| 16.0)
        + g.as_ref().map_or(0.0, |g| g.size().x + if glyph.is_some() { 6.0 } else { 0.0 });
    let r = Rect::from_min_size(pos2(*x, cy - 16.0), vec2(w, 32.0));
    *x += w;
    let sense = if enabled { Sense::click() } else { Sense::hover() };
    let resp = ui.interact(r, Id::unique(("tbtn", id)), sense);
    let p = ui.painter();
    let mut color = MUTED;
    if enabled && resp.hovered() {
        p.rect_filled(r, CornerRadius::same(8), HOVER);
        color = TEXT;
    }
    if !enabled {
        color = color.gamma_multiply(0.35);
    }
    let mut cx = r.left() + 10.0;
    if let Some(ic) = glyph {
        p.text(pos2(cx + 8.0, cy), Align2::CENTER_CENTER, ic, icon_font(16.0), color);
        cx += 22.0;
    }
    if let Some(g) = g {
        p.galley(pos2(cx, cy - g.size().y / 2.0), g, color);
    }
    resp
}

/// kbd: 10.5 px mono, 1 px border with a 2 px bottom, right-aligned at `right_center`. Returns its width.
fn kbd(ui: &Ui, right_center: Pos2, text: &str, _right: bool) -> f32 {
    let g = ui.painter().layout_no_wrap(text.into(), mono_medium(10.5), MUTED);
    let w = (g.size().x + 10.0).max(18.0);
    let r = Rect::from_min_size(pos2(right_center.x - w, right_center.y - 9.5), vec2(w, 19.0));
    let p = ui.painter();
    kbd_frame(p, r);
    p.galley(pos2(r.center().x - g.size().x / 2.0, r.top() + 1.0 + (16.0 - g.size().y) / 2.0), g, MUTED);
    w
}

/// The kbd border: 1 px, 2 px at the bottom, radius 5.
fn kbd_frame(p: &egui::Painter, r: Rect) {
    p.rect_stroke(r, CornerRadius::same(5), Stroke::new(1.0, LINE2), StrokeKind::Inside);
    p.line_segment(
        [pos2(r.left() + 2.5, r.bottom() - 1.5), pos2(r.right() - 2.5, r.bottom() - 1.5)],
        Stroke::new(1.0, LINE2),
    );
}

/// Draw kbd frames around the given characters of a laid-out paragraph at `origin`.
fn kbd_frames(p: &egui::Painter, origin: Pos2, g: &egui::Galley, chars: &[char]) {
    for row in &g.rows {
        for gl in &row.row.glyphs {
            if chars.contains(&gl.chr) {
                let cx = origin.x + row.pos.x + gl.pos.x + gl.advance_width / 2.0;
                let cy = origin.y + row.pos.y + row.row.size.y / 2.0;
                let w = (gl.advance_width + 8.0).max(18.0);
                kbd_frame(p, Rect::from_center_size(pos2(cx, cy + 0.5), vec2(w, 19.0)));
            }
        }
    }
}

/// Uppercase label with CSS letter-spacing (`em` times the font size, in points).
fn spaced(text: &str, font: FontId, color: Color32, spacing: f32) -> LayoutJob {
    let mut job = LayoutJob::default();
    job.append(text, 0.0, TextFormat { font_id: font, color, extra_letter_spacing: spacing, ..Default::default() });
    job.halign = Align::LEFT;
    job
}

fn group_thousands(n: u64) -> String {
    let s = n.to_string();
    let mut out = String::new();
    for (i, c) in s.chars().enumerate() {
        if i > 0 && (s.len() - i).is_multiple_of(3) {
            out.push(',');
        }
        out.push(c);
    }
    out
}

// #brand .logo: the 24-unit hexagonal cube from the prototype's SVG, scaled to `size`.
fn logo(ui: &Ui, at: Pos2, size: f32) {
    let s = size / 24.0;
    let pt = |x: f32, y: f32| at + vec2(x * s, y * s);
    let hex = vec![pt(12.0, 2.5), pt(20.5, 7.4), pt(20.5, 16.6), pt(12.0, 21.5), pt(3.5, 16.6), pt(3.5, 7.4)];
    let p = ui.painter();
    p.add(egui::Shape::convex_polygon(hex.clone(), ACCENT.gamma_multiply(0.25), Stroke::NONE));
    let st = Stroke::new(1.6 * s, ACCENT);
    p.add(egui::Shape::closed_line(hex, st));
    let c = pt(12.0, 12.0);
    for e in [pt(20.5, 7.4), pt(12.0, 21.5), pt(3.5, 7.4)] {
        p.line_segment([c, e], st);
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn thousands() {
        assert_eq!(super::group_thousands(145064), "145,064");
        assert_eq!(super::group_thousands(999), "999");
        assert_eq!(super::group_thousands(1_000_000), "1,000,000");
    }
}
