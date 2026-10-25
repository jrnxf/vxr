//! The scene graph as an editable tree. An object is an nTRN over an nSHP, a group an nTRN
//! over an nGRP; each keeps its name, hidden flag, layer and base transform. The layers
//! (`LAYR`) come with it. [`Document::tree`] reads it, and [`Document::set_tree`] writes an
//! edited tree (and edited or new models) back, touching only the chunks whose meaning
//! changed, so names, keyframes, unknown keys and every other chunk keep their bytes.

use std::collections::{BTreeMap, HashSet};

use crate::build::{frame, world_blocks};
use crate::bytes::Dict;
use crate::chunk::{self, Layr, Ngrp, Nshp, Ntrn, Size, Tdcz, Typed, Xyzi};
use crate::doc::{Document, WorldSave};
use crate::raw::Chunk;
use crate::scene::{Affine, IDENTITY_R, MAX_DEPTH, MAX_VISITS, Node, Scene, rotation_to_byte};
use crate::{Error, Result};

/// What a tree node holds: child nodes (by index into [`Tree::nodes`]) or one model.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Kind {
    Group(Vec<usize>),
    /// A model index: the file's models first, then [`ModelEdits::add`] in order.
    Object(usize),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TreeNode {
    /// `_name`, as bytes so names that are not UTF-8 survive.
    pub name: Option<Vec<u8>>,
    pub hidden: bool,
    pub layer: i32,
    /// The base transform (see [`Scene::base_transform`]), relative to the parent.
    pub local: Affine,
    pub kind: Kind,
    /// The nTRN and the nGRP or nSHP this node was read from, or None for a new node.
    pub ids: Option<(i32, i32)>,
}

impl TreeNode {
    pub fn new(kind: Kind, local: Affine) -> TreeNode {
        TreeNode { name: None, hidden: false, layer: 0, local, kind, ids: None }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LayerInfo {
    pub id: i32,
    pub name: Option<Vec<u8>>,
    pub hidden: bool,
}

/// Node 0 is the root group (nTRN 0 over the top nGRP). Only nodes reachable from it
/// count; a node may be reached once (a tree, not a graph).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Tree {
    pub nodes: Vec<TreeNode>,
    pub layers: Vec<LayerInfo>,
}

/// Model data for [`Document::set_tree`]: new contents for file models (index, size,
/// voxels) and models to add, in MagicaVoxel's z-up model coordinates. Voxels are
/// (x, y, z, palette index); coordinates are wider than a byte because a model over 256 on
/// an axis is allowed here (it is saved as blocks).
#[derive(Clone, Debug, Default)]
pub struct ModelEdits {
    pub set: Vec<(usize, [i32; 3], Vec<[u16; 4]>)>,
    pub add: Vec<([i32; 3], Vec<[u16; 4]>)>,
}

/// Largest model MagicaVoxel reads, per axis.
pub const MAX_MODEL: i32 = 256;

impl Tree {
    /// Read the tree from a parsed scene. A file without a scene graph shows model 0, so
    /// it becomes a root with that one object (neither has ids). Nodes reached a second
    /// time (shared by several parents) become new nodes, so each object can be edited on
    /// its own; hostile graphs are cut off as in [`Scene::instances`].
    pub fn read(s: &Scene) -> Tree {
        let layers = s
            .layers
            .iter()
            .map(|l| LayerInfo {
                id: l.id,
                name: l.attrs.get("_name").map(<[u8]>::to_vec),
                hidden: l.attrs.is_true("_hidden"),
            })
            .collect();
        let mut t = Tree { nodes: vec![TreeNode::new(Kind::Group(vec![]), Affine::IDENTITY)], layers };
        if s.nodes.is_empty() {
            if !s.models.is_empty() {
                t.nodes.push(TreeNode::new(Kind::Object(0), Affine::IDENTITY));
                t.nodes[0].kind = Kind::Group(vec![1]);
            }
            return t;
        }
        let mut r = Reader { s, seen: HashSet::new(), budget: MAX_VISITS };
        match (s.nodes.get(&0), child_of(s, 0)) {
            (Some(Node::Transform(n)), Some(Node::Group(g))) => {
                r.seen.insert(0);
                r.seen.insert(g.id);
                t.nodes[0] = node_from(n, Kind::Group(vec![]), Some((0, g.id)));
                let mut kids = Vec::new();
                for &c in &g.children {
                    if let Some(k) = r.add(&mut t, c, 1, false) {
                        kids.push(k);
                    }
                }
                t.nodes[0].kind = Kind::Group(kids);
            }
            _ => {
                if let Some(k) = r.add(&mut t, 0, 1, false) {
                    t.nodes[0].kind = Kind::Group(vec![k]);
                }
            }
        }
        t
    }

