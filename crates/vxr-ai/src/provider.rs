//! The clients, the models the picker offers, and what a request costs.
//!
//! The owner's picks (`docs/decisions.md`, 2026-10-27): `claude-opus-5-5`, `gpt-6.1-sol` and
//! `claude-sonnet-5-5`; the default follows the key: Opus at effort medium for an Anthropic
//! key, `gpt-6.1-sol` for an OpenAI key, Opus suggested for OpenRouter, Sonnet listed as the
//! fast option for small scenes.

/// Which client a request goes through.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Kind {
    /// Anthropic Messages.
    Anthropic,
    /// OpenAI Responses (Chat Completions refuses `gpt-6.1-sol`'s tools with reasoning on).
    OpenAi,
    /// Any OpenAI-compatible Chat Completions endpoint: OpenRouter, Ollama, LM Studio.
    Compatible,
}

impl Kind {
    pub const ALL: [Kind; 3] = [Kind::Anthropic, Kind::OpenAi, Kind::Compatible];

    /// What the settings call it.
    pub fn name(self) -> &'static str {
        match self {
            Kind::Anthropic => "Anthropic",
            Kind::OpenAi => "OpenAI",
            Kind::Compatible => "Compatible",
        }
    }

    /// A stable id, for the keychain entry and the browser's store.
    pub fn id(self) -> &'static str {
        match self {
            Kind::Anthropic => "anthropic",
            Kind::OpenAi => "openai",
            Kind::Compatible => "compatible",
        }
    }

    pub fn from_id(id: &str) -> Option<Kind> {
        Kind::ALL.into_iter().find(|k| k.id() == id)
    }

    pub fn index(self) -> usize {
        self as usize
    }

    /// Whether a request needs a key (a local server may not).
    pub fn needs_key(self) -> bool {
        self != Kind::Compatible
    }
}

/// A model the picker offers.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Model {
    pub id: &'static str,
    pub kind: Kind,
    /// The picker's line under it.
    pub note: &'static str,
}

pub const OPUS: &str = "claude-opus-5-5";
pub const SONNET: &str = "claude-sonnet-5-5";
pub const GPT: &str = "gpt-6.1-sol";

/// The picker's models, in its order.
pub const MODELS: [Model; 3] = [
    Model { id: OPUS, kind: Kind::Anthropic, note: "Default for an Anthropic key" },
    Model { id: GPT, kind: Kind::OpenAi, note: "Default for an OpenAI key" },
    Model { id: SONNET, kind: Kind::Anthropic, note: "Fast, for small scenes" },
];

/// OpenRouter, the compatible endpoint's default.
pub const OPENROUTER: &str = "https://openrouter.ai/api/v1";

/// The same three models by OpenRouter's names, Opus first as the suggestion.
pub const COMPATIBLE_MODELS: [&str; 3] =
    ["anthropic/claude-opus-5.5", "openai/gpt-6.1-sol", "anthropic/claude-sonnet-5.5"];

/// The model a key of this kind starts with.
pub fn default_model(kind: Kind) -> &'static str {
    match kind {
        Kind::Anthropic => OPUS,
        Kind::OpenAi => GPT,
        Kind::Compatible => COMPATIBLE_MODELS[0],
    }
}

/// What a request goes out with. The key is not here: the host's [`crate::Keys`] gives it
/// when the request is sent.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Settings {
    pub kind: Kind,
    pub model: String,
    /// The compatible endpoint's base URL (`.../v1`); unused by the other two.
    pub base: String,
    /// The request is sent from a web page (Anthropic then needs its browser header).
    pub browser: bool,
}

impl Settings {
    pub fn new(kind: Kind, browser: bool) -> Settings {
        Settings { kind, model: default_model(kind).into(), base: OPENROUTER.into(), browser }
    }

    /// The host a request goes to, as the settings and errors name it.
    pub fn host(&self) -> String {
        let url = match self.kind {
            Kind::Anthropic => "https://api.anthropic.com",
            Kind::OpenAi => "https://api.openai.com",
            Kind::Compatible => &self.base,
        };
        let rest = url.split_once("://").map_or(url, |(_, r)| r);
        rest.split('/').next().unwrap_or(rest).to_string()
    }
}

