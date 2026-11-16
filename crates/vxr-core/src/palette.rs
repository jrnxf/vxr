//! Palette colour maths: HSV, and the group edit that shifts the hue, saturation and value
//! of several slots at once (MagicaVoxel's palette multi-select).

use crate::Scene;

/// Hue in degrees (0 to 360), saturation and value from 0 to 1.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Hsv {
    pub h: f32,
    pub s: f32,
    pub v: f32,
}

impl Hsv {
    pub fn from_rgb(c: [u8; 3]) -> Hsv {
        let [r, g, b] = c.map(|x| x as f32 / 255.0);
        let max = r.max(g).max(b);
        let d = max - r.min(g).min(b);
        let h = if d == 0.0 {
            0.0
        } else if max == r {
            60.0 * ((g - b) / d).rem_euclid(6.0)
        } else if max == g {
            60.0 * ((b - r) / d + 2.0)
        } else {
            60.0 * ((r - g) / d + 4.0)
        };
        Hsv { h, s: if max == 0.0 { 0.0 } else { d / max }, v: max }
    }

    pub fn to_rgb(self) -> [u8; 3] {
        let h = self.h.rem_euclid(360.0) / 60.0;
        let (s, v) = (self.s.clamp(0.0, 1.0), self.v.clamp(0.0, 1.0));
        let c = v * s;
        let x = c * (1.0 - (h % 2.0 - 1.0).abs());
        let (r, g, b) = match h as u32 {
            0 => (c, x, 0.0),
            1 => (x, c, 0.0),
            2 => (0.0, c, x),
            3 => (0.0, x, c),
            4 => (x, 0.0, c),
            _ => (c, 0.0, x),
        };
        let m = v - c;
        [r, g, b].map(|k| ((k + m) * 255.0).round() as u8)
    }
}

/// A group edit of colours: `hue` turns the hue by that many degrees, `sat` and `val` scale
/// the saturation and the value by `1 + x` (-1 removes it, 1 doubles it). Scaling keeps a
/// grey grey and black black, and keeps the order of the colours in a ramp.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Shift {
    pub hue: f32,
    pub sat: f32,
    pub val: f32,
}

impl Shift {
    pub fn is_zero(self) -> bool {
        self == Shift::default()
    }

    /// The colour after the shift. Alpha is kept, and no shift gives back the same bytes.
    pub fn apply(self, c: [u8; 4]) -> [u8; 4] {
        if self.is_zero() {
            return c;
        }
        let hsv = Hsv::from_rgb([c[0], c[1], c[2]]);
        let [r, g, b] = Hsv {
            h: hsv.h + self.hue,
            s: (hsv.s * (1.0 + self.sat)).clamp(0.0, 1.0),
            v: (hsv.v * (1.0 + self.val)).clamp(0.0, 1.0),
        }
        .to_rgb();
        [r, g, b, c[3]]
    }
}

/// The palette slots a set of voxels uses, in slot order.
pub fn slots_of(voxels: impl IntoIterator<Item = u8>) -> Vec<u8> {
    let mut seen = [false; 256];
    for v in voxels {
        seen[v as usize] = true;
    }
    (1..=255u8).filter(|&s| seen[s as usize]).collect()
}

impl Scene {
    /// Shift each slot of `base` (a slot and its colour before the group edit) inside the
    /// open edit, opening one if needed. Always from the colours in `base`, so a slider
    /// dragged through many values does not drift, and the whole drag is one undo step at
    /// [`Scene::commit`].
    pub fn shift_colors(&mut self, base: &[(u8, [u8; 4])], shift: Shift) {
        for &(slot, rgba) in base {
            if slot != 0 {
                self.put_color(slot, shift.apply(rgba));
            }
        }
    }

