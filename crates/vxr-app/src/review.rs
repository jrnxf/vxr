//! The review step for an AI change (`docs/research/ai.md`, "Keeping a change reviewable"):
//! a [`vxr_ops::Session`] staged on the live scene, the calls held until the user agrees to a
//! bigger change, and what the review card shows. The editor draws the staged cells as a
//! ghost and keeps every other edit out while a review is open; Apply makes it one undo step
//! named after the request, Discard or Escape leaves none.
//!
//! Terminal agents' edits go through the same step when the user turns on "Review every
//! agent change" ([`Review::for_agents`], [`crate::agents::Agents::stage`]): one change that
//! every agent adds to, each step in its agent's lane, applied as one undo step per edit.
//!
//! Until the assistant exists, a [`Script`] drives it: a request and its tool calls, run
//! through the same session, card and ghost as a model's would be.

use serde_json::Value;
use vxr_core::Scene;
use vxr_ops::{Changes, ErrorKind, Session};

use crate::ui::{ReviewCard, SlotChange, StepRow, WorldLine};

/// One tool call, as a model sends it: the tool's name and its JSON input.
#[derive(Clone, Debug, PartialEq)]
pub struct Call {
    pub name: String,
    pub input: Value,
}

/// A request and the calls that answer it, read from JSON:
/// `{"prompt": "make the roof red", "calls": [{"name": "paint", "input": {...}}, ...]}`.
/// `calls` may instead be `turns`, a list of call lists, each one model turn.
#[derive(Clone, Debug, PartialEq)]
pub struct Script {
    pub prompt: String,
    pub turns: Vec<Vec<Call>>,
}

impl Script {
    pub fn parse(json: &str) -> Result<Script, String> {
        let v: Value = serde_json::from_str(json).map_err(|e| format!("not JSON: {e}"))?;
        let prompt = v.get("prompt").and_then(Value::as_str).ok_or("the script needs a \"prompt\" string")?;
        let calls = |list: &Value| -> Result<Vec<Call>, String> {
            let list = list.as_array().ok_or("calls are a list")?;
            list.iter()
                .map(|c| {
                    let name = c.get("name").and_then(Value::as_str).ok_or("each call needs a \"name\"")?;
                    let input = c.get("input").cloned().unwrap_or_else(|| Value::Object(Default::default()));
                    Ok(Call { name: name.into(), input })
                })
                .collect()
        };
        let turns = match (v.get("calls"), v.get("turns")) {
            (Some(c), None) => vec![calls(c)?],
            (None, Some(t)) => {
                t.as_array().ok_or("turns are a list of call lists")?.iter().map(calls).collect::<Result<_, _>>()?
            }
            _ => return Err("the script needs \"calls\" or \"turns\"".into()),
        };
        Ok(Script { prompt: prompt.trim().into(), turns })
    }
}

/// What one call answered: its text, or the error and whether it was held for the user.
#[derive(Clone, Debug, PartialEq)]
pub enum Answer {
    Ok(String),
    Err(String),
    /// Over the cells the user allowed: not run, kept until Allow or Stop.
    Held,
}

pub struct Review {
    pub session: Session,
    /// The request, as the user wrote it.
    pub prompt: String,
    /// Turns not run yet because the change would grow past what the user allowed: the rest of
    /// the turn that asked, its first call the one that asked, then every turn after it.
    pub held: Vec<Vec<Call>>,
    /// The change as of its last edit, which Before keeps showing on the card.
    changes: Changes,
    /// Bumped on every change to the staged cells, the palette or the steps.
    pub version: u64,
    /// Terminal agents' edits, not the assistant's: each agent that made a step, by
    /// connection (its lane) and name, in the order they first did.
    pub agents: Option<Vec<(u64, String)>>,
}

impl Review {
    pub fn new(scene: &Scene, prompt: &str) -> Review {
        Review {
            session: Session::new(scene, None),
            prompt: prompt.into(),
            held: Vec::new(),
            changes: Changes::default(),
            version: 0,
            agents: None,
        }
    }

    /// An empty change for terminal agents' edits, under their limits.
    pub fn for_agents(scene: &Scene) -> Review {
        Review {
            session: Session::with_limits(scene, None, crate::agents::limits()),
            agents: Some(Vec::new()),
            ..Review::new(scene, "")
        }
    }

