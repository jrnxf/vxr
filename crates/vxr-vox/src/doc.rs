//! A `.vox` document: the raw chunks plus edits that re-encode only what they touch.

use std::collections::{BTreeMap, HashMap};

use crate::build::{Builder, frame, world_blocks};
use crate::bytes::Dict;
use crate::camera::Camera;
use crate::chunk::{self, IdDict, Ngrp, Nshp, Ntrn, Rgba, Size, Tdcz, Typed, Xyzi};
use crate::default_palette::DEFAULT_PALETTE;
use crate::material::Material;
use crate::raw::{Chunk, RawFile};
use crate::scene::{Affine, IDENTITY_R, Instance, Node, Rule, Scene, mat_vec};
use crate::{Error, Result};

/// How [`Document::set_world`] saved the voxels.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WorldSave {
    /// Edits went into the models that hold the edited cells; cells outside every model
    /// became new models under the root group. Every other chunk is untouched.
    Kept,
    /// The edits could not be placed exactly (a model shared by several instances, or an
    /// unusual scene graph root), so the models and scene graph were rebuilt from the
    /// world voxels. Every chunk that is not a model or a scene graph node is untouched.
    Rebuilt,
}

/// Is `id` a model or scene graph chunk, the ones [`Document::set_world`] may rebuild?
fn is_geometry(id: &[u8; 4]) -> bool {
    [chunk::SIZE, chunk::XYZI, chunk::TDCZ, chunk::NTRN, chunk::NGRP, chunk::NSHP].contains(&id)
}

/// Model cell of world cell `p` under instance world transform `world`, the inverse of
/// [`Scene::world_cell`]. `None` when it falls outside the model.
fn model_cell(world: &Affine, size: [i32; 3], p: [i32; 3]) -> Option<[u8; 3]> {
    // world_cell maps the doubled centre c2 = 2v + 1 - 2 floor(s/2) to 2p + 1 = r c2 + 2t.
    // r is a signed permutation, so its inverse is its transpose.
    let d = [0, 1, 2].map(|k| 2 * p[k] as i64 + 1 - 2 * world.t[k] as i64);
    let mut v = [0u8; 3];
    for k in 0..3 {
        let c2: i64 = (0..3).map(|i| world.r[i][k] as i64 * d[i]).sum();
        let x = (c2 - 1 + 2 * (size[k] / 2) as i64).div_euclid(2);
        if x < 0 || x >= size[k] as i64 {
            return None;
        }
        v[k] = x as u8;
    }
    Some(v)
}

fn transpose(r: &[[i32; 3]; 3]) -> [[i32; 3]; 3] {
    [0, 1, 2].map(|i| [0, 1, 2].map(|j| r[j][i]))
}

#[derive(Clone, Debug)]
pub struct Document {
    pub raw: RawFile,
}

impl Document {
    /// Parse the raw layer and check that every known chunk parses semantically.
    pub fn read(bytes: &[u8]) -> Result<Document> {
        let raw = RawFile::parse(bytes)?;
        Scene::from_raw(&raw)?;
        Ok(Document { raw })
    }

    pub fn write(&self) -> Vec<u8> {
        self.raw.write()
    }

    pub fn scene(&self) -> Result<Scene> {
        Scene::from_raw(&self.raw)
    }

    /// Set (or with colour 0, clear) one voxel of model `model`. Touches only that
    /// model's XYZI (or TDCZ) chunk.
    pub fn set_voxel(&mut self, model: usize, p: [u8; 3], color: u8) -> Result<usize> {
        let scene = self.scene()?;
        let m = scene.models.get(model).ok_or_else(|| Error::new(format!("no model {model}")))?;
        if (0..3).any(|k| p[k] as i32 >= m.size[k]) {
            return Err(Error::new(format!("voxel {p:?} outside model size {:?}", m.size)));
        }
        let idx = m.data_chunk;
        let c = &mut self.raw.chunks[idx];
        match Typed::parse(c)? {
            Typed::Xyzi(mut x) => {
                match (x.voxels.iter().rposition(|v| v[..3] == p), color) {
                    (Some(_), 0) => x.voxels.retain(|v| v[..3] != p),
                    (Some(i), col) => x.voxels[i][3] = col,
                    (None, 0) => {}
                    (None, col) => x.voxels.push([p[0], p[1], p[2], col]),
                }
                c.content = Xyzi::encode(&x);
            }
            Typed::Tdcz(t) => {
                let mut dense = t.decode()?;
                let (sx, sy) = (t.x as usize, t.y as usize);
                dense[p[0] as usize + sx * (p[1] as usize + sy * p[2] as usize)] = color;
                c.content = Tdcz::encode_dense(t.x, t.y, t.z, &dense).encode();
            }
            _ => unreachable!("model data chunk is XYZI or TDCZ"),
        }
        Ok(idx)
    }

