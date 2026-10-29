//! Touch gestures on the view: one finger uses the tool, two fingers orbit or pinch to zoom,
//! three fingers pan, and a quick two-finger tap undoes. Positions are in the view's pixels;
//! no window or GPU, so the editor and its tests feed it straight.

/// What a change of fingers asks the editor to do.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Gesture {
    /// The tool's press, drag and release, as the left mouse button.
    Press([f32; 2]),
    Drag([f32; 2]),
    Release([f32; 2]),
    /// Two fingers moved together: orbit by this many pixels.
    Orbit([f32; 2]),
    /// Three fingers moved: pan by this many pixels.
    Pan([f32; 2]),
    /// Two fingers spread (`scale` above 1) or closed, around `at`.
    Pinch {
        scale: f32,
        at: [f32; 2],
    },
    Undo,
}

/// A still finger starts the tool after this long, so a second finger landing a moment later
/// turns the touch into a camera gesture without leaving a voxel behind.
pub const HOLD_MS: f64 = 150.0;
/// Two fingers down and up within this long, without moving, undo.
pub const TAP_MS: f64 = 300.0;

#[derive(Clone, Copy, Debug, PartialEq)]
enum Lock {
    /// Not past the slop yet: a tap or the start of a gesture.
    None,
    Orbit,
    Pinch,
    Pan,
}

#[derive(Clone, Copy, Debug, PartialEq)]
enum State {
    Idle,
    /// One finger down, not yet the tool: a second finger may still come.
    Pending {
        start: [f32; 2],
        at: f64,
    },
    Tool,
    /// Two or more fingers move the camera. `since` is when the count last changed, `from`
    /// the fingers' centre and spread then, and `travel` how far they have moved and spread
    /// since.
    Camera {
        since: f64,
        from: ([f32; 2], f32),
        travel: f32,
        lock: Lock,
        tap: bool,
    },
    /// A finger lifted from a camera gesture: nothing more until every finger is up.
    Done,
}

/// The fingers on the view and what they are doing.
#[derive(Clone, Debug)]
pub struct Touches {
    /// Movement below this many pixels is a finger resting, not a drag.
    slop: f32,
    fingers: Vec<(u64, [f32; 2])>,
    state: State,
}

fn dist(a: [f32; 2], b: [f32; 2]) -> f32 {
    (a[0] - b[0]).hypot(a[1] - b[1])
}

impl Touches {
    pub fn new(slop: f32) -> Self {
        Touches { slop, fingers: Vec::new(), state: State::Idle }
    }

    /// A new slop, for a new scale, keeping the fingers down and what they are doing.
    pub fn set_slop(&mut self, slop: f32) {
        self.slop = slop;
    }

    /// Fingers down now.
    pub fn count(&self) -> usize {
        self.fingers.len()
    }

    /// The fingers' centre and their mean distance from it.
    fn shape(&self) -> ([f32; 2], f32) {
        let n = self.fingers.len().max(1) as f32;
        let c = self.fingers.iter().fold([0.0, 0.0], |c, (_, p)| [c[0] + p[0] / n, c[1] + p[1] / n]);
        (c, self.fingers.iter().map(|(_, p)| dist(*p, c)).sum::<f32>() / n)
    }

    fn camera(&mut self, t: f64, tap: bool) {
        self.state = State::Camera { since: t, from: self.shape(), travel: 0.0, lock: Lock::None, tap };
    }

    /// Finger `id` touched down at `p`, `t` milliseconds into some clock.
    pub fn start(&mut self, id: u64, p: [f32; 2], t: f64) -> Vec<Gesture> {
        let mut out = Vec::new();
        self.fingers.retain(|f| f.0 != id);
        self.fingers.push((id, p));
        match (self.state, self.fingers.len()) {
            (State::Idle, 1) => self.state = State::Pending { start: p, at: t },
            (State::Pending { .. }, _) => self.camera(t, true),
            (State::Tool, _) => {
                // The stroke so far stays; the fingers move the camera from here.
                let last = self.fingers[0].1;
                out.push(Gesture::Release(last));
                self.camera(t, false);
            }
            // A third finger turns orbit or pinch into pan, and is never a tap.
            (State::Camera { .. }, _) => self.camera(t, false),
            _ => {}
        }
        out
    }