    /// The agents whose edits are in the change, by name: "Claude Code", "Claude Code and
    /// Codex", "3 agents".
    fn agent_names(&self) -> String {
        let names: Vec<&str> = self.agents.iter().flatten().map(|(_, n)| n.as_str()).collect();
        match names.as_slice() {
            [] => "Terminal agents".into(),
            [one] => one.to_string(),
            [a, b] => format!("{a} and {b}"),
            many => format!("{} agents", many.len()),
        }
    }
    /// The undo step Apply makes: "AI: make the roof red", cut to 80 characters.
    pub fn label(&self) -> String {
        let p: String = vxr_ops::sanitize(&self.prompt);
        let short: String = p.chars().take(76).collect();
        let dots = if short.len() < p.len() { "…" } else { "" };
        format!("AI: {short}{dots}")
    }

    /// Run one model turn. Once a call is held, it and the rest wait for the user, and so do
    /// later turns.
    pub fn turn(&mut self, scene: &mut Scene, calls: Vec<Call>) -> Vec<Answer> {
        if !self.held.is_empty() {
            self.held.push(calls);
            return Vec::new();
        }
        self.session.begin_turn();
        self.run(scene, calls)
    }

    fn run(&mut self, scene: &mut Scene, calls: Vec<Call>) -> Vec<Answer> {
        let mut out = Vec::new();
        let mut calls = calls.into_iter();
        while let Some(c) = calls.next() {
            match self.session.call(scene, &c.name, c.input.clone()) {
                Ok(t) => out.push(Answer::Ok(t)),
                Err(e) if e.kind == ErrorKind::NeedsApproval => {
                    out.push(Answer::Held);
                    self.held.push(std::iter::once(c).chain(calls).collect());
                    break;
                }
                Err(e) => out.push(Answer::Err(e.message)),
            }
        }
        self.changed(scene);
        out
    }

    /// The change moved: count it again for the card.
    pub fn changed(&mut self, scene: &Scene) {
        self.changes = self.session.changes(scene);
        self.version += 1;
    }

    /// The user allowed a bigger change: stage up to the request's limit and run what was held,
    /// the rest of the turn that asked and then each later turn as its own turn.
    pub fn allow(&mut self, scene: &mut Scene) -> Vec<Answer> {
        self.session.approve_more();
        let mut turns = std::mem::take(&mut self.held).into_iter();
        let mut out = turns.next().map(|t| self.run(scene, t)).unwrap_or_default();
        for t in turns {
            out.extend(self.turn(scene, t));
        }
        out
    }

    /// The user stopped the held calls: they are dropped, what is staged stays.
    pub fn stop(&mut self) {
        self.held.clear();
        self.version += 1;
    }

    pub fn set_dropped(&mut self, scene: &mut Scene, i: usize, dropped: bool) {
        if self.session.set_dropped(scene, i, dropped) {
            self.changed(scene);
        }
    }

    pub fn show_before(&mut self, scene: &mut Scene, on: bool) {
        if self.session.showing_before() != on {
            self.session.show_before(scene, on);
            self.version += 1;
        }
    }

    /// The staged cells as the renderer's ghost mask (empty while Before shows).
    pub fn ghost_mask(&self, scene: &Scene) -> Vec<u32> {
        let cells = self.session.ghost(scene);
        scene.ghost_mask(cells.into_iter().map(|(p, g)| {
            let kind = match g {
                vxr_ops::Ghost::Added => 1,
                // A new object's voxels keep their colour, outlined, as repainted ones do.
                vxr_ops::Ghost::Repainted | vxr_ops::Ghost::Moved => 2,
                vxr_ops::Ghost::Removed => 3,
            };
            (p, kind)
        }))
    }

