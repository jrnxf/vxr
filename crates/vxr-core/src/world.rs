//! The world: several objects per scene, as in MagicaVoxel's world editor. Objects and
//! groups form a tree ([`vxr_vox::Tree`]: names, hidden flags, layers and transforms in
//! the file's z-up axes), and each object shows one model. Linked copies (instances) share
//! a model, so editing it changes every copy; a duplicate gets its own. Models are brick
//! maps in vxr's y-up axes, so the model editor can edit one directly.
//!
//! Saving patches the file the world was opened from ([`vxr_vox::Document::set_tree`]):
//! only the nodes, layers and models that changed are written, so an unedited save is
//! byte-identical and every other chunk keeps its bytes.

use vxr_vox::scene::Affine;
use vxr_vox::tree::{Kind, LayerInfo, MAX_MODEL, ModelEdits, Tree, TreeNode};
use vxr_vox::{Document, WorldSave};

use crate::brickmap::BrickMap;
use crate::scene::{MAX_EXTENT, vox_to_world, world_to_vox};

/// Most models a file may hold, and most cells (their sizes multiplied out) across them.
/// Each model is a dense grid of brick slots over its size, so a few bytes of `SIZE` chunk
/// would otherwise ask for 64 MiB, and each costs about a kilobyte however small. The
/// budget is one scene of [`MAX_EXTENT`] on every axis, more than MagicaVoxel can make.
pub const MAX_MODELS: usize = 1 << 16;
pub const MAX_CELLS: u64 = (MAX_EXTENT as u64).pow(3);

/// What models take of a scene's size limit: the file models they are saved as and their
/// cells. A file over [`MAX_MODELS`] or [`MAX_CELLS`] does not open, so no edit may make a
/// world over either, or the file vxr saves would not open again.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Budget {
    pub models: u64,
    pub cells: u64,
}

impl Budget {
    /// A model of `size`: saved as one file model, or over 256 on an axis as blocks of up
    /// to 256 (at most that many per axis, rounded up).
    pub fn of(size: [u32; 3]) -> Budget {
        Budget {
            models: size.iter().map(|&k| (k as u64).div_ceil(MAX_MODEL as u64)).product(),
            cells: size.iter().map(|&k| k as u64).product(),
        }
    }

    pub fn plus(self, o: Budget) -> Budget {
        Budget { models: self.models + o.models, cells: self.cells + o.cells }
    }

    pub fn minus(self, o: Budget) -> Budget {
        Budget { models: self.models.saturating_sub(o.models), cells: self.cells.saturating_sub(o.cells) }
    }

    /// Within the size limit, or why not.
    pub fn check(self) -> Result<(), String> {
        if self.models > MAX_MODELS as u64 {
            return Err(format!("the scene would have over {} models, the size limit of a scene", grouped(MAX_MODELS as u64)));
        }
        if self.cells > MAX_CELLS {
            let billions = MAX_CELLS as f64 / 1e9;
            return Err(format!("the models would add up to over {billions:.1} billion cells, the size limit of a scene"));
        }
        Ok(())
    }
}

/// `n` with a comma between every three digits, as people read it: 65536 is "65,536".
fn grouped(n: u64) -> String {
    let d = n.to_string();
    let mut s = String::new();
    for (i, c) in d.chars().enumerate() {
        if i > 0 && (d.len() - i) % 3 == 0 {
            s.push(',');
        }
        s.push(c);
    }
    s
}

#[derive(Clone)]
pub struct World {
    pub tree: Tree,
    /// One brick map per model, in vxr axes: the file's model voxel (x, y, z) is map cell
    /// (x, z, sy - 1 - y), so the map's size is (sx, sz, sy).
    pub models: Vec<BrickMap>,
    /// The file version: from 200 the earlier object wins where two overlap, before it the later.
    pub version: i32,
}

/// The file's (z-up) size of a model map.
pub fn vox_size(m: &BrickMap) -> [i32; 3] {
    let s = m.size_voxels();
    [s[0] as i32, s[2] as i32, s[1] as i32]
}

/// Map cell of the file's model voxel `v` in a model of file size `size`.
pub fn vox_to_local(size: [i32; 3], v: [i32; 3]) -> [i32; 3] {
    [v[0], v[2], size[1] - 1 - v[1]]
}