    pub fn children(&self, i: usize) -> &[usize] {
        match &self.nodes[i].kind {
            Kind::Group(c) => c,
            Kind::Object(_) => &[],
        }
    }

    /// Reachable nodes in depth-first order from the root (the root first), each once.
    pub fn order(&self) -> Vec<usize> {
        let mut out = Vec::new();
        let mut seen = vec![false; self.nodes.len()];
        let mut stack = vec![0];
        while let Some(i) = stack.pop() {
            if i >= self.nodes.len() || std::mem::replace(&mut seen[i], true) {
                continue;
            }
            out.push(i);
            stack.extend(self.children(i).iter().rev());
        }
        out
    }

    /// Parent of each reachable node (the root has none).
    pub fn parents(&self) -> Vec<Option<usize>> {
        let mut p = vec![None; self.nodes.len()];
        for i in self.order() {
            for &c in self.children(i) {
                p[c] = Some(i);
            }
        }
        p
    }

    /// Check that the tree can be written: node 0 is a group, children exist and are each
    /// reached once, and objects name a model below `models`.
    pub fn validate(&self, models: usize) -> Result<()> {
        if !matches!(self.nodes.first().map(|n| &n.kind), Some(Kind::Group(_))) {
            return Err(Error::new("tree root is not a group"));
        }
        let mut seen = vec![false; self.nodes.len()];
        let mut stack = vec![0usize];
        while let Some(i) = stack.pop() {
            if std::mem::replace(&mut seen[i], true) {
                return Err(Error::new(format!("tree node {i} is reached twice")));
            }
            match &self.nodes[i].kind {
                Kind::Group(c) => {
                    for &k in c {
                        if k >= self.nodes.len() || k == 0 {
                            return Err(Error::new(format!("tree node {i} has a bad child {k}")));
                        }
                        stack.push(k);
                    }
                }
                Kind::Object(m) if *m >= models => {
                    return Err(Error::new(format!("tree node {i} names missing model {m}")));
                }
                Kind::Object(_) => {}
            }
        }
        Ok(())
    }

    /// The tree without file ids, in depth-first order, for comparing a written tree with
    /// the one asked for.
    fn shape(&self) -> Vec<NodeShape> {
        self.order()
            .into_iter()
            .map(|i| {
                let n = &self.nodes[i];
                let k = match &n.kind {
                    Kind::Group(c) => Err(c.len()),
                    Kind::Object(m) => Ok(*m),
                };
                (n.name.clone(), n.hidden, n.layer, n.local, k)
            })
            .collect()
    }
}

/// A node without its file ids: name, hidden, layer, transform, and its model (`Ok`) or
/// its number of children (`Err`).
type NodeShape = (Option<Vec<u8>>, bool, i32, Affine, std::result::Result<usize, usize>);

struct Reader<'a> {
    s: &'a Scene,
    seen: HashSet<i32>,
    budget: usize,
}

