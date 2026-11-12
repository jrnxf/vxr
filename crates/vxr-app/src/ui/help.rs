//! Help (`docs/decisions.md` "Help and tour"): a panel under the top bar of live demo tiles,
//! each a short loop of the edit it names ([`crate::demo`]), with a search field and Try,
//! which closes Help and puts the user in that tool or gesture. It opens from the top bar's
//! Help button, F1 and Cmd-K "Help", and docks at the right like the assistant, left of it
//! when both are open; on a phone it is a sheet the width of the screen.

use crate::demo::{self, Button, DEMOS, TryIt};

use super::*;

/// The panel's width on a desktop or tablet, in points.
pub const HELP_W: f32 = 384.0;
/// The least of the view beside the docked assistant and Help: narrower, one closes the other.
pub const MIN_VIEW: f32 = 320.0;
const PAD: f32 = 14.0;
const GAP: f32 = 10.0;
/// A tile's image is this much wider than it is high.
const IMAGE_ASPECT: f32 = 1.75;
/// Under the image: the name and Try, then the keys.
const TILE_FOOT: f32 = 10.0 + 22.0 + 3.0 + 15.0 + 10.0;
/// The header (title and close), the tour's row and the search field.
const HEAD: f32 = 46.0 + TOUR_ROW + 10.0 + 34.0 + 12.0;
/// The "Take the tour" row.
const TOUR_ROW: f32 = 44.0;
/// The "Tour" button of a short panel, beside the search field.
const TOUR_PILL: f32 = 76.0;
/// A panel shorter than this (a phone held sideways) drops the title and puts the keyboard
/// shortcuts row after the tiles, so a whole tile shows.
const SHORT: f32 = 420.0;
/// The search field and close of a short panel.
const SHORT_HEAD: f32 = 12.0 + 34.0 + 12.0;
/// The narrowest tile of two to a row: room for the longest name beside Try.
const MIN_TILE_W: f32 = 160.0;
/// The keyboard shortcuts row at the bottom.
const FOOT: f32 = 48.0;
/// How often the tiles move while Help is open.
pub(super) const TILE_FRAME: std::time::Duration = std::time::Duration::from_millis(33);

/// Help as the UI shows it. The editor fills `tiles` and plays the demos in `shown`.
#[derive(Default)]
pub struct Help {
    pub open: bool,
    pub query: String,
    /// What each demo's tile shows now, set by the editor (in [`DEMOS`] order).
    pub tiles: Vec<TileView>,
    /// The demos whose tiles showed in the last frame: only these play.
    pub shown: Vec<usize>,
    /// The size of a tile's image in points, for the editor to render it at.
    pub image: egui::Vec2,
    /// The demo whose Try ran last, for the checks.
    pub tried: Option<&'static str>,
    /// The search field had the keyboard last frame: Escape there closes Help.
    pub(super) focused: bool,
    /// Scroll back to the top on the next frame (Help opened, or the search changed).
    to_top: bool,
}

/// One tile's image and what is drawn over it.
#[derive(Clone, Copy, Debug, Default)]
pub struct TileView {
    pub texture: Option<egui::TextureId>,
    /// The pointer, 0 to 1 across and down the image, and the button it holds.
    pub pointer: Option<[f32; 2]>,
    pub down: Option<Button>,
    /// The keys being pressed, shown in the image's corner.
    pub key: Option<&'static str>,
}

impl Hybrid {
    /// The top bar's Help button, F1 and Cmd-K "Help". On a phone held upright Help and the
    /// assistant are both sheets, and a narrow window has no room for both: one closes the
    /// other.
    pub fn toggle_help(&mut self) {
        if self.help.open {
            self.close_help();
        } else {
            self.open_help();
        }
    }

    pub fn open_help(&mut self) {
        if !self.help.open {
            self.help.open = true;
            self.help.to_top = true;
        }
        if self.crowded {
            self.ai_open = false;
        }
        // The tour's last step points here: opening Help finishes it.
        self.tour_help_opened();
    }

    pub fn close_help(&mut self) {
        self.help.open = false;
        self.help.shown.clear();
    }

