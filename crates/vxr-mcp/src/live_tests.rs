//! Live mode over a real socket: a [`Host`] in a temporary folder answered by the test, as
//! the app's thread answers it, and [`Remote`]s and raw connections against it.

use std::io::{BufRead, BufReader, Write};
use std::os::unix::fs::PermissionsExt;
use std::os::unix::net::UnixStream;
use std::path::PathBuf;
use std::time::{Duration, Instant};

use serde_json::{Value, json};

use crate::live::{Ask, Host, Live, NOT_LIVE, REFUSED, Remote, SOCKET, STOPPED, TOKEN};
use crate::protocol::{Server, VERSION};

/// A folder whose `live` subfolder the host makes, removed at the end of the test. Kept
/// short: a socket path has to fit in about 100 bytes.
struct Dir(PathBuf);

impl Dir {
    fn new(name: &str) -> Dir {
        let p = std::env::temp_dir().join(format!("vxl-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&p);
        std::fs::create_dir_all(&p).unwrap();
        Dir(std::fs::canonicalize(p).unwrap().join("live"))
    }
}

impl Drop for Dir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(self.0.parent().unwrap());
    }
}

/// `vxr mcp --live` as the agent's MCP client sees it.
struct Agent {
    server: Server<Live<Remote>>,
    next: u64,
}

impl Agent {
    fn new(dir: &Dir, client: &str) -> Agent {
        let mut server = Server::with(Live(Remote::new(dir.0.clone())));
        let init = json!({"jsonrpc": "2.0", "id": 0, "method": "initialize",
            "params": {"protocolVersion": "2025-06-18", "capabilities": {}, "clientInfo": {"name": client, "version": "1"}}});
        server.handle(&init).unwrap();
        Agent { server, next: 0 }
    }

    fn request(&mut self, method: &str, mut params: Value) -> Value {
        params["_meta"] = json!({"io.modelcontextprotocol/protocolVersion": VERSION});
        self.next += 1;
        let msg = json!({"jsonrpc": "2.0", "id": self.next, "method": method, "params": params});
        self.server.handle(&msg).expect("a reply")
    }
}

/// Run `f` (an agent's request) while answering the host's requests as the app does: a call
/// runs `ran <tool> <args>`, one with a `fail` argument fails, the summary reads `summary`, other uris are missing.
fn answering<T: Send>(host: &Host, f: impl FnOnce() -> T + Send) -> T {
    std::thread::scope(|s| {
        let h = s.spawn(f);
        let start = Instant::now();
        while !h.is_finished() {
            for r in host.poll() {
                match r.ask.clone() {
                    Ask::Call { args, .. } if args.get("fail").is_some() => r.answer(Err("it failed".into())),
                    Ask::Call { name, args } => {
                        let who = r.client.clone();
                        r.answer(Ok(format!("{who} ran {name} {args}")))
                    }
                    Ask::Read(uri) if uri == "vxr://scene/summary" => r.answer(Ok("summary".into())),
                    Ask::Read(_) => r.missing(),
                }
            }
            assert!(start.elapsed() < Duration::from_secs(20), "the agent's request never finished");
            std::thread::sleep(Duration::from_millis(2));
        }
        h.join().unwrap()
    })
}

/// A tool call's text and whether it was an error.
fn tool(host: &Host, a: &mut Agent, name: &str, args: Value) -> (String, bool) {
    let r = answering(host, || a.request("tools/call", json!({"name": name, "arguments": args})));
    let r = &r["result"];
    (r["content"][0]["text"].as_str().unwrap().to_string(), r["isError"].as_bool().unwrap())
}

/// A raw connection's first line back after sending `hello`, and (when `refused`) whether it
/// then hung up.
fn raw(dir: &Dir, hello: &str, refused: bool) -> (Value, bool) {
    let s = UnixStream::connect(dir.0.join(SOCKET)).unwrap();
    s.set_read_timeout(Some(Duration::from_secs(10))).unwrap();
    let mut w = s.try_clone().unwrap();
    w.write_all(hello.as_bytes()).unwrap();
    let mut r = BufReader::new(s);
    let mut line = String::new();
    r.read_line(&mut line).unwrap();
    let mut rest = String::new();
    let closed = refused && r.read_line(&mut rest).map(|n| n == 0).unwrap_or(false);
    (serde_json::from_str(&line).unwrap(), closed)
}

fn token(dir: &Dir) -> String {
    std::fs::read_to_string(dir.0.join(TOKEN)).unwrap()
}

