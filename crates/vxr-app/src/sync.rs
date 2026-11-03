//! The optional account and its library of projects, in the browser.
//!
//! Signed in on the page's `/account`, the editor's projects go to the account's library on the
//! vxr Worker (`crates/vxr-web/worker`): Save uploads the scene there, the projects list shows
//! the library's projects too, and opening one this device does not have downloads it. Each
//! save names the version it replaces; when another device saved first, the Worker keeps both,
//! this save as "<name> (conflicted copy)" ([`Event::Renamed`]).
//!
//! [`Library`] has no network of its own: it queues [`Call`]s, the page sends them
//! ([`Library::take_out`]) and hands each [`Reply`] back to [`Library::answer`], which says what the
//! editor should do ([`Event`]). The desktop never signs in, so its account stays
//! [`Account::Off`]. [`FakeLibrary`] answers the calls in memory, as the Worker does, for the tests
//! and the headless check.

use std::collections::HashMap;

use serde::Deserialize;

use crate::projects::{Cloud, Projects};

/// Where the account is.
#[derive(Clone, Debug, Default, PartialEq)]
pub enum Account {
    /// No sign-in here: the desktop, or a page whose Worker has none.
    #[default]
    Off,
    /// Sign-in is there, and nobody is signed in.
    Out,
    In(User),
}

impl Account {
    pub fn user(&self) -> Option<&User> {
        match self {
            Account::In(u) => Some(u),
            _ => None,
        }
    }
}

/// The signed-in user.
#[derive(Clone, Debug, PartialEq)]
pub struct User {
    pub id: String,
    /// None for an account made with a passkey only.
    pub email: Option<String>,
    /// The bytes the library holds, and the most it and one file may.
    pub usage: u64,
    pub limit: u64,
    pub file_limit: u64,
}

/// A project as the library lists it.
#[derive(Clone, Debug, Deserialize, PartialEq)]
pub struct Remote {
    pub id: String,
    pub name: String,
    pub version: String,
    #[serde(default)]
    pub size: u64,
    #[serde(default)]
    pub thumb: bool,
    #[serde(default)]
    pub device: String,
    /// Milliseconds since 1970.
    #[serde(default)]
    pub updated: f64,
}

/// A request for the page to send, same-origin with the session cookie.
#[derive(Clone, Debug, PartialEq)]
pub struct Call {
    pub id: u32,
    pub method: &'static str,
    pub path: String,
    pub headers: Vec<(&'static str, String)>,
    pub body: Vec<u8>,
}

/// What came back: `status` 0 when the request did not get through. `version` is the
/// `x-vxr-version` header of a download.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Reply {
    pub status: u16,
    pub body: Vec<u8>,
    pub version: Option<String>,
}

/// What a reply asks of the editor.
#[derive(Clone, Debug, PartialEq)]
pub enum Event {
    Toast(String),
    /// Open `project` from the library's `bytes` at `version`: chosen in the list, or
    /// (`reload`) a newer version of the open scene, unless it has edits by now.
    Open {
        project: String,
        bytes: Vec<u8>,
        version: String,
        reload: bool,
    },
    /// The library could not give `project`, which has a copy on this device: open that.
    OpenHere {
        project: String,
    },
    /// The save of `project` was kept as a copy named `name`, which the open scene takes.
    Renamed {
        project: String,
        name: String,
    },
    /// The account or the library list changed.
    Listed,
}

/// What a call in flight is for.
enum Want {
    Me,
    /// The list asked for as the `generation`th.
    List {
        generation: u32,
    },
    Upload {
        project: String,
        thumb: Option<Vec<u8>>,
    },
    /// `from` is the open scene's project as the download was asked for.
    Download {
        project: String,
        reload: bool,
        from: Option<String>,
    },
    GetThumb {
        project: String,
    },
    PutThumb,
}

/// A save waiting for the one before it to finish.
type Queued = Option<(Vec<u8>, Option<Vec<u8>>)>;

#[derive(Default)]
pub struct Library {
    pub account: Account,
    out: Vec<Call>,
    next: u32,
    waiting: HashMap<u32, Want>,
    /// The projects with an upload in flight, each with the newest save made since.
    uploading: HashMap<String, Queued>,
    /// How many lists were asked for, whether another is wanted when the one in flight
    /// answers, and the library ids uploaded since, each with the lists asked for before it
    /// went up: a list asked for before an upload may not show it yet.
    lists: u32,
    relist: bool,
    uploaded: HashMap<String, u32>,
}

#[derive(Deserialize)]
struct Limits {
    account: u64,
    file: u64,
}

#[derive(Deserialize)]
struct MeUser {
    id: String,
    #[serde(default)]
    email: Option<String>,
}

