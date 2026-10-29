//! Live mode over a real socket: a [`Host`] in a temporary folder answered by the test, as
//! the app's thread answers it, and [`Remote`]s and raw connections against it.

use std::io::{BufRead, BufReader, Write};
use std::os::unix::fs::PermissionsExt;
use std::os::unix::net::UnixStream;
use std::path::PathBuf;
use std::time::{Duration, Instant};

use serde_json::{Value, json};

use crate::live::{Ask, Host, Live, NOT_LIVE, REFUSED, Remote, SOCKET, STOPPED, TOKEN};
use crate::protocol::{Reply, Server, VERSION};

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
    assert!(names.contains(&"edit_object") && names.contains(&"review_status"));
    // Save and New scene are the window's own; opening a file is the user's, in its dialog.
    assert!(names.contains(&"save") && names.contains(&"new_scene"));
    assert!(names.contains(&"look") && names.contains(&"render_settings") && names.contains(&"export"));
    for files_only in ["list_files", "open"] {
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

/// A raw connection that has presented the token, and the host's id for it.
fn paired(dir: &Dir, host: &Host) -> (UnixStream, u64) {
    let s = UnixStream::connect(dir.0.join(SOCKET)).unwrap();
    s.set_read_timeout(Some(Duration::from_secs(10))).unwrap();
    let mut w = s.try_clone().unwrap();
    w.write_all(format!("{{\"vxr\": \"live\", \"token\": \"{}\"}}\n", token(dir)).as_bytes()).unwrap();
    let mut line = String::new();
    BufReader::new(s.try_clone().unwrap()).read_line(&mut line).unwrap();
    assert_eq!(serde_json::from_str::<Value>(&line).unwrap(), json!({"ok": true}));
    wait_for(|| !host.clients().is_empty());
    let id = host.clients().last().unwrap().0;
    (s, id)
}

#[test]
fn stop_does_not_wait_for_an_agent_that_stopped_reading() {
    let dir = Dir::new("stall");
    let host = Host::start(&dir.0).unwrap();
    let (s, id) = paired(&dir, &host);
    // The agent asks for something big and then never reads its answer: the reply fills
    // the socket and its writer blocks.
    s.try_clone().unwrap().write_all(b"{\"id\": 1, \"read\": \"vxr://scene/summary\"}\n").unwrap();
    let start = Instant::now();
    let r = loop {
        if let Some(r) = host.poll().pop() {
            break r;
        }
        assert!(start.elapsed() < Duration::from_secs(5), "the read never came");
        std::thread::sleep(Duration::from_millis(2));
    };
    r.answer(Ok("x".repeat(8 << 20)));
    std::thread::sleep(Duration::from_millis(200));
    let t = Instant::now();
    host.stop(id);
    assert!(t.elapsed() < Duration::from_millis(500), "Stop took {:?}", t.elapsed());
    assert!(host.clients().is_empty());
    // Turning live mode off does not wait either.
    let (s2, _) = paired(&dir, &host);
    s2.try_clone().unwrap().write_all(b"{\"id\": 1, \"read\": \"vxr://scene/summary\"}\n").unwrap();
    let start = Instant::now();
    let r = loop {
        if let Some(r) = host.poll().pop() {
            break r;
        }
        assert!(start.elapsed() < Duration::from_secs(5), "the read never came");
        std::thread::sleep(Duration::from_millis(2));
    };
    r.answer(Ok("x".repeat(8 << 20)));
    std::thread::sleep(Duration::from_millis(200));
    let t = Instant::now();
    drop(host);
    assert!(t.elapsed() < Duration::from_millis(500), "turning off took {:?}", t.elapsed());
    drop((s, s2));
}

#[test]
fn stop_during_a_reply_the_agent_reads_still_says_stopped() {
    let dir = Dir::new("midreply");
    let host = Host::start(&dir.0).unwrap();
    // A raw agent: the reply is still being written when Stop comes, and the reason follows it.
    let (s, id) = paired(&dir, &host);
    s.try_clone().unwrap().write_all(b"{\"id\": 1, \"read\": \"vxr://scene/summary\"}\n").unwrap();
    wait_for(|| match host.poll().pop() {
        Some(r) => {
            r.answer(Ok("x".repeat(3 << 20)));
            true
        }
        None => false,
    });
    std::thread::sleep(Duration::from_millis(200));
    host.stop(id);
    assert!(host.clients().is_empty());
    let mut r = BufReader::new(s);
    let mut line = String::new();
    r.read_line(&mut line).unwrap();
    assert_eq!(serde_json::from_str::<Value>(&line).unwrap()["text"].as_str().unwrap().len(), 3 << 20);
    line.clear();
    r.read_line(&mut line).unwrap();
    assert_eq!(serde_json::from_str::<Value>(&line).unwrap(), json!({"closed": "stopped"}));

    // `vxr mcp --live`: Stop as its big read is answered, and it stays stopped.
    let mut a = Agent::new(&dir, "claude-code");
    std::thread::scope(|sc| {
        let h = sc.spawn(|| a.request("resources/read", json!({"uri": "vxr://scene/summary"})));
        wait_for(|| match host.poll().pop() {
            Some(r) => {
                let conn = r.conn;
                r.answer(Ok("x".repeat(3 << 20)));
                host.stop(conn);
                true
            }
            None => false,
        });
        let reply = h.join().unwrap();
        let text = reply["result"]["contents"][0]["text"].as_str().unwrap();
        assert!(text.len() == 3 << 20 || text == STOPPED, "the read gets its answer or the reason");
    });
    assert_eq!(tool(&host, &mut a, "erase", json!({})), (STOPPED.into(), true));
    assert_eq!(tool(&host, &mut a, "erase", json!({})), (STOPPED.into(), true));
    assert!(host.clients().is_empty(), "the stopped server does not connect again");
}

#[test]
fn a_connection_id_is_never_given_out_again() {
    let dir = Dir::new("ids");
    let host = Host::start(&dir.0).unwrap();
    let (s1, first) = paired(&dir, &host);
    drop(host);
    let host = Host::start(&dir.0).unwrap();
    let (s2, second) = paired(&dir, &host);
    assert_ne!(first, second, "after off and on a new agent is not taken for the old one");
    drop((s1, s2));
}

/// What the window does with an agent's calls, without review: each edit is one `vxr-ops`
/// session applied at once as one undo step, with the agent's own selection and clipboard
/// kept between calls; `undo` takes back the last step.
struct Window {
    scene: vxr_core::Scene,
    selection: Vec<[i32; 3]>,
    clip: Option<vxr_core::select::Clip>,
}

impl Window {
    fn answer(&mut self, name: &str, args: Value) -> Result<Reply, String> {
        let mut image = None;
        let r = self.run(name, args, &mut image);
        r.map(|t| Reply::with_png(t, image.as_deref()))
    }

    fn run(&mut self, name: &str, args: Value, image: &mut Option<Vec<u8>>) -> Result<String, String> {
        if name == "undo" {
            return if self.scene.undo() { Ok("took back".into()) } else { Err("nothing to undo".into()) };
        }
        if name == "redo" {
            return if self.scene.redo() { Ok("brought back".into()) } else { Err("nothing to redo".into()) };
        }
        if name == "show_world" {
            return crate::files::show_world(&mut self.scene).map(|_| "the world".into());
        }
        if name == "edit_object" {
            let node = args["node"].as_u64().unwrap() as usize;
            return if self.scene.edit_object(node) { Ok("the model".into()) } else { Err("not an object".into()) };
        }
        let limits = vxr_ops::Limits { ask_over: vxr_ops::Limits::default().changed_cells, ..Default::default() };
        let mut o = vxr_ops::Session::with_limits(&self.scene, None, limits);
        o.set_selection(self.selection.iter().copied());
        o.set_clipboard(self.clip.clone());
        match o.call(&mut self.scene, name, args) {
            Ok(t) => {
                self.selection = o.selection().collect();
                self.clip = o.clipboard().cloned();
                *image = o.take_image();
                o.apply(&mut self.scene);
                Ok(t)
            }
            Err(e) => {
                o.discard(&mut self.scene);
                Err(e.message)
            }
        }
    }
}

/// An agent's tool call, answered by `w` on this thread as the app's thread answers it.
fn edit(host: &Host, w: &mut Window, a: &mut Agent, name: &str, args: Value) -> (String, bool) {
    std::thread::scope(|s| {
        let h = s.spawn(|| a.request("tools/call", json!({"name": name, "arguments": args})));
        let start = Instant::now();
        while !h.is_finished() {
            for r in host.poll() {
                match r.ask.clone() {
                    Ask::Call { name, args } => r.reply(w.answer(&name, args)),
                    Ask::Read(_) => r.missing(),
                }
            }
            assert!(start.elapsed() < Duration::from_secs(20), "the agent's call never finished");
            std::thread::sleep(Duration::from_millis(2));
        }
        let r = h.join().unwrap();
        let r = &r["result"];
        (r["content"][0]["text"].as_str().unwrap().to_string(), r["isError"].as_bool().unwrap())
    })
}

#[test]
fn the_edit_tools_change_the_window_and_undo_gives_the_file_back() {
    // A 4 x 4 slot-10 slab at x and z 0..3 with a slot-20 column of 3 on (1, 1), in a 32^3
    // model, opened from its bytes.
    let mut s = vxr_core::Scene::new([32, 32, 32]);
    let mut cells: Vec<([i32; 3], u8)> = (0..16).map(|i| (s.to_map([i % 4, 0, i / 4]), 10)).collect();
    cells.extend((1..4).map(|y| (s.to_map([1, y, 1]), 20)));
    s.edit(cells);
    let mut opened = vxr_core::Scene::from_vox(&s.to_vox()).unwrap();
    opened.set_bounds([-16, 0, -16], [32, 32, 32]);
    let bytes = vxr_core::Scene::from_vox(&opened.to_vox()).unwrap().to_vox();
    let mut w = Window { scene: vxr_core::Scene::from_vox(&bytes).unwrap(), selection: Vec::new(), clip: None };
    assert_eq!(w.scene.bounds().size, [32, 32, 32]);

    let dir = Dir::new("edits");
    let host = Host::start(&dir.0).unwrap();
    let mut a = Agent::new(&dir, "claude-code");
    let mut call = |name: &str, args: Value| {
        let (t, e) = edit(&host, &mut w, &mut a, name, args);
        assert!(!e, "{name}: {t}");
        t
    };
    // The whole model up one, then (S, Cmd-A, Shift-Down) the selection down one.
    let t = call("transform", json!({"kind": "move", "offset": [0, 1, 0]}));
    assert!(t.starts_with("moved 19 voxels (every voxel), now from [0, 1, 0] to [3, 4, 3]; 33 cells changed"), "{t}");
    assert_eq!(call("select", json!({"all": true})), "19 voxels selected from [0, 1, 0] to [3, 4, 3]");
    let t = call("transform", json!({"kind": "move", "offset": [0, -1, 0]}));
    assert!(t.starts_with("moved 19 voxels (the selection), now from [0, 0, 0] to [3, 3, 3]; 33 cells changed"), "{t}");
    // Once more is below the ground: refused, nothing moves.
    let (t, e) = edit(&host, &mut w, &mut a, "transform", json!({"kind": "move", "offset": [0, -1, 0]}));
    assert!(e && t.starts_with("16 of 19 voxels would land outside the editable bounds"), "{t}");
    // Copy in one call, paste in the next: the clipboard is the agent's, kept between calls.
    let mut call = |name: &str, args: Value| {
        let (t, e) = edit(&host, &mut w, &mut a, name, args);
        assert!(!e, "{name}: {t}");
        t
    };
    call("select", json!({"color": 20}));
    assert_eq!(call("copy", json!({})), "copied 3 voxels, a 1 x 3 x 1 box from [1, 1, 1]");
    let t = call("paste", json!({"at": [5, 0, 5]}));
    assert!(t.starts_with("pasted at [5, 0, 5] (low corner); 3 cells changed"), "{t}");
    // Taller bounds, then a post above the old top.
    let t = call("resize", json!({"size": [32, 40, 32]}));
    assert!(t.starts_with("bounds x -16..15, y 0..39, z -16..15 (32 x 40 x 32); 0 cells changed"), "{t}");
    let t = call("box", json!({"from": [-5, 0, -5], "to": [-5, 35, -5], "slot": 4}));
    assert!(t.starts_with("36 cells changed") && !t.contains("warning"), "{t}");
    let t = call("material_set", json!({"slot": 10, "type": "metal", "roughness": 0.25}));
    assert_eq!(t, "slot 10: Diffuse -> Metal (roughness 0.25, metalness 1); 16 voxels use it");
    let t = call("hollow", json!({}));
    assert!(t.ends_with("; warning: nothing changed"), "{t}");
    let t = call("crop", json!({}));
    assert!(t.starts_with("bounds x -5..5, y 0..35, z -5..5 (11 x 36 x 11); 0 cells changed"), "{t}");
    // Seven edits, seven undo steps: undone, the save is the file's bytes.
    assert_eq!(w.scene.history().position(), 7);
    for _ in 0..7 {
        assert!(w.scene.undo());
    }
    assert!(!w.scene.undo());
    assert_eq!(w.scene.to_vox(), bytes);
}

#[test]
fn the_world_tools_change_the_window_and_undo_gives_the_file_back() {
    // The generated 22-world: the slab (node 1, 64 voxels), the trees group (2) of three
    // linked 13-voxel trees (3, 4 and 6, in the grove group 5), the gem (7) on the hidden
    // layer 2 and the hidden lost gem (8).
    let f = vxr_vox::fixtures::all().into_iter().find(|f| f.name == "22-world").unwrap();
    let bytes = vxr_vox::Document { raw: f.builder.build() }.write();
    let mut w = Window { scene: vxr_core::Scene::from_vox(&bytes).unwrap(), selection: Vec::new(), clip: None };
    let shown = |w: &Window| w.scene.world_voxels(true).len();
    assert_eq!(shown(&w), 64 + 3 * 13);

    let dir = Dir::new("world");
    let host = Host::start(&dir.0).unwrap();
    let mut a = Agent::new(&dir, "claude-code");
    let call = |w: &mut Window, a: &mut Agent, name: &str, args: Value| {
        let (t, e) = edit(&host, w, a, name, args);
        assert!(!e, "{name}: {t}");
        t
    };
    let t = call(&mut w, &mut a, "move_object", json!({"node": 1, "offset": [0, 0, 4]}));
    assert!(t.starts_with("moved object 1 by [0, 0, 4], now from [-4, 0, 1] to [3, 0, 8]; 1 node changed in the world, no voxel of any model"), "{t}");
    let t = call(&mut w, &mut a, "duplicate", json!({"node": 1}));
    assert!(t.starts_with("copied object 1 as object 9 (its own model)"), "{t}");
    assert_eq!(shown(&w), 2 * 64 + 39);
    let t = call(&mut w, &mut a, "duplicate", json!({"node": 2, "linked": true}));
    assert!(
        t.starts_with("copied group 2 (3 objects) as group 14 (3 objects) (linked copies, sharing their models)"),
        "{t}"
    );
    assert_eq!(shown(&w), 2 * 64 + 2 * 39);
    let t = call(&mut w, &mut a, "group", json!({"nodes": [1, 9]}));
    assert!(t.starts_with("grouped 2 nodes [1, 9] into group 15 (2 objects); 3 nodes changed"), "{t}");
    let t = call(&mut w, &mut a, "hide", json!({"node": 15}));
    assert!(t.starts_with("group 15 (2 objects) is hidden"), "{t}");
    assert_eq!(shown(&w), 2 * 39);
    let t = call(&mut w, &mut a, "ungroup", json!({"node": 14}));
    assert!(t.starts_with("ungrouped group 14 (3 objects)"), "{t}");
    let t = call(&mut w, &mut a, "set_layer", json!({"node": 7, "layer": 0}));
    assert!(t.starts_with("put object 7 on layer 0; 1 node changed"), "{t}");
    assert_eq!(shown(&w), 2 * 39 + 1);
    // A move that does nothing says so and makes no step.
    let t = call(&mut w, &mut a, "move_object", json!({"node": 3, "offset": [0, 0, 0]}));
    assert!(t.ends_with("; warning: nothing changed"), "{t}");
    // In a model the world tools refuse; show_world brings the world back.
    call(&mut w, &mut a, "edit_object", json!({"node": 3}));
    let (t, e) = edit(&host, &mut w, &mut a, "hide", json!({"node": 3}));
    assert!(e && t.contains("show_world"), "{t}");
    call(&mut w, &mut a, "show_world", json!({}));
    // Seven edits, seven undo steps: undone, the save is the file's bytes.
    assert_eq!(w.scene.history().position(), 7);
    for _ in 0..7 {
        assert!(w.scene.undo());
    }
    assert!(!w.scene.undo());
    assert_eq!(w.scene.to_vox(), bytes);
}

#[test]
fn make_object_splits_the_window_s_model_and_undo_gives_the_file_back() {
    // The generated 23-desk: one model of a desk, a cup with coffee in it and a book.
    let f = vxr_vox::fixtures::all().into_iter().find(|f| f.name == "23-desk").unwrap();
    let bytes = vxr_vox::Document { raw: f.builder.build() }.write();
    let mut w = Window { scene: vxr_core::Scene::from_vox(&bytes).unwrap(), selection: Vec::new(), clip: None };
    let all = w.scene.world_voxels(false);

    let dir = Dir::new("split");
    let host = Host::start(&dir.0).unwrap();
    let mut a = Agent::new(&dir, "claude-code");
    let call = |w: &mut Window, a: &mut Agent, name: &str, args: Value| {
        let (t, e) = edit(&host, w, a, name, args);
        assert!(!e, "{name}: {t}");
        t
    };
    let t = call(&mut w, &mut a, "make_object", json!({"parts": ["white", "orange"], "name": "mug"}));
    assert!(t.starts_with("new object 2 \"mug\": 60 voxels"), "{t}");
    let t = call(&mut w, &mut a, "make_object", json!({"from_slot": 4, "name": "book"}));
    assert!(t.starts_with("new object 3 \"book\": 60 voxels"), "{t}");
    call(&mut w, &mut a, "rename", json!({"node": 1, "name": "desk"}));
    let (t, e) = edit(&host, &mut w, &mut a, "rename", json!({"node": 7, "name": "x"}));
    assert!(e && t.starts_with("node 7: not an object"), "{t}");
    let world = w.scene.world().unwrap();
    let names: Vec<_> = world.objects().into_iter().map(|i| world.tree.nodes[i].name.clone().unwrap()).collect();
    // Each call is its own step, and each new object goes right after the desk, as Selection
    // to new object puts it.
    assert_eq!(names, [b"desk".to_vec(), b"book".to_vec(), b"mug".to_vec()]);
    assert_eq!(w.scene.world_voxels(false), all, "nothing moved in the world");
    // Three edits, three undo steps: undone, the save is the file's bytes.
    assert_eq!(w.scene.history().position(), 3);
    for _ in 0..3 {
        assert!(w.scene.undo());
    }
    assert_eq!(w.scene.to_vox(), bytes);
}

#[test]
fn the_housekeeping_select_and_palette_tools_change_the_window_and_undo_gives_the_file_back() {
    // The generated 22-world: the slab (node 1, 64 voxels on layer 0), the trees group (2)
    // of three linked 13-voxel trees on layer 1, the gem (7) on the hidden layer 2 and the
    // hidden lost gem (8).
    let f = vxr_vox::fixtures::all().into_iter().find(|f| f.name == "22-world").unwrap();
    let bytes = vxr_vox::Document { raw: f.builder.build() }.write();
    let mut w = Window { scene: vxr_core::Scene::from_vox(&bytes).unwrap(), selection: Vec::new(), clip: None };
    let shown = |w: &Window| w.scene.world_voxels(true).len();
    assert_eq!(shown(&w), 64 + 3 * 13);

    let dir = Dir::new("house");
    let host = Host::start(&dir.0).unwrap();
    let mut a = Agent::new(&dir, "claude-code");
    let call = |w: &mut Window, a: &mut Agent, name: &str, args: Value| {
        let (t, e) = edit(&host, w, a, name, args);
        assert!(!e, "{name}: {t}");
        t
    };
    let t = call(&mut w, &mut a, "hide_layer", json!({"layer": 1}));
    assert!(t.starts_with("layer 1 is hidden (3 objects on it); 1 layer changed in the world"), "{t}");
    assert_eq!(shown(&w), 64);
    let t = call(&mut w, &mut a, "new_layer", json!({}));
    assert!(t.starts_with("added layer 3 \"layer 3\""), "{t}");
    let t = call(&mut w, &mut a, "rename", json!({"layer": 3, "name": "props"}));
    assert!(t.starts_with("layer 3 is named \"props\"; 1 layer changed"), "{t}");
    let t = call(&mut w, &mut a, "rename", json!({"node": 2, "name": ""}));
    assert!(t.starts_with("group 2 (3 objects) is named no name"), "{t}");
    let t = call(&mut w, &mut a, "delete_object", json!({"node": 1}));
    assert!(t.starts_with("deleted object 1; 1 node changed"), "{t}");
    assert_eq!(shown(&w), 0);
    // Undo and redo are the window's: the slab comes and goes.
    call(&mut w, &mut a, "undo", json!({}));
    assert_eq!(shown(&w), 64);
    call(&mut w, &mut a, "redo", json!({}));
    assert_eq!(shown(&w), 0);
    call(&mut w, &mut a, "undo", json!({}));

    // In the slab's model: its top face, its volume, the inverse, a circle and the palette.
    call(&mut w, &mut a, "edit_object", json!({"node": 1}));
    call(&mut w, &mut a, "resize", json!({"size": [8, 2, 8]}));
    let t = call(&mut w, &mut a, "face", json!({"cell": [0, 0, 0], "side": "top", "slot": 9}));
    assert!(t.starts_with("64 cells changed"), "{t}");
    let t = call(&mut w, &mut a, "face", json!({"cell": [0, 1, 0], "mode": "paint", "slot": 12}));
    assert!(t.starts_with("64 cells changed"), "{t}");
    assert_eq!(
        call(&mut w, &mut a, "select", json!({"face": [-4, 0, 0], "side": "left"})),
        "8 voxels selected from [-4, 0, -3] to [-4, 0, 4]"
    );
    assert!(call(&mut w, &mut a, "select", json!({"invert": true})).starts_with("120 voxels selected"));
    assert!(call(&mut w, &mut a, "select", json!({"volume": [0, 1, 0]})).starts_with("128 voxels selected"));
    let t = call(&mut w, &mut a, "circle", json!({"center": [0, 1, 0], "radius": 1, "mode": "erase"}));
    assert!(t.starts_with("5 cells changed"), "a plus of five: {t}");
    let t = call(&mut w, &mut a, "palette_copy", json!({"from": 12, "to": 13}));
    assert!(t.contains("(0 voxels use it)"), "{t}");
    let t = call(&mut w, &mut a, "palette_swap", json!({"a": 12, "b": 13}));
    assert!(t.contains("slot 13 is now") && t.contains("(59 voxels)"), "{t}");
    call(&mut w, &mut a, "show_world", json!({}));
    // Ten edits applied, one undo step each (the delete was undone): undone, the save is the
    // file's bytes.
    let mut steps = 0;
    while w.scene.undo() {
        steps += 1;
    }
    assert_eq!(steps, 10);
    assert_eq!(w.scene.to_vox(), bytes);
}

#[test]
fn look_sends_its_picture_over_the_socket() {
    let mut s = vxr_core::Scene::new([16, 16, 16]);
    let cells: Vec<_> = (0..3).map(|y| (s.to_map([0, y, 0]), 20)).collect();
    s.edit(cells);
    let mut w = Window { scene: s, selection: Vec::new(), clip: None };
    let dir = Dir::new("look");
    let host = Host::start(&dir.0).unwrap();
    let mut a = Agent::new(&dir, "claude-code");
    let r = std::thread::scope(|sc| {
        let h =
            sc.spawn(|| a.request("tools/call", json!({"name": "look", "arguments": {"view": "left", "size": 96}})));
        while !h.is_finished() {
            for r in host.poll() {
                if let Ask::Call { name, args } = r.ask.clone() {
                    r.reply(w.answer(&name, args));
                }
            }
            std::thread::sleep(Duration::from_millis(2));
        }
        h.join().unwrap()
    });
    let content = r["result"]["content"].as_array().unwrap();
    assert_eq!(content.len(), 2, "{r}");
    let text: Value = serde_json::from_str(content[0]["text"].as_str().unwrap()).unwrap();
    assert_eq!((text["view"].as_str(), text["across"]["axis"].as_str()), (Some("left"), Some("z")));
    assert_eq!(content[1]["mimeType"], "image/png");
    let png = crate::tests::unbase64(content[1]["data"].as_str().unwrap());
    assert_eq!(&png[1..4], b"PNG");
    // A picture that is not base64 does not reach the agent.
    let r = answering_with(
        &host,
        |r| r.reply(Ok(Reply { text: "x".into(), image: Some("not base64!".into()) })),
        || a.request("tools/call", json!({"name": "look", "arguments": {}})),
    );
    assert_eq!(r["result"]["content"].as_array().unwrap().len(), 1, "{r}");
}

/// Run `f` while every request the host gets is answered by `answer`.
fn answering_with<T: Send>(host: &Host, answer: impl Fn(crate::live::Request), f: impl FnOnce() -> T + Send) -> T {
    std::thread::scope(|s| {
        let h = s.spawn(f);
        let start = Instant::now();
        while !h.is_finished() {
            for r in host.poll() {
                answer(r);
            }
            assert!(start.elapsed() < Duration::from_secs(20), "the agent's request never finished");
            std::thread::sleep(Duration::from_millis(2));
        }
        h.join().unwrap()
    })
}
