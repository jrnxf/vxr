//! `vxr bridge`: live mode for the browser editor at vxr.jrnxf.co (`docs/research/ai.md`
//! section 3). A web page cannot host the socket `vxr mcp --live` connects to, so this process
//! hosts it for the page, and the page reaches this process over a loopback WebSocket.
//!
//! - It listens for the page on a loopback TCP port ([`PORT`]) and nowhere else:
//!   [`Bridge::bind`] refuses an address that is not loopback.
//! - Only the editor's own page may connect: the handshake's `Origin` must be [`ORIGIN`]
//!   (a debug build also takes `http://localhost`, `http://127.0.0.1` and `http://[::1]` on
//!   any port, for `crates/vxr-web/check`), and its `Host` must name the loopback address and
//!   this port, which turns away DNS rebinding. Anything else gets 403 and is hung up on.
//! - Pairing: the bridge prints a random code ([`new_code`]); the user types it into the
//!   assistant's Agents tab in the page, and the page sends it first. Nothing is forwarded
//!   before it matches (compared in constant time). Five wrong codes retire it and print a new
//!   one. One page at a time.
//! - Once paired, the bridge hosts the desktop app's socket ([`crate::live::Host`]), so `vxr
//!   mcp --live` connects to it unchanged with its token file, and forwards each agent's
//!   request to the page and the page's answer back. The page leaving closes the socket.
//!
//! The WebSocket carries one JSON object per text message. The page opens with
//! `{"vxr": "bridge", "code": ...}` and gets `{"ok": true}`, or `{"error": ...}` and a close.
//! Then the bridge sends `{"id": n, "conn": c, "client": name, "call": tool, "args": {...}}`
//! or `{"id": n, "conn": c, "client": name, "read": uri}`, answered with `{"id": n, "text":
//! ..., "isError": ...}` (with `"image"`, a base64 PNG, for a picture) or `{"id": n,
//! "missing": true}`, and `{"clients": [[conn, name],
//! ...]}` whenever the agents connected change. The page may send `{"stop": conn}`, `{"stop":
//! "all"}` and `{"regenerate": true}` (a new token, answered with `{"regenerated": true}` or
//! `{"regenerated": false, "error": ...}`).

/// The loopback port the bridge listens on and the page connects to. Fixed, so the page needs
/// no setting; the page's Content-Security-Policy (`crates/vxr-web/www/_headers`) names it.
pub const PORT: u16 = 41723;

/// The one page origin a release build accepts.
pub const ORIGIN: &str = "https://vxr.jrnxf.co";

/// Where the page connects.
pub fn url() -> String {
    format!("ws://127.0.0.1:{PORT}")
}

/// The pairing code's letters: Crockford's base32, without I, L, O and U.
const ALPHABET: &[u8; 32] = b"0123456789ABCDEFGHJKMNPQRSTVWXYZ";

/// Letters in a pairing code: 40 bits.
pub const CODE_LEN: usize = 8;

/// Wrong codes before the code is retired and a new one printed.
pub const MAX_FAILURES: u32 = 5;

/// Refusals the page shows.
pub const WRONG_CODE: &str = "vxr bridge refused that pairing code: type the code it printed and connect again.";
pub const BUSY: &str = "Another tab is already paired with vxr bridge: turn it off there first.";
pub const NO_HELLO: &str = "vxr bridge expected a pairing code first.";

/// One typed character as a code's: upper case, O read as 0 and I and L as 1. None for a space
/// or a dash (which the field skips) and for a character no code has (which it refuses): see
/// [`code_char_skipped`].
pub fn code_char(c: char) -> Option<char> {
    let c = match c.to_ascii_uppercase() {
        'O' => '0',
        'I' | 'L' => '1',
        c => c,
    };
    (c.is_ascii() && ALPHABET.contains(&(c as u8))).then_some(c)
}

/// Whether the field skips `c` without a word: a space or a dash, as a code is written.
pub fn code_char_skipped(c: char) -> bool {
    c == ' ' || c == '-'
}

