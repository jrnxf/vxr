//! Test 1 round 3: the `vxr-ops` crate behind the same server protocol. A request stages its
//! change in a `vxr_ops::Session`; `finish` applies it as the review card's Apply would, then
//! the change list is checked against the saved file as before.

use std::collections::BTreeMap;

use serde_json::{Value, json};
use vxr_core::Scene;
use vxr_ops::{ErrorKind, Session};

use crate::ops;

/// What `finish` needs from a request, the spike's own `Ops` or a staged session.
pub trait Run {
    fn card(&self) -> Value;
    fn changes(&self) -> Value;
    fn scene(&mut self) -> &mut Scene;
    fn before(&self) -> &BTreeMap<[i32; 3], u8>;
    fn before_palette(&self) -> [[u8; 4]; 256];
    fn focus(&self) -> Option<([i32; 3], [i32; 3])>;
}

impl Run for ops::Ops {
    fn card(&self) -> Value {
        ops::Ops::card(self)
    }
    fn changes(&self) -> Value {
        ops::Ops::changes(self)
    }
    fn scene(&mut self) -> &mut Scene {
        &mut self.scene
    }
    fn before(&self) -> &BTreeMap<[i32; 3], u8> {
        ops::Ops::before(self)
    }
    fn before_palette(&self) -> [[u8; 4]; 256] {
        ops::Ops::before_palette(self)
    }
    fn focus(&self) -> Option<([i32; 3], [i32; 3])> {
        ops::Ops::focus(self)
    }
}

pub struct Staged {
    scene: Scene,
    session: Option<Session>,
    before: BTreeMap<[i32; 3], u8>,
    before_palette: [[u8; 4]; 256],
    /// Set at `close`: the session's steps, summary and change counts before Apply.
    steps: Vec<Value>,
    done: Option<String>,
    staged: Value,
    /// Calls over the ask-first size, approved as the user would.
    approvals: usize,
}

impl Staged {
    pub fn new(scene: Scene, bytes: Option<&[u8]>) -> Staged {
        let session = Session::new(&scene, bytes);
        Staged {
            before: scene.world_voxels(false),
            before_palette: scene.palette,
            scene,
            session: Some(session),
            steps: Vec::new(),
            done: None,
            staged: Value::Null,
            approvals: 0,
        }
    }

    fn session(&mut self) -> &mut Session {
        self.session.as_mut().expect("open session")
    }

    pub fn summary(&mut self) -> String {
        let s = self.session.as_ref().expect("open session");
        s.summary(&self.scene)
    }

    pub fn turn(&mut self) {
        self.session().begin_turn();
    }

    pub fn call(&mut self, tool: &str, args: Value) -> Result<String, String> {
        let s = self.session.as_mut().expect("open session");
        match s.call(&mut self.scene, tool, args.clone()) {
            Err(e) if e.kind == ErrorKind::NeedsApproval => {
                self.approvals += 1;
                s.approve_more();
                s.call(&mut self.scene, tool, args).map_err(|e| e.message)
            }
            r => r.map_err(|e| e.message),
        }
    }

    /// Record the steps and the staged counts, then Apply.
    pub fn close(&mut self) {
        let Some(s) = self.session.take() else { return };
        let c = s.changes(&self.scene);
        self.staged = json!({
            "added": c.added.len(), "removed": c.removed.len(), "repainted": c.repainted.len(),
            "palette": c.palette.iter().map(|p| p.0).collect::<Vec<_>>(),
            "calls": s.calls(), "approvals": self.approvals,
        });
        self.steps = s
            .steps()
            .iter()
            .map(|t| json!({"tool": t.tool, "ok": t.ok, "dropped": t.dropped, "result": t.result}))
            .collect();
        self.done = s.done().map(str::to_string);
        s.apply(&mut self.scene);
    }
}

impl Run for Staged {
    fn card(&self) -> Value {
        let after = self.scene.world_voxels(false);
        let (added, removed, repainted) = ops::diff(&self.before, &after);
        let palette: Vec<Value> = (1..=255usize)
            .filter(|&i| self.scene.palette[i] != self.before_palette[i])
            .map(|i| json!({"slot": i, "from": ops::hex(self.before_palette[i]), "to": ops::hex(self.scene.palette[i]), "voxels": self.scene.map.counts[i]}))
            .collect();
        // The staged counts (what a review card shows before Apply) against the applied result.
        let staged_matches = self.staged["added"] == added.len()
            && self.staged["removed"] == removed.len()
            && self.staged["repainted"] == repainted.len();
        json!({
            "added": added.len(),
            "removed": removed.len(),
            "repainted": repainted.len(),
            "palette": palette,
            "steps": self.steps,
            "voxels_before": self.before.len(),
            "voxels_after": after.len(),
            "done": self.done,
            "staged": self.staged,
            "staged_matches_applied": staged_matches,
        })
    }

    fn changes(&self) -> Value {
        let after = self.scene.world_voxels(false);
        let (added, removed, repainted) = ops::diff(&self.before, &after);
        let mut out: Vec<[i32; 5]> = Vec::new();
        out.extend(added.iter().map(|(p, v)| [p[0], p[1], p[2], 0, *v as i32]));
        out.extend(removed.iter().map(|p| [p[0], p[1], p[2], self.before[p] as i32, 0]));
        out.extend(repainted.iter().map(|(p, o, v)| [p[0], p[1], p[2], *o as i32, *v as i32]));
        let palette: BTreeMap<String, String> =
            (1..=255usize).map(|i| (i.to_string(), ops::hex(self.scene.palette[i]))).collect();
        json!({"cells": out, "palette": palette})
    }

    fn scene(&mut self) -> &mut Scene {
        &mut self.scene
    }

    fn before(&self) -> &BTreeMap<[i32; 3], u8> {
        &self.before
    }

    fn before_palette(&self) -> [[u8; 4]; 256] {
        self.before_palette
    }

    fn focus(&self) -> Option<([i32; 3], [i32; 3])> {
        ops::focus(&self.before, &self.before_palette, &self.scene)
    }
}
