//! Model tools, as in MagicaVoxel's and vengi's edit menus: hollow (keep only the voxels
//! that can be seen from outside) and fill (fill the space the model encloses). Both work on
//! the selection, or the whole model when nothing is selected, and each is one undo step.
//! "Outside" is every empty cell reachable from beyond the voxels' bounds through face
//! neighbours, so a closed shell counts as solid and an open cup does not get filled.

use crate::Scene;
use crate::select::bounds;

const SOLID: u8 = 1;
const OUTSIDE: u8 = 2;

/// A dense grid over the bounds of some voxels, padded by one cell on every side, with each
/// cell marked solid, outside (reachable from the padding) or neither (enclosed).
struct Space {
    lo: [i32; 3],
    dims: [usize; 3],
    cells: Vec<u8>,
}

impl Space {
    fn new(solid: &[[i32; 3]]) -> Option<Space> {
        let (lo, hi) = bounds(solid.iter().copied())?;
        let lo = lo.map(|c| c - 1);
        let dims: [usize; 3] = std::array::from_fn(|a| (hi[a] + 1 - lo[a] + 1) as usize);
        let mut sp = Space { lo, dims, cells: vec![0; dims[0] * dims[1] * dims[2]] };
        for &p in solid {
            let i = sp.index(p);
            sp.cells[i] = SOLID;
        }
        // Flood the outside from the padding's corner, which is never solid.
        let mut stack = vec![0usize];
        sp.cells[0] = OUTSIDE;
        let (sx, sy) = (1, dims[0]);
        let sz = dims[0] * dims[1];
        while let Some(i) = stack.pop() {
            let (x, y, z) = (i % dims[0], i / sy % dims[1], i / sz);
            let mut go = |j: usize, cells: &mut Vec<u8>| {
                if cells[j] == 0 {
                    cells[j] = OUTSIDE;
                    stack.push(j);
                }
            };
            if x > 0 {
                go(i - sx, &mut sp.cells);
            }
            if x + 1 < dims[0] {
                go(i + sx, &mut sp.cells);
            }
            if y > 0 {
                go(i - sy, &mut sp.cells);
            }
            if y + 1 < dims[1] {
                go(i + sy, &mut sp.cells);
            }
            if z > 0 {
                go(i - sz, &mut sp.cells);
            }
            if z + 1 < dims[2] {
                go(i + sz, &mut sp.cells);
            }
        }
        Some(sp)
    }

    fn index(&self, p: [i32; 3]) -> usize {
        let [x, y, z]: [usize; 3] = std::array::from_fn(|a| (p[a] - self.lo[a]) as usize);
        x + self.dims[0] * (y + self.dims[1] * z)
    }

    fn get(&self, p: [i32; 3]) -> u8 {
        self.cells[self.index(p)]
    }

    /// Whether solid cell `p` touches the outside through one of its six faces.
    fn seen(&self, p: [i32; 3]) -> bool {
        (0..3).any(|a| {
            [-1, 1].into_iter().any(|d| {
                let mut q = p;
                q[a] += d;
                self.get(q) == OUTSIDE
            })
        })
    }

    /// Every cell that is neither solid nor outside.
    fn enclosed(&self) -> impl Iterator<Item = [i32; 3]> + '_ {
        self.cells.iter().enumerate().filter(|&(_, &c)| c == 0).map(|(i, _)| {
            let (x, y, z) = (i % self.dims[0], i / self.dims[0] % self.dims[1], i / (self.dims[0] * self.dims[1]));
            [x as i32 + self.lo[0], y as i32 + self.lo[1], z as i32 + self.lo[2]]
        })
    }
}

impl Scene {
    /// Erase every voxel of the selection (or the whole model) that cannot be seen from
    /// outside it, leaving a shell one voxel thick, as one undo step. Returns how many
    /// voxels were removed.
    pub fn hollow(&mut self) -> usize {
        self.commit();
        let (cells, _) = self.subject();
        let solid: Vec<_> = cells.iter().map(|c| c.0).collect();
        let Some(sp) = Space::new(&solid) else { return 0 };
        let inner: Vec<_> = solid.into_iter().filter(|&p| !sp.seen(p)).collect();
        self.edit(inner.into_iter().map(|p| (p, 0)))
    }

    /// Fill the empty space the selection (or the whole model) encloses with palette index
    /// `v`, as one undo step. Cells another voxel already holds are left alone. Returns how
    /// many voxels were added.
    pub fn fill_interior(&mut self, v: u8) -> usize {
        self.commit();
        if v == 0 {
            return 0;
        }
        let (cells, _) = self.subject();
        let solid: Vec<_> = cells.iter().map(|c| c.0).collect();
        let Some(sp) = Space::new(&solid) else { return 0 };
        let holes: Vec<_> = sp.enclosed().filter(|p| self.map.get(p[0], p[1], p[2]) == 0).collect();
        self.edit(holes.into_iter().map(|p| (p, v)))
    }
}
