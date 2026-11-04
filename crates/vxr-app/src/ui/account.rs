//! The account control at the far right of the top bar and its popover, where there is
//! sign-in, after the sign-in mocks' direction A ("Front door"): "Sign in", or the account's
//! initial with a dot for how the library's sync stands. Signed out, the popover asks for an
//! email, which gets a link and a code in one message, and takes the code in six cells; a link
//! clicked in another tab signs this one in too (the page asks the Worker again). The desktop
//! app's email has only the code ([`SignIn::app`]). Signed in, it shows the email, the
//! library's use, how sync stands, and Sign out.

use egui::{Galley, Mesh, Painter, Shape, UiBuilder, Vec2};

use super::*;

/// How the library's sync stands, for the account's dot and its menu (set by the editor).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub enum Sync {
    #[default]
    Synced,
    /// An upload, a download or the list is in flight.
    Syncing,
    /// A scene in the library has edits saved here that have not gone up yet.
    Waiting,
}

/// Where signing in is up to.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub enum Step {
    #[default]
    Email,
    Code,
    /// Signed in: the check, before the popover closes by itself.
    Done,
}

/// What the popover shows: a step, or the signed-in menu.
#[derive(Clone, Copy, Debug, PartialEq)]
enum View {
    Email,
    Code,
    Done,
    Menu,
}

/// The digits in a sign-in code.
pub const CODE_LEN: usize = 6;
const W: f32 = 320.0;
const PAD: f32 = 18.0;
const CELL_W: f32 = 40.0;
const CELL_H: f32 = 46.0;
const CELL_GAP: f32 = 6.0;
/// The extra gap between the code's two halves.
const HALF_GAP: f32 = 8.0;
/// Seconds before Resend is offered, the wrong code's shake, and how long the check shows.
const RESEND: f64 = 30.0;
const SHAKE: f64 = 0.32;
const DONE_FOR: f64 = 1.1;
/// The size of the page's bot check (Turnstile), which it lays over the popover when it has
/// a question for the user.
const CHALLENGE: Vec2 = vec2(300.0, 65.0);
pub const WRONG_CODE: &str = "That code doesn't match. Check the newest email.";

/// The sign-in popover's state.
#[derive(Default)]
pub struct SignIn {
    pub open: bool,
    /// Opened since the last frame: its motion starts at the next.
    fresh: bool,
    opened: f64,
    pub step: Step,
    pub email: String,
    pub code: String,
    /// The code last sent for checking, so a full code goes once.
    tried: String,
    /// When each cell was filled, for its pop.
    filled_at: [f64; CODE_LEN],
    /// The bot check is running for the email asked for; the page answers through
    /// `host::challenged`.
    pub checking: bool,
    /// The bot check has a question for the user, in the space it is given here.
    pub challenge: bool,
    /// The desktop app: its email has a code and no link (set by the editor).
    pub app: bool,
    pub error: Option<String>,
    /// When the code was found wrong: the cells shake, then clear.
    bad_at: Option<f64>,
    /// When the email went, for Resend's countdown.
    sent_at: f64,
    /// When signing in was seen to work, while the popover asked for it.
    done_at: Option<f64>,
    /// Whether the account was signed in at the last frame (None before it was known).
    was_in: Option<bool>,
    /// Focus the step's field at the next frame.
    focus: bool,
    /// The view last drawn, when it came up, and each view's height as last measured.
    view: Option<View>,
    view_at: f64,
    heights: [f32; 4],
    /// The height the popover grows or shrinks from, to, and since when.
    h_from: f32,
    h_to: f32,
    h_at: f64,
}

impl SignIn {
    /// The email went: type its code.
    pub fn sent(&mut self, now: f64) {
        self.step = Step::Code;
        self.sent_at = now;
        self.code.clear();
        self.tried.clear();
        self.error = None;
        self.bad_at = None;
        self.challenge = false;
        self.focus = true;
    }

    /// The code typed is not the newest email's: the cells shake and clear.
    pub fn wrong_code(&mut self, now: f64) {
        self.bad_at = Some(now);
        self.error = Some(WRONG_CODE.into());
    }

    /// Asking went wrong, in words.
    pub fn failed(&mut self, why: &str) {
        self.checking = false;
        self.challenge = false;
        self.error = Some(why.into());
    }

    /// The address the sign-in email goes to, while the field still holds a plausible one.
    pub fn email_to_send(&self) -> Option<String> {
        looks_like_email(&self.email).then(|| self.email.trim().to_string())
    }

    /// Whether the code typed goes now: once all six are in, and again once it was edited, so
    /// the same code can go again after the network or the rate limit failed it.
    fn code_to_send(&mut self, busy: bool) -> bool {
        if self.code.len() < CODE_LEN {
            self.tried.clear();
            return false;
        }
        if self.code == self.tried || busy {
            return false;
        }
        self.tried = self.code.clone();
        true
    }

    /// What the popover shows, for the checks: "email", "code", "done" or "menu".
    pub fn shown(&self) -> Option<&'static str> {
        self.view.filter(|_| self.open).map(|v| match v {
            View::Email => "email",
            View::Code => "code",
            View::Done => "done",
            View::Menu => "menu",
        })
    }

    fn back_to_email(&mut self) {
        self.step = Step::Email;
        self.code.clear();
        self.tried.clear();
        self.error = None;
        self.bad_at = None;
        self.done_at = None;
        self.checking = false;
        self.challenge = false;
    }

    fn shown_h(&self, now: f64) -> f32 {
        self.h_from + (self.h_to - self.h_from) * ease_out(((now - self.h_at) / 0.24) as f32)
    }
}