    /// The width Help takes from the right of the view: none on a phone, where it is a sheet.
    pub fn help_width(&self) -> f32 {
        if self.help.open && !self.phone { HELP_W + 8.0 } else { 0.0 }
    }

    /// The width the docked assistant and Help take from the right of the view together.
    pub fn dock_width(&self) -> f32 {
        self.ai_width() + self.help_width()
    }

    /// What a tile's Try does: close Help, then the tool, brush and mirror, the camera keys,
    /// or world mode.
    fn try_demo(&mut self, d: &'static demo::Demo) {
        self.close_help();
        self.help.tried = Some(d.id);
        match d.try_it {
            TryIt::Tool(tool, shape, mirror) => {
                self.tool = tool;
                if tool != Tool::Select && tool != Tool::Pick {
                    self.shape = shape;
                }
                // Only the mirror tile turns mirror on, across X as it shows; the others leave
                // the user's mirror as it was.
                if mirror {
                    self.mirror = true;
                    self.mirror_on = [true, false, false];
                }
                // Tools edit a model, not the world: edit the picked object, or the last one.
                if self.world {
                    self.actions.push(Action::ToggleWorld);
                }
            }
            TryIt::Camera => self.camera_tip = true,
            TryIt::World => self.actions.push(Action::ToggleWorld),
        }
    }

    /// Where Help goes in `view` (the screen left of the docked assistant): the right column
    /// under the top bar, or on a phone the room the dock leaves.
    fn help_rect(&self, full: Rect, view: Rect) -> Rect {
        if self.phone {
            self.panel_region(full)
        } else {
            // At the right edge on touch too, where the dock centres in the view left of it.
            let right = view.right() - 12.0;
            // Narrower on a window too narrow for it, never off its left edge.
            let left = (right - HELP_W).max(view.left() + 12.0).min(right - MIN_TILE_W - 2.0 * PAD);
            Rect::from_min_max(pos2(left, TOP), pos2(right, (view.bottom() - 12.0).max(TOP + 240.0)))
        }
    }

