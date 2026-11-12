//! The first-run tour (`docs/decisions.md` "Help and tour"): five steps on a card at the right
//! of the view. Each step plays its Help demo, rings the control it needs and waits for the
//! user to do the real thing, then moves on. The last step rings the Help button, where the
//! demos and the tour itself live. It shows once: Skip or finish sets the seen flag, which the
//! host keeps ([`Action::TourSeen`]). Cmd-K "Take the tour" and Help's top row replay it.

use crate::demo;

use super::help::{TILE_FRAME, draw_image};
use super::*;

/// The card's width, in points.
pub const TOUR_W: f32 = 400.0;
/// The narrowest card that still reads: it moves no further left for the render corner.
const TOUR_MIN_W: f32 = 240.0;
const PAD: f32 = 14.0;
/// The demo's image is this much wider than it is high.
const IMAGE_ASPECT: f32 = 2.4;
/// How long a done step shows "Done" before the next one.
const NEXT_AFTER: f64 = 1.2;

/// A step of the tour, in order.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TourStep {
    Add,
    Look,
    Box,
    Move,
    Help,
}

pub const STEPS: [TourStep; 5] = [TourStep::Add, TourStep::Look, TourStep::Box, TourStep::Move, TourStep::Help];

impl TourStep {
    /// Its name in `state()` and the demo it plays.
    pub fn id(self) -> &'static str {
        match self {
            TourStep::Add => "add",
            TourStep::Look => "orbit",
            TourStep::Box => "box",
            TourStep::Move => "select",
            TourStep::Help => "help",
        }
    }

    /// The demo the card plays for this step: none on the last.
    pub fn demo(self) -> Option<usize> {
        if self == TourStep::Help { None } else { demo::demo(self.id()) }
    }

    fn title(self) -> &'static str {
        match self {
            TourStep::Add => "Add a voxel",
            TourStep::Look => "Look around",
            TourStep::Box => "Draw a box",
            TourStep::Move => "Move a selection",
            TourStep::Help => "Help is up here",
        }
    }

    /// What to do, with `*bold*` keys and gestures: taps and finger gestures on touch.
    pub fn body(self, touch: bool) -> &'static str {
        match (self, touch) {
            (TourStep::Add, false) => "*Click* the scene to add a voxel. *Drag* to keep drawing along that surface.",
            (TourStep::Add, true) => {
                "*Tap* the scene to add a voxel. *Drag* a finger to keep drawing along that surface."
            }
            (TourStep::Look, false) => {
                "*Right-drag* to orbit, *Scroll* to zoom and *Shift+right-drag* to pan. *.* frames the scene again."
            }
            (TourStep::Look, true) => {
                "Drag with *two fingers* to orbit, *pinch* to zoom and drag with *three fingers* to pan."
            }
            (TourStep::Box, false) => {
                "Press *B* and drag the base. Let go, move up to set its height, then *click* to place it."
            }
            (TourStep::Box, true) => {
                "Tap the *box brush* and drag the base. Let go, then drag up to set its height and let go to place it."
            }
            (TourStep::Move, false) => "Press *S* and drag a box around something, then drag it by one of its voxels.",
            (TourStep::Move, true) => {
                "Tap *Select* and drag a box around something, then drag it by one of its voxels."
            }
            (TourStep::Help, false) => {
                "Every tool has a short demo in Help, and the tour is in there too. *F1* or *Cmd-K Help* opens it."
            }
            (TourStep::Help, true) => {
                "Every tool has a short demo in Help, and the tour is in there too. Tap *Help* to open it."
            }
        }
    }

    /// The control the step rings, by its name in `hits`.
    fn ring(self) -> Option<&'static str> {
        match self {
            TourStep::Add => Some("tool-add"),
            TourStep::Look => None,
            TourStep::Box => Some("brush-box"),
            TourStep::Move => Some("select"),
            TourStep::Help => Some("help"),
        }
    }
}