/// A code as the user typed it, in its canonical form (upper case, no dashes or spaces, O read
/// as 0 and I and L as 1), or None if it cannot be one.
pub fn normalize_code(s: &str) -> Option<String> {
    let mut out = String::new();
    for c in s.chars() {
        if code_char_skipped(c) {
            continue;
        }
        let c = code_char(c)?;
        if out.len() == CODE_LEN {
            return None;
        }
        out.push(c);
    }
    (out.len() == CODE_LEN).then_some(out)
}

/// A canonical code as the terminal shows it: "K7QF-3MXP".
pub fn show_code(c: &str) -> String {
    if c.len() == CODE_LEN { format!("{}-{}", &c[..4], &c[4..]) } else { c.to_string() }
}

/// A new random pairing code, in canonical form.
#[cfg(unix)]
pub fn new_code() -> Result<String, String> {
    let mut bytes = [0u8; CODE_LEN];
    crate::live::random(&mut bytes)?;
    // 256 is a multiple of 32, so taking the low five bits keeps every letter equally likely.
    Ok(bytes.iter().map(|b| ALPHABET[(b & 31) as usize] as char).collect())
}

/// Whether a handshake's `Origin` may connect: [`ORIGIN`], and with `dev` a page served from
/// this computer over plain HTTP on any port.
pub fn origin_ok(origin: &str, dev: bool) -> bool {
    if origin == ORIGIN {
        return true;
    }
    dev && ["http://localhost", "http://127.0.0.1", "http://[::1]"].iter().any(|h| {
        origin.strip_prefix(h).is_some_and(|rest| {
            rest.is_empty()
                || rest
                    .strip_prefix(':')
                    .is_some_and(|p| p.bytes().all(|b| b.is_ascii_digit()) && p.parse::<u16>().is_ok_and(|n| n > 0))
        })
    })
}

/// Whether a handshake's `Host` names this computer's loopback address and `port`.
pub fn host_ok(host: &str, port: u16) -> bool {
    let h = host.to_ascii_lowercase();
    ["127.0.0.1", "localhost", "[::1]"].iter().any(|n| h == format!("{n}:{port}"))
}

#[cfg(unix)]
pub use server::{Bridge, Config, Running};

/// `vxr bridge`'s arguments (after the subcommand): listen until stopped.
#[cfg(unix)]
pub fn run(args: &[String]) -> Result<(), String> {
    if let Some(a) = args.first() {
        match a.as_str() {
            "-h" | "--help" => {
                println!("{HELP}");
                return Ok(());
            }
            other => return Err(format!("unknown argument {other}; vxr bridge --help")),
        }
    }
    let dev = cfg!(debug_assertions);
    let cfg =
        Config { addr: std::net::SocketAddr::from(([127, 0, 0, 1], PORT)), dir: crate::live::dir(), dev_origins: dev };
    let bridge = Bridge::bind(cfg, |line| println!("{line}"))?;
    let shown =
        if dev { format!("{ORIGIN} (and, in this debug build, http://localhost pages)") } else { ORIGIN.into() };
    println!("vxr bridge is listening on 127.0.0.1:{PORT} for {shown}.\n");
    println!("  Pairing code: {}\n", show_code(&bridge.code()));
    println!("{NEXT}");
    bridge.serve();
    Ok(())
}

#[cfg(not(unix))]
pub fn run(_args: &[String]) -> Result<(), String> {
    Err("vxr bridge is not built for this platform yet".into())
}

#[cfg(unix)]
const HELP: &str = r#"usage: vxr bridge

Lets terminal agents edit the scene open in the browser editor at https://vxr.jrnxf.co.
It prints a pairing code and listens on 127.0.0.1 only; once the page is paired, an agent's
vxr-live server (vxr mcp --live) edits the scene in that tab, as it does the desktop app.
Ctrl-C stops it."#;

#[cfg(unix)]
const NEXT: &str = "In the page, press Cmd-K, choose \"Terminal agents (MCP)…\" and type the code: the tab\nconnects on its eighth character. Then your agent's vxr-live server (vxr mcp --live)\nedits the scene there. Ctrl-C stops.";

