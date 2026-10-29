//! One request's staged change on the live scene (`docs/research/ai.md`, "Keeping a change
//! reviewable"). Every edit runs inside one open edit of the [`Scene`]: added and repainted
//! cells go into the map at once, so the view and the render corner show the after state;
//! removed cells stay in the map until [`Session::apply`] and are kept in a set, so a ghost
//! can draw what goes, and every query treats them as gone. Apply closes the edit as one undo
//! step, [`Session::discard`] puts everything back and leaves none. Each call is a step of the
//! review card; dropping one replays the others from the scene as it was.
//!
//! While a session is open nothing else may edit the scene: an undo or a commit from the
//! editor would close the open edit with the staged cells in it.

use std::collections::{HashMap, HashSet};

use serde_json::Value;
use vxr_core::history::At;
use vxr_core::material::{Kind, Material};
use vxr_core::scene::MAX_EXTENT;
use vxr_core::select::{Clip, bounds};
use vxr_core::{Scene, World};
use vxr_vox::chunk::{NOTE, Note};
use vxr_vox::raw::RawFile;
use vxr_vox::tree::{Kind as NodeKind, Tree, TreeNode};

use crate::args::*;
use crate::look::Camera;
use crate::parts::{self, Part};
use crate::text::{clean, hex, parse_hex};

/// Limits per request (`docs/research/ai.md` section 5, and per turn from test 1).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Limits {
    /// Calls the model may make in one turn (one answer, its calls run in parallel).
    pub calls_per_turn: usize,
    /// Calls in the whole request. `done` is always allowed.
    pub calls_per_request: usize,
    /// Cells one `place`, `erase` or `get_voxels` call may list.
    pub cells_per_call: usize,
    /// Cells one `box` may span.
    pub box_cells: i64,
    /// Changed cells the request may stage.
    pub changed_cells: usize,
    /// Over this many staged cells the user is asked before more are staged.
    pub ask_over: usize,
    /// Count `changed_cells` and `ask_over` per call, in the cells each call changes, rather
    /// than across everything staged.
    pub per_call: bool,
}

impl Default for Limits {
    fn default() -> Limits {
        Limits {
            calls_per_turn: 16,
            calls_per_request: 60,
            cells_per_call: 4096,
            box_cells: 2_000_000,
            changed_cells: 200_000,
            ask_over: 10_000,
            per_call: false,
        }
    }
}

const MAX_RADIUS: i32 = 64;
const MAX_LINE: i32 = 4096;
const WORLD: &str = "the scene shows the world of several objects, where voxels cannot be edited; the user opens one object's model (Tab) to edit it";
const NO_WORLD: &str =
    "the scene is one model, not a world of several objects: the world tools work on a file with several objects";
const IN_MODEL: &str = "the scene shows one object's model: the world tools work on the world of objects, which show_world (or the user's Tab) shows";
/// Longest layer name a call may give.
const MAX_LAYER_NAME: usize = 64;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ErrorKind {
    /// The call is wrong (bad arguments, nothing selected, an unknown part): the model can fix it.
    Invalid,
    /// A per-turn or per-request limit: the call was not run.
    Limit,
    /// The call would stage more than [`Session::allowed`] cells: not run until the user
    /// agrees ([`Session::approve_more`]).
    NeedsApproval,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Error {
    pub kind: ErrorKind,
    pub message: String,
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.message)
    }
}

pub(crate) fn invalid(m: impl Into<String>) -> Error {
    Error { kind: ErrorKind::Invalid, message: m.into() }
}

fn limit(m: impl Into<String>) -> Error {
    Error { kind: ErrorKind::Limit, message: m.into() }
}

/// One call that could change something, as the review card lists it.
#[derive(Clone, Debug)]
pub struct Step {
    pub tool: &'static str,
    pub args: Value,
    pub ok: bool,
    /// What the call answered (cut to 200 characters).
    pub result: String,
    /// Dropped from the change by the user or by the model's `undo`.
    pub dropped: bool,
    /// The lane that made it ([`Session::set_lane`]).
    pub lane: u64,
    /// The call to replay, for steps that change the scene or the selection.
    op: Option<Op>,
    /// The world nodes and layer the call names, as the replay finds them.
    refs: Refs,
    /// The world nodes and layers the call made when it last ran.
    made: Made,
}

/// A world node or layer a step names: one it found, or the `k`th one step `j` made.
#[derive(Clone, Copy, Debug)]
enum Ref<T> {
    Found(T),
    Made(usize, usize),
}

#[derive(Clone, Debug, Default)]
struct Refs {
    nodes: Vec<Ref<usize>>,
    layer: Option<Ref<i32>>,
}

#[derive(Clone, Debug, Default)]
struct Made {
    nodes: Vec<usize>,
    layers: Vec<i32>,
}

/// What a staged cell shows as.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Ghost {
    Added,
    Repainted,
    Removed,
}

/// The staged change against the scene as it was, in world cells (sorted), as the review
/// card counts it.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Changes {
    /// Cell and its new slot.
    pub added: Vec<(Cell, u8)>,
    /// Cell and the slot it held.
    pub removed: Vec<(Cell, u8)>,
    /// Cell, old slot, new slot.
    pub repainted: Vec<(Cell, u8, u8)>,
    /// Slot, old colour, new colour.
    pub palette: Vec<(u8, [u8; 4], [u8; 4])>,
    /// Slot, old material, new material.
    pub materials: Vec<(u8, Material, Material)>,
    /// World nodes (objects and groups) the change adds, takes out, moves, hides or puts on
    /// another layer: a world change has no cells.
    pub nodes: usize,
    /// Layers of the world the change adds, renames, hides or shows.
    pub layers: usize,
}

pub struct Session {
    pub limits: Limits,
    /// Map cells the change touched, with what they held before it.
    pub(crate) base: HashMap<Cell, u8>,
    /// Map cells the change removes; they still hold their old voxel in the map.
    pub(crate) removed: HashSet<Cell>,
    /// Cells whose value differs from `base`.
    staged: usize,
    pub(crate) palette_before: [[u8; 4]; 256],
    materials_before: [Material; 256],
    /// The world's objects and groups as the change found them.
    tree_before: Option<Tree>,
    /// The assistant's selection (map cells), seeded from the user's.
    pub(crate) selection: HashSet<Cell>,
    selection_before: HashSet<Cell>,
    /// What the assistant's copy or cut took, its `from` a world cell.
    clip: Option<Clip>,
    clip_before: Option<Clip>,
    /// The caller whose selection and clipboard these are ([`Session::set_lane`]), and every
    /// other caller's.
    lane: u64,
    lanes: HashMap<u64, Lane>,
    /// How far map cells moved since the session began, by resize and crop.
    shift: Cell,
    pub(crate) parts: Vec<Part>,
    /// How many of `parts` were found in the scene; the rest the model named.
    detected: usize,
    /// Every name an edit gave, in the order first given: name `i` is part `detected + i + 1`
    /// however the steps replay.
    names: Vec<String>,
    /// The file's `NOTE` palette row names, cleaned.
    pub(crate) notes: Vec<String>,
    steps: Vec<Step>,
    /// Steps the model's `undo` dropped, last last, for its `redo`.
    undone: Vec<usize>,
    calls: usize,
    turn_calls: usize,
    allowed: usize,
    /// Whether the scene's edit is open (the first edit opens it).
    open: bool,
    replaying: bool,
    done: Option<String>,
    /// Whether the scene shows the change's Before ([`Session::show_before`]).
    before: bool,
    /// The user's view, for `look` from it ([`Session::set_camera`]).
    pub(crate) camera: Option<Camera>,
    /// The last `look`'s picture, until taken ([`Session::take_image`]).
    pub(crate) image: Option<Vec<u8>>,
}

/// The `NOTE` chunk's names, cleaned, if the file has one.
pub fn notes_of(bytes: &[u8]) -> Vec<String> {
    let Ok(raw) = RawFile::parse(bytes) else { return Vec::new() };
    raw.chunks
        .iter()
        .filter(|c| c.is(NOTE))
        .filter_map(|c| Note::parse(&c.content).ok())
        .flat_map(|n| n.names.into_iter().map(|b| clean(&b)))
        .collect()
}

impl Session {
    /// A session on `scene` as it is now. `file` is the `.vox` it was opened from, for the
    /// palette notes. Nothing changes until the first edit.
    pub fn new(scene: &Scene, file: Option<&[u8]>) -> Session {
        Session::with_limits(scene, file, Limits::default())
    }

    pub fn with_limits(scene: &Scene, file: Option<&[u8]>, limits: Limits) -> Session {
        let parts = parts::detect(scene);
        let selection: HashSet<Cell> = scene.selection().collect();
        Session {
            limits,
            base: HashMap::new(),
            removed: HashSet::new(),
            staged: 0,
            palette_before: scene.palette,
            materials_before: scene.materials,
            tree_before: scene.world().map(|w| w.tree.clone()),
            selection_before: selection.clone(),
            clip: None,
            clip_before: None,
            shift: [0; 3],
            selection,
            lane: 0,
            lanes: HashMap::new(),
            detected: parts.len(),
            names: Vec::new(),
            parts,
            notes: file.map(notes_of).unwrap_or_default(),
            steps: Vec::new(),
            undone: Vec::new(),
            calls: 0,
            turn_calls: 0,
            allowed: limits.ask_over,
            open: false,
            replaying: false,
            done: None,
            before: false,
            camera: None,
            image: None,
        }
    }

