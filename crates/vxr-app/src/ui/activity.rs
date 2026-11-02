//! What a connected agent is doing, read from its latest calls: the one state the hint-bar chip
//! and the Agents tab both draw (`docs/decisions.md`, "Agent activity"), the plain words and icon
//! of each call, and the rows of the ACTIVITY list. Nothing here draws; vxr only knows when an
//! agent last called and what, never what the model is thinking, so the state counts the time
//! since its last answered call and says no more.

use super::{AgentsView, icon};
use crate::agents::AgentCall;

/// A call within this many seconds is activity (as the chip's "editing" once was).
pub const ACTIVE_SECS: f64 = 5.0;
/// No call for this long turns the chip and the banner amber and counts up.
pub const STALL_SECS: f64 = 45.0;
/// No call for this long: "may be stuck", with Stop made prominent.
pub const LONG_SECS: f64 = 120.0;
/// How long `done`'s summary stays in place of the idle line.
pub const DONE_SECS: f64 = 12.0;

/// Where an agent stands, from the time since its last call.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Live {
    /// A call has not been answered yet (a render, a New scene waiting for the user).
    Running,
    /// Called in the last [`ACTIVE_SECS`].
    Active,
    /// Called under [`STALL_SECS`] ago.
    Quiet,
    /// No call for [`STALL_SECS`] to [`LONG_SECS`].
    Stall,
    /// No call for [`LONG_SECS`] or more, however long, until a call, `done` or a disconnect.
    Long,
    /// It called `done`.
    Done,
    /// Connected and not calling.
    Idle,
}

impl Live {
    /// Which of two agents matters more to the one chip: the longer it has been silent, the more.
    pub fn rank(self) -> u8 {
        match self {
            Live::Long => 6,
            Live::Stall => 5,
            Live::Running => 4,
            Live::Active => 3,
            Live::Quiet => 2,
            Live::Done => 1,
            Live::Idle => 0,
        }
    }

    /// The warning states, drawn amber.
    pub fn warn(self) -> bool {
        matches!(self, Live::Stall | Live::Long)
    }

    /// The name a check reads it by.
    pub fn name(self) -> &'static str {
        match self {
            Live::Running => "running",
            Live::Active => "active",
            Live::Quiet => "quiet",
            Live::Stall => "stall",
            Live::Long => "long",
            Live::Done => "done",
            Live::Idle => "idle",
        }
    }
}

/// An agent's state at `now`, its latest call and the seconds since it was answered (since it
/// started, while it runs, and since the agent was listed when it has not called).
#[derive(Clone, Copy, Debug)]
pub struct Now<'a> {
    pub state: Live,
    pub call: Option<&'a AgentCall>,
    pub quiet: f64,
}

/// The state of an agent first listed at `since` whose calls are `calls`, oldest first.
pub fn agent_now<'a>(now: f64, since: f64, calls: &[&'a AgentCall]) -> Now<'a> {
    let Some(&last) = calls.last() else { return Now { state: Live::Idle, call: None, quiet: (now - since).max(0.0) } };
    let Some(answered) = last.answered else {
        return Now { state: Live::Running, call: Some(last), quiet: (now - last.started).max(0.0) };
    };
    let quiet = (now - answered).max(0.0);
    let state = if last.tool == "done" {
        if quiet < DONE_SECS { Live::Done } else { Live::Idle }
    } else if quiet < ACTIVE_SECS {
        Live::Active
    } else if quiet < STALL_SECS {
        Live::Quiet
    } else if quiet < LONG_SECS {
        Live::Stall
    } else {
        Live::Long
    };
    Now { state, call: Some(last), quiet }
}

impl AgentsView {
    /// Connection `conn`'s calls, oldest first.
    pub fn calls_of(&self, conn: u64) -> Vec<&AgentCall> {
        self.calls.iter().filter(|(c, _)| *c == conn).map(|(_, call)| call).collect()
    }

