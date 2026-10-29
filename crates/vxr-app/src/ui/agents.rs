//! Terminal agents in the Hybrid UI (`docs/research/ai.md` section 3, live mode, after the
//! `mcp.html` and `mcp-live.html` mocks): the assistant panel's Agents tab (Cmd-K), a checklist
//! with "Allow agents to edit this window", the setup line and the agents connected, then the
//! review switch and the Agent edits list, each applied call with an Undo of its own; and the
//! chip in the hint bar while an agent is connected, with Stop. In the browser the tab says to
//! run `vxr bridge` and takes the pairing code it prints ([`crate::bridge`]). Drawn in the
//! panel style.

pub use crate::agents::Fail;
use egui::{ScrollArea, UiBuilder};

use super::*;

/// The setup lines the panel shows, for Claude Code: `vxr setup claude` registers files mode as
/// `vxr` and live mode as `vxr-live`, and the manual line adds only `vxr-live`, so neither
/// replaces the files mode server.
pub const SETUP: &str = "vxr setup claude";
pub const LIVE_SETUP: &str = "claude mcp add vxr-live -- vxr mcp --live";
pub const FILES_SETUP: &str = "claude mcp add vxr -- vxr mcp --root <folder>";
/// The browser's first step: the bridge between this tab and `vxr mcp --live`.
pub const BRIDGE_SETUP: &str = "vxr bridge";

/// Most Agent edits rows shown, newest first, and how tall each is.
const CARD_ROWS: usize = 8;
const EDIT_ROW: f32 = 30.0;

/// Terminal agents as the UI shows them (set by the editor).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct AgentsView {
    /// Live mode can run here (the desktop app on a Unix system, or the browser through
    /// `vxr bridge`).
    pub available: bool,
    /// The browser's variant: a pairing code for `vxr bridge`, not a socket of its own.
    pub web: bool,
    /// The browser is connecting to `vxr bridge`.
    pub connecting: bool,
    pub allowed: bool,
    /// "Review every agent change" is on.
    pub review: bool,
    /// Why turning it on failed.
    pub error: Option<String>,
    /// The connected agents: connection and name.
    pub clients: Vec<(u64, String)>,
    /// Every applied call, oldest first.
    pub edits: Vec<EditRow>,
    /// What kind of failure `error` is.
    pub fail: Option<Fail>,
    /// When the link came up (egui time, seconds), while it is up.
    pub since: Option<f64>,
    /// For each of `clients`: when it was first listed, and when it last called.
    pub times: Vec<(f64, Option<f64>)>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct EditRow {
    /// The agent's connection, which tells two agents of the same name apart.
    pub conn: u64,
    pub label: String,
    /// The edit is in the scene (not undone).
    pub applied: bool,
}

fn wrapped(ctx: &Context, text: &str, font: FontId, color: Color32, width: f32) -> Arc<egui::Galley> {
    let mut job = LayoutJob::single_section(
        text.to_string(),
        TextFormat { font_id: font, color, line_height: Some(17.0), ..Default::default() },
    );
    job.wrap.max_width = width;
    ctx.fonts_mut(|f| f.layout_job(job))
}

/// One line, cut with an ellipsis to `width`.
fn one_line(ui: &Ui, text: &str, font: FontId, color: Color32, width: f32) -> Arc<egui::Galley> {
    let mut job =
        LayoutJob::single_section(text.to_string(), TextFormat { font_id: font, color, ..Default::default() });
    job.wrap = egui::text::TextWrapping {
        max_width: width.max(1.0),
        max_rows: 1,
        break_anywhere: true,
        overflow_character: Some('…'),
    };
    ui.painter().layout_job(job)
}

/// An on and off switch, 30 by 18.
fn switch(ui: &mut Ui, r: Rect, id: &str, on: bool) -> egui::Response {
    let resp = ui.interact(r, Id::unique(id), Sense::click());
    let p = ui.painter();
    let fill = if on { th().accent } else { th().tint(28) };
    p.rect_filled(r, CornerRadius::same(9), fill);
    let x = if on { r.right() - 9.0 } else { r.left() + 9.0 };
    let knob = if on { th().on_accent } else { th().muted };
    p.circle_filled(pos2(x, r.center().y), 6.5, knob);
    resp
}

impl Hybrid {
    /// Open the assistant panel on its Agents tab (Cmd-K), closing what would sit over it.
    pub fn open_agents(&mut self) {
        self.agents_panel = true;
        self.ai_open = true;
        self.ai_setup = false;
        self.export_menu = false;
        self.view_panel = false;
    }

    /// The agent chip at the left of the hint bar: who is editing, and Stop. Returns its
    /// width, 0 with no agent connected.
    pub(super) fn agent_chip_width(&self, ctx: &Context) -> f32 {
        if self.agents.clients.is_empty() {
            return 0.0;
        }
        let g = ctx.fonts_mut(|f| f.layout_no_wrap(self.agent_chip_text(), sans_medium(12.0), th().text));
        14.0 + g.size().x + 10.0 + 58.0 + 14.0
    }

    fn agent_chip_text(&self) -> String {
        match self.agents.clients.as_slice() {
            [(_, one)] => format!("{one} is editing"),
            many => format!("{} agents are editing", many.len()),
        }
    }

