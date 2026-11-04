//! The projects list and autosaved drafts. Every scene opened or made on this device (or in
//! this browser) is a [`Project`]: its name, where its file is, when it was last edited and a
//! thumbnail. While a scene has edits not yet saved, a draft of the whole scene waits in
//! vxr's own storage as an ordinary `.vox`, written about [`SETTLE`] seconds after the edits
//! stop, one per scene and overwritten each time. The real file is only written by Save,
//! which deletes the draft. A crash or a force quit leaves the draft, and opening the scene
//! again (or the next launch) offers it back.
//!
//! Where it is kept is a [`Store`]: [`Folder`] in the config folder on the desktop, the
//! browser's IndexedDB on the web (`vxr-web`), [`Memory`] in tests. The store holds
//! `device.json` (this device's id), one record per project (`list/<id>.json`),
//! `drafts/<id>.vox` and `thumbs/<id>.png`; the browser also keeps its file handles or copies
//! of the files under the project's id. Together they stay under [`CAP`]: thumbnails go
//! first, then copies of files, then the oldest drafts.
//!
//! Several windows or tabs share one store. Each writes only the records it changed, so none
//! drops another's projects, and each holds the project it has open ([`Store::hold`]): Clear
//! and the cap leave the projects another window or tab has open alone.
//!
//! Each project has a stable id and each draft says when and on which device it was made.
//! Signed in to an account ([`crate::sync`]), a project can also be in the account's library
//! ([`Cloud`]): the list then shows the library's projects too, those not on this device yet
//! as rows with no file ([`Project::stub`]) that download when opened. On the desktop a
//! library project is also a `.vox` file once saved here: a save, or a change on disk seen as
//! it opens, marks it for upload ([`Cloud::dirty`]).

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

/// Seconds the edits must rest before the draft is written.
pub const SETTLE: f64 = 2.0;
/// The most the store holds, in bytes.
pub const CAP: u64 = 200 << 20;
/// A draft not touched for this long (seconds) is deleted.
pub const STALE: f64 = 30.0 * 86_400.0;
/// The thumbnail's longer side, in pixels.
pub const THUMB: u32 = 160;
/// The key of this device's id in the store.
pub const DEVICE: &str = "device.json";
/// Where the projects' records are in the store.
pub const LIST: &str = "list/";

/// The key of project `id`'s record in the store.
pub fn record_key(id: &str) -> String {
    format!("{LIST}{id}.json")
}

/// The project a record's key is for.
fn record_id(key: &str) -> Option<&str> {
    key.strip_prefix(LIST)?.strip_suffix(".json")
}

/// The key of project `id`'s draft in the store.
pub fn draft_key(id: &str) -> String {
    format!("drafts/{id}.vox")
}

/// The key of project `id`'s thumbnail in the store.
pub fn thumb_key(id: &str) -> String {
    format!("thumbs/{id}.png")
}

/// The key of the browser's handle to project `id`'s file, or of its copy of the file.
pub fn file_key(id: &str) -> String {
    format!("files/{id}")
}

/// What a [`Store`] read found.
pub enum Read {
    Here(Vec<u8>),
    /// The store answers later, through [`Projects::stored`] (the browser's IndexedDB).
    Later,
    Missing,
}

/// Where the list, the drafts and the thumbnails are kept.
pub trait Store {
    fn read(&mut self, key: &str) -> Read;
    fn write(&mut self, key: &str, bytes: &[u8]) -> Result<(), String>;
    fn remove(&mut self, key: &str);
    /// The keys under `prefix`.
    fn keys(&mut self, prefix: &str) -> Vec<String>;
    /// Whether a read sees what another window wrote since. The browser's store does not:
    /// its page passes other tabs' changes in instead ([`Projects::listed`]).
    fn live(&self) -> bool {
        true
    }
    /// This window or tab now has project `id` open (None: no project), and no longer the
    /// one before.
    fn hold(&mut self, _id: Option<&str>) {}
    /// Whether another window or tab has project `id` open.
    fn held(&self, _id: &str) -> bool {
        false
    }
    /// Seconds since 1970.
    fn now(&self) -> f64;
    /// A new random id, for a project or this device.
    fn new_id(&mut self) -> String;
    /// When the file at `path` last changed (seconds since 1970), or None when there is no
    /// such file. Only a store on a machine with files can tell ([`Store::files`]).
    fn modified(&self, _path: &Path) -> Option<f64> {
        None
    }
    /// Whether [`Store::modified`] can tell that a file is gone.
    fn files(&self) -> bool {
        false
    }
}

/// A scene's autosaved edits.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Draft {
    /// When it was written, in seconds since 1970.
    pub saved: f64,
    /// The device that wrote it ([`Projects::device`]).
    pub device: String,
    pub size: u64,
    /// When the scene's file had last changed as the draft's edits began (see
    /// [`Project::modified`]): a later change on disk is not in the draft.
    #[serde(default)]
    pub base: Option<f64>,
}

/// Where a project is in an account's library.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Cloud {
    /// The account's user id.
    pub user: String,
    /// Its id in the library: the project's own id, unless a conflict kept it as a copy.
    pub id: String,
    /// The library version this device's copy holds: None before its first upload, and for a
    /// project not downloaded yet.
    #[serde(default)]
    pub version: Option<String>,
    /// The newest version the library last listed.
    #[serde(default)]
    pub latest: Option<String>,
    /// Saved here and not in the library yet (the upload failed or is under way).
    #[serde(default)]
    pub dirty: bool,
    /// Saved here, and the library refused it (over a limit, or not a `.vox` it reads): only
    /// this device has these edits. It does not go up again by itself, nothing downloads over
    /// it, and when the library moves on it is kept as its own project ([`Projects::merge_library`]).
    /// The next upload the library takes clears it.
    #[serde(default)]
    pub held: bool,
}

impl Cloud {
    /// The library has a version newer than this device's copy.
    pub fn behind(&self) -> bool {
        self.version.is_some() && self.latest.is_some() && self.latest != self.version
    }
}

/// What [`Projects::merge_library`] did that the editor follows up on.
#[derive(Debug, Default, PartialEq)]
pub struct Merged {
    /// The new projects whose thumbnails the library has, to fetch.
    pub thumbs: Vec<String>,
    /// The projects kept here as conflicted copies, with their new names.
    pub kept: Vec<(String, String)>,
}

/// A scene opened or made here.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Project {
    pub id: String,
    pub name: String,
    /// Its file, on the desktop.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub path: Option<PathBuf>,
    /// In the browser: the page holds a handle to its file (File System Access).
    #[serde(default)]
    pub handle: bool,
    /// In the browser: the bytes of the copy of its file the page keeps where it has no
    /// handles, else 0.
    #[serde(default)]
    pub copy: u64,
    /// In the browser, for a copy: the [`content_hash`] of the file it was last opened from
    /// through the file input or saved as, so opening that file again finds this project.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source: Option<u64>,
    /// When it was last edited (a draft or a save) or, if later, when its file changed.
    pub edited: f64,
    /// When it was last opened or edited: the list's order.
    pub used: f64,
    /// When its file had last changed as it was opened or saved.
    #[serde(default)]
    pub modified: Option<f64>,
    /// The thumbnail's bytes in the store, or 0.
    #[serde(default)]
    pub thumb: u64,
    #[serde(default)]
    pub draft: Option<Draft>,
    /// Its place in an account's library.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cloud: Option<Cloud>,
}

