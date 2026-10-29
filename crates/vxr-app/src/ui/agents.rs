//! Terminal agents in the Hybrid UI (`docs/research/ai.md` section 3, live mode, after the
//! `mcp.html` and `mcp-live.html` mocks): the Terminal agents panel (Cmd-K) with "Allow
//! terminal agents to edit this window", the setup line, the pairing token and the agents
//! connected; the chip in the hint bar while an agent is connected, with Stop; and the
//! Agent edits card, each applied call with an Undo of its own. In the browser the panel
//! says to run `vxr bridge` and takes the pairing code it prints ([`crate::bridge`]). Drawn
//! in the panel style.

use super::*;

/// The setup lines the panel shows, for Claude Code: `vxr setup claude` registers files mode as
/// `vxr` and live mode as `vxr-live`, and the manual line adds only `vxr-live`, so neither
/// replaces the files mode server.
pub const SETUP: &str = "vxr setup claude";
pub const LIVE_SETUP: &str = "claude mcp add vxr-live -- vxr mcp --live";
pub const FILES_SETUP: &str = "claude mcp add vxr -- vxr mcp --root <folder>";
/// The browser's first step: the bridge between this tab and `vxr mcp --live`.
pub const BRIDGE_SETUP: &str = "vxr bridge";

/// Longest pairing code the field keeps: the code, a dash and room for spaces.
const CODE_FIELD_MAX: usize = 12;