fn wait_for(mut f: impl FnMut() -> bool) {
    let start = Instant::now();
    while !f() {
        assert!(start.elapsed() < Duration::from_secs(5), "timed out");
        std::thread::sleep(Duration::from_millis(5));
    }
}

#[test]
fn a_call_goes_over_the_socket_and_back() {
    let dir = Dir::new("round");
    let host = Host::start(&dir.0).unwrap();
    // The folder only this user can open, the token only this user can read.
    let mode = |p: PathBuf| std::fs::metadata(p).unwrap().permissions().mode() & 0o777;
    assert_eq!(mode(dir.0.clone()), 0o700);
    assert_eq!(mode(dir.0.join(TOKEN)), 0o600);
    assert_eq!(token(&dir).len(), 64);

    let mut a = Agent::new(&dir, "claude-code");
    // Listing tools needs no window: the app is reached on the first call.
    let r = a.request("tools/list", json!({}));
    let names: Vec<&str> =
        r["result"]["tools"].as_array().unwrap().iter().map(|t| t["name"].as_str().unwrap()).collect();
    for op in vxr_ops::tools() {
        assert!(names.contains(&op.name), "{} is a tool", op.name);
    }
    assert!(names.contains(&"edit_object"));
    for files_only in ["list_files", "open", "save", "new_scene"] {
        assert!(!names.contains(&files_only), "{files_only} is files mode only");
    }
    assert!(host.clients().is_empty());

    let (t, e) = tool(&host, &mut a, "paint", json!({"slot": 3}));
    assert!(!e);
    assert_eq!(t, r#"Claude Code ran paint {"slot":3}"#);
    assert_eq!(host.clients().iter().map(|c| c.1.as_str()).collect::<Vec<_>>(), ["Claude Code"]);
    assert_eq!(tool(&host, &mut a, "paint", json!({"fail": true})), ("it failed".into(), true));

    let r = answering(&host, || a.request("resources/read", json!({"uri": "vxr://scene/summary"})));
    assert_eq!(r["result"]["contents"][0]["text"], "summary");
    let r = answering(&host, || a.request("resources/read", json!({"uri": "vxr://scene/palette"})));
    assert_eq!(r["error"]["code"], -32602);
    let r = a.request("resources/read", json!({"uri": "vxr://file/a.vox"}));
    assert_eq!(r["error"]["code"], -32602);

    // A second window cannot take the socket.
    assert!(Host::start(&dir.0).err().unwrap().contains("another vxr window"));
}

#[test]
fn a_wrong_or_missing_token_is_refused() {
    let dir = Dir::new("token");
    let host = Host::start(&dir.0).unwrap();
    let wrong = "0".repeat(64);
    for hello in [
        format!("{{\"vxr\": \"live\", \"token\": \"{wrong}\", \"client\": \"x\"}}\n"),
        "{\"vxr\": \"live\", \"client\": \"x\"}\n".to_string(),
        format!("{{\"vxr\": \"live\", \"token\": \"{}\"}}\n", &token(&dir)[..63]),
        "{\"call\": \"paint\", \"args\": {}}\n".to_string(),
        "not json\n".to_string(),
    ] {
        let (reply, closed) = raw(&dir, &hello, true);
        assert!(reply.get("error").is_some() && reply.get("ok").is_none(), "{hello} got {reply}");
        assert!(closed, "{hello}: the connection closes");
    }
    assert!(host.clients().is_empty());
    assert!(host.poll().is_empty(), "nothing unauthenticated reaches the app");

    // The right token is let in.
    let (reply, _) = raw(&dir, &format!("{{\"vxr\": \"live\", \"token\": \"{}\"}}\n", token(&dir)), false);
    assert_eq!(reply, json!({"ok": true}));

    // A server holding a token that is no longer the file's is turned away for good.
    let mut a = Agent::new(&dir, "codex");
    assert!(!tool(&host, &mut a, "scene_summary", json!({})).1);
    drop(host);
    std::fs::write(dir.0.join(TOKEN), &wrong).unwrap();
    let host = Host::start(&dir.0).unwrap();
    assert_eq!(token(&dir), wrong, "a token file that holds a token is kept");
    let mut b = Agent::new(&dir, "codex");
    assert!(!tool(&host, &mut b, "scene_summary", json!({})).1, "a new server reads the file");
    // The old server finds its connection closed, then is refused when it connects again.
    assert_eq!(tool(&host, &mut a, "scene_summary", json!({})), (NOT_LIVE.into(), true));
    assert_eq!(tool(&host, &mut a, "scene_summary", json!({})), (REFUSED.into(), true));
}

#[test]
fn a_new_token_disconnects_every_agent() {
    let dir = Dir::new("regen");
    let host = Host::start(&dir.0).unwrap();
    let old = token(&dir);
    let mut a = Agent::new(&dir, "claude-code");
    let mut b = Agent::new(&dir, "cursor");
    assert!(!tool(&host, &mut a, "place", json!({})).1);
    assert!(!tool(&host, &mut b, "place", json!({})).1);
    assert_eq!(host.clients().len(), 2);

    host.regenerate().unwrap();
    assert_ne!(token(&dir), old);
    assert_eq!(std::fs::metadata(dir.0.join(TOKEN)).unwrap().permissions().mode() & 0o777, 0o600);
    assert!(host.clients().is_empty());
    for agent in [&mut a, &mut b] {
        assert_eq!(tool(&host, agent, "place", json!({})), (REFUSED.into(), true));
        assert!(!agent.server.backend().0.connected());
    }
    // A raw connection with the old token is refused too; a restarted server pairs again.
    let (reply, _) = raw(&dir, &format!("{{\"vxr\": \"live\", \"token\": \"{old}\"}}\n"), true);
    assert!(reply.get("error").is_some());
    let mut c = Agent::new(&dir, "claude-code");
    assert!(!tool(&host, &mut c, "place", json!({})).1);
}

#[test]
fn stop_ends_one_agent_for_good() {
    let dir = Dir::new("stop");
    let host = Host::start(&dir.0).unwrap();
    let mut a = Agent::new(&dir, "claude-code");
    let mut b = Agent::new(&dir, "codex");
    assert!(!tool(&host, &mut a, "erase", json!({})).1);
    assert!(!tool(&host, &mut b, "erase", json!({})).1);
    let id = host.clients().iter().find(|c| c.1 == "Claude Code").unwrap().0;

    host.stop(id);
    wait_for(|| host.clients().len() == 1);
    assert_eq!(host.clients()[0].1, "Codex");
    // The stopped server says so, and stays stopped: no reconnect on the next call.
    assert_eq!(tool(&host, &mut a, "erase", json!({})), (STOPPED.into(), true));
    assert_eq!(tool(&host, &mut a, "erase", json!({})), (STOPPED.into(), true));
    assert_eq!(host.clients().len(), 1);
    assert!(!tool(&host, &mut b, "erase", json!({})).1, "the other agent goes on");
}

#[test]
fn turning_live_editing_off_and_on_keeps_the_pairing() {
    let dir = Dir::new("off");
    // Nothing listening yet: the agent is told how to turn it on.
    let mut a = Agent::new(&dir, "claude-code");
    let r = a.request("tools/call", json!({"name": "paint", "arguments": {}}));
    assert_eq!(r["result"]["content"][0]["text"], NOT_LIVE);
    assert_eq!(r["result"]["isError"], true);

    let host = Host::start(&dir.0).unwrap();
    let t = token(&dir);
    assert!(!tool(&host, &mut a, "paint", json!({})).1);
    drop(host);
    assert!(!dir.0.join(SOCKET).exists(), "off removes the socket");
    let r = a.request("tools/call", json!({"name": "paint", "arguments": {}}));
    assert_eq!(r["result"]["content"][0]["text"], NOT_LIVE);

    let host = Host::start(&dir.0).unwrap();
    assert_eq!(token(&dir), t);
    assert!(!tool(&host, &mut a, "paint", json!({})).1, "the same server connects again");
}

#[test]
fn a_planted_folder_or_token_is_not_trusted() {
    let dir = Dir::new("plant");
    std::fs::create_dir(&dir.0).unwrap();
    std::fs::set_permissions(&dir.0, std::fs::Permissions::from_mode(0o755)).unwrap();
    assert!(Host::start(&dir.0).err().unwrap().contains("only you can open"));
    std::fs::set_permissions(&dir.0, std::fs::Permissions::from_mode(0o700)).unwrap();
    // A token others can read is replaced.
    let wrong = "1".repeat(64);
    std::fs::write(dir.0.join(TOKEN), &wrong).unwrap();
    std::fs::set_permissions(dir.0.join(TOKEN), std::fs::Permissions::from_mode(0o644)).unwrap();
    let _host = Host::start(&dir.0).unwrap();
    assert_ne!(token(&dir), wrong);
    assert_eq!(std::fs::metadata(dir.0.join(TOKEN)).unwrap().permissions().mode() & 0o777, 0o600);
}