    /// What the review card shows. Counted from the change itself, so it holds in Before too.
    pub fn card(&self, scene: &Scene) -> ReviewCard {
        let s = &self.session;
        let c = &self.changes;
        let mut slots: Vec<SlotChange> =
            c.palette.iter().map(|&(slot, old, new)| SlotChange { slot, old, new, material: None }).collect();
        for &(i, old, new) in &c.materials {
            let text = if old.kind == new.kind {
                format!("{} settings", new.kind.name())
            } else {
                format!("{} → {}", old.kind.name(), new.kind.name())
            };
            match slots.iter_mut().find(|c| c.slot == i) {
                Some(c) => c.material = Some(text),
                None => {
                    let c = scene.palette[i as usize];
                    slots.push(SlotChange { slot: i, old: c, new: c, material: Some(text) })
                }
            }
        }
        slots.sort_by_key(|c| c.slot);
        // `done` is not a change: its summary heads the card. With several agents each step
        // says whose it is.
        let several = self.agents.as_ref().is_some_and(|a| a.len() > 1);
        let who =
            |lane: u64| self.agents.iter().flatten().find(|(c, _)| *c == lane).map_or("Agent", |(_, n)| n.as_str());
        let steps = s
            .steps()
            .iter()
            .enumerate()
            .filter(|(_, st)| st.tool != "done")
            .map(|(index, st)| StepRow {
                index,
                tool: st.tool,
                detail: if several { format!("{} · {}", who(st.lane), step_detail(st)) } else { step_detail(st) },
                ok: st.ok,
                dropped: st.dropped,
                droppable: st.ok && !matches!(st.tool, "undo" | "redo"),
            })
            .collect();
        ReviewCard {
            prompt: if self.agents.is_some() { self.agent_names() } else { self.prompt.clone() },
            agents: self.agents.is_some(),
            summary: s.done().map(str::to_string),
            counts: [c.added.len(), c.removed.len(), c.repainted.len()],
            objects: c.nodes,
            world: world_lines(c),
            slots,
            steps,
            held: (!self.held.is_empty()).then_some((s.allowed(), s.limits.changed_cells)),
            before: s.showing_before(),
            working: None,
        }
    }
}

/// The card's lines for the new objects and names a change makes: "NEW 2", "\"mug\"",
/// "60 voxels"; "OBJECT 1", "named \"desk\"".
fn world_lines(c: &Changes) -> Vec<WorldLine> {
    let quoted = |n: &Option<String>| n.as_deref().map(|n| Value::from(n).to_string());
    let made = c.objects.iter().map(|o| WorldLine {
        node: o.node,
        label: format!("NEW {}", o.node),
        text: quoted(&o.name).unwrap_or_else(|| "object".into()),
        detail: format!(
            "{} voxel{}",
            crate::ui::group_thousands(o.voxels as u64),
            if o.voxels == 1 { "" } else { "s" }
        ),
    });
    let named = c.names.iter().map(|(node, name)| WorldLine {
        node: *node,
        label: format!("OBJECT {node}"),
        text: quoted(name).map_or_else(|| "name cleared".into(), |n| format!("named {n}")),
        detail: String::new(),
    });
    made.chain(named).collect()
}

