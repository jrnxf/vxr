//! The assistant in the Hybrid UI (`docs/research/ai.md`, directions A and B): the docked
//! panel (Cmd-J or the top bar's sparkles) with the conversation, each answer's tool calls,
//! its model and cost, the composer with Stop, the key settings and the model picker; and
//! Cmd-K "Ask AI", a one-line prompt bar whose request lands in the same conversation and
//! review card. Not in the mockups; drawn in the panel style.

use std::collections::HashSet;
use std::sync::Arc;

use egui::{Galley, ScrollArea, UiBuilder};
use vxr_ai::net::{BROWSER_LIMIT, KeyPlace, browser_reachable};
use vxr_ai::provider::{COMPATIBLE_MODELS, Kind, MODELS, OPENROUTER, Settings};

use super::*;
use crate::assistant::{AnswerView, CallState, Entry, Progress};

/// The panel's width, docked right of the view.
pub const PANEL_W: f32 = 340.0;

/// What the panel and the prompt bar ask the editor to do.
#[derive(Clone, Debug, PartialEq)]
pub enum AiAction {
    Send(String),
    /// Stop the request in progress.
    Stop,
    /// After Stop: keep what was staged under review, or drop it.
    Keep,
    Drop,
    SaveKey {
        kind: Kind,
        key: String,
        remember: bool,
    },
    RemoveKey(Kind),
    Settings(Settings),
    /// Start a new conversation.
    NewChat,
}

/// The assistant as the panel shows it (set by the editor when it changes).
#[derive(Clone, Debug)]
pub struct ChatView {
    pub entries: Vec<Entry>,
    pub running: bool,
    /// The run waits for the user to allow a bigger change.
    pub held: bool,
    /// Stop was pressed with a change staged: keep or drop it.
    pub stop_ask: bool,
    pub settings: Settings,
    pub has_key: [bool; 3],
    pub place: KeyPlace,
    pub remembered: [bool; 3],
    /// US dollars spent in this conversation.
    pub spent: f64,
}

impl ChatView {
    pub fn new() -> ChatView {
        ChatView {
            entries: Vec::new(),
            running: false,
            held: false,
            stop_ask: false,
            settings: Settings::new(Kind::Anthropic, false),
            has_key: [false; 3],
            place: KeyPlace::Memory,
            remembered: [false; 3],
            spent: 0.0,
        }
    }

    /// Whether a request can go out: a key for the provider, or an endpoint that needs none.
    pub fn ready(&self) -> bool {
        !self.settings.kind.needs_key() || self.has_key[self.settings.kind.index()]
    }

    fn last_answer(&self) -> Option<&AnswerView> {
        match self.entries.last() {
            Some(Entry::Answer(a)) => Some(a),
            _ => None,
        }
    }
}

impl Default for ChatView {
    fn default() -> Self {
        ChatView::new()
    }
}

/// Cmd-K "Ask AI"'s prompt bar.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct AskBar {
    pub text: String,
    /// The request went out; the bar follows it.
    pub sent: bool,
    /// The field takes focus on the next frame.
    focus: bool,
}

/// The panel's own state between frames.
pub(super) struct AiState {
    draft: String,
    key: String,
    /// The provider the key section edits.
    key_kind: Option<Kind>,
    remember: bool,
    base: String,
    model: String,
    /// The model menu above the composer's chip is open.
    pub(super) menu: bool,
    /// The composer or a settings field had focus last frame (Escape then only leaves it).
    pub(super) focus: bool,
    /// Answers whose every call is listed.
    expanded: HashSet<usize>,
    /// Where the prompt bar's bottom was drawn, for the toast to go under it.
    pub(super) bar_bottom: Option<f32>,
}

impl Default for AiState {
    fn default() -> Self {
        AiState {
            draft: String::new(),
            key: String::new(),
            key_kind: None,
            remember: false,
            base: OPENROUTER.into(),
            model: COMPATIBLE_MODELS[0].into(),
            menu: false,
            focus: false,
            expanded: HashSet::new(),
            bar_bottom: None,
        }
    }
}

const EXAMPLES: [&str; 3] = ["Make the roof red", "Add a small tree beside the house", "Build a fence around it"];
/// Calls an answer lists before the rest fold away.
const CALL_ROWS: usize = 6;

/// `text` wrapped to `width` in at most `rows` rows (0: as many as it takes).
fn wrap(ui: &Ui, text: &str, font: FontId, color: Color32, width: f32, rows: usize) -> Arc<Galley> {
    let mut job = LayoutJob::single_section(
        text.to_string(),
        TextFormat { font_id: font, color, line_height: Some(18.0), ..Default::default() },
    );
    job.wrap = egui::text::TextWrapping {
        max_width: width.max(1.0),
        max_rows: if rows == 0 { usize::MAX } else { rows },
        break_anywhere: false,
        overflow_character: Some('…'),
    };
    ui.fonts_mut(|f| f.layout_job(job))
}

/// A 24-point square icon button.
fn icon_button(ui: &mut Ui, r: Rect, glyph: char, id: &str, on: bool) -> egui::Response {
    let resp = ui.interact(r, Id::unique(("aibtn", id)), Sense::click());
    let p = ui.painter();
    let color = if on {
        p.rect_filled(r, CornerRadius::same(7), th().accent_bg);
        th().accent
    } else if resp.hovered() {
        p.rect_filled(r, CornerRadius::same(7), th().hover);
        th().text
    } else {
        th().muted
    };
    p.text(r.center(), Align2::CENTER_CENTER, glyph, icon_font(14.0), color);
    resp
}

/// The accent button (#apply's look), greyed out when not `enabled`.
pub(super) fn accent_button(
    ui: &mut Ui,
    r: Rect,
    glyph: Option<char>,
    label: &str,
    id: &str,
    enabled: bool,
) -> egui::Response {
    let resp = ui.interact(r, Id::unique(("aiacc", id)), if enabled { Sense::click() } else { Sense::hover() });
    let p = ui.painter();
    let (fill, ink) = if !enabled {
        (th().tint(18), th().faint)
    } else if resp.hovered() {
        (th().accent_hover, th().on_accent)
    } else {
        (th().accent, th().on_accent)
    };
    p.rect_filled(r, CornerRadius::same(8), fill);
    let g = (!label.is_empty()).then(|| p.layout_no_wrap(label.into(), sans_semibold(12.5), ink));
    let w =
        glyph.map_or(0.0, |_| 14.0) + g.as_ref().map_or(0.0, |g| g.size().x + if glyph.is_some() { 6.0 } else { 0.0 });
    let mut x = r.center().x - w / 2.0;
    if let Some(gl) = glyph {
        p.text(pos2(x + 7.0, r.center().y), Align2::CENTER_CENTER, gl, icon_font(14.0), ink);
        x += 20.0;
    }
    if let Some(g) = g {
        p.galley(pos2(x, r.center().y - g.size().y / 2.0), g, ink);
    }
    resp
}

/// A section label like the review card's STEPS.
fn section(ui: &Ui, at: Pos2, text: &str) {
    let g = ui.painter().layout_job(spaced(text, sans(9.0), th().faint, 0.72));
    ui.painter().galley(pos2(at.x, at.y + 6.0 - g.size().y / 2.0), g, th().faint);
}

