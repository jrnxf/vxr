//! The optional account and its library of projects, in the browser and the desktop app.
//!
//! Signed in, the editor's projects go to the account's library on the vxr Worker
//! (`crates/vxr-web/worker`): Save uploads the scene there, the projects list shows the
//! library's projects too, and opening one this device does not have downloads it. Each save
//! names the version it replaces; when another device saved first, the Worker keeps both, this
//! save as "<name> (conflicted copy)" ([`Event::Renamed`]).
//!
//! [`Library`] has no network of its own: it queues [`Call`]s, the host sends them
//! ([`Library::take_out`]) and hands each [`Reply`] back to [`Library::answer`], which says what the
//! editor should do ([`Event`]). The page sends them same-origin with the session cookie; the
//! desktop app (`crate::courier`) over HTTPS with the session as a bearer token.
//! [`FakeLibrary`] answers the calls in memory, as the Worker does, for the tests and the headless
//! check.
//!
//! Signing in and out goes through the same calls, from the editor's account popover. On the page
//! one email carries a link and a 6-digit code ([`Library::send_code`], behind the page's Turnstile
//! check), the code signs in here ([`Library::verify_code`]), and the link signs in wherever it is
//! opened, which the page hears of and checks `/api/me` again ([`Library::check`]). The desktop
//! app ([`Library::native`]) has no page for Turnstile or a link: its calls say they are the app,
//! its email has only the code, and the Worker hands it the session token, which the host keeps in
//! the Keychain ([`Event::Session`]).
//!
//! The popover also exports the whole library as one zip of `.vox` files ([`Library::export`],
//! [`Event::Export`]) and deletes the account with everything in its library
//! ([`Library::delete_account`]); the scenes on this device stay.

use std::collections::HashMap;

use serde::Deserialize;

use crate::projects::{Cloud, Projects};

/// Where the account is.
#[derive(Clone, Debug, Default, PartialEq)]
pub enum Account {
    /// No sign-in here (a page whose Worker has none), or not known yet: the Worker has not been
    /// reached.
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
    /// The account's email address (None if the Worker sends none).
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

/// A request for the host to send: same-origin with the session cookie on the page, to the
/// Worker with the bearer token in the desktop app.
#[derive(Clone, Debug, PartialEq)]
pub struct Call {
    pub id: u32,
    pub method: &'static str,
    pub path: String,
    pub headers: Vec<(&'static str, String)>,
    pub body: Vec<u8>,
}

/// What came back: `status` 0 when the request did not get through. `version` is the
/// `x-vxr-version` header of a download, `token` the `set-auth-token` header of a sign-in (which
/// the Worker sends only to the desktop app).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Reply {
    pub status: u16,
    pub body: Vec<u8>,
    pub version: Option<String>,
    pub token: Option<String>,
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
    /// How a sign-in step went.
    Auth(Auth),
    /// The desktop app's session token to keep (signed in) or to delete (signed out).
    Session(Option<String>),
    /// The whole library as a zip of `count` `.vox` files, to save as `name`.
    Export {
        name: String,
        bytes: Vec<u8>,
        count: usize,
    },
}

/// How asking for a sign-in email, or typing its code, went.
#[derive(Clone, Debug, PartialEq)]
pub enum Auth {
    /// The email with the link and the code is on its way.
    Sent,
    /// The code does not match the newest email's.
    WrongCode,
    /// Anything else, in words: the code expired, the check did not pass, too many tries.
    Failed(String),
}

/// What a call in flight is for.
enum Want {
    /// `confirm`: asked right after a code was accepted, to learn who signed in. `session` is
    /// the desktop app's token it went with: an answer for an older one is stale.
    Me {
        confirm: bool,
        session: Option<String>,
    },
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
    Code,
    Verify,
    SignOut,
    /// The list an export downloads, then each of its files.
    ExportList,
    ExportFile {
        name: String,
    },
    DeleteAccount,
}

/// An export under way: the files downloaded so far and how many are still to come.
#[derive(Default)]
struct Export {
    files: Vec<crate::files::File>,
    left: usize,
    failed: bool,
}

/// The name an exported library downloads as.
pub const EXPORT_NAME: &str = "vxr-library.zip";

/// A save waiting for the one before it to finish.
type Queued = Option<(Vec<u8>, Option<Vec<u8>>)>;

#[derive(Default)]
pub struct Library {
    pub account: Account,
    /// The desktop app: no Turnstile, code only, and `session` as a bearer token.
    pub native: bool,
    session: Option<String>,
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
    export: Option<Export>,
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
    #[serde(default)]
    usage: Option<u64>,
}

/// What went wrong with a sign-in call: a Worker problem (`{error}`) or a Better Auth error
/// (`{code, message}`).
#[derive(Default, Deserialize)]
struct AuthProblem {
    #[serde(default)]
    error: Option<String>,
    #[serde(default)]
    code: Option<String>,
}

impl Library {
    /// The desktop app's library, signed in with the `session` token it kept, if any.
    pub fn native(session: Option<String>) -> Self {
        Self { native: true, session, ..Self::default() }
    }