#[derive(Deserialize)]
struct Me {
    #[serde(rename = "signIn")]
    sign_in: bool,
    user: Option<MeUser>,
    #[serde(default)]
    usage: u64,
    limits: Limits,
}

#[derive(Deserialize)]
struct List {
    projects: Vec<Remote>,
    usage: u64,
}

#[derive(Deserialize)]
struct Put {
    project: Remote,
}

#[derive(Deserialize)]
struct Conflict {
    copy: Remote,
}

#[derive(Deserialize)]
struct Problem {
    error: String,
    #[serde(default)]
    limit: u64,
}

impl Library {
    /// Ask the Worker who is signed in (at launch, and when the page comes back into view).
    pub fn check(&mut self) {
        if !self.waiting.values().any(|w| matches!(w, Want::Me)) {
            self.send(Want::Me, "GET", "/api/me".into(), Vec::new(), Vec::new());
        }
    }

    /// The calls to send now.
    pub fn take_out(&mut self) -> Vec<Call> {
        std::mem::take(&mut self.out)
    }

    /// Whether a call is in flight.
    pub fn busy(&self) -> bool {
        !self.waiting.is_empty()
    }

    /// Whether `project` is uploading.
    pub fn uploading(&self, project: &str) -> bool {
        self.uploading.contains_key(project)
    }

