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
use vxr_vox::{Camera, Document, Material};

use crate::brickmap::BrickMap;
use crate::history::{
    At, Bounds, BoundsChange, CameraChange, Change, ColorChange, History, Made, MaterialChange, SelectionChange, Step,
    WorldChange,
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
    /// The saved cameras, from the file's `rCAM` chunks, in file order. A new scene has none.
    pub cameras: Vec<Camera>,
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
    /// vxr does not edit (render settings, notes, unknown chunks) is kept.
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
    /// The object picked in world mode, whose voxels show selected there.
    picked: Option<usize>,
    /// The world edit in progress (a drag, or an AI change's new objects).
    world_stroke: Option<WorldStroke>,
}

/// What a world edit in progress started from, to close it as one step or put it back.
struct WorldStroke {
    tree: vxr_vox::Tree,
    /// How many models there were.
    models: usize,
    /// Models changed in place, with what they held before.
    replaced: Vec<(usize, BrickMap)>,
    /// What the map held.
    at: At,
    /// The world made for a new scene by this edit, which putting it back takes away.
    made: Option<Box<Made>>,
}

impl WorldStroke {
    fn of(w: &World, at: At) -> WorldStroke {
        WorldStroke { tree: w.tree.clone(), models: w.models.len(), replaced: Vec::new(), at, made: None }
    }

    fn changed(&self, w: &World) -> bool {
        w.tree != self.tree || w.models.len() != self.models || !self.replaced.is_empty() || self.made.is_some()
    }
}

/// Why Selection to new object cannot run.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SplitError {
    /// The world shows: voxels are selected in a model.
    World,
    NothingSelected,
    /// The whole model is selected: it is already the object.
    Everything,
    /// The model is shared by linked copies, so the voxels would leave every copy.
    Linked,
    /// The new object's model would put the scene over its size limit
    /// ([`crate::world::check_cells`]).
    TooBig,
}