    /// Recolour palette index `index` (1 to 255). Touches only the RGBA chunk; adds one
    /// (from the default palette) after the last model if the file has none.
    pub fn set_palette(&mut self, index: u8, rgba: [u8; 4]) -> Result<usize> {
        if index == 0 {
            return Err(Error::new("palette index 0 is the empty voxel"));
        }
        let slot = index as usize - 1;
        if let Some(i) = self.raw.chunks.iter().position(|c| c.is(chunk::RGBA)) {
            let Typed::Rgba(mut p) = Typed::parse(&self.raw.chunks[i])? else { unreachable!() };
            if slot >= p.colors.len() {
                return Err(Error::new(format!("RGBA chunk has only {} entries", p.colors.len())));
            }
            p.colors[slot] = rgba;
            self.raw.chunks[i].content = p.encode();
            return Ok(i);
        }
        let mut colors = DEFAULT_PALETTE.to_vec();
        colors[slot] = rgba;
        let at = self
            .raw
            .chunks
            .iter()
            .rposition(|c| c.is(chunk::XYZI) || c.is(chunk::TDCZ))
            .map(|i| i + 1)
            .unwrap_or(self.raw.chunks.len());
        self.raw.chunks.insert(at, Chunk::new(chunk::RGBA, Rgba { colors, extra: vec![] }.encode()));
        Ok(at)
    }

    /// The material of palette index `index` (1 to 255): its `MATL` dict, or the default.
    pub fn material(&self, index: u8) -> Material {
        self.raw
            .chunks
            .iter()
            .filter(|c| c.is(chunk::MATL))
            .filter_map(|c| IdDict::parse(chunk::MATL, &c.content).ok())
            .find(|m| m.id == index as i32)
            .map_or_else(Material::default, |m| Material::from_dict(&m.dict))
    }

    /// Set the material of palette index `index` (1 to 255). Touches only that index's
    /// `MATL` chunk, and in it only the keys of the material's type; adds a chunk after the
    /// last `MATL` (or the palette) if the file has none for it.
    pub fn set_material(&mut self, index: u8, m: &Material) -> Result<usize> {
        if index == 0 {
            return Err(Error::new("palette index 0 is the empty voxel"));
        }
        for (i, c) in self.raw.chunks.iter_mut().enumerate() {
            if !c.is(chunk::MATL) {
                continue;
            }
            let mut d = IdDict::parse(chunk::MATL, &c.content)?;
            if d.id != index as i32 {
                continue;
            }
            m.apply_to(&mut d.dict);
            c.content = d.encode();
            return Ok(i);
        }
        let chunks = &self.raw.chunks;
        let at = chunks
            .iter()
            .rposition(|c| c.is(chunk::MATL))
            .or_else(|| chunks.iter().rposition(|c| c.is(chunk::RGBA) || c.is(chunk::IMAP)))
            .map_or(chunks.len(), |i| i + 1);
        let d = IdDict { id: index as i32, dict: m.to_dict(), extra: vec![] };
        self.raw.chunks.insert(at, Chunk::new(chunk::MATL, d.encode()));
        Ok(at)
    }

    /// The saved cameras, in file order. A camera id that appears twice is read from its
    /// first `rCAM`, as [`Document::camera`] and [`Document::set_camera`] see it.
    pub fn cameras(&self) -> Vec<Camera> {
        let mut out: Vec<Camera> = Vec::new();
        for d in self.raw.chunks.iter().filter(|c| c.is(chunk::RCAM)) {
            if let Ok(d) = IdDict::parse(chunk::RCAM, &d.content)
                && !out.iter().any(|c| c.id == d.id)
            {
                out.push(Camera::from_dict(d.id, &d.dict));
            }
        }
        out
    }

    /// The saved camera `id`, if the file has one.
    pub fn camera(&self, id: i32) -> Option<Camera> {
        self.raw
            .chunks
            .iter()
            .filter(|c| c.is(chunk::RCAM))
            .filter_map(|c| IdDict::parse(chunk::RCAM, &c.content).ok())
            .find(|d| d.id == id)
            .map(|d| Camera::from_dict(id, &d.dict))
    }