/// The file's model voxel of map cell `u` in a model of file size `size`.
pub fn local_to_vox(size: [i32; 3], u: [i32; 3]) -> [i32; 3] {
    [u[0], size[1] - 1 - u[2], u[1]]
}

/// The model voxel of world cell `q` (file axes) for a model of file size `size` placed by
/// `w`: the inverse of [`vxr_vox::Scene::world_cell`]. It may fall outside the model.
pub fn local_of(w: &Affine, size: [i32; 3], q: [i32; 3]) -> [i32; 3] {
    let d: [i32; 3] = std::array::from_fn(|a| 2 * q[a] + 1 - 2 * w.t[a]);
    let c = mat_vec(&transpose(&w.r), d);
    std::array::from_fn(|a| (c[a] - 1).div_euclid(2) + size[a] / 2)
}

fn transpose(r: &[[i32; 3]; 3]) -> [[i32; 3]; 3] {
    std::array::from_fn(|i| std::array::from_fn(|j| r[j][i]))
}

fn mat_vec(r: &[[i32; 3]; 3], v: [i32; 3]) -> [i32; 3] {
    std::array::from_fn(|i| (0..3).map(|k| r[i][k] * v[k]).sum())
}

impl World {
    /// Read every object of a `.vox` document, hidden ones included.
    pub fn from_doc(doc: &Document) -> Result<World, String> {
        let s = doc.scene().map_err(|e| e.to_string())?;
        let tree = vxr_vox::Tree::read(&s);
        let mut budget = Budget { models: s.models.len() as u64, cells: 0 };
        budget.check()?;
        for m in &s.models {
            if m.size.iter().any(|&k| k < 1 || k as u32 > MAX_EXTENT) {
                return Err(format!("a model is {:?} voxels; each side must be 1 to {MAX_EXTENT}", m.size));
            }
            // XYZI holds byte coordinates, so no voxel of it can be past 256.
            if !m.compressed && m.size.iter().any(|&k| k > MAX_MODEL) {
                return Err(format!(
                    "a model is {:?} voxels; each side of an XYZI model must be 1 to {MAX_MODEL}",
                    m.size
                ));
            }
            budget.cells += m.size.iter().map(|&k| k as u64).product::<u64>();
            budget.check()?;
        }
        let mut models = Vec::with_capacity(s.models.len());
        for m in &s.models {
            let mut map = BrickMap::new([m.size[0] as u32, m.size[2] as u32, m.size[1] as u32]);
            for v in m.effective_voxels() {
                let u = vox_to_local(m.size, [v[0] as i32, v[1] as i32, v[2] as i32]);
                map.set(u[0], u[1], u[2], v[3]);
            }
            models.push(map);
        }
        Ok(World { tree, models, version: s.version })
    }

    /// What every model takes of the size limit, linked copies counted once.
    pub fn budget(&self) -> Budget {
        self.models.iter().fold(Budget::default(), |b, m| b.plus(Budget::of(m.size_voxels())))
    }

    /// Objects (tree nodes holding a model) in tree order.
    pub fn objects(&self) -> Vec<usize> {
        self.tree.order().into_iter().filter(|&i| matches!(self.tree.nodes[i].kind, Kind::Object(_))).collect()
    }

    /// Whether node `i` is in the tree under the root (an object or a group).
    pub fn in_tree(&self, i: usize) -> bool {
        i < self.tree.nodes.len() && self.tree.parents()[i].is_some()
    }

    /// The model object `i` shows.
    pub fn model_of(&self, i: usize) -> Option<usize> {
        match self.tree.nodes[i].kind {
            Kind::Object(m) => Some(m),
            Kind::Group(_) => None,
        }
    }

    /// Node `i`'s transform in the world (file axes): its parents' then its own.
    pub fn world_transform(&self, i: usize) -> Affine {
        self.transform_in(&self.tree.parents(), i)
    }

    /// [`World::world_transform`] with the tree's `parents` taken once by the caller, so a
    /// pass over every object costs the tree's size once, not once per object.
    fn transform_in(&self, parents: &[Option<usize>], i: usize) -> Affine {
        let mut chain = vec![i];
        while let Some(p) = parents[*chain.last().unwrap()] {
            chain.push(p);
        }
        chain.iter().rev().fold(Affine::IDENTITY, |w, &k| w.then(&self.tree.nodes[k].local))
    }