/// The tour as the UI shows it.
#[derive(Default)]
pub struct Tour {
    /// The step on screen (an index in [`STEPS`]), or `None` when the tour is not running.
    pub step: Option<usize>,
    /// The user did this step's action.
    pub done: bool,
    /// When the card first showed the step done (egui's clock), to move on a moment later.
    done_at: Option<f64>,
    /// The seen flag: the tour was skipped or finished once, here or in an earlier run.
    pub seen: bool,
    /// How the last run ended: `skipped` or `finished`, for the checks.
    pub ended: Option<&'static str>,
    /// The demo the card showed in the last frame: only it plays.
    pub shown: Option<usize>,
    /// Start in a model: set by [`Hybrid::start_tour`], done in the next frame, when the UI
    /// knows the scene's mode.
    to_model: bool,
    /// The size of the card's image in points, for the editor to render it at.
    pub image: egui::Vec2,
    /// What the card's demo shows now, set by the editor.
    pub tile: TileView,
}

impl Tour {
    pub fn current(&self) -> Option<TourStep> {
        self.step.map(|i| STEPS[i])
    }
}

impl Hybrid {
    /// At launch: the tour, unless it was seen. The host leaves it out for a link to a scene
    /// or project, an embed, and a returning user.
    pub fn first_tour(&mut self) {
        if !self.tour.seen {
            self.start_tour();
        }
    }

    /// Cmd-K "Take the tour" and Help's top row: the tour from its first step, with Add and
    /// the one-voxel brush, editing a model.
    pub fn start_tour(&mut self) {
        self.close_help();
        self.tour.step = Some(0);
        self.tour.done = false;
        self.tour.done_at = None;
        self.tour.ended = None;
        self.tool = Tool::Add;
        self.shape = Shape::Voxel;
        self.tour.to_model = true;
    }

    /// On a phone a panel the user opened (the palette, the colour editor, the size, bounds or
    /// render panel) takes the card's room: the card waits under it. The flyouts that open by
    /// themselves (selection, mirror and lock axes, a brush size over 1) wait under the card.
    pub(super) fn tour_yields(&self) -> bool {
        self.phone
            && (self.palette_open
                || self.color_edit.is_some()
                || self.size_panel
                || self.bounds_panel.is_some()
                || self.render_panel)
    }

    /// Skip or finish: the tour closes and the host keeps the seen flag.
    pub fn end_tour(&mut self, finished: bool) {
        if self.tour.step.take().is_none() {
            return;
        }
        self.tour.shown = None;
        self.tour.ended = Some(if finished { "finished" } else { "skipped" });
        self.tour.seen = true;
        self.actions.push(Action::TourSeen);
    }

    /// The tour card this frame drew stands over part of the view, where the scene is: the
    /// largest room it leaves the scene in the view `free` (the screen left of a docked
    /// assistant), under the top bar, above the dock, the gesture hints, the stats and the hint
    /// bar and clear of the rail,
    /// the scene tree, the render corner and the camera bar. That is right of the card on a
    /// tablet held sideways, under it on a tablet held upright, left of it on a desktop or a
    /// phone held sideways, and above it on a phone.
    pub fn tour_room(&self, free: Rect) -> Option<Rect> {
        const GAP: f32 = 8.0;
        let card = self.hits.get("tour")?;
        let bottom = ["dock", "dock-brushes", "dock-more", "gestures", "stats", "hint"]
            .iter()
            .filter_map(|n| self.hits.get(*n))
            .fold(free.bottom(), |t, r| t.min(r.top()));
        let left = if self.touch { free.left() } else { free.left() + 12.0 + self.rail_w };
        let view = Rect::from_min_max(pos2(left + GAP, TOP), pos2(free.right() - GAP, bottom - GAP));
        let area = |r: &Rect| r.width().max(0.0) * r.height().max(0.0);
        let largest = |rooms: [Rect; 4]| rooms.into_iter().max_by(|a, b| area(a).total_cmp(&area(b)));
        let (l, r, t, b) = (view.left(), view.right(), view.top(), view.bottom());
        let mut room = largest([
            Rect::from_min_max(pos2(l, t), pos2(r, card.top() - GAP)),
            Rect::from_min_max(pos2(l, card.bottom() + GAP), pos2(r, b)),
            Rect::from_min_max(pos2(l, t), pos2(card.left() - GAP, b)),
            Rect::from_min_max(pos2(card.right() + GAP, t), pos2(r, b)),
        ])?;
        // A panel standing in the room cuts it on the side that keeps the most of it.
        for o in [self.hits.get("tree").copied(), self.render_dock, self.nav_rect].into_iter().flatten() {
            if room.intersects(o) {
                let (l, r, t, b) = (room.left(), room.right(), room.top(), room.bottom());
                room = largest([
                    Rect::from_min_max(pos2(l, t), pos2(r, o.top() - GAP)),
                    Rect::from_min_max(pos2(l, o.bottom() + GAP), pos2(r, b)),
                    Rect::from_min_max(pos2(l, t), pos2(o.left() - GAP, b)),
                    Rect::from_min_max(pos2(o.right() + GAP, t), pos2(r, b)),
                ])?;
            }
        }
        (room.width() >= 40.0 && room.height() >= 40.0).then_some(room)
    }