/// The interface's ease-out, cubic-bezier(0.23, 1, 0.32, 1): fast at once, settling softly.
pub(super) fn ease_out(t: f32) -> f32 {
    // The ends exactly, which the search below only comes near.
    if t <= 0.0 || t >= 1.0 {
        return t.clamp(0.0, 1.0);
    }
    let bez = |a: f32, b: f32, s: f32| 3.0 * a * s * (1.0 - s) * (1.0 - s) + 3.0 * b * s * s * (1.0 - s) + s * s * s;
    let (mut lo, mut hi) = (0.0, 1.0);
    for _ in 0..24 {
        let mid = (lo + hi) / 2.0;
        if bez(0.23, 0.32, mid) < t {
            lo = mid;
        } else {
            hi = mid;
        }
    }
    bez(1.0, 1.0, (lo + hi) / 2.0)
}

/// Whether to focus the step's field now: once asked, and not while a press is in flight, as
/// the press that opened the popover would take the focus away again.
fn take_focus(ui: &Ui, focus: &mut bool) -> bool {
    let free = ui.input(|i| !i.pointer.any_down() && !i.pointer.any_released());
    *focus && free && {
        *focus = false;
        true
    }
}

/// A plausible address: something on both sides of one @, and no spaces.
fn looks_like_email(s: &str) -> bool {
    let s = s.trim();
    match s.split_once('@') {
        Some((a, b)) => {
            !a.is_empty()
                && b.contains('.')
                && !b.starts_with('.')
                && !b.ends_with('.')
                && !s.contains(' ')
                && !b.contains('@')
        }
        None => false,
    }
}

/// The wrong code's shake: left 5, right 4, left 2, back, over 320 ms.
fn shake(t: f64) -> f32 {
    const KEYS: [(f32, f32); 5] = [(0.0, 0.0), (0.2, -5.0), (0.45, 4.0), (0.7, -2.0), (1.0, 0.0)];
    let u = (t / SHAKE) as f32;
    if !(0.0..1.0).contains(&u) {
        return 0.0;
    }
    let k = KEYS.windows(2).find(|w| u < w[1].0).unwrap_or(&KEYS[3..5]);
    let (a, b) = (k[0], k[1]);
    a.1 + (b.1 - a.1) * ease_out((u - a.0) / (b.0 - a.0))
}

/// The pulse of a waiting dot: full, down to a quarter, back, every 1.6 s.
fn pulse(now: f64) -> f32 {
    let s = (0.5 - 0.5 * (now * std::f64::consts::TAU / 1.6).cos()) as f32;
    1.0 - 0.75 * s
}

/// Paragraph text: `parts` in their fonts and colours, wrapped to `width`.
fn para(ui: &Ui, parts: &[(&str, FontId, Color32)], width: f32, line: f32) -> Arc<Galley> {
    let mut job = LayoutJob::default();
    for (text, font, color) in parts {
        job.append(
            text,
            0.0,
            TextFormat { font_id: font.clone(), color: *color, line_height: Some(line), ..Default::default() },
        );
    }
    job.wrap.max_width = width;
    ui.fonts_mut(|f| f.layout_job(job))
}

/// A step's heading and the paragraph under it; returns the y below them.
fn heading(ui: &Ui, x: f32, y: f32, title: &str, body: &[(&str, FontId, Color32)], width: f32) -> f32 {
    let t = ui.painter().layout_no_wrap(title.into(), sans_semibold(15.0), th().text);
    let th_ = t.size().y;
    ui.painter().galley(pos2(x, y), t, th().text);
    let g = para(ui, body, width, 19.0);
    let gh = g.size().y;
    ui.painter().galley(pos2(x, y + th_ + 4.0), g, th().muted);
    y + th_ + 4.0 + gh
}

/// A spinner: three quarters of a ring, turning every 600 ms.
fn spinner(p: &Painter, c: Pos2, r: f32, color: Color32, now: f64) {
    let start = (now / 0.6).fract() as f32 * std::f32::consts::TAU;
    let points = (0..=24)
        .map(|i| {
            let a = start + i as f32 / 24.0 * std::f32::consts::TAU * 0.75;
            c + vec2(a.cos(), a.sin()) * r
        })
        .collect();
    p.add(Shape::line(points, Stroke::new(2.0, color)));
}

/// The check of the Done step, drawn `t` (0 to 1) of the way, in a `size` box about `c`.
fn check_mark(p: &Painter, c: Pos2, size: f32, t: f32, color: Color32) {
    let at = |x: f32, y: f32| c + vec2(x - 12.0, y - 12.0) * (size / 24.0);
    let points = [at(4.0, 12.0), at(9.0, 17.0), at(20.0, 6.0)];
    let w = 2.0 * size / 24.0;
    let lens = [points[0].distance(points[1]), points[1].distance(points[2])];
    let mut left = t * (lens[0] + lens[1]);
    if left <= 0.0 {
        return;
    }
    let mut path = vec![points[0]];
    for (k, len) in lens.into_iter().enumerate() {
        let (a, b) = (points[k], points[k + 1]);
        if left >= len {
            path.push(b);
            left -= len;
        } else {
            path.push(a + (b - a) * (left / len));
            break;
        }
    }
    // Round caps and joint, as the SVG's.
    for q in &path {
        p.circle_filled(*q, w / 2.0, color);
    }
    p.add(Shape::line(path, Stroke::new(w, color)));
}