    /// The assistant's selection as the change has it (map cells).
    pub fn selection(&self) -> impl Iterator<Item = Cell> + '_ {
        self.selection.iter().copied()
    }

    /// Start from this selection instead of the user's (map cells), before any call: a
    /// terminal agent keeps its own between calls so it never changes the user's.
    pub fn set_selection(&mut self, cells: impl IntoIterator<Item = Cell>) {
        self.selection = cells.into_iter().collect();
        self.selection_before = self.selection.clone();
    }

    /// Run the next calls for caller `lane`, with a selection of its own: several callers
    /// (terminal agents) stage one change, and each step keeps the lane that made it, so a
    /// dropped step or Apply replays every step with its own caller's selection. A lane's
    /// first call starts from `seed` (map cells); later ones from where it left off. The
    /// session starts in lane 0 with the user's selection.
    pub fn set_lane(&mut self, lane: u64, seed: impl IntoIterator<Item = Cell>) {
        if lane == self.lane {
            return;
        }
        let seed: HashSet<Cell> = seed.into_iter().collect();
        // As it was before a resize of the change moved the map, for a revert.
        let back = self.shift.map(|d| -d);
        let before = seed.iter().map(|&p| add(p, back)).collect();
        self.lanes.entry(lane).or_insert_with(|| Lane { selection_before: before, selection: seed, ..Lane::default() });
        self.switch(lane);
    }

    /// Make `lane`'s selection and clipboard the current ones (it must be in `lanes` unless
    /// it is current).
    fn switch(&mut self, lane: u64) {
        if lane == self.lane {
            return;
        }
        let l = self.lanes.remove(&lane).unwrap_or_default();
        let old = Lane {
            selection_before: std::mem::replace(&mut self.selection_before, l.selection_before),
            selection: std::mem::replace(&mut self.selection, l.selection),
            clip_before: std::mem::replace(&mut self.clip_before, l.clip_before),
            clip: std::mem::replace(&mut self.clip, l.clip),
        };
        self.lanes.insert(self.lane, old);
        self.lane = lane;
    }

    /// The current lane ([`Session::set_lane`]).
    pub fn lane(&self) -> u64 {
        self.lane
    }

    /// What the assistant's copy or cut took (`from` is a world cell).
    pub fn clipboard(&self) -> Option<&Clip> {
        self.clip.as_ref()
    }

    /// Start from this clipboard, before any call: a terminal agent keeps its own between
    /// calls so it never changes the user's.
    pub fn set_clipboard(&mut self, clip: Option<Clip>) {
        self.clip = clip.clone();
        self.clip_before = clip;
    }

    /// A new model turn: the per-turn call count starts again.
    pub fn begin_turn(&mut self) {
        self.turn_calls = 0;
    }

    /// A new request on the same staged change (the review card's "Ask for changes"): the
    /// per-request call count starts again; what is staged and the cell limits stay.
    pub fn begin_request(&mut self) {
        self.calls = 0;
        self.turn_calls = 0;
    }

    /// Run one tool call by name with its JSON input, counting it against the limits. Calls
    /// that can change something become steps.
    pub fn call(&mut self, scene: &mut Scene, tool: &str, args: Value) -> Result<String, Error> {
        let parsed = Op::parse(tool, args.clone());
        if !matches!(parsed, Ok(Op::Done(_))) {
            if self.turn_calls >= self.limits.calls_per_turn {
                return Err(limit(format!(
                    "at most {} calls in one turn: this call was not run; send it again in your next turn",
                    self.limits.calls_per_turn
                )));
            }
            if self.calls >= self.limits.calls_per_request {
                return Err(limit(format!(
                    "the limit of {} tool calls for this request is reached: call done now",
                    self.limits.calls_per_request
                )));
            }
        }
        self.calls += 1;
        self.turn_calls += 1;
        let op = parsed.map_err(|e| invalid(format!("{tool}: {e}")))?;
        let replay = matches!(
            op,
            Op::Place(_)
                | Op::Erase(_)
                | Op::Box(_)
                | Op::Line(_)
                | Op::Sphere(_)
                | Op::Circle(_)
                | Op::Face(_)
                | Op::Fill(_)
                | Op::Paint(_)
                | Op::Select(_)
                | Op::Transform(_)
                | Op::Copy(_)
                | Op::Cut(_)
                | Op::Paste(_)
                | Op::Delete(_)
                | Op::Hollow(_)
                | Op::FillInside(_)
                | Op::Resize(_)
                | Op::Crop(_)
                | Op::PaletteSet(_)
                | Op::PaletteSwap(_)
                | Op::PaletteCopy(_)
                | Op::MaterialSet(_)
                | Op::MoveObject(_)
                | Op::Duplicate(_)
                | Op::Group(_)
                | Op::Ungroup(_)
                | Op::Hide(_)
                | Op::SetLayer(_)
                | Op::DeleteObject(_)
                | Op::Rename(_)
                | Op::NewLayer(_)
                | Op::HideLayer(_)
        );
        let query = op.is_query();
        self.show_before(scene, false);
        let edit = !matches!(op, Op::Select(_) | Op::Copy(_));
        let refs = self.refer(&mut op.clone());
        let mark = Mark::of(scene);
        let r = self.run(scene, op.clone());
        if matches!(&r, Err(e) if e.kind == ErrorKind::NeedsApproval) {
            // Not run: the same call comes again once the user answers.
            self.calls -= 1;
            self.turn_calls -= 1;
            return r;
        }
        if !query {
            let text = match &r {
                Ok(t) => t,
                Err(e) => &e.message,
            };
            self.steps.push(Step {
                tool: op.name(),
                args,
                ok: r.is_ok(),
                result: text.chars().take(200).collect(),
                dropped: false,
                lane: self.lane,
                op: (replay && r.is_ok()).then_some(op),
                refs,
                made: if replay && r.is_ok() { mark.made(scene) } else { Made::default() },
            });
            // A new edit of this caller's: what its undo took back cannot come back.
            if replay && edit && self.steps.last().is_some_and(|s| s.ok) {
                let (steps, lane) = (&self.steps, self.lane);
                self.undone.retain(|&j| steps[j].lane != lane);
            }
        }
        r
    }

    /// Run one operation, without the call limits.
    pub fn run(&mut self, scene: &mut Scene, op: Op) -> Result<String, Error> {
        self.show_before(scene, false);
        match op {
            Op::SceneSummary(_) => Ok(self.summary(scene)),
            Op::GetVoxels(a) => self.get_voxels(scene, &a),
            Op::Slice(a) => self.slice(scene, &a),
            Op::FindParts(a) => Ok(self.find_parts(scene, a.query.as_deref())),
            Op::Look(a) => self.look(scene, &a),
            Op::Place(a) => {
                self.list_limit(a.cells.len())?;
                let slot = slot(a.slot)?;
                self.edit(scene, a.cells, Mode::Add, slot, a.name.as_deref())
            }
            Op::Erase(a) => {
                let cells = match (self.parts_cells(scene, &a.part, &a.parts)?, a.cells.is_empty()) {
                    (Some(_), false) => return Err(invalid("give cells or parts, not both")),
                    (Some(cells), true) => cells,
                    (None, _) => {
                        self.list_limit(a.cells.len())?;
                        a.cells
                    }
                };
                self.edit(scene, cells, Mode::Erase, 0, None)
            }
            Op::Box(a) => {
                let slot = shape_slot(a.mode, a.slot)?;
                let (lo, hi) = order(a.from, a.to);
                let volume = span(lo, hi);
                if volume > self.limits.box_cells {
                    return Err(invalid(format!("the box spans {volume} cells; at most {}", self.limits.box_cells)));
                }
                let mut cells = Vec::new();
                for z in lo[2]..=hi[2] {
                    for y in lo[1]..=hi[1] {
                        for x in lo[0]..=hi[0] {
                            let p = [x, y, z];
                            if !a.hollow || (0..3).any(|k| p[k] == lo[k] || p[k] == hi[k]) {
                                cells.push(p);
                            }
                        }
                    }
                }
                self.edit(scene, cells, a.mode, slot, a.name.as_deref())
            }
            Op::Line(a) => {
                let slot = shape_slot(a.mode, a.slot)?;
                let steps = (0..3).map(|k| (a.to[k] as i64 - a.from[k] as i64).abs()).max().unwrap_or(0);
                if steps > MAX_LINE as i64 {
                    return Err(invalid(format!("the line is over {MAX_LINE} cells")));
                }
                // As the line brush: equal steps, rounded.
                let (f, t) = (a.from, a.to);
                let cells: Vec<Cell> = (0..=steps)
                    .map(|s| {
                        if steps == 0 {
                            return f;
                        }
                        std::array::from_fn(|k| {
                            (f[k] as f64 + (t[k] as f64 - f[k] as f64) * s as f64 / steps as f64).round() as i32
                        })
                    })
                    .collect();
                self.edit(scene, cells, a.mode, slot, a.name.as_deref())
            }
            Op::Sphere(a) => {
                let slot = shape_slot(a.mode, a.slot)?;
                let cells = round(a.center, a.radius, a.flat_axis)?;
                self.edit(scene, cells, a.mode, slot, a.name.as_deref())
            }
            Op::Circle(a) => {
                let slot = shape_slot(a.mode, a.slot)?;
                let cells = round(a.center, a.radius, Some(a.axis.unwrap_or(Axis::Y)))?;
                self.edit(scene, cells, a.mode, slot, a.name.as_deref())
            }
            Op::Face(a) => {
                let slot = shape_slot(a.mode, a.slot)?;
                let side = a.side.unwrap_or(Side::Top);
                let n = side.normal();
                let face = self.face(scene, a.cell, side)?;
                // Add builds on the open side; erase and paint work on the face's own voxels.
                let d = if a.mode == Mode::Add { n } else { [0; 3] };
                let cells = face.into_iter().map(|p| add(scene.to_world(p), d)).collect();
                self.edit(scene, cells, a.mode, slot, a.name.as_deref())
            }
            Op::Fill(a) => {
                let slot = slot(a.slot)?;
                let cells = self.selected(scene);
                if cells.is_empty() {
                    return Err(invalid("nothing is selected: call select first"));
                }
                self.edit(scene, cells, Mode::Paint, slot, None)
            }
            Op::Paint(a) => {
                let slot = slot(a.slot)?;
                let cells: Vec<Cell> = if let Some(cells) = self.parts_cells(scene, &a.part, &a.parts)? {
                    cells
                } else if let Some(from) = a.from_slot {
                    let from = self::slot(from)?;
                    self.voxels_where(scene, |_, v| v == from)
                } else if a.selection {
                    self.selected(scene)
                } else if let (Some(f), Some(t)) = (a.from, a.to) {
                    let (lo, hi) = order(f, t);
                    self.voxels_where(scene, |q, _| inside(q, lo, hi))
                } else {
                    return Err(invalid("give part or parts, from_slot, selection: true, or a box (from, to)"));
                };
                self.edit(scene, cells, Mode::Paint, slot, None)
            }
            Op::Select(a) => self.select(scene, &a),
            Op::Transform(a) => self.transform(scene, &a),
            Op::Copy(_) => {
                let c = self.clip(scene)?;
                let s = format!(
                    "copied {} voxels, a {} x {} x {} box from {:?}",
                    c.voxels.len(),
                    c.size[0],
                    c.size[1],
                    c.size[2],
                    c.from
                );
                self.clip = Some(c);
                Ok(s)
            }
            Op::Cut(_) => {
                let clip = self.clip(scene)?;
                let st = self.delete(scene)?;
                let n = clip.voxels.len();
                self.clip = Some(clip);
                Ok(format!("cut {n} voxels; {}", self.stage_text(scene, &st)))
            }
            Op::Paste(a) => {
                let Some(clip) = self.clip.clone() else {
                    return Err(invalid("the clipboard is empty: copy or cut first"));
                };
                let corner = clip.corner(a.at);
                let cells = clip.voxels.iter().map(|&(d, v)| (std::array::from_fn(|k| corner[k] + d[k]), v));
                let st = self.stage(scene, cells.collect(), false)?;
                self.selection = st.made.iter().copied().collect();
                Ok(format!("pasted at {corner:?} (low corner); {}", self.stage_text(scene, &st)))
            }
            Op::Delete(_) => {
                let st = self.delete(scene)?;
                Ok(self.stage_text(scene, &st))
            }
            Op::Hollow(_) => {
                let solid: Vec<Cell> = self.subject(scene).0.into_iter().map(|c| c.0).collect();
                let inner = vxr_core::model::inner(&solid).into_iter().map(|p| (scene.to_world(p), 0)).collect();
                let st = self.stage(scene, inner, false)?;
                Ok(self.stage_text(scene, &st))
            }
            Op::FillInside(a) => {
                let slot = slot(a.slot)?;
                let solid: Vec<Cell> = self.subject(scene).0.into_iter().map(|c| c.0).collect();
                let holes = vxr_core::model::enclosed(&solid)
                    .into_iter()
                    .filter(|&p| self.get(scene, p) == 0)
                    .map(|p| (scene.to_world(p), slot))
                    .collect();
                let st = self.stage(scene, holes, false)?;
                Ok(self.stage_text(scene, &st))
            }
            Op::Resize(a) => {
                if a.size.iter().any(|&n| n == 0 || n > MAX_EXTENT) {
                    return Err(invalid(format!("size {:?}: each side is 1 to {MAX_EXTENT}", a.size)));
                }
                self.rebound(scene, scene.resized_lo(a.size), a.size)
            }
            Op::Crop(_) => {
                let mut cells = Vec::new();
                self.for_each(scene, |p, _| cells.push(p));
                let Some((lo, hi)) = bounds(cells) else {
                    return Err(invalid("the model is empty: there is nothing to crop to"));
                };
                self.rebound(scene, scene.to_world(lo), std::array::from_fn(|k| (hi[k] - lo[k] + 1) as u32))
            }
            Op::MaterialSet(a) => self.material_set(scene, &a),
            Op::MoveObject(a) => {
                let what = self.node(scene, a.node)?;
                let t = self.world_edit(scene, |w| w.move_by(a.node, a.offset))?.1;
                Ok(format!("moved {what} by {:?}{}; {t}", a.offset, self.extent_text(scene, a.node)))
            }
            Op::Duplicate(a) => {
                let what = self.node(scene, a.node)?;
                let (c, t) = self.world_edit(scene, |w| w.duplicate(a.node, a.linked))?;
                let c = c.ok_or_else(|| invalid(format!("node {}: the root cannot be copied", a.node)))?;
                let copy = node_text(scene.world().expect("a world"), c);
                let group = matches!(scene.world().expect("a world").tree.nodes[c].kind, NodeKind::Group(_));
                let how = match (a.linked, group) {
                    (true, false) => "a linked copy, sharing its model",
                    (true, true) => "linked copies, sharing their models",
                    (false, false) => "its own model",
                    (false, true) => "each with its own model",
                };
                Ok(format!("copied {what} as {copy} ({how}){}; {t}", self.extent_text(scene, c)))
            }
            Op::Group(a) => {
                let mut nodes = Vec::new();
                for &n in &a.nodes {
                    self.node(scene, n)?;
                    if !nodes.contains(&n) {
                        nodes.push(n);
                    }
                }
                if nodes.is_empty() {
                    return Err(invalid("nodes: give the objects or groups to group"));
                }
                let (g, t) = self.world_edit(scene, |w| w.group(&nodes))?;
                let g = g.ok_or_else(|| {
                    invalid("the nodes do not share a parent: only nodes in the same group are grouped together")
                })?;
                Ok(format!(
                    "grouped {} nodes {:?} into {}; {t}",
                    nodes.len(),
                    nodes,
                    node_text(scene.world().expect("a world"), g)
                ))
            }
            Op::Ungroup(a) => {
                let what = self.node(scene, a.node)?;
                let w = scene.world().expect("a world");
                let NodeKind::Group(kids) = &w.tree.nodes[a.node].kind else {
                    return Err(invalid(format!("node {}: {what} is not a group", a.node)));
                };
                let kids = kids.clone();
                let parent = w.tree.parents()[a.node].expect("a node in the tree has a parent");
                let t = self.world_edit(scene, |w| w.ungroup(a.node))?.1;
                let into = if parent == 0 { "the top of the world".to_string() } else { format!("group {parent}") };
                Ok(format!("ungrouped {what}: nodes {kids:?} moved up into {into}; {t}"))
            }
            Op::Hide(a) => {
                let what = self.node(scene, a.node)?;
                let t = self.world_edit(scene, |w| w.set_hidden(a.node, a.hidden))?.1;
                let w = scene.world().expect("a world");
                let mut s = format!("{what} is {}; {t}", if a.hidden { "hidden" } else { "shown" });
                if !a.hidden && !w.shown(a.node) {
                    s += "; warning: it still does not show: its layer or a group it is in is hidden";
                }
                Ok(s)
            }
            Op::SetLayer(a) => self.set_layer(scene, &a),
            Op::DeleteObject(a) => {
                let what = self.node(scene, a.node)?;
                let t = self.world_edit(scene, |w| w.remove(a.node))?.1;
                Ok(format!("deleted {what}; {t}"))
            }
            Op::Rename(a) => {
                let name = a.name.chars().filter(|c| !c.is_control()).collect::<String>();
                let name = name.trim();
                if name.chars().count() > MAX_LAYER_NAME {
                    return Err(invalid(format!("name: at most {MAX_LAYER_NAME} characters")));
                }
                let new = (!name.is_empty()).then_some(name);
                let to = new.map_or("no name".to_string(), |n| Value::from(n).to_string());
                match (a.node, a.layer) {
                    (Some(n), None) => {
                        let what = self.node(scene, n)?;
                        let t = self.world_edit(scene, |w| w.set_name(n, new))?.1;
                        Ok(format!("{what} is named {to}; {t}"))
                    }
                    (None, Some(id)) => {
                        self.layer(scene, id)?;
                        let t = self.world_edit(scene, |w| w.set_layer_name(id, new))?.1;
                        Ok(format!("layer {id} is named {to}; {t}"))
                    }
                    _ => Err(invalid("give node (an object or group) or layer (a layer id), not both")),
                }
            }
            Op::NewLayer(a) => {
                self.world_here(scene)?;
                let name = match a.name {
                    Some(n) => n.chars().filter(|c| !c.is_control()).collect::<String>().trim().to_string(),
                    None => {
                        let w = scene.world().expect("a world");
                        format!("layer {}", w.layers().iter().map(|l| l.id + 1).max().unwrap_or(0))
                    }
                };
                if name.is_empty() || name.chars().count() > MAX_LAYER_NAME {
                    return Err(invalid(format!("name: 1 to {MAX_LAYER_NAME} characters")));
                }
                let (id, t) = self.world_edit(scene, |w| w.add_layer(&name))?;
                Ok(format!("added layer {id} {}; {t}", Value::from(name)))
            }
            Op::HideLayer(a) => {
                self.layer(scene, a.layer)?;
                let t = self.world_edit(scene, |w| w.set_layer_hidden(a.layer, a.hidden))?.1;
                let w = scene.world().expect("a world");
                let n = w.objects().into_iter().filter(|&i| w.tree.nodes[i].layer == a.layer).count();
                let state = if a.hidden { "hidden" } else { "shown" };
                Ok(format!("layer {} is {state} ({n} object{} on it); {t}", a.layer, if n == 1 { "" } else { "s" }))
            }
            Op::PaletteSet(a) => {
                let s = slot(a.slot)?;
                let c = parse_hex(&a.hex).map_err(invalid)?;
                self.open(scene);
                let old = scene.palette[s as usize];
                scene.put_color(s, c);
                Ok(format!("slot {s}: {} -> {} ({} voxels use it)", hex(old), hex(c), self.slot_count(scene, s)))
            }
            Op::PaletteSwap(a) => {
                let (x, y) = (slot(a.a)?, slot(a.b)?);
                if x == y {
                    return Err(invalid(format!("a and b are both slot {x}: give two slots")));
                }
                let cells = self.voxels_of(scene, |v| v == x || v == y);
                let remap = cells.into_iter().map(|(w, v)| (w, if v == x { y } else { x })).collect();
                let st = self.stage(scene, remap, true)?;
                self.open(scene);
                let (cx, cy) = (scene.palette[x as usize], scene.palette[y as usize]);
                let (mx, my) = (scene.materials[x as usize], scene.materials[y as usize]);
                scene.put_color(x, cy);
                scene.put_color(y, cx);
                scene.put_material(x, my);
                scene.put_material(y, mx);
                let mut t = format!(
                    "swapped slots {x} and {y}: slot {x} is now {} ({} voxels), slot {y} is now {} ({} voxels); {}",
                    hex(cy),
                    self.slot_count(scene, x),
                    hex(cx),
                    self.slot_count(scene, y),
                    self.staged_text(scene)
                );
                if st.outside > 0 {
                    t += &self.warning(scene, &st);
                }
                if scene.world().is_some_and(|w| w.objects().len() > 1) {
                    t += "; warning: only this model's voxels took the other slot: other objects' voxels of these slots now show the other colour";
                }
                Ok(t)
            }
            Op::PaletteCopy(a) => {
                let (from, to) = (slot(a.from)?, slot(a.to)?);
                if from == to {
                    return Err(invalid(format!("from and to are both slot {from}: give two slots")));
                }
                self.open(scene);
                let (c, m) = (scene.palette[from as usize], scene.materials[from as usize]);
                scene.put_color(to, c);
                scene.put_material(to, m);
                Ok(format!(
                    "slot {to} now has slot {from}'s colour {} and material {}{} ({} voxels use it)",
                    hex(c),
                    m.kind.name(),
                    settings(&m),
                    self.slot_count(scene, to)
                ))
            }
            Op::Undo(_) => {
                let Some(i) = self.steps.iter().rposition(|s| {
                    !s.dropped
                        && s.lane == self.lane
                        && s.op.as_ref().is_some_and(|o| !matches!(o, Op::Select(_) | Op::Copy(_)))
                }) else {
                    return Err(invalid("nothing of yours to undo"));
                };
                self.steps[i].dropped = true;
                self.undone.push(i);
                self.rebuild(scene);
                Ok(format!("took back step {} ({}); {}", i + 1, self.steps[i].tool, self.staged_text(scene)))
            }
            Op::Redo(_) => {
                let (steps, lane) = (&self.steps, self.lane);
                let Some(k) = self.undone.iter().rposition(|&j| steps[j].lane == lane && steps[j].dropped) else {
                    return Err(invalid(
                        "nothing of yours to redo: redo brings back what your undo took back, until your next edit",
                    ));
                };
                let i = self.undone.remove(k);
                self.steps[i].dropped = false;
                self.rebuild(scene);
                Ok(format!("brought back step {} ({}); {}", i + 1, self.steps[i].tool, self.staged_text(scene)))
            }
            Op::Done(a) => {
                let s: String = a.summary.chars().filter(|c| !c.is_control()).take(500).collect();
                self.done = Some(s);
                Ok("noted; the change goes to the user for review".into())
            }
        }
    }

    /// The model's one-line summary, once it called `done`.
    pub fn done(&self) -> Option<&str> {
        self.done.as_deref()
    }

    /// Every step, in order, the dropped ones included.
    pub fn steps(&self) -> &[Step] {
        &self.steps
    }

    /// Calls made in the request so far.
    pub fn calls(&self) -> usize {
        self.calls
    }

    /// How many changed cells may be staged before the user is asked again.
    pub fn allowed(&self) -> usize {
        self.allowed
    }

    /// The user agreed to a bigger change: stage up to the request's limit.
    pub fn approve_more(&mut self) {
        self.allowed = self.limits.changed_cells;
    }

    /// How many cells the change stages (added, removed and repainted).
    pub fn staged(&self) -> usize {
        self.staged
    }

    /// Drop step `i` from the change, or take it back (the review card's checkbox): the scene
    /// goes back to how it was and the steps still kept run again. Returns false for a step
    /// that changed nothing.
    pub fn set_dropped(&mut self, scene: &mut Scene, i: usize, dropped: bool) -> bool {
        self.show_before(scene, false);
        let Some(s) = self.steps.get_mut(i).filter(|s| s.op.is_some()) else { return false };
        if s.dropped != dropped {
            s.dropped = dropped;
            self.rebuild(scene);
        }
        true
    }

    /// Every staged cell in the map (map cells) and how it shows, for the ghost overlay.
    pub fn ghost(&self, scene: &Scene) -> Vec<(Cell, Ghost)> {
        let size = scene.map.size_voxels();
        let mut out = self.staged_cells(scene);
        out.retain(|(p, _)| (0..3).all(|k| p[k] >= 0 && p[k] < size[k] as i32));
        out
    }

    /// Every staged cell (map cells), those a resize left outside the map included.
    fn staged_cells(&self, scene: &Scene) -> Vec<(Cell, Ghost)> {
        let mut out: Vec<(Cell, Ghost)> = self
            .base
            .iter()
            .filter_map(|(&p, &b)| {
                let v = self.get(scene, p);
                (v != b).then_some((
                    p,
                    if b == 0 {
                        Ghost::Added
                    } else if v == 0 {
                        Ghost::Removed
                    } else {
                        Ghost::Repainted
                    },
                ))
            })
            .collect();
        out.sort_unstable_by_key(|c| c.0);
        out
    }

    /// The staged change in world cells, as the review card shows it.
    pub fn changes(&self, scene: &Scene) -> Changes {
        let mut c = Changes::default();
        for (p, g) in self.staged_cells(scene) {
            let w = scene.to_world(p);
            let (b, v) = (self.base[&p], self.get(scene, p));
            match g {
                Ghost::Added => c.added.push((w, v)),
                Ghost::Removed => c.removed.push((w, b)),
                Ghost::Repainted => c.repainted.push((w, b, v)),
            }
        }
        c.added.sort_unstable();
        c.removed.sort_unstable();
        c.repainted.sort_unstable();
        c.palette = (1..=255u8)
            .filter(|&i| scene.palette[i as usize] != self.palette_before[i as usize])
            .map(|i| (i, self.palette_before[i as usize], scene.palette[i as usize]))
            .collect();
        c.materials = (1..=255u8)
            .filter(|&i| scene.materials[i as usize] != self.materials_before[i as usize])
            .map(|i| (i, self.materials_before[i as usize], scene.materials[i as usize]))
            .collect();
        if let (Some(old), Some(w)) = (&self.tree_before, scene.world()) {
            c.nodes = changed_nodes(old, &w.tree);
            c.layers = changed_layers(old, &w.tree);
        }
        c
    }

    /// Apply: erase the removed cells and close the edit as one undo step. Returns how many
    /// voxels changed.
    pub fn apply(self, scene: &mut Scene) -> usize {
        self.apply_as(scene, None)
    }

    /// Apply as one undo step called `label` ("AI: make the roof red").
    pub fn apply_named(self, scene: &mut Scene, label: &str) -> usize {
        self.apply_as(scene, Some(label))
    }

    fn apply_as(mut self, scene: &mut Scene, label: Option<&str>) -> usize {
        self.show_before(scene, false);
        if !self.open {
            return 0;
        }
        for &p in &self.removed {
            scene.put(p, 0);
        }
        match label {
            Some(l) => scene.commit_named(l),
            None => scene.commit(),
        }
    }

    /// Apply each kept step as an undo step of its own, called `label(index, step, what it
    /// changed)` ("Claude Code: box 8 voxels"): the change runs again from the scene as it
    /// was, one step at a time, each with its own lane's selection. Returns each step that changed
    /// something, by index, with the history position right after it.
    pub fn apply_steps(
        mut self,
        scene: &mut Scene,
        mut label: impl FnMut(usize, &Step, &Changes) -> String,
    ) -> Vec<(usize, u64)> {
        self.show_before(scene, false);
        if !self.open {
            return Vec::new();
        }
        self.revert(scene);
        // Nothing is left in the open edit: closing it leaves no step.
        scene.commit();
        self.open = false;
        self.replaying = true;
        let me = self.lane;
        let mut out = Vec::new();
        for i in 0..self.steps.len() {
            if !self.replay(scene, i) || !self.open {
                continue;
            }
            let changes = self.changes(scene);
            for &p in &self.removed {
                scene.put(p, 0);
            }
            let before = scene.history().position();
            scene.commit_named(&label(i, &self.steps[i], &changes));
            if scene.history().position() != before {
                out.push((i, scene.history().position()));
            }
            // The next step is staged against the scene as this one left it.
            self.open = false;
            self.base.clear();
            self.removed.clear();
            self.staged = 0;
            self.palette_before = scene.palette;
            self.materials_before = scene.materials;
            self.tree_before = scene.world().map(|w| w.tree.clone());
        }
        self.replaying = false;
        self.switch(me);
        out
    }

    /// Show the scene as it was before the change (true) or with it again (false), for the
    /// review card's Before and After. Before puts every cell, colour, material and the bounds
    /// back inside the open edit; After runs the kept steps again, as dropping a step does. A
    /// call, a dropped step, Apply or Discard first shows the change again. Before shows no
    /// ghost and no changes.
    pub fn show_before(&mut self, scene: &mut Scene, on: bool) {
        if on == self.before {
            return;
        }
        self.before = on;
        if on {
            self.revert(scene);
        } else {
            self.rebuild(scene);
        }
    }

    /// Whether the scene shows the change's Before ([`Session::show_before`]).
    pub fn showing_before(&self) -> bool {
        self.before
    }

    /// Discard: put every cell and colour back and close the edit, leaving no undo step.
    pub fn discard(mut self, scene: &mut Scene) {
        self.show_before(scene, false);
        self.revert(scene);
        if self.open {
            scene.commit();
        }
    }

    /// A cell as the change has it: staged removals are empty.
    pub(crate) fn get(&self, scene: &Scene, p: Cell) -> u8 {
        if self.removed.contains(&p) { 0 } else { scene.map.get(p[0], p[1], p[2]) }
    }

    /// Every voxel as the change has it (map cells).
    pub(crate) fn for_each(&self, scene: &Scene, mut f: impl FnMut(Cell, u8)) {
        scene.for_each_voxel(|p, v| {
            if !self.removed.contains(&p) {
                f(p, v)
            }
        });
    }

    /// World cells of the voxels for which `keep(world cell, slot)` holds.
    fn voxels_where(&self, scene: &Scene, keep: impl Fn(Cell, u8) -> bool) -> Vec<Cell> {
        let mut out = Vec::new();
        self.for_each(scene, |p, v| {
            let w = scene.to_world(p);
            if keep(w, v) {
                out.push(w);
            }
        });
        out
    }

    pub(crate) fn slot_count(&self, scene: &Scene, s: u8) -> usize {
        let gone = self.removed.iter().filter(|p| scene.map.get(p[0], p[1], p[2]) == s).count();
        scene.map.counts[s as usize] as usize - gone
    }

    /// Voxels in the scene as the change has it.
    pub(crate) fn voxel_count(&self, scene: &Scene) -> usize {
        scene.map.voxels as usize - self.removed.len()
    }

    /// The selected voxels that still hold one, as world cells.
    fn selected(&self, scene: &Scene) -> Vec<Cell> {
        let mut out: Vec<Cell> =
            self.selection.iter().filter(|&&p| self.get(scene, p) != 0).map(|&p| scene.to_world(p)).collect();
        out.sort_unstable();
        out
    }

    fn list_limit(&self, n: usize) -> Result<(), Error> {
        if n > self.limits.cells_per_call {
            return Err(invalid(format!(
                "{n} cells; at most {} per call (use box, line or sphere)",
                self.limits.cells_per_call
            )));
        }
        Ok(())
    }

    fn open(&mut self, scene: &mut Scene) {
        if !self.open {
            scene.commit();
            scene.begin();
            self.open = true;
        }
    }

    /// Stage one map cell: removals keep their old voxel in the map.
    fn set(&mut self, scene: &mut Scene, p: Cell, v: u8) {
        let cur = self.get(scene, p);
        let b = *self.base.entry(p).or_insert_with(|| scene.map.get(p[0], p[1], p[2]));
        if v == 0 && b != 0 {
            scene.put(p, b);
            self.removed.insert(p);
        } else {
            self.removed.remove(&p);
            scene.put(p, v);
        }
        self.staged = self.staged + (v != b) as usize - (cur != b) as usize;
    }

    /// Apply `mode` to world cells as one step of the change.
    fn edit(
        &mut self,
        scene: &mut Scene,
        cells: Vec<Cell>,
        mode: Mode,
        slot: u8,
        name: Option<&str>,
    ) -> Result<String, Error> {
        let v = if mode == Mode::Erase { 0 } else { slot };
        let st = self.stage(scene, cells.into_iter().map(|q| (q, v)).collect(), mode == Mode::Paint)?;
        let mut s = format!("{} cells changed; {}", st.changed, self.staged_text(scene));
        if let Some(n) = name.map(|n| clean(n.as_bytes())).filter(|n| !n.is_empty() && !st.made.is_empty()) {
            let made = st.made.clone();
            let id = match self.parts[self.detected..].iter().position(|p| p.name.eq_ignore_ascii_case(&n)) {
                Some(k) => {
                    self.parts[self.detected + k].cells.extend(made);
                    self.parts[self.detected + k].id.clone()
                }
                None => {
                    let i = match self.names.iter().position(|m| m.eq_ignore_ascii_case(&n)) {
                        Some(i) => i,
                        None => {
                            self.names.push(n.clone());
                            self.names.len() - 1
                        }
                    };
                    let part = parts::named(self.detected + i + 1, &n, made);
                    let id = part.id.clone();
                    self.parts.push(part);
                    id
                }
            };
            s += &format!("; part {id} {}", Value::from(n));
        }
        s += &self.warning(scene, &st);
        Ok(s)
    }

    /// Stage world cells, each to its slot (0 erases), as one step of the change; with
    /// `paint` only cells that hold a voxel. Cells outside the bounds are skipped and counted;
    /// a cell listed twice keeps its first slot.
    fn stage(&mut self, scene: &mut Scene, cells: Vec<(Cell, u8)>, paint: bool) -> Result<Staged, Error> {
        if scene.at() == At::World {
            return Err(invalid(WORLD));
        }
        let size = scene.map.size_voxels();
        let (mut outside, mut seen, mut plan, mut made) = (0usize, HashSet::new(), Vec::new(), Vec::new());
        let mut delta = 0isize;
        for (q, v) in cells {
            let p = scene.to_map(q);
            if (0..3).any(|a| p[a] < 0 || p[a] >= size[a] as i32) {
                outside += 1;
                continue;
            }
            if !seen.insert(p) {
                continue;
            }
            let cur = self.get(scene, p);
            if paint && cur == 0 {
                continue;
            }
            if v != 0 {
                made.push(p);
            }
            if cur != v {
                let b = self.base.get(&p).copied().unwrap_or_else(|| scene.map.get(p[0], p[1], p[2]));
                delta += (v != b) as isize - (cur != b) as isize;
                plan.push((p, v));
            }
        }
        let (after, grows, per) = match self.limits.per_call {
            true => (plan.len(), !plan.is_empty(), "call"),
            false => ((self.staged as isize + delta).max(0) as usize, delta > 0, "request"),
        };
        if !self.replaying && grows {
            if after > self.limits.changed_cells {
                return Err(limit(format!(
                    "this would stage {after} changed cells; the limit is {} per {per}",
                    self.limits.changed_cells
                )));
            }
            if after > self.allowed {
                return Err(Error {
                    kind: ErrorKind::NeedsApproval,
                    message: format!(
                        "this would stage {after} changed cells; over {} the user is asked first",
                        self.allowed
                    ),
                });
            }
        }
        if !plan.is_empty() {
            self.open(scene);
        }
        for &(p, v) in &plan {
            self.set(scene, p, v);
        }
        Ok(Staged { changed: plan.len(), outside, made })
    }

    /// "12 cells changed; staged so far: ...", and a warning when it clipped or did nothing.
    fn stage_text(&self, scene: &Scene, st: &Staged) -> String {
        format!("{} cells changed; {}{}", st.changed, self.staged_text(scene), self.warning(scene, st))
    }

    fn warning(&self, scene: &Scene, st: &Staged) -> String {
        if st.outside > 0 {
            format!(
                "; warning: {} cells were outside the editable bounds ({}) and skipped",
                st.outside,
                self.bounds_text(scene)
            )
        } else if st.changed == 0 {
            "; warning: nothing changed".into()
        } else {
            String::new()
        }
    }

    /// The voxels a transform, hollow or fill inside works on (map cells and slots, sorted):
    /// the selection, or every voxel when nothing is selected; and whether it is the selection.
    fn subject(&self, scene: &Scene) -> (Vec<(Cell, u8)>, bool) {
        let mut out: Vec<(Cell, u8)> =
            self.selection.iter().map(|&p| (p, self.get(scene, p))).filter(|c| c.1 != 0).collect();
        let selected = !out.is_empty();
        if !selected {
            self.for_each(scene, |p, v| out.push((p, v)));
        }
        out.sort_unstable();
        (out, selected)
    }

    fn transform(&mut self, scene: &mut Scene, a: &Transform) -> Result<String, Error> {
        use vxr_core::select::Transform as T;
        let axis = || a.axis.map(Axis::index).ok_or_else(|| invalid("axis is needed for rotate, flip and loop"));
        let t = match a.kind {
            TransformKind::Move => T::Move(a.offset.ok_or_else(|| invalid("move needs offset [dx, dy, dz]"))?),
            TransformKind::Rotate => T::Rotate(axis()?),
            TransformKind::Flip => T::Flip(axis()?),
            TransformKind::Loop => T::Loop(axis()?, a.steps.unwrap_or(1)),
        };
        if scene.at() == At::World {
            return Err(invalid(WORLD));
        }
        let (cells, selected) = self.subject(scene);
        let Some((lo, hi)) = bounds(cells.iter().map(|c| c.0)) else {
            return Ok(format!(
                "0 cells changed; {}; warning: nothing changed: there are no voxels",
                self.staged_text(scene)
            ));
        };
        let size = scene.map.size_voxels();
        let out = cells
            .iter()
            .filter(|c| {
                let q = t.apply(c.0, lo, hi);
                (0..3).any(|k| q[k] < 0 || q[k] >= size[k] as i32)
            })
            .count();
        if out > 0 {
            return Err(invalid(format!(
                "{out} of {} voxels would land outside the editable bounds ({}): nothing moved; resize first, or move less",
                cells.len(),
                self.bounds_text(scene)
            )));
        }
        // Lift every voxel, then set each down where it goes.
        let mut plan: HashMap<Cell, u8> = cells.iter().map(|c| (c.0, 0)).collect();
        let mut moved = HashSet::with_capacity(cells.len());
        for &(p, v) in &cells {
            let q = t.apply(p, lo, hi);
            plan.insert(q, v);
            moved.insert(q);
        }
        let plan = plan.into_iter().map(|(p, v)| (scene.to_world(p), v)).collect();
        let (nlo, nhi) = bounds(moved.iter().copied()).expect("voxels moved");
        let st = self.stage(scene, plan, false)?;
        if selected {
            self.selection = moved;
        }
        let verb = match a.kind {
            TransformKind::Move => "moved",
            TransformKind::Rotate => "turned",
            TransformKind::Flip => "flipped",
            TransformKind::Loop => "looped",
        };
        Ok(format!(
            "{verb} {} voxels ({}), now from {:?} to {:?}; {}",
            cells.len(),
            if selected { "the selection" } else { "every voxel" },
            scene.to_world(nlo),
            scene.to_world(nhi),
            self.stage_text(scene, &st)
        ))
    }

    /// The selected voxels as a clip.
    fn clip(&self, scene: &Scene) -> Result<Clip, Error> {
        let (cells, selected) = self.subject(scene);
        let Some((lo, hi)) = bounds(cells.iter().map(|c| c.0)).filter(|_| selected) else {
            return Err(invalid("nothing is selected: call select first"));
        };
        let voxels = cells.iter().map(|&(p, v)| (std::array::from_fn(|k| p[k] - lo[k]), v)).collect();
        Ok(Clip { voxels, size: std::array::from_fn(|k| hi[k] - lo[k] + 1), from: scene.to_world(lo) })
    }

    /// Erase the selected voxels; the selection empties.
    fn delete(&mut self, scene: &mut Scene) -> Result<Staged, Error> {
        let cells = self.selected(scene);
        if cells.is_empty() {
            return Err(invalid("nothing is selected: call select first"));
        }
        let st = self.stage(scene, cells.into_iter().map(|q| (q, 0)).collect(), false)?;
        self.selection.clear();
        Ok(st)
    }

    /// Change the bounds to `size` from world cell `lo`, erasing the voxels outside.
    fn rebound(&mut self, scene: &mut Scene, lo: Cell, size: [u32; 3]) -> Result<String, Error> {
        if scene.at() == At::World {
            return Err(invalid(WORLD));
        }
        let was = scene.bounds();
        let inside = |w: Cell| (0..3).all(|k| w[k] >= lo[k] && w[k] < lo[k] + size[k] as i32);
        let outside = self.voxels_where(scene, |w, _| !inside(w));
        let st = self.stage(scene, outside.into_iter().map(|w| (w, 0)).collect(), false)?;
        self.open(scene);
        let (_, d) = scene
            .put_bounds(lo, size)
            .ok_or_else(|| invalid(format!("size {size:?}: each side is 1 to {MAX_EXTENT}")))?;
        if d != [0; 3] {
            let mv = |p: Cell| add(p, d);
            self.base = self.base.drain().map(|(p, b)| (mv(p), b)).collect();
            self.removed = self.removed.drain().map(mv).collect();
            self.selection = self.selection.drain().map(mv).collect();
            for l in self.lanes.values_mut() {
                l.selection = l.selection.drain().map(mv).collect();
            }
            for p in &mut self.parts {
                p.cells.iter_mut().for_each(|c| *c = mv(*c));
            }
            self.shift = add(self.shift, d);
        }
        // The erased cells outside the map are gone from it already: they read as empty.
        let size = scene.map.size_voxels();
        self.removed.retain(|p| (0..3).all(|k| p[k] >= 0 && p[k] < size[k] as i32));
        let b = scene.bounds();
        let mut s = format!(
            "bounds {} ({} x {} x {}); {} cells changed; {}",
            self.bounds_text(scene),
            b.size[0],
            b.size[1],
            b.size[2],
            st.changed,
            self.staged_text(scene)
        );
        if st.changed > 0 {
            s += &format!("; warning: {} voxels outside the new bounds were erased", st.changed);
        } else if (b.lo, b.size) == (was.lo, was.size) {
            s += "; warning: nothing changed";
        }
        Ok(s)
    }

    fn material_set(&mut self, scene: &mut Scene, a: &MaterialSet) -> Result<String, Error> {
        let s = slot(a.slot)?;
        let old = scene.materials[s as usize];
        let mut m = old;
        if let Some(k) = a.kind {
            m.kind = match k {
                MaterialKind::Diffuse => Kind::Diffuse,
                MaterialKind::Metal => Kind::Metal,
                MaterialKind::Plastic => Kind::Plastic,
                MaterialKind::Glass => Kind::Glass,
                MaterialKind::Emit => Kind::Emit,
            };
        }
        let sliders = [
            ("roughness", a.roughness, 0.0, 1.0, &mut m.rough),
            ("metalness", a.metalness, 0.0, 1.0, &mut m.metal),
            ("specular", a.specular, 0.0, 1.0, &mut m.spec),
            ("transparency", a.transparency, 0.0, 1.0, &mut m.trans),
            ("refraction", a.refraction, 1.0, 3.0, &mut m.ior),
            ("attenuation", a.attenuation, 0.0, 1.0, &mut m.att),
            ("emission", a.emission, 0.0, 1.0, &mut m.emit),
            ("power", a.power, 0.0, 4.0, &mut m.flux),
        ];
        for (name, v, lo, hi, field) in sliders {
            if let Some(v) = v {
                if !(lo..=hi).contains(&v) {
                    return Err(invalid(format!("{name} {v}: {lo} to {hi}")));
                }
                *field = v;
            }
        }
        if m != old {
            self.open(scene);
            scene.put_material(s, m);
        }
        let mut t = format!(
            "slot {s}: {} -> {}{}; {} voxels use it",
            old.kind.name(),
            m.kind.name(),
            settings(&m),
            self.slot_count(scene, s)
        );
        if m == old {
            t += "; warning: nothing changed";
        }
        Ok(t)
    }

    /// Node `i` of the world the scene shows, as text ("object 3", "group 2 (3 objects)"),
    /// or why a world tool cannot work on it.
    fn node(&self, scene: &Scene, i: usize) -> Result<String, Error> {
        let w = match scene.at() {
            At::World => scene.world().expect("world mode has a world"),
            At::Object(_) => return Err(invalid(IN_MODEL)),
            At::Map => return Err(invalid(NO_WORLD)),
        };
        if i == 0 {
            return Err(invalid(
                "node 0 is the root of the world: give an object or group from the summary's world list",
            ));
        }
        if !w.in_tree(i) {
            return Err(invalid(format!(
                "node {i}: not an object or group of this world; the summary's world list has them"
            )));
        }
        Ok(node_text(w, i))
    }

    /// Change the world as one step of the change, inside its open edit. Returns what `f`
    /// gave and "2 nodes changed ...; staged so far: ...", with a warning when nothing changed.
    fn world_edit<R>(&mut self, scene: &mut Scene, f: impl FnOnce(&mut World) -> R) -> Result<(R, String), Error> {
        self.open(scene);
        let old = scene.world().expect("a world").tree.clone();
        let r = scene.put_world(f).ok_or_else(|| {
            invalid(format!("nothing changed: the world would be over {MAX_EXTENT} voxels on an axis"))
        })?;
        // The map holds the world again: what shows picked is what shows selected.
        self.selection = scene.selection().collect();
        let tree = &scene.world().expect("a world").tree;
        let (n, l) = (changed_nodes(&old, tree), changed_layers(&old, tree));
        let all = self.tree_before.as_ref().map_or((0, 0), |b| (changed_nodes(b, tree), changed_layers(b, tree)));
        let mut t = format!(
            "{} changed in the world, no voxel of any model; staged so far: {} in the world{}",
            world_text(n, l),
            world_text(all.0, all.1),
            if self.base.is_empty() { String::new() } else { format!(", {}", self.staged_text(scene)) }
        );
        if n == 0 && l == 0 {
            t += "; warning: nothing changed";
        }
        Ok((r, t))
    }

    /// ", now from [x, y, z] to [x, y, z]": where node `i`'s objects are in the world.
    fn extent_text(&self, scene: &Scene, i: usize) -> String {
        match scene.world().and_then(|w| w.extent(i)) {
            Some((lo, size)) => {
                let hi: Cell = std::array::from_fn(|k| lo[k] + size[k] as i32 - 1);
                format!(", now from {lo:?} to {hi:?}")
            }
            None => String::new(),
        }
    }

    fn set_layer(&mut self, scene: &mut Scene, a: &SetLayer) -> Result<String, Error> {
        let what = self.node(scene, a.node)?;
        let name = a.new_layer.as_deref().map(|n| n.chars().filter(|c| !c.is_control()).collect::<String>());
        let id = match (a.layer, name) {
            (Some(_), Some(_)) => return Err(invalid("give layer or new_layer, not both")),
            (None, None) => return Err(invalid("give layer (an id from the summary's layers) or new_layer")),
            (Some(id), None) => {
                self.layer(scene, id).map_err(|e| invalid(format!("{e}, or give new_layer")))?;
                id
            }
            (None, Some(n)) => {
                let n = n.trim();
                if n.is_empty() || n.chars().count() > MAX_LAYER_NAME {
                    return Err(invalid(format!("new_layer: a name of 1 to {MAX_LAYER_NAME} characters")));
                }
                let n = n.to_string();
                self.world_edit(scene, |w| w.add_layer(&n))?.0
            }
        };
        let t = self.world_edit(scene, |w| w.put_on_layer(a.node, id))?.1;
        let new = if a.new_layer.is_some() { "new " } else { "" };
        let mut s = format!("put {what} on {new}layer {id}; {t}");
        if scene.world().expect("a world").layers().iter().any(|l| l.id == id && l.hidden) {
            s += &format!("; warning: layer {id} is hidden, so {what} does not show");
        }
        Ok(s)
    }

    /// The face at world cell `c` on `side`, as map cells: the voxels of its colour connected
    /// to it in its layer whose side is open, as the change has them.
    fn face(&self, scene: &Scene, c: Cell, side: Side) -> Result<Vec<Cell>, Error> {
        let p = scene.to_map(c);
        if self.get(scene, p) == 0 {
            return Err(invalid(format!("{c:?} is empty: give a voxel of the face")));
        }
        let face = vxr_core::tools::face_cells(|q| self.get(scene, q), p, side.normal());
        if face.is_empty() {
            return Err(invalid(format!(
                "the {} side of {c:?} is covered by the voxel at {:?}: give a voxel whose side is open",
                side.name(),
                add(c, side.normal())
            )));
        }
        Ok(face)
    }

    /// Why the layer tools cannot work here, if they cannot: they work on the world.
    fn world_here(&self, scene: &Scene) -> Result<(), Error> {
        match scene.at() {
            At::World => Ok(()),
            At::Object(_) => Err(invalid(IN_MODEL)),
            At::Map => Err(invalid(NO_WORLD)),
        }
    }

    /// Layer `id` of the world the scene shows, or why a layer tool cannot work on it.
    fn layer(&self, scene: &Scene, id: i32) -> Result<(), Error> {
        self.world_here(scene)?;
        let w = scene.world().expect("world mode has a world");
        if !w.layers().iter().any(|l| l.id == id) {
            let ids: Vec<i32> = w.layers().iter().map(|l| l.id).collect();
            return Err(invalid(format!("layer {id}: no such layer; the layers are {ids:?}")));
        }
        Ok(())
    }

    /// World cells and slots of the voxels whose slot `keep` takes.
    fn voxels_of(&self, scene: &Scene, keep: impl Fn(u8) -> bool) -> Vec<(Cell, u8)> {
        let mut out = Vec::new();
        self.for_each(scene, |p, v| {
            if keep(v) {
                out.push((scene.to_world(p), v));
            }
        });
        out
    }

    fn select(&mut self, scene: &Scene, a: &Select) -> Result<String, Error> {
        if a.none {
            self.selection.clear();
        } else if a.invert {
            let mut cells = HashSet::new();
            self.for_each(scene, |p, _| {
                if !self.selection.contains(&p) {
                    cells.insert(p);
                }
            });
            self.selection = cells;
        } else {
            let cells: Vec<Cell> = if a.all {
                self.voxels_where(scene, |_, _| true)
            } else if let Some(c) = a.color {
                let c = slot(c)?;
                self.voxels_where(scene, |_, v| v == c)
            } else if let Some(cells) = self.parts_cells(scene, &a.part, &a.parts)? {
                cells
            } else if let Some(c) = a.face {
                self.face(scene, c, a.side.unwrap_or(Side::Top))?.into_iter().map(|p| scene.to_world(p)).collect()
            } else if let Some(c) = a.volume {
                let p = scene.to_map(c);
                if self.get(scene, p) == 0 {
                    return Err(invalid(format!("volume: {c:?} is empty; give a filled cell")));
                }
                vxr_core::select::connected(|q| self.get(scene, q) != 0, p)
                    .into_iter()
                    .map(|q| scene.to_world(q))
                    .collect()
            } else if let (Some(f), Some(t)) = (a.from, a.to) {
                let (lo, hi) = order(f, t);
                self.voxels_where(scene, |q, _| inside(q, lo, hi))
            } else {
                return Err(invalid("give from and to, color, part or parts, face, volume, all, none or invert"));
            };
            let cells = cells.into_iter().map(|w| scene.to_map(w));
            match a.mode {
                SelectMode::Replace => self.selection = cells.collect(),
                SelectMode::Add => self.selection.extend(cells),
                SelectMode::Subtract => {
                    for p in cells {
                        self.selection.remove(&p);
                    }
                }
            }
        }
        let sel = self.selected(scene);
        let b = vxr_core::select::bounds(sel.iter().copied())
            .map(|(lo, hi)| format!(" from {lo:?} to {hi:?}"))
            .unwrap_or_default();
        Ok(format!("{} voxels selected{b}", sel.len()))
    }

    /// The part with id or name `key`: an id, a name the model gave, or the one part whose
    /// name has every word of `key`.
    pub(crate) fn part(&self, key: &str) -> Result<usize, Error> {
        let k = key.trim();
        if let Some(i) = self.parts.iter().position(|p| p.id.eq_ignore_ascii_case(k)) {
            return Ok(i);
        }
        if let Some(i) = self.parts.iter().rposition(|p| p.named && p.name.eq_ignore_ascii_case(k)) {
            return Ok(i);
        }
        let hits: Vec<usize> = (0..self.parts.len()).filter(|&i| self.parts[i].matches(k)).collect();
        match hits.as_slice() {
            [i] => Ok(*i),
            [] => Err(invalid(format!("no part matches {}; find_parts lists them", Value::from(k)))),
            many => {
                let list: Vec<String> =
                    many.iter().take(8).map(|&i| format!("{} {}", self.parts[i].id, self.parts[i].name)).collect();
                Err(invalid(format!(
                    "{} parts match {}: {}{}; give an id",
                    many.len(),
                    Value::from(k),
                    list.join(", "),
                    if many.len() > 8 { ", ..." } else { "" }
                )))
            }
        }
    }

    /// The world cells still filled of `part` and `parts` together, or None when neither is
    /// given. An empty name counts as not given (models send `"part": ""` beside other arguments).
    fn parts_cells(&self, scene: &Scene, part: &Option<String>, parts: &[String]) -> Result<Option<Vec<Cell>>, Error> {
        let keys: Vec<&String> = part.iter().chain(parts).filter(|k| !k.trim().is_empty()).collect();
        if keys.is_empty() {
            return Ok(None);
        }
        let mut seen = HashSet::new();
        let mut cells = Vec::new();
        for k in keys {
            let i = self.part(k)?;
            if seen.insert(i) {
                cells.extend(self.part_cells(scene, i).into_iter().map(|p| scene.to_world(p)));
            }
        }
        Ok(Some(cells))
    }

    /// Part `i`'s cells that still hold a voxel (map cells).
    pub(crate) fn part_cells(&self, scene: &Scene, i: usize) -> Vec<Cell> {
        self.parts[i].cells.iter().copied().filter(|&p| self.get(scene, p) != 0).collect()
    }

    pub(crate) fn bounds_text(&self, scene: &Scene) -> String {
        let b = scene.bounds();
        let hi: [i32; 3] = std::array::from_fn(|a| b.lo[a] + b.size[a] as i32 - 1);
        format!("x {}..{}, y {}..{}, z {}..{}", b.lo[0], hi[0], b.lo[1], hi[1], b.lo[2], hi[2])
    }

    /// "staged so far: 12 added, 0 removed, 3 repainted; 1,234 voxels".
    pub(crate) fn staged_text(&self, scene: &Scene) -> String {
        let (mut a, mut r, mut p) = (0, 0, 0);
        for (q, &b) in &self.base {
            let v = self.get(scene, *q);
            match (b, v) {
                (b, v) if b == v => {}
                (0, _) => a += 1,
                (_, 0) => r += 1,
                _ => p += 1,
            }
        }
        format!("staged so far: {a} added, {r} removed, {p} repainted; {} voxels in the scene", self.voxel_count(scene))
    }

    /// Put the scene back as it was, keeping the edit open and the steps.
    fn revert(&mut self, scene: &mut Scene) {
        if self.open {
            scene.revert_voxels();
            for i in 1..=255u8 {
                if scene.palette[i as usize] != self.palette_before[i as usize] {
                    scene.put_color(i, self.palette_before[i as usize]);
                }
                if scene.materials[i as usize] != self.materials_before[i as usize] {
                    scene.put_material(i, self.materials_before[i as usize]);
                }
            }
        }
        self.base.clear();
        self.removed.clear();
        self.staged = 0;
        self.selection = self.selection_before.clone();
        self.clip = self.clip_before.clone();
        for l in self.lanes.values_mut() {
            l.selection = l.selection_before.clone();
            l.clip = l.clip_before.clone();
        }
        self.parts.truncate(self.detected);
        // The map is back in the cells it began in.
        let back = self.shift.map(|d| -d);
        for p in &mut self.parts {
            p.cells.iter_mut().for_each(|c| *c = add(*c, back));
        }
        self.shift = [0; 3];
    }

    /// Run the kept steps again from the scene as it was.
    fn rebuild(&mut self, scene: &mut Scene) {
        self.revert(scene);
        self.replaying = true;
        let me = self.lane;
        for i in 0..self.steps.len() {
            self.replay(scene, i);
        }
        self.switch(me);
        self.replaying = false;
    }

    /// Run step `i` again in its lane, naming the world nodes and layers this replay made in
    /// place of those it made before. A dropped step, or one naming what a dropped or failed
    /// step made, is not run: returns false.
    fn replay(&mut self, scene: &mut Scene, i: usize) -> bool {
        self.steps[i].made = Made::default();
        let s = &self.steps[i];
        let Some(mut op) = s.op.clone().filter(|_| !s.dropped) else { return false };
        let lane = s.lane;
        let (nodes, layer) = world_args(&mut op);
        for (a, &r) in nodes.into_iter().zip(&s.refs.nodes) {
            match self.resolve(r, |m| &m.nodes) {
                Some(n) => *a = n,
                None => return false,
            }
        }
        if let (Some(a), Some(r)) = (layer, s.refs.layer) {
            match self.resolve(r, |m| &m.layers) {
                Some(l) => *a = l,
                None => return false,
            }
        }
        self.switch(lane);
        let mark = Mark::of(scene);
        if self.run(scene, op).is_ok() {
            self.steps[i].made = mark.made(scene);
        }
        true
    }

    /// The node or layer `r` names in the world as the steps last made it.
    fn resolve<T: Copy>(&self, r: Ref<T>, made: impl Fn(&Made) -> &Vec<T>) -> Option<T> {
        match r {
            Ref::Found(v) => Some(v),
            Ref::Made(j, k) => made(&self.steps[j].made).get(k).copied(),
        }
    }

    /// What `op`'s world nodes and layer are: made by a step still kept, or found.
    fn refer(&self, op: &mut Op) -> Refs {
        fn find<T: Copy + PartialEq>(steps: &[Step], v: T, made: impl Fn(&Made) -> &Vec<T>) -> Ref<T> {
            steps
                .iter()
                .enumerate()
                .find_map(|(j, s)| made(&s.made).iter().position(|&m| m == v).map(|k| Ref::Made(j, k)))
                .unwrap_or(Ref::Found(v))
        }
        let (nodes, layer) = world_args(op);
        Refs {
            nodes: nodes.into_iter().map(|&mut n| find(&self.steps, n, |m| &m.nodes)).collect(),
            layer: layer.map(|&mut l| find(&self.steps, l, |m| &m.layers)),
        }
    }
}