impl Project {
    /// Whether there is a file to open it from, as opposed to a draft only (a scene never
    /// saved).
    pub fn has_file(&self) -> bool {
        self.path.is_some() || self.handle || self.copy > 0
    }

    /// Its place in `user`'s library, when it has one there.
    pub fn library(&self, user: &str) -> Option<&Cloud> {
        self.cloud.as_ref().filter(|c| c.user == user)
    }

    /// A project of the library with nothing on this device yet: no file and no draft.
    pub fn stub(&self) -> bool {
        self.cloud.is_some() && !self.has_file() && self.draft.is_none()
    }

    /// Whether there is anything to open it from: a file, a draft or the library.
    fn kept(&self) -> bool {
        self.has_file() || self.draft.is_some() || self.cloud.is_some()
    }

    fn size(&self) -> u64 {
        self.draft.as_ref().map_or(0, |d| d.size) + self.thumb + self.copy
    }
}

#[derive(Default)]
struct Index {
    device: String,
    projects: Vec<Project>,
}

/// What `device.json` holds.
#[derive(Serialize, Deserialize)]
struct Device {
    device: String,
}

/// What [`Projects::tick`] asks of the editor this frame.
#[derive(Debug, PartialEq)]
pub enum Tick {
    Idle,
    /// The edits have rested: write the draft ([`Projects::write_draft`]).
    Write,
    /// Undo went back to the saved scene: its draft has nothing to keep.
    Drop,
}

/// The draft the restore prompt offers.
#[derive(Clone, Debug, PartialEq)]
pub struct Offer {
    pub id: String,
    pub name: String,
    /// When the draft was written.
    pub saved: f64,
    /// The file changed on disk after the draft's edits began.
    pub changed: bool,
    /// The scene is not open yet (the offer at launch): Discard leaves the blank scene.
    pub launch: bool,
}

/// The list and the open scene's place in it.
pub struct Projects {
    store: Box<dyn Store>,
    index: Index,
    /// Thumbnails read or made this session, decoded, with their texture once drawn.
    thumbs: HashMap<String, (egui::ColorImage, Option<egui::TextureHandle>)>,
    /// The open scene's project: None for a scene with no file and no draft yet.
    pub current: Option<String>,
    /// The history position last written as a draft or saved.
    drafted: u64,
    /// The history position the edits last moved to, and the frame time they did.
    moved: Option<(u64, f64)>,
    /// A draft read the store answers later, and what it is for.
    waiting: Option<String>,
    /// The draft the restore prompt is asking about.
    pub offer: Option<Offer>,
    /// Each project's record as this window last wrote or read it: [`Projects::keep`]
    /// writes only the records that changed here.
    written: HashMap<String, Vec<u8>>,
    /// The names of the projects whose drafts the cap deleted, for the editor to say so.
    pub evicted: Vec<String>,
}

impl Default for Projects {
    fn default() -> Self {
        Self::load(Box::new(Memory::default()))
    }
}

impl Projects {
    /// The list as `store` keeps it, its stale drafts and missing files cleaned up.
    pub fn load(mut store: Box<dyn Store>) -> Self {
        let mut device = match store.read(DEVICE) {
            Read::Here(b) => serde_json::from_slice::<Device>(&b).map(|d| d.device).unwrap_or_default(),
            _ => String::new(),
        };
        if device.is_empty() {
            device = store.new_id();
            if let Ok(json) = serde_json::to_vec(&Device { device: device.clone() })
                && let Err(e) = store.write(DEVICE, &json)
            {
                eprintln!("vxr: could not keep this device's id: {e}");
            }
        }
        let mut p = Self {
            store,
            index: Index { device, projects: Vec::new() },
            thumbs: HashMap::new(),
            current: None,
            drafted: 0,
            moved: None,
            waiting: None,
            offer: None,
            written: HashMap::new(),
            evicted: Vec::new(),
        };
        p.read_list();
        if p.clean() {
            p.keep();
        }
        p
    }

    /// Read every record in the store; the open scene's stays as this window has it, and
    /// projects whose records are gone (another window forgot them) leave the list.
    fn read_list(&mut self) {
        let mut seen = HashSet::new();
        for key in self.store.keys(LIST) {
            let Some(id) = record_id(&key).map(str::to_string) else { continue };
            if let Read::Here(b) = self.store.read(&key) {
                self.listed_record(&id, Some(b));
                seen.insert(id);
            }
        }
        let gone: Vec<String> = self
            .index
            .projects
            .iter()
            .filter(|p| !seen.contains(&p.id) && self.written.contains_key(&p.id))
            .map(|p| p.id.clone())
            .collect();
        for id in gone {
            self.listed_record(&id, None);
        }
        self.sort();
    }

    /// Read the list again for what other windows changed since (the desktop: the browser's
    /// page passes other tabs' changes in through [`Projects::listed`]).
    pub fn refresh(&mut self) {
        if self.store.live() {
            self.read_list();
        }
    }

    /// Another tab changed `key` in the store, as the page heard: a project's record or its
    /// thumbnail, with its bytes now, or None when it was deleted.
    pub fn listed(&mut self, key: &str, bytes: Option<Vec<u8>>) {
        if let Some(id) = record_id(key) {
            self.listed_record(id, bytes);
            self.sort();
        } else if let Some(id) = key.strip_prefix("thumbs/").and_then(|k| k.strip_suffix(".png")) {
            match bytes {
                Some(b) => self.thumb_bytes(id, &b),
                None => _ = self.thumbs.remove(id),
            }
        }
    }

    /// Take project `id`'s record as the store has it (None: deleted), unless it is the open
    /// scene's, whose record this window keeps.
    fn listed_record(&mut self, id: &str, bytes: Option<Vec<u8>>) {
        if self.current.as_deref() == Some(id) {
            return;
        }
        let Some(b) = bytes else {
            self.index.projects.retain(|p| p.id != id);
            self.thumbs.remove(id);
            self.written.remove(id);
            return;
        };
        if self.written.get(id) == Some(&b) {
            return;
        }
        let Ok(p) = serde_json::from_slice::<Project>(&b) else { return };
        if p.id != id {
            return;
        }
        let thumb = p.thumb;
        let before = self.get(id).map(|o| o.thumb);
        match self.index.projects.iter().position(|o| o.id == id) {
            Some(i) => self.index.projects[i] = p,
            None => self.index.projects.push(p),
        }
        self.written.insert(id.into(), b);
        if thumb == 0 {
            self.thumbs.remove(id);
        } else if before != Some(thumb)
            && let Read::Here(png) = self.store.read(&thumb_key(id))
        {
            self.thumb_bytes(id, &png);
        }
    }

    fn sort(&mut self) {
        self.index.projects.sort_by(|a, b| b.used.total_cmp(&a.used));
    }

    /// The open scene's project is now `id`, which this window holds.
    fn set_current(&mut self, id: Option<String>) {
        self.store.hold(id.as_deref());
        self.current = id;
    }

    /// Whether Clear, the cap and the cleanup must leave project `id` alone: it is open here
    /// or in another window or tab.
    fn busy(&self, id: &str) -> bool {
        self.current.as_deref() == Some(id) || self.store.held(id)
    }

    /// The list, most recently used first.
    pub fn list(&self) -> &[Project] {
        &self.index.projects
    }

    pub fn get(&self, id: &str) -> Option<&Project> {
        self.index.projects.iter().find(|p| p.id == id)
    }

