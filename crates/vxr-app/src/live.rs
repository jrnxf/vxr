//! The desktop app's end of live mode: the socket ([`vxr_mcp::live::Host`]), opened and
//! closed by the Agents tab's switch, and the agents' requests, run on the
//! editor's thread once a frame ([`Editor::agent_call`]). A request waits while the user is
//! in the middle of a drag, so an agent never edits under the pointer.

use std::collections::VecDeque;
use std::path::{Path, PathBuf};

use serde_json::json;
use vxr_mcp::Reply;
use vxr_mcp::live::{Ask, EXPORT_TOOL, Host, Request, Wake};

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
    /// Called from the host's threads when a request arrives, for a window that sleeps.
    wake: Option<Wake>,
}

impl Live {
    /// Live mode in `dir` (normally [`vxr_mcp::live::dir`]), off until the user turns it on.
    pub fn new(dir: PathBuf) -> Live {
        let home = std::env::var_os("HOME").map(PathBuf::from).unwrap_or_else(std::env::temp_dir);
        Live { dir, host: None, pending: VecDeque::new(), waiting: None, downloads: home.join("Downloads"), wake: None }
    }

    /// Put agents' exports in `dir` instead of Downloads.
    pub fn downloads_to(mut self, dir: PathBuf) -> Live {
        self.downloads = dir;
        self
    }

    /// Call `wake` whenever an agent's request arrives, so a window that sleeps between
    /// events can run [`Live::tick`] at once, even when it is hidden and draws nothing.
    pub fn waking(mut self, wake: Wake) -> Live {
        self.wake = Some(wake);
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
            Action::AllowAgents(true) if self.host.is_none() => match match &self.wake {
                Some(w) => Host::start_waking(&self.dir, w.clone()),
                None => Host::start(&self.dir),
            } {
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

    /// Write an agent's exported files into Downloads, never over a file there ("x-1.png"),
    /// and the answer naming where they went.
    fn deliver(&self, ed: &mut Editor, client: &str, what: &str) -> Result<String, String> {
        let files = std::mem::take(&mut ed.agent_files);
        std::fs::create_dir_all(&self.downloads).map_err(|e| format!("could not write into Downloads: {e}"))?;
        let (mut written, mut names) = (Vec::new(), Vec::new());
        let files = unused_names(&self.downloads, files, &ed.scene)?;
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

/// `files` under names that replace nothing in `dir`: their own, or with "-n" after their
/// stem. No space, since an OBJ's `mtllib` and its `.mtl`'s `map_Kd` split names at one; an
/// OBJ's three files name each other, so they are made again from `scene` under the new name.
fn unused_names(
    dir: &Path,
    files: Vec<crate::files::File>,
    scene: &vxr_core::Scene,
) -> Result<Vec<crate::files::File>, String> {
    let Some((old, new)) = unused_stem(dir, &files) else { return Ok(files) };
    if files.len() > 1 {
        return Ok(crate::files::obj_files(scene, &new)?.0);
    }
    Ok(files.into_iter().map(|(n, b)| (n.replacen(old.as_str(), &new, 1), b)).collect())
}

/// The stem the files can share in `dir` without replacing a file: their own, or with "-n"
/// after it. None keeps their names.
fn unused_stem(dir: &Path, files: &[crate::files::File]) -> Option<(String, String)> {
    let first = &files.first()?.0;
    let stem = Path::new(first).file_stem()?.to_string_lossy().into_owned();
    let free = |s: &str| files.iter().all(|(n, _)| !dir.join(n.replacen(stem.as_str(), s, 1)).exists());
    if free(&stem) {
        return None;
    }
    (1..1000).map(|i| format!("{stem}-{i}")).find(|s| free(s)).map(|s| (stem, s))
}

/// Write a file that is not there yet.
fn write_new(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    use std::io::Write;
    std::fs::OpenOptions::new().write(true).create_new(true).open(path)?.write_all(bytes)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_second_obj_export_of_a_spaced_name_names_files_without_spaces_that_name_each_other() {
        let dir = std::env::temp_dir().join(format!("vxr-live-names-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let mut scene = vxr_core::Scene::new([4, 4, 4]);
        scene.edit([([1, 1, 1], 3)]);
        let export = || crate::files::obj_files(&scene, "my house").unwrap().0;
        for want in
            [["my-house.obj", "my-house.mtl", "my-house.png"], ["my-house-1.obj", "my-house-1.mtl", "my-house-1.png"]]
        {
            let files = unused_names(&dir, export(), &scene).unwrap();
            let names: Vec<&str> = files.iter().map(|f| f.0.as_str()).collect();
            assert_eq!(names, want);
            for (name, bytes) in &files {
                write_new(&dir.join(name), bytes).unwrap();
            }
            let line = |file: &str, key: &str| {
                let text = std::fs::read_to_string(dir.join(file)).unwrap();
                text.lines().find_map(|l| l.strip_prefix(key)).unwrap().to_string()
            };
            let (mtl, png) = (line(want[0], "mtllib "), line(want[1], "map_Kd "));
            assert!(!mtl.contains(char::is_whitespace) && !png.contains(char::is_whitespace), "{mtl} {png}");
            assert_eq!([mtl.as_str(), png.as_str()], [want[1], want[2]]);
        }
        let slices = vec![("my-house-slices.png".to_string(), vec![1])];
        write_new(&dir.join(&slices[0].0), &slices[0].1).unwrap();
        let again = unused_names(&dir, slices, &scene).unwrap();
        assert_eq!(again[0].0, "my-house-slices-1.png");
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