    /// The slots the selected voxels use, or the slots the whole model uses while nothing
    /// is selected, in slot order.
    pub fn colors_used(&self) -> Vec<u8> {
        if self.selected_count() > 0 {
            slots_of(self.selection().map(|p| self.map.get(p[0], p[1], p[2])))
        } else {
            (1..=255u8).filter(|&s| self.map.counts[s as usize] > 0).collect()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::select::Mode;
    use proptest::prelude::*;

    #[test]
    fn hsv_round_trips_every_grey_and_a_spread_of_colours() {
        for r in (0..=255u32).step_by(15) {
            for g in (0..=255u32).step_by(15) {
                for b in (0..=255u32).step_by(15) {
                    let c = [r as u8, g as u8, b as u8];
                    assert_eq!(Hsv::from_rgb(c).to_rgb(), c, "{c:?}");
                }
            }
        }
    }

    #[test]
    fn known_hues() {
        assert_eq!(Hsv::from_rgb([255, 0, 0]), Hsv { h: 0.0, s: 1.0, v: 1.0 });
        assert_eq!(Hsv::from_rgb([0, 255, 0]).h, 120.0);
        assert_eq!(Hsv::from_rgb([0, 0, 255]).h, 240.0);
        assert_eq!(Hsv { h: 300.0, s: 1.0, v: 1.0 }.to_rgb(), [255, 0, 255]);
        assert_eq!(Hsv { h: 360.0, s: 1.0, v: 1.0 }.to_rgb(), [255, 0, 0]);
    }

    fn near(a: [u8; 4], want: [f32; 3]) -> bool {
        (0..3).all(|k| (a[k] as f32 - want[k]).abs() <= 1.0)
    }

    proptest! {
        // The sliders stated without HSV: a third of a turn moves each channel to the next,
        // saturation moves each channel away from or toward the largest, value scales all.
        #[test]
        fn a_shift_does_what_its_slider_says(c in any::<[u8; 4]>(), k in -1.0f32..=1.0) {
            let [r, g, b, a] = c;
            prop_assert_eq!(Shift { hue: 120.0, ..Shift::default() }.apply(c), [b, r, g, a]);
            prop_assert_eq!(Shift { hue: -120.0, ..Shift::default() }.apply(c), [g, b, r, a]);
            let max = r.max(g).max(b) as f32;
            let min = r.min(g).min(b) as f32;
            let grow = if max > min { (1.0 + k).min(max / (max - min)) } else { 1.0 };
            let got = Shift { sat: k, ..Shift::default() }.apply(c);
            prop_assert!(near(got, [r, g, b].map(|x| max - (max - x as f32) * grow)), "sat {k}: {c:?} -> {got:?}");
            let grow = if max > 0.0 { (1.0 + k).min(255.0 / max) } else { 1.0 };
            let got = Shift { val: k, ..Shift::default() }.apply(c);
            prop_assert!(near(got, [r, g, b].map(|x| x as f32 * grow)), "val {k}: {c:?} -> {got:?}");
            prop_assert_eq!(got[3], a);
            prop_assert_eq!(Shift::default().apply(c), c);
        }

        // A drag through many values lands where one move to the last value would, is one
        // undo step, and touches only the slots given.
        #[test]
        fn a_group_edit_is_one_step_from_the_base_colours(
            slots in proptest::collection::btree_set(1u8..=255, 1..6),
            drag in proptest::collection::vec((-180.0f32..=180.0, -1.0f32..=1.0, -1.0f32..=1.0), 1..5),
        ) {
            let mut scene = Scene::new([8; 3]);
            let before = scene.palette;
            let base: Vec<_> = slots.iter().map(|&s| (s, before[s as usize])).collect();
            scene.commit();
            for &(hue, sat, val) in &drag {
                scene.shift_colors(&base, Shift { hue, sat, val });
            }
            scene.commit();
            let (hue, sat, val) = *drag.last().unwrap();
            let last = Shift { hue, sat, val };
            for s in 0..=255u8 {
                let want = if slots.contains(&s) { last.apply(before[s as usize]) } else { before[s as usize] };
                prop_assert_eq!(scene.palette[s as usize], want, "slot {}", s);
            }
            scene.undo();
            prop_assert_eq!(scene.palette, before);
        }
    }

    #[test]
    fn colors_used_follows_the_selection() {
        let mut scene = Scene::new([8; 3]);
        scene.edit([([0, 0, 0], 7), ([1, 0, 0], 3), ([2, 0, 0], 7), ([3, 0, 0], 200)]);
        assert_eq!(scene.colors_used(), [3, 7, 200]);
        scene.select([[0, 0, 0], [2, 0, 0], [3, 0, 0], [9, 9, 9]], Mode::Replace);
        assert_eq!(scene.colors_used(), [7, 200]);
        scene.select([[1, 0, 0]], Mode::Replace);
        assert_eq!(scene.colors_used(), [3]);
        scene.deselect();
        assert_eq!(scene.colors_used(), [3, 7, 200]);
    }
}