    fn layer_hidden(&self, id: i32) -> bool {
        self.tree.layers.iter().any(|l| l.id == id && l.hidden)
    }

    /// Whether node `i` shows: neither it nor a group above it is hidden or on a hidden layer.
    pub fn shown(&self, i: usize) -> bool {
        self.shown_in(&self.tree.parents(), i)
    }

    fn shown_in(&self, parents: &[Option<usize>], i: usize) -> bool {
        let mut k = Some(i);
        while let Some(n) = k {
            let node = &self.tree.nodes[n];
            if node.hidden || self.layer_hidden(node.layer) {
                return false;
            }
            k = parents[n];
        }
        true
    }

    /// Every voxel of every object (only shown ones with `shown_only`) as (object, vxr
    /// world cell, palette index), objects in the order that wins overlaps: the first call
    /// for a cell is the one that shows.
    pub fn for_each_voxel(&self, shown_only: bool, mut f: impl FnMut(usize, [i32; 3], u8)) {
        let parents = self.tree.parents();
        for i in self.priority_order() {
            if !shown_only || self.shown_in(&parents, i) {
                self.for_each_object_voxel_in(&parents, i, |p, c| f(i, p, c));
            }
        }
    }

    /// Objects in the order that wins overlaps: the first one with a voxel at a cell shows it.
    pub(crate) fn priority_order(&self) -> Vec<usize> {
        let mut objects = self.objects();
        if self.version < 200 {
            objects.reverse();
        }
        objects
    }

    /// Every voxel of object `i` as (vxr world cell, palette index), hidden or not.
    pub fn for_each_object_voxel(&self, i: usize, f: impl FnMut([i32; 3], u8)) {
        self.for_each_object_voxel_in(&self.tree.parents(), i, f);
    }

    fn for_each_object_voxel_in(&self, parents: &[Option<usize>], i: usize, mut f: impl FnMut([i32; 3], u8)) {
        let Some(m) = self.model_of(i) else { return };
        let w = self.transform_in(parents, i);
        let map = &self.models[m];
        let size = vox_size(map);
        map.for_each(|u, c| {
            let v = local_to_vox(size, u);
            f(vox_to_world(vxr_vox::Scene::world_cell(&w, size, v)), c);
        });
    }

    /// The world's voxels by vxr cell, with the overlap rule.
    pub fn world_voxels(&self, shown_only: bool) -> std::collections::BTreeMap<[i32; 3], u8> {
        let mut out = std::collections::BTreeMap::new();
        self.for_each_voxel(shown_only, |_, p, c| {
            out.entry(p).or_insert(c);
        });
        out
    }

    /// Where object `i`'s model lands in the world: the vxr cell of its low corner and its
    /// size in vxr axes. None for a group.
    pub fn placement(&self, i: usize) -> Option<([i32; 3], [u32; 3])> {
        self.placement_in(&self.tree.parents(), i)
    }

    fn placement_in(&self, parents: &[Option<usize>], i: usize) -> Option<([i32; 3], [u32; 3])> {
        let m = self.model_of(i)?;
        let size = vox_size(&self.models[m]);
        let w = self.transform_in(parents, i);
        let a = vox_to_world(vxr_vox::Scene::world_cell(&w, size, [0, 0, 0]));
        let b = vox_to_world(vxr_vox::Scene::world_cell(&w, size, size.map(|k| k - 1)));
        Some((std::array::from_fn(|k| a[k].min(b[k])), std::array::from_fn(|k| (a[k] - b[k]).unsigned_abs() + 1)))
    }

    /// Object `i`'s model as a map in vxr world axes (rotated and mirrored as it shows),
    /// with the world cell of its low corner, for the model editor. None for a group.
    pub fn object_map(&self, i: usize) -> Option<(BrickMap, [i32; 3])> {
        let (lo, size) = self.placement(i)?;
        let map = &self.models[self.model_of(i)?];
        let (w, vs) = (self.world_transform(i), vox_size(map));
        let mut out = BrickMap::new(size);
        map.for_each(|u, c| {
            let p = vox_to_world(vxr_vox::Scene::world_cell(&w, vs, local_to_vox(vs, u)));
            out.set(p[0] - lo[0], p[1] - lo[1], p[2] - lo[2], c);
        });
        Some((out, lo))
    }

