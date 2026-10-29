//! Terminal agents editing the window through `vxr mcp --live` (`docs/research/ai.md`
//! section 3, live mode): each edit call applies at once as one undo step named after the
//! agent ("Claude Code: paint 412 voxels"), listed in the Agent edits card with an Undo of
//! its own. With "Review every agent change" on, edits wait instead in one change under
//! review that every agent adds to ([`Agents::stage`]), drawn as the assistant's is, until
//! the user applies it (each edit then lands as the same undo step) or discards it; each
//! agent hears which on its next call or from `review_status`. The socket is the host's
//! ([`crate::live`]); this is what a call does to the scene, with no GPU and no socket, so it
//! is the same in tests.

use std::collections::HashMap;

use serde_json::{Value, json};
use vxr_core::Scene;
use vxr_core::history::At;
use vxr_ops::{Changes, Limits, Op, Session, clean};

use crate::review::Review;
use crate::ui::group_thousands;

type Cell = [i32; 3];

/// One edit an agent applied: its undo step, and the step that took it back, if any.
#[derive(Clone, Debug, PartialEq)]
pub struct AgentEdit {
    /// The agent's connection, which tells two agents of the same name apart.
    pub conn: u64,
    pub client: String,
    pub label: String,
    /// The history id of its step (`History::position` right after it).
    pub step: u64,
    /// The step that took it back when it was not the last one (see [`Agents::undo_edit`]).
    pub undone_by: Option<u64>,
}

/// The agents and what they did, beside the scene.
#[derive(Default)]
pub struct Agents {
    pub edits: Vec<AgentEdit>,
    /// The connected agents: connection and name, set by the host each frame.
    pub clients: Vec<(u64, String)>,
    /// The window listens for agents ("Allow terminal agents to edit this window").
    pub allowed: bool,
    /// Live mode can run here: the desktop app on a system with Unix domain sockets, or the
    /// browser through `vxr bridge`.
    pub available: bool,
    /// The browser's live mode: the tab pairs with `vxr bridge` ([`crate::bridge`]).
    pub web: bool,
    /// The browser is opening the link to `vxr bridge`, or waiting for it to take the code.
    pub connecting: bool,
    /// Why turning it on failed.
    pub error: Option<String>,
    /// What kind of failure `error` is, for the checklist's hint and button.
    pub fail: Option<Fail>,
    /// When the tab paired with `vxr bridge` (egui time, seconds), while it is paired; in the
    /// desktop app, when the socket opened.
    pub since: Option<f64>,
    /// When each connection was first listed, and its last call: when and which.
    seen: HashMap<u64, f64>,
    last: HashMap<u64, (f64, String)>,
    /// Each agent's own selection (map cells) by connection, kept between its calls so it
    /// never changes the user's; it starts from the user's. Kept with the model and map it
    /// was made on, and dropped once either changes.
    selections: HashMap<u64, (At, u64, Vec<Cell>)>,
    /// Bumped whenever the edits change.
    pub version: u64,
    /// "Review every agent change": edits wait in a change under review for the user to
    /// apply or discard, instead of applying at once. Off until the user turns it on.
    pub review: bool,
    /// What each connection has not heard yet: the user applied or discarded edits of its
    /// that waited for review. Told with its next call, or by `review_status`.
    notices: HashMap<u64, Vec<String>>,
}

/// Why the link to the agents is not up, which the Agents tab answers with its own words
/// and button.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Fail {
    /// `vxr bridge` refused the pairing code.
    Wrong,
    /// It hung up before it took the code.
    Hung,
    /// Nothing answered on its port.
    Nothing,
    /// The browser refuses the page access to apps on this computer.
    Denied,
    /// The browser cannot let a page reach them at all.
    Blocked,
    Safari,
    Brave,
    /// The bridge stopped while paired.
    Lost,
    /// Anything else, with the text in `error`.
    Other,
}

/// Added to an edit's result while it waits for review.
pub const WAITING: &str = "waiting for the user's review in vxr: it is not in the scene until they apply it; review_status (or your next call) tells you whether they applied or discarded it";

/// The limits of a terminal agent's call: it is not asked per 10,000 cells, the hard limit
/// stays, counted per call with review on as off.
pub fn limits() -> Limits {
    Limits { ask_over: Limits::default().changed_cells, per_call: true, ..Limits::default() }
}

/// Whether a call changes the scene, so with review on it waits for the user: an edit or a
/// colour, not a query, a selection, `undo` or `done`.
pub fn edits(name: &str, args: &Value) -> bool {
    matches!(Op::parse(name, args.clone()),
        Ok(op) if !op.is_query() && !matches!(op, Op::Select(_) | Op::Undo(_) | Op::Done(_)))
}

/// Whether a step of a change under review is an edit (what Apply lands and the notices
/// count), not a selection, an `undo`, `done` or a call that failed.
fn is_edit(st: &vxr_ops::Step) -> bool {
    st.ok && !matches!(st.tool, "select" | "undo" | "done")
}

