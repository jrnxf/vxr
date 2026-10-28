//! The desktop app's end of live mode: the socket ([`vxr_mcp::live::Host`]), opened and
//! closed by the Terminal agents panel's switch, and the agents' requests, run on the
//! editor's thread once a frame ([`Editor::agent_call`]). A request waits while the user is
//! in the middle of a drag, so an agent never edits under the pointer.

use std::collections::VecDeque;
use std::path::PathBuf;

use vxr_mcp::live::{Ask, Host, Request};

use crate::Editor;
use crate::ui::Action;

pub struct Live {
    dir: PathBuf,
    host: Option<Host>,
    pending: VecDeque<Request>,
}

impl Live {
    /// Live mode in `dir` (normally [`vxr_mcp::live::dir`]), off until the user turns it on.
    pub fn new(dir: PathBuf) -> Live {
        Live { dir, host: None, pending: VecDeque::new() }
    }

    /// Whether the window listens for agents.
    pub fn on(&self) -> bool {
        self.host.is_some()
    }

    /// Handle what the UI asked of live mode: the switch, a new token, Stop. Returns whether
    /// the action was one of those.
    pub fn act(&mut self, a: &Action, ed: &mut Editor) -> bool {
        let ctx = ed.ctx.clone();
        match a {
            Action::AllowAgents(true) if self.host.is_none() => match Host::start(&self.dir) {
                Ok(h) => {
                    self.host = Some(h);
                    ed.agents.error = None;
                    ed.ui.toast(&ctx, "Terminal agents can edit this window");
                }
                Err(e) => {
                    ed.ui.toast(&ctx, format!("Could not allow terminal agents: {e}"));
                    ed.agents.error = Some(e);
                }
            },
            Action::AllowAgents(false) => {
                // Requests not run yet are dropped: their agents are told the window is off.
                self.pending.clear();
                if self.host.take().is_some() {
                    ed.ui.toast(&ctx, "Terminal agents can no longer edit this window");
                }
            }
            Action::AllowAgents(true) => {}
            Action::RegenerateToken => match self.host.as_ref().map(Host::regenerate) {
                Some(Ok(())) => {
                    self.pending.clear();
                    ed.ui.toast(&ctx, "New pairing token: every agent was disconnected");
                }
                Some(Err(e)) => ed.ui.toast(&ctx, format!("Could not make a new token: {e}")),
                None => {}
            },
            Action::StopAgent(conn) => self.stop(ed, |c| c == *conn),
            Action::StopAgents => self.stop(ed, |_| true),
            _ => return false,
        }
        self.sync(ed);
        true
    }

    fn stop(&mut self, ed: &mut Editor, which: impl Fn(u64) -> bool) {
        let Some(h) = &self.host else { return };
        let stopped: Vec<(u64, String)> = h.clients().into_iter().filter(|(c, _)| which(*c)).collect();
        self.pending.retain(|r| !which(r.conn));
        for (c, _) in &stopped {
            h.stop(*c);
        }
        let ctx = ed.ctx.clone();
        match stopped.as_slice() {
            [] => {}
            [(_, name)] => ed.ui.toast(&ctx, format!("Stopped {name}")),
            many => ed.ui.toast(&ctx, format!("Stopped {} agents", many.len())),
        }
    }

    fn sync(&self, ed: &mut Editor) {
        ed.agents.available = true;
        ed.agents.allowed = self.host.is_some();
        ed.agents.clients = self.host.as_ref().map(Host::clients).unwrap_or_default();
    }

    /// Run what agents asked since the last frame, unless the user is mid-drag.
    pub fn tick(&mut self, ed: &mut Editor) {
        if let Some(h) = &self.host {
            self.pending.extend(h.poll());
        }
        while !ed.agent_busy() {
            let Some(r) = self.pending.pop_front() else { break };
            match r.ask.clone() {
                Ask::Call { name, args } => {
                    let client = r.client.clone();
                    r.answer(ed.agent_call(&client, &name, &args));
                }
                Ask::Read(uri) => match ed.agent_read(&uri) {
                    Some(text) => r.answer(text),
                    None => r.missing(),
                },
            }
        }
        self.sync(ed);
    }
}
