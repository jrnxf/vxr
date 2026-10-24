//! Semantic layer: what the chunks mean.

use std::collections::BTreeMap;

use crate::bytes::Dict;
use crate::chunk::{self, DictOnly, IdDict, Layr, Ngrp, Note, Nshp, Ntrn, Pack, Typed};
use crate::default_palette::DEFAULT_PALETTE;
use crate::raw::RawFile;
use crate::{Error, Result};

/// A signed-permutation rotation (or mirror) plus an integer translation.
/// `world = r * p + t`, with `r` stored as rows, as MagicaVoxel's `_r` byte does.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct Affine {
    pub r: [[i32; 3]; 3],
    pub t: [i32; 3],
}

pub const IDENTITY_R: [[i32; 3]; 3] = [[1, 0, 0], [0, 1, 0], [0, 0, 1]];

impl Affine {
    pub const IDENTITY: Affine = Affine { r: IDENTITY_R, t: [0, 0, 0] };

    /// `self` applied after `child`: the parent-to-child composition ogt_vox uses.
    pub fn then(&self, child: &Affine) -> Affine {
        Affine { r: mat_mul(&self.r, &child.r), t: add(mat_vec(&self.r, child.t), self.t) }
    }

    pub fn apply(&self, p: [i32; 3]) -> [i32; 3] {
        add(mat_vec(&self.r, p), self.t)
    }

    pub fn det(&self) -> i32 {
        let m = &self.r;
        m[0][0] * (m[1][1] * m[2][2] - m[1][2] * m[2][1]) - m[0][1] * (m[1][0] * m[2][2] - m[1][2] * m[2][0])
            + m[0][2] * (m[1][0] * m[2][1] - m[1][1] * m[2][0])
    }
}

pub fn mat_mul(a: &[[i32; 3]; 3], b: &[[i32; 3]; 3]) -> [[i32; 3]; 3] {
    let mut o = [[0; 3]; 3];
    for i in 0..3 {
        for j in 0..3 {
            o[i][j] = (0..3).map(|k| a[i][k] * b[k][j]).sum();
        }
    }
    o
}

pub fn mat_vec(a: &[[i32; 3]; 3], v: [i32; 3]) -> [i32; 3] {
    let mut o = [0; 3];
    for i in 0..3 {
        o[i] = (0..3).map(|k| a[i][k].wrapping_mul(v[k])).fold(0i32, i32::wrapping_add);
    }
    o
}

fn add(a: [i32; 3], b: [i32; 3]) -> [i32; 3] {
    [a[0].wrapping_add(b[0]), a[1].wrapping_add(b[1]), a[2].wrapping_add(b[2])]
}

/// Decode the `_r` byte. Bits 0-1: column of row 0's non-zero entry, bits 2-3: row 1's,
/// row 2 takes the column left over; bits 4, 5, 6: signs of rows 0, 1, 2.
pub fn rotation_from_byte(b: u8) -> Option<[[i32; 3]; 3]> {
    let c0 = (b & 3) as usize;
    let c1 = ((b >> 2) & 3) as usize;
    if c0 == 3 || c1 == 3 || c0 == c1 {
        return None;
    }
    let c2 = 3 - c0 - c1;
    let mut r = [[0; 3]; 3];
    for (row, col) in [c0, c1, c2].into_iter().enumerate() {
        r[row][col] = if b & (1 << (4 + row)) != 0 { -1 } else { 1 };
    }
    Some(r)
}

pub fn rotation_to_byte(r: &[[i32; 3]; 3]) -> Option<u8> {
    let mut cols = [0u8; 3];
    let mut b = 0u8;
    for (row, vals) in r.iter().enumerate() {
        let mut found = None;
        for (col, &v) in vals.iter().enumerate() {
            match v {
                0 => {}
                1 | -1 if found.is_none() => found = Some((col, v < 0)),
                _ => return None,
            }
        }
        let (col, neg) = found?;
        cols[row] = col as u8;
        if neg {
            b |= 1 << (4 + row);
        }
    }
    if cols[0] == cols[1] || cols[0] == cols[2] || cols[1] == cols[2] {
        return None;
    }
    Some(b | cols[0] | (cols[1] << 2))
}

fn parse_ints<const N: usize>(v: &[u8]) -> Option<[i32; N]> {
    let s = std::str::from_utf8(v).ok()?;
    let mut out = [0; N];
    let mut it = s.split_ascii_whitespace();
    for o in out.iter_mut() {
        *o = it.next()?.parse().ok()?;
    }
    Some(out)
}

