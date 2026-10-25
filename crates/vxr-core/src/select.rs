//! The selection and what can be done with it, as in MagicaVoxel's select mode: select by
//! box or by colour (replace, add or subtract), then move, copy, cut, paste, delete or fill
//! it, and rotate 90 degrees, flip or loop (shift with wrap-around) it. With nothing selected
//! the transforms apply to the whole model. Every change to voxels is one undo step that
//! also records the selection, so undo puts both back. Choosing what is selected is not an
//! undo step. All positions are map cells.

use std::collections::HashSet;

use crate::Scene;

/// How a new selection combines with the current one (click, Shift-click, Alt-click).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Mode {
    Replace,
    Add,
    Subtract,
}

/// A transform of the selection, or of the whole model when nothing is selected.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Transform {
    /// Move by an offset.
    Move([i32; 3]),
    /// Turn 90 degrees about an axis (counter-clockwise looking down it), around the
    /// centre of the bounds.
    Rotate(usize),
    /// Mirror across the middle of the bounds on an axis.
    Flip(usize),
    /// Shift one cell along an axis (1 or -1); cells pushed out of the bounds come back
    /// in on the other side.
    Loop(usize, i32),
}

/// What Copy or Cut took: voxels relative to the low corner of their bounds.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Clip {
    pub voxels: Vec<([i32; 3], u8)>,
    pub size: [i32; 3],
    /// Map cell of the low corner where it was copied from.
    pub from: [i32; 3],
}

/// Inclusive bounds of a set of cells.
pub fn bounds(cells: impl IntoIterator<Item = [i32; 3]>) -> Option<([i32; 3], [i32; 3])> {
    let mut it = cells.into_iter();
    let first = it.next()?;
    Some(it.fold((first, first), |(lo, hi), p| {
        (std::array::from_fn(|a| lo[a].min(p[a])), std::array::from_fn(|a| hi[a].max(p[a])))
    }))
}

impl Transform {
    /// Where cell `p` goes, for cells inside the inclusive bounds `lo`..`hi`.
    pub fn apply(self, p: [i32; 3], lo: [i32; 3], hi: [i32; 3]) -> [i32; 3] {
        let mut q = p;
        match self {
            Transform::Move(d) => q = std::array::from_fn(|a| p[a] + d[a]),
            Transform::Flip(a) => q[a] = lo[a] + hi[a] - p[a],
            Transform::Loop(a, dir) => {
                let size = hi[a] - lo[a] + 1;
                q[a] = lo[a] + (p[a] - lo[a] + dir).rem_euclid(size);
            }
            Transform::Rotate(a) => {
                let (u, v) = ((a + 1) % 3, (a + 2) % 3);
                let (su, sv) = (hi[u] - lo[u] + 1, hi[v] - lo[v] + 1);
                // Keep the centre: the turned box starts half the size difference in. Division
                // truncates toward zero, so turning back undoes the offset exactly.
                let (nu, nv) = (lo[u] + (su - sv) / 2, lo[v] + (sv - su) / 2);
                q[u] = nu + (sv - 1 - (p[v] - lo[v]));
                q[v] = nv + (p[u] - lo[u]);
            }
        }
        q
    }
}

