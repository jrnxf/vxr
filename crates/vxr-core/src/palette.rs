//! Palette colour maths: HSV, the group edit that shifts the hue, saturation and value of
//! several slots at once (MagicaVoxel's palette multi-select), sorting the palette, and the
//! palette as a PNG strip and as text.

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

/// What Sort palette orders the colours by.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SortKey {
    /// Greys first, from dark to light, then the colours around the wheel from red.
    Hue,
    /// From grey to vivid.
    Saturation,
    /// From dark to light.
    Value,
}

impl SortKey {
    pub const ALL: [SortKey; 3] = [SortKey::Hue, SortKey::Saturation, SortKey::Value];

    /// The name agents and file names use.
    pub fn name(self) -> &'static str {
        match self {
            SortKey::Hue => "hue",
            SortKey::Saturation => "saturation",
            SortKey::Value => "value",
        }
    }

    pub fn from_name(name: &str) -> Option<SortKey> {
        SortKey::ALL.into_iter().find(|k| k.name() == name)
    }

    /// The order of one colour: the key first, then the other two, so equal keys still
    /// come out in an order the eye follows.
    fn of(self, c: [u8; 4]) -> [f32; 4] {
        let Hsv { h, s, v } = Hsv::from_rgb([c[0], c[1], c[2]]);
        match self {
            SortKey::Hue => [(s > 0.0) as u8 as f32, h, v, s],
            SortKey::Saturation => [s, h, v, 0.0],
            SortKey::Value => [v, h, s, 0.0],
        }
    }
}

/// Where each slot goes when the colours of `slots` are sorted by `key` among those slots:
/// `table[s]` is the slot that takes slot `s`'s colour. Slots not in `slots`, and slot 0,
/// stay. Equal colours keep their order, so sorting twice moves nothing.
pub fn sort_table(palette: &[[u8; 4]; 256], slots: &[u8], key: SortKey) -> [u8; 256] {
    let mut places: Vec<u8> = slots.iter().copied().filter(|&s| s != 0).collect();
    places.sort_unstable();
    places.dedup();
    let mut order = places.clone();
    order.sort_by(|&a, &b| {
        let (ka, kb) = (key.of(palette[a as usize]), key.of(palette[b as usize]));
        ka.iter().zip(&kb).map(|(x, y)| x.total_cmp(y)).find(|o| o.is_ne()).unwrap_or(std::cmp::Ordering::Equal)
    });
    let mut table: [u8; 256] = std::array::from_fn(|s| s as u8);
    for (&from, &to) in order.iter().zip(&places) {
        table[from as usize] = to;
    }
    table
}

/// The palette as a strip of 256 RGBA pixels, the layout of a `.vox` palette and of
/// MagicaVoxel's palette PNG: pixel `x` is slot `x + 1`, and the last pixel is slot 0.
pub fn strip(palette: &[[u8; 4]; 256]) -> Vec<u8> {
    (0..256).flat_map(|x| palette[(x + 1) % 256]).collect()
}

