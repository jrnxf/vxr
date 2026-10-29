//! The Hybrid UI (direction D) in egui, moved from the test 3 spike. Every size and colour below comes from the prototype's CSS
//! (`prototype/src/editor.html`); the CSS selector each block copies is named in its comment.

use egui::emath::TSTransform;
use egui::text::{LayoutJob, TextFormat};
use egui::{
    Align, Align2, Area, Color32, Context, CornerRadius, FontData, FontDefinitions, FontFamily, FontId, Id, Key,
    Modifiers, Order, Pos2, Rect, Sense, Shadow, Stroke, StrokeKind, Ui, pos2, vec2,
};
use std::collections::HashMap;
use std::sync::Arc;

use vxr_core::material::{Kind, Material};

use crate::color::{Hsv, hex, parse_hex};
use crate::render::{EXPORT_SIZES, RenderSettings, SPP_CHOICES};
use crate::theme::{self, Theme, ThemeChoice, th};

mod agents;
mod assistant;
pub use agents::{AgentsView, EditRow, FILES_SETUP, LIVE_SETUP, SETUP};
pub use assistant::{AiAction, AskBar, ChatView, PANEL_W};

pub(crate) const R: u8 = 12;
/// The top of the panels under the top bar's two 40 px islands.
pub(crate) const TOP: f32 = 60.0;
/// The tool rail's width: 34 px buttons with 6 px padding.
const RAIL_W: f32 = 46.0;
/// A rail button's size, and its step down the rail.
const TOOL: f32 = 34.0;
const TOOL_STEP: f32 = 36.0;
/// The dock's bars: 44-point buttons with 5 points of padding, 8 apart.
const DOCK_H: f32 = 46.0;
const DOCK_BTN: f32 = 36.0;
/// A touch screen whose short side is under this many points is a phone; one narrower than
/// `NARROW_W` is held upright.
const PHONE_MIN: f32 = 400.0;
const NARROW_W: f32 = 460.0;

/// Font bytes, loaded by the caller (see `crate::fonts`).
pub struct FontBytes {
    pub sans_regular: Vec<u8>,
    pub sans_medium: Vec<u8>,
    pub sans_semibold: Vec<u8>,
    pub mono_regular: Vec<u8>,
    pub mono_medium: Vec<u8>,
    /// Geist Mono UltraBlack, for the "vxr" wordmark.
    pub mono_ultrablack: Vec<u8>,
    /// Inter Regular, for the symbols Geist lacks (⌘ and ⌫).
    pub symbols: Vec<u8>,
    pub icons: Vec<u8>,
}

fn family(name: &str) -> FontFamily {
    FontFamily::Name(name.into())
}
fn sans(size: f32) -> FontId {
    FontId::new(size, FontFamily::Proportional)
}
fn sans_medium(size: f32) -> FontId {
    FontId::new(size, family("medium"))
}
fn sans_semibold(size: f32) -> FontId {
    FontId::new(size, family("semibold"))
}
fn mono(size: f32) -> FontId {
    FontId::new(size, FontFamily::Monospace)
}
fn mono_medium(size: f32) -> FontId {
    FontId::new(size, family("mono-medium"))
}
/// The wordmark's face: Geist Mono UltraBlack.
fn brand_font(size: f32) -> FontId {
    FontId::new(size, family("brand"))
}
fn icon_font(size: f32) -> FontId {
    FontId::new(size, family("icons"))
}

/// Install Geist, Geist Mono and the Lucide icon font, and the dark style. Without the
/// fonts, egui's built-in ones stand in (and the icons show as boxes).
pub fn install(ctx: &Context, fonts: Option<FontBytes>) {
    // The touch layout sets the zoom (`Editor::set_touch`); Cmd-= and Cmd-- zoom the view.
    ctx.options_mut(|o| o.zoom_with_keyboard = false);
    let defs = match fonts {
        Some(f) => font_definitions(f),
        None => {
            let mut defs = FontDefinitions::default();
            let prop = defs.families[&FontFamily::Proportional].clone();
            let mono = defs.families[&FontFamily::Monospace].clone();
            for (name, list) in
                [("medium", &prop), ("semibold", &prop), ("icons", &prop), ("mono-medium", &mono), ("brand", &mono)]
            {
                defs.families.insert(family(name), list.clone());
            }
            defs
        }
    };
    ctx.set_fonts(defs);
    style(ctx);
}

fn font_definitions(fonts: FontBytes) -> FontDefinitions {
    let mut defs = FontDefinitions::empty();
    let mut add = |name: &str, bytes: Vec<u8>, families: &[FontFamily]| {
        defs.font_data.insert(name.into(), Arc::new(FontData::from_owned(bytes)));
        for f in families {
            defs.families.entry(f.clone()).or_default().push(name.into());
        }
    };
    add("geist", fonts.sans_regular, &[FontFamily::Proportional]);
    add("geist-medium", fonts.sans_medium, &[family("medium")]);
    add("geist-semibold", fonts.sans_semibold, &[family("semibold")]);
    add("geist-mono", fonts.mono_regular, &[FontFamily::Monospace]);
    add("geist-mono-medium", fonts.mono_medium, &[family("mono-medium")]);
    add("geist-mono-ultrablack", fonts.mono_ultrablack, &[family("brand")]);
    add("lucide", fonts.icons, &[family("icons")]);
    // Inter covers the symbols Geist lacks (the ⌘ and ⌫ of the shortcut keys); keep it as every
    // family's fallback.
    add(
        "inter",
        fonts.symbols,
        &[FontFamily::Proportional, family("medium"), family("semibold"), FontFamily::Monospace, family("mono-medium")],
    );
    defs
}

/// Style egui for the current theme. egui's own light or dark pick is pinned to it, so the
/// system's setting reaches the UI only through [`ThemeChoice::System`].
fn style(ctx: &Context) {
    let (theme, mut visuals) = if th().dark {
        (egui::Theme::Dark, egui::Visuals::dark())
    } else {
        (egui::Theme::Light, egui::Visuals::light())
    };
    visuals.selection.bg_fill = th().tint(40);
    visuals.text_cursor.stroke = Stroke::new(1.5, th().accent);
    visuals.extreme_bg_color = Color32::TRANSPARENT;
    visuals.override_text_color = Some(th().text);
    ctx.set_theme(theme);
    ctx.set_visuals_of(theme, visuals);
}

// Lucide 1.61.0 codepoints.
mod icon {
    pub const BOX: char = '\u{e061}';
    pub const ERASER: char = '\u{e28f}';
    pub const PAINTBRUSH: char = '\u{e2e7}';
    pub const PIPETTE: char = '\u{e13b}';
    pub const POINTER: char = '\u{e1c3}';
    pub const SQUARE_DASHED: char = '\u{e1cb}';
    pub const LAYERS: char = '\u{e529}';
    pub const PLUS: char = '\u{e13d}';
    pub const MINUS: char = '\u{e11c}';
    /// The mockup's mirror glyph (Lucide 0.460 `flip-horizontal-2`, renamed `flip-vertical-2` in 1.x).
    pub const FLIP: char = '\u{e360}';
    pub const SCAN: char = '\u{e257}';
    pub const FOLDER_OPEN: char = '\u{e247}';
    pub const DOWNLOAD: char = '\u{e0b2}';
    pub const KEYBOARD: char = '\u{e284}';
    pub const CODE: char = '\u{e206}';
    pub const UNDO: char = '\u{e2a1}';
    pub const REDO: char = '\u{e2a0}';
    pub const SEARCH: char = '\u{e151}';
    pub const CHEVRON_DOWN: char = '\u{e06d}';
    pub const ARROW_LEFT_RIGHT: char = '\u{e24a}';
    pub const COPY: char = '\u{e09e}';
    pub const APERTURE: char = '\u{e040}';
    pub const IMAGE_DOWN: char = '\u{e53c}';
    pub const SUN: char = '\u{e178}';
    pub const SUNSET: char = '\u{e17a}';
    pub const SLASH: char = '\u{e51d}';
    pub const CIRCLE_DOT: char = '\u{e345}';
    pub const GLOBE: char = '\u{e0e8}';
    pub const LOCK: char = '\u{e10b}';
    pub const SELECT: char = '\u{e509}';
    pub const PAINT_BUCKET: char = '\u{e2e6}';
    pub const SCISSORS: char = '\u{e14e}';
    pub const CLIPBOARD_PASTE: char = '\u{e3e8}';
    pub const TRASH: char = '\u{e18e}';
    pub const X: char = '\u{e1b2}';
    pub const AXIS_3D: char = '\u{e2fe}';
    pub const EYE: char = '\u{e0ba}';
    pub const EYE_OFF: char = '\u{e0bb}';
    pub const FOLDER: char = '\u{e0d7}';
    pub const LINK: char = '\u{e102}';
    pub const FILE_OUTPUT: char = '\u{e0c8}';
    pub const FILE_PLUS: char = '\u{e0c9}';
    pub const MAXIMIZE_2: char = '\u{e113}';
    pub const MINIMIZE_2: char = '\u{e11b}';
    pub const SCAN_SEARCH: char = '\u{e537}';
    pub const ROTATE_CCW: char = '\u{e148}';
    pub const PENCIL: char = '\u{e1f9}';
    pub const SPARKLES: char = '\u{e412}';
    pub const CHECK: char = '\u{e06c}';
    pub const TRIANGLE_ALERT: char = '\u{e193}';
    pub const CORNER_DOWN_LEFT: char = '\u{e0a1}';
    pub const KEY_ROUND: char = '\u{e4a3}';
    pub const SQUARE: char = '\u{e167}';
    pub const PALETTE: char = '\u{e1dd}';
    pub const ARROW_UP: char = '\u{e04a}';
    pub const MESSAGE_SQUARE_PLUS: char = '\u{e40c}';
    pub const PANEL_RIGHT: char = '\u{e431}';
    pub const CHEVRON_RIGHT: char = '\u{e06f}';
    pub const SHIELD_CHECK: char = '\u{e1ff}';
    pub const TERMINAL: char = '\u{e181}';
    pub const SQUARE_TERMINAL: char = '\u{e20a}';
    pub const REFRESH_CW: char = '\u{e145}';
}

pub use vxr_core::select::{Region, Transform};
pub use vxr_core::tools::{MAX_SIZE, Shape, Tool};
pub use vxr_render::View;

/// What the UI asks the app to do this frame.
#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Action {
    /// Start a blank scene (the UI has already asked about unsaved edits).
    New,
    Open,
    Save,
    Undo,
    Redo,
    Frame,
    /// Frame the selected voxels, keeping the angle.
    FrameSelection,
    /// Back to the starting angle in perspective, framing the scene.
    ResetCamera,
    /// Zoom in (true) or out by a step, about the middle of the view.
    Zoom(bool),
    /// Look from one side, or isometric (which turns orthographic on).
    Snap(View),
    /// Orthographic (true) or perspective projection.
    Ortho(bool),
    Embed,
    /// Export what the view shows as OBJ with its palette texture (the host writes the files).
    ExportObj,
    /// Export the model's layers side by side as one PNG (the host writes the file).
    ExportSlices,
    /// Show `rgba` in palette slot `slot` now, inside the open colour edit.
    Recolor {
        slot: u8,
        rgba: [u8; 4],
    },
    /// Close the colour or material edit in progress as one undo step.
    CommitColor,
    /// Give palette slot `slot` this material now, inside the open edit.
    SetMaterial {
        slot: u8,
        material: Material,
    },
    /// Swap two slots, keeping the scene's look.
    SwapColors(u8, u8),
    /// Give slot `to` the colour of slot `from`.
    CopyColor {
        from: u8,
        to: u8,
    },
    /// Start a PNG render export with the Render panel's settings, or cancel the one running.
    ExportPng,
    CancelExport,
    /// For the host: an export finished; take its PNG from `Editor::exported`.
    Exported,
    /// Move, rotate, flip or loop the selection, or the whole model when nothing is selected.
    Transform(Transform),
    SelectAll,
    InvertSelection,
    /// Select every voxel of the current colour.
    SelectColor,
    Deselect,
    DeleteSelection,
    /// Paint the selection with the current colour.
    FillSelection,
    /// Keep only the voxels seen from outside the selection, or the whole model.
    Hollow,
    /// Fill the space the selection, or the whole model, encloses with the current colour.
    FillInterior,
    /// Resize the model bounds to this size, keeping the bottom and the centre.
    Resize([u32; 3]),
    /// Fit the model bounds to the voxels.
    Crop,
    /// Switch between editing a model and the world of every object (Tab).
    ToggleWorld,
    /// Copy the picked object beside itself in world mode: its own model, or a linked copy
    /// sharing it (an instance).
    Duplicate {
        linked: bool,
    },
    /// A scene tree row was clicked: pick that object in world mode, or edit its model.
    PickNode(usize),
    /// Hide or show a tree node in world mode (the row's eye).
    SetHidden(usize, bool),
    /// Hide or show the picked object (H).
    ToggleHidden,
    /// Put the picked object or group into a new group of its own (Cmd-G).
    Group,
    /// Put the picked object or group on this layer (a layer row click in world mode).
    SetLayer(i32),
    /// Hide or show a layer (its row's eye).
    SetLayerHidden(i32, bool),
    /// Hide or show the picked object's layer (Cmd-K).
    ToggleLayer,
    /// Add a layer (the + beside LAYERS).
    AddLayer,
    /// Move the picked group's children up into its parent (Cmd-Shift-G).
    Ungroup,
    Copy,
    Cut,
    Paste,
    /// Place the box whose height is being set (a click, Enter).
    PlaceBox,
    /// Drop the box whose height is being set (Escape).
    CancelBox,
    /// Apply the AI change under review as one undo step (the card's Apply, Enter).
    ApplyReview,
    /// Drop the AI change under review (the card's Discard, Escape).
    DiscardReview,
    /// Drop a step of the change, or take it back (its checkbox).
    DropStep(usize, bool),
    /// Show the scene as it was before the change (true) or with it (false).
    ReviewBefore(bool),
    /// Let the change grow past 10,000 cells, and run the calls that were waiting.
    AllowMore,
    /// Drop the calls waiting to grow the change; what is staged stays.
    StopHeld,
    /// Let terminal agents edit this window (true), or stop listening (for the host).
    AllowAgents(bool),
    /// Review every agent change (true): agents' edits wait for Apply or Discard.
    ReviewAgents(bool),
    /// In the browser: pair this tab with `vxr bridge` with the code typed into the panel
    /// (`Hybrid::agents_code`), for the host.
    PairBridge,
    /// Make a new pairing token, disconnecting every agent (for the host).
    RegenerateToken,
    /// Disconnect one agent, or every agent (for the host).
    StopAgent(u64),
    StopAgents,
    /// Take back one agent edit (the Agent edits card's Undo).
    UndoAgentEdit(usize),
    /// The theme was picked (for the host to keep: `Hybrid::theme`).
    Theme,
}

/// The review card of an AI change (set by the editor while one is under review).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ReviewCard {
    /// The request, and the model's one-line summary once it called `done`. For terminal
    /// agents' edits, the agents.
    pub prompt: String,
    /// Terminal agents' edits ("Review every agent change"), not the assistant's: no Ask for
    /// changes, since the agents take requests in the terminal.
    pub agents: bool,
    pub summary: Option<String>,
    /// Cells added, removed and repainted.
    pub counts: [usize; 3],
    /// Palette slots whose colour or material the change sets.
    pub slots: Vec<SlotChange>,
    pub steps: Vec<StepRow>,
    /// Calls wait for the user: the cells staged without asking, and the most allowed.
    pub held: Option<(usize, usize)>,
    /// The scene shows Before.
    pub before: bool,
    /// The assistant is still working on it: what it is doing now.
    pub working: Option<String>,
}

/// A palette slot the change sets: its colour before and after, and its material change.
#[derive(Clone, Debug, PartialEq)]
pub struct SlotChange {
    pub slot: u8,
    pub old: [u8; 4],
    pub new: [u8; 4],
    pub material: Option<String>,
}

/// One step of the change on the card.
#[derive(Clone, Debug, PartialEq)]
pub struct StepRow {
    /// The step's place in the request (what `Action::DropStep` names).
    pub index: usize,
    pub tool: &'static str,
    pub detail: String,
    /// The call worked; failed calls are listed but change nothing.
    pub ok: bool,
    pub dropped: bool,
    /// Its checkbox can drop it.
    pub droppable: bool,
}

/// Steps the review card shows before its list scrolls.
const REVIEW_ROWS: usize = 6;

/// One row of the scene tree: an object or a group, in tree order (set by the editor).
#[derive(Clone, Debug, PartialEq)]
pub struct TreeRow {
    pub node: usize,
    /// Groups above it, the root not counted.
    pub depth: usize,
    pub name: String,
    pub group: bool,
    /// Objects showing this object's model, itself included: more than 1 is a linked copy.
    pub linked: usize,
    /// The model's size, for objects.
    pub size: [u32; 3],
    pub hidden: bool,
    /// Neither it nor a group above it is hidden or on a hidden layer.
    pub shown: bool,
    pub layer: i32,
}

/// One row of the scene tree's LAYERS list, in world mode (set by the editor).
#[derive(Clone, Debug, PartialEq)]
pub struct LayerRow {
    pub id: i32,
    pub name: String,
    /// Objects on the layer.
    pub objects: usize,
    pub hidden: bool,
}

/// What an in-place rename field names: a tree node or a layer.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Named {
    Node(usize),
    Layer(i32),
}

/// The scene tree's width, and how many rows it shows before scrolling.
const TREE_W: f32 = 212.0;
const TREE_ROWS: usize = 14;

/// What the next swatch click does, armed from the colour editor or Cmd-K.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SlotOp {
    Swap,
    Copy,
}

/// The colour editor's two tabs.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum EditTab {
    #[default]
    Color,
    Material,
}

/// The colour editor's state. It edits the current slot.
pub struct ColorEdit {
    pub tab: EditTab,
    /// Kept between frames so the hue survives greys and black.
    hsv: Hsv,
    /// Text of the hex, R, G, B, H, S and V fields.
    fields: [String; 7],
}

const FIELDS: [&str; 7] = ["HEX", "R", "G", "B", "H", "S", "V"];

/// The palette card's width, and its swatch grid: 8 columns of 19 px with 3 px gaps.
pub(crate) const PALETTE_W: f32 = 200.0;
const SWATCH: f32 = 19.0;
const SWATCH_STEP: f32 = 22.0;
/// The most swatch rows the palette shows before it scrolls.
const SWATCH_ROWS: f32 = 8.0;

/// Readouts for the stats corner.
#[derive(Clone, Debug, Default)]
pub struct Stats {
    pub voxels: u64,
    pub fps: f32,
    pub frame_ms: f32,
    pub cursor: Option<[i32; 3]>,
    /// Size of the box being dragged, shown in place of the cursor.
    /// What a shape drag in progress spans: ("BOX", "4×1×3"), ("LINE", "12"), ("RADIUS", "5").
    pub span: Option<(&'static str, String)>,
    /// How many voxels are selected.
    pub selected: usize,
    /// The render corner's samples per pixel and whether it is still sampling, if shown.
    pub render: Option<(f32, bool)>,
    /// A render export in progress: its size, samples per pixel so far and the target.
    pub export: Option<((u32, u32), f32, u32)>,
    /// Export sizes this GPU can render (by `EXPORT_SIZES` index).
    pub export_fits: [bool; 4],
}

pub struct Hybrid {
    pub tool: Tool,
    pub shape: Shape,
    /// Mirror on (M), and on which axes when it is.
    pub mirror: bool,
    pub mirror_on: [bool; 3],
    /// Axis lock (K cycles X, Y, Z, off).
    pub lock: Option<usize>,
    /// The voxel brush's size, 1 to [`MAX_SIZE`] (- and = step it).
    pub size: u8,
    /// What a select click on a voxel takes: its colour, its face or its volume.
    pub region: Region,
    /// The size flyout beside the voxel brush button is open (V again, or a click on the
    /// button while the voxel brush is on). It also shows while the size is above 1.
    pub size_panel: bool,
    pub slot: u8,
    /// RGBA by voxel index, as the scene stores it.
    pub palette: [[u8; 4]; 256],
    /// Material by voxel index.
    pub materials: [Material; 256],
    /// Palette indices with voxels in the scene.
    pub used: [bool; 256],
    pub scene_name: String,
    pub coach: bool,
    /// The Cmd-K query while the palette is open.
    pub cmdk: Option<String>,
    pub cmdk_sel: usize,
    pub can_undo: bool,
    /// The scene differs from what was last opened or saved.
    pub unsaved: bool,
    pub can_redo: bool,
    pub color_edit: Option<ColorEdit>,
    pub slot_op: Option<SlotOp>,
    /// Show the live path-traced render corner.
    pub show_render: bool,
    /// Where the render corner's image goes this frame, in points: the corner, or the whole
    /// view in the full render view.
    pub render_rect: Option<Rect>,
    /// The path-traced image fills the view instead of the corner.
    pub full_render: bool,
    /// The corner's place, which the Render panel lines up with in both views.
    render_dock: Option<Rect>,
    /// The Render panel is open, and what it edits.
    pub render_panel: bool,
    pub render: RenderSettings,
    /// Text of the Render panel's sun colour field.
    sun_hex: String,
    /// The model bounds' size, and the Bounds panel's X, Y and Z fields while it is open.
    pub bounds: [u32; 3],
    pub bounds_panel: Option<[String; 3]>,
    /// The camera's projection and the snap view it is at, if any (set by the editor).
    pub ortho: bool,
    pub view: Option<View>,
    /// The View flyout beside the camera bar is open.
    pub view_panel: bool,
    /// Show the camera moves in the hint bar: Space is held or the camera is being dragged
    /// (set by the editor).
    pub camera_hints: bool,
    /// A Cmd-K camera gesture was chosen: toast how the camera moves.
    camera_tip: bool,
    /// The Export menu under the top bar's Export button is open.
    pub export_menu: bool,
    /// The picked theme (System follows the system's light or dark setting), and the Theme
    /// menu under the top bar's theme button.
    pub theme: ThemeChoice,
    pub theme_menu: bool,
    /// The theme the UI was last styled for.
    styled: Option<&'static str>,
    /// The New scene confirm is open: the scene has edits that starting a new one would drop.
    pub confirm_new: bool,
    /// Left edge of the top bar's Export button, where the Export menu lines up.
    export_x: f32,
    /// Guides drawn in the view: the ground grid, the ground plane and the bounds frame.
    pub grid: bool,
    pub ground: bool,
    pub frame: bool,
    /// Where each clickable part of the UI was last drawn, in points, by name, for the
    /// headless checks ("edit-color", "sv", "hue", "swap", "copy", "sw<slot>", ...).
    pub hits: HashMap<String, Rect>,
    /// The touch layout: the bottom dock in place of the rail, larger targets and gestures
    /// (set by the editor, see `Editor::set_touch`).
    pub touch: bool,
    /// The palette shows above the dock (its colour button toggles it).
    pub palette_open: bool,
    /// The top of the dock's upper bar, where flyouts open above, and the view it is centred on.
    dock_top: f32,
    dock_view: Rect,
    /// Everything the dock's bars cover.
    dock_area: Rect,
    /// A phone-sized touch screen (its short side under `PHONE_MIN` points): panels fit the
    /// room the dock leaves, and the top bar and dock shrink to the width.
    phone: bool,
    /// A phone held upright (narrower than `NARROW_W`): the assistant is a sheet above the dock.
    narrow: bool,
    /// A phone held sideways: the dock stands at the left and flyouts open to its right.
    short: bool,
    /// Top of the mirror button in the rail, where the axis flyout lines up.
    axes_y: f32,
    voxel_y: f32,
    /// Top of the select button in the rail, where the selection flyout lines up.
    select_y: f32,
    /// How many voxels are selected, and whether Copy or Cut has filled the clipboard.
    pub selected: usize,
    pub has_clip: bool,
    /// World mode is showing (set by the editor), and the name of the object picked there.
    pub world: bool,
    pub picked: Option<String>,
    /// The scene tree's rows, empty (no tree shown) unless there are two objects or more,
    /// the row shown on (the picked object, or the object being edited) and the picked node.
    pub tree: Vec<TreeRow>,
    pub tree_on: Option<usize>,
    pub picked_node: Option<usize>,
    /// The layers listed under the tree in world mode, and the picked node's layer.
    pub layers: Vec<LayerRow>,
    pub layer_on: Option<i32>,
    /// The first tree row shown, when there are more than fit.
    tree_scroll: usize,
    /// The node or layer being renamed and its field's text, and whether the field still
    /// needs focus.
    renaming: Option<(Named, String)>,
    rename_focus: bool,
    /// A finished rename for the editor to apply: node or layer and new name.
    pub renamed: Option<(Named, String)>,
    /// The box being dragged with the select tool, in points.
    pub marquee: Option<Rect>,
    /// A box brush's base is drawn and its height follows the pointer (set by the editor).
    pub lifting: bool,
    /// Swatch being dragged with Cmd (swap) or Cmd+Shift (copy).
    drag_from: Option<u8>,
    /// The slot the swatch grid last scrolled to.
    shown_slot: u8,
    /// The palette shows all 255 slots, not only the colours in the scene.
    pub palette_all: bool,
    shift: bool,
    alt: bool,
    actions: Vec<Action>,
    /// Arrow keys pressed this frame with a selection, for the editor to move it.
    pub arrows: Vec<Key>,
    toast: Option<(String, f64)>,
    /// The AI change under review, if any (set by the editor).
    pub review: Option<ReviewCard>,
    /// The review card's "Ask for changes" text, whether it had focus last frame, and the
    /// first step shown when the list scrolls.
    review_ask: String,
    ask_focus: bool,
    review_scroll: usize,
    /// An "Ask for changes" the user sent, for the assistant to take.
    pub asked: Option<String>,
    /// The assistant panel (Cmd-J) is open, and its key settings show in it.
    pub ai_open: bool,
    pub ai_setup: bool,
    /// The assistant as the panel shows it (set by the editor when it changes).
    pub chat: ChatView,
    /// What the panel and the prompt bar asked the editor to do this frame.
    pub ai_out: Vec<AiAction>,
    /// Cmd-K "Ask AI"'s prompt bar.
    pub ask_bar: Option<AskBar>,
    ai: assistant::AiState,
    /// Right edge of the stats corner, which the hint bar keeps clear of.
    stats_right: f32,
    /// Where the camera bar was drawn, which the review card keeps clear of.
    nav_rect: Option<Rect>,
    /// Terminal agents (set by the editor), the Terminal agents panel, and how many edits
    /// there were when the Agent edits card was closed (it shows again on the next).
    pub agents: AgentsView,
    pub agents_panel: bool,
    pub agents_card_closed: usize,
    /// The pairing code typed into the panel, in the browser (kept for this tab only).
    pub agents_code: String,
}

struct Command {
    label: &'static str,
    keys: &'static [&'static str],
    run: fn(&mut Hybrid),
}