/// The changes of the edit in progress: one entry per cell or palette slot, first old value
/// and last new value.
#[derive(Default)]
struct Stroke {
    changes: Vec<Change>,
    index: HashMap<[i32; 3], usize>,
    colors: Vec<ColorChange>,
    materials: Vec<MaterialChange>,
    cameras: Vec<CameraChange>,
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
            cameras: Vec::new(),
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
            picked: None,
            world_stroke: None,
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
        s.cameras = doc.cameras();
        s.source = Some(doc);
        s.world = Some(world);
        s.load(if objects.len() == 1 { At::Object(objects[0]) } else { At::World })?;
        s.reshapes = 0;
        Ok(s)
    }

    /// Write the scene as a `.vox` file. A scene opened from a file is saved losslessly:
    /// the world is written into that file through [`World::save_into`], the palette and
    /// materials through [`Document::set_palette`] and [`Document::set_material`], and the
    /// cameras through [`Document::set_camera`], so every chunk they do not touch keeps its
    /// bytes. A new scene is written as a fresh version 200 file, split into models of at
    /// most 256^3. An opened file that cannot be written this way is an error, never a fresh
    /// file: that would drop its names, groups, layers and hidden flags.
    pub fn to_vox(&self) -> Result<Vec<u8>, String> {
        if let (Some(doc), Some(world)) = (&self.source, &self.world) {
            return self.save_into(doc.clone(), world.clone());
        }
        let world: BTreeMap<[i32; 3], u8> =
            self.world_voxels(false).into_iter().map(|(p, c)| (world_to_vox(p), c)).collect();
        let mut b = self.builder();
        b.add_world(&world);
        Ok(b.build().write())
    }

    /// A fresh version 200 file with the palette, materials and cameras and no voxels.
    fn builder(&self) -> Builder {
        let mut b = Builder::new(200);
        b.palette = Some((0..256).map(|k| self.palette[(k + 1) % 256]).collect());
        b.materials = (1..=255u8)
            .filter(|&i| self.materials[i as usize] != Material::default())
            .map(|i| (i as i32, self.materials[i as usize].to_dict()))
            .collect();
        b.cameras = self.cameras.iter().map(|c| (c.id, c.to_dict())).collect();
        b
    }

    fn save_into(&self, mut doc: Document, mut world: World) -> Result<Vec<u8>, String> {
        let file = doc.scene().map_err(|e| e.to_string())?;
        for i in 1..=255u8 {
            if file.color(i) != self.palette[i as usize] {
                doc.set_palette(i, self.palette[i as usize]).map_err(|e| format!("colour {i}: {e}"))?;
            }
            if doc.material(i) != self.materials[i as usize] {
                doc.set_material(i, &self.materials[i as usize]).map_err(|e| format!("material {i}: {e}"))?;
            }
        }
        for c in &self.cameras {
            if doc.camera(c.id) != Some(*c) {
                doc.set_camera(c).map_err(|e| format!("camera {}: {e}", c.id))?;
            }
        }
        if let At::Object(i) = self.at {
            world.store(i, &self.map, self.origin);
        }
        world.save_into(&mut doc)?;
        Ok(doc.write())
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
    /// group, hide, rename, layers) and add models (a duplicate's own). An error outside
    /// world mode, or if the result would be over [`MAX_EXTENT`] on an axis or its models
    /// over the size limit ([`crate::world::check_cells`]); then nothing changes.
    pub fn edit_world<R>(&mut self, f: impl FnOnce(&mut World) -> R) -> Result<R, String> {
        self.commit();
        let r = self.put_world(f);
        self.commit();
        r
    }

    /// A world change inside the open edit (opening one if needed), shown at once, so a
    /// drag through many positions becomes one undo step at [`Scene::commit`]. An error
    /// outside world mode, or if the result would be over [`MAX_EXTENT`] on an axis or its
    /// models over the size limit (then this change is dropped and the edit keeps what it had).
    pub fn put_world<R>(&mut self, f: impl FnOnce(&mut World) -> R) -> Result<R, String> {
        if self.at != At::World {
            return Err("the world does not show".into());
        }
        let w = self.world.as_mut().ok_or("no world")?;
        let (tree, models) = (w.tree.clone(), w.models.len());
        self.world_stroke.get_or_insert_with(|| WorldStroke::of(w, At::World));
        let r = f(w);
        if w.tree == tree && w.models.len() == models {
            return Ok(r);
        }
        if let Err(e) = crate::world::check_cells(w.cells()).and_then(|()| self.load(At::World)) {
            let w = self.world.as_mut().expect("a world");
            w.tree = tree;
            w.models.truncate(models);
            return Err(e);
        }
        Ok(r)
    }

    /// Move node `i` by `d` (vxr axes) inside the open edit, shown at once: the drag tick.
    /// While the shown objects still fill the same box, only the cells the moved objects
    /// left and now cover change in the map (the cost of the moved voxels, not the world's,
    /// and the renderer uploads just those bricks); otherwise the map is composed again, as
    /// by [`Scene::put_world`]. None outside world mode, if `i` is not in the tree, or if
    /// the result would be over [`MAX_EXTENT`] on an axis (then nothing changes).
    pub fn move_in_world(&mut self, i: usize, d: [i32; 3]) -> Option<()> {
        if self.at != At::World || !self.world.as_ref()?.in_tree(i) {
            return None;
        }
        let w = self.world.as_mut()?;
        self.world_stroke.get_or_insert_with(|| WorldStroke::of(w, At::World));
        let tree = w.tree.clone();
        // The shown objects that move, in the order that wins overlaps, and the cells they leave.
        let under = w.objects_under(i);
        let moved: Vec<usize> = w.priority_order().into_iter().filter(|o| under.contains(o) && w.shown(*o)).collect();
        let mut cells = Vec::new();
        for &o in &moved {
            w.for_each_object_voxel(o, |p, _| cells.push(p));
        }
        w.move_by(i, d);
        let (lo, hi) = w.shown_box();
        let kept = lo == self.origin && (0..3).all(|a| hi[a] - lo[a] + 1 == self.map.size_voxels()[a] as i32);
        // Past a third of the world's voxels the full compose costs less than patching.
        if !kept || cells.len() * 3 > self.map.voxels as usize {
            if self.load(At::World).is_err() {
                self.world.as_mut()?.tree = tree;
                return None;
            }
            return Some(());
        }
        let w = self.world.as_ref()?;
        let (mut cl, mut ch) = ([i32::MAX; 3], [i32::MIN; 3]);
        for &o in &moved {
            let (l, s) = w.placement(o)?;
            for a in 0..3 {
                cl[a] = cl[a].min(l[a]);
                ch[a] = ch[a].max(l[a] + s[a] as i32 - 1);
            }
        }
        for p in &cells {
            for a in 0..3 {
                cl[a] = cl[a].min(p[a]);
                ch[a] = ch[a].max(p[a]);
            }
        }
        let (map, origin) = (&mut self.map, self.origin);
        let at = |p: [i32; 3]| -> [i32; 3] { std::array::from_fn(|a| p[a] - origin[a]) };
        let stack = w.stack().within(cl, ch);
        if stack.holds_only(&moved) {
            // Nothing else shows around the moved objects: clear what they left, paint what
            // they cover now, lowest priority first.
            for p in &cells {
                let m = at(*p);
                map.set(m[0], m[1], m[2], 0);
            }
            for &o in moved.iter().rev() {
                w.for_each_object_voxel(o, |p, c| {
                    let m = at(p);
                    map.set(m[0], m[1], m[2], c);
                });
            }
        } else {
            for &o in &moved {
                w.for_each_object_voxel(o, |p, _| cells.push(p));
            }
            for p in cells {
                let m = at(p);
                map.set(m[0], m[1], m[2], stack.color_at(w, p));
            }
        }
        self.show_picked();
        Some(())
    }

    /// Close the open world edit: what it changed, if anything, for the undo step.
    fn commit_world(&mut self) -> Option<Box<WorldChange>> {
        let (Some(s), Some(w)) = (self.world_stroke.take(), &self.world) else { return None };
        if !s.changed(w) {
            return None;
        }
        Some(Box::new(WorldChange {
            new: w.tree.clone(),
            added: w.models[s.models..].to_vec(),
            replaced: s.replaced.into_iter().map(|(m, before)| (m, before, w.models[m].clone())).collect(),
            old: s.tree,
            models: s.models,
            at: [s.at, self.at],
            selection: None,
            made: s.made,
        }))
    }

    /// The object or group picked in world mode, if it is still in the world.
    pub fn picked(&self) -> Option<usize> {
        self.picked
    }

    /// Pick object or group `i` in world mode (None picks nothing): the voxels of every
    /// object at or under it show selected. Not an undo step. False outside world mode or
    /// if `i` is not a node under the root.
    pub fn pick(&mut self, i: Option<usize>) -> bool {
        let ok = self.at == At::World && i.is_none_or(|i| self.world.as_ref().is_some_and(|w| w.in_tree(i)));
        if ok {
            self.picked = i;
            self.show_picked();
        }
        ok
    }

    /// The object whose voxel shows at map cell `p`, in world mode.
    pub fn object_at(&self, p: [i32; 3]) -> Option<usize> {
        self.world.as_ref().filter(|_| self.at == At::World)?.object_at(self.to_world(p))
    }

    /// Select the picked object's voxels in the world map, or nothing.
    fn show_picked(&mut self) {
        let mut cells = HashSet::new();
        if let (Some(i), Some(w)) = (self.picked, &self.world) {
            if w.in_tree(i) {
                for o in w.objects_under(i) {
                    w.for_each_object_voxel(o, |q, _| {
                        let p = self.to_map(q);
                        if self.map.get(p[0], p[1], p[2]) != 0 {
                            cells.insert(p);
                        }
                    });
                }
            } else {
                self.picked = None;
            }
        }
        self.selection = cells;
        self.selection_version += 1;
        self.selected_cache.set(None);
    }

    /// Store the current model into the world and load `at` into the map.
    fn switch(&mut self, at: At) -> Result<(), String> {
        self.commit();
        self.store_model();
        self.load(at)
    }

    /// Store the model being edited into the world, so the world's copy is current.
    fn store_model(&mut self) {
        if let (At::Object(i), Some(w)) = (self.at, &mut self.world) {
            w.store(i, &self.map, self.origin);
        }
    }

    /// Fill the map from the world: one object's model in world axes, or every shown object.
    fn load(&mut self, at: At) -> Result<(), String> {
        let w = self.world.as_ref().ok_or("no world")?;
        let (mut map, lo) = match at {
            At::Object(i) => w.object_map(i).ok_or("not an object")?,
            At::World => {
                let (lo, hi) = w.shown_box();
                let mut size = [0u32; 3];
                for a in 0..3 {
                    let extent = hi[a] as i64 - lo[a] as i64 + 1;
                    if extent > MAX_EXTENT as i64 {
                        return Err(format!("the world would be over {MAX_EXTENT} voxels on an axis ({extent})"));
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
        if self.picked.is_some_and(|i| !w.in_tree(i)) {
            self.picked = None;
        }
        map.track_dirty(self.map.tracking());
        let size = map.size_voxels();
        self.map = map;
        self.origin = lo;
        self.floor = 0;
        // Mirror across the middle of the model on every axis.
        self.pivot = std::array::from_fn(|a| 2 * lo[a] + size[a] as i32);
        self.at = at;
        if at == At::World {
            self.show_picked();
        } else {
            self.selection.clear();
            self.selection_version += 1;
            self.selected_cache.set(None);
        }
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

    /// Change one saved camera inside the open edit (opening one if needed), so a value
    /// dragged through many steps becomes one undo step at [`Scene::commit`]. Returns false
    /// if the scene has no camera `c.id` or `c` holds a value that is not finite.
    pub fn put_camera(&mut self, c: Camera) -> bool {
        let finite = [c.radius, c.frustum, c.fov].into_iter().chain(c.focus).chain(c.angle).all(f32::is_finite);
        if !finite {
            return false;
        }
        let Some(k) = self.cameras.iter().position(|o| o.id == c.id) else { return false };
        let old = self.cameras[k];
        if old == c {
            return true;
        }
        self.cameras[k] = c;
        let s = self.stroke.get_or_insert_default();
        match s.cameras.iter_mut().find(|x| x.new.id == c.id) {
            Some(x) => x.new = c,
            None => s.cameras.push(CameraChange { old, new: c }),
        }
        true
    }

    /// Set one saved camera as an undoable step. Returns false if the scene has no camera `c.id` or `c` is not finite.
    pub fn set_camera(&mut self, c: Camera) -> bool {
        self.commit();
        let found = self.put_camera(c);
        self.commit();
        found
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

    /// Put back every cell the open edit changed, the bounds and the world, keeping the edit
    /// open (its colour and material changes stay). Shape drags redraw from scratch on each
    /// move this way.
    pub fn revert_voxels(&mut self) {
        if let (Some(s), Some(w)) = (&mut self.world_stroke, &mut self.world)
            && s.changed(w)
        {
            w.tree = s.tree.clone();
            w.models.truncate(s.models);
            for (m, before) in s.replaced.drain(..) {
                w.models[m] = before;
            }
            if s.made.take().is_some() {
                // Back to a new scene: the map is its one model again, as it was.
                (self.world, self.source, self.at, self.picked) = (None, None, At::Map, None);
            } else if self.at == At::World {
                // It fitted when the edit began, so it fits again.
                let _ = self.load(At::World);
            }
        }
        if let Some(b) = self.stroke.as_mut().and_then(|s| s.bounds.take()) {
            self.reshape(b.old);
        }
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
        self.commit_as(None)
    }

    /// Close the open edit as one undo step called `label` (an AI change: "AI: make the roof
    /// red"), as [`Scene::commit`] does.
    pub fn commit_named(&mut self, label: &str) -> usize {
        self.commit_as(Some(label.to_string()))
    }

    fn commit_as(&mut self, label: Option<String>) -> usize {
        // A world change and the palette changes of the same edit are one step.
        let world = self.commit_world();
        let Some(mut s) = self.stroke.take() else {
            if world.is_some() {
                self.history.push(Step { world, at: self.at, label, ..Step::default() });
            }
            return 0;
        };
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
            cameras: s.cameras.into_iter().filter(|c| c.old != c.new).collect(),
            selection,
            bounds: s.bounds.filter(|b| b.old != b.new),
            world,
            at: self.at,
            label,
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
        if step.world.is_some() {
            // A world step loads the map it lands on itself.
            self.store_model();
        } else if step.needs_map() && step.at != self.at && self.switch(step.at).is_err() {
            self.history.redo();
            return false;
        }
        self.apply(&mut step, true);
        true
    }

    pub fn redo(&mut self) -> bool {
        self.commit();
        let Some(mut step) = self.history.redo().cloned() else { return false };
        if step.world.is_some() {
            self.store_model();
        } else if step.needs_map() && step.at != self.at && self.switch(step.at).is_err() {
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
        if let Some(c) = &step.world {
            self.apply_world(c, revert);
        }
        if revert && let Some(b) = step.bounds {
            let d = self.reshape(b.old);
            step.translate(d);
        }
        if !revert && let Some(b) = step.bounds {
            // Cells the step adds may lie outside the old bounds, so the map moves first.
            let d = self.reshape(b.new);
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
        for c in &step.cameras {
            if let Some(k) = self.cameras.iter_mut().find(|k| k.id == c.new.id) {
                *k = if revert { c.old } else { c.new };
            }
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
    }

    /// Apply or revert a world step, then load the map it lands on (unless it only changed
    /// the tree where the map already is) and its selection.
    fn apply_world(&mut self, c: &WorldChange, revert: bool) {
        if !revert && let Some(m) = &c.made {
            self.world = Some(m.world.clone());
            self.source = Some(m.source.clone());
        }
        let Some(w) = &mut self.world else { return };
        if revert {
            w.tree = c.old.clone();
            w.models.truncate(c.models);
        } else {
            w.tree = c.new.clone();
            w.models.extend(c.added.iter().cloned());
        }
        for (m, before, after) in &c.replaced {
            w.models[*m] = if revert { before } else { after }.clone();
        }
        let at = c.at[!revert as usize];
        if revert && let Some(m) = &c.made {
            // Back to a new scene without a world: its one object's model is the map again.
            let (mut map, lo) = w.object_map(w.objects()[0]).expect("the made world has one object");
            map.track_dirty(self.map.tracking());
            (self.map, self.origin, self.pivot, self.floor) = (map, lo, m.pivot, m.floor);
            (self.world, self.source, self.at, self.picked) = (None, None, At::Map, None);
            self.reshapes += 1;
        } else if !(c.added.is_empty() && c.replaced.is_empty() && c.at[0] == c.at[1] && at == self.at)
            || at == At::World
        {
            // It fitted when it was made, so it fits again.
            let _ = self.load(at);
        }
        if let Some(sel) = &c.selection {
            self.selection = sel[!revert as usize].iter().map(|&q| self.to_map(q)).collect();
            self.selection_version += 1;
            self.selected_cache.set(None);
        }
    }

    /// Selection to new object, as one undo step: the selected voxels leave the model being
    /// edited and become a new object right after it (see [`World::add_object_after`]), each
    /// on the world cell it was on. A new scene first becomes a world of one object, which
    /// undo takes away again. The map then holds the new object, its voxels selected.
    /// Returns the new object, or why it cannot run (nothing changes then).
    pub fn selection_to_object(&mut self) -> Result<usize, SplitError> {
        self.commit();
        let cells: Vec<[i32; 3]> = self.selection().collect();
        match self.at {
            At::World => return Err(SplitError::World),
            _ if cells.is_empty() => return Err(SplitError::NothingSelected),
            _ if cells.len() as u64 == self.map.voxels => return Err(SplitError::Everything),
            At::Object(i) => {
                let w = self.world.as_ref().expect("an object is in the world");
                if w.model_of(i).is_some_and(|m| w.users(m) > 1) {
                    return Err(SplitError::Linked);
                }
            }
            At::Map => {}
        }
        let (lo, hi) = crate::select::bounds(cells.iter().copied()).expect("cells");
        self.room_for(lo, hi)?;
        let before_at = self.at;
        let made = (self.world.is_none()).then(|| self.make_world());
        let At::Object(src) = self.at else { unreachable!("a model is edited") };
        let old_selection: Vec<[i32; 3]> = self.selection.iter().map(|&p| self.to_world(p)).collect();
        let mut rest = self.map.clone();
        let mut part = BrickMap::new(std::array::from_fn(|a| (hi[a] - lo[a] + 1) as u32));
        for p in &cells {
            part.set(p[0] - lo[0], p[1] - lo[1], p[2] - lo[2], self.map.get(p[0], p[1], p[2]));
            rest.set(p[0], p[1], p[2], 0);
        }
        let new_selection: Vec<[i32; 3]> = cells.iter().map(|&p| self.to_world(p)).collect();
        let (map, origin, part_lo) = (&self.map, self.origin, self.to_world(lo));
        let w = self.world.as_mut().unwrap();
        // The world's copy of the model being edited is stale until it is stored.
        w.store(src, map, origin);
        let (old, models) = (w.tree.clone(), w.models.len());
        let m = w.model_of(src).unwrap();
        let before = w.models[m].clone();
        w.store(src, &rest, origin);
        let after = w.models[m].clone();
        let new = w.add_object_after(src, &part, part_lo).unwrap();
        let change = WorldChange {
            old,
            new: w.tree.clone(),
            models,
            added: w.models[models..].to_vec(),
            replaced: vec![(m, before, after)],
            at: [before_at, At::Object(new)],
            selection: Some([old_selection, new_selection]),
            made,
        };
        // Back to where it was, so the step applies the same way now as on redo.
        (w.tree, w.models[m]) = (change.old.clone(), change.replaced[0].1.clone());
        w.models.truncate(models);
        self.apply_world(&change, false);
        self.history.push(Step { world: Some(Box::new(change)), at: At::Object(new), ..Step::default() });
        Ok(new)
    }

    /// The world of a new scene: one object whose model is the map, where it is, saved into
    /// a fresh file with the palette, materials and cameras. Not an undo step by itself.
    fn make_world(&mut self) -> Box<Made> {
        let source = Document::read(&self.builder().build().write()).expect("a fresh file reads");
        let mut w = World::from_doc(&source).expect("a fresh file has a world");
        let s = self.map.size_voxels();
        w.models.push(BrickMap::new([s[0], s[2], s[1]]));
        w.tree
            .nodes
            .push(vxr_vox::tree::TreeNode::new(vxr_vox::tree::Kind::Object(0), vxr_vox::scene::Affine::IDENTITY));
        w.tree.nodes[0].kind = vxr_vox::tree::Kind::Group(vec![1]);
        w.store(1, &self.map, self.origin);
        let made = Box::new(Made { world: w.clone(), source: source.clone(), pivot: self.pivot, floor: self.floor });
        (self.world, self.source, self.at) = (Some(w), Some(source), At::Object(1));
        made
    }

    /// Whether a new object's model spanning map cells `lo` to `hi` (inclusive) keeps the
    /// scene within its size limit ([`crate::world::check_cells`]).
    fn room_for(&self, lo: [i32; 3], hi: [i32; 3]) -> Result<(), SplitError> {
        let part = crate::world::cells(std::array::from_fn(|a| (hi[a] - lo[a] + 1) as u32));
        crate::world::check_cells(self.model_cells() + part).map_err(|_| SplitError::TooBig)
    }

    /// The cells of every model ([`World::cells`]), the one being edited at its size now.
    fn model_cells(&self) -> u64 {
        let here = crate::world::cells(self.map.size_voxels());
        match (&self.world, self.at) {
            (Some(w), At::Object(i)) => {
                let stale = w.model_of(i).map_or(0, |m| crate::world::cells(w.models[m].size_voxels()));
                w.cells() - stale + here
            }
            (Some(w), _) => w.cells(),
            (None, _) => here,
        }
    }

    /// Why the voxels `cells` (map cells) of the model being edited cannot become an object
    /// of their own: the world shows, there are none, they are every voxel, or the model is
    /// shared by linked copies.
    pub fn split_error(&self, cells: usize) -> Option<SplitError> {
        match self.at {
            At::World => Some(SplitError::World),
            _ if cells == 0 => Some(SplitError::NothingSelected),
            _ if cells as u64 >= self.map.voxels => Some(SplitError::Everything),
            At::Object(i) => {
                let w = self.world.as_ref().expect("an object is in the world");
                w.model_of(i).is_some_and(|m| w.users(m) > 1).then_some(SplitError::Linked)
            }
            At::Map => None,
        }
    }

    /// Selection to new object inside the open edit (opening one if needed), for a change
    /// that closes as one step at [`Scene::commit`]: `voxels` (map cells and their slots)
    /// become a new object right after the one being edited, named `name` or the first free
    /// `object N`, each on its world cell, and the cells leave the map (after any object this
    /// edit already made from it). Unlike
    /// [`Scene::selection_to_object`] the map stays on the model being edited. A new scene
    /// first becomes a world of one object, which undo (or [`Scene::revert_voxels`]) takes
    /// away again. Returns the new object, or why it cannot run (nothing changes then).
    pub fn put_object(&mut self, voxels: &[([i32; 3], u8)], name: Option<&str>) -> Result<usize, SplitError> {
        if let Some(e) = self.split_error(voxels.len()) {
            return Err(e);
        }
        let (lo, hi) = crate::select::bounds(voxels.iter().map(|c| c.0)).expect("voxels");
        self.room_for(lo, hi)?;
        let mut part = BrickMap::new(std::array::from_fn(|a| (hi[a] - lo[a] + 1) as u32));
        for &(p, v) in voxels {
            part.set(p[0] - lo[0], p[1] - lo[1], p[2] - lo[2], v);
        }
        let part_lo = self.to_world(lo);
        let at = self.at;
        let made = (self.world.is_none()).then(|| self.make_world());
        self.open_world_stroke(at, made);
        let At::Object(src) = self.at else { unreachable!("a model is edited") };
        for &(p, _) in voxels {
            self.put(p, 0);
        }
        let base = self.world_stroke.as_ref().map_or(0, |s| s.tree.nodes.len());
        let w = self.world.as_mut().expect("a world");
        w.store(src, &self.map, self.origin);
        // After the objects this edit already made from it, so they keep their order.
        let mut after = src;
        if let Some(p) = w.tree.parents()[src] {
            let kids = w.tree.children(p);
            let from = kids.iter().position(|&k| k == src).unwrap_or(0);
            after = kids[from..].iter().take_while(|&&k| k == src || k >= base).last().copied().unwrap_or(src);
        }
        let new = w.add_object_after(after, &part, part_lo).expect("an object under the root");
        if let Some(n) = name {
            w.set_name(new, Some(n));
        }
        Ok(new)
    }

    /// Name tree node `i` (None clears the name) inside the open edit, in either mode, for a
    /// change that closes as one step at [`Scene::commit`]. False if there is no world or
    /// `i` is not in it.
    pub fn put_node_name(&mut self, i: usize, name: Option<&str>) -> bool {
        if !self.world.as_ref().is_some_and(|w| w.in_tree(i)) {
            return false;
        }
        self.open_world_stroke(self.at, None);
        self.world.as_mut().expect("a world").set_name(i, name);
        true
    }

    /// Start the world edit in progress, if none is, from the world as it is, `at` being
    /// what the map held. While a model is edited its world copy is stored first, and goes
    /// back with the tree, so the tree and the copy always match.
    fn open_world_stroke(&mut self, at: At, made: Option<Box<Made>>) {
        if self.world_stroke.is_some() {
            return;
        }
        self.store_model();
        let w = self.world.as_ref().expect("a world");
        let mut s = WorldStroke { made, ..WorldStroke::of(w, at) };
        if let At::Object(i) = self.at
            && let Some(m) = w.model_of(i)
        {
            s.replaced.push((m, w.models[m].clone()));
        }
        self.world_stroke = Some(s);
    }

    /// Name tree node `i` (None clears the name) as one undo step, in world mode or while
    /// editing a model. False if there is no world or `i` is not in it.
    pub fn set_node_name(&mut self, i: usize, name: Option<&str>) -> bool {
        if self.at == At::World {
            return self.edit_world(|w| w.set_name(i, name)).is_ok();
        }
        self.commit();
        self.store_model();
        let Some(w) = self.world.as_mut().filter(|w| w.in_tree(i)) else { return false };
        let old = w.tree.clone();
        w.set_name(i, name);
        if w.tree != old {
            let change = WorldChange::tree(old, w.tree.clone(), w.models.len(), Vec::new(), self.at);
            self.history.push(Step { world: Some(Box::new(change)), at: self.at, ..Step::default() });
        }
        true
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
    /// fall outside. Returns how many voxels were erased, or why it cannot: a size is 0 or
    /// over [`MAX_EXTENT`], or the models would go over the size limit.
    pub fn set_bounds(&mut self, lo: [i32; 3], size: [u32; 3]) -> Result<usize, String> {
        self.commit();
        self.put_bounds(lo, size)?;
        Ok(self.commit())
    }

    /// Change the model bounds as [`Scene::set_bounds`] does, inside the open edit (opening
    /// one if needed), so a resize between other edits joins them in one undo step. Map cells
    /// move by the shift the result gives (new minus old). Returns how many voxels were
    /// erased and that shift, or why it cannot, changing nothing: the scene shows the world,
    /// a size is 0 or over [`MAX_EXTENT`], or the models would go over the size limit
    /// ([`crate::world::check_cells`]).
    pub fn put_bounds(&mut self, lo: [i32; 3], size: [u32; 3]) -> Result<(usize, [i32; 3]), String> {
        if self.at == At::World {
            return Err("the world shows: edit an object to change its bounds".into());
        }
        if size.iter().any(|&s| s == 0 || s > MAX_EXTENT) {
            return Err(format!("each side is 1 to {MAX_EXTENT}"));
        }
        crate::world::check_cells(
            self.model_cells() - crate::world::cells(self.map.size_voxels()) + crate::world::cells(size),
        )?;
        let old = self.bounds();
        let floor = old.floor.clamp(lo[1], lo[1] + size[1] as i32 - 1);
        let new = Bounds { lo, size, floor };
        if new == old {
            return Ok((0, [0; 3]));
        }
        let inside = |w: [i32; 3]| (0..3).all(|a| w[a] >= lo[a] && w[a] < lo[a] + size[a] as i32);
        let mut outside = Vec::new();
        self.for_each_voxel(|p, _| {
            if !inside(self.to_world(p)) {
                outside.push(p);
            }
        });
        let mut n = 0;
        for p in outside {
            n += self.put(p, 0) as usize;
        }
        let s = self.stroke.get_or_insert_default();
        s.bounds = Some(BoundsChange { old: s.bounds.map_or(old, |b| b.old), new });
        // The reshape drops selected cells outside: undo puts them back.
        if s.selection.is_none() && !self.selection.is_empty() {
            s.selection = Some(self.selection.iter().copied().collect());
        }
        Ok((n, self.reshape(new)))
    }

    /// Replace the map with one covering `b`, keeping every voxel inside it, and move every
    /// map cell the scene remembers (selection, history, clipboard, the open edit) with it.
    /// Returns the shift from old map cells to new ones.
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
        if let Some(s) = &mut self.stroke {
            s.changes.iter_mut().for_each(|c| c.pos = shift(c.pos));
            s.index = s.changes.iter().enumerate().map(|(i, c)| (c.pos, i)).collect();
            if let Some(sel) = &mut s.selection {
                sel.iter_mut().for_each(|p| *p = shift(*p));
            }
        }
        self.reshapes += 1;
        d
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Catches: a lossless save that failed fell back to a fresh file without the scene
    /// graph (and with the hidden objects shown) instead of failing. Opening checks every
    /// chunk, so a `MATL` chunk that does not parse is put into the opened document here.
    #[test]
    fn a_failed_lossless_save_is_an_error_not_a_flattened_file() {
        let bytes =
            vxr_vox::fixtures::all().into_iter().find(|f| f.name == "22-world").unwrap().builder.build().write();
        let mut s = Scene::from_vox(&bytes).unwrap();
        s.source.as_mut().unwrap().raw.chunks.push(vxr_vox::Chunk::new(vxr_vox::chunk::MATL, vec![1, 2]));
        s.set_color(5, [1, 2, 3, 255]);

        let e = s.to_vox().unwrap_err();
        assert!(e.contains("MATL"), "{e}");
        assert_eq!(crate::files::vox_bytes(&s).unwrap_err(), e);
    }
}