impl Reader<'_> {
    /// Add nTRN `id` and what it holds; `fresh` gives the node and its subtree no ids.
    fn add(&mut self, t: &mut Tree, id: i32, depth: usize, fresh: bool) -> Option<usize> {
        if depth > MAX_DEPTH || self.budget == 0 {
            return None;
        }
        self.budget -= 1;
        let Some(Node::Transform(n)) = self.s.nodes.get(&id) else { return None };
        let child = self.s.nodes.get(&n.child)?;
        let fresh = fresh || !self.seen.insert(id) || !self.seen.insert(n.child);
        let ids = (!fresh).then_some((id, n.child));
        match child {
            Node::Transform(_) => None,
            Node::Shape(sh) => {
                let &(m, _) = sh.models.first()?;
                if m < 0 || m as usize >= self.s.models.len() {
                    return None;
                }
                t.nodes.push(node_from(n, Kind::Object(m as usize), ids));
                Some(t.nodes.len() - 1)
            }
            Node::Group(g) => {
                t.nodes.push(node_from(n, Kind::Group(vec![]), ids));
                let me = t.nodes.len() - 1;
                let mut kids = Vec::new();
                for &c in &g.children {
                    if let Some(k) = self.add(t, c, depth + 1, fresh) {
                        kids.push(k);
                    }
                }
                t.nodes[me].kind = Kind::Group(kids);
                Some(me)
            }
        }
    }
}

fn child_of(s: &Scene, trn: i32) -> Option<&Node> {
    match s.nodes.get(&trn) {
        Some(Node::Transform(n)) => s.nodes.get(&n.child),
        _ => None,
    }
}

fn node_from(n: &Ntrn, kind: Kind, ids: Option<(i32, i32)>) -> TreeNode {
    TreeNode {
        name: n.attrs.get("_name").map(<[u8]>::to_vec),
        hidden: n.attrs.is_true("_hidden"),
        layer: n.layer,
        local: Scene::base_transform(n, &mut Vec::new()),
        kind,
        ids,
    }
}

/// Write `a` into a dict as `rkey` (left out when identity) and `tkey` (left out when zero
/// and not there before).
fn set_rt(d: &mut Dict, rkey: &str, tkey: &str, a: &Affine) {
    if a.r == IDENTITY_R {
        d.remove(rkey);
    } else if let Some(b) = rotation_to_byte(&a.r) {
        d.set(rkey, b.to_string());
    }
    if a.t != [0, 0, 0] || d.has(tkey) {
        d.set(tkey, format!("{} {} {}", a.t[0], a.t[1], a.t[2]));
    }
}

/// Make nTRN `n` say what `want` says, changing only what differs. Returns whether it changed.
fn patch_trn(n: &mut Ntrn, want: &TreeNode) -> bool {
    let before = n.clone();
    if n.attrs.get("_name") != want.name.as_deref() {
        match &want.name {
            Some(v) => n.attrs.set("_name", v.clone()),
            None => n.attrs.remove("_name"),
        }
    }
    if n.attrs.is_true("_hidden") != want.hidden {
        if want.hidden {
            n.attrs.set("_hidden", "1");
        } else {
            n.attrs.remove("_hidden");
        }
    }
    n.layer = want.layer;
    if Scene::base_transform(n, &mut Vec::new()) != want.local {
        if n.frames.is_empty() {
            n.frames.push(Dict::default());
        }
        set_rt(&mut n.frames[0], "_r", "_t", &want.local);
        if n.attrs.has("_frm_r") || n.attrs.has("_frm_t") {
            set_rt(&mut n.attrs, "_frm_r", "_frm_t", &want.local);
        }
    }
    *n != before
}

fn new_trn(id: i32, child: i32, n: &TreeNode) -> Ntrn {
    let mut attrs = Dict::default();
    if let Some(v) = &n.name {
        attrs.set("_name", v.clone());
    }
    if n.hidden {
        attrs.set("_hidden", "1");
    }
    let r = (n.local.r != IDENTITY_R).then_some(n.local.r);
    Ntrn { id, attrs, child, reserved: -1, layer: n.layer, frames: vec![frame(r, n.local.t)], extra: vec![] }
}

