//! The conversation as the app keeps it, and the request each client builds from it.
//!
//! The history only grows: each answer is kept as its provider sent it ([`Turn::raw`]) and
//! sent back unchanged to the same client, so Anthropic's thinking blocks (with their
//! signatures) and OpenAI's encrypted reasoning items replay as they must. An answer from
//! another client is rebuilt from its text and calls.

use serde_json::{Value, json};
use vxr_ops::Tool;

use crate::net::Request;
use crate::provider::{Kind, Settings, Usage};

/// One tool call from an answer.
#[derive(Clone, Debug, PartialEq)]
pub struct ToolCall {
    pub id: String,
    pub name: String,
    /// The parsed input; `{}` when it was not valid JSON.
    pub input: Value,
    /// The input as sent, when it was not valid JSON (the call is answered with an error).
    pub bad: Option<String>,
}

/// Why an answer ended.
#[derive(Clone, Debug, PartialEq)]
pub enum Stop {
    /// The model finished its answer.
    Done,
    /// It asked for tool calls.
    Tools,
    /// It ran out of output tokens: a tool call may be cut off, so none of this answer runs.
    MaxTokens,
    /// The provider declined the request (with its reason, if it gave one).
    Refusal(Option<String>),
    Other(String),
}

/// One answer: its text, its calls, what it used, and the provider's own form of it.
#[derive(Clone, Debug, PartialEq)]
pub struct Turn {
    pub kind: Kind,
    /// The model that answered, as the provider named it.
    pub model: String,
    pub text: String,
    /// What the model said of its thinking, if the provider shows it.
    pub thinking: String,
    pub calls: Vec<ToolCall>,
    pub stop: Stop,
    pub usage: Usage,
    /// Anthropic: the content blocks. OpenAI Responses: the output items. Chat Completions:
    /// the assistant message.
    pub raw: Value,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ToolResult {
    pub id: String,
    pub text: String,
    pub error: bool,
}

#[derive(Clone, Debug, PartialEq)]
pub enum Item {
    User(String),
    Assistant(Turn),
    Results(Vec<ToolResult>),
}

/// Output tokens one answer may use. Streaming lets it be large; effort medium keeps answers
/// short in practice (round 3 of risk test 1 had one Opus answer think past 16,000).
pub const MAX_TOKENS: u32 = 64_000;
/// Chat Completions servers include small local models whose context is the limit.
pub const COMPATIBLE_MAX_TOKENS: u32 = 16_000;

/// The request for the next answer: the system prompt, the tools and the whole history.
/// `key` goes into the headers only.
pub fn request(settings: &Settings, key: Option<&str>, system: &str, tools: &[Tool], items: &[Item]) -> Request {
    match settings.kind {
        Kind::Anthropic => anthropic(settings, key, system, tools, items),
        Kind::OpenAi => responses(settings, key, system, tools, items),
        Kind::Compatible => compatible(settings, key, system, tools, items),
    }
}

fn header(k: &str, v: &str) -> (String, String) {
    (k.into(), v.into())
}

/// Anthropic Messages: thinking adaptive with its summary shown, effort medium, the tools and
/// system prompt cached for an hour (so a request after a pause still reads them) and the
/// history for 5 minutes, and tool inputs streamed as they
/// are written.
fn anthropic(settings: &Settings, key: Option<&str>, system: &str, tools: &[Tool], items: &[Item]) -> Request {
    let mut messages: Vec<Value> = Vec::new();
    let mut push = |role: &str, mut content: Vec<Value>| {
        if content.is_empty() {
            return;
        }
        match messages.last_mut() {
            // One user message may carry tool results and then text; keep roles alternating.
            Some(m) if m["role"] == role => m["content"].as_array_mut().unwrap().append(&mut content),
            _ => messages.push(json!({"role": role, "content": content})),
        }
    };
    for item in items {
        match item {
            Item::User(text) => push("user", vec![json!({"type": "text", "text": text})]),
            Item::Results(rs) => push(
                "user",
                rs.iter()
                    .map(|r| {
                        let mut v = json!({"type": "tool_result", "tool_use_id": r.id, "content": r.text});
                        if r.error {
                            v["is_error"] = true.into();
                        }
                        v
                    })
                    .collect(),
            ),
            Item::Assistant(t) if t.kind == Kind::Anthropic => {
                push("assistant", t.raw.as_array().cloned().unwrap_or_default())
            }
            Item::Assistant(t) => {
                let mut c = Vec::new();
                if !t.text.is_empty() {
                    c.push(json!({"type": "text", "text": t.text}));
                }
                c.extend(
                    t.calls.iter().map(|k| json!({"type": "tool_use", "id": k.id, "name": k.name, "input": k.input})),
                );
                push("assistant", c)
            }
        }
    }
    let tools: Vec<Value> = tools
        .iter()
        .map(|t| {
            json!({"name": t.name, "description": t.description, "input_schema": t.parameters,
                "eager_input_streaming": true})
        })
        .collect();
    let body = json!({
        "model": settings.model,
        "max_tokens": MAX_TOKENS,
        "stream": true,
        "system": [{"type": "text", "text": system, "cache_control": {"type": "ephemeral", "ttl": "1h"}}],
        "tools": tools,
        "messages": messages,
        "thinking": {"type": "adaptive", "display": "summarized"},
        "output_config": {"effort": "medium"},
        "cache_control": {"type": "ephemeral"},
    });
    let mut headers = vec![header("content-type", "application/json"), header("anthropic-version", "2023-06-01")];
    if let Some(k) = key {
        headers.push(header("x-api-key", k));
    }
    if settings.browser {
        headers.push(header("anthropic-dangerous-direct-browser-access", "true"));
    }
    Request { url: "https://api.anthropic.com/v1/messages".into(), headers, body: body.to_string() }
}

/// OpenAI Responses, stateless: nothing is stored at OpenAI (`store: false`), so every
/// request sends the whole history, the model's reasoning as the encrypted items it gave.
fn responses(settings: &Settings, key: Option<&str>, system: &str, tools: &[Tool], items: &[Item]) -> Request {
    let mut input: Vec<Value> = Vec::new();
    for item in items {
        match item {
            Item::User(text) => input.push(json!({"role": "user", "content": [{"type": "input_text", "text": text}]})),
            Item::Results(rs) => input
                .extend(rs.iter().map(|r| json!({"type": "function_call_output", "call_id": r.id, "output": r.text}))),
            Item::Assistant(t) if t.kind == Kind::OpenAi => {
                input.extend(t.raw.as_array().into_iter().flatten().filter_map(replay_item))
            }
            Item::Assistant(t) => {
                if !t.text.is_empty() {
                    input.push(json!({"type": "message", "role": "assistant",
                        "content": [{"type": "output_text", "text": t.text}]}));
                }
                input.extend(t.calls.iter().map(|k| {
                    json!({"type": "function_call", "call_id": k.id, "name": k.name, "arguments": k.input.to_string()})
                }));
            }
        }
    }
    let tools: Vec<Value> = tools
        .iter()
        .map(|t| json!({"type": "function", "name": t.name, "description": t.description, "parameters": t.parameters}))
        .collect();
    let body = json!({
        "model": settings.model,
        "instructions": system,
        "input": input,
        "tools": tools,
        "reasoning": {"effort": "medium"},
        "max_output_tokens": MAX_TOKENS,
        "stream": true,
        "store": false,
        "include": ["reasoning.encrypted_content"],
    });
    let mut headers = vec![header("content-type", "application/json")];
    if let Some(k) = key {
        headers.push(header("authorization", &format!("Bearer {k}")));
    }
    Request { url: "https://api.openai.com/v1/responses".into(), headers, body: body.to_string() }
}

/// An output item as input again. With nothing stored, ids other than a reasoning item's
/// name nothing OpenAI can find, so they go; reasoning keeps its id and encrypted content.
fn replay_item(item: &Value) -> Option<Value> {
    let mut v = item.clone();
    let o = v.as_object_mut()?;
    match o.get("type").and_then(Value::as_str)? {
        "reasoning" => {
            o.remove("status");
        }
        "message" | "function_call" => {
            o.remove("id");
            o.remove("status");
        }
        _ => return None,
    }
    Some(v)
}

/// OpenAI-compatible Chat Completions with a base URL (OpenRouter, Ollama, LM Studio).
fn compatible(settings: &Settings, key: Option<&str>, system: &str, tools: &[Tool], items: &[Item]) -> Request {
    let mut messages = vec![json!({"role": "system", "content": system})];
    for item in items {
        match item {
            Item::User(text) => messages.push(json!({"role": "user", "content": text})),
            Item::Results(rs) => {
                messages.extend(rs.iter().map(|r| json!({"role": "tool", "tool_call_id": r.id, "content": r.text})))
            }
            Item::Assistant(t) if t.kind == Kind::Compatible && t.raw.is_object() => messages.push(t.raw.clone()),
            Item::Assistant(t) => {
                if t.text.is_empty() && t.calls.is_empty() {
                    continue;
                }
                let mut m = json!({"role": "assistant", "content": if t.text.is_empty() { Value::Null } else { t.text.clone().into() }});
                if !t.calls.is_empty() {
                    m["tool_calls"] = t
                        .calls
                        .iter()
                        .map(|k| json!({"id": k.id, "type": "function", "function": {"name": k.name, "arguments": k.input.to_string()}}))
                        .collect();
                }
                messages.push(m);
            }
        }
    }
    let tools: Vec<Value> = tools
        .iter()
        .map(|t| json!({"type": "function", "function": {"name": t.name, "description": t.description, "parameters": t.parameters}}))
        .collect();
    let body = json!({
        "model": settings.model,
        "messages": messages,
        "tools": tools,
        "max_tokens": COMPATIBLE_MAX_TOKENS,
        "stream": true,
        "stream_options": {"include_usage": true},
    });
    let mut headers = vec![header("content-type", "application/json"), header("x-title", "vxr")];
    if let Some(k) = key.filter(|k| !k.is_empty()) {
        headers.push(header("authorization", &format!("Bearer {k}")));
    }
    let url = format!("{}/chat/completions", settings.base.trim().trim_end_matches('/'));
    Request { url, headers, body: body.to_string() }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tools() -> Vec<Tool> {
        vxr_ops::tools().into_iter().filter(|t| t.name == "place" || t.name == "done").collect()
    }

    /// A request, an answer with thinking and a call, its result, and a second request.
    fn history(kind: Kind, raw: Value) -> Vec<Item> {
        let call = ToolCall {
            id: "c1".into(),
            name: "place".into(),
            input: json!({"cells": [[0, 0, 0]], "slot": 3}),
            bad: None,
        };
        vec![
            Item::User("Request: add a dot".into()),
            Item::Assistant(Turn {
                kind,
                model: "m".into(),
                text: "Adding it.".into(),
                thinking: String::new(),
                calls: vec![call],
                stop: Stop::Tools,
                usage: Usage::default(),
                raw,
            }),
            Item::Results(vec![ToolResult { id: "c1".into(), text: "1 cells changed".into(), error: false }]),
            Item::User("Request: and another".into()),
        ]
    }

    fn body(r: &Request) -> Value {
        serde_json::from_str(&r.body).unwrap()
    }

    #[test]
    fn anthropic_replays_its_blocks_and_merges_user_turns() {
        let raw = json!([
            {"type": "thinking", "thinking": "a dot", "signature": "sig=="},
            {"type": "text", "text": "Adding it."},
            {"type": "tool_use", "id": "c1", "name": "place", "input": {"cells": [[0, 0, 0]], "slot": 3}}
        ]);
        let mut s = Settings::new(Kind::Anthropic, true);
        let r = request(&s, Some("sk-ant-k"), "sys", &tools(), &history(Kind::Anthropic, raw.clone()));
        assert_eq!(r.url, "https://api.anthropic.com/v1/messages");
        assert!(r.headers.contains(&("x-api-key".into(), "sk-ant-k".into())));
        assert!(r.headers.contains(&("anthropic-dangerous-direct-browser-access".into(), "true".into())));
        assert!(!r.body.contains("sk-ant-k"));
        let b = body(&r);
        assert_eq!(b["model"], "claude-opus-5-5");
        assert_eq!(b["output_config"]["effort"], "medium");
        assert_eq!(b["stream"], true);
        assert!(b.get("fallbacks").is_none(), "the model picked answers, no other");
        assert!(!r.headers.iter().any(|(k, _)| k == "anthropic-beta"));
        assert_eq!(b["system"][0]["cache_control"], json!({"type": "ephemeral", "ttl": "1h"}));
        assert_eq!(b["cache_control"], json!({"type": "ephemeral"}), "the history for 5 minutes");
        assert_eq!(b["tools"][0]["name"], "place");
        assert_eq!(b["tools"][0]["eager_input_streaming"], true);
        assert!(b["tools"][0]["input_schema"]["properties"]["cells"].is_object());
        let m = b["messages"].as_array().unwrap();
        assert_eq!(m.len(), 3);
        assert_eq!(m[1]["content"], raw, "the answer goes back unchanged, thinking and all");
        assert_eq!(m[2]["role"], "user");
        assert_eq!(m[2]["content"][0]["type"], "tool_result");
        assert_eq!(m[2]["content"][1]["text"], "Request: and another");
        s.browser = false;
        let r = request(&s, Some("k"), "sys", &tools(), &history(Kind::Anthropic, raw));
        assert!(!r.headers.iter().any(|(k, _)| k == "anthropic-dangerous-direct-browser-access"));
    }

    #[test]
    fn responses_sends_everything_and_stores_nothing() {
        let raw = json!([
            {"type": "reasoning", "id": "rs_1", "summary": [], "encrypted_content": "enc"},
            {"type": "message", "id": "msg_1", "status": "completed", "role": "assistant",
                "content": [{"type": "output_text", "text": "Adding it.", "annotations": []}]},
            {"type": "function_call", "id": "fc_1", "status": "completed", "call_id": "c1", "name": "place",
                "arguments": "{\"cells\":[[0,0,0]],\"slot\":3}"}
        ]);
        let s = Settings::new(Kind::OpenAi, false);
        let r = request(&s, Some("sk-o"), "sys", &tools(), &history(Kind::OpenAi, raw));
        assert_eq!(r.url, "https://api.openai.com/v1/responses");
        assert!(r.headers.contains(&("authorization".into(), "Bearer sk-o".into())));
        let b = body(&r);
        assert_eq!((b["store"].clone(), b["model"].clone()), (json!(false), json!("gpt-6.1-sol")));
        assert_eq!(b["include"][0], "reasoning.encrypted_content");
        assert_eq!(b["instructions"], "sys");
        assert_eq!(b["tools"][0]["type"], "function");
        let input = b["input"].as_array().unwrap();
        let types: Vec<&str> = input.iter().map(|i| i["type"].as_str().unwrap_or("user")).collect();
        assert_eq!(types, ["user", "reasoning", "message", "function_call", "function_call_output", "user"]);
        assert_eq!(input[1]["id"], "rs_1");
        assert!(input[2].get("id").is_none() && input[3].get("status").is_none());
        assert_eq!(input[4]["call_id"], "c1");
    }

    #[test]
    fn compatible_goes_to_its_base_url_and_rebuilds_other_answers() {
        let mut s = Settings::new(Kind::Compatible, false);
        s.base = "http://127.0.0.1:11434/v1/".into();
        // An answer from another client is rebuilt from its text and calls.
        let r = request(&s, None, "sys", &tools(), &history(Kind::Anthropic, json!([])));
        assert_eq!(r.url, "http://127.0.0.1:11434/v1/chat/completions");
        assert!(!r.headers.iter().any(|(k, _)| k == "authorization"), "a local server needs no key");
        let b = body(&r);
        let m = b["messages"].as_array().unwrap();
        assert_eq!(
            m.iter().map(|m| m["role"].as_str().unwrap()).collect::<Vec<_>>(),
            ["system", "user", "assistant", "tool", "user"]
        );
        assert_eq!(m[2]["tool_calls"][0]["function"]["name"], "place");
        assert_eq!(
            serde_json::from_str::<Value>(m[2]["tool_calls"][0]["function"]["arguments"].as_str().unwrap()).unwrap()["slot"],
            3
        );
        assert_eq!(b["stream_options"]["include_usage"], true);
    }
}
