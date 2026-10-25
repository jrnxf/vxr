//! Bounds-checked little-endian reading and writing, plus the ordered DICT type.

use crate::{Error, Result};

pub struct Reader<'a> {
    buf: &'a [u8],
    pos: usize,
}

impl<'a> Reader<'a> {
    pub fn new(buf: &'a [u8]) -> Self {
        Reader { buf, pos: 0 }
    }

    pub fn pos(&self) -> usize {
        self.pos
    }

    pub fn remaining(&self) -> usize {
        self.buf.len() - self.pos
    }

    pub fn bytes(&mut self, n: usize) -> Result<&'a [u8]> {
        if n > self.remaining() {
            return Err(Error::new(format!("need {n} bytes at offset {}, only {} left", self.pos, self.remaining())));
        }
        let s = &self.buf[self.pos..self.pos + n];
        self.pos += n;
        Ok(s)
    }

    pub fn array<const N: usize>(&mut self) -> Result<[u8; N]> {
        let s = self.bytes(N)?;
        let mut a = [0u8; N];
        a.copy_from_slice(s);
        Ok(a)
    }

    pub fn u8(&mut self) -> Result<u8> {
        Ok(self.bytes(1)?[0])
    }

    pub fn i32(&mut self) -> Result<i32> {
        Ok(i32::from_le_bytes(self.array()?))
    }

    /// An int32 count of items that each take at least `min_item` bytes. Rejects
    /// negative counts and counts that cannot fit in what is left, so callers can
    /// allocate `count` items without trusting the file.
    pub fn count(&mut self, min_item: usize, what: &str) -> Result<usize> {
        let at = self.pos;
        let n = self.i32()?;
        if n < 0 {
            return Err(Error::new(format!("negative {what} count {n} at offset {at}")));
        }
        let n = n as usize;
        if n.checked_mul(min_item).is_none_or(|need| need > self.remaining()) {
            return Err(Error::new(format!(
                "{what} count {n} at offset {at} does not fit in {} bytes",
                self.remaining()
            )));
        }
        Ok(n)
    }

    pub fn string(&mut self) -> Result<Vec<u8>> {
        let n = self.count(1, "string length")?;
        Ok(self.bytes(n)?.to_vec())
    }

    pub fn dict(&mut self) -> Result<Dict> {
        // Each pair is at least two empty strings: 8 bytes.
        let n = self.count(8, "dict pair")?;
        let mut pairs = Vec::with_capacity(n);
        for _ in 0..n {
            let k = self.string()?;
            let v = self.string()?;
            pairs.push((k, v));
        }
        Ok(Dict(pairs))
    }

    /// Everything not read yet.
    pub fn rest(&mut self) -> Vec<u8> {
        let s = self.buf[self.pos..].to_vec();
        self.pos = self.buf.len();
        s
    }
}

#[derive(Default)]
pub struct Writer(pub Vec<u8>);

impl Writer {
    pub fn i32(&mut self, v: i32) {
        self.0.extend_from_slice(&v.to_le_bytes());
    }

    pub fn bytes(&mut self, b: &[u8]) {
        self.0.extend_from_slice(b);
    }

    pub fn string(&mut self, s: &[u8]) {
        self.i32(s.len() as i32);
        self.bytes(s);
    }

    pub fn dict(&mut self, d: &Dict) {
        self.i32(d.0.len() as i32);
        for (k, v) in &d.0 {
            self.string(k);
            self.string(v);
        }
    }
}

/// A DICT with its pairs in file order. Keys and values stay bytes, so non-UTF-8
/// names and duplicate keys survive a round trip.
#[derive(Clone, Debug, Default, PartialEq, Eq, Hash)]
pub struct Dict(pub Vec<(Vec<u8>, Vec<u8>)>);

impl Dict {
    pub fn get(&self, key: &str) -> Option<&[u8]> {
        self.0.iter().find(|(k, _)| k == key.as_bytes()).map(|(_, v)| v.as_slice())
    }

    pub fn get_str(&self, key: &str) -> Option<&str> {
        self.get(key).and_then(|v| std::str::from_utf8(v).ok())
    }

    pub fn has(&self, key: &str) -> bool {
        self.get(key).is_some()
    }

    /// Replace the first pair with this key in place, or append a new pair.
    pub fn set(&mut self, key: &str, value: impl Into<Vec<u8>>) {
        let value = value.into();
        match self.0.iter_mut().find(|(k, _)| k == key.as_bytes()) {
            Some(pair) => pair.1 = value,
            None => self.0.push((key.as_bytes().to_vec(), value)),
        }
    }

    /// Drop every pair with this key.
    pub fn remove(&mut self, key: &str) {
        self.0.retain(|(k, _)| k != key.as_bytes());
    }

    pub fn with(mut self, key: &str, value: impl Into<Vec<u8>>) -> Self {
        self.set(key, value);
        self
    }

    pub fn is_true(&self, key: &str) -> bool {
        matches!(self.get_str(key).map(str::trim), Some("1") | Some("true"))
    }
}
