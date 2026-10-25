//! An editable scene: the brick map, the palette and the undo history, plus `.vox` open
//! and save.
//!
//! Axes: vxr is y-up right-handed. MagicaVoxel is z-up right-handed, so a file voxel at
//! (x, y, z) is the vxr cell (x, z, -y). Map cell `p` is the world cell `p + origin`, so
//! the map can start at zero while the scene keeps its file coordinates.

use std::collections::{BTreeMap, HashMap};

use vxr_vox::build::Builder;
use vxr_vox::scene::Rule;
use vxr_vox::{Document, Material};

use crate::brickmap::{B, BRICK_VOXELS, BrickMap, EMPTY, MASK_WORDS};
use crate::history::{Change, ColorChange, History, MaterialChange, Step};

/// Largest scene extent per axis, in voxels. The brick grid for 2048^3 is 64 MiB.
pub const MAX_EXTENT: u32 = 2048;
/// Free space kept around a loaded model so it can grow in every direction.
pub const PAD: u32 = 16;

pub struct Scene {
    pub map: BrickMap,
    /// RGBA by voxel index. Index 0 is empty and never drawn.
    pub palette: [[u8; 4]; 256],
    /// Material by voxel index, from the file's `MATL` chunks. Index 0 is unused.
    pub materials: [Material; 256],
    /// World cell of map cell (0, 0, 0).
    pub origin: [i32; 3],
    /// Map layer the ground plane sits on: clicks that miss every voxel land here. It is the
    /// bottom of the opened model, or 0 in a new scene.
    pub floor: i32,
    /// The file this scene was opened from. Saving applies the edits to it, so whatever
    /// vxr does not edit (materials, layers, cameras, names, unknown chunks) is kept.
    source: Option<Document>,
    history: History,
    stroke: Option<Stroke>,
}

