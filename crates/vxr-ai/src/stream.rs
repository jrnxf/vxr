//! A streamed answer, decoded as it arrives: text and thinking for the panel to show while
//! the model works, then one [`Turn`] when the stream ends.

use serde_json::{Map, Value, json};

use crate::chat::{Stop, ToolCall, Turn};
use crate::provider::{Kind, Usage};
use crate::sse::{Event, Sse};

/// What the panel can show before the answer is complete.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Delta {
    Text(String),
    Thinking(String),
    /// A tool call started, by name.
    Call(String),
}

pub struct Decoder {
    kind: Kind,
    sse: Sse,
    model: String,
    text: String,
    thinking: String,
    usage: Usage,
    stop: Option<Stop>,
    /// The stream said it was complete.
    ended: bool,
    /// Anthropic: content blocks by index, with tool inputs as text until their block ends.
    blocks: Vec<Value>,
    inputs: Vec<String>,
    /// OpenAI Responses: the output items.
    output: Vec<Value>,
    /// Chat Completions: the calls by index (id, name, arguments).
    calls: Vec<(String, String, String)>,
}

impl Decoder {
    /// `model` is the model asked for; the provider's name for it replaces it when given.
    pub fn new(kind: Kind, model: &str) -> Decoder {
        Decoder {
            kind,
            sse: Sse::default(),
            model: model.into(),
            text: String::new(),
            thinking: String::new(),
            usage: Usage::default(),
            stop: None,
            ended: false,
            blocks: Vec::new(),
            inputs: Vec::new(),
            output: Vec::new(),
            calls: Vec::new(),
        }
    }

    /// Take the next bytes of the stream. An error the provider streamed ends it.
    pub fn push(&mut self, bytes: &[u8]) -> Result<Vec<Delta>, String> {
        let mut out = Vec::new();
        for e in self.sse.push(bytes)? {
            self.event(&e, &mut out)?;
        }
        Ok(out)
    }

    /// The stream ended: the answer, or why there is none.
    pub fn finish(mut self) -> Result<Turn, String> {
        if let Some(e) = self.sse.finish() {
            self.event(&e, &mut Vec::new())?;
        }
        if !self.ended {
            return Err("the answer stopped before it was complete".into());
        }
        let (calls, raw) = match self.kind {
            Kind::Anthropic => {
                let calls = self
                    .blocks
                    .iter()
                    .enumerate()
                    .filter(|(_, b)| b["type"] == "tool_use")
                    .map(|(i, b)| call(b["id"].as_str(), b["name"].as_str(), &self.inputs[i]))
                    .collect();
                (calls, Value::Array(self.blocks))
            }
            Kind::OpenAi => {
                let calls = self
                    .output
                    .iter()
                    .filter(|o| o["type"] == "function_call")
                    .map(|o| call(o["call_id"].as_str(), o["name"].as_str(), o["arguments"].as_str().unwrap_or("")))
                    .collect();
                (calls, Value::Array(self.output))
            }
            Kind::Compatible => {
                let calls: Vec<ToolCall> = self.calls.iter().map(|(id, n, a)| call(Some(id), Some(n), a)).collect();
                let mut m = json!({"role": "assistant",
                    "content": if self.text.is_empty() { Value::Null } else { self.text.clone().into() }});
                if !calls.is_empty() {
                    m["tool_calls"] = self
                        .calls
                        .iter()
                        .map(
                            |(id, n, a)| json!({"id": id, "type": "function", "function": {"name": n, "arguments": a}}),
                        )
                        .collect();
                }
                (calls, m)
            }
        };
        let stop = match self.stop {
            Some(Stop::Done) | None if !calls.is_empty() => Stop::Tools,
            Some(s) => s,
            None => Stop::Done,
        };
        Ok(Turn {
            kind: self.kind,
            model: self.model,
            text: self.text,
            thinking: self.thinking,
            calls,
            stop,
            usage: self.usage,
            raw,
        })
    }

    fn event(&mut self, e: &Event, out: &mut Vec<Delta>) -> Result<(), String> {
        if e.data == "[DONE]" {
            self.ended = true;
            return Ok(());
        }
        let d: Value =
            serde_json::from_str(&e.data).map_err(|err| format!("the provider sent text that is not JSON ({err})"))?;
        match self.kind {
            Kind::Anthropic => self.anthropic(&d, out),
            Kind::OpenAi => self.responses(&d, out),
            Kind::Compatible => self.compatible(&d, out),
        }
    }