impl Scene {
    /// The selected cells that hold a voxel.
    pub fn selection(&self) -> impl Iterator<Item = [i32; 3]> + '_ {
        self.selection.iter().copied().filter(|p| self.map.get(p[0], p[1], p[2]) != 0)
    }

    /// Whether map cell `p` is selected and holds a voxel.
    pub fn is_selected(&self, p: [i32; 3]) -> bool {
        self.selection.contains(&p) && self.map.get(p[0], p[1], p[2]) != 0
    }

    /// How many voxels are selected. Counted again only after the selection changes or a
    /// cell fills or empties, so calling it every frame is cheap.
    pub fn selected_count(&self) -> usize {
        let key = (self.selection_version, self.map.fills);
        if let Some((v, f, n)) = self.selected_cache.get()
            && (v, f) == key
        {
            return n;
        }
        let n = self.selection().count();
        self.selected_cache.set(Some((key.0, key.1, n)));
        n
    }

    /// Changes whenever the selection does.
    pub fn selection_version(&self) -> u64 {
        self.selection_version
    }

    /// Inclusive bounds of the selected voxels.
    pub fn selection_bounds(&self) -> Option<([i32; 3], [i32; 3])> {
        bounds(self.selection())
    }

    /// Select cells (only those holding a voxel count). Not an undo step.
    pub fn select(&mut self, cells: impl IntoIterator<Item = [i32; 3]>, mode: Mode) {
        let cells = cells.into_iter().filter(|p| self.map.get(p[0], p[1], p[2]) != 0);
        match mode {
            Mode::Replace => self.selection = cells.collect(),
            Mode::Add => {
                let add: Vec<_> = cells.collect();
                self.selection.extend(add);
            }
            Mode::Subtract => {
                let sub: Vec<_> = cells.collect();
                for p in sub {
                    self.selection.remove(&p);
                }
            }
        }
        self.selection.retain(|p| self.map.get(p[0], p[1], p[2]) != 0);
        self.selection_version += 1;
    }

    /// Select every voxel of palette index `v` (MagicaVoxel's colour select).
    pub fn select_color(&mut self, v: u8, mode: Mode) {
        let mut cells = Vec::new();
        if v != 0 && self.map.counts[v as usize] > 0 {
            self.for_each_voxel(|p, c| {
                if c == v {
                    cells.push(p);
                }
            });
        }
        self.select(cells, mode);
    }

    pub fn select_all(&mut self) {
        let mut cells = Vec::new();
        self.for_each_voxel(|p, _| cells.push(p));
        self.select(cells, Mode::Replace);
    }

    pub fn deselect(&mut self) {
        if !self.selection.is_empty() {
            self.selection.clear();
            self.selection_version += 1;
        }
    }

    /// The voxels a transform works on: the selection, or every voxel when nothing is selected.
    pub(crate) fn subject(&self) -> (Vec<([i32; 3], u8)>, bool) {
        let mut out: Vec<_> = self.selection().map(|p| (p, self.map.get(p[0], p[1], p[2]))).collect();
        if !out.is_empty() {
            return (out, true);
        }
        self.for_each_voxel(|p, v| out.push((p, v)));
        (out, false)
    }

    /// Whether every cell of `cells` stays inside the map where `t` puts it.
    fn fits(&self, cells: &[([i32; 3], u8)], t: Transform) -> bool {
        let Some((lo, hi)) = bounds(cells.iter().map(|c| c.0)) else { return true };
        let size = self.map.size_voxels();
        cells.iter().all(|&(p, _)| {
            let q = t.apply(p, lo, hi);
            (0..3).all(|a| q[a] >= 0 && q[a] < size[a] as i32)
        })
    }

    /// Lift `cells` and set them down where `t` puts them, inside the open edit. The
    /// selection follows them when `selected`. Every target must be inside the map
    /// ([`Scene::fits`]).
    fn place(&mut self, cells: &[([i32; 3], u8)], t: Transform, selected: bool) {
        let Some((lo, hi)) = bounds(cells.iter().map(|c| c.0)) else { return };
        for &(p, _) in cells {
            self.put(p, 0);
        }
        let mut moved = HashSet::with_capacity(cells.len());
        for &(p, v) in cells {
            let q = t.apply(p, lo, hi);
            self.put(q, v);
            moved.insert(q);
        }
        if selected {
            self.put_selection(moved);
        }
    }

    /// Move, rotate, flip or loop the selection (or the whole model when nothing is
    /// selected) as one undo step. Returns how many voxels changed, or `None`, changing
    /// nothing, when the result would not fit in the map.
    pub fn transform(&mut self, t: Transform) -> Option<usize> {
        self.commit();
        let (cells, selected) = self.subject();
        if !self.fits(&cells, t) {
            return None;
        }
        self.begin();
        self.place(&cells, t, selected);
        Some(self.commit())
    }

    /// Erase the selected voxels as one undo step; the selection empties.
    pub fn delete_selection(&mut self) -> usize {
        self.commit();
        let cells: Vec<_> = self.selection().collect();
        self.begin();
        for p in cells {
            self.put(p, 0);
        }
        self.put_selection(HashSet::new());
        self.commit()
    }

    /// Paint the selected voxels with palette index `v` as one undo step.
    pub fn fill_selection(&mut self, v: u8) -> usize {
        self.commit();
        let cells: Vec<_> = self.selection().collect();
        self.edit(cells.into_iter().map(|p| (p, v)))
    }

    /// Copy the selected voxels to the clipboard. Returns how many were copied.
    pub fn copy_selection(&mut self) -> usize {
        let cells: Vec<_> = self.selection().map(|p| (p, self.map.get(p[0], p[1], p[2]))).collect();
        let Some((lo, hi)) = bounds(cells.iter().map(|c| c.0)) else { return 0 };
        let voxels = cells.into_iter().map(|(p, v)| (std::array::from_fn(|a| p[a] - lo[a]), v)).collect();
        self.clipboard = Some(Clip { voxels, size: std::array::from_fn(|a| hi[a] - lo[a] + 1), from: lo });
        self.clipboard.as_ref().map_or(0, |c| c.voxels.len())
    }

    /// Copy, then delete, the selected voxels.
    pub fn cut_selection(&mut self) -> usize {
        if self.copy_selection() == 0 {
            return 0;
        }
        self.delete_selection()
    }

    /// Where Paste puts the clipboard's low corner: its bottom centre on cell `at` (the cell
    /// in front of the face under the pointer), or beside where it was copied from on +X.
    pub fn paste_corner(&self, at: Option<[i32; 3]>) -> Option<[i32; 3]> {
        let c = self.clipboard.as_ref()?;
        Some(match at {
            Some(p) => [p[0] - c.size[0] / 2, p[1], p[2] - c.size[2] / 2],
            None => [c.from[0] + c.size[0], c.from[1], c.from[2]],
        })
    }

    /// Paste the clipboard with its low corner at `corner`, as one undo step, and select it.
    pub fn paste(&mut self, corner: [i32; 3]) -> usize {
        let Some(clip) = self.clipboard.clone() else { return 0 };
        self.commit();
        self.begin();
        let mut pasted = HashSet::with_capacity(clip.voxels.len());
        for (d, v) in clip.voxels {
            let p = std::array::from_fn(|a| corner[a] + d[a]);
            self.put(p, v);
            if self.map.get(p[0], p[1], p[2]) == v {
                pasted.insert(p);
            }
        }
        self.put_selection(pasted);
        self.commit()
    }

    /// Start dragging the selection along the plane across `axis` through cell `start`.
    pub fn start_move(&mut self, axis: usize, start: [i32; 3]) -> Option<MoveDrag> {
        self.commit();
        let cells: Vec<_> = self.selection().map(|p| (p, self.map.get(p[0], p[1], p[2]))).collect();
        if cells.is_empty() {
            return None;
        }
        self.begin();
        Some(MoveDrag { cells, axis, layer: start[axis], start, offset: [0; 3] })
    }
}

