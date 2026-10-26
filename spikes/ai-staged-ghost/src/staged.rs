//! A staged change, as the AI review step would hold it: one open edit on the live scene.
//! Added and repainted cells go into the map at once (so the view and the render corner show
//! the after state); removed cells stay in the map until Apply so the ghost can draw what
//! goes. Apply closes the edit as one undo step, Discard reverts it.

use vxr_core::Scene;
use vxr_core::brickmap::{B, EMPTY, local_index};

/// Words of ghost mask per brick slot: 512 voxels x 2 bits.
pub const GHOST_WORDS: usize = 32;

pub const ADDED: u32 = 1;
pub const REPAINTED: u32 = 2;
pub const REMOVED: u32 = 3;

#[derive(Clone, Default)]
pub struct Staged {
    pub added: Vec<([i32; 3], u8)>,
    pub repainted: Vec<([i32; 3], u8)>,
    pub removed: Vec<[i32; 3]>,
}

impl Staged {
    pub fn cells(&self) -> usize {
        self.added.len() + self.repainted.len() + self.removed.len()
    }

    /// Open the edit and put the added and repainted cells into the map.
    pub fn stage(&self, scene: &mut Scene) {
        scene.begin();
        for &(p, v) in self.added.iter().chain(&self.repainted) {
            scene.put(p, v);
        }
    }

    /// The ghost: 2 bits per voxel in [`GHOST_WORDS`] words per brick slot of the map as it is
    /// now (staged cells included), as `march.wgsl` reads it.
    pub fn ghost_mask(&self, scene: &Scene) -> Vec<u32> {
        let map = &scene.map;
        let mut words = vec![0u32; map.brick_slots() * GHOST_WORDS];
        let mut mark = |p: [i32; 3], kind: u32| {
            let [x, y, z] = p.map(|c| c as u32);
            let b = map.grid[map.cell_index(x / B, y / B, z / B)];
            if b == EMPTY {
                return;
            }
            let li = local_index(x % B, y % B, z % B);
            words[b as usize * GHOST_WORDS + li / 16] |= kind << ((li % 16) * 2);
        };
        self.added.iter().for_each(|&(p, _)| mark(p, ADDED));
        self.repainted.iter().for_each(|&(p, _)| mark(p, REPAINTED));
        self.removed.iter().for_each(|&p| mark(p, REMOVED));
        words
    }

    /// Erase the removed cells and close the edit as one undo step. Returns the voxels changed.
    pub fn apply(&self, scene: &mut Scene) -> usize {
        for &p in &self.removed {
            scene.put(p, 0);
        }
        scene.commit()
    }

    /// Put back every staged cell and close the (now empty) edit: no undo step.
    pub fn discard(scene: &mut Scene) {
        scene.revert_voxels();
        scene.commit();
    }
}

/// A `total`-cell change on `scene` split into added, repainted and removed cells in the given
/// shares, each on its own third of the model's columns (sorted by x) so all three show:
/// removed takes the top voxels of the first third layer by layer, repainted the surface down
/// of the middle third, added builds layers on top of the last third. A share that runs out
/// of room in its third spills over the other columns.
pub fn plan(scene: &Scene, added: usize, repainted: usize, removed: usize) -> Staged {
    let size = scene.map.size_voxels();
    let (sx, sz) = (size[0] as usize, size[2] as usize);
    // Column (x, z) -> its voxels' heights, highest first.
    let mut cols: Vec<Vec<i32>> = vec![Vec::new(); sx * sz];
    scene.for_each_voxel(|p, _| cols[p[0] as usize + p[2] as usize * sx].push(p[1]));
    cols.iter_mut().for_each(|c| c.sort_unstable_by(|a, b| b.cmp(a)));
    let mut order: Vec<usize> = (0..sx * sz).filter(|&i| !cols[i].is_empty()).collect();
    order.sort_by_key(|&i| (i % sx, i / sx));
    let third = order.len() / 3;
    let groups = [&order[..third], &order[third..2 * third], &order[2 * third..]];
    let xyz = |i: usize, y: i32| [(i % sx) as i32, y, (i / sx) as i32];
    let mut s = Staged::default();
    // Visit `cell(column, layer)` for every column of each third in `prefer` order, layer by
    // layer within a third, until `want` cells are taken or every layer is used.
    let fill = |prefer: [usize; 3], want: usize, mut cell: Box<dyn FnMut(usize, usize) -> bool + '_>| {
        let mut n = 0;
        for g in prefer {
            for layer in 0..size[1] as usize {
                let mut any = false;
                for &i in groups[g] {
                    if n == want {
                        return;
                    }
                    if cell(i, layer) {
                        n += 1;
                        any = true;
                    }
                }
                if !any {
                    break;
                }
            }
        }
    };

    // Removed: the top voxels of the first third, one layer at a time.
    let mut taken = vec![0usize; sx * sz];
    let mut removed_cells = Vec::new();
    fill(
        [0, 1, 2],
        removed,
        Box::new(|i, layer| match cols[i].get(layer) {
            Some(&y) => {
                removed_cells.push(xyz(i, y));
                taken[i] = layer + 1;
                true
            }
            None => false,
        }),
    );
    s.removed = removed_cells;

    // Repainted: the middle third from the surface down, to the slot after each voxel's own.
    let mut repainted_cells = Vec::new();
    fill(
        [1, 2, 0],
        repainted,
        Box::new(|i, layer| match cols[i].get(taken[i] + layer) {
            Some(&y) => {
                let p = xyz(i, y);
                let v = scene.map.get(p[0], p[1], p[2]);
                repainted_cells.push((p, if v == 255 { 1 } else { v + 1 }));
                true
            }
            None => false,
        }),
    );
    s.repainted = repainted_cells;

    // Added: layers on top of the last third, inside the bounds.
    let mut added_cells = Vec::new();
    fill(
        [2, 1, 0],
        added,
        Box::new(|i, layer| {
            let y = cols[i][0] + 1 + layer as i32;
            if y >= size[1] as i32 {
                return false;
            }
            added_cells.push((xyz(i, y), 10));
            true
        }),
    );
    s.added = added_cells;
    s
}
