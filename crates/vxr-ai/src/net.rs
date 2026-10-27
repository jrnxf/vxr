//! What a host gives the assistant: a way to send a request and stream the answer back
//! ([`Transport`]: a thread with a blocking client natively, `fetch` in the browser, a
//! scripted provider in tests), and where keys live ([`Keys`]: the OS keychain natively, the
//! tab's memory in the browser).

use crate::provider::Kind;

/// One HTTP POST with a JSON body. `Debug` hides the headers that carry a key.
#[derive(Clone, PartialEq, Eq)]
pub struct Request {
    pub url: String,
    pub headers: Vec<(String, String)>,
    pub body: String,
}

/// Headers whose value is a key.
pub fn secret_header(name: &str) -> bool {
    matches!(name.to_ascii_lowercase().as_str(), "authorization" | "x-api-key" | "api-key")
}

impl std::fmt::Debug for Request {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let headers: Vec<(&str, &str)> = self
            .headers
            .iter()
            .map(|(k, v)| (k.as_str(), if secret_header(k) { "<hidden>" } else { v.as_str() }))
            .collect();
        f.debug_struct("Request").field("url", &self.url).field("headers", &headers).field("body", &self.body).finish()
    }
}

/// What a stream gives, in order: the status, body bytes as they arrive, then the end or a
/// failure.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Piece {
    Status(u16),
    Bytes(Vec<u8>),
    End,
    /// No answer, or it broke off (the network): worth sending again.
    Failed(String),
    /// Refused before it was sent (a host the page may not reach): not worth sending again.
    Blocked(String),
}

/// A request on its way. Polled once a frame; it never blocks.
pub trait Stream {
    /// Everything that arrived since the last poll.
    fn poll(&mut self) -> Vec<Piece>;
    /// Stop reading and drop the connection; later polls return nothing.
    fn cancel(&mut self);
}

pub trait Transport {
    fn send(&mut self, request: Request) -> Box<dyn Stream>;
}

/// Where a key is kept, as the settings say it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum KeyPlace {
    /// The OS keychain (macOS Keychain, Windows Credential Manager, Secret Service).
    Keychain,
    /// The browser tab's memory, or the browser itself when remembered.
    Browser,
    /// Memory only (tests, the headless checks).
    Memory,
}

/// The keys, one per client. A key is read when a request is sent and kept nowhere else.
pub trait Keys {
    fn get(&mut self, kind: Kind) -> Option<String>;
    /// Keep `key` for `kind`. `remember` asks the browser to keep it beyond the tab
    /// (encrypted on the device); hosts that always keep keys ignore it.
    fn set(&mut self, kind: Kind, key: &str, remember: bool) -> Result<(), String>;
    fn remove(&mut self, kind: Kind) -> Result<(), String>;
    /// Whether a key is kept, without reading it out where it can be avoided.
    fn has(&mut self, kind: Kind) -> bool;
    fn place(&self) -> KeyPlace;
    /// Whether the browser remembers this kind's key beyond the tab.
    fn remembered(&self, _kind: Kind) -> bool {
        false
    }
}

/// Seconds a streamed answer may go without a byte before it counts as dropped.
pub const IDLE_SECS: u64 = 60;

/// The providers a web page may reach, by origin. The page's content security policy
/// (`crates/vxr-web/www/_headers`) lists the same, and local servers on any port.
pub const BROWSER_ORIGINS: [&str; 3] = ["https://api.anthropic.com", "https://api.openai.com", "https://openrouter.ai"];

/// What a web page cannot reach, as the settings and errors say it.
pub const BROWSER_LIMIT: &str = "Only Anthropic, OpenAI, OpenRouter and local servers (localhost or 127.0.0.1) work from the browser; the desktop app can reach other hosts";

/// The origin of `url` (`scheme://host[:port]`), in lower case and without user info.
pub fn origin(url: &str) -> String {
    let (scheme, rest) = url.trim().split_once("://").unwrap_or(("", url));
    let authority = rest.split(['/', '?', '#']).next().unwrap_or(rest);
    let authority = authority.rsplit('@').next().unwrap_or(authority);
    format!("{scheme}://{authority}").to_ascii_lowercase()
}

/// Whether a web page may reach `url`: one of [`BROWSER_ORIGINS`], or a local server over
/// `http` on any port. The page's own origin is the host's to allow.
pub fn browser_reachable(url: &str) -> bool {
    let o = origin(url);
    if BROWSER_ORIGINS.contains(&o.as_str()) {
        return true;
    }
    let Some(authority) = o.strip_prefix("http://") else { return false };
    let host = match authority.rsplit_once(':') {
        Some((h, p)) if p.chars().all(|c| c.is_ascii_digit()) => h,
        _ => authority,
    };
    host == "localhost" || host == "127.0.0.1"
}