/// The account's avatar: its initial on the blue-to-violet gradient, a circle of radius `r`.
fn avatar(p: &Painter, c: Pos2, r: f32, initial: &str, font: FontId) {
    let (a, b) = (Color32::from_rgb(0x60, 0xa5, 0xfa), Color32::from_rgb(0xa7, 0x8b, 0xfa));
    // linear-gradient(135deg): top left to bottom right.
    let at = |q: Pos2| {
        let t = ((q.x - c.x) + (q.y - c.y)) / (2.0 * r * std::f32::consts::SQRT_2) + 0.5;
        a.lerp_to_gamma(b, t.clamp(0.0, 1.0))
    };
    let mut mesh = Mesh::default();
    mesh.colored_vertex(c, at(c));
    const N: u32 = 40;
    for i in 0..N {
        let angle = i as f32 / N as f32 * std::f32::consts::TAU;
        let dir = vec2(angle.cos(), angle.sin());
        let inner = c + dir * (r - 0.5);
        mesh.colored_vertex(inner, at(inner));
        // A feathered edge, for anti-aliasing.
        mesh.colored_vertex(c + dir * (r + 0.5), Color32::TRANSPARENT);
    }
    for i in 0..N {
        let (inner, outer) = (1 + 2 * i, 2 + 2 * i);
        let (inner2, outer2) = (1 + 2 * ((i + 1) % N), 2 + 2 * ((i + 1) % N));
        mesh.add_triangle(0, inner, inner2);
        mesh.add_triangle(inner, outer, outer2);
        mesh.add_triangle(inner, outer2, inner2);
    }
    p.add(Shape::mesh(mesh));
    p.text(c, Align2::CENTER_CENTER, initial, font, Color32::from_rgb(0x0a, 0x0a, 0x0a));
}

/// Draw `draw` at `alpha` as if blurred by `blur` points: four faint offset copies around a
/// fainter one, which reads as a soft cross-fade without a blur pass.
fn soft(p: &Painter, alpha: f32, blur: f32, draw: impl Fn(&Painter, Vec2)) {
    if alpha <= 0.01 {
        return;
    }
    if blur < 0.3 {
        let mut q = p.clone();
        q.multiply_opacity(alpha);
        return draw(&q, Vec2::ZERO);
    }
    let mut q = p.clone();
    q.multiply_opacity(alpha * 0.5);
    draw(&q, Vec2::ZERO);
    let mut q = p.clone();
    q.multiply_opacity(alpha * 0.25);
    let d = blur * 0.5;
    for v in [vec2(d, 0.0), vec2(-d, 0.0), vec2(0.0, d), vec2(0.0, -d)] {
        draw(&q, v);
    }
}

/// The account's initial: the email's first letter.
fn initial(row: &AccountRow) -> String {
    row.email.as_deref().and_then(|e| e.chars().next()).map_or("?".into(), |c| c.to_uppercase().collect())
}

/// An input in the mock's style: 36 tall, an inset fill, a ring when focused.
fn input_frame(p: &Painter, r: Rect, radius: u8, focused: bool, bad: bool) {
    if focused && !bad {
        p.rect_stroke(
            r,
            CornerRadius::same(radius),
            Stroke::new(3.0, th().ring.gamma_multiply(0.25)),
            StrokeKind::Outside,
        );
    }
    let line = if bad {
        th().danger
    } else if focused {
        th().ring
    } else {
        th().line2
    };
    p.rect(r, CornerRadius::same(radius), th().scrim(100), Stroke::new(1.0, line), StrokeKind::Inside);
}

/// The popover's primary button; a spinner while `busy`, half faded when it cannot be pressed.
fn primary(ui: &mut Ui, r: Rect, label: &str, id: &str, enabled: bool, busy: bool, now: f64) -> egui::Response {
    let live = enabled && !busy;
    let resp = ui.interact(r, Id::unique(id), if live { Sense::click() } else { Sense::hover() });
    let fill = if live && resp.hovered() { th().accent_hover } else { th().accent };
    let alpha = if live { 1.0 } else { 0.5 };
    let p = ui.painter();
    p.rect_filled(r, CornerRadius::same(9), fill.gamma_multiply(alpha));
    if busy {
        spinner(p, r.center(), 6.0, th().on_accent.gamma_multiply(alpha), now);
    } else {
        p.text(r.center(), Align2::CENTER_CENTER, label, sans_medium(14.0), th().on_accent.gamma_multiply(alpha));
    }
    resp
}

/// Underlined small text that can be pressed; returns its rect and whether it was.
fn link(ui: &mut Ui, at: Pos2, text: &str, id: &str) -> (Rect, bool) {
    let probe = ui.painter().layout_no_wrap(text.into(), sans(12.0), th().muted);
    let r = Rect::from_min_size(at, probe.size());
    let resp = ui.interact(r.expand(2.0), Id::unique(id), Sense::click());
    let color = if resp.hovered() { th().text } else { th().muted };
    let job = LayoutJob::single_section(
        text.into(),
        TextFormat { font_id: sans(12.0), color, underline: Stroke::new(1.0, color), ..Default::default() },
    );
    let g = ui.fonts_mut(|f| f.layout_job(job));
    ui.painter().galley(at, g, color);
    (r, resp.clicked())
}

impl Hybrid {
    /// Open the account popover (the top bar's account control, and Cmd-K's Sign in and
    /// Account), closing the menus it would sit over.
    pub fn open_account(&mut self) {
        self.cmdk = None;
        self.theme_menu = false;
        self.projects_open = false;
        self.file_menu = false;
        if !self.sign_in.open {
            self.sign_in.open = true;
            self.sign_in.fresh = true;
            self.sign_in.focus = true;
        }
    }