    /// Draw the chip into the hint panel's `ui`, starting at `x`.
    pub(super) fn agent_chip(&mut self, ui: &mut Ui, rect: Rect, w: f32) {
        if w == 0.0 {
            return;
        }
        let cy = rect.center().y;
        let x = rect.left() + 12.0;
        let t = ui.input(|i| i.time);
        let pulse = 0.55 + 0.45 * (t * 3.0).sin().abs() as f32;
        ui.painter().circle_filled(pos2(x + 3.5, cy), 3.5, th().ok.gamma_multiply(pulse));
        ui.ctx().request_repaint();
        let g = ui.painter().layout_no_wrap(self.agent_chip_text(), sans_medium(12.0), th().text);
        let gw = g.size().x;
        ui.painter().galley(pos2(x + 13.0, cy - g.size().y / 2.0), g, th().text);
        let stop = Rect::from_min_size(pos2(x + 13.0 + gw + 10.0, cy - 11.0), vec2(56.0, 22.0));
        if small_button(ui, stop, icon::SQUARE, "Stop", "agent-stop", false).clicked() {
            self.actions.push(Action::StopAgents);
        }
        self.hits.insert("agent-stop".into(), stop);
        self.hits.insert("agent-chip".into(), Rect::from_min_max(rect.min, pos2(rect.left() + w, rect.bottom())));
        let sx = rect.left() + w - 0.5;
        ui.painter()
            .line_segment([pos2(sx, rect.top() + 8.0), pos2(sx, rect.bottom() - 8.0)], Stroke::new(1.0, th().line));
    }

    // The Agents tab of the assistant panel: a checklist that ends with the agents (docs/
    // decisions.md). A banner says where the link stands, a bar counts the steps done, and each
    // step ticks green and folds to one line, or turns red and holds its error. In the browser:
    // run `vxr bridge`, type the code it prints (the eighth character connects), connect an
    // agent; in the desktop app: allow the window, connect an agent. Then the review switch,
    // the Agent edits list and the footer.
    pub(super) fn agents_tab(&mut self, ctx: &Context) {
        let Some(body) = self.ai_body.filter(|_| self.ai_open && self.agents_panel) else { return };
        let w = body.width() - 6.0;
        let now = ctx.input(|i| i.time);
        // A change under review shows its card at the top, as it does in the chat.
        let review_h = self.review_height(ctx, w - 24.0);
        let mut slot_out = None;
        let mut tab = Tab {
            v: self.agents.clone(),
            now,
            w,
            hits: Vec::new(),
            actions: Vec::new(),
            copied: None,
            open_url: None,
            code: std::mem::take(&mut self.agents_code),
            sent: self.agents_code_sent,
            paired_code: std::mem::take(&mut self.agents_paired_code),
            refused: self.agents_refused,
            focus: false,
        };
        Self::bare(ctx, "agents-tab", body, body, |ui, body| {
            ui.scope_builder(UiBuilder::new().max_rect(body), |ui| {
                ScrollArea::vertical().id_salt("agents-tab").auto_shrink([false; 2]).show(ui, |ui| {
                    ui.spacing_mut().item_spacing = vec2(0.0, 0.0);
                    ui.add_space(6.0);
                    if let Some(h) = review_h {
                        let (r, _) = ui.allocate_exact_size(vec2(w, h), Sense::hover());
                        slot_out = Some(Rect::from_min_size(r.min + vec2(12.0, 0.0), vec2(w - 24.0, h)));
                        ui.add_space(14.0);
                    }
                    tab.draw(ui);
                    ui.add_space(12.0);
                });
            });
        });
        self.review_slot = slot_out.map(|r| (r, body, None));
        self.agents_code = tab.code;
        self.agents_code_sent = tab.sent;
        self.agents_paired_code = tab.paired_code;
        self.agents_refused = tab.refused;
        self.ai.focus |= tab.focus;
        if let Some(text) = tab.copied {
            ctx.copy_text(text);
            self.toast(ctx, "Copied the setup line");
        }
        if let Some(url) = tab.open_url {
            ctx.open_url(egui::OpenUrl::new_tab(url));
        }
        self.hits.extend(tab.hits);
        self.hits.insert("agents-panel".into(), body);
        self.actions.extend(tab.actions);
    }

    /// What the Agents tab's label shows: a dot for the link's state and how many agents are
    /// connected.
    pub(super) fn agents_status(&self) -> (Option<Tone>, usize) {
        let v = &self.agents;
        let tone = if v.fail.is_some() {
            Some(Tone::Bad)
        } else if v.connecting {
            Some(Tone::Busy)
        } else if v.allowed {
            Some(Tone::Ok)
        } else {
            None
        };
        (tone, v.clients.len())
    }
}

/// How a banner, a dot or a bar segment reads.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) enum Tone {
    Plain,
    Busy,
    Ok,
    Bad,
}

impl Tone {
    pub(super) fn color(self) -> Color32 {
        match self {
            Tone::Plain => th().faint,
            Tone::Busy => th().mid,
            Tone::Ok => th().ok,
            Tone::Bad => th().danger,
        }
    }
}

/// Where a checklist step stands.
#[derive(Clone, Copy, PartialEq)]
enum St {
    Todo,
    Active,
    Busy,
    Done,
    Bad,
}

/// The browser's code field: eight characters in two groups of four.
const CELL_W: f32 = 29.0;
const CELL_H: f32 = 38.0;
const CELL_GAP: f32 = 4.0;
const DASH_W: f32 = 16.0;

/// How long a refused character's word stays.
const REFUSED_SECS: f64 = 4.0;

/// An elapsed time, in a few words.
fn ago(secs: f64) -> String {
    let s = secs.max(0.0) as u64;
    match s {
        0..=4 => "just now".into(),
        5..=59 => format!("{s} s"),
        60..=3599 => format!("{} min", s / 60),
        _ => format!("{} h", s / 3600),
    }
}

/// An agent is editing while it called in the last seconds.
fn editing(now: f64, last: Option<f64>) -> bool {
    last.is_some_and(|t| now - t < 5.0)
}