    /// Put back object `i`'s model from `map`, a map in world axes whose cell (0, 0, 0) is
    /// world cell `lo` (what [`World::object_map`] gave, edited, maybe resized). The model
    /// takes the map's bounds as its size; when they moved, every object showing the model
    /// is moved to match, so nothing shifts in the world. Linked copies change with it.
    pub fn store(&mut self, i: usize, map: &BrickMap, lo: [i32; 3]) {
        let Some(m) = self.model_of(i) else { return };
        self.store_from(i, vox_size(&self.models[m]), map, lo);
    }

    /// [`World::store`] with object `i`'s model taken to be of file size `s` before it.
    fn store_from(&mut self, i: usize, s: [i32; 3], map: &BrickMap, lo: [i32; 3]) {
        let Some(m) = self.model_of(i) else { return };
        let w = self.world_transform(i);
        let size = map.size_voxels();
        let hi: [i32; 3] = std::array::from_fn(|k| lo[k] + size[k] as i32 - 1);
        let (p, q) = (local_of(&w, s, world_to_vox(lo)), local_of(&w, s, world_to_vox(hi)));
        let a: [i32; 3] = std::array::from_fn(|k| p[k].min(q[k]));
        let ns: [i32; 3] = std::array::from_fn(|k| (p[k] - q[k]).abs() + 1);
        let mut model = BrickMap::new([ns[0] as u32, ns[2] as u32, ns[1] as u32]);
        map.for_each(|c, v| {
            let l = local_of(&w, s, world_to_vox(std::array::from_fn(|k| lo[k] + c[k])));
            let u = vox_to_local(ns, std::array::from_fn(|k| l[k] - a[k]));
            model.set(u[0], u[1], u[2], v);
        });
        if ns != s || a != [0; 3] {
            // Model voxel v is now v - a: shift each copy by r * d in its parent's frame.
            let d: [i32; 3] = std::array::from_fn(|k| ns[k] / 2 - s[k] / 2 + a[k]);
            for n in &mut self.tree.nodes {
                if n.kind == Kind::Object(m) {
                    let r = mat_vec(&n.local.r, d);
                    n.local.t = std::array::from_fn(|k| n.local.t[k] + r[k]);
                }
            }
        }
        self.models[m] = model;
    }

    /// The shown object whose voxel shows at vxr world cell `q`, with the overlap rule.
    pub fn object_at(&self, q: [i32; 3]) -> Option<usize> {
        self.stack().object_at(self, q)
    }

    /// The shown objects in overlap order with where each lands, for looking up cells.
    pub(crate) fn stack(&self) -> Stack {
        let parents = self.tree.parents();
        let placed = self
            .priority_order()
            .into_iter()
            .filter(|&i| self.shown_in(&parents, i))
            .filter_map(|i| {
                let (lo, size) = self.placement_in(&parents, i)?;
                let m = self.model_of(i)?;
                let hi = std::array::from_fn(|a| lo[a] + size[a] as i32 - 1);
                Some(Placed { object: i, model: m, at: self.transform_in(&parents, i), lo, hi })
            })
            .collect();
        Stack { placed }
    }

    /// The box the shown objects fill in the world (vxr axes): low and high corner, both
    /// inclusive; (0, 0, 0) twice when nothing shows.
    pub(crate) fn shown_box(&self) -> ([i32; 3], [i32; 3]) {
        let (mut lo, mut hi) = ([i32::MAX; 3], [i32::MIN; 3]);
        for p in &self.stack().placed {
            for a in 0..3 {
                lo[a] = lo[a].min(p.lo[a]);
                hi[a] = hi[a].max(p.hi[a]);
            }
        }
        if lo[0] > hi[0] { ([0; 3], [0; 3]) } else { (lo, hi) }
    }

    /// The objects showing model `m`: more than one means linked copies.
    pub fn users(&self, m: usize) -> usize {
        self.objects().into_iter().filter(|&i| self.model_of(i) == Some(m)).count()
    }