    /// Agent `i`'s state at `now`.
    pub fn now_of(&self, i: usize, now: f64) -> Now<'_> {
        let since = self.times.get(i).map_or(now, |t| t.0);
        agent_now(now, since, &self.calls_of(self.clients[i].0))
    }

    /// The agent the chip and the banner speak for: the one with the most to say; among agents in
    /// a warning state the longest silent, else the one that called last.
    pub fn lead(&self, now: f64) -> Option<(usize, Now<'_>)> {
        (0..self.clients.len()).map(|i| (i, self.now_of(i, now))).max_by(|a, b| {
            use std::cmp::Ordering::Equal;
            let by_quiet = a.1.quiet.partial_cmp(&b.1.quiet).unwrap_or(Equal);
            a.1.state.rank().cmp(&b.1.state.rank()).then(if a.1.state.warn() { by_quiet } else { by_quiet.reverse() })
        })
    }
}

/// "5 s", "1 min": an elapsed time in a few words.
pub fn ago(secs: f64) -> String {
    let s = secs.max(0.0) as u64;
    match s {
        0..=4 => "just now".into(),
        5..=59 => format!("{s} s"),
        60..=3599 => format!("{} min", s / 60),
        _ => format!("{} h", s / 3600),
    }
}

/// The longer form a stall counts in: "1 min 2 s".
pub fn span(secs: f64) -> String {
    let s = secs.max(0.0) as u64;
    match (s / 60, s % 60) {
        (0, s) => format!("{s} s"),
        (m, 0) => format!("{m} min"),
        (m, s) => format!("{m} min {s} s"),
    }
}

/// A call in plain words, as it runs and as it was: "Looking at the scene", "Made ‘mug’".
pub fn verb(c: &AgentCall, running: bool) -> String {
    let arg = &c.arg;
    let (now, past): (&str, String) = match c.tool.as_str() {
        "scene_summary" => ("Reading the scene", "Read the scene".into()),
        "find_parts" => {
            ("Finding parts", if arg.is_empty() { "Found parts".into() } else { format!("Found parts {arg}") })
        }
        "get_voxels" => ("Reading voxels", "Read voxels".into()),
        "slice" => ("Reading a slice", "Read a slice".into()),
        "look" => ("Looking at the scene", "Looked at the scene".into()),
        "make_object" => {
            ("Making an object", if arg.is_empty() { "Made an object".into() } else { format!("Made {arg}") })
        }
        "rename" => ("Renaming", if arg.is_empty() { "Renamed an object".into() } else { format!("Renamed to {arg}") }),
        "done" => ("Finishing", "Done".into()),
        "place" => ("Placing voxels", "Placed voxels".into()),
        "erase" => ("Erasing voxels", "Erased voxels".into()),
        "paint" => ("Painting voxels", "Painted voxels".into()),
        "box" => ("Drawing a box", "Drew a box".into()),
        "sphere" => ("Drawing a sphere", "Drew a sphere".into()),
        "circle" => ("Drawing a circle", "Drew a circle".into()),
        "line" => ("Drawing a line", "Drew a line".into()),
        "fill" => ("Filling", "Filled voxels".into()),
        "fill_inside" => ("Filling inside", "Filled inside".into()),
        "hollow" => ("Hollowing", "Hollowed the model".into()),
        "select" => ("Selecting", "Selected voxels".into()),
        "copy" => ("Copying", "Copied voxels".into()),
        "cut" => ("Cutting", "Cut voxels".into()),
        "paste" => ("Pasting", "Pasted voxels".into()),
        "delete" => ("Deleting voxels", "Deleted voxels".into()),
        "transform" => ("Transforming", "Transformed voxels".into()),
        "resize" => ("Resizing", "Resized the bounds".into()),
        "crop" => ("Cropping", "Cropped to content".into()),
        "palette_set" => ("Setting colours", "Set colours".into()),
        "palette_swap" => ("Swapping colours", "Swapped colours".into()),
        "palette_copy" => ("Copying a colour", "Copied a colour".into()),
        "material_set" => ("Setting a material", "Set a material".into()),
        "move_object" => ("Moving an object", "Moved an object".into()),
        "duplicate" => ("Duplicating", "Duplicated an object".into()),
        "delete_object" => ("Deleting an object", "Deleted an object".into()),
        "group" => ("Grouping", "Grouped objects".into()),
        "ungroup" => ("Ungrouping", "Ungrouped a group".into()),
        "hide" => ("Hiding", "Hid or showed an object".into()),
        "hide_layer" => ("Hiding a layer", "Hid or showed a layer".into()),
        "new_layer" => ("Adding a layer", "Added a layer".into()),
        "set_layer" => ("Moving to a layer", "Moved to a layer".into()),
        "edit_object" => ("Opening an object", "Opened an object".into()),
        "show_world" => ("Showing the world", "Showed the world".into()),
        "undo" => ("Undoing", "Undid an edit".into()),
        "redo" => ("Redoing", "Redid an edit".into()),
        "render_settings" => ("Changing render settings", "Changed render settings".into()),
        "export" => ("Exporting", "Exported".into()),
        "save" => ("Saving", "Saved".into()),
        "new_scene" => ("Starting a new scene", "Started a new scene".into()),
        "review_status" => ("Checking review", "Checked review status".into()),
        other => return if running { format!("Calling {other}") } else { format!("Called {other}") },
    };
    if running { now.to_string() } else { past }
}