    fn get_mut(&mut self, id: &str) -> Option<&mut Project> {
        self.index.projects.iter_mut().find(|p| p.id == id)
    }

    /// This device's id, which each draft records.
    pub fn device(&self) -> &str {
        &self.index.device
    }

    pub fn now(&self) -> f64 {
        self.store.now()
    }

    /// The bytes the store holds.
    pub fn usage(&self) -> u64 {
        self.index.projects.iter().map(Project::size).sum()
    }

    /// The open scene's draft, if it has one.
    pub fn current_draft(&self) -> Option<&Draft> {
        self.current.as_deref().and_then(|id| self.get(id)).and_then(|p| p.draft.as_ref())
    }

    /// Write the records that changed here since this window last wrote or read them.
    fn keep(&mut self) {
        self.sort();
        for p in &self.index.projects {
            let Ok(json) = serde_json::to_vec(p) else { continue };
            if self.written.get(&p.id) == Some(&json) {
                continue;
            }
            match self.store.write(&record_key(&p.id), &json) {
                Ok(()) => _ = self.written.insert(p.id.clone(), json),
                Err(e) => eprintln!("vxr: could not keep {} in the projects list: {e}", p.name),
            }
        }
    }

    /// Delete stale drafts, and projects with nothing left to open: neither a file nor a
    /// draft. A project whose file is gone (moved, or on a drive not mounted) keeps its
    /// draft, as a scene with no file; only [`STALE`] deletes a draft. True when anything
    /// changed.
    fn clean(&mut self) -> bool {
        let now = self.store.now();
        let mut gone = Vec::new();
        let mut changed = false;
        let ids: Vec<String> = self.index.projects.iter().map(|p| p.id.clone()).collect();
        for id in ids {
            if self.busy(&id) {
                continue;
            }
            let missing = self.store.files()
                && self.get(&id).and_then(|p| p.path.as_ref()).is_some_and(|f| self.store.modified(f).is_none());
            let Some(p) = self.get_mut(&id) else { continue };
            if p.draft.as_ref().is_some_and(|d| now - d.saved > STALE) {
                p.draft = None;
                self.store.remove(&draft_key(&id));
                changed = true;
            }
            let Some(p) = self.get_mut(&id) else { continue };
            if missing {
                p.path = None;
                p.modified = None;
                changed = true;
            }
            if !p.kept() {
                gone.push(id);
            }
        }
        for id in &gone {
            self.forget(id);
        }
        changed || !gone.is_empty()
    }

    /// Delete a project and everything kept for it.
    fn forget(&mut self, id: &str) {
        self.store.remove(&draft_key(id));
        self.store.remove(&thumb_key(id));
        // The browser's handle or copy of its file.
        self.store.remove(&file_key(id));
        self.store.remove(&record_key(id));
        self.written.remove(id);
        self.thumbs.remove(id);
        self.index.projects.retain(|p| p.id != id);
    }

    /// Free space until the store is under [`CAP`], what is easiest to get back first:
    /// thumbnails, then copies of files the user also has, then drafts, the oldest first,
    /// each named in [`Projects::evicted`]. The projects open here or in another window or
    /// tab keep everything.
    fn fit(&mut self) {
        // Other windows' projects count too, and one may have changed a record this one is
        // about to rewrite.
        self.refresh();
        if self.usage() <= CAP {
            return;
        }
        let mut order: Vec<(f64, String)> =
            self.index.projects.iter().filter(|p| !self.busy(&p.id)).map(|p| (p.used, p.id.clone())).collect();
        order.sort_by(|a, b| a.0.total_cmp(&b.0));
        for pass in 0..3 {
            for (_, id) in &order {
                if self.usage() <= CAP {
                    return;
                }
                let Some(p) = self.get_mut(id) else { continue };
                match pass {
                    0 if p.thumb > 0 => {
                        p.thumb = 0;
                        self.store.remove(&thumb_key(id));
                        self.thumbs.remove(id);
                    }
                    1 if replaceable_copy(p) => {
                        p.copy = 0;
                        p.source = None;
                        // A library project stays listed, to download again.
                        if let Some(c) = &mut p.cloud {
                            c.version = None;
                        }
                        let left = p.kept();
                        self.store.remove(&file_key(id));
                        if !left {
                            self.forget(id);
                        }
                    }
                    2 if p.draft.is_some() => {
                        p.draft = None;
                        let (name, left) = (p.name.clone(), p.kept());
                        self.store.remove(&draft_key(id));
                        self.evicted.push(name);
                        if !left {
                            self.forget(id);
                        }
                    }
                    _ => {}
                }
            }
        }
    }

    /// A new scene is open, at history `position`, with no project yet.
    pub fn reset(&mut self, position: u64) {
        self.set_current(None);
        self.drafted = position;
        self.moved = None;
        self.offer = None;
        self.waiting = None;
    }

    /// A scene was opened from a file (`path` on the desktop; the browser's project `id`,
    /// with a `handle` to its file or a copy of `copy` bytes): it becomes the open scene's
    /// project, found by its id or path, else new. `modified` is when the file last
    /// changed. Offers its draft back when it has one.
    pub fn opened(
        &mut self,
        name: &str,
        path: Option<&Path>,
        id: Option<&str>,
        modified: Option<f64>,
        handle: bool,
        copy: u64,
    ) -> String {
        // Another window may have drafted it since.
        self.refresh();
        let now = self.store.now();
        let found = self
            .index
            .projects
            .iter()
            .position(|p| id.is_some_and(|id| p.id == id) || (path.is_some() && p.path.as_deref() == path));
        let id = match found {
            Some(i) => self.index.projects[i].id.clone(),
            None => {
                let id = id.map_or_else(|| self.store.new_id(), str::to_string);
                self.index.projects.push(Project {
                    id: id.clone(),
                    name: name.into(),
                    path: None,
                    handle: false,
                    copy: 0,
                    source: None,
                    edited: modified.unwrap_or(now),
                    used: now,
                    modified,
                    thumb: 0,
                    draft: None,
                    cloud: None,
                });
                id
            }
        };
        let p = self.get_mut(&id).expect("just found or made");
        p.name = name.into();
        if path.is_some() {
            p.path = path.map(Path::to_path_buf);
        }
        p.handle |= handle;
        if copy > 0 {
            p.copy = copy;
        }
        p.used = now;
        // A library project's file changed on disk since vxr last saw it: that goes up.
        if path.is_some()
            && let (Some(m), Some(seen), Some(c)) = (modified, p.modified, p.cloud.as_mut())
            && m > seen + 1.0
        {
            c.dirty = true;
        }
        // A draft holds edits made on the file as it was when they began: keep that time.
        if p.draft.is_none() || modified.is_some() {
            p.modified = modified.or(p.modified);
        }
        if let Some(m) = modified {
            p.edited = p.edited.max(m);
        }
        let offer = p.draft.as_ref().map(|d| Offer {
            id: id.clone(),
            name: p.name.clone(),
            saved: d.saved,
            changed: changed(modified, d.base),
            launch: false,
        });
        self.set_current(Some(id.clone()));
        self.offer = offer;
        self.keep();
        id
    }