fn model_chunks(size: [i32; 3], voxels: Vec<[u8; 4]>) -> [Chunk; 2] {
    [
        Chunk::new(chunk::SIZE, Size { x: size[0], y: size[1], z: size[2], extra: vec![] }.encode()),
        Chunk::new(chunk::XYZI, Xyzi { voxels, extra: vec![] }.encode()),
    ]
}

/// Voxels inside `size`, later duplicates winning, index 0 dropped, sorted: the same
/// normal form as [`crate::scene::Model::effective_voxels`].
fn normal(size: [i32; 3], voxels: &[[u16; 4]]) -> Vec<[u16; 4]> {
    let mut m: BTreeMap<[u16; 3], u16> = BTreeMap::new();
    for v in voxels {
        if (0..3).all(|k| (v[k] as i32) < size[k]) {
            m.insert([v[0], v[1], v[2]], v[3]);
        }
    }
    m.into_iter().filter(|(_, c)| *c != 0).map(|(p, c)| [p[0], p[1], p[2], c]).collect()
}

/// Voxels of a model of at most 256 per axis, in a byte each.
fn narrow(voxels: &[[u16; 4]]) -> Vec<[u8; 4]> {
    voxels.iter().map(|v| v.map(|x| x as u8)).collect()
}

fn widen(voxels: &[[u8; 4]]) -> Vec<[u16; 4]> {
    voxels.iter().map(|v| v.map(u16::from)).collect()
}

impl Document {
    pub fn tree(&self) -> Result<Tree> {
        Ok(Tree::read(&self.scene()?))
    }

    /// Make the file's scene graph, layers and models say what `tree` and `models` say.
    /// Nodes read from the file are patched in place (only the keys that differ), new nodes
    /// and models are appended after the last node and model chunks, nodes no longer in the
    /// tree are dropped, and every other chunk keeps its bytes; an unchanged tree changes
    /// nothing. The result is checked by reading it back; if the tree cannot be written that
    /// way (a file without a scene graph, a model over 256 on an axis), the models and scene
    /// graph are rebuilt from it instead (see [`WorldSave`]).
    pub fn set_tree(&mut self, tree: &Tree, models: &ModelEdits) -> Result<WorldSave> {
        let scene = self.scene()?;
        let old = Tree::read(&scene);
        let total = scene.models.len() + models.add.len();
        tree.validate(total)?;
        for (i, size, _) in &models.set {
            if *i >= scene.models.len() || size.iter().any(|&s| s < 1) {
                return Err(Error::new(format!("bad model edit for model {i}")));
            }
        }
        if old == *tree && models.set.is_empty() && models.add.is_empty() {
            return Ok(WorldSave::Kept);
        }
        let mut kept = self.clone();
        if kept.patch_tree(&scene, &old, tree, models).is_ok() && kept.check_tree(&scene, tree, models) {
            *self = kept;
            return Ok(WorldSave::Kept);
        }
        self.rebuild_tree(&scene, tree, models)?;
        Ok(WorldSave::Rebuilt)
    }