const COMMANDS: &[Command] = &[
    Command { label: "Add", keys: &["A"], run: |h| h.tool = Tool::Add },
    Command { label: "Erase", keys: &["E"], run: |h| h.tool = Tool::Erase },
    Command { label: "Paint", keys: &["P"], run: |h| h.tool = Tool::Paint },
    Command { label: "Pick color", keys: &["I"], run: |h| h.tool = Tool::Pick },
    Command { label: "Select", keys: &["S"], run: |h| h.tool = Tool::Select },
    Command { label: "Select all", keys: &["⌘", "A"], run: |h| h.actions.push(Action::SelectAll) },
    Command { label: "Invert selection", keys: &["⌘", "⇧", "I"], run: |h| h.actions.push(Action::InvertSelection) },
    Command { label: "Select color", keys: &[], run: |h| h.actions.push(Action::SelectColor) },
    Command { label: "Click selects color", keys: &[], run: |h| h.set_region(Region::Color) },
    Command { label: "Click selects face", keys: &[], run: |h| h.set_region(Region::Face) },
    Command { label: "Click selects volume", keys: &[], run: |h| h.set_region(Region::Volume) },
    Command { label: "Deselect", keys: &["Esc"], run: |h| h.actions.push(Action::Deselect) },
    Command { label: "Delete selection", keys: &["⌫"], run: |h| h.actions.push(Action::DeleteSelection) },
    Command { label: "Fill selection", keys: &[], run: |h| h.actions.push(Action::FillSelection) },
    Command { label: "Hollow", keys: &[], run: |h| h.actions.push(Action::Hollow) },
    Command { label: "Fill inside", keys: &[], run: |h| h.actions.push(Action::FillInterior) },
    Command { label: "Resize bounds…", keys: &[], run: |h| h.toggle_bounds_panel() },
    Command { label: "Crop to content", keys: &[], run: |h| h.actions.push(Action::Crop) },
    Command { label: "Copy", keys: &["⌘", "C"], run: |h| h.actions.push(Action::Copy) },
    Command { label: "Cut", keys: &["⌘", "X"], run: |h| h.actions.push(Action::Cut) },
    Command { label: "Paste", keys: &["⌘", "V"], run: |h| h.actions.push(Action::Paste) },
    Command { label: "Rotate X", keys: &[], run: |h| h.actions.push(Action::Transform(Transform::Rotate(0))) },
    Command { label: "Rotate Y", keys: &[], run: |h| h.actions.push(Action::Transform(Transform::Rotate(1))) },
    Command { label: "Rotate Z", keys: &[], run: |h| h.actions.push(Action::Transform(Transform::Rotate(2))) },
    Command { label: "Flip X", keys: &[], run: |h| h.actions.push(Action::Transform(Transform::Flip(0))) },
    Command { label: "Flip Y", keys: &[], run: |h| h.actions.push(Action::Transform(Transform::Flip(1))) },
    Command { label: "Flip Z", keys: &[], run: |h| h.actions.push(Action::Transform(Transform::Flip(2))) },
    Command { label: "Loop X", keys: &[], run: |h| h.actions.push(Action::Transform(Transform::Loop(0, 1))) },
    Command { label: "Loop Y", keys: &[], run: |h| h.actions.push(Action::Transform(Transform::Loop(1, 1))) },
    Command { label: "Loop Z", keys: &[], run: |h| h.actions.push(Action::Transform(Transform::Loop(2, 1))) },
    Command {
        label: "Move up",
        keys: &["⇧", "↑"],
        run: |h| h.actions.push(Action::Transform(Transform::Move([0, 1, 0]))),
    },
    Command {
        label: "Move down",
        keys: &["⇧", "↓"],
        run: |h| h.actions.push(Action::Transform(Transform::Move([0, -1, 0]))),
    },
    Command { label: "Voxel brush", keys: &["V"], run: |h| h.shape = Shape::Voxel },
    Command {
        label: "Voxel brush size",
        keys: &["V", "V"],
        run: |h| {
            h.shape = Shape::Voxel;
            h.size_panel = true;
        },
    },
    Command { label: "Bigger brush", keys: &["="], run: |h| h.step_size(1) },
    Command { label: "Smaller brush", keys: &["-"], run: |h| h.step_size(-1) },
    Command { label: "Box brush", keys: &["B"], run: |h| h.shape = Shape::Box },
    Command { label: "Place box", keys: &["↵"], run: |h| h.actions.push(Action::PlaceBox) },
    Command { label: "Cancel box", keys: &["Esc"], run: |h| h.actions.push(Action::CancelBox) },
    Command { label: "Face brush", keys: &["F"], run: |h| h.shape = Shape::Face },
    Command { label: "Line brush", keys: &["L"], run: |h| h.shape = Shape::Line },
    Command { label: "Center brush: circle", keys: &["C"], run: |h| h.shape = Shape::Circle },
    Command { label: "Center brush: sphere", keys: &["C", "C"], run: |h| h.shape = Shape::Sphere },
    Command { label: "Mirror", keys: &["M"], run: |h| h.mirror = !h.mirror },
    Command { label: "Mirror X", keys: &[], run: |h| h.toggle_mirror_axis(0) },
    Command { label: "Mirror Y", keys: &[], run: |h| h.toggle_mirror_axis(1) },
    Command { label: "Mirror Z", keys: &[], run: |h| h.toggle_mirror_axis(2) },
    Command { label: "Lock axis X", keys: &[], run: |h| h.toggle_lock(0) },
    Command { label: "Lock axis Y", keys: &[], run: |h| h.toggle_lock(1) },
    Command { label: "Lock axis Z", keys: &[], run: |h| h.toggle_lock(2) },
    Command { label: "Cycle axis lock", keys: &["K"], run: |h| h.cycle_lock() },
    Command { label: "World mode / edit model", keys: &["Tab"], run: |h| h.actions.push(Action::ToggleWorld) },
    Command {
        label: "Duplicate object",
        keys: &["⌘", "D"],
        run: |h| h.actions.push(Action::Duplicate { linked: false }),
    },
    Command {
        label: "Linked copy (instance)",
        keys: &["⌘", "⇧", "D"],
        run: |h| h.actions.push(Action::Duplicate { linked: true }),
    },
    Command { label: "Delete object", keys: &["⌫"], run: |h| h.actions.push(Action::DeleteSelection) },
    Command { label: "Hide or show object", keys: &["H"], run: |h| h.actions.push(Action::ToggleHidden) },
    Command { label: "Rename object", keys: &[], run: |h| h.start_rename(h.picked_node) },
    Command { label: "Group object", keys: &["⌘", "G"], run: |h| h.actions.push(Action::Group) },
    Command { label: "Ungroup", keys: &["⌘", "⇧", "G"], run: |h| h.actions.push(Action::Ungroup) },
    Command { label: "New layer", keys: &[], run: |h| h.actions.push(Action::AddLayer) },
    Command { label: "Hide or show layer", keys: &[], run: |h| h.actions.push(Action::ToggleLayer) },
    Command { label: "Rename layer", keys: &[], run: |h| h.start_layer_rename(h.layer_on) },
    Command { label: "Frame scene", keys: &["."], run: |h| h.actions.push(Action::Frame) },
    Command { label: "Frame selection", keys: &[], run: |h| h.actions.push(Action::FrameSelection) },
    Command { label: "Reset camera", keys: &["Home"], run: |h| h.actions.push(Action::ResetCamera) },
    Command { label: "Zoom in", keys: &["⌘", "="], run: |h| h.actions.push(Action::Zoom(true)) },
    Command { label: "Zoom out", keys: &["⌘", "-"], run: |h| h.actions.push(Action::Zoom(false)) },
    Command { label: "Orbit the camera", keys: &["Right-drag"], run: |h| h.camera_tip = true },
    Command { label: "Pan the camera", keys: &["Space-drag"], run: |h| h.camera_tip = true },
    Command { label: "Orbit around a point", keys: &["X", "Click"], run: |h| h.camera_tip = true },
    Command { label: "View options", keys: &[], run: |h| h.view_panel = !h.view_panel },
    Command { label: "Perspective", keys: &["O"], run: |h| h.actions.push(Action::Ortho(false)) },
    Command { label: "Orthographic", keys: &["O"], run: |h| h.actions.push(Action::Ortho(true)) },
    Command { label: "Isometric view", keys: &["0"], run: |h| h.actions.push(Action::Snap(View::Iso)) },
    Command { label: "Front view", keys: &["1"], run: |h| h.actions.push(Action::Snap(View::Front)) },
    Command { label: "Back view", keys: &["2"], run: |h| h.actions.push(Action::Snap(View::Back)) },
    Command { label: "Left view", keys: &["3"], run: |h| h.actions.push(Action::Snap(View::Left)) },
    Command { label: "Right view", keys: &["4"], run: |h| h.actions.push(Action::Snap(View::Right)) },
    Command { label: "Top view", keys: &["5"], run: |h| h.actions.push(Action::Snap(View::Top)) },
    Command { label: "Bottom view", keys: &["6"], run: |h| h.actions.push(Action::Snap(View::Bottom)) },
    Command { label: "Show grid", keys: &["G"], run: |h| h.grid = !h.grid },
    Command { label: "Show ground", keys: &[], run: |h| h.ground = !h.ground },
    Command { label: "Show bounds", keys: &[], run: |h| h.frame = !h.frame },
    Command { label: "Undo", keys: &["⌘", "Z"], run: |h| h.actions.push(Action::Undo) },
    Command { label: "Redo", keys: &["⌘", "⇧", "Z"], run: |h| h.actions.push(Action::Redo) },
    Command { label: "New scene", keys: &["⌘", "N"], run: |h| h.request_new() },
    Command { label: "Open .vox", keys: &["⌘", "O"], run: |h| h.actions.push(Action::Open) },
    Command { label: "Save .vox", keys: &["⌘", "S"], run: |h| h.actions.push(Action::Save) },
    Command { label: "Embed", keys: &[], run: |h| h.actions.push(Action::Embed) },
    Command { label: "Edit color", keys: &[], run: |h| h.open_color_edit() },
    Command { label: "Edit material", keys: &[], run: |h| h.open_material_edit() },
    Command { label: "Swap color with…", keys: &[], run: |h| h.slot_op = Some(SlotOp::Swap) },
    Command { label: "Copy color to…", keys: &[], run: |h| h.slot_op = Some(SlotOp::Copy) },
    Command {
        label: "Toggle render preview",
        keys: &[],
        run: |h| {
            h.show_render = !h.show_render;
            h.full_render = false;
        },
    },
    Command { label: "Render settings", keys: &["R"], run: |h| h.toggle_render_panel() },
    Command { label: "Full render view", keys: &[], run: |h| h.toggle_full_render() },
    Command { label: "Export PNG", keys: &[], run: |h| h.actions.push(Action::ExportPng) },
    Command { label: "Export OBJ", keys: &[], run: |h| h.actions.push(Action::ExportObj) },
    Command { label: "Export PNG slices", keys: &[], run: |h| h.actions.push(Action::ExportSlices) },
    Command { label: "Ask AI…", keys: &["Tab"], run: |h| h.open_ask_bar("") },
    Command { label: "Assistant panel", keys: &["⌘", "J"], run: |h| h.toggle_ai() },
    Command { label: "Assistant keys and model", keys: &[], run: |h| h.open_ai_setup(None) },
    Command { label: "Terminal agents (MCP)…", keys: &[], run: |h| h.open_agents() },
    Command { label: "Agent edits", keys: &[], run: |h| h.show_agent_edits() },
    Command { label: "Theme: System", keys: &[], run: |h| h.set_theme(ThemeChoice::System) },
    Command { label: "Theme: Neutral", keys: &[], run: |h| h.set_theme(ThemeChoice::Neutral) },
    Command { label: "Theme: Paper", keys: &[], run: |h| h.set_theme(ThemeChoice::Paper) },
    Command { label: "Theme: Gruvbox", keys: &[], run: |h| h.set_theme(ThemeChoice::Gruvbox) },
    Command { label: "Theme: Tokyo Night", keys: &[], run: |h| h.set_theme(ThemeChoice::TokyoNight) },
    Command { label: "Theme: Kanagawa", keys: &[], run: |h| h.set_theme(ThemeChoice::Kanagawa) },
    Command { label: "Theme: Nord", keys: &[], run: |h| h.set_theme(ThemeChoice::Nord) },
];

impl Hybrid {
    pub fn new(palette: [[u8; 4]; 256], used: [bool; 256], scene_name: &str) -> Self {
        Self {
            tool: Tool::Add,
            shape: Shape::Voxel,
            mirror: false,
            mirror_on: [true, false, false],
            lock: None,
            size: 1,
            region: Region::Color,
            size_panel: false,
            slot: 28,
            palette,
            materials: [Material::default(); 256],
            used,
            scene_name: scene_name.into(),
            coach: true,
            cmdk: None,
            cmdk_sel: 0,
            can_undo: false,
            unsaved: false,
            can_redo: false,
            color_edit: None,
            slot_op: None,
            show_render: true,
            render_rect: None,
            full_render: false,
            render_dock: None,
            render_panel: false,
            render: RenderSettings::default(),
            sun_hex: String::new(),
            bounds: [0; 3],
            bounds_panel: None,
            ortho: false,
            view: None,
            view_panel: false,
            camera_hints: false,
            camera_tip: false,
            export_menu: false,
            theme: ThemeChoice::default(),
            theme_menu: false,
            styled: None,
            confirm_new: false,
            export_x: 0.0,
            grid: true,
            ground: true,
            frame: true,
            hits: HashMap::new(),
            touch: false,
            palette_open: false,
            dock_top: 0.0,
            dock_view: Rect::NOTHING,
            dock_area: Rect::NOTHING,
            phone: false,
            narrow: false,
            short: false,
            axes_y: 0.0,
            voxel_y: 0.0,
            select_y: 0.0,
            selected: 0,
            has_clip: false,
            world: false,
            picked: None,
            tree: Vec::new(),
            tree_on: None,
            picked_node: None,
            tree_scroll: 0,
            layers: Vec::new(),
            layer_on: None,
            renaming: None,
            rename_focus: false,
            renamed: None,
            marquee: None,
            lifting: false,
            drag_from: None,
            shown_slot: 28,
            palette_all: false,
            shift: false,
            alt: false,
            actions: Vec::new(),
            arrows: Vec::new(),
            toast: None,
            review: None,
            review_ask: String::new(),
            ask_focus: false,
            review_scroll: 0,
            asked: None,
            ai_open: false,
            ai_setup: false,
            chat: ChatView::new(),
            ai_out: Vec::new(),
            ask_bar: None,
            ai: Default::default(),
            stats_right: 0.0,
            nav_rect: None,
            agents: AgentsView::default(),
            agents_panel: false,
            agents_card_closed: 0,
            agents_code: String::new(),
        }
    }

    /// Show a short message under the top bar for 1.8 s (#toast).
    pub fn toast(&mut self, ctx: &Context, msg: impl Into<String>) {
        self.toast = Some((msg.into(), ctx.input(|i| i.time) + 1.8));
    }

    /// Whether Shift or Alt is held, as last seen by the UI.
    pub fn modifiers(&self) -> (bool, bool) {
        (self.shift, self.alt)
    }

    /// The tool a click uses now: Alt picks, Shift erases (with the select tool they
    /// subtract from and add to the selection instead).
    pub fn effective_tool(&self) -> Tool {
        if self.tool == Tool::Select {
            Tool::Select
        } else if self.alt {
            Tool::Pick
        } else if self.shift && self.tool != Tool::Pick {
            Tool::Erase
        } else {
            self.tool
        }
    }

    /// The axes edits are mirrored on now.
    pub fn mirror_axes(&self) -> [bool; 3] {
        if self.mirror { self.mirror_on } else { [false; 3] }
    }

    /// Turn mirroring on one axis on or off. Mirror turns on with it, and off with the last axis.
    pub fn toggle_mirror_axis(&mut self, a: usize) {
        if !self.mirror {
            self.mirror = true;
            self.mirror_on = [false; 3];
        }
        self.mirror_on[a] = !self.mirror_on[a];
        if self.mirror_on == [false; 3] {
            self.mirror = false;
            self.mirror_on = [a == 0, a == 1, a == 2];
        }
    }

    /// Lock drags across axis `a`, or unlock if it is already locked.
    pub fn toggle_lock(&mut self, a: usize) {
        self.lock = if self.lock == Some(a) { None } else { Some(a) };
    }

    /// K: lock X, then Y, then Z, then off.
    pub fn cycle_lock(&mut self) {
        self.lock = match self.lock {
            None => Some(0),
            Some(a) if a < 2 => Some(a + 1),
            Some(_) => None,
        };
    }

    /// C: the center brush, switching between circle and sphere when it is already on.
    /// V: the voxel brush, or with it already on, open or close its size flyout.
    pub fn voxel_brush(&mut self) {
        if self.shape == Shape::Voxel {
            self.size_panel = !self.size_panel;
        }
        self.shape = Shape::Voxel;
    }

    /// Step the voxel brush's size by `d`, within 1 to [`MAX_SIZE`].
    pub fn step_size(&mut self, d: i32) {
        self.size = (self.size as i32 + d).clamp(1, MAX_SIZE as i32) as u8;
    }

    /// Whether the size flyout shows: the voxel brush with add, erase or paint in model mode,
    /// opened or above size 1, and no selection flyout in its place.
    fn size_shown(&self) -> bool {
        self.shape == Shape::Voxel
            && matches!(self.tool, Tool::Add | Tool::Erase | Tool::Paint)
            && !self.world
            && self.selected == 0
            && (self.size_panel || self.size > 1)
    }

    pub fn center_brush(&mut self) {
        self.shape = if self.shape == Shape::Circle { Shape::Sphere } else { Shape::Circle };
    }

    /// Open the colour editor on the current slot.
    pub fn open_color_edit(&mut self) {
        // The two side panels share the space left of the palette and the render corner.
        self.render_panel = false;
        if self.color_edit.is_none() {
            let c = self.palette[self.slot as usize];
            self.color_edit = Some(ColorEdit {
                tab: EditTab::Color,
                hsv: Hsv::from_rgb([c[0], c[1], c[2]]),
                fields: Default::default(),
            });
        }
    }

    /// Open the colour editor on its Material tab.
    pub fn open_material_edit(&mut self) {
        self.open_color_edit();
        if let Some(ed) = &mut self.color_edit {
            ed.tab = EditTab::Material;
        }
    }

    /// Open or close the Render panel (it shows the render corner too).
    pub fn toggle_render_panel(&mut self) {
        self.render_panel = !self.render_panel;
        if self.render_panel {
            self.color_edit = None;
            self.slot_op = None;
            self.show_render = true;
        }
    }

    /// Swap the path-traced image between the corner and the whole view (it shows the corner).
    pub fn toggle_full_render(&mut self) {
        self.full_render = !self.full_render;
        self.show_render = true;
    }

    /// Open or close the Bounds panel, its fields showing the current size.
    pub fn toggle_bounds_panel(&mut self) {
        self.bounds_panel = match self.bounds_panel {
            Some(_) => None,
            None => Some(self.bounds.map(|v| v.to_string())),
        };
    }

    /// Escape: cancel an armed swap or copy, else close the colour editor, the Bounds panel,
    /// the View flyout or the Render panel, else leave the full render view, else deselect,
    /// else the coach card.
    /// Where flyouts beside the rail start: right of the rail, or of the scene tree when it shows.
    fn flyout_x(&self) -> f32 {
        12.0 + RAIL_W + 8.0 + if self.tree.is_empty() { 0.0 } else { TREE_W + 8.0 }
    }

    /// Where a flyout of `size` opens: beside the rail level with `y` (its button's top), or
    /// over the dock, centred on the button named `anchor`.
    fn flyout_rect(&self, y: f32, anchor: &str, size: egui::Vec2) -> Rect {
        if !self.touch {
            return Rect::from_min_size(pos2(self.flyout_x(), y - 6.0), size);
        }
        let at = self.hits.get(anchor).map_or(self.dock_view.center().x, |r| r.center().x);
        let x = (at - size.x / 2.0).min(self.dock_view.right() - 12.0 - size.x).max(self.dock_view.left() + 12.0);
        Rect::from_min_size(pos2(x, (self.dock_top - 8.0 - size.y).max(TOP)), size)
    }

    /// Open the rename field on tree node `node`, in world mode.
    pub fn start_rename(&mut self, node: Option<usize>) {
        let Some(row) = node.and_then(|n| self.tree.iter().find(|r| r.node == n)) else { return };
        if self.world {
            self.renaming = Some((Named::Node(row.node), row.name.clone()));
            self.rename_focus = true;
        }
    }

    /// Open the rename field on layer `id`, in world mode.
    pub fn start_layer_rename(&mut self, id: Option<i32>) {
        let Some(row) = id.and_then(|id| self.layers.iter().find(|l| l.id == id)) else { return };
        if self.world && !self.tree.is_empty() {
            self.renaming = Some((Named::Layer(row.id), row.name.clone()));
            self.rename_focus = true;
        }
    }

    /// New scene: start it now, or ask first when the scene has unsaved edits to lose.
    pub fn request_new(&mut self) {
        self.cmdk = None;
        if self.unsaved {
            self.confirm_new = true;
        } else {
            self.actions.push(Action::New);
        }
    }

    fn escape(&mut self) {
        if self.ask_bar.take().is_some() || std::mem::take(&mut self.ai.menu) {
            return;
        }
        if self.slot_op.take().is_some() || self.color_edit.take().is_some() || self.bounds_panel.take().is_some() {
            return;
        }
        if std::mem::take(&mut self.agents_panel) {
            return;
        }
        if std::mem::take(&mut self.view_panel)
            || std::mem::take(&mut self.export_menu)
            || std::mem::take(&mut self.theme_menu)
        {
            return;
        }
        if self.size_shown() && std::mem::take(&mut self.size_panel) {
            return;
        }
        if self.render_panel {
            self.render_panel = false;
        } else if self.full_render {
            self.full_render = false;
        } else if self.review.is_some() {
            self.actions.push(Action::DiscardReview);
        } else if self.selected > 0 {
            self.actions.push(Action::Deselect);
        } else {
            self.coach = false;
        }
    }

    /// A swatch was clicked: pick it, or finish an armed swap or copy with it.
    fn click_slot(&mut self, slot: u8) {
        match self.slot_op.take() {
            Some(SlotOp::Swap) if slot != self.slot => self.actions.push(Action::SwapColors(self.slot, slot)),
            Some(SlotOp::Copy) if slot != self.slot => {
                self.actions.push(Action::CopyColor { from: self.slot, to: slot })
            }
            _ => {}
        }
        self.slot = slot;
    }

    fn color(&self, slot: u8) -> Color32 {
        let c = self.palette[slot as usize];
        Color32::from_rgb(c[0], c[1], c[2])
    }

    /// Pick a theme; the host keeps it.
    pub fn set_theme(&mut self, choice: ThemeChoice) {
        if self.theme != choice {
            self.theme = choice;
            self.actions.push(Action::Theme);
        }
    }