    /// How wide the account control is: "Sign in", narrowing to the avatar as the account
    /// signs in (and back).
    pub(super) fn account_chip_width(&self, ctx: &Context) -> f32 {
        let Some(a) = &self.account else { return 0.0 };
        let t = ctx.animate_bool_with_time_and_easing(Id::unique("account-chip"), a.signed_in, 0.22, ease_out);
        let text = ctx.fonts_mut(|f| f.layout_no_wrap("Sign in".into(), sans_medium(13.0), th().text)).size().x;
        let out = 10.0 + text + 10.0;
        out + (30.0 - out) * t
    }

    /// The account control in rect `r` of the top bar: "Sign in", or the avatar with its sync
    /// dot, cross-fading with a 3 px blur over 220 ms as the account signs in or out.
    pub(super) fn account_chip(&mut self, ui: &mut Ui, r: Rect) {
        let Some(a) = self.account.clone() else { return };
        let ctx = ui.ctx().clone();
        let now = ui.input(|i| i.time);
        let t = ctx.animate_bool_with_time_and_easing(Id::unique("account-chip"), a.signed_in, 0.22, ease_out);
        let resp = ui.interact(r, Id::unique("account"), Sense::click());
        self.hits.insert("account".into(), r);
        let p = ui.painter().with_clip_rect(r.intersect(ui.clip_rect()));
        if self.sign_in.open {
            p.rect_filled(r, CornerRadius::same(8), th().accent_bg);
        } else if resp.hovered() {
            p.rect_filled(r, CornerRadius::same(8), th().hover);
        }
        // Signed out: the words, shrinking a little as they go.
        soft(&p, 1.0 - t, 3.0 * t, |p, d| {
            let k = 1.0 - 0.04 * t;
            let color = if resp.hovered() || self.sign_in.open { th().text } else { th().muted };
            let g = p.layout_no_wrap("Sign in".into(), sans_medium(13.0 * k), color);
            p.galley(pos2(r.left() + 10.0, r.center().y - g.size().y / 2.0) + d, g, color);
        });
        // Signed in: the avatar, growing in.
        let dot = match a.sync {
            Sync::Synced => th().ok,
            Sync::Syncing => th().accent.gamma_multiply(pulse(now)),
            Sync::Waiting => th().mid,
        };
        let letter = initial(&a);
        soft(&p, t, 3.0 * (1.0 - t), |p, d| {
            let k = 0.9 + 0.1 * t;
            let c = r.center() + d;
            avatar(p, c, 12.0 * k, &letter, sans_semibold(12.0 * k));
            // The dot, 9 across with a 2 px ring of the panel, over the avatar's corner.
            let at = c + vec2(12.0 * k + 2.0 - 4.5, 12.0 * k + 2.0 - 4.5);
            p.circle_filled(at, 4.5, th().panel_solid);
            p.circle_filled(at, 2.5, dot);
        });
        if a.signed_in && a.sync == Sync::Syncing {
            ctx.request_repaint_after(std::time::Duration::from_millis(50));
        }
        if resp.clicked() {
            if self.sign_in.open {
                self.sign_in.open = false;
            } else {
                self.open_account();
            }
        }
    }

    /// Follow the account: a sign-in the popover asked for shows its check, then closes; one
    /// from another tab, with the popover shut, says so; a sign-out resets the popover.
    fn account_changes(&mut self, ctx: &Context, row: &AccountRow, now: f64) {
        let was = self.sign_in.was_in.replace(row.signed_in);
        let said = match row.email.as_deref().or(Some(self.sign_in.email.trim()).filter(|e| !e.is_empty())) {
            Some(e) => format!("Signed in as {e}"),
            None => "Signed in".to_string(),
        };
        match (was, row.signed_in) {
            (Some(false), true) => {
                let s = &mut self.sign_in;
                s.checking = false;
                s.challenge = false;
                s.error = None;
                if s.open {
                    s.step = Step::Done;
                    s.done_at = Some(now);
                } else {
                    s.back_to_email();
                    self.toast(ctx, said.clone());
                }
            }
            (Some(true), false) => {
                self.sign_in.open = false;
                self.sign_in.back_to_email();
            }
            _ => {}
        }
        if let Some(t) = self.sign_in.done_at
            && now >= t + DONE_FOR
        {
            self.sign_in.open = false;
            self.sign_in.back_to_email();
            self.toast(ctx, said);
        }
    }