fn slot(s: u8) -> Result<u8, Error> {
    if s == 0 { Err(invalid("slot 0 is empty, not a colour: slots are 1 to 255")) } else { Ok(s) }
}

/// The slot a shape sets: none for erase.
fn shape_slot(mode: Mode, s: Option<u8>) -> Result<u8, Error> {
    match (mode, s) {
        (Mode::Erase, _) => Ok(0),
        (_, Some(s)) => slot(s),
        (_, None) => Err(invalid("slot is needed unless mode is erase")),
    }
}

/// A caller's selection and clipboard, as they started and as they are now.
#[derive(Default)]
struct Lane {
    selection_before: HashSet<Cell>,
    selection: HashSet<Cell>,
    clip_before: Option<Clip>,
    clip: Option<Clip>,
}

/// The world nodes and layers an op names.
fn world_args(op: &mut Op) -> (Vec<&mut usize>, Option<&mut i32>) {
    match op {
        Op::MoveObject(a) => (vec![&mut a.node], None),
        Op::Duplicate(a) => (vec![&mut a.node], None),
        Op::Group(a) => (a.nodes.iter_mut().collect(), None),
        Op::Ungroup(a) => (vec![&mut a.node], None),
        Op::Hide(a) => (vec![&mut a.node], None),
        Op::SetLayer(a) => (vec![&mut a.node], a.layer.as_mut()),
        Op::DeleteObject(a) => (vec![&mut a.node], None),
        Op::Rename(a) => (a.node.iter_mut().collect(), a.layer.as_mut()),
        Op::HideLayer(a) => (Vec::new(), Some(&mut a.layer)),
        _ => (Vec::new(), None),
    }
}