    /// The user did `step`'s action: the step is done if it is the one on screen.
    pub fn tour_did(&mut self, step: TourStep) {
        if self.tour.current() == Some(step) {
            self.tour.done = true;
        }
    }

    /// Next, or a done step's moment passed: the next step, or the end after the last.
    fn next_step(&mut self) {
        let Some(i) = self.tour.step else { return };
        if i + 1 >= STEPS.len() {
            self.end_tour(true);
        } else {
            self.tour.step = Some(i + 1);
            self.tour.done = false;
            self.tour.done_at = None;
        }
    }

    /// Where the card goes: at the right of `view` under the top bar, left of the render corner
    /// and camera bar if they stand there; at the top left on a tablet, right of the scene tree;
    /// on a phone at the bottom of the room the dock leaves.
    fn tour_rect(&self, full: Rect, view: Rect, h: f32) -> Rect {
        if self.phone {
            let region = self.panel_region(full);
            let w = region.width().min(TOUR_W);
            return Rect::from_min_size(pos2(region.right() - w, (region.bottom() - h).max(region.top())), vec2(w, h));
        }
        // Clear of the rail, and of the scene tree when the scene has one.
        let left = self
            .hits
            .get("tree")
            .map_or(view.left() + 12.0 + if self.touch { 0.0 } else { self.rail_w + 8.0 }, |t| t.right() + 8.0);
        // A tablet has the render corner and camera bar at the right and no rail at the left:
        // the card takes the free top left, clear of the model in the middle.
        let span = |right: f32| {
            let w = TOUR_W.min(right - left).max(TOUR_MIN_W.min(view.width() - 24.0));
            let x = if self.touch { left } else { right - w };
            Rect::from_min_size(pos2(x, TOP), vec2(w, h))
        };
        let mut right = view.right() - 12.0;
        // The render corner and camera bar stand at the right: the card keeps left of them,
        // unless that leaves it too narrow to read.
        for r in [self.render_dock, self.nav_rect].into_iter().flatten() {
            if span(right).intersects(r.expand(8.0)) && r.left() - 8.0 - left >= TOUR_MIN_W {
                right = right.min(r.left() - 8.0);
            }
        }
        span(right)
    }

    /// The card's image height for a card `w` wide, and the height of the card around it.
    fn tour_layout(&self, ctx: &Context, step: TourStep, w: f32, room: f32) -> (f32, Arc<egui::Galley>, f32) {
        let body = ctx.fonts_mut(|f| f.layout_job(body_job(step.body(self.touch), w - 2.0 * PAD)));
        // The step and dots, the title, the body and the buttons' row.
        let rest = PAD + 18.0 + 10.0 + 20.0 + 4.0 + body.size().y + 14.0 + 32.0 + PAD;
        let image_h = match step.demo() {
            Some(_) => {
                let h = ((w - 2.0 * PAD) / IMAGE_ASPECT).round().min(room - rest - 12.0);
                // Too short for the demo to read (a phone held sideways): the words alone.
                if h < 72.0 { 0.0 } else { h }
            }
            None => 0.0,
        };
        let h = rest + if image_h > 0.0 { image_h + 12.0 } else { 0.0 };
        (image_h, body, h)
    }

