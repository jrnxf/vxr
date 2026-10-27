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
use vxr_core::Scene;
use vxr_core::history::At;
use vxr_vox::chunk::{NOTE, Note};
use vxr_vox::raw::RawFile;

use crate::args::*;
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
        }
    }
}

const MAX_RADIUS: i32 = 64;
const MAX_LINE: i32 = 4096;
const WORLD: &str = "the scene shows the world of several objects, where voxels cannot be edited; the user opens one object's model (Tab) to edit it";

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
    /// The call to replay, for steps that change the scene or the selection.
    op: Option<Op>,
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
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Changes {
    /// Cell and its new slot.
    pub added: Vec<(Cell, u8)>,
    /// Cell and the slot it held.
    pub removed: Vec<(Cell, u8)>,
    /// Cell, old slot, new slot.
    pub repainted: Vec<(Cell, u8, u8)>,
    /// Slot, old colour, new colour.
    pub palette: Vec<(u8, [u8; 4], [u8; 4])>,
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
    /// The assistant's selection (map cells), seeded from the user's.
    pub(crate) selection: HashSet<Cell>,
    selection_before: HashSet<Cell>,
    pub(crate) parts: Vec<Part>,
    /// How many of `parts` were found in the scene; the rest the model named.
    detected: usize,
    /// Every name an edit gave, in the order first given: name `i` is part `detected + i + 1`
    /// however the steps replay.
    names: Vec<String>,
    /// The file's `NOTE` palette row names, cleaned.
    pub(crate) notes: Vec<String>,
    steps: Vec<Step>,
    calls: usize,
    turn_calls: usize,
    allowed: usize,
    /// Whether the scene's edit is open (the first edit opens it).
    open: bool,
    replaying: bool,
    done: Option<String>,
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
            selection_before: selection.clone(),
            selection,
            detected: parts.len(),
            names: Vec::new(),
            parts,
            notes: file.map(notes_of).unwrap_or_default(),
            steps: Vec::new(),
            calls: 0,
            turn_calls: 0,
            allowed: limits.ask_over,
            open: false,
            replaying: false,
            done: None,
        }
    }

    /// A new model turn: the per-turn call count starts again.
    pub fn begin_turn(&mut self) {
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
                | Op::Fill(_)
                | Op::Paint(_)
                | Op::Select(_)
                | Op::PaletteSet(_)
        );
        let query = op.is_query();
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
                op: (replay && r.is_ok()).then_some(op),
            });
        }
        r
    }

    /// Run one operation, without the call limits.
    pub fn run(&mut self, scene: &mut Scene, op: Op) -> Result<String, Error> {
        match op {
            Op::SceneSummary(_) => Ok(self.summary(scene)),
            Op::GetVoxels(a) => self.get_voxels(scene, &a),
            Op::Slice(a) => self.slice(scene, &a),
            Op::FindParts(a) => Ok(self.find_parts(scene, a.query.as_deref())),
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
                let r = a.radius;
                if !(1..=MAX_RADIUS).contains(&r) {
                    return Err(invalid(format!("radius {r}: 1 to {MAX_RADIUS}")));
                }
                let flat = a.flat_axis.map(Axis::index);
                // As the center brush: cells within r^2 + r/2 of the centre.
                let reach = (r * r) as f32 + r as f32 / 2.0;
                let span = |k: usize| if flat == Some(k) { 0 } else { r };
                let c = a.center;
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
            Op::PaletteSet(a) => {
                let s = slot(a.slot)?;
                let c = parse_hex(&a.hex).map_err(invalid)?;
                self.open(scene);
                let old = scene.palette[s as usize];
                scene.put_color(s, c);
                Ok(format!("slot {s}: {} -> {} ({} voxels use it)", hex(old), hex(c), self.slot_count(scene, s)))
            }
            Op::Undo(_) => {
                let Some(i) = self
                    .steps
                    .iter()
                    .rposition(|s| !s.dropped && s.op.as_ref().is_some_and(|o| !matches!(o, Op::Select(_))))
                else {
                    return Err(invalid("nothing of yours to undo"));
                };
                self.steps[i].dropped = true;
                self.rebuild(scene);
                Ok(format!("took back step {} ({}); {}", i + 1, self.steps[i].tool, self.staged_text(scene)))
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
        let Some(s) = self.steps.get_mut(i).filter(|s| s.op.is_some()) else { return false };
        if s.dropped != dropped {
            s.dropped = dropped;
            self.rebuild(scene);
        }
        true
    }

    /// Every staged cell (map cells) and how it shows, for the ghost overlay.
    pub fn ghost(&self, scene: &Scene) -> Vec<(Cell, Ghost)> {
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
        for (p, g) in self.ghost(scene) {
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
        c
    }

    /// Apply: erase the removed cells and close the edit as one undo step. Returns how many
    /// voxels changed.
    pub fn apply(self, scene: &mut Scene) -> usize {
        if !self.open {
            return 0;
        }
        for &p in &self.removed {
            scene.put(p, 0);
        }
        scene.commit()
    }

    /// Discard: put every cell and colour back and close the edit, leaving no undo step.
    pub fn discard(mut self, scene: &mut Scene) {
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
        if scene.at() == At::World {
            return Err(invalid(WORLD));
        }
        let size = scene.map.size_voxels();
        let (mut outside, mut seen, mut plan, mut made) = (0usize, HashSet::new(), Vec::new(), Vec::new());
        let mut delta = 0isize;
        for q in cells {
            let p = scene.to_map(q);
            if (0..3).any(|a| p[a] < 0 || p[a] >= size[a] as i32) {
                outside += 1;
                continue;
            }
            if !seen.insert(p) {
                continue;
            }
            let cur = self.get(scene, p);
            let v = match mode {
                Mode::Add => slot,
                Mode::Erase => 0,
                Mode::Paint if cur != 0 => slot,
                Mode::Paint => continue,
            };
            if v != 0 {
                made.push(p);
            }
            if cur != v {
                let b = self.base.get(&p).copied().unwrap_or_else(|| scene.map.get(p[0], p[1], p[2]));
                delta += (v != b) as isize - (cur != b) as isize;
                plan.push((p, v));
            }
        }
        let after = (self.staged as isize + delta).max(0) as usize;
        if !self.replaying && delta > 0 {
            if after > self.limits.changed_cells {
                return Err(limit(format!(
                    "this would stage {after} changed cells; the limit is {} per request",
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
        let mut s = format!("{} cells changed; {}", plan.len(), self.staged_text(scene));
        if let Some(n) = name.map(|n| clean(n.as_bytes())).filter(|n| !n.is_empty() && !made.is_empty()) {
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
        if outside > 0 {
            s += &format!(
                "; warning: {outside} cells were outside the editable bounds ({}) and skipped",
                self.bounds_text(scene)
            );
        } else if plan.is_empty() {
            s += "; warning: nothing changed";
        }
        Ok(s)
    }

    fn select(&mut self, scene: &Scene, a: &Select) -> Result<String, Error> {
        if a.none {
            self.selection.clear();
        } else {
            let cells: Vec<Cell> = if a.all {
                self.voxels_where(scene, |_, _| true)
            } else if let Some(c) = a.color {
                let c = slot(c)?;
                self.voxels_where(scene, |_, v| v == c)
            } else if let Some(cells) = self.parts_cells(scene, &a.part, &a.parts)? {
                cells
            } else if let (Some(f), Some(t)) = (a.from, a.to) {
                let (lo, hi) = order(f, t);
                self.voxels_where(scene, |q, _| inside(q, lo, hi))
            } else {
                return Err(invalid("give from and to, color, part or parts, all or none"));
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
            }
        }
        self.base.clear();
        self.removed.clear();
        self.staged = 0;
        self.selection = self.selection_before.clone();
        self.parts.truncate(self.detected);
    }

    /// Run the kept steps again from the scene as it was.
    fn rebuild(&mut self, scene: &mut Scene) {
        self.revert(scene);
        self.replaying = true;
        let ops: Vec<Op> = self.steps.iter().filter(|s| !s.dropped).filter_map(|s| s.op.clone()).collect();
        for op in ops {
            let _ = self.run(scene, op);
        }
        self.replaying = false;
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