/// The changes of the edit in progress: one entry per cell or palette slot, first old value
/// and last new value.
#[derive(Default)]
struct Stroke {
    changes: Vec<Change>,
    index: HashMap<[i32; 3], usize>,
    colors: Vec<ColorChange>,
    materials: Vec<MaterialChange>,
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
        Scene {
            map: BrickMap::new(size),
            palette,
            materials: [Material::default(); 256],
            origin: [0; 3],
            floor: 0,
            source: None,
            history: History::default(),
            stroke: None,
        }
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
        for i in 1..=255u8 {
            s.materials[i as usize] = doc.material(i);
        }
        s.origin = lo.map(|l| l - PAD as i32);
        s.floor = PAD as i32;
        for (p, c) in world {
            let m = s.to_map(p);
            s.map.set(m[0], m[1], m[2], c);
        }
        s.source = Some(doc);
        Ok(s)
    }

    /// Write the scene as a `.vox` file. A scene opened from a file is saved losslessly:
    /// the edits are applied to that file through [`Document::set_world`] and
    /// [`Document::set_palette`], so every chunk they do not touch keeps its bytes. A new
    /// scene is written as a fresh version 200 file, split into models of at most 256^3.
    pub fn to_vox(&self) -> Vec<u8> {
        let mut world = BTreeMap::new();
        self.for_each_voxel(|p, c| {
            world.insert(world_to_vox(self.to_world(p)), c);
        });
        if let Some(doc) = &self.source
            && let Some(bytes) = self.save_into(doc.clone(), &world)
        {
            return bytes;
        }
        let mut b = Builder::new(200);
        b.palette = Some((0..256).map(|k| self.palette[(k + 1) % 256]).collect());
        b.materials = (1..=255u8)
            .filter(|&i| self.materials[i as usize] != Material::default())
            .map(|i| (i as i32, self.materials[i as usize].to_dict()))
            .collect();
        b.add_world(&world);
        b.build().write()
    }

    fn save_into(&self, mut doc: Document, world: &BTreeMap<[i32; 3], u8>) -> Option<Vec<u8>> {
        let file = doc.scene().ok()?;
        for i in 1..=255u8 {
            if file.color(i) != self.palette[i as usize] {
                doc.set_palette(i, self.palette[i as usize]).ok()?;
            }
            if doc.material(i) != self.materials[i as usize] {
                doc.set_material(i, &self.materials[i as usize]).ok()?;
            }
        }
        doc.set_world(world).ok()?;
        Some(doc.write())
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
        self.begin();
        for (p, v) in cells {
            self.put(p, v);
        }
        self.commit()
    }

    /// Start an edit that spans many calls, such as a drag. Cells set with [`Scene::put`]
    /// show at once and become one undo step at [`Scene::commit`].
    pub fn begin(&mut self) {
        self.stroke.get_or_insert_default();
    }

    /// Set one map cell inside the open edit (opening one if needed). Returns true if it changed.
    pub fn put(&mut self, p: [i32; 3], v: u8) -> bool {
        let old = self.map.get(p[0], p[1], p[2]);
        if !self.map.set(p[0], p[1], p[2], v) {
            return false;
        }
        let s = self.stroke.get_or_insert_default();
        match s.index.get(&p) {
            Some(&i) => s.changes[i].new = v,
            None => {
                s.index.insert(p, s.changes.len());
                s.changes.push(Change { pos: p, old, new: v });
            }
        }
        true
    }

    /// Recolour one palette slot inside the open edit (opening one if needed), so a colour
    /// dragged through many values becomes one undo step at [`Scene::commit`]. Every voxel
    /// using the slot changes with it; nothing is remeshed.
    pub fn put_color(&mut self, index: u8, rgba: [u8; 4]) {
        let old = self.palette[index as usize];
        if old == rgba {
            return;
        }
        self.palette[index as usize] = rgba;
        let s = self.stroke.get_or_insert_default();
        match s.colors.iter_mut().find(|c| c.index == index) {
            Some(c) => c.new = rgba,
            None => s.colors.push(ColorChange { index, old, new: rgba }),
        }
    }

    /// Change one slot's material inside the open edit (opening one if needed), so a slider
    /// dragged through many values becomes one undo step at [`Scene::commit`].
    pub fn put_material(&mut self, index: u8, m: Material) {
        let old = self.materials[index as usize];
        if old == m || index == 0 {
            return;
        }
        self.materials[index as usize] = m;
        let s = self.stroke.get_or_insert_default();
        match s.materials.iter_mut().find(|c| c.index == index) {
            Some(c) => c.new = m,
            None => s.materials.push(MaterialChange { index, old, new: m }),
        }
    }

    /// Set one slot's material as an undoable step.
    pub fn set_material(&mut self, index: u8, m: Material) {
        self.commit();
        self.put_material(index, m);
        self.commit();
    }

    /// Close the open edit as one undo step. Cells and slots that ended where they started
    /// are dropped. Returns how many voxels changed.
    pub fn commit(&mut self) -> usize {
        let Some(s) = self.stroke.take() else { return 0 };
        let step = Step {
            voxels: s.changes.into_iter().filter(|c| c.old != c.new).collect(),
            colors: s.colors.into_iter().filter(|c| c.old != c.new).collect(),
            materials: s.materials.into_iter().filter(|c| c.old != c.new).collect(),
        };
        let n = step.voxels.len();
        if !step.is_empty() {
            self.history.push(step);
        }
        n
    }

    /// Recolour one palette slot as an undoable step.
    pub fn set_color(&mut self, index: u8, rgba: [u8; 4]) {
        self.commit();
        self.put_color(index, rgba);
        self.commit();
    }

    /// Give slot `to` the colour and material of slot `from`, as one undo step
    /// (MagicaVoxel's Ctrl+Shift+drag). Voxels using `to` take them.
    pub fn copy_color(&mut self, from: u8, to: u8) {
        self.commit();
        self.put_color(to, self.palette[from as usize]);
        self.put_material(to, self.materials[from as usize]);
        self.commit();
    }

    /// Swap the colours and materials of two slots and the voxels that use them, as one undo
    /// step, so the scene looks the same and only the palette order changes (MagicaVoxel's Ctrl+drag).
    /// Returns how many voxels were remapped.
    pub fn swap_colors(&mut self, a: u8, b: u8) -> usize {
        if a == b || a == 0 || b == 0 {
            return 0;
        }
        self.commit();
        let (ca, cb) = (self.palette[a as usize], self.palette[b as usize]);
        self.put_color(a, cb);
        self.put_color(b, ca);
        let (ma, mb) = (self.materials[a as usize], self.materials[b as usize]);
        self.put_material(a, mb);
        self.put_material(b, ma);
        let mut cells = Vec::new();
        if self.map.counts[a as usize] > 0 || self.map.counts[b as usize] > 0 {
            self.for_each_voxel(|p, v| {
                if v == a || v == b {
                    cells.push((p, if v == a { b } else { a }));
                }
            });
        }
        for (p, v) in cells {
            self.put(p, v);
        }
        self.commit()
    }

    pub fn undo(&mut self) -> bool {
        self.commit();
        let Some(step) = self.history.undo().cloned() else { return false };
        self.apply(&step, true);
        true
    }

    pub fn redo(&mut self) -> bool {
        self.commit();
        let Some(step) = self.history.redo().cloned() else { return false };
        self.apply(&step, false);
        true
    }

    pub fn history(&self) -> &History {
        &self.history
    }

    fn apply(&mut self, step: &Step, revert: bool) {
        let mut set = |c: &Change| {
            self.map.set(c.pos[0], c.pos[1], c.pos[2], if revert { c.old } else { c.new });
        };
        if revert {
            step.voxels.iter().rev().for_each(&mut set);
        } else {
            step.voxels.iter().for_each(&mut set);
        }
        for c in &step.colors {
            self.palette[c.index as usize] = if revert { c.old } else { c.new };
        }
        for c in &step.materials {
            self.materials[c.index as usize] = if revert { c.old } else { c.new };
        }
    }
}
