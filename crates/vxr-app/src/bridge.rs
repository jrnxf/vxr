//! The browser editor's end of live mode: the tab pairs with `vxr bridge` on the user's
//! computer ([`vxr_mcp::bridge`]), which hosts the socket `vxr mcp --live` connects to and
//! hands each agent's request to the tab. The requests run on the editor once a frame, as the
//! desktop app's do ([`crate::live`]), and wait while the user is in the middle of a drag.
//!
//! No I/O here: the page's script owns the WebSocket (`crates/vxr-web/www/app.js`). It opens
//! one on [`Out::Connect`], hands each message in ([`Page::message`]) and sends what
//! [`Page::take_out`] gives it, so the same code runs natively against a real bridge in tests.

use std::collections::VecDeque;

use serde_json::{Value, json};
use vxr_mcp::bridge::{PORT, normalize_code};

use crate::Editor;
use crate::ui::Action;

/// What the page's script does with the WebSocket.
#[derive(Clone, Debug, PartialEq)]
pub enum Out {
    /// Open one to [`vxr_mcp::bridge::url`].
    Connect,
    /// Send one text message.
    Send(String),
    /// Close it.
    Close,
}

/// Where the tab is with the bridge.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Link {
    Off,
    /// The WebSocket is opening (the browser may be asking the user first).
    Connecting,
    /// Open, the pairing code sent, no answer yet.
    Pairing,
    Paired,
}

enum Ask {
    Call { name: String, args: Value },
    Read(String),
}

struct Request {
    id: Value,
    conn: u64,
    client: String,
    ask: Ask,
}

/// Why the WebSocket did not open, as the page's script tells [`Page::closed`].
pub fn failure(why: &str) -> String {
    match why {
        "denied" => "This browser blocks the page from reaching apps on this computer. Allow it in the site settings (the icon left of the address: Apps on device, or Local network access), then connect again.".into(),
        "blocked" => "This browser does not let a web page reach apps on this computer. Use Chrome, Edge or Firefox, or the desktop app.".into(),
        "safari" => format!("Nothing answered on 127.0.0.1:{PORT}. Safari may not let a web page reach apps on this computer: if vxr bridge is running, use Chrome, Edge or Firefox, or the desktop app."),
        "brave" => format!("Nothing answered on 127.0.0.1:{PORT}. Brave blocks pages from apps on this computer unless allowed: run vxr bridge, allow Localhost access for this site in its settings, and connect again."),
        _ => format!("Nothing answered on 127.0.0.1:{PORT}: run vxr bridge in a terminal and connect again. If the browser asks to let this page reach apps on this device, allow it."),
    }
}

pub const BAD_CODE: &str = "Type the code vxr bridge printed: 8 letters and digits, like K7QF-3MXP.";
pub const HUNG_UP: &str = "vxr bridge hung up before pairing: connect again.";
pub const LOST: &str =
    "vxr bridge stopped, so agents can no longer edit this tab. Run it again and connect with its new code.";

/// Text from the bridge, shown in the UI: no control characters, cut short.
fn shown(s: &str, max: usize) -> String {
    s.chars().filter(|c| !c.is_control()).take(max).collect::<String>().trim().to_string()
}

/// The tab's link to the bridge, and the agents' requests waiting to run.
pub struct Page {
    link: Link,
    code: String,
    pending: VecDeque<Request>,
    out: Vec<Out>,
}

impl Default for Page {
    fn default() -> Self {
        Page::new()
    }
}

impl Page {
    pub fn new() -> Page {
        Page { link: Link::Off, code: String::new(), pending: VecDeque::new(), out: Vec::new() }
    }

    pub fn link(&self) -> Link {
        self.link
    }

    /// What the page's script should do now, oldest first.
    pub fn take_out(&mut self) -> Vec<Out> {
        std::mem::take(&mut self.out)
    }