/// How many nodes the world had and its layers, before a step.
struct Mark {
    nodes: usize,
    layers: Vec<i32>,
}

impl Mark {
    fn of(scene: &Scene) -> Mark {
        match scene.world() {
            Some(w) => Mark { nodes: w.tree.nodes.len(), layers: w.layers().iter().map(|l| l.id).collect() },
            None => Mark { nodes: 0, layers: Vec::new() },
        }
    }

    /// The nodes and layers the world gained since.
    fn made(&self, scene: &Scene) -> Made {
        let Some(w) = scene.world() else { return Made::default() };
        Made {
            nodes: (self.nodes..w.tree.nodes.len()).collect(),
            layers: w.layers().iter().map(|l| l.id).filter(|id| !self.layers.contains(id)).collect(),
        }
    }
}

/// What staging some cells did.
struct Staged {
    changed: usize,
    /// Cells outside the bounds, skipped.
    outside: usize,
    /// Map cells set to a voxel, changed or not.
    made: Vec<Cell>,
}

/// "object 3", "group 2 (3 objects)".
fn node_text(w: &World, i: usize) -> String {
    match &w.tree.nodes[i].kind {
        NodeKind::Object(_) => format!("object {i}"),
        NodeKind::Group(_) => {
            let n = w.objects_under(i).len();
            format!("group {i} ({n} object{})", if n == 1 { "" } else { "s" })
        }
    }
}