    /// The account popover, under the account control.
    pub(super) fn account_popover(&mut self, ctx: &Context) {
        let now = ctx.input(|i| i.time);
        let Some(row) = self.account.clone() else {
            self.sign_in.open = false;
            self.sign_in.was_in = None;
            return;
        };
        self.account_changes(ctx, &row, now);
        if !self.sign_in.open {
            self.sign_in.view = None;
            return;
        }
        let screen = ctx.content_rect();
        let w = W.min(screen.width() - 24.0);
        let view = match self.sign_in.step {
            _ if row.signed_in && self.sign_in.step != Step::Done => View::Menu,
            Step::Email => View::Email,
            Step::Code => View::Code,
            Step::Done => View::Done,
        };
        let s = &mut self.sign_in;
        if std::mem::take(&mut s.fresh) {
            s.opened = now;
            s.view = None;
        }
        if s.view != Some(view) {
            // A step that follows another slides in; the first one shows with the popover.
            s.view_at = if s.view.is_some() { now } else { f64::NEG_INFINITY };
            s.view = Some(view);
        }
        // The height: as measured, growing or shrinking to a new one over 240 ms.
        let target = s.heights[view as usize];
        if s.view_at == f64::NEG_INFINITY && now == s.opened {
            (s.h_from, s.h_to, s.h_at) = (target, target, f64::NEG_INFINITY);
        } else if target > 0.0 && target != s.h_to {
            (s.h_from, s.h_to, s.h_at) = (s.shown_h(now), target, now);
        }
        let h = if s.h_to > 0.0 { s.shown_h(now) } else { 240.0 };
        let rect = Rect::from_min_size(pos2(screen.right() - 12.0 - w, 12.0 + 44.0 + 6.0), vec2(w, h));
        self.hits.insert("account-popover".into(), rect);

        // It scales in from the account control: 0.96 and 4 px up to where it rests, 200 ms.
        let id = "account-popover";
        let layer = egui::LayerId::new(Order::Foreground, Id::unique(id));
        Self::fit_panel(ctx, id, Order::Foreground, rect);
        let fit = ctx.layer_transform_to_global(layer).unwrap_or(TSTransform::IDENTITY);
        let s = &self.sign_in;
        let e = ease_out(((now - s.opened) / 0.2) as f32);
        let fade = ease_out(((now - s.opened) / 0.18) as f32);
        let chip = self.hits.get("account").copied().unwrap_or(Rect::NOTHING);
        let origin = pos2(if chip.is_positive() { chip.center().x } else { rect.right() - 14.0 }, rect.top());
        let k = 0.96 + 0.04 * e;
        let anim = TSTransform::new(origin.to_vec2() * (1.0 - k) + vec2(0.0, -4.0 * (1.0 - e)), k);
        ctx.set_transform_layer(layer, fit * anim);
        let ve = ease_out(((now - s.view_at) / 0.24) as f32);
        let mut measured = 0.0;
        Area::new(Id::unique(id)).fade_in(false).order(Order::Foreground).fixed_pos(rect.min).constrain(false).show(
            ctx,
            |ui| {
                ui.multiply_opacity(fade);
                ui.set_clip_rect(rect.expand(64.0));
                let shadow = ui.painter().add(Shape::Noop);
                let fill = ui.painter().add(Shape::Noop);
                ui.allocate_rect(rect, Sense::click());
                let mut body = ui.new_child(UiBuilder::new().max_rect(rect));
                body.set_clip_rect(rect.shrink(1.0));
                body.multiply_opacity(ve);
                let o = pos2(rect.left() + 8.0 * (1.0 - ve), rect.top());
                measured = match view {
                    View::Email => self.email_view(&mut body, o, w, &row, now),
                    View::Code => self.code_view(&mut body, o, w, &row, now),
                    View::Done => self.done_view(&mut body, o, w, now),
                    View::Menu => self.menu_view(&mut body, o, w, &row, now),
                };
                let p = ui.painter();
                p.set(
                    shadow,
                    Shadow { offset: [0, 16], blur: 48, spread: 0, color: th().shadow }
                        .as_shape(rect, CornerRadius::same(14)),
                );
                p.set(fill, Shape::rect_filled(rect, CornerRadius::same(14), th().panel_solid));
                p.rect_stroke(rect, CornerRadius::same(14), Stroke::new(1.0, th().line), StrokeKind::Inside);
            },
        );
        let s = &mut self.sign_in;
        let first = s.heights[view as usize] == 0.0;
        s.heights[view as usize] = measured;
        if first && s.h_to == 0.0 {
            (s.h_from, s.h_to, s.h_at) = (measured, measured, f64::NEG_INFINITY);
        }
        // Motion, the pulse and the countdown keep drawing; a still view does not.
        let moving = now - s.opened < 0.3
            || now - s.view_at < 0.3
            || now - s.h_at < 0.3
            || s.bad_at.is_some_and(|t| now - t < SHAKE + 0.05)
            || measured != s.h_to;
        if moving || view == View::Done || s.checking || row.busy {
            ctx.request_repaint();
        } else if view == View::Code {
            ctx.request_repaint_after(std::time::Duration::from_millis(50));
        } else if view == View::Menu && now - s.view_at.max(s.opened) < 0.7 {
            ctx.request_repaint();
        }
        let shown = fit.mul_rect(rect);
        let outside = ctx.input(|i| {
            i.pointer.any_pressed() && i.pointer.interact_pos().is_some_and(|p| !shown.contains(p) && !chip.contains(p))
        });
        if outside {
            self.sign_in.open = false;
        }
    }

    fn send_code(&mut self) {
        let s = &mut self.sign_in;
        s.checking = true;
        s.error = None;
        self.actions.push(Action::SendCode);
    }