    /// Handle what the UI asked of live mode: pair with a code, turn off, a new token, Stop.
    /// Returns whether the action was one of those.
    pub fn act(&mut self, a: &Action, ed: &mut Editor) -> bool {
        let ctx = ed.ctx.clone();
        match a {
            Action::PairBridge if self.link == Link::Off => match normalize_code(&ed.ui.agents_code) {
                Some(c) => {
                    self.code = c;
                    self.link = Link::Connecting;
                    ed.agents.error = None;
                    self.out.push(Out::Connect);
                }
                None => ed.agents.error = Some(BAD_CODE.into()),
            },
            Action::PairBridge | Action::AllowAgents(true) => {}
            Action::AllowAgents(false) => {
                if self.link != Link::Off {
                    self.out.push(Out::Close);
                    let was = std::mem::replace(&mut self.link, Link::Off);
                    self.drop_all(ed);
                    if was == Link::Paired {
                        ed.ui.toast(&ctx, "Terminal agents can no longer edit this tab");
                    }
                }
            }
            Action::RegenerateToken if self.link == Link::Paired => {
                self.pending.clear();
                self.send(json!({"regenerate": true}));
            }
            Action::RegenerateToken => {}
            Action::StopAgent(conn) => self.stop(ed, Some(*conn)),
            Action::StopAgents => self.stop(ed, None),
            _ => return false,
        }
        self.sync(ed);
        true
    }

    fn send(&mut self, v: Value) {
        self.out.push(Out::Send(v.to_string()));
    }

    fn stop(&mut self, ed: &mut Editor, which: Option<u64>) {
        if self.link != Link::Paired {
            return;
        }
        let stopped: Vec<String> =
            ed.agents.clients.iter().filter(|(c, _)| which.is_none_or(|w| w == *c)).map(|(_, n)| n.clone()).collect();
        self.pending.retain(|r| which.is_some_and(|w| w != r.conn));
        match which {
            Some(c) => self.send(json!({"stop": c})),
            None => self.send(json!({"stop": "all"})),
        }
        // The bridge's next clients message confirms it; until then the chip is gone.
        ed.agents.clients.retain(|(c, _)| which.is_some_and(|w| w != *c));
        let ctx = ed.ctx.clone();
        match stopped.as_slice() {
            [] => {}
            [name] => ed.ui.toast(&ctx, format!("Stopped {name}")),
            many => ed.ui.toast(&ctx, format!("Stopped {} agents", many.len())),
        }
    }

    /// Requests and agents go with the link.
    fn drop_all(&mut self, ed: &mut Editor) {
        self.pending.clear();
        for (conn, _) in std::mem::take(&mut ed.agents.clients) {
            ed.agents.forget(conn);
        }
    }

    /// The WebSocket opened: the pairing code goes first.
    pub fn opened(&mut self) {
        if self.link == Link::Connecting {
            self.link = Link::Pairing;
            let code = std::mem::take(&mut self.code);
            self.send(json!({"vxr": "bridge", "code": code}));
        }
    }

    /// The WebSocket closed, or never opened: `why` is the page script's reason when it could
    /// tell (see [`failure`]).
    pub fn closed(&mut self, why: &str, ed: &mut Editor) {
        let was = std::mem::replace(&mut self.link, Link::Off);
        self.code.clear();
        match was {
            Link::Off => return,
            Link::Connecting => ed.agents.error = Some(failure(why)),
            Link::Pairing => {
                ed.agents.error.get_or_insert_with(|| HUNG_UP.into());
            }
            Link::Paired => {
                let ctx = ed.ctx.clone();
                ed.ui.toast(&ctx, "vxr bridge stopped: agents can no longer edit this tab");
                ed.agents.error = Some(LOST.into());
            }
        }
        self.drop_all(ed);
        self.sync(ed);
    }