    /// Set saved camera `c.id`. Touches only that camera's `rCAM` chunk, and in it only the
    /// keys whose value changed; adds a chunk after the last `rCAM` (or the render settings,
    /// materials or palette) if the file has none for it.
    pub fn set_camera(&mut self, c: &Camera) -> Result<usize> {
        for (i, ch) in self.raw.chunks.iter_mut().enumerate() {
            if !ch.is(chunk::RCAM) {
                continue;
            }
            let mut d = IdDict::parse(chunk::RCAM, &ch.content)?;
            if d.id != c.id {
                continue;
            }
            c.apply_to(&mut d.dict);
            ch.content = d.encode();
            return Ok(i);
        }
        let chunks = &self.raw.chunks;
        let at = chunks
            .iter()
            .rposition(|c| c.is(chunk::RCAM))
            .or_else(|| {
                chunks.iter().rposition(|c| [chunk::ROBJ, chunk::MATL, chunk::RGBA, chunk::IMAP].contains(&&c.id))
            })
            .map_or(chunks.len(), |i| i + 1);
        let d = IdDict { id: c.id, dict: c.to_dict(), extra: vec![] };
        self.raw.chunks.insert(at, Chunk::new(chunk::RCAM, d.encode()));
        Ok(at)
    }

    /// Move transform node `id` to translation `t`: frame 0's `_t`, and `_frm_t` too
    /// if the node has a base transform. Touches only that nTRN chunk.
    pub fn set_translation(&mut self, id: i32, t: [i32; 3]) -> Result<usize> {
        let i = self
            .raw
            .chunks
            .iter()
            .position(|c| c.is(chunk::NTRN) && Ntrn::parse(&c.content).map(|n| n.id == id).unwrap_or(false))
            .ok_or_else(|| Error::new(format!("no nTRN with id {id}")))?;
        let mut n = Ntrn::parse(&self.raw.chunks[i].content)?;
        let v = format!("{} {} {}", t[0], t[1], t[2]);
        if n.frames.is_empty() {
            n.frames.push(Default::default());
        }
        n.frames[0].set("_t", v.clone());
        if n.attrs.has("_frm_t") {
            n.attrs.set("_frm_t", v);
        }
        self.raw.chunks[i].content = n.encode();
        Ok(i)
    }

    /// Make the file's world voxels (MagicaVoxel z-up coordinates, the
    /// [`Scene::world_voxels`] view with [`Rule::Compose`]) equal `world`, touching as little
    /// as possible. Erased cells are cleared in every model that covers them, painted cells
    /// go into the model that shows them (or would, by the overlap rule), added cells into
    /// the first shown model that covers them (not a hidden node or layer, where MagicaVoxel
    /// would not show them), and cells no shown model covers become new models of at most 256^3 under the root group. Only
    /// the data chunks of edited models and the root group are re-encoded; everything else
    /// stays byte for byte. If that cannot reproduce `world` exactly, the models and scene
    /// graph are rebuilt instead (see [`WorldSave`]).
    pub fn set_world(&mut self, world: &BTreeMap<[i32; 3], u8>) -> Result<WorldSave> {
        let old = self.scene()?.world_voxels(Rule::Compose);
        let mut changes: BTreeMap<[i32; 3], u8> = BTreeMap::new();
        for (p, &c) in &old {
            if world.get(p).copied().unwrap_or(0) != c {
                changes.insert(*p, world.get(p).copied().unwrap_or(0));
            }
        }
        for (p, &c) in world {
            if c != 0 && !old.contains_key(p) {
                changes.insert(*p, c);
            }
        }
        if changes.is_empty() {
            return Ok(WorldSave::Kept);
        }
        let want: BTreeMap<[i32; 3], u8> = world.iter().filter(|(_, c)| **c != 0).map(|(p, c)| (*p, *c)).collect();
        let mut kept = self.clone();
        if kept.apply_changes(&changes, &old).is_ok()
            && kept.scene().is_ok_and(|s| s.world_voxels(Rule::Compose) == want)
        {
            *self = kept;
            return Ok(WorldSave::Kept);
        }
        self.rebuild_geometry(&want);
        Ok(WorldSave::Rebuilt)
    }