/// A selection being dragged. Each move puts the voxels back and sets them down at the new
/// offset, so the scene shows the move live; the release is one undo step.
#[derive(Clone, Debug)]
pub struct MoveDrag {
    cells: Vec<([i32; 3], u8)>,
    /// Axis and layer of the drag plane.
    pub axis: usize,
    pub layer: i32,
    start: [i32; 3],
    pub offset: [i32; 3],
}

impl MoveDrag {
    /// The pointer moved over cell `p` on the drag plane.
    pub fn move_to(&mut self, scene: &mut Scene, p: [i32; 3]) {
        let mut d: [i32; 3] = std::array::from_fn(|a| p[a] - self.start[a]);
        d[self.axis] = 0;
        self.shift_to(scene, d);
    }

    /// Show the selection moved by `d` from where the drag started. An offset that would
    /// push any of it out of the map is ignored, so it stops at the edge.
    pub fn shift_to(&mut self, scene: &mut Scene, d: [i32; 3]) {
        if d == self.offset || !scene.fits(&self.cells, Transform::Move(d)) {
            return;
        }
        self.offset = d;
        scene.revert_voxels();
        scene.place(&self.cells, Transform::Move(d), true);
    }

    /// End the drag. Returns how many voxels changed.
    pub fn release(self, scene: &mut Scene) -> usize {
        scene.commit()
    }
}

impl Scene {
    /// The selection as one bit per voxel in the brick map's mask layout (16 words per brick
    /// slot, bit `local_index` of the cell), for the renderer to highlight. Brick slots move
    /// when bricks are freed and reused, so build it again after edits too.
    pub fn selection_mask(&self) -> Vec<u32> {
        use crate::brickmap::{B, EMPTY, MASK_WORDS, local_index};
        let mut words = Vec::new();
        for p in self.selection() {
            let [x, y, z] = p.map(|c| c as u32);
            let b = self.map.grid[self.map.cell_index(x / B, y / B, z / B)];
            if b == EMPTY {
                continue;
            }
            if words.is_empty() {
                words = vec![0u32; self.map.brick_slots() * MASK_WORDS];
            }
            let li = local_index(x % B, y % B, z % B);
            words[b as usize * MASK_WORDS + li / 32] |= 1 << (li % 32);
        }
        words
    }
}