/// What a failure says, and what the step offers.
fn failure_words(f: Fail, error: &str) -> (String, String) {
    match f {
        Fail::Wrong => (
            "vxr bridge refused that code".into(),
            "Check the code in the terminal and type it again. After 5 wrong codes vxr bridge prints a new one.".into(),
        ),
        Fail::Denied => (
            "This browser blocked access to this computer".into(),
            "Click the icon left of the address, allow Apps on device (or Local network access), then try again.".into(),
        ),
        Fail::Blocked => (
            "This browser cannot reach vxr bridge".into(),
            "It does not let web pages reach apps on this computer. Use Chrome, Edge or Firefox, or the desktop app.".into(),
        ),
        Fail::Safari => (
            "Safari cannot reach vxr bridge".into(),
            "Safari does not let web pages reach apps on this computer. Use Chrome, Edge or Firefox, or the desktop app.".into(),
        ),
        Fail::Brave => (
            "Brave blocks vxr bridge".into(),
            "Allow Localhost access for this site in Brave's settings, then try again.".into(),
        ),
        Fail::Nothing => (
            format!("Nothing answered on 127.0.0.1:{}", vxr_mcp::bridge::PORT),
            "Run vxr bridge in a terminal, then try again. If the browser asks to let this page reach apps on this device, allow it.".into(),
        ),
        Fail::Lost => (
            "vxr bridge stopped".into(),
            "Agents can no longer edit this tab. Run vxr bridge again and type its new code.".into(),
        ),
        Fail::Hung => ("vxr bridge hung up".into(), "It closed before it took the code. Try again.".into()),
        Fail::Other => ("Could not connect".into(), error.to_string()),
    }
}

/// The desktop app's page, for a browser that cannot reach vxr bridge.
const DESKTOP_URL: &str = "https://github.com/jrnxf/vxr#install";

/// The Agents tab while it draws: the view, what it collects and the code being typed.
struct Tab {
    v: AgentsView,
    now: f64,
    w: f32,
    hits: Vec<(String, Rect)>,
    actions: Vec<Action>,
    copied: Option<String>,
    open_url: Option<&'static str>,
    code: String,
    sent: bool,
    paired_code: String,
    refused: Option<(char, f64)>,
    /// The code field has the keyboard: Escape leaves it first.
    focus: bool,
}

impl Tab {
    /// A block `h` tall across the tab, inset by the panel's padding.
    fn block(&self, ui: &mut Ui, h: f32) -> Rect {
        let (r, _) = ui.allocate_exact_size(vec2(self.w, h), Sense::hover());
        Rect::from_min_max(pos2(r.left() + 12.0, r.top()), pos2(r.right() - 12.0, r.bottom()))
    }

    fn text_h(ui: &Ui, text: &str, font: FontId, w: f32) -> Arc<egui::Galley> {
        wrapped(ui.ctx(), text, font, th().muted, w)
    }

    fn banner(&self, ui: &mut Ui, tone: Tone, text: &str, right: &str) {
        let r = self.block(ui, 34.0);
        let p = ui.painter();
        let (fill, line) = match tone {
            Tone::Plain => (th().tint(9), th().line),
            Tone::Busy => (th().mid_bg, th().mid_line),
            Tone::Ok => (th().ok.gamma_multiply(0.12), th().ok.gamma_multiply(0.4)),
            Tone::Bad => (th().danger_bg, th().danger_line),
        };
        p.rect(r, CornerRadius::same(9), fill, Stroke::new(1.0, line), StrokeKind::Inside);
        let c = pos2(r.left() + 14.0, r.center().y);
        if tone == Tone::Plain {
            p.circle_stroke(c, 3.5, Stroke::new(1.2, th().faint));
        } else {
            p.circle_filled(c, 3.5, tone.color());
        }
        p.text(pos2(r.left() + 26.0, r.center().y), Align2::LEFT_CENTER, text, sans_medium(12.5), th().text);
        p.text(pos2(r.right() - 12.0, r.center().y), Align2::RIGHT_CENTER, right, mono(11.0), th().faint);
        ui.add_space(8.0);
    }

    /// The bar: one segment per step.
    fn progress(&self, ui: &mut Ui, steps: &[St]) {
        let r = self.block(ui, 3.0);
        let n = steps.len() as f32;
        let gap = 4.0;
        let seg = (r.width() - gap * (n - 1.0)) / n;
        for (i, st) in steps.iter().enumerate() {
            let color = match st {
                St::Done => th().ok,
                St::Busy => th().mid,
                St::Bad => th().danger,
                St::Todo | St::Active => th().tint(28),
            };
            let x = r.left() + i as f32 * (seg + gap);
            ui.painter().rect_filled(
                Rect::from_min_size(pos2(x, r.top()), vec2(seg, 3.0)),
                CornerRadius::same(2),
                color,
            );
        }
        ui.add_space(16.0);
    }

