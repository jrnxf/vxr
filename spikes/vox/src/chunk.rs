//! Typed chunks. Each `parse` keeps whatever it did not consume in `extra`, and
//! each `encode` writes it back, so `encode(parse(c)) == c` holds for every chunk.

use crate::bytes::{Dict, Reader, Writer};
use crate::raw::Chunk;
use crate::{Error, Result, id_str};

pub const SIZE: &[u8; 4] = b"SIZE";
pub const XYZI: &[u8; 4] = b"XYZI";
pub const TDCZ: &[u8; 4] = b"TDCZ";
pub const RGBA: &[u8; 4] = b"RGBA";
pub const NTRN: &[u8; 4] = b"nTRN";
pub const NGRP: &[u8; 4] = b"nGRP";
pub const NSHP: &[u8; 4] = b"nSHP";
pub const MATL: &[u8; 4] = b"MATL";
pub const MATT: &[u8; 4] = b"MATT";
pub const LAYR: &[u8; 4] = b"LAYR";
pub const ROBJ: &[u8; 4] = b"rOBJ";
pub const RCAM: &[u8; 4] = b"rCAM";
pub const NOTE: &[u8; 4] = b"NOTE";
pub const IMAP: &[u8; 4] = b"IMAP";
pub const META: &[u8; 4] = b"META";
pub const PACK: &[u8; 4] = b"PACK";