    /// Ask the Worker who is signed in (at launch, and when the page or the window comes back
    /// into view).
    pub fn check(&mut self) {
        if !self.waiting.values().any(|w| matches!(w, Want::Me { .. })) {
            self.me(false);
        }
    }

    fn me(&mut self, confirm: bool) {
        let session = self.session.clone();
        self.send(Want::Me { confirm, session }, "GET", "/api/me".into(), Vec::new(), Vec::new());
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
        mut headers: Vec<(&'static str, String)>,
        body: Vec<u8>,
    ) {
        if self.native {
            headers.push(("x-vxr-client", "app".into()));
            if let Some(token) = &self.session {
                headers.push(("authorization", format!("Bearer {token}")));
            }
        }
        self.next += 1;
        self.out.push(Call { id: self.next, method, path, headers, body });
        self.waiting.insert(self.next, want);
    }

    /// Whether a sign-in call is in flight.
    pub fn signing_in(&self) -> bool {
        self.waiting
            .values()
            .any(|w| matches!(w, Want::Code | Want::Verify | Want::SignOut | Want::Me { confirm: true, .. }))
    }

    /// Whether the library is moving a scene: an upload, a download or a list in flight.
    pub fn syncing(&self) -> bool {
        !self.uploading.is_empty()
            || self.waiting.values().any(|w| matches!(w, Want::List { .. } | Want::Download { .. }))
    }

    /// Email `email` a sign-in link and code. `token` is the page's Turnstile answer, which the
    /// Worker checks before it sends anything; the desktop app has none, and its email has only
    /// the code.
    pub fn send_code(&mut self, email: &str, token: Option<&str>) {
        let body = if self.native {
            serde_json::json!({ "email": email })
        } else {
            serde_json::json!({
                "email": email,
                "callbackURL": "/account?from=editor",
                "errorCallbackURL": "/account?from=editor&link=expired",
            })
        };
        let mut headers = vec![("content-type", "application/json".into())];
        if let Some(token) = token {
            headers.push(("x-turnstile-token", token.into()));
        }
        let body = serde_json::to_vec(&body).unwrap_or_default();
        self.send(Want::Code, "POST", "/api/auth/sign-in/magic-link".into(), headers, body);
    }

    /// Sign in as `email` with the code from its newest email.
    pub fn verify_code(&mut self, email: &str, code: &str) {
        let body = serde_json::to_vec(&serde_json::json!({ "email": email, "otp": code })).unwrap_or_default();
        let headers = vec![("content-type", "application/json".into())];
        self.send(Want::Verify, "POST", "/api/auth/sign-in/email-otp".into(), headers, body);
    }

    /// End the session. The scenes stay on this device.
    pub fn sign_out(&mut self) {
        let headers = vec![("content-type", "application/json".into())];
        self.send(Want::SignOut, "POST", "/api/auth/sign-out".into(), headers, b"{}".to_vec());
    }

    /// Download every project in the library, and hand them over as one zip
    /// ([`Event::Export`]).
    pub fn export(&mut self) {
        if self.account.user().is_none() || self.export.is_some() {
            return;
        }
        self.export = Some(Export::default());
        self.send(Want::ExportList, "GET", "/api/projects".into(), Vec::new(), Vec::new());
    }

    /// Whether an export is under way.
    pub fn exporting(&self) -> bool {
        self.export.is_some()
    }

    /// Delete the account and everything in its library, confirmed with its `email` typed
    /// again. The scenes on this device stay.
    pub fn delete_account(&mut self, email: &str) {
        if self.account.user().is_none() || self.deleting() {
            return;
        }
        let body = serde_json::to_vec(&serde_json::json!({ "email": email.trim() })).unwrap_or_default();
        let headers = vec![("content-type", "application/json".into())];
        self.send(Want::DeleteAccount, "DELETE", "/api/account".into(), headers, body);
    }

    /// Whether deleting the account is in flight.
    pub fn deleting(&self) -> bool {
        self.waiting.values().any(|w| matches!(w, Want::DeleteAccount))
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
            // Asked with a token signed out (or replaced) since: whatever it says is old.
            Want::Me { session, .. } if session != self.session => {}
            Want::Me { confirm, .. } => {
                let me = if ok { serde_json::from_slice::<Me>(&reply.body).ok() } else { None };
                if confirm && !me.as_ref().is_some_and(|m| m.user.is_some()) {
                    events.push(Event::Auth(Auth::Failed(
                        "The code was right, but vxr could not be reached after. Try again.".into(),
                    )));
                }
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
                if account == Account::Out {
                    // The kept token no longer signs in: it expired or was signed out elsewhere.
                    self.end_session(&mut events);
                }
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
                        let why = match problem.as_ref().map(|p| (p.error.as_str(), p.limit, p.usage)) {
                            Some(("file_too_large", limit, _)) => {
                                format!("it is over the {} a file the library takes", size(limit))
                            }
                            Some(("quota", limit, used)) => {
                                if let (Account::In(user), Some(used)) = (&mut self.account, used) {
                                    user.usage = used;
                                }
                                let used = used.map(|u| format!("{} of {} used", size(u), size(limit)));
                                format!(
                                    "the library is full ({}). Delete scenes from it to make room.",
                                    used.unwrap_or_else(|| size(limit))
                                )
                            }
                            _ => "the library did not take it as a .vox file".into(),
                        };
                        events.push(Event::Toast(format!("Kept {name} on this device: {why}")));
                        // The usage line shows the library's own count.
                        events.push(Event::Listed);
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
            Want::Code if ok => events.push(Event::Auth(Auth::Sent)),
            Want::Code => events.push(Event::Auth(Auth::Failed(auth_reason(&reply)))),
            // Signed in: who as, and the library, come with the next /api/me, which the desktop
            // app asks with its new token.
            Want::Verify if ok && self.native => match reply.token.filter(|t| !t.is_empty()) {
                Some(token) => {
                    self.session = Some(token.clone());
                    events.push(Event::Session(Some(token)));
                    self.me(true);
                }
                None => events.push(Event::Auth(Auth::Failed(
                    "The code was right, but vxr sent no session back. Try again.".into(),
                ))),
            },
            Want::Verify if ok => self.me(true),
            Want::Verify => events.push(Event::Auth(match auth_code(&reply).as_deref() {
                Some("INVALID_OTP") => Auth::WrongCode,
                _ => Auth::Failed(auth_reason(&reply)),
            })),
            Want::SignOut if ok => {
                if matches!(self.account, Account::In(_)) {
                    self.forget(projects, &mut events);
                    events.push(Event::Toast("Signed out. Your scenes stay on this device.".into()));
                }
            }
            // The session was already gone on the Worker.
            Want::SignOut if reply.status == 401 => {
                if matches!(self.account, Account::In(_)) {
                    self.forget(projects, &mut events);
                    events.push(Event::Toast("Signed out. Your scenes stay on this device.".into()));
                }
            }
            // Offline, or the Worker failed: the desktop app still lets its token go, so a Mac
            // with no network can be signed out. The page cannot drop its cookie, so it waits.
            Want::SignOut if self.native && (reply.status == 0 || reply.status >= 500) => {
                if matches!(self.account, Account::In(_)) {
                    self.forget(projects, &mut events);
                    events.push(Event::Toast(
                        "Signed out on this device. vxr could not be reached, so the session ends there when it expires."
                            .into(),
                    ));
                }
            }
            Want::SignOut => {
                events.push(Event::Toast("Could not sign out: the library could not be reached. Try again.".into()))
            }
            Want::ExportList => match (ok, serde_json::from_slice::<List>(&reply.body)) {
                (true, Ok(list)) if list.projects.is_empty() => {
                    self.export = None;
                    events.push(Event::Toast("Your library is empty: there is nothing to export".into()));
                }
                (true, Ok(list)) => {
                    let names = zip_names(list.projects.iter().map(|p| p.name.as_str()));
                    if let Some(export) = &mut self.export {
                        export.left = list.projects.len();
                    }
                    for (p, name) in list.projects.iter().zip(names) {
                        let path = format!("/api/projects/{}", p.id);
                        self.send(Want::ExportFile { name }, "GET", path, Vec::new(), Vec::new());
                    }
                }
                _ if reply.status == 401 => {
                    self.export = None;
                    self.signed_out(projects, &mut events);
                }
                _ => {
                    self.export = None;
                    events.push(Event::Toast(
                        "Could not export your library: it could not be reached. Try again.".into(),
                    ));
                }
            },
            Want::ExportFile { name } => {
                if reply.status == 401 {
                    self.export = None;
                    self.signed_out(projects, &mut events);
                    return events;
                }
                let Some(export) = &mut self.export else { return events };
                match reply.status {
                    200 => export.files.push((name, reply.body)),
                    // Deleted from the library since it was listed.
                    404 => {}
                    _ => export.failed = true,
                }
                export.left -= 1;
                if export.left == 0 {
                    let export = self.export.take().unwrap_or_default();
                    if export.failed {
                        events.push(Event::Toast(
                            "Could not export your library: a file could not be downloaded. Try again.".into(),
                        ));
                    } else if export.files.is_empty() {
                        events.push(Event::Toast("Your library is empty: there is nothing to export".into()));
                    } else {
                        let count = export.files.len();
                        let bytes = crate::files::zip(&export.files);
                        events.push(Event::Export { name: EXPORT_NAME.into(), bytes, count });
                    }
                }
            }
            Want::DeleteAccount if ok => {
                let user = match &self.account {
                    Account::In(u) => Some(u.id.clone()),
                    _ => None,
                };
                self.forget(projects, &mut events);
                if let Some(user) = user {
                    projects.leave_library(&user);
                }
                events.push(Event::Toast(
                    "Deleted your account and its library. Your scenes stay on this device.".into(),
                ));
            }
            Want::DeleteAccount if reply.status == 401 => self.signed_out(projects, &mut events),
            Want::DeleteAccount => events.push(Event::Toast(
                match serde_json::from_slice::<Problem>(&reply.body).ok().map(|p| p.error).as_deref() {
                    Some("confirm") => "Could not delete your account: the email did not match it.",
                    _ => "Could not delete your account: vxr could not be reached. Try again.",
                }
                .into(),
            )),
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
            self.forget(projects, events);
            events.push(Event::Toast("Signed out: saves stay on this device until you sign in again".into()));
        }
    }

    /// Nobody is signed in now: the library's rows and uploads go, and the desktop app's token.
    fn forget(&mut self, projects: &mut Projects, events: &mut Vec<Event>) {
        self.end_session(events);
        self.export = None;
        self.account = Account::Out;
        projects.signed_out(None);
        self.uploading.clear();
        self.uploaded.clear();
        self.relist = false;
        events.push(Event::Listed);
    }

    /// The desktop app's token is no good any more: the host deletes it.
    fn end_session(&mut self, events: &mut Vec<Event>) {
        if self.session.take().is_some() {
            events.push(Event::Session(None));
        }
    }
}

/// The error code of a failed sign-in call, if it has one.
fn auth_code(reply: &Reply) -> Option<String> {
    let p = serde_json::from_slice::<AuthProblem>(&reply.body).unwrap_or_default();
    p.error.or(p.code)
}

/// What went wrong with a sign-in call, in words (as the account page says it).
fn auth_reason(reply: &Reply) -> String {
    match auth_code(reply).as_deref() {
        Some("challenge") => "The check did not pass. Try again.",
        Some("too_many_links") => "Too many sign-in emails asked for. Try again in an hour.",
        Some("email_off" | "sign_up_off" | "sign_in_off") => "Sign-in is off here.",
        Some("bad_email" | "INVALID_EMAIL") => "That email address does not look right.",
        Some("INVALID_OTP") => "That code doesn't match. Check the newest email.",
        Some("OTP_EXPIRED" | "TOO_MANY_ATTEMPTS") => "That code has expired or was tried too often. Send a new email.",
        _ if reply.status == 429 => "Too many tries. Wait a minute and try again.",
        _ if reply.status == 0 => "Could not reach vxr. Check your connection and try again.",
        _ => "Something went wrong. Try again.",
    }
    .into()
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

/// The file names `names` take in an exported zip: `.vox` on the end, no path separators, and
/// a number after a name already taken ("Castle.vox", "Castle (2).vox").
fn zip_names<'a>(names: impl Iterator<Item = &'a str>) -> Vec<String> {
    let mut taken = std::collections::HashSet::new();
    names
        .map(|name| {
            let clean: String =
                name.chars().map(|c| if matches!(c, '/' | '\\' | ':') || c.is_control() { '_' } else { c }).collect();
            let clean = clean.trim();
            let stem = match clean.len().checked_sub(4) {
                Some(at) if clean.is_char_boundary(at) && clean[at..].eq_ignore_ascii_case(".vox") => &clean[..at],
                _ => clean,
            }
            .trim();
            let stem = if stem.is_empty() || stem.chars().all(|c| c == '.') { "Untitled" } else { stem };
            let mut n = 1;
            loop {
                let name = if n == 1 { format!("{stem}.vox") } else { format!("{stem} ({n}).vox") };
                if taken.insert(name.to_lowercase()) {
                    return name;
                }
                n += 1;
            }
        })
        .collect()
}

fn size(n: u64) -> String {
    crate::projects::bytes(n)
}

/// A library in memory that answers [`Call`]s as the Worker does: one account, the version
/// check, conflicts kept as copies, and the limits. Signing in by code takes any code but
/// [`FakeLibrary::WRONG_CODE`], for any email a code was sent to. The desktop app's calls need no
/// Turnstile answer and are signed in only with the token its sign-in got. For the tests and the
/// headless check.
#[derive(Default)]
pub struct FakeLibrary {
    /// None: nobody is signed in.
    pub user: Option<String>,
    /// The signed-in user's email address.
    pub email: Option<String>,
    /// The addresses sign-in emails went to, in order.
    pub sent: Vec<String>,
    /// The desktop app's session token, once it signed in by code.
    pub session: Option<String>,
    /// Every project: id, name, version, bytes, thumbnail, updated (ms).
    pub projects: Vec<Stored>,
    pub limit: u64,
    pub file_limit: u64,
    /// Answer every call with status 0, as when offline.
    pub offline: bool,
    versions: u32,
    sessions: u32,
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
    /// The code that never matches.
    pub const WRONG_CODE: &str = "000000";