/// "1 edit", "3 edits".
fn edits_text(n: usize) -> String {
    format!("{n} edit{}", if n == 1 { "" } else { "s" })
}

/// What an edit's undo step is called: the agent, the tool and what it changed.
pub fn label(client: &str, tool: &str, c: &Changes) -> String {
    let cells = c.added.len() + c.removed.len() + c.repainted.len();
    match (cells, c.palette.len()) {
        (1, _) => format!("{client}: {tool} 1 voxel"),
        (0, 1) => format!("{client}: {tool} slot {}", c.palette[0].0),
        (0, 0) => format!("{client}: {tool}"),
        (0, k) => format!("{client}: {tool} {k} slots"),
        (n, _) => format!("{client}: {tool} {} voxels", group_thousands(n as u64)),
    }
}

impl Agents {
    /// Whether edit `i` is in the scene now: its step is applied and nothing took it back.
    pub fn in_effect(&self, scene: &Scene, i: usize) -> bool {
        let e = &self.edits[i];
        let h = scene.history();
        h.applied(e.step) && !e.undone_by.is_some_and(|u| h.applied(u))
    }

    /// Forget every edit and selection: the scene they were made on is gone. What the
    /// agents have not heard yet stays.
    pub fn clear(&mut self) {
        self.edits.clear();
        self.selections.clear();
        self.version += 1;
    }

    /// A connection closed: its selection and what it had not heard go with it. Its edits
    /// stay listed, for the user to take back.
    pub fn forget(&mut self, conn: u64) {
        self.selections.remove(&conn);
        self.notices.remove(&conn);
        self.seen.remove(&conn);
        self.last.remove(&conn);
    }

    /// Note the connections listed at `now`: when each first appeared.
    pub fn note_clients(&mut self, now: f64) {
        for (conn, _) in &self.clients {
            self.seen.entry(*conn).or_insert(now);
        }
        self.seen.retain(|c, _| self.clients.iter().any(|(n, _)| n == c));
    }

    /// Note that connection `conn` called `what` at `now`.
    pub fn note_call(&mut self, conn: u64, now: f64, what: &str) {
        self.seen.entry(conn).or_insert(now);
        self.last.insert(conn, (now, what.to_string()));
    }

    /// When connection `conn` was first listed, and its last call.
    pub fn times(&self, conn: u64) -> (Option<f64>, Option<f64>) {
        (self.seen.get(&conn).copied(), self.last.get(&conn).map(|l| l.0))
    }

    /// What connection `conn` has not heard yet, taken: one line per applied or discarded
    /// change, to put before its next call's answer.
    pub fn take_notices(&mut self, conn: u64) -> Option<String> {
        self.notices.remove(&conn).map(|n| n.join("\n"))
    }

    fn notify(&mut self, conn: u64, text: String) {
        if self.clients.iter().any(|(c, _)| *c == conn) {
            self.notices.entry(conn).or_default().push(text);
        }
    }

    /// `review_status`: what the user did with the connection's edits that waited (not heard
    /// yet), then whether any wait now and whether review is on. `review` is the agents'
    /// change under review, if one is open.
    pub fn status(&mut self, conn: u64, review: Option<&Review>) -> String {
        let mut out: Vec<String> = self.take_notices(conn).into_iter().collect();
        let mine = review
            .map_or(0, |r| r.session.steps().iter().filter(|s| s.lane == conn && is_edit(s) && !s.dropped).count());
        out.push(match (mine, review.is_some(), self.review) {
            (0, true, _) => "Nothing of yours waits for review. The user is reviewing other agents' edits: your next edit joins that change and waits too.".into(),
            (0, false, true) => "Nothing of yours waits for review. Review is on: each edit waits for the user to apply or discard it.".into(),
            (0, false, false) => "Nothing of yours waits for review. Review is off: each edit applies at once as one undo step.".into(),
            (n, _, _) => format!(
                "{} of yours {} for the user's review in vxr: none is in the scene until they apply the change. Call review_status again later; do not build on them as if they were in.",
                edits_text(n),
                if n == 1 { "waits" } else { "wait" }
            ),
        });
        out.join("\n")
    }

    /// The selection connection `conn` left on a model, or None when it has none, so it
    /// starts from the user's. One made on another model or map is dropped: empty.
    fn selection_of(&self, conn: u64, made_on: (At, u64)) -> Option<Vec<Cell>> {
        let (at, reshapes, sel) = self.selections.get(&conn)?;
        Some(if (*at, *reshapes) == made_on { sel.clone() } else { Vec::new() })
    }

