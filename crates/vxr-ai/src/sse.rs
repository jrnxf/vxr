//! Server-sent events, as every provider streams its answer: blocks split by a blank line,
//! `event:` and `data:` fields, `:` comments (OpenRouter sends keep-alive ones). Bytes may
//! arrive cut anywhere, inside a line or a UTF-8 character.

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Event {
    /// The `event:` field, or `message` when the block has none.
    pub event: String,
    /// The `data:` lines, joined by newlines.
    pub data: String,
}

/// The most bytes held while waiting for a blank line. A real event is a few KiB; a broken or
/// hostile endpoint that never sends one must not fill memory.
pub const MAX_BUFFERED: usize = 4 << 20;

#[derive(Default)]
pub struct Sse {
    buf: Vec<u8>,
    /// How much of `buf` has been searched for a blank line already.
    searched: usize,
}

impl Sse {
    /// Take the next bytes of the stream and return the events they complete, or an error when
    /// more than [`MAX_BUFFERED`] bytes arrived without one finishing.
    pub fn push(&mut self, bytes: &[u8]) -> Result<Vec<Event>, String> {
        self.buf.extend_from_slice(bytes);
        let mut out = Vec::new();
        // A blank line may straddle the old end: search again from 3 bytes before it.
        while let Some((end, skip)) = block_end(&self.buf, self.searched.saturating_sub(3)) {
            let block: Vec<u8> = self.buf.drain(..end + skip).take(end).collect();
            self.searched = 0;
            if let Some(e) = parse(&block) {
                out.push(e);
            }
        }
        self.searched = self.buf.len();
        if self.buf.len() > MAX_BUFFERED {
            return Err("the provider sent an event too large to read".into());
        }
        Ok(out)
    }

    /// The stream ended: a last block without its blank line still counts.
    pub fn finish(&mut self) -> Option<Event> {
        let block = std::mem::take(&mut self.buf);
        self.searched = 0;
        parse(&block)
    }
}

/// Where the first block ends, searching from `from`, and how long the blank line after it is.
fn block_end(b: &[u8], from: usize) -> Option<(usize, usize)> {
    for i in from..b.len() {
        for sep in [&b"\r\n\r\n"[..], b"\n\n", b"\r\r"] {
            if b[i..].starts_with(sep) {
                return Some((i, sep.len()));
            }
        }
    }
    None
}

fn parse(block: &[u8]) -> Option<Event> {
    let text = String::from_utf8_lossy(block);
    let mut event = None;
    let mut data: Vec<&str> = Vec::new();
    for line in text.split(['\n', '\r']) {
        if line.starts_with(':') || line.is_empty() {
            continue;
        }
        let (field, value) = line.split_once(':').unwrap_or((line, ""));
        let value = value.strip_prefix(' ').unwrap_or(value);
        match field {
            "event" => event = Some(value.to_string()),
            "data" => data.push(value),
            _ => {}
        }
    }
    if data.is_empty() {
        return None;
    }
    Some(Event { event: event.unwrap_or_else(|| "message".into()), data: data.join("\n") })
}

#[cfg(test)]
mod tests {
    use super::*;

    const STREAM: &[u8] =
        b": keep-alive\n\nevent: a\ndata: {\"x\":\"\xc3\xa9\"}\n\ndata: one\r\ndata: two\r\n\r\ndata: [DONE]\n\n";

    #[test]
    fn events_come_out_whole_however_the_bytes_are_cut() {
        let whole = Sse::default().push(STREAM).unwrap();
        assert_eq!(whole.len(), 3);
        assert_eq!(whole[0], Event { event: "a".into(), data: "{\"x\":\"é\"}".into() });
        assert_eq!(whole[1], Event { event: "message".into(), data: "one\ntwo".into() });
        assert_eq!(whole[2].data, "[DONE]");
        for cut in 1..STREAM.len() {
            let mut s = Sse::default();
            let mut got = s.push(&STREAM[..cut]).unwrap();
            got.extend(s.push(&STREAM[cut..]).unwrap());
            assert_eq!(got, whole, "cut at {cut}");
        }
        let mut s = Sse::default();
        let one_by_one: Vec<Event> = STREAM.iter().flat_map(|b| s.push(&[*b]).unwrap()).collect();
        assert_eq!(one_by_one, whole);
    }

    #[test]
    fn a_last_block_without_its_blank_line_counts() {
        let mut s = Sse::default();
        assert!(s.push(b"data: end").unwrap().is_empty());
        assert_eq!(s.finish().map(|e| e.data), Some("end".into()));
        assert_eq!(s.finish(), None);
    }

    #[test]
    fn an_event_that_never_ends_is_refused() {
        let mut s = Sse::default();
        let chunk = vec![b'x'; 64 << 10];
        let mut fed = 0;
        let err = loop {
            match s.push(&chunk) {
                Ok(_) => fed += chunk.len(),
                Err(e) => break e,
            }
            assert!(fed <= MAX_BUFFERED, "kept buffering past the cap");
        };
        assert!(err.contains("too large"), "{err}");
    }
}