    fn anthropic(&mut self, d: &Value, out: &mut Vec<Delta>) -> Result<(), String> {
        let index = d["index"].as_u64().unwrap_or(0) as usize;
        match d["type"].as_str().unwrap_or("") {
            "message_start" => {
                let m = &d["message"];
                if let Some(id) = m["model"].as_str() {
                    self.model = id.into();
                }
                self.anthropic_usage(&m["usage"]);
            }
            "content_block_start" => {
                check_index(index)?;
                while self.blocks.len() <= index {
                    self.blocks.push(Value::Null);
                    self.inputs.push(String::new());
                }
                let b = d["content_block"].clone();
                if b["type"] == "tool_use" {
                    out.push(Delta::Call(b["name"].as_str().unwrap_or("").into()));
                }
                self.blocks[index] = b;
            }
            "content_block_delta" => {
                let Some(b) = self.blocks.get_mut(index) else { return Ok(()) };
                let delta = &d["delta"];
                let append = |b: &mut Value, field: &str, s: &str| {
                    let old = b[field].as_str().unwrap_or("").to_string();
                    b[field] = (old + s).into();
                };
                match delta["type"].as_str().unwrap_or("") {
                    "text_delta" => {
                        let s = delta["text"].as_str().unwrap_or("");
                        append(b, "text", s);
                        self.text.push_str(s);
                        out.push(Delta::Text(s.into()));
                    }
                    "thinking_delta" => {
                        let s = delta["thinking"].as_str().unwrap_or("");
                        append(b, "thinking", s);
                        self.thinking.push_str(s);
                        out.push(Delta::Thinking(s.into()));
                    }
                    "signature_delta" => b["signature"] = delta["signature"].clone(),
                    "input_json_delta" => self.inputs[index].push_str(delta["partial_json"].as_str().unwrap_or("")),
                    _ => {}
                }
            }
            "content_block_stop" => {
                if let Some(b) = self.blocks.get_mut(index).filter(|b| b["type"] == "tool_use") {
                    // The history needs an object here; a bad input is answered as an error.
                    let text = self.inputs[index].trim();
                    b["input"] = match serde_json::from_str::<Value>(if text.is_empty() { "{}" } else { text }) {
                        Ok(v @ Value::Object(_)) => v,
                        _ => Value::Object(Map::new()),
                    };
                }
            }
            "message_delta" => {
                self.anthropic_usage(&d["usage"]);
                if let Some(r) = d["delta"]["stop_reason"].as_str() {
                    self.stop = Some(match r {
                        "end_turn" | "stop_sequence" => Stop::Done,
                        "tool_use" => Stop::Tools,
                        "max_tokens" | "model_context_window_exceeded" => Stop::MaxTokens,
                        "refusal" => Stop::Refusal(
                            d["delta"]["stop_details"]["explanation"]
                                .as_str()
                                .or(d["delta"]["stop_details"]["category"].as_str())
                                .map(str::to_string),
                        ),
                        other => Stop::Other(other.into()),
                    });
                }
            }
            "message_stop" => self.ended = true,
            "error" => return Err(error_text(&d["error"])),
            _ => {}
        }
        Ok(())
    }

    /// Anthropic counts input once (message_start) and output as it goes (message_delta).
    fn anthropic_usage(&mut self, u: &Value) {
        let n = |k: &str| u[k].as_u64();
        if let Some(v) = n("input_tokens") {
            self.usage.input = v;
        }
        if let Some(v) = n("cache_read_input_tokens") {
            self.usage.cache_read = v;
        }
        if let Some(v) = n("cache_creation_input_tokens") {
            self.usage.cache_write = v;
        }
        // Split by how long the entries live, when the provider says.
        let by_ttl = &u["cache_creation"];
        if let (Some(m5), Some(h1)) =
            (by_ttl["ephemeral_5m_input_tokens"].as_u64(), by_ttl["ephemeral_1h_input_tokens"].as_u64())
        {
            (self.usage.cache_write, self.usage.cache_write_1h) = (m5, h1);
        }
        if let Some(v) = n("output_tokens") {
            self.usage.output = v;
        }
    }

