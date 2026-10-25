//! An editable scene: the brick map, the palette and the undo history, plus `.vox` open
//! and save.
//!
//! Axes: vxr is y-up right-handed. MagicaVoxel is z-up right-handed, so a file voxel at
//! (x, y, z) is the vxr cell (x, z, -y). Map cell `p` is the world cell `p + origin`, so
//! the map can start at zero while the scene keeps its file coordinates.

use std::collections::BTreeMap;

use vxr_vox::Document;
use vxr_vox::build::Builder;
use vxr_vox::scene::Rule;

use crate::brickmap::{B, BRICK_VOXELS, BrickMap, EMPTY, MASK_WORDS};
use crate::history::{Change, History, Step};

/// Largest scene extent per axis, in voxels. The brick grid for 2048^3 is 64 MiB.
pub const MAX_EXTENT: u32 = 2048;
/// Free space kept around a loaded model so it can grow in every direction.
pub const PAD: u32 = 16;

pub struct Scene {
    pub map: BrickMap,
    /// RGBA by voxel index. Index 0 is empty and never drawn.
    pub palette: [[u8; 4]; 256],
    /// World cell of map cell (0, 0, 0).
    pub origin: [i32; 3],
    history: History,
}

pub fn vox_to_world(p: [i32; 3]) -> [i32; 3] {
    [p[0], p[2], -p[1]]
}

pub fn world_to_vox(p: [i32; 3]) -> [i32; 3] {
    [p[0], -p[2], p[1]]
}

impl Scene {
    /// An empty scene of at least `size` voxels, with MagicaVoxel's default palette.
    pub fn new(size: [u32; 3]) -> Scene {
        let mut palette = [[0; 4]; 256];
        for (i, c) in palette.iter_mut().enumerate() {
            *c = vxr_vox::default_palette::DEFAULT_PALETTE[(i + 255) % 256];
        }
        Scene { map: BrickMap::new(size), palette, origin: [0; 3], history: History::default() }
    }

    /// Read a `.vox` file. Every instance in the scene graph is flattened into one map,
    /// hidden ones included, with MagicaVoxel's overlap rule.
    pub fn from_vox(bytes: &[u8]) -> Result<Scene, String> {
        let doc = Document::read(bytes).map_err(|e| e.to_string())?;
        let vs = doc.scene().map_err(|e| e.to_string())?;
        let world: Vec<([i32; 3], u8)> =
            vs.world_voxels(Rule::Compose).into_iter().map(|(p, c)| (vox_to_world(p), c)).collect();
        let (mut lo, mut hi) = ([0i32; 3], [0i32; 3]);
        if let Some(first) = world.first() {
            (lo, hi) = (first.0, first.0);
        }
        for (p, _) in &world {
            for a in 0..3 {
                lo[a] = lo[a].min(p[a]);
                hi[a] = hi[a].max(p[a]);
            }
        }
        let mut size = [0u32; 3];
        for a in 0..3 {
            let extent = (hi[a] as i64 - lo[a] as i64 + 1) as u64;
            if extent > MAX_EXTENT as u64 {
                return Err(format!("scene is {extent} voxels on one axis; the limit is {MAX_EXTENT}"));
            }
            size[a] = extent as u32 + 2 * PAD;
        }
        let mut s = Scene::new(size);
        for (i, c) in s.palette.iter_mut().enumerate() {
            *c = vs.color(i as u8);
        }
        s.origin = lo.map(|l| l - PAD as i32);
        for (p, c) in world {
            let m = s.to_map(p);
            s.map.set(m[0], m[1], m[2], c);
        }
        Ok(s)
    }

    /// Write a version 200 `.vox` file: the palette and every voxel, split into models of
    /// at most 256^3. Materials, layers, cameras and the scene graph of an opened file are
    /// not kept yet.
    pub fn to_vox(&self) -> Vec<u8> {
        let mut world = BTreeMap::new();
        self.for_each_voxel(|p, c| {
            world.insert(world_to_vox(self.to_world(p)), c);
        });
        let mut b = Builder::new(200);
        b.palette = Some((0..256).map(|k| self.palette[(k + 1) % 256]).collect());
        b.add_world(&world);
        b.build().write()
    }

    pub fn to_map(&self, world: [i32; 3]) -> [i32; 3] {
        [world[0] - self.origin[0], world[1] - self.origin[1], world[2] - self.origin[2]]
    }

    pub fn to_world(&self, map: [i32; 3]) -> [i32; 3] {
        [map[0] + self.origin[0], map[1] + self.origin[1], map[2] + self.origin[2]]
    }

    /// Every occupied map cell with its palette index, brick by brick.
    pub fn for_each_voxel(&self, mut f: impl FnMut([i32; 3], u8)) {
        let m = &self.map;
        for bz in 0..m.dims[2] {
            for by in 0..m.dims[1] {
                for bx in 0..m.dims[0] {
                    let b = m.grid[m.cell_index(bx, by, bz)];
                    if b == EMPTY {
                        continue;
                    }
                    let masks = &m.masks[b as usize * MASK_WORDS..(b as usize + 1) * MASK_WORDS];
                    for (w, &bits) in masks.iter().enumerate() {
                        let mut bits = bits;
                        while bits != 0 {
                            let li = w * 32 + bits.trailing_zeros() as usize;
                            bits &= bits - 1;
                            let (x, y, z) = (li as u32 & 7, (li as u32 >> 3) & 7, li as u32 >> 6);
                            let p = [(bx * B + x) as i32, (by * B + y) as i32, (bz * B + z) as i32];
                            f(p, m.pal[b as usize * BRICK_VOXELS + li]);
                        }
                    }
                }
            }
        }
    }

    /// Set map cells as one undoable step. Cells outside the map and cells that already
    /// hold the value are skipped. Returns how many voxels changed.
    pub fn edit(&mut self, cells: impl IntoIterator<Item = ([i32; 3], u8)>) -> usize {
        let mut changes = Vec::new();
        for (p, v) in cells {
            let old = self.map.get(p[0], p[1], p[2]);
            if self.map.set(p[0], p[1], p[2], v) {
                changes.push(Change { pos: p, old, new: v });
            }
        }
        let n = changes.len();
        if n > 0 {
            self.history.push(Step::Voxels(changes));
        }
        n
    }

    /// Recolour one palette slot as an undoable step.
    pub fn set_color(&mut self, index: u8, rgba: [u8; 4]) {
        let old = self.palette[index as usize];
        if old != rgba {
            self.palette[index as usize] = rgba;
            self.history.push(Step::Palette { index, old, new: rgba });
        }
    }

    pub fn undo(&mut self) -> bool {
        let Some(step) = self.history.undo().cloned() else { return false };
        self.apply(&step, true);
        true
    }

    pub fn redo(&mut self) -> bool {
        let Some(step) = self.history.redo().cloned() else { return false };
        self.apply(&step, false);
        true
    }

    pub fn history(&self) -> &History {
        &self.history
    }

    fn apply(&mut self, step: &Step, revert: bool) {
        match step {
            Step::Voxels(changes) => {
                let mut set = |c: &Change| {
                    self.map.set(c.pos[0], c.pos[1], c.pos[2], if revert { c.old } else { c.new });
                };
                if revert {
                    changes.iter().rev().for_each(&mut set);
                } else {
                    changes.iter().for_each(&mut set);
                }
            }
            Step::Palette { index, old, new } => self.palette[*index as usize] = if revert { *old } else { *new },
        }
    }
}