    fn patch_tree(&mut self, scene: &Scene, old: &Tree, tree: &Tree, models: &ModelEdits) -> Result<()> {
        let fits = |s: &[i32; 3]| s.iter().all(|&k| (1..=MAX_MODEL).contains(&k));
        if !models.set.iter().all(|(_, s, _)| fits(s)) || !models.add.iter().all(|(s, _)| fits(s)) {
            return Err(Error::new("a model is over 256 on an axis"));
        }
        let chunks = &mut self.raw.chunks;
        for (i, size, voxels) in &models.set {
            let m = &scene.models[*i];
            if *size != m.size {
                chunks[m.size_chunk].content = Size { x: size[0], y: size[1], z: size[2], extra: vec![] }.encode();
            }
            let c = &mut chunks[m.data_chunk];
            c.content = match Typed::parse(c)? {
                Typed::Tdcz(_) => {
                    let (sx, sy) = (size[0] as usize, size[1] as usize);
                    let mut dense = vec![0u8; sx * sy * size[2] as usize];
                    for v in normal(*size, voxels) {
                        dense[v[0] as usize + sx * (v[1] as usize + sy * v[2] as usize)] = v[3] as u8;
                    }
                    Tdcz::encode_dense(size[0], size[1], size[2], &dense).encode()
                }
                Typed::Xyzi(x) => Xyzi { voxels: narrow(&normal(*size, voxels)), extra: x.extra }.encode(),
                _ => unreachable!("model data chunk is XYZI or TDCZ"),
            };
        }
        let new_models: Vec<Chunk> =
            models.add.iter().flat_map(|(size, voxels)| model_chunks(*size, narrow(&normal(*size, voxels)))).collect();

        let mut new_nodes = Vec::new();
        let mut drop: HashSet<usize> = HashSet::new();
        if old.nodes != tree.nodes {
            if tree.nodes[0].ids.is_none() {
                return Err(Error::new("the file has no scene graph root to edit"));
            }
            // Ids for new nodes, two each (nTRN, then nGRP or nSHP).
            let mut next = scene.nodes.keys().next_back().map_or(0, |k| k + 1);
            let order = tree.order();
            let mut ids: Vec<(i32, i32)> = vec![(-1, -1); tree.nodes.len()];
            for &i in &order {
                ids[i] = tree.nodes[i].ids.unwrap_or_else(|| {
                    next += 2;
                    (next - 2, next - 1)
                });
            }
            let live: HashSet<i32> = order.iter().flat_map(|&i| [ids[i].0, ids[i].1]).collect();
            for &i in &order {
                let n = &tree.nodes[i];
                let (t, c) = ids[i];
                let child = match &n.kind {
                    Kind::Group(kids) => Typed::Ngrp(Ngrp {
                        id: c,
                        attrs: Dict::default(),
                        children: kids.iter().map(|&k| ids[k].0).collect(),
                        extra: vec![],
                    }),
                    Kind::Object(m) => Typed::Nshp(Nshp {
                        id: c,
                        attrs: Dict::default(),
                        models: vec![(*m as i32, Dict::default())],
                        extra: vec![],
                    }),
                };
                if n.ids.is_none() {
                    let trn = new_trn(t, c, n);
                    new_nodes.push(Chunk::new(chunk::NTRN, trn.encode()));
                    let id = if matches!(child, Typed::Ngrp(_)) { chunk::NGRP } else { chunk::NSHP };
                    new_nodes.push(Chunk::new(id, child.encode()));
                    continue;
                }
                let ti = scene.node_chunks[&t];
                let mut trn = Ntrn::parse(&chunks[ti].content)?;
                if patch_trn(&mut trn, n) {
                    chunks[ti].content = trn.encode();
                }
                let ci = scene.node_chunks[&c];
                match (Typed::parse(&chunks[ci])?, child) {
                    (Typed::Ngrp(mut g), Typed::Ngrp(want)) => {
                        if g.children != want.children {
                            g.children = want.children;
                            chunks[ci].content = g.encode();
                        }
                    }
                    (Typed::Nshp(mut s), Typed::Nshp(want)) => {
                        if s.models.first().map(|m| m.0) != Some(want.models[0].0) {
                            s.models[0].0 = want.models[0].0;
                            chunks[ci].content = s.encode();
                        }
                    }
                    _ => return Err(Error::new(format!("node {t} changed between group and object"))),
                }
            }
            for i in old.order() {
                if let Some((t, c)) = old.nodes[i].ids {
                    for id in [t, c] {
                        if !live.contains(&id) {
                            drop.insert(scene.node_chunks[&id]);
                        }
                    }
                }
            }
        }
        let after_models = chunks.iter().rposition(|c| c.is(chunk::XYZI) || c.is(chunk::TDCZ)).map_or(0, |i| i + 1);
        let after_nodes = chunks
            .iter()
            .rposition(|c| c.is(chunk::NTRN) || c.is(chunk::NGRP) || c.is(chunk::NSHP))
            .map_or(after_models, |i| i + 1);
        let mut out = Vec::with_capacity(chunks.len() + new_models.len() + new_nodes.len());
        let mut new_models = Some(new_models);
        let mut new_nodes = Some(new_nodes);
        for (i, c) in std::mem::take(chunks).into_iter().enumerate() {
            if i == after_models {
                out.extend(new_models.take().unwrap_or_default());
            }
            if i == after_nodes {
                out.extend(new_nodes.take().unwrap_or_default());
            }
            if !drop.contains(&i) {
                out.push(c);
            }
        }
        out.extend(new_models.take().unwrap_or_default());
        out.extend(new_nodes.take().unwrap_or_default());
        *chunks = out;
        self.set_layers(&tree.layers)
    }