/// A single-line field with a mono label on its left, as the Bounds panel's; `password`
/// hides what is typed. Returns the response of its text edit.
pub(super) fn ai_field(
    ui: &mut Ui,
    r: Rect,
    label: &str,
    id: Id,
    text: &mut String,
    password: bool,
    hint: &str,
) -> egui::Response {
    let focused = ui.memory(|m| m.has_focus(id));
    let p = ui.painter().clone();
    let border = if focused { th().ring } else { th().line };
    p.rect(r, CornerRadius::same(7), th().tint(9), Stroke::new(1.0, border), StrokeKind::Inside);
    let mut tx = r.left() + 10.0;
    if !label.is_empty() {
        let g = p.layout_no_wrap(label.into(), mono(10.0), th().faint);
        tx += g.size().x + 8.0;
        p.galley(pos2(r.left() + 10.0, r.center().y - g.size().y / 2.0), g, th().faint);
    }
    let tr = Rect::from_min_max(pos2(tx, r.top() + 7.0), pos2(r.right() - 9.0, r.bottom() - 7.0));
    if text.is_empty() && !focused {
        p.text(pos2(tr.left(), r.center().y), Align2::LEFT_CENTER, hint, sans(12.0), th().faint);
    }
    let edit = egui::TextEdit::singleline(text)
        .id(id)
        .font(mono(12.0))
        .text_color(th().text)
        .password(password)
        .frame(egui::Frame::NONE)
        .margin(egui::Margin::ZERO)
        .desired_width(tr.width());
    ui.put(tr, edit)
}

impl Hybrid {
    /// The width the docked panel takes from the right of the view, with its gap.
    pub fn ai_width(&self) -> f32 {
        // The sheet over a phone held upright takes none of the view.
        if self.ai_open && !self.sheet() { PANEL_W + 8.0 } else { 0.0 }
    }

    /// Open the panel on its key settings, for `kind` (or the provider in use).
    pub fn open_ai_setup(&mut self, kind: Option<Kind>) {
        self.ai_open = true;
        self.ai_setup = true;
        self.ai.key_kind = kind.or(Some(self.chat.settings.kind));
    }

    /// Cmd-J and the top bar's button: close the panel, or open it, on its key settings
    /// (kept until Done) while there is no key yet.
    pub fn toggle_ai(&mut self) {
        if self.ai_open {
            self.ai_open = false;
        } else if !self.chat.ready() && self.review.is_none() {
            self.open_ai_setup(None);
        } else {
            self.ai_open = true;
        }
    }

    /// Cmd-K "Ask AI": the prompt bar, starting from `text`.
    pub fn open_ask_bar(&mut self, text: &str) {
        self.cmdk = None;
        self.ask_bar = Some(AskBar { text: text.trim().to_string(), sent: false, focus: true });
    }

    /// Send a request from the panel or the bar: the editor runs it, or opens the key
    /// settings when there is no key yet.
    fn ai_send(&mut self, text: &str) -> bool {
        let text = text.trim();
        if text.is_empty() || self.chat.running {
            return false;
        }
        if !self.chat.ready() {
            self.open_ai_setup(None);
            return false;
        }
        self.ai_out.push(AiAction::Send(text.to_string()));
        true
    }

    /// Cmd-K "Organize into objects (AI)": the assistant opens on [`ORGANIZE`]. Without a key
    /// it opens on its key settings, the request waiting in the composer.
    pub fn organize(&mut self) {
        self.cmdk = None;
        if self.ai_send(ORGANIZE) {
            self.ai_open = true;
        } else if self.ai.draft.trim().is_empty() {
            self.ai.draft = ORGANIZE.to_string();
        }
    }

    /// What the composer holds.
    pub fn ai_draft(&self) -> &str {
        &self.ai.draft
    }

    /// The editor did not start a request from the bar or the panel (its toast says why): the
    /// words go back where they were typed, to send again.
    pub fn ai_refused(&mut self, text: &str) {
        let text = text.trim();
        if let Some(bar) = &mut self.ask_bar
            && bar.sent
            && bar.text == text
        {
            bar.sent = false;
            bar.focus = true;
        } else if self.ai.draft.trim().is_empty() {
            self.ai.draft = text.to_string();
        }
    }

