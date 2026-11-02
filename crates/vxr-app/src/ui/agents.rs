//! Terminal agents in the Hybrid UI (`docs/research/ai.md` section 3, live mode, after the
//! `mcp.html` and `mcp-live.html` mocks): the assistant panel's Agents tab (Cmd-K), a checklist
//! with "Allow agents to edit this window", the setup line and the agents connected, then the
//! review switch and the ACTIVITY list, each call with its answer and each edit with an Undo of its own; and the
//! chip in the hint bar while an agent is connected, with Stop. In the browser the tab says to
//! run `vxr bridge` and takes the pairing code it prints ([`crate::bridge`]). Drawn in the
//! panel style.

pub use crate::agents::{AgentCall, Fail};
use egui::{ScrollArea, UiBuilder};

use super::activity::{self, Item, Live, ago, span};

use super::*;

/// The setup lines the panel shows, for Claude Code: `vxr setup claude` registers files mode as
/// `vxr` and live mode as `vxr-live`, and the manual line adds only `vxr-live`, so neither
/// replaces the files mode server.
pub const SETUP: &str = "vxr setup claude";
pub const LIVE_SETUP: &str = "claude mcp add vxr-live -- vxr mcp --live";
pub const FILES_SETUP: &str = "claude mcp add vxr -- vxr mcp --root <folder>";
/// The browser's first step: the bridge between this tab and `vxr mcp --live`.
pub const BRIDGE_SETUP: &str = "vxr bridge";

/// How tall an ACTIVITY row is: a call, an edit with its Undo, and a gap in the calls.
const CALL_ROW: f32 = 40.0;
const EDIT_ROW: f32 = 50.0;
const GAP_ROW: f32 = 24.0;

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
    /// Every applied edit, oldest first.
    pub edits: Vec<EditRow>,
    /// Each connection's latest calls (the ACTIVITY list), oldest first within a connection.
    pub calls: Vec<(u64, AgentCall)>,
    /// For each of `clients`: how many calls it made since it connected.
    pub made: Vec<u64>,
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

/// What the hint-bar chip draws.
struct Chip {
    /// The agent, or the sentence about it (a stall names it in the middle).
    who: String,
    /// The last call's icon, left of its words.
    icon: Option<char>,
    /// The last call in plain words.
    verb: String,
    /// How long ago, or what else there is to say, fainter.
    tail: String,
    lead: Lead,
    /// Amber: no call for a while.
    warn: bool,
    /// Stop is made prominent: the agent may be stuck.
    strong: bool,
}

impl Chip {
    fn text(&self) -> String {
        let mid = if self.verb.is_empty() { "" } else { " \u{b7} " };
        format!("{}{mid}{}{}", self.who, self.verb, self.tail)
    }
}

/// The mark at the chip's left, and an agent row's.
#[derive(Clone, Copy, PartialEq)]
enum Lead {
    /// A call is running: a dot with a ring turning round it.
    Spin,
    /// Called a moment ago: a dot that pulses.
    Pulse,
    Dot,
    /// Quiet too long: an amber dot.
    Warn,
    /// May be stuck: the alert icon.
    Alert,
    /// Connected and not calling: a ring.
    Hollow,
}

/// How often a turning ring redraws: a call can run for as long as the user leaves a dialog open,
/// so it does not draw every frame.
const SPIN_REPAINT: std::time::Duration = std::time::Duration::from_millis(50);

/// Where the chip's pieces start: padding and the mark, the gap before Stop and Stop itself, and
/// the padding after it. Its icon takes `CHIP_ICON` before the call's words.
const CHIP_LEAD: f32 = 25.0;
const CHIP_STOP: f32 = 10.0 + 56.0;
const CHIP_END: f32 = 14.0;
const CHIP_ICON: f32 = 8.0 + 12.0 + 5.0;