    /// Patch or add `LAYR` chunks so each layer has its name and hidden flag. Layers the
    /// file has and `layers` does not are left alone.
    pub fn set_layers(&mut self, layers: &[LayerInfo]) -> Result<()> {
        for l in layers {
            let at = self
                .raw
                .chunks
                .iter()
                .position(|c| c.is(chunk::LAYR) && Layr::parse(&c.content).is_ok_and(|x| x.id == l.id));
            let mut x = match at {
                Some(i) => Layr::parse(&self.raw.chunks[i].content)?,
                None => Layr { id: l.id, attrs: Dict::default(), reserved: -1, extra: vec![] },
            };
            let before = x.clone();
            if x.attrs.get("_name") != l.name.as_deref() {
                match &l.name {
                    Some(v) => x.attrs.set("_name", v.clone()),
                    None => x.attrs.remove("_name"),
                }
            }
            if x.attrs.is_true("_hidden") != l.hidden {
                if l.hidden {
                    x.attrs.set("_hidden", "1");
                } else {
                    x.attrs.remove("_hidden");
                }
            }
            match at {
                Some(i) if x != before => self.raw.chunks[i].content = x.encode(),
                Some(_) => {}
                None => {
                    let chunks = &self.raw.chunks;
                    let pos = chunks
                        .iter()
                        .rposition(|c| c.is(chunk::LAYR))
                        .or_else(|| {
                            chunks.iter().rposition(|c| {
                                [chunk::NTRN, chunk::NGRP, chunk::NSHP, chunk::XYZI, chunk::TDCZ].contains(&&c.id)
                            })
                        })
                        .map_or(0, |i| i + 1);
                    self.raw.chunks.insert(pos, Chunk::new(chunk::LAYR, x.encode()));
                }
            }
        }
        Ok(())
    }

    /// Does the document now read back as `tree` with `models`?
    fn check_tree(&self, before: &Scene, tree: &Tree, models: &ModelEdits) -> bool {
        let Ok(s) = self.scene() else { return false };
        let back = Tree::read(&s);
        if back.shape() != tree.shape()
            || back.layers.iter().filter(|l| tree.layers.contains(l)).count() != tree.layers.len()
        {
            return false;
        }
        if s.models.len() != before.models.len() + models.add.len() {
            return false;
        }
        let same = |i: usize, size: [i32; 3], v: &[[u16; 4]]| {
            s.models[i].size == size && widen(&s.models[i].effective_voxels()) == normal(size, v)
        };
        models.set.iter().all(|(i, size, v)| same(*i, *size, v))
            && models.add.iter().enumerate().all(|(k, (size, v))| same(before.models.len() + k, *size, v))
    }