/// Reads `_r`/`_t` style keys from a dict. Unparseable values become identity/zero
/// with a warning, like ogt_vox (which does not validate them at all).
fn transform_from(d: &Dict, rkey: &str, tkey: &str, warnings: &mut Vec<String>) -> Affine {
    let mut a = Affine::IDENTITY;
    if let Some(v) = d.get(rkey) {
        match parse_ints::<1>(v).and_then(|[b]| u8::try_from(b).ok()).and_then(rotation_from_byte) {
            Some(r) => a.r = r,
            None => warnings.push(format!("bad {rkey} value {:?}", String::from_utf8_lossy(v))),
        }
    }
    if let Some(v) = d.get(tkey) {
        match parse_ints::<3>(v) {
            Some(t) => a.t = t,
            None => warnings.push(format!("bad {tkey} value {:?}", String::from_utf8_lossy(v))),
        }
    }
    a
}

#[derive(Clone, Debug)]
pub struct Model {
    pub size: [i32; 3],
    /// (x, y, z, colour index) in file order. For TDCZ, derived from the dense array.
    pub voxels: Vec<[u8; 4]>,
    pub size_chunk: usize,
    pub data_chunk: usize,
    pub compressed: bool,
}

impl Model {
    pub fn pivot(&self) -> [i32; 3] {
        [self.size[0] / 2, self.size[1] / 2, self.size[2] / 2]
    }

    /// Voxels inside the model bounds, later duplicates winning, index 0 dropped,
    /// sorted by position. This is what a dense reader such as ogt_vox ends up with.
    pub fn effective_voxels(&self) -> Vec<[u8; 4]> {
        let mut m: BTreeMap<[u8; 3], u8> = BTreeMap::new();
        for v in &self.voxels {
            let inside = (v[0] as i32) < self.size[0] && (v[1] as i32) < self.size[1] && (v[2] as i32) < self.size[2];
            if inside {
                m.insert([v[0], v[1], v[2]], v[3]);
            }
        }
        m.into_iter().filter(|(_, c)| *c != 0).map(|(p, c)| [p[0], p[1], p[2], c]).collect()
    }
}

#[derive(Clone, Debug)]
pub enum Node {
    Transform(Ntrn),
    Group(Ngrp),
    Shape(Nshp),
}

/// Scene graph limits for files from strangers.
pub const MAX_DEPTH: usize = 256;
pub const MAX_VISITS: usize = 1 << 20;

/// How transforms combine down the scene graph.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Rule {
    /// Parent then child, as a full affine transform (ogt_vox, vengi, the prototype).
    Compose,
    /// dot_vox's `traverse_graph` example: translations add up without being rotated
    /// by the parent, rotations multiply, and a node without `_r` resets rotation.
    DotVoxExample,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Instance {
    pub model: usize,
    /// Id of the nSHP and of the nTRN that holds it.
    pub shape_node: i32,
    pub transform_node: Option<i32>,
    pub world: Affine,
    pub layer: i32,
    pub hidden: bool,
    pub name: Option<Vec<u8>>,
}

#[derive(Clone, Debug, Default)]
pub struct Scene {
    pub version: i32,
    pub models: Vec<Model>,
    /// Palette in file order (see [`Scene::color`]).
    pub palette: Vec<[u8; 4]>,
    pub has_rgba: bool,
    pub imap: Option<Vec<u8>>,
    pub materials: Vec<IdDict>,
    pub legacy_matt: usize,
    pub layers: Vec<Layr>,
    pub nodes: BTreeMap<i32, Node>,
    pub node_chunks: BTreeMap<i32, usize>,
    pub cameras: Vec<IdDict>,
    pub render_objects: Vec<DictOnly>,
    pub note: Option<Note>,
    pub meta: Option<DictOnly>,
    pub pack: Option<Pack>,
    pub opaque: Vec<[u8; 4]>,
    pub warnings: Vec<String>,
}