    fn send(
        &mut self,
        want: Want,
        method: &'static str,
        path: String,
        headers: Vec<(&'static str, String)>,
        body: Vec<u8>,
    ) {
        self.next += 1;
        self.out.push(Call { id: self.next, method, path, headers, body });
        self.waiting.insert(self.next, want);
    }

    /// Ask for the library's list, once the one in flight (if any) has answered.
    pub fn list(&mut self) {
        if self.account.user().is_none() {
            return;
        }
        if self.waiting.values().any(|w| matches!(w, Want::List { .. })) {
            self.relist = true;
            return;
        }
        self.lists += 1;
        let generation = self.lists;
        self.send(Want::List { generation }, "GET", "/api/projects".into(), Vec::new(), Vec::new());
    }

    /// Upload `vox` (and its `thumb`) as the newest version of `project`, which joins the
    /// library when it is not in it. One upload per project is in flight; a save made during
    /// it goes once it is done.
    pub fn upload(&mut self, projects: &mut Projects, project: &str, vox: Vec<u8>, thumb: Option<Vec<u8>>) {
        let Some(user) = self.account.user().map(|u| u.id.clone()) else { return };
        if let Some(next) = self.uploading.get_mut(project) {
            *next = Some((vox, thumb));
            return;
        }
        let Some(p) = projects.get(project) else { return };
        let mut cloud = p.library(&user).cloned().unwrap_or(Cloud {
            user,
            id: project.into(),
            version: None,
            latest: None,
            dirty: true,
            held: false,
        });
        cloud.dirty = true;
        let name = p.name.strip_suffix(".vox").unwrap_or(&p.name).to_string();
        let precondition = match &cloud.version {
            Some(v) => ("if-match", format!("\"{v}\"")),
            None => ("if-none-match", "*".into()),
        };
        let path = format!("/api/projects/{}", cloud.id);
        projects.update(project, |p| p.cloud = Some(cloud));
        self.uploading.insert(project.into(), None);
        let headers = vec![
            precondition,
            ("x-vxr-name", encode(&name)),
            ("x-vxr-device", projects.device().into()),
            ("content-type", "application/octet-stream".into()),
        ];
        self.send(Want::Upload { project: project.into(), thumb }, "PUT", path, headers, vox);
    }

    /// Whether opening `project` takes the library's copy: it has none here, or the library's
    /// is newer and this device has no edits of its own (a draft, a save not up yet, or one
    /// the library refused).
    pub fn wants_download(&self, projects: &Projects, project: &str) -> bool {
        let Some(user) = self.account.user() else { return false };
        let Some(p) = projects.get(project) else { return false };
        let Some(c) = p.library(&user.id) else { return false };
        c.latest.is_some() && (p.stub() || (c.behind() && !c.dirty && !c.held && p.draft.is_none()))
    }

    /// Download `project` from the library, to open it.
    pub fn download(&mut self, projects: &Projects, project: &str, reload: bool) {
        let Some(user) = self.account.user() else { return };
        let Some(c) = projects.get(project).and_then(|p| p.library(&user.id)) else { return };
        if self.waiting.values().any(|w| matches!(w, Want::Download { project: p, .. } if p == project)) {
            return;
        }
        let path = format!("/api/projects/{}", c.id);
        let from = projects.current.clone();
        self.send(Want::Download { project: project.into(), reload, from }, "GET", path, Vec::new(), Vec::new());
    }

    /// The page got `reply` for call `id`: what the editor should do about it.
    pub fn answer(&mut self, id: u32, reply: Reply, projects: &mut Projects) -> Vec<Event> {
        let Some(want) = self.waiting.remove(&id) else { return Vec::new() };
        let ok = (200..300).contains(&reply.status);
        let mut events = Vec::new();
        match want {
            Want::Me => {
                let me = if ok { serde_json::from_slice::<Me>(&reply.body).ok() } else { None };
                let account = match me {
                    // A Worker with no sign-in, or none at all (a plain static server).
                    _ if reply.status == 404 => Account::Off,
                    Some(me) if !me.sign_in => Account::Off,
                    Some(Me { user: None, .. }) => Account::Out,
                    Some(Me { user: Some(u), usage, limits, .. }) => Account::In(User {
                        id: u.id,
                        email: u.email,
                        usage,
                        limit: limits.account,
                        file_limit: limits.file,
                    }),
                    // Offline, or the Worker failed: nothing is known that was not before.
                    None => return events,
                };
                let user = account.user().map(|u| u.id.clone());
                if account != self.account {
                    projects.signed_out(user.as_deref());
                    self.account = account;
                    events.push(Event::Listed);
                }
                if user.is_some() {
                    self.list();
                }
            }
            Want::List { generation } => match (ok, serde_json::from_slice::<List>(&reply.body)) {
                (true, Ok(list)) => {
                    let Account::In(user) = &mut self.account else { return events };
                    user.usage = list.usage;
                    let user = user.id.clone();
                    // Uploaded after this list was asked for: it may not show them yet.
                    self.uploaded.retain(|_, before| *before >= generation);
                    let fresh: Vec<String> = self.uploaded.keys().cloned().collect();
                    let merged = projects.merge_library(&user, &list.projects, &fresh);
                    for project in merged.thumbs {
                        let Some(c) = projects.get(&project).and_then(|p| p.library(&user)) else { continue };
                        let path = format!("/api/projects/{}/thumb", c.id);
                        self.send(Want::GetThumb { project }, "GET", path, Vec::new(), Vec::new());
                    }
                    for (project, name) in merged.kept {
                        events.push(Event::Renamed { project, name: name.clone() });
                        events.push(Event::Toast(format!(
                            "Your library has a newer version: kept this device's edits as {name}"
                        )));
                    }
                    events.push(Event::Listed);
                    if std::mem::take(&mut self.relist) {
                        self.list();
                    }
                }
                _ if reply.status == 401 => self.signed_out(projects, &mut events),
                _ => self.relist = false,
            },
            Want::Upload { project, thumb } => {
                let next = self.uploading.remove(&project).flatten();
                let name = projects.get(&project).map(|p| p.name.clone()).unwrap_or_default();
                match reply.status {
                    200 | 201 => {
                        if let Ok(Put { project: r }) = serde_json::from_slice(&reply.body) {
                            let cloud_id = r.id.clone();
                            self.uploaded.insert(cloud_id.clone(), self.lists);
                            projects.update(&project, |p| {
                                if let Some(c) = &mut p.cloud {
                                    c.version = Some(r.version.clone());
                                    c.latest = Some(r.version);
                                    c.dirty = next.is_some();
                                    c.held = false;
                                }
                            });
                            if let Some(png) = thumb.filter(|_| next.is_none()) {
                                self.put_thumb(&cloud_id, png);
                            }
                            if next.is_none() {
                                events.push(Event::Toast(format!("Saved {name} to your library")));
                            }
                        }
                        self.list();
                    }
                    409 => {
                        if let Ok(Conflict { copy }) = serde_json::from_slice(&reply.body) {
                            let user = self.account.user().map(|u| u.id.clone()).unwrap_or_default();
                            self.uploaded.insert(copy.id.clone(), self.lists);
                            projects.update(&project, |p| {
                                p.name = copy.name.clone();
                                p.cloud = Some(Cloud {
                                    user,
                                    id: copy.id.clone(),
                                    version: Some(copy.version.clone()),
                                    latest: Some(copy.version.clone()),
                                    dirty: next.is_some(),
                                    held: false,
                                });
                            });
                            if let Some(png) = thumb.filter(|_| next.is_none()) {
                                self.put_thumb(&copy.id, png);
                            }
                            events.push(Event::Renamed { project: project.clone(), name: copy.name.clone() });
                            events.push(Event::Toast(format!(
                                "{name} changed on another device: kept both, this one as {}",
                                copy.name
                            )));
                        }
                        self.list();
                    }
                    401 => {
                        self.signed_out(projects, &mut events);
                        return events;
                    }
                    400 | 413 => {
                        // Only this device has it now: it waits for a save the library takes,
                        // and nothing downloads over it.
                        projects.update(&project, |p| {
                            if let Some(c) = &mut p.cloud {
                                c.dirty = false;
                                c.held = true;
                            }
                        });
                        let problem = serde_json::from_slice::<Problem>(&reply.body).ok();
                        let why = match problem.as_ref().map(|p| (p.error.as_str(), p.limit)) {
                            Some(("file_too_large", limit)) => {
                                format!("it is over the {} a file the library takes", size(limit))
                            }
                            Some(("quota", limit)) => format!("the library is full ({})", size(limit)),
                            _ => "the library did not take it as a .vox file".into(),
                        };
                        events.push(Event::Toast(format!("Kept {name} on this device: {why}")));
                        return events;
                    }
                    _ => {
                        events.push(Event::Toast(format!(
                            "Kept {name} on this device: the library could not be reached. It goes up with the next save."
                        )));
                        return events;
                    }
                }
                if let Some((vox, thumb)) = next {
                    self.upload(projects, &project, vox, thumb);
                }
            }
            Want::Download { project, reload, from } => {
                // Chosen in the list while another scene was open: once a different one is,
                // the download is no longer wanted.
                if !reload && projects.current != from && reply.status != 401 {
                    return events;
                }
                let name = projects.get(&project).map(|p| p.name.clone()).unwrap_or_default();
                match (reply.status, reply.version) {
                    (200, Some(version)) => events.push(Event::Open { project, bytes: reply.body, version, reload }),
                    (401, _) => self.signed_out(projects, &mut events),
                    _ if reload => {}
                    (status, _) => {
                        let here = projects.get(&project).is_some_and(|p| p.has_file());
                        if here {
                            events.push(Event::OpenHere { project });
                        } else if status == 404 {
                            events.push(Event::Toast(format!("{name} is no longer in your library")));
                            self.list();
                        } else {
                            events.push(Event::Toast(format!(
                                "Could not download {name}: the library could not be reached"
                            )));
                        }
                    }
                }
            }
            Want::GetThumb { project } => {
                if reply.status == 200 && !reply.body.is_empty() {
                    projects.set_thumb(&project, &reply.body);
                    events.push(Event::Listed);
                }
            }
            Want::PutThumb => {}
        }
        events
    }

    fn put_thumb(&mut self, cloud_id: &str, png: Vec<u8>) {
        let headers = vec![("content-type", "image/png".into())];
        self.send(Want::PutThumb, "PUT", format!("/api/projects/{cloud_id}/thumb"), headers, png);
    }

    /// The session ended (signed out on another tab, or it expired).
    fn signed_out(&mut self, projects: &mut Projects, events: &mut Vec<Event>) {
        if matches!(self.account, Account::In(_)) {
            self.account = Account::Out;
            projects.signed_out(None);
            self.uploading.clear();
            self.uploaded.clear();
            self.relist = false;
            events.push(Event::Listed);
            events.push(Event::Toast("Signed out: saves stay on this device until you sign in again".into()));
        }
    }
}

/// `name` percent-encoded as `encodeURIComponent` does, for a header.
fn encode(name: &str) -> String {
    let mut out = String::new();
    for b in name.bytes() {
        if b.is_ascii_alphanumeric() || b"-_.!~*'()".contains(&b) {
            out.push(b as char);
        } else {
            out.push_str(&format!("%{b:02X}"));
        }
    }
    out
}

fn size(n: u64) -> String {
    crate::projects::bytes(n)
}

/// A library in memory that answers [`Call`]s as the Worker does: one account, the version
/// check, conflicts kept as copies, and the limits. For the tests and the headless check.
#[derive(Default)]
pub struct FakeLibrary {
    /// None: nobody is signed in.
    pub user: Option<String>,
    /// Every project: id, name, version, bytes, thumbnail, updated (ms).
    pub projects: Vec<Stored>,
    pub limit: u64,
    pub file_limit: u64,
    /// Answer every call with status 0, as when offline.
    pub offline: bool,
    versions: u32,
    pub clock: f64,
}

#[derive(Clone, Debug)]
pub struct Stored {
    pub id: String,
    pub name: String,
    pub version: String,
    pub bytes: Vec<u8>,
    pub thumb: Vec<u8>,
    pub updated: f64,
}

impl FakeLibrary {
    /// A library for `user`, signed in, with the free tier's limits.
    pub fn signed_in(user: &str) -> Self {
        Self { user: Some(user.into()), limit: 100 << 20, file_limit: 20 << 20, ..Self::default() }
    }