    /// Finger `id` moved to `p`.
    pub fn moved(&mut self, id: u64, p: [f32; 2], t: f64) -> Vec<Gesture> {
        let mut out = Vec::new();
        let Some(i) = self.fingers.iter().position(|f| f.0 == id) else { return out };
        let (c0, s0) = self.shape();
        self.fingers[i].1 = p;
        let (c1, s1) = self.shape();
        match &mut self.state {
            State::Pending { start, at } => {
                if dist(p, *start) > self.slop || t - *at >= HOLD_MS {
                    out.extend([Gesture::Press(*start), Gesture::Drag(p)]);
                    self.state = State::Tool;
                }
            }
            State::Tool => out.push(Gesture::Drag(p)),
            State::Camera { from, travel, lock, .. } => {
                let d = [c1[0] - c0[0], c1[1] - c0[1]];
                *travel += d[0].hypot(d[1]) + (s1 - s0).abs();
                if *lock == Lock::None && *travel > self.slop {
                    // Pinch or orbit, whichever the fingers have done more of, until the count
                    // changes. One finger moving away from a still one moves the centre and
                    // the spread alike, so spreading wins at half the centre's move.
                    let (c, s) = *from;
                    *lock = if self.fingers.len() >= 3 {
                        Lock::Pan
                    } else if (s1 - s).abs() > 0.5 * dist(c1, c) {
                        Lock::Pinch
                    } else {
                        Lock::Orbit
                    };
                }
                match *lock {
                    Lock::None => {}
                    Lock::Orbit => out.push(Gesture::Orbit(d)),
                    Lock::Pan => out.push(Gesture::Pan(d)),
                    Lock::Pinch if s0 > 1.0 => out.push(Gesture::Pinch { scale: s1 / s0, at: c1 }),
                    Lock::Pinch => {}
                }
            }
            State::Idle | State::Done => {}
        }
        out
    }