    /// Apply `changes` to the models that cover them; `old` is the world before them.
    fn apply_changes(&mut self, changes: &BTreeMap<[i32; 3], u8>, old: &BTreeMap<[i32; 3], u8>) -> Result<()> {
        let scene = self.scene()?;
        let (mut inst, _) = scene.instances(Rule::Compose);
        // From version 200 the earlier instance wins an overlap, before 200 the later one.
        if scene.version < 200 {
            inst.reverse();
        }
        // Edits per instance (index into `inst`), so a linked copy can be split off.
        let mut edits: BTreeMap<usize, BTreeMap<[u8; 3], u8>> = BTreeMap::new();
        let mut added: BTreeMap<[i32; 3], u8> = BTreeMap::new();
        let hidden_layers: Vec<i32> =
            scene.layers.iter().filter(|l| l.attrs.is_true("_hidden")).map(|l| l.id).collect();
        let shown = |i: &Instance| !i.hidden && !hidden_layers.contains(&i.layer);
        for (&p, &c) in changes {
            let mut covering = inst
                .iter()
                .enumerate()
                .filter_map(|(k, i)| model_cell(&i.world, scene.models[i.model].size, p).map(|v| (k, v)));
            let into = if c == 0 {
                for (k, v) in covering {
                    edits.entry(k).or_default().insert(v, 0);
                }
                continue;
            } else if old.contains_key(&p) {
                covering.next()
            } else {
                covering.find(|&(k, _)| shown(&inst[k]))
            };
            match into {
                Some((k, v)) => {
                    edits.entry(k).or_default().insert(v, c);
                }
                None => {
                    added.insert(p, c);
                }
            }
        }
        let mut model_uses: HashMap<usize, usize> = HashMap::new();
        let mut shape_uses: HashMap<i32, usize> = HashMap::new();
        for i in &inst {
            *model_uses.entry(i.model).or_default() += 1;
            *shape_uses.entry(i.shape_node).or_default() += 1;
        }
        // New SIZE and data chunk pairs, appended after the last model.
        let mut new_models: Vec<Chunk> = Vec::new();
        for (k, e) in &edits {
            let i = &inst[*k];
            let m = &scene.models[i.model];
            if model_uses[&i.model] == 1 {
                set_model_voxels(&mut self.raw.chunks[m.data_chunk], e)?;
                continue;
            }
            // A linked copy: vxr shows each instance on its own, so the edited one gets its
            // own copy of the model. Its shape node must not be shared too.
            if i.shape_node < 0 || shape_uses[&i.shape_node] != 1 {
                return Err(Error::new("edited a shape node shared by several instances"));
            }
            let index = scene.models.len() + new_models.len() / 2;
            let mut data = self.raw.chunks[m.data_chunk].clone();
            set_model_voxels(&mut data, e)?;
            new_models.push(self.raw.chunks[m.size_chunk].clone());
            new_models.push(data);
            let at = scene.node_chunks[&i.shape_node];
            let mut shp = Nshp::parse(&self.raw.chunks[at].content)?;
            shp.models[0].0 = index as i32;
            self.raw.chunks[at].content = shp.encode();
        }
        let mut new_nodes = Vec::new();
        if !added.is_empty() {
            self.add_models(&scene, &inst, &added, &mut new_models, &mut new_nodes)?;
        }
        let chunks = &mut self.raw.chunks;
        let after_models =
            chunks.iter().rposition(|c| c.is(chunk::XYZI) || c.is(chunk::TDCZ)).map(|i| i + 1).unwrap_or(0);
        let n_models = new_models.len();
        chunks.splice(after_models..after_models, new_models);
        let after_nodes = chunks
            .iter()
            .rposition(|c| c.is(chunk::NTRN) || c.is(chunk::NGRP) || c.is(chunk::NSHP))
            .map(|i| i + 1)
            .unwrap_or(after_models + n_models);
        chunks.splice(after_nodes..after_nodes, new_nodes);
        Ok(())
    }