    /// Run one tool call from connection `conn`, named `client`, into `review`, the agents'
    /// change under review: an edit is staged there with the agent's own selection (its
    /// lane), drawn as a ghost until the user applies or discards it; a query sees the scene
    /// with the change; `undo` drops the agent's last edit from it.
    pub fn stage(
        &mut self,
        scene: &mut Scene,
        review: &mut Review,
        conn: u64,
        client: &str,
        name: &str,
        args: &Value,
    ) -> Result<String, String> {
        if !vxr_mcp::live::is_tool(name) {
            return Err(format!("Unknown tool: {}", clean(name.as_bytes())));
        }
        if name == "edit_object" {
            let node = args.get("node").and_then(Value::as_u64).unwrap_or(0) as usize;
            if scene.at() != At::Object(node) {
                return Err(format!(
                    "node {node}: edits wait for the user's review in this object's model: another object can be edited once they apply or discard them (review_status tells you)"
                ));
            }
            return self.stage(scene, review, conn, client, "scene_summary", &json!({}));
        }
        let mine = |r: &Review| r.session.steps().iter().any(|s| s.lane == conn && is_edit(s) && !s.dropped);
        if name == "undo" && !mine(review) {
            return Err("nothing of yours waits for review to take back; your applied edits can be taken back once the user applies or discards the change under review".into());
        }
        let made_on = (scene.at(), scene.reshapes());
        let seed = self.selection_of(conn, made_on).unwrap_or_else(|| scene.selection().collect());
        review.session.set_lane(conn, seed);
        review.session.begin_request();
        let steps = review.session.steps().len();
        let r = review.session.call(scene, name, args.clone());
        self.selections.insert(conn, (made_on.0, made_on.1, review.session.selection().collect()));
        if review.session.steps().len() != steps || name == "undo" {
            let names = review.agents.get_or_insert_default();
            if !names.iter().any(|(c, _)| *c == conn) {
                names.push((conn, client.to_string()));
            }
            review.changed(scene);
        }
        match r {
            Ok(t) if edits(name, args) || name == "undo" => Ok(format!("{t}; {WAITING}")),
            Ok(t) => Ok(t),
            Err(e) => Err(e.message),
        }
    }

    /// The user applied the agents' change under review: each edit kept lands as one undo
    /// step named after its agent, as without review, listed in the Agent edits card, and
    /// each agent hears what became of its edits. Returns how many steps landed.
    pub fn apply_review(&mut self, scene: &mut Scene, review: Review) -> usize {
        let names = review.agents.clone().unwrap_or_default();
        let name_of =
            |lane: u64| names.iter().find(|(c, _)| *c == lane).map_or("Agent".to_string(), |(_, n)| n.clone());
        let counts = Self::counts(&review);
        let mut labels = HashMap::new();
        let landed = review.session.apply_steps(scene, |i, st, c| {
            let l = label(&name_of(st.lane), st.tool, c);
            labels.insert(i, (st.lane, l.clone()));
            l
        });
        for (i, step) in &landed {
            let (conn, label) = labels.remove(i).expect("a landed step was labelled");
            self.edits.push(AgentEdit { conn, client: name_of(conn), label, step: *step, undone_by: None });
        }
        for (conn, kept, all) in counts {
            let text = match kept {
                0 => format!(
                    "vxr: the user applied the change under review without your {} (they dropped them): none of them is in the scene.",
                    edits_text(all)
                ),
                k if k == all => format!(
                    "vxr: the user applied your {} that waited for review: they are in the scene now, each an undo step named after you.",
                    edits_text(all)
                ),
                k => format!(
                    "vxr: the user applied {k} of your {} that waited for review and dropped the rest: those {k} are in the scene now.",
                    edits_text(all)
                ),
            };
            self.notify(conn, text);
        }
        self.version += 1;
        landed.len()
    }

    /// The user discarded the agents' change under review: the scene is as it was, and each
    /// agent hears that none of its edits that waited is in it.
    pub fn discard_review(&mut self, scene: &mut Scene, review: Review) {
        for (conn, _, all) in Self::counts(&review) {
            self.notify(conn, format!("vxr: the user discarded the change under review: none of your {} that waited is in the scene. Look at the scene again before you go on.", edits_text(all)));
        }
        review.session.discard(scene);
        self.version += 1;
    }

    /// Each agent's edits in the change under review: connection, kept, all.
    fn counts(review: &Review) -> Vec<(u64, usize, usize)> {
        let mut out: Vec<(u64, usize, usize)> = Vec::new();
        for st in review.session.steps().iter().filter(|s| is_edit(s)) {
            let i = match out.iter().position(|c| c.0 == st.lane) {
                Some(i) => i,
                None => {
                    out.push((st.lane, 0, 0));
                    out.len() - 1
                }
            };
            out[i].1 += !st.dropped as usize;
            out[i].2 += 1;
        }
        out
    }