/// Tokens one answer used, and its price if the provider said it (OpenRouter does).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Usage {
    /// Input tokens not read from the cache.
    pub input: u64,
    pub cache_read: u64,
    /// Written to the cache for 5 minutes (OpenAI: for its default retention).
    pub cache_write: u64,
    /// Written to the cache for an hour (Anthropic's system prompt and tools).
    pub cache_write_1h: u64,
    pub output: u64,
    pub cost: Option<f64>,
}

impl Usage {
    pub fn add(&mut self, u: Usage) {
        self.input += u.input;
        self.cache_read += u.cache_read;
        self.cache_write += u.cache_write;
        self.cache_write_1h += u.cache_write_1h;
        self.output += u.output;
        self.cost = match (self.cost, u.cost) {
            (Some(a), Some(b)) => Some(a + b),
            (a, b) => a.or(b),
        };
    }

    /// The price in US dollars: the provider's own figure, else from [`price`].
    pub fn dollars(&self, model: &str) -> Option<f64> {
        if self.cost.is_some() {
            return self.cost;
        }
        let p = price(model)?;
        Some(
            (self.input as f64 * p.input
                + self.output as f64 * p.output
                + self.cache_read as f64 * p.cache_read
                + self.cache_write as f64 * p.cache_write
                + self.cache_write_1h as f64 * p.cache_write_1h)
                / 1e6,
        )
    }
}

/// US dollars per million tokens.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Price {
    pub input: f64,
    pub output: f64,
    pub cache_read: f64,
    /// A 5-minute cache write (OpenAI: its 30-minute one).
    pub cache_write: f64,
    /// A 1-hour cache write (OpenAI: its 12-hour one).
    pub cache_write_1h: f64,
}

/// The price of the picker's models under their own or OpenRouter's names, from Anthropic's
/// and OpenAI's pricing pages (2026-11-01; OpenAI's short-context rates, under 272K input).
pub fn price(model: &str) -> Option<Price> {
    let id = model.rsplit('/').next().unwrap_or(model).replace('.', "-");
    let id = id.split('@').next().unwrap_or(&id);
    let [input, output, cache_read, cache_write, cache_write_1h] = match id {
        "claude-opus-5-5" => [4.0, 20.0, 0.20, 5.0, 8.0],
        "claude-sonnet-5-5" => [2.0, 10.0, 0.10, 2.5, 4.0],
        "gpt-6-1-sol" => [2.0, 10.0, 0.10, 2.5, 5.0],
        _ => return None,
    };
    Some(Price { input, output, cache_read, cache_write, cache_write_1h })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn prices_and_hosts() {
        let u = Usage { input: 1_000_000, output: 100_000, cache_read: 1_000_000, ..Default::default() };
        assert_eq!(u.dollars(OPUS), Some(4.0 + 2.0 + 0.2));
        let w = Usage { cache_write: 1_000_000, cache_write_1h: 1_000_000, ..Default::default() };
        assert_eq!(w.dollars(OPUS), Some(5.0 + 8.0), "an hour's write costs more than 5 minutes'");
        assert_eq!(u.dollars("anthropic/claude-sonnet-5.5"), Some(2.0 + 1.0 + 0.1));
        assert_eq!(u.dollars(GPT), Some(2.0 + 1.0 + 0.1));
        assert_eq!(Usage { cache_write: 1_000_000, ..Default::default() }.dollars(GPT), Some(2.5));
        assert_eq!(u.dollars("qwen3:8b"), None);
        assert_eq!(Usage { cost: Some(0.5), ..u }.dollars("qwen3:8b"), Some(0.5));
        let mut a = Usage { cost: Some(0.25), ..Default::default() };
        a.add(Usage { input: 3, cost: Some(0.5), ..Default::default() });
        assert_eq!((a.input, a.cost), (3, Some(0.75)));
        assert_eq!(Settings::new(Kind::Anthropic, false).host(), "api.anthropic.com");
        let mut s = Settings::new(Kind::Compatible, false);
        assert_eq!((s.host().as_str(), s.model.as_str()), ("openrouter.ai", "anthropic/claude-opus-5.5"));
        s.base = "http://127.0.0.1:11434/v1".into();
        assert_eq!(s.host(), "127.0.0.1:11434");
        assert_eq!(Kind::ALL.map(|k| Kind::from_id(k.id())), Kind::ALL.map(Some));
    }
}