    /// What to offer at a launch with no file: the most recently used project's draft.
    pub fn launch_offer(&mut self) -> Option<Offer> {
        let p = self.index.projects.iter().max_by(|a, b| a.used.total_cmp(&b.used))?;
        let d = p.draft.as_ref()?;
        let now = p.path.as_deref().and_then(|f| self.store.modified(f));
        Some(Offer {
            id: p.id.clone(),
            name: p.name.clone(),
            saved: d.saved,
            changed: changed(now, d.base),
            launch: true,
        })
    }

    /// The scene was saved as `name`: to `path` on the desktop; in the browser to its file's
    /// `handle`, or as a download whose copy of `copy` bytes the page keeps. The draft goes.
    /// A scene with no project yet gets one.
    pub fn saved(&mut self, name: &str, path: Option<&Path>, handle: bool, copy: u64, position: u64) {
        self.refresh();
        let now = self.store.now();
        let id = self.ensure(name);
        // Saved over another project's file: that one is this one now.
        if let Some(path) = path {
            let other: Vec<String> = self
                .index
                .projects
                .iter()
                .filter(|p| p.id != id && p.path.as_deref() == Some(path))
                .map(|p| p.id.clone())
                .collect();
            for o in other {
                self.forget(&o);
            }
        }
        let modified = path.and_then(|f| self.store.modified(f)).unwrap_or(now);
        let had = self.get(&id).is_some_and(|p| p.draft.is_some());
        let p = self.get_mut(&id).expect("ensured");
        p.name = name.into();
        if path.is_some() {
            p.path = path.map(Path::to_path_buf);
        }
        p.handle |= handle;
        if copy > 0 {
            p.copy = copy;
        }
        p.draft = None;
        p.edited = now;
        p.used = now;
        p.modified = Some(modified);
        // Saved to a library project's file (signed out, or before its upload): it goes up
        // when it can.
        if path.is_some()
            && let Some(c) = &mut p.cloud
        {
            c.dirty = true;
        }
        if had {
            self.store.remove(&draft_key(&id));
        }
        self.drafted = position;
        self.moved = None;
        self.keep();
    }

    /// The open scene's project, made now (named `name`) when it has none.
    pub fn ensure(&mut self, name: &str) -> String {
        if let Some(id) = &self.current
            && self.get(id).is_some()
        {
            return id.clone();
        }
        let now = self.store.now();
        let id = self.store.new_id();
        self.index.projects.push(Project {
            id: id.clone(),
            name: name.into(),
            path: None,
            handle: false,
            copy: 0,
            source: None,
            edited: now,
            used: now,
            modified: None,
            thumb: 0,
            draft: None,
            cloud: None,
        });
        self.set_current(Some(id.clone()));
        id
    }

    /// Called each frame with the history `position`, the position of the last save and the
    /// frame `time` (seconds); `busy` while a gesture is under way. Says when to write the
    /// draft: once the position has rested [`SETTLE`] seconds.
    pub fn tick(&mut self, position: u64, saved: u64, time: f64, busy: bool) -> Tick {
        if position == self.drafted {
            self.moved = None;
            return Tick::Idle;
        }
        if position == saved {
            self.drafted = position;
            self.moved = None;
            return if self.current_draft().is_some() { Tick::Drop } else { Tick::Idle };
        }
        match self.moved {
            Some((p, t)) if p == position => {
                if !busy && time - t >= SETTLE {
                    Tick::Write
                } else {
                    Tick::Idle
                }
            }
            _ => {
                self.moved = Some((position, time));
                Tick::Idle
            }
        }
    }

    /// When the next frame must draw so the draft is written on time (frame time), if one
    /// is waiting.
    pub fn due(&self) -> Option<f64> {
        self.moved.map(|(_, t)| t + SETTLE)
    }

    /// Whether the open scene has edits newer than its draft.
    pub fn behind(&self, position: u64, saved: u64) -> bool {
        position != self.drafted && position != saved
    }

    /// Write the open scene's draft: its `.vox` bytes at history `position`, named `name`,
    /// and its thumbnail (PNG).
    pub fn write_draft(&mut self, name: &str, vox: &[u8], thumb: Option<&[u8]>, position: u64) {
        let id = self.ensure(name);
        self.drafted = position;
        self.moved = None;
        if let Err(e) = self.store.write(&draft_key(&id), vox) {
            eprintln!("vxr: could not keep a draft of {name}: {e}");
            return;
        }
        let now = self.store.now();
        let device = self.index.device.clone();
        let p = self.get_mut(&id).expect("ensured");
        let base = p.draft.as_ref().map_or(p.modified, |d| d.base);
        p.draft = Some(Draft { saved: now, device, size: vox.len() as u64, base });
        p.name = name.into();
        p.edited = now;
        p.used = now;
        if let Some(png) = thumb {
            self.set_thumb(&id, png);
        }
        self.fit();
        self.keep();
    }

    /// Keep `png` as project `id`'s thumbnail.
    pub fn set_thumb(&mut self, id: &str, png: &[u8]) {
        if self.store.write(&thumb_key(id), png).is_ok()
            && let Some(p) = self.get_mut(id)
        {
            p.thumb = png.len() as u64;
        }
        self.thumb_bytes(id, png);
    }

    fn thumb_bytes(&mut self, id: &str, png: &[u8]) {
        if let Some(img) = decode(png) {
            self.thumbs.insert(id.into(), (img, None));
        }
    }

    /// Project `id`'s thumbnail as a texture, made on first use.
    pub fn texture(&mut self, ctx: &egui::Context, id: &str) -> Option<egui::TextureId> {
        let (img, tex) = self.thumbs.get_mut(id)?;
        let tex = tex.get_or_insert_with(|| ctx.load_texture(format!("thumb-{id}"), img.clone(), Default::default()));
        Some(tex.id())
    }

    /// Delete project `id`'s draft (Discard, Don't save, undo back to the saved scene). A
    /// project with no file goes with it.
    pub fn drop_draft(&mut self, id: &str) {
        let Some(p) = self.get_mut(id) else { return };
        p.draft = None;
        let keep = p.kept();
        self.store.remove(&draft_key(id));
        if !keep {
            self.forget(id);
            if self.current.as_deref() == Some(id) {
                self.set_current(None);
            }
        }
        if self.offer.as_ref().is_some_and(|o| o.id == id) {
            self.offer = None;
        }
        self.keep();
    }

    /// Read project `id`'s draft. [`Read::Later`] when the store answers through
    /// [`Projects::stored`].
    pub fn read_draft(&mut self, id: &str) -> Read {
        let r = self.store.read(&draft_key(id));
        if matches!(r, Read::Later) {
            self.waiting = Some(id.into());
        }
        r
    }

    /// The store answered a read it said would come later: the draft's bytes, with the id
    /// of the project it is for, when it is the draft waited for.
    pub fn stored(&mut self, key: &str, bytes: Vec<u8>) -> Option<(String, Vec<u8>)> {
        let id = self.waiting.take_if(|id| draft_key(id) == key)?;
        Some((id, bytes))
    }

    /// The open scene is project `id`'s restored draft, at history `position`: still not
    /// saved, and already drafted.
    pub fn restored(&mut self, id: &str, position: u64) {
        self.set_current(Some(id.into()));
        self.drafted = position;
        self.moved = None;
        self.offer = None;
        let now = self.store.now();
        if let Some(p) = self.get_mut(id) {
            p.used = now;
        }
        self.keep();
    }