    pub(super) fn help_panel(&mut self, ctx: &Context, full: Rect, view: Rect) {
        if !self.help.open {
            return;
        }
        // The tiles move: draw again soon, and only while Help is open.
        ctx.request_repaint_after(TILE_FRAME);
        let rect = self.help_rect(full, view);
        self.hits.insert("help-panel".into(), rect);
        // Two tiles to a row, or one on a sheet too narrow for two names and their Try. On a
        // short sheet the image is wider, so a whole tile shows.
        let cols = if rect.width() - 2.0 * PAD - GAP >= 2.0 * MIN_TILE_W { 2 } else { 1 };
        let tile_w = ((rect.width() - 2.0 * PAD - GAP * (cols - 1) as f32) / cols as f32).floor();
        let short = rect.height() < SHORT;
        let room = rect.height() - if short { SHORT_HEAD } else { HEAD + FOOT } - TILE_FOOT - 4.0;
        let image_h = (tile_w / IMAGE_ASPECT).min(room).max(tile_w / 4.0).round();
        self.help.image = vec2(tile_w, image_h);
        let touch = self.touch;
        let found = demo::search(&self.help.query, touch);
        let mut query = std::mem::take(&mut self.help.query);
        let to_top = std::mem::take(&mut self.help.to_top);
        let tiles = self.help.tiles.clone();
        let (mut close, mut tried, mut commands, mut focused_now, mut tour) = (false, None, false, false, false);
        let seen = self.tour.seen;
        let mut hits: Vec<(String, Rect)> = Vec::new();
        let mut shown = Vec::new();
        let order = if self.phone { Order::Foreground } else { Order::Middle };
        Self::panel_filled(ctx, "help", rect, order, th().panel_solid, |ui, rect| {
            ui.allocate_rect(rect, Sense::click());
            let p = ui.painter().clone();
            // The header: the title and close, over the search field (beside it when short).
            let hy = if short { rect.top() + 12.0 + 17.0 } else { rect.top() + 24.0 };
            if !short {
                let g = p.layout_no_wrap("Help".into(), sans_semibold(14.0), th().text);
                p.galley(pos2(rect.left() + PAD, hy - g.size().y / 2.0), g, th().text);
            }
            let x = Rect::from_center_size(pos2(rect.right() - PAD - 8.0, hy), vec2(28.0, 28.0));
            let xr = ui.interact(x, Id::unique("help-close"), Sense::click());
            hits.push(("help-close".into(), x));
            if xr.hovered() {
                p.rect_filled(x, CornerRadius::same(7), th().hover);
            }
            p.text(
                x.center(),
                Align2::CENTER_CENTER,
                icon::X,
                icon_font(16.0),
                if xr.hovered() { th().text } else { th().muted },
            );
            close |= xr.clicked();
            // The tour's row, then the search field.
            let field = if short {
                // A short panel has the tour as a button between the search field and close.
                let pill = Rect::from_min_max(
                    pos2(x.left() - 8.0 - TOUR_PILL, rect.top() + 12.0),
                    pos2(x.left() - 8.0, rect.top() + 46.0),
                );
                tour |= tour_pill(ui, pill, &mut hits);
                Rect::from_min_max(
                    pos2(rect.left() + PAD, rect.top() + 12.0),
                    pos2(pill.left() - 8.0, rect.top() + 46.0),
                )
            } else {
                let row = Rect::from_min_size(
                    pos2(rect.left() + PAD, rect.top() + 46.0),
                    vec2(rect.width() - 2.0 * PAD, TOUR_ROW),
                );
                tour |= tour_row(ui, row, seen, &mut hits);
                Rect::from_min_size(pos2(rect.left() + PAD, row.bottom() + 10.0), vec2(rect.width() - 2.0 * PAD, 34.0))
            };
            let focused = ui.memory(|m| m.has_focus(Id::unique("help-search")));
            p.rect(
                field,
                CornerRadius::same(8),
                th().tint(7),
                Stroke::new(1.0, if focused { th().ring } else { th().line }),
                StrokeKind::Inside,
            );
            p.text(
                pos2(field.left() + 16.0, field.center().y),
                Align2::CENTER_CENTER,
                icon::SEARCH,
                icon_font(14.0),
                th().faint,
            );
            let text_rect = Rect::from_min_max(
                pos2(field.left() + 31.0, field.top() + 8.0),
                pos2(field.right() - 10.0, field.bottom() - 8.0),
            );
            let edit = egui::TextEdit::singleline(&mut query)
                .id(Id::unique("help-search"))
                .font(sans(13.0))
                .frame(egui::Frame::NONE)
                .margin(egui::Margin::ZERO)
                .hint_text(egui::RichText::new("Search help").color(th().faint).font(sans(13.0)))
                .desired_width(text_rect.width());
            let resp = ui.put(text_rect, edit);
            hits.push(("help-search".into(), field));
            let changed = resp.changed();
            focused_now = resp.has_focus();
            // The tiles, two to a row, in a view that scrolls.
            let (head, foot_h) = if short { (SHORT_HEAD, 0.0) } else { (HEAD, FOOT) };
            let body =
                Rect::from_min_max(pos2(rect.left(), rect.top() + head), pos2(rect.right(), rect.bottom() - foot_h));
            let mut child = ui.new_child(egui::UiBuilder::new().max_rect(body));
            child.set_clip_rect(body);
            child.spacing_mut().scroll = egui::style::ScrollStyle::thin();
            let tile_h = image_h + TILE_FOOT;
            let rows = found.len().div_ceil(cols);
            let mut area =
                egui::ScrollArea::vertical().id_salt("help-tiles").auto_shrink([false; 2]).max_height(body.height());
            if to_top || changed {
                area = area.vertical_scroll_offset(0.0);
            }
            area.show(&mut child, |ui| {
                let top = ui.max_rect().top();
                let tiles_h = (rows as f32 * (tile_h + GAP) - GAP).max(0.0);
                let h = if short { tiles_h.max(40.0) + GAP + 34.0 + 12.0 } else { tiles_h + 4.0 };
                ui.allocate_space(vec2(body.width(), h.max(1.0)));
                if found.is_empty() {
                    let msg = "No demo matches. The command list (?) has every command.";
                    let g = ui.painter().layout(msg.into(), sans(12.5), th().muted, body.width() - 2.0 * PAD);
                    ui.painter().galley(pos2(body.left() + PAD, top + 6.0), g, th().muted);
                }
                let clip = ui.clip_rect();
                for (n, &i) in found.iter().enumerate() {
                    let d = &DEMOS[i];
                    let (row, col) = (n / cols, n % cols);
                    let card = Rect::from_min_size(
                        pos2(body.left() + PAD + col as f32 * (tile_w + GAP), top + row as f32 * (tile_h + GAP)),
                        vec2(tile_w, tile_h),
                    );
                    if !clip.intersects(card) {
                        continue;
                    }
                    shown.push(i);
                    let seen = card.intersect(clip);
                    hits.push((format!("help-tile-{}", d.id), seen));
                    let tile = tiles.get(i).copied().unwrap_or_default();
                    if draw_tile(ui, card, image_h, d, &tile, touch, &mut hits) {
                        tried = Some(i);
                    }
                }
                if short {
                    // The way to every command and key, after the tiles.
                    let y = top + tiles_h.max(40.0) + GAP;
                    let row = Rect::from_min_size(
                        pos2(body.left() + PAD - 6.0, y),
                        vec2(body.width() - 2.0 * PAD + 12.0, 34.0),
                    );
                    commands |= shortcuts_row(ui, row, &mut hits);
                }
            });
            if !short {
                // The way to every command and key.
                let foot = Rect::from_min_max(pos2(rect.left(), rect.bottom() - FOOT), rect.max);
                p.line_segment(
                    [pos2(foot.left() + 1.0, foot.top() + 0.5), pos2(foot.right() - 1.0, foot.top() + 0.5)],
                    Stroke::new(1.0, th().line),
                );
                let row = Rect::from_min_max(
                    pos2(foot.left() + 6.0, foot.top() + 7.0),
                    pos2(foot.right() - 6.0, foot.bottom() - 7.0),
                );
                commands |= shortcuts_row(ui, row, &mut hits);
            }
            if changed {
                ui.ctx().request_repaint();
            }
        });
        self.help.query = query;
        self.help.focused = focused_now;
        self.help.shown = shown;
        self.hits.extend(hits);
        if commands {
            self.cmdk = Some(String::new());
        }
        if let Some(i) = tried {
            self.try_demo(&DEMOS[i]);
        } else if tour {
            self.start_tour();
        } else if close {
            self.close_help();
        }
    }
}

