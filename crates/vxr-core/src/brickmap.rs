//! The sparse brick map: a dense grid of brick slots over the scene bounds,
//! and a pool of 8^3 bricks, each a 512-bit occupancy mask plus one palette byte per voxel.

pub const B: u32 = 8;
pub const BRICK_VOXELS: usize = 512;
pub const MASK_WORDS: usize = 16;
pub const EMPTY: u32 = u32::MAX;
/// Bricks per palette page: 2^18 bricks x 512 B = 128 MiB, the default max storage binding.
pub const PAGE_SHIFT: u32 = 18;
pub const PAGE_BRICKS: u32 = 1 << PAGE_SHIFT;
pub const MAX_PAGES: u32 = 4;

#[inline]
pub fn local_index(x: u32, y: u32, z: u32) -> usize {
    (x | (y << 3) | (z << 6)) as usize
}

#[derive(Clone)]
pub struct BrickMap {
    /// Scene size in bricks.
    pub dims: [u32; 3],
    /// Scene size in voxels (the model bounds): cells beyond it, in the last brick on each
    /// axis, stay empty.
    size: [u32; 3],
    /// One slot per brick cell, x fastest, then y, then z. EMPTY or a brick id.
    pub grid: Vec<u32>,
    /// 16 u32 per brick.
    pub masks: Vec<u32>,
    /// 512 bytes per brick, palette index per voxel (0 = empty).
    pub pal: Vec<u8>,
    pub free: Vec<u32>,
    pub voxels: u64,
    /// Bumped whenever a cell turns filled or empty (not when it only changes colour).
    pub fills: u64,
    /// Voxels per palette index; `counts[0]` is unused.
    pub counts: [u32; 256],
    track: bool,
    dirty_brick_flag: Vec<bool>,
    pub dirty_bricks: Vec<u32>,
    pub dirty_cells: Vec<u32>,
    /// Voxels changed since the last `take_dirty_box`, as (min, max inclusive).
    dirty_box: Option<([i32; 3], [i32; 3])>,
}

#[derive(Clone, Copy, Debug)]
pub struct Hit {
    pub t: f32,
    pub voxel: [i32; 3],
    /// Unit normal of the face hit, as -step on the crossed axis.
    pub normal: [i32; 3],
    pub value: u8,
}

impl BrickMap {
    /// A map of `size` voxels on each axis (at least one), stored in whole bricks.
    pub fn new(size: [u32; 3]) -> Self {
        let size = size.map(|s| s.max(1));
        let dims = size.map(|s| s.div_ceil(B));
        let cells = dims[0] as usize * dims[1] as usize * dims[2] as usize;
        Self {
            dims,
            size,
            grid: vec![EMPTY; cells],
            masks: Vec::new(),
            pal: Vec::new(),
            free: Vec::new(),
            voxels: 0,
            counts: [0; 256],
            track: false,
            dirty_brick_flag: Vec::new(),
            dirty_bricks: Vec::new(),
            dirty_cells: Vec::new(),
            dirty_box: None,
            fills: 0,
        }
    }

    /// The size in voxels: every cell inside it can hold a voxel.
    pub fn size_voxels(&self) -> [u32; 3] {
        self.size
    }

    /// Whether changes are being recorded for partial GPU uploads.
    pub fn tracking(&self) -> bool {
        self.track
    }

    /// Start recording which bricks and grid cells change, for partial GPU uploads.
    pub fn track_dirty(&mut self, on: bool) {
        self.track = on;
        self.dirty_brick_flag.resize(self.brick_slots(), false);
    }

    /// Allocated brick slots, including freed ones kept for reuse.
    pub fn brick_slots(&self) -> usize {
        self.masks.len() / MASK_WORDS
    }

    pub fn live_bricks(&self) -> usize {
        self.brick_slots() - self.free.len()
    }

    #[inline]
    pub fn cell_index(&self, bx: u32, by: u32, bz: u32) -> usize {
        (bx + self.dims[0] * (by + self.dims[1] * bz)) as usize
    }

    #[inline]
    fn in_bounds(&self, x: i32, y: i32, z: i32) -> bool {
        let s = self.size_voxels();
        x >= 0 && y >= 0 && z >= 0 && (x as u32) < s[0] && (y as u32) < s[1] && (z as u32) < s[2]
    }

    pub fn get(&self, x: i32, y: i32, z: i32) -> u8 {
        if !self.in_bounds(x, y, z) {
            return 0;
        }
        let (x, y, z) = (x as u32, y as u32, z as u32);
        let b = self.grid[self.cell_index(x / B, y / B, z / B)];
        if b == EMPTY {
            return 0;
        }
        self.pal[b as usize * BRICK_VOXELS + local_index(x % B, y % B, z % B)]
    }

