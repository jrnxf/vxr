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
use vxr_vox::tree::{Kind, LayerInfo, ModelEdits, Tree, TreeNode};
use vxr_vox::{Document, WorldSave};

use crate::brickmap::BrickMap;
use crate::scene::{MAX_EXTENT, vox_to_world, world_to_vox};

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
        let mut models = Vec::with_capacity(s.models.len());
        for m in &s.models {
            if m.size.iter().any(|&k| k < 1 || k as u32 > MAX_EXTENT) {
                return Err(format!("a model is {:?} voxels; each side must be 1 to {MAX_EXTENT}", m.size));
            }
            let mut map = BrickMap::new([m.size[0] as u32, m.size[2] as u32, m.size[1] as u32]);
            for v in m.effective_voxels() {
                let u = vox_to_local(m.size, [v[0] as i32, v[1] as i32, v[2] as i32]);
                map.set(u[0], u[1], u[2], v[3]);
            }
            models.push(map);
        }
        Ok(World { tree, models, version: s.version })
    }

    /// Objects (tree nodes holding a model) in tree order.
    pub fn objects(&self) -> Vec<usize> {
        self.tree.order().into_iter().filter(|&i| matches!(self.tree.nodes[i].kind, Kind::Object(_))).collect()
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
        let parents = self.tree.parents();
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
        let parents = self.tree.parents();
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
        let mut objects = self.objects();
        if self.version < 200 {
            objects.reverse();
        }
        for i in objects {
            if shown_only && !self.shown(i) {
                continue;
            }
            let Some(m) = self.model_of(i) else { continue };
            let w = self.world_transform(i);
            let map = &self.models[m];
            let size = vox_size(map);
            map.for_each(|u, c| {
                let v = local_to_vox(size, u);
                f(i, vox_to_world(vxr_vox::Scene::world_cell(&w, size, v)), c);
            });
        }
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
        let m = self.model_of(i)?;
        let size = vox_size(&self.models[m]);
        let w = self.world_transform(i);
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
        let (w, s) = (self.world_transform(i), vox_size(&self.models[m]));
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

    /// Move node `i` by `d` world cells (vxr axes).
    pub fn move_by(&mut self, i: usize, d: [i32; 3]) {
        let parent = self.tree.parents()[i].map_or(Affine::IDENTITY, |p| self.world_transform(p));
        let local = mat_vec(&transpose(&parent.r), world_to_vox(d));
        let t = &mut self.tree.nodes[i].local.t;
        *t = std::array::from_fn(|a| t[a] + local[a]);
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
        let id = self.tree.layers.iter().map(|l| l.id + 1).max().unwrap_or(0);
        self.tree.layers.push(LayerInfo { id, name: Some(name.as_bytes().to_vec()), hidden: false });
        id
    }

    pub fn layer_mut(&mut self, id: i32) -> Option<&mut LayerInfo> {
        self.tree.layers.iter_mut().find(|l| l.id == id)
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
