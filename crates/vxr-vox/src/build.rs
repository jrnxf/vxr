//! Build new `.vox` files from scratch, laid out the way MagicaVoxel 0.99.7 writes
//! them: META, SIZE/XYZI pairs, the scene graph (nTRN 0, nGRP 1, then each child as
//! nTRN followed by its nGRP or nSHP), LAYR, RGBA, IMAP, MATL, rOBJ, rCAM, NOTE.

use std::collections::BTreeMap;

use crate::bytes::Dict;
use crate::chunk::{self, DictOnly, IdDict, Layr, Ngrp, Note, Nshp, Ntrn, Rgba, Size, Xyzi};
use crate::default_palette::DEFAULT_PALETTE;
use crate::raw::{Chunk, RawFile};
use crate::scene::{IDENTITY_R, rotation_to_byte};

#[derive(Clone, Debug)]
pub enum SceneNode {
    Shape { attrs: Dict, layer: i32, frames: Vec<Dict>, models: Vec<(usize, Dict)> },
    Group { attrs: Dict, layer: i32, frames: Vec<Dict>, children: Vec<SceneNode> },
}

impl SceneNode {
    pub fn shape(model: usize, r: Option<[[i32; 3]; 3]>, t: [i32; 3]) -> SceneNode {
        SceneNode::Shape {
            attrs: Dict::default(),
            layer: 0,
            frames: vec![frame(r, t)],
            models: vec![(model, Dict::default())],
        }
    }

    pub fn group(r: Option<[[i32; 3]; 3]>, t: [i32; 3], children: Vec<SceneNode>) -> SceneNode {
        SceneNode::Group { attrs: Dict::default(), layer: 0, frames: vec![frame(r, t)], children }
    }

    pub fn named(mut self, name: &str) -> SceneNode {
        match &mut self {
            SceneNode::Shape { attrs, .. } | SceneNode::Group { attrs, .. } => attrs.set("_name", name),
        }
        self
    }

    pub fn hidden(mut self) -> SceneNode {
        match &mut self {
            SceneNode::Shape { attrs, .. } | SceneNode::Group { attrs, .. } => attrs.set("_hidden", "1"),
        }
        self
    }

    pub fn on_layer(mut self, l: i32) -> SceneNode {
        match &mut self {
            SceneNode::Shape { layer, .. } | SceneNode::Group { layer, .. } => *layer = l,
        }
        self
    }

    fn id_count(&self) -> i32 {
        match self {
            SceneNode::Shape { .. } => 2,
            SceneNode::Group { children, .. } => 2 + children.iter().map(SceneNode::id_count).sum::<i32>(),
        }
    }
}

/// A frame dict with `_r` (only when not identity) and `_t` (only when not zero).
pub fn frame(r: Option<[[i32; 3]; 3]>, t: [i32; 3]) -> Dict {
    let mut d = Dict::default();
    if let Some(r) = r
        && r != IDENTITY_R
    {
        d.set("_r", rotation_to_byte(&r).expect("signed permutation").to_string());
    }
    if t != [0, 0, 0] {
        d.set("_t", format!("{} {} {}", t[0], t[1], t[2]));
    }
    d
}

#[derive(Clone, Debug, Default)]
pub struct Builder {
    pub version: i32,
    pub meta: Option<Dict>,
    pub models: Vec<([i32; 3], Vec<[u8; 4]>)>,
    pub root: Vec<SceneNode>,
    pub layers: Vec<Dict>,
    /// File order, 256 entries; the default palette if `None`.
    pub palette: Option<Vec<[u8; 4]>>,
    pub imap: Option<Vec<u8>>,
    pub materials: Vec<(i32, Dict)>,
    pub render_objects: Vec<Dict>,
    pub cameras: Vec<(i32, Dict)>,
    pub note: Option<Vec<Vec<u8>>>,
}

impl Builder {
    pub fn new(version: i32) -> Self {
        Builder { version, ..Default::default() }
    }

    pub fn model(&mut self, size: [i32; 3], voxels: Vec<[u8; 4]>) -> usize {
        self.models.push((size, voxels));
        self.models.len() - 1
    }

    /// Add world-space voxels, split into models of at most 256^3, each placed by its
    /// own nTRN so that model voxel `v` lands at the world cell it came from.
    pub fn add_world(&mut self, voxels: &BTreeMap<[i32; 3], u8>) {
        for (size, local, t) in world_blocks(voxels) {
            let m = self.model(size, local);
            self.root.push(SceneNode::shape(m, None, t));
        }
    }