    /// Clear: forget every project but the open scene's and those another window or tab has
    /// open, with their drafts, thumbnails and copies.
    pub fn clear(&mut self) {
        self.refresh();
        let ids: Vec<String> = self.index.projects.iter().filter(|p| !self.busy(&p.id)).map(|p| p.id.clone()).collect();
        for id in ids {
            self.forget(&id);
        }
        self.keep();
    }

    /// The project the browser keeps a copy of the file `name` for, with [`content_hash`]
    /// `hash`: the same file opened again through the file input. A file of the same name
    /// with other bytes is another project.
    pub fn copy_of(&self, name: &str, hash: u64) -> Option<String> {
        self.list()
            .iter()
            .find(|p| !p.handle && p.copy > 0 && p.name == name && p.source == Some(hash))
            .map(|p| p.id.clone())
    }

    /// Project `id`'s copy is of the file with [`content_hash`] `hash` (the browser).
    pub fn set_source(&mut self, id: &str, hash: u64) {
        if let Some(p) = self.get_mut(id) {
            p.source = Some(hash);
            self.keep();
        }
    }

    /// Forget every project, the open scene's too, as if nothing had been kept here.
    pub fn forget_all(&mut self) {
        let ids: Vec<String> = self.index.projects.iter().map(|p| p.id.clone()).collect();
        for id in ids {
            self.forget(&id);
        }
        self.set_current(None);
        self.offer = None;
        self.waiting = None;
        self.keep();
    }

    /// Change project `id` with `f`, and keep the list.
    pub fn update(&mut self, id: &str, f: impl FnOnce(&mut Project)) {
        if let Some(p) = self.get_mut(id) {
            f(p);
            self.keep();
        }
    }

    /// Whether this device has files of its own (the desktop): a library project is then
    /// saved to a `.vox` file, and its copy here is only for opening it until it is.
    pub fn files(&self) -> bool {
        self.store.files()
    }

    /// When the file at `path` last changed, where the store can tell ([`Store::modified`]).
    pub fn disk_modified(&self, path: &Path) -> Option<f64> {
        self.store.modified(path)
    }

    /// Project `id`'s copy of its file, as [`Projects::keep_copy`] kept it.
    pub fn read_copy(&mut self, id: &str) -> Read {
        self.store.read(&file_key(id))
    }

    /// Delete project `id`'s copy of its file: it has a file of its own now.
    pub fn drop_copy(&mut self, id: &str) {
        let Some(p) = self.get_mut(id) else { return };
        if p.copy == 0 {
            return;
        }
        p.copy = 0;
        self.store.remove(&file_key(id));
        self.keep();
    }

    /// Keep `vox` as project `id`'s copy of its file (a library save or download), in place
    /// of any handle to a file of this device's.
    pub fn keep_copy(&mut self, id: &str, vox: &[u8]) -> Result<(), String> {
        self.store.write(&file_key(id), vox)?;
        if let Some(p) = self.get_mut(id) {
            p.copy = vox.len() as u64;
            p.handle = false;
        }
        self.fit();
        self.keep();
        Ok(())
    }

    /// `user`'s library lists `remote`: add the projects not here yet (with no file, to
    /// download when opened), note each one's newest version, and let go of the ones the
    /// library no longer has (another device deleted them). `fresh` are the library ids
    /// uploaded since the list was asked for, which it may show as they were before (or not
    /// at all): they are left as they are.
    ///
    /// A project the library refused a save of ([`Cloud::held`]) that the library has moved
    /// on from is kept as its own project here, "<name> (conflicted copy)", out of the
    /// library, and the library's version is listed beside it to download.
    pub fn merge_library(&mut self, user: &str, remote: &[crate::sync::Remote], fresh: &[String]) -> Merged {
        let mut merged = Merged::default();
        for r in remote {
            if fresh.contains(&r.id) {
                continue;
            }
            let updated = r.updated / 1000.0;
            let here = self.index.projects.iter_mut().find(|p| p.library(user).is_some_and(|c| c.id == r.id));
            if let Some(p) = here {
                let stub = p.stub();
                let c = p.cloud.as_mut().expect("found by its library");
                if c.held && c.version.as_deref() != Some(r.version.as_str()) {
                    let stem = p.name.strip_suffix(".vox").unwrap_or(&p.name);
                    p.name = format!("{stem} (conflicted copy)");
                    p.cloud = None;
                    merged.kept.push((p.id.clone(), p.name.clone()));
                } else {
                    c.latest = Some(r.version.clone());
                    if stub {
                        p.name = r.name.clone();
                        p.edited = p.edited.max(updated);
                        if r.thumb && p.thumb == 0 {
                            merged.thumbs.push(p.id.clone());
                        }
                    }
                    continue;
                }
            }
            let id = if self.get(&r.id).is_none() { r.id.clone() } else { self.store.new_id() };
            self.index.projects.push(Project {
                id: id.clone(),
                name: r.name.clone(),
                path: None,
                handle: false,
                copy: 0,
                source: None,
                edited: updated,
                used: updated,
                modified: None,
                thumb: 0,
                draft: None,
                cloud: Some(Cloud {
                    user: user.into(),
                    id: r.id.clone(),
                    version: None,
                    latest: Some(r.version.clone()),
                    dirty: false,
                    held: false,
                }),
            });
            if r.thumb {
                merged.thumbs.push(id);
            }
        }
        let mut gone = Vec::new();
        for p in &mut self.index.projects {
            let Some(c) = p.library(user) else { continue };
            // Not uploaded yet, listed, or uploaded after the list was asked for: nothing to
            // let go of.
            if (c.version.is_none() && c.latest.is_none())
                || remote.iter().any(|r| r.id == c.id)
                || fresh.contains(&c.id)
            {
                continue;
            }
            if p.stub() && self.current.as_ref() != Some(&p.id) {
                gone.push(p.id.clone());
            } else if !c.dirty {
                p.cloud = None;
            }
        }
        for id in &gone {
            self.forget(id);
        }
        self.keep();
        merged
    }

    /// Signed out, or in as `user`: forget the library projects of anyone else that have
    /// nothing on this device.
    pub fn signed_out(&mut self, user: Option<&str>) {
        let gone: Vec<String> = self
            .index
            .projects
            .iter()
            .filter(|p| p.stub() && p.cloud.as_ref().is_some_and(|c| Some(c.user.as_str()) != user))
            .filter(|p| self.current.as_ref() != Some(&p.id))
            .map(|p| p.id.clone())
            .collect();
        if gone.is_empty() {
            return;
        }
        for id in &gone {
            self.forget(id);
        }
        self.keep();
    }
}

/// Whether the cap may delete `p`'s copy of its file: one the user also has, as the download
/// it was saved as or the file it was opened from, or the library's. A save the library has
/// not taken yet (waiting to upload, or refused and held) is the only one, and keeps its copy.
fn replaceable_copy(p: &Project) -> bool {
    p.copy > 0 && !p.cloud.as_ref().is_some_and(|c| c.dirty || c.held)
}

/// A hash of a file's bytes (64-bit FNV-1a), to know the same file opened again.
pub fn content_hash(bytes: &[u8]) -> u64 {
    bytes.iter().fold(0xcbf2_9ce4_8422_2325, |h, b| (h ^ u64::from(*b)).wrapping_mul(0x0100_0000_01b3))
}

/// Whether a file modified at `now` changed after a draft based on it at `base`.
fn changed(now: Option<f64>, base: Option<f64>) -> bool {
    match (now, base) {
        (Some(n), Some(b)) => n > b + 1.0,
        _ => false,
    }
}