    /// Replace every model and scene graph chunk with ones written from `tree`, at the
    /// place the first of them was. Models over 256 on an axis are split into blocks, and
    /// each object using one becomes a group of those blocks with the object's name.
    fn rebuild_tree(&mut self, scene: &Scene, tree: &Tree, models: &ModelEdits) -> Result<()> {
        let mut all: Vec<([i32; 3], Vec<[u16; 4]>)> =
            scene.models.iter().map(|m| (m.size, widen(&m.effective_voxels()))).collect();
        for (i, size, v) in &models.set {
            all[*i] = (*size, normal(*size, v));
        }
        all.extend(models.add.iter().map(|(s, v)| (*s, normal(*s, v))));
        // Each tree model becomes one file model, or blocks (file model, translation).
        let mut geometry = Vec::new();
        let mut placed: Vec<Placed> = Vec::new();
        let mut count = 0usize;
        for (size, voxels) in all {
            if size.iter().all(|&s| s <= MAX_MODEL) {
                geometry.extend(model_chunks(size, narrow(&voxels)));
                placed.push(Placed::One(count));
                count += 1;
                continue;
            }
            let pivot = size.map(|s| s / 2);
            let cells: BTreeMap<[i32; 3], u8> = voxels
                .iter()
                .map(|v| ([v[0] as i32 - pivot[0], v[1] as i32 - pivot[1], v[2] as i32 - pivot[2]], v[3] as u8))
                .collect();
            let mut blocks = Vec::new();
            for (bs, bv, bt) in world_blocks(&cells) {
                geometry.extend(model_chunks(bs, bv));
                blocks.push((count, bt));
                count += 1;
            }
            placed.push(Placed::Blocks(blocks));
        }
        let mut next = 0;
        let mut nodes = Vec::new();
        emit_tree(tree, 0, &placed, &mut next, &mut nodes);
        geometry.extend(nodes);
        let is_geometry =
            |id: &[u8; 4]| [chunk::SIZE, chunk::XYZI, chunk::TDCZ, chunk::NTRN, chunk::NGRP, chunk::NSHP].contains(&id);
        let at = self.raw.chunks.iter().position(|c| is_geometry(&c.id)).unwrap_or(0);
        let before = self.raw.chunks[..at].iter().filter(|c| !is_geometry(&c.id)).count();
        self.raw.chunks.retain(|c| !is_geometry(&c.id));
        self.raw.chunks.splice(before..before, geometry);
        self.set_layers(&tree.layers)
    }
}

/// Where a tree model went in a rebuilt file: one file model, or blocks (file model,
/// translation) when it is over 256 on an axis.
enum Placed {
    One(usize),
    Blocks(Vec<(usize, [i32; 3])>),
}

/// Write node `i` and its subtree with fresh ids from `next`; returns its nTRN id.
fn emit_tree(tree: &Tree, i: usize, placed: &[Placed], next: &mut i32, out: &mut Vec<Chunk>) -> i32 {
    let n = &tree.nodes[i];
    let (t, c) = (*next, *next + 1);
    *next += 2;
    out.push(Chunk::new(chunk::NTRN, new_trn(t, c, n).encode()));
    let at = out.len();
    let group = |c: i32, children: Vec<i32>| {
        Chunk::new(chunk::NGRP, Ngrp { id: c, attrs: Dict::default(), children, extra: vec![] }.encode())
    };
    match &n.kind {
        Kind::Group(kids) => {
            out.push(group(c, vec![]));
            let ids = kids.iter().map(|&k| emit_tree(tree, k, placed, next, out)).collect();
            out[at] = group(c, ids);
        }
        Kind::Object(m) => match &placed[*m] {
            Placed::One(fm) => {
                let shp =
                    Nshp { id: c, attrs: Dict::default(), models: vec![(*fm as i32, Dict::default())], extra: vec![] };
                out.push(Chunk::new(chunk::NSHP, shp.encode()));
            }
            Placed::Blocks(blocks) => {
                out.push(group(c, vec![]));
                let mut ids = Vec::new();
                for &(fm, bt) in blocks {
                    let (bt_id, bs_id) = (*next, *next + 1);
                    *next += 2;
                    ids.push(bt_id);
                    let block = TreeNode::new(Kind::Object(fm), Affine { r: IDENTITY_R, t: bt });
                    out.push(Chunk::new(chunk::NTRN, new_trn(bt_id, bs_id, &block).encode()));
                    let shp = Nshp {
                        id: bs_id,
                        attrs: Dict::default(),
                        models: vec![(fm as i32, Dict::default())],
                        extra: vec![],
                    };
                    out.push(Chunk::new(chunk::NSHP, shp.encode()));
                }
                out[at] = group(c, ids);
            }
        },
    }
    t
}