    pub(super) fn tour_card(&mut self, ctx: &Context, full: Rect, view: Rect) {
        self.tour.shown = None;
        let Some(i) = self.tour.step else { return };
        // The scene's mode is known now: the tour edits a model.
        if std::mem::take(&mut self.tour.to_model) && self.world {
            self.actions.push(Action::ToggleWorld);
        }
        // Help covers the right of the view, a sheet the view itself, and on a phone a panel the
        // user opened the card's room: the tour waits.
        if self.help.open || self.sheet() || self.tour_yields() {
            return;
        }
        let step = STEPS[i];
        let now = ctx.input(|i| i.time);
        if self.tour.done {
            let at = *self.tour.done_at.get_or_insert(now);
            if now - at >= NEXT_AFTER {
                self.next_step();
                return self.tour_card(ctx, full, view);
            }
            ctx.request_repaint_after(std::time::Duration::from_secs_f64(NEXT_AFTER - (now - at) + 0.01));
        }
        let room = if self.phone { self.panel_region(full).height() } else { view.bottom() - 12.0 - TOP };
        let w = if self.phone { self.panel_region(full).width().min(TOUR_W) } else { TOUR_W };
        let first = self.tour_layout(ctx, step, w, room);
        let rect = self.tour_rect(full, view, first.2);
        // The body wraps again only when the card came out narrower.
        let (image_h, body, h) =
            if rect.width() == w { first } else { self.tour_layout(ctx, step, rect.width(), room) };
        let rect = Rect::from_min_size(rect.min, vec2(rect.width(), h));
        self.tour.image = vec2(rect.width() - 2.0 * PAD, image_h);
        self.hits.insert("tour".into(), rect);
        if let Some(ring) = step.ring().and_then(|n| self.hits.get(n).copied()) {
            draw_ring(ctx, ring, now);
            ctx.request_repaint_after(TILE_FRAME);
        }
        let demo = step.demo().filter(|_| image_h > 0.0);
        if demo.is_some() {
            // The demo moves: draw again soon, and only while it shows.
            ctx.request_repaint_after(TILE_FRAME);
        }
        let tile = self.tour.tile;
        let (touch, done, last) = (self.touch, self.tour.done, i + 1 == STEPS.len());
        let mut hits: Vec<(String, Rect)> = Vec::new();
        let (mut skip, mut next) = (false, false);
        let order = if self.phone { Order::Foreground } else { Order::Middle };
        Self::panel_filled(ctx, "tour", rect, order, th().panel_solid, |ui, rect| {
            ui.allocate_rect(rect, Sense::click());
            let p = ui.painter().clone();
            let left = rect.left() + PAD;
            // "Step 2 of 5" and a dot for each step, the ones reached filled.
            let hy = rect.top() + PAD + 9.0;
            let g = p.layout_no_wrap(format!("Step {} of {}", i + 1, STEPS.len()), sans_medium(12.0), th().muted);
            p.galley(pos2(left, hy - g.size().y / 2.0), g, th().muted);
            for k in 0..STEPS.len() {
                let c = pos2(rect.right() - PAD - 3.0 - (STEPS.len() - 1 - k) as f32 * 12.0, hy);
                if k <= i {
                    p.circle_filled(c, 3.0, th().text);
                } else {
                    p.circle_stroke(c, 2.5, Stroke::new(1.0, th().faint));
                }
            }
            let mut y = rect.top() + PAD + 18.0 + 10.0;
            if demo.is_some() {
                let image = Rect::from_min_size(pos2(left, y), vec2(rect.width() - 2.0 * PAD, image_h));
                p.rect(
                    image.expand(1.0),
                    CornerRadius::same(10),
                    th().tint(5),
                    Stroke::new(1.0, th().line),
                    StrokeKind::Inside,
                );
                draw_image(ui, image, CornerRadius::same(9), &tile, touch);
                hits.push(("tour-demo".into(), image));
                y = image.bottom() + 12.0;
            }
            let g = p.layout_no_wrap(step.title().into(), sans_semibold(15.0), th().text);
            p.galley(pos2(left, y + 10.0 - g.size().y / 2.0), g, th().text);
            y += 20.0 + 4.0;
            p.galley(pos2(left, y), body.clone(), th().muted);
            // The buttons' row: what the step waits for at the left, Skip and Next at the right.
            let by = rect.bottom() - PAD - 32.0;
            let mut right = rect.right() - PAD;
            if done || last {
                let label = if last { "Open Help" } else { "Next" };
                let g = p.layout_no_wrap(label.into(), sans_semibold(13.0), th().on_accent);
                let b = Rect::from_min_size(pos2(right - g.size().x - 28.0, by), vec2(g.size().x + 28.0, 32.0));
                let resp = ui.interact(b, Id::unique(("tour-next", last)), Sense::click());
                let plate = pressed(b, &resp);
                p.rect_filled(
                    plate,
                    CornerRadius::same(8),
                    if resp.hovered() { th().accent_hover } else { th().accent },
                );
                p.galley(plate.center() - g.size() / 2.0, g, th().on_accent);
                hits.push(((if last { "tour-help" } else { "tour-next" }).into(), b));
                next |= resp.clicked();
                right = b.left() - 6.0;
            }
            let label = if last { "Done" } else { "Skip tour" };
            let g = p.layout_no_wrap(label.into(), sans(13.0), th().muted);
            let b = Rect::from_min_size(pos2(right - g.size().x - 28.0, by), vec2(g.size().x + 28.0, 32.0));
            let resp = ui.interact(b, Id::unique("tour-skip"), Sense::click());
            if resp.hovered() {
                p.rect_filled(b, CornerRadius::same(8), th().hover);
            }
            let ink = if resp.hovered() { th().text } else { th().muted };
            p.galley(b.center() - g.size() / 2.0, g, ink);
            hits.push(("tour-skip".into(), b));
            skip |= resp.clicked();
            if !last {
                let cy = b.center().y;
                if done {
                    p.text(pos2(left + 7.0, cy), Align2::CENTER_CENTER, icon::CHECK, icon_font(15.0), th().text);
                    p.text(pos2(left + 21.0, cy), Align2::LEFT_CENTER, "Done", sans_medium(12.5), th().text);
                } else {
                    // A slow pulse says the card is waiting on the user.
                    let a = 0.45 + 0.35 * (now * 3.0).sin().abs() as f32;
                    p.circle_filled(pos2(left + 4.0, cy), 3.5, th().faint.gamma_multiply(a / 0.8));
                    let g = p.layout_no_wrap("Waiting for you to try it".into(), sans(12.5), th().faint);
                    p.galley(pos2(left + 14.0, cy - g.size().y / 2.0), g, th().faint);
                }
            }
        });
        self.hits.extend(hits);
        self.tour.shown = demo;
        if next {
            if last {
                // Opening Help finishes the tour.
                self.open_help();
            } else {
                self.next_step();
            }
        } else if skip {
            self.end_tour(last);
        }
    }

