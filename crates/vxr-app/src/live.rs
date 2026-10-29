//! The desktop app's end of live mode: the socket ([`vxr_mcp::live::Host`]), opened and
//! closed by the Agents tab's switch, and the agents' requests, run on the
//! editor's thread once a frame ([`Editor::agent_call`]). A request waits while the user is
//! in the middle of a drag, so an agent never edits under the pointer.

use std::collections::VecDeque;
use std::path::{Path, PathBuf};

use serde_json::json;
use vxr_mcp::Reply;
use vxr_mcp::live::{Ask, EXPORT_TOOL, Host, Request};

use crate::Editor;
use crate::ui::Action;

pub struct Live {
    dir: PathBuf,
    host: Option<Host>,
    pending: VecDeque<Request>,
    /// An agent's `export` of a render, answered once the export ends.
    waiting: Option<Request>,
    /// Where an agent's exports go: the user's Downloads folder.
    downloads: PathBuf,
}

impl Live {
    /// Live mode in `dir` (normally [`vxr_mcp::live::dir`]), off until the user turns it on.
    pub fn new(dir: PathBuf) -> Live {
        let home = std::env::var_os("HOME").map(PathBuf::from).unwrap_or_else(std::env::temp_dir);
        Live { dir, host: None, pending: VecDeque::new(), waiting: None, downloads: home.join("Downloads") }
    }

    /// Put agents' exports in `dir` instead of Downloads.
    pub fn downloads_to(mut self, dir: PathBuf) -> Live {
        self.downloads = dir;
        self
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
                    ed.agents.fail = None;
                    ed.ui.toast(&ctx, "Terminal agents can edit this window");
                }
                Err(e) => {
                    ed.ui.toast(&ctx, format!("Could not allow terminal agents: {e}"));
                    ed.agents.error = Some(e);
                    ed.agents.fail = Some(crate::agents::Fail::Other);
                }
            },
            Action::AllowAgents(false) => {
                // Requests not run yet are dropped: their agents are told the window is off.
                self.pending.clear();
                self.waiting = None;
                ed.forget_agent_render();
                if self.host.take().is_some() {
                    ed.ui.toast(&ctx, "Terminal agents can no longer edit this window");
                }
                ed.agents.since = None;
            }
            Action::AllowAgents(true) => {}
            Action::DismissAgentsError => {
                ed.agents.error = None;
                ed.agents.fail = None;
            }
            Action::RegenerateToken => match self.host.as_ref().map(Host::regenerate) {
                Some(Ok(())) => {
                    self.pending.clear();
                    if let Some(h) = &self.host {
                        let open = h.clients();
                        self.pending.extend(h.poll().into_iter().filter(|r| open.iter().any(|(c, _)| *c == r.conn)));
                    }
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
        self.pending.extend(h.poll());
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
        let now = ed.ctx.input(|i| i.time);
        if ed.agents.allowed {
            ed.agents.since.get_or_insert(now);
        } else {
            ed.agents.since = None;
        }
        let clients = self.host.as_ref().map(Host::clients).unwrap_or_default();
        for (conn, _) in std::mem::replace(&mut ed.agents.clients, clients.clone()) {
            if !clients.iter().any(|(c, _)| *c == conn) {
                ed.agents.forget(conn);
            }
        }
        ed.agents.note_clients(now);
    }

    /// Run what agents asked since the last frame, unless the user is mid-drag.
    pub fn tick(&mut self, ed: &mut Editor) {
        if let Some(h) = &self.host {
            self.pending.extend(h.poll());
            let open = h.clients();
            self.pending.retain(|r| open.iter().any(|(c, _)| *c == r.conn));
            if self.waiting.as_ref().is_some_and(|w| !open.iter().any(|(c, _)| *c == w.conn)) {
                self.waiting = None;
                ed.forget_agent_render();
            }
        }
        if self.waiting.is_some()
            && let Some(r) = ed.take_agent_render()
        {
            let w = self.waiting.take().unwrap();
            let client = w.client.clone();
            w.answer(r.and_then(|what| self.deliver(ed, &client, &what)));
        }
        while !ed.agent_busy() && self.waiting.is_none() {
            let Some(r) = self.pending.pop_front() else { break };
            match r.ask.clone() {
                Ask::Call { name, args } => {
                    let client = r.client.clone();
                    let conn = r.conn;
                    let Some(a) = ed.agent_call(conn, &client, &name, &args) else {
                        self.pending.push_front(r);
                        continue;
                    };
                    if ed.agent_waits(conn) {
                        self.waiting = Some(r);
                    } else if name == EXPORT_TOOL {
                        r.reply(a.and_then(|a| self.deliver(ed, &client, &a.text).map(Reply::from)));
                    } else {
                        r.reply(a);
                    }
                }
                Ask::Read(uri) => match ed.agent_read(&uri) {
                    Some(text) => r.answer(text),
                    None => r.missing(),
                },
            }
        }
        self.sync(ed);
    }

    /// Write an agent's exported files into Downloads, as a browser names a download (never
    /// over a file there: "x (1).png"), and the answer naming where they went.
    fn deliver(&self, ed: &mut Editor, client: &str, what: &str) -> Result<String, String> {
        let mut files = std::mem::take(&mut ed.agent_files);
        std::fs::create_dir_all(&self.downloads).map_err(|e| format!("could not write into Downloads: {e}"))?;
        let (mut written, mut names) = (Vec::new(), Vec::new());
        if let Some((old, new)) = unused_stem(&self.downloads, &files) {
            files = if files.len() > 1 {
                // An OBJ's three files name each other: made again under the new name.
                crate::files::obj_files(&ed.scene, &new)?.0
            } else {
                files.into_iter().map(|(n, b)| (n.replacen(old.as_str(), &new, 1), b)).collect()
            };
        }
        for (name, bytes) in &files {
            let path = self.downloads.join(name);
            write_new(&path, bytes).map_err(|e| format!("could not write {name} into Downloads: {e}"))?;
            written.push(path.to_string_lossy().into_owned());
            names.push(name.clone());
        }
        let ctx = ed.ctx.clone();
        ed.ui.toast(&ctx, format!("{client} exported {} into Downloads", names.join(", ")));
        Ok(json!({"exported": written, "what": what, "note": "written into the user's Downloads folder"}).to_string())
    }
}

/// The stem the files can share in `dir` without replacing a file: their own, or with " (n)"
/// after it. None keeps their names.
fn unused_stem(dir: &Path, files: &[crate::files::File]) -> Option<(String, String)> {
    let first = &files.first()?.0;
    let stem = Path::new(first).file_stem()?.to_string_lossy().into_owned();
    let free = |s: &str| files.iter().all(|(n, _)| !dir.join(n.replacen(stem.as_str(), s, 1)).exists());
    if free(&stem) {
        return None;
    }
    (1..1000).map(|i| format!("{stem} ({i})")).find(|s| free(s)).map(|s| (stem, s))
}

/// Write a file that is not there yet.
fn write_new(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    use std::io::Write;
    std::fs::OpenOptions::new().write(true).create_new(true).open(path)?.write_all(bytes)
}