    /// A step's head: the mark, the title and the time at the right, then room for a body
    /// `body_h` tall under it, which is returned. A line joins the marks of the steps.
    #[allow(clippy::too_many_arguments)]
    fn step(&self, ui: &mut Ui, st: St, n: usize, title: &str, right: Option<String>, last: bool, body_h: f32) -> Rect {
        let h = 22.0 + if body_h > 0.0 { 8.0 + body_h } else { 0.0 } + if last { 0.0 } else { 14.0 };
        let r = self.block(ui, h);
        let p = ui.painter();
        let c = pos2(r.left() + 10.0, r.top() + 11.0);
        if !last {
            p.line_segment([pos2(c.x, c.y + 11.0), pos2(c.x, r.bottom())], Stroke::new(1.0, th().line2));
        }
        match st {
            St::Done => {
                p.circle_filled(c, 10.0, th().ok);
                p.text(c, Align2::CENTER_CENTER, icon::CHECK, icon_font(13.0), th().on_accent);
            }
            St::Bad => {
                p.circle_filled(c, 10.0, th().danger);
                p.text(c, Align2::CENTER_CENTER, icon::X, icon_font(13.0), th().on_accent);
            }
            St::Busy => {
                let t = ui.input(|i| i.time);
                let pulse = 0.55 + 0.45 * (t * 4.0).sin().abs() as f32;
                p.circle_stroke(c, 9.5, Stroke::new(1.5, th().mid.gamma_multiply(pulse)));
                ui.ctx().request_repaint();
            }
            St::Active => {
                p.circle_stroke(c, 9.5, Stroke::new(1.2, th().text));
                p.text(c, Align2::CENTER_CENTER, n.to_string(), mono_medium(10.5), th().text);
            }
            St::Todo => {
                p.circle_stroke(c, 9.5, Stroke::new(1.0, th().line2));
                p.text(c, Align2::CENTER_CENTER, n.to_string(), mono_medium(10.5), th().faint);
            }
        }
        let color = if st == St::Todo { th().muted } else { th().text };
        p.text(pos2(r.left() + 30.0, c.y), Align2::LEFT_CENTER, title, sans_semibold(12.5), color);
        if let Some(t) = right {
            p.text(pos2(r.right(), c.y), Align2::RIGHT_CENTER, t, mono(11.0), th().faint);
        }
        Rect::from_min_size(pos2(r.left() + 30.0, r.top() + 22.0 + 8.0), vec2(r.width() - 30.0, body_h))
    }

    /// A command with its Copy: `cmd` is what is copied, drawn after a `$`.
    fn command(&mut self, ui: &mut Ui, r: Rect, cmd: &str, id: &str) {
        let p = ui.painter();
        p.rect(r, CornerRadius::same(8), th().tint(9), Stroke::new(1.0, th().line), StrokeKind::Inside);
        p.text(pos2(r.left() + 10.0, r.center().y), Align2::LEFT_CENTER, "$", mono(11.5), th().faint);
        let b = Rect::from_center_size(pos2(r.right() - 17.0, r.center().y), vec2(24.0, 24.0));
        let g = one_line(ui, cmd, mono(11.5), th().text, b.left() - 6.0 - (r.left() + 24.0));
        ui.painter().galley(pos2(r.left() + 24.0, r.center().y - g.size().y / 2.0), g, th().text);
        let resp = ui.interact(b, Id::unique(id), Sense::click());
        if resp.hovered() {
            ui.painter().rect_filled(b, CornerRadius::same(6), th().hover);
        }
        ui.painter().text(
            b.center(),
            Align2::CENTER_CENTER,
            icon::COPY,
            icon_font(13.0),
            if resp.hovered() { th().text } else { th().muted },
        );
        if resp.clicked() {
            self.copied = Some(cmd.to_string());
        }
        self.hits.push((id.to_string(), b));
        self.hits.push((format!("{id}-line"), r));
    }

    /// The error box: a title and what to do, tinted red.
    fn error_box(&self, ui: &mut Ui, r: Rect, title: &str, text: &Arc<egui::Galley>) {
        let p = ui.painter();
        p.rect(r, CornerRadius::same(8), th().danger_bg, Stroke::new(1.0, th().danger_line), StrokeKind::Inside);
        p.text(
            pos2(r.left() + 16.0, r.top() + 16.0),
            Align2::CENTER_CENTER,
            icon::TRIANGLE_ALERT,
            icon_font(13.0),
            th().danger,
        );
        let g = ui.painter().layout(title.into(), sans_semibold(12.0), th().text, r.width() - 40.0);
        let gh = g.size().y;
        ui.painter().galley(pos2(r.left() + 30.0, r.top() + 9.0), g, th().text);
        ui.painter().galley(pos2(r.left() + 30.0, r.top() + 9.0 + gh + 3.0), text.clone(), th().muted);
    }