/// The icon that stands for a call: reading, looking, making, painting, taking back.
pub fn tool_icon(tool: &str) -> char {
    match tool {
        "scene_summary" => icon::LIST,
        "find_parts" => icon::SEARCH,
        "get_voxels" | "slice" => icon::GRID,
        "look" => icon::EYE,
        "make_object" | "duplicate" => icon::PACKAGE_PLUS,
        "rename" => icon::PENCIL,
        "move_object" => icon::MOVE,
        "erase" | "delete" | "cut" | "delete_object" => icon::ERASER,
        "paint" | "palette_set" | "palette_swap" | "palette_copy" | "material_set" => icon::PAINTBRUSH,
        "select" | "copy" | "paste" => icon::SELECT,
        "undo" => icon::UNDO,
        "redo" => icon::REDO,
        "done" => icon::CIRCLE_CHECK,
        "save" | "export" => icon::DOWNLOAD,
        "new_scene" => icon::FILE_PLUS,
        "hide" | "hide_layer" => icon::EYE_OFF,
        "group" | "ungroup" | "new_layer" | "set_layer" | "edit_object" | "show_world" => icon::LAYERS,
        "render_settings" => icon::APERTURE,
        "review_status" => icon::SHIELD_CHECK,
        _ => icon::BOX,
    }
}

/// "made ‘chair’" from "Made ‘chair’": a verb in the middle of a line.
pub fn lower_first(s: &str) -> String {
    let mut chars = s.chars();
    chars.next().map(|c| c.to_lowercase().chain(chars).collect()).unwrap_or_default()
}

/// One row of the ACTIVITY list, newest first.
#[derive(Clone, Debug, PartialEq)]
pub enum Item {
    /// A call: an index into [`AgentsView::calls`].
    Call(usize),
    /// A pause of `secs` with no call, by agent `who` (named when several are connected). The
    /// live one, still going, is at the top.
    Gap { secs: f64, live: bool, who: Option<String> },
}

/// How many calls the list shows.
pub const LIST_ROWS: usize = 10;