/// Most Agent edits rows shown, newest first.
const CARD_ROWS: usize = 8;

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
    /// Why turning it on failed.
    pub error: Option<String>,
    /// The connected agents: connection and name.
    pub clients: Vec<(u64, String)>,
    /// Every applied call, oldest first.
    pub edits: Vec<EditRow>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct EditRow {
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

/// A setup line in a box, with Copy.
fn setup_line(ui: &mut Ui, r: Rect, text: &str, id: &str, hits: &mut Vec<(String, Rect)>, copied: &mut Option<String>) {
    let p = ui.painter();
    p.rect(r, CornerRadius::same(8), th().tint(9), Stroke::new(1.0, th().line), StrokeKind::Inside);
    let b = Rect::from_min_size(pos2(r.right() - 6.0 - 64.0, r.top() + 6.0), vec2(64.0, 22.0));
    let g = one_line(ui, text, mono(11.5), th().text, b.left() - 8.0 - (r.left() + 10.0));
    ui.painter().galley(pos2(r.left() + 10.0, r.center().y - g.size().y / 2.0), g, th().text);
    if small_button(ui, b, icon::COPY, "Copy", id, false).clicked() {
        *copied = Some(text.to_string());
    }
    hits.push((id.to_string(), b));
    hits.push((format!("{id}-line"), r));
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
    /// Open the Terminal agents panel (Cmd-K), closing what would sit over it.
    pub fn open_agents(&mut self) {
        self.agents_panel = true;
        self.export_menu = false;
        self.view_panel = false;
    }

    /// Show the Agent edits card again after it was closed.
    pub fn show_agent_edits(&mut self) {
        self.agents_card_closed = 0;
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

    // The Agent edits card: where the review card goes, while no change is under review.
    pub(super) fn agent_card(&mut self, ctx: &Context, screen: Rect) {
        let n = self.agents.edits.len();
        if n == 0 || n <= self.agents_card_closed || self.review.is_some() {
            return;
        }
        let w = 304.0;
        let inner_w = w - 24.0;
        let rows = n.min(CARD_ROWS);
        let row_h = 30.0;
        let more = n - rows;
        let mut h = 12.0 + 15.0 + 8.0 + rows as f32 * row_h + 12.0;
        if more > 0 {
            h += 18.0;
        }
        let (x, top) = self.card_origin(screen, w, h);
        let rect = Rect::from_min_size(pos2(x, top), vec2(w, h));
        let mut hits: Vec<(String, Rect)> = Vec::new();
        let mut actions = Vec::new();
        let mut close = false;
        let edits = self.agents.edits.clone();
        Self::panel(ctx, "agent-card", rect, Order::Middle, |ui, rect| {
            let inner = rect.shrink(12.0);
            let p = ui.painter().clone();
            let hy = inner.top() + 7.5;
            p.text(pos2(inner.left() + 6.0, hy), Align2::CENTER_CENTER, icon::TERMINAL, icon_font(13.0), th().accent);
            let g = p.layout_job(spaced("AGENT EDITS", sans_semibold(11.0), th().muted, 0.88));
            p.galley(pos2(inner.left() + 18.0, hy - g.size().y / 2.0), g, th().muted);
            let cr = Rect::from_center_size(pos2(inner.right() - 8.0, hy), vec2(22.0, 22.0));
            let resp = ui.interact(cr, Id::unique("agent-card-close"), Sense::click());
            if resp.hovered() {
                p.rect_filled(cr, CornerRadius::same(6), th().hover);
            }
            p.text(
                cr.center(),
                Align2::CENTER_CENTER,
                icon::X,
                icon_font(13.0),
                if resp.hovered() { th().text } else { th().faint },
            );
            close = resp.clicked();
            hits.push(("agent-card-close".into(), cr));
            let applied = edits.iter().filter(|e| e.applied).count();
            p.text(
                pos2(cr.left() - 6.0, hy),
                Align2::RIGHT_CENTER,
                format!("{applied} applied"),
                mono(11.0),
                th().faint,
            );
            let mut y = inner.top() + 15.0 + 8.0;
            for i in (0..n).rev().take(rows) {
                let e = &edits[i];
                let r = Rect::from_min_size(pos2(inner.left() - 4.0, y), vec2(inner_w + 8.0, row_h));
                let cy = r.center().y;
                let dot = if e.applied { th().ok } else { th().faint };
                p.circle_filled(pos2(r.left() + 9.0, cy), 3.0, dot);
                let b = Rect::from_min_size(pos2(r.right() - 4.0 - 64.0, cy - 11.0), vec2(64.0, 22.0));
                let color = if e.applied { th().text } else { th().faint };
                let g = one_line(ui, &e.label, sans(12.0), color, b.left() - 8.0 - (r.left() + 20.0));
                let gw = g.size().x;
                p.galley(pos2(r.left() + 20.0, cy - g.size().y / 2.0), g, color);
                if e.applied {
                    let id = format!("agent-undo-{i}");
                    if small_button(ui, b, icon::UNDO, "Undo", &id, false).clicked() {
                        actions.push(Action::UndoAgentEdit(i));
                    }
                    hits.push((id, b));
                } else {
                    p.line_segment(
                        [pos2(r.left() + 20.0, cy), pos2(r.left() + 20.0 + gw, cy)],
                        Stroke::new(1.0, th().faint),
                    );
                    p.text(pos2(b.right() - 6.0, cy), Align2::RIGHT_CENTER, "Undone", sans(11.5), th().faint);
                }
                hits.push((format!("agent-edit-{i}"), r));
                y += row_h;
            }
            if more > 0 {
                p.text(
                    pos2(inner.left(), y + 9.0),
                    Align2::LEFT_CENTER,
                    format!("{more} earlier"),
                    sans(11.5),
                    th().faint,
                );
            }
        });
        self.hits.extend(hits);
        self.hits.insert("agent-card".into(), rect);
        self.actions.extend(actions);
        if close {
            self.agents_card_closed = n;
        }
    }

    // The Terminal agents panel (Cmd-K): live mode's switch, setup, pairing and connections.
    // In the browser it also says to run `vxr bridge` and takes the code it printed.
    pub(super) fn agents_panel(&mut self, ctx: &Context, screen: Rect) {
        if !self.agents_panel {
            return;
        }
        let w = 400.0;
        let inner_w = w - 32.0;
        let v = self.agents.clone();
        let intro = if v.web {
            "Let an agent in your terminal, like Claude Code, edit this scene while you watch. vxr bridge, in the vxr app (brew install jrnxf/tap/vxr), links this tab to it on this computer only. Each call applies at once as one undo step, listed under Agent edits."
        } else if v.available {
            "Let an agent in your terminal, like Claude Code, edit this scene while you watch. Each call applies at once as one undo step, listed under Agent edits."
        } else {
            "Live mode is not built for this system yet. Files mode edits .vox files on disk without a window:"
        };
        let intro = wrapped(ctx, intro, sans(12.0), th().muted, inner_w);
        let error = v.error.as_deref().map(|e| wrapped(ctx, e, sans(12.0), th().danger, inner_w));
        let pairing = if v.web {
            "vxr mcp --live reads a token from a file only you can read, and vxr bridge passes its calls to this tab. A new token disconnects every agent; each must restart its MCP server to pair again."
        } else {
            "vxr mcp --live reads a token from a file only you can read. A new token disconnects every agent; each must restart its MCP server to pair again."
        };
        let pairing = wrapped(ctx, pairing, sans(11.5), th().faint, inner_w - 120.0);
        let row_h = 32.0;
        let mut h = 16.0 + 18.0 + 12.0 + intro.size().y + 14.0;
        if v.available {
            if v.web {
                h += 16.0 + 6.0 + 34.0 + 14.0 + 16.0 + 6.0 + 34.0 + 10.0;
            }
            h += 44.0 + 10.0;
            if let Some(g) = &error {
                h += g.size().y + 10.0;
            }
            h += 16.0 + 6.0 + 34.0 + 6.0 + 34.0 + 14.0;
            h += pairing.size().y.max(28.0) + 14.0;
            h += 16.0 + 6.0 + v.clients.len().max(1) as f32 * row_h + 4.0;
        } else {
            h += 34.0 + 4.0;
        }
        h += 16.0;
        let rect = Rect::from_min_size(pos2((screen.center().x - w / 2.0).round(), TOP), vec2(w, h));
        let mut hits: Vec<(String, Rect)> = Vec::new();
        let mut actions = Vec::new();
        let mut close = false;
        let mut copied = None;
        let mut code = std::mem::take(&mut self.agents_code);
        Self::panel_filled(ctx, "agents-panel", rect, Order::Foreground, th().panel_solid, |ui, rect| {
            let inner = rect.shrink(16.0);
            let p = ui.painter().clone();
            let hy = inner.top() + 9.0;
            p.text(
                pos2(inner.left() + 7.0, hy),
                Align2::CENTER_CENTER,
                icon::SQUARE_TERMINAL,
                icon_font(14.0),
                th().accent,
            );
            let g = p.layout_job(spaced("TERMINAL AGENTS", sans_semibold(11.0), th().muted, 0.88));
            p.galley(pos2(inner.left() + 22.0, hy - g.size().y / 2.0), g, th().muted);
            let cr = Rect::from_center_size(pos2(inner.right() - 9.0, hy), vec2(24.0, 24.0));
            let resp = ui.interact(cr, Id::unique("agents-close"), Sense::click());
            if resp.hovered() {
                p.rect_filled(cr, CornerRadius::same(6), th().hover);
            }
            p.text(
                cr.center(),
                Align2::CENTER_CENTER,
                icon::X,
                icon_font(14.0),
                if resp.hovered() { th().text } else { th().faint },
            );
            close = resp.clicked();
            hits.push(("agents-close".into(), cr));
            let mut y = inner.top() + 18.0 + 12.0;
            let ih = intro.size().y;
            p.galley(pos2(inner.left(), y), intro, th().muted);
            y += ih + 14.0;
            let line = Rect::from_min_size(pos2(inner.left(), y), vec2(inner_w, 34.0));
            if !v.available {
                setup_line(ui, line, FILES_SETUP, "agents-copy-files", &mut hits, &mut copied);
                return;
            }
            let label = |ui: &Ui, y: f32, text: &str| {
                let g = ui.painter().layout_job(spaced(text, sans(9.0), th().faint, 0.72));
                ui.painter().galley(pos2(inner.left(), y + 8.0 - g.size().y / 2.0), g, th().faint);
            };
            // The browser: run vxr bridge, then type its code.
            let on = v.allowed || v.connecting;
            let mut pair = false;
            if v.web {
                label(ui, y, "1 · RUN IN A TERMINAL");
                y += 16.0 + 6.0;
                let line = Rect::from_min_size(pos2(inner.left(), y), vec2(inner_w, 34.0));
                setup_line(ui, line, BRIDGE_SETUP, "agents-copy-bridge", &mut hits, &mut copied);
                y += 34.0 + 14.0;
                label(ui, y, "2 · TYPE THE PAIRING CODE IT PRINTS");
                y += 16.0 + 6.0;
                let r = Rect::from_min_size(pos2(inner.left(), y), vec2(inner_w, 34.0));
                let resp =
                    super::assistant::ai_field(ui, r, "CODE", Id::unique("agents-code"), &mut code, false, "K7QF-3MXP");
                if code.chars().count() > CODE_FIELD_MAX {
                    code = code.chars().take(CODE_FIELD_MAX).collect();
                }
                pair = resp.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) && !on;
                hits.push(("agents-code".into(), r));
                y += 34.0 + 10.0;
            }
            // Allow terminal agents to edit this window (or tab).
            let r = Rect::from_min_size(pos2(inner.left(), y), vec2(inner_w, 44.0));
            let resp = ui.interact(r, Id::unique("agents-allow-row"), Sense::click());
            let p = ui.painter();
            p.rect(
                r,
                CornerRadius::same(8),
                if v.allowed { th().accent_bg } else { th().tint(9) },
                Stroke::new(1.0, if v.allowed { th().ring } else { th().line }),
                StrokeKind::Inside,
            );
            let title = if v.web {
                "Allow terminal agents to edit this tab"
            } else {
                "Allow terminal agents to edit this window"
            };
            p.text(pos2(r.left() + 12.0, r.top() + 14.0), Align2::LEFT_CENTER, title, sans_medium(12.5), th().text);
            let sub = match (v.web, v.allowed, v.connecting) {
                (false, true, _) => "On: listening on a local socket only you can open",
                (false, false, _) => "Off: nothing listens",
                (true, true, _) => "On: paired with vxr bridge on this computer",
                (true, false, true) => "Connecting… if the browser asks, allow apps on this device",
                (true, false, false) => "Off: this tab connects to nothing",
            };
            p.text(pos2(r.left() + 12.0, r.top() + 31.0), Align2::LEFT_CENTER, sub, sans(11.0), th().muted);
            let sw = Rect::from_min_size(pos2(r.right() - 12.0 - 30.0, r.center().y - 9.0), vec2(30.0, 18.0));
            let toggled = switch(ui, sw, "agents-allow", on).clicked() || resp.clicked();
            if v.web && (pair || (toggled && !on)) {
                actions.push(Action::PairBridge);
            } else if toggled {
                actions.push(Action::AllowAgents(!on));
            }
            hits.push(("agents-allow".into(), r));
            y += 44.0 + 10.0;
            if let Some(g) = error {
                let gh = g.size().y;
                ui.painter().galley(pos2(inner.left(), y), g, th().danger);
                y += gh + 10.0;
            }
            label(ui, y, if v.web { "3 · ADD VXR TO CLAUDE CODE" } else { "ADD VXR TO CLAUDE CODE" });
            y += 16.0 + 6.0;
            let line = Rect::from_min_size(pos2(inner.left(), y), vec2(inner_w, 34.0));
            setup_line(ui, line, SETUP, "agents-copy", &mut hits, &mut copied);
            y += 34.0 + 6.0;
            let line = Rect::from_min_size(pos2(inner.left(), y), vec2(inner_w, 34.0));
            setup_line(ui, line, LIVE_SETUP, "agents-copy-live", &mut hits, &mut copied);
            y += 34.0 + 14.0;
            // Pairing: the token file, and Regenerate.
            let ph = pairing.size().y.max(28.0);
            ui.painter().galley(pos2(inner.left(), y), pairing, th().faint);
            let b = Rect::from_min_size(pos2(inner.right() - 108.0, y + ph / 2.0 - 14.0), vec2(108.0, 28.0));
            let resp = small_button(ui, b, icon::REFRESH_CW, "Regenerate", "agents-regenerate", false);
            if resp.clicked() && v.allowed {
                actions.push(Action::RegenerateToken);
            }
            hits.push(("agents-regenerate".into(), b));
            y += ph + 14.0;
            label(ui, y, "CONNECTED");
            y += 16.0 + 6.0;
            if v.clients.is_empty() {
                let none = if v.allowed { "No agent connected yet" } else { "No agent can connect while this is off" };
                ui.painter().text(
                    pos2(inner.left(), y + row_h / 2.0),
                    Align2::LEFT_CENTER,
                    none,
                    sans(12.0),
                    th().faint,
                );
            }
            for (conn, name) in &v.clients {
                let r = Rect::from_min_size(pos2(inner.left(), y), vec2(inner_w, row_h));
                let cy = r.center().y;
                ui.painter().circle_filled(pos2(r.left() + 4.0, cy), 3.5, th().ok);
                ui.painter().text(pos2(r.left() + 16.0, cy), Align2::LEFT_CENTER, name, sans(12.5), th().text);
                let b = Rect::from_min_size(pos2(r.right() - 64.0, cy - 11.0), vec2(64.0, 22.0));
                let id = format!("agents-stop-{conn}");
                if small_button(ui, b, icon::SQUARE, "Stop", &id, false).clicked() {
                    actions.push(Action::StopAgent(*conn));
                }
                hits.push((id, b));
                y += row_h;
            }
        });
        self.agents_code = code;
        if let Some(text) = copied {
            ctx.copy_text(text);
            self.toast(ctx, "Copied the setup line");
        }
        self.hits.extend(hits);
        self.hits.insert("agents-panel".into(), rect);
        self.actions.extend(actions);
        if close {
            self.agents_panel = false;
        }
    }
}