/// The "Keyboard shortcuts" row, which opens the command list. Returns whether it was clicked.
fn shortcuts_row(ui: &mut Ui, row: Rect, hits: &mut Vec<(String, Rect)>) -> bool {
    let p = ui.painter().clone();
    let rr = ui.interact(row, Id::unique("help-shortcuts"), Sense::click());
    if p.clip_rect().contains_rect(row) {
        hits.push(("help-shortcuts".into(), row));
    }
    if rr.hovered() {
        p.rect_filled(row, CornerRadius::same(7), th().hover);
    }
    let cy = row.center().y;
    p.text(pos2(row.left() + 16.0, cy), Align2::CENTER_CENTER, icon::KEYBOARD, icon_font(15.0), th().muted);
    p.text(pos2(row.left() + 32.0, cy), Align2::LEFT_CENTER, "Keyboard shortcuts", sans(13.0), th().text);
    kbd(ui, pos2(row.right() - 10.0, cy), "?", true);
    rr.clicked()
}

/// Help's top row: "Take the tour", which replays it, with "Done" once it was seen. Returns
/// whether it was clicked.
fn tour_row(ui: &mut Ui, row: Rect, seen: bool, hits: &mut Vec<(String, Rect)>) -> bool {
    let p = ui.painter().clone();
    let resp = ui.interact(row, Id::unique("help-tour"), Sense::click());
    if p.clip_rect().contains_rect(row) {
        hits.push(("help-tour".into(), row));
    }
    p.rect(
        row,
        CornerRadius::same(8),
        if resp.hovered() { th().hover } else { th().tint(5) },
        Stroke::new(1.0, th().line),
        StrokeKind::Inside,
    );
    let cy = row.center().y;
    p.text(pos2(row.left() + 20.0, cy), Align2::CENTER_CENTER, icon::PLAY, icon_font(15.0), th().text);
    let x = row.left() + 38.0;
    p.text(pos2(x, cy - 8.0), Align2::LEFT_CENTER, "Take the tour", sans_medium(13.0), th().text);
    p.text(
        pos2(x, cy + 9.0),
        Align2::LEFT_CENTER,
        format!("{} steps, about a minute", super::tour::STEPS.len()),
        sans(11.5),
        th().muted,
    );
    if seen {
        let g = p.layout_no_wrap("Done".into(), sans(11.5), th().muted);
        let gx = row.right() - 12.0 - g.size().x;
        p.galley(pos2(gx, cy - g.size().y / 2.0), g, th().muted);
        p.text(pos2(gx - 10.0, cy), Align2::CENTER_CENTER, icon::CHECK, icon_font(13.0), th().muted);
    }
    resp.clicked()
}