fn decode(png: &[u8]) -> Option<egui::ColorImage> {
    let mut d = png::Decoder::new(std::io::Cursor::new(png));
    d.set_transformations(png::Transformations::EXPAND | png::Transformations::STRIP_16);
    let mut r = d.read_info().ok()?;
    let mut buf = vec![0; r.output_buffer_size()?];
    let info = r.next_frame(&mut buf).ok()?;
    let size = [info.width as usize, info.height as usize];
    let px = &buf[..info.buffer_size()];
    Some(match info.color_type {
        png::ColorType::Rgb => egui::ColorImage::from_rgb(size, px),
        png::ColorType::Rgba => egui::ColorImage::from_rgba_unmultiplied(size, px),
        _ => return None,
    })
}

/// How long ago `t` was, from `now` (both seconds since 1970), for the list: "just now",
/// "5 min ago", "yesterday", "3 weeks ago".
pub fn ago(now: f64, t: f64) -> String {
    let s = (now - t).max(0.0);
    let n = |v: f64, unit: &str| {
        let v = v.floor() as u64;
        format!("{v} {unit}{} ago", if v == 1 { "" } else { "s" })
    };
    match s {
        s if s < 60.0 => "just now".into(),
        s if s < 3600.0 => format!("{} min ago", (s / 60.0).floor() as u64),
        s if s < 86_400.0 => n(s / 3600.0, "hour"),
        s if s < 2.0 * 86_400.0 => "yesterday".into(),
        s if s < 14.0 * 86_400.0 => n(s / 86_400.0, "day"),
        s if s < 60.0 * 86_400.0 => n(s / (7.0 * 86_400.0), "week"),
        s if s < 365.0 * 86_400.0 => n(s / (30.0 * 86_400.0), "month"),
        s => n(s / (365.0 * 86_400.0), "year"),
    }
}

/// A size in bytes for the Storage line: "640 KB", "1.2 MB".
pub fn bytes(n: u64) -> String {
    let mb = n as f64 / (1024.0 * 1024.0);
    if n < 1024 * 1024 {
        format!("{} KB", n.div_ceil(1024))
    } else if mb < 10.0 {
        format!("{mb:.1} MB")
    } else {
        format!("{} MB", mb.round() as u64)
    }
}

/// A store in memory, shared with its clones: for tests and the headless check.
#[derive(Clone, Default)]
pub struct Memory {
    pub files: std::rc::Rc<std::cell::RefCell<std::collections::BTreeMap<String, Vec<u8>>>>,
    /// The clock, in seconds since 1970.
    pub clock: std::rc::Rc<std::cell::Cell<f64>>,
    next: std::rc::Rc<std::cell::Cell<u32>>,
    /// The project each window has open, by window.
    holds: std::rc::Rc<std::cell::RefCell<HashMap<u32, String>>>,
    windows: std::rc::Rc<std::cell::Cell<u32>>,
    /// This window.
    window: u32,
    /// For a store that stands in for the desktop's ([`Memory::desktop`]): when each file on
    /// disk last changed, which the test sets. None: a store with no files (the browser's).
    pub disk: Option<std::rc::Rc<std::cell::RefCell<HashMap<PathBuf, f64>>>>,
}

impl Memory {
    /// A store as the desktop's is: it can tell when a file changed ([`Store::files`]).
    pub fn desktop() -> Self {
        Self { disk: Some(Default::default()), ..Self::default() }
    }

    /// The file at `path` changed now, by the store's clock.
    pub fn touch(&self, path: &Path) {
        if let Some(disk) = &self.disk {
            disk.borrow_mut().insert(path.into(), self.clock.get());
        }
    }

    /// The same store as another window (or tab) sees it: what each has open is its own.
    pub fn another(&self) -> Self {
        self.windows.set(self.windows.get() + 1);
        Self { window: self.windows.get(), ..self.clone() }
    }
}

impl Store for Memory {
    fn read(&mut self, key: &str) -> Read {
        self.files.borrow().get(key).cloned().map_or(Read::Missing, Read::Here)
    }

    fn write(&mut self, key: &str, bytes: &[u8]) -> Result<(), String> {
        self.files.borrow_mut().insert(key.into(), bytes.to_vec());
        Ok(())
    }

    fn remove(&mut self, key: &str) {
        self.files.borrow_mut().remove(key);
    }

    fn keys(&mut self, prefix: &str) -> Vec<String> {
        self.files.borrow().keys().filter(|k| k.starts_with(prefix)).cloned().collect()
    }

    fn hold(&mut self, id: Option<&str>) {
        let mut holds = self.holds.borrow_mut();
        match id {
            Some(id) => _ = holds.insert(self.window, id.into()),
            None => _ = holds.remove(&self.window),
        }
    }

    fn held(&self, id: &str) -> bool {
        self.holds.borrow().iter().any(|(w, h)| *w != self.window && h == id)
    }

    fn now(&self) -> f64 {
        self.clock.get()
    }

    fn new_id(&mut self) -> String {
        self.next.set(self.next.get() + 1);
        format!("p{}", self.next.get())
    }

    fn modified(&self, path: &Path) -> Option<f64> {
        self.disk.as_ref()?.borrow().get(path).copied()
    }

    fn files(&self) -> bool {
        self.disk.is_some()
    }
}

/// The desktop's store: files in a folder (`projects` in the config folder). Each window
/// holds the project it has open with a lock on `open/<id>.lock`, which the system lets go
/// when the window closes or crashes.
#[cfg(not(target_arch = "wasm32"))]
pub struct Folder {
    dir: PathBuf,
    /// The project this window holds, and its locked file.
    open: Option<(String, std::fs::File)>,
}

#[cfg(not(target_arch = "wasm32"))]
impl Folder {
    pub fn new(dir: PathBuf) -> Self {
        Self { dir, open: None }
    }

    fn lock_path(&self, id: &str) -> PathBuf {
        self.dir.join("open").join(format!("{id}.lock"))
    }
}

#[cfg(not(target_arch = "wasm32"))]
impl Store for Folder {
    fn read(&mut self, key: &str) -> Read {
        match std::fs::read(self.dir.join(key)) {
            Ok(b) => Read::Here(b),
            Err(_) => Read::Missing,
        }
    }

    fn write(&mut self, key: &str, bytes: &[u8]) -> Result<(), String> {
        let path = self.dir.join(key);
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
        }
        crate::files::save_atomic(&path, bytes)
    }

    fn remove(&mut self, key: &str) {
        _ = std::fs::remove_file(self.dir.join(key));
    }

    fn keys(&mut self, prefix: &str) -> Vec<String> {
        let Ok(entries) = std::fs::read_dir(self.dir.join(prefix)) else { return Vec::new() };
        entries.flatten().filter_map(|e| e.file_name().to_str().map(|n| format!("{prefix}{n}"))).collect()
    }

    fn hold(&mut self, id: Option<&str>) {
        if self.open.as_ref().map(|(o, _)| o.as_str()) == id {
            return;
        }
        // Closing the file lets its lock go.
        self.open = None;
        let Some(id) = id else { return };
        let path = self.lock_path(id);
        if let Some(dir) = path.parent() {
            _ = std::fs::create_dir_all(dir);
        }
        if let Ok(f) = std::fs::OpenOptions::new().create(true).truncate(false).write(true).open(&path)
            && f.try_lock().is_ok()
        {
            self.open = Some((id.into(), f));
        }
    }

    fn held(&self, id: &str) -> bool {
        if self.open.as_ref().is_some_and(|(o, _)| o == id) {
            return false;
        }
        let Ok(f) = std::fs::OpenOptions::new().write(true).open(self.lock_path(id)) else { return false };
        matches!(f.try_lock(), Err(std::fs::TryLockError::WouldBlock))
    }

    fn now(&self) -> f64 {
        std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0.0, |d| d.as_secs_f64())
    }

    fn new_id(&mut self) -> String {
        use std::hash::{BuildHasher, Hasher};
        // The standard library seeds each `RandomState` from the operating system.
        let half = || {
            let mut h = std::collections::hash_map::RandomState::new().build_hasher();
            h.write_u128(
                std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_nanos()),
            );
            h.finish()
        };
        format!("{:016x}{:016x}", half(), half())
    }

    fn modified(&self, path: &Path) -> Option<f64> {
        modified(path)
    }

    fn files(&self) -> bool {
        true
    }
}