    /// The code field: the cells, drawn from the text of a transparent field over them. Returns
    /// whether the text changed.
    fn cells(&mut self, ui: &mut Ui, r: Rect, bad: bool, live: bool) {
        let id = Id::unique("agents-code");
        let before = self.code.clone();
        if live {
            ui.scope(|ui| {
                ui.visuals_mut().text_cursor.stroke = Stroke::NONE;
                ui.visuals_mut().selection.bg_fill = Color32::TRANSPARENT;
                ui.visuals_mut().selection.stroke = Stroke::NONE;
                let edit = egui::TextEdit::singleline(&mut self.code)
                    .id(id)
                    .font(mono(17.0))
                    .text_color(Color32::TRANSPARENT)
                    .frame(egui::Frame::NONE)
                    .margin(egui::Margin::ZERO)
                    .desired_width(r.width());
                ui.put(r, edit);
            });
            // As typed or pasted: upper case, O as 0, I and L as 1, no spaces or dashes, at most
            // eight characters; a character no code has is refused, with a word.
            let mut clean = String::new();
            for c in self.code.chars() {
                if vxr_mcp::bridge::code_char_skipped(c) {
                    continue;
                }
                match vxr_mcp::bridge::code_char(c) {
                    Some(c) if clean.len() < vxr_mcp::bridge::CODE_LEN => clean.push(c),
                    Some(_) => {}
                    None => self.refused = Some((c, self.now)),
                }
            }
            if clean != self.code {
                self.code = clean;
            }
            if self.code != before {
                self.sent = false;
                if self.v.fail.is_some() {
                    self.actions.push(Action::DismissAgentsError);
                }
            }
        }
        let focused = ui.memory(|m| m.has_focus(id));
        self.focus |= focused;
        let n = self.code.chars().count();
        let (line, ink) = if bad { (th().danger_line, th().danger) } else { (th().line2, th().text) };
        let mut x = r.left();
        for i in 0..vxr_mcp::bridge::CODE_LEN {
            if i == 4 {
                ui.painter().text(
                    pos2(x + DASH_W / 2.0, r.center().y),
                    Align2::CENTER_CENTER,
                    "–",
                    sans(14.0),
                    th().faint,
                );
                x += DASH_W;
            }
            let cell = Rect::from_min_size(pos2(x, r.top()), vec2(CELL_W, CELL_H));
            let on = focused && i == n.min(7);
            ui.painter().rect(
                cell,
                CornerRadius::same(7),
                if bad { th().danger_bg } else { th().tint(if live { 9 } else { 14 }) },
                Stroke::new(1.0, if on { th().ring } else { line }),
                StrokeKind::Inside,
            );
            match self.code.chars().nth(i) {
                Some(c) => {
                    ui.painter().text(cell.center(), Align2::CENTER_CENTER, c, mono_medium(17.0), ink);
                }
                None if live => {
                    if on && (ui.input(|i| i.time) * 2.0) as i64 % 2 == 0 {
                        ui.painter().line_segment(
                            [pos2(cell.center().x, cell.top() + 9.0), pos2(cell.center().x, cell.bottom() - 9.0)],
                            Stroke::new(1.0, th().text),
                        );
                    } else if !on {
                        ui.painter().circle_filled(cell.center(), 1.2, th().faint);
                    }
                }
                None => {}
            }
            x += CELL_W + CELL_GAP;
        }
        if focused {
            ui.ctx().request_repaint_after(std::time::Duration::from_millis(500));
        }
        self.hits.push(("agents-code".into(), r));
        // Eight characters connect.
        if live && n == vxr_mcp::bridge::CODE_LEN && !self.sent && self.v.fail.is_none() && !self.v.connecting {
            self.sent = true;
            self.actions.push(Action::PairBridge);
        }
    }

    /// A full-width button under an error: Try again, or the desktop app.
    fn error_button(&mut self, ui: &mut Ui, r: Rect, label: &str, id: &str) -> bool {
        super::assistant::accent_button(ui, r, None, label, id, true);
        self.hits.push((id.to_string(), r));
        ui.interact(r, Id::unique(("agents-btn", id)), Sense::click()).clicked()
    }

    /// An agent row: the name and what it is doing, Stop, and when it came and what it did.
    fn agent(&mut self, ui: &mut Ui, i: usize) {
        let (conn, name) = self.v.clients[i].clone();
        let (since, last) = self.v.times.get(i).copied().unwrap_or((self.now, None));
        let is_editing = editing(self.now, last);
        let n = self.v.edits.iter().filter(|e| e.conn == conn).count();
        let what = self
            .v
            .edits
            .iter()
            .rev()
            .find(|e| e.conn == conn)
            .and_then(|e| e.label.strip_prefix(&format!("{name}: ")))
            .map(str::to_string)
            .unwrap_or_default();
        let r = self.block(ui, 44.0);
        let r = Rect::from_min_max(pos2(r.left() + 30.0, r.top()), r.max);
        let p = ui.painter().clone();
        let c = pos2(r.left() + 4.0, r.top() + 11.0);
        if is_editing {
            p.circle_filled(c, 3.5, th().ok);
            ui.ctx().request_repaint_after(std::time::Duration::from_millis(1000));
        } else {
            p.circle_stroke(c, 3.5, Stroke::new(1.2, th().faint));
        }
        let g = p.layout_no_wrap(name.clone(), sans_semibold(12.5), th().text);
        let gw = g.size().x;
        p.galley(pos2(r.left() + 16.0, c.y - g.size().y / 2.0), g, th().text);
        let (state, color) = if is_editing {
            ("editing now".to_string(), th().ok)
        } else {
            (format!("idle {}", last.map_or(ago(self.now - since), |t| ago(self.now - t))), th().faint)
        };
        p.text(pos2(r.left() + 16.0 + gw + 8.0, c.y), Align2::LEFT_CENTER, state, sans(11.5), color);
        let b = Rect::from_min_size(pos2(r.right() - 58.0, r.top()), vec2(58.0, 22.0));
        let id = format!("agents-stop-{conn}");
        if small_button(ui, b, icon::SQUARE, "Stop", &id, false).clicked() {
            self.actions.push(Action::StopAgent(conn));
        }
        self.hits.push((id, b));
        let mut meta = format!("since {}", ago(self.now - since));
        if n > 0 {
            meta += &format!(" · {n} edit{}", if n == 1 { "" } else { "s" });
        }
        if !what.is_empty() {
            meta += &format!(" · {what}");
        }
        let g = one_line(ui, &meta, sans(11.0), th().faint, r.width() - 16.0);
        ui.painter().galley(pos2(r.left() + 16.0, r.top() + 25.0), g, th().faint);
    }

    /// A switch row: the title, what it says now and the switch.
    fn switch_row(&mut self, ui: &mut Ui, title: &str, sub: &str, id: &str, on: bool) -> bool {
        let r = self.block(ui, 44.0);
        let resp = ui.interact(r, Id::unique((id, "row")), Sense::click());
        let p = ui.painter().clone();
        p.text(pos2(r.left(), r.top() + 14.0), Align2::LEFT_CENTER, title, sans_semibold(12.5), th().text);
        p.text(pos2(r.left(), r.top() + 31.0), Align2::LEFT_CENTER, sub, sans(11.0), th().muted);
        let sw = Rect::from_min_size(pos2(r.right() - 30.0, r.top() + 5.0), vec2(30.0, 18.0));
        let toggled = switch(ui, sw, id, on).clicked() || resp.clicked();
        self.hits.push((id.to_string(), r));
        toggled
    }