/// The "Tour" button of a short panel. Returns whether it was clicked.
fn tour_pill(ui: &mut Ui, pill: Rect, hits: &mut Vec<(String, Rect)>) -> bool {
    let p = ui.painter().clone();
    let resp = ui.interact(pill, Id::unique("help-tour"), Sense::click());
    hits.push(("help-tour".into(), pill));
    p.rect(
        pill,
        CornerRadius::same(8),
        if resp.hovered() { th().hover } else { th().tint(5) },
        Stroke::new(1.0, th().line),
        StrokeKind::Inside,
    );
    let cy = pill.center().y;
    p.text(pos2(pill.left() + 18.0, cy), Align2::CENTER_CENTER, icon::PLAY, icon_font(13.0), th().text);
    p.text(pos2(pill.left() + 31.0, cy), Align2::LEFT_CENTER, "Tour", sans_medium(13.0), th().text);
    resp.clicked()
}

/// One demo tile: its image with the pointer and keys over it, its name, Try and keys.
/// Returns true when Try was clicked.
fn draw_tile(
    ui: &mut Ui,
    card: Rect,
    image_h: f32,
    d: &'static demo::Demo,
    tile: &TileView,
    touch: bool,
    hits: &mut Vec<(String, Rect)>,
) -> bool {
    let p = ui.painter().clone();
    p.rect(card, CornerRadius::same(10), th().tint(5), Stroke::new(1.0, th().line), StrokeKind::Inside);
    let image = Rect::from_min_size(card.min + vec2(1.0, 1.0), vec2(card.width() - 2.0, image_h - 1.0));
    draw_image(ui, image, CornerRadius { nw: 9, ne: 9, sw: 0, se: 0 }, tile, touch);
    p.line_segment(
        [pos2(card.left() + 1.0, image.bottom() + 0.5), pos2(card.right() - 1.0, image.bottom() + 0.5)],
        Stroke::new(1.0, th().line),
    );
    // The name and Try, then the keys.
    let ny = image.bottom() + 10.0 + 11.0;
    let tr = Rect::from_min_size(pos2(card.right() - 10.0 - 44.0, ny - 11.0), vec2(44.0, 22.0));
    let name_w = tr.left() - 6.0 - (card.left() + 10.0);
    let mut job = LayoutJob::single_section(
        d.name.into(),
        TextFormat { font_id: sans_medium(12.5), color: th().text, ..Default::default() },
    );
    job.wrap = egui::text::TextWrapping {
        max_width: name_w.max(1.0),
        max_rows: 1,
        break_anywhere: false,
        overflow_character: Some('…'),
    };
    let g = p.layout_job(job);
    p.galley(pos2(card.left() + 10.0, ny - g.size().y / 2.0), g, th().text);
    let resp = ui.interact(tr, Id::unique(("help-try", d.id)), Sense::click());
    if p.clip_rect().contains_rect(tr) {
        hits.push((format!("help-try-{}", d.id), tr));
    }
    let fill = if resp.hovered() { th().accent_hover } else { th().accent };
    let plate = pressed(tr, &resp);
    p.rect_filled(plate, CornerRadius::same(6), fill);
    p.text(plate.center(), Align2::CENTER_CENTER, "Try", sans_semibold(12.0), th().on_accent);
    let ky = ny + 11.0 + 3.0 + 7.5;
    let mut job = LayoutJob::single_section(
        (if touch { d.touch_keys } else { d.keys }).into(),
        TextFormat { font_id: sans(11.5), color: th().muted, ..Default::default() },
    );
    job.wrap = egui::text::TextWrapping {
        max_width: card.width() - 20.0,
        max_rows: 1,
        break_anywhere: false,
        overflow_character: Some('…'),
    };
    let g = p.layout_job(job);
    p.galley(pos2(card.left() + 10.0, ky - g.size().y / 2.0), g, th().muted);
    resp.clicked()
}