    /// Put a new object right after object `i`, under the same group, on the same layer and
    /// turned the same way, whose model holds `map`: a map in world axes whose cell (0, 0, 0)
    /// is world cell `lo`, so every voxel shows where it is. It is shown and named the first
    /// free `object N`. Returns it, or None if `i` is not an object under the root.
    pub fn add_object_after(&mut self, i: usize, map: &BrickMap, lo: [i32; 3]) -> Option<usize> {
        let p = self.tree.parents()[i]?;
        let m = self.model_of(i)?;
        let mut n = self.tree.nodes[i].clone();
        n.ids = None;
        n.name = None;
        n.hidden = false;
        n.kind = Kind::Object(self.models.len());
        self.models.push(BrickMap::new([1, 1, 1]));
        let k = self.tree.nodes.len();
        self.tree.nodes.push(n);
        if let Kind::Group(kids) = &mut self.tree.nodes[p].kind {
            let at = kids.iter().position(|&c| c == i).map_or(kids.len(), |c| c + 1);
            kids.insert(at, k);
        }
        // Same parent and transform as `i`: placed as if it showed `i`'s model, then fitted.
        self.store_from(k, vox_size(&self.models[m]), map, lo);
        let name = self.free_object_name(k);
        self.set_name(k, Some(&name));
        Some(k)
    }

    /// The first `object N` that no node is called and no other unnamed object shows as
    /// (an unnamed object shows as `object` and its place among the objects).
    pub fn free_object_name(&self, skip: usize) -> String {
        let mut used: std::collections::HashSet<Vec<u8>> =
            self.tree.order().into_iter().filter_map(|i| self.tree.nodes[i].name.clone()).collect();
        for (k, i) in self.objects().into_iter().enumerate() {
            if i != skip && self.tree.nodes[i].name.is_none() {
                used.insert(format!("object {}", k + 1).into_bytes());
            }
        }
        (1..).map(|k| format!("object {k}")).find(|n| !used.contains(n.as_bytes())).unwrap()
    }

    /// Take node `i` (and what it holds) out of the tree. Its models stay, so undo can put
    /// it back. False for the root.
    pub fn remove(&mut self, i: usize) -> bool {
        let Some(p) = self.tree.parents()[i] else { return false };
        let Kind::Group(kids) = &mut self.tree.nodes[p].kind else { return false };
        kids.retain(|&k| k != i);
        true
    }

    /// Move node `i` by `d` world cells (vxr axes).
    pub fn move_by(&mut self, i: usize, d: [i32; 3]) {
        let parent = self.tree.parents()[i].map_or(Affine::IDENTITY, |p| self.world_transform(p));
        let local = mat_vec(&transpose(&parent.r), world_to_vox(d));
        let t = &mut self.tree.nodes[i].local.t;
        *t = std::array::from_fn(|a| t[a] + local[a]);
    }

    /// The objects at or under node `i`.
    pub fn objects_under(&self, i: usize) -> Vec<usize> {
        match &self.tree.nodes[i].kind {
            Kind::Object(_) => vec![i],
            Kind::Group(kids) => kids.iter().flat_map(|&k| self.objects_under(k)).collect(),
        }
    }

    /// The box node `i`'s objects fill in the world (vxr axes): low corner and size. None
    /// for an empty group.
    pub fn extent(&self, i: usize) -> Option<([i32; 3], [u32; 3])> {
        let (mut lo, mut hi) = ([i32::MAX; 3], [i32::MIN; 3]);
        for o in self.objects_under(i) {
            let (l, s) = self.placement(o)?;
            for a in 0..3 {
                lo[a] = lo[a].min(l[a]);
                hi[a] = hi[a].max(l[a] + s[a] as i32);
            }
        }
        (lo[0] < hi[0]).then(|| (lo, std::array::from_fn(|a| (hi[a] - lo[a]) as u32)))
    }

    /// Copy node `i` (as [`World::copy`]) and put the copy beside it on +X with one cell
    /// between, so it shows. Returns the copy.
    pub fn duplicate(&mut self, i: usize, linked: bool) -> Option<usize> {
        let w = self.extent(i).map_or(0, |(_, s)| s[0] as i32 + 1);
        let c = self.copy(i, linked)?;
        self.move_by(c, [w, 0, 0]);
        Some(c)
    }

    /// Copy node `i` (and what it holds) next to it. `linked` shares the models (a linked
    /// copy, MagicaVoxel's instance); otherwise each object gets its own copy of its model.
    /// Returns the copy, or None for the root.
    pub fn copy(&mut self, i: usize, linked: bool) -> Option<usize> {
        let p = self.tree.parents()[i]?;
        let c = self.copy_subtree(i, linked);
        if let Kind::Group(kids) = &mut self.tree.nodes[p].kind {
            let at = kids.iter().position(|&k| k == i).map_or(kids.len(), |k| k + 1);
            kids.insert(at, c);
        }
        Some(c)
    }