    // The docked panel.
    pub(super) fn assistant_panel(&mut self, ctx: &Context, screen: Rect) {
        self.review_slot = None;
        self.ai_body = None;
        if !self.ai_open {
            self.ai.menu = false;
            self.review_pinned = false;
            return;
        }
        let rect = if self.sheet() {
            // A phone held upright: the width of the screen, down to the dock. While a change is
            // under review only the lower half, so the scene and its ghost show above it.
            let bottom = self.dock_rows(screen).last().map_or(screen.bottom() - 12.0, |r| r.top() - 8.0);
            let bottom = bottom.max(TOP + 240.0);
            let top = if self.review.is_some() { ((TOP + bottom) / 2.0).min(bottom - 240.0) } else { TOP };
            Rect::from_min_max(pos2(screen.left() + 12.0, top), pos2(screen.right() - 12.0, bottom))
        } else {
            // The right column, top to bottom.
            Rect::from_min_max(
                pos2(screen.right() - 12.0 - PANEL_W, TOP),
                pos2(screen.right() - 12.0, (screen.bottom() - 12.0).max(TOP + 240.0)),
            )
        };
        // The half sheet of a review on a phone gives the card the composer's room (the card
        // has its own field to ask for changes), except while a request runs or waits on Keep
        // or Drop, which the composer holds.
        let compact = self.sheet() && self.review.is_some() && !self.chat.running && !self.chat.stop_ask;
        let agents_tab = self.agents_panel;
        let (agents_tone, agents_n) = self.agents_status();
        // The review card sits at the end of the thread, as wide as the thread.
        let review_h = self.review_height(ctx, rect.width() - 24.0 - 6.0);
        // On the half sheet its footer is pinned under the thread, and the thread, which then
        // holds only the card, opens on its top.
        let foot_h = self.review_foot_height(ctx, rect.width() - 24.0 - 6.0).filter(|_| compact && !self.agents_panel);
        let open_top = foot_h.is_some() && !self.review_pinned;
        self.review_pinned = foot_h.is_some();
        let (mut slot_out, mut body_out, mut tab_click) = (None, None, None);
        // The keys wait while a change is under review: its card is in the chat.
        let setup = (self.ai_setup || !self.chat.ready()) && review_h.is_none();
        let chat = &self.chat;
        let st = &mut self.ai;
        let mut out: Vec<AiAction> = Vec::new();
        let mut hits: Vec<(String, Rect)> = Vec::new();
        let (mut close, mut toggle_setup, mut send, mut focus, mut setup_done) = (false, false, None, false, false);
        let mut menu_at = None;
        Self::panel(ctx, "assistant", rect, Order::Middle, |ui, rect| {
            let inner = rect.shrink(12.0);
            let p = ui.painter().clone();
            // Header: ASSISTANT, then new conversation, keys and close on the right.
            let hy = inner.top() + 7.5;
            p.text(pos2(inner.left() + 6.0, hy), Align2::CENTER_CENTER, icon::SPARKLES, icon_font(13.0), th().accent);
            let g = p.layout_job(spaced("ASSISTANT", sans_semibold(11.0), th().muted, 0.88));
            p.galley(pos2(inner.left() + 18.0, hy - g.size().y / 2.0), g, th().muted);
            let mut bx = inner.right() + 4.0;
            for (glyph, id, on) in [
                (icon::X, "ai-close", false),
                (icon::KEY_ROUND, "ai-settings", setup),
                (icon::MESSAGE_SQUARE_PLUS, "ai-new", false),
            ] {
                let r = Rect::from_center_size(pos2(bx - 12.0, hy), vec2(24.0, 24.0));
                bx -= 28.0;
                hits.push((id.into(), r));
                if icon_button(ui, r, glyph, id, on).clicked() {
                    match id {
                        "ai-close" => close = true,
                        "ai-settings" => toggle_setup = true,
                        _ => out.push(AiAction::NewChat),
                    }
                }
            }
            // Tabs: the chat, and the terminal agents (a dot while one is connected).
            let ty = inner.top() + 15.0 + 8.0;
            let mut tx = inner.left();
            for (id, label, on) in [("ai-tab-chat", "Chat", !agents_tab), ("ai-tab-agents", "Agents", agents_tab)] {
                let g = p.layout_no_wrap(label.into(), sans_medium(12.0), if on { th().text } else { th().muted });
                let is_agents = id == "ai-tab-agents";
                let count =
                    (is_agents && agents_n > 0).then(|| p.layout_no_wrap(agents_n.to_string(), mono(10.5), th().muted));
                let dot = if is_agents { agents_tone } else { None };
                let extra = dot.map_or(0.0, |_| 12.0) + count.as_ref().map_or(0.0, |c| c.size().x + 6.0);
                let r = Rect::from_min_size(pos2(tx, ty), vec2(g.size().x + 20.0 + extra, 26.0));
                tx = r.right() + 4.0;
                let resp = ui.interact(r, Id::unique(id), Sense::click());
                if on {
                    p.rect_filled(r, CornerRadius::same(7), th().hover);
                } else if resp.hovered() {
                    p.rect_filled(r, CornerRadius::same(7), th().tint(9));
                }
                let gc = if on || resp.hovered() { th().text } else { th().muted };
                let gw = g.size().x;
                p.galley(pos2(r.left() + 10.0, r.center().y - g.size().y / 2.0), g, gc);
                let mut x = r.left() + 10.0 + gw + 6.0;
                if let Some(t) = dot {
                    p.circle_filled(pos2(x + 2.75, r.center().y), 2.75, t.color());
                    x += 8.0 + 4.0;
                }
                if let Some(c) = count {
                    p.galley(pos2(x, r.center().y - c.size().y / 2.0), c, th().muted);
                }
                hits.push((id.into(), r));
                if resp.clicked() {
                    tab_click = Some(id == "ai-tab-agents");
                }
            }
            p.hline(rect.x_range(), ty + 26.0 + 6.0, Stroke::new(1.0, th().line));
            if agents_tab {
                // The Agents tab draws over the panel (`agents_tab`).
                body_out = Some(Rect::from_min_max(
                    pos2(rect.left(), ty + 26.0 + 8.0),
                    pos2(rect.right(), rect.bottom() - 8.0),
                ));
                return;
            }
            let top = ty + 26.0 + 14.0;
            if setup {
                let body = Rect::from_min_max(pos2(inner.left(), top), inner.max);
                ui.scope_builder(UiBuilder::new().max_rect(body), |ui| {
                    ScrollArea::vertical().id_salt("ai-setup").auto_shrink([false; 2]).show(ui, |ui| {
                        let (f, d) = Self::ai_setup_view(ui, body.width() - 6.0, chat, st, &mut out, &mut hits);
                        focus |= f;
                        setup_done |= d;
                    });
                });
                return;
            }
            // The composer at the bottom, Keep or Drop above it after a Stop.
            let w = inner.width();
            let measure = wrap(ui, &format!("{}.", st.draft), sans(13.0), th().text, w - 24.0, 0);
            let rows = ((measure.size().y / 18.0).round() as usize).clamp(2, 6);
            let text_h = rows as f32 * 18.0;
            let comp_h = 10.0 + text_h + 6.0 + 28.0 + 8.0;
            let comp = Rect::from_min_size(pos2(inner.left(), inner.bottom() - comp_h), vec2(w, comp_h));
            let mut bottom = if compact { inner.bottom() } else { comp.top() - 10.0 };
            if chat.stop_ask {
                let b = Rect::from_min_size(pos2(inner.left(), bottom - 76.0), vec2(w, 76.0));
                bottom = b.top() - 10.0;
                p.rect(b, CornerRadius::same(8), th().mid_bg, Stroke::new(1.0, th().mid_line), StrokeKind::Inside);
                p.text(
                    pos2(b.left() + 17.0, b.top() + 20.5),
                    Align2::CENTER_CENTER,
                    icon::TRIANGLE_ALERT,
                    icon_font(13.0),
                    th().mid,
                );
                p.text(
                    pos2(b.left() + 30.0, b.top() + 20.5),
                    Align2::LEFT_CENTER,
                    "Stopped. Keep what it staged?",
                    sans(12.0),
                    th().text,
                );
                let keep = Rect::from_min_size(pos2(b.right() - 10.0 - 72.0, b.top() + 38.0), vec2(72.0, 26.0));
                let drop = keep.translate(vec2(-78.0, 0.0));
                hits.push(("ai-keep".into(), keep));
                hits.push(("ai-drop".into(), drop));
                if small_button(ui, drop, icon::TRASH, "Drop", "ai-drop", false).clicked() {
                    out.push(AiAction::Drop);
                }
                if small_button(ui, keep, icon::CHECK, "Keep", "ai-keep", true).clicked() {
                    out.push(AiAction::Keep);
                }
            }
            let w = inner.width() - 6.0;
            let foot = foot_h
                .map(|f| Rect::from_min_max(pos2(inner.left(), bottom - (f - 12.0)), pos2(inner.left() + w, bottom)));
            let body =
                Rect::from_min_max(pos2(inner.left(), top), pos2(inner.right(), foot.map_or(bottom, |f| f.top())));
            ui.scope_builder(UiBuilder::new().max_rect(body), |ui| {
                let mut area = ScrollArea::vertical().id_salt("ai-chat").auto_shrink([false; 2]);
                area = if open_top { area.vertical_scroll_offset(0.0) } else { area.stick_to_bottom(foot.is_none()) };
                area.show(ui, |ui| {
                    ui.spacing_mut().item_spacing = vec2(0.0, 0.0);
                    if chat.entries.is_empty()
                        && review_h.is_none()
                        && let Some(t) = Self::ai_welcome(ui, w, &mut hits)
                    {
                        st.draft = t.to_string();
                    }
                    if foot.is_none() {
                        for (i, e) in chat.entries.iter().enumerate() {
                            match e {
                                Entry::User(t) => Self::ai_user(ui, w, t),
                                Entry::Answer(a) => Self::ai_answer(ui, w, i, a, st, &mut hits),
                            }
                        }
                    }
                    // The change under review, under the reply that made it, or alone with its
                    // footer pinned.
                    if let Some(h) = review_h {
                        if foot.is_none() {
                            ui.add_space(10.0);
                        }
                        let shown = h - foot_h.map_or(0.0, |f| f - 12.0);
                        let (r, _) = ui.allocate_exact_size(vec2(w, shown), Sense::hover());
                        slot_out = Some((r, body, foot));
                    }
                });
            });
            if compact {
                return;
            }
            // The composer: the request, the model chip, the spend, Send or Stop.
            let id = Id::unique("ai-draft");
            let focused = ui.memory(|m| m.has_focus(id));
            focus |= focused;
            let border = if focused { th().ring } else { th().line };
            p.rect(comp, CornerRadius::same(10), th().tint(9), Stroke::new(1.0, border), StrokeKind::Inside);
            let tr = Rect::from_min_size(pos2(comp.left() + 12.0, comp.top() + 10.0), vec2(w - 24.0, text_h));
            if st.draft.is_empty() {
                let hint = if chat.running { "Working on it…" } else { "Ask for a change…" };
                p.text(pos2(tr.left(), tr.top() + 9.0), Align2::LEFT_CENTER, hint, sans(13.0), th().faint);
            }
            if focused && ui.input_mut(|i| i.consume_key(Modifiers::NONE, Key::Enter)) && !chat.running {
                send = Some(std::mem::take(&mut st.draft));
            }
            let edit = egui::TextEdit::multiline(&mut st.draft)
                .id(id)
                .font(sans(13.0))
                .text_color(th().text)
                .frame(egui::Frame::NONE)
                .margin(egui::Margin::ZERO)
                .desired_rows(rows)
                .desired_width(tr.width());
            ui.put(tr, edit);
            hits.push(("ai-draft".into(), tr));
            let ry = comp.bottom() - 8.0 - 14.0;
            let label = chat.settings.model.clone();
            let lg = p.layout_no_wrap(label, mono(11.0), th().muted);
            let chip = Rect::from_min_size(pos2(comp.left() + 6.0, ry - 12.0), vec2(lg.size().x + 32.0, 24.0));
            let resp = ui.interact(chip, Id::unique("ai-model-chip"), Sense::click());
            if resp.hovered() || st.menu {
                p.rect_filled(chip, CornerRadius::same(7), th().hover);
            }
            let c = if resp.hovered() || st.menu { th().text } else { th().muted };
            p.galley(pos2(chip.left() + 8.0, ry - lg.size().y / 2.0), lg, c);
            p.text(pos2(chip.right() - 12.0, ry), Align2::CENTER_CENTER, icon::CHEVRON_DOWN, icon_font(12.0), c);
            hits.push(("ai-model-chip".into(), chip));
            if resp.clicked() {
                st.menu = !st.menu;
            }
            if st.menu {
                menu_at = Some(chip);
            }
            if chat.spent > 0.0 {
                p.text(
                    pos2(chip.right() + 8.0, ry),
                    Align2::LEFT_CENTER,
                    format!("${:.2}", chat.spent),
                    mono(10.5),
                    th().faint,
                );
            }
            if chat.running {
                let r = Rect::from_min_size(pos2(comp.right() - 6.0 - 72.0, ry - 14.0), vec2(72.0, 28.0));
                hits.push(("ai-stop".into(), r));
                if small_button(ui, r, icon::SQUARE, "Stop", "ai-stop", false).clicked() {
                    out.push(AiAction::Stop);
                }
            } else {
                let r = Rect::from_min_size(pos2(comp.right() - 6.0 - 28.0, ry - 14.0), vec2(28.0, 28.0));
                hits.push(("ai-send".into(), r));
                if accent_button(ui, r, Some(icon::ARROW_UP), "", "ai-send", !st.draft.trim().is_empty()).clicked() {
                    send = Some(std::mem::take(&mut st.draft));
                }
            }
        });
        self.review_slot = slot_out;
        self.ai_body = body_out;
        if let Some(agents) = tab_click {
            self.agents_panel = agents;
        }
        if let Some(chip) = menu_at {
            self.ai_model_menu(ctx, chip, &mut out, &mut hits);
        }
        if close {
            self.ai_open = false;
        }
        if setup_done {
            self.ai_setup = false;
        }
        if toggle_setup {
            self.ai_setup = !setup;
            self.ai.key_kind = Some(self.chat.settings.kind);
        }
        if out.iter().any(|a| matches!(a, AiAction::SaveKey { .. })) && !self.chat.has_key.iter().any(|h| *h) {
            // The first key: back to the conversation once it is kept.
            self.ai_setup = false;
        }
        self.ai.focus = focus;
        if let Some(t) = send
            && !self.ai_send(&t)
            && !t.trim().is_empty()
        {
            self.ai.draft = t;
        }
        self.ai_out.extend(out);
        self.hits.extend(hits);
        self.hits.insert("assistant".into(), rect);
    }