impl Scene {
    pub fn from_raw(raw: &RawFile) -> Result<Scene> {
        let mut s = Scene { version: raw.version, palette: DEFAULT_PALETTE.to_vec(), ..Default::default() };
        let mut pending_size: Option<(chunk::Size, usize)> = None;
        let mut last_size: Option<(chunk::Size, usize)> = None;
        let mut tdcz_budget: usize = 64 << 20;
        for (i, c) in raw.chunks.iter().enumerate() {
            match Typed::parse(c)? {
                Typed::Size(sz) => {
                    if pending_size.is_some() {
                        s.warnings.push(format!("chunk {i}: SIZE without XYZI before it"));
                    }
                    pending_size = Some((sz.clone(), i));
                    last_size = Some((sz, i));
                }
                Typed::Xyzi(x) => {
                    let (sz, si) = match pending_size.take() {
                        Some(p) => p,
                        None => {
                            s.warnings.push(format!("chunk {i}: XYZI without a SIZE before it"));
                            last_size.clone().ok_or_else(|| Error::new("XYZI before any SIZE"))?
                        }
                    };
                    s.models.push(Model {
                        size: sz.dims(),
                        voxels: x.voxels,
                        size_chunk: si,
                        data_chunk: i,
                        compressed: false,
                    });
                }
                Typed::Tdcz(t) => {
                    let (sz, si) = match pending_size.take() {
                        Some(p) => p,
                        None => last_size.clone().ok_or_else(|| Error::new("TDCZ before any SIZE"))?,
                    };
                    if [t.x, t.y, t.z] != sz.dims() {
                        s.warnings.push(format!("chunk {i}: TDCZ size differs from SIZE"));
                    }
                    let vol = t.volume()?;
                    tdcz_budget = tdcz_budget
                        .checked_sub(vol)
                        .ok_or_else(|| Error::new("TDCZ models expand past the 64 Mi voxel budget"))?;
                    let dense = t.decode()?;
                    let mut voxels = Vec::new();
                    let (sx, sy) = (t.x as usize, t.y as usize);
                    for (k, &c) in dense.iter().enumerate() {
                        if c != 0 {
                            voxels.push([(k % sx) as u8, ((k / sx) % sy) as u8, (k / (sx * sy)) as u8, c]);
                        }
                    }
                    s.models.push(Model {
                        size: [t.x, t.y, t.z],
                        voxels,
                        size_chunk: si,
                        data_chunk: i,
                        compressed: true,
                    });
                }
                Typed::Rgba(p) => {
                    for (k, col) in p.colors.iter().enumerate() {
                        s.palette[k] = *col;
                    }
                    s.has_rgba = true;
                }
                Typed::Ntrn(n) => s.add_node(n.id, i, Node::Transform(n)),
                Typed::Ngrp(n) => s.add_node(n.id, i, Node::Group(n)),
                Typed::Nshp(n) => s.add_node(n.id, i, Node::Shape(n)),
                Typed::Matl(m) => s.materials.push(m),
                Typed::Rcam(m) => s.cameras.push(m),
                Typed::Layr(l) => s.layers.push(l),
                Typed::Robj(o) => s.render_objects.push(o),
                Typed::Meta(m) => s.meta = Some(m),
                Typed::Note(n) => s.note = Some(n),
                Typed::Pack(p) => s.pack = Some(p),
                Typed::Opaque(b) => {
                    if c.is(chunk::IMAP) {
                        s.imap = Some(b);
                    } else if c.is(chunk::MATT) {
                        s.legacy_matt += 1;
                    } else {
                        s.opaque.push(c.id);
                    }
                }
            }
        }
        if pending_size.is_some() {
            s.warnings.push("last SIZE has no XYZI".into());
        }
        Ok(s)
    }

    fn add_node(&mut self, id: i32, chunk: usize, node: Node) {
        if self.nodes.insert(id, node).is_some() {
            self.warnings.push(format!("node id {id} appears twice; the later one wins (as in ogt_vox)"));
        }
        self.node_chunks.insert(id, chunk);
    }

    /// Colour of voxel index `i` (index 0 is empty and maps to the unused 256th entry).
    pub fn color(&self, i: u8) -> [u8; 4] {
        self.palette[(i as usize + 255) % 256]
    }

    /// The base transform of a node: `_frm_r`/`_frm_t` from its attributes when present
    /// (MagicaVoxel 0.99.7.2 and later), otherwise frame 0's `_r`/`_t`. Same as ogt_vox.
    pub fn base_transform(n: &Ntrn, warnings: &mut Vec<String>) -> Affine {
        if n.attrs.has("_frm_r") || n.attrs.has("_frm_t") {
            transform_from(&n.attrs, "_frm_r", "_frm_t", warnings)
        } else {
            n.frames.first().map(|f| transform_from(f, "_r", "_t", warnings)).unwrap_or(Affine::IDENTITY)
        }
    }

