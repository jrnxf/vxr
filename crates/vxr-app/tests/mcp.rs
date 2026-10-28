//! `vxr mcp` as an agent runs it: the built binary, newline-delimited JSON-RPC on its
//! stdin and stdout.

use std::io::{BufRead, BufReader, Write};
use std::process::{Command, Stdio};

fn vxr() -> Command {
    Command::new(env!("CARGO_BIN_EXE_vxr"))
}

#[test]
fn builds_and_saves_a_scene_over_stdio() {
    let dir = std::env::temp_dir().join(format!("vxr-mcp-stdio-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let mut child = vxr()
        .args(["mcp", "--root"])
        .arg(&dir)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::inherit())
        .spawn()
        .unwrap();
    let mut stdin = child.stdin.take().unwrap();
    let mut stdout = BufReader::new(child.stdout.take().unwrap());
    let meta = r#""_meta":{"io.modelcontextprotocol/protocolVersion":"2026-07-28","io.modelcontextprotocol/clientInfo":{"name":"t","version":"0"},"io.modelcontextprotocol/clientCapabilities":{}}"#;
    let mut ask = |id: u32, method: &str, params: &str| -> serde_json::Value {
        writeln!(
            stdin,
            r#"{{"jsonrpc":"2.0","id":{id},"method":"{method}","params":{{{params}{}}}}}"#,
            if params.is_empty() { meta.to_string() } else { format!(",{meta}") }
        )
        .unwrap();
        stdin.flush().unwrap();
        let mut line = String::new();
        stdout.read_line(&mut line).unwrap();
        let v: serde_json::Value = serde_json::from_str(&line).unwrap_or_else(|e| panic!("{e}: {line}"));
        assert_eq!(v["id"], id);
        v
    };

    let r = ask(1, "server/discover", "");
    assert_eq!(r["result"]["supportedVersions"][0], "2026-07-28");
    let r = ask(2, "tools/list", "");
    assert!(r["result"]["tools"].as_array().unwrap().iter().any(|t| t["name"] == "box"));
    let call = |name: &str, args: &str| format!(r#""name":"{name}","arguments":{args}"#);
    let r = ask(3, "tools/call", &call("new_scene", r#"{"path":"hut.vox","size":[8,8,8]}"#));
    assert_eq!(r["result"]["isError"], false);
    let r = ask(4, "tools/call", &call("box", r#"{"from":[0,0,0],"to":[3,2,3],"slot":2}"#));
    assert_eq!(r["result"]["isError"], false, "{r}");
    let r = ask(5, "tools/call", &call("save", "{}"));
    assert_eq!(r["result"]["isError"], false, "{r}");
    let r = ask(6, "resources/list", "");
    assert!(r["result"]["resources"].as_array().unwrap().iter().any(|x| x["uri"] == "vxr://file/hut.vox"));

    // Closing stdin ends the server.
    drop(stdin);
    assert!(child.wait().unwrap().success());
    let bytes = std::fs::read(dir.join("hut.vox")).unwrap();
    assert_eq!(&bytes[..4], b"VOX ");
    std::fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn refuses_a_missing_or_bad_root() {
    for args in [&["mcp"][..], &["mcp", "--root"], &["mcp", "--root", "/nonexistent-vxr-root"], &["mcp", "--other"]] {
        let out = vxr().args(args).output().unwrap();
        assert_eq!(out.status.code(), Some(1), "{args:?}");
        assert!(String::from_utf8_lossy(&out.stderr).starts_with("vxr: "), "{args:?}");
        assert!(out.stdout.is_empty());
    }
}