    /// Run one tool call from connection `conn`, named `client`, on the scene, as `vxr mcp`
    /// files mode does: an edit applies at once as one undo step named after the client.
    pub fn call(
        &mut self,
        scene: &mut Scene,
        conn: u64,
        client: &str,
        name: &str,
        args: &Value,
    ) -> Result<String, String> {
        if !vxr_mcp::live::is_tool(name) {
            return Err(format!("Unknown tool: {}", clean(name.as_bytes())));
        }
        match name {
            "undo" => {
                let i = (0..self.edits.len())
                    .rev()
                    .find(|&i| self.edits[i].conn == conn && self.in_effect(scene, i))
                    .ok_or("nothing of yours to undo")?;
                self.undo_edit(scene, i).map(|l| format!("took back {l}"))
            }
            "done" => {
                let s: String = args
                    .get("summary")
                    .and_then(Value::as_str)
                    .unwrap_or("")
                    .chars()
                    .filter(|c| !c.is_control())
                    .take(500)
                    .collect();
                Ok(format!("noted: {s}"))
            }
            "edit_object" => {
                let node =
                    args.get("node").and_then(Value::as_u64).ok_or("node: an object's node number from the summary")?
                        as usize;
                let n = scene.world().map_or(0, |w| w.tree.nodes.len());
                if node == 0 || node >= n {
                    return Err(format!("node {node}: not a node of this file"));
                }
                if scene.at() != At::Object(node) && !scene.edit_object(node) {
                    return Err(format!("node {node}: not an object (a group has no model of its own)"));
                }
                self.selections.remove(&conn);
                self.query(scene, Some((conn, client)), "scene_summary")
            }
            _ => self.run(scene, Some((conn, client)), name, args),
        }
    }

    /// A resource's text, or None for a uri that names nothing.
    pub fn read(&mut self, scene: &mut Scene, uri: &str) -> Option<Result<String, String>> {
        match uri {
            "vxr://scene/summary" => Some(self.query(scene, None, "scene_summary")),
            "vxr://scene/palette" => Some(Ok(vxr_mcp::files::palette_text(scene, &[]))),
            _ => None,
        }
    }

    fn query(&mut self, scene: &mut Scene, agent: Option<(u64, &str)>, name: &str) -> Result<String, String> {
        self.run(scene, agent, name, &json!({}))
    }

    /// One `vxr-ops` call in its own session, applied at once as one undo step. `agent` is
    /// the caller's connection and name, None for a resource read.
    fn run(
        &mut self,
        scene: &mut Scene,
        agent: Option<(u64, &str)>,
        name: &str,
        args: &Value,
    ) -> Result<String, String> {
        let mut session = Session::with_limits(scene, None, limits());
        let made_on = (scene.at(), scene.reshapes());
        let (conn, client) = agent.unwrap_or((0, ""));
        if let Some(sel) = agent.and_then(|_| self.selection_of(conn, made_on)) {
            session.set_selection(sel);
        }
        match session.call(scene, name, args.clone()) {
            Ok(text) => {
                if agent.is_some() {
                    self.selections.insert(conn, (made_on.0, made_on.1, session.selection().collect()));
                }
                let label = label(client, name, &session.changes(scene));
                let before = scene.history().position();
                session.apply_named(scene, &label);
                let step = scene.history().position();
                if step != before {
                    self.edits.push(AgentEdit { conn, client: client.to_string(), label, step, undone_by: None });
                    self.version += 1;
                }
                Ok(text)
            }
            Err(e) => {
                session.discard(scene);
                Err(e.message)
            }
        }
    }