    fn copy_subtree(&mut self, i: usize, linked: bool) -> usize {
        let mut n = self.tree.nodes[i].clone();
        n.ids = None;
        n.kind = match n.kind {
            Kind::Object(m) if linked => Kind::Object(m),
            Kind::Object(m) => {
                self.models.push(self.models[m].clone());
                Kind::Object(self.models.len() - 1)
            }
            Kind::Group(kids) => Kind::Group(kids.iter().map(|&k| self.copy_subtree(k, linked)).collect()),
        };
        self.tree.nodes.push(n);
        self.tree.nodes.len() - 1
    }

    /// Put `nodes` (which share a parent, the root not among them) into a new group at the
    /// place of the first of them. Returns the group.
    pub fn group(&mut self, nodes: &[usize]) -> Option<usize> {
        let parents = self.tree.parents();
        let p = parents[*nodes.first()?]?;
        if nodes.iter().any(|&n| parents[n] != Some(p)) {
            return None;
        }
        let g = self.tree.nodes.len();
        let Kind::Group(kids) = &mut self.tree.nodes[p].kind else { return None };
        let at = kids.iter().position(|k| nodes.contains(k))?;
        let members: Vec<usize> = kids.iter().copied().filter(|k| nodes.contains(k)).collect();
        kids.retain(|k| !nodes.contains(k));
        kids.insert(at, g);
        self.tree.nodes.push(TreeNode::new(Kind::Group(members), Affine::IDENTITY));
        Some(g)
    }

    /// Move group `g`'s children up into its parent, where it was, keeping where they are in
    /// the world (and hidden if it was). False if `g` is the root or not a group.
    pub fn ungroup(&mut self, g: usize) -> bool {
        let Some(p) = self.tree.parents()[g] else { return false };
        let Kind::Group(members) = self.tree.nodes[g].kind.clone() else { return false };
        let (local, hidden) = (self.tree.nodes[g].local, self.tree.nodes[g].hidden);
        for &m in &members {
            let n = &mut self.tree.nodes[m];
            n.local = local.then(&n.local);
            n.hidden |= hidden;
        }
        let Kind::Group(kids) = &mut self.tree.nodes[p].kind else { return false };
        let Some(at) = kids.iter().position(|&k| k == g) else { return false };
        kids.splice(at..=at, members);
        true
    }

    pub fn set_name(&mut self, i: usize, name: Option<&str>) {
        self.tree.nodes[i].name = name.map(|n| n.as_bytes().to_vec());
    }

    pub fn set_hidden(&mut self, i: usize, hidden: bool) {
        self.tree.nodes[i].hidden = hidden;
    }

    pub fn set_layer(&mut self, i: usize, layer: i32) {
        self.tree.nodes[i].layer = layer;
    }

    /// Add a layer named `name`; returns its id.
    pub fn add_layer(&mut self, name: &str) -> i32 {
        let id = self.layers().iter().map(|l| l.id + 1).max().unwrap_or(0);
        self.tree.layers.push(LayerInfo { id, name: Some(name.as_bytes().to_vec()), hidden: false });
        id
    }

    pub fn layer_mut(&mut self, id: i32) -> Option<&mut LayerInfo> {
        self.tree.layers.iter_mut().find(|l| l.id == id)
    }

    /// Every layer by id: the file's `LAYR` entries and any layer an object is on without
    /// one (unnamed, shown).
    pub fn layers(&self) -> Vec<LayerInfo> {
        let mut out = self.tree.layers.clone();
        for i in self.objects() {
            let id = self.tree.nodes[i].layer;
            if id >= 0 && !out.iter().any(|l| l.id == id) {
                out.push(LayerInfo { id, name: None, hidden: false });
            }
        }
        out.sort_by_key(|l| l.id);
        out
    }

    /// Layer `id`'s entry, added (unnamed, shown) when the file has none, so saving writes it.
    fn layer_entry(&mut self, id: i32) -> &mut LayerInfo {
        if !self.tree.layers.iter().any(|l| l.id == id) {
            self.tree.layers.push(LayerInfo { id, name: None, hidden: false });
        }
        self.layer_mut(id).unwrap()
    }

