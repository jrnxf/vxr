//! Phase 2 spike: a lossless `.vox` codec in two layers.
//!
//! - [`raw`]: the file as an ordered list of chunks, every byte kept. An unedited
//!   file is written back byte for byte, including chunks we do not understand.
//! - [`chunk`]: typed parse and encode for each known chunk. Every typed chunk keeps
//!   the bytes it did not consume, so `encode(parse(c)) == c` for every chunk.
//! - [`scene`]: the semantic view (models, palette, scene graph, world instances).
//! - [`doc`]: a raw file plus edits that re-encode only the chunks they touch.
//! - [`build`]: writes new files from scratch (fixtures, property tests).
//!
//! The reader is bounds checked everywhere and returns `Err` on malformed input.

pub mod build;
pub mod bytes;
pub mod chunk;
pub mod default_palette;
pub mod doc;
pub mod fixtures;
pub mod raw;
pub mod scene;

pub use bytes::Dict;
pub use doc::Document;
pub use raw::{Chunk, RawFile};
pub use scene::{Affine, Instance, Scene};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Error {
    pub msg: String,
}

impl Error {
    pub fn new(msg: impl Into<String>) -> Self {
        Error { msg: msg.into() }
    }
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.msg)
    }
}

impl std::error::Error for Error {}

pub type Result<T> = std::result::Result<T, Error>;

/// Four-byte chunk id as text, for messages.
pub fn id_str(id: &[u8; 4]) -> String {
    id.iter().map(|&b| if b.is_ascii_graphic() || b == b' ' { b as char } else { '?' }).collect()
}