    fn usage(&self) -> u64 {
        self.projects.iter().map(|p| (p.bytes.len() + p.thumb.len()) as u64).sum()
    }

    fn show(p: &Stored) -> serde_json::Value {
        serde_json::json!({
            "id": p.id, "name": p.name, "version": p.version, "size": p.bytes.len(),
            "thumb": !p.thumb.is_empty(), "device": "", "created": p.updated, "updated": p.updated,
        })
    }

    fn json(status: u16, v: serde_json::Value) -> Reply {
        Reply { status, body: serde_json::to_vec(&v).unwrap_or_default(), version: None }
    }

    /// Store `bytes` as a new version (`id` None: a new project).
    pub fn put(&mut self, id: Option<&str>, name: &str, bytes: Vec<u8>) -> Stored {
        self.versions += 1;
        self.clock += 1000.0;
        let version = format!("v{}", self.versions);
        let at = id.and_then(|id| self.projects.iter().position(|p| p.id == id));
        let p = Stored {
            id: id.map_or_else(|| format!("c{}", self.versions), str::to_string),
            name: name.into(),
            version,
            bytes,
            thumb: at.map(|i| self.projects[i].thumb.clone()).unwrap_or_default(),
            updated: self.clock,
        };
        match at {
            Some(i) => self.projects[i] = p.clone(),
            None => self.projects.push(p.clone()),
        }
        p
    }