/// The cells of a ball of radius `r` around `c`, or a disc flat across `flat`, as the center
/// brush draws them: within r^2 + r/2 of the centre.
fn round(c: Cell, r: i32, flat: Option<Axis>) -> Result<Vec<Cell>, Error> {
    if !(1..=MAX_RADIUS).contains(&r) {
        return Err(invalid(format!("radius {r}: 1 to {MAX_RADIUS}")));
    }
    let flat = flat.map(Axis::index);
    let reach = (r * r) as f32 + r as f32 / 2.0;
    let span = |k: usize| if flat == Some(k) { 0 } else { r };
    let mut cells = Vec::new();
    for z in -span(2)..=span(2) {
        for y in -span(1)..=span(1) {
            for x in -span(0)..=span(0) {
                if ((x * x + y * y + z * z) as f32) <= reach {
                    cells.push([c[0] + x, c[1] + y, c[2] + z]);
                }
            }
        }
    }
    Ok(cells)
}

/// "1 node", "2 layers", "1 node and 1 layer".
fn world_text(nodes: usize, layers: usize) -> String {
    let l = format!("{layers} layer{}", if layers == 1 { "" } else { "s" });
    match (nodes, layers) {
        (_, 0) => nodes_text(nodes),
        (0, _) => l,
        _ => format!("{} and {l}", nodes_text(nodes)),
    }
}

