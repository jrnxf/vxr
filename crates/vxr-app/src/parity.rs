//! The "UI and agent tools in step" rule as a compile-time question: every [`Action`] says
//! which `vxr-ops` tool (or files/live tool) does the same, or that it is UI only.

use vxr_core::material::Material;
use vxr_core::select::Transform;

use crate::ui::Action;

/// The tool that does what `a` does, or `None` for a UI-only action (the camera, the view
/// guides, panels, the assistant and the Agents tab: see "Keep the UI and the agent tools in
/// step" in AGENTS.md). No wildcard arm: a new `Action` does not compile until it is
/// decided here, and a tool needs the same change in `vxr-ops`.
fn tool(a: &Action) -> Option<&'static str> {
    match a {
        Action::New => Some("new_scene"),
        Action::Open => Some("open"),
        Action::Save => Some("save"),
        Action::Undo => Some("undo"),
        Action::Redo => Some("redo"),
        Action::ExportObj | Action::ExportSlices | Action::ExportPng => Some("export"),
        Action::Recolor { .. } => Some("palette_set"),
        Action::SetMaterial { .. } => Some("material_set"),
        Action::SwapColors(..) => Some("palette_swap"),
        Action::CopyColor { .. } => Some("palette_copy"),
        Action::Transform(_) => Some("transform"),
        Action::SelectAll | Action::InvertSelection | Action::SelectColor | Action::Deselect => Some("select"),
        Action::DeleteSelection => Some("delete"),
        Action::FillSelection => Some("fill"),
        Action::Hollow => Some("hollow"),
        Action::FillInterior => Some("fill_inside"),
        Action::Resize(_) => Some("resize"),
        Action::Crop => Some("crop"),
        Action::ToObject => Some("make_object"),
        Action::ToggleWorld => Some("show_world"),
        Action::Duplicate { .. } => Some("duplicate"),
        Action::PickNode(_) => Some("edit_object"),
        Action::SetHidden(..) | Action::ToggleHidden => Some("hide"),
        Action::Group => Some("group"),
        Action::Ungroup => Some("ungroup"),
        Action::SetLayer(_) => Some("set_layer"),
        Action::SetLayerHidden(..) | Action::ToggleLayer => Some("hide_layer"),
        Action::AddLayer => Some("new_layer"),
        Action::Copy => Some("copy"),
        Action::Cut => Some("cut"),
        Action::Paste => Some("paste"),
        // UI only, on purpose.
        Action::Frame
        | Action::FrameSelection
        | Action::ResetCamera
        | Action::Zoom(_)
        | Action::Snap(_)
        | Action::Ortho(_)
        | Action::Embed
        // Closes the colour edit that `Recolor` and `SetMaterial` opened: tools are one step.
        | Action::CommitColor
        | Action::CancelExport
        | Action::Exported
        | Action::PlaceBox
        | Action::CancelBox
        | Action::ApplyReview
        | Action::DiscardReview
        | Action::DropStep(..)
        | Action::ReviewBefore(_)
        | Action::AllowMore
        | Action::StopHeld
        | Action::AllowAgents(_)
        | Action::ReviewAgents(_)
        | Action::PairBridge
        | Action::DismissAgentsError
        | Action::RegenerateToken
        | Action::StopAgent(_)
        | Action::StopAgents
        | Action::UndoAgentEdit(_)
        | Action::Theme => None,
    }
}

/// One of each `Action` that names a tool, so the names can be checked against the tool
/// lists (the match above is what forces a new variant to be decided).
fn with_tools() -> Vec<Action> {
    vec![
        Action::New,
        Action::Open,
        Action::Save,
        Action::Undo,
        Action::Redo,
        Action::ExportObj,
        Action::ExportSlices,
        Action::ExportPng,
        Action::Recolor { slot: 1, rgba: [0; 4] },
        Action::SetMaterial { slot: 1, material: Material::default() },
        Action::SwapColors(1, 2),
        Action::CopyColor { from: 1, to: 2 },
        Action::Transform(Transform::Rotate(0)),
        Action::SelectAll,
        Action::InvertSelection,
        Action::SelectColor,
        Action::Deselect,
        Action::DeleteSelection,
        Action::FillSelection,
        Action::Hollow,
        Action::FillInterior,
        Action::Resize([1, 1, 1]),
        Action::Crop,
        Action::ToObject,
        Action::ToggleWorld,
        Action::Duplicate { linked: false },
        Action::PickNode(1),
        Action::SetHidden(1, true),
        Action::ToggleHidden,
        Action::Group,
        Action::Ungroup,
        Action::SetLayer(0),
        Action::SetLayerHidden(0, true),
        Action::ToggleLayer,
        Action::AddLayer,
        Action::Copy,
        Action::Cut,
        Action::Paste,
    ]
}

#[test]
fn every_action_that_names_a_tool_names_one_agents_are_offered() {
    // Every name an agent can call: the files and live tools, each with the ops tools.
    let offered: Vec<String> = vxr_mcp::files::tools()
        .into_iter()
        .chain(vxr_mcp::live::tools())
        .filter_map(|t| t["name"].as_str().map(String::from))
        .collect();
    for a in with_tools() {
        let name = tool(&a).expect("listed with a tool");
        assert!(offered.iter().any(|n| n == name), "{a:?} names {name}, which no agent is offered");
    }
}