    /// The answer to `call`.
    pub fn answer(&mut self, call: &Call) -> Reply {
        if self.offline {
            return Reply::default();
        }
        let header = |k: &str| call.headers.iter().find(|(h, _)| *h == k).map(|(_, v)| v.as_str());
        if call.path == "/api/me" {
            let user = self.user.as_ref().map(|id| serde_json::json!({ "id": id, "email": null, "name": "" }));
            return Self::json(
                200,
                serde_json::json!({
                    "signIn": true, "email": false, "user": user, "usage": self.usage(),
                    "limits": { "account": self.limit, "file": self.file_limit },
                }),
            );
        }
        if self.user.is_none() {
            return Self::json(401, serde_json::json!({ "error": "sign_in" }));
        }
        let rest = call.path.strip_prefix("/api/projects").unwrap_or_default();
        if rest.is_empty() {
            let list: Vec<_> = self.projects.iter().map(Self::show).collect();
            return Self::json(200, serde_json::json!({ "projects": list, "usage": self.usage() }));
        }
        let rest = rest.trim_start_matches('/');
        let (id, thumb) = match rest.strip_suffix("/thumb") {
            Some(id) => (id, true),
            None => (rest, false),
        };
        let at = self.projects.iter().position(|p| p.id == id);
        match (call.method, thumb, at) {
            ("GET", false, Some(i)) => {
                let p = &self.projects[i];
                Reply { status: 200, body: p.bytes.clone(), version: Some(p.version.clone()) }
            }
            ("GET", true, Some(i)) if !self.projects[i].thumb.is_empty() => {
                Reply { status: 200, body: self.projects[i].thumb.clone(), version: None }
            }
            ("PUT", true, Some(i)) => {
                self.projects[i].thumb = call.body.clone();
                Reply { status: 204, ..Reply::default() }
            }
            ("PUT", false, _) => {
                let name = header("x-vxr-name").unwrap_or("Untitled").replace("%20", " ");
                let size = call.body.len() as u64;
                if size > self.file_limit {
                    return Self::json(413, serde_json::json!({ "error": "file_too_large", "limit": self.file_limit }));
                }
                if vxr_core::Scene::from_vox(&call.body).is_err() {
                    return Self::json(400, serde_json::json!({ "error": "not_vox" }));
                }
                let old = at.map_or(0, |i| self.projects[i].bytes.len() as u64);
                let current = at.map(|i| self.projects[i].clone());
                let fits = |s: &Self, replaced: u64| s.usage() - replaced + size <= s.limit;
                let matches = match (header("if-match"), header("if-none-match"), &current) {
                    (Some(v), _, Some(c)) => v.trim_matches('"') == c.version,
                    (None, Some("*"), None) => true,
                    _ => false,
                };
                if matches {
                    if !fits(self, old) {
                        return Self::json(413, serde_json::json!({ "error": "quota", "limit": self.limit }));
                    }
                    let p = self.put(Some(id), &name, call.body.clone());
                    return Self::json(
                        if current.is_some() { 200 } else { 201 },
                        serde_json::json!({ "project": Self::show(&p) }),
                    );
                }
                if current.as_ref().is_some_and(|c| c.bytes == call.body) {
                    return Self::json(200, serde_json::json!({ "project": Self::show(current.as_ref().unwrap()) }));
                }
                if !fits(self, 0) {
                    return Self::json(413, serde_json::json!({ "error": "quota", "limit": self.limit }));
                }
                let copy = self.put(None, &format!("{name} (conflicted copy)"), call.body.clone());
                Self::json(
                    409,
                    serde_json::json!({
                        "error": "conflict", "id": id, "copy": Self::show(&copy),
                        "current": current.as_ref().map(Self::show),
                    }),
                )
            }
            _ => Self::json(404, serde_json::json!({ "error": "not_found" })),
        }
    }