    fn alloc_brick(&mut self) -> u32 {
        if let Some(b) = self.free.pop() {
            return b;
        }
        let b = self.brick_slots() as u32;
        self.masks.extend_from_slice(&[0u32; MASK_WORDS]);
        self.pal.resize(self.pal.len() + BRICK_VOXELS, 0);
        if self.track {
            self.dirty_brick_flag.push(false);
        }
        b
    }

    #[inline]
    fn mark_brick(&mut self, b: u32) {
        if self.track && !self.dirty_brick_flag[b as usize] {
            self.dirty_brick_flag[b as usize] = true;
            self.dirty_bricks.push(b);
        }
    }

    /// Set one voxel; `v = 0` erases. Returns true if it changed.
    pub fn set(&mut self, x: i32, y: i32, z: i32, v: u8) -> bool {
        if !self.in_bounds(x, y, z) {
            return false;
        }
        let (x, y, z) = (x as u32, y as u32, z as u32);
        let ci = self.cell_index(x / B, y / B, z / B);
        let mut b = self.grid[ci];
        if b == EMPTY {
            if v == 0 {
                return false;
            }
            b = self.alloc_brick();
            self.grid[ci] = b;
            if self.track {
                self.dirty_cells.push(ci as u32);
            }
        }
        let li = local_index(x % B, y % B, z % B);
        let slot = &mut self.pal[b as usize * BRICK_VOXELS + li];
        let old = *slot;
        if old == v {
            return false;
        }
        *slot = v;
        if old == 0 || v == 0 {
            self.fills += 1;
        }
        if old != 0 {
            self.counts[old as usize] -= 1;
        }
        if v != 0 {
            self.counts[v as usize] += 1;
        }
        let w = &mut self.masks[b as usize * MASK_WORDS + (li >> 5)];
        if v == 0 {
            *w &= !(1 << (li & 31));
            self.voxels -= 1;
        } else {
            *w |= 1 << (li & 31);
            if old == 0 {
                self.voxels += 1;
            }
        }
        self.mark_brick(b);
        if self.track {
            let p = [x as i32, y as i32, z as i32];
            self.dirty_box = Some(match self.dirty_box {
                Some((lo, hi)) => (std::array::from_fn(|a| lo[a].min(p[a])), std::array::from_fn(|a| hi[a].max(p[a]))),
                None => (p, p),
            });
        }
        if v == 0 {
            let base = b as usize * MASK_WORDS;
            if self.masks[base..base + MASK_WORDS].iter().all(|&m| m == 0) {
                self.grid[ci] = EMPTY;
                self.free.push(b);
                if self.track {
                    self.dirty_cells.push(ci as u32);
                }
            }
        }
        true
    }

    /// Hand the dirty lists to the uploader and reset them.
    pub fn take_dirty(&mut self) -> (Vec<u32>, Vec<u32>) {
        for &b in &self.dirty_bricks {
            self.dirty_brick_flag[b as usize] = false;
        }
        (std::mem::take(&mut self.dirty_bricks), std::mem::take(&mut self.dirty_cells))
    }

    /// The box of voxels changed since the last call (dirty tracking on), for the preview.
    pub fn take_dirty_box(&mut self) -> Option<([i32; 3], [i32; 3])> {
        self.dirty_box.take()
    }

    /// Bounds of occupied bricks, in voxels: (min, max exclusive). None if empty.
    pub fn occupied_bounds(&self) -> Option<([u32; 3], [u32; 3])> {
        let mut lo = [u32::MAX; 3];
        let mut hi = [0u32; 3];
        let mut any = false;
        for bz in 0..self.dims[2] {
            for by in 0..self.dims[1] {
                for bx in 0..self.dims[0] {
                    if self.grid[self.cell_index(bx, by, bz)] != EMPTY {
                        any = true;
                        let p = [bx, by, bz];
                        for a in 0..3 {
                            lo[a] = lo[a].min(p[a]);
                            hi[a] = hi[a].max(p[a] + 1);
                        }
                    }
                }
            }
        }
        any.then(|| ([lo[0] * B, lo[1] * B, lo[2] * B], [hi[0] * B, hi[1] * B, hi[2] * B]))
    }

