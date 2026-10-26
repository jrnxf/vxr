//! Phase AI spike, test 5: a throwaway `vxr mcp` files mode.
//!
//! `vxr-mcp --root <dir> [--log <file>]` is a stdio MCP server (newline-delimited JSON-RPC)
//! that opens, edits and saves `.vox` files under `<dir>` through `vxr_core::Scene`, so a
//! save goes through the same lossless path as the editor's. It exposes the first-version
//! operations the test needs, one tool per operation. `--log` appends one JSON line per
//! tool call (tool, arguments, time, result) for the numbers in `RESULTS.md`.

mod ops;

use std::io::{BufRead, Write};
use std::path::PathBuf;

use serde_json::{Value, json};

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let flag = |name: &str| args.iter().position(|a| a == name).and_then(|i| args.get(i + 1)).cloned();
    let Some(root) = flag("--root") else {
        eprintln!("usage: vxr-mcp --root <dir> [--log <file>]");
        std::process::exit(2);
    };
    let root = match std::fs::canonicalize(&root) {
        Ok(r) => r,
        Err(e) => {
            eprintln!("vxr-mcp: {root}: {e}");
            std::process::exit(2);
        }
    };
    let log = flag("--log").map(PathBuf::from);
    let mut server = ops::Server::new(root, log);
    let stdin = std::io::stdin();
    let mut out = std::io::stdout().lock();
    for line in stdin.lock().lines() {
        let Ok(line) = line else { break };
        if line.trim().is_empty() {
            continue;
        }
        let reply = match serde_json::from_str::<Value>(&line) {
            Ok(msg) => handle(&mut server, &msg),
            Err(e) => Some(json!({"jsonrpc": "2.0", "id": null, "error": {"code": -32700, "message": e.to_string()}})),
        };
        if let Some(r) = reply {
            let _ = writeln!(out, "{r}");
            let _ = out.flush();
        }
    }
}

/// One JSON-RPC message in, at most one reply out (none for notifications).
fn handle(server: &mut ops::Server, msg: &Value) -> Option<Value> {
    let id = msg.get("id").cloned()?;
    let method = msg.get("method").and_then(Value::as_str).unwrap_or("");
    let params = msg.get("params").cloned().unwrap_or(Value::Null);
    let result = match method {
        "initialize" => Ok(json!({
            // Answer with the client's revision: the spike speaks the common subset.
            "protocolVersion": params.get("protocolVersion").cloned().unwrap_or(json!("2025-06-18")),
            "capabilities": {"tools": {}},
            "serverInfo": {"name": "vxr", "version": "0.0.0-spike"},
            "instructions": ops::INSTRUCTIONS,
        })),
        "ping" => Ok(json!({})),
        "tools/list" => Ok(json!({"tools": ops::tools()})),
        "tools/call" => {
            let name = params.get("name").and_then(Value::as_str).unwrap_or("");
            let args = params.get("arguments").cloned().unwrap_or(json!({}));
            let (text, is_error) = match server.call(name, &args) {
                Ok(t) => (t, false),
                Err(e) => (format!("error: {e}"), true),
            };
            Ok(json!({"content": [{"type": "text", "text": text}], "isError": is_error}))
        }
        _ => Err(json!({"code": -32601, "message": format!("method not found: {method}")})),
    };
    Some(match result {
        Ok(r) => json!({"jsonrpc": "2.0", "id": id, "result": r}),
        Err(e) => json!({"jsonrpc": "2.0", "id": id, "error": e}),
    })
}