    // The email step: what an account is for, the address, and Email me a sign-in code.
    fn email_view(&mut self, ui: &mut Ui, o: Pos2, w: f32, row: &AccountRow, now: f64) -> f32 {
        let iw = w - 2.0 * PAD;
        let x = o.x + PAD;
        let body = "Keep your scenes in sync between devices. Optional: the editor works the same without an account.";
        let mut y = heading(ui, x, o.y + PAD, "Sign in to vxr", &[(body, sans(13.0), th().muted)], iw) + 14.0;
        let f = Rect::from_min_size(pos2(x, y), vec2(iw, 36.0));
        let id = Id::unique("account-email");
        if take_focus(ui, &mut self.sign_in.focus) {
            ui.memory_mut(|m| m.request_focus(id));
        }
        let focused = ui.memory(|m| m.has_focus(id));
        input_frame(ui.painter(), f, 9, focused, false);
        let tr = f.shrink2(vec2(12.0, 9.0));
        if self.sign_in.email.is_empty() {
            ui.painter().text(
                pos2(tr.left(), f.center().y),
                Align2::LEFT_CENTER,
                "you@example.com",
                sans(14.0),
                th().faint,
            );
        }
        let edit = egui::TextEdit::singleline(&mut self.sign_in.email)
            .id(id)
            .font(sans(14.0))
            .text_color(th().text)
            .frame(egui::Frame::NONE)
            .margin(egui::Margin::ZERO)
            .desired_width(tr.width());
        let resp = ui.put(tr, edit);
        self.hits.insert("account-email".into(), f);
        if resp.changed() {
            self.sign_in.error = None;
        }
        let enter = resp.lost_focus() && ui.input(|i| i.key_pressed(Key::Enter));
        y += 36.0 + 10.0;
        let busy = self.sign_in.checking || row.busy;
        let ok = looks_like_email(&self.sign_in.email);
        let b = Rect::from_min_size(pos2(x, y), vec2(iw, 36.0));
        let pressed = primary(ui, b, "Email me a sign-in code", "account-send", ok, busy, now).clicked();
        self.hits.insert("account-send".into(), b);
        if (pressed || enter) && ok && !busy {
            self.send_code();
        }
        y += 36.0;
        if let Some(e) = &self.sign_in.error {
            let g = para(ui, &[(e, sans(12.0), th().danger)], iw, 17.0);
            let gh = g.size().y;
            ui.painter().galley(pos2(x, y + 8.0), g, th().danger);
            y += 8.0 + gh;
        }
        y = self.challenge_slot(o, w, y);
        let fine = "New here? The same email makes your account. No password, no passkey.";
        let g = para(ui, &[(fine, sans(12.0), th().faint)], iw, 17.0);
        let gh = g.size().y;
        ui.painter().galley(pos2(x, y + 12.0), g, th().faint);
        y + 12.0 + gh + PAD - o.y
    }

    /// The room the page's bot check takes when it has a question for the user, under `y`.
    fn challenge_slot(&mut self, o: Pos2, w: f32, y: f32) -> f32 {
        if !self.sign_in.challenge {
            self.hits.remove("account-challenge");
            return y;
        }
        let r = Rect::from_min_size(pos2(o.x + ((w - CHALLENGE.x) / 2.0).max(0.0), y + 10.0), CHALLENGE);
        self.hits.insert("account-challenge".into(), r);
        y + 10.0 + CHALLENGE.y
    }

    // The code step: where the email went, the six cells, the link to wait for, and Resend.
    fn code_view(&mut self, ui: &mut Ui, o: Pos2, w: f32, row: &AccountRow, now: f64) -> f32 {
        let iw = w - 2.0 * PAD;
        let x = o.x + PAD;
        let email = self.sign_in.email.trim().to_string();
        let app = self.sign_in.app;
        let body = if app {
            [
                ("We sent a code to ", sans(13.0), th().muted),
                (email.as_str(), sans_medium(13.0), th().text),
                (". Type it here to sign in to this app.", sans(13.0), th().muted),
            ]
        } else {
            [
                ("We sent a link and a code to ", sans(13.0), th().muted),
                (email.as_str(), sans_medium(13.0), th().text),
                (". Click the link in any tab, or type the code here.", sans(13.0), th().muted),
            ]
        };
        let mut y = heading(ui, x, o.y + PAD, "Check your email", &body, iw) + 14.0 + 4.0;
        self.code_cells(ui, pos2(x, y), now, row.busy);
        y += CELL_H + 2.0;
        let err = self.sign_in.error.clone().unwrap_or_default();
        let g = para(ui, &[(&err, sans(12.0), th().danger)], iw, 17.0);
        let gh = g.size().y.max(18.0);
        ui.painter().galley(pos2(x, y + 6.0), g, th().danger);
        y += 6.0 + gh + 10.0;
        // Waiting for the link (the page asks the Worker again when another tab signs in), or
        // checking the code. The app has no link to wait for: how long the code lasts.
        let cy = y + 7.0;
        let checking = row.busy && self.sign_in.code.len() == CODE_LEN;
        if checking {
            spinner(ui.painter(), pos2(x + 5.0, cy), 4.5, th().faint, now);
            ui.painter().text(pos2(x + 15.0, cy), Align2::LEFT_CENTER, "Checking the code…", sans(12.0), th().faint);
        } else if app {
            let text = "The code works for 10 minutes.";
            ui.painter().text(pos2(x, cy), Align2::LEFT_CENTER, text, sans(12.0), th().faint);
        } else {
            ui.painter().circle_filled(pos2(x + 3.5, cy), 3.5, th().accent.gamma_multiply(pulse(now)));
            let text = "Waiting for you to click the link…";
            ui.painter().text(pos2(x + 15.0, cy), Align2::LEFT_CENTER, text, sans(12.0), th().faint);
        }
        y += 14.0 + 12.0;
        // Resend, once 30 s have passed, and another address.
        let left = RESEND - (now - self.sign_in.sent_at);
        let mut lx = x;
        if left > 0.0 || self.sign_in.checking {
            let words = if self.sign_in.checking {
                "Sending…".to_string()
            } else {
                format!("Resend in 0:{:02}", left.ceil() as u32)
            };
            let g = ui.painter().layout_no_wrap(words, sans(12.0), th().faint);
            lx += g.size().x;
            ui.painter().galley(pos2(x, y), g, th().faint);
            self.hits.remove("account-resend");
        } else {
            let (r, pressed) = link(ui, pos2(x, y), "Resend the email", "account-resend");
            self.hits.insert("account-resend".into(), r);
            lx = r.right();
            if pressed {
                self.send_code();
            }
        }
        let g = ui.painter().layout_no_wrap(" · ".into(), sans(12.0), th().faint);
        let dw = g.size().x;
        ui.painter().galley(pos2(lx, y), g, th().faint);
        let (r, pressed) = link(ui, pos2(lx + dw, y), "Use a different email", "account-change");
        self.hits.insert("account-change".into(), r);
        if pressed {
            self.sign_in.back_to_email();
            self.sign_in.focus = true;
        }
        y += 17.0;
        y = self.challenge_slot(o, w, y);
        y + PAD - o.y
    }