    fn responses(&mut self, d: &Value, out: &mut Vec<Delta>) -> Result<(), String> {
        match d["type"].as_str().unwrap_or("") {
            "response.output_item.added" if d["item"]["type"] == "function_call" => {
                out.push(Delta::Call(d["item"]["name"].as_str().unwrap_or("").into()))
            }
            "response.output_text.delta" => {
                let s = d["delta"].as_str().unwrap_or("");
                self.text.push_str(s);
                out.push(Delta::Text(s.into()));
            }
            "response.reasoning_summary_text.delta" => {
                let s = d["delta"].as_str().unwrap_or("");
                self.thinking.push_str(s);
                out.push(Delta::Thinking(s.into()));
            }
            "response.output_item.done" => {
                let i = d["output_index"].as_u64().map_or(self.output.len(), |i| i as usize);
                check_index(i)?;
                while self.output.len() <= i {
                    self.output.push(Value::Null);
                }
                self.output[i] = d["item"].clone();
            }
            t @ ("response.completed" | "response.incomplete") => {
                let r = &d["response"];
                if let Some(o) = r["output"].as_array().filter(|o| !o.is_empty()) {
                    self.output = o.clone();
                }
                self.output.retain(|o| !o.is_null());
                if let Some(m) = r["model"].as_str() {
                    self.model = m.into();
                }
                let u = &r["usage"];
                // Input counts the cached tokens and those written to the cache.
                let cached = u["input_tokens_details"]["cached_tokens"].as_u64().unwrap_or(0);
                let written = u["input_tokens_details"]["cache_write_tokens"].as_u64().unwrap_or(0);
                (self.usage.cache_read, self.usage.cache_write) = (cached, written);
                self.usage.input = u["input_tokens"].as_u64().unwrap_or(0).saturating_sub(cached + written);
                self.usage.output = u["output_tokens"].as_u64().unwrap_or(0);
                if self.text.is_empty() {
                    self.text = output_text(&self.output);
                }
                self.stop = Some(if t == "response.completed" {
                    Stop::Done
                } else {
                    match r["incomplete_details"]["reason"].as_str() {
                        Some("max_output_tokens") => Stop::MaxTokens,
                        Some("content_filter") => Stop::Refusal(None),
                        Some(other) => Stop::Other(other.into()),
                        None => Stop::Other("incomplete".into()),
                    }
                });
                self.ended = true;
            }
            "response.failed" => return Err(error_text(&d["response"]["error"])),
            "error" => return Err(error_text(if d["error"].is_object() { &d["error"] } else { d })),
            _ => {}
        }
        Ok(())
    }

    fn compatible(&mut self, d: &Value, out: &mut Vec<Delta>) -> Result<(), String> {
        if d["error"].is_object() {
            return Err(error_text(&d["error"]));
        }
        if let Some(m) = d["model"].as_str() {
            self.model = m.into();
        }
        let u = &d["usage"];
        if u.is_object() {
            let cached = u["prompt_tokens_details"]["cached_tokens"].as_u64().unwrap_or(0);
            self.usage.cache_read = cached;
            self.usage.input = u["prompt_tokens"].as_u64().unwrap_or(0).saturating_sub(cached);
            self.usage.output = u["completion_tokens"].as_u64().unwrap_or(0);
            self.usage.cost = u["cost"].as_f64();
        }
        let Some(c) = d["choices"].get(0) else { return Ok(()) };
        let delta = &c["delta"];
        if let Some(s) = delta["content"].as_str().filter(|s| !s.is_empty()) {
            self.text.push_str(s);
            out.push(Delta::Text(s.into()));
        }
        if let Some(s) = delta["reasoning"].as_str().filter(|s| !s.is_empty()) {
            self.thinking.push_str(s);
            out.push(Delta::Thinking(s.into()));
        }
        for tc in delta["tool_calls"].as_array().into_iter().flatten() {
            let i = tc["index"].as_u64().map_or(self.calls.len().saturating_sub(1), |i| i as usize);
            check_index(i)?;
            while self.calls.len() <= i {
                self.calls.push(Default::default());
            }
            let slot = &mut self.calls[i];
            if let Some(id) = tc["id"].as_str() {
                slot.0 = id.into();
            }
            if let Some(n) = tc["function"]["name"].as_str() {
                if slot.1.is_empty() {
                    out.push(Delta::Call(n.into()));
                }
                slot.1.push_str(n);
            }
            slot.2.push_str(tc["function"]["arguments"].as_str().unwrap_or(""));
        }
        if let Some(r) = c["finish_reason"].as_str() {
            self.stop = Some(match r {
                "stop" => Stop::Done,
                "tool_calls" | "function_call" => Stop::Tools,
                "length" => Stop::MaxTokens,
                "content_filter" => Stop::Refusal(None),
                other => Stop::Other(other.into()),
            });
            // Some local servers end without [DONE]; a finish reason is the end of the answer.
            self.ended = true;
        }
        Ok(())
    }
}

fn call(id: Option<&str>, name: Option<&str>, args: &str) -> ToolCall {
    let text = args.trim();
    let (input, bad) = match serde_json::from_str::<Value>(if text.is_empty() { "{}" } else { text }) {
        Ok(v @ Value::Object(_)) => (v, None),
        _ => (Value::Object(Map::new()), Some(args.to_string())),
    };
    ToolCall { id: id.unwrap_or("").into(), name: name.unwrap_or("").into(), input, bad }
}

fn output_text(output: &[Value]) -> String {
    output
        .iter()
        .filter(|o| o["type"] == "message")
        .flat_map(|o| o["content"].as_array().cloned().unwrap_or_default())
        .filter(|c| c["type"] == "output_text")
        .filter_map(|c| c["text"].as_str().map(str::to_string))
        .collect()
}

/// The most blocks, output items or tool calls one answer may index. The provider names the
/// index and the lists grow up to it, so a broken or hostile endpoint could ask for billions.
const MAX_INDEX: usize = 64;