    /// Finger `id` lifted (or the system took it: a cancel ends the same way).
    pub fn end(&mut self, id: u64, p: [f32; 2], t: f64) -> Vec<Gesture> {
        let mut out = Vec::new();
        let Some(i) = self.fingers.iter().position(|f| f.0 == id) else { return out };
        let was = self.fingers.len();
        self.fingers.remove(i);
        match self.state {
            State::Pending { start, .. } => out.extend([Gesture::Press(start), Gesture::Release(p)]),
            State::Tool => out.push(Gesture::Release(p)),
            State::Camera { since, travel, tap, .. } => {
                if tap && was == 2 && travel <= self.slop && t - since <= TAP_MS {
                    out.push(Gesture::Undo);
                }
            }
            State::Idle | State::Done => {}
        }
        self.state = if self.fingers.is_empty() { State::Idle } else { State::Done };
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use Gesture::*;

    #[test]
    fn a_tap_is_a_click_where_the_finger_landed() {
        let mut t = Touches::new(10.0);
        assert!(t.start(1, [100.0, 100.0], 0.0).is_empty());
        assert_eq!(t.end(1, [102.0, 101.0], 80.0), [Press([100.0, 100.0]), Release([102.0, 101.0])]);
        assert_eq!(t.count(), 0);
    }

    #[test]
    fn one_finger_drags_the_tool_once_it_moves_or_holds() {
        let mut t = Touches::new(10.0);
        t.start(1, [0.0, 0.0], 0.0);
        assert!(t.moved(1, [4.0, 0.0], 20.0).is_empty(), "inside the slop and before the hold");
        assert_eq!(t.moved(1, [20.0, 0.0], 40.0), [Press([0.0, 0.0]), Drag([20.0, 0.0])]);
        assert_eq!(t.moved(1, [30.0, 5.0], 60.0), [Drag([30.0, 5.0])]);
        assert_eq!(t.end(1, [30.0, 5.0], 80.0), [Release([30.0, 5.0])]);

        // A still finger starts the tool after HOLD_MS.
        t.start(2, [0.0, 0.0], 1000.0);
        assert_eq!(t.moved(2, [1.0, 0.0], 1000.0 + HOLD_MS), [Press([0.0, 0.0]), Drag([1.0, 0.0])]);
    }

    #[test]
    fn a_second_finger_before_the_tool_starts_leaves_no_edit() {
        let mut t = Touches::new(10.0);
        t.start(1, [100.0, 100.0], 0.0);
        assert!(t.start(2, [200.0, 100.0], 60.0).is_empty());
        // Both move right together: orbit, never a press.
        let mut g = Vec::new();
        for i in 1..=15 {
            let x = 2.0 * i as f32;
            g.extend(t.moved(1, [100.0 + x, 100.0], 60.0 + i as f64));
            g.extend(t.moved(2, [200.0 + x, 100.0], 60.0 + i as f64));
        }
        assert!(g.iter().all(|g| matches!(g, Orbit(_))), "{g:?}");
        let dx: f32 = g.iter().map(|g| if let Orbit(d) = g { d[0] } else { 0.0 }).sum();
        assert!(dx > 20.0 && dx <= 30.0, "{dx}");
        assert!(t.end(1, [130.0, 100.0], 200.0).is_empty());
        // The other finger left down does nothing until it lifts too.
        assert!(t.moved(2, [300.0, 100.0], 220.0).is_empty());
        assert!(t.end(2, [300.0, 100.0], 240.0).is_empty());
    }

    #[test]
    fn spreading_two_fingers_pinches_around_their_middle() {
        let mut t = Touches::new(10.0);
        t.start(1, [100.0, 100.0], 0.0);
        t.start(2, [200.0, 100.0], 10.0);
        let mut g = Vec::new();
        for i in 1..=10 {
            let x = 2.0 * i as f32;
            g.extend(t.moved(1, [100.0 - x, 100.0], 20.0 + i as f64));
            g.extend(t.moved(2, [200.0 + x, 100.0], 20.0 + i as f64));
        }
        let scale: f32 = g
            .iter()
            .map(|g| match g {
                Pinch { scale, at } => {
                    assert!((at[0] - 150.0).abs() <= 1.0 && at[1] == 100.0, "{at:?}");
                    *scale
                }
                other => panic!("{other:?}"),
            })
            .product();
        assert!(scale > 1.2 && scale < 1.41, "{scale}");
    }

    #[test]
    fn three_fingers_pan() {
        let mut t = Touches::new(10.0);
        for (i, x) in [100.0, 150.0, 200.0].into_iter().enumerate() {
            t.start(i as u64, [x, 100.0], 0.0);
        }
        let mut g = Vec::new();
        for (i, x) in [100.0, 150.0, 200.0].into_iter().enumerate() {
            g.extend(t.moved(i as u64, [x, 160.0], 50.0));
        }
        assert!(!g.is_empty() && g.iter().all(|g| matches!(g, Pan(_))), "{g:?}");
        let dy: f32 = g.iter().map(|g| if let Pan(d) = g { d[1] } else { 0.0 }).sum();
        assert!(dy > 40.0 && dy <= 60.0, "{dy}");
    }

    #[test]
    fn a_quick_two_finger_tap_undoes_once() {
        let mut t = Touches::new(10.0);
        t.start(1, [100.0, 100.0], 0.0);
        t.start(2, [160.0, 100.0], 20.0);
        assert_eq!(t.end(1, [100.0, 100.0], 120.0), [Undo]);
        assert!(t.end(2, [160.0, 100.0], 130.0).is_empty());

        // Too slow, moved, or three fingers: no undo.
        t.start(1, [100.0, 100.0], 1000.0);
        t.start(2, [160.0, 100.0], 1010.0);
        assert!(t.end(1, [100.0, 100.0], 1010.0 + TAP_MS + 1.0).is_empty());
        t.end(2, [160.0, 100.0], 1400.0);
        t.start(1, [100.0, 100.0], 2000.0);
        t.start(2, [160.0, 100.0], 2010.0);
        t.moved(2, [200.0, 100.0], 2050.0);
        assert!(t.end(1, [100.0, 100.0], 2080.0).is_empty());
        t.end(2, [200.0, 100.0], 2090.0);
        t.start(1, [100.0, 100.0], 3000.0);
        t.start(2, [160.0, 100.0], 3010.0);
        t.start(3, [220.0, 100.0], 3020.0);
        assert!(t.end(1, [100.0, 100.0], 3080.0).is_empty());
    }

    #[test]
    fn a_second_finger_mid_stroke_ends_the_stroke_and_moves_the_camera() {
        let mut t = Touches::new(10.0);
        t.start(1, [0.0, 0.0], 0.0);
        t.moved(1, [40.0, 0.0], 30.0);
        assert_eq!(t.start(2, [100.0, 0.0], 60.0), [Release([40.0, 0.0])]);
        // Not a tap, even if they lift at once.
        assert!(t.end(2, [100.0, 0.0], 90.0).is_empty());
        assert!(t.end(1, [40.0, 0.0], 95.0).is_empty());
    }
}