    /// The six cells, drawn from a transparent field over them: digits only (typed or
    /// pasted, spaces and dashes dropped), sent once all six are in.
    fn code_cells(&mut self, ui: &mut Ui, at: Pos2, now: f64, busy: bool) {
        let id = Id::unique("account-code");
        let s = &mut self.sign_in;
        let full = Rect::from_min_size(at, vec2(CODE_LEN as f32 * (CELL_W + CELL_GAP) - CELL_GAP + HALF_GAP, CELL_H));
        self.hits.insert("account-code".into(), full);
        // After the shake, the wrong code clears for the next try.
        if let Some(t) = s.bad_at
            && now >= t + SHAKE
            && !s.code.is_empty()
            && s.code == s.tried
        {
            s.code.clear();
            s.tried.clear();
            s.bad_at = None;
            s.focus = true;
        }
        if take_focus(ui, &mut s.focus) {
            ui.memory_mut(|m| m.request_focus(id));
        }
        let before = s.code.clone();
        ui.scope(|ui| {
            ui.visuals_mut().text_cursor.stroke = Stroke::NONE;
            ui.visuals_mut().selection.bg_fill = Color32::TRANSPARENT;
            ui.visuals_mut().selection.stroke = Stroke::NONE;
            let edit = egui::TextEdit::singleline(&mut s.code)
                .id(id)
                .font(mono(20.0))
                .text_color(Color32::TRANSPARENT)
                .frame(egui::Frame::NONE)
                .margin(egui::Margin::ZERO)
                .desired_width(full.width());
            ui.put(full, edit);
        });
        let clean: String = s.code.chars().filter(char::is_ascii_digit).take(CODE_LEN).collect();
        if clean != s.code {
            s.code = clean;
        }
        if s.code != before {
            for t in s.filled_at.iter_mut().take(s.code.len()).skip(before.len()) {
                *t = now;
            }
            if !s.code.is_empty() {
                s.bad_at = None;
                s.error = None;
            }
        }
        if s.code_to_send(busy) {
            s.error = None;
            self.actions.push(Action::VerifyCode);
        }
        let s = &self.sign_in;
        let focused = ui.memory(|m| m.has_focus(id));
        let bad = s.bad_at.is_some();
        let dx = s.bad_at.map_or(0.0, |t| shake(now - t));
        let n = s.code.len();
        for i in 0..CODE_LEN {
            let left = at.x + dx + i as f32 * (CELL_W + CELL_GAP) + if i >= 3 { HALF_GAP } else { 0.0 };
            let cell = Rect::from_min_size(pos2(left, at.y), vec2(CELL_W, CELL_H));
            let digit = s.code.chars().nth(i);
            // A digit pops in from 0.92.
            let k = if digit.is_some() { 0.92 + 0.08 * ease_out(((now - s.filled_at[i]) / 0.16) as f32) } else { 1.0 };
            let cell = Rect::from_center_size(cell.center(), cell.size() * k);
            input_frame(ui.painter(), cell, 10, focused && i == n.min(CODE_LEN - 1), bad);
            if let Some(c) = digit {
                ui.painter().text(cell.center(), Align2::CENTER_CENTER, c, mono_medium(20.0 * k), th().text);
            }
        }
    }

    // Done: a check drawn in, and what signing in did.
    fn done_view(&mut self, ui: &mut Ui, o: Pos2, w: f32, now: f64) -> f32 {
        let iw = w - 2.0 * PAD;
        let x = o.x + PAD;
        let y = o.y + PAD;
        let c = pos2(x + 22.0, y + 22.0);
        ui.painter().circle_filled(c, 22.0, th().ok.gamma_multiply(0.12));
        let since = now - self.sign_in.done_at.unwrap_or(now);
        check_mark(ui.painter(), c, 22.0, ease_out(((since - 0.08) / 0.36) as f32), th().ok);
        let body = "New scenes you save go to your library, so they open on your other devices too.";
        let y = heading(ui, x, y + 44.0 + 12.0, "You're signed in", &[(body, sans(13.0), th().muted)], iw);
        y + PAD - o.y
    }