    /// A library for `user`, signed in, with the free tier's limits.
    pub fn signed_in(user: &str) -> Self {
        Self { user: Some(user.into()), limit: 100 << 20, file_limit: 20 << 20, ..Self::default() }
    }

    /// The same library with nobody signed in.
    pub fn signed_out() -> Self {
        Self { user: None, ..Self::signed_in("") }
    }

    /// A sign-in call's answer, if `call` is one.
    fn auth(&mut self, call: &Call) -> Option<Reply> {
        let body = serde_json::from_slice::<serde_json::Value>(&call.body).unwrap_or_default();
        let field = |k: &str| body.get(k).and_then(|v| v.as_str()).unwrap_or_default().to_string();
        let token = call.headers.iter().any(|(h, v)| *h == "x-turnstile-token" && !v.is_empty());
        let app = Self::app(call);
        Some(match call.path.as_str() {
            "/api/auth/sign-in/magic-link" if !token && !app => {
                Self::json(403, serde_json::json!({ "error": "challenge" }))
            }
            "/api/auth/sign-in/magic-link" => {
                self.sent.push(field("email"));
                Self::json(200, serde_json::json!({ "status": true }))
            }
            "/api/auth/sign-in/email-otp" => {
                let email = field("email");
                if field("otp") == Self::WRONG_CODE || !self.sent.contains(&email) {
                    return Some(Self::json(
                        400,
                        serde_json::json!({ "code": "INVALID_OTP", "message": "Invalid OTP" }),
                    ));
                }
                self.user = Some(format!("u-{email}"));
                self.email = Some(email);
                self.sessions += 1;
                let session = app.then(|| format!("s{}.sig", self.sessions));
                self.session = session.clone();
                Reply { token: session, ..Self::json(200, serde_json::json!({ "token": "t" })) }
            }
            "/api/auth/sign-out" => {
                if self.signed_in_for(call) {
                    self.user = None;
                    self.email = None;
                    self.session = None;
                }
                Self::json(200, serde_json::json!({ "success": true }))
            }
            _ => return None,
        })
    }