    /// CPU ray cast through the map (voxel-by-voxel DDA with empty-brick skipping), for picking.
    pub fn raycast(&self, ro: [f32; 3], dir: [f32; 3], max_t: f32) -> Option<Hit> {
        let size = self.size_voxels();
        let inv = dir.map(|d| if d.abs() < 1e-9 { 1e9_f32.copysign(d) } else { 1.0 / d });
        let mut t0 = 0f32;
        let mut t1 = max_t;
        let mut entry_axis = 1usize;
        for a in 0..3 {
            let ta = (0.0 - ro[a]) * inv[a];
            let tb = (size[a] as f32 - ro[a]) * inv[a];
            let (tn, tf) = if ta < tb { (ta, tb) } else { (tb, ta) };
            if tn > t0 {
                t0 = tn;
                entry_axis = a;
            }
            t1 = t1.min(tf);
        }
        if t0 > t1 {
            return None;
        }
        let p = [ro[0] + dir[0] * t0, ro[1] + dir[1] * t0, ro[2] + dir[2] * t0];
        let mut v = [0i32; 3];
        let step: [i32; 3] = dir.map(|d| if d >= 0.0 { 1 } else { -1 });
        let mut tmax = [0f32; 3];
        let tdelta = inv.map(|i| i.abs());
        for a in 0..3 {
            v[a] = (p[a].floor() as i32).clamp(0, size[a] as i32 - 1);
            let next = v[a] + if step[a] > 0 { 1 } else { 0 };
            tmax[a] = (next as f32 - ro[a]) * inv[a];
        }
        let mut axis = entry_axis;
        let mut t = t0;
        loop {
            let val = self.get(v[0], v[1], v[2]);
            if val != 0 {
                let mut n = [0; 3];
                n[axis] = -step[axis];
                return Some(Hit { t, voxel: v, normal: n, value: val });
            }
            axis = if tmax[0] < tmax[1] && tmax[0] < tmax[2] {
                0
            } else if tmax[1] < tmax[2] {
                1
            } else {
                2
            };
            t = tmax[axis];
            if t > t1 {
                return None;
            }
            v[axis] += step[axis];
            tmax[axis] += tdelta[axis];
            if v[axis] < 0 || v[axis] >= size[axis] as i32 {
                return None;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn set_get_and_counts() {
        let mut m = BrickMap::new([24, 24, 24]);
        assert_eq!((m.size_voxels(), m.dims), ([24, 24, 24], [3, 3, 3]));
        assert!(m.set(1, 2, 3, 5));
        assert!(!m.set(1, 2, 3, 5), "same value is not a change");
        assert!(m.set(9, 2, 3, 5));
        assert_eq!((m.get(1, 2, 3), m.get(0, 0, 0)), (5, 0));
        assert_eq!((m.voxels, m.counts[5], m.live_bricks()), (2, 2, 2));
        assert!(m.set(1, 2, 3, 7), "paint");
        assert_eq!((m.voxels, m.counts[5], m.counts[7]), (2, 1, 1));
        assert!(!m.set(-1, 0, 0, 1) && !m.set(0, 24, 0, 1), "out of bounds");
        assert_eq!(m.get(-1, 0, 0), 0);
    }

    #[test]
    fn erasing_the_last_voxel_frees_its_brick_for_reuse() {
        let mut m = BrickMap::new([16, 8, 8]);
        m.set(0, 0, 0, 1);
        m.set(1, 0, 0, 1);
        m.set(0, 0, 0, 0);
        assert_eq!(m.live_bricks(), 1);
        m.set(1, 0, 0, 0);
        assert_eq!((m.live_bricks(), m.voxels, m.counts[1]), (0, 0, 0));
        assert_eq!(m.grid[0], EMPTY);
        m.set(9, 0, 0, 2);
        assert_eq!(m.brick_slots(), 1, "freed slot reused");
        assert_eq!(m.occupied_bounds(), Some(([8, 0, 0], [16, 8, 8])));
    }

    #[test]
    fn dirty_tracking_lists_each_brick_once() {
        let mut m = BrickMap::new([16, 8, 8]);
        m.track_dirty(true);
        m.set(0, 0, 0, 1);
        m.set(1, 0, 0, 1);
        m.set(8, 0, 0, 1);
        let (bricks, cells) = m.take_dirty();
        assert_eq!((bricks.len(), cells.len()), (2, 2));
        m.set(1, 0, 0, 3);
        let (bricks, cells) = m.take_dirty();
        assert_eq!((bricks.len(), cells.len()), (1, 0));
    }

    #[test]
    fn raycast_hits_the_first_voxel_and_its_face() {
        let mut m = BrickMap::new([32, 32, 32]);
        m.set(10, 5, 5, 4);
        m.set(20, 5, 5, 6);
        let h = m.raycast([-3.0, 5.5, 5.5], [1.0, 0.0, 0.0], 100.0).unwrap();
        assert_eq!((h.voxel, h.normal, h.value), ([10, 5, 5], [-1, 0, 0], 4));
        assert!((h.t - 13.0).abs() < 1e-4);
        let h = m.raycast([40.0, 5.5, 5.5], [-1.0, 0.0, 0.0], 100.0).unwrap();
        assert_eq!((h.voxel, h.normal), ([20, 5, 5], [1, 0, 0]));
        assert!(m.raycast([-3.0, 6.5, 5.5], [1.0, 0.0, 0.0], 100.0).is_none());
    }
}