/// When the file at `path` last changed, in seconds since 1970.
#[cfg(not(target_arch = "wasm32"))]
pub fn modified(path: &Path) -> Option<f64> {
    let m = std::fs::metadata(path).ok()?.modified().ok()?;
    Some(m.duration_since(std::time::UNIX_EPOCH).map_or(0.0, |d| d.as_secs_f64()))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn memory() -> (Memory, Projects) {
        let m = Memory::default();
        m.clock.set(1_000_000.0);
        (m.clone(), Projects::load(Box::new(m)))
    }

    #[test]
    fn a_draft_waits_for_the_edits_to_rest() {
        let (m, mut p) = memory();
        assert_eq!(p.tick(0, 0, 0.0, false), Tick::Idle);
        assert_eq!(p.tick(3, 0, 1.0, false), Tick::Idle);
        assert_eq!(p.tick(3, 0, 2.9, false), Tick::Idle);
        // Another edit starts the wait again.
        assert_eq!(p.tick(4, 0, 3.0, false), Tick::Idle);
        assert_eq!(p.tick(4, 0, 4.5, false), Tick::Idle);
        assert_eq!(p.tick(4, 0, 5.0, true), Tick::Idle, "not during a gesture");
        assert_eq!(p.due(), Some(5.0));
        assert_eq!(p.tick(4, 0, 5.0, false), Tick::Write);
        p.write_draft("Untitled", b"vox", Some(&[]), 4);
        assert_eq!(p.tick(4, 0, 9.0, false), Tick::Idle);
        assert_eq!(p.due(), None);
        let id = p.current.clone().unwrap();
        let d = p.get(&id).unwrap().draft.clone().unwrap();
        assert_eq!((d.saved, d.size, d.device.as_str()), (1_000_000.0, 3, p.device()));
        assert_eq!(m.files.borrow().get(&draft_key(&id)).map(Vec::as_slice), Some(&b"vox"[..]));
        // Undo back to the saved scene: a never-saved scene has nothing left to list.
        assert_eq!(p.tick(0, 0, 10.0, false), Tick::Drop);
        p.drop_draft(&id);
        assert!(p.list().is_empty() && !m.files.borrow().contains_key(&draft_key(&id)));
    }

    #[test]
    fn the_list_survives_a_restart_and_offers_the_draft_back() {
        let (m, mut p) = memory();
        let id = p.opened("castle.vox", Some(Path::new("/home/user/castle.vox")), None, Some(500.0), false, 0);
        assert!(p.offer.is_none());
        p.write_draft("castle.vox", b"draft", None, 2);
        let device = p.device().to_string();
        drop(p);
        m.clock.set(1_000_100.0);
        let mut p = Projects::load(Box::new(m.clone()));
        assert_eq!(p.device(), device, "the device keeps its id");
        assert_eq!(p.list().len(), 1);
        let offer = p.launch_offer().unwrap();
        assert_eq!((offer.id.as_str(), offer.changed, offer.launch), (id.as_str(), false, true));
        // Opened again, with the file changed on disk since.
        let again = p.opened("castle.vox", Some(Path::new("/home/user/castle.vox")), None, Some(900.0), false, 0);
        assert_eq!(again, id, "the same file is the same project");
        let offer = p.offer.clone().unwrap();
        assert!(offer.changed && !offer.launch && offer.saved == 1_000_000.0);
        let Read::Here(b) = p.read_draft(&id) else { panic!("the draft is kept") };
        assert_eq!(b, b"draft");
        p.restored(&id, 7);
        assert!(p.offer.is_none() && p.current_draft().is_some());
        assert_eq!(p.tick(7, u64::MAX, 0.0, false), Tick::Idle, "a restored draft is already written");
    }

    #[test]
    fn save_deletes_the_draft_and_keeps_the_project() {
        let (m, mut p) = memory();
        p.write_draft("Untitled", b"draft", Some(b"png"), 3);
        let id = p.current.clone().unwrap();
        p.saved("tower.vox", Some(Path::new("/home/user/tower.vox")), false, 0, 3);
        let pr = p.get(&id).unwrap();
        assert!(pr.draft.is_none() && pr.has_file() && pr.name == "tower.vox");
        assert!(!m.files.borrow().contains_key(&draft_key(&id)));
        assert_eq!(p.tick(3, 3, 0.0, false), Tick::Idle);
        // Discarding a draft of a project with a file keeps the project.
        p.write_draft("tower.vox", b"more", None, 4);
        p.drop_draft(&id);
        assert_eq!(p.list().len(), 1);
    }

    #[test]
    fn old_drafts_and_the_oldest_over_the_cap_go() {
        let (m, mut p) = memory();
        p.write_draft("Untitled", b"old", None, 1);
        let old = p.current.clone().unwrap();
        p.reset(0);
        m.clock.set(1_000_000.0 + STALE + 10.0);
        p.write_draft("Untitled", b"new", None, 1);
        let new = p.current.clone().unwrap();
        drop(p);
        let p = Projects::load(Box::new(m.clone()));
        assert!(p.get(&old).is_none(), "a draft over 30 days old goes, and its file-less project");
        assert!(p.get(&new).is_some());

        let (m, mut p) = memory();
        let big = vec![0u8; (CAP / 3) as usize];
        for k in 0..4 {
            m.clock.set(1_000_000.0 + k as f64);
            p.reset(0);
            p.write_draft("Untitled", &big, None, 1);
        }
        assert!(p.usage() <= CAP);
        assert_eq!(p.list().len(), 3, "the oldest draft went");
        assert_eq!(p.list()[0].used, 1_000_003.0);
    }

    #[test]
    fn a_copy_the_library_refused_stays_when_the_store_is_full() {
        let (m, mut p) = memory();
        let big = vec![0u8; (CAP / 2) as usize];
        let add = |p: &mut Projects, at: f64, held: Option<bool>| {
            m.clock.set(at);
            p.reset(0);
            let id = p.ensure("Untitled");
            let cloud = held.map(|held| Cloud {
                user: "u1".into(),
                id: id.clone(),
                version: Some("v1".into()),
                latest: Some("v1".into()),
                dirty: false,
                held,
            });
            p.update(&id, |p| p.cloud = cloud);
            p.keep_copy(&id, &big).unwrap();
            id
        };
        let held = add(&mut p, 1_000_000.0, Some(true));
        let synced = add(&mut p, 1_000_001.0, Some(false));
        add(&mut p, 1_000_002.0, None);
        assert!(p.usage() <= CAP);
        assert_eq!(p.get(&held).map(|p| p.copy), Some(big.len() as u64), "the only copy of its edits stays");
        let synced = p.get(&synced).expect("listed, to download again");
        assert_eq!(synced.copy, 0, "a copy the library holds goes first");
    }

    #[test]
    fn clear_keeps_only_the_open_scene() {
        let (_, mut p) = memory();
        p.write_draft("Untitled", b"a", None, 1);
        p.reset(0);
        p.opened("b.vox", Some(Path::new("/home/user/b.vox")), None, None, false, 0);
        p.clear();
        assert_eq!(p.list().len(), 1);
        assert_eq!(p.list()[0].name, "b.vox");
    }

    #[test]
    fn a_folder_store_keeps_files_and_keeps_the_draft_of_one_gone() {
        let dir = std::env::temp_dir().join(format!("vxr-projects-test-{}", std::process::id()));
        _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let file = dir.join("scene.vox");
        std::fs::write(&file, b"x").unwrap();
        let store = dir.join("projects");
        let mut p = Projects::load(Box::new(Folder::new(store.clone())));
        let m = modified(&file);
        let id = p.opened("scene.vox", Some(&file), None, m, false, 0);
        p.write_draft("scene.vox", b"draft", Some(b"png"), 1);
        assert!(store.join(draft_key(&id)).exists());
        assert!(store.join(record_key(&id)).exists() && store.join(DEVICE).exists());
        assert_ne!(Folder::new(dir.clone()).new_id(), Folder::new(dir.clone()).new_id());
        // Another window sees this one hold the project, until it closes.
        assert!(Folder::new(store.clone()).held(&id));
        drop(p);
        assert!(!Folder::new(store.clone()).held(&id));
        // The file moved (or its drive is not mounted): the unsaved edits stay, as a scene
        // with no file.
        std::fs::remove_file(&file).unwrap();
        let mut p = Projects::load(Box::new(Folder::new(store.clone())));
        let kept = p.get(&id).expect("a project whose file is gone keeps its draft");
        assert!(kept.path.is_none() && !kept.has_file() && kept.draft.is_some());
        assert!(store.join(draft_key(&id)).exists());
        assert_eq!(p.launch_offer().map(|o| o.id), Some(id.clone()));
        _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn two_windows_keep_each_others_projects() {
        let (m, mut one) = memory();
        // The second window started before the first drafted anything.
        let mut two = Projects::load(Box::new(m.another()));
        let a = one.opened("one.vox", Some(Path::new("/home/user/one.vox")), None, Some(1.0), false, 0);
        one.write_draft("one.vox", b"one", None, 1);
        let b = two.opened("two.vox", Some(Path::new("/home/user/two.vox")), None, Some(1.0), false, 0);
        two.write_draft("two.vox", b"two", None, 1);
        let three = Projects::load(Box::new(m.another()));
        let mut names: Vec<&str> = three.list().iter().map(|p| p.name.as_str()).collect();
        names.sort();
        assert_eq!(names, ["one.vox", "two.vox"], "neither window's write drops the other's project");
        assert!(three.get(&a).unwrap().draft.is_some(), "the first window's draft stays listed");
        // Clear in the second window leaves the project the first has open.
        two.clear();
        assert!(two.get(&a).is_some() && m.files.borrow().contains_key(&draft_key(&a)));
        // The first window moves on: its old project is no longer held, and Clear takes it.
        one.reset(0);
        two.clear();
        assert!(two.get(&a).is_none() && two.get(&b).is_some());
        // The first window hears of it when it reads the list again.
        one.refresh();
        assert_eq!(one.list().iter().map(|p| p.id.as_str()).collect::<Vec<_>>(), [b.as_str()]);
        // A tab hears of another's change through its page instead.
        let mut tab = Projects::load(Box::new(m.another()));
        let mut moved = two.get(&b).unwrap().clone();
        moved.name = "renamed.vox".into();
        tab.listed(&record_key(&b), Some(serde_json::to_vec(&moved).unwrap()));
        assert_eq!(tab.get(&b).unwrap().name, "renamed.vox");
        tab.listed(&record_key(&b), None);
        assert!(tab.list().is_empty());
    }

    #[test]
    fn the_cap_takes_thumbnails_and_copies_before_drafts() {
        let (m, mut p) = memory();
        // An old scene never saved: its draft is the only copy of its edits.
        p.write_draft("Untitled", b"old", Some(&vec![0; 1000]), 1);
        let old = p.current.clone().unwrap();
        // A downloaded file's copy fills the store.
        m.clock.set(1_000_001.0);
        let copy = p.opened("big.vox", None, Some("c"), None, false, CAP - 500);
        m.clock.set(1_000_002.0);
        p.reset(0);
        p.write_draft("Untitled", &vec![0; 2000], None, 1);
        assert!(p.usage() <= CAP);
        assert!(p.get(&old).is_some_and(|o| o.draft.is_some() && o.thumb == 0), "the thumbnail went, the draft stayed");
        assert!(p.get(&copy).is_none(), "the copy went first");
        assert!(p.evicted.is_empty());

        // Only drafts left: the oldest goes, and the editor hears its name; the one another
        // window has open stays.
        let (m, mut p) = memory();
        let big = vec![0u8; (CAP / 3) as usize];
        p.write_draft("first", &big, None, 1);
        let mut other = Projects::load(Box::new(m.another()));
        m.clock.set(1_000_001.0);
        other.write_draft("open elsewhere", &big, None, 1);
        for (k, name) in ["third", "fourth"].iter().enumerate() {
            m.clock.set(1_000_002.0 + k as f64);
            p.reset(0);
            p.write_draft(name, &big, None, 1);
        }
        let names: Vec<&str> = p.list().iter().map(|p| p.name.as_str()).collect();
        assert_eq!(names, ["fourth", "third", "open elsewhere"]);
        assert_eq!(p.evicted, ["first"]);
    }

    #[test]
    fn a_copy_is_found_again_only_with_the_same_bytes() {
        let (_, mut p) = memory();
        let id = p.opened("scene.vox", None, Some("a"), None, false, 3);
        p.set_source(&id, content_hash(b"one"));
        assert_eq!(p.copy_of("scene.vox", content_hash(b"one")), Some(id));
        assert_eq!(p.copy_of("scene.vox", content_hash(b"two")), None, "another file of the same name");
        assert_eq!(p.copy_of("other.vox", content_hash(b"one")), None);
    }

    #[test]
    fn times_and_sizes_read_plainly() {
        assert_eq!(ago(1000.0, 990.0), "just now");
        assert_eq!(ago(10_000.0, 10_000.0 - 300.0), "5 min ago");
        assert_eq!(ago(100_000.0, 100_000.0 - 3700.0), "1 hour ago");
        assert_eq!(ago(1e6, 1e6 - 1.5 * 86_400.0), "yesterday");
        assert_eq!(ago(1e7, 1e7 - 3.0 * 86_400.0), "3 days ago");
        assert_eq!(ago(1e8, 1e8 - 21.0 * 86_400.0), "3 weeks ago");
        assert_eq!(bytes(1000), "1 KB");
        assert_eq!(bytes(1_300_000), "1.2 MB");
        assert_eq!(bytes(200 << 20), "200 MB");
    }
}