/// How many layers differ between `old` and `new` by name or hidden flag, a layer with no
/// entry counting as unnamed and shown.
fn changed_layers(old: &Tree, new: &Tree) -> usize {
    let of =
        |t: &Tree, id: i32| t.layers.iter().find(|l| l.id == id).map_or((None, false), |l| (l.name.clone(), l.hidden));
    let mut ids: Vec<i32> = old.layers.iter().chain(&new.layers).map(|l| l.id).collect();
    ids.sort_unstable();
    ids.dedup();
    ids.into_iter().filter(|&id| of(old, id) != of(new, id)).count()
}

/// "1 node", "3 nodes".
fn nodes_text(n: usize) -> String {
    format!("{n} node{}", if n == 1 { "" } else { "s" })
}

/// How many nodes under the root of `new` differ from `old`: added or taken out of the tree,
/// moved to another parent, or with another name, flag, layer, transform or model. A
/// group whose children change does not count; the children do.
fn changed_nodes(old: &Tree, new: &Tree) -> usize {
    fn at<'a>(t: &'a Tree, p: &[Option<usize>], i: usize) -> Option<(&'a TreeNode, usize)> {
        Some((t.nodes.get(i)?, p.get(i).copied().flatten()?))
    }
    let (po, pn) = (old.parents(), new.parents());
    (1..old.nodes.len().max(new.nodes.len()))
        .filter(|&i| match (at(old, &po, i), at(new, &pn, i)) {
            (None, None) => false,
            (Some((a, pa)), Some((b, pb))) => pa != pb || !same_node(a, b),
            _ => true,
        })
        .count()
}