/// The ACTIVITY list at `now`: the newest [`LIST_ROWS`] calls of every agent, newest first, with
/// a gap row where an agent made no call for [`STALL_SECS`] or more, and the pause going on now
/// on top while an agent is in a warning state.
pub fn items(v: &AgentsView, now: f64) -> Vec<Item> {
    let mut order: Vec<usize> = (0..v.calls.len()).collect();
    order.sort_by(|&a, &b| v.calls[b].1.started.total_cmp(&v.calls[a].1.started));
    order.truncate(LIST_ROWS);
    let Some(&oldest) = order.last() else { return Vec::new() };
    let from = v.calls[oldest].1.started;
    let name = |conn: u64| {
        (v.clients.len() > 1).then(|| v.clients.iter().find(|c| c.0 == conn).map(|c| c.1.clone())).flatten()
    };
    let mut timed: Vec<(f64, Item)> = order.iter().map(|&i| (v.calls[i].1.started, Item::Call(i))).collect();
    for (conn, _) in &v.clients {
        let calls = v.calls_of(*conn);
        for pair in calls.windows(2) {
            let (a, b) = (pair[0], pair[1]);
            let secs = b.started - a.answered.unwrap_or(a.started);
            if secs >= STALL_SECS && b.started >= from {
                timed.push((b.started - 0.001, Item::Gap { secs, live: false, who: name(*conn) }));
            }
        }
    }
    timed.sort_by(|a, b| b.0.total_cmp(&a.0));
    let mut out: Vec<Item> = Vec::new();
    for (i, (conn, _)) in v.clients.iter().enumerate() {
        let n = v.now_of(i, now);
        if n.state.warn() {
            out.push(Item::Gap { secs: n.quiet, live: true, who: name(*conn) });
        }
    }
    out.extend(timed.into_iter().map(|t| t.1));
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn call(tool: &str, started: f64, answered: Option<f64>) -> AgentCall {
        AgentCall {
            tool: tool.into(),
            arg: String::new(),
            result: String::new(),
            ok: true,
            started,
            answered,
            edit: None,
            staged: None,
        }
    }

    #[test]
    fn the_state_follows_the_time_since_the_last_answered_call() {
        let c = call("box", 100.0, Some(100.0));
        let at = |now| agent_now(now, 0.0, &[&c]).state;
        assert_eq!(
            [at(102.0), at(110.0), at(150.0), at(230.0), at(5000.0)],
            [Live::Active, Live::Quiet, Live::Stall, Live::Long, Live::Long]
        );
        let running = call("look", 100.0, None);
        assert_eq!(agent_now(160.0, 0.0, &[&running]).state, Live::Running);
        let done = call("done", 100.0, Some(100.0));
        assert_eq!(
            [agent_now(105.0, 0.0, &[&done]).state, agent_now(130.0, 0.0, &[&done]).state],
            [Live::Done, Live::Idle]
        );
        assert_eq!(agent_now(50.0, 0.0, &[]).state, Live::Idle);
    }

    #[test]
    fn the_chip_speaks_for_the_longest_silent_warning_else_the_latest_call() {
        let view = |a: f64, b: f64| AgentsView {
            clients: vec![(1, "Claude Code".into()), (2, "Codex".into())],
            times: vec![(0.0, None), (0.0, None)],
            calls: vec![(1, call("box", a, Some(a))), (2, call("box", b, Some(b)))],
            ..Default::default()
        };
        // Both long, Claude Code silent for 10 min and Codex for 2 min 30 s.
        let long = view(0.0, 450.0);
        assert_eq!(long.lead(600.0).unwrap().0, 0);
        // Both active: the one that called last.
        let active = view(597.0, 599.0);
        assert_eq!(active.lead(600.0).unwrap().0, 1);
    }

    #[test]
    fn a_pause_of_45_s_is_a_gap_row() {
        let v = AgentsView {
            clients: vec![(1, "Claude Code".into())],
            times: vec![(0.0, Some(200.0))],
            calls: vec![
                (1, call("scene_summary", 10.0, Some(10.0))),
                (1, call("box", 100.0, Some(100.0))),
                (1, call("box", 103.0, Some(103.0))),
            ],
            ..Default::default()
        };
        let items = items(&v, 104.0);
        assert!(matches!(
            items.as_slice(),
            [Item::Call(2), Item::Call(1), Item::Gap { live: false, .. }, Item::Call(0)]
        ));
    }
}
