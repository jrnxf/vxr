//! Each client's stream decoded from a recorded fixture, in chunks of every size from one byte
//! up, and sent back in the next request as the provider expects it.

use serde_json::{Value, json};
use vxr_ai::Turn;
use vxr_ai::chat::{Item, Stop, ToolResult, request};
use vxr_ai::provider::{Kind, Settings};
use vxr_ai::stream::{Decoder, Delta};

fn fixture(name: &str) -> String {
    let path = format!("{}/tests/fixtures/{name}", env!("CARGO_MANIFEST_DIR"));
    std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("{path}: {e}"))
}

/// The stream's turn, the same for every chunk size, and its deltas.
fn decode(kind: Kind, stream: &str) -> (Turn, Vec<Delta>) {
    let mut first: Option<(Turn, Vec<Delta>)> = None;
    for n in [1, 2, 3, 7, 64, 1000, stream.len()] {
        let mut d = Decoder::new(kind, "asked");
        let mut deltas = Vec::new();
        for c in stream.as_bytes().chunks(n) {
            deltas.extend(d.push(c).unwrap());
        }
        let turn = d.finish().unwrap();
        // Text deltas can split differently; joined they are the same.
        let joined = |ds: &[Delta]| -> (String, String, Vec<String>) {
            let mut t = (String::new(), String::new(), Vec::new());
            for d in ds {
                match d {
                    Delta::Text(s) => t.0.push_str(s),
                    Delta::Thinking(s) => t.1.push_str(s),
                    Delta::Call(c) => t.2.push(c.clone()),
                }
            }
            t
        };
        match &first {
            None => first = Some((turn, deltas)),
            Some((t, ds)) => {
                assert_eq!(&turn, t, "chunks of {n}");
                assert_eq!(joined(&deltas), joined(ds), "chunks of {n}");
            }
        }
    }
    first.unwrap()
}

fn next_request(kind: Kind, turn: &Turn) -> Value {
    let items = vec![
        Item::User("Request: make the roof red".into()),
        Item::Assistant(turn.clone()),
        Item::Results(vec![ToolResult {
            id: turn.calls[0].id.clone(),
            text: "392 cells changed".into(),
            error: false,
        }]),
    ];
    let r = request(&Settings::new(kind, false), Some("k"), "sys", &vxr_ops::tools(), &items);
    serde_json::from_str(&r.body).unwrap()
}

#[test]
fn anthropic_messages() {
    let (t, deltas) = decode(Kind::Anthropic, &fixture("anthropic-paint.sse"));
    assert_eq!(t.model, "claude-opus-5-5");
    assert_eq!(t.text, "I'll paint the roof red.");
    assert_eq!(t.thinking, "The roof is part p3; slot 3 is unused.");
    assert_eq!(t.stop, Stop::Tools);
    assert_eq!(t.calls.len(), 1);
    assert_eq!((t.calls[0].id.as_str(), t.calls[0].name.as_str()), ("toolu_01Fixture", "paint"));
    assert_eq!(t.calls[0].input, json!({"part": "p3", "slot": 3}));
    assert_eq!((t.usage.input, t.usage.cache_write, t.usage.output), (12, 4100, 87));
    assert!(deltas.contains(&Delta::Call("paint".into())));
    // The blocks go back whole: the thinking with its signature, the tool input as an object.
    let b = next_request(Kind::Anthropic, &t);
    let blocks = &b["messages"][1]["content"];
    assert_eq!(blocks[0]["type"], "thinking");
    assert_eq!(blocks[0]["signature"], "EqQBCkgIARABGAIiQFixture==");
    assert_eq!(blocks[2]["input"], json!({"part": "p3", "slot": 3}));
    assert_eq!(b["messages"][2]["content"][0]["tool_use_id"], "toolu_01Fixture");
}

#[test]
fn openai_responses() {
    let (t, deltas) = decode(Kind::OpenAi, &fixture("responses-paint.sse"));
    assert_eq!(t.model, "gpt-6.1-sol");
    assert_eq!(t.stop, Stop::Tools);
    assert_eq!((t.calls[0].id.as_str(), t.calls[0].name.as_str()), ("call_fixture", "paint"));
    assert_eq!(t.calls[0].input, json!({"part": "p3", "slot": 3}));
    assert_eq!((t.usage.input, t.usage.cache_read, t.usage.output), (5000 - 4096, 4096, 120));
    assert_eq!(deltas, vec![Delta::Call("paint".into())]);
    let b = next_request(Kind::OpenAi, &t);
    let input = b["input"].as_array().unwrap();
    assert_eq!(input[1]["encrypted_content"], "gAAAAABfixture");
    assert_eq!(input[2]["call_id"], "call_fixture");
    assert!(input[2].get("id").is_none());
    assert_eq!(input[3]["type"], "function_call_output");
}

#[test]
fn openrouter_chat_completions() {
    let (t, _) = decode(Kind::Compatible, &fixture("openrouter-paint.sse"));
    assert_eq!(t.model, "anthropic/claude-opus-5.5");
    assert_eq!(t.text, "Painting the roof.");
    assert_eq!(t.stop, Stop::Tools);
    assert_eq!(t.calls[0].input, json!({"part": "p3", "slot": 3}));
    assert_eq!(t.usage.cost, Some(0.0224));
    assert_eq!(t.usage.dollars(&t.model), Some(0.0224));
    let b = next_request(Kind::Compatible, &t);
    assert_eq!(b["messages"][2]["tool_calls"][0]["id"], "toolu_or");
    assert_eq!(b["messages"][3]["role"], "tool");
}