    /// One message from the bridge.
    pub fn message(&mut self, text: &str, ed: &mut Editor) {
        let Ok(v) = serde_json::from_str::<Value>(text) else { return };
        match self.link {
            Link::Pairing => {
                if v.get("ok").and_then(Value::as_bool) == Some(true) {
                    self.link = Link::Paired;
                    ed.agents.error = None;
                    let ctx = ed.ctx.clone();
                    ed.ui.toast(&ctx, "Paired with vxr bridge: terminal agents can edit this tab");
                } else {
                    let e = v.get("error").and_then(Value::as_str).unwrap_or(HUNG_UP);
                    ed.agents.error = Some(shown(e, 300));
                    self.link = Link::Off;
                    self.out.push(Out::Close);
                    self.drop_all(ed);
                }
            }
            Link::Paired => self.paired_message(&v, ed),
            Link::Off | Link::Connecting => {}
        }
        self.sync(ed);
    }

    fn paired_message(&mut self, v: &Value, ed: &mut Editor) {
        if let Some(id) = v.get("id").filter(|id| id.is_u64()) {
            let conn = v.get("conn").and_then(Value::as_u64).unwrap_or(0);
            let client = shown(v.get("client").and_then(Value::as_str).unwrap_or(""), 32);
            let ask = match (v.get("call").and_then(Value::as_str), v.get("read").and_then(Value::as_str)) {
                (Some(name), _) => {
                    Ask::Call { name: name.to_string(), args: v.get("args").cloned().unwrap_or_else(|| json!({})) }
                }
                (None, Some(uri)) => Ask::Read(uri.to_string()),
                _ => return,
            };
            self.pending.push_back(Request { id: id.clone(), conn, client, ask });
        } else if let Some(list) = v.get("clients").and_then(Value::as_array) {
            let clients: Vec<(u64, String)> =
                list.iter().filter_map(|c| Some((c.get(0)?.as_u64()?, shown(c.get(1)?.as_str()?, 32)))).collect();
            let gone: Vec<u64> =
                ed.agents.clients.iter().map(|(c, _)| *c).filter(|c| !clients.iter().any(|(n, _)| n == c)).collect();
            for conn in gone {
                ed.agents.forget(conn);
            }
            self.pending.retain(|r| clients.iter().any(|(c, _)| *c == r.conn));
            ed.agents.clients = clients;
        } else if let Some(ok) = v.get("regenerated").and_then(Value::as_bool) {
            let ctx = ed.ctx.clone();
            let msg = if ok {
                "New pairing token: every agent was disconnected".to_string()
            } else {
                format!(
                    "Could not make a new token: {}",
                    shown(v.get("error").and_then(Value::as_str).unwrap_or(""), 200)
                )
            };
            ed.ui.toast(&ctx, msg);
        }
    }

    /// Run what agents asked since the last frame, unless the user is mid-drag.
    pub fn tick(&mut self, ed: &mut Editor) {
        while self.link == Link::Paired && !ed.agent_busy() {
            let Some(r) = self.pending.pop_front() else { break };
            let reply = match r.ask {
                Ask::Call { name, args } => {
                    let (text, is_error) = match ed.agent_call(r.conn, &r.client, &name, &args) {
                        Ok(t) => (t, false),
                        Err(e) => (e, true),
                    };
                    json!({"id": r.id, "text": text, "isError": is_error})
                }
                Ask::Read(uri) => match ed.agent_read(&uri) {
                    Some(Ok(t)) => json!({"id": r.id, "text": t, "isError": false}),
                    Some(Err(e)) => json!({"id": r.id, "text": e, "isError": true}),
                    None => json!({"id": r.id, "missing": true}),
                },
            };
            self.send(reply);
        }
        self.sync(ed);
    }

    /// The panel's state: the browser's variant, paired or not, and connecting.
    pub fn sync(&self, ed: &mut Editor) {
        ed.agents.available = true;
        ed.agents.web = true;
        ed.agents.allowed = self.link == Link::Paired;
        ed.agents.connecting = matches!(self.link, Link::Connecting | Link::Pairing);
    }
}