/// A step's line on the card: what it answered up to the first `;` ("8 cells changed", or the
/// error), or for a palette slot its colours.
fn step_detail(st: &vxr_ops::Step) -> String {
    let first = st.result.split(';').next().unwrap_or_default().trim();
    let part = st.args.get("name").and_then(Value::as_str).filter(|n| !n.is_empty());
    match part {
        Some(n) if st.ok => format!("{first} · {}", vxr_ops::clean(n.as_bytes())),
        _ => first.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    /// An 8x8 floor of slot 10 in the corner of a 32^3 model (world x and z from -16).
    fn scene() -> Scene {
        let mut s = Scene::new([32, 32, 32]);
        s.edit((0..8).flat_map(|x| (0..8).map(move |z| ([x, 0, z], 10u8))));
        s
    }

    #[test]
    fn scripts_read_calls_or_turns() {
        let a = Script::parse(
            r#"{"prompt": " roof ", "calls": [{"name": "box", "input": {"slot": 3}}, {"name": "done"}]}"#,
        )
        .unwrap();
        assert_eq!(a.prompt, "roof");
        assert_eq!(a.turns.len(), 1);
        assert_eq!(a.turns[0][1], Call { name: "done".into(), input: json!({}) });
        let b = Script::parse(r#"{"prompt": "p", "turns": [[{"name": "undo"}], []]}"#).unwrap();
        assert_eq!(b.turns.len(), 2);
        assert!(Script::parse(r#"{"calls": []}"#).is_err());
        assert!(Script::parse(r#"{"prompt": "p"}"#).is_err());
        assert!(Script::parse("nope").is_err());
    }

    #[test]
    fn a_big_change_is_held_until_allowed_or_stopped() {
        let mut s = scene();
        let mut r = Review::new(&s, "make a big block");
        let calls = vec![
            Call { name: "box".into(), input: json!({"from": [-16, 1, -16], "to": [15, 10, 15], "slot": 3}) },
            Call { name: "done".into(), input: json!({"summary": "a block"}) },
        ];
        let answers = r.turn(&mut s, calls.clone());
        assert_eq!(answers, vec![Answer::Held]);
        assert_eq!(r.held, vec![calls]);
        assert_eq!(r.card(&s).held, Some((10_000, 200_000)));
        assert_eq!(r.session.staged(), 0);
        let answers = r.allow(&mut s);
        assert!(matches!(answers[0], Answer::Ok(_)), "{answers:?}");
        assert!(r.held.is_empty());
        let card = r.card(&s);
        assert_eq!(card.counts, [32 * 10 * 32, 0, 0]);
        assert_eq!(card.summary.as_deref(), Some("a block"));
        assert_eq!(card.held, None);
        // Stop drops what is held and keeps what is staged.
        let mut r2 = Review::new(&s, "again");
        r2.turn(
            &mut s,
            vec![Call { name: "box".into(), input: json!({"from": [-16, 11, -16], "to": [15, 25, 15], "slot": 4}) }],
        );
        assert_eq!(r2.held.len(), 1);
        r2.stop();
        assert!(r2.held.is_empty());
    }

    #[test]
    fn held_turns_run_as_their_own_turns_when_allowed() {
        let mut s = scene();
        let mut r = Review::new(&s, "a block and dots");
        let dot = |x: i32| Call { name: "box".into(), input: json!({"from": [x, 20, 0], "to": [x, 20, 0], "slot": 5}) };
        let mut first =
            vec![Call { name: "box".into(), input: json!({"from": [-16, 1, -16], "to": [15, 10, 15], "slot": 3}) }];
        first.extend((-16..-7).map(dot));
        let second: Vec<Call> = (0..10).map(dot).collect();
        assert_eq!(r.turn(&mut s, first), vec![Answer::Held]);
        assert_eq!(r.turn(&mut s, second), vec![]);
        assert_eq!(r.held.len(), 2);
        let answers = r.allow(&mut s);
        assert_eq!(answers.len(), 20);
        assert!(answers.iter().all(|a| matches!(a, Answer::Ok(_))), "{answers:?}");
        assert!(r.held.is_empty());
        assert_eq!(r.card(&s).counts, [32 * 10 * 32 + 19, 0, 0]);
    }

    #[test]
    fn the_card_counts_steps_slots_and_materials() {
        let mut s = scene();
        let mut r = Review::new(&s, "paint it");
        r.turn(
            &mut s,
            vec![
                Call { name: "paint".into(), input: json!({"slot": 3, "from_slot": 10}) },
                Call { name: "erase".into(), input: json!({"cells": [[-16, 0, -16]]}) },
                Call { name: "place".into(), input: json!({"cells": [[-16, 1, -15]], "slot": 3, "name": "flag"}) },
                Call { name: "palette_set".into(), input: json!({"slot": 3, "hex": "#ff0000"}) },
                Call { name: "erase".into(), input: json!({"part": "p999"}) },
                Call { name: "material_set".into(), input: json!({"slot": 9, "type": "metal"}) },
            ],
        );
        let card = r.card(&s);
        assert_eq!(card.counts, [1, 1, 63]);
        assert_eq!(card.slots.len(), 2);
        assert_eq!((card.slots[0].slot, card.slots[0].new), (3, [255, 0, 0, 255]));
        assert_eq!(card.slots[1].material.as_deref(), Some("Diffuse → Metal"));
        assert_eq!(card.steps.len(), 6);
        assert_eq!(card.steps[2].detail, "1 cells changed · flag");
        assert!(!card.steps[4].ok && !card.steps[4].droppable);
        // Dropping a step takes it out of the change; Before counts the same.
        r.set_dropped(&mut s, 0, true);
        assert_eq!(r.card(&s).counts, [1, 1, 0]);
        assert!(r.card(&s).steps[0].dropped);
        let v = r.version;
        r.show_before(&mut s, true);
        assert!(r.version > v);
        assert!(r.ghost_mask(&s).is_empty());
        assert!(r.card(&s).before);
        assert_eq!(r.card(&s).counts, [1, 1, 0]);
        assert_eq!(r.card(&s).slots[1].material.as_deref(), Some("Diffuse → Metal"));
        assert_eq!(s.materials[9].kind, vxr_core::material::Kind::Diffuse);
        r.show_before(&mut s, false);
        assert!(!r.ghost_mask(&s).is_empty());
        assert_eq!(r.label(), "AI: paint it");
    }
}
