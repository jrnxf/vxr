//! Raw layer: the file as written, chunk by chunk.
//!
//! Layout: `"VOX "`, int32 version, then the `MAIN` chunk. A chunk is a 4-byte id,
//! int32 content size N, int32 children size M, N content bytes, M child bytes.
//! Every other chunk is a child of `MAIN`. Bytes after `MAIN` are kept as `trailing`.

use crate::bytes::Reader;
use crate::{Error, Result, id_str};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Chunk {
    pub id: [u8; 4],
    pub content: Vec<u8>,
    /// Children of a non-MAIN chunk. The spec has none; we keep them as bytes.
    pub children: Vec<u8>,
}

impl Chunk {
    pub fn new(id: &[u8; 4], content: Vec<u8>) -> Self {
        Chunk { id: *id, content, children: Vec::new() }
    }

    pub fn is(&self, id: &[u8; 4]) -> bool {
        &self.id == id
    }

    pub fn encoded_len(&self) -> usize {
        12 + self.content.len() + self.children.len()
    }

    fn write_to(&self, out: &mut Vec<u8>) {
        out.extend_from_slice(&self.id);
        out.extend_from_slice(&(self.content.len() as i32).to_le_bytes());
        out.extend_from_slice(&(self.children.len() as i32).to_le_bytes());
        out.extend_from_slice(&self.content);
        out.extend_from_slice(&self.children);
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RawFile {
    /// Written back as read: 150, 200, or anything else.
    pub version: i32,
    /// Content of the MAIN chunk itself (empty in every known file).
    pub main_content: Vec<u8>,
    pub chunks: Vec<Chunk>,
    /// Bytes after the MAIN chunk.
    pub trailing: Vec<u8>,
}

/// `at` is where the chunk starts, for the message.
fn size_field(r: &mut Reader, what: &str, id: &[u8; 4], at: usize) -> Result<usize> {
    let n = r.i32()?;
    if n < 0 {
        return Err(Error::new(format!("{} chunk at offset {at}: negative {what} size {n}", id_str(id))));
    }
    Ok(n as usize)
}

fn read_chunk(r: &mut Reader) -> Result<Chunk> {
    let at = r.pos();
    let id: [u8; 4] = r.array()?;
    let n = size_field(r, "content", &id, at)?;
    let m = size_field(r, "children", &id, at)?;
    if n.saturating_add(m) > r.remaining() {
        return Err(Error::new(format!(
            "{} chunk at offset {at}: sizes {n} + {m} run past the end ({} left)",
            id_str(&id),
            r.remaining()
        )));
    }
    let content = r.bytes(n)?.to_vec();
    let children = r.bytes(m)?.to_vec();
    Ok(Chunk { id, content, children })
}

impl RawFile {
    pub fn parse(bytes: &[u8]) -> Result<RawFile> {
        let mut r = Reader::new(bytes);
        let magic: [u8; 4] = r.array().map_err(|_| Error::new("file shorter than the 8-byte header"))?;
        if &magic != b"VOX " {
            return Err(Error::new(format!("not a MagicaVoxel file (magic {:?})", id_str(&magic))));
        }
        let version = r.i32()?;
        let main_id: [u8; 4] = r.array().map_err(|_| Error::new("no MAIN chunk"))?;
        if &main_id != b"MAIN" {
            return Err(Error::new(format!("first chunk is {}, not MAIN", id_str(&main_id))));
        }
        let n = size_field(&mut r, "content", &main_id, 8)?;
        let m = size_field(&mut r, "children", &main_id, 8)?;
        let main_content = r.bytes(n)?.to_vec();
        let children = r.bytes(m).map_err(|e| Error::new(format!("MAIN children size {m} runs past the end: {e}")))?;
        let mut cr = Reader::new(children);
        let mut chunks = Vec::new();
        while cr.remaining() > 0 {
            chunks.push(read_chunk(&mut cr)?);
        }
        let trailing = r.rest();
        Ok(RawFile { version, main_content, chunks, trailing })
    }

    pub fn write(&self) -> Vec<u8> {
        let children_len: usize = self.chunks.iter().map(Chunk::encoded_len).sum();
        let mut out = Vec::with_capacity(20 + self.main_content.len() + children_len + self.trailing.len());
        out.extend_from_slice(b"VOX ");
        out.extend_from_slice(&self.version.to_le_bytes());
        out.extend_from_slice(b"MAIN");
        out.extend_from_slice(&(self.main_content.len() as i32).to_le_bytes());
        out.extend_from_slice(&(children_len as i32).to_le_bytes());
        out.extend_from_slice(&self.main_content);
        for c in &self.chunks {
            c.write_to(&mut out);
        }
        out.extend_from_slice(&self.trailing);
        out
    }
}