    /// Flatten the scene graph into world-space instances, in depth-first order from
    /// node 0. MagicaVoxel shares nodes between parents (linked copies), so a node can
    /// be reached more than once; cycles and huge fan-outs in hostile files are cut off
    /// by a depth limit and a visit budget. A file with no scene graph shows model 0.
    pub fn instances(&self, rule: Rule) -> (Vec<Instance>, Vec<String>) {
        let mut out = Vec::new();
        let mut warnings = Vec::new();
        if self.nodes.is_empty() {
            if !self.models.is_empty() {
                out.push(Instance {
                    model: 0,
                    shape_node: -1,
                    transform_node: None,
                    world: Affine::IDENTITY,
                    layer: 0,
                    hidden: false,
                    name: None,
                });
            }
            return (out, warnings);
        }
        struct Item {
            node: i32,
            world: Affine,
            trn: Option<i32>,
            depth: usize,
        }
        let mut stack = vec![Item { node: 0, world: Affine::IDENTITY, trn: None, depth: 0 }];
        let mut budget = MAX_VISITS;
        while let Some(it) = stack.pop() {
            if budget == 0 {
                warnings.push(format!("scene graph visit budget of {MAX_VISITS} used up; stopped"));
                break;
            }
            budget -= 1;
            if it.depth > MAX_DEPTH {
                warnings.push(format!("node {} is deeper than {MAX_DEPTH} (a cycle?); skipped", it.node));
                continue;
            }
            let Some(node) = self.nodes.get(&it.node) else {
                warnings.push(format!("missing node {}", it.node));
                continue;
            };
            match node {
                Node::Transform(n) => {
                    let world = match rule {
                        Rule::Compose => it.world.then(&Self::base_transform(n, &mut warnings)),
                        Rule::DotVoxExample => {
                            let f = n.frames.first().cloned().unwrap_or_default();
                            let local = transform_from(&f, "_r", "_t", &mut warnings);
                            let r = if f.has("_r") { mat_mul(&it.world.r, &local.r) } else { IDENTITY_R };
                            Affine { r, t: add(it.world.t, local.t) }
                        }
                    };
                    stack.push(Item { node: n.child, world, trn: Some(n.id), depth: it.depth + 1 });
                }
                Node::Group(g) => {
                    for &c in g.children.iter().rev() {
                        stack.push(Item { node: c, world: it.world, trn: it.trn, depth: it.depth + 1 });
                    }
                }
                Node::Shape(sh) => {
                    let Some(&(m, _)) = sh.models.first() else {
                        warnings.push(format!("shape node {} has no model", sh.id));
                        continue;
                    };
                    if m < 0 || m as usize >= self.models.len() {
                        warnings.push(format!("shape node {} points at missing model {m}", sh.id));
                        continue;
                    }
                    let trn = it.trn.and_then(|id| match self.nodes.get(&id) {
                        Some(Node::Transform(t)) => Some(t),
                        _ => None,
                    });
                    out.push(Instance {
                        model: m as usize,
                        shape_node: sh.id,
                        transform_node: it.trn,
                        world: it.world,
                        layer: trn.map(|t| t.layer).unwrap_or(0),
                        hidden: trn.map(|t| t.attrs.is_true("_hidden")).unwrap_or(false),
                        name: trn.and_then(|t| t.attrs.get("_name").map(<[u8]>::to_vec)),
                    });
                }
            }
        }
        (out, warnings)
    }

    /// World cell of model voxel `v` under `world`, with MagicaVoxel's pivot of
    /// floor(size / 2). Done in doubled coordinates so odd and even sizes stay exact.
    pub fn world_cell(world: &Affine, size: [i32; 3], v: [i32; 3]) -> [i32; 3] {
        let mut c2 = [0; 3];
        for k in 0..3 {
            c2[k] = 2 * v[k] + 1 - 2 * (size[k] / 2);
        }
        let t2 = world.t.map(|t| t.wrapping_mul(2));
        let w2 = add(mat_vec(&world.r, c2), t2);
        w2.map(|w| w.wrapping_sub(1).div_euclid(2))
    }

    /// Every visible-or-hidden voxel in world space, as colour index. Overlaps follow
    /// the file version: from 200 the lower instance wins, before 200 the higher one.
    pub fn world_voxels(&self, rule: Rule) -> BTreeMap<[i32; 3], u8> {
        let (inst, _) = self.instances(rule);
        let mut out = BTreeMap::new();
        let lower_wins = self.version >= 200;
        for i in &inst {
            let m = &self.models[i.model];
            for v in m.effective_voxels() {
                let p = Self::world_cell(&i.world, m.size, [v[0] as i32, v[1] as i32, v[2] as i32]);
                if lower_wins {
                    out.entry(p).or_insert(v[3]);
                } else {
                    out.insert(p, v[3]);
                }
            }
        }
        out
    }
}