fn check_index(i: usize) -> Result<(), String> {
    if i >= MAX_INDEX {
        Err(format!("the provider sent block index {i}, past the {MAX_INDEX} an answer may have"))
    } else {
        Ok(())
    }
}

/// A provider's error as one line: its message, else its type or code.
pub fn error_text(e: &Value) -> String {
    let msg = e["message"].as_str().or(e["error"]["message"].as_str());
    let kind = e["type"].as_str().or(e["code"].as_str());
    match (msg, kind) {
        (Some(m), _) => m.to_string(),
        (None, Some(k)) => k.to_string(),
        _ => "the provider sent an error".into(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Feed `stream` in chunks of `n` bytes.
    fn decode(kind: Kind, stream: &str, n: usize) -> (Result<Turn, String>, Vec<Delta>) {
        let mut d = Decoder::new(kind, "asked");
        let mut deltas = Vec::new();
        for c in stream.as_bytes().chunks(n) {
            match d.push(c) {
                Ok(x) => deltas.extend(x),
                Err(e) => return (Err(e), deltas),
            }
        }
        (d.finish(), deltas)
    }

    #[test]
    fn a_stream_cut_short_is_an_error() {
        let s =
            "event: message_start\ndata: {\"type\":\"message_start\",\"message\":{\"model\":\"m\",\"usage\":{}}}\n\n";
        assert!(decode(Kind::Anthropic, s, 7).0.is_err());
        let e = "event: error\ndata: {\"type\":\"error\",\"error\":{\"type\":\"overloaded_error\",\"message\":\"Overloaded\"}}\n\n";
        assert_eq!(decode(Kind::Anthropic, e, 5).0, Err("Overloaded".into()));
    }

    #[test]
    fn cache_writes_are_split_by_how_long_they_live() {
        let s = concat!(
            "event: message_start\ndata: {\"type\":\"message_start\",\"message\":{\"model\":\"claude-opus-5-5\",",
            "\"usage\":{\"input_tokens\":4,\"cache_read_input_tokens\":0,\"cache_creation_input_tokens\":14708,",
            "\"cache_creation\":{\"ephemeral_5m_input_tokens\":4330,\"ephemeral_1h_input_tokens\":10378},\"output_tokens\":1}}}\n\n",
            "event: message_delta\ndata: {\"type\":\"message_delta\",\"delta\":{\"stop_reason\":\"end_turn\"},\"usage\":{\"output_tokens\":90}}\n\n",
            "event: message_stop\ndata: {\"type\":\"message_stop\"}\n\n",
        );
        let u = decode(Kind::Anthropic, s, 9).0.unwrap().usage;
        assert_eq!((u.input, u.cache_write, u.cache_write_1h, u.output), (4, 4330, 10378, 90));
        let want = (4.0 * 4.0 + 4330.0 * 5.0 + 10378.0 * 8.0 + 90.0 * 20.0) / 1e6;
        assert!((u.dollars("claude-opus-5-5").unwrap() - want).abs() < 1e-12);
    }

    #[test]
    fn bad_tool_input_is_kept_for_the_error_answer() {
        let s = concat!(
            "data: {\"choices\":[{\"delta\":{\"tool_calls\":[{\"index\":0,\"id\":\"a\",\"function\":{\"name\":\"place\",\"arguments\":\"{\\\"cells\\\": [\"}}]}}]}\n\n",
            "data: {\"choices\":[{\"delta\":{},\"finish_reason\":\"tool_calls\"}]}\n\n",
        );
        let t = decode(Kind::Compatible, s, 3).0.unwrap();
        assert_eq!(t.calls[0].bad.as_deref(), Some("{\"cells\": ["));
        assert_eq!(t.calls[0].input, json!({}));
        assert_eq!(t.stop, Stop::Tools);
    }

    #[test]
    fn a_hostile_index_does_not_grow_the_answer() {
        let big = u32::MAX;
        for (kind, s) in [
            (
                Kind::Anthropic,
                format!(
                    "event: content_block_start\ndata: {{\"type\":\"content_block_start\",\"index\":{big},\"content_block\":{{\"type\":\"text\",\"text\":\"\"}}}}\n\n"
                ),
            ),
            (
                Kind::Compatible,
                format!(
                    "data: {{\"choices\":[{{\"delta\":{{\"tool_calls\":[{{\"index\":{big},\"id\":\"a\"}}]}}}}]}}\n\n"
                ),
            ),
            (
                Kind::OpenAi,
                format!("data: {{\"type\":\"response.output_item.done\",\"output_index\":{big},\"item\":{{}}}}\n\n"),
            ),
        ] {
            let e = decode(kind, &s, 1000).0.unwrap_err();
            assert!(e.contains("block index"), "{e}");
        }
    }
}