/// Whether two nodes are the same but for the children a group holds.
fn same_node(a: &TreeNode, b: &TreeNode) -> bool {
    let kind = match (&a.kind, &b.kind) {
        (NodeKind::Object(x), NodeKind::Object(y)) => x == y,
        (NodeKind::Group(_), NodeKind::Group(_)) => true,
        _ => false,
    };
    kind && (&a.name, a.hidden, a.layer, a.local) == (&b.name, b.hidden, b.layer, b.local)
}

fn add(p: Cell, d: Cell) -> Cell {
    std::array::from_fn(|k| p[k] + d[k])
}

/// The sliders a material's type reads, as text: " (roughness 0.2, metalness 1)".
fn settings(m: &Material) -> String {
    let v: Vec<(&str, f32)> = match m.kind {
        Kind::Diffuse => vec![],
        Kind::Metal => vec![("roughness", m.rough), ("metalness", m.metal)],
        Kind::Plastic => vec![("roughness", m.rough), ("specular", m.spec)],
        Kind::Glass => {
            vec![("roughness", m.rough), ("transparency", m.trans), ("refraction", m.ior), ("attenuation", m.att)]
        }
        Kind::Emit => vec![("emission", m.emit), ("power", m.flux)],
    };
    if v.is_empty() {
        return String::new();
    }
    let list: Vec<String> = v.iter().map(|(n, x)| format!("{n} {}", (x * 1000.0).round() / 1000.0)).collect();
    format!(" ({})", list.join(", "))
}

pub(crate) fn order(a: Cell, b: Cell) -> (Cell, Cell) {
    (std::array::from_fn(|k| a[k].min(b[k])), std::array::from_fn(|k| a[k].max(b[k])))
}

/// The cells in the box from `lo` to `hi`, saturating.
pub(crate) fn span(lo: Cell, hi: Cell) -> i64 {
    (0..3).fold(1i64, |n, k| n.saturating_mul(hi[k] as i64 - lo[k] as i64 + 1))
}

pub(crate) fn inside(p: Cell, lo: Cell, hi: Cell) -> bool {
    (0..3).all(|k| p[k] >= lo[k] && p[k] <= hi[k])
}