    /// The tour's own end on its last step: Help opened (its button, F1 or Cmd-K) finishes it.
    pub(super) fn tour_help_opened(&mut self) {
        if self.tour.current() == Some(TourStep::Help) {
            self.end_tour(true);
        }
    }
}

/// The step's words, its `*bold*` parts in the text colour.
fn body_job(text: &str, width: f32) -> LayoutJob {
    let mut job = LayoutJob::default();
    for (k, part) in text.split('*').enumerate() {
        let bold = k % 2 == 1;
        job.append(
            part,
            0.0,
            TextFormat {
                font_id: if bold { sans_medium(13.0) } else { sans(13.0) },
                color: if bold { th().text } else { th().muted },
                line_height: Some(19.0),
                ..Default::default()
            },
        );
    }
    job.wrap.max_width = width;
    job
}

/// A ring that pulses around the control a step needs, over everything but dialogs.
fn draw_ring(ctx: &Context, at: Rect, now: f64) {
    let pulse = (now * 2.4).sin() as f32 * 0.5 + 0.5;
    let painter = ctx.layer_painter(egui::LayerId::new(Order::Foreground, Id::unique("tour-ring")));
    let r = at.expand(3.0 + 2.0 * pulse);
    painter.rect_stroke(
        r,
        CornerRadius::same(10),
        Stroke::new(2.0, th().ring.gamma_multiply(0.6 + 0.4 * pulse)),
        StrokeKind::Outside,
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::demo::DEMOS;

    #[test]
    fn every_step_but_the_last_plays_a_help_demo() {
        for s in STEPS {
            assert_eq!(s.demo().is_some(), s != TourStep::Help, "{s:?}");
            if let Some(d) = s.demo() {
                assert_eq!(DEMOS[d].id, s.id());
            }
        }
    }

    #[test]
    fn touch_words_name_no_mouse_or_key() {
        for s in STEPS {
            let t = s.body(true).to_lowercase();
            for w in ["right-drag", "click", "scroll", "press "] {
                assert!(!t.contains(w), "{s:?} on touch says {w}: {t}");
            }
        }
    }
}