/// Draw a lead mark centred on `c` at `t` seconds.
fn lead_mark(p: &egui::Painter, c: Pos2, lead: Lead, t: f64) {
    match lead {
        Lead::Spin | Lead::Pulse => {
            let pulse = if lead == Lead::Pulse { 0.55 + 0.45 * (t * 3.0).sin().abs() as f32 } else { 1.0 };
            p.circle_filled(c, 3.5, th().ok.gamma_multiply(pulse));
            if lead == Lead::Spin {
                // A ring a third round, turning once in under a second.
                let a0 = (t * 7.5) as f32;
                let arc: Vec<Pos2> = (0..=10)
                    .map(|k| {
                        let a = a0 + k as f32 * 0.2;
                        c + vec2(a.cos(), a.sin()) * 6.5
                    })
                    .collect();
                p.add(egui::Shape::line(arc, Stroke::new(1.5, th().ok)));
            }
        }
        Lead::Dot => {
            p.circle_filled(c, 3.5, th().ok);
        }
        Lead::Warn => {
            p.circle_filled(c, 3.5, th().mid);
        }
        Lead::Alert => {
            p.text(c, Align2::CENTER_CENTER, icon::TRIANGLE_ALERT, icon_font(13.0), th().mid);
        }
        Lead::Hollow => {
            p.circle_stroke(c, 3.0, Stroke::new(1.0, th().faint));
        }
    }
}

