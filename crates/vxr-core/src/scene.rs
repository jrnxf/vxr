//! An editable scene: the brick map, the palette and the undo history, plus `.vox` open
//! and save.
//!
//! Axes: vxr is y-up right-handed. MagicaVoxel is z-up right-handed, so a file voxel at
//! (x, y, z) is the vxr cell (x, z, -y). Map cell `p` is the world cell `p + origin`, so
//! the map can start at zero while the scene keeps its file coordinates.
//!
//! An opened file is a [`World`] of objects. The map holds either one object's model, in
//! world axes where it shows (model mode, where every tool edits), or every shown object
//! composed into one map (world mode, where voxels cannot be edited and world edits move,
//! copy, group and hide objects). A new scene has no world: the map is its one model.

use std::collections::{BTreeMap, HashMap, HashSet};

use vxr_vox::build::Builder;
use vxr_vox::{Document, Material};

use crate::brickmap::BrickMap;
use crate::history::{
    At, Bounds, BoundsChange, Change, ColorChange, History, MaterialChange, SelectionChange, Step, WorldChange,
};
use crate::select::Clip;
use crate::world::World;

/// Largest scene extent per axis, in voxels. The brick grid for 2048^3 is 64 MiB.
pub const MAX_EXTENT: u32 = 2048;

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
    /// Twice the world coordinate of each mirror plane: world cell `q` mirrors to
    /// `pivot - 1 - q`. Planes are the opened file's x = 0 and y = 0 (MagicaVoxel models are
    /// centred on their translation), or x = 0 and z = 0 in a new scene, and the middle of the
    /// opened model's height, or of a new scene's map.
    pub pivot: [i32; 3],
    /// The file this scene was opened from. Saving applies the edits to it, so whatever
    /// vxr does not edit (materials, layers, cameras, names, unknown chunks) is kept.
    source: Option<Document>,
    history: History,
    stroke: Option<Stroke>,
    /// Selected map cells (see [`crate::select`]). An edit that empties selected cells drops
    /// them at [`Scene::commit`], so a voxel added there later is not selected; until then
    /// they stay in the set but are ignored.
    pub(crate) selection: HashSet<[i32; 3]>,
    /// Bumped on every selection change, so the renderer knows when to upload it again.
    pub(crate) selection_version: u64,
    /// [`Scene::selected_count`] at (`selection_version`, `map.fills`).
    pub(crate) selected_cache: std::cell::Cell<Option<(u64, u64, usize)>>,
    /// What Copy or Cut took, for Paste.
    pub clipboard: Option<Clip>,
    /// Bumped whenever the map is replaced by a resize or crop (or their undo), so the
    /// renderer knows to upload it again.
    reshapes: u64,
    /// The objects of the opened file, or None for a new scene. While the map holds an
    /// object's model (`at` is [`At::Object`]), the world's copy of that model is stale.
    world: Option<World>,
    /// What the map holds.
    at: At,
}