    /// Add world voxels that no model covers as new models, one nTRN and nSHP each, under
    /// the root group (creating the usual nTRN 0 / nGRP 1 root if the file has no graph).
    fn add_models(
        &mut self,
        scene: &Scene,
        inst: &[Instance],
        added: &BTreeMap<[i32; 3], u8>,
        models: &mut Vec<Chunk>,
        nodes: &mut Vec<Chunk>,
    ) -> Result<()> {
        let mut next_id = scene.nodes.keys().next_back().map(|k| k + 1).unwrap_or(0);
        let (group, parent) = if scene.nodes.is_empty() {
            // No scene graph: model 0 sits at the origin, which an empty nTRN reproduces.
            let mut children = Vec::new();
            if !inst.is_empty() {
                nodes.push(Chunk::new(chunk::NTRN, trn(2, 3, Dict::default()).encode()));
                nodes.push(Chunk::new(chunk::NSHP, shp(3, 0).encode()));
                children.push(2);
            }
            next_id = 4;
            nodes.insert(0, Chunk::new(chunk::NTRN, trn(0, 1, Dict::default()).encode()));
            (Ngrp { id: 1, attrs: Dict::default(), children, extra: vec![] }, Affine::IDENTITY)
        } else {
            let Some(Node::Transform(root)) = scene.nodes.get(&0) else {
                return Err(Error::new("scene graph root is not a transform"));
            };
            let Some(Node::Group(g)) = scene.nodes.get(&root.child) else {
                return Err(Error::new("scene graph root does not hold a group"));
            };
            (g.clone(), Scene::base_transform(root, &mut Vec::new()))
        };
        let mut group = group;
        let first_model = scene.models.len() + models.len() / 2;
        for (k, (size, voxels, t)) in world_blocks(added).into_iter().enumerate() {
            // The new node sits under the root transform, so undo it: local = parent^-1 * world.
            let rt = transpose(&parent.r);
            let lt = mat_vec(&rt, [t[0] - parent.t[0], t[1] - parent.t[1], t[2] - parent.t[2]]);
            let f = frame(if rt == IDENTITY_R { None } else { Some(rt) }, lt);
            models.push(Chunk::new(chunk::SIZE, Size { x: size[0], y: size[1], z: size[2], extra: vec![] }.encode()));
            models.push(Chunk::new(chunk::XYZI, Xyzi { voxels, extra: vec![] }.encode()));
            let mut n = trn(next_id, next_id + 1, f);
            n.layer = 0;
            nodes.push(Chunk::new(chunk::NTRN, n.encode()));
            nodes.push(Chunk::new(chunk::NSHP, shp(next_id + 1, (first_model + k) as i32).encode()));
            group.children.push(next_id);
            next_id += 2;
        }
        let group_chunk = Chunk::new(chunk::NGRP, group.encode());
        if scene.nodes.is_empty() {
            nodes.insert(1, group_chunk);
        } else {
            self.raw.chunks[scene.node_chunks[&group.id]] = group_chunk;
        }
        Ok(())
    }

    /// Replace every model and scene graph chunk with `world` laid out by [`Builder`], at
    /// the place the first of them was. Other chunks keep their bytes and order.
    fn rebuild_geometry(&mut self, world: &BTreeMap<[i32; 3], u8>) {
        let mut b = Builder::new(self.raw.version);
        b.add_world(world);
        let geometry: Vec<Chunk> = b.build().chunks.into_iter().filter(|c| is_geometry(&c.id)).collect();
        let at = self.raw.chunks.iter().position(|c| is_geometry(&c.id)).unwrap_or(0);
        let before = self.raw.chunks[..at].iter().filter(|c| !is_geometry(&c.id)).count();
        self.raw.chunks.retain(|c| !is_geometry(&c.id));
        self.raw.chunks.splice(before..before, geometry);
    }
}

/// Apply model-space edits (colour 0 clears) to one XYZI or TDCZ chunk.
fn set_model_voxels(c: &mut Chunk, e: &BTreeMap<[u8; 3], u8>) -> Result<()> {
    match Typed::parse(c)? {
        Typed::Xyzi(mut x) => {
            x.voxels.retain(|v| !e.contains_key(&[v[0], v[1], v[2]]));
            x.voxels.extend(e.iter().filter(|(_, c)| **c != 0).map(|(p, c)| [p[0], p[1], p[2], *c]));
            c.content = Xyzi::encode(&x);
        }
        Typed::Tdcz(t) => {
            let mut dense = t.decode()?;
            let (sx, sy) = (t.x as usize, t.y as usize);
            for (p, &col) in e {
                dense[p[0] as usize + sx * (p[1] as usize + sy * p[2] as usize)] = col;
            }
            c.content = Tdcz::encode_dense(t.x, t.y, t.z, &dense).encode();
        }
        _ => unreachable!("model data chunk is XYZI or TDCZ"),
    }
    Ok(())
}

fn trn(id: i32, child: i32, frame: Dict) -> Ntrn {
    Ntrn { id, attrs: Dict::default(), child, reserved: -1, layer: -1, frames: vec![frame], extra: vec![] }
}

fn shp(id: i32, model: i32) -> Nshp {
    Nshp { id, attrs: Dict::default(), models: vec![(model, Dict::default())], extra: vec![] }
}