/// A button in the accent colour, for the one thing to do now.
fn solid_button(ui: &mut Ui, r: Rect, glyph: char, label: &str, id: &str) -> bool {
    let resp = ui.interact(r, Id::unique(("sbtn", id)), Sense::click());
    let p = ui.painter();
    let fill = if resp.hovered() { th().accent.gamma_multiply(0.85) } else { th().accent };
    p.rect_filled(r, CornerRadius::same(7), fill);
    const SIZE: f32 = 12.0;
    let g = p.layout_no_wrap(label.into(), sans(SIZE), th().on_accent);
    let icon = label_icon(glyph, SIZE);
    let iw = icon.size;
    let x = r.center().x - (iw + 6.0 + g.size().x) / 2.0;
    p.text(pos2(x + iw / 2.0, r.center().y), Align2::CENTER_CENTER, glyph, icon, th().on_accent);
    p.galley(pos2(x + iw + 6.0, r.center().y - g.size().y / 2.0), g, th().on_accent);
    resp.clicked()
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

    /// What the hint-bar chip says: the agent that matters most (the longest silent, else the one
    /// that called last), from the same state as the Agents tab's rows and banner.
    fn chip_parts(&self, now: f64) -> Option<Chip> {
        let (i, n) = self.agents.lead(now)?;
        let more = self.agents.clients.len() - 1;
        let name = &self.agents.clients[i].1;
        let name = if more > 0 { format!("{name} +{more}") } else { name.clone() };
        let call = n.call;
        let said = |running| call.map(|c| activity::verb(c, running)).unwrap_or_default();
        let c = |who: String, icon, verb: String, tail: String, lead| Chip {
            who,
            icon,
            verb,
            tail,
            lead,
            warn: n.state.warn(),
            strong: n.state == Live::Long,
        };
        let tool_icon = call.map(|c| activity::tool_icon(&c.tool));
        Some(match n.state {
            Live::Running => c(name, tool_icon, said(true), format!(" \u{b7} {} s", n.quiet as u64), Lead::Spin),
            Live::Active => c(name, tool_icon, said(false), format!(" \u{b7} {}", ago(n.quiet)), Lead::Pulse),
            Live::Quiet => c(name, tool_icon, said(false), format!(" \u{b7} {} ago", ago(n.quiet)), Lead::Dot),
            Live::Stall => c(
                format!("No call from {name} for {}", span(n.quiet)),
                None,
                String::new(),
                format!(" \u{b7} last {}", activity::lower_first(&said(false))),
                Lead::Warn,
            ),
            Live::Long => c(
                format!("{name} may be stuck: no call for {}", span(n.quiet)),
                None,
                String::new(),
                String::new(),
                Lead::Alert,
            ),
            Live::Done => {
                let summary = call.map(|c| c.arg.clone()).filter(|s| !s.is_empty());
                c(
                    format!("{name} is done"),
                    None,
                    String::new(),
                    summary.map(|s| format!(" \u{b7} {s}")).unwrap_or_default(),
                    Lead::Dot,
                )
            }
            Live::Idle => c(
                format!("{name} connected"),
                None,
                String::new(),
                format!(" \u{b7} idle {}", ago(n.quiet)),
                Lead::Hollow,
            ),
        })
    }

    /// The chip's words in one line, as the hint bar says them.
    pub fn agent_chip_text(&self, now: f64) -> String {
        self.chip_parts(now).map(|c| c.text()).unwrap_or_default()
    }

    /// The chip's text laid out on one line, cut with an ellipsis where the window is too narrow:
    /// the galley, where its tool icon goes (left of the verb, when the whole name fits before
    /// it) and the width it takes. That width is measured with every digit a zero, so the card
    /// does not shift sideways as a counter ticks, only when it gains a digit.
    fn chip_galley(&self, ctx: &Context, c: &Chip) -> (Arc<egui::Galley>, Option<f32>, f32) {
        let (text, faint) = if c.warn { (th().mid, th().mid) } else { (th().text, th().faint) };
        let fmt = |font: FontId, color| TextFormat { font_id: font, color, ..Default::default() };
        let who = ctx.fonts_mut(|f| f.layout_no_wrap(c.who.clone(), sans_medium(12.0), text));
        // The card spans the window less its margins, and the Stop button and padding.
        let budget = ctx.content_rect().width() - 24.0 - (CHIP_LEAD + CHIP_STOP + CHIP_END);
        let icon = c.icon.filter(|_| who.size().x + CHIP_ICON <= budget).map(|_| who.size().x + 8.0);
        let lay = |zeros: bool| {
            let t = |s: &str| if zeros { s.replace(|ch: char| ch.is_ascii_digit(), "0") } else { s.to_string() };
            let mut job = LayoutJob::default();
            job.append(&t(&c.who), 0.0, fmt(sans_medium(12.0), text));
            if !c.verb.is_empty() {
                job.append(&t(&c.verb), if icon.is_some() { CHIP_ICON } else { 6.0 }, fmt(sans(12.0), text));
            }
            job.append(&t(&c.tail), 0.0, fmt(sans(12.0), faint));
            job.wrap = egui::text::TextWrapping {
                max_width: budget.max(1.0),
                max_rows: 1,
                break_anywhere: true,
                overflow_character: Some('\u{2026}'),
            };
            ctx.fonts_mut(|f| f.layout_job(job))
        };
        let (g, steady) = (lay(false), lay(true));
        let w = steady.size().x.max(g.size().x);
        (g, icon, w)
    }

    /// The agent chip at the left of the hint bar: which agent, what it did last and how long
    /// ago, and Stop. Returns its width, 0 with no agent connected.
    pub(super) fn agent_chip_width(&self, ctx: &Context) -> f32 {
        let Some(c) = self.chip_parts(ctx.input(|i| i.time)) else { return 0.0 };
        CHIP_LEAD + self.chip_galley(ctx, &c).2 + CHIP_STOP + CHIP_END
    }

    /// Draw the chip into the hint panel's `ui`, starting at `x`.
    pub(super) fn agent_chip(&mut self, ui: &mut Ui, rect: Rect, w: f32) {
        let t = ui.input(|i| i.time);
        let Some(c) = self.chip_parts(t).filter(|_| w > 0.0) else { return };
        let cy = rect.center().y;
        let x = rect.left() + 12.0;
        let (g, icon_at, steady) = self.chip_galley(ui.ctx(), &c);
        let p = ui.painter().clone();
        if c.warn {
            let r = CornerRadius { nw: R, sw: R, ne: 0, se: 0 };
            p.rect_filled(Rect::from_min_max(rect.min, pos2(rect.left() + w, rect.bottom())), r, th().mid_bg);
        }
        let dot = pos2(x + 3.5, cy);
        lead_mark(&p, dot, c.lead, t);
        if c.lead == Lead::Pulse {
            ui.ctx().request_repaint();
        } else if c.lead == Lead::Spin {
            ui.ctx().request_repaint_after(SPIN_REPAINT);
        } else {
            // The times tick on, and the chip's width with them.
            ui.ctx().request_repaint_after(std::time::Duration::from_secs(1));
        }
        let gx = x + 13.0;
        let gw = steady;
        let color = if c.warn { th().mid } else { th().text };
        if let (Some(tool), Some(dx)) = (c.icon, icon_at) {
            let icon = label_icon(tool, 12.0);
            let icon_color = if c.warn { th().mid } else { th().muted };
            p.text(pos2(gx + dx + 6.0, cy), Align2::CENTER_CENTER, tool, icon, icon_color);
        }
        p.galley(pos2(gx, cy - g.size().y / 2.0), g, color);
        let stop = Rect::from_min_size(pos2(gx + gw + 10.0, cy - 11.0), vec2(56.0, 22.0));
        let clicked = if c.strong {
            solid_button(ui, stop, icon::SQUARE, "Stop", "agent-stop")
        } else {
            small_button(ui, stop, icon::SQUARE, "Stop", "agent-stop", false).clicked()
        };
        if clicked {
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
    // the ACTIVITY list and the footer.
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
        let since = self.v.times.get(i).map_or(self.now, |t| t.0);
        let n = self.v.now_of(i, self.now);
        let (state, color, lead) = match (n.state, n.call) {
            (Live::Running, Some(c)) => {
                (format!("{} \u{b7} {} s", activity::verb(c, true), n.quiet as u64), th().ok, Lead::Spin)
            }
            (Live::Active, Some(c)) => {
                (format!("{} \u{b7} {}", activity::verb(c, false), ago(n.quiet)), th().ok, Lead::Pulse)
            }
            (Live::Quiet, _) => (format!("last call {} ago", ago(n.quiet)), th().faint, Lead::Dot),
            (Live::Stall, _) => (format!("no call for {}", span(n.quiet)), th().mid, Lead::Warn),
            (Live::Long, _) => (format!("no call for {} \u{b7} may be stuck", span(n.quiet)), th().mid, Lead::Warn),
            (Live::Done, _) => (format!("done \u{b7} {}", ago(n.quiet)), th().ok, Lead::Dot),
            _ => (format!("idle {}", ago(n.quiet)), th().faint, Lead::Hollow),
        };
        let edits = self.v.edits.iter().filter(|e| e.conn == conn).count();
        let calls = self.v.made.get(i).copied().unwrap_or(0);
        let r = self.block(ui, 44.0);
        let r = Rect::from_min_max(pos2(r.left() + 30.0, r.top()), r.max);
        let p = ui.painter().clone();
        let c = pos2(r.left() + 4.0, r.top() + 11.0);
        lead_mark(&p, c, lead, self.now);
        if lead == Lead::Pulse {
            ui.ctx().request_repaint();
        } else if lead == Lead::Spin {
            ui.ctx().request_repaint_after(SPIN_REPAINT);
        } else {
            // The times tick on.
            ui.ctx().request_repaint_after(std::time::Duration::from_secs(1));
        }
        self.hits.push((format!("agents-dot-{conn}"), Rect::from_center_size(c, vec2(7.0, 7.0))));
        let g = p.layout_no_wrap(name.clone(), sans_semibold(12.5), th().text);
        let gw = g.size().x;
        p.galley(pos2(r.left() + 16.0, c.y - g.size().y / 2.0), g, th().text);
        let b = Rect::from_min_size(pos2(r.right() - 58.0, r.top()), vec2(58.0, 22.0));
        let g = one_line(ui, &state, sans(11.5), color, (b.left() - 8.0) - (r.left() + 16.0 + gw + 8.0));
        ui.painter().galley(pos2(r.left() + 16.0 + gw + 8.0, c.y - g.size().y / 2.0), g, color);
        let id = format!("agents-stop-{conn}");
        let clicked = if n.state == Live::Long {
            solid_button(ui, b, icon::SQUARE, "Stop", &id)
        } else {
            small_button(ui, b, icon::SQUARE, "Stop", &id, false).clicked()
        };
        if clicked {
            self.actions.push(Action::StopAgent(conn));
        }
        self.hits.push((id, b));
        let mut meta = format!("since {}", ago(self.now - since));
        if calls > 0 {
            meta += &format!(" \u{b7} {calls} call{}", if calls == 1 { "" } else { "s" });
        }
        if edits > 0 {
            meta += &format!(" \u{b7} {edits} edit{}", if edits == 1 { "" } else { "s" });
        }
        let g = one_line(ui, &meta, sans(11.0), th().faint, r.width() - 16.0);
        ui.painter().galley(pos2(r.left() + 16.0, r.top() + 25.0), g, th().faint);
    }

    /// A pause in an agent's calls: amber while it goes on, grey once the agent called again.
    fn gap_row(&mut self, ui: &mut Ui, secs: f64, live: bool, who: Option<&str>) {
        let r = self.block(ui, GAP_ROW);
        let who = who.map(|w| format!("{w}: ")).unwrap_or_default();
        let text =
            if live { format!("{who}no call for {}", span(secs)) } else { format!("{who}{} with no call", span(secs)) };
        let (color, line) = if live { (th().mid, th().mid_line) } else { (th().faint, th().line) };
        let g = ui.painter().layout_no_wrap(text, sans(11.0), color);
        let (w, cy) = (g.size().x, r.center().y);
        if w + 40.0 < r.width() {
            let (l, rt) = (r.center().x - w / 2.0 - 8.0, r.center().x + w / 2.0 + 8.0);
            ui.painter().hline(r.left()..=l, cy, Stroke::new(1.0, line));
            ui.painter().hline(rt..=r.right(), cy, Stroke::new(1.0, line));
            ui.painter().galley(pos2(r.center().x - w / 2.0, cy - g.size().y / 2.0), g, color);
        } else {
            ui.painter().galley(pos2(r.left(), cy - g.size().y / 2.0), g, color);
        }
        self.hits.push((if live { "agent-gap-live" } else { "agent-gap" }.into(), r));
    }

    /// A call: its icon, what it was and how it ended in a line, when, and for an edit in the scene
    /// its Undo.
    fn call_row(&mut self, ui: &mut Ui, i: usize) {
        let (_, c) = self.v.calls[i].clone();
        let edit = c.edit.and_then(|e| self.v.edits.get(e).map(|row| (e, row.applied)));
        let undoable = edit.is_some_and(|(_, applied)| applied);
        let r = self.block(ui, if undoable { EDIT_ROW } else { CALL_ROW });
        let running = c.answered.is_none();
        let undone = edit.is_some_and(|(_, applied)| !applied);
        let mark = pos2(r.left() + 5.0, r.top() + 11.0);
        let p = ui.painter().clone();
        let (tone, line2) = if running {
            (th().ok, th().ok)
        } else if !c.ok {
            (th().danger, th().danger)
        } else if undone {
            (th().faint, th().faint)
        } else if edit.is_some() {
            (th().ok, th().muted)
        } else {
            (th().faint, th().faint)
        };
        if running {
            lead_mark(&p, mark, Lead::Spin, self.now);
            ui.ctx().request_repaint_after(SPIN_REPAINT);
        } else {
            let glyph = activity::tool_icon(&c.tool);
            p.text(mark, Align2::CENTER_CENTER, glyph, label_icon(glyph, 12.0), tone);
        }
        let right = 64.0;
        let tw = r.width() - 20.0 - right - 8.0;
        let call_color = if undone || (!running && edit.is_none() && c.ok) { th().muted } else { th().text };
        let mut job = LayoutJob::default();
        let fmt = |color| TextFormat { font_id: mono(11.5), color, ..Default::default() };
        job.append(&c.tool, 0.0, fmt(call_color));
        if !c.arg.is_empty() {
            job.append(&c.arg, 6.0, fmt(th().faint));
        }
        job.wrap = egui::text::TextWrapping {
            max_width: tw.max(1.0),
            max_rows: 1,
            break_anywhere: true,
            overflow_character: Some('\u{2026}'),
        };
        let g = ui.painter().layout_job(job);
        p.galley(pos2(r.left() + 20.0, r.top() + 11.0 - g.size().y / 2.0), g, call_color);
        let res = if running {
            format!("running \u{b7} {} s", (self.now - c.started).max(0.0) as u64)
        } else {
            c.result.clone()
        };
        let g = one_line(ui, &res, sans(11.5), line2, tw);
        p.galley(pos2(r.left() + 20.0, r.top() + 27.0 - g.size().y / 2.0), g, line2);
        p.text(
            pos2(r.right(), r.top() + 10.0),
            Align2::RIGHT_CENTER,
            ago(self.now - c.started),
            mono(10.5),
            th().faint,
        );
        if let Some((e, applied)) = edit {
            let b = Rect::from_min_size(pos2(r.right() - right, r.top() + 22.0), vec2(right, 22.0));
            if applied {
                let id = format!("agent-undo-{e}");
                if small_button(ui, b, icon::UNDO, "Undo", &id, false).clicked() {
                    self.actions.push(Action::UndoAgentEdit(e));
                }
                self.hits.push((id, b));
            } else {
                p.text(pos2(r.right(), r.top() + 27.0), Align2::RIGHT_CENTER, "Undone", sans(11.5), th().faint);
            }
            self.hits.push((format!("agent-edit-{e}"), r));
        }
        self.hits.push((format!("agent-call-{i}"), r));
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
        // The banner, from the same state as the hint bar's chip: amber while the agent that matters
        // most has made no call for a while.
        let top = v.lead(now);
        let working = (0..clients)
            .filter(|&i| matches!(v.now_of(i, now).state, Live::Running | Live::Active | Live::Quiet))
            .count();
        let lead = match (clients, top) {
            (0, _) | (_, None) => String::new(),
            (_, Some((i, n))) if n.state.warn() => {
                let who = &v.clients[i].1;
                if n.state == Live::Long {
                    format!("{who} may be stuck")
                } else {
                    format!("{who}: no call for {}", span(n.quiet))
                }
            }
            (1, Some((_, n))) => {
                let who = &v.clients[0].1;
                match n.state {
                    Live::Running | Live::Active | Live::Quiet => format!("{who} is working"),
                    Live::Done => format!("{who} is done"),
                    _ => format!("{who} is connected"),
                }
            }
            (n, _) if working == 0 => format!("{n} agents connected"),
            (n, _) => format!("{n} agents, {working} working"),
        };
        let warn = top.is_some_and(|(_, n)| n.state.warn());
        let since = v.since.map(|t| ago(now - t));
        let (tone, text, right) = if live && clients > 0 {
            let right = since
                .clone()
                .map(|s| if web { format!("paired {s}") } else { format!("since {s}") })
                .unwrap_or_default();
            if warn { (Tone::Busy, lead, String::new()) } else { (Tone::Ok, format!("Live \u{b7} {lead}"), right) }
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
        // ACTIVITY: the agents' latest calls, newest first, with a row where they went quiet.
        let list = activity::items(&v, now);
        if !list.is_empty() {
            ui.add_space(8.0);
            let calls: Vec<usize> =
                list.iter().filter_map(|it| if let Item::Call(i) = it { Some(*i) } else { None }).collect();
            let applied = calls
                .iter()
                .filter(|&&i| v.calls[i].1.edit.and_then(|e| v.edits.get(e)).is_some_and(|e| e.applied))
                .count();
            let r = self.block(ui, 22.0);
            let g = ui.painter().layout_job(spaced("ACTIVITY", sans(9.0), th().faint, 0.72));
            ui.painter().galley(pos2(r.left(), r.top() + 8.0 - g.size().y / 2.0), g, th().faint);
            ui.painter().text(
                pos2(r.right(), r.top() + 8.0),
                Align2::RIGHT_CENTER,
                format!("{} call{} \u{b7} {applied} applied", calls.len(), if calls.len() == 1 { "" } else { "s" }),
                mono(11.0),
                th().faint,
            );
            for item in &list {
                match item {
                    Item::Gap { secs, live, who } => self.gap_row(ui, *secs, *live, who.as_deref()),
                    Item::Call(i) => self.call_row(ui, *i),
                }
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