/// Keys in memory only.
#[derive(Default)]
pub struct MemoryKeys {
    keys: [Option<String>; 3],
}

impl Keys for MemoryKeys {
    fn get(&mut self, kind: Kind) -> Option<String> {
        self.keys[kind.index()].clone()
    }
    fn set(&mut self, kind: Kind, key: &str, _remember: bool) -> Result<(), String> {
        self.keys[kind.index()] = Some(key.to_string());
        Ok(())
    }
    fn remove(&mut self, kind: Kind) -> Result<(), String> {
        self.keys[kind.index()] = None;
        Ok(())
    }
    fn has(&mut self, kind: Kind) -> bool {
        self.keys[kind.index()].is_some()
    }
    fn place(&self) -> KeyPlace {
        KeyPlace::Memory
    }
}

/// Provider text made safe to show: cut to 400 characters, with the key and anything
/// key-shaped replaced by `<key>` (some providers quote part of a bad key back).
pub fn scrub(text: &str, key: Option<&str>) -> String {
    let mut s: String = text.chars().take(400).collect();
    if let Some(k) = key.filter(|k| k.len() >= 8) {
        s = s.replace(k, "<key>");
        let head: String = k.chars().take(12).collect();
        s = s.replace(&head, "<key>");
    }
    let mut out = String::with_capacity(s.len());
    let mut word = String::new();
    let flush = |word: &mut String, out: &mut String| {
        let keyish = (word.starts_with("sk-") || word.starts_with("AIza")) && word.len() >= 10;
        out.push_str(if keyish { "<key>" } else { word });
        word.clear();
    };
    for c in s.chars() {
        if c.is_ascii_alphanumeric() || c == '-' || c == '_' || c == '*' {
            word.push(c);
        } else {
            flush(&mut word, &mut out);
            out.push(c);
        }
    }
    flush(&mut word, &mut out);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_page_reaches_the_providers_and_local_servers_only() {
        for url in [
            "https://api.anthropic.com/v1/messages",
            "https://api.openai.com/v1/responses",
            "https://openrouter.ai/api/v1/chat/completions",
            "http://localhost:11434/v1/chat/completions",
            "http://127.0.0.1:1234/v1",
            "http://127.0.0.1:8080/v1",
            "HTTP://LocalHost:9000/v1",
            "http://localhost/v1",
        ] {
            assert!(browser_reachable(url), "{url}");
        }
        for url in [
            "http://192.168.1.20:11434/v1",
            "https://localhost:11434/v1",
            "http://api.openai.com/v1",
            "https://api.openai.com:8443/v1",
            "https://example.com/v1",
            "http://localhost@example.com/v1",
            "http://localhost.example.com/v1",
            "openrouter.ai/api/v1",
        ] {
            assert!(!browser_reachable(url), "{url}");
        }
        assert_eq!(origin("http://user:pw@Example.com:81/v1?x#y"), "http://example.com:81");
    }

    #[test]
    fn keys_never_show() {
        let r = Request {
            url: "https://api.anthropic.com/v1/messages".into(),
            headers: vec![
                ("x-api-key".into(), "sk-ant-secret-123".into()),
                ("Authorization".into(), "Bearer sk-1".into()),
            ],
            body: "{}".into(),
        };
        let d = format!("{r:?}");
        assert!(!d.contains("secret") && !d.contains("sk-1") && d.contains("<hidden>"), "{d}");
        assert_eq!(
            scrub("Incorrect API key provided: sk-proj-abc*****wxyz. See", Some("sk-proj-abcdefghwxyz")),
            "Incorrect API key provided: <key>. See"
        );
        assert_eq!(scrub("bad key sk-ant-api03-zzzzzzzz here", None), "bad key <key> here");
        assert_eq!(scrub("the key abcdefgh123 was wrong", Some("abcdefgh123")), "the key <key> was wrong");
        let mut k = MemoryKeys::default();
        assert!(!k.has(Kind::OpenAi));
        k.set(Kind::OpenAi, "sk-x", false).unwrap();
        assert_eq!(k.get(Kind::OpenAi).as_deref(), Some("sk-x"));
        k.remove(Kind::OpenAi).unwrap();
        assert!(!k.has(Kind::OpenAi));
    }
}