/// The changes of the edit in progress: one entry per cell or palette slot, first old value
/// and last new value.
#[derive(Default)]
struct Stroke {
    changes: Vec<Change>,
    index: HashMap<[i32; 3], usize>,
    colors: Vec<ColorChange>,
    materials: Vec<MaterialChange>,
    /// The selection when the edit first changed it.
    selection: Option<Vec<[i32; 3]>>,
    bounds: Option<BoundsChange>,
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
            origin: [-(size[0] as i32 / 2), 0, -(size[2] as i32 / 2)],
            floor: 0,
            pivot: [0, size[1] as i32, 0],
            source: None,
            history: History::default(),
            stroke: None,
            selection: HashSet::new(),
            selection_version: 0,
            selected_cache: Default::default(),
            clipboard: None,
            reshapes: 0,
            world: None,
            at: At::Map,
        }
    }

    /// Read a `.vox` file as a world of objects, hidden ones included. A file with one
    /// object opens in model mode on it, any other in world mode.
    pub fn from_vox(bytes: &[u8]) -> Result<Scene, String> {
        let doc = Document::read(bytes).map_err(|e| e.to_string())?;
        let vs = doc.scene().map_err(|e| e.to_string())?;
        let world = World::from_doc(&doc)?;
        let objects = world.objects();
        let mut s = Scene::new([1, 1, 1]);
        for (i, c) in s.palette.iter_mut().enumerate() {
            *c = vs.color(i as u8);
        }
        for i in 1..=255u8 {
            s.materials[i as usize] = doc.material(i);
        }
        s.source = Some(doc);
        s.world = Some(world);
        s.load(if objects.len() == 1 { At::Object(objects[0]) } else { At::World })?;
        s.reshapes = 0;
        Ok(s)
    }

    /// Write the scene as a `.vox` file. A scene opened from a file is saved losslessly:
    /// the world is written into that file through [`World::save_into`] and the palette and
    /// materials through [`Document::set_palette`] and [`Document::set_material`], so every
    /// chunk they do not touch keeps its bytes. A new scene is written as a fresh version
    /// 200 file, split into models of at most 256^3.
    pub fn to_vox(&self) -> Vec<u8> {
        if let (Some(doc), Some(world)) = (&self.source, &self.world)
            && let Some(bytes) = self.save_into(doc.clone(), world.clone())
        {
            return bytes;
        }
        let world: BTreeMap<[i32; 3], u8> =
            self.world_voxels(false).into_iter().map(|(p, c)| (world_to_vox(p), c)).collect();
        let mut b = Builder::new(200);
        b.palette = Some((0..256).map(|k| self.palette[(k + 1) % 256]).collect());
        b.materials = (1..=255u8)
            .filter(|&i| self.materials[i as usize] != Material::default())
            .map(|i| (i as i32, self.materials[i as usize].to_dict()))
            .collect();
        b.add_world(&world);
        b.build().write()
    }

    fn save_into(&self, mut doc: Document, mut world: World) -> Option<Vec<u8>> {
        let file = doc.scene().ok()?;
        for i in 1..=255u8 {
            if file.color(i) != self.palette[i as usize] {
                doc.set_palette(i, self.palette[i as usize]).ok()?;
            }
            if doc.material(i) != self.materials[i as usize] {
                doc.set_material(i, &self.materials[i as usize]).ok()?;
            }
        }
        if let At::Object(i) = self.at {
            world.store(i, &self.map, self.origin);
        }
        world.save_into(&mut doc).ok()?;
        Some(doc.write())
    }

    /// Every voxel of the scene by world cell: every object's (only shown ones with
    /// `shown_only`) with the overlap rule, the current model's edits included.
    pub fn world_voxels(&self, shown_only: bool) -> BTreeMap<[i32; 3], u8> {
        let Some(w) = &self.world else {
            let mut out = BTreeMap::new();
            self.for_each_voxel(|p, c| {
                out.insert(self.to_world(p), c);
            });
            return out;
        };
        match self.at {
            At::Object(i) => {
                let mut w = w.clone();
                w.store(i, &self.map, self.origin);
                w.world_voxels(shown_only)
            }
            _ => w.world_voxels(shown_only),
        }
    }

    /// The objects of the opened file, or None for a new scene. While an object's model
    /// is being edited, the world's copy of it is stale: read voxels through the scene.
    pub fn world(&self) -> Option<&World> {
        self.world.as_ref()
    }

    /// What the map holds: a new scene's model, the world, or one object's model.
    pub fn at(&self) -> At {
        self.at
    }

    /// Edit object `i`'s model (model mode), keeping the edits made to the one before.
    /// False if there is no world, `i` is not an object, or it is already being edited.
    pub fn edit_object(&mut self, i: usize) -> bool {
        self.world.as_ref().is_some_and(|w| w.model_of(i).is_some())
            && self.at != At::Object(i)
            && self.switch(At::Object(i)).is_ok()
    }

    /// Show every object (world mode), keeping the edits made to the current model. False
    /// if there is no world, it is already shown, or it is over [`MAX_EXTENT`] on an axis.
    pub fn show_world(&mut self) -> bool {
        self.world.is_some() && self.at != At::World && self.switch(At::World).is_ok()
    }

    /// A world edit as one undo step, in world mode: `f` may change the tree (move, copy,
    /// group, hide, rename, layers) and add models (a duplicate's own). None outside world
    /// mode, or if the result would be over [`MAX_EXTENT`] on an axis (then nothing changes).
    pub fn edit_world<R>(&mut self, f: impl FnOnce(&mut World) -> R) -> Option<R> {
        if self.at != At::World {
            return None;
        }
        self.commit();
        let w = self.world.as_mut()?;
        let (old, models) = (w.tree.clone(), w.models.len());
        let r = f(w);
        if w.tree == old && w.models.len() == models {
            return Some(r);
        }
        let change = WorldChange { old, new: w.tree.clone(), models, added: w.models[models..].to_vec() };
        if self.load(At::World).is_err() {
            let w = self.world.as_mut()?;
            w.tree = change.old;
            w.models.truncate(models);
            return None;
        }
        self.history.push(Step { world: Some(Box::new(change)), at: At::World, ..Step::default() });
        Some(r)
    }

    /// Store the current model into the world and load `at` into the map.
    fn switch(&mut self, at: At) -> Result<(), String> {
        self.commit();
        if let (At::Object(i), Some(w)) = (self.at, &mut self.world) {
            w.store(i, &self.map, self.origin);
        }
        self.load(at)
    }

    /// Fill the map from the world: one object's model in world axes, or every shown object.
    fn load(&mut self, at: At) -> Result<(), String> {
        let w = self.world.as_ref().ok_or("no world")?;
        let (mut map, lo) = match at {
            At::Object(i) => w.object_map(i).ok_or("not an object")?,
            At::World => {
                let (mut lo, mut hi) = ([i32::MAX; 3], [i32::MIN; 3]);
                for i in w.objects() {
                    if let (true, Some((l, size))) = (w.shown(i), w.placement(i)) {
                        for a in 0..3 {
                            lo[a] = lo[a].min(l[a]);
                            hi[a] = hi[a].max(l[a] + size[a] as i32 - 1);
                        }
                    }
                }
                if lo[0] > hi[0] {
                    (lo, hi) = ([0; 3], [0; 3]);
                }
                let mut size = [0u32; 3];
                for a in 0..3 {
                    let extent = hi[a] as i64 - lo[a] as i64 + 1;
                    if extent > MAX_EXTENT as i64 {
                        return Err(format!("scene is {extent} voxels on one axis; the limit is {MAX_EXTENT}"));
                    }
                    size[a] = extent as u32;
                }
                let mut map = BrickMap::new(size);
                w.for_each_voxel(true, |_, p, c| {
                    let m: [i32; 3] = std::array::from_fn(|a| p[a] - lo[a]);
                    if map.get(m[0], m[1], m[2]) == 0 {
                        map.set(m[0], m[1], m[2], c);
                    }
                });
                (map, lo)
            }
            At::Map => return Err("no world".into()),
        };
        map.track_dirty(self.map.tracking());
        let size = map.size_voxels();
        self.map = map;
        self.origin = lo;
        self.floor = 0;
        // Mirror across the middle of the model on every axis.
        self.pivot = std::array::from_fn(|a| 2 * lo[a] + size[a] as i32);
        self.at = at;
        if !self.selection.is_empty() {
            self.selection.clear();
        }
        self.selection_version += 1;
        self.selected_cache.set(None);
        self.reshapes += 1;
        Ok(())
    }

    pub fn to_map(&self, world: [i32; 3]) -> [i32; 3] {
        [world[0] - self.origin[0], world[1] - self.origin[1], world[2] - self.origin[2]]
    }

    pub fn to_world(&self, map: [i32; 3]) -> [i32; 3] {
        [map[0] + self.origin[0], map[1] + self.origin[1], map[2] + self.origin[2]]
    }

    /// Every occupied map cell with its palette index, brick by brick.
    pub fn for_each_voxel(&self, f: impl FnMut([i32; 3], u8)) {
        self.map.for_each(f);
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
        if self.at == At::World {
            return false;
        }
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

    /// Replace the selection inside the open edit (opening one if needed), so undo restores it.
    pub(crate) fn put_selection(&mut self, cells: HashSet<[i32; 3]>) {
        let s = self.stroke.get_or_insert_default();
        if s.selection.is_none() {
            s.selection = Some(self.selection.iter().copied().collect());
        }
        self.selection = cells;
        self.selection_version += 1;
    }

    /// Put back every cell the open edit changed, keeping the edit open (its colour and
    /// material changes stay). Shape drags redraw from scratch on each move this way.
    pub fn revert_voxels(&mut self) {
        let Some(s) = &mut self.stroke else { return };
        let changes = std::mem::take(&mut s.changes);
        s.index.clear();
        for c in changes.iter().rev() {
            self.map.set(c.pos[0], c.pos[1], c.pos[2], c.old);
        }
    }

    /// Close the open edit as one undo step. Cells and slots that ended where they started
    /// are dropped. Returns how many voxels changed.
    pub fn commit(&mut self) -> usize {
        let Some(mut s) = self.stroke.take() else { return 0 };
        if !self.selection.is_empty() {
            let emptied: Vec<_> =
                s.changes.iter().filter(|c| c.new == 0 && self.selection.contains(&c.pos)).map(|c| c.pos).collect();
            if !emptied.is_empty() {
                s.selection.get_or_insert_with(|| self.selection.iter().copied().collect());
                for p in emptied {
                    self.selection.remove(&p);
                }
                self.selection_version += 1;
            }
        }
        let selection = s.selection.and_then(|old| {
            let same = old.len() == self.selection.len() && old.iter().all(|p| self.selection.contains(p));
            (!same).then(|| Box::new(SelectionChange { old, new: self.selection.iter().copied().collect() }))
        });
        let step = Step {
            voxels: s.changes.into_iter().filter(|c| c.old != c.new).collect(),
            colors: s.colors.into_iter().filter(|c| c.old != c.new).collect(),
            materials: s.materials.into_iter().filter(|c| c.old != c.new).collect(),
            selection,
            bounds: s.bounds.filter(|b| b.old != b.new),
            world: None,
            at: self.at,
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
        let Some(mut step) = self.history.undo().cloned() else { return false };
        if step.needs_map() && step.at != self.at && self.switch(step.at).is_err() {
            self.history.redo();
            return false;
        }
        self.apply(&mut step, true);
        true
    }

    pub fn redo(&mut self) -> bool {
        self.commit();
        let Some(mut step) = self.history.redo().cloned() else { return false };
        if step.needs_map() && step.at != self.at && self.switch(step.at).is_err() {
            self.history.undo();
            return false;
        }
        self.apply(&mut step, false);
        true
    }

    pub fn history(&self) -> &History {
        &self.history
    }

    /// Apply or revert a step. A bounds change moves the map before the voxels go back
    /// (revert) or after they are dropped (apply), so the step's cells are always in the
    /// map they name.
    fn apply(&mut self, step: &mut Step, revert: bool) {
        if let (Some(c), Some(w)) = (&step.world, &mut self.world) {
            if revert {
                w.tree = c.old.clone();
                w.models.truncate(c.models);
            } else {
                w.tree = c.new.clone();
                w.models.extend(c.added.iter().cloned());
            }
            // It fitted when it was made, so it fits again.
            let _ = self.load(At::World);
        }
        if revert && let Some(b) = step.bounds {
            let d = self.reshape(b.old);
            step.translate(d);
        }
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
        if let Some(c) = &step.selection {
            self.selection = (if revert { &c.old } else { &c.new }).iter().copied().collect();
            self.selection_version += 1;
        }
        if !self.selection.is_empty() {
            let mut emptied = false;
            for c in &step.voxels {
                if (if revert { c.old } else { c.new }) == 0 && self.selection.remove(&c.pos) {
                    emptied = true;
                }
            }
            if emptied {
                self.selection_version += 1;
            }
        }
        if !revert && let Some(b) = step.bounds {
            self.reshape(b.new);
        }
    }

    /// The model bounds: where the map sits in the world, its size, and the ground layer.
    pub fn bounds(&self) -> Bounds {
        Bounds { lo: self.origin, size: self.map.size_voxels(), floor: self.origin[1] + self.floor }
    }

    /// How many times the map has been replaced (see [`Scene::set_bounds`]).
    pub fn reshapes(&self) -> u64 {
        self.reshapes
    }

    /// Change the model bounds to `size` voxels from world cell `lo`, as one undo step.
    /// Voxels outside the new bounds are erased; the ground stays where it is unless it would
    /// fall outside. Returns how many voxels were erased, or None if a size is 0 or over
    /// [`MAX_EXTENT`].
    pub fn set_bounds(&mut self, lo: [i32; 3], size: [u32; 3]) -> Option<usize> {
        self.commit();
        if self.at == At::World || size.iter().any(|&s| s == 0 || s > MAX_EXTENT) {
            return None;
        }
        let old = self.bounds();
        let floor = old.floor.clamp(lo[1], lo[1] + size[1] as i32 - 1);
        let new = Bounds { lo, size, floor };
        if new == old {
            return Some(0);
        }
        let inside = |w: [i32; 3]| (0..3).all(|a| w[a] >= lo[a] && w[a] < lo[a] + size[a] as i32);
        let mut outside = Vec::new();
        self.for_each_voxel(|p, _| {
            if !inside(self.to_world(p)) {
                outside.push(p);
            }
        });
        self.begin();
        for p in outside {
            self.put(p, 0);
        }
        self.stroke.get_or_insert_default().bounds = Some(BoundsChange { old, new });
        let n = self.commit();
        self.reshape(new);
        Some(n)
    }

    /// Replace the map with one covering `b`, keeping every voxel inside it, and move every
    /// map cell the scene remembers (selection, history, clipboard) with it. Returns the
    /// shift from old map cells to new ones.
    fn reshape(&mut self, b: Bounds) -> [i32; 3] {
        let d: [i32; 3] = std::array::from_fn(|a| self.origin[a] - b.lo[a]);
        let mut map = BrickMap::new(b.size);
        map.track_dirty(self.map.tracking());
        self.for_each_voxel(|p, v| {
            map.set(p[0] + d[0], p[1] + d[1], p[2] + d[2], v);
        });
        self.map = map;
        self.origin = b.lo;
        self.floor = b.floor - b.lo[1];
        let shift = |p: [i32; 3]| -> [i32; 3] { std::array::from_fn(|a| p[a] + d[a]) };
        let size = b.size;
        self.selection = self
            .selection
            .iter()
            .map(|&p| shift(p))
            .filter(|p| (0..3).all(|a| p[a] >= 0 && p[a] < size[a] as i32))
            .collect();
        self.selection_version += 1;
        self.selected_cache.set(None);
        self.history.translate(self.at, d);
        if let Some(c) = &mut self.clipboard {
            c.from = shift(c.from);
        }
        self.reshapes += 1;
        d
    }
}