    // Signed in: who, the library's use, how sync stands, then Add to library (when the scene
    // is not in it), Open library and Sign out.
    fn menu_view(&mut self, ui: &mut Ui, o: Pos2, w: f32, row: &AccountRow, now: f64) -> f32 {
        let p = ui.painter().clone();
        // Who.
        let c = pos2(o.x + 14.0 + 16.0, o.y + 14.0 + 16.0);
        avatar(&p, c, 16.0, &initial(row), sans_semibold(14.0));
        let tx = o.x + 14.0 + 32.0 + 10.0;
        let email = row.email.clone().unwrap_or_else(|| "Your account".into());
        let g = agents::one_line(ui, &email, sans_medium(13.0), th().text, o.x + w - 14.0 - tx);
        p.galley(pos2(tx, c.y - 1.0 - g.size().y), g, th().text);
        p.text(pos2(tx, c.y + 1.0), Align2::LEFT_TOP, "Signed in", sans(12.0), th().faint);
        let mut y = o.y + 14.0 + 32.0 + 12.0;
        // The library's use.
        let x = o.x + 14.0;
        p.text(pos2(x, y), Align2::LEFT_TOP, "Library", sans(12.0), th().muted);
        let used = format!("{} of {}", crate::projects::bytes(row.used), crate::projects::bytes(row.cap));
        p.text(pos2(o.x + w - 14.0, y), Align2::RIGHT_TOP, used, mono(12.0), th().muted);
        y += 15.0 + 6.0;
        let bar = Rect::from_min_size(pos2(x, y), vec2(w - 28.0, 6.0));
        p.rect_filled(bar, CornerRadius::same(3), th().tint(18));
        let frac = if row.cap > 0 { (row.used as f32 / row.cap as f32).clamp(0.0, 1.0) } else { 0.0 };
        let grow = ease_out(((now - self.sign_in.view_at.max(self.sign_in.opened) - 0.12) / 0.52) as f32);
        let fw = bar.width() * frac * grow;
        if fw > 0.0 {
            let f = Rect::from_min_size(bar.min, vec2(fw.max(1.0), bar.height()));
            p.rect_filled(f, CornerRadius::same(3), th().text);
        }
        y += 6.0 + 12.0;
        // How sync stands.
        let scenes = match row.scenes {
            0 => "no scenes yet".to_string(),
            1 => "1 scene".to_string(),
            n => format!("{n} scenes"),
        };
        let (glyph, color, words) = match row.sync {
            Sync::Synced => (icon::CLOUD_CHECK, th().ok, format!("Synced · {scenes}")),
            Sync::Syncing => (icon::REFRESH_CW, th().accent, format!("Syncing · {scenes}")),
            Sync::Waiting => (icon::CLOUD, th().mid, format!("Waiting to sync · {scenes}")),
        };
        let cy = y + 7.5;
        p.text(pos2(x + 7.0, cy), Align2::CENTER_CENTER, glyph, icon_font(14.0), color);
        p.text(pos2(x + 20.0, cy), Align2::LEFT_CENTER, words, sans(12.0), th().faint);
        y += 15.0 + 12.0;
        // The menu.
        p.line_segment([pos2(o.x + 1.0, y + 0.5), pos2(o.x + w - 1.0, y + 0.5)], Stroke::new(1.0, th().line));
        y += 1.0 + 6.0;
        let mut items = Vec::new();
        if row.can_add {
            items.push(("account-add", icon::CLOUD, "Add to library"));
        }
        items.push(("account-library", icon::FOLDER, "Open library"));
        items.push(("account-sign-out", icon::LOG_OUT, "Sign out"));
        let mut picked = None;
        for (id, glyph, label) in items {
            let r = Rect::from_min_size(pos2(o.x + 6.0, y), vec2(w - 12.0, 34.0));
            let resp = ui.interact(r, Id::unique(id), Sense::click());
            self.hits.insert(id.into(), r);
            if resp.hovered() {
                p.rect_filled(r, CornerRadius::same(8), th().hover);
            }
            let color = if resp.hovered() { th().text } else { th().muted };
            p.text(pos2(r.left() + 18.0, r.center().y), Align2::CENTER_CENTER, glyph, icon_font(16.0), color);
            p.text(pos2(r.left() + 36.0, r.center().y), Align2::LEFT_CENTER, label, sans(13.0), th().text);
            if resp.clicked() {
                picked = Some(id);
            }
            y += 34.0;
        }
        match picked {
            Some("account-add") => {
                self.sign_in.open = false;
                self.actions.push(Action::AddToLibrary);
            }
            Some("account-library") => {
                self.sign_in.open = false;
                if !self.projects_open {
                    self.toggle_projects();
                }
            }
            Some(_) => {
                self.sign_in.open = false;
                self.actions.push(Action::SignOut);
            }
            None => {}
        }
        y + 6.0 - o.y
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_ease_runs_from_0_to_1_and_starts_fast() {
        assert_eq!(ease_out(0.0), 0.0);
        assert!((ease_out(1.0) - 1.0).abs() < 1e-4);
        assert!(ease_out(0.2) > 0.6, "{}", ease_out(0.2));
    }

    #[test]
    fn only_plausible_addresses_are_sent() {
        assert!(looks_like_email("you@example.com"));
        assert!(looks_like_email("  you@example.com "));
        for bad in ["", "you", "you@", "@example.com", "you@example", "you @example.com", "a@b@c.com"] {
            assert!(!looks_like_email(bad), "{bad}");
        }
    }

    #[test]
    fn a_code_goes_once_and_again_after_an_edit() {
        let mut s = SignIn { code: "123456".into(), ..Default::default() };
        assert!(!s.code_to_send(true), "not while a call is out");
        assert!(s.code_to_send(false));
        assert!(!s.code_to_send(false), "not twice");
        s.failed("Could not reach vxr.");
        assert!(!s.code_to_send(false), "a failure alone does not resend");
        s.code.pop();
        assert!(!s.code_to_send(false));
        s.code.push('6');
        assert!(s.code_to_send(false), "the same code goes again once retyped");
    }

    #[test]
    fn the_shake_ends_where_it_began() {
        assert_eq!(shake(0.0), 0.0);
        assert_eq!(shake(SHAKE), 0.0);
        assert!(shake(0.2 * SHAKE) < -4.0);
    }
}