    pub fn build(&self) -> RawFile {
        let mut chunks = Vec::new();
        if let Some(m) = &self.meta {
            chunks.push(Chunk::new(chunk::META, DictOnly { dict: m.clone(), extra: vec![] }.encode()));
        }
        for (size, voxels) in &self.models {
            chunks.push(Chunk::new(chunk::SIZE, Size { x: size[0], y: size[1], z: size[2], extra: vec![] }.encode()));
            chunks.push(Chunk::new(chunk::XYZI, Xyzi { voxels: voxels.clone(), extra: vec![] }.encode()));
        }
        if !self.root.is_empty() {
            let root_trn = Ntrn {
                id: 0,
                attrs: Dict::default(),
                child: 1,
                reserved: -1,
                layer: -1,
                frames: vec![Dict::default()],
                extra: vec![],
            };
            chunks.push(Chunk::new(chunk::NTRN, root_trn.encode()));
            let mut ids = Vec::new();
            let mut next = 2;
            for n in &self.root {
                ids.push(next);
                next += n.id_count();
            }
            chunks.push(Chunk::new(
                chunk::NGRP,
                Ngrp { id: 1, attrs: Dict::default(), children: ids.clone(), extra: vec![] }.encode(),
            ));
            for (n, id) in self.root.iter().zip(ids) {
                emit(n, id, &mut chunks);
            }
        }
        for (i, l) in self.layers.iter().enumerate() {
            chunks.push(Chunk::new(
                chunk::LAYR,
                Layr { id: i as i32, attrs: l.clone(), reserved: -1, extra: vec![] }.encode(),
            ));
        }
        let colors = self.palette.clone().unwrap_or_else(|| DEFAULT_PALETTE.to_vec());
        chunks.push(Chunk::new(chunk::RGBA, Rgba { colors, extra: vec![] }.encode()));
        if let Some(m) = &self.imap {
            chunks.push(Chunk::new(chunk::IMAP, m.clone()));
        }
        for (id, d) in &self.materials {
            chunks.push(Chunk::new(chunk::MATL, IdDict { id: *id, dict: d.clone(), extra: vec![] }.encode()));
        }
        for d in &self.render_objects {
            chunks.push(Chunk::new(chunk::ROBJ, DictOnly { dict: d.clone(), extra: vec![] }.encode()));
        }
        for (id, d) in &self.cameras {
            chunks.push(Chunk::new(chunk::RCAM, IdDict { id: *id, dict: d.clone(), extra: vec![] }.encode()));
        }
        if let Some(n) = &self.note {
            chunks.push(Chunk::new(chunk::NOTE, Note { names: n.clone(), extra: vec![] }.encode()));
        }
        RawFile { version: self.version, main_content: vec![], chunks, trailing: vec![] }
    }
}

/// One model cut out of world voxels: its size, its voxels, and the translation that puts
/// model voxel `v` back at its world cell.
pub type Block = ([i32; 3], Vec<[u8; 4]>, [i32; 3]);

/// World-space voxels split into models of at most 256^3, on a 256 grid anchored at the
/// voxels' low corner, so voxels that fit in 256^3 anywhere stay one model.
pub fn world_blocks(voxels: &BTreeMap<[i32; 3], u8>) -> Vec<Block> {
    let mut low = [i32::MAX; 3];
    for (p, _) in voxels.iter().filter(|(_, c)| **c != 0) {
        for k in 0..3 {
            low[k] = low[k].min(p[k]);
        }
    }
    let mut blocks: BTreeMap<[i32; 3], Vec<([i32; 3], u8)>> = BTreeMap::new();
    for (p, &c) in voxels {
        if c != 0 {
            let cell = [0, 1, 2].map(|k| ((i64::from(p[k]) - i64::from(low[k])) / 256) as i32);
            blocks.entry(cell).or_default().push((*p, c));
        }
    }
    let mut out = Vec::new();
    for (_, vs) in blocks {
        let mut lo = [i32::MAX; 3];
        let mut hi = [i32::MIN; 3];
        for (p, _) in &vs {
            for k in 0..3 {
                lo[k] = lo[k].min(p[k]);
                hi[k] = hi[k].max(p[k]);
            }
        }
        let size = [hi[0] - lo[0] + 1, hi[1] - lo[1] + 1, hi[2] - lo[2] + 1];
        let local =
            vs.iter().map(|(p, c)| [(p[0] - lo[0]) as u8, (p[1] - lo[1]) as u8, (p[2] - lo[2]) as u8, *c]).collect();
        out.push((size, local, [lo[0] + size[0] / 2, lo[1] + size[1] / 2, lo[2] + size[2] / 2]));
    }
    out
}

fn emit(n: &SceneNode, trn_id: i32, chunks: &mut Vec<Chunk>) {
    let (attrs, layer, frames) = match n {
        SceneNode::Shape { attrs, layer, frames, .. } | SceneNode::Group { attrs, layer, frames, .. } => {
            (attrs, layer, frames)
        }
    };
    let trn = Ntrn {
        id: trn_id,
        attrs: attrs.clone(),
        child: trn_id + 1,
        reserved: -1,
        layer: *layer,
        frames: frames.clone(),
        extra: vec![],
    };
    chunks.push(Chunk::new(chunk::NTRN, trn.encode()));
    match n {
        SceneNode::Shape { models, .. } => {
            let shp = Nshp {
                id: trn_id + 1,
                attrs: Dict::default(),
                models: models.iter().map(|(m, d)| (*m as i32, d.clone())).collect(),
                extra: vec![],
            };
            chunks.push(Chunk::new(chunk::NSHP, shp.encode()));
        }
        SceneNode::Group { children, .. } => {
            let mut ids = Vec::new();
            let mut next = trn_id + 2;
            for c in children {
                ids.push(next);
                next += c.id_count();
            }
            chunks.push(Chunk::new(
                chunk::NGRP,
                Ngrp { id: trn_id + 1, attrs: Dict::default(), children: ids.clone(), extra: vec![] }.encode(),
            ));
            for (c, id) in children.iter().zip(ids) {
                emit(c, id, chunks);
            }
        }
    }
}