    /// Whether `call` comes from the desktop app.
    fn app(call: &Call) -> bool {
        call.headers.iter().any(|(h, v)| *h == "x-vxr-client" && v == "app")
    }

    /// Whether `call` is signed in: the desktop app's with its token, the page's always (its
    /// cookie is not modelled).
    fn signed_in_for(&self, call: &Call) -> bool {
        if !Self::app(call) {
            return self.user.is_some();
        }
        let bearer = call.headers.iter().find(|(h, _)| *h == "authorization").map(|(_, v)| v.as_str());
        self.user.is_some() && self.session.as_ref().is_some_and(|t| bearer == Some(&format!("Bearer {t}")))
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
        Reply { status, body: serde_json::to_vec(&v).unwrap_or_default(), ..Reply::default() }
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
            let user = (self.user.as_ref())
                .filter(|_| self.signed_in_for(call))
                .map(|id| serde_json::json!({ "id": id, "email": self.email, "name": "" }));
            return Self::json(
                200,
                serde_json::json!({
                    "signIn": true, "email": false, "user": user, "usage": self.usage(),
                    "limits": { "account": self.limit, "file": self.file_limit },
                }),
            );
        }
        if let Some(reply) = self.auth(call) {
            return reply;
        }
        if !self.signed_in_for(call) {
            return Self::json(401, serde_json::json!({ "error": "sign_in" }));
        }
        if call.path == "/api/account" {
            let body = serde_json::from_slice::<serde_json::Value>(&call.body).unwrap_or_default();
            let typed = body.get("email").and_then(|v| v.as_str()).unwrap_or_default().trim().to_lowercase();
            if call.method != "DELETE" || Some(typed) != self.email.as_ref().map(|e| e.to_lowercase()) {
                return Self::json(400, serde_json::json!({ "error": "confirm" }));
            }
            self.user = None;
            self.email = None;
            self.session = None;
            self.projects.clear();
            return Reply { status: 204, ..Reply::default() };
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
                Reply { status: 200, body: p.bytes.clone(), version: Some(p.version.clone()), token: None }
            }
            ("GET", true, Some(i)) if !self.projects[i].thumb.is_empty() => {
                Reply { status: 200, body: self.projects[i].thumb.clone(), ..Reply::default() }
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
                        return Self::json(
                            413,
                            serde_json::json!({ "error": "quota", "limit": self.limit, "usage": self.usage() }),
                        );
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
                    return Self::json(
                        413,
                        serde_json::json!({ "error": "quota", "limit": self.limit, "usage": self.usage() }),
                    );
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
    fn signs_in_with_a_code_and_out() {
        let (_, mut p, mut s) = device();
        let mut lib = FakeLibrary::signed_out();
        lib.put(Some("c1"), "castle", vox(1));
        s.check();
        lib.serve(&mut s, &mut p);
        assert_eq!(s.account, Account::Out);

        // No Turnstile answer, no email.
        s.send_code("ada@example.com", Some(""));
        assert_eq!(
            lib.serve(&mut s, &mut p),
            vec![Event::Auth(Auth::Failed("The check did not pass. Try again.".into()))]
        );
        s.send_code("ada@example.com", Some("token"));
        let call = s.out[0].clone();
        assert_eq!(call.path, "/api/auth/sign-in/magic-link");
        assert!(call.headers.contains(&("x-turnstile-token", "token".into())));
        assert!(s.signing_in());
        assert_eq!(lib.serve(&mut s, &mut p), vec![Event::Auth(Auth::Sent)]);
        assert_eq!(lib.sent, ["ada@example.com"]);

        s.verify_code("ada@example.com", FakeLibrary::WRONG_CODE);
        assert_eq!(lib.serve(&mut s, &mut p), vec![Event::Auth(Auth::WrongCode)]);
        assert_eq!(s.account, Account::Out);

        s.verify_code("ada@example.com", "123456");
        let events = lib.serve(&mut s, &mut p);
        assert!(events.contains(&Event::Listed));
        let user = s.account.user().expect("signed in");
        assert_eq!(user.email.as_deref(), Some("ada@example.com"));
        assert!(p.get("c1").is_some(), "the library is listed");
        assert!(!s.signing_in());

        s.sign_out();
        let events = lib.serve(&mut s, &mut p);
        assert_eq!(events, vec![Event::Listed, Event::Toast("Signed out. Your scenes stay on this device.".into())]);
        assert_eq!((s.account.clone(), lib.user.clone()), (Account::Out, None));
        assert!(p.get("c1").is_none(), "the library's rows went");
    }

    #[test]
    fn the_desktop_app_signs_in_by_code_and_keeps_a_bearer_token() {
        let (_, mut p, _) = device();
        let mut s = Library::native(None);
        let mut lib = FakeLibrary::signed_out();
        lib.put(Some("c1"), "castle", vox(1));
        let has = |c: &Call, h: &str| c.headers.iter().any(|(k, _)| *k == h);
        s.check();
        assert!(s.out[0].headers.contains(&("x-vxr-client", "app".into())));
        assert!(!has(&s.out[0], "authorization"));
        lib.serve(&mut s, &mut p);
        assert_eq!(s.account, Account::Out);

        // No Turnstile answer, and no link to come back to.
        s.send_code("ada@example.com", None);
        let call = s.out[0].clone();
        assert!(!has(&call, "x-turnstile-token"));
        assert!(!String::from_utf8_lossy(&call.body).contains("callbackURL"));
        assert_eq!(lib.serve(&mut s, &mut p), vec![Event::Auth(Auth::Sent)]);

        s.verify_code("ada@example.com", "123456");
        let events = lib.serve(&mut s, &mut p);
        let token = lib.session.clone().expect("the Worker made a session");
        assert_eq!(events[0], Event::Session(Some(token.clone())), "the host keeps the token");
        assert_eq!(s.account.user().and_then(|u| u.email.as_deref()), Some("ada@example.com"));
        assert!(p.get("c1").is_some(), "the library is listed with it");
        s.list();
        assert!(s.out[0].headers.contains(&("authorization", format!("Bearer {token}"))));
        lib.serve(&mut s, &mut p);

        // Launched again with the kept token, it is still signed in.
        let mut again = Library::native(Some(token.clone()));
        again.check();
        lib.serve(&mut again, &mut p);
        assert!(again.account.user().is_some());

        // Signing out ends the session on the Worker and deletes the token here.
        s.sign_out();
        let events = lib.serve(&mut s, &mut p);
        assert_eq!(events[0], Event::Session(None));
        assert!(events.contains(&Event::Toast("Signed out. Your scenes stay on this device.".into())));
        assert_eq!((lib.session.clone(), s.account.clone()), (None, Account::Out));
        again.check();
        assert_eq!(lib.serve(&mut again, &mut p), vec![Event::Session(None), Event::Listed]);
    }

    #[test]
    fn the_desktop_app_keeps_its_token_until_the_worker_says_it_is_gone() {
        let (_, mut p, _) = device();
        let mut lib = FakeLibrary::signed_in("u1");
        lib.session = Some("s1.sig".into());

        // Offline at launch: nothing is known, and the token stays.
        let mut s = Library::native(Some("s1.sig".into()));
        lib.offline = true;
        s.check();
        assert!(lib.serve(&mut s, &mut p).is_empty());
        assert_eq!(s.account, Account::Off);
        lib.offline = false;
        s.check();
        lib.serve(&mut s, &mut p);
        assert!(s.account.user().is_some());

        // Signed out elsewhere (or expired): the next call that needs it lets it go.
        lib.session = Some("s2.sig".into());
        s.list();
        let events = lib.serve(&mut s, &mut p);
        assert_eq!(events[0], Event::Session(None));
        assert_eq!(s.account, Account::Out);

        // A /api/me asked before a sign-in and answered after it is stale, not a sign-out.
        let mut s = Library::native(None);
        s.check();
        let stale = s.take_out().remove(0);
        lib.sent.push("ada@example.com".into());
        s.verify_code("ada@example.com", "123456");
        lib.serve(&mut s, &mut p);
        assert!(s.account.user().is_some());
        let reply = FakeLibrary::signed_out().answer(&stale);
        assert!(s.answer(stale.id, reply, &mut p).is_empty());
        assert!(s.account.user().is_some());
    }

    #[test]
    fn the_desktop_app_signs_out_here_when_the_worker_is_out_of_reach() {
        let (_, mut p, _) = device();
        let mut lib = FakeLibrary::signed_in("u1");
        lib.put(Some("c1"), "castle", vox(1));
        lib.session = Some("s1.sig".into());
        let mut s = Library::native(Some("s1.sig".into()));
        s.check();
        lib.serve(&mut s, &mut p);
        assert!(p.get("c1").is_some());

        // Offline: the token and the library's rows still go.
        lib.offline = true;
        s.sign_out();
        let events = lib.serve(&mut s, &mut p);
        assert_eq!(events[0], Event::Session(None), "the host deletes the token");
        assert!(events.iter().any(|e| matches!(e, Event::Toast(t) if t.contains("ends there when it expires"))));
        assert_eq!(s.account, Account::Out);
        assert!(p.get("c1").is_none(), "the library's rows went");
        assert_eq!(lib.session.as_deref(), Some("s1.sig"), "the Worker never heard");

        // The Worker failing does the same; another refusal keeps the session.
        let answer = |status: u16, p: &mut Projects| {
            let mut s = Library::native(Some("s1.sig".into()));
            s.account = Account::In(User { id: "u1".into(), email: None, usage: 0, limit: 0, file_limit: 0 });
            s.sign_out();
            let call = s.take_out().remove(0);
            (s.answer(call.id, Reply { status, ..Reply::default() }, p), s)
        };
        let (events, s) = answer(503, &mut p);
        assert_eq!((events[0].clone(), s.account), (Event::Session(None), Account::Out));
        let (events, s) = answer(401, &mut p);
        assert_eq!((events[0].clone(), s.account), (Event::Session(None), Account::Out));
        let (events, s) = answer(400, &mut p);
        assert_eq!(
            events,
            vec![Event::Toast("Could not sign out: the library could not be reached. Try again.".into())]
        );
        assert!(s.account.user().is_some());

        // The page cannot drop its cookie, so offline it keeps the session and asks again.
        let mut s = Library {
            account: Account::In(User { id: "u1".into(), email: None, usage: 0, limit: 0, file_limit: 0 }),
            ..Library::default()
        };
        s.sign_out();
        let call = s.take_out().remove(0);
        let events = s.answer(call.id, Reply::default(), &mut p);
        assert!(matches!(events.as_slice(), [Event::Toast(t)] if t.contains("Try again")));
        assert!(s.account.user().is_some());
    }

    #[test]
    fn sign_in_problems_read_as_words() {
        let (_, mut p, mut s) = device();
        let mut failed = |status: u16, body: &str| {
            s.verify_code("ada@example.com", "123456");
            let call = s.take_out().remove(0);
            let reply = Reply { status, body: body.as_bytes().to_vec(), ..Reply::default() };
            s.answer(call.id, reply, &mut p)
        };
        let words = |e: Vec<Event>| match e.as_slice() {
            [Event::Auth(Auth::Failed(w))] => w.clone(),
            other => panic!("{other:?}"),
        };
        assert!(words(failed(400, r#"{"code":"OTP_EXPIRED"}"#)).contains("expired"));
        assert!(words(failed(403, r#"{"code":"TOO_MANY_ATTEMPTS"}"#)).contains("tried too often"));
        assert!(words(failed(429, r#"{"error":"too_many_links"}"#)).contains("in an hour"));
        assert!(words(failed(429, "")).contains("Wait a minute"));
        assert!(words(failed(0, "")).contains("connection"));
        assert!(words(failed(503, r#"{"error":"email_off"}"#)).contains("off here"));
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
    fn exports_the_library_as_one_zip() {
        let (_, mut p, mut s) = device();
        let mut lib = FakeLibrary::signed_in("u1");
        lib.put(Some("c1"), "Castle", vox(2));
        lib.put(Some("c2"), "castle.vox", vox(3));
        lib.put(Some("c3"), "a/b", vox(4));
        lib.put(Some("c4"), "gone", vox(5));
        s.check();
        lib.serve(&mut s, &mut p);

        s.export();
        assert!(s.exporting());
        s.export();
        let calls = s.take_out();
        assert_eq!(calls.len(), 1, "one export at a time");
        let reply = lib.answer(&calls[0]);
        assert!(s.answer(calls[0].id, reply, &mut p).is_empty());
        // Deleted on another device after the list: it is left out.
        lib.projects.retain(|x| x.id != "c4");
        let events = lib.serve(&mut s, &mut p);
        let files =
            [("Castle.vox", vox(2)), ("castle (2).vox", vox(3)), ("a_b.vox", vox(4))].map(|(n, b)| (n.to_string(), b));
        assert_eq!(
            events,
            vec![Event::Export { name: EXPORT_NAME.into(), bytes: crate::files::zip(&files), count: 3 }]
        );
        assert!(!s.exporting());
    }

    #[test]
    fn export_says_when_there_is_nothing_or_it_fails() {
        let (_, mut p, mut s) = device();
        let mut lib = FakeLibrary::signed_in("u1");
        s.check();
        lib.serve(&mut s, &mut p);
        s.export();
        let events = lib.serve(&mut s, &mut p);
        assert!(matches!(&events[..], [Event::Toast(t)] if t.contains("library is empty")), "{events:?}");

        lib.put(Some("c1"), "Castle", vox(2));
        lib.offline = true;
        s.export();
        let events = lib.serve(&mut s, &mut p);
        assert!(matches!(&events[..], [Event::Toast(t)] if t.contains("could not be reached")), "{events:?}");
        assert!(!s.exporting(), "it can be tried again");

        // Signed out: nothing to send.
        lib.offline = false;
        s.sign_out();
        lib.serve(&mut s, &mut p);
        s.export();
        assert!(s.take_out().is_empty());
    }

    #[test]
    fn names_in_the_export_are_safe_and_unique() {
        let names = ["Island", "island.VOX", "..", "", "x:y\\z\u{7}", "Island (2)"];
        assert_eq!(
            zip_names(names.into_iter()),
            ["Island.vox", "island (2).vox", "Untitled.vox", "Untitled (2).vox", "x_y_z_.vox", "Island (2) (2).vox"]
        );
    }

    #[test]
    fn deletes_the_account_once_its_email_is_typed() {
        let (_, mut p, mut s) = device();
        let mut lib = FakeLibrary::signed_in("u1");
        lib.email = Some("me@example.com".into());
        s.check();
        lib.serve(&mut s, &mut p);
        let id = save(&mut p, &mut s, "castle.vox", vox(2));
        lib.serve(&mut s, &mut p);
        assert_eq!(lib.projects.len(), 1);

        s.delete_account("someone@example.com");
        assert!(s.deleting());
        let events = lib.serve(&mut s, &mut p);
        assert!(matches!(&events[..], [Event::Toast(t)] if t.contains("email did not match")), "{events:?}");
        assert!(matches!(s.account, Account::In(_)) && !s.deleting());

        s.delete_account(" ME@example.com ");
        let events = lib.serve(&mut s, &mut p);
        assert!(events.iter().any(|e| matches!(e, Event::Toast(t) if t.starts_with("Deleted your account"))));
        assert_eq!(s.account, Account::Out);
        assert!(lib.user.is_none() && lib.projects.is_empty());
        let row = p.get(&id).expect("the scene stays on this device");
        assert!(row.cloud.is_none() && row.has_file(), "{row:?}");
    }

    #[test]
    fn a_full_library_names_its_use() {
        let (_, mut p, mut s) = device();
        let mut lib = FakeLibrary::signed_in("u1");
        lib.put(Some("c1"), "old", vec![0; 900]);
        lib.limit = 1000;
        s.check();
        lib.serve(&mut s, &mut p);
        save(&mut p, &mut s, "big.vox", vox(9));
        let events = lib.serve(&mut s, &mut p);
        assert!(events.contains(&Event::Listed));
        let toast = events.iter().find_map(|e| match e {
            Event::Toast(t) => Some(t.clone()),
            _ => None,
        });
        assert!(toast.as_deref().is_some_and(|t| t.contains("the library is full (1 KB of 1 KB used)")), "{toast:?}");
        let Account::In(user) = &s.account else { panic!("signed in") };
        assert_eq!(user.usage, 900, "the usage line takes the library's count");
    }

    #[test]
    fn encodes_names_like_the_page() {
        assert_eq!(encode("Island 2"), "Island%202");
        assert_eq!(encode("café"), "caf%C3%A9");
        assert_eq!(encode("a-b_c.(1)"), "a-b_c.(1)");
    }
}
