//! `vxr bridge` over real sockets: a [`Bridge`] on a free loopback port with its live folder in
//! a temporary folder, pages connecting as a browser would (a WebSocket with an `Origin`), and
//! agents connecting as `vxr mcp --live` does.

use std::net::{SocketAddr, TcpStream};
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use serde_json::{Value, json};
use tungstenite::client::IntoClientRequest;
use tungstenite::http::HeaderValue;
use tungstenite::{Message, WebSocket};

use crate::bridge::{
    BUSY, Bridge, Config, MAX_FAILURES, NO_HELLO, ORIGIN, Running, WRONG_CODE, code_char, code_char_skipped,
    normalize_code, show_code,
};
use crate::live::{Live, NOT_LIVE, REFUSED, Remote, SOCKET, STOPPED, TOKEN};
use crate::protocol::{Server, VERSION};

/// A folder whose `live` subfolder the bridge's host makes, removed at the end of the test.
struct Dir(PathBuf);

impl Dir {
    fn new(name: &str) -> Dir {
        let p = std::env::temp_dir().join(format!("vxb-{name}-{}", std::process::id()));
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

/// A bridge on a free port, with what it printed.
fn bridge(dir: &Dir, dev: bool) -> (Running, Arc<Mutex<Vec<String>>>) {
    let log = Arc::new(Mutex::new(Vec::new()));
    let l = log.clone();
    let cfg = Config { addr: SocketAddr::from(([127, 0, 0, 1], 0)), dir: dir.0.clone(), dev_origins: dev };
    let b = Bridge::bind(cfg, move |line| l.lock().unwrap().push(line.to_string())).unwrap();
    (b.spawn(), log)
}

type Ws = WebSocket<TcpStream>;

/// A WebSocket handshake as a browser makes it, with `origin` (None sends none) and `host`
/// (None for the address connected to).
fn handshake(addr: SocketAddr, origin: Option<&str>, host: Option<&str>) -> Result<Ws, String> {
    let mut req = format!("ws://{addr}/").into_client_request().unwrap();
    if let Some(o) = origin {
        req.headers_mut().insert("origin", HeaderValue::from_str(o).unwrap());
    }
    if let Some(h) = host {
        req.headers_mut().insert("host", HeaderValue::from_str(h).unwrap());
    }
    let stream = TcpStream::connect(addr).unwrap();
    stream.set_read_timeout(Some(Duration::from_secs(10))).unwrap();
    match tungstenite::client(req, stream) {
        Ok((ws, _)) => Ok(ws),
        Err(tungstenite::HandshakeError::Failure(tungstenite::Error::Http(r))) => {
            Err(format!("http {}", r.status().as_u16()))
        }
        Err(e) => Err(e.to_string()),
    }
}

fn send(ws: &mut Ws, v: Value) {
    ws.send(Message::text(v.to_string())).unwrap();
}

/// The next JSON message, or None once the bridge hung up.
fn next(ws: &mut Ws) -> Option<Value> {
    loop {
        match ws.read() {
            Ok(Message::Text(t)) => return Some(serde_json::from_str(&t).unwrap()),
            Ok(Message::Close(_)) | Err(_) => return None,
            Ok(_) => {}
        }
    }
}

/// A page that sent `code` and got the bridge's first answer.
fn pair(b: &Running, code: &str) -> (Ws, Value) {
    let mut ws = handshake(b.addr(), Some(ORIGIN), None).unwrap();
    send(&mut ws, json!({"vxr": "bridge", "code": code}));
    let first = next(&mut ws).expect("an answer");
    (ws, first)
}

fn paired(b: &Running) -> Ws {
    let (ws, first) = pair(b, &show_code(&b.code()));
    assert_eq!(first, json!({"ok": true}));
    ws
}

/// `vxr mcp --live` as the agent's MCP client sees it.
struct Agent(Server<Live<Remote>>);

impl Agent {
    fn new(dir: &Dir, client: &str) -> Agent {
        let mut server = Server::with(Live(Remote::new(dir.0.clone())));
        let init = json!({"jsonrpc": "2.0", "id": 0, "method": "initialize",
            "params": {"protocolVersion": "2025-06-18", "capabilities": {}, "clientInfo": {"name": client, "version": "1"}}});
        server.handle(&init).unwrap();
        Agent(server)
    }

    /// A tool call: its text and whether it was an error.
    fn call(&mut self, name: &str, args: Value) -> (String, bool) {
        let msg = json!({"jsonrpc": "2.0", "id": 1, "method": "tools/call",
            "params": {"name": name, "arguments": args, "_meta": {"io.modelcontextprotocol/protocolVersion": VERSION}}});
        let r = self.0.handle(&msg).unwrap();
        let r = &r["result"];
        (r["content"][0]["text"].as_str().unwrap_or("").to_string(), r["isError"].as_bool().unwrap_or(true))
    }
}

/// Run `f` (an agent's request) while the page answers the bridge as the editor does: a call
/// answers `ran <tool> <args>`, the summary reads `summary`. Returns `f`'s result and every
/// message the page got.
fn answering<T: Send>(ws: &mut Ws, f: impl FnOnce() -> T + Send) -> (T, Vec<Value>) {
    ws.get_ref().set_read_timeout(Some(Duration::from_millis(20))).unwrap();
    let mut got = Vec::new();
    let out = std::thread::scope(|s| {
        let h = s.spawn(f);
        let start = Instant::now();
        while !h.is_finished() {
            match ws.read() {
                Ok(Message::Text(t)) => {
                    let m: Value = serde_json::from_str(&t).unwrap();
                    if let Some(id) = m.get("id").cloned() {
                        let reply = match (m.get("call"), m.get("read")) {
                            (Some(c), _) => {
                                json!({"id": id, "text": format!("{} ran {} {}", m["client"].as_str().unwrap(), c.as_str().unwrap(), m["args"]), "isError": false})
                            }
                            (_, Some(u)) if u == "vxr://scene/summary" => json!({"id": id, "text": "summary"}),
                            _ => json!({"id": id, "missing": true}),
                        };
                        send(ws, reply);
                    }
                    got.push(m);
                }
                Ok(_) => {}
                Err(tungstenite::Error::Io(e))
                    if matches!(e.kind(), std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut) => {}
                Err(e) => panic!("the page lost the bridge: {e}"),
            }
            assert!(start.elapsed() < Duration::from_secs(20), "the agent's request never finished");
        }
        h.join().unwrap()
    });
    (out, got)
}

/// The agents the page was last told of, from the messages in `seen` and then from reading
/// until a `clients` message lists `n` of them.
fn clients(ws: &mut Ws, seen: &[Value], n: usize) -> Vec<Value> {
    let last = seen.iter().rev().find_map(|m| m.get("clients").and_then(Value::as_array).cloned());
    if let Some(c) = last.filter(|c| c.len() == n) {
        return c;
    }
    ws.get_ref().set_read_timeout(Some(Duration::from_secs(5))).unwrap();
    loop {
        let m = next(ws).expect("a clients message");
        if let Some(c) = m.get("clients").and_then(Value::as_array).filter(|c| c.len() == n) {
            return c.clone();
        }
    }
}

fn wait(what: &str, f: impl Fn() -> bool) {
    let start = Instant::now();
    while !f() {
        assert!(start.elapsed() < Duration::from_secs(5), "{what}");
        std::thread::sleep(Duration::from_millis(5));
    }
}

#[test]
fn codes_are_typed_loosely_and_shown_in_two_halves() {
    assert_eq!(normalize_code("k7qf-3mxp").as_deref(), Some("K7QF3MXP"));
    assert_eq!(normalize_code(" K7QF 3MXP ").as_deref(), Some("K7QF3MXP"));
    assert_eq!(normalize_code("ko1l-iabc").as_deref(), Some("K0111ABC"));
    assert_eq!(normalize_code("K7QF3MX"), None, "too short");
    assert_eq!(normalize_code("K7QF3MXPQ"), None, "too long");
    assert_eq!(normalize_code("K7QF3MXU"), None, "U is not a letter of the code");
    assert_eq!(normalize_code("K7QF3MXé"), None);
    assert_eq!(show_code("K7QF3MXP"), "K7QF-3MXP");
    let codes: Vec<String> = (0..64).map(|_| crate::bridge::new_code().unwrap()).collect();
    assert!(codes.iter().all(|c| normalize_code(c).as_deref() == Some(c.as_str())));
    let mut unique = codes.clone();
    unique.sort();
    unique.dedup();
    assert_eq!(unique.len(), codes.len(), "random codes");
}

#[test]
fn only_a_loopback_address_is_listened_on() {
    let dir = Dir::new("bind");
    for addr in ["0.0.0.0:0", "192.0.2.1:0", "[::]:0", "198.51.100.7:41723"] {
        let cfg = Config { addr: addr.parse().unwrap(), dir: dir.0.clone(), dev_origins: false };
        let e = Bridge::bind(cfg, |_| {}).err().unwrap_or_else(|| panic!("{addr} was listened on"));
        assert!(e.contains("not a loopback address"), "{addr}: {e}");
    }
    for addr in ["127.0.0.1:0", "[::1]:0"] {
        let cfg = Config { addr: addr.parse().unwrap(), dir: dir.0.clone(), dev_origins: false };
        // A system without IPv6 cannot bind [::1]; it is still not refused as non-loopback.
        if let Err(e) = Bridge::bind(cfg, |_| {}) {
            assert!(!e.contains("not a loopback address"), "{addr}: {e}");
        }
    }
}

#[test]
fn only_the_editors_page_may_connect() {
    let dir = Dir::new("origin");
    let (b, log) = bridge(&dir, false);
    let a = b.addr();
    for origin in [
        Some("https://evil.example.com"),
        Some("https://vxr.jrnxf.co.example.com"),
        Some("http://vxr.jrnxf.co"),
        Some("https://vxr.jrnxf.co:8443"),
        Some("null"),
        Some("http://localhost:8080"),
        Some("http://127.0.0.1:5173"),
        None,
    ] {
        assert_eq!(handshake(a, origin, None).err().as_deref(), Some("http 403"), "{origin:?}");
    }
    // DNS rebinding: the right origin, but a name that only now points at this computer.
    let host = format!("rebind.example.com:{}", a.port());
    assert_eq!(handshake(a, Some(ORIGIN), Some(&host)).err().as_deref(), Some("http 403"));
    assert_eq!(handshake(a, Some(ORIGIN), Some("127.0.0.1:1")).err().as_deref(), Some("http 403"), "another port");
    // A plain request is no WebSocket: nothing comes back but an error.
    let mut s = TcpStream::connect(a).unwrap();
    s.set_read_timeout(Some(Duration::from_secs(5))).unwrap();
    std::io::Write::write_all(&mut s, b"GET / HTTP/1.1\r\nHost: 127.0.0.1\r\n\r\n").unwrap();
    let mut buf = String::new();
    let _ = std::io::Read::read_to_string(&mut s, &mut buf);
    assert!(!buf.contains("200"), "{buf}");
    assert!(!dir.0.join(SOCKET).exists(), "nothing listens for agents before a page pairs");
    // Every refusal is in the terminal, cleaned.
    let log = log.lock().unwrap().clone();
    assert!(log.iter().any(|l| l == "Refused a connection from https://evil.example.com"), "{log:?}");
    assert!(log.iter().any(|l| l == "Refused a connection with no origin"), "{log:?}");
    assert!(log.iter().any(|l| l.starts_with("Refused a connection for another host")), "{log:?}");
    // The page itself, by name or by address, and `localhost` as Chrome may send it.
    for host in [None, Some(format!("localhost:{}", a.port()))] {
        let mut ws = handshake(a, Some(ORIGIN), host.as_deref()).unwrap();
        send(&mut ws, json!({"vxr": "bridge", "code": "0000-0000"}));
        assert_eq!(next(&mut ws), Some(json!({"error": WRONG_CODE})));
    }
}

#[test]
fn a_debug_build_also_takes_pages_served_from_this_computer() {
    let dir = Dir::new("dev");
    let (b, _) = bridge(&dir, true);
    for origin in ["http://localhost:8080", "http://127.0.0.1:5173", "http://[::1]:3000", "http://localhost", ORIGIN] {
        assert!(handshake(b.addr(), Some(origin), None).is_ok(), "{origin}");
    }
    for origin in
        ["http://localhost.example.com", "http://127.0.0.1:99999", "http://127.0.0.1:", "https://evil.example.com"]
    {
        assert_eq!(handshake(b.addr(), Some(origin), None).err().as_deref(), Some("http 403"), "{origin}");
    }
}

#[test]
fn nothing_reaches_the_page_before_the_code_matches() {
    let dir = Dir::new("code");
    let (b, log) = bridge(&dir, false);
    // No code, a wrong code, the wrong first message.
    let (mut ws, first) = pair(&b, "0000-0000");
    assert_eq!(first, json!({"error": WRONG_CODE}));
    assert_eq!(next(&mut ws), None, "hung up");
    for hello in [json!({"vxr": "bridge"}), json!({"vxr": "bridge", "code": 7}), json!({"call": "erase"})] {
        let mut ws = handshake(b.addr(), Some(ORIGIN), None).unwrap();
        send(&mut ws, hello.clone());
        let want = if hello.get("vxr").is_some() { WRONG_CODE } else { NO_HELLO };
        assert_eq!(next(&mut ws), Some(json!({"error": want})), "{hello}");
        assert_eq!(next(&mut ws), None);
    }
    let mut ws = handshake(b.addr(), Some(ORIGIN), None).unwrap();
    ws.send(Message::binary(vec![1, 2, 3])).unwrap();
    assert_eq!(next(&mut ws), Some(json!({"error": NO_HELLO})));
    assert!(!dir.0.join(SOCKET).exists(), "no socket for agents");
    // An agent finds no window to edit.
    let mut agent = Agent::new(&dir, "claude-code");
    assert_eq!(agent.call("scene_summary", json!({})), (NOT_LIVE.to_string(), true));
    assert!(!b.paired());
    // The code is never in the terminal after it was first printed.
    assert!(log.lock().unwrap().iter().all(|l| !l.contains(&b.code()) && !l.contains(&show_code(&b.code()))));
}

#[test]
fn wrong_codes_retire_the_code() {
    let dir = Dir::new("retire");
    let (b, log) = bridge(&dir, false);
    let old = b.code();
    let mut wrong = old.clone().into_bytes();
    wrong[0] = if wrong[0] == b'0' { b'1' } else { b'0' };
    let wrong = String::from_utf8(wrong).unwrap();
    for _ in 0..MAX_FAILURES {
        assert_eq!(pair(&b, &wrong).1, json!({"error": WRONG_CODE}));
    }
    let new = b.code();
    assert_ne!(new, old, "a new code");
    assert_eq!(pair(&b, &old).1, json!({"error": WRONG_CODE}), "the old code no longer works");
    let printed = log.lock().unwrap().iter().any(|l| l.contains(&show_code(&new)) && l.contains("guessing"));
    assert!(printed, "the new code is printed with the reason");
    drop(paired(&b));
}

#[test]
fn a_paired_page_answers_agents_as_the_window_does() {
    let dir = Dir::new("pair");
    let (b, log) = bridge(&dir, false);
    let mut page = paired(&b);
    wait("the socket opens", || dir.0.join(SOCKET).exists());
    let mode = std::fs::metadata(dir.0.join(TOKEN)).unwrap().permissions();
    assert_eq!(std::os::unix::fs::PermissionsExt::mode(&mode) & 0o777, 0o600, "the token is the user's alone");
    assert!(b.paired());
    // A second page is turned away while one is paired, even with the right code.
    assert_eq!(pair(&b, &b.code()).1, json!({"error": BUSY}));

    let mut agent = Agent::new(&dir, "claude-code");
    let (r, got) = answering(&mut page, || agent.call("box", json!({"from": [0, 0, 0], "to": [1, 1, 1], "slot": 3})));
    assert_eq!(r, (r#"Claude Code ran box {"from":[0,0,0],"slot":3,"to":[1,1,1]}"#.to_string(), false));
    let call = got.iter().find(|m| m.get("call").is_some()).unwrap();
    assert_eq!(call["client"], "Claude Code");
    assert!(call["conn"].as_u64().is_some());
    // The page hears of an agent before its first request, so it can drop requests from agents
    // it no longer lists.
    let listed = got.iter().position(|m| m.get("clients").and_then(Value::as_array).is_some_and(|c| c.len() == 1));
    assert!(listed.is_some_and(|i| i < got.iter().position(|m| m.get("call").is_some()).unwrap()), "{got:?}");
    // A resource read.
    let msg = json!({"jsonrpc": "2.0", "id": 2, "method": "resources/read",
        "params": {"uri": "vxr://scene/summary", "_meta": {"io.modelcontextprotocol/protocolVersion": VERSION}}});
    let (r, got2) = answering(&mut page, || agent.0.handle(&msg).unwrap());
    assert_eq!(r["result"]["contents"][0]["text"], "summary");
    // The page hears who is connected.
    let c = clients(&mut page, &[got, got2].concat(), 1);
    assert_eq!(c[0][1], "Claude Code");
    assert!(log.lock().unwrap().iter().any(|l| l == "Claude Code connected"));

    // The page leaving closes the socket: the agent finds the window off.
    drop(page);
    wait("the page slot frees", || !b.paired());
    wait("the socket closes", || !dir.0.join(SOCKET).exists());
    assert_eq!(agent.call("scene_summary", json!({})), (NOT_LIVE.to_string(), true));
    assert!(log.lock().unwrap().iter().any(|l| l.starts_with("The page left")));
    // The same code pairs again, and the agent reconnects with the same token.
    let mut page = paired(&b);
    wait("the socket opens again", || dir.0.join(SOCKET).exists());
    let (r, _) = answering(&mut page, || agent.call("erase", json!({"cells": [[0, 0, 0]]})));
    assert!(!r.1, "{}", r.0);
}

#[test]
fn stop_and_regenerate_from_the_page_behave_as_on_desktop() {
    let dir = Dir::new("stop");
    let (b, _) = bridge(&dir, false);
    let mut page = paired(&b);
    wait("the socket opens", || dir.0.join(SOCKET).exists());
    let mut claude = Agent::new(&dir, "claude-code");
    let mut codex = Agent::new(&dir, "codex");
    let (_, a) = answering(&mut page, || claude.call("scene_summary", json!({})));
    let (_, b2) = answering(&mut page, || codex.call("scene_summary", json!({})));
    let c = clients(&mut page, &[a, b2].concat(), 2);
    let claude_conn = c.iter().find(|c| c[1] == "Claude Code").unwrap()[0].as_u64().unwrap();

    // Stop one: it is told so, for good; the other keeps editing.
    send(&mut page, json!({"stop": claude_conn}));
    let c = clients(&mut page, &[], 1);
    assert_eq!(c[0][1], "Codex");
    assert_eq!(claude.call("scene_summary", json!({})), (STOPPED.to_string(), true));
    let (r, _) = answering(&mut page, || codex.call("scene_summary", json!({})));
    assert!(!r.1);

    // Regenerate: a new token, every agent disconnected and turned away until it restarts.
    let old = std::fs::read_to_string(dir.0.join(TOKEN)).unwrap();
    send(&mut page, json!({"regenerate": true}));
    page.get_ref().set_read_timeout(Some(Duration::from_secs(5))).unwrap();
    let mut regenerated = false;
    while !regenerated {
        let m = next(&mut page).unwrap();
        regenerated = m == json!({"regenerated": true});
    }
    assert_ne!(std::fs::read_to_string(dir.0.join(TOKEN)).unwrap(), old);
    clients(&mut page, &[], 0);
    assert_eq!(codex.call("scene_summary", json!({})), (REFUSED.to_string(), true));
    // A restarted server reads the new token.
    let mut fresh = Agent::new(&dir, "codex");
    let (r, got) = answering(&mut page, || fresh.call("scene_summary", json!({})));
    assert!(!r.1, "{}", r.0);
    // Stop "all".
    clients(&mut page, &got, 1);
    send(&mut page, json!({"stop": "all"}));
    clients(&mut page, &[], 0);
    assert_eq!(fresh.call("scene_summary", json!({})), (STOPPED.to_string(), true));
}

#[test]
fn the_desktop_app_and_the_bridge_never_share_the_socket() {
    let dir = Dir::new("both");
    let host = crate::live::Host::start(&dir.0).unwrap();
    let (b, _) = bridge(&dir, false);
    let (mut ws, first) = pair(&b, &b.code());
    assert!(first["error"].as_str().unwrap().contains("desktop app already lets terminal agents"), "{first}");
    assert_eq!(next(&mut ws), None);
    wait("the page slot frees", || !b.paired());
    drop(host);
    drop(paired(&b));
}

#[test]
fn the_code_field_reads_one_character_as_normalize_code_reads_a_code() {
    let typed = "k7-qf 3mxp o1l";
    let field: String = typed.chars().filter(|c| !code_char_skipped(*c)).filter_map(code_char).collect();
    assert_eq!(field, "K7QF3MXP011");
    assert_eq!(normalize_code(&typed[..10]).as_deref(), Some(&field[..8]));
    for c in ['u', 'U', 'é', '!', '/'] {
        assert_eq!(code_char(c), None, "{c} is not a letter of the code");
        assert!(!code_char_skipped(c));
    }
    assert!(code_char_skipped(' ') && code_char_skipped('-'));
}