    fn draw(&mut self, ui: &mut Ui) {
        let v = self.v.clone();
        let now = self.now;
        if !v.available {
            let intro = Self::text_h(
                ui,
                "Live mode is not built for this system yet. Files mode edits .vox files on disk without a window:",
                sans(12.0),
                self.w - 24.0,
            );
            let r = self.block(ui, intro.size().y + 10.0 + 34.0);
            ui.painter().galley(r.min, intro.clone(), th().muted);
            let line = Rect::from_min_size(pos2(r.left(), r.top() + intro.size().y + 10.0), vec2(r.width(), 34.0));
            self.command(ui, line, FILES_SETUP, "agents-copy-files");
            return;
        }
        let web = v.web;
        let live = v.allowed;
        let clients = v.clients.len();
        let is_editing: Vec<bool> = (0..clients).map(|i| editing(now, v.times.get(i).and_then(|t| t.1))).collect();
        let editing_n = is_editing.iter().filter(|e| **e).count();
        // Where each step stands.
        let (s1, s2, s3) = if web {
            let s1 = match v.fail {
                Some(Fail::Nothing | Fail::Lost) => St::Bad,
                _ if v.connecting || live || v.fail.is_some() => St::Done,
                _ => St::Active,
            };
            let s2 = if live {
                St::Done
            } else if v.connecting {
                St::Busy
            } else if v.fail.is_some() && !matches!(v.fail, Some(Fail::Nothing | Fail::Lost)) {
                St::Bad
            } else {
                St::Active
            };
            (
                s1,
                s2,
                if clients > 0 {
                    St::Done
                } else if live {
                    St::Active
                } else {
                    St::Todo
                },
            )
        } else {
            let s1 = if v.fail.is_some() {
                St::Bad
            } else if live {
                St::Done
            } else {
                St::Active
            };
            (
                s1,
                if clients > 0 {
                    St::Done
                } else if live {
                    St::Active
                } else {
                    St::Todo
                },
                St::Todo,
            )
        };
        let steps: Vec<St> = if web { vec![s1, s2, s3] } else { vec![s1, s2] };
        let done = steps.iter().filter(|s| **s == St::Done).count();
        // The banner.
        let lead = match clients {
            0 => String::new(),
            1 => format!("{} {}", v.clients[0].1, if editing_n == 1 { "is editing" } else { "is connected" }),
            n if editing_n == 0 => format!("{n} agents connected"),
            n => format!("{n} agents, {editing_n} editing"),
        };
        let since = v.since.map(|t| ago(now - t));
        let (tone, text, right) = if live && clients > 0 {
            let right = since
                .clone()
                .map(|s| if web { format!("paired {s}") } else { format!("since {s}") })
                .unwrap_or_default();
            (Tone::Ok, format!("Live · {lead}"), right)
        } else if live {
            let t = if web { "Paired · waiting for an agent" } else { "Listening · waiting for an agent" };
            (Tone::Ok, t.to_string(), format!("{done} of {}", steps.len()))
        } else if v.connecting {
            (Tone::Busy, "Connecting…".to_string(), format!("{done} of {}", steps.len()))
        } else if v.fail.is_some() {
            (Tone::Bad, "Not connected".to_string(), format!("{done} of {}", steps.len()))
        } else {
            (Tone::Plain, if web { "Not connected" } else { "Off" }.to_string(), format!("{done} of {}", steps.len()))
        };
        self.banner(ui, tone, &text, &right);
        self.progress(ui, &steps);
        let bw = self.w - 24.0 - 30.0;
        let err_text = v.error.clone().unwrap_or_default();
        let (err_title, err_body) = v.fail.map(|f| failure_words(f, &err_text)).unwrap_or_default();
        let err_g = Self::text_h(ui, &err_body, sans(11.5), bw - 40.0);
        let err_h =
            9.0 + {
                let t = ui.painter().layout(err_title.clone(), sans_semibold(12.0), th().text, bw - 40.0);
                t.size().y
            } + 3.0
                + err_g.size().y
                + 11.0;
        if web {
            // 1 · Start vxr bridge.
            match s1 {
                St::Done => {
                    let b = self.step(ui, s1, 1, "Start vxr bridge", None, false, 16.0);
                    ui.painter().text(
                        b.left_center(),
                        Align2::LEFT_CENTER,
                        "In a terminal on this computer",
                        sans(11.5),
                        th().faint,
                    );
                }
                _ => {
                    let bad = s1 == St::Bad;
                    let title = if v.fail == Some(Fail::Lost) { "vxr bridge stopped" } else { "Start vxr bridge" };
                    let h = 30.0 + if bad { 8.0 + err_h + 8.0 + 30.0 } else { 8.0 + 16.0 };
                    let b = self.step(ui, s1, 1, title, None, false, h);
                    self.command(
                        ui,
                        Rect::from_min_size(b.min, vec2(b.width(), 30.0)),
                        BRIDGE_SETUP,
                        "agents-copy-bridge",
                    );
                    let mut y = b.top() + 30.0 + 8.0;
                    if bad {
                        let er = Rect::from_min_size(pos2(b.left(), y), vec2(b.width(), err_h));
                        self.error_box(ui, er, &err_title, &err_g);
                        y += err_h + 8.0;
                        let br = Rect::from_min_size(pos2(b.left(), y), vec2(b.width(), 30.0));
                        if self.error_button(ui, br, "Try again", "agents-retry") {
                            self.actions.push(Action::DismissAgentsError);
                            self.code.clear();
                            self.sent = false;
                        }
                    } else {
                        ui.painter().text(
                            pos2(b.left(), y + 6.0),
                            Align2::LEFT_CENTER,
                            "Not installed? brew install jrnxf/tap/vxr",
                            mono(10.5),
                            th().faint,
                        );
                    }
                }
            }
            // 2 · Type the pairing code.
            match s2 {
                St::Done => {
                    let right = since.clone();
                    let b = self.step(ui, s2, 2, "Paired this tab", right, false, 16.0);
                    let code = if self.paired_code.len() == vxr_mcp::bridge::CODE_LEN {
                        format!("Code {} · this computer only", vxr_mcp::bridge::show_code(&self.paired_code))
                    } else {
                        "This computer only".to_string()
                    };
                    ui.painter().text(b.left_center(), Align2::LEFT_CENTER, code, sans(11.5), th().faint);
                }
                St::Busy => {
                    let b =
                        self.step(ui, s2, 2, "Type the pairing code", None, false, CELL_H + 8.0 + 18.0 + 8.0 + 24.0);
                    self.cells(ui, Rect::from_min_size(b.min, vec2(b.width(), CELL_H)), false, false);
                    let y = b.top() + CELL_H + 8.0;
                    ui.painter().text(
                        pos2(b.left(), y + 9.0),
                        Align2::LEFT_CENTER,
                        "Waiting for vxr bridge. Allow it if the browser asks.",
                        sans(11.5),
                        th().mid,
                    );
                    let cb = Rect::from_min_size(pos2(b.left(), y + 18.0 + 8.0), vec2(78.0, 24.0));
                    if small_button(ui, cb, icon::X, "Cancel", "agents-cancel", false).clicked() {
                        self.actions.push(Action::AllowAgents(false));
                    }
                    self.hits.push(("agents-cancel".into(), cb));
                }
                St::Bad => {
                    let has_btn = true;
                    let gh = CELL_H + 8.0 + err_h + 8.0 + if has_btn { 30.0 } else { 0.0 };
                    let b = self.step(ui, s2, 2, "Type the pairing code", None, false, gh);
                    let wrong = v.fail == Some(Fail::Wrong);
                    self.cells(ui, Rect::from_min_size(b.min, vec2(b.width(), CELL_H)), wrong, true);
                    let er = Rect::from_min_size(pos2(b.left(), b.top() + CELL_H + 8.0), vec2(b.width(), err_h));
                    self.error_box(ui, er, &err_title, &err_g);
                    let br = Rect::from_min_size(pos2(b.left(), er.bottom() + 8.0), vec2(b.width(), 30.0));
                    let desktop = matches!(v.fail, Some(Fail::Safari | Fail::Blocked));
                    if desktop {
                        if self.error_button(ui, br, "Get the desktop app", "agents-desktop") {
                            self.open_url = Some(DESKTOP_URL);
                        }
                    } else if self.error_button(ui, br, "Try again", "agents-retry") {
                        self.actions.push(Action::DismissAgentsError);
                        if wrong || self.code.is_empty() {
                            self.code.clear();
                        } else {
                            self.actions.push(Action::PairBridge);
                        }
                        self.sent = !wrong && !self.code.is_empty();
                    }
                }
                _ => {
                    let hint = if v.fail == Some(Fail::Lost) {
                        "Type the new code it prints."
                    } else {
                        "Paste works too. Connects on the 8th character."
                    };
                    let b = self.step(ui, s2, 2, "Type the pairing code", None, false, CELL_H + 8.0 + 16.0);
                    self.cells(ui, Rect::from_min_size(b.min, vec2(b.width(), CELL_H)), false, true);
                    let y = b.top() + CELL_H + 8.0;
                    let refused = self.refused.filter(|(_, t)| now - t < REFUSED_SECS);
                    match refused {
                        Some((c, _)) => {
                            ui.painter().text(
                                pos2(b.left(), y + 8.0),
                                Align2::LEFT_CENTER,
                                format!("No \u{201c}{c}\u{201d} in a code: it has 0-9 and A-Z, without U."),
                                sans(11.5),
                                th().danger,
                            );
                            ui.ctx().request_repaint_after(std::time::Duration::from_millis(500));
                        }
                        None => {
                            ui.painter().text(
                                pos2(b.left(), y + 8.0),
                                Align2::LEFT_CENTER,
                                hint,
                                sans(11.5),
                                th().faint,
                            );
                        }
                    }
                }
            }
            // The code the tab paired with is kept for the line above, and the field starts empty.
            if live && self.code.len() == vxr_mcp::bridge::CODE_LEN {
                self.paired_code = std::mem::take(&mut self.code);
                self.sent = false;
            }
        } else {
            // 1 · Allow agents to edit this window.
            let sub = if live { "Listening on a local socket only you can open" } else { "Off: nothing listens" };
            let h = if v.fail.is_some() { 44.0 + 8.0 + err_h } else { 44.0 };
            let b = self.step(ui, s1, 1, "Allow agents to edit this window", None, false, h);
            let row = Rect::from_min_size(b.min, vec2(b.width(), 44.0));
            let (rect_block, _) = (row, ());
            let resp = ui.interact(rect_block, Id::unique("agents-allow-row"), Sense::click());
            ui.painter().text(pos2(row.left(), row.top() + 10.0), Align2::LEFT_CENTER, sub, sans(11.5), th().muted);
            let sw = Rect::from_min_size(pos2(row.right() - 30.0, row.top()), vec2(30.0, 18.0));
            let toggled = switch(ui, sw, "agents-allow", live).clicked() || resp.clicked();
            self.hits.push(("agents-allow".into(), row));
            if toggled {
                self.actions.push(Action::AllowAgents(!live));
            }
            if v.fail.is_some() {
                let er = Rect::from_min_size(pos2(b.left(), b.top() + 52.0), vec2(b.width(), err_h));
                self.error_box(ui, er, &err_title, &err_g);
            }
        }
        // Connect an agent.
        let (sn, step_n) = if web { (s3, 3) } else { (s2, 2) };
        let title = match clients {
            0 => "Connect an agent".to_string(),
            1 => "Agent connected".to_string(),
            n => format!("{n} agents connected"),
        };
        let first = v.times.iter().map(|t| t.0).fold(f64::MAX, f64::min);
        let right = (clients > 0).then(|| ago(now - first));
        match sn {
            St::Todo => {
                self.step(ui, sn, step_n, &title, None, true, 0.0);
            }
            St::Active => {
                let note =
                    Self::text_h(ui, "Then start Claude Code. It shows up here when it connects.", sans(11.5), bw);
                let h = 30.0 + 6.0 + 30.0 + 8.0 + note.size().y;
                let b = self.step(ui, sn, step_n, &title, None, true, h);
                self.command(ui, Rect::from_min_size(b.min, vec2(b.width(), 30.0)), SETUP, "agents-copy");
                self.command(
                    ui,
                    Rect::from_min_size(pos2(b.left(), b.top() + 36.0), vec2(b.width(), 30.0)),
                    LIVE_SETUP,
                    "agents-copy-live",
                );
                ui.painter().galley(pos2(b.left(), b.top() + 74.0), note, th().faint);
            }
            _ => {
                self.step(ui, sn, step_n, &title, right, true, 0.0);
                for i in 0..clients {
                    self.agent(ui, i);
                }
            }
        }
        if !live {
            return;
        }
        // The review switch, the edits and the footer.
        ui.add_space(6.0);
        let r = self.block(ui, 1.0);
        ui.painter().hline(r.x_range(), r.top(), Stroke::new(1.0, th().line));
        ui.add_space(10.0);
        if self.switch_row(
            ui,
            "Review every agent change",
            if v.review { "Edits wait as a card in Chat until you apply them" } else { "Each edit applies at once" },
            "agents-review",
            v.review,
        ) {
            self.actions.push(Action::ReviewAgents(!v.review));
        }
        let n = v.edits.len();
        if n > 0 {
            ui.add_space(8.0);
            let applied = v.edits.iter().filter(|e| e.applied).count();
            let r = self.block(ui, 22.0);
            let g = ui.painter().layout_job(spaced("AGENT EDITS", sans(9.0), th().faint, 0.72));
            ui.painter().galley(pos2(r.left(), r.top() + 8.0 - g.size().y / 2.0), g, th().faint);
            ui.painter().text(
                pos2(r.right(), r.top() + 8.0),
                Align2::RIGHT_CENTER,
                format!("{applied} applied"),
                mono(11.0),
                th().faint,
            );
            let rows = n.min(CARD_ROWS);
            for i in (0..n).rev().take(rows) {
                let e = v.edits[i].clone();
                let r = self.block(ui, EDIT_ROW);
                let cy = r.center().y;
                ui.painter().circle_filled(pos2(r.left() + 5.0, cy), 3.0, if e.applied { th().ok } else { th().faint });
                let b = Rect::from_min_size(pos2(r.right() - 64.0, cy - 11.0), vec2(64.0, 22.0));
                let color = if e.applied { th().text } else { th().faint };
                let g = one_line(ui, &e.label, sans(12.0), color, b.left() - 8.0 - (r.left() + 16.0));
                let gw = g.size().x;
                ui.painter().galley(pos2(r.left() + 16.0, cy - g.size().y / 2.0), g, color);
                if e.applied {
                    let id = format!("agent-undo-{i}");
                    if small_button(ui, b, icon::UNDO, "Undo", &id, false).clicked() {
                        self.actions.push(Action::UndoAgentEdit(i));
                    }
                    self.hits.push((id, b));
                } else {
                    ui.painter().line_segment(
                        [pos2(r.left() + 16.0, cy), pos2(r.left() + 16.0 + gw, cy)],
                        Stroke::new(1.0, th().faint),
                    );
                    ui.painter().text(
                        pos2(b.right() - 6.0, cy),
                        Align2::RIGHT_CENTER,
                        "Undone",
                        sans(11.5),
                        th().faint,
                    );
                }
                self.hits.push((format!("agent-edit-{i}"), r));
            }
            if n > rows {
                let r = self.block(ui, 20.0);
                ui.painter().text(
                    r.left_center(),
                    Align2::LEFT_CENTER,
                    format!("{} earlier", n - rows),
                    sans(11.5),
                    th().faint,
                );
            }
        }
        ui.add_space(8.0);
        let r = self.block(ui, 28.0);
        let regen = Rect::from_min_size(r.min, vec2(130.0, 28.0));
        if small_button(ui, regen, icon::KEY_ROUND, "Regenerate token", "agents-regenerate", false).clicked() {
            self.actions.push(Action::RegenerateToken);
        }
        self.hits.push(("agents-regenerate".into(), regen));
        if web {
            let d = Rect::from_min_size(pos2(r.right() - 96.0, r.top()), vec2(96.0, 28.0));
            if small_button(ui, d, icon::X, "Disconnect", "agents-disconnect", false).clicked() {
                self.actions.push(Action::AllowAgents(false));
            }
            self.hits.push(("agents-disconnect".into(), d));
        }
    }
}