/// The palette as text, one `rrggbb` line per slot from 1 to 255 (the `.hex` format of
/// Lospec and most pixel art tools). Alpha is not written.
pub fn hex(palette: &[[u8; 4]; 256]) -> String {
    palette[1..].iter().map(|c| format!("{:02x}{:02x}{:02x}\n", c[0], c[1], c[2])).collect()
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

    /// Give slot `table[s]` the colour and material of slot `s`, for every slot, inside the
    /// open edit (opening one if needed). `table` is a permutation of the slots.
    pub fn move_slots(&mut self, table: &[u8; 256]) {
        let (palette, materials) = (self.palette, self.materials);
        for s in 1..=255u8 {
            let to = table[s as usize];
            if to != s && to != 0 {
                self.put_color(to, palette[s as usize]);
                self.put_material(to, materials[s as usize]);
            }
        }
    }

    /// Sort the colours of `slots` by `key` among those slots, as one undo step
    /// ([`sort_table`]). Materials go with their colours and every voxel of the file follows
    /// its colour, so the scene looks the same and only the palette order changes. Returns
    /// where each slot went and how many voxels took another slot.
    pub fn sort_colors(&mut self, slots: &[u8], key: SortKey) -> ([u8; 256], usize) {
        let table = sort_table(&self.palette, slots, key);
        self.commit();
        self.move_slots(&table);
        let n = self.remap_slots(&table);
        self.commit();
        (table, n)
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

    /// Every voxel of the scene by world cell, as the colour and material it shows.
    fn look(s: &Scene) -> std::collections::BTreeMap<[i32; 3], ([u8; 4], crate::material::Material)> {
        s.world_voxels(false).into_iter().map(|(p, v)| (p, (s.palette[v as usize], s.materials[v as usize]))).collect()
    }

    fn metal() -> crate::material::Material {
        crate::material::Material { kind: crate::material::Kind::Metal, ..Default::default() }
    }

    proptest! {
        // The scene looks the same, only the chosen slots trade places, and they end in the
        // order the key names, stated on the channels without HSV.
        #[test]
        fn a_sort_orders_the_chosen_slots_and_keeps_the_look(
            colors in proptest::collection::vec(any::<[u8; 3]>(), 12),
            voxels in proptest::collection::vec((0..4i32, 0..4i32, 0..4i32, 1u8..=12), 1..40),
            chosen in proptest::collection::btree_set(1u8..=12, 0..=12),
            key in 0..3usize,
        ) {
            let key = SortKey::ALL[key];
            let mut scene = Scene::new([8; 3]);
            for (i, c) in colors.iter().enumerate() {
                scene.put_color(i as u8 + 1, [c[0], c[1], c[2], 255]);
            }
            scene.put_material(3, metal());
            scene.edit(voxels.iter().map(|&(x, y, z, v)| ([x, y, z], v)));
            let (palette, materials, seen) = (scene.palette, scene.materials, look(&scene));
            // Nothing chosen sorts the whole palette, as the command does with no slots selected.
            let slots: Vec<u8> = if chosen.is_empty() { (1..=255).collect() } else { chosen.iter().copied().collect() };
            let (table, moved) = scene.sort_colors(&slots, key);
            prop_assert_eq!(look(&scene), seen.clone());
            let uses = |s: u8| voxels.iter().map(|v| ([v.0, v.1, v.2], v.3)).collect::<std::collections::BTreeMap<_, _>>().values().filter(|&&v| v == s).count();
            prop_assert_eq!(moved, (1..=255u8).filter(|&s| table[s as usize] != s).map(uses).sum::<usize>());
            for s in 0..=255u8 {
                let to = table[s as usize];
                prop_assert_eq!((scene.palette[to as usize], scene.materials[to as usize]), (palette[s as usize], materials[s as usize]));
                prop_assert!(to == s || (slots.contains(&s) && slots.contains(&to)), "slot {} went to {}", s, to);
            }
            for pair in slots.windows(2) {
                let [a, b] = [pair[0], pair[1]].map(|s| scene.palette[s as usize]);
                let [(amax, amin), (bmax, bmin)] = [a, b].map(|c| (c[..3].iter().copied().max().unwrap() as u32, c[..3].iter().copied().min().unwrap() as u32));
                match key {
                    SortKey::Value => prop_assert!(amax <= bmax, "{:?} before {:?}", a, b),
                    // (max - min) / max, compared without dividing.
                    SortKey::Saturation => prop_assert!((amax - amin) * bmax.max(1) <= (bmax - bmin) * amax.max(1) + 1, "{:?} before {:?}", a, b),
                    SortKey::Hue => prop_assert!(amax != amin || bmax != bmin || amax <= bmax, "{:?} before {:?}", a, b),
                }
                if key == SortKey::Hue {
                    prop_assert!(amax == amin || bmax != bmin, "a colour before a grey: {:?} {:?}", a, b);
                }
            }
            let sorted = scene.palette;
            let (again, n) = scene.sort_colors(&slots, key);
            prop_assert_eq!((again, n, scene.palette), (std::array::from_fn(|s| s as u8), 0, sorted));
            // A sort that moved nothing left no undo step.
            if table != again {
                scene.undo();
            }
            prop_assert_eq!((scene.palette, scene.materials, look(&scene)), (palette, materials, seen));
        }
    }

    #[test]
    fn a_hue_sort_goes_around_the_wheel_after_the_greys() {
        let mut scene = Scene::new([8; 3]);
        let wheel: [[u8; 3]; 8] = [
            [40, 40, 40],
            [200, 200, 200],
            [255, 0, 0],
            [255, 255, 0],
            [0, 255, 0],
            [0, 255, 255],
            [0, 0, 255],
            [255, 0, 255],
        ];
        // Shuffled into slots 10 to 17, with slot 9 left out of the sort.
        for (i, k) in [5, 2, 7, 0, 3, 6, 1, 4].into_iter().enumerate() {
            let c = wheel[k];
            scene.put_color(10 + i as u8, [c[0], c[1], c[2], 255]);
        }
        scene.put_color(9, [0, 0, 255, 255]);
        scene.sort_colors(&[17, 10, 11, 12, 13, 14, 15, 16], SortKey::Hue);
        let got: Vec<[u8; 3]> = (10..18).map(|s| scene.palette[s]).map(|c| [c[0], c[1], c[2]]).collect();
        assert_eq!(got, wheel);
        assert_eq!(scene.palette[9], [0, 0, 255, 255]);
    }

    /// Sorting in a file of several objects, from the world and from inside one object: every
    /// object keeps its look, the save reads back the same, and undo gives the file back.
    #[test]
    fn a_sort_keeps_every_object_of_a_world() {
        let bytes =
            vxr_vox::fixtures::all().into_iter().find(|f| f.name == "22-world").unwrap().builder.build().write();
        for object in [None, Some("slab"), Some("tree a")] {
            for key in SortKey::ALL {
                let mut s = Scene::from_vox(&bytes).unwrap();
                if let Some(name) = object {
                    let w = s.world().unwrap();
                    let i = (0..w.tree.nodes.len()).find(|&i| w.tree.nodes[i].name.as_deref() == Some(name.as_bytes()));
                    assert!(s.edit_object(i.unwrap()));
                    let first = {
                        let mut c = Vec::new();
                        s.for_each_voxel(|p, _| c.push(p));
                        c[0]
                    };
                    s.select([first], Mode::Replace);
                }
                let (saved, seen, palette) = (s.to_vox().unwrap(), look(&s), s.palette);
                let all: Vec<u8> = (1..=255).collect();
                let (_, moved) = s.sort_colors(&all, key);
                assert!(
                    moved > 0 && s.palette != palette,
                    "{object:?} {key:?}: the fixture's slots are not in {key:?} order"
                );
                assert_eq!(look(&s), seen, "{object:?} {key:?}");
                let after = s.to_vox().unwrap();
                assert_eq!(look(&Scene::from_vox(&after).unwrap()), seen, "{object:?} {key:?}: saved and opened");
                assert!(s.undo());
                assert_eq!(s.to_vox().unwrap(), saved, "{object:?} {key:?}: undo");
                assert_eq!(
                    s.selected_count(),
                    object.is_some() as usize,
                    "{object:?} {key:?}: undo keeps the selection"
                );
                assert!(s.redo());
                assert_eq!(s.to_vox().unwrap(), after, "{object:?} {key:?}: redo");
                if object.is_some() {
                    assert!(s.show_world());
                    assert_eq!(look(&s), seen, "{object:?} {key:?}: back in the world");
                }
            }
        }
    }

    #[test]
    fn the_palette_files_put_slot_1_first() {
        let mut scene = Scene::new([8; 3]);
        scene.put_color(1, [10, 20, 30, 255]);
        scene.put_color(255, [255, 238, 221, 255]);
        let text = hex(&scene.palette);
        assert!(text.starts_with("0a141e\n") && text.ends_with("ffeedd\n"), "{text}");
        assert_eq!(text.lines().count(), 255);
        let png = crate::files::palette_png(&scene).unwrap();
        let mut reader = png::Decoder::new(std::io::Cursor::new(png)).read_info().unwrap();
        let mut px = vec![0; reader.output_buffer_size().unwrap()];
        let info = reader.next_frame(&mut px).unwrap();
        assert_eq!((info.width, info.height, info.color_type), (256, 1, png::ColorType::Rgba));
        assert_eq!((&px[..4], &px[254 * 4..255 * 4]), (&[10, 20, 30, 255][..], &[255, 238, 221, 255][..]));
        assert_eq!(px[255 * 4..], scene.palette[0]);
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