#[cfg(unix)]
mod server {
    use std::collections::HashMap;
    use std::io::ErrorKind;
    use std::net::{SocketAddr, TcpListener, TcpStream};
    use std::path::PathBuf;
    use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
    use std::sync::{Arc, Mutex};
    use std::thread::JoinHandle;
    use std::time::{Duration, Instant};

    use serde_json::{Value, json};
    use tungstenite::handshake::server::{ErrorResponse, Request as Handshake, Response};
    use tungstenite::http::StatusCode;
    use tungstenite::protocol::WebSocketConfig;
    use tungstenite::{Message, WebSocket};

    use super::*;
    use crate::live::{Ask, Host, Request, same};
    use crate::protocol::{Reply, is_base64};

    /// How long a new connection has for its handshake, and then for its pairing code.
    const HANDSHAKE_WAIT: Duration = Duration::from_secs(5);
    const HELLO_WAIT: Duration = Duration::from_secs(10);
    /// Most connections not yet paired at once.
    const MAX_HANDSHAKES: usize = 8;
    /// Largest message either way, as the socket's longest line.
    const MAX_MESSAGE: usize = 4 << 20;
    /// How long a paired connection's read waits before the agents' requests are looked at.
    const POLL: Duration = Duration::from_millis(5);
    /// How long a message to the page may wait for it to read before the page is dropped.
    const WRITE_WAIT: Duration = Duration::from_secs(5);

    type Log = Arc<dyn Fn(&str) + Send + Sync>;

    /// Where the bridge listens and hosts the agents' socket.
    pub struct Config {
        /// A loopback address; port 0 picks a free one.
        pub addr: SocketAddr,
        /// The live folder (see [`crate::live::dir`]).
        pub dir: PathBuf,
        /// Also take pages served from this computer over plain HTTP (debug builds).
        pub dev_origins: bool,
    }

    struct Shared {
        code: Mutex<String>,
        failures: Mutex<u32>,
        paired: AtomicBool,
        handshakes: AtomicUsize,
        quit: AtomicBool,
        dir: PathBuf,
        dev: bool,
        port: u16,
        log: Log,
    }