    /// Answer every call `sync` has queued, and every call those answers queue, into
    /// `projects`: the events, in order.
    pub fn serve(&mut self, sync: &mut Library, projects: &mut Projects) -> Vec<Event> {
        let mut events = Vec::new();
        loop {
            let calls = sync.take_out();
            if calls.is_empty() {
                return events;
            }
            for call in calls {
                let reply = self.answer(&call);
                events.extend(sync.answer(call.id, reply, projects));
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::projects::Memory;

    fn vox(n: u8) -> Vec<u8> {
        let mut scene = vxr_core::Scene::new([8, 8, 8]);
        scene.edit((0..n).map(|k| ([k as i32, 0, 0], 1)));
        crate::files::vox_bytes(&scene).unwrap()
    }

    fn device() -> (Memory, Projects, Library) {
        let m = Memory::default();
        m.clock.set(1_000_000.0);
        (m.clone(), Projects::load(Box::new(m)), Library::default())
    }

    /// A scene saved to the library on this device, as `host::run_action` does.
    fn save(projects: &mut Projects, sync: &mut Library, name: &str, bytes: Vec<u8>) -> String {
        let id = projects.ensure(name);
        projects.keep_copy(&id, &bytes).unwrap();
        projects.saved(name, None, false, bytes.len() as u64, 1);
        sync.upload(projects, &id, bytes, Some(b"png".to_vec()));
        id
    }

    #[test]
    fn signs_in_and_lists_the_library() {
        let (_, mut p, mut s) = device();
        let mut lib = FakeLibrary::signed_in("u1");
        let castle = lib.put(Some("c-castle"), "castle", vox(2));
        lib.projects[0].thumb = b"png".to_vec();
        s.check();
        let events = lib.serve(&mut s, &mut p);
        assert!(events.contains(&Event::Listed));
        let Account::In(user) = &s.account else { panic!("signed in") };
        assert_eq!((user.id.as_str(), user.usage), ("u1", castle.bytes.len() as u64 + 3));
        let row = p.get("c-castle").expect("the library's project is listed");
        assert!(row.stub() && row.name == "castle" && row.thumb == 3, "with its thumbnail fetched");
        assert!(s.wants_download(&p, "c-castle"));

        // Opening it downloads it.
        s.download(&p, "c-castle", false);
        let events = lib.serve(&mut s, &mut p);
        assert_eq!(
            events,
            vec![Event::Open { project: "c-castle".into(), bytes: vox(2), version: castle.version, reload: false }]
        );
    }

    #[test]
    fn signed_out_or_off_forgets_the_library_rows_only() {
        let (_, mut p, mut s) = device();
        let mut lib = FakeLibrary::signed_in("u1");
        lib.put(Some("c1"), "castle", vox(1));
        s.check();
        lib.serve(&mut s, &mut p);
        p.opened("mine.vox", None, Some("local"), None, false, 10);
        assert_eq!(p.list().len(), 2);
        lib.user = None;
        s.check();
        lib.serve(&mut s, &mut p);
        assert_eq!(s.account, Account::Out);
        assert_eq!(p.list().len(), 1, "the library's row went, the device's stayed");

        // A page with no Worker behind it (a static server) has no sign-in.
        let (_, mut p, mut s) = device();
        s.check();
        let call = s.take_out().remove(0);
        assert!(s.answer(call.id, Reply { status: 404, ..Reply::default() }, &mut p).is_empty());
        assert_eq!(s.account, Account::Off);
    }

    #[test]
    fn save_uploads_with_the_version_it_replaces() {
        let (_, mut p, mut s) = device();
        let mut lib = FakeLibrary::signed_in("u1");
        s.check();
        lib.serve(&mut s, &mut p);
        let id = save(&mut p, &mut s, "tower.vox", vox(1));
        let call = s.take_out().remove(0);
        assert_eq!(call.headers[0], ("if-none-match", "*".into()), "a new project");
        assert_eq!(call.headers[1], ("x-vxr-name", "tower".into()));
        let reply = lib.answer(&call);
        let events = s.answer(call.id, reply, &mut p);
        assert_eq!(events, vec![Event::Toast("Saved tower.vox to your library".into())]);
        lib.serve(&mut s, &mut p);
        let c = p.get(&id).unwrap().cloud.clone().unwrap();
        assert_eq!((c.version.as_deref(), c.dirty), (Some("v1"), false));
        assert_eq!(lib.projects[0].thumb, b"png", "the thumbnail went up after the file");

        // The next save replaces v1.
        save(&mut p, &mut s, "tower.vox", vox(2));
        let call = s.take_out().remove(0);
        assert_eq!(call.headers[0], ("if-match", "\"v1\"".into()));
        s.out.push(call);
        lib.serve(&mut s, &mut p);
        assert_eq!(lib.projects.len(), 1);
        assert_eq!(lib.projects[0].bytes, vox(2));
    }

    #[test]
    fn a_save_during_an_upload_goes_after_it() {
        let (_, mut p, mut s) = device();
        let mut lib = FakeLibrary::signed_in("u1");
        s.check();
        lib.serve(&mut s, &mut p);
        let id = save(&mut p, &mut s, "a.vox", vox(1));
        let first = s.take_out();
        save(&mut p, &mut s, "a.vox", vox(3));
        assert!(s.take_out().is_empty(), "one upload in flight per project");
        s.out = first;
        lib.serve(&mut s, &mut p);
        assert_eq!(lib.projects[0].bytes, vox(3));
        assert!(!p.get(&id).unwrap().cloud.as_ref().unwrap().dirty);
    }

    #[test]
    fn a_conflict_keeps_both_copies() {
        let (_, mut p, mut s) = device();
        let mut lib = FakeLibrary::signed_in("u1");
        s.check();
        lib.serve(&mut s, &mut p);
        let id = save(&mut p, &mut s, "island.vox", vox(1));
        lib.serve(&mut s, &mut p);
        // Another device saves first.
        lib.put(Some(&id), "island", vox(2));
        save(&mut p, &mut s, "island.vox", vox(4));
        let events = lib.serve(&mut s, &mut p);
        assert!(events.contains(&Event::Renamed { project: id.clone(), name: "island (conflicted copy)".into() }));
        assert_eq!(lib.projects.len(), 2);
        assert_eq!(lib.projects[0].bytes, vox(2), "the other device's save stays");
        assert_eq!(lib.projects[1].bytes, vox(4), "this one is the copy");
        let c = p.get(&id).unwrap().cloud.clone().unwrap();
        assert_eq!(c.id, lib.projects[1].id, "this device's project is the copy now");
        // The other device's version shows in the list as its own project, to download.
        let other = p.list().iter().find(|r| r.cloud.as_ref().is_some_and(|c| c.id == id && c.user == "u1"));
        assert!(other.is_some_and(|r| r.stub()));
    }

    #[test]
    fn a_newer_version_elsewhere_is_noted() {
        let (_, mut p, mut s) = device();
        let mut lib = FakeLibrary::signed_in("u1");
        s.check();
        lib.serve(&mut s, &mut p);
        let id = save(&mut p, &mut s, "a.vox", vox(1));
        lib.serve(&mut s, &mut p);
        assert!(!s.wants_download(&p, &id));
        lib.put(Some(&id), "a", vox(5));
        s.check();
        lib.serve(&mut s, &mut p);
        assert!(p.get(&id).unwrap().cloud.as_ref().unwrap().behind());
        assert!(s.wants_download(&p, &id), "opened again, it takes the newer version");
    }

    #[test]
    fn limits_and_failures_keep_the_save_here() {
        let (_, mut p, mut s) = device();
        let mut lib = FakeLibrary::signed_in("u1");
        lib.file_limit = 10;
        s.check();
        lib.serve(&mut s, &mut p);
        let id = save(&mut p, &mut s, "big.vox", vox(9));
        let events = lib.serve(&mut s, &mut p);
        assert!(matches!(&events[0], Event::Toast(t) if t.contains("over the 1 KB a file")), "{events:?}");
        let pr = p.get(&id).unwrap();
        let c = pr.cloud.as_ref().unwrap();
        assert!(pr.has_file() && c.held && !c.dirty, "kept here only, and not sent again by itself");
        assert!(!s.busy());

        lib.file_limit = 20 << 20;
        lib.offline = true;
        save(&mut p, &mut s, "big.vox", vox(2));
        let events = lib.serve(&mut s, &mut p);
        assert!(matches!(&events[0], Event::Toast(t) if t.contains("could not be reached")));
        assert!(p.get(&id).unwrap().cloud.as_ref().unwrap().dirty, "it goes up with the next save");
        lib.offline = false;
        save(&mut p, &mut s, "big.vox", vox(2));
        lib.serve(&mut s, &mut p);
        assert_eq!(lib.projects[0].bytes, vox(2));
        let c = p.get(&id).unwrap().cloud.clone().unwrap();
        assert!(!c.held && !c.dirty, "the save the library took clears it");
    }

    #[test]
    fn a_refused_save_is_kept_when_another_device_saves() {
        let (m, mut p, mut s) = device();
        let mut lib = FakeLibrary::signed_in("u1");
        s.check();
        lib.serve(&mut s, &mut p);
        let id = save(&mut p, &mut s, "a.vox", vox(1));
        lib.serve(&mut s, &mut p);
        lib.file_limit = vox(1).len() as u64;
        save(&mut p, &mut s, "a.vox", vox(9));
        lib.serve(&mut s, &mut p);
        assert!(!s.wants_download(&p, &id), "nothing downloads over the refused save");
        lib.file_limit = 20 << 20;
        lib.put(Some(&id), "a", vox(5));
        s.check();
        let events = lib.serve(&mut s, &mut p);

        // This device's edits are their own project now, out of the library.
        let mine = p.get(&id).unwrap();
        assert_eq!(mine.name, "a (conflicted copy)");
        assert!(mine.cloud.is_none() && mine.has_file());
        assert_eq!(m.files.borrow().get(&format!("files/{id}")), Some(&vox(9)), "with the bytes the library refused");
        assert!(events.contains(&Event::Renamed { project: id.clone(), name: "a (conflicted copy)".into() }));
        assert!(events.iter().any(|e| matches!(e, Event::Toast(t) if t.contains("kept this device's edits"))));
        // The library's version is listed beside it, to download.
        let theirs = p.list().iter().find(|r| r.library("u1").is_some_and(|c| c.id == id)).map(|r| r.id.clone());
        let theirs = theirs.expect("the library's version is listed");
        assert!(p.get(&theirs).unwrap().stub() && s.wants_download(&p, &theirs));
        assert_eq!(lib.projects.len(), 1, "the library holds only its own version");
    }

    #[test]
    fn a_download_for_a_scene_no_longer_open_is_dropped() {
        let (_, mut p, mut s) = device();
        let mut lib = FakeLibrary::signed_in("u1");
        lib.put(Some("c1"), "castle", vox(2));
        s.check();
        lib.serve(&mut s, &mut p);
        p.opened("here.vox", None, Some("here"), None, false, 10);
        s.download(&p, "c1", false);
        // Another scene opens while the download runs.
        p.opened("other.vox", None, Some("other"), None, false, 10);
        assert!(lib.serve(&mut s, &mut p).is_empty(), "the scene now open stays");

        s.download(&p, "c1", false);
        assert!(matches!(&lib.serve(&mut s, &mut p)[..], [Event::Open { .. }]), "while it stays open, it opens");
    }

    #[test]
    fn a_list_asked_for_before_an_upload_leaves_it_in_the_library() {
        let (_, mut p, mut s) = device();
        let mut lib = FakeLibrary::signed_in("u1");
        s.check();
        lib.serve(&mut s, &mut p);
        s.list();
        let call = s.take_out().remove(0);
        let stale = lib.answer(&call);
        let id = save(&mut p, &mut s, "new.vox", vox(1));
        lib.serve(&mut s, &mut p);
        assert_eq!(lib.projects.len(), 1);
        // The list from before the upload answers last, without the project.
        s.answer(call.id, stale, &mut p);
        let c = p.get(&id).unwrap().cloud.clone().expect("still in the library");
        assert_eq!((c.version.as_deref(), c.latest.as_deref()), (Some("v1"), Some("v1")));
        // The list asked for after the upload went once that one answered.
        lib.serve(&mut s, &mut p);
        assert!(p.get(&id).unwrap().cloud.is_some());

        // Once a list asked for after the upload leaves it out, it was deleted elsewhere.
        lib.projects.clear();
        s.list();
        lib.serve(&mut s, &mut p);
        assert!(p.get(&id).unwrap().cloud.is_none());
    }

    #[test]
    fn a_project_deleted_elsewhere_lets_go() {
        let (_, mut p, mut s) = device();
        let mut lib = FakeLibrary::signed_in("u1");
        lib.put(Some("gone"), "gone", vox(1));
        s.check();
        lib.serve(&mut s, &mut p);
        let id = save(&mut p, &mut s, "kept.vox", vox(1));
        lib.serve(&mut s, &mut p);
        lib.projects.clear();
        s.check();
        lib.serve(&mut s, &mut p);
        assert!(p.get("gone").is_none(), "a row with nothing here goes");
        assert!(p.get(&id).is_some_and(|r| r.cloud.is_none() && r.has_file()), "a project with a copy here stays");
    }

    #[test]
    fn encodes_names_like_the_page() {
        assert_eq!(encode("Island 2"), "Island%202");
        assert_eq!(encode("café"), "caf%C3%A9");
        assert_eq!(encode("a-b_c.(1)"), "a-b_c.(1)");
    }
}