    pub fn set_layer_hidden(&mut self, id: i32, hidden: bool) {
        self.layer_entry(id).hidden = hidden;
    }

    /// Name layer `id`; `None` clears the name.
    pub fn set_layer_name(&mut self, id: i32, name: Option<&str>) {
        self.layer_entry(id).name = name.map(|n| n.as_bytes().to_vec());
    }

    /// Put node `i` and every node under it on layer `id`.
    pub fn put_on_layer(&mut self, i: usize, id: i32) {
        self.tree.nodes[i].layer = id;
        if let Kind::Group(kids) = self.tree.nodes[i].kind.clone() {
            for k in kids {
                self.put_on_layer(k, id);
            }
        }
    }

    /// A model's voxels in the file's form: (x, y, z, index), sorted by position.
    pub fn vox_voxels(&self, m: usize) -> Vec<[u16; 4]> {
        let map = &self.models[m];
        let size = vox_size(map);
        let mut out = Vec::with_capacity(map.voxels as usize);
        map.for_each(|u, c| {
            let v = local_to_vox(size, u);
            out.push([v[0] as u16, v[1] as u16, v[2] as u16, c as u16]);
        });
        out.sort_unstable();
        out
    }

    /// Write the world into `doc`, the file it was opened from: only models whose size or
    /// voxels differ from the file's, new models and changed nodes and layers are written.
    pub fn save_into(&self, doc: &mut Document) -> Result<WorldSave, String> {
        let file = doc.scene().map_err(|e| e.to_string())?;
        let mut edits = ModelEdits::default();
        for m in 0..self.models.len() {
            let size = vox_size(&self.models[m]);
            let voxels = self.vox_voxels(m);
            match file.models.get(m) {
                Some(f)
                    if f.size == size
                        && f.effective_voxels().iter().map(|v| v.map(u16::from)).eq(voxels.iter().copied()) => {}
                Some(_) => edits.set.push((m, size, voxels)),
                None => edits.add.push((size, voxels)),
            }
        }
        doc.set_tree(&self.tree, &edits).map_err(|e| e.to_string())
    }
}

/// One shown object placed in the world: its model, transform and the box it fills.
pub(crate) struct Placed {
    object: usize,
    model: usize,
    at: Affine,
    lo: [i32; 3],
    hi: [i32; 3],
}

/// The shown objects in the order that wins overlaps (see [`World::stack`]).
pub(crate) struct Stack {
    placed: Vec<Placed>,
}

impl Stack {
    /// The first object with a voxel at vxr world cell `q`, and its palette index.
    fn hit(&self, w: &World, q: [i32; 3]) -> Option<(usize, u8)> {
        let v = world_to_vox(q);
        self.placed.iter().find_map(|p| {
            if (0..3).any(|a| q[a] < p.lo[a] || q[a] > p.hi[a]) {
                return None;
            }
            let map = &w.models[p.model];
            let size = vox_size(map);
            let l = local_of(&p.at, size, v);
            if (0..3).any(|a| l[a] < 0 || l[a] >= size[a]) {
                return None;
            }
            let u = vox_to_local(size, l);
            Some((p.object, map.get(u[0], u[1], u[2]))).filter(|&(_, c)| c != 0)
        })
    }

    /// Only the objects whose box meets the box `lo` to `hi` (inclusive), for many lookups inside it.
    pub(crate) fn within(mut self, lo: [i32; 3], hi: [i32; 3]) -> Stack {
        self.placed.retain(|p| (0..3).all(|a| p.lo[a] <= hi[a] && p.hi[a] >= lo[a]));
        self
    }

    /// Whether every object here is one of `objects`.
    pub(crate) fn holds_only(&self, objects: &[usize]) -> bool {
        self.placed.iter().all(|p| objects.contains(&p.object))
    }

    pub(crate) fn object_at(&self, w: &World, q: [i32; 3]) -> Option<usize> {
        self.hit(w, q).map(|(o, _)| o)
    }

    /// The palette index showing at vxr world cell `q`, 0 for none.
    pub(crate) fn color_at(&self, w: &World, q: [i32; 3]) -> u8 {
        self.hit(w, q).map_or(0, |(_, c)| c)
    }
}