    impl Shared {
        /// Check a pairing code and claim the one page slot.
        fn pair(&self, given: Option<&str>) -> Result<(), &'static str> {
            let mut code = self.code.lock().unwrap();
            let ok = given.and_then(normalize_code).is_some_and(|g| same(&g, &code));
            if !ok {
                let mut f = self.failures.lock().unwrap();
                *f += 1;
                if *f >= MAX_FAILURES {
                    *f = 0;
                    match new_code() {
                        Ok(c) => {
                            *code = c;
                            (self.log)(&format!(
                                "{MAX_FAILURES} wrong pairing codes: something on this computer may be guessing. The old code no longer works.\n\n  Pairing code: {}\n",
                                show_code(&code)
                            ));
                        }
                        // Without randomness for a new code, nothing may pair again.
                        Err(e) => {
                            code.clear();
                            (self.log)(&format!(
                                "{MAX_FAILURES} wrong pairing codes, and no new code: {e}. Restart vxr bridge."
                            ));
                        }
                    }
                } else {
                    (self.log)("Refused a wrong pairing code");
                }
                return Err(WRONG_CODE);
            }
            if self.paired.swap(true, Ordering::SeqCst) {
                return Err(BUSY);
            }
            // The user's own typos do not add up across pairings.
            *self.failures.lock().unwrap() = 0;
            Ok(())
        }
    }

    /// The bridge, listening; [`Bridge::serve`] runs it.
    pub struct Bridge {
        listener: TcpListener,
        shared: Arc<Shared>,
    }

    /// A bridge serving on a thread of its own ([`Bridge::spawn`]); dropping it stops it.
    pub struct Running {
        addr: SocketAddr,
        shared: Arc<Shared>,
        thread: Option<JoinHandle<()>>,
    }

    impl Bridge {
        /// Listen on `cfg.addr`, which must be loopback, with a new pairing code. `log` gets a
        /// line for each thing the user should see in the terminal.
        pub fn bind(cfg: Config, log: impl Fn(&str) + Send + Sync + 'static) -> Result<Bridge, String> {
            if !cfg.addr.ip().is_loopback() {
                return Err(format!(
                    "{} is not a loopback address: vxr bridge only listens on this computer",
                    cfg.addr.ip()
                ));
            }
            let listener = TcpListener::bind(cfg.addr).map_err(|e| match e.kind() {
                ErrorKind::AddrInUse => format!("port {} is in use: is vxr bridge already running?", cfg.addr.port()),
                _ => format!("could not listen on {}: {e}", cfg.addr),
            })?;
            listener.set_nonblocking(true).map_err(|e| e.to_string())?;
            let port = listener.local_addr().map_err(|e| e.to_string())?.port();
            let shared = Arc::new(Shared {
                code: Mutex::new(new_code()?),
                failures: Mutex::new(0),
                paired: AtomicBool::new(false),
                handshakes: AtomicUsize::new(0),
                quit: AtomicBool::new(false),
                dir: cfg.dir,
                dev: cfg.dev_origins,
                port,
                log: Arc::new(log),
            });
            Ok(Bridge { listener, shared })
        }

        pub fn addr(&self) -> SocketAddr {
            self.listener.local_addr().expect("a bound listener has an address")
        }

        /// The pairing code now, in canonical form.
        pub fn code(&self) -> String {
            self.shared.code.lock().unwrap().clone()
        }

        /// Accept pages until the process ends (or [`Running`] is dropped).
        pub fn serve(self) {
            let Bridge { listener, shared } = self;
            while !shared.quit.load(Ordering::SeqCst) {
                match listener.accept() {
                    Ok((stream, peer)) => {
                        // Bound to loopback, every peer is this computer; check anyway.
                        if !peer.ip().is_loopback()
                            || shared.handshakes.fetch_add(1, Ordering::SeqCst) >= MAX_HANDSHAKES
                        {
                            if peer.ip().is_loopback() {
                                shared.handshakes.fetch_sub(1, Ordering::SeqCst);
                            }
                            continue;
                        }
                        let s = shared.clone();
                        let spawned = std::thread::Builder::new().name("vxr-bridge-conn".into()).spawn({
                            let s = s.clone();
                            move || connection(stream, s)
                        });
                        if spawned.is_err() {
                            s.handshakes.fetch_sub(1, Ordering::SeqCst);
                        }
                    }
                    Err(e) if e.kind() == ErrorKind::WouldBlock => std::thread::sleep(Duration::from_millis(20)),
                    Err(_) => std::thread::sleep(Duration::from_millis(100)),
                }
            }
        }

        /// Serve on a thread of its own, until the returned handle is dropped.
        pub fn spawn(self) -> Running {
            let (addr, shared) = (self.addr(), self.shared.clone());
            let thread = std::thread::Builder::new().name("vxr-bridge".into()).spawn(move || self.serve()).ok();
            Running { addr, shared, thread }
        }
    }

    impl Running {
        pub fn addr(&self) -> SocketAddr {
            self.addr
        }

        pub fn code(&self) -> String {
            self.shared.code.lock().unwrap().clone()
        }

        /// Whether a page is paired now.
        pub fn paired(&self) -> bool {
            self.shared.paired.load(Ordering::SeqCst)
        }
    }

    impl Drop for Running {
        fn drop(&mut self) {
            self.shared.quit.store(true, Ordering::SeqCst);
            if let Some(t) = self.thread.take() {
                let _ = t.join();
            }
            // A paired page's thread sees `quit` within one poll and closes the socket.
            let start = Instant::now();
            while self.paired() && start.elapsed() < Duration::from_secs(2) {
                std::thread::sleep(Duration::from_millis(5));
            }
        }
    }

    fn forbidden(why: &str) -> ErrorResponse {
        let mut r = ErrorResponse::new(Some(why.to_string()));
        *r.status_mut() = StatusCode::FORBIDDEN;
        r
    }

    fn send(ws: &mut WebSocket<TcpStream>, v: &Value) -> bool {
        ws.send(Message::text(v.to_string())).is_ok()
    }

    /// Tell the page why it is turned away, and hang up.
    fn refuse(mut ws: WebSocket<TcpStream>, why: &str) {
        let _ = send(&mut ws, &json!({"error": why}));
        let _ = ws.close(None);
        let _ = ws.flush();
    }

    /// Whether a read only ran out of time.
    fn idle(e: &tungstenite::Error) -> bool {
        matches!(e, tungstenite::Error::Io(e) if matches!(e.kind(), ErrorKind::WouldBlock | ErrorKind::TimedOut))
    }

    /// Text from the other side, shown in the terminal: no control characters (so no escape
    /// sequences), cut short.
    fn shown(s: &str) -> String {
        let c: String = vxr_ops::sanitize(s);
        if c.chars().count() > 80 { c.chars().take(80).chain(['…']).collect() } else { c }
    }

    /// One connection: the handshake's checks, the pairing code, then the session.
    // The handshake's check returns tungstenite's error response, which is large.
    #[allow(clippy::result_large_err)]
    fn connection(stream: TcpStream, shared: Arc<Shared>) {
        let unpaired = Unpaired(shared.clone());
        let _ = stream.set_nonblocking(false);
        let _ = stream.set_read_timeout(Some(HANDSHAKE_WAIT));
        let _ = stream.set_write_timeout(Some(WRITE_WAIT));
        let mut refused = None;
        let check = |req: &Handshake, resp: Response| {
            let header = |k: &str| req.headers().get(k).and_then(|v| v.to_str().ok()).unwrap_or("");
            let (host, origin) = (header("host"), header("origin"));
            if !host_ok(host, shared.port) {
                refused = Some(format!("Refused a connection for another host ({})", shown(host)));
                return Err(forbidden("vxr bridge only answers 127.0.0.1"));
            }
            if !origin_ok(origin, shared.dev) {
                refused = Some(match origin {
                    "" => "Refused a connection with no origin".to_string(),
                    o => format!("Refused a connection from {}", shown(o)),
                });
                return Err(forbidden("vxr bridge only answers the vxr editor's page"));
            }
            Ok(resp)
        };
        let config = WebSocketConfig::default().max_message_size(Some(MAX_MESSAGE)).max_frame_size(Some(MAX_MESSAGE));
        // A failed handshake's error holds the check, so it is dropped before `refused` is read.
        let accepted = tungstenite::accept_hdr_with_config(stream, check, Some(config)).ok();
        if let Some(r) = refused {
            (shared.log)(&r);
        }
        let Some(mut ws) = accepted else { return };
        // The pairing code first, within HELLO_WAIT.
        let start = Instant::now();
        let hello = loop {
            let left = HELLO_WAIT.saturating_sub(start.elapsed());
            if left.is_zero() || ws.get_ref().set_read_timeout(Some(left)).is_err() {
                break None;
            }
            match ws.read() {
                Ok(Message::Text(t)) => break serde_json::from_str::<Value>(&t).ok(),
                Ok(Message::Ping(_) | Message::Pong(_)) => {}
                _ => break None,
            }
        };
        let Some(hello) = hello.filter(|h| h.get("vxr").and_then(Value::as_str) == Some("bridge")) else {
            return refuse(ws, NO_HELLO);
        };
        if let Err(why) = shared.pair(hello.get("code").and_then(Value::as_str)) {
            return refuse(ws, why);
        }
        let paired = Paired(shared.clone());
        drop(unpaired);
        let host = match Host::start(&shared.dir) {
            Ok(h) => h,
            Err(e) => {
                let e = if e.contains("already lets terminal agents") {
                    "The vxr desktop app already lets terminal agents edit it: turn that off there first.".to_string()
                } else {
                    format!("vxr bridge could not open the agents' socket: {e}")
                };
                (shared.log)(&e);
                return refuse(ws, &e);
            }
        };
        if !send(&mut ws, &json!({"ok": true})) {
            return;
        }
        (shared.log)("Paired with the page: terminal agents can edit its scene now");
        session(&mut ws, &host, &shared);
        drop(host);
        let _ = ws.close(None);
        let _ = ws.flush();
        drop(paired);
        (shared.log)("The page left: agents can no longer edit it");
    }

    /// A connection not yet paired, counted against MAX_HANDSHAKES until dropped.
    struct Unpaired(Arc<Shared>);

    impl Drop for Unpaired {
        fn drop(&mut self) {
            self.0.handshakes.fetch_sub(1, Ordering::SeqCst);
        }
    }

    /// The page slot, held while paired.
    struct Paired(Arc<Shared>);

    impl Drop for Paired {
        fn drop(&mut self) {
            self.0.paired.store(false, Ordering::SeqCst);
        }
    }

    /// A paired page: agents' requests out, the page's answers and commands in, until either
    /// side hangs up.
    fn session(ws: &mut WebSocket<TcpStream>, host: &Host, shared: &Shared) {
        if ws.get_ref().set_read_timeout(Some(POLL)).is_err() {
            return;
        }
        let mut pending: HashMap<u64, Request> = HashMap::new();
        let mut next = 0u64;
        let mut clients: Vec<(u64, String)> = Vec::new();
        while !shared.quit.load(Ordering::SeqCst) {
            match ws.read() {
                Ok(Message::Text(t)) => {
                    if !from_page(ws, host, &mut pending, &t) {
                        return;
                    }
                }
                Ok(Message::Ping(_) | Message::Pong(_) | Message::Frame(_)) => {}
                Ok(Message::Close(_) | Message::Binary(_)) => return,
                Err(e) if idle(&e) => {}
                Err(_) => return,
            }
            let asked = host.poll();
            let now = host.clients();
            if now != clients {
                for (c, name) in &now {
                    if !clients.iter().any(|(o, _)| o == c) {
                        (shared.log)(&format!("{name} connected"));
                    }
                }
                for (c, name) in &clients {
                    if !now.iter().any(|(n, _)| n == c) {
                        (shared.log)(&format!("{name} disconnected"));
                    }
                }
                clients = now;
                if !send(ws, &json!({"clients": clients})) {
                    return;
                }
            }
            for r in asked.into_iter().filter(|r| clients.iter().any(|(c, _)| *c == r.conn)) {
                next += 1;
                let mut msg = json!({"id": next, "conn": r.conn, "client": r.client});
                match &r.ask {
                    Ask::Call { name, args } => {
                        msg["call"] = json!(name);
                        msg["args"] = args.clone();
                    }
                    Ask::Read(uri) => msg["read"] = json!(uri),
                }
                if !send(ws, &msg) {
                    return;
                }
                pending.insert(next, r);
            }
        }
    }

    /// One message from the page. False ends the session.
    fn from_page(ws: &mut WebSocket<TcpStream>, host: &Host, pending: &mut HashMap<u64, Request>, text: &str) -> bool {
        let Ok(v) = serde_json::from_str::<Value>(text) else { return false };
        if let Some(id) = v.get("id").and_then(Value::as_u64) {
            // An answer to a request Stop or Regenerate dropped goes nowhere.
            if let Some(r) = pending.remove(&id) {
                if v.get("missing").and_then(Value::as_bool) == Some(true) {
                    r.missing();
                } else {
                    let text = v.get("text").and_then(Value::as_str).unwrap_or("").to_string();
                    let image = v.get("image").and_then(Value::as_str).filter(|i| is_base64(i)).map(str::to_string);
                    r.reply(if v.get("isError").and_then(Value::as_bool) == Some(true) {
                        Err(text)
                    } else {
                        Ok(Reply { text, image })
                    });
                }
            }
            return true;
        }
        if let Some(s) = v.get("stop") {
            let which: Vec<u64> = match s {
                Value::String(a) if a == "all" => host.clients().into_iter().map(|(c, _)| c).collect(),
                _ => s.as_u64().into_iter().collect(),
            };
            pending.retain(|_, r| !which.contains(&r.conn));
            for c in which {
                host.stop(c);
            }
            return true;
        }
        if v.get("regenerate").and_then(Value::as_bool) == Some(true) {
            let reply = match host.regenerate() {
                Ok(()) => {
                    pending.clear();
                    json!({"regenerated": true})
                }
                Err(e) => json!({"regenerated": false, "error": e}),
            };
            return send(ws, &reply);
        }
        // Anything else is for a later version of the page.
        true
    }
}