    /// The theme the picked one shows now: System follows the system's setting (dark when the
    /// system does not say).
    pub fn shown_theme(&self, ctx: &Context) -> &'static Theme {
        self.theme.resolve(ctx.system_theme().is_none_or(|t| t == egui::Theme::Dark))
    }

    /// Draw with the shown theme, restyling egui when it changed.
    fn apply_theme(&mut self, ctx: &Context) {
        let t = self.shown_theme(ctx);
        theme::set(t);
        if self.styled != Some(t.name) {
            self.styled = Some(t.name);
            style(ctx);
            ctx.request_repaint();
        }
    }

    /// Draw the whole UI and return what the app should do.
    pub fn ui(&mut self, ctx: &Context, stats: &Stats) -> Vec<Action> {
        self.apply_theme(ctx);
        self.keys(ctx);
        self.hits.clear();
        let screen = ctx.content_rect();
        self.phone = self.touch && screen.width().min(screen.height()) < PHONE_MIN;
        self.narrow = self.phone && screen.width() < NARROW_W;
        self.short = self.phone && !self.narrow;
        self.top_bar(ctx, screen);
        // The docked assistant takes the right edge; the panels anchored there move left of it.
        // Held upright on a phone it is a sheet above the dock instead.
        self.assistant_panel(ctx, screen);
        let full = screen;
        let screen = Rect::from_min_max(screen.min, pos2(screen.right() - self.ai_width(), screen.bottom()));
        if self.touch {
            self.dock(ctx, screen);
        } else {
            self.rail(ctx);
        }
        // On a phone every panel fits the room the dock leaves.
        let region = self.phone.then(|| self.panel_region(full));
        ctx.data_mut(|d| d.insert_temp(Id::unique("fit-region"), region));
        // The assistant sheet covers the camera bar and its View flyout.
        let sheet = self.sheet();
        if self.phone {
            // Under the flyouts, which open over the camera bar and the render corner.
            self.render_corner(ctx, screen, stats);
            if sheet {
                self.nav_rect = None;
            } else {
                self.nav_bar(ctx, screen);
            }
        }
        self.fitted(ctx, |s| s.tree_panel(ctx));
        self.fitted(ctx, |s| s.axes(ctx));
        self.fitted(ctx, |s| s.size_flyout(ctx));
        self.fitted(ctx, |s| s.selection_panel(ctx));
        self.fitted(ctx, |s| s.bounds_panel(ctx));
        self.fitted(ctx, |s| s.new_confirm(ctx, screen));
        self.fitted(ctx, |s| s.export_menu(ctx));
        self.fitted(ctx, |s| s.theme_menu(ctx));
        self.marquee(ctx);
        self.fitted(ctx, |s| s.palette_panel(ctx, screen));
        self.fitted(ctx, |s| s.color_editor(ctx, screen));
        self.fitted(ctx, |s| s.review_card(ctx, screen));
        self.fitted(ctx, |s| s.agent_card(ctx, screen));
        // On a phone the assistant covers the tips card, which would show through it.
        if self.coach && !(self.phone && self.ai_open) {
            self.fitted(ctx, |s| s.coach_card(ctx, screen));
        }
        if self.touch {
            self.stats_right = screen.left() + 4.0;
        } else {
            self.stats(ctx, screen, stats);
        }
        if !self.phone {
            self.render_corner(ctx, screen, stats);
        }
        self.fitted(ctx, |s| s.render_panel(ctx, screen, stats));
        if !self.phone {
            self.nav_bar(ctx, screen);
        }
        if !sheet {
            self.fitted(ctx, |s| s.view_panel(ctx, screen));
        }
        if !self.touch {
            self.hint_bar(ctx, screen);
        } else {
            self.gesture_hints(ctx, screen);
        }
        self.fitted(ctx, |s| s.ask_bar(ctx, screen));
        self.fitted(ctx, |s| s.agents_panel(ctx, screen));
        self.toast_panel(ctx, screen);
        if self.cmdk.is_some() {
            self.fitted(ctx, |s| s.cmdk(ctx, full));
        }
        if std::mem::take(&mut self.camera_tip) {
            self.toast(
                ctx,
                "Right-drag orbits · Middle-drag or Space-drag pans · Scroll or pinch zooms · X-click orbits there",
            );
        }
        if self.shown_theme(ctx).name != self.styled.unwrap_or_default() {
            // Picked this frame: draw the next one in it.
            ctx.request_repaint();
        }
        std::mem::take(&mut self.actions)
    }

    fn keys(&mut self, ctx: &Context) {
        let typing = ctx.memory(|m| m.focused().is_some());
        ctx.input_mut(|i| {
            self.shift = i.modifiers.shift;
            self.alt = i.modifiers.alt;
            // egui drops focus on Escape before the UI runs, so the rename field's Escape
            // (cancel) is handled here, ahead of the keys Escape would otherwise reach.
            if self.renaming.is_some() && i.consume_key(Modifiers::NONE, Key::Escape) {
                self.renaming = None;
                return;
            }
            // The same for the review card's ask field: Escape leaves the field, it does not
            // discard the change.
            if self.ask_focus && i.consume_key(Modifiers::NONE, Key::Escape) {
                self.ask_focus = false;
                return;
            }
            if self.ai.focus && i.consume_key(Modifiers::NONE, Key::Escape) {
                self.ai.focus = false;
                return;
            }
            // The New scene confirm takes every key: Enter starts the blank scene, Escape keeps this one.
            if self.confirm_new {
                if i.consume_key(Modifiers::NONE, Key::Escape) {
                    self.confirm_new = false;
                } else if i.consume_key(Modifiers::NONE, Key::Enter) {
                    self.confirm_new = false;
                    self.actions.push(Action::New);
                }
                return;
            }
            if i.consume_key(Modifiers::COMMAND, Key::J) {
                self.toggle_ai();
            }
            if i.consume_key(Modifiers::COMMAND, Key::K) {
                self.cmdk = if self.cmdk.is_some() { None } else { Some(String::new()) };
                self.cmdk_sel = 0;
            }
            if self.cmdk.is_some() || typing {
                return;
            }
            if self.review.is_some() && i.consume_key(Modifiers::NONE, Key::Enter) {
                self.actions.push(Action::ApplyReview);
            }
            if self.lifting {
                if i.consume_key(Modifiers::NONE, Key::Enter) {
                    self.actions.push(Action::PlaceBox);
                }
                if i.consume_key(Modifiers::NONE, Key::Escape) {
                    self.actions.push(Action::CancelBox);
                }
            }
            if i.consume_key(Modifiers::COMMAND | Modifiers::SHIFT, Key::Z) {
                self.actions.push(Action::Redo);
            }
            if i.consume_key(Modifiers::COMMAND, Key::Z) {
                self.actions.push(Action::Undo);
            }
            if i.consume_key(Modifiers::COMMAND, Key::Equals) || i.consume_key(Modifiers::COMMAND, Key::Plus) {
                self.actions.push(Action::Zoom(true));
            }
            if i.consume_key(Modifiers::COMMAND, Key::Minus) {
                self.actions.push(Action::Zoom(false));
            }
            if i.consume_key(Modifiers::COMMAND, Key::N) {
                self.request_new();
            }
            if i.consume_key(Modifiers::COMMAND, Key::O) {
                self.actions.push(Action::Open);
            }
            if i.consume_key(Modifiers::COMMAND, Key::S) {
                self.actions.push(Action::Save);
            }
            if i.consume_key(Modifiers::COMMAND, Key::A) {
                self.actions.push(Action::SelectAll);
            }
            if i.consume_key(Modifiers::COMMAND | Modifiers::SHIFT, Key::I) {
                self.actions.push(Action::InvertSelection);
            }
            if i.consume_key(Modifiers::COMMAND | Modifiers::SHIFT, Key::D) {
                self.actions.push(Action::Duplicate { linked: true });
            }
            if i.consume_key(Modifiers::COMMAND, Key::D) {
                self.actions.push(Action::Duplicate { linked: false });
            }
            if i.consume_key(Modifiers::COMMAND | Modifiers::SHIFT, Key::G) {
                self.actions.push(Action::Ungroup);
            }
            if i.consume_key(Modifiers::COMMAND, Key::G) {
                self.actions.push(Action::Group);
            }
            // Natively egui-winit turns Cmd-C, Cmd-X and Cmd-V into clipboard events (Paste
            // only when the system clipboard has text); the browser page sends the keys.
            let (mut copy, mut cut, mut paste) = (false, false, false);
            i.events.retain(|e| match e {
                egui::Event::Copy => !std::mem::replace(&mut copy, true),
                egui::Event::Cut => !std::mem::replace(&mut cut, true),
                egui::Event::Paste(_) => !std::mem::replace(&mut paste, true),
                _ => true,
            });
            copy |= i.consume_key(Modifiers::COMMAND, Key::C);
            cut |= i.consume_key(Modifiers::COMMAND, Key::X);
            paste |= i.consume_key(Modifiers::COMMAND, Key::V);
            for (on, a) in [(copy, Action::Copy), (cut, Action::Cut), (paste, Action::Paste)] {
                if on {
                    self.actions.push(a);
                }
            }
            if self.selected > 0 {
                for key in [Key::Delete, Key::Backspace] {
                    if i.consume_key(Modifiers::NONE, key) {
                        self.actions.push(Action::DeleteSelection);
                    }
                }
            }
            for (key, d) in [(Key::ArrowUp, 1), (Key::ArrowDown, -1)] {
                if self.selected > 0 && i.consume_key(Modifiers::SHIFT, key) {
                    self.actions.push(Action::Transform(Transform::Move([0, d, 0])));
                }
            }
            // Arrows move along the ground; the editor turns them to match the view.
            for key in [Key::ArrowLeft, Key::ArrowRight, Key::ArrowUp, Key::ArrowDown] {
                if self.selected > 0 && i.consume_key(Modifiers::NONE, key) {
                    self.arrows.push(key);
                }
            }
            for (key, f) in [
                (Key::A, (|h: &mut Hybrid| h.tool = Tool::Add) as fn(&mut Hybrid)),
                (Key::E, |h| h.tool = Tool::Erase),
                (Key::P, |h| h.tool = Tool::Paint),
                (Key::I, |h| h.tool = Tool::Pick),
                (Key::S, |h| h.tool = Tool::Select),
                (Key::V, |h| h.voxel_brush()),
                (Key::B, |h| h.shape = Shape::Box),
                (Key::F, |h| h.shape = Shape::Face),
                (Key::L, |h| h.shape = Shape::Line),
                (Key::C, |h| h.center_brush()),
                (Key::M, |h| h.mirror = !h.mirror),
                (Key::K, |h| h.cycle_lock()),
                (Key::R, |h| h.toggle_render_panel()),
                (Key::Escape, |h| h.escape()),
                // The coach card's "? for every key": the command list shows every key.
                (Key::Questionmark, |h| {
                    h.cmdk = Some(String::new());
                    h.cmdk_sel = 0;
                }),
                (Key::Period, |h| h.actions.push(Action::Frame)),
                (Key::Home, |h| h.actions.push(Action::ResetCamera)),
                (Key::O, |h| h.actions.push(Action::Ortho(!h.ortho))),
                (Key::G, |h| h.grid = !h.grid),
                (Key::H, |h| {
                    if h.world {
                        h.actions.push(Action::ToggleHidden)
                    }
                }),
                (Key::Num0, |h| h.actions.push(Action::Snap(View::Iso))),
                (Key::Num1, |h| h.actions.push(Action::Snap(View::Front))),
                (Key::Num2, |h| h.actions.push(Action::Snap(View::Back))),
                (Key::Num3, |h| h.actions.push(Action::Snap(View::Left))),
                (Key::Num4, |h| h.actions.push(Action::Snap(View::Right))),
                (Key::Num5, |h| h.actions.push(Action::Snap(View::Top))),
                (Key::Num6, |h| h.actions.push(Action::Snap(View::Bottom))),
                (Key::Minus, |h| h.step_size(-1)),
                (Key::Equals, |h| h.step_size(1)),
                (Key::Plus, |h| h.step_size(1)),
                (Key::OpenBracket, |h| h.slot = h.slot.saturating_sub(1).max(1)),
                (Key::CloseBracket, |h| h.slot = h.slot.saturating_add(1)),
            ] {
                if i.consume_key(Modifiers::NONE, key) {
                    f(self);
                }
            }
        });
    }

    /// The bars that lay themselves out to the phone's width; every other panel fits the region.
    const OWN_LAYOUT: [&str; 9] =
        ["top", "top-right", "dock", "dock-brushes", "dock-more", "assistant", "nav", "toast", "stats"];

    /// On a phone (the region is set), scale the panel `id` down about its corner until it fits
    /// the region, and move it inside. The bars that lay themselves out keep their place.
    fn fit_panel(ctx: &Context, id: &str, order: Order, rect: Rect) -> bool {
        if Self::OWN_LAYOUT.contains(&id) {
            return false;
        }
        let layer = egui::LayerId::new(order, Id::unique(id));
        let Some(region) = ctx.data(|d| d.get_temp::<Option<Rect>>(Id::unique("fit-region"))).flatten() else {
            ctx.set_transform_layer(layer, TSTransform::IDENTITY);
            return false;
        };
        let k = (region.width() / rect.width()).min(region.height() / rect.height()).min(1.0);
        let size = rect.size() * k;
        let at = pos2(
            rect.min.x.clamp(region.left(), (region.right() - size.x).max(region.left())),
            rect.min.y.clamp(region.top(), (region.bottom() - size.y).max(region.top())),
        );
        if k >= 1.0 && at == rect.min {
            ctx.set_transform_layer(layer, TSTransform::IDENTITY);
            return false;
        }
        let t = TSTransform::new(at.to_vec2() - rect.min.to_vec2() * k, k);
        ctx.set_transform_layer(layer, t);
        let log = Id::unique("fit-log");
        ctx.data_mut(|d| {
            if let Some(v) = d.get_temp::<Vec<(Rect, TSTransform)>>(log) {
                let mut v = v;
                v.push((rect, t));
                d.insert_temp(log, v);
            }
        });
        true
    }

    // .panel
    fn panel(ctx: &Context, id: &str, rect: Rect, order: Order, contents: impl FnOnce(&mut Ui, Rect)) {
        // On a phone the panels over the view are solid, so the camera bar behind one does not show.
        let over_view = ctx.data(|d| d.get_temp::<Option<Rect>>(Id::unique("fit-region"))).flatten().is_some()
            && !Self::OWN_LAYOUT.contains(&id);
        Self::panel_filled(ctx, id, rect, order, if over_view { th().panel_solid } else { th().panel }, contents)
    }

    /// A panel with its own fill: the theme's `panel_solid` for a menu over other panels.
    fn panel_filled(
        ctx: &Context,
        id: &str,
        rect: Rect,
        order: Order,
        fill: Color32,
        contents: impl FnOnce(&mut Ui, Rect),
    ) {
        let scaled = Self::fit_panel(ctx, id, order, rect);
        Area::new(Id::unique(id)).fade_in(false).order(order).fixed_pos(rect.min).constrain(false).show(ctx, |ui| {
            if scaled {
                // The screen clips the panel where it would be unscaled, not where it shows.
                ui.set_clip_rect(rect.expand(64.0));
            }
            let p = ui.painter();
            p.add(
                Shadow { offset: [0, 10], blur: 30, spread: 0, color: th().shadow }
                    .as_shape(rect, CornerRadius::same(R)),
            );
            p.rect(rect, CornerRadius::same(R), fill, Stroke::new(1.0, th().line), StrokeKind::Inside);
            ui.allocate_rect(rect, Sense::hover());
            contents(ui, rect);
        });
    }

    // #top: two 40 px islands, so the scene shows between them. The left one has the scene,
    // the file actions as icons and undo/redo; the right one the assistant, the keys and Embed.
    fn top_bar(&mut self, ctx: &Context, screen: Rect) {
        let name = ctx.fonts_mut(|f| f.layout_no_wrap(self.scene_name.clone(), sans(13.0), th().text));
        let brand = ctx.fonts_mut(|f| f.layout_no_wrap("vxr".into(), brand_font(13.0), th().text));
        let sel_w = name.size().x + 34.0;
        // Over the dock, undo and redo are on it.
        let history = !self.touch;
        // Held upright a phone has room for the file buttons, the theme, the assistant and the
        // keyboard button only: no name, no New and no Embed (both in the command list, which the
        // keyboard button opens).
        let slim = self.narrow;
        let w = if slim {
            4.0 + 3.0 * 36.0 + 4.0
        } else {
            12.0 + LOGO_STEP
                + brand.size().x
                + 10.0
                + sel_w
                + 13.0
                + 4.0 * 36.0
                + if history { 13.0 + 2.0 * 36.0 } else { 0.0 }
                + 4.0
        };
        let left = Rect::from_min_size(pos2(12.0, 12.0), vec2(w, 40.0));
        let mut tip: Option<(Rect, &'static str, &'static [&'static str])> = None;
        Self::panel(ctx, "top", left, Order::Middle, |ui, rect| {
            let cy = rect.center().y;
            let mut x = rect.left() + 12.0;
            let sep = |ui: &Ui, x: &mut f32| {
                *x += 6.0;
                ui.painter()
                    .line_segment([pos2(*x + 0.5, cy - 10.0), pos2(*x + 0.5, cy + 10.0)], Stroke::new(1.0, th().line));
                *x += 7.0;
            };
            if slim {
                x = rect.left() + 4.0;
            } else {
                logo(ui, pos2(x, cy - 9.0), 18.0);
                x += LOGO_STEP;
                ui.painter().galley(pos2(x, cy - brand.size().y / 2.0), brand.clone(), th().text);
                x += brand.size().x + 10.0;
                // #scene-select
                let sel = Rect::from_min_size(pos2(x, cy - 14.0), vec2(sel_w, 28.0));
                let resp = ui.interact(sel, Id::unique("scene-select"), Sense::click());
                let p = ui.painter();
                p.rect(sel, CornerRadius::same(7), th().tint(10), Stroke::new(1.0, th().line), StrokeKind::Inside);
                if resp.hovered() {
                    p.rect_filled(sel, CornerRadius::same(7), th().hover);
                }
                p.galley(pos2(sel.left() + 10.0, cy - name.size().y / 2.0), name, th().text);
                p.text(
                    pos2(sel.right() - 13.0, cy),
                    Align2::CENTER_CENTER,
                    icon::CHEVRON_DOWN,
                    icon_font(12.0),
                    th().muted,
                );
                x = sel.right();
                sep(ui, &mut x);
            }
            let file: [(&str, char, &'static str, &'static [&'static str]); 4] = [
                ("new", icon::FILE_PLUS, "New scene", &[]),
                ("open", icon::FOLDER_OPEN, "Open .vox", &["⌘", "O"]),
                ("save", icon::DOWNLOAD, "Save .vox", &["⌘", "S"]),
                ("export", icon::FILE_OUTPUT, "Export", &[]),
            ];
            for (id, glyph, label, keys) in file {
                if slim && id == "new" {
                    continue;
                }
                if id == "export" {
                    self.export_x = x;
                }
                let r = tbtn_on(ui, &mut x, cy, Some(glyph), "", id, true, id == "export" && self.export_menu);
                self.hits.insert(id.into(), r.rect);
                if r.hovered() && !(id == "export" && self.export_menu) {
                    tip = Some((r.rect, label, keys));
                }
                if r.clicked() {
                    match id {
                        "new" => self.request_new(),
                        "open" => self.actions.push(Action::Open),
                        "save" => self.actions.push(Action::Save),
                        _ => self.export_menu = !self.export_menu,
                    }
                }
            }
            if !history {
                return;
            }
            sep(ui, &mut x);
            // #history, moved up from its own card.
            for (id, glyph, label, keys, enabled, action) in [
                ("undo", icon::UNDO, "Undo", &["⌘", "Z"][..], self.can_undo, Action::Undo),
                ("redo", icon::REDO, "Redo", &["⌘", "⇧", "Z"][..], self.can_redo, Action::Redo),
            ] {
                let r = tbtn(ui, &mut x, cy, Some(glyph), "", id, enabled);
                self.hits.insert(id.into(), r.rect);
                if r.hovered() {
                    tip = Some((r.rect, label, keys));
                }
                if r.clicked() {
                    self.actions.push(action);
                }
            }
        });
        if let Some((at, label, keys)) = tip {
            // Below the button: above it is the window's edge.
            tooltip(ctx, at, label, keys, Some((Rect::from_min_max(pos2(-1e6, -1e6), pos2(1e6, at.top())), true)));
        }
        // The right island, laid out from the right edge.
        let embed_label = ctx.fonts_mut(|f| f.layout_no_wrap("Embed".into(), sans_semibold(13.0), th().on_accent));
        let ew = if slim { 0.0 } else { 10.0 + 16.0 + 6.0 + embed_label.size().x + 10.0 + 4.0 };
        let rw = 4.0 + 36.0 + 4.0 + 36.0 + 4.0 + 36.0 + 4.0 + ew;
        let right = Rect::from_min_size(pos2(screen.right() - 12.0 - rw, 12.0), vec2(rw, 40.0));
        let mut theme_tip = None;
        Self::panel(ctx, "top-right", right, Order::Middle, |ui, rect| {
            let cy = rect.center().y;
            let edge = rect.right() - ew;
            if !slim {
                let embed = Rect::from_min_size(pos2(edge, cy - 16.0), vec2(ew - 4.0, 32.0));
                let resp = ui.interact(embed, Id::unique("embed"), Sense::click());
                let fill = if resp.hovered() { th().accent_hover } else { th().accent };
                let p = ui.painter();
                p.rect_filled(embed, CornerRadius::same(8), fill);
                p.text(
                    pos2(embed.left() + 18.0, cy),
                    Align2::CENTER_CENTER,
                    icon::CODE,
                    icon_font(16.0),
                    th().on_accent,
                );
                p.galley(pos2(embed.left() + 32.0, cy - embed_label.size().y / 2.0), embed_label, th().on_accent);
                if resp.clicked() {
                    self.actions.push(Action::Embed);
                }
            }
            let mut kx = edge - 4.0 - 36.0;
            let r = tbtn(ui, &mut kx, cy, Some(icon::KEYBOARD), "", "keys", true);
            self.hits.insert("keys".into(), r.rect);
            if r.clicked() {
                self.cmdk = Some(String::new());
            }
            // The assistant panel (Cmd-J).
            let mut ax = edge - 4.0 - 36.0 - 4.0 - 36.0;
            let r = tbtn_on(ui, &mut ax, cy, Some(icon::SPARKLES), "", "assistant", true, self.ai_open);
            self.hits.insert("ai-toggle".into(), r.rect);
            if self.ai_open {
                ui.painter().text(r.rect.center(), Align2::CENTER_CENTER, icon::SPARKLES, icon_font(16.0), th().accent);
            }
            if r.clicked() {
                self.toggle_ai();
            }
            // The Theme menu.
            let mut tx = ax - 36.0 - 4.0 - 36.0;
            let r = tbtn_on(ui, &mut tx, cy, Some(icon::PALETTE), "", "theme", true, self.theme_menu);
            self.hits.insert("theme".into(), r.rect);
            if r.hovered() && !self.theme_menu {
                theme_tip = Some(r.rect);
            }
            if r.clicked() {
                self.theme_menu = !self.theme_menu;
            }
        });
        if let Some(at) = theme_tip {
            tooltip(ctx, at, "Theme", &[], Some((Rect::from_min_max(pos2(-1e6, -1e6), pos2(1e6, at.top())), true)));
        }
    }

    // The Theme menu under the top bar's theme button: System (Neutral in dark mode, Paper in
    // light), then each theme with its card, accent and text colours; a check on the picked one.
    fn theme_menu(&mut self, ctx: &Context) {
        if !self.theme_menu {
            return;
        }
        let button = self.hits.get("theme").copied().unwrap_or(Rect::NOTHING);
        let screen = ctx.content_rect();
        let w = 236.0;
        let rows = ThemeChoice::ALL;
        let h = 36.0 + 34.0 * rows.len() as f32 + 6.0;
        let x = (button.left()).min(screen.right() - 12.0 - w);
        let rect = Rect::from_min_size(pos2(x, 12.0 + 44.0 + 6.0), vec2(w, h));
        self.hits.insert("theme-menu".into(), rect);
        let shown = self.shown_theme(ctx).name;
        let picked = self.theme;
        let mut run = None;
        let mut hits = Vec::new();
        Self::panel_filled(ctx, "theme-menu", rect, Order::Foreground, th().panel_solid, |ui, rect| {
            ui.allocate_rect(rect, Sense::click());
            let hy = rect.top() + 18.0;
            let g = ui.painter().layout_job(spaced("THEME", sans(9.0), th().faint, 0.72));
            ui.painter().galley(pos2(rect.left() + 12.0, hy - g.size().y / 2.0), g, th().faint);
            for (k, choice) in rows.into_iter().enumerate() {
                let r = Rect::from_min_size(
                    pos2(rect.left() + 6.0, rect.top() + 36.0 + 34.0 * k as f32),
                    vec2(rect.width() - 12.0, 32.0),
                );
                let id = format!("theme-{}", choice.id());
                let resp = ui.interact(r, Id::unique(&id), Sense::click());
                hits.push((id, r));
                let p = ui.painter();
                if choice == picked {
                    p.rect_filled(r, CornerRadius::same(7), th().accent_bg);
                } else if resp.hovered() {
                    p.rect_filled(r, CornerRadius::same(7), th().hover);
                }
                let cy = r.center().y;
                // A chip of the theme: its card, with its accent and text as dots. System shows
                // its dark and light halves.
                let chip = Rect::from_center_size(pos2(r.left() + 22.0, cy), vec2(28.0, 18.0));
                let swatch = |p: &egui::Painter, t: &Theme, area: Rect| {
                    p.rect_filled(area, CornerRadius::same(4), t.panel_solid);
                    p.circle_filled(pos2(area.center().x - 3.5, cy), 3.0, t.accent);
                    p.circle_filled(pos2(area.center().x + 4.0, cy), 3.0, t.text);
                };
                if choice == ThemeChoice::System {
                    let (dark, light) = (choice.resolve(true), choice.resolve(false));
                    let (a, b) = chip.split_left_right_at_fraction(0.5);
                    p.rect_filled(a, CornerRadius { nw: 4, sw: 4, ne: 0, se: 0 }, dark.panel_solid);
                    p.rect_filled(b, CornerRadius { nw: 0, sw: 0, ne: 4, se: 4 }, light.panel_solid);
                    p.circle_filled(a.center(), 3.0, dark.text);
                    p.circle_filled(b.center(), 3.0, light.text);
                } else {
                    swatch(p, choice.resolve(true), chip);
                }
                p.rect_stroke(chip, CornerRadius::same(4), Stroke::new(1.0, th().line2), StrokeKind::Inside);
                let color = if choice == picked || resp.hovered() { th().text } else { th().muted };
                p.text(pos2(r.left() + 46.0, cy), Align2::LEFT_CENTER, choice.name(), sans(13.0), color);
                let mut right = r.right() - 10.0;
                if choice == picked {
                    p.text(pos2(right - 6.0, cy), Align2::CENTER_CENTER, icon::CHECK, icon_font(14.0), th().text);
                    right -= 20.0;
                }
                if choice == ThemeChoice::System {
                    p.text(pos2(right, cy), Align2::RIGHT_CENTER, "Light / dark", sans(11.0), th().faint);
                } else if picked == ThemeChoice::System && choice.resolve(true).name == shown {
                    p.text(pos2(right, cy), Align2::RIGHT_CENTER, "now", sans(11.0), th().faint);
                }
                if resp.clicked() {
                    run = Some(choice);
                }
            }
        });
        self.hits.extend(hits);
        let outside = ctx.input(|i| {
            i.pointer.any_pressed()
                && i.pointer.interact_pos().is_some_and(|p| !rect.contains(p) && !button.contains(p))
        });
        if let Some(choice) = run {
            self.set_theme(choice);
        }
        if outside {
            self.theme_menu = false;
        }
    }

    // #rail
    fn rail(&mut self, ctx: &Context) {
        let h = 6.0 + 5.0 * TOOL_STEP + 9.0 + 5.0 * TOOL_STEP + 9.0 + 2.0 * TOOL_STEP + 6.0 - 2.0;
        let rect = Rect::from_min_size(pos2(12.0, TOP), vec2(RAIL_W, h));
        Self::panel(ctx, "rail", rect, Order::Middle, |ui, rect| {
            let x = rect.left() + 6.0;
            let mut y = rect.top() + 6.0;
            let tool = self.tool;
            for (t, ic, k) in [
                (Tool::Add, icon::BOX, "A"),
                (Tool::Erase, icon::ERASER, "E"),
                (Tool::Paint, icon::PAINTBRUSH, "P"),
                (Tool::Pick, icon::PIPETTE, "I"),
                (Tool::Select, icon::SELECT, "S"),
            ] {
                if t == Tool::Select {
                    self.select_y = y;
                }
                let r = tool_button(ui, pos2(x, y), ic, k, tool == t);
                if t == Tool::Select {
                    self.hits.insert("select".into(), r.rect);
                }
                if r.clicked() {
                    self.tool = t;
                }
                y += TOOL_STEP;
            }
            rail_sep(ui, rect, &mut y);
            let shape = self.shape;
            for (s, ic, k) in [
                (Shape::Voxel, icon::POINTER, "V"),
                (Shape::Box, icon::SQUARE_DASHED, "B"),
                (Shape::Face, icon::LAYERS, "F"),
                (Shape::Line, icon::SLASH, "L"),
            ] {
                let r = tool_button(ui, pos2(x, y), ic, k, shape == s);
                if s == Shape::Voxel {
                    self.voxel_y = y;
                    self.hits.insert("voxel".into(), r.rect);
                }
                if r.clicked() {
                    if s == Shape::Voxel {
                        self.voxel_brush();
                    } else {
                        self.shape = s;
                    }
                }
                y += TOOL_STEP;
            }
            // One button for the center brush; its icon shows circle or sphere.
            let center = matches!(shape, Shape::Circle | Shape::Sphere);
            let ic = if shape == Shape::Sphere { icon::GLOBE } else { icon::CIRCLE_DOT };
            let r = tool_button(ui, pos2(x, y), ic, "C", center);
            self.hits.insert("center".into(), r.rect);
            if r.clicked() {
                self.center_brush();
            }
            y += TOOL_STEP;
            rail_sep(ui, rect, &mut y);
            self.axes_y = y;
            let r = tool_button(ui, pos2(x, y), icon::FLIP, "M", self.mirror);
            self.hits.insert("mirror".into(), r.rect);
            if r.clicked() {
                self.mirror = !self.mirror;
            }
            y += TOOL_STEP;
            let r = tool_button(ui, pos2(x, y), icon::LOCK, "K", self.lock.is_some());
            self.hits.insert("lock".into(), r.rect);
            if r.clicked() {
                self.cycle_lock();
            }
        });
    }

    // The dock, in place of the rail on a touch screen: the tools, the colour (it opens the
    // palette above) and undo/redo along the bottom; the brushes, mirror and axis lock on a
    // bar above. Flyouts open over both. Where the view is too narrow for those two bars they
    // become three: the tools, the brushes, then the colour, mirror, axis lock and undo/redo.
    fn dock(&mut self, ctx: &Context, screen: Rect) {
        let rows = self.dock_rows(screen);
        let three = rows.len() == 3;
        (self.dock_top, self.dock_view) = (rows[rows.len() - 1].top(), screen);
        self.dock_area = rows.iter().fold(rows[0], |a, r| a.union(*r));
        for (name, r) in ["dock", "dock-brushes", "dock-more"].into_iter().zip(&rows) {
            self.hits.insert(name.into(), *r);
        }
        let mut hits = Vec::new();
        Self::panel(ctx, "dock", rows[0], Order::Middle, |ui, rect| {
            let (mut x, y) = (rect.left() + 5.0, rect.top() + 5.0);
            self.dock_tools(ui, &mut x, y, &mut hits);
            if !three {
                dock_sep(ui, rect, &mut x);
                self.dock_color(ui, &mut x, y, &mut hits);
                dock_sep(ui, rect, &mut x);
                self.dock_history(ui, &mut x, y, &mut hits);
            }
        });
        Self::panel(ctx, "dock-brushes", rows[1], Order::Middle, |ui, rect| {
            let (mut x, y) = (rect.left() + 5.0, rect.top() + 5.0);
            self.dock_brushes(ui, &mut x, y, &mut hits);
            if !three {
                dock_sep(ui, rect, &mut x);
                self.dock_axes(ui, &mut x, y, &mut hits);
            }
        });
        if three {
            Self::panel(ctx, "dock-more", rows[2], Order::Middle, |ui, rect| {
                let (mut x, y) = (rect.left() + 5.0, rect.top() + 5.0);
                self.dock_color(ui, &mut x, y, &mut hits);
                dock_sep(ui, rect, &mut x);
                self.dock_axes(ui, &mut x, y, &mut hits);
                dock_sep(ui, rect, &mut x);
                self.dock_history(ui, &mut x, y, &mut hits);
            });
        }
        self.hits.extend(hits.into_iter().map(|(n, r)| (n.to_string(), r)));
    }

    /// The dock's bars, bottom first, in a view of `view`: two where they fit, else three.
    /// Centred, or at the left of a phone held sideways.
    fn dock_rows(&self, view: Rect) -> Vec<Rect> {
        let step = DOCK_BTN + 2.0;
        let bar = |n: f32, seps: f32| 5.0 + n * step - 2.0 + seps * sep_w() + 5.0;
        let widths: &[f32] = if view.width() < bar(8.0, 2.0) + 24.0 {
            &[bar(5.0, 0.0), bar(5.0, 0.0), bar(5.0, 2.0)]
        } else {
            &[bar(8.0, 2.0), bar(7.0, 1.0)]
        };
        let mut top = view.bottom() - 12.0;
        widths
            .iter()
            .map(|w| {
                top -= DOCK_H;
                let x = if self.short { view.left() + 12.0 } else { view.center().x - w / 2.0 };
                let r = Rect::from_min_size(pos2(x, top), vec2(*w, DOCK_H));
                top -= 8.0;
                r
            })
            .collect()
    }

    /// The assistant is a sheet above the dock: a phone held upright, with the panel open.
    fn sheet(&self) -> bool {
        self.narrow && self.ai_open
    }

    /// The room a phone's panels fit in: above the dock, or beside it when the phone is held
    /// sideways and the dock leaves no height.
    fn panel_region(&self, full: Rect) -> Rect {
        let view = Rect::from_min_max(full.min, pos2(full.right() - self.ai_width(), full.bottom()));
        let rows = self.dock_rows(view);
        if self.short {
            let right = rows.iter().map(|r| r.right()).fold(f32::MIN, f32::max);
            Rect::from_min_max(pos2(right + 8.0, TOP), pos2(full.right() - 12.0, full.bottom() - 12.0))
        } else {
            Rect::from_min_max(
                pos2(full.left() + 12.0, TOP),
                pos2(full.right() - 12.0, rows[rows.len() - 1].top() - 8.0),
            )
        }
    }

    /// Run `draw`, a panel that opens over the view: where it scaled itself to fit the phone's
    /// panel region, its hits move with it.
    fn fitted(&mut self, ctx: &Context, draw: impl FnOnce(&mut Self)) {
        if !self.phone {
            return draw(self);
        }
        let before = self.hits.clone();
        let log = Id::unique("fit-log");
        ctx.data_mut(|d| d.insert_temp(log, Vec::<(Rect, TSTransform)>::new()));
        draw(self);
        let fits = ctx.data_mut(|d| d.remove_temp::<Vec<(Rect, TSTransform)>>(log)).unwrap_or_default();
        for (name, r) in &mut self.hits {
            if before.get(name) == Some(r) {
                continue;
            }
            if let Some((_, t)) = fits.iter().find(|(from, _)| from.expand(0.5).contains_rect(*r)) {
                *r = Rect::from_min_max(*t * r.min, *t * r.max);
            }
        }
    }

    fn dock_tools(&mut self, ui: &mut Ui, x: &mut f32, y: f32, hits: &mut Vec<(&'static str, Rect)>) {
        let tool = self.tool;
        for (t, ic, id) in [
            (Tool::Add, icon::BOX, "tool-add"),
            (Tool::Erase, icon::ERASER, "tool-erase"),
            (Tool::Paint, icon::PAINTBRUSH, "tool-paint"),
            (Tool::Pick, icon::PIPETTE, "tool-pick"),
            (Tool::Select, icon::SELECT, "select"),
        ] {
            let r = dock_button(ui, pos2(*x, y), ic, id, tool == t, true);
            hits.push((id, r.rect));
            if r.clicked() {
                self.tool = t;
            }
            *x += DOCK_BTN + 2.0;
        }
    }

    // The current colour; the palette opens over the dock.
    fn dock_color(&mut self, ui: &mut Ui, x: &mut f32, y: f32, hits: &mut Vec<(&'static str, Rect)>) {
        let c = Rect::from_min_size(pos2(*x, y), vec2(DOCK_BTN, DOCK_BTN));
        let resp = ui.interact(c, Id::unique("dock-color"), Sense::click());
        let p = ui.painter();
        if self.palette_open {
            p.rect(c, CornerRadius::same(8), th().accent_bg, Stroke::new(1.0, th().ring), StrokeKind::Inside);
        } else if resp.hovered() {
            p.rect_filled(c, CornerRadius::same(8), th().hover);
        }
        let sw = Rect::from_center_size(c.center(), vec2(22.0, 22.0));
        p.rect(sw, CornerRadius::same(6), self.color(self.slot), Stroke::new(1.0, th().line2), StrokeKind::Outside);
        hits.push(("dock-color", c));
        if resp.clicked() {
            self.palette_open = !self.palette_open;
        }
        *x += DOCK_BTN + 2.0;
    }

    fn dock_history(&mut self, ui: &mut Ui, x: &mut f32, y: f32, hits: &mut Vec<(&'static str, Rect)>) {
        for (id, ic, enabled, action) in
            [("undo", icon::UNDO, self.can_undo, Action::Undo), ("redo", icon::REDO, self.can_redo, Action::Redo)]
        {
            let r = dock_button(ui, pos2(*x, y), ic, id, false, enabled);
            hits.push((id, r.rect));
            if r.clicked() {
                self.actions.push(action);
            }
            *x += DOCK_BTN + 2.0;
        }
    }

    fn dock_brushes(&mut self, ui: &mut Ui, x: &mut f32, y: f32, hits: &mut Vec<(&'static str, Rect)>) {
        let shape = self.shape;
        for (s, ic, id) in [
            (Shape::Voxel, icon::POINTER, "voxel"),
            (Shape::Box, icon::SQUARE_DASHED, "brush-box"),
            (Shape::Face, icon::LAYERS, "brush-face"),
            (Shape::Line, icon::SLASH, "brush-line"),
        ] {
            let r = dock_button(ui, pos2(*x, y), ic, id, shape == s, true);
            hits.push((id, r.rect));
            if r.clicked() {
                if s == Shape::Voxel {
                    self.voxel_brush();
                } else {
                    self.shape = s;
                }
            }
            *x += DOCK_BTN + 2.0;
        }
        let center = matches!(shape, Shape::Circle | Shape::Sphere);
        let ic = if shape == Shape::Sphere { icon::GLOBE } else { icon::CIRCLE_DOT };
        let r = dock_button(ui, pos2(*x, y), ic, "center", center, true);
        hits.push(("center", r.rect));
        if r.clicked() {
            self.center_brush();
        }
        *x += DOCK_BTN + 2.0;
    }

    fn dock_axes(&mut self, ui: &mut Ui, x: &mut f32, y: f32, hits: &mut Vec<(&'static str, Rect)>) {
        let r = dock_button(ui, pos2(*x, y), icon::FLIP, "mirror", self.mirror, true);
        hits.push(("mirror", r.rect));
        if r.clicked() {
            self.mirror = !self.mirror;
        }
        *x += DOCK_BTN + 2.0;
        let r = dock_button(ui, pos2(*x, y), icon::LOCK, "lock", self.lock.is_some(), true);
        hits.push(("lock", r.rect));
        if r.clicked() {
            self.cycle_lock();
        }
        *x += DOCK_BTN + 2.0;
    }

    // The touch gestures on a scrim chip over the view (they stay on, so they must read over
    // any model): bottom left beside the dock, or centred over it when the screen is too
    // narrow. Under every panel, and never takes a finger.
    fn gesture_hints(&mut self, ctx: &Context, screen: Rect) {
        // The assistant's sheet covers the view the chip hints at.
        if self.sheet() {
            return;
        }
        let mut job = LayoutJob::default();
        let wide: [&[(&str, &str)]; 2] =
            [&[("Two fingers", "orbit"), ("Pinch", "zoom")], &[("Three fingers", "pan"), ("Two-finger tap", "undo")]];
        let upright: [&[(&str, &str)]; 3] = [
            &[("Two fingers", "orbit"), ("Pinch", "zoom")],
            &[("Three fingers", "pan")],
            &[("Two-finger tap", "undo")],
        ];
        // One line less wide where the wide chip does not fit the view.
        let lines: &[&[(&str, &str)]] = if screen.width() < 360.0 { &upright } else { &wide };
        for (l, line) in lines.iter().enumerate() {
            for (i, (k, v)) in line.iter().enumerate() {
                let lead = if i > 0 { 12.0 } else { 0.0 };
                let k = if l > 0 && i == 0 { format!("\n{k}") } else { k.to_string() };
                let f = |font_id, color| TextFormat { font_id, color, line_height: Some(18.0), ..Default::default() };
                job.append(&k, lead, f(sans_medium(12.0), th().text));
                job.append(v, 5.0, f(sans(12.0), th().muted));
            }
        }
        let g = ctx.fonts_mut(|f| f.layout_job(job));
        let dock = self.hits.get("dock").copied().unwrap_or(Rect::NOTHING);
        let pad = vec2(10.0, 6.0);
        let size = g.size() + 2.0 * pad;
        let at = if screen.left() + 12.0 + size.x + 8.0 <= dock.left() {
            pos2(screen.left() + 12.0, screen.bottom() - 12.0 - size.y)
        } else {
            pos2(screen.center().x - size.x / 2.0, self.dock_top - 8.0 - size.y)
        };
        let chip = Rect::from_min_size(at, size);
        self.hits.insert("gestures".into(), chip);
        Area::new(Id::unique("gestures"))
            .fade_in(false)
            .order(Order::Background)
            .interactable(false)
            .fixed_pos(at)
            .constrain(false)
            .show(ctx, |ui| {
                let p = ui.painter();
                p.rect_filled(chip, CornerRadius::same(10), th().scrim(150));
                p.galley(at + pad, g, th().muted);
            });
    }

    // The scene tree (Diorama's .tree, kept for Hybrid): beside the rail once a second object
    // exists. A row per object and group in tree order, indented under its groups, with the
    // model's size and an eye. A click picks the object in world mode or edits its model;
    // in world mode the eye hides and shows, and a double-click renames.
    fn tree_panel(&mut self, ctx: &Context) {
        if self.tree.is_empty() {
            self.renaming = None;
            return;
        }
        let row_h = 28.0;
        let rows = self.tree.len().min(TREE_ROWS);
        self.tree_scroll = self.tree_scroll.min(self.tree.len() - rows);
        let world = self.world;
        // The LAYERS list sits under the rows in world mode: a rule, its header and a row each.
        let layers: &[LayerRow] = if world { &self.layers } else { &[] };
        let layers_h = if world { 1.0 + 30.0 + layers.len() as f32 * row_h } else { 0.0 };
        let rect = Rect::from_min_size(
            pos2(if self.touch { 12.0 } else { 12.0 + RAIL_W + 8.0 }, TOP),
            vec2(TREE_W, 36.0 + rows as f32 * row_h + layers_h + 6.0),
        );
        let objects = self.tree.iter().filter(|r| !r.group).count();
        let (on, scroll, layer_on) = (self.tree_on, self.tree_scroll, self.layer_on);
        let mut renaming = self.renaming.take();
        let mut focus = std::mem::take(&mut self.rename_focus);
        let mut run = Vec::new();
        let mut hits = Vec::new();
        let mut renamed = None;
        let mut start_rename = None;
        let mut wheel = 0.0;
        Self::panel(ctx, "tree", rect, Order::Middle, |ui, rect| {
            let header = |ui: &egui::Ui, y: f32, title: &str| {
                let g = ui.painter().layout_job(spaced(title, sans(9.0), th().faint, 0.72));
                ui.painter().galley(pos2(rect.left() + 12.0, y - g.size().y / 2.0), g, th().faint);
            };
            let hy = rect.top() + 18.0;
            header(ui, hy, "SCENE");
            ui.painter().text(
                pos2(rect.right() - 12.0, hy),
                Align2::RIGHT_CENTER,
                format!("{objects} objects"),
                sans(11.0),
                th().faint,
            );
            let body = Rect::from_min_size(
                pos2(rect.left() + 6.0, rect.top() + 34.0),
                vec2(rect.width() - 12.0, rows as f32 * row_h),
            );
            if ui.rect_contains_pointer(body) {
                wheel = ui.input(|i| i.smooth_scroll_delta.y);
            }
            for (k, row) in self.tree.iter().skip(scroll).take(rows).enumerate() {
                let r =
                    Rect::from_min_size(pos2(body.left(), body.top() + k as f32 * row_h), vec2(body.width(), row_h));
                let id = format!("tree-{}", row.node);
                let resp = ui.interact(r, Id::unique(&id), Sense::click());
                hits.push((id, r));
                let eye = Rect::from_center_size(pos2(r.right() - 15.0, r.center().y), vec2(22.0, 22.0));
                let eye_resp = world.then(|| ui.interact(eye, Id::unique(("tree-eye", row.node)), Sense::click()));
                if world {
                    hits.push((format!("tree-eye-{}", row.node), eye));
                }
                let hover = resp.hovered() || eye_resp.as_ref().is_some_and(|e| e.hovered());
                let is_on = on == Some(row.node);
                let p = ui.painter();
                if is_on {
                    p.rect_filled(r, CornerRadius::same(7), th().accent_bg);
                } else if hover {
                    p.rect_filled(r, CornerRadius::same(7), th().hover);
                }
                let color = match (is_on, row.shown) {
                    (true, _) => th().text,
                    (false, true) if hover => th().text,
                    (false, true) => th().muted,
                    (false, false) => th().faint,
                };
                let x = r.left() + 8.0 + row.depth as f32 * 14.0;
                let glyph = match (row.group, row.linked > 1) {
                    (true, _) => icon::FOLDER,
                    (false, true) => icon::LINK,
                    (false, false) => icon::BOX,
                };
                p.text(pos2(x + 7.0, r.center().y), Align2::CENTER_CENTER, glyph, icon_font(13.0), color);
                let mut right = eye.left() - 4.0;
                if !row.group {
                    let s = row.size;
                    let g = p.layout_no_wrap(format!("{}×{}×{}", s[0], s[1], s[2]), mono(10.0), th().faint);
                    right -= g.size().x;
                    p.galley(pos2(right, r.center().y - g.size().y / 2.0), g, th().faint);
                    right -= 8.0;
                }
                let name_r = Rect::from_min_max(pos2(x + 20.0, r.top() + 4.0), pos2(right, r.bottom() - 4.0));
                let named = Named::Node(row.node);
                match renaming.as_mut() {
                    Some((n, text)) if *n == named => {
                        hits.push(("tree-rename".to_string(), name_r));
                        if Self::rename_field(ui, name_r, text, &mut focus) {
                            renamed = Some((named, text.clone()));
                            renaming = None;
                        }
                    }
                    _ => Self::row_name(ui, name_r, r, &row.name, color),
                }
                if let Some(e) = &eye_resp {
                    Self::eye(ui, eye, e, row.hidden, row.hidden || hover);
                }
                if eye_resp.as_ref().is_some_and(|e| e.clicked()) {
                    run.push(Action::SetHidden(row.node, !row.hidden));
                } else if resp.double_clicked() {
                    start_rename = Some(named);
                } else if resp.clicked() {
                    run.push(Action::PickNode(row.node));
                }
            }
            if !world {
                return;
            }
            // LAYERS: a click puts the picked object or group on the layer (its row shows a
            // dot), the eye hides and shows the layer, a double-click renames, + adds one.
            let top = body.bottom() + 6.0;
            ui.painter().hline(rect.left() + 12.0..=rect.right() - 12.0, top, Stroke::new(1.0, th().line));
            let hy = top + 16.0;
            header(ui, hy, "LAYERS");
            let add = Rect::from_center_size(pos2(rect.right() - 21.0, hy), vec2(22.0, 22.0));
            let add_resp = ui.interact(add, Id::unique("layer-add"), Sense::click());
            hits.push(("layer-add".to_string(), add));
            if add_resp.hovered() {
                ui.painter().rect_filled(add, CornerRadius::same(6), th().hover);
            }
            let c = if add_resp.hovered() { th().text } else { th().faint };
            ui.painter().text(add.center(), Align2::CENTER_CENTER, icon::PLUS, icon_font(13.0), c);
            if add_resp.clicked() {
                run.push(Action::AddLayer);
            }
            for (k, l) in layers.iter().enumerate() {
                let r =
                    Rect::from_min_size(pos2(body.left(), top + 31.0 + k as f32 * row_h), vec2(body.width(), row_h));
                let id = format!("layer-{}", l.id);
                let resp = ui.interact(r, Id::unique(&id), Sense::click());
                hits.push((id, r));
                let eye = Rect::from_center_size(pos2(r.right() - 15.0, r.center().y), vec2(22.0, 22.0));
                let eye_resp = ui.interact(eye, Id::unique(("layer-eye", l.id)), Sense::click());
                hits.push((format!("layer-eye-{}", l.id), eye));
                let hover = resp.hovered() || eye_resp.hovered();
                let p = ui.painter();
                if hover {
                    p.rect_filled(r, CornerRadius::same(7), th().hover);
                }
                let color = match (l.hidden, hover) {
                    (true, _) => th().faint,
                    (false, true) => th().text,
                    (false, false) => th().muted,
                };
                let x = r.left() + 8.0;
                p.text(pos2(x + 7.0, r.center().y), Align2::CENTER_CENTER, icon::LAYERS, icon_font(13.0), color);
                if layer_on == Some(l.id) {
                    p.circle_filled(pos2(r.left() + 3.0, r.center().y), 2.5, th().accent);
                }
                let g = p.layout_no_wrap(l.objects.to_string(), mono(10.0), th().faint);
                let right = eye.left() - 4.0 - g.size().x;
                p.galley(pos2(right, r.center().y - g.size().y / 2.0), g, th().faint);
                let name_r = Rect::from_min_max(pos2(x + 20.0, r.top() + 4.0), pos2(right - 8.0, r.bottom() - 4.0));
                let named = Named::Layer(l.id);
                match renaming.as_mut() {
                    Some((n, text)) if *n == named => {
                        hits.push(("tree-rename".to_string(), name_r));
                        if Self::rename_field(ui, name_r, text, &mut focus) {
                            renamed = Some((named, text.clone()));
                            renaming = None;
                        }
                    }
                    _ => Self::row_name(ui, name_r, r, &l.name, color),
                }
                Self::eye(ui, eye, &eye_resp, l.hidden, l.hidden || hover);
                if eye_resp.clicked() {
                    run.push(Action::SetLayerHidden(l.id, !l.hidden));
                } else if resp.double_clicked() {
                    start_rename = Some(named);
                } else if resp.clicked() {
                    run.push(Action::SetLayer(l.id));
                }
            }
        });
        if wheel != 0.0 {
            let steps = (wheel / row_h).round() as isize;
            let steps = if steps == 0 { -(wheel.signum() as isize) } else { -steps };
            self.tree_scroll = self.tree_scroll.saturating_add_signed(steps).min(self.tree.len() - rows);
        }
        self.renaming = renaming;
        self.rename_focus = focus;
        self.renamed = renamed.or(self.renamed.take());
        self.hits.extend(hits);
        self.actions.extend(run);
        match start_rename {
            Some(Named::Node(n)) => self.start_rename(Some(n)),
            Some(Named::Layer(id)) => self.start_layer_rename(Some(id)),
            None => {}
        }
    }

    /// A tree or layer row's name, cut with an ellipsis to fit `name_r`.
    fn row_name(ui: &egui::Ui, name_r: Rect, r: Rect, name: &str, color: Color32) {
        let mut job = LayoutJob::single_section(
            name.to_string(),
            TextFormat { font_id: sans(12.0), color, ..Default::default() },
        );
        job.wrap = egui::text::TextWrapping {
            max_width: name_r.width().max(1.0),
            max_rows: 1,
            break_anywhere: true,
            overflow_character: Some('…'),
        };
        let g = ui.painter().layout_job(job);
        ui.painter().galley(pos2(name_r.left(), r.center().y - g.size().y / 2.0), g, color);
    }

    /// A row's eye button, drawn when `show` (hovered or hidden).
    fn eye(ui: &egui::Ui, eye: Rect, resp: &egui::Response, hidden: bool, show: bool) {
        if !show {
            return;
        }
        if resp.hovered() {
            ui.painter().rect_filled(eye, CornerRadius::same(6), th().hover);
        }
        let glyph = if hidden { icon::EYE_OFF } else { icon::EYE };
        let c = if resp.hovered() { th().text } else { th().faint };
        ui.painter().text(eye.center(), Align2::CENTER_CENTER, glyph, icon_font(13.0), c);
    }

    /// The in-place rename field over a row's name; true once it loses focus (done).
    fn rename_field(ui: &mut egui::Ui, name_r: Rect, text: &mut String, focus: &mut bool) -> bool {
        ui.painter().rect(
            name_r.expand2(vec2(4.0, 0.0)),
            CornerRadius::same(6),
            th().tint(9),
            Stroke::new(1.0, th().ring),
            StrokeKind::Inside,
        );
        let edit = egui::TextEdit::singleline(text)
            .id(Id::unique("tree-rename"))
            .font(sans(12.0))
            .text_color(th().text)
            .frame(egui::Frame::NONE)
            .margin(egui::Margin::ZERO)
            .desired_width(name_r.width());
        let line = ui.fonts_mut(|f| f.row_height(&sans(12.0)));
        let text_r = Rect::from_center_size(name_r.center(), vec2(name_r.width(), line));
        let out = ui.put(text_r, edit);
        if std::mem::take(focus) {
            out.request_focus();
            false
        } else {
            out.lost_focus()
        }
    }

    // The Export menu under the top bar's Export button: one row per export, each with what
    // it writes. A row runs its export and closes the menu; so do Escape and a click outside.
    fn export_menu(&mut self, ctx: &Context) {
        if !self.export_menu {
            return;
        }
        let rows = [
            ("export-render", icon::IMAGE_DOWN, "Render PNG", "R"),
            ("export-obj", icon::BOX, "OBJ with palette texture", ".obj .mtl .png"),
            ("export-slices", icon::LAYERS, "PNG slices", ".png"),
        ];
        let rect = Rect::from_min_size(
            pos2(self.export_x, 12.0 + 44.0 + 6.0),
            vec2(340.0, 36.0 + 34.0 * rows.len() as f32 + 6.0),
        );
        let mut run = None;
        let mut hits = Vec::new();
        Self::panel(ctx, "export-menu", rect, Order::Foreground, |ui, rect| {
            let hy = rect.top() + 18.0;
            let g = ui.painter().layout_job(spaced("EXPORT", sans(9.0), th().faint, 0.72));
            ui.painter().galley(pos2(rect.left() + 12.0, hy - g.size().y / 2.0), g, th().faint);
            for (k, (id, glyph, label, hint)) in rows.into_iter().enumerate() {
                let r = Rect::from_min_size(
                    pos2(rect.left() + 6.0, rect.top() + 36.0 + 34.0 * k as f32),
                    vec2(rect.width() - 12.0, 32.0),
                );
                let resp = ui.interact(r, Id::unique(id), Sense::click());
                hits.push((id.to_string(), r));
                let p = ui.painter();
                let color = if resp.hovered() {
                    p.rect_filled(r, CornerRadius::same(7), th().hover);
                    th().text
                } else {
                    th().muted
                };
                let cy = r.center().y;
                p.text(pos2(r.left() + 16.0, cy), Align2::CENTER_CENTER, glyph, icon_font(15.0), color);
                p.text(pos2(r.left() + 32.0, cy), Align2::LEFT_CENTER, label, sans(13.0), th().text);
                if hint.len() == 1 {
                    kbd(ui, pos2(r.right() - 8.0, cy), hint, true);
                } else {
                    ui.painter().text(pos2(r.right() - 8.0, cy), Align2::RIGHT_CENTER, hint, mono(10.5), th().faint);
                }
                if resp.clicked() {
                    run = Some(k);
                }
            }
        });
        self.hits.extend(hits);
        let button = self.hits.get("export").copied().unwrap_or(Rect::NOTHING);
        let outside = ctx.input(|i| {
            i.pointer.any_pressed()
                && i.pointer.interact_pos().is_some_and(|p| !rect.contains(p) && !button.contains(p))
        });
        match run {
            Some(0) => {
                if !self.render_panel {
                    self.toggle_render_panel();
                }
            }
            Some(1) => self.actions.push(Action::ExportObj),
            Some(_) => self.actions.push(Action::ExportSlices),
            None => {}
        }
        if run.is_some() || outside {
            self.export_menu = false;
        }
    }

    // The View flyout of the camera bar: the projection (perspective, orthographic,
    // isometric) and the six side views with their number keys. It opens below the bar (over
    // the render corner) if it fits on screen, else above the bar if it clears the palette,
    // else left of the bar and the palette, always on screen. The view the camera is at
    // shows on.
    fn view_panel(&mut self, ctx: &Context, screen: Rect) {
        if !self.view_panel {
            return;
        }
        let size = vec2(204.0, 176.0);
        let palette_bottom = self.hits.get("palette").map_or(screen.top(), |r| r.bottom());
        let floor = screen.bottom() - 12.0;
        let nav = self.hits.get("nav").copied().unwrap_or(Rect::from_min_size(screen.right_bottom(), egui::Vec2::ZERO));
        let right = nav.right().min(screen.right() - 12.0);
        let below = Rect::from_min_size(pos2(right - size.x, nav.bottom() + 8.0), size);
        let above = Rect::from_min_size(pos2(right - size.x, nav.top() - 8.0 - size.y), size);
        let rect = if self.touch {
            // Left of the standing bar, level with its top.
            Rect::from_min_size(pos2(nav.left() - 8.0 - size.x, nav.top().min(floor - size.y)), size)
        } else if below.bottom() <= floor {
            below
        } else if above.top() >= palette_bottom + 8.0 {
            above
        } else {
            // A short window: left of the bar, bottom-aligned with it, kept on screen and left of
            // the palette when it would reach up beside it.
            let top = (nav.bottom() - size.y).clamp(TOP, (screen.bottom() - 12.0 - size.y).max(TOP));
            let palette = self.hits.get("palette").copied().filter(|p| p.bottom() + 8.0 > top);
            let right = palette.map_or(nav.left(), |p| nav.left().min(p.left()));
            Rect::from_min_size(pos2((right - 8.0 - size.x).max(screen.left() + 12.0), top), size)
        };
        self.view_flyout(ctx, rect);
    }

    fn view_flyout(&mut self, ctx: &Context, rect: Rect) {
        self.hits.insert("view-flyout".into(), rect);
        let (ortho, view) = (self.ortho, self.view);
        let mut guides = [self.grid, self.ground, self.frame];
        let mut run = Vec::new();
        let mut hits = Vec::new();
        let mut close = false;
        Self::panel_filled(ctx, "view", rect, Order::Foreground, th().panel_solid, |ui, rect| {
            // It can sit over the render corner (above it in layer order): its blank space takes the
            // click, not the corner.
            ui.allocate_rect(rect, Sense::click());
            let hy = rect.top() + 16.0;
            let g = ui.painter().layout_job(spaced("VIEW", sans(9.0), th().faint, 0.72));
            ui.painter().galley(pos2(rect.left() + 12.0, hy - g.size().y / 2.0), g, th().faint);
            let x = Rect::from_center_size(pos2(rect.right() - 19.0, hy), vec2(22.0, 22.0));
            let resp = ui.interact(x, Id::unique("view-close"), Sense::click());
            hits.push(("view-close".to_string(), x));
            if resp.hovered() {
                ui.painter().rect_filled(x, CornerRadius::same(6), th().hover);
            }
            let c = if resp.hovered() { th().text } else { th().muted };
            ui.painter().text(x.center(), Align2::CENTER_CENTER, icon::X, icon_font(13.0), c);
            close = resp.clicked();
            ui.painter().text(
                pos2(x.left() - 4.0, hy),
                Align2::RIGHT_CENTER,
                view.map_or("Free", |v| v.name()),
                sans(11.0),
                th().faint,
            );
            let iso = view == Some(View::Iso);
            let sel = if iso { 2 } else { ortho as usize };
            let row = Rect::from_min_size(pos2(rect.left() + 12.0, hy + 14.0), vec2(rect.width() - 24.0, 24.0));
            let mut seg = Vec::new();
            let labels = ["Persp", "Ortho", "Iso"].map(String::from);
            let names = ["view-persp", "view-ortho", "view-iso"];
            match segmented(ui, row, &names, &labels, sel, &[true; 3], sans(11.0), &mut seg) {
                Some(0) => run.push(Action::Ortho(false)),
                Some(1) if iso || !ortho => run.push(Action::Ortho(true)),
                Some(2) => run.push(Action::Snap(View::Iso)),
                _ => {}
            }
            hits.extend(seg.into_iter().map(|(n, r)| (n.to_string(), r)));
            ui.painter().line_segment(
                [pos2(rect.left() + 10.0, hy + 47.5), pos2(rect.right() - 10.0, hy + 47.5)],
                Stroke::new(1.0, th().line),
            );
            let cw = (rect.width() - 24.0 - 8.0) / 3.0;
            let cell = |k: usize, cy: f32| {
                Rect::from_min_size(pos2(rect.left() + 12.0 + (k % 3) as f32 * (cw + 4.0), cy - 12.0), vec2(cw, 24.0))
            };
            for (k, v) in View::ALL[..6].iter().copied().enumerate() {
                let c = cell(k, hy + 68.0 + (k / 3) as f32 * 28.0);
                let id = format!("view-{}", v.name().to_lowercase());
                if view_chip(ui, c, &id, v.name(), &(k + 1).to_string(), view == Some(v)) {
                    run.push(Action::Snap(v));
                }
                hits.push((id, c));
            }
            ui.painter().line_segment(
                [pos2(rect.left() + 10.0, hy + 117.5), pos2(rect.right() - 10.0, hy + 117.5)],
                Stroke::new(1.0, th().line),
            );
            for (k, (id, label, key)) in
                [("view-grid", "Grid", "G"), ("view-ground", "Ground", ""), ("view-bounds", "Bounds", "")]
                    .into_iter()
                    .enumerate()
            {
                let c = cell(k, hy + 138.0);
                if view_chip(ui, c, id, label, key, guides[k]) {
                    guides[k] = !guides[k];
                }
                hits.push((id.to_string(), c));
            }
        });
        [self.grid, self.ground, self.frame] = guides;
        self.hits.extend(hits);
        self.actions.extend(run);
        if close {
            self.view_panel = false;
        }
    }

    // The size flyout beside the rail's voxel brush button: SIZE, minus, the size, plus.
    // Built like the axis flyout.
    fn size_flyout(&mut self, ctx: &Context) {
        if !self.size_shown() {
            return;
        }
        let rect = self.flyout_rect(self.voxel_y, "voxel", vec2(150.0, 6.0 + 40.0 + 6.0));
        let size = self.size;
        let mut step = 0;
        let mut hits = Vec::new();
        Self::panel(ctx, "size", rect, Order::Middle, |ui, rect| {
            let cy = rect.center().y;
            let g = ui.painter().layout_job(spaced("SIZE", sans(9.0), th().faint, 0.72));
            ui.painter().galley(pos2(rect.left() + 12.0, cy - g.size().y / 2.0), g, th().faint);
            for (k, (id, glyph, d)) in
                [("size-minus", icon::MINUS, -1), ("size-plus", icon::PLUS, 1)].into_iter().enumerate()
            {
                let c = Rect::from_min_size(pos2(rect.left() + 58.0 + k as f32 * 56.0, cy - 13.0), vec2(26.0, 26.0));
                let off = (d < 0 && size <= 1) || (d > 0 && size >= MAX_SIZE);
                let resp = ui.interact(c, Id::unique(id), if off { Sense::hover() } else { Sense::click() });
                hits.push((id.to_string(), c));
                let hover = resp.hovered() && !off;
                let border = if hover { th().tint(64) } else { th().line2 };
                let p = ui.painter();
                p.rect_stroke(c, CornerRadius::same(7), Stroke::new(1.0, border), StrokeKind::Inside);
                let color = if off {
                    th().faint
                } else if hover {
                    th().text
                } else {
                    th().muted
                };
                p.text(c.center(), Align2::CENTER_CENTER, glyph, icon_font(13.0), color);
                if resp.clicked() {
                    step = d;
                }
            }
            ui.painter().text(
                pos2(rect.left() + 58.0 + 26.0 + 15.0, cy),
                Align2::CENTER_CENTER,
                size.to_string(),
                mono_medium(12.0),
                th().text,
            );
        });
        self.hits.extend(hits);
        self.step_size(step);
    }

    // The axis flyout beside the rail's mirror and lock buttons, shown while either is on:
    // MIRROR X Y Z (any of them) and LOCK X Y Z (one of them).
    fn axes(&mut self, ctx: &Context) {
        if !self.mirror && self.lock.is_none() {
            return;
        }
        let rect = self.flyout_rect(self.axes_y, "mirror", vec2(150.0, 6.0 + 2.0 * 40.0 + 2.0 + 6.0));
        let (mirror, lock) = (self.mirror_axes(), self.lock);
        let mut clicked = (None, None);
        let mut hits = Vec::new();
        Self::panel(ctx, "axes", rect, Order::Middle, |ui, rect| {
            for (row, label) in ["MIRROR", "LOCK"].into_iter().enumerate() {
                let cy = rect.top() + 6.0 + 20.0 + row as f32 * 42.0;
                let g = ui.painter().layout_job(spaced(label, sans(9.0), th().faint, 0.72));
                ui.painter().galley(pos2(rect.left() + 12.0, cy - g.size().y / 2.0), g, th().faint);
                for (a, name) in ["X", "Y", "Z"].into_iter().enumerate() {
                    let on = if row == 0 { mirror[a] } else { lock == Some(a) };
                    let c =
                        Rect::from_min_size(pos2(rect.left() + 58.0 + a as f32 * 28.0, cy - 13.0), vec2(26.0, 26.0));
                    let id = format!("{}-{}", if row == 0 { "mirror" } else { "lock" }, name.to_lowercase());
                    let resp = ui.interact(c, Id::unique(&id), Sense::click());
                    hits.push((id, c));
                    let p = ui.painter();
                    let color = if on {
                        p.rect(
                            c,
                            CornerRadius::same(7),
                            th().accent_bg,
                            Stroke::new(1.0, th().ring),
                            StrokeKind::Inside,
                        );
                        th().accent
                    } else {
                        let hover = resp.hovered();
                        let border = if hover { th().tint(64) } else { th().line2 };
                        p.rect_stroke(c, CornerRadius::same(7), Stroke::new(1.0, border), StrokeKind::Inside);
                        if hover { th().text } else { th().muted }
                    };
                    p.text(c.center(), Align2::CENTER_CENTER, name, mono_medium(11.5), color);
                    if resp.clicked() {
                        if row == 0 {
                            clicked.0 = Some(a);
                        } else {
                            clicked.1 = Some(a);
                        }
                    }
                }
            }
        });
        self.hits.extend(hits);
        if let Some(a) = clicked.0 {
            self.toggle_mirror_axis(a);
        }
        if let Some(a) = clicked.1 {
            self.toggle_lock(a);
        }
    }

    /// Choose what a select click takes, and switch to the select tool.
    pub fn set_region(&mut self, region: Region) {
        self.region = region;
        self.tool = Tool::Select;
    }

    // The selection flyout beside the rail's select button, shown with the select tool or a
    // selection: the count, ROTATE / FLIP / LOOP X Y Z and hollow and fill inside (the
    // whole model when nothing is selected), crop and resize of the model bounds, select all
    // and invert, what a click selects (colour, face or volume), and fill, copy, cut, paste
    // and delete. Built like the axis flyout.
    fn selection_panel(&mut self, ctx: &Context) {
        if self.world || (self.tool != Tool::Select && self.selected == 0) {
            return;
        }
        let rect = self.flyout_rect(self.select_y, "select", vec2(162.0, 310.0));
        self.hits.insert("selection".into(), rect);
        let (selected, has_clip) = (self.selected, self.has_clip);
        let mut region = self.region;
        let bounds_open = self.bounds_panel.is_some();
        let mut toggle_bounds = false;
        let mut run = Vec::new();
        let mut hits = Vec::new();
        Self::panel(ctx, "selection", rect, Order::Middle, |ui, rect| {
            let hy = rect.top() + 18.0;
            let g = ui.painter().layout_job(spaced("SELECTION", sans(9.0), th().faint, 0.72));
            ui.painter().galley(pos2(rect.left() + 12.0, hy - g.size().y / 2.0), g, th().faint);
            if selected > 0 {
                let x = Rect::from_center_size(pos2(rect.right() - 19.0, hy), vec2(22.0, 22.0));
                let resp = ui.interact(x, Id::unique("deselect"), Sense::click());
                hits.push(("deselect".to_string(), x));
                if resp.hovered() {
                    ui.painter().rect_filled(x, CornerRadius::same(6), th().hover);
                }
                let c = if resp.hovered() { th().text } else { th().muted };
                ui.painter().text(x.center(), Align2::CENTER_CENTER, icon::X, icon_font(13.0), c);
                ui.painter().text(
                    pos2(x.left() - 4.0, hy),
                    Align2::RIGHT_CENTER,
                    group_thousands(selected as u64),
                    mono(11.0),
                    th().accent,
                );
                if resp.clicked() {
                    run.push(Action::Deselect);
                }
            } else {
                ui.painter().text(
                    pos2(rect.right() - 12.0, hy),
                    Align2::RIGHT_CENTER,
                    "whole model",
                    sans(11.0),
                    th().faint,
                );
            }
            for (row, label) in ["ROTATE", "FLIP", "LOOP"].into_iter().enumerate() {
                let cy = hy + 30.0 + row as f32 * 34.0;
                let g = ui.painter().layout_job(spaced(label, sans(9.0), th().faint, 0.72));
                ui.painter().galley(pos2(rect.left() + 12.0, cy - g.size().y / 2.0), g, th().faint);
                for (a, name) in ["X", "Y", "Z"].into_iter().enumerate() {
                    let c =
                        Rect::from_min_size(pos2(rect.left() + 70.0 + a as f32 * 28.0, cy - 13.0), vec2(26.0, 26.0));
                    let id = format!("{}-{}", label.to_lowercase(), name.to_lowercase());
                    let resp = ui.interact(c, Id::unique(&id), Sense::click());
                    hits.push((id, c));
                    let hover = resp.hovered();
                    let p = ui.painter();
                    let border = if hover { th().tint(64) } else { th().line2 };
                    p.rect_stroke(c, CornerRadius::same(7), Stroke::new(1.0, border), StrokeKind::Inside);
                    p.text(
                        c.center(),
                        Align2::CENTER_CENTER,
                        name,
                        mono_medium(11.5),
                        if hover { th().text } else { th().muted },
                    );
                    if resp.clicked() {
                        run.push(Action::Transform(match row {
                            0 => Transform::Rotate(a),
                            1 => Transform::Flip(a),
                            _ => Transform::Loop(a, 1),
                        }));
                    }
                }
            }
            // Text buttons, two per row: hollow and fill inside, crop and resize (which opens
            // the Bounds panel, and shows on while it is open), then select all and invert.
            for (k, (label, id, action)) in [
                ("Hollow", "hollow", Some(Action::Hollow)),
                ("Fill inside", "fill-in", Some(Action::FillInterior)),
                ("Crop", "crop", Some(Action::Crop)),
                ("Resize…", "resize", None),
                ("All", "select-all", Some(Action::SelectAll)),
                ("Invert", "invert", Some(Action::InvertSelection)),
            ]
            .into_iter()
            .enumerate()
            {
                let cy = hy + 30.0 + (3 + k / 2) as f32 * 34.0;
                let c =
                    Rect::from_min_size(pos2(rect.left() + 12.0 + (k % 2) as f32 * 70.0, cy - 13.0), vec2(68.0, 26.0));
                let resp = ui.interact(c, Id::unique(id), Sense::click());
                hits.push((id.to_string(), c));
                let (hover, on) = (resp.hovered(), action.is_none() && bounds_open);
                let p = ui.painter();
                if on {
                    p.rect_filled(c, CornerRadius::same(7), th().accent_tint);
                }
                let border = if on {
                    th().ring
                } else if hover {
                    th().tint(64)
                } else {
                    th().line2
                };
                p.rect_stroke(c, CornerRadius::same(7), Stroke::new(1.0, border), StrokeKind::Inside);
                let color = if hover || on { th().text } else { th().muted };
                p.text(c.center(), Align2::CENTER_CENTER, label, sans(11.0), color);
                if resp.clicked() {
                    match action {
                        Some(a) => run.push(a),
                        None => toggle_bounds = true,
                    }
                }
            }
            // What a click on a voxel selects.
            let row = Rect::from_min_size(pos2(rect.left() + 12.0, hy + 30.0 + 6.0 * 34.0 - 13.0), vec2(138.0, 26.0));
            let mut seg = Vec::new();
            let labels = ["Color", "Face", "Volume"].map(String::from);
            let names = ["click-color", "click-face", "click-volume"];
            let regions = [Region::Color, Region::Face, Region::Volume];
            let on = regions.iter().position(|&r| r == region).unwrap_or(0);
            if let Some(k) = segmented(ui, row, &names, &labels, on, &[true; 3], sans(10.5), &mut seg) {
                region = regions[k];
            }
            hits.extend(seg.into_iter().map(|(n, r)| (n.to_string(), r)));
            let cy = hy + 30.0 + 7.0 * 34.0 + 4.0;
            ui.painter().line_segment(
                [pos2(rect.left() + 10.0, cy - 20.5), pos2(rect.right() - 10.0, cy - 20.5)],
                Stroke::new(1.0, th().line),
            );
            for (k, (glyph, id, action, on)) in [
                (icon::PAINT_BUCKET, "fill", Action::FillSelection, selected > 0),
                (icon::COPY, "copy-sel", Action::Copy, selected > 0),
                (icon::SCISSORS, "cut", Action::Cut, selected > 0),
                (icon::CLIPBOARD_PASTE, "paste", Action::Paste, has_clip),
                (icon::TRASH, "delete", Action::DeleteSelection, selected > 0),
            ]
            .into_iter()
            .enumerate()
            {
                let c = Rect::from_min_size(pos2(rect.left() + 12.0 + k as f32 * 28.0, cy - 13.0), vec2(26.0, 26.0));
                hits.push((id.to_string(), c));
                let p = ui.painter().clone();
                if !on {
                    p.rect_stroke(c, CornerRadius::same(7), Stroke::new(1.0, th().line), StrokeKind::Inside);
                    p.text(c.center(), Align2::CENTER_CENTER, glyph, icon_font(14.0), th().faint);
                    continue;
                }
                let resp = ui.interact(c, Id::unique(id), Sense::click());
                let hover = resp.hovered();
                let danger = id == "delete";
                let border = if hover { th().tint(64) } else { th().line2 };
                p.rect_stroke(c, CornerRadius::same(7), Stroke::new(1.0, border), StrokeKind::Inside);
                let color = match (hover, danger) {
                    (true, true) => th().danger,
                    (true, false) => th().text,
                    _ => th().muted,
                };
                p.text(c.center(), Align2::CENTER_CENTER, glyph, icon_font(14.0), color);
                if resp.clicked() {
                    run.push(action);
                }
            }
        });
        self.hits.extend(hits);
        self.actions.extend(run);
        self.region = region;
        if toggle_bounds {
            self.toggle_bounds_panel();
        }
    }

    // The New scene confirm, centred over the view: the edits would be lost, so Cancel is the
    // default look and Enter or the accent button starts the blank scene. A dim backdrop under
    // it takes the pointer, so the bars, the palette and the view wait for an answer.
    fn new_confirm(&mut self, ctx: &Context, screen: Rect) {
        if !self.confirm_new {
            return;
        }
        Area::new(Id::unique("new-backdrop"))
            .fade_in(false)
            .order(Order::Foreground)
            .fixed_pos(screen.min)
            .constrain(false)
            .show(ctx, |ui| {
                ui.painter().rect_filled(screen, CornerRadius::ZERO, Color32::from_black_alpha(64));
                ui.allocate_rect(screen, Sense::click_and_drag());
            });
        let (w, h) = (320.0, 138.0);
        let rect = Rect::from_center_size(screen.center(), vec2(w, h));
        let mut hits = Vec::new();
        let (mut cancel, mut discard) = (false, false);
        Self::panel(ctx, "new-confirm", rect, Order::Foreground, |ui, rect| {
            let title = ui.painter().layout_no_wrap("Start a new scene?".into(), sans_semibold(13.0), th().text);
            ui.painter().galley(pos2(rect.left() + 16.0, rect.top() + 16.0), title, th().text);
            let mut job = LayoutJob::default();
            job.append(
                "The edits to this scene are not saved and will be lost.",
                0.0,
                TextFormat { font_id: sans(12.0), color: th().muted, line_height: Some(17.0), ..Default::default() },
            );
            job.wrap.max_width = w - 32.0;
            let note = ui.fonts_mut(|f| f.layout_job(job));
            ui.painter().galley(pos2(rect.left() + 16.0, rect.top() + 40.0), note, th().muted);
            let by = rect.bottom() - 16.0 - 32.0;
            let ink = th().on_accent;
            let label = ui.painter().layout_no_wrap("Start new".into(), sans_semibold(13.0), ink);
            let dw = label.size().x + 28.0;
            let d = Rect::from_min_size(pos2(rect.right() - 16.0 - dw, by), vec2(dw, 32.0));
            let resp = ui.interact(d, Id::unique("new-discard"), Sense::click());
            let fill = if resp.hovered() { th().accent_hover } else { th().accent };
            ui.painter().rect_filled(d, CornerRadius::same(8), fill);
            ui.painter().galley(pos2(d.left() + 14.0, d.center().y - label.size().y / 2.0), label, ink);
            discard = resp.clicked();
            hits.push(("new-discard".to_string(), d));
            let cl = ui.painter().layout_no_wrap("Cancel".into(), sans(13.0), th().muted);
            let cw = cl.size().x + 28.0;
            let c = Rect::from_min_size(pos2(d.left() - 6.0 - cw, by), vec2(cw, 32.0));
            let resp = ui.interact(c, Id::unique("new-cancel"), Sense::click());
            let color = if resp.hovered() { th().text } else { th().muted };
            if resp.hovered() {
                ui.painter().rect_filled(c, CornerRadius::same(8), th().hover);
            }
            ui.painter().galley(pos2(c.left() + 14.0, c.center().y - cl.size().y / 2.0), cl, color);
            cancel = resp.clicked();
            hits.push(("new-cancel".to_string(), c));
        });
        ctx.move_to_top(egui::LayerId::new(Order::Foreground, Id::unique("new-confirm")));
        self.hits.extend(hits);
        if discard {
            self.confirm_new = false;
            self.actions.push(Action::New);
        } else if cancel {
            self.confirm_new = false;
        }
    }

    // The Bounds panel, beside the selection flyout at its Resize button: the model's size as
    // X, Y and Z fields (vxr's axes, Y up). Enter or leaving a field resizes, keeping the
    // bottom and the centre, as one undo step.
    fn bounds_panel(&mut self, ctx: &Context) {
        let Some(mut fields) = self.bounds_panel.take() else { return };
        let size = self.bounds;
        let (w, inner_w) = (232.0, 208.0);
        let mut job = LayoutJob::default();
        job.append(
            "Keeps the bottom and the center. Erases the voxels outside.",
            0.0,
            TextFormat { font_id: sans(11.0), color: th().faint, line_height: Some(15.95), ..Default::default() },
        );
        job.wrap.max_width = inner_w;
        let note = ctx.fonts_mut(|f| f.layout_job(job));
        let h = 36.0 + 28.0 + 12.0 + note.size().y + 14.0;
        // The fields' row lines up with the Resize button (its centre is select_y + 178).
        let rect = match self.hits.get("selection").copied().filter(|_| self.touch) {
            // Over the dock: beside the selection flyout, level with its bottom.
            Some(sel) => Rect::from_min_size(pos2(sel.right() + 8.0, sel.bottom() - h), vec2(w, h)),
            None => {
                let top = self.select_y + 178.0 - 50.0;
                Rect::from_min_size(pos2(self.flyout_x() + 162.0 + 8.0, top), vec2(w, h))
            }
        };
        let mut close = false;
        let mut resize = None;
        let mut hits = Vec::new();
        Self::panel(ctx, "bounds", rect, Order::Middle, |ui, rect| {
            let hy = rect.top() + 18.0;
            let g = ui.painter().layout_job(spaced("BOUNDS", sans(9.0), th().faint, 0.72));
            ui.painter().galley(pos2(rect.left() + 12.0, hy - g.size().y / 2.0), g, th().faint);
            let x = Rect::from_center_size(pos2(rect.right() - 19.0, hy), vec2(22.0, 22.0));
            let resp = ui.interact(x, Id::unique("bounds-close"), Sense::click());
            hits.push(("bounds-close".to_string(), x));
            if resp.hovered() {
                ui.painter().rect_filled(x, CornerRadius::same(6), th().hover);
            }
            let c = if resp.hovered() { th().text } else { th().muted };
            ui.painter().text(x.center(), Align2::CENTER_CENTER, icon::X, icon_font(13.0), c);
            ui.painter().text(
                pos2(x.left() - 4.0, hy),
                Align2::RIGHT_CENTER,
                format!("{}×{}×{}", size[0], size[1], size[2]),
                mono(11.0),
                th().muted,
            );
            close = resp.clicked();
            let third = (inner_w - 12.0) / 3.0;
            for (a, label) in ["X", "Y", "Z"].into_iter().enumerate() {
                let fr = Rect::from_min_size(
                    pos2(rect.left() + 12.0 + a as f32 * (third + 6.0), rect.top() + 36.0),
                    vec2(third, 28.0),
                );
                let name = format!("bounds-{}", label.to_lowercase());
                let id = Id::unique(&name);
                if !ui.memory(|m| m.has_focus(id)) {
                    fields[a] = size[a].to_string();
                }
                hits.push((name, fr));
                if field(ui, fr, label, id, &mut fields[a])
                    && let Ok(v) = fields[a].trim().parse::<u32>()
                    && v != size[a]
                {
                    let mut to = size;
                    to[a] = v;
                    resize = Some(to);
                }
            }
            ui.painter().galley(pos2(rect.left() + 12.0, rect.top() + 36.0 + 28.0 + 12.0), note, th().faint);
        });
        self.hits.extend(hits);
        if let Some(to) = resize {
            self.actions.push(Action::Resize(to));
        }
        if !close {
            self.bounds_panel = Some(fields);
        }
    }

    // The review card of an AI change: not in the mockups' Hybrid set (docs/research/ai.md,
    // section 2, and its review mock). A .panel left of the palette: the request, what the
    // change adds, removes and repaints, the palette slots it sets, the steps with a checkbox
    // each, Before and After, a field to ask for changes, and Discard and Apply. Over 10,000
    // cells it asks before staging more.
    fn review_card(&mut self, ctx: &Context, screen: Rect) {
        let Some(card) = self.review.take() else {
            self.ask_focus = false;
            self.review_ask.clear();
            self.review_scroll = 0;
            return;
        };
        let w = 304.0;
        let inner_w = w - 24.0;
        let x = (screen.right() - 12.0 - PALETTE_W - 8.0 - w).max(12.0 + RAIL_W + 12.0);
        let wrapped = |text: &str, font: FontId, color: Color32, rows: usize| {
            let mut job = LayoutJob::single_section(
                text.to_string(),
                TextFormat { font_id: font, color, line_height: Some(17.0), ..Default::default() },
            );
            job.wrap = egui::text::TextWrapping {
                max_width: inner_w,
                max_rows: rows,
                break_anywhere: false,
                overflow_character: Some('…'),
            };
            ctx.fonts_mut(|f| f.layout_job(job))
        };
        let prompt = wrapped(&card.prompt, sans_semibold(13.0), th().text, 3);
        let summary = card.summary.as_deref().filter(|s| !s.is_empty()).map(|s| wrapped(s, sans(12.0), th().muted, 2));
        let held_text = card.held.map(|(asked, most)| {
            let mut job = LayoutJob::default();
            job.append(
                &format!(
                    "The change would go past {} cells. Let it grow to {}?",
                    group_thousands(asked as u64),
                    group_thousands(most as u64)
                ),
                0.0,
                TextFormat { font_id: sans(12.0), color: th().text, line_height: Some(17.0), ..Default::default() },
            );
            job.wrap.max_width = inner_w - 20.0 - 20.0;
            ctx.fonts_mut(|f| f.layout_job(job))
        });
        let [added, removed, repainted] = card.counts.map(|c| group_thousands(c as u64));
        let mut parts: Vec<(String, Color32)> = Vec::new();
        if card.counts[0] > 0 {
            parts.push((format!("+{added} added"), th().ok));
        }
        if card.counts[1] > 0 {
            parts.push((format!("{removed} removed"), th().danger));
        }
        if card.counts[2] > 0 {
            parts.push((format!("{repainted} repainted"), th().accent));
        }
        match card.slots.len() {
            0 => {}
            1 => parts.push((format!("slot {} changed", card.slots[0].slot), th().text)),
            k => parts.push((format!("{k} slots changed"), th().text)),
        }
        if parts.is_empty() {
            parts.push(("No changes yet".into(), th().muted));
        }
        // Lines break between the counts, never inside one, and no line starts or ends with
        // a separator.
        let fmt = |color| TextFormat { font_id: mono(11.5), color, line_height: Some(17.0), ..Default::default() };
        let width = |t: &str| ctx.fonts_mut(|f| f.layout_no_wrap(t.into(), mono(11.5), th().text)).size().x;
        let (sep, max) = (width(" · "), inner_w - 20.0);
        let mut job = LayoutJob::default();
        let mut line = 0.0;
        for (i, (t, c)) in parts.iter().enumerate() {
            let w = width(t);
            if i > 0 && line + sep + w > max {
                job.append("\n", 0.0, fmt(th().faint));
                line = 0.0;
            } else if i > 0 {
                job.append(" · ", 0.0, fmt(th().faint));
                line += sep;
            }
            job.append(t, 0.0, fmt(*c));
            line += w;
        }
        let counts_text = ctx.fonts_mut(|f| f.layout_job(job));
        let counts_h = counts_text.size().y + 13.0;
        let mut rows = card.steps.len().min(REVIEW_ROWS);
        let (row_h, slot_h) = (26.0, 24.0);
        let mut h = 12.0 + 15.0 + 10.0 + prompt.size().y;
        if let Some(g) = &summary {
            h += 4.0 + g.size().y;
        }
        if card.working.is_some() {
            h += 6.0 + 17.0;
        }
        h += 10.0 + counts_h;
        if !card.slots.is_empty() {
            h += 6.0 + card.slots.len() as f32 * slot_h;
        }
        if let Some(g) = &held_text {
            h += 10.0 + 12.0 + g.size().y + 10.0 + 26.0 + 12.0;
        }
        if rows > 0 {
            h += 14.0 + 12.0 + 6.0 + rows as f32 * row_h;
        }
        h += 12.0 + 28.0 + 8.0 + 32.0 + 12.0;
        if !card.agents {
            h += 30.0 + 12.0;
        }
        // The card ends above the render corner and the camera bar where they share columns:
        // fewer steps show (the list scrolls), down to two.
        let below = [self.render_dock.filter(|_| !self.full_render), self.nav_rect];
        if let Some(top) = below.into_iter().flatten().filter(|c| c.left() < x + w).map(|c| c.top()).reduce(f32::min) {
            while rows > 2 && TOP + h > top - 8.0 {
                rows -= 1;
                h -= row_h;
            }
        }
        let rect = Rect::from_min_size(pos2(x, TOP), vec2(w, h));
        let mut hits: Vec<(String, Rect)> = Vec::new();
        let mut actions = Vec::new();
        let mut scroll = self.review_scroll.min(card.steps.len().saturating_sub(rows));
        let mut wheel = 0.0;
        let mut ask = std::mem::take(&mut self.review_ask);
        let mut ask_focus = false;
        let mut sent = None;
        Self::panel(ctx, "review", rect, Order::Middle, |ui, rect| {
            let inner = rect.shrink(12.0);
            let p = ui.painter().clone();
            // Header: AI CHANGE (or AGENT CHANGE), and how many steps.
            let hy = inner.top() + 7.5;
            let (glyph, title) =
                if card.agents { (icon::TERMINAL, "AGENT CHANGE") } else { (icon::SPARKLES, "AI CHANGE") };
            p.text(pos2(inner.left() + 6.0, hy), Align2::CENTER_CENTER, glyph, icon_font(13.0), th().accent);
            let g = p.layout_job(spaced(title, sans_semibold(11.0), th().muted, 0.88));
            p.galley(pos2(inner.left() + 18.0, hy - g.size().y / 2.0), g, th().muted);
            let n = card.steps.len();
            let right = if card.before {
                ("BEFORE".to_string(), th().mid)
            } else {
                (format!("{n} step{}", if n == 1 { "" } else { "s" }), th().faint)
            };
            p.text(pos2(inner.right(), hy), Align2::RIGHT_CENTER, right.0, mono(11.0), right.1);
            let mut y = inner.top() + 15.0 + 10.0;
            let ph = prompt.size().y;
            p.galley(pos2(inner.left(), y), prompt, th().text);
            y += ph;
            if let Some(g) = summary {
                y += 4.0;
                let gh = g.size().y;
                p.galley(pos2(inner.left(), y), g, th().muted);
                y += gh;
            }
            // The assistant is still at it: a pulsing dot and what it is doing.
            if let Some(w) = &card.working {
                y += 6.0;
                let t = ui.input(|i| i.time);
                let pulse = 0.55 + 0.45 * (t * 4.0).sin().abs() as f32;
                p.circle_filled(pos2(inner.left() + 4.0, y + 8.5), 3.5, th().accent.gamma_multiply(pulse));
                let dots = ".".repeat(1 + (t * 3.0) as usize % 3);
                p.text(
                    pos2(inner.left() + 14.0, y + 8.5),
                    Align2::LEFT_CENTER,
                    format!("{w}{dots}"),
                    sans(12.0),
                    th().muted,
                );
                ui.ctx().request_repaint();
                y += 17.0;
            }
            // The counts: +added · removed · repainted · slots changed.
            y += 10.0;
            let counts = Rect::from_min_size(pos2(inner.left(), y), vec2(inner_w, counts_h));
            p.rect(counts, CornerRadius::same(8), th().tint(9), Stroke::new(1.0, th().line), StrokeKind::Inside);
            let g = counts_text;
            p.galley(pos2(counts.left() + 10.0, counts.center().y - g.size().y / 2.0), g, th().text);
            hits.push(("review-counts".into(), counts));
            y += counts_h;
            // Palette slots, by slot: old and new colour, the new hex, the material change.
            if !card.slots.is_empty() {
                y += 6.0;
                for c in &card.slots {
                    let r = Rect::from_min_size(pos2(inner.left(), y), vec2(inner_w, slot_h));
                    let cy = r.center().y;
                    p.text(
                        pos2(r.left() + 2.0, cy),
                        Align2::LEFT_CENTER,
                        format!("SLOT {}", c.slot),
                        mono(10.0),
                        th().faint,
                    );
                    let sw = |at: f32, rgba: [u8; 4]| {
                        let q = Rect::from_min_size(pos2(at, cy - 7.0), vec2(14.0, 14.0));
                        p.rect(
                            q,
                            CornerRadius::same(4),
                            Color32::from_rgb(rgba[0], rgba[1], rgba[2]),
                            Stroke::new(1.0, th().line2),
                            StrokeKind::Inside,
                        );
                    };
                    let sx = r.left() + 62.0;
                    sw(sx, c.old);
                    p.text(pos2(sx + 23.0, cy), Align2::CENTER_CENTER, "→", sans(11.0), th().faint);
                    sw(sx + 32.0, c.new);
                    if c.old != c.new {
                        let hex = format!("#{:02X}{:02X}{:02X}", c.new[0], c.new[1], c.new[2]);
                        p.text(pos2(sx + 54.0, cy), Align2::LEFT_CENTER, hex, mono(11.0), th().text);
                    }
                    if let Some(m) = &c.material {
                        p.text(pos2(r.right(), cy), Align2::RIGHT_CENTER, m, sans(11.5), th().muted);
                    }
                    hits.push((format!("review-slot-{}", c.slot), r));
                    y += slot_h;
                }
            }
            // Over the cells allowed without asking: Stop or Allow.
            if let Some(g) = held_text {
                y += 10.0;
                let gh = g.size().y;
                let b = Rect::from_min_size(pos2(inner.left(), y), vec2(inner_w, 12.0 + gh + 10.0 + 26.0 + 12.0));
                p.rect(b, CornerRadius::same(8), th().mid_bg, Stroke::new(1.0, th().mid_line), StrokeKind::Inside);
                p.text(
                    pos2(b.left() + 10.0 + 7.0, b.top() + 12.0 + 8.5),
                    Align2::CENTER_CENTER,
                    icon::TRIANGLE_ALERT,
                    icon_font(13.0),
                    th().mid,
                );
                p.galley(pos2(b.left() + 30.0, b.top() + 12.0), g, th().text);
                let by = b.top() + 12.0 + gh + 10.0;
                let allow = Rect::from_min_size(pos2(b.right() - 10.0 - 72.0, by), vec2(72.0, 26.0));
                let stop = allow.translate(vec2(-72.0 - 6.0, 0.0));
                if small_button(ui, stop, icon::X, "Stop", "review-stop", false).clicked() {
                    actions.push(Action::StopHeld);
                }
                if small_button(ui, allow, icon::CHECK, "Allow", "review-allow", true).clicked() {
                    actions.push(Action::AllowMore);
                }
                hits.push(("review-stop".into(), stop));
                hits.push(("review-allow".into(), allow));
                y = b.bottom();
            }
            // The steps, each with a checkbox that drops it from the change.
            if rows > 0 {
                y += 14.0;
                let g = p.layout_job(spaced("STEPS", sans(9.0), th().faint, 0.72));
                p.galley(pos2(inner.left(), y + 6.0 - g.size().y / 2.0), g, th().faint);
                let kept = card.steps.iter().filter(|s| s.ok && !s.dropped).count();
                p.text(
                    pos2(inner.right(), y + 6.0),
                    Align2::RIGHT_CENTER,
                    format!("{kept} kept"),
                    mono(10.0),
                    th().faint,
                );
                y += 12.0 + 6.0;
                let body = Rect::from_min_size(pos2(inner.left() - 4.0, y), vec2(inner_w + 8.0, rows as f32 * row_h));
                if card.steps.len() > rows && ui.rect_contains_pointer(body) {
                    wheel = ui.input(|i| i.smooth_scroll_delta.y);
                }
                for (k, st) in card.steps.iter().enumerate().skip(scroll).take(rows) {
                    let r = Rect::from_min_size(
                        pos2(body.left(), body.top() + (k - scroll) as f32 * row_h),
                        vec2(body.width(), row_h),
                    );
                    let id = format!("review-step-{k}");
                    let resp =
                        ui.interact(r, Id::unique(&id), if st.droppable { Sense::click() } else { Sense::hover() });
                    hits.push((id, r));
                    let p = ui.painter();
                    if st.droppable && resp.hovered() {
                        p.rect_filled(r, CornerRadius::same(7), th().hover);
                    }
                    let cb = Rect::from_center_size(pos2(r.left() + 4.0 + 7.0, r.center().y), vec2(14.0, 14.0));
                    if !st.ok {
                        p.text(cb.center(), Align2::CENTER_CENTER, icon::X, icon_font(12.0), th().danger);
                    } else if st.dropped {
                        p.rect_stroke(cb, CornerRadius::same(4), Stroke::new(1.0, th().line2), StrokeKind::Inside);
                    } else {
                        p.rect_filled(cb, CornerRadius::same(4), th().accent);
                        p.text(cb.center(), Align2::CENTER_CENTER, icon::CHECK, icon_font(11.0), th().on_accent);
                    }
                    let kept = st.ok && !st.dropped;
                    let tool_c = if kept { th().text } else { th().faint };
                    let tg = p.layout_no_wrap(st.tool.to_string(), mono(11.5), tool_c);
                    let tw = tg.size().x;
                    let tx = cb.right() + 9.0;
                    p.galley(pos2(tx, r.center().y - tg.size().y / 2.0), tg, tool_c);
                    let dx = tx + tw + 8.0;
                    let detail_c = if !st.ok {
                        th().danger.gamma_multiply(0.8)
                    } else if kept {
                        th().muted
                    } else {
                        th().faint
                    };
                    let mut job = LayoutJob::single_section(
                        st.detail.clone(),
                        TextFormat { font_id: sans(11.5), color: detail_c, ..Default::default() },
                    );
                    job.wrap = egui::text::TextWrapping {
                        max_width: (r.right() - 6.0 - dx).max(1.0),
                        max_rows: 1,
                        break_anywhere: true,
                        overflow_character: Some('…'),
                    };
                    let dg = p.layout_job(job);
                    p.galley(pos2(dx, r.center().y - dg.size().y / 2.0), dg, detail_c);
                    if st.dropped {
                        let end = dx
                            + p.layout_no_wrap(st.detail.clone(), sans(11.5), th().faint)
                                .size()
                                .x
                                .min(r.right() - 6.0 - dx);
                        p.line_segment([pos2(tx, r.center().y), pos2(end, r.center().y)], Stroke::new(1.0, th().faint));
                    }
                    if st.droppable && resp.clicked() {
                        actions.push(Action::DropStep(st.index, !st.dropped));
                    }
                }
                y += rows as f32 * row_h;
            }
            // Before and After.
            y += 12.0;
            let seg = Rect::from_min_size(pos2(inner.left(), y), vec2(inner_w, 28.0));
            let mut seg_hits = Vec::new();
            if let Some(k) = segmented(
                ui,
                seg,
                &["review-before", "review-after"],
                &["Before".to_string(), "After".to_string()],
                if card.before { 0 } else { 1 },
                &[true, true],
                sans(12.0),
                &mut seg_hits,
            ) {
                actions.push(Action::ReviewBefore(k == 0));
            }
            hits.extend(seg_hits.into_iter().map(|(n, r)| (n.to_string(), r)));
            y += 28.0 + 8.0;
            // Ask for changes: the assistant takes what is sent (`asked`). Agents take theirs in
            // the terminal.
            if !card.agents {
                let fr = Rect::from_min_size(pos2(inner.left(), y), vec2(inner_w, 30.0));
                let id = Id::unique("review-ask");
                let focused = ui.memory(|m| m.has_focus(id));
                let border = if focused { th().ring } else { th().line };
                p.rect(fr, CornerRadius::same(7), th().tint(9), Stroke::new(1.0, border), StrokeKind::Inside);
                p.text(
                    pos2(fr.right() - 16.0, fr.center().y),
                    Align2::CENTER_CENTER,
                    icon::CORNER_DOWN_LEFT,
                    icon_font(13.0),
                    th().faint,
                );
                if ask.is_empty() && !focused {
                    p.text(
                        pos2(fr.left() + 10.0, fr.center().y),
                        Align2::LEFT_CENTER,
                        "Ask for changes…",
                        sans(12.0),
                        th().faint,
                    );
                }
                let tr = Rect::from_min_max(
                    pos2(fr.left() + 10.0, fr.top() + 7.0),
                    pos2(fr.right() - 30.0, fr.bottom() - 7.0),
                );
                let edit = egui::TextEdit::singleline(&mut ask)
                    .id(id)
                    .font(sans(12.0))
                    .text_color(th().text)
                    .frame(egui::Frame::NONE)
                    .margin(egui::Margin::ZERO)
                    .desired_width(tr.width());
                let resp = ui.put(tr, edit);
                hits.push(("review-ask".into(), fr));
                ask_focus = resp.has_focus();
                if resp.lost_focus() && ui.input(|i| i.key_pressed(Key::Enter)) && !ask.trim().is_empty() {
                    sent = Some(std::mem::take(&mut ask).trim().to_string());
                }
                y += 30.0 + 12.0;
            }
            // Discard (Escape) and Apply (Enter).
            let ink = th().on_accent;
            let label = p.layout_no_wrap("Apply".into(), sans_semibold(13.0), ink);
            let kw = ui.painter().layout_no_wrap("↵".into(), mono_medium(10.5), th().muted).size().x.max(8.0) + 10.0;
            let aw = label.size().x + 28.0 + kw + 8.0;
            let a = Rect::from_min_size(pos2(inner.right() - aw, y), vec2(aw, 32.0));
            let resp = ui.interact(a, Id::unique("review-apply"), Sense::click());
            let fill = if resp.hovered() { th().accent_hover } else { th().accent };
            p.rect_filled(a, CornerRadius::same(8), fill);
            p.galley(pos2(a.left() + 14.0, a.center().y - label.size().y / 2.0), label, ink);
            let kr = Rect::from_min_size(pos2(a.right() - 14.0 - kw, a.center().y - 9.5), vec2(kw, 19.0));
            p.rect_stroke(kr, CornerRadius::same(5), Stroke::new(1.0, ink.gamma_multiply(0.35)), StrokeKind::Inside);
            p.text(kr.center(), Align2::CENTER_CENTER, "↵", mono_medium(10.5), ink);
            if resp.clicked() {
                actions.push(Action::ApplyReview);
            }
            hits.push(("review-apply".into(), a));
            let dl = p.layout_no_wrap("Discard".into(), sans(13.0), th().muted);
            let ew = p.layout_no_wrap("esc".into(), mono_medium(10.5), th().muted).size().x + 10.0;
            let dw = dl.size().x + 28.0 + ew + 8.0;
            let d = Rect::from_min_size(pos2(a.left() - 6.0 - dw, y), vec2(dw, 32.0));
            let resp = ui.interact(d, Id::unique("review-discard"), Sense::click());
            let color = if resp.hovered() { th().text } else { th().muted };
            p.rect_stroke(
                d,
                CornerRadius::same(8),
                Stroke::new(1.0, if resp.hovered() { th().tint(64) } else { th().line2 }),
                StrokeKind::Inside,
            );
            p.galley(pos2(d.left() + 14.0, d.center().y - dl.size().y / 2.0), dl, color);
            kbd(ui, pos2(d.right() - 14.0, d.center().y), "esc", true);
            if resp.clicked() {
                actions.push(Action::DiscardReview);
            }
            hits.push(("review-discard".into(), d));
        });
        if wheel != 0.0 {
            let max = card.steps.len().saturating_sub(rows);
            let step = (-wheel / row_h).round() as i64;
            scroll = (scroll as i64 + step).clamp(0, max as i64) as usize;
        }
        self.review_scroll = scroll;
        self.review_ask = ask;
        self.ask_focus = ask_focus;
        if sent.is_some() {
            self.asked = sent;
        }
        self.hits.extend(hits);
        self.hits.insert("review".into(), rect);
        self.actions.extend(actions);
        self.review = Some(card);
    }

    /// The editor did not start the card's "Ask for changes" (its toast says why): the words
    /// go back into the field.
    pub fn review_ask_refused(&mut self, text: &str) {
        if self.review_ask.trim().is_empty() {
            self.review_ask = text.trim().to_string();
        }
    }

    // The select tool's box while it is dragged: an accent outline over a faint accent fill.
    fn marquee(&self, ctx: &Context) {
        if let Some(r) = self.marquee {
            let p = ctx.layer_painter(egui::LayerId::new(Order::Background, Id::unique("marquee")));
            p.rect(r, CornerRadius::same(2), th().accent_tint, Stroke::new(1.0, th().accent), StrokeKind::Inside);
        }
    }

    // #palette
    fn palette_panel(&mut self, ctx: &Context, screen: Rect) {
        let used_count = self.used.iter().skip(1).filter(|u| **u).count();
        // The colours in the scene (and the current one), or every slot: when asked, while a
        // swap or copy waits for its target, and while the scene has no colour yet.
        let all = self.palette_all || self.slot_op.is_some() || used_count == 0;
        let slots: Vec<u8> = (1..=255u8).filter(|&i| all || self.used[i as usize] || i == self.slot).collect();
        let rows = (slots.len() as f32 / 8.0).ceil();
        let view_h = rows.min(SWATCH_ROWS) * SWATCH_STEP + 2.0;
        let h = 12.0 + 20.0 + 8.0 + 38.0 + 10.0 + view_h + 10.0;
        let rect = if self.touch {
            // Over the dock, from its colour button.
            if !self.palette_open {
                return;
            }
            let at = self.hits.get("dock-color").map_or(screen.center().x, |r| r.center().x);
            let x = (at - PALETTE_W / 2.0).min(screen.right() - 12.0 - PALETTE_W).max(screen.left() + 12.0);
            Rect::from_min_size(pos2(x, (self.dock_top - 8.0 - h).max(TOP)), vec2(PALETTE_W, h))
        } else {
            Rect::from_min_size(pos2(screen.right() - 12.0 - PALETTE_W, TOP), vec2(PALETTE_W, h))
        };
        self.hits.insert("palette".into(), rect);
        let mut set_all = None;
        Self::panel(ctx, "palette", rect, Order::Middle, |ui, rect| {
            let inner = rect.shrink(12.0);
            let p = ui.painter().clone();
            // .ph, with the Used / All toggle.
            let hy = inner.top() + 10.0;
            let g = ui.painter().layout_job(spaced("PALETTE", sans_semibold(10.5), th().muted, 0.84));
            p.galley(pos2(inner.left(), hy - g.size().y / 2.0), g, th().muted);
            let mut x = inner.right();
            for (id, label, on) in
                [("palette-all", "All".to_string(), all), ("palette-used", format!("Used {used_count}"), !all)]
            {
                let g = p.layout_no_wrap(label, sans_medium(10.5), if on { th().text } else { th().faint });
                let r = Rect::from_min_size(pos2(x - g.size().x - 12.0, hy - 10.0), vec2(g.size().x + 12.0, 20.0));
                x = r.left() - 2.0;
                let resp = ui.interact(r, Id::unique(id), Sense::click());
                if on {
                    p.rect(r, CornerRadius::same(6), th().accent_bg, Stroke::new(1.0, th().line2), StrokeKind::Inside);
                } else if resp.hovered() {
                    p.rect_filled(r, CornerRadius::same(6), th().hover);
                }
                p.galley(r.center() - g.size() / 2.0, g, if on || resp.hovered() { th().text } else { th().faint });
                if resp.clicked() {
                    set_all = Some(id == "palette-all");
                }
                self.hits.insert(id.into(), r);
            }
            // #current: the slot's colour, hex and number on one row, and its edit button.
            let cur = Rect::from_min_size(pos2(inner.left(), inner.top() + 28.0), vec2(inner.width(), 38.0));
            p.rect(cur, CornerRadius::same(8), th().tint(9), Stroke::new(1.0, th().line), StrokeKind::Inside);
            let chip = Rect::from_min_size(cur.min + vec2(6.0, 6.0), vec2(26.0, 26.0));
            p.rect(
                chip,
                CornerRadius::same(6),
                self.color(self.slot),
                Stroke::new(1.0, th().tint(38)),
                StrokeKind::Inside,
            );
            let c = self.palette[self.slot as usize];
            let hex = format!("#{:02X}{:02X}{:02X}", c[0], c[1], c[2]);
            let mx = chip.right() + 8.0;
            p.text(pos2(mx, cur.center().y), Align2::LEFT_BOTTOM, hex, mono(12.0), th().text);
            let sub = if self.used[self.slot as usize] {
                format!("Slot {} · in scene", self.slot)
            } else {
                format!("Slot {}", self.slot)
            };
            p.text(pos2(mx, cur.center().y + 1.0), Align2::LEFT_TOP, sub, sans(10.5), th().faint);
            let edit = Rect::from_min_size(pos2(cur.right() - 6.0 - 26.0, cur.center().y - 13.0), vec2(26.0, 26.0));
            let er = ui.interact(edit, Id::unique("edit-color"), Sense::click());
            self.hits.insert("edit-color".into(), edit);
            let open = self.color_edit.is_some();
            if open || er.hovered() {
                p.rect_filled(edit, CornerRadius::same(6), th().hover);
            }
            let ec = if er.hovered() || open { th().text } else { th().muted };
            p.text(edit.center(), Align2::CENTER_CENTER, icon::PENCIL, icon_font(14.0), ec);
            if er.hovered() {
                tooltip(ctx, edit, "Edit color", &[], Some((Rect::NOTHING, true)));
            }
            if er.clicked() {
                if open {
                    self.color_edit = None;
                } else {
                    self.open_color_edit();
                }
            }
            // #swatches: 8 columns, in a view that scrolls past eight rows.
            let view = Rect::from_min_size(pos2(inner.left(), cur.bottom() + 10.0), vec2(inner.width(), view_h));
            let mut child = ui.new_child(egui::UiBuilder::new().max_rect(view));
            child.spacing_mut().scroll = egui::style::ScrollStyle::thin();
            egui::ScrollArea::vertical()
                .id_salt("swatches")
                .max_height(view_h)
                .auto_shrink([false; 2])
                .show(&mut child, |ui| self.swatches(ui, view.width(), &slots));
        });
        if let Some(all) = set_all {
            self.palette_all = all;
        }
    }

    // #swatches, inside the scroll view: `slots` in order, 8 to a row.
    fn swatches(&mut self, ui: &mut Ui, width: f32, slots: &[u8]) {
        let rows = (slots.len() as f32 / 8.0).ceil();
        let (area, _) = ui.allocate_exact_size(vec2(width, rows * SWATCH_STEP + 2.0), Sense::hover());
        let grid = area.min + vec2(2.0, 2.0);
        let p = ui.painter().clone();
        let (pointer, released, cmd, shift) =
            ui.input(|i| (i.pointer.latest_pos(), i.pointer.any_released(), i.modifiers.command, i.modifiers.shift));
        let visible = ui.clip_rect();
        let mut drop_on = None;
        for (i, &slot) in slots.iter().enumerate() {
            let (col, row) = ((i % 8) as f32, (i / 8) as f32);
            let r = Rect::from_min_size(grid + vec2(col * SWATCH_STEP, row * SWATCH_STEP), vec2(SWATCH, SWATCH));
            let resp = ui.interact(r, Id::unique(("sw", slot)), Sense::click_and_drag());
            if visible.contains_rect(r) {
                self.hits.insert(format!("sw{slot}"), r);
            }
            let on = self.slot == slot;
            if on && self.shown_slot != slot {
                ui.scroll_to_rect(r.expand(4.0), None);
                self.shown_slot = slot;
            }
            let scale = if on {
                1.05
            } else if resp.hovered() || (self.drag_from.is_some() && pointer.is_some_and(|q| r.contains(q))) {
                1.12
            } else {
                1.0
            };
            let r = Rect::from_center_size(r.center(), r.size() * scale);
            if on {
                p.rect_filled(r.expand(3.0), CornerRadius::same(7), th().text);
                p.rect_filled(r.expand(1.5), CornerRadius::same(5), th().panel_solid);
            }
            p.rect(r, CornerRadius::same(4), self.color(slot), Stroke::new(1.0, th().tint(20)), StrokeKind::Inside);
            if self.materials[slot as usize].kind != Kind::Diffuse {
                // A material other than diffuse: a light corner, like a glint.
                let c = r.right_top() + vec2(-1.5, 1.5);
                p.add(egui::Shape::convex_polygon(
                    vec![c + vec2(-6.0, 0.0), c, c + vec2(0.0, 6.0)],
                    th().tint(200),
                    Stroke::new(1.0, Color32::from_black_alpha(90)),
                ));
            }
            if self.used[slot as usize] {
                // mix-blend-mode: difference against white at 75%.
                let bg = self.color(slot);
                let d = |v: u8| (v as f32 * 0.25 + (255.0 - v as f32) * 0.75) as u8;
                p.circle_filled(r.center(), 1.75, Color32::from_rgb(d(bg.r()), d(bg.g()), d(bg.b())));
            }
            if resp.drag_started() && cmd {
                self.drag_from = Some(slot);
            }
            if pointer.is_some_and(|q| r.contains(q)) {
                drop_on = Some(slot);
            }
            if resp.double_clicked() {
                self.slot = slot;
                self.open_color_edit();
            } else if resp.clicked() {
                self.click_slot(slot);
            }
        }
        // MagicaVoxel's palette drags: Cmd (Ctrl) swaps, Cmd+Shift (Ctrl+Shift) copies.
        if let Some(from) = self.drag_from {
            if let Some(q) = pointer {
                let ghost = Rect::from_center_size(q + vec2(10.0, 10.0), vec2(SWATCH, SWATCH));
                let lp = ui.ctx().layer_painter(egui::LayerId::new(Order::Tooltip, Id::unique("sw-drag")));
                lp.rect(
                    ghost,
                    CornerRadius::same(4),
                    self.color(from),
                    Stroke::new(1.5, Color32::WHITE),
                    StrokeKind::Outside,
                );
            }
            if released {
                self.drag_from = None;
                if let Some(to) = drop_on.filter(|&t| t != from) {
                    self.slot = from;
                    self.slot_op = Some(if shift { SlotOp::Copy } else { SlotOp::Swap });
                    self.click_slot(to);
                }
            }
        }
    }

    // The colour editor: not in the mockups. A .panel beside #palette, built from its parts
    // (.ph header, #current-style fields, #edit-color buttons, .note).
    fn color_editor(&mut self, ctx: &Context, screen: Rect) {
        let Some(mut ed) = self.color_edit.take() else { return };
        let slot = self.slot;
        let c = self.palette[slot as usize];
        let rgb = [c[0], c[1], c[2]];
        if ed.hsv.to_rgb() != rgb {
            ed.hsv = Hsv::from_rgb(rgb);
        }
        let w = 232.0;
        let inner_w = w - 24.0;
        let mut job = LayoutJob::default();
        let fmt = |color| TextFormat { font_id: sans(11.0), color, line_height: Some(15.95), ..Default::default() };
        match self.slot_op {
            Some(op) => {
                let what = match op {
                    SlotOp::Swap => format!("Click a slot to swap with slot {slot}. "),
                    SlotOp::Copy => "Click a slot to give it this color. ".to_string(),
                };
                job.append(&what, 0.0, fmt(th().text));
                job.append("Esc cancels.", 0.0, fmt(th().muted));
            }
            None => job.append(
                "Swap trades places with the next slot you click and keeps the scene's look. Copy gives that slot this color.",
                0.0,
                fmt(th().faint),
            ),
        }
        job.wrap.max_width = inner_w;
        let note = ctx.fonts_mut(|f| f.layout_job(job));
        let mat = self.materials[slot as usize];
        let mut mjob = LayoutJob::default();
        let mat_note = match mat.kind {
            Kind::Diffuse => "Matte, lit by the sun and the sky.",
            Kind::Metal => "Reflects its surroundings in its own color. Rough metal blurs them.",
            Kind::Glass => {
                "Light passes through, tinted by the color, and the surface reflects. Attenuation deepens the tint with the glass crossed."
            }
            Kind::Emit => "Glows in its color and lights what is near it.",
            Kind::Plastic => "Matte color under a clear glossy coat. Specular sets how much the coat reflects.",
        };
        mjob.append(mat_note, 0.0, fmt(th().muted));
        mjob.append(
            " Shows in the render corner and export, and saves to the file's MATL chunks.",
            0.0,
            fmt(th().faint),
        );
        mjob.wrap.max_width = inner_w;
        let mat_note = ctx.fonts_mut(|f| f.layout_job(mjob));
        let sv_h = 136.0;
        let tabs_h = 28.0 + 10.0;
        let h = 12.0
            + 15.0
            + 10.0
            + tabs_h
            + sv_h
            + 10.0
            + 12.0
            + 12.0
            + 3.0 * 28.0
            + 2.0 * 6.0
            + 12.0
            + 28.0
            + 10.0
            + note.size().y
            + 12.0;
        // The Material tab: type buttons, a slider per setting, the note.
        let rows = match mat.kind {
            Kind::Diffuse => 0.0,
            Kind::Metal | Kind::Emit | Kind::Plastic => 2.0,
            Kind::Glass => 4.0,
        };
        let h = if ed.tab == EditTab::Material {
            12.0 + 15.0
                + 10.0
                + tabs_h
                + 28.0
                + 12.0
                + rows * 34.0
                + if rows > 0.0 { 6.0 } else { 0.0 }
                + mat_note.size().y
                + 12.0
        } else {
            h
        };
        let rect = match self.hits.get("palette").copied().filter(|_| self.touch) {
            // Over the dock: beside the palette, level with its bottom, on whichever side fits.
            Some(pal) => {
                let x =
                    if pal.left() - 8.0 - w >= screen.left() + 12.0 { pal.left() - 8.0 - w } else { pal.right() + 8.0 };
                Rect::from_min_size(pos2(x, (pal.bottom() - h).max(TOP)), vec2(w, h))
            }
            None => Rect::from_min_size(pos2(screen.right() - 12.0 - PALETTE_W - 8.0 - w, TOP), vec2(w, h)),
        };
        let (mut close, mut commit, mut live) = (false, false, false);
        let mut new_mat = None;
        let mut op = None;
        let mut hits = Vec::new();
        let current = [
            hex(rgb),
            rgb[0].to_string(),
            rgb[1].to_string(),
            rgb[2].to_string(),
            format!("{:.0}", ed.hsv.h),
            format!("{:.0}", ed.hsv.s * 100.0),
            format!("{:.0}", ed.hsv.v * 100.0),
        ];
        Self::panel(ctx, "color-edit", rect, Order::Middle, |ui, rect| {
            let inner = rect.shrink(12.0);
            let p = ui.painter().clone();
            // .ph
            let hy = inner.top() + 7.5;
            let title = if ed.tab == EditTab::Material { "EDIT MATERIAL" } else { "EDIT COLOR" };
            let g = p.layout_job(spaced(title, sans_semibold(11.0), th().muted, 0.88));
            p.galley(pos2(inner.left(), hy - g.size().y / 2.0), g, th().muted);
            let x = Rect::from_center_size(pos2(inner.right() - 6.0, hy), vec2(16.0, 22.0));
            let xr = ui.interact(x, Id::unique("color-edit-x"), Sense::click());
            hits.push(("color-edit-x", x));
            p.text(
                x.center(),
                Align2::CENTER_CENTER,
                "×",
                sans(18.0),
                if xr.hovered() { th().text } else { th().faint },
            );
            close |= xr.clicked();
            p.text(pos2(x.left() - 6.0, hy), Align2::RIGHT_CENTER, format!("Slot {slot}"), mono(11.0), th().faint);
            // Color and Material tabs.
            let tr = Rect::from_min_size(pos2(inner.left(), inner.top() + 25.0), vec2(inner.width(), 28.0));
            let tabs = ["Color".to_string(), "Material".to_string()];
            let sel = (ed.tab == EditTab::Material) as usize;
            if let Some(k) =
                segmented(ui, tr, &["tab-color", "tab-material"], &tabs, sel, &[true; 2], sans(12.0), &mut hits)
            {
                ed.tab = if k == 0 { EditTab::Color } else { EditTab::Material };
            }
            if ed.tab == EditTab::Material {
                let mut y = tr.bottom() + 10.0;
                // Shown in MagicaVoxel's order; each button's hit is `mt` and the type's number.
                let kinds = Kind::SHOWN.map(|k| k.name().to_string());
                let names = Kind::SHOWN.map(|k| match k {
                    Kind::Diffuse => "mt0",
                    Kind::Metal => "mt1",
                    Kind::Glass => "mt2",
                    Kind::Emit => "mt3",
                    Kind::Plastic => "mt4",
                });
                let sel = Kind::SHOWN.iter().position(|&k| k == mat.kind).unwrap_or(0);
                let r = Rect::from_min_size(pos2(inner.left(), y), vec2(inner.width(), 28.0));
                if let Some(k) = segmented(ui, r, &names, &kinds, sel, &[true; 5], sans(11.0), &mut hits)
                    && Kind::SHOWN[k] != mat.kind
                {
                    new_mat = Some((Material { kind: Kind::SHOWN[k], ..mat }, true));
                }
                y += 28.0 + 12.0;
                let mut m = mat;
                let pct: fn(f32) -> String = |v| format!("{:.0}%", v * 100.0);
                let rows: Vec<SliderRow> = match mat.kind {
                    Kind::Diffuse => vec![],
                    Kind::Metal => vec![
                        ("mat-rough", "Roughness", &mut m.rough, 0.0, 1.0, pct),
                        ("mat-metal", "Metalness", &mut m.metal, 0.0, 1.0, pct),
                    ],
                    Kind::Glass => vec![
                        ("mat-rough", "Roughness", &mut m.rough, 0.0, 1.0, pct),
                        ("mat-trans", "Transparency", &mut m.trans, 0.0, 1.0, pct),
                        ("mat-ior", "Refraction", &mut m.ior, 1.0, 3.0, |v| format!("{v:.2}")),
                        ("mat-att", "Attenuation", &mut m.att, 0.0, 1.0, pct),
                    ],
                    Kind::Plastic => vec![
                        ("mat-rough", "Roughness", &mut m.rough, 0.0, 1.0, pct),
                        ("mat-spec", "Specular", &mut m.spec, 0.0, 1.0, pct),
                    ],
                    Kind::Emit => vec![
                        ("mat-emit", "Emission", &mut m.emit, 0.0, 1.0, pct),
                        ("mat-flux", "Power", &mut m.flux, 0.0, 4.0, |v| format!("{v:.1}")),
                    ],
                };
                let (mut changed, mut done) = (false, false);
                for (id, label, v, lo, hi, f) in rows {
                    let r = Rect::from_min_size(pos2(inner.left(), y), vec2(inner.width(), 34.0));
                    hits.push((id, r));
                    let (c, d) = slider(ui, r, id, label, v, lo, hi, f);
                    (changed, done) = (changed | c, done | d);
                    y += 34.0;
                }
                if changed || done {
                    new_mat = Some((m, done));
                }
                p.galley(
                    pos2(inner.left(), y + if mat.kind == Kind::Diffuse { 0.0 } else { 6.0 }),
                    mat_note,
                    th().faint,
                );
                return;
            }
            // Saturation (across) and value (down) at the current hue.
            let sv = Rect::from_min_size(pos2(inner.left(), tr.bottom() + 10.0), vec2(inner.width(), sv_h));
            let hue = ed.hsv.h;
            gradient(&p, sv, 16, 8, |u, v| {
                let c = Hsv { h: hue, s: u, v: 1.0 - v }.to_rgb();
                Color32::from_rgb(c[0], c[1], c[2])
            });
            p.rect_stroke(sv, CornerRadius::ZERO, Stroke::new(1.0, th().line), StrokeKind::Inside);
            let r = ui.interact(sv, Id::unique("sv"), Sense::click_and_drag());
            hits.push(("sv", sv));
            if r.is_pointer_button_down_on()
                && let Some(q) = r.interact_pointer_pos()
            {
                ed.hsv.s = ((q.x - sv.left()) / sv.width()).clamp(0.0, 1.0);
                ed.hsv.v = 1.0 - ((q.y - sv.top()) / sv.height()).clamp(0.0, 1.0);
                live = true;
            }
            commit |= r.drag_stopped() || r.clicked();
            handle(
                &p,
                pos2(sv.left() + ed.hsv.s * sv.width(), sv.top() + (1.0 - ed.hsv.v) * sv.height()),
                ed.hsv.to_rgb(),
            );
            // Hue.
            let hb = Rect::from_min_size(pos2(inner.left(), sv.bottom() + 10.0), vec2(inner.width(), 12.0));
            gradient(&p, hb, 36, 1, |u, _| {
                let c = Hsv { h: u * 360.0, s: 1.0, v: 1.0 }.to_rgb();
                Color32::from_rgb(c[0], c[1], c[2])
            });
            p.rect_stroke(hb, CornerRadius::ZERO, Stroke::new(1.0, th().line), StrokeKind::Inside);
            let r = ui.interact(hb.expand2(vec2(0.0, 4.0)), Id::unique("hue"), Sense::click_and_drag());
            hits.push(("hue", hb));
            if r.is_pointer_button_down_on()
                && let Some(q) = r.interact_pointer_pos()
            {
                ed.hsv.h = ((q.x - hb.left()) / hb.width()).clamp(0.0, 1.0) * 360.0;
                live = true;
            }
            commit |= r.drag_stopped() || r.clicked();
            let hc = Hsv { h: ed.hsv.h, s: 1.0, v: 1.0 }.to_rgb();
            handle(&p, pos2(hb.left() + ed.hsv.h / 360.0 * hb.width(), hb.center().y), hc);
            // Fields: hex across, then R G B and H S V.
            let mut y = hb.bottom() + 12.0;
            let third = (inner.width() - 12.0) / 3.0;
            let mut rects = vec![Rect::from_min_size(pos2(inner.left(), y), vec2(inner.width(), 28.0))];
            for _ in 0..2 {
                y += 28.0 + 6.0;
                for k in 0..3 {
                    rects
                        .push(Rect::from_min_size(pos2(inner.left() + k as f32 * (third + 6.0), y), vec2(third, 28.0)));
                }
            }
            for (i, fr) in rects.into_iter().enumerate() {
                let id = Id::unique(("color-field", i));
                if !ui.memory(|m| m.has_focus(id)) {
                    ed.fields[i] = current[i].clone();
                }
                hits.push((["field-hex", "field-r", "field-g", "field-b", "field-h", "field-s", "field-v"][i], fr));
                if field(ui, fr, FIELDS[i], id, &mut ed.fields[i]) && ed.fields[i] != current[i] {
                    let t = ed.fields[i].trim().trim_end_matches('%').trim_end_matches('°');
                    let num = t.parse::<f32>().ok();
                    let mut c = rgb;
                    let mut hsv = ed.hsv;
                    let ok = match i {
                        0 => parse_hex(t).map(|v| c = v).is_some(),
                        1..=3 => num.map(|n| c[i - 1] = n.round().clamp(0.0, 255.0) as u8).is_some(),
                        4 => num.map(|n| hsv.h = n.clamp(0.0, 360.0)).is_some(),
                        _ => num.map(|n| *[&mut hsv.s, &mut hsv.v][i - 5] = (n / 100.0).clamp(0.0, 1.0)).is_some(),
                    };
                    if ok {
                        ed.hsv = if i >= 4 { hsv } else { Hsv::from_rgb(c) };
                        (live, commit) = (true, true);
                    }
                }
            }
            // Swap and copy.
            let by = y + 28.0 + 12.0;
            let half = (inner.width() - 6.0) / 2.0;
            for (k, (o, glyph, label, name)) in [
                (SlotOp::Swap, icon::ARROW_LEFT_RIGHT, "Swap with…", "swap"),
                (SlotOp::Copy, icon::COPY, "Copy to…", "copy"),
            ]
            .into_iter()
            .enumerate()
            {
                let br = Rect::from_min_size(pos2(inner.left() + k as f32 * (half + 6.0), by), vec2(half, 28.0));
                hits.push((name, br));
                if small_button(ui, br, glyph, label, name, self.slot_op == Some(o)).clicked() {
                    op = Some(if self.slot_op == Some(o) { None } else { Some(o) });
                }
            }
            p.galley(pos2(inner.left(), by + 28.0 + 10.0), note, th().faint);
        });
        for (k, r) in hits {
            self.hits.insert(k.into(), r);
        }
        if let Some(o) = op {
            self.slot_op = o;
        }
        if live {
            let c = ed.hsv.to_rgb();
            self.actions.push(Action::Recolor { slot, rgba: [c[0], c[1], c[2], self.palette[slot as usize][3]] });
        }
        if commit {
            self.actions.push(Action::CommitColor);
        }
        if let Some((material, done)) = new_mat {
            self.actions.push(Action::SetMaterial { slot, material });
            if done {
                self.actions.push(Action::CommitColor);
            }
        }
        if !close {
            self.color_edit = Some(ed);
        }
    }

    // #welcome
    fn coach_card(&mut self, ctx: &Context, screen: Rect) {
        let w = 440.0_f32.min(screen.width() - 32.0);
        let text_w = w - 32.0 - 14.0 - 19.0;
        let mut jobs = Vec::new();
        let touch: [&[(&str, bool)]; 3] = [
            &[("Tap", true), (" a surface to add a voxel. ", false), ("Drag", true), (" to draw along it.", false)],
            &[
                ("Two fingers", true),
                (" orbit, ", false),
                ("pinch", true),
                (" to zoom, ", false),
                ("three fingers", true),
                (" pan.", false),
            ],
            &[("Tap with two fingers", true), (" to undo.", false)],
        ];
        let mouse: [&[(&str, bool)]; 3] = [
            &[
                ("Click", true),
                (" a surface to add a voxel. ", false),
                ("Drag", true),
                (" to draw along that surface.", false),
            ],
            &[
                ("Right-drag", true),
                (" to orbit, ", false),
                ("scroll", true),
                (" to zoom, ", false),
                ("Shift+right-drag", true),
                (" to pan.", false),
            ],
            &[
                ("Hold ", false),
                ("Shift", true),
                (" to erase, ", false),
                ("Alt", true),
                (" to pick a color. ", false),
                ("?", true),
                (" for every key.", false),
            ],
        ];
        for line in if self.touch { touch } else { mouse } {
            let mut job = LayoutJob::default();
            for (t, b) in line {
                if *t == "?" {
                    job.append(
                        "?",
                        5.0,
                        TextFormat {
                            font_id: mono_medium(10.5),
                            color: th().muted,
                            line_height: Some(17.5),
                            ..Default::default()
                        },
                    );
                    continue;
                }
                let lead = if t.starts_with(" for every") { 5.0 } else { 0.0 };
                let f = if *b {
                    TextFormat {
                        font_id: sans_medium(12.5),
                        color: th().text,
                        line_height: Some(17.5),
                        ..Default::default()
                    }
                } else {
                    TextFormat { font_id: sans(12.5), color: th().muted, line_height: Some(17.5), ..Default::default() }
                };
                job.append(t, lead, f);
            }
            job.wrap.max_width = text_w - 18.0;
            jobs.push(job);
        }
        let galleys: Vec<_> = jobs.into_iter().map(|j| ctx.fonts_mut(|f| f.layout_job(j))).collect();
        let body: f32 = galleys.iter().map(|g| g.size().y + 4.0).sum();
        let h = 14.0 + 19.6 + 4.0 + 8.0 - 4.0 + body + 14.0;
        let rect = Rect::from_min_size(pos2(screen.center().x - w / 2.0, 70.0), vec2(w, h));
        Self::panel(ctx, "welcome", rect, Order::Middle, |ui, rect| {
            let p = ui.painter().clone();
            let left = rect.left() + 16.0;
            p.text(
                pos2(left, rect.top() + 14.0 + 9.8),
                Align2::LEFT_CENTER,
                "Sculpt in under a minute",
                sans_semibold(14.0),
                th().text,
            );
            let mut y = rect.top() + 14.0 + 19.6 + 4.0 + 8.0;
            for (i, g) in galleys.into_iter().enumerate() {
                y += 2.0;
                p.text(
                    pos2(left + 14.0, y + 8.75),
                    Align2::RIGHT_CENTER,
                    format!("{}.", i + 1),
                    sans(12.5),
                    th().muted,
                );
                let h = g.size().y;
                kbd_frames(&p, pos2(left + 18.0, y), &g, &['?']);
                p.galley(pos2(left + 18.0, y), g, th().muted);
                y += h + 2.0;
            }
            let x = Rect::from_center_size(pos2(rect.right() - 16.0 - 8.0, rect.top() + 14.0 + 11.0), vec2(16.0, 22.0));
            let resp = ui.interact(x, Id::unique("welcome-x"), Sense::click());
            p.text(
                x.center(),
                Align2::CENTER_CENTER,
                "×",
                sans(18.0),
                if resp.hovered() { th().text } else { th().faint },
            );
            if resp.clicked() {
                self.coach = false;
            }
        });
    }

    // #stats
    fn stats(&mut self, ctx: &Context, screen: Rect, s: &Stats) {
        let fps_color = if s.fps >= 55.0 {
            th().ok
        } else if s.fps >= 30.0 {
            th().mid
        } else {
            th().danger
        };
        let cols = [
            ("VOXELS", group_thousands(s.voxels), th().text),
            ("FPS", if s.fps > 0.0 { format!("{:.0}", s.fps) } else { "-".into() }, fps_color),
            ("FRAME", format!("{:.1}ms", s.frame_ms), th().text),
            match &s.span {
                _ if s.selected > 0 && s.span.is_none() => {
                    ("SELECTED", group_thousands(s.selected as u64), th().accent)
                }
                Some((label, v)) => (*label, v.clone(), th().text),
                None => (
                    "CURSOR",
                    s.cursor.map(|c| format!("{},{},{}", c[0], c[1], c[2])).unwrap_or_else(|| "-".into()),
                    th().text,
                ),
            },
        ];
        // One 28 px row: each label beside its value.
        let galleys: Vec<_> = cols
            .iter()
            .map(|(l, v, c)| {
                let lg = ctx.fonts_mut(|f| f.layout_job(spaced(l, mono(9.0), th().faint, 0.72)));
                let vg = ctx.fonts_mut(|f| f.layout_no_wrap(v.clone(), mono(11.5), *c));
                (lg, vg)
            })
            .collect();
        let w: f32 = 20.0
            + galleys.iter().map(|(l, v)| l.size().x + 5.0 + v.size().x).sum::<f32>()
            + 12.0 * (cols.len() - 1) as f32;
        let h = 28.0;
        let rect = Rect::from_min_size(pos2(12.0, screen.bottom() - 12.0 - h), vec2(w, h));
        self.stats_right = rect.right();
        self.hits.insert("stats".into(), rect);
        Self::panel(ctx, "stats", rect, Order::Middle, |ui, rect| {
            let p = ui.painter();
            let cy = rect.center().y;
            let mut x = rect.left() + 10.0;
            for (lg, vg) in galleys {
                p.galley(pos2(x, cy - lg.size().y / 2.0), lg.clone(), th().faint);
                x += lg.size().x + 5.0;
                p.galley(pos2(x, cy - vg.size().y / 2.0), vg.clone(), th().text);
                x += vg.size().x + 12.0;
            }
        });
    }

    // The live render corner (direction D, pin 4: right:1%, bottom:12%, width:24%, a label
    // chip at .5em). The editor writes the path-traced image into the rect under the UI, so the
    // panel has a border but no fill. 16:9, the preview's own shape, and it sits above #history.
    // A click on it swaps the image into the whole view (MagicaVoxel's Render tab); there the
    // chips stay where the corner's bottom edge was and the view takes the pointer as usual.
    fn render_corner(&mut self, ctx: &Context, screen: Rect, s: &Stats) {
        self.render_rect = None;
        self.render_dock = None;
        let mut open = false;
        let mut swap = false;
        let Some((spp, live)) = s.render.filter(|_| self.show_render) else { return };
        let w = 208.0;
        let size = vec2(w, (w * 9.0 / 16.0).round());
        let full = self.full_render;
        // Over the dock the corner moves up under the top bar, and a portrait screen leaves
        // it out (the full render view still shows).
        if self.touch && screen.height() > screen.width() && !full {
            return;
        }
        let top = if self.touch { TOP } else { screen.bottom() - 12.0 - size.y };
        let corner = Rect::from_min_size(pos2(screen.right() - 12.0 - size.x, top), size);
        // A phone held sideways with the assistant open has no room for it beside the dock.
        if self.phone && !full && corner.intersects(self.dock_area.expand(8.0)) {
            return;
        }
        let rect = if full { screen } else { corner };
        self.render_rect = Some(rect);
        self.render_dock = Some(corner);
        self.hits.insert("render".into(), rect);
        let label = match s.export {
            Some(((w, h), done, of)) => format!("Exporting {w}×{h} · {:.0}/{of} spp", done.floor()),
            None => format!("{:.0} spp{}", spp.floor(), if live { " · live" } else { "" }),
        };
        let g = ctx.fonts_mut(|f| f.layout_no_wrap(label, sans_medium(10.5), th().text));
        let chip_size = g.size() + vec2(14.0, 6.0);
        // The chips' row: the corner's top, or its bottom edge in the full view.
        let top = if full { corner.bottom() - 6.0 - chip_size.y } else { corner.top() + 6.0 };
        let row = Rect::from_min_max(pos2(corner.left(), top - 8.0), pos2(corner.right(), top + chip_size.y + 8.0));
        let area = if full { row } else { corner };
        Area::new(Id::unique("render")).fade_in(false).order(Order::Middle).fixed_pos(area.min).constrain(false).show(
            ctx,
            |ui| {
                if full {
                    // Only the chips' row keeps the pointer from the view.
                    ui.allocate_rect(row, Sense::hover());
                } else {
                    // Click-to-swap; the chips below are on top of it.
                    let r = ui.allocate_rect(corner, Sense::click());
                    swap |= r.clicked();
                    if r.hovered() {
                        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
                    }
                    ui.painter().rect_stroke(
                        corner,
                        CornerRadius::same(R),
                        Stroke::new(1.0, th().line2),
                        StrokeKind::Inside,
                    );
                }
                let chip = Rect::from_min_size(pos2(corner.left() + 6.0, top), chip_size);
                let p = ui.painter();
                p.rect_filled(chip, CornerRadius::same(6), th().scrim(140));
                p.galley(chip.min + vec2(7.0, 3.0), g, th().text);
                // The Render panel's button and the swap button, chips at the other corner.
                let b = Rect::from_min_size(pos2(corner.right() - 6.0 - 24.0, top), vec2(24.0, chip.height()));
                let f = b.translate(vec2(-24.0 - 4.0, 0.0));
                let glyph = if full { icon::MINIMIZE_2 } else { icon::MAXIMIZE_2 };
                for (rect, id, glyph, on) in
                    [(f, "render-full", glyph, false), (b, "render-settings", icon::APERTURE, self.render_panel)]
                {
                    let r = ui.interact(rect, Id::unique(id), Sense::click());
                    let fill = if r.hovered() { th().scrim(190) } else { th().scrim(140) };
                    let p = ui.painter();
                    p.rect_filled(rect, CornerRadius::same(6), fill);
                    if on {
                        p.rect_filled(rect, CornerRadius::same(6), th().accent_bg);
                    }
                    let c = if on {
                        th().accent
                    } else if r.hovered() {
                        th().text
                    } else {
                        th().muted
                    };
                    p.text(rect.center(), Align2::CENTER_CENTER, glyph, icon_font(12.0), c);
                    if r.clicked() {
                        if id == "render-full" {
                            swap = true;
                        } else {
                            open = true;
                        }
                    }
                    self.hits.insert(id.into(), rect);
                }
            },
        );
        if open {
            self.toggle_render_panel();
        }
        if swap {
            self.toggle_full_render();
        }
    }

    // The Render panel: not in the mockups (the board lists Render as "next"). A .panel left of
    // the render corner, bottom-aligned with it, built from the colour editor's parts.
    fn render_panel(&mut self, ctx: &Context, screen: Rect, s: &Stats) {
        if !self.render_panel {
            return;
        }
        let w = 232.0;
        let row = 34.0;
        let h = 12.0 + 15.0 + 10.0 + 9.0 * row + 28.0 + 8.0 + 28.0 + 12.0 + 2.0 * (14.0 + 28.0 + 10.0) + 32.0 + 12.0;
        let right = self.render_dock.map_or(screen.right() - 12.0 - PALETTE_W, |r| r.left()) - 8.0;
        let bottom = self.render_dock.map_or(screen.bottom() - 62.0, |r| r.bottom());
        let rect = Rect::from_min_size(pos2(right - w, (bottom - h).max(TOP)), vec2(w, h));
        self.hits.insert("render-panel".into(), rect);
        let mut rs = self.render.clone();
        let mut hits = Vec::new();
        let (mut close, mut export) = (false, false);
        let mut hex_text = std::mem::take(&mut self.sun_hex);
        Self::panel(ctx, "render-panel", rect, Order::Middle, |ui, rect| {
            let inner = rect.shrink(12.0);
            let p = ui.painter().clone();
            let hy = inner.top() + 7.5;
            let g = p.layout_job(spaced("RENDER", sans_semibold(11.0), th().muted, 0.88));
            p.galley(pos2(inner.left(), hy - g.size().y / 2.0), g, th().muted);
            let x = Rect::from_center_size(pos2(inner.right() - 6.0, hy), vec2(16.0, 22.0));
            let xr = ui.interact(x, Id::unique("render-panel-x"), Sense::click());
            hits.push(("render-panel-x", x));
            p.text(
                x.center(),
                Align2::CENTER_CENTER,
                "×",
                sans(18.0),
                if xr.hovered() { th().text } else { th().faint },
            );
            close |= xr.clicked();
            p.text(pos2(x.left() - 6.0, hy), Align2::RIGHT_CENTER, "Path trace", mono(11.0), th().faint);
            let mut y = inner.top() + 25.0;
            let mut next = |hh: f32| {
                let r = Rect::from_min_size(pos2(inner.left(), y), vec2(inner.width(), hh));
                y += hh;
                r
            };
            let sliders: [SliderRow; 9] = [
                ("rs-sun-angle", "Sun direction", &mut rs.sun_angle, 0.0, 360.0, |v| format!("{v:.0}°")),
                ("rs-sun-height", "Sun height", &mut rs.sun_height, 2.0, 90.0, |v| format!("{v:.0}°")),
                ("rs-sun-strength", "Sun strength", &mut rs.sun_strength, 0.0, 10.0, |v| format!("{v:.1}")),
                ("rs-sky", "Sky light", &mut rs.sky, 0.0, 3.0, |v| format!("{v:.2}")),
                ("rs-exposure", "Exposure", &mut rs.exposure_ev, -3.0, 3.0, |v| format!("{v:+.1} EV")),
                ("rs-gamma", "Gamma", &mut rs.gamma, 1.0, 3.0, |v| format!("{v:.2}")),
                ("rs-vignette", "Vignette", &mut rs.vignette, 0.0, 1.0, |v| {
                    if v <= 0.0 { "off".into() } else { format!("{:.0}%", v * 100.0) }
                }),
                ("rs-blur", "Depth of field", &mut rs.blur, 0.0, 1.0, |v| {
                    if v <= 0.0 { "off".into() } else { format!("{:.0}%", v * 100.0) }
                }),
                ("rs-focus", "Focus", &mut rs.focus, 0.25, 2.0, |v| format!("{v:.2}× target")),
            ];
            for (id, label, v, lo, hi, fmt) in sliders {
                let r = next(row);
                hits.push((id, r));
                slider(ui, r, id, label, v, lo, hi, fmt);
            }
            // Sun colour: a chip and a hex field.
            let r = next(28.0);
            let chip = Rect::from_min_size(r.min, vec2(28.0, 28.0));
            let c = rs.sun_color;
            p.rect(
                chip,
                CornerRadius::same(7),
                Color32::from_rgb(c[0], c[1], c[2]),
                Stroke::new(1.0, th().tint(38)),
                StrokeKind::Inside,
            );
            let fr = Rect::from_min_max(pos2(chip.right() + 6.0, r.top()), r.max);
            let id = Id::unique("rs-sun-color");
            if !ui.memory(|m| m.has_focus(id)) {
                hex_text = hex(c);
            }
            hits.push(("rs-sun-color", fr));
            if field(ui, fr, "SUN", id, &mut hex_text)
                && let Some(v) = parse_hex(hex_text.trim())
            {
                rs.sun_color = v;
            }
            next(8.0);
            // The sky backdrop and physical sky toggles, side by side.
            let r = next(28.0);
            let half = (r.width() - 6.0) / 2.0;
            let (a, b) = (
                Rect::from_min_size(r.min, vec2(half, r.height())),
                Rect::from_min_size(pos2(r.right() - half, r.top()), vec2(half, r.height())),
            );
            hits.push(("rs-sky-backdrop", a));
            if small_button(ui, a, icon::SUN, "Show sky", "rs-sky-backdrop", rs.sky_backdrop).clicked() {
                rs.sky_backdrop = !rs.sky_backdrop;
            }
            hits.push(("rs-physical-sky", b));
            if small_button(ui, b, icon::SUNSET, "Physical sky", "rs-physical-sky", rs.physical_sky).clicked() {
                rs.physical_sky = !rs.physical_sky;
            }
            next(12.0);
            // Samples (where the corner stops, and what an export traces) and export size.
            let spp_labels = SPP_CHOICES.map(|n| if n >= 1024 { format!("{}K", n / 1024) } else { n.to_string() });
            let size_labels = EXPORT_SIZES.map(|(_, h)| if h >= 2160 { "4K".to_string() } else { format!("{h}p") });
            let n = s.export.map_or(String::new(), |_| " (exporting)".into());
            for (name, names, label, labels, sel, ok) in [
                (
                    "rs-spp",
                    ["rs-spp0", "rs-spp1", "rs-spp2", "rs-spp3"],
                    format!("Samples{n}"),
                    &spp_labels,
                    &mut rs.spp,
                    [true; 4],
                ),
                (
                    "rs-size",
                    ["rs-size0", "rs-size1", "rs-size2", "rs-size3"],
                    "Export size".to_string(),
                    &size_labels,
                    &mut rs.size,
                    s.export_fits,
                ),
            ] {
                let lr = next(14.0);
                p.text(lr.left_center(), Align2::LEFT_CENTER, label, sans(12.0), th().muted);
                if name == "rs-size" {
                    let (w, h) = EXPORT_SIZES[*sel];
                    p.text(lr.right_center(), Align2::RIGHT_CENTER, format!("{w}×{h}"), mono(11.0), th().faint);
                }
                let r = next(28.0);
                if let Some(k) = segmented(ui, r, &names, labels, *sel, &ok, mono(11.5), &mut hits) {
                    *sel = k;
                }
                next(10.0);
            }
            // Export PNG, the panel's one primary action (the Embed button's look).
            let r = next(32.0);
            hits.push(("rs-export", r));
            let resp = ui.interact(r, Id::unique("rs-export"), Sense::click());
            let ink = th().on_accent;
            let p = ui.painter();
            let label = match s.export {
                Some((_, done, of)) => {
                    p.rect(r, CornerRadius::same(8), th().accent_bg, Stroke::new(1.0, th().line2), StrokeKind::Inside);
                    let k = (done / of as f32).clamp(0.0, 1.0);
                    let fill = Rect::from_min_size(r.min, vec2(r.width() * k, r.height()));
                    p.rect_filled(fill, CornerRadius::same(8), th().accent.gamma_multiply(0.45));
                    format!("Cancel export · {:.0}%", k * 100.0)
                }
                None => {
                    p.rect_filled(
                        r,
                        CornerRadius::same(8),
                        if resp.hovered() { th().accent_hover } else { th().accent },
                    );
                    "Export PNG".to_string()
                }
            };
            let color = if s.export.is_some() { th().text } else { ink };
            let g = p.layout_no_wrap(label, sans_semibold(13.0), color);
            let total = 16.0 + 6.0 + g.size().x;
            let x0 = r.center().x - total / 2.0;
            p.text(pos2(x0 + 8.0, r.center().y), Align2::CENTER_CENTER, icon::IMAGE_DOWN, icon_font(16.0), color);
            p.galley(pos2(x0 + 22.0, r.center().y - g.size().y / 2.0), g, color);
            export |= resp.clicked();
        });
        for (k, r) in hits {
            self.hits.insert(k.into(), r);
        }
        self.sun_hex = hex_text;
        self.render = rs;
        if export {
            self.actions.push(if s.export.is_some() { Action::CancelExport } else { Action::ExportPng });
        }
        if close {
            self.render_panel = false;
        }
    }

    // #hint
    fn hint_bar(&mut self, ctx: &Context, screen: Rect) {
        let t = self.effective_tool();
        let armed;
        let mut items: Vec<(&str, &str)> = match (self.slot_op, t, self.shape) {
            (Some(op), _, _) => {
                armed = match op {
                    SlotOp::Swap => format!("a slot to swap with slot {}", self.slot),
                    SlotOp::Copy => format!("a slot to copy slot {} to", self.slot),
                };
                vec![("Click", armed.as_str()), ("Esc", "cancel")]
            }
            (None, _, _) if self.review.is_some() => {
                let v = [("↵", "apply"), ("Esc", "discard"), ("Right-drag", "orbit"), ("Scroll", "zoom")];
                return self.hint_panel(ctx, screen, &v);
            }
            (None, _, _) if self.camera_hints => {
                let v = [
                    ("Right-drag", "orbit"),
                    ("Space-drag", "pan"),
                    ("Middle-drag", "pan"),
                    ("Scroll", "zoom"),
                    ("X-click", "orbit there"),
                    ("Period", "frame"),
                    ("Home", "reset"),
                ];
                return self.hint_panel(ctx, screen, &v);
            }
            (None, _, _) if self.lifting => {
                let v = [("Move", "set the height"), ("Click or ↵", "place"), ("Esc", "cancel")];
                return self.hint_panel(ctx, screen, &v);
            }
            (None, _, _) if self.world => {
                let mut v = vec![("Click", "pick an object"), ("Drag", "move it")];
                if self.picked.is_some() {
                    v.extend([("Arrows", "nudge"), ("⌘D", "duplicate"), ("⌘⇧D", "linked copy"), ("⌫", "delete")]);
                }
                v.extend([("Tab", "edit its model"), ("Right-drag", "orbit")]);
                return self.hint_panel(ctx, screen, &v);
            }
            (None, t, shape) => self.tool_hints(t, shape),
        };
        let size = format!("size {}", self.size);
        if self.slot_op.is_none() && self.shape == Shape::Voxel && matches!(t, Tool::Add | Tool::Erase | Tool::Paint) {
            items.push(("- =", size.as_str()));
        }
        if self.slot_op.is_none() && t == Tool::Select {
            items.extend([("Shift", "add"), ("Alt", "subtract"), ("Right-drag", "orbit")]);
        } else if self.slot_op.is_none() {
            items.extend([("Shift", "erase"), ("Alt", "pick"), ("Right-drag", "orbit")]);
            if t == Tool::Erase && self.shift {
                let n = items.len();
                items.remove(n - 3);
            }
        }
        self.hint_panel(ctx, screen, &items);
    }

    fn tool_hints(&self, t: Tool, shape: Shape) -> Vec<(&'static str, &'static str)> {
        match (t, shape) {
            (Tool::Add, Shape::Voxel) => vec![("Click", "add"), ("Drag", "draw on that plane")],
            (Tool::Add, Shape::Box) => vec![("Drag", "draw a rectangle, then lift it")],
            (Tool::Add, Shape::Face) => vec![("Click", "extrude the whole face")],
            (Tool::Erase, Shape::Voxel) => vec![("Click", "erase"), ("Drag", "erase as you go")],
            (Tool::Erase, Shape::Box) => vec![("Drag", "clear a rectangle, then deepen it")],
            (Tool::Erase, Shape::Face) => vec![("Click", "peel the whole face")],
            (Tool::Paint, Shape::Voxel) => vec![("Click", "paint"), ("Drag", "paint as you go")],
            (Tool::Paint, Shape::Box) => vec![("Drag", "paint a rectangle, then deepen it")],
            (Tool::Paint, Shape::Face) => vec![("Click", "fill the connected face")],
            (Tool::Add, Shape::Line) => vec![("Drag", "draw a line")],
            (Tool::Erase, Shape::Line) => vec![("Drag", "erase along a line")],
            (Tool::Paint, Shape::Line) => vec![("Drag", "paint along a line")],
            (Tool::Add, Shape::Circle) => vec![("Drag", "grow a circle from its center"), ("C", "sphere")],
            (Tool::Erase, Shape::Circle) => vec![("Drag", "clear a circle"), ("C", "sphere")],
            (Tool::Paint, Shape::Circle) => vec![("Drag", "paint a circle"), ("C", "sphere")],
            (Tool::Add, Shape::Sphere) => vec![("Drag", "grow a sphere from its center"), ("C", "circle")],
            (Tool::Erase, Shape::Sphere) => vec![("Drag", "carve a sphere"), ("C", "circle")],
            (Tool::Paint, Shape::Sphere) => vec![("Drag", "paint a sphere"), ("C", "circle")],
            (Tool::Pick, _) => vec![("Click", "take that color")],
            (Tool::Select, _) if self.selected > 0 => {
                vec![("Drag", "box select"), ("Drag selection", "move it"), ("Arrows", "nudge"), ("⌫", "delete")]
            }
            (Tool::Select, _) => vec![("Drag", "box select"), ("Click", self.region_hint())],
        }
    }

    fn region_hint(&self) -> &'static str {
        match self.region {
            Region::Color => "select that color",
            Region::Face => "select that face",
            Region::Volume => "select that volume",
        }
    }

    fn hint_panel(&mut self, ctx: &Context, screen: Rect, items: &[(&str, &str)]) {
        let mut job = LayoutJob::default();
        for (i, (k, v)) in items.iter().enumerate() {
            let lead = if i == 0 { 0.0 } else { 14.0 };
            job.append(
                k,
                lead,
                TextFormat {
                    font_id: sans_medium(12.0),
                    color: th().text,
                    line_height: Some(16.8),
                    ..Default::default()
                },
            );
            job.append(
                v,
                5.0,
                TextFormat { font_id: sans(12.0), color: th().muted, line_height: Some(16.8), ..Default::default() },
            );
        }
        let g = ctx.fonts_mut(|f| f.layout_job(job));
        // A connected terminal agent shows at the left, with Stop, on a card; the hints alone
        // are bare text over the view, with a dark edge so they read over a light model.
        let chip = self.agent_chip_width(ctx);
        let size = vec2(chip + g.size().x + 28.0, 16.8 + 16.0);
        // Centred on the view, kept clear of the stats, the render corner and the camera bar.
        let row_top = screen.bottom() - 12.0 - size.y;
        let right = [
            self.render_dock.filter(|_| !self.full_render),
            self.hits.get("render-panel").copied(),
            self.nav_rect.filter(|r| r.bottom() > row_top),
        ]
        .into_iter()
        .flatten()
        .fold(screen.right() - 12.0, |x, r| x.min(r.left()));
        let (lo, hi) = (self.stats_right + 8.0, right - 8.0);
        let rect = if size.x <= hi - lo {
            let x = (screen.center().x - size.x / 2.0).min(hi - size.x).max(lo);
            Rect::from_min_size(pos2(x.round(), row_top), size)
        } else {
            // Too wide for the gap (the assistant is docked): a row up, over the stats.
            let stats_top = self.hits.get("stats").map_or(screen.bottom() - 12.0, |r| r.top());
            Rect::from_min_size(pos2(screen.left() + 12.0, stats_top - 4.0 - size.y), size)
        };
        self.hits.insert("hint".into(), rect);
        if chip > 0.0 {
            Self::panel(ctx, "hint", rect, Order::Middle, |ui, rect| {
                self.agent_chip(ui, rect, chip);
                ui.painter().galley(pos2(rect.left() + chip + 14.0, rect.top() + 8.0), g, th().muted);
            });
            return;
        }
        Area::new(Id::unique("hint")).fade_in(false).order(Order::Middle).fixed_pos(rect.min).constrain(false).show(
            ctx,
            |ui| {
                let at = pos2(rect.left() + 14.0, rect.top() + 8.0);
                let p = ui.painter();
                p.galley_with_override_text_color(at + vec2(0.0, 1.0), g.clone(), th().text_shadow);
                p.galley(at, g, th().muted);
            },
        );
    }

    // The camera bar: above the render corner (or in the bottom right corner when it is hidden),
    // level with its right edge, so the camera's moves are always one click away. Projection, the
    // View flyout and the ways back to the scene, each with a tooltip.
    fn nav_bar(&mut self, ctx: &Context, screen: Rect) {
        let proj = match (self.ortho, self.view) {
            (_, Some(View::Iso)) => "Iso",
            (true, _) => "Ortho",
            (false, _) => "Persp",
        };
        let pg = ctx.fonts_mut(|f| f.layout_no_wrap(proj.into(), sans_medium(12.0), th().muted));
        let pw = (pg.size().x + 20.0).max(52.0);
        let sel = self.selected > 0;
        let len = 4.0 + 34.0 + 9.0 + (2.0 + f32::from(u8::from(sel))) * 34.0 - 2.0 + 4.0;
        // Over the dock the bar stands on the right edge, under the render corner.
        let vertical = self.touch;
        let rect = if vertical {
            let size = vec2(4.0 + pw + 4.0, len + 32.0 + 2.0);
            let below = self.render_dock.map_or(TOP, |r| r.bottom() + 8.0);
            let right = screen.right() - 12.0 - size.x;
            let under = Rect::from_min_size(pos2(right, (screen.center().y - size.y / 2.0).max(below)), size);
            if self.phone {
                // On a phone: under the corner, else beside it, else at the top; where the dock
                // or the screen's edge leaves no room the bar stays away.
                let mut places = vec![under];
                if let Some(c) = self.render_dock {
                    places.push(Rect::from_min_size(pos2(c.left() - 8.0 - size.x, c.top()), size));
                }
                places.push(Rect::from_min_size(pos2(right, TOP), size));
                let clear = |r: &Rect| {
                    r.bottom() <= screen.bottom() - 12.0
                        && r.left() >= screen.left() + 12.0
                        && !r.intersects(self.dock_area.expand(8.0))
                };
                match places.into_iter().find(clear) {
                    Some(r) => r,
                    None => {
                        self.nav_rect = None;
                        return;
                    }
                }
            } else {
                under
            }
        } else {
            let w = len + pw + 2.0;
            let size = vec2(w, 40.0);
            let dock = self.render_dock.unwrap_or_else(|| {
                Rect::from_min_max(
                    pos2(screen.right() - 12.0, screen.bottom() - 12.0),
                    pos2(screen.right() - 12.0, screen.bottom() - 12.0),
                )
            });
            let palette_bottom = self.hits.get("palette").map_or(screen.top(), |r| r.bottom());
            let gap = if self.render_dock.is_some() { 8.0 } else { 0.0 };
            let mut rect = Rect::from_min_size(pos2(dock.right() - w, dock.top() - gap - size.y), size);
            if rect.top() < palette_bottom + 8.0 {
                // A short window: beside the corner instead, level with its bottom.
                rect = Rect::from_min_size(pos2(dock.left() - 8.0 - w, dock.bottom() - size.y), size);
            }
            rect
        };
        self.hits.insert("nav".into(), rect);
        self.nav_rect = Some(rect);
        let view_open = self.view_panel;
        let mut run = Vec::new();
        let mut tip: Option<(Rect, &'static str, &'static [&'static str])> = None;
        let mut hits = Vec::new();
        let mut toggle_view = false;
        Self::panel(ctx, "nav", rect, Order::Middle, |ui, rect| {
            let (cx, cy) = (rect.center().x, rect.center().y);
            // How far along the bar: across it, or down it when it stands.
            let mut x = if vertical { rect.top() + 4.0 } else { rect.left() + 4.0 };
            let at = |x: f32, w: f32| {
                if vertical {
                    Rect::from_min_size(pos2(cx - w / 2.0, x), vec2(w, 32.0))
                } else {
                    Rect::from_min_size(pos2(x, cy - 16.0), vec2(w, 32.0))
                }
            };
            let sep = |ui: &Ui, x: &mut f32| {
                *x += 4.0;
                let line = if vertical {
                    [pos2(cx - 10.0, *x + 0.5), pos2(cx + 10.0, *x + 0.5)]
                } else {
                    [pos2(*x + 0.5, cy - 10.0), pos2(*x + 0.5, cy + 10.0)]
                };
                ui.painter().line_segment(line, Stroke::new(1.0, th().line));
                *x += 5.0;
            };
            let ortho = proj != "Persp";
            let (proj_tip, proj_keys): (&'static str, &'static [&'static str]) =
                if ortho { ("Perspective", &["O"]) } else { ("Orthographic", &["O"]) };
            let b = at(x, pw);
            let r = nav_button(ui, b, None, Some(pg), "nav-proj", true, false);
            if r.clicked() {
                run.push(Action::Ortho(!ortho));
            }
            if r.hovered() {
                tip = Some((b, proj_tip, proj_keys));
            }
            hits.push(("nav-proj", b));
            x += if vertical { 32.0 } else { pw } + 2.0;
            // Zoom is the wheel, pinch and Cmd-= / Cmd--; Frame selection shows while there is one.
            let buttons: [(&str, char, &str, &[&str], bool); 4] = [
                ("nav-views", icon::AXIS_3D, "Views", &["0–6"], true),
                ("nav-frame-sel", icon::SCAN_SEARCH, "Frame selection", &[], sel),
                ("nav-frame", icon::SCAN, "Frame scene", &["."], true),
                ("nav-reset", icon::ROTATE_CCW, "Reset camera", &["Home"], true),
            ];
            let mut need_sep = true;
            for (id, glyph, label, keys, shown) in buttons {
                if !shown {
                    continue;
                }
                if id != "nav-views" && std::mem::take(&mut need_sep) {
                    sep(ui, &mut x);
                }
                let b = at(x, 32.0);
                let on = id == "nav-views" && view_open;
                let r = nav_button(ui, b, Some(glyph), None, id, true, on);
                if r.hovered() {
                    tip = Some((b, label, keys));
                }
                if r.clicked() {
                    match id {
                        "nav-views" => toggle_view = true,
                        "nav-frame-sel" => run.push(Action::FrameSelection),
                        "nav-frame" => run.push(Action::Frame),
                        _ => run.push(Action::ResetCamera),
                    }
                }
                hits.push((id, b));
                x += 32.0 + 2.0;
            }
        });
        if toggle_view {
            self.view_panel = !self.view_panel;
        }
        self.hits.extend(hits.into_iter().map(|(n, r)| (n.to_string(), r)));
        self.actions.extend(run);
        if let Some((at, label, keys)) = tip {
            // Above the button, unless that crowds the palette: then below, over the render
            // corner, and not at all while the View flyout is open.
            let palette = self.hits.get("palette").copied().unwrap_or(Rect::NOTHING);
            let shown = tooltip(ctx, at, label, keys, Some((palette.expand(4.0), !view_open)));
            self.hits.extend(shown.map(|r| ("nav-tip".to_string(), r)));
        }
    }

    // #toast
    fn toast_panel(&mut self, ctx: &Context, screen: Rect) {
        let Some((msg, until)) = &self.toast else { return };
        let now = ctx.input(|i| i.time);
        if now > *until {
            self.toast = None;
            return;
        }
        ctx.request_repaint_after(std::time::Duration::from_secs_f64(until - now));
        let g = ctx.fonts_mut(|f| f.layout_no_wrap(msg.clone(), sans(12.5), th().text));
        let size = vec2(g.size().x + 28.0, g.size().y + 18.0);
        // Centred on the view, or on the part of it left of the review card (or the Agent edits
        // card) when that shows.
        let card = self.hits.get("review").filter(|_| self.review.is_some()).or_else(|| self.hits.get("agent-card"));
        let mut cx = card.map_or(screen.center().x, |c| (12.0 + RAIL_W + 12.0 + c.left()) / 2.0);
        let mut top = self.ai.bar_bottom.map_or(TOP, |b| b + 8.0);
        // Under the Terminal agents panel while it is open, whose own toasts it would cover.
        if let Some(p) = self.hits.get("agents-panel").filter(|_| self.agents_panel) {
            (cx, top) = (p.center().x, p.bottom() + 8.0);
        }
        let rect = Rect::from_min_size(pos2((cx - size.x / 2.0).round(), top), size);
        Self::panel(ctx, "toast", rect, Order::Foreground, |ui, rect| {
            ui.painter().galley(pos2(rect.left() + 14.0, rect.top() + 9.0), g, th().text);
        });
    }

    // Cmd-K: not in the mockups; styled like #help (a centred .panel).
    fn cmdk(&mut self, ctx: &Context, screen: Rect) {
        let query = self.cmdk.clone().unwrap_or_default().to_lowercase();
        let hits: Vec<usize> =
            (0..COMMANDS.len()).filter(|&i| COMMANDS[i].label.to_lowercase().contains(&query)).collect();
        self.cmdk_sel = self.cmdk_sel.min(hits.len().saturating_sub(1));
        let (mut run, mut close, mut ask) = (None, false, false);
        ctx.input_mut(|i| {
            if i.consume_key(Modifiers::NONE, Key::Escape) {
                close = true;
            }
            if i.consume_key(Modifiers::NONE, Key::Tab) {
                ask = true;
            }
            if i.consume_key(Modifiers::NONE, Key::ArrowDown) {
                self.cmdk_sel = (self.cmdk_sel + 1).min(hits.len().saturating_sub(1));
            }
            if i.consume_key(Modifiers::NONE, Key::ArrowUp) {
                self.cmdk_sel = self.cmdk_sel.saturating_sub(1);
            }
            if i.consume_key(Modifiers::NONE, Key::Enter) {
                run = hits.get(self.cmdk_sel).copied();
                ask |= hits.is_empty();
            }
        });
        let shown = hits.len().min(8);
        let w = 560.0_f32.min(screen.width() - 32.0);
        let h = 8.0 + 40.0 + 9.0 + shown.max(1) as f32 * 34.0 + 8.0;
        let rect =
            Rect::from_min_size(pos2(screen.center().x - w / 2.0, (screen.height() * 0.18).max(70.0)), vec2(w, h));
        self.hits.insert("cmdk".into(), rect);
        Self::panel(ctx, "cmdk", rect, Order::Foreground, |ui, rect| {
            let field = Rect::from_min_size(rect.min + vec2(8.0, 8.0), vec2(rect.width() - 16.0, 40.0));
            ui.painter().text(
                pos2(field.left() + 18.0, field.center().y),
                Align2::CENTER_CENTER,
                icon::SEARCH,
                icon_font(16.0),
                th().muted,
            );
            // Tab asks AI, from any row.
            let kx = field.right() - 10.0;
            let ag = ui.painter().layout_no_wrap("Ask AI".into(), sans(12.0), th().faint);
            let aw = ag.size().x;
            ui.painter().galley(pos2(kx - aw, field.center().y - ag.size().y / 2.0), ag, th().faint);
            let tw = kbd(ui, pos2(kx - aw - 6.0, field.center().y), "Tab", true);
            let text_rect = Rect::from_min_max(
                pos2(field.left() + 36.0, field.top() + 10.0),
                pos2(kx - aw - 6.0 - tw - 10.0, field.bottom() - 10.0),
            );
            let q = self.cmdk.as_mut().unwrap();
            let edit = egui::TextEdit::singleline(q)
                .id(Id::unique("cmdk-field"))
                .font(sans(14.0))
                .frame(egui::Frame::NONE)
                .hint_text(egui::RichText::new("Type a command").color(th().faint).font(sans(14.0)))
                .desired_width(text_rect.width());
            let resp = ui.put(text_rect, edit);
            resp.request_focus();
            // Typing starts again from the best match, whatever row the pointer rests on.
            if resp.changed() {
                self.cmdk_sel = 0;
            }
            let sep_y = field.bottom() + 4.5;
            ui.painter().line_segment(
                [pos2(rect.left() + 1.0, sep_y), pos2(rect.right() - 1.0, sep_y)],
                Stroke::new(1.0, th().line),
            );
            let mut y = sep_y + 4.5;
            if hits.is_empty() {
                // No command matches: the one row asks AI with the query.
                let row = Rect::from_min_size(pos2(rect.left() + 8.0, y), vec2(rect.width() - 16.0, 34.0));
                let r = ui.interact(row, Id::unique("cmd-ask-ai"), Sense::click());
                let p = ui.painter();
                p.rect_filled(row, CornerRadius::same(7), th().hover);
                p.text(
                    pos2(row.left() + 18.0, row.center().y),
                    Align2::CENTER_CENTER,
                    icon::SPARKLES,
                    icon_font(14.0),
                    th().accent,
                );
                let g = p.layout_no_wrap("Ask AI".into(), sans(13.0), th().text);
                let gw = g.size().x;
                p.galley(pos2(row.left() + 32.0, row.center().y - g.size().y / 2.0), g, th().text);
                let mut job = LayoutJob::single_section(
                    format!("“{}”", self.cmdk.as_deref().unwrap_or("").trim()),
                    TextFormat { font_id: sans(13.0), color: th().muted, ..Default::default() },
                );
                job.wrap = egui::text::TextWrapping {
                    max_width: (row.width() - 32.0 - gw - 8.0 - 70.0).max(1.0),
                    max_rows: 1,
                    break_anywhere: true,
                    overflow_character: Some('…'),
                };
                let q = p.layout_job(job);
                p.galley(pos2(row.left() + 32.0 + gw + 8.0, row.center().y - q.size().y / 2.0), q, th().muted);
                kbd(ui, pos2(row.right() - 10.0, row.center().y), "↵", true);
                self.hits.insert("cmd-ask-ai".into(), row);
                if r.clicked() {
                    ask = true;
                }
            }
            for (n, &i) in hits.iter().take(shown).enumerate() {
                let row = Rect::from_min_size(pos2(rect.left() + 8.0, y), vec2(rect.width() - 16.0, 34.0));
                let r = ui.interact(row, Id::unique(("cmd", i)), Sense::click());
                // Only a pointer that moves picks a row, so one resting over the list does not
                // steal Enter from the typed match.
                if r.hovered() && ui.input(|i| i.pointer.delta() != egui::Vec2::ZERO) {
                    self.cmdk_sel = n;
                }
                let p = ui.painter();
                if n == self.cmdk_sel {
                    p.rect_filled(row, CornerRadius::same(7), th().hover);
                }
                p.text(
                    pos2(row.left() + 10.0, row.center().y),
                    Align2::LEFT_CENTER,
                    COMMANDS[i].label,
                    sans(13.0),
                    th().text,
                );
                let mut kx = row.right() - 10.0;
                for k in COMMANDS[i].keys.iter().rev() {
                    kx -= kbd(ui, pos2(kx, row.center().y), k, true) + 4.0;
                }
                if r.clicked() {
                    run = Some(i);
                }
                y += 34.0;
            }
        });
        if ask {
            let q = self.cmdk.clone().unwrap_or_default();
            ctx.memory_mut(|m| m.surrender_focus(Id::unique("cmdk-field")));
            self.open_ask_bar(&q);
            return;
        }
        if let Some(i) = run {
            (COMMANDS[i].run)(self);
            close = true;
        }
        if close {
            self.cmdk = None;
            ctx.memory_mut(|m| m.surrender_focus(Id::unique("cmdk-field")));
        }
    }
}

// .rail-sep
fn rail_sep(ui: &Ui, rail: Rect, y: &mut f32) {
    let sy = *y + 2.5;
    ui.painter()
        .line_segment([pos2(rail.left() + 10.0, sy), pos2(rail.right() - 10.0, sy)], Stroke::new(1.0, th().line));
    *y += 9.0;
}

// A text button of the View flyout with its key in the corner, accent when on. Returns
// whether it was clicked.
fn view_chip(ui: &mut Ui, c: Rect, id: &str, label: &str, key: &str, on: bool) -> bool {
    let resp = ui.interact(c, Id::unique(id), Sense::click());
    let hover = resp.hovered();
    let p = ui.painter();
    let color = if on {
        p.rect(c, CornerRadius::same(7), th().accent_bg, Stroke::new(1.0, th().ring), StrokeKind::Inside);
        th().accent
    } else {
        let border = if hover { th().tint(64) } else { th().line2 };
        p.rect_stroke(c, CornerRadius::same(7), Stroke::new(1.0, border), StrokeKind::Inside);
        if hover { th().text } else { th().muted }
    };
    let shift = if key.is_empty() { 0.0 } else { 3.0 };
    p.text(pos2(c.center().x - shift, c.center().y), Align2::CENTER_CENTER, label, sans(11.0), color);
    let kc = if on { th().accent.gamma_multiply(0.8) } else { th().faint };
    p.text(pos2(c.right() - 4.0, c.bottom() - 2.0), Align2::RIGHT_BOTTOM, key, mono_medium(8.5), kc);
    resp.clicked()
}

/// A separator's width in the dock.
fn sep_w() -> f32 {
    11.0
}

fn dock_sep(ui: &Ui, bar: Rect, x: &mut f32) {
    let sx = *x + 3.5;
    ui.painter().line_segment([pos2(sx, bar.top() + 13.0), pos2(sx, bar.bottom() - 13.0)], Stroke::new(1.0, th().line));
    *x += 11.0;
}

// A dock button: a rail button at touch size, with no key.
fn dock_button(ui: &mut Ui, at: Pos2, glyph: char, id: &str, on: bool, enabled: bool) -> egui::Response {
    let r = Rect::from_min_size(at, vec2(DOCK_BTN, DOCK_BTN));
    let sense = if enabled { Sense::click() } else { Sense::hover() };
    let resp = ui.interact(r, Id::unique(("dock", id)), sense);
    let p = ui.painter();
    let mut color = th().muted;
    if on {
        p.rect(r, CornerRadius::same(8), th().accent_bg, Stroke::new(1.0, th().ring), StrokeKind::Inside);
        color = th().text;
    } else if enabled && resp.hovered() {
        p.rect_filled(r, CornerRadius::same(8), th().hover);
        color = th().text;
    }
    if !enabled {
        color = color.gamma_multiply(0.35);
    }
    p.text(r.center(), Align2::CENTER_CENTER, glyph, icon_font(18.0), color);
    resp
}

// .tool, .tool.on, .tool .k
fn tool_button(ui: &mut Ui, at: Pos2, glyph: char, key: &str, on: bool) -> egui::Response {
    let r = Rect::from_min_size(at, vec2(TOOL, TOOL));
    let resp = ui.interact(r, Id::unique(("tool", key)), Sense::click());
    let p = ui.painter();
    let mut color = th().muted;
    if on {
        p.rect(r, CornerRadius::same(8), th().accent_bg, Stroke::new(1.0, th().ring), StrokeKind::Inside);
        color = th().text;
    } else if resp.hovered() {
        p.rect_filled(r, CornerRadius::same(8), th().hover);
        color = th().text;
    }
    p.text(r.center(), Align2::CENTER_CENTER, glyph, icon_font(17.0), color);
    let kc = if on { th().muted } else { th().faint };
    p.text(pos2(r.right() - 2.5, r.bottom() - 1.5), Align2::RIGHT_BOTTOM, key, mono_medium(8.0), kc);
    resp
}

// .tbtn: 32 px high, 10 px side padding, 16 px icon, 6 px gap.
fn tbtn(
    ui: &mut Ui,
    x: &mut f32,
    cy: f32,
    glyph: Option<char>,
    label: &str,
    id: &str,
    enabled: bool,
) -> egui::Response {
    tbtn_on(ui, x, cy, glyph, label, id, enabled, false)
}

/// A top bar button, drawn hovered while `on` (its menu is open).
#[allow(clippy::too_many_arguments)]
fn tbtn_on(
    ui: &mut Ui,
    x: &mut f32,
    cy: f32,
    glyph: Option<char>,
    label: &str,
    id: &str,
    enabled: bool,
    on: bool,
) -> egui::Response {
    let g = (!label.is_empty()).then(|| ui.painter().layout_no_wrap(label.into(), sans(13.0), th().muted));
    let w = 20.0
        + glyph.map_or(0.0, |_| 16.0)
        + g.as_ref().map_or(0.0, |g| g.size().x + if glyph.is_some() { 6.0 } else { 0.0 });
    let r = Rect::from_min_size(pos2(*x, cy - 16.0), vec2(w, 32.0));
    *x += w;
    let sense = if enabled { Sense::click() } else { Sense::hover() };
    let resp = ui.interact(r, Id::unique(("tbtn", id)), sense);
    let p = ui.painter();
    let mut color = th().muted;
    if enabled && (resp.hovered() || on) {
        p.rect_filled(r, CornerRadius::same(8), th().hover);
        color = th().text;
    }
    if !enabled {
        color = color.gamma_multiply(0.35);
    }
    let mut cx = r.left() + 10.0;
    if let Some(ic) = glyph {
        p.text(pos2(cx + 8.0, cy), Align2::CENTER_CENTER, ic, icon_font(16.0), color);
        cx += 22.0;
    }
    if let Some(g) = g {
        p.galley(pos2(cx, cy - g.size().y / 2.0), g, color);
    }
    resp
}

/// A camera bar button: an icon, or a label, hovered like the top bar's buttons.
fn nav_button(
    ui: &mut Ui,
    r: Rect,
    glyph: Option<char>,
    label: Option<std::sync::Arc<egui::Galley>>,
    id: &str,
    enabled: bool,
    on: bool,
) -> egui::Response {
    let sense = if enabled { Sense::click() } else { Sense::hover() };
    let resp = ui.interact(r, Id::unique(("nav", id)), sense);
    let p = ui.painter();
    let mut color = th().muted;
    if on {
        p.rect_filled(r, CornerRadius::same(8), th().accent_bg);
        color = th().accent;
    } else if enabled && resp.hovered() {
        p.rect_filled(r, CornerRadius::same(8), th().hover);
        color = th().text;
    }
    if !enabled {
        color = color.gamma_multiply(0.35);
    }
    if let Some(g) = glyph {
        p.text(r.center(), Align2::CENTER_CENTER, g, icon_font(16.0), color);
    }
    if let Some(g) = label {
        p.galley(r.center() - g.size() / 2.0, g, color);
    }
    resp
}

/// A tooltip above `at`: what the control does and its keys, as kbd chips.
///
/// It keeps off `clear.0`: where above `at` would cross it, it goes below `at` if `clear.1`
/// allows, else it is not shown.
fn tooltip(ctx: &Context, at: Rect, label: &str, keys: &[&str], clear: Option<(Rect, bool)>) -> Option<Rect> {
    let g = ctx.fonts_mut(|f| f.layout_no_wrap(label.into(), sans(12.0), th().text));
    let kg: Vec<_> =
        keys.iter().map(|k| ctx.fonts_mut(|f| f.layout_no_wrap((*k).into(), mono_medium(10.5), th().muted))).collect();
    let kw: f32 = kg.iter().map(|k| (k.size().x + 10.0).max(18.0) + 3.0).sum();
    let w = 10.0 + g.size().x + if kg.is_empty() { 0.0 } else { 8.0 + kw - 3.0 } + 10.0;
    let size = vec2(w, 28.0);
    let screen = ctx.content_rect();
    let x = (at.center().x - w / 2.0).clamp(screen.left() + 12.0, screen.right() - 12.0 - w).round();
    let mut rect = Rect::from_min_size(pos2(x, at.top() - 8.0 - 4.0 - size.y), size);
    if let Some((_, below_ok)) = clear.filter(|(avoid, _)| avoid.intersects(rect)) {
        if !below_ok {
            return None;
        }
        rect = Rect::from_min_size(pos2(x, at.bottom() + 8.0 + 4.0), size);
    }
    Area::new(Id::unique("tooltip")).fade_in(false).order(Order::Tooltip).fixed_pos(rect.min).constrain(false).show(
        ctx,
        |ui| {
            let p = ui.painter();
            p.add(
                Shadow { offset: [0, 6], blur: 18, spread: 0, color: th().shadow }
                    .as_shape(rect, CornerRadius::same(8)),
            );
            p.rect(rect, CornerRadius::same(8), th().panel, Stroke::new(1.0, th().line2), StrokeKind::Inside);
            let cy = rect.center().y;
            p.galley(pos2(rect.left() + 10.0, cy - g.size().y / 2.0), g.clone(), th().text);
            let mut kx = rect.left() + 10.0 + g.size().x + 8.0;
            for k in kg {
                let kw = (k.size().x + 10.0).max(18.0);
                let r = Rect::from_min_size(pos2(kx, cy - 9.5), vec2(kw, 19.0));
                kbd_frame(p, r);
                p.galley(
                    pos2(r.center().x - k.size().x / 2.0, r.top() + 1.0 + (16.0 - k.size().y) / 2.0),
                    k,
                    th().muted,
                );
                kx += kw + 3.0;
            }
        },
    );
    Some(rect)
}

/// kbd: 10.5 px mono, 1 px border with a 2 px bottom, right-aligned at `right_center`. Returns its width.
fn kbd(ui: &Ui, right_center: Pos2, text: &str, _right: bool) -> f32 {
    let g = ui.painter().layout_no_wrap(text.into(), mono_medium(10.5), th().muted);
    let w = (g.size().x + 10.0).max(18.0);
    let r = Rect::from_min_size(pos2(right_center.x - w, right_center.y - 9.5), vec2(w, 19.0));
    let p = ui.painter();
    kbd_frame(p, r);
    p.galley(pos2(r.center().x - g.size().x / 2.0, r.top() + 1.0 + (16.0 - g.size().y) / 2.0), g, th().muted);
    w
}

/// The kbd border: 1 px, 2 px at the bottom, radius 5.
fn kbd_frame(p: &egui::Painter, r: Rect) {
    p.rect_stroke(r, CornerRadius::same(5), Stroke::new(1.0, th().line2), StrokeKind::Inside);
    p.line_segment(
        [pos2(r.left() + 2.5, r.bottom() - 1.5), pos2(r.right() - 2.5, r.bottom() - 1.5)],
        Stroke::new(1.0, th().line2),
    );
}

/// Draw kbd frames around the given characters of a laid-out paragraph at `origin`.
fn kbd_frames(p: &egui::Painter, origin: Pos2, g: &egui::Galley, chars: &[char]) {
    for row in &g.rows {
        for gl in &row.row.glyphs {
            if chars.contains(&gl.chr) {
                let cx = origin.x + row.pos.x + gl.pos.x + gl.advance_width / 2.0;
                let cy = origin.y + row.pos.y + row.row.size.y / 2.0;
                let w = (gl.advance_width + 8.0).max(18.0);
                kbd_frame(p, Rect::from_center_size(pos2(cx, cy + 0.5), vec2(w, 19.0)));
            }
        }
    }
}

/// Uppercase label with CSS letter-spacing (`em` times the font size, in points).
fn spaced(text: &str, font: FontId, color: Color32, spacing: f32) -> LayoutJob {
    let mut job = LayoutJob::default();
    job.append(text, 0.0, TextFormat { font_id: font, color, extra_letter_spacing: spacing, ..Default::default() });
    job.halign = Align::LEFT;
    job
}

pub(crate) fn group_thousands(n: u64) -> String {
    let s = n.to_string();
    let mut out = String::new();
    for (i, c) in s.chars().enumerate() {
        if i > 0 && (s.len() - i).is_multiple_of(3) {
            out.push(',');
        }
        out.push(c);
    }
    out
}

/// A smooth gradient over `rect`: a grid of `cols` x `rows` quads coloured by `f(u, v)` at
/// each corner, so the colours are exact at the grid points.
fn gradient(p: &egui::Painter, rect: Rect, cols: usize, rows: usize, f: impl Fn(f32, f32) -> Color32) {
    let mut m = egui::Mesh::default();
    for j in 0..=rows {
        for i in 0..=cols {
            let (u, v) = (i as f32 / cols as f32, j as f32 / rows as f32);
            m.colored_vertex(rect.lerp_inside(vec2(u, v)), f(u, v));
        }
    }
    let row = (cols + 1) as u32;
    for j in 0..rows as u32 {
        for i in 0..cols as u32 {
            let a = j * row + i;
            m.add_triangle(a, a + 1, a + row);
            m.add_triangle(a + 1, a + row + 1, a + row);
        }
    }
    p.add(m);
}

/// A picker handle: the colour in a white ring with a soft dark edge.
fn handle(p: &egui::Painter, at: Pos2, c: [u8; 3]) {
    p.circle(at, 6.0, Color32::from_rgb(c[0], c[1], c[2]), Stroke::new(2.0, Color32::WHITE));
    p.circle_stroke(at, 7.5, Stroke::new(1.0, Color32::from_black_alpha(100)));
}

/// A text field in the #current box style, with a faint mono label. Returns true when the
/// field gives up focus (Enter, Tab or a click elsewhere), the moment to apply it.
fn field(ui: &mut Ui, rect: Rect, label: &str, id: Id, text: &mut String) -> bool {
    let focused = ui.memory(|m| m.has_focus(id));
    let p = ui.painter().clone();
    let border = if focused { th().ring } else { th().line };
    p.rect(rect, CornerRadius::same(7), th().tint(9), Stroke::new(1.0, border), StrokeKind::Inside);
    let g = p.layout_no_wrap(label.into(), mono(10.0), th().faint);
    let lw = g.size().x;
    p.galley(pos2(rect.left() + 9.0, rect.center().y - g.size().y / 2.0), g, th().faint);
    let tr = Rect::from_min_max(
        pos2(rect.left() + 9.0 + lw + 7.0, rect.top() + 6.0),
        pos2(rect.right() - 7.0, rect.bottom() - 6.0),
    );
    let edit = egui::TextEdit::singleline(text)
        .id(id)
        .font(mono(12.0))
        .text_color(th().text)
        .frame(egui::Frame::NONE)
        .margin(egui::Margin::ZERO)
        .desired_width(tr.width());
    ui.put(tr, edit).lost_focus()
}

/// A Render panel slider: hit name, label, value, range and how the value reads.
type SliderRow<'a> = (&'static str, &'static str, &'a mut f32, f32, f32, fn(f32) -> String);

/// A labelled slider row: label and value on top, a thin track below. Dragging or clicking
/// anywhere on the row sets the value. Returns whether the value changed, and whether the
/// drag or click ended (the moment to make it one undo step).
#[allow(clippy::too_many_arguments)]
fn slider(
    ui: &mut Ui,
    r: Rect,
    id: &str,
    label: &str,
    v: &mut f32,
    lo: f32,
    hi: f32,
    fmt: fn(f32) -> String,
) -> (bool, bool) {
    let resp = ui.interact(r, Id::unique(("slider", id)), Sense::click_and_drag());
    let track = Rect::from_min_size(pos2(r.left() + 1.0, r.top() + 22.0), vec2(r.width() - 2.0, 4.0));
    let mut changed = false;
    if resp.is_pointer_button_down_on()
        && let Some(q) = resp.interact_pointer_pos()
    {
        let k = ((q.x - track.left()) / track.width()).clamp(0.0, 1.0);
        let nv = lo + k * (hi - lo);
        changed = nv != *v;
        *v = nv;
    }
    let p = ui.painter();
    let active = resp.hovered() || resp.dragged();
    p.text(
        r.left_top() + vec2(0.0, 7.0),
        Align2::LEFT_CENTER,
        label,
        sans(12.0),
        if active { th().text } else { th().muted },
    );
    p.text(r.right_top() + vec2(0.0, 7.0), Align2::RIGHT_CENTER, fmt(*v), mono(11.0), th().text);
    let k = ((*v - lo) / (hi - lo)).clamp(0.0, 1.0);
    p.rect_filled(track, CornerRadius::same(2), th().line2);
    p.rect_filled(Rect::from_min_size(track.min, vec2(track.width() * k, 4.0)), CornerRadius::same(2), th().accent);
    let at = pos2(track.left() + track.width() * k, track.center().y);
    p.circle(at, 6.0, th().text, Stroke::new(1.0, Color32::from_black_alpha(100)));
    (changed, resp.drag_stopped() || resp.clicked())
}

/// A row of equal buttons, one selected (the .tool.on look); `ok[k]` false greys one out.
/// Records each as `names[k]` in `hits` and returns the one clicked.
#[allow(clippy::too_many_arguments)]
fn segmented(
    ui: &mut Ui,
    r: Rect,
    names: &[&'static str],
    labels: &[String],
    sel: usize,
    ok: &[bool],
    font: FontId,
    hits: &mut Vec<(&'static str, Rect)>,
) -> Option<usize> {
    let n = labels.len();
    let free = r.width() - 4.0 * (n - 1) as f32;
    // Equal cells, unless a label would not keep 8 points either side: then each cell is its
    // label's width plus an equal share of what is left.
    let text: Vec<f32> = labels
        .iter()
        .map(|l| ui.fonts_mut(|f| f.layout_no_wrap(l.clone(), font.clone(), th().text)).size().x)
        .collect();
    let widths: Vec<f32> = if text.iter().all(|&t| t + 16.0 <= free / n as f32) {
        vec![free / n as f32; n]
    } else {
        let pad = (free - text.iter().sum::<f32>()) / n as f32;
        text.iter().map(|t| t + pad).collect()
    };
    let mut out = None;
    let mut x = r.left();
    for (k, label) in labels.iter().enumerate() {
        let c = Rect::from_min_size(pos2(x, r.top()), vec2(widths[k], r.height()));
        x += widths[k] + 4.0;
        hits.push((names[k], c));
        let enabled = ok[k];
        let resp = ui.interact(c, Id::unique(names[k]), if enabled { Sense::click() } else { Sense::hover() });
        let p = ui.painter();
        let color = if k == sel {
            p.rect(c, CornerRadius::same(7), th().accent_bg, Stroke::new(1.0, th().ring), StrokeKind::Inside);
            th().accent
        } else {
            let hover = enabled && resp.hovered();
            p.rect_stroke(
                c,
                CornerRadius::same(7),
                Stroke::new(1.0, if hover { th().tint(64) } else { th().line2 }),
                StrokeKind::Inside,
            );
            if !enabled {
                th().faint.gamma_multiply(0.6)
            } else if hover {
                th().text
            } else {
                th().muted
            }
        };
        p.text(c.center(), Align2::CENTER_CENTER, label, font.clone(), color);
        if enabled && resp.clicked() {
            out = Some(k);
        }
    }
    out
}

// #edit-color, at a fixed size with an icon; `on` is the .tool.on look for an armed action.
fn small_button(ui: &mut Ui, r: Rect, glyph: char, label: &str, id: &str, on: bool) -> egui::Response {
    let resp = ui.interact(r, Id::unique(("sbtn", id)), Sense::click());
    let p = ui.painter();
    let (color, border) = if on {
        p.rect_filled(r, CornerRadius::same(7), th().accent_bg);
        (th().accent, th().ring)
    } else if resp.hovered() {
        (th().text, th().tint(64))
    } else {
        (th().muted, th().line2)
    };
    p.rect_stroke(r, CornerRadius::same(7), Stroke::new(1.0, border), StrokeKind::Inside);
    let g = p.layout_no_wrap(label.into(), sans(12.0), color);
    let total = 14.0 + 6.0 + g.size().x;
    let x = r.center().x - total / 2.0;
    p.text(pos2(x + 7.0, r.center().y), Align2::CENTER_CENTER, glyph, icon_font(14.0), color);
    p.galley(pos2(x + 20.0, r.center().y - g.size().y / 2.0), g, color);
    resp
}

/// The top bar's logo and the gap before the wordmark: the 18-point cube is 15.6 wide, and
/// the gap is a little wider than the lockup's (a third of the cube's height, not a quarter),
/// so the word stays clear of the cube at this size.
const LOGO_STEP: f32 = 22.0;

/// The logo: a 2x2x2 voxel cube with its top front voxel out, in the theme's three face tones
/// (`Theme::logo`), `size` tall with its top left at `at` (`assets/logo` has it as SVG). Each
/// polygon is convex; where two of one tone meet, a hairline of their own colour hides the
/// anti-aliased seam.
fn logo(ui: &Ui, at: Pos2, size: f32) {
    // On a unit-high box 0.866 wide: (tone, corners). Tone 0 is the top, 1 the left, 2 the right.
    const FACES: [(usize, &[[f32; 2]]); 9] = [
        (0, &[[0.433, 0.0], [0.866, 0.25], [0.6495, 0.375], [0.2165, 0.125]]),
        (0, &[[0.2165, 0.125], [0.433, 0.25], [0.2165, 0.375], [0.0, 0.25]]),
        (1, &[[0.0, 0.25], [0.2165, 0.375], [0.2165, 0.875], [0.0, 0.75]]),
        (2, &[[0.866, 0.25], [0.866, 0.75], [0.6495, 0.875], [0.6495, 0.375]]),
        // The notch: its two walls and its floor.
        (1, &[[0.433, 0.25], [0.6495, 0.375], [0.6495, 0.625], [0.433, 0.5]]),
        (2, &[[0.433, 0.25], [0.433, 0.5], [0.2165, 0.625], [0.2165, 0.375]]),
        (0, &[[0.433, 0.5], [0.6495, 0.625], [0.433, 0.75], [0.2165, 0.625]]),
        // The front corner voxel's two sides, under the notch.
        (1, &[[0.2165, 0.625], [0.433, 0.75], [0.433, 1.0], [0.2165, 0.875]]),
        (2, &[[0.6495, 0.625], [0.6495, 0.875], [0.433, 1.0], [0.433, 0.75]]),
    ];
    let tones = th().logo;
    let p = ui.painter();
    for (tone, corners) in FACES {
        let pts = corners.iter().map(|[x, y]| at + vec2(x * size, y * size)).collect();
        p.add(egui::Shape::convex_polygon(pts, tones[tone], Stroke::new(0.5, tones[tone])));
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn thousands() {
        assert_eq!(super::group_thousands(145064), "145,064");
        assert_eq!(super::group_thousands(999), "999");
        assert_eq!(super::group_thousands(1_000_000), "1,000,000");
    }

    #[test]
    fn the_full_render_view_swaps_and_escape_backs_out_in_order() {
        let mut h = super::Hybrid::new([[0; 4]; 256], [false; 256], "test");
        h.show_render = false;
        h.toggle_full_render();
        assert!(h.full_render && h.show_render, "it shows the render too");
        h.toggle_render_panel();
        h.escape();
        assert!(!h.render_panel && h.full_render, "Escape closes the Render panel first");
        h.escape();
        assert!(!h.full_render && h.show_render, "then swaps back to the corner");
        h.toggle_full_render();
        let hide = super::COMMANDS.iter().find(|c| c.label == "Toggle render preview").unwrap();
        (hide.run)(&mut h);
        assert!(!h.show_render && !h.full_render, "hiding the render leaves the full view");
    }

    /// Draw the touch layout on a screen of `points` with `open` applied, and list what is
    /// wrong: a hit or a panel off the screen, and a dock or top bar button under another panel.
    fn phone_problems(points: (f32, f32), open: impl Fn(&mut super::Hybrid)) -> Vec<String> {
        let ctx = egui::Context::default();
        super::install(&ctx, None);
        let mut h = super::Hybrid::new([[0; 4]; 256], [false; 256], "test");
        h.touch = true;
        open(&mut h);
        // The render corner shows, as it does once the path tracer has a frame.
        let stats = super::Stats { render: Some((1.0, true)), ..Default::default() };
        let screen = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(points.0, points.1));
        for _ in 0..4 {
            let input = egui::RawInput { screen_rect: Some(screen), ..Default::default() };
            let mut out = ctx.run_ui(input, |u| {
                h.ui(u.ctx(), &stats);
            });
            out.textures_delta.clear();
        }
        let mut bad = Vec::new();
        // What the assistant's scrolling content lays out past its edge is reached by scrolling.
        let scrolled = |n: &str| n.starts_with("ai-") && !n.starts_with("ai-toggle");
        let sane = screen.expand(0.5);
        for (n, r) in &h.hits {
            if !scrolled(n) && !sane.contains_rect(*r) {
                bad.push(format!("hit {n} {r:?} leaves the screen"));
            }
        }
        let areas: Vec<_> = ctx.memory(|m| m.areas().visible_layer_ids().into_iter().collect());
        for id in areas {
            if id.order == egui::Order::Tooltip {
                continue;
            }
            let Some(r) = ctx.memory(|m| m.area_rect(id.id)) else { continue };
            let r = ctx.layer_transform_to_global(id).map_or(r, |t| egui::Rect::from_min_max(t * r.min, t * r.max));
            // A panel's shadow is not the panel: judge the rect it was asked for.
            if !sane.contains_rect(r.shrink(0.0)) && r.width() < 2.0 * screen.width() {
                bad.push(format!("panel {:?} {r:?} leaves the screen", id.id));
            }
        }
        for n in [
            "tool-add",
            "tool-erase",
            "tool-paint",
            "tool-pick",
            "select",
            "dock-color",
            "undo",
            "redo",
            "voxel",
            "brush-box",
            "brush-face",
            "brush-line",
            "center",
            "mirror",
            "lock",
            "open",
            "save",
            "export",
            "theme",
            "ai-toggle",
            "keys",
        ] {
            let Some(r) = h.hits.get(n) else {
                bad.push(format!("no {n} button"));
                continue;
            };
            let top = ctx.layer_id_at(r.center());
            // The confirm is a modal: the backdrop takes the touch until it is answered.
            let own = ["dock", "dock-brushes", "dock-more", "top", "top-right", "new-backdrop"].map(egui::Id::unique);
            if !top.is_some_and(|l| own.contains(&l.id)) {
                bad.push(format!("{n} at {:?} is under {top:?}", r.center()));
            }
        }
        bad
    }

    #[test]
    fn the_touch_layout_fits_phones() {
        type Open = fn(&mut super::Hybrid);
        let scenarios: [(&str, Open); 15] = [
            ("plain", |_| {}),
            ("assistant", |h| h.ai_open = true),
            ("assistant+palette", |h| (h.ai_open, h.palette_open) = (true, true)),
            ("palette", |h| h.palette_open = true),
            ("select", |h| h.tool = super::Tool::Select),
            ("assistant+select", |h| (h.ai_open, h.tool) = (true, super::Tool::Select)),
            ("size", |h| h.size_panel = true),
            ("bounds", |h| h.bounds_panel = Some(["1".into(), "1".into(), "1".into()])),
            ("export", |h| h.export_menu = true),
            ("theme", |h| h.theme_menu = true),
            ("view", |h| h.view_panel = true),
            ("render", |h| h.render_panel = true),
            ("commands", |h| h.cmdk = Some(String::new())),
            ("colour editor", |h| h.open_color_edit()),
            ("confirm", |h| h.confirm_new = true),
        ];
        // 360, 390 and 430 CSS px wide upright, then 844 and 932 wide sideways, at 1.3 points.
        for pts in [(277.0, 649.0), (300.0, 649.0), (331.0, 685.0), (649.0, 300.0), (717.0, 331.0)] {
            for (name, open) in &scenarios {
                let bad = phone_problems(pts, open);
                assert!(bad.is_empty(), "{pts:?} {name}: {bad:#?}");
            }
        }
    }
}