    /// Take edit `i` back. The last step is undone as Cmd-Z would (so redo brings it back);
    /// an earlier one is reverted as a new step, "Undo Claude Code: paint 412 voxels", when
    /// the cells and slots it changed still hold what it left. Returns the edit's label.
    pub fn undo_edit(&mut self, scene: &mut Scene, i: usize) -> Result<String, String> {
        if i >= self.edits.len() || !self.in_effect(scene, i) {
            return Err("already taken back".into());
        }
        let e = &self.edits[i];
        let label = e.label.clone();
        if scene.history().position() == e.step {
            scene.undo();
            self.version += 1;
            return Ok(label);
        }
        let step = scene.history().step(e.step).cloned().ok_or("already taken back")?;
        if step.needs_map() && step.at != scene.at() {
            return Err(format!("{label} was made in another model: open it to take this back"));
        }
        if step.bounds.is_some() || step.world.is_some() || !step.materials.is_empty() || !step.cameras.is_empty() {
            return Err(format!("{label} cannot be taken back on its own"));
        }
        let moved = step.voxels.iter().any(|c| scene.map.get(c.pos[0], c.pos[1], c.pos[2]) != c.new)
            || step.colors.iter().any(|c| scene.palette[c.index as usize] != c.new);
        if moved {
            return Err(format!("what {label} changed was edited since: undo those edits first"));
        }
        scene.commit();
        for c in step.voxels.iter().rev() {
            scene.put(c.pos, c.old);
        }
        for c in &step.colors {
            scene.put_color(c.index, c.old);
        }
        scene.commit_named(&format!("Undo {label}"));
        self.edits[i].undone_by = Some(scene.history().position());
        self.version += 1;
        Ok(label)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scene() -> Scene {
        Scene::new([16, 16, 16])
    }

    /// The slot at world cell `w`.
    fn at(s: &Scene, w: Cell) -> u8 {
        let p = s.to_map(w);
        s.map.get(p[0], p[1], p[2])
    }

    /// The user's own edit at world cells, as one undo step.
    fn by_hand(s: &mut Scene, cells: &[(Cell, u8)]) {
        let cells: Vec<_> = cells.iter().map(|&(w, v)| (s.to_map(w), v)).collect();
        s.edit(cells);
    }

    fn ok(a: &mut Agents, s: &mut Scene, conn: u64, client: &str, name: &str, args: Value) -> String {
        a.call(s, conn, client, name, &args).unwrap_or_else(|e| panic!("{name}: {e}"))
    }

    #[test]
    fn each_edit_is_one_undo_step_named_after_the_agent() {
        let (mut a, mut s) = (Agents::default(), scene());
        ok(&mut a, &mut s, 1, "Claude Code", "box", json!({"from": [0, 0, 0], "to": [3, 0, 3], "slot": 5}));
        assert_eq!(s.history().undo_label(), Some("Claude Code: box 16 voxels"));
        ok(&mut a, &mut s, 1, "Claude Code", "place", json!({"cells": [[0, 1, 0]], "slot": 7}));
        assert_eq!(s.history().undo_label(), Some("Claude Code: place 1 voxel"));
        ok(&mut a, &mut s, 2, "Codex", "palette_set", json!({"slot": 5, "hex": "#ff0000"}));
        assert_eq!(s.history().undo_label(), Some("Codex: palette_set slot 5"));
        // Queries and a selection leave no step.
        let n = a.edits.len();
        ok(&mut a, &mut s, 2, "Codex", "scene_summary", json!({}));
        ok(&mut a, &mut s, 2, "Codex", "select", json!({"from": [0, 0, 0], "to": [1, 0, 1]}));
        assert_eq!(a.edits.len(), n);
        let labels: Vec<&str> = a.edits.iter().map(|e| e.label.as_str()).collect();
        assert_eq!(labels, ["Claude Code: box 16 voxels", "Claude Code: place 1 voxel", "Codex: palette_set slot 5"]);
        // An error stages nothing.
        assert!(a.call(&mut s, 2, "Codex", "place", &json!({"cells": [[0, 1]], "slot": 7})).is_err());
        assert!(a.call(&mut s, 2, "Codex", "list_files", &json!({})).unwrap_err().contains("Unknown tool"));
        assert_eq!(s.history().undo_label(), Some("Codex: palette_set slot 5"));
        // Cmd-Z takes them back in order, by name.
        assert!(s.undo());
        assert!(!a.in_effect(&s, 2));
        assert_eq!(s.palette[5], Scene::new([1, 1, 1]).palette[5]);
        assert!(s.redo());
        assert!(a.in_effect(&s, 2));
    }

    #[test]
    fn undo_takes_back_one_edit_even_under_later_ones() {
        let (mut a, mut s) = (Agents::default(), scene());
        ok(&mut a, &mut s, 1, "Claude Code", "place", json!({"cells": [[0, 0, 0], [1, 0, 0]], "slot": 3}));
        ok(&mut a, &mut s, 1, "Claude Code", "place", json!({"cells": [[5, 0, 5]], "slot": 4}));
        // The user's own edit on top.
        by_hand(&mut s, &[([6, 0, 6], 6)]);
        // The card's Undo on the first: reverted as a step of its own, the rest stays.
        assert_eq!(a.undo_edit(&mut s, 0).unwrap(), "Claude Code: place 2 voxels");
        assert_eq!(s.history().undo_label(), Some("Undo Claude Code: place 2 voxels"));
        assert_eq!((at(&s, [0, 0, 0]), at(&s, [5, 0, 5]), at(&s, [6, 0, 6])), (0, 4, 6));
        assert!(!a.in_effect(&s, 0) && a.in_effect(&s, 1));
        assert!(a.undo_edit(&mut s, 0).is_err(), "only once");
        // Undoing the revert brings the edit back into effect.
        assert!(s.undo());
        assert!(a.in_effect(&s, 0));
        assert_eq!(at(&s, [0, 0, 0]), 3);

        // Cells changed since are not overwritten.
        by_hand(&mut s, &[([5, 0, 5], 8)]);
        assert!(a.undo_edit(&mut s, 1).unwrap_err().contains("edited since"));
        assert_eq!(at(&s, [5, 0, 5]), 8);

        // The agent's undo takes back its own last edit in effect, not the user's.
        assert!(s.undo());
        let t = ok(&mut a, &mut s, 1, "Claude Code", "undo", json!({}));
        assert_eq!(t, "took back Claude Code: place 1 voxel");
        assert_eq!((at(&s, [5, 0, 5]), at(&s, [6, 0, 6])), (0, 6));
        assert!(a.call(&mut s, 2, "Codex", "undo", &json!({})).unwrap_err().contains("nothing of yours"));
        // The last step undoes as Cmd-Z does: redo brings it back.
        let t = ok(&mut a, &mut s, 1, "Claude Code", "undo", json!({}));
        assert_eq!(t, "took back Claude Code: place 2 voxels");
        assert_eq!(at(&s, [0, 0, 0]), 0);
    }

    /// Agent calls as the editor runs them with "Review every agent change" on: an edit
    /// opens the agents' change under review, later calls go into it while it is open.
    fn staged(
        a: &mut Agents,
        s: &mut Scene,
        r: &mut Option<Review>,
        conn: u64,
        client: &str,
        name: &str,
        args: Value,
    ) -> Result<String, String> {
        let notice = a.take_notices(conn);
        let out = if r.is_some() || (a.review && edits(name, &args)) {
            let review = r.get_or_insert_with(|| Review::for_agents(s));
            a.stage(s, review, conn, client, name, &args)
        } else {
            a.call(s, conn, client, name, &args)
        };
        match notice {
            Some(n) => out.map(|t| format!("{n}\n{t}")),
            None => out,
        }
    }

    #[test]
    fn with_review_on_edits_wait_until_applied_as_steps_named_after_the_agent() {
        let (mut a, mut s, mut r) = (Agents::default(), scene(), None);
        a.review = true;
        a.clients = vec![(1, "Claude Code".into()), (2, "Codex".into())];
        let position = s.history().position();
        // A query and a selection open nothing.
        staged(&mut a, &mut s, &mut r, 1, "Claude Code", "scene_summary", json!({})).unwrap();
        staged(&mut a, &mut s, &mut r, 1, "Claude Code", "select", json!({"from": [0, 0, 0], "to": [1, 0, 0]}))
            .unwrap();
        assert!(r.is_none());
        let t = staged(
            &mut a,
            &mut s,
            &mut r,
            1,
            "Claude Code",
            "box",
            json!({"from": [0, 0, 0], "to": [3, 0, 3], "slot": 5}),
        )
        .unwrap();
        assert!(t.ends_with(WAITING), "{t}");
        assert_eq!(s.history().position(), position, "no undo step while it waits");
        staged(&mut a, &mut s, &mut r, 1, "Claude Code", "select", json!({"from": [0, 0, 0], "to": [1, 0, 0]}))
            .unwrap();
        // Another agent adds to the same change, with its own selection.
        staged(&mut a, &mut s, &mut r, 2, "Codex", "place", json!({"cells": [[0, 1, 0]], "slot": 7})).unwrap();
        staged(&mut a, &mut s, &mut r, 1, "Claude Code", "paint", json!({"slot": 9, "selection": true})).unwrap();
        staged(&mut a, &mut s, &mut r, 2, "Codex", "place", json!({"cells": [[5, 1, 5]], "slot": 7})).unwrap();
        // An agent's undo drops its own last edit from the change.
        let t = staged(&mut a, &mut s, &mut r, 2, "Codex", "undo", json!({})).unwrap();
        assert!(t.starts_with("took back step") && t.ends_with(WAITING), "{t}");
        // A query sees the change; status says what waits.
        let review = r.as_ref().unwrap();
        assert_eq!(review.card(&s).prompt, "Claude Code and Codex");
        assert!(review.card(&s).agents);
        assert_eq!(review.card(&s).counts, [17, 0, 0]);
        assert!(review.card(&s).steps[0].detail.starts_with("Claude Code · "));
        assert!(a.status(1, r.as_ref()).starts_with("2 edits of yours wait"));
        // Edit another object, or undo without a step waiting: refused.
        assert!(
            staged(&mut a, &mut s, &mut r, 3, "Gemini CLI", "undo", json!({}))
                .unwrap_err()
                .contains("nothing of yours waits")
        );
        // Apply: one undo step per edit kept, named as without review.
        let n = a.apply_review(&mut s, r.take().unwrap());
        assert_eq!(n, 3);
        let labels: Vec<&str> = a.edits.iter().map(|e| e.label.as_str()).collect();
        assert_eq!(labels, ["Claude Code: box 16 voxels", "Codex: place 1 voxel", "Claude Code: paint 2 voxels"]);
        assert_eq!(s.history().position(), position + 3);
        assert_eq!(
            (at(&s, [0, 0, 0]), at(&s, [1, 0, 0]), at(&s, [2, 0, 0]), at(&s, [0, 1, 0]), at(&s, [5, 1, 5])),
            (9, 9, 5, 7, 0)
        );
        assert!((0..3).all(|i| a.in_effect(&s, i)));
        // Each agent hears it on its next call, once; status after that has nothing new.
        let t = staged(&mut a, &mut s, &mut r, 2, "Codex", "scene_summary", json!({})).unwrap();
        assert!(t.starts_with("vxr: the user applied 1 of your 2 edits"), "{t}");
        let t = a.status(1, None);
        assert!(t.starts_with("vxr: the user applied your 2 edits that waited for review"), "{t}");
        assert!(a.status(1, None).starts_with("Nothing of yours waits for review. Review is on"));
        // The agent's undo after Apply takes back its own applied step, as without review.
        assert_eq!(
            staged(&mut a, &mut s, &mut r, 1, "Claude Code", "undo", json!({})).unwrap(),
            "took back Claude Code: paint 2 voxels"
        );

        // Discard: nothing lands and the agent is told.
        let before = s.history().position();
        staged(&mut a, &mut s, &mut r, 1, "Claude Code", "erase", json!({"cells": [[0, 0, 0]]})).unwrap();
        assert_eq!(at(&s, [0, 0, 0]), 5, "a staged removal still shows, as a ghost");
        assert_eq!(r.as_ref().unwrap().card(&s).counts, [0, 1, 0]);
        a.discard_review(&mut s, r.take().unwrap());
        assert_eq!((at(&s, [0, 0, 0]), s.history().position()), (5, before));
        let t = staged(&mut a, &mut s, &mut r, 1, "Claude Code", "scene_summary", json!({})).unwrap();
        assert!(t.starts_with("vxr: the user discarded the change under review: none of your 1 edit"), "{t}");
        // A closed connection is not told.
        staged(&mut a, &mut s, &mut r, 2, "Codex", "place", json!({"cells": [[6, 0, 6]], "slot": 3})).unwrap();
        a.forget(2);
        a.clients.retain(|c| c.0 != 2);
        a.discard_review(&mut s, r.take().unwrap());
        assert!(a.take_notices(2).is_none());
        // Off: edits apply at once again.
        a.review = false;
        staged(&mut a, &mut s, &mut r, 1, "Claude Code", "place", json!({"cells": [[6, 0, 6]], "slot": 3})).unwrap();
        assert!(r.is_none());
        assert_eq!(s.history().undo_label(), Some("Claude Code: place 1 voxel"));
    }

    #[test]
    fn apply_names_each_landed_edit_after_its_own_agent() {
        let (mut a, mut s, mut r) = (Agents::default(), scene(), None);
        a.review = true;
        a.clients = vec![(1, "Claude Code".into()), (2, "Codex".into())];
        let position = s.history().position();
        staged(&mut a, &mut s, &mut r, 1, "Claude Code", "palette_set", json!({"slot": 5, "hex": "#ff0000"})).unwrap();
        // The colour the slot already has with the change: it lands as nothing.
        staged(&mut a, &mut s, &mut r, 2, "Codex", "palette_set", json!({"slot": 5, "hex": "#ff0000"})).unwrap();
        staged(&mut a, &mut s, &mut r, 1, "Claude Code", "box", json!({"from": [0, 0, 0], "to": [1, 0, 0], "slot": 5}))
            .unwrap();
        staged(&mut a, &mut s, &mut r, 2, "Codex", "place", json!({"cells": [[5, 0, 5]], "slot": 7})).unwrap();
        assert_eq!(a.apply_review(&mut s, r.take().unwrap()), 3);
        let edits: Vec<(u64, &str)> = a.edits.iter().map(|e| (e.conn, e.label.as_str())).collect();
        assert_eq!(
            edits,
            [(1, "Claude Code: palette_set slot 5"), (1, "Claude Code: box 2 voxels"), (2, "Codex: place 1 voxel")]
        );
        assert_eq!(s.history().position(), position + 3);
        // Each agent's undo takes back its own edit.
        assert_eq!(
            staged(&mut a, &mut s, &mut r, 2, "Codex", "undo", json!({})).unwrap().lines().last(),
            Some("took back Codex: place 1 voxel")
        );
        assert_eq!((at(&s, [5, 0, 5]), at(&s, [0, 0, 0])), (0, 5));
        assert_eq!(
            staged(&mut a, &mut s, &mut r, 1, "Claude Code", "undo", json!({})).unwrap().lines().last(),
            Some("took back Claude Code: box 2 voxels")
        );
        assert_eq!(at(&s, [0, 0, 0]), 0);
    }

    #[test]
    fn with_review_on_the_cell_limit_counts_each_call() {
        let (mut a, mut s, mut r) = (Agents::default(), Scene::new([64, 64, 64]), None);
        a.review = true;
        let position = s.history().position();
        let lo = s.bounds().lo;
        let [x, z] = [lo[0], lo[2]];
        let half = |y: i32| json!({"from": [x, lo[1] + y, z], "to": [x + 63, lo[1] + y + 31, z + 63], "slot": 3});
        // Each call changes 131,072 cells: both fit, though together they pass 200,000.
        staged(&mut a, &mut s, &mut r, 1, "Claude Code", "box", half(0)).unwrap();
        staged(&mut a, &mut s, &mut r, 2, "Codex", "box", half(32)).unwrap();
        assert_eq!(r.as_ref().unwrap().session.staged(), 262_144);
        // One call over the limit is still refused.
        let all = json!({"from": lo, "to": [x + 63, lo[1] + 63, z + 63], "mode": "erase"});
        let e = staged(&mut a, &mut s, &mut r, 1, "Claude Code", "box", all).unwrap_err();
        assert!(e.contains("the limit is 200000 per call"), "{e}");
        assert_eq!(a.apply_review(&mut s, r.take().unwrap()), 2);
        assert_eq!(s.history().position(), position + 2);
        let labels: Vec<&str> = a.edits.iter().map(|e| e.label.as_str()).collect();
        assert_eq!(labels, ["Claude Code: box 131,072 voxels", "Codex: box 131,072 voxels"]);
    }

    #[test]
    fn edit_object_waits_for_the_review() {
        let f = vxr_vox::fixtures::all().into_iter().find(|f| f.name == "22-world").unwrap();
        let mut s = Scene::from_vox(&f.builder.build().write()).unwrap();
        let (mut a, mut r) = (Agents::default(), None);
        a.review = true;
        let objects = s.world().unwrap().objects();
        let (first, second) = (objects[0], objects[1]);
        staged(&mut a, &mut s, &mut r, 1, "Claude Code", "edit_object", json!({"node": first})).unwrap();
        staged(&mut a, &mut s, &mut r, 1, "Claude Code", "place", json!({"cells": [[0, 0, 0]], "slot": 3})).unwrap();
        assert!(r.is_some());
        assert!(staged(&mut a, &mut s, &mut r, 1, "Claude Code", "edit_object", json!({"node": first})).is_ok());
        let e = staged(&mut a, &mut s, &mut r, 1, "Claude Code", "edit_object", json!({"node": second})).unwrap_err();
        assert!(e.contains("wait for the user's review"), "{e}");
        assert_eq!(s.at(), At::Object(first));
    }

    #[test]
    fn an_agent_keeps_its_own_selection() {
        let (mut a, mut s) = (Agents::default(), scene());
        by_hand(&mut s, &[([0, 0, 0], 2), ([1, 0, 0], 2), ([2, 0, 0], 2)]);
        ok(&mut a, &mut s, 1, "Claude Code", "select", json!({"from": [0, 0, 0], "to": [1, 0, 0]}));
        assert_eq!(s.selected_count(), 0, "the user's selection is untouched");
        ok(&mut a, &mut s, 1, "Claude Code", "paint", json!({"slot": 9, "selection": true}));
        assert_eq!((at(&s, [0, 0, 0]), at(&s, [1, 0, 0]), at(&s, [2, 0, 0])), (9, 9, 2));
        assert_eq!(s.history().undo_label(), Some("Claude Code: paint 2 voxels"));
    }

    #[test]
    fn agents_of_the_same_name_are_told_apart_by_connection() {
        let (mut a, mut s) = (Agents::default(), scene());
        by_hand(&mut s, &[([0, 0, 0], 2), ([1, 0, 0], 2)]);
        ok(&mut a, &mut s, 1, "Claude Code", "select", json!({"from": [0, 0, 0], "to": [0, 0, 0]}));
        ok(&mut a, &mut s, 2, "Claude Code", "select", json!({"from": [1, 0, 0], "to": [1, 0, 0]}));
        ok(&mut a, &mut s, 1, "Claude Code", "paint", json!({"slot": 9, "selection": true}));
        assert_eq!((at(&s, [0, 0, 0]), at(&s, [1, 0, 0])), (9, 2), "each keeps its own selection");
        assert!(a.call(&mut s, 2, "Claude Code", "undo", &json!({})).unwrap_err().contains("nothing of yours"));
        assert_eq!(at(&s, [0, 0, 0]), 9);
        ok(&mut a, &mut s, 1, "Claude Code", "undo", json!({}));
        assert_eq!(at(&s, [0, 0, 0]), 2);
    }

    #[test]
    fn a_closed_connection_takes_its_selection_with_it() {
        let (mut a, mut s) = (Agents::default(), scene());
        by_hand(&mut s, &[([0, 0, 0], 2), ([1, 0, 0], 2)]);
        ok(&mut a, &mut s, 1, "Claude Code", "select", json!({"from": [1, 0, 0], "to": [1, 0, 0]}));
        a.forget(1);
        let _ = a.call(&mut s, 1, "Claude Code", "paint", &json!({"slot": 9, "selection": true}));
        assert_eq!(at(&s, [1, 0, 0]), 2, "the old connection's selection is gone");
        assert_eq!(a.edits.len(), 0);
    }

    #[test]
    fn a_selection_is_dropped_once_the_map_changes() {
        let (mut a, mut s) = (Agents::default(), scene());
        by_hand(&mut s, &[([0, 0, 0], 2), ([1, 0, 0], 2)]);
        ok(&mut a, &mut s, 1, "Claude Code", "select", json!({"from": [1, 0, 0], "to": [1, 0, 0]}));
        // The user grows the model on its low side: every map cell moves.
        let b = s.bounds();
        let size = s.map.size_voxels();
        s.set_bounds([b.lo[0] - 1, b.lo[1], b.lo[2]], [size[0] + 1, size[1], size[2]]).unwrap();
        // Nor is the user's selection taken in its place.
        s.select_all();
        assert_eq!(s.selected_count(), 2);
        let _ = a.call(&mut s, 1, "Claude Code", "paint", &json!({"slot": 9, "selection": true}));
        assert_eq!((at(&s, [0, 0, 0]), at(&s, [1, 0, 0])), (2, 2), "no cell painted through a stale selection");
    }
}
