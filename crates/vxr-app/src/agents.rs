//! Terminal agents editing the window through `vxr mcp --live` (`docs/research/ai.md`
//! section 3, live mode): each edit call applies at once as one undo step named after the
//! agent ("Claude Code: paint 412 voxels"), listed in the Agent edits card with an Undo of
//! its own. The socket is the host's ([`crate::live`]); this is what a call does to the
//! scene, with no GPU and no socket, so it is the same in tests.

use std::collections::HashMap;

use serde_json::{Value, json};
use vxr_core::Scene;
use vxr_core::history::At;
use vxr_ops::{Changes, Limits, Session, clean};

use crate::ui::group_thousands;

type Cell = [i32; 3];

/// One edit an agent applied: its undo step, and the step that took it back, if any.
#[derive(Clone, Debug, PartialEq)]
pub struct AgentEdit {
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
    /// Live mode can run here: the desktop app on a system with Unix domain sockets.
    pub available: bool,
    /// Why turning it on failed.
    pub error: Option<String>,
    /// Each agent's own selection (map cells), kept between its calls so it never changes
    /// the user's; it starts from the user's.
    selections: HashMap<String, Vec<Cell>>,
    /// Bumped whenever the edits change.
    pub version: u64,
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

    /// Forget every edit and selection: the scene they were made on is gone.
    pub fn clear(&mut self) {
        self.edits.clear();
        self.selections.clear();
        self.version += 1;
    }

    /// Run one tool call from `client` on the scene, as `vxr mcp` files mode does: an edit
    /// applies at once as one undo step named after the client.
    pub fn call(&mut self, scene: &mut Scene, client: &str, name: &str, args: &Value) -> Result<String, String> {
        if !vxr_mcp::live::is_tool(name) {
            return Err(format!("Unknown tool: {}", clean(name.as_bytes())));
        }
        match name {
            "undo" => {
                let i = (0..self.edits.len())
                    .rev()
                    .find(|&i| self.edits[i].client == client && self.in_effect(scene, i))
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
                self.selections.remove(client);
                self.query(scene, client, "scene_summary")
            }
            _ => self.run(scene, client, name, args),
        }
    }

    /// A resource's text, or None for a uri that names nothing.
    pub fn read(&mut self, scene: &mut Scene, uri: &str) -> Option<Result<String, String>> {
        match uri {
            "vxr://scene/summary" => Some(self.query(scene, "", "scene_summary")),
            "vxr://scene/palette" => Some(Ok(vxr_mcp::files::palette_text(scene, &[]))),
            _ => None,
        }
    }

    fn query(&mut self, scene: &mut Scene, client: &str, name: &str) -> Result<String, String> {
        self.run(scene, client, name, &json!({}))
    }

    /// One `vxr-ops` call in its own session, applied at once as one undo step.
    fn run(&mut self, scene: &mut Scene, client: &str, name: &str, args: &Value) -> Result<String, String> {
        // A terminal agent is not asked per 10,000 cells: the hard limit stays.
        let limits = Limits { ask_over: Limits::default().changed_cells, ..Limits::default() };
        let mut session = Session::with_limits(scene, None, limits);
        if let Some(sel) = self.selections.get(client) {
            session.set_selection(sel.iter().copied());
        }
        match session.call(scene, name, args.clone()) {
            Ok(text) => {
                if !client.is_empty() {
                    self.selections.insert(client.to_string(), session.selection().collect());
                }
                let label = label(client, name, &session.changes(scene));
                let before = scene.history().position();
                session.apply_named(scene, &label);
                let step = scene.history().position();
                if step != before {
                    self.edits.push(AgentEdit { client: client.to_string(), label, step, undone_by: None });
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

    fn ok(a: &mut Agents, s: &mut Scene, client: &str, name: &str, args: Value) -> String {
        a.call(s, client, name, &args).unwrap_or_else(|e| panic!("{name}: {e}"))
    }

    #[test]
    fn each_edit_is_one_undo_step_named_after_the_agent() {
        let (mut a, mut s) = (Agents::default(), scene());
        ok(&mut a, &mut s, "Claude Code", "box", json!({"from": [0, 0, 0], "to": [3, 0, 3], "slot": 5}));
        assert_eq!(s.history().undo_label(), Some("Claude Code: box 16 voxels"));
        ok(&mut a, &mut s, "Claude Code", "place", json!({"cells": [[0, 1, 0]], "slot": 7}));
        assert_eq!(s.history().undo_label(), Some("Claude Code: place 1 voxel"));
        ok(&mut a, &mut s, "Codex", "palette_set", json!({"slot": 5, "hex": "#ff0000"}));
        assert_eq!(s.history().undo_label(), Some("Codex: palette_set slot 5"));
        // Queries and a selection leave no step.
        let n = a.edits.len();
        ok(&mut a, &mut s, "Codex", "scene_summary", json!({}));
        ok(&mut a, &mut s, "Codex", "select", json!({"from": [0, 0, 0], "to": [1, 0, 1]}));
        assert_eq!(a.edits.len(), n);
        let labels: Vec<&str> = a.edits.iter().map(|e| e.label.as_str()).collect();
        assert_eq!(labels, ["Claude Code: box 16 voxels", "Claude Code: place 1 voxel", "Codex: palette_set slot 5"]);
        // An error stages nothing.
        assert!(a.call(&mut s, "Codex", "place", &json!({"cells": [[0, 1]], "slot": 7})).is_err());
        assert!(a.call(&mut s, "Codex", "list_files", &json!({})).unwrap_err().contains("Unknown tool"));
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
        ok(&mut a, &mut s, "Claude Code", "place", json!({"cells": [[0, 0, 0], [1, 0, 0]], "slot": 3}));
        ok(&mut a, &mut s, "Claude Code", "place", json!({"cells": [[5, 0, 5]], "slot": 4}));
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
        let t = ok(&mut a, &mut s, "Claude Code", "undo", json!({}));
        assert_eq!(t, "took back Claude Code: place 1 voxel");
        assert_eq!((at(&s, [5, 0, 5]), at(&s, [6, 0, 6])), (0, 6));
        assert!(a.call(&mut s, "Codex", "undo", &json!({})).unwrap_err().contains("nothing of yours"));
        // The last step undoes as Cmd-Z does: redo brings it back.
        let t = ok(&mut a, &mut s, "Claude Code", "undo", json!({}));
        assert_eq!(t, "took back Claude Code: place 2 voxels");
        assert_eq!(at(&s, [0, 0, 0]), 0);
    }

    #[test]
    fn an_agent_keeps_its_own_selection() {
        let (mut a, mut s) = (Agents::default(), scene());
        by_hand(&mut s, &[([0, 0, 0], 2), ([1, 0, 0], 2), ([2, 0, 0], 2)]);
        ok(&mut a, &mut s, "Claude Code", "select", json!({"from": [0, 0, 0], "to": [1, 0, 0]}));
        assert_eq!(s.selected_count(), 0, "the user's selection is untouched");
        ok(&mut a, &mut s, "Claude Code", "paint", json!({"slot": 9, "selection": true}));
        assert_eq!((at(&s, [0, 0, 0]), at(&s, [1, 0, 0]), at(&s, [2, 0, 0])), (9, 9, 2));
        assert_eq!(s.history().undo_label(), Some("Claude Code: paint 2 voxels"));
    }
}