    /// The empty conversation: what the assistant does, and a few requests to start from.
    /// Returns an example clicked.
    fn ai_welcome(ui: &mut Ui, w: f32, hits: &mut Vec<(String, Rect)>) -> Option<&'static str> {
        let title = wrap(ui, "Ask for a change to the scene", sans_semibold(13.0), th().text, w, 2);
        let body = wrap(
            ui,
            "The assistant edits the open model with vxr's own tools. Every change waits in the review card until you apply it, as one undo step.",
            sans(12.0),
            th().muted,
            w,
            6,
        );
        let h = 6.0 + title.size().y + 6.0 + body.size().y + 16.0 + 12.0 + 6.0 + EXAMPLES.len() as f32 * 36.0;
        let (r, _) = ui.allocate_exact_size(vec2(w, h), Sense::hover());
        let p = ui.painter().clone();
        let mut y = r.top() + 6.0;
        let title_h = title.size().y;
        p.galley(pos2(r.left(), y), title, th().text);
        y += title_h + 6.0;
        let bh = body.size().y;
        p.galley(pos2(r.left(), y), body, th().muted);
        y += bh + 16.0;
        section(ui, pos2(r.left(), y), "TRY");
        y += 12.0 + 6.0;
        let mut picked = None;
        for (k, ex) in EXAMPLES.iter().enumerate() {
            let row = Rect::from_min_size(pos2(r.left(), y), vec2(w, 30.0));
            let id = format!("ai-example-{k}");
            let resp = ui.interact(row, Id::unique(&id), Sense::click());
            let hover = resp.hovered();
            p.rect_stroke(
                row,
                CornerRadius::same(8),
                Stroke::new(1.0, if hover { th().tint(64) } else { th().line2 }),
                StrokeKind::Inside,
            );
            p.text(
                pos2(row.left() + 12.0, row.center().y),
                Align2::LEFT_CENTER,
                *ex,
                sans(12.0),
                if hover { th().text } else { th().muted },
            );
            p.text(
                pos2(row.right() - 14.0, row.center().y),
                Align2::CENTER_CENTER,
                icon::ARROW_UP,
                icon_font(12.0),
                th().faint,
            );
            hits.push((id, row));
            if resp.clicked() {
                picked = Some(*ex);
            }
            y += 36.0;
        }
        picked
    }

    /// A request, right-aligned in an accent bubble.
    fn ai_user(ui: &mut Ui, w: f32, text: &str) {
        let g = wrap(ui, text, sans(13.0), th().text, w * 0.85 - 24.0, 0);
        let size = g.size() + vec2(24.0, 16.0);
        let (r, _) = ui.allocate_exact_size(vec2(w, size.y + 14.0), Sense::hover());
        let b = Rect::from_min_size(pos2(r.right() - size.x, r.top()), size);
        let p = ui.painter();
        p.rect(b, CornerRadius::same(10), th().accent_bg, Stroke::new(1.0, th().ring), StrokeKind::Inside);
        p.galley(b.min + vec2(12.0, 8.0), g, th().text);
    }

    /// An answer: its text, its calls, then how it went, or what it is doing now.
    fn ai_answer(ui: &mut Ui, w: f32, i: usize, a: &AnswerView, st: &mut AiState, hits: &mut Vec<(String, Rect)>) {
        let working = matches!(a.progress, Progress::Working(_));
        let text = (!a.text.is_empty()).then(|| wrap(ui, &a.text, sans(12.5), th().text, w, 0));
        let thinking = (working && !a.thinking.trim().is_empty()).then(|| {
            let t = a.thinking.trim();
            let tail: String = t.chars().rev().take(220).collect::<Vec<_>>().into_iter().rev().collect();
            let tail = if tail.len() < t.len() { format!("…{}", tail.trim_start()) } else { tail };
            wrap(ui, &tail, sans(11.5), th().faint, w, 3)
        });
        let n = a.calls.len();
        let folded = n > CALL_ROWS && !st.expanded.contains(&i);
        let shown: Vec<(usize, &crate::assistant::CallRow)> = if folded {
            a.calls.iter().enumerate().skip(n - (CALL_ROWS - 1)).collect()
        } else {
            a.calls.iter().enumerate().collect()
        };
        let rows = shown.len() + folded as usize;
        let error = match &a.progress {
            Progress::Failed(e) => Some(wrap(ui, e, sans(12.0), th().danger, w - 20.0, 4)),
            _ => None,
        };
        let row_h = 22.0;
        let mut h = 0.0;
        if let Some(g) = &text {
            h += g.size().y + 8.0;
        }
        if let Some(g) = &thinking {
            h += g.size().y + 8.0;
        }
        if rows > 0 {
            h += rows as f32 * row_h + 8.0 + 8.0;
        }
        if let Some(g) = &error {
            h += g.size().y + 16.0 + 8.0;
        }
        h += 18.0 + 16.0;
        let (r, _) = ui.allocate_exact_size(vec2(w, h), Sense::hover());
        let p = ui.painter().clone();
        let mut y = r.top();
        if let Some(g) = text {
            let gh = g.size().y;
            p.galley(pos2(r.left(), y), g, th().text);
            y += gh + 8.0;
        }
        if let Some(g) = thinking {
            let gh = g.size().y;
            p.galley(pos2(r.left(), y), g, th().faint);
            y += gh + 8.0;
        }
        if rows > 0 {
            let b = Rect::from_min_size(pos2(r.left(), y), vec2(w, rows as f32 * row_h + 8.0));
            p.rect(b, CornerRadius::same(8), th().tint(6), Stroke::new(1.0, th().line), StrokeKind::Inside);
            let mut ry = b.top() + 4.0;
            if folded {
                let row = Rect::from_min_size(pos2(b.left() + 4.0, ry), vec2(w - 8.0, row_h));
                let id = format!("ai-calls-{i}");
                let resp = ui.interact(row, Id::unique(&id), Sense::click());
                if resp.hovered() {
                    p.rect_filled(row, CornerRadius::same(6), th().hover);
                }
                let more = n - shown.len();
                p.text(
                    pos2(row.left() + 10.0, row.center().y),
                    Align2::CENTER_CENTER,
                    icon::CHEVRON_RIGHT,
                    icon_font(11.0),
                    th().muted,
                );
                p.text(
                    pos2(row.left() + 22.0, row.center().y),
                    Align2::LEFT_CENTER,
                    format!("{more} earlier calls"),
                    sans(11.5),
                    th().muted,
                );
                hits.push((id, row));
                if resp.clicked() {
                    st.expanded.insert(i);
                }
                ry += row_h;
            }
            for (_, c) in shown {
                let row = Rect::from_min_size(pos2(b.left() + 4.0, ry), vec2(w - 8.0, row_h));
                let cy = row.center().y;
                let (glyph, color) = match c.state {
                    CallState::Ok => (icon::CHECK, th().ok),
                    CallState::Failed => (icon::X, th().danger),
                    CallState::Held => (icon::TRIANGLE_ALERT, th().mid),
                };
                p.text(pos2(row.left() + 10.0, cy), Align2::CENTER_CENTER, glyph, icon_font(11.0), color);
                let tg = p.layout_no_wrap(c.tool.clone(), mono(11.0), th().text);
                let tw = tg.size().x;
                p.galley(pos2(row.left() + 22.0, cy - tg.size().y / 2.0), tg, th().text);
                let dx = row.left() + 22.0 + tw + 8.0;
                let dc = if c.state == CallState::Failed { th().danger.gamma_multiply(0.8) } else { th().muted };
                let mut job = LayoutJob::single_section(
                    c.detail.clone(),
                    TextFormat { font_id: sans(11.5), color: dc, ..Default::default() },
                );
                job.wrap = egui::text::TextWrapping {
                    max_width: (row.right() - 6.0 - dx).max(1.0),
                    max_rows: 1,
                    break_anywhere: true,
                    overflow_character: Some('…'),
                };
                let dg = p.layout_job(job);
                p.galley(pos2(dx, cy - dg.size().y / 2.0), dg, dc);
                ry += row_h;
            }
            y = b.bottom() + 8.0;
        }
        if let Some(g) = error {
            let gh = g.size().y;
            let b = Rect::from_min_size(pos2(r.left(), y), vec2(w, gh + 16.0));
            p.rect(b, CornerRadius::same(8), th().danger_bg, Stroke::new(1.0, th().danger_line), StrokeKind::Inside);
            p.galley(pos2(b.left() + 10.0, b.top() + 8.0), g, th().danger);
            y = b.bottom() + 8.0;
        }
        let fy = y + 9.0;
        match &a.progress {
            Progress::Working(what) => {
                let t = ui.input(|i| i.time);
                let pulse = 0.55 + 0.45 * (t * 4.0).sin().abs() as f32;
                p.circle_filled(pos2(r.left() + 4.0, fy), 3.5, th().accent.gamma_multiply(pulse));
                let dots = ".".repeat(1 + (t * 3.0) as usize % 3);
                p.text(pos2(r.left() + 14.0, fy), Align2::LEFT_CENTER, format!("{what}{dots}"), sans(12.0), th().muted);
                ui.ctx().request_repaint();
            }
            done => {
                let mut parts = vec![a.model.clone()];
                if n > 0 {
                    parts.push(format!("{n} call{}", if n == 1 { "" } else { "s" }));
                }
                if let Some(c) = a.cost {
                    parts.push(if c < 0.01 { format!("${c:.3}") } else { format!("${c:.2}") });
                }
                parts.push(format!("{:.1} s", a.secs));
                if *done == Progress::Stopped {
                    parts.insert(0, "Stopped".into());
                }
                let mut job = LayoutJob::single_section(
                    parts.join(" · "),
                    TextFormat { font_id: sans(11.0), color: th().faint, ..Default::default() },
                );
                job.wrap = egui::text::TextWrapping {
                    max_width: w,
                    max_rows: 1,
                    break_anywhere: true,
                    overflow_character: Some('…'),
                };
                let g = p.layout_job(job);
                p.galley(pos2(r.left(), fy - g.size().y / 2.0), g, th().faint);
            }
        }
    }

    /// The models, the key for the chosen provider and where it is kept. Returns whether a
    /// field has focus, and whether Done was clicked.
    fn ai_setup_view(
        ui: &mut Ui,
        w: f32,
        chat: &ChatView,
        st: &mut AiState,
        out: &mut Vec<AiAction>,
        hits: &mut Vec<(String, Rect)>,
    ) -> (bool, bool) {
        let mut done_clicked = false;
        let kind = st.key_kind.unwrap_or(chat.settings.kind);
        let mut focus = false;
        let (r, _) = ui.allocate_exact_size(vec2(w, 18.0), Sense::hover());
        section(ui, r.min, "MODEL");
        // The picker's models and the compatible endpoint, each with its key's state.
        let mut rows: Vec<(String, &str, Kind, Settings)> = MODELS
            .iter()
            .map(|m| {
                let s = Settings { kind: m.kind, model: m.id.into(), ..chat.settings.clone() };
                (m.id.to_string(), m.note, m.kind, s)
            })
            .collect();
        let compat = Settings {
            kind: Kind::Compatible,
            model: st.model.clone(),
            base: st.base.clone(),
            browser: chat.settings.browser,
        };
        rows.push(("Compatible endpoint".into(), "OpenRouter, Ollama, LM Studio", Kind::Compatible, compat));
        for (k, (label, note, mkind, s)) in rows.into_iter().enumerate() {
            let (row, resp) = ui.allocate_exact_size(vec2(w, 44.0), Sense::click());
            let on = if mkind == Kind::Compatible {
                chat.settings.kind == Kind::Compatible
            } else {
                chat.settings.kind == mkind && chat.settings.model == label
            };
            let p = ui.painter();
            if on {
                p.rect(row, CornerRadius::same(8), th().accent_bg, Stroke::new(1.0, th().ring), StrokeKind::Inside);
            } else if resp.hovered() {
                p.rect_filled(row, CornerRadius::same(8), th().hover);
            }
            let c = pos2(row.left() + 16.0, row.top() + 15.0);
            p.circle_stroke(c, 6.0, Stroke::new(1.5, if on { th().accent } else { th().line2 }));
            if on {
                p.circle_filled(c, 3.0, th().accent);
            }
            let font = if mkind == Kind::Compatible { sans_medium(12.5) } else { mono(12.0) };
            p.text(
                pos2(row.left() + 30.0, row.top() + 15.0),
                Align2::LEFT_CENTER,
                &label,
                font,
                if on { th().text } else { th().muted },
            );
            p.text(pos2(row.left() + 30.0, row.top() + 31.0), Align2::LEFT_CENTER, note, sans(11.0), th().faint);
            let has = chat.has_key[mkind.index()];
            let (tag, tc) = match (has, mkind) {
                (true, _) => ("key kept", th().ok),
                (false, Kind::Compatible) => ("key optional", th().faint),
                (false, _) => ("no key", th().faint),
            };
            p.text(pos2(row.right() - 10.0, row.top() + 15.0), Align2::RIGHT_CENTER, tag, mono(10.0), tc);
            let id = format!("ai-pick-{k}");
            hits.push((id, row));
            if resp.clicked() {
                st.key_kind = Some(mkind);
                out.push(AiAction::Settings(s));
            }
            ui.add_space(4.0);
        }
        ui.add_space(14.0);
        let (r, _) = ui.allocate_exact_size(vec2(w, 18.0), Sense::hover());
        section(ui, r.min, &format!("KEY · {}", kind.name().to_uppercase()));
        if kind == Kind::Compatible {
            let (r, _) = ui.allocate_exact_size(vec2(w, 32.0), Sense::hover());
            let resp = ai_field(ui, r, "URL", Id::unique("ai-base"), &mut st.base, false, OPENROUTER);
            focus |= resp.has_focus();
            hits.push(("ai-base".into(), r));
            ui.add_space(6.0);
            if chat.settings.browser && !st.base.trim().is_empty() && !browser_reachable(&st.base) {
                let g = wrap(ui, BROWSER_LIMIT, sans(11.5), th().mid, w, 4);
                let (r, _) = ui.allocate_exact_size(vec2(w, g.size().y), Sense::hover());
                ui.painter().galley(r.min, g, th().mid);
                hits.push(("ai-base-limit".into(), r));
                ui.add_space(6.0);
            }
            let (r, _) = ui.allocate_exact_size(vec2(w, 32.0), Sense::hover());
            let resp2 = ai_field(ui, r, "MODEL", Id::unique("ai-model"), &mut st.model, false, COMPATIBLE_MODELS[0]);
            focus |= resp2.has_focus();
            hits.push(("ai-model".into(), r));
            if (resp.lost_focus() || resp2.lost_focus()) && chat.settings.kind == Kind::Compatible {
                out.push(AiAction::Settings(Settings {
                    kind,
                    model: st.model.clone(),
                    base: st.base.clone(),
                    ..chat.settings.clone()
                }));
            }
            ui.add_space(6.0);
        }
        let has = chat.has_key[kind.index()];
        if has {
            let (r, _) = ui.allocate_exact_size(vec2(w, 36.0), Sense::hover());
            let p = ui.painter();
            p.rect(r, CornerRadius::same(8), th().tint(9), Stroke::new(1.0, th().line), StrokeKind::Inside);
            p.text(
                pos2(r.left() + 17.0, r.center().y),
                Align2::CENTER_CENTER,
                icon::SHIELD_CHECK,
                icon_font(14.0),
                th().ok,
            );
            let saved = if chat.remembered[kind.index()] { "Key kept in this browser" } else { "Key kept" };
            p.text(pos2(r.left() + 32.0, r.center().y), Align2::LEFT_CENTER, saved, sans(12.0), th().text);
            let rm = Rect::from_min_size(pos2(r.right() - 5.0 - 84.0, r.top() + 5.0), vec2(84.0, 26.0));
            hits.push(("ai-key-remove".into(), rm));
            if small_button(ui, rm, icon::TRASH, "Remove", "ai-key-remove", false).clicked() {
                out.push(AiAction::RemoveKey(kind));
            }
        } else {
            let (r, _) = ui.allocate_exact_size(vec2(w, 32.0), Sense::hover());
            let field = Rect::from_min_max(r.min, pos2(r.right() - 72.0 - 6.0, r.bottom()));
            let hint = match kind {
                Kind::Anthropic => "sk-ant-…",
                Kind::OpenAi => "sk-…",
                Kind::Compatible => "none for a local server",
            };
            let resp = ai_field(ui, field, "", Id::unique("ai-key"), &mut st.key, true, hint);
            focus |= resp.has_focus();
            hits.push(("ai-key".into(), field));
            let save = Rect::from_min_size(pos2(r.right() - 72.0, r.top()), vec2(72.0, 32.0));
            hits.push(("ai-key-save".into(), save));
            let enter = resp.lost_focus() && ui.input(|i| i.key_pressed(Key::Enter));
            if (accent_button(ui, save, None, "Save", "ai-key-save", !st.key.trim().is_empty()).clicked() || enter)
                && !st.key.trim().is_empty()
            {
                out.push(AiAction::SaveKey { kind, key: std::mem::take(&mut st.key), remember: st.remember });
            }
        }
        ui.add_space(10.0);
        // Where the key is kept.
        match chat.place {
            KeyPlace::Browser => {
                for (k, (title, note)) in [
                    ("This tab only", "The key is gone when this tab closes."),
                    ("Remember in this browser", "Encrypted on this device, for the next visit."),
                ]
                .into_iter()
                .enumerate()
                {
                    let on = st.remember == (k == 1);
                    let (row, resp) = ui.allocate_exact_size(vec2(w, 38.0), Sense::click());
                    let p = ui.painter();
                    if resp.hovered() && !has {
                        p.rect_filled(row, CornerRadius::same(8), th().hover);
                    }
                    let c = pos2(row.left() + 16.0, row.top() + 12.0);
                    p.circle_stroke(c, 6.0, Stroke::new(1.5, if on { th().accent } else { th().line2 }));
                    if on {
                        p.circle_filled(c, 3.0, th().accent);
                    }
                    p.text(
                        pos2(row.left() + 30.0, row.top() + 12.0),
                        Align2::LEFT_CENTER,
                        title,
                        sans(12.0),
                        if on { th().text } else { th().muted },
                    );
                    p.text(
                        pos2(row.left() + 30.0, row.top() + 27.0),
                        Align2::LEFT_CENTER,
                        note,
                        sans(11.0),
                        th().faint,
                    );
                    let id = if k == 0 { "ai-tab-only" } else { "ai-remember" };
                    hits.push((id.into(), row));
                    if resp.clicked() && !has {
                        st.remember = k == 1;
                    }
                }
                if st.remember && !has {
                    ui.add_space(4.0);
                    let g = wrap(
                        ui,
                        "Any script that runs on this page could still read a remembered key. Use a key with a spend limit.",
                        sans(11.5),
                        th().text,
                        w - 40.0,
                        4,
                    );
                    let (r, _) = ui.allocate_exact_size(vec2(w, g.size().y + 20.0), Sense::hover());
                    let p = ui.painter();
                    p.rect(r, CornerRadius::same(8), th().mid_bg, Stroke::new(1.0, th().mid_line), StrokeKind::Inside);
                    p.text(
                        pos2(r.left() + 17.0, r.top() + 18.5),
                        Align2::CENTER_CENTER,
                        icon::TRIANGLE_ALERT,
                        icon_font(13.0),
                        th().mid,
                    );
                    p.galley(pos2(r.left() + 30.0, r.top() + 10.0), g, th().text);
                }
            }
            place => {
                let text = match place {
                    KeyPlace::Keychain => {
                        "Kept in your system keychain and read only when a request goes out. Never in a scene file."
                    }
                    _ => "Kept in memory until vxr closes.",
                };
                let g = wrap(ui, text, sans(11.5), th().muted, w, 4);
                let (r, _) = ui.allocate_exact_size(vec2(w, g.size().y), Sense::hover());
                ui.painter().galley(r.min, g, th().muted);
            }
        }
        ui.add_space(12.0);
        let host = Settings { kind, base: st.base.clone(), ..chat.settings.clone() }.host();
        let g = wrap(
            ui,
            &format!(
                "Requests go straight from vxr to {host}, with the scene summary and your words. No vxr server sees them."
            ),
            sans(11.5),
            th().faint,
            w,
            4,
        );
        let (r, _) = ui.allocate_exact_size(vec2(w, g.size().y), Sense::hover());
        ui.painter().galley(r.min, g, th().faint);
        if chat.ready() {
            ui.add_space(14.0);
            let (r, _) = ui.allocate_exact_size(vec2(w, 32.0), Sense::hover());
            let done = Rect::from_min_size(pos2(r.right() - 88.0, r.top()), vec2(88.0, 32.0));
            hits.push(("ai-done".into(), done));
            if accent_button(ui, done, None, "Done", "ai-done", true).clicked() {
                st.key_kind = None;
                done_clicked = true;
            }
        }
        (focus, done_clicked)
    }

    /// The model menu, above the composer's chip.
    fn ai_model_menu(&mut self, ctx: &Context, chip: Rect, out: &mut Vec<AiAction>, hits: &mut Vec<(String, Rect)>) {
        let w = 270.0;
        let n = MODELS.len() + 1;
        let h = 8.0 + n as f32 * 40.0 + 8.0;
        let rect = Rect::from_min_size(pos2(chip.left() - 6.0, chip.top() - 8.0 - h), vec2(w, h));
        let chat = &self.chat;
        let mut picked = None;
        Self::panel_filled(ctx, "ai-menu", rect, Order::Foreground, th().panel_solid, |ui, rect| {
            for k in 0..n {
                let row = Rect::from_min_size(
                    pos2(rect.left() + 8.0, rect.top() + 8.0 + k as f32 * 40.0),
                    vec2(w - 16.0, 40.0),
                );
                let (label, note, s) = match MODELS.get(k) {
                    Some(m) => (
                        m.id.to_string(),
                        m.note,
                        Settings { kind: m.kind, model: m.id.into(), ..chat.settings.clone() },
                    ),
                    None => {
                        let s = if chat.settings.kind == Kind::Compatible {
                            chat.settings.clone()
                        } else {
                            Settings {
                                kind: Kind::Compatible,
                                model: self.ai.model.clone(),
                                base: self.ai.base.clone(),
                                ..chat.settings.clone()
                            }
                        };
                        (s.model.clone(), "Compatible endpoint", s)
                    }
                };
                let on = chat.settings.kind == s.kind && chat.settings.model == s.model;
                let resp = ui.interact(row, Id::unique(("ai-menu", k)), Sense::click());
                let p = ui.painter();
                if resp.hovered() {
                    p.rect_filled(row, CornerRadius::same(7), th().hover);
                }
                p.text(
                    pos2(row.left() + 10.0, row.top() + 13.0),
                    Align2::LEFT_CENTER,
                    &label,
                    mono(12.0),
                    if on { th().accent } else { th().text },
                );
                p.text(pos2(row.left() + 10.0, row.top() + 28.0), Align2::LEFT_CENTER, note, sans(11.0), th().faint);
                if on {
                    p.text(
                        pos2(row.right() - 12.0, row.center().y),
                        Align2::CENTER_CENTER,
                        icon::CHECK,
                        icon_font(13.0),
                        th().accent,
                    );
                } else if s.kind.needs_key() && !chat.has_key[s.kind.index()] {
                    p.text(
                        pos2(row.right() - 10.0, row.center().y),
                        Align2::RIGHT_CENTER,
                        "no key",
                        mono(10.0),
                        th().faint,
                    );
                }
                hits.push((format!("ai-menu-{k}"), row));
                if resp.clicked() {
                    picked = Some(s);
                }
            }
        });
        let clicked_out = ctx.input(|i| i.pointer.primary_clicked())
            && ctx.input(|i| i.pointer.interact_pos()).is_some_and(|q| !rect.contains(q) && !chip.contains(q));
        if let Some(s) = picked {
            self.ai.menu = false;
            if s.kind.needs_key() && !self.chat.has_key[s.kind.index()] {
                self.open_ai_setup(Some(s.kind));
            }
            out.push(AiAction::Settings(s));
        } else if clicked_out {
            self.ai.menu = false;
        }
    }

    /// Cmd-K "Ask AI": a one-line prompt bar left of where the review card goes. Enter sends
    /// the request into the panel's conversation; the bar then follows it, and closes once its
    /// change shows in the review card (an answer without a change, or a failed one, stays
    /// until Escape).
    pub(super) fn ask_bar(&mut self, ctx: &Context, view: Rect) {
        self.ai.bar_bottom = None;
        let Some(mut bar) = self.ask_bar.take() else { return };
        let answer = self.chat.last_answer().cloned();
        let done = bar.sent && !self.chat.running;
        let failed = matches!(answer.as_ref().map(|a| &a.progress), Some(Progress::Failed(_)));
        if done && self.review.is_some() && !self.chat.stop_ask && !failed {
            return;
        }
        let right = view.right() - 12.0 - PALETTE_W - 8.0 - 304.0 - 8.0;
        let (l, r) = (12.0 + self.rail_w + 12.0, right.max(12.0 + self.rail_w + 12.0 + 360.0));
        let w = 560.0_f32.min(r - l - 16.0).max(320.0);
        let cx = (l + r) / 2.0;
        let result = (done && !self.chat.stop_ask).then_some(answer.as_ref()).flatten().map(|a| match &a.progress {
            Progress::Failed(e) => (e.clone(), th().danger),
            _ if a.text.trim().is_empty() => ("No change, and nothing to say.".to_string(), th().muted),
            _ => (a.text.trim().to_string(), th().text),
        });
        let res_g = result.as_ref().map(|(t, c)| {
            ctx.fonts_mut(|f| {
                let mut job = LayoutJob::single_section(
                    t.clone(),
                    TextFormat { font_id: sans(12.5), color: *c, line_height: Some(18.0), ..Default::default() },
                );
                job.wrap = egui::text::TextWrapping {
                    max_width: w - 28.0,
                    max_rows: 4,
                    break_anywhere: false,
                    overflow_character: Some('…'),
                };
                f.layout_job(job)
            })
        });
        let stop_h = if bar.sent && self.chat.stop_ask { 44.0 } else { 0.0 };
        let h = 48.0 + res_g.as_ref().map_or(0.0, |g| g.size().y + 14.0) + stop_h;
        let rect = Rect::from_min_size(pos2((cx - w / 2.0).round(), TOP), vec2(w, h));
        let mut out = Vec::new();
        let mut hits: Vec<(String, Rect)> = Vec::new();
        let (mut close, mut panel, mut send) = (false, false, None);
        let model = self.chat.settings.model.clone();
        let running = self.chat.running;
        let id = Id::unique("ask-bar-field");
        Self::panel(ctx, "ask-bar", rect, Order::Foreground, |ui, rect| {
            let cy = rect.top() + 24.0;
            let p = ui.painter().clone();
            p.text(pos2(rect.left() + 22.0, cy), Align2::CENTER_CENTER, icon::SPARKLES, icon_font(16.0), th().accent);
            let mut x = rect.right() - 10.0;
            if bar.sent {
                // Close (when it is over) or Stop, and Open in the panel.
                let b = Rect::from_center_size(pos2(x - 14.0, cy), vec2(28.0, 28.0));
                x -= 32.0;
                if running {
                    let s = Rect::from_min_size(pos2(b.right() - 72.0, cy - 14.0), vec2(72.0, 28.0));
                    x = s.left() - 4.0;
                    hits.push(("ask-bar-stop".into(), s));
                    if small_button(ui, s, icon::SQUARE, "Stop", "ask-bar-stop", false).clicked() {
                        out.push(AiAction::Stop);
                    }
                } else {
                    hits.push(("ask-bar-close".into(), b));
                    if icon_button(ui, b, icon::X, "ask-bar-close", false).clicked() {
                        close = true;
                    }
                }
                let pb = Rect::from_center_size(pos2(x - 14.0, cy), vec2(28.0, 28.0));
                x -= 32.0;
                hits.push(("ask-bar-panel".into(), pb));
                if icon_button(ui, pb, icon::PANEL_RIGHT, "ask-bar-panel", false).clicked() {
                    panel = true;
                }
                let status = match answer.as_ref().map(|a| &a.progress) {
                    Some(Progress::Working(s)) => format!("{s}…"),
                    Some(Progress::Stopped) => "Stopped".into(),
                    Some(Progress::Failed(_)) => "Failed".into(),
                    _ => "Done".into(),
                };
                let sg = p.layout_no_wrap(status, sans(12.0), th().muted);
                let sw = sg.size().x;
                p.galley(pos2(x - sw - 4.0, cy - sg.size().y / 2.0), sg, th().muted);
                let tl = rect.left() + 42.0;
                let mut job = LayoutJob::single_section(
                    bar.text.clone(),
                    TextFormat { font_id: sans(13.0), color: th().text, ..Default::default() },
                );
                job.wrap = egui::text::TextWrapping {
                    max_width: (x - sw - 16.0 - tl).max(1.0),
                    max_rows: 1,
                    break_anywhere: true,
                    overflow_character: Some('…'),
                };
                let g = p.layout_job(job);
                p.galley(pos2(tl, cy - g.size().y / 2.0), g, th().text);
                if running {
                    ui.ctx().request_repaint();
                }
            } else {
                let kw = kbd(ui, pos2(x, cy), "↵", true);
                x -= kw + 8.0;
                let mg = p.layout_no_wrap(model.clone(), mono(11.0), th().faint);
                let mw = mg.size().x;
                p.galley(pos2(x - mw, cy - mg.size().y / 2.0), mg, th().faint);
                x -= mw + 12.0;
                let tr = Rect::from_min_max(pos2(rect.left() + 42.0, cy - 9.0), pos2(x, cy + 9.0));
                if bar.text.is_empty() {
                    p.text(
                        pos2(tr.left(), cy),
                        Align2::LEFT_CENTER,
                        "Ask AI to change the scene…",
                        sans(13.0),
                        th().faint,
                    );
                }
                if ui.memory(|m| m.has_focus(id)) && ui.input_mut(|i| i.consume_key(Modifiers::NONE, Key::Enter)) {
                    send = Some(bar.text.clone());
                }
                let edit = egui::TextEdit::singleline(&mut bar.text)
                    .id(id)
                    .font(sans(13.0))
                    .text_color(th().text)
                    .frame(egui::Frame::NONE)
                    .margin(egui::Margin::ZERO)
                    .desired_width(tr.width());
                let resp = ui.put(tr, edit);
                if bar.focus {
                    resp.request_focus();
                    bar.focus = false;
                }
                hits.push(("ask-bar".into(), tr));
            }
            let mut y = rect.top() + 48.0;
            if let Some(g) = res_g {
                p.line_segment([pos2(rect.left() + 1.0, y), pos2(rect.right() - 1.0, y)], Stroke::new(1.0, th().line));
                let gh = g.size().y;
                p.galley(pos2(rect.left() + 14.0, y + 7.0), g, th().text);
                y += gh + 14.0;
            }
            if stop_h > 0.0 {
                p.line_segment([pos2(rect.left() + 1.0, y), pos2(rect.right() - 1.0, y)], Stroke::new(1.0, th().line));
                p.text(
                    pos2(rect.left() + 14.0, y + 22.0),
                    Align2::LEFT_CENTER,
                    "Stopped. Keep what it staged?",
                    sans(12.0),
                    th().text,
                );
                let keep = Rect::from_min_size(pos2(rect.right() - 10.0 - 72.0, y + 9.0), vec2(72.0, 26.0));
                let drop = keep.translate(vec2(-78.0, 0.0));
                hits.push(("ask-bar-keep".into(), keep));
                hits.push(("ask-bar-drop".into(), drop));
                if small_button(ui, drop, icon::TRASH, "Drop", "ask-bar-drop", false).clicked() {
                    out.push(AiAction::Drop);
                }
                if small_button(ui, keep, icon::CHECK, "Keep", "ask-bar-keep", true).clicked() {
                    out.push(AiAction::Keep);
                }
            }
        });
        if panel {
            self.ai_open = true;
            self.ai_setup = false;
            close = true;
        }
        if let Some(t) = send
            && self.ai_send(&t)
        {
            bar.sent = true;
            bar.text = t.trim().to_string();
            ctx.memory_mut(|m| m.surrender_focus(id));
        }
        self.ai_out.extend(out);
        self.hits.extend(hits);
        self.hits.insert("ask-bar-box".into(), rect);
        if !close {
            self.ai.bar_bottom = Some(rect.bottom());
            self.ask_bar = Some(bar);
        }
    }
}