fn ctx<T>(id: &[u8; 4], r: Result<T>) -> Result<T> {
    r.map_err(|e| Error::new(format!("{}: {e}", id_str(id))))
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Size {
    pub x: i32,
    pub y: i32,
    pub z: i32,
    pub extra: Vec<u8>,
}

impl Size {
    pub fn parse(c: &[u8]) -> Result<Self> {
        ctx(
            SIZE,
            (|| {
                let mut r = Reader::new(c);
                let (x, y, z) = (r.i32()?, r.i32()?, r.i32()?);
                if x < 0 || y < 0 || z < 0 {
                    return Err(Error::new(format!("negative size {x} {y} {z}")));
                }
                Ok(Size { x, y, z, extra: r.rest() })
            })(),
        )
    }

    pub fn encode(&self) -> Vec<u8> {
        let mut w = Writer::default();
        w.i32(self.x);
        w.i32(self.y);
        w.i32(self.z);
        w.bytes(&self.extra);
        w.0
    }

    pub fn dims(&self) -> [i32; 3] {
        [self.x, self.y, self.z]
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Xyzi {
    /// (x, y, z, colour index), in file order.
    pub voxels: Vec<[u8; 4]>,
    pub extra: Vec<u8>,
}

impl Xyzi {
    pub fn parse(c: &[u8]) -> Result<Self> {
        ctx(
            XYZI,
            (|| {
                let mut r = Reader::new(c);
                let n = r.count(4, "voxel")?;
                let mut voxels = Vec::with_capacity(n);
                for _ in 0..n {
                    voxels.push(r.array()?);
                }
                Ok(Xyzi { voxels, extra: r.rest() })
            })(),
        )
    }

    pub fn encode(&self) -> Vec<u8> {
        let mut w = Writer::default();
        w.i32(self.voxels.len() as i32);
        for v in &self.voxels {
            w.bytes(v);
        }
        w.bytes(&self.extra);
        w.0
    }
}

/// Teardown's compressed model: int32 x, y, z, then a zlib stream of x*y*z colour
/// indices, x fastest. The stream is kept as is; it is only re-compressed on edit.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Tdcz {
    pub x: i32,
    pub y: i32,
    pub z: i32,
    pub compressed: Vec<u8>,
}

/// Largest dense model a TDCZ may expand to (MagicaVoxel's 256^3 limit).
pub const MAX_DENSE: usize = 256 * 256 * 256;

impl Tdcz {
    pub fn parse(c: &[u8]) -> Result<Self> {
        ctx(
            TDCZ,
            (|| {
                let mut r = Reader::new(c);
                let (x, y, z) = (r.i32()?, r.i32()?, r.i32()?);
                if x < 0 || y < 0 || z < 0 {
                    return Err(Error::new(format!("negative size {x} {y} {z}")));
                }
                Ok(Tdcz { x, y, z, compressed: r.rest() })
            })(),
        )
    }

    pub fn encode(&self) -> Vec<u8> {
        let mut w = Writer::default();
        w.i32(self.x);
        w.i32(self.y);
        w.i32(self.z);
        w.bytes(&self.compressed);
        w.0
    }

    pub fn volume(&self) -> Result<usize> {
        if [self.x, self.y, self.z].iter().any(|&d| d > 256) {
            return Err(Error::new(format!("TDCZ model {} x {} x {} is over 256 on an axis", self.x, self.y, self.z)));
        }
        let v = self.x as usize * self.y as usize * self.z as usize;
        debug_assert!(v <= MAX_DENSE);
        Ok(v)
    }

    /// Decompress into a dense x-fastest array. Never allocates more than the
    /// declared volume, which is capped at 256^3.
    pub fn decode(&self) -> Result<Vec<u8>> {
        use std::io::Read;
        let volume = self.volume()?;
        let mut out = Vec::new();
        let mut z = flate2::read::ZlibDecoder::new(self.compressed.as_slice()).take(volume as u64);
        // Teardown streams end with a sync flush, not a final block, so a short
        // read error after all bytes arrived is expected.
        let res = z.read_to_end(&mut out);
        if out.len() < volume {
            return Err(Error::new(format!(
                "TDCZ: stream gave {} of {volume} bytes ({:?})",
                out.len(),
                res.err().map(|e| e.to_string())
            )));
        }
        Ok(out)
    }

    pub fn encode_dense(x: i32, y: i32, z: i32, dense: &[u8]) -> Tdcz {
        use std::io::Write;
        let mut e = flate2::write::ZlibEncoder::new(Vec::new(), flate2::Compression::best());
        e.write_all(dense).expect("write to Vec");
        Tdcz { x, y, z, compressed: e.finish().expect("finish to Vec") }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Rgba {
    /// File order: entry i is the colour of voxel index i + 1 (entry 255 is unused).
    pub colors: Vec<[u8; 4]>,
    pub extra: Vec<u8>,
}

impl Rgba {
    pub fn parse(c: &[u8]) -> Result<Self> {
        let mut r = Reader::new(c);
        let n = (c.len() / 4).min(256);
        let mut colors = Vec::with_capacity(n);
        for _ in 0..n {
            colors.push(r.array()?);
        }
        Ok(Rgba { colors, extra: r.rest() })
    }

    pub fn encode(&self) -> Vec<u8> {
        let mut w = Writer::default();
        for c in &self.colors {
            w.bytes(c);
        }
        w.bytes(&self.extra);
        w.0
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Ntrn {
    pub id: i32,
    pub attrs: Dict,
    pub child: i32,
    pub reserved: i32,
    pub layer: i32,
    pub frames: Vec<Dict>,
    pub extra: Vec<u8>,
}

impl Ntrn {
    pub fn parse(c: &[u8]) -> Result<Self> {
        ctx(
            NTRN,
            (|| {
                let mut r = Reader::new(c);
                let id = r.i32()?;
                let attrs = r.dict()?;
                let child = r.i32()?;
                let reserved = r.i32()?;
                let layer = r.i32()?;
                let n = r.count(4, "frame")?;
                let mut frames = Vec::with_capacity(n);
                for _ in 0..n {
                    frames.push(r.dict()?);
                }
                Ok(Ntrn { id, attrs, child, reserved, layer, frames, extra: r.rest() })
            })(),
        )
    }

    pub fn encode(&self) -> Vec<u8> {
        let mut w = Writer::default();
        w.i32(self.id);
        w.dict(&self.attrs);
        w.i32(self.child);
        w.i32(self.reserved);
        w.i32(self.layer);
        w.i32(self.frames.len() as i32);
        for f in &self.frames {
            w.dict(f);
        }
        w.bytes(&self.extra);
        w.0
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Ngrp {
    pub id: i32,
    pub attrs: Dict,
    pub children: Vec<i32>,
    pub extra: Vec<u8>,
}

impl Ngrp {
    pub fn parse(c: &[u8]) -> Result<Self> {
        ctx(
            NGRP,
            (|| {
                let mut r = Reader::new(c);
                let id = r.i32()?;
                let attrs = r.dict()?;
                let n = r.count(4, "child")?;
                let mut children = Vec::with_capacity(n);
                for _ in 0..n {
                    children.push(r.i32()?);
                }
                Ok(Ngrp { id, attrs, children, extra: r.rest() })
            })(),
        )
    }

    pub fn encode(&self) -> Vec<u8> {
        let mut w = Writer::default();
        w.i32(self.id);
        w.dict(&self.attrs);
        w.i32(self.children.len() as i32);
        for c in &self.children {
            w.i32(*c);
        }
        w.bytes(&self.extra);
        w.0
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Nshp {
    pub id: i32,
    pub attrs: Dict,
    /// (model id, attributes such as `_f`).
    pub models: Vec<(i32, Dict)>,
    pub extra: Vec<u8>,
}

impl Nshp {
    pub fn parse(c: &[u8]) -> Result<Self> {
        ctx(
            NSHP,
            (|| {
                let mut r = Reader::new(c);
                let id = r.i32()?;
                let attrs = r.dict()?;
                let n = r.count(8, "shape model")?;
                let mut models = Vec::with_capacity(n);
                for _ in 0..n {
                    let m = r.i32()?;
                    models.push((m, r.dict()?));
                }
                Ok(Nshp { id, attrs, models, extra: r.rest() })
            })(),
        )
    }

    pub fn encode(&self) -> Vec<u8> {
        let mut w = Writer::default();
        w.i32(self.id);
        w.dict(&self.attrs);
        w.i32(self.models.len() as i32);
        for (m, d) in &self.models {
            w.i32(*m);
            w.dict(d);
        }
        w.bytes(&self.extra);
        w.0
    }
}

/// Chunks that are an int32 id followed by a DICT: MATL and rCAM.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct IdDict {
    pub id: i32,
    pub dict: Dict,
    pub extra: Vec<u8>,
}

impl IdDict {
    pub fn parse(id: &[u8; 4], c: &[u8]) -> Result<Self> {
        ctx(
            id,
            (|| {
                let mut r = Reader::new(c);
                let id = r.i32()?;
                let dict = r.dict()?;
                Ok(IdDict { id, dict, extra: r.rest() })
            })(),
        )
    }

    pub fn encode(&self) -> Vec<u8> {
        let mut w = Writer::default();
        w.i32(self.id);
        w.dict(&self.dict);
        w.bytes(&self.extra);
        w.0
    }
}

/// Chunks that are a single DICT: rOBJ and META.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DictOnly {
    pub dict: Dict,
    pub extra: Vec<u8>,
}

impl DictOnly {
    pub fn parse(id: &[u8; 4], c: &[u8]) -> Result<Self> {
        ctx(
            id,
            (|| {
                let mut r = Reader::new(c);
                let dict = r.dict()?;
                Ok(DictOnly { dict, extra: r.rest() })
            })(),
        )
    }

    pub fn encode(&self) -> Vec<u8> {
        let mut w = Writer::default();
        w.dict(&self.dict);
        w.bytes(&self.extra);
        w.0
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Layr {
    pub id: i32,
    pub attrs: Dict,
    pub reserved: i32,
    pub extra: Vec<u8>,
}

impl Layr {
    pub fn parse(c: &[u8]) -> Result<Self> {
        ctx(
            LAYR,
            (|| {
                let mut r = Reader::new(c);
                let id = r.i32()?;
                let attrs = r.dict()?;
                let reserved = r.i32()?;
                Ok(Layr { id, attrs, reserved, extra: r.rest() })
            })(),
        )
    }

    pub fn encode(&self) -> Vec<u8> {
        let mut w = Writer::default();
        w.i32(self.id);
        w.dict(&self.attrs);
        w.i32(self.reserved);
        w.bytes(&self.extra);
        w.0
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Note {
    pub names: Vec<Vec<u8>>,
    pub extra: Vec<u8>,
}

impl Note {
    pub fn parse(c: &[u8]) -> Result<Self> {
        ctx(
            NOTE,
            (|| {
                let mut r = Reader::new(c);
                let n = r.count(4, "note")?;
                let mut names = Vec::with_capacity(n);
                for _ in 0..n {
                    names.push(r.string()?);
                }
                Ok(Note { names, extra: r.rest() })
            })(),
        )
    }

    pub fn encode(&self) -> Vec<u8> {
        let mut w = Writer::default();
        w.i32(self.names.len() as i32);
        for s in &self.names {
            w.string(s);
        }
        w.bytes(&self.extra);
        w.0
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Pack {
    pub models: i32,
    pub extra: Vec<u8>,
}

impl Pack {
    pub fn parse(c: &[u8]) -> Result<Self> {
        ctx(
            PACK,
            (|| {
                let mut r = Reader::new(c);
                let models = r.i32()?;
                Ok(Pack { models, extra: r.rest() })
            })(),
        )
    }

    pub fn encode(&self) -> Vec<u8> {
        let mut w = Writer::default();
        w.i32(self.models);
        w.bytes(&self.extra);
        w.0
    }
}

/// Typed view of one chunk. `Opaque` covers IMAP (256 raw bytes), legacy render
/// chunks (rLIT, rAIR, rLEN, POST, rDIS), MATT and anything unknown.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Typed {
    Size(Size),
    Xyzi(Xyzi),
    Tdcz(Tdcz),
    Rgba(Rgba),
    Ntrn(Ntrn),
    Ngrp(Ngrp),
    Nshp(Nshp),
    Matl(IdDict),
    Rcam(IdDict),
    Layr(Layr),
    Robj(DictOnly),
    Meta(DictOnly),
    Note(Note),
    Pack(Pack),
    Opaque(Vec<u8>),
}

impl Typed {
    pub fn parse(c: &Chunk) -> Result<Typed> {
        let b = c.content.as_slice();
        Ok(match &c.id {
            SIZE => Typed::Size(Size::parse(b)?),
            XYZI => Typed::Xyzi(Xyzi::parse(b)?),
            TDCZ => Typed::Tdcz(Tdcz::parse(b)?),
            RGBA => Typed::Rgba(Rgba::parse(b)?),
            NTRN => Typed::Ntrn(Ntrn::parse(b)?),
            NGRP => Typed::Ngrp(Ngrp::parse(b)?),
            NSHP => Typed::Nshp(Nshp::parse(b)?),
            MATL => Typed::Matl(IdDict::parse(MATL, b)?),
            RCAM => Typed::Rcam(IdDict::parse(RCAM, b)?),
            LAYR => Typed::Layr(Layr::parse(b)?),
            ROBJ => Typed::Robj(DictOnly::parse(ROBJ, b)?),
            META => Typed::Meta(DictOnly::parse(META, b)?),
            NOTE => Typed::Note(Note::parse(b)?),
            PACK => Typed::Pack(Pack::parse(b)?),
            _ => Typed::Opaque(b.to_vec()),
        })
    }

    pub fn encode(&self) -> Vec<u8> {
        match self {
            Typed::Size(x) => x.encode(),
            Typed::Xyzi(x) => x.encode(),
            Typed::Tdcz(x) => x.encode(),
            Typed::Rgba(x) => x.encode(),
            Typed::Ntrn(x) => x.encode(),
            Typed::Ngrp(x) => x.encode(),
            Typed::Nshp(x) => x.encode(),
            Typed::Matl(x) | Typed::Rcam(x) => x.encode(),
            Typed::Layr(x) => x.encode(),
            Typed::Robj(x) | Typed::Meta(x) => x.encode(),
            Typed::Note(x) => x.encode(),
            Typed::Pack(x) => x.encode(),
            Typed::Opaque(b) => b.clone(),
        }
    }

    /// Bytes the typed parse did not account for (non-zero means the writer added
    /// something we do not model, which we still keep).
    pub fn extra_len(&self) -> usize {
        match self {
            Typed::Size(x) => x.extra.len(),
            Typed::Xyzi(x) => x.extra.len(),
            Typed::Rgba(x) => x.extra.len(),
            Typed::Ntrn(x) => x.extra.len(),
            Typed::Ngrp(x) => x.extra.len(),
            Typed::Nshp(x) => x.extra.len(),
            Typed::Matl(x) | Typed::Rcam(x) => x.extra.len(),
            Typed::Layr(x) => x.extra.len(),
            Typed::Robj(x) | Typed::Meta(x) => x.extra.len(),
            Typed::Note(x) => x.extra.len(),
            Typed::Pack(x) => x.extra.len(),
            Typed::Tdcz(_) | Typed::Opaque(_) => 0,
        }
    }
}