/// A demo's image with its pointer and keys over it: a Help tile's or the tour card's.
pub(super) fn draw_image(ui: &Ui, image: Rect, corners: CornerRadius, tile: &TileView, touch: bool) {
    let p = ui.painter();
    match tile.texture {
        Some(t) => {
            let uv = Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0));
            p.add(egui::epaint::RectShape::filled(image, corners, Color32::WHITE).with_texture(t, uv));
        }
        None => {
            p.rect_filled(image, corners, th().tint(9));
        }
    }
    let clip = p.clip_rect().intersect(image);
    let over = p.with_clip_rect(clip);
    if let Some([u, v]) = tile.pointer {
        let at = pos2(image.left() + u * image.width(), image.top() + v * image.height());
        draw_pointer(&over, at, tile.down, touch);
    }
    if let Some(k) = tile.key {
        let g = over.layout_no_wrap(k.into(), sans_medium(11.0), Color32::WHITE);
        let chip = Rect::from_min_size(image.min + vec2(6.0, 6.0), g.size() + vec2(10.0, 6.0));
        over.rect_filled(chip, CornerRadius::same(5), Color32::from_black_alpha(150));
        over.galley(chip.min + vec2(5.0, 3.0), g, Color32::WHITE);
    }
}

/// The demo's pointer: an arrow, with a ring while a button is held (R for the right button,
/// a pan arrow for Shift and the right button); on touch one, two or three fingers.
fn draw_pointer(p: &egui::Painter, at: Pos2, down: Option<Button>, touch: bool) {
    let white = Color32::WHITE;
    let dark = Color32::from_black_alpha(170);
    if touch {
        let fingers = match down {
            None => 1,
            Some(Button::Left) => 1,
            Some(Button::Right) => 2,
            Some(Button::Pan) => 3,
        };
        let fill = if down.is_some() { Color32::from_white_alpha(200) } else { Color32::from_white_alpha(120) };
        for k in 0..fingers {
            let c = at + vec2((k as f32 - (fingers - 1) as f32 / 2.0) * 16.0, 0.0);
            p.circle(c, 7.0, fill, Stroke::new(1.5, dark));
        }
        return;
    }
    if let Some(b) = down {
        p.circle_stroke(at, 9.0, Stroke::new(2.0, Color32::from_white_alpha(210)));
        let label = match b {
            Button::Left => None,
            Button::Right => Some("R"),
            Button::Pan => Some("⇧R"),
        };
        if let Some(l) = label {
            let g = p.layout_no_wrap(l.into(), mono_medium(9.5), white);
            let chip = Rect::from_min_size(at + vec2(12.0, 10.0), g.size() + vec2(8.0, 4.0));
            p.rect_filled(chip, CornerRadius::same(4), dark);
            p.galley(chip.min + vec2(4.0, 2.0), g, white);
        }
    }
    // The arrow, a white body with a dark edge, its tip on the point.
    let pts = vec![at, at + vec2(0.0, 14.0), at + vec2(10.0, 10.0)];
    p.add(egui::Shape::convex_polygon(pts, white, Stroke::new(1.25, dark)));
}
