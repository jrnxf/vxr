//! Help's demo tiles: short scripts of `vxr-core` edits and camera moves, replayed on a small
//! scene that loops. A [`Player`] runs one script on the CPU with no GPU, so tests drive it
//! straight; [`Tiles`] gives each shown player a small offscreen render (a sibling of the
//! editor's renderer, so no shader build) that egui draws as an image. The same code runs
//! native and in the browser. Nothing here runs while Help is closed.

use glam::Vec3;
use vxr_core::Scene;
use vxr_core::select::{Mode, MoveDrag};
use vxr_core::tools::{Brush, Gesture, Press, Shape, Target, Tool, box_cells};
use vxr_render::{GpuMap, OrbitCamera, Renderer};

/// What a tile's Try button puts the user in.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TryIt {
    /// A tool and brush, with mirror on or off.
    Tool(Tool, Shape, bool),
    /// The camera: Try shows the camera keys.
    Camera,
    /// World mode and an object's model (Tab).
    World,
}

/// One demo: its tile's words and its script.
pub struct Demo {
    /// Its name in `hit(...)` and `state()`: `help-tile-<id>` and `help-try-<id>`.
    pub id: &'static str,
    pub name: &'static str,
    /// The keys line under the name, with a mouse and keyboard, and on touch.
    pub keys: &'static str,
    pub touch_keys: &'static str,
    /// More words search finds it by.
    pub words: &'static str,
    pub try_it: TryIt,
    scene: fn() -> Scene,
    steps: fn() -> Vec<Step>,
}

/// Every demo, in the order Help shows them: the four things a newcomer cannot guess come first.
pub const DEMOS: &[Demo] = &[
    Demo {
        id: "add",
        name: "Add and draw",
        keys: "Click · Drag",
        touch_keys: "Tap · Drag",
        words: "add voxel draw brush stroke basics",
        try_it: TryIt::Tool(Tool::Add, Shape::Voxel, false),
        scene: ground,
        steps: add_steps,
    },
    Demo {
        id: "orbit",
        name: "Look around",
        keys: "Right-drag · Scroll",
        touch_keys: "Two fingers · Pinch",
        words: "camera orbit rotate zoom pan shift right drag view navigate",
        try_it: TryIt::Camera,
        scene: house,
        steps: orbit_steps,
    },
    Demo {
        id: "box",
        name: "Box then lift",
        keys: "B · Drag · Move up · Click",
        touch_keys: "B · Drag · Lift · Tap",
        words: "box brush rectangle height lift cube shapes",
        try_it: TryIt::Tool(Tool::Add, Shape::Box, false),
        scene: ground,
        steps: box_steps,
    },
    Demo {
        id: "select",
        name: "Select and move",
        keys: "S · Drag · Drag a voxel",
        touch_keys: "S · Drag · Drag a voxel",
        words: "select selection marquee move drag transform",
        try_it: TryIt::Tool(Tool::Select, Shape::Voxel, false),
        scene: crate_scene,
        steps: select_steps,
    },
    Demo {
        id: "world",
        name: "World or model",
        keys: "Tab",
        touch_keys: "Tab",
        words: "world model object objects tab scene tree edit",
        try_it: TryIt::World,
        scene: world_scene,
        steps: world_steps,
    },
    Demo {
        id: "erase",
        name: "Erase",
        keys: "E · Click · Drag",
        touch_keys: "E · Tap · Drag",
        words: "erase delete remove rubber",
        try_it: TryIt::Tool(Tool::Erase, Shape::Voxel, false),
        scene: wall,
        steps: erase_steps,
    },
    Demo {
        id: "paint",
        name: "Paint a face",
        keys: "P · F · Click",
        touch_keys: "P · F · Tap",
        words: "paint color colour face recolor",
        try_it: TryIt::Tool(Tool::Paint, Shape::Face, false),
        scene: block,
        steps: paint_steps,
    },
    Demo {
        id: "pick",
        name: "Pick a color",
        keys: "I · Click",
        touch_keys: "I · Tap",
        words: "pick color colour eyedropper pipette sample",
        try_it: TryIt::Tool(Tool::Pick, Shape::Voxel, false),
        scene: house,
        steps: pick_steps,
    },
    Demo {
        id: "extrude",
        name: "Extrude a face",
        keys: "A · F · Click",
        touch_keys: "A · F · Tap",
        words: "extrude face pull raise grow",
        try_it: TryIt::Tool(Tool::Add, Shape::Face, false),
        scene: block,
        steps: extrude_steps,
    },
    Demo {
        id: "line",
        name: "Draw a line",
        keys: "L · Drag",
        touch_keys: "L · Drag",
        words: "line straight brush",
        try_it: TryIt::Tool(Tool::Add, Shape::Line, false),
        scene: ground,
        steps: line_steps,
    },
    Demo {
        id: "round",
        name: "Circle, sphere",
        keys: "C · Drag · C again",
        touch_keys: "C · Drag · C again",
        words: "circle sphere ball disc round center centre",
        try_it: TryIt::Tool(Tool::Add, Shape::Circle, false),
        scene: ground,
        steps: round_steps,
    },
    Demo {
        id: "mirror",
        name: "Mirror",
        keys: "M · Draw",
        touch_keys: "M · Draw",
        words: "mirror symmetry symmetric",
        try_it: TryIt::Tool(Tool::Add, Shape::Voxel, true),
        scene: ground,
        steps: mirror_steps,
    },
];

/// The demo named `id`.
pub fn demo(id: &str) -> Option<usize> {
    DEMOS.iter().position(|d| d.id == id)
}

/// The demos whose name, keys or words hold every word of `query` (any case), in Help's order.
pub fn search(query: &str, touch: bool) -> Vec<usize> {
    let words: Vec<String> = query.split_whitespace().map(str::to_lowercase).collect();
    (0..DEMOS.len())
        .filter(|&i| {
            let d = &DEMOS[i];
            let text = format!("{} {} {}", d.name, if touch { d.touch_keys } else { d.keys }, d.words).to_lowercase();
            words.iter().all(|w| text.contains(w.as_str()))
        })
        .collect()
}

/// The button a demo's pointer holds, drawn over the tile (on touch: one, two or three fingers).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Button {
    Left,
    /// Orbit: the right button, or two fingers.
    Right,
    /// Pan: Shift and the right button, or three fingers.
    Pan,
}

/// One step of a script: what happens and how long it takes.
#[derive(Clone, Copy, Debug)]
struct Step {
    ms: f32,
    op: Op,
}

/// Cells are in the map of the scene the script starts on; a world switch (Tab) moves them.
#[derive(Clone, Copy, Debug)]
enum Op {
    /// Show these keys over the tile, with a mouse and keyboard, and on touch ("" hides them).
    Key(&'static str, &'static str),
    /// The pointer glides to the face of `cell` along `normal`.
    Point([i32; 3], [i32; 3]),
    /// Press the brush on the face of voxel `cell` along `normal` (on the ground: y -1, up).
    Press(Tool, Shape, [i32; 3], [i32; 3]),
    /// Drag the stroke to cell `p`, a cell at a time over the step.
    Drag([i32; 3]),
    /// Let go: a box starts to lift, any other stroke ends.
    Up,
    /// Lift the box to layer `h` on its drag axis.
    LiftTo(i32),
    /// Click to place the lifted box.
    Place,
    /// Drag a box from `lo` to `hi` and select the voxels in it.
    Select([i32; 3], [i32; 3]),
    /// Press on the top of selected voxel `cell` to move the selection.
    Grab([i32; 3]),
    /// Move the grabbed selection to cell `p` on its plane.
    MoveTo([i32; 3]),
    /// Let go of the selection.
    Drop,
    /// Orbit by this yaw and pitch (radians), the right button held.
    Orbit(f32, f32),
    /// Zoom in (above 0) or out by this many scroll steps.
    Zoom(f32),
    /// Pan by this many voxels right and up, Shift and the right button held.
    Pan(f32, f32),
    /// Tab: edit the first object's model, or show the world again.
    Tab,
    /// Draw with this palette slot from now on.
    Color(u8),
    /// Mirror the brush across x from now on.
    Mirror(bool),
    Wait,
}

use Op::*;

const UP: [i32; 3] = [0, 1, 0];

/// Where a pointer is: on a point of the map, or (camera moves) at a place on the tile.
#[derive(Clone, Copy, Debug, PartialEq)]
enum Pointer {
    At(Vec3),
    /// 0 to 1 across and down the tile.
    Tile([f32; 2]),
}

/// One demo playing. `advance` moves it to a time; it loops by building its scene again.
pub struct Player {
    pub demo: usize,
    pub scene: Scene,
    pub cam: OrbitCamera,
    /// The tile's width over its height.
    pub aspect: f32,
    home: OrbitCamera,
    steps: Vec<Step>,
    /// When each step starts, in ms from the loop's start, and the loop's length.
    starts: Vec<f32>,
    total: f32,
    /// The loop's start, in seconds; None until the first advance.
    started: Option<f64>,
    step: usize,
    begun: bool,
    /// What a step that moves something started from.
    from_cam: OrbitCamera,
    from_pointer: Option<Pointer>,
    from_cell: [i32; 3],
    gesture: Option<Gesture>,
    moving: Option<MoveDrag>,
    color: u8,
    mirror: bool,
    /// The scene's origin when the script started, so cells follow a world switch.
    origin: [i32; 3],
    pointer: Option<Pointer>,
    pub down: Option<Button>,
    key: (&'static str, &'static str),
    /// Loops played, for the checks.
    pub loops: u64,
}

impl Player {
    pub fn new(demo: usize, aspect: f32) -> Self {
        let d = &DEMOS[demo];
        let steps = (d.steps)();
        let mut starts = Vec::with_capacity(steps.len());
        let mut total = 0.0;
        for s in &steps {
            starts.push(total);
            total += s.ms;
        }
        let scene = (d.scene)();
        let home = framing(&scene, aspect);
        Self {
            demo,
            origin: scene.origin,
            scene,
            cam: home,
            aspect,
            home,
            steps,
            starts,
            total,
            started: None,
            step: 0,
            begun: false,
            from_cam: home,
            from_pointer: None,
            from_cell: [0; 3],
            gesture: None,
            moving: None,
            color: ROOF,
            mirror: false,
            pointer: None,
            down: None,
            key: ("", ""),
            loops: 0,
        }
    }

    /// How long one loop takes, in seconds.
    pub fn length(&self) -> f64 {
        self.total as f64 / 1000.0
    }

    /// Play on to `now` (seconds, egui's clock): every step due is done, the one under way
    /// partly. After the last step the scene is built again and the script starts over.
    pub fn advance(&mut self, now: f64) {
        let started = *self.started.get_or_insert(now);
        let mut t = ((now - started) * 1000.0) as f32;
        if t >= self.total {
            let loops = ((t / self.total) as u64).max(1);
            self.restart();
            self.loops += loops;
            self.started = Some(started + (loops as f64 * self.total as f64) / 1000.0);
            t = t.rem_euclid(self.total);
        }
        loop {
            let i = self.step;
            if !self.begun {
                self.begin(i);
                self.begun = true;
            }
            let f = ((t - self.starts[i]) / self.steps[i].ms.max(1.0)).clamp(0.0, 1.0);
            self.tween(i, f);
            if f < 1.0 || i + 1 == self.steps.len() {
                break;
            }
            self.step += 1;
            self.begun = false;
        }
    }

    /// Frame the demo again for an image of another shape. It plays on from where it is, the
    /// camera moved as its start moved.
    pub fn set_aspect(&mut self, aspect: f32) {
        if aspect == self.aspect {
            return;
        }
        let home = framing(&(DEMOS[self.demo].scene)(), aspect);
        let scale = home.distance / self.home.distance.max(1e-3);
        for c in [&mut self.cam, &mut self.from_cam] {
            c.target += home.target - self.home.target;
            c.distance *= scale;
        }
        (self.home, self.aspect) = (home, aspect);
    }

    /// Play from the start again at the next advance.
    pub fn rewind(&mut self) {
        self.restart();
        self.started = None;
    }

    /// Start the script again on a new copy of its scene.
    fn restart(&mut self) {
        let scene = (DEMOS[self.demo].scene)();
        self.origin = scene.origin;
        self.scene = scene;
        self.cam = self.home;
        (self.step, self.begun, self.gesture, self.moving) = (0, false, None, None);
        (self.color, self.mirror, self.pointer, self.down, self.key) = (ROOF, false, None, None, ("", ""));
    }

    /// The keys shown over the tile now, if any.
    pub fn key(&self, touch: bool) -> Option<&'static str> {
        let k = if touch { self.key.1 } else { self.key.0 };
        (!k.is_empty()).then_some(k)
    }

    /// The pointer, 0 to 1 across and down the tile, if it shows.
    pub fn pointer(&self) -> Option<[f32; 2]> {
        match self.pointer? {
            Pointer::Tile(p) => Some(p),
            Pointer::At(p) => {
                let c = self.cam.view_proj(self.aspect).project_point3(p);
                Some([c.x * 0.5 + 0.5, 0.5 - c.y * 0.5])
            }
        }
    }

    /// A cell of the script in the map shown now.
    fn cell(&self, p: [i32; 3]) -> [i32; 3] {
        std::array::from_fn(|a| p[a] + self.origin[a] - self.scene.origin[a])
    }

    /// The middle of the face of `cell` along `normal`, in the map shown now.
    fn face(&self, cell: [i32; 3], normal: [i32; 3]) -> Vec3 {
        let c = self.cell(cell);
        Vec3::from_array(std::array::from_fn(|a| c[a] as f32 + 0.5 + normal[a] as f32 * 0.5))
    }

    fn brush(&self, tool: Tool, shape: Shape) -> Brush {
        Brush { tool, shape, mirror: [self.mirror, false, false], lock: None, color: self.color, size: 1 }
    }

    /// The camera keeps its place in the world when the map changes under it (Tab).
    fn follow(&mut self, before: [i32; 3]) {
        let d: [i32; 3] = std::array::from_fn(|a| before[a] - self.scene.origin[a]);
        let d = Vec3::new(d[0] as f32, d[1] as f32, d[2] as f32);
        self.cam.target += d;
        self.from_cam.target += d;
    }

    fn begin(&mut self, i: usize) {
        let op = self.steps[i].op;
        self.from_cam = self.cam;
        self.from_pointer = self.pointer;
        match op {
            Key(k, t) => self.key = (k, t),
            Press(tool, shape, cell, normal) => {
                let p = self.cell(cell);
                let ground = p[1] < self.scene.floor;
                let value = if ground { 0 } else { self.scene.map.get(p[0], p[1], p[2]) };
                let t = Target { voxel: p, normal, value, ground };
                self.pointer = Some(Pointer::At(self.face(cell, normal)));
                self.down = Some(Button::Left);
                match self.scene.press(self.brush(tool, shape), t) {
                    Press::Drag(g) => {
                        self.from_cell = g.end;
                        self.gesture = Some(g);
                    }
                    Press::Picked(v) => self.color = v,
                    Press::Done(_) | Press::Nothing => {}
                }
            }
            Drag(_) | LiftTo(_) => {
                if let Some(g) = &self.gesture {
                    self.from_cell = g.end;
                }
            }
            Up => {
                self.down = None;
                match self.gesture.take() {
                    Some(mut g) if g.brush().shape == Shape::Box => {
                        g.lift();
                        self.gesture = Some(g);
                    }
                    Some(g) => {
                        g.release(&mut self.scene);
                    }
                    None => {}
                }
            }
            Place => {
                self.down = Some(Button::Left);
                if let Some(g) = self.gesture.take() {
                    g.release(&mut self.scene);
                }
            }
            Select(lo, _) => {
                self.pointer = Some(Pointer::At(self.face(lo, UP)));
                self.from_pointer = self.pointer;
                self.down = Some(Button::Left);
            }
            Grab(cell) => {
                self.pointer = Some(Pointer::At(self.face(cell, UP)));
                self.down = Some(Button::Left);
                let c = self.cell(cell);
                self.from_cell = c;
                self.moving = self.scene.start_move(1, c);
            }
            Drop => {
                self.down = None;
                if let Some(m) = self.moving.take() {
                    m.release(&mut self.scene);
                }
            }
            Orbit(..) | Pan(..) => {
                // A camera drag starts where the pointer is on the tile.
                let at = self.pointer().unwrap_or([0.5, 0.55]);
                self.pointer = Some(Pointer::Tile(at));
                self.from_pointer = self.pointer;
                self.down = Some(if matches!(op, Orbit(..)) { Button::Right } else { Button::Pan });
            }
            Zoom(_) => {
                let at = self.pointer().unwrap_or([0.5, 0.55]);
                self.pointer = Some(Pointer::Tile(at));
                self.down = None;
            }
            Tab => {
                let before = self.scene.origin;
                let objects = self.scene.world().map(|w| w.objects()).unwrap_or_default();
                let switched = match self.scene.at() {
                    vxr_core::history::At::World => objects.first().is_some_and(|&o| self.scene.edit_object(o)),
                    _ => self.scene.show_world(),
                };
                if switched {
                    self.follow(before);
                }
                self.pointer = None;
            }
            Color(v) => self.color = v,
            Mirror(on) => self.mirror = on,
            Point(..) | MoveTo(_) | Wait => {}
        }
    }

    fn tween(&mut self, i: usize, f: f32) {
        let e = ease(f);
        match self.steps[i].op {
            Point(cell, normal) => {
                let to = self.face(cell, normal);
                self.pointer = Some(match self.from_pointer {
                    Some(Pointer::At(a)) => Pointer::At(a.lerp(to, e)),
                    _ => Pointer::At(to),
                });
            }
            Drag(p) => {
                let p = self.cell(p);
                let c = lerp_cell(self.from_cell, p, f);
                if let Some(g) = &mut self.gesture {
                    g.move_to(&mut self.scene, c);
                    let n: [i32; 3] = std::array::from_fn(|a| (a == g.axis) as i32);
                    let on = g.on_plane();
                    // On the drag plane the pointer is on the face below the cell drawn; a
                    // surface stroke (erase, paint) points at the voxel's own face.
                    let mut q = Vec3::from_array(std::array::from_fn(|a| c[a] as f32 + 0.5));
                    let side = if on && g.brush().tool == Tool::Add { -0.5 } else { 0.5 };
                    q += Vec3::from_array(std::array::from_fn(|a| n[a] as f32 * side));
                    self.pointer = Some(Pointer::At(q));
                }
            }
            LiftTo(h) => {
                if let Some(g) = &mut self.gesture {
                    let a = g.axis;
                    let from = self.from_cell[a];
                    let to = from + ((h - from) as f32 * f).round() as i32;
                    g.lift_to(&mut self.scene, to);
                    let mut q = Vec3::from_array(std::array::from_fn(|k| g.end[k] as f32 + 0.5));
                    q[a] = to as f32 + 1.0;
                    self.pointer = Some(Pointer::At(q));
                }
            }
            Place if f >= 1.0 => self.down = None,
            Select(lo, hi) => {
                let (lo, hi) = (self.cell(lo), self.cell(hi));
                let end = lerp_cell(lo, hi, f);
                let (a, b): ([i32; 3], [i32; 3]) =
                    (std::array::from_fn(|k| lo[k].min(end[k])), std::array::from_fn(|k| lo[k].max(end[k])));
                let map = &self.scene.map;
                let cells: Vec<[i32; 3]> = box_cells(a, b, false).filter(|p| map.get(p[0], p[1], p[2]) != 0).collect();
                self.scene.select(cells, Mode::Replace);
                let top = Vec3::from_array(std::array::from_fn(|k| end[k] as f32 + if k == 1 { 1.0 } else { 0.5 }));
                self.pointer = Some(Pointer::At(top));
                if f >= 1.0 {
                    self.down = None;
                }
            }
            MoveTo(p) => {
                let p = self.cell(p);
                let c = lerp_cell(self.from_cell, p, f);
                if let Some(m) = &mut self.moving {
                    m.move_to(&mut self.scene, c);
                    self.pointer =
                        Some(Pointer::At(Vec3::new(c[0] as f32 + 0.5, c[1] as f32 + 1.0, c[2] as f32 + 0.5)));
                }
            }
            Orbit(yaw, pitch) => {
                self.cam.yaw = self.from_cam.yaw + yaw * e;
                self.cam.pitch = (self.from_cam.pitch + pitch * e).clamp(-1.5, 1.5);
                if let Some(Pointer::Tile(p)) = self.from_pointer {
                    // The pointer drags the opposite way to the yaw, as in the editor.
                    self.pointer = Some(Pointer::Tile([p[0] - yaw * 0.25 * e, p[1] - pitch * 0.25 * e]));
                }
                if f >= 1.0 {
                    self.down = None;
                }
            }
            Pan(x, y) => {
                let (_, right, up) = self.from_cam.basis();
                self.cam.target = self.from_cam.target - (right * x + up * y) * e;
                if let Some(Pointer::Tile(p)) = self.from_pointer {
                    let h = 2.0 * self.from_cam.half_height();
                    self.pointer = Some(Pointer::Tile([p[0] + x / (h * self.aspect) * e, p[1] + y / h * e]));
                }
                if f >= 1.0 {
                    self.down = None;
                }
            }
            Zoom(steps) => self.cam.distance = (self.from_cam.distance * 0.88f32.powf(steps * e)).max(1.0),
            _ => {}
        }
    }
}

fn ease(f: f32) -> f32 {
    f * f * (3.0 - 2.0 * f)
}

fn lerp_cell(a: [i32; 3], b: [i32; 3], f: f32) -> [i32; 3] {
    std::array::from_fn(|k| a[k] + ((b[k] - a[k]) as f32 * f).round() as i32)
}

/// The camera a demo starts at: the platform a little from above, filling most of the tile.
fn framing(scene: &Scene, aspect: f32) -> OrbitCamera {
    let s = scene.map.size_voxels();
    let (lo, hi) = (Vec3::new(1.0, 0.0, 1.0), Vec3::new(s[0] as f32 - 1.0, 6.0, s[2] as f32 - 1.0));
    let mut c = OrbitCamera::framing_aspect(lo, hi, aspect);
    c.pitch = 0.5;
    c.fit(lo, hi, aspect, 0.92);
    c
}

/// Palette slots the demo scenes use, set to colours that read on every theme.
const GRASS: u8 = 1;
const DIRT: u8 = 2;
const STONE: u8 = 3;
const WOOD: u8 = 4;
const LEAF: u8 = 5;
const ROOF: u8 = 6;
const WATER: u8 = 7;
const SAND: u8 = 8;
const PINK: u8 = 9;

/// Map side of every demo scene, in voxels.
const SIDE: u32 = 16;

/// The ground every demo starts from: a grass platform with a dirt edge.
fn ground() -> Scene {
    let mut s = Scene::new([SIDE, 10, SIDE]);
    let colors: [(u8, [u8; 4]); 9] = [
        (GRASS, [111, 176, 92, 255]),
        (DIRT, [139, 101, 72, 255]),
        (STONE, [160, 160, 168, 255]),
        (WOOD, [176, 122, 76, 255]),
        (LEAF, [72, 140, 84, 255]),
        (ROOF, [214, 92, 74, 255]),
        (WATER, [86, 150, 218, 255]),
        (SAND, [232, 206, 140, 255]),
        (PINK, [226, 120, 178, 255]),
    ];
    for (i, c) in colors {
        s.palette[i as usize] = c;
    }
    s.begin();
    for x in 2..14 {
        for z in 2..14 {
            let edge = x == 2 || x == 13 || z == 2 || z == 13;
            s.put([x, 0, z], if edge { DIRT } else { GRASS });
        }
    }
    s.commit();
    s
}

fn fill(s: &mut Scene, lo: [i32; 3], hi: [i32; 3], v: u8) {
    s.begin();
    for p in box_cells(lo, hi, false) {
        s.put(p, v);
    }
    s.commit();
}

/// The ground with a small house and a tree.
fn house() -> Scene {
    let mut s = ground();
    fill(&mut s, [4, 1, 5], [8, 3, 9], SAND);
    fill(&mut s, [4, 4, 5], [8, 4, 9], ROOF);
    fill(&mut s, [5, 5, 6], [7, 5, 8], ROOF);
    fill(&mut s, [6, 1, 9], [6, 2, 9], WOOD);
    fill(&mut s, [11, 1, 10], [11, 2, 10], WOOD);
    fill(&mut s, [10, 3, 9], [12, 4, 11], LEAF);
    s
}

/// The ground with a crate of stone and wood.
fn crate_scene() -> Scene {
    let mut s = ground();
    fill(&mut s, [4, 1, 5], [6, 3, 7], WOOD);
    fill(&mut s, [4, 3, 5], [6, 3, 7], STONE);
    s
}

/// The ground with a low wall along it.
fn wall() -> Scene {
    let mut s = ground();
    fill(&mut s, [3, 1, 7], [12, 2, 8], STONE);
    s
}

/// The ground with a block on it.
fn block() -> Scene {
    let mut s = ground();
    fill(&mut s, [5, 1, 5], [10, 3, 10], SAND);
    s
}

/// A world of two objects: the ground with a house, and a tree beside it.
fn world_scene() -> Scene {
    let mut s = house();
    let tree: Vec<[i32; 3]> =
        box_cells([10, 1, 9], [12, 4, 11], false).filter(|p| s.map.get(p[0], p[1], p[2]) != 0).collect();
    s.select(tree, Mode::Replace);
    let _ = s.selection_to_object();
    s.deselect();
    s.show_world();
    s
}

fn step(ms: f32, op: Op) -> Step {
    Step { ms, op }
}

fn add_steps() -> Vec<Step> {
    vec![
        step(300.0, Key("A", "A")),
        step(500.0, Point([4, 0, 8], UP)),
        step(250.0, Press(Tool::Add, Shape::Voxel, [4, 0, 8], UP)),
        step(900.0, Drag([10, 1, 8])),
        step(350.0, Drag([10, 1, 5])),
        step(300.0, Up),
        step(1400.0, Wait),
    ]
}

fn orbit_steps() -> Vec<Step> {
    vec![
        step(200.0, Key("Right-drag", "Two fingers")),
        step(1300.0, Orbit(0.9, 0.15)),
        step(250.0, Wait),
        step(1300.0, Orbit(-0.9, -0.15)),
        step(200.0, Key("Scroll", "Pinch")),
        step(600.0, Zoom(3.0)),
        step(600.0, Zoom(-3.0)),
        step(200.0, Key("⇧ Right-drag", "Three fingers")),
        step(700.0, Pan(2.5, 0.0)),
        step(700.0, Pan(-2.5, 0.0)),
        step(600.0, Wait),
    ]
}

fn box_steps() -> Vec<Step> {
    vec![
        step(300.0, Key("B", "B")),
        step(500.0, Point([4, 0, 4], UP)),
        step(250.0, Press(Tool::Add, Shape::Box, [4, 0, 4], UP)),
        step(900.0, Drag([9, 1, 9])),
        step(200.0, Key("Let go", "Lift")),
        step(300.0, Up),
        step(200.0, Key("Move up", "Drag up")),
        step(900.0, LiftTo(4)),
        step(200.0, Key("Click", "Tap")),
        step(300.0, Place),
        step(1400.0, Wait),
    ]
}

fn select_steps() -> Vec<Step> {
    vec![
        step(300.0, Key("S", "S")),
        step(800.0, Select([3, 1, 4], [7, 3, 8])),
        step(400.0, Wait),
        step(200.0, Key("Drag it", "Drag it")),
        step(400.0, Point([5, 3, 6], UP)),
        step(250.0, Grab([5, 3, 6])),
        step(900.0, MoveTo([10, 3, 9])),
        step(250.0, Drop),
        step(1400.0, Wait),
    ]
}

fn world_steps() -> Vec<Step> {
    vec![
        step(800.0, Wait),
        step(250.0, Key("Tab", "Tab")),
        step(700.0, Tab),
        step(200.0, Key("A", "A")),
        step(400.0, Point([6, 5, 7], UP)),
        step(250.0, Press(Tool::Add, Shape::Voxel, [6, 5, 7], UP)),
        step(400.0, Drag([6, 6, 7])),
        step(250.0, Up),
        step(500.0, Wait),
        step(250.0, Key("Tab", "Tab")),
        step(1400.0, Tab),
    ]
}

fn erase_steps() -> Vec<Step> {
    vec![
        step(300.0, Key("E", "E")),
        step(500.0, Point([4, 2, 8], UP)),
        step(250.0, Press(Tool::Erase, Shape::Voxel, [4, 2, 8], UP)),
        step(900.0, Drag([11, 2, 8])),
        step(300.0, Up),
        step(1400.0, Wait),
    ]
}

fn paint_steps() -> Vec<Step> {
    vec![
        step(300.0, Key("P · F", "P · F")),
        step(150.0, Color(ROOF)),
        step(500.0, Point([7, 3, 7], UP)),
        step(300.0, Press(Tool::Paint, Shape::Face, [7, 3, 7], UP)),
        step(200.0, Up),
        step(150.0, Color(WATER)),
        step(500.0, Point([7, 2, 10], [0, 0, 1])),
        step(300.0, Press(Tool::Paint, Shape::Face, [7, 2, 10], [0, 0, 1])),
        step(200.0, Up),
        step(1400.0, Wait),
    ]
}

fn pick_steps() -> Vec<Step> {
    vec![
        step(300.0, Key("I", "I")),
        step(500.0, Point([6, 4, 9], [0, 0, 1])),
        step(300.0, Press(Tool::Pick, Shape::Voxel, [6, 4, 9], [0, 0, 1])),
        step(200.0, Up),
        step(200.0, Key("A", "A")),
        step(500.0, Point([4, 0, 12], UP)),
        step(250.0, Press(Tool::Add, Shape::Voxel, [4, 0, 12], UP)),
        step(700.0, Drag([11, 1, 12])),
        step(300.0, Up),
        step(1400.0, Wait),
    ]
}

fn extrude_steps() -> Vec<Step> {
    vec![
        step(300.0, Key("A · F", "A · F")),
        step(150.0, Color(SAND)),
        step(500.0, Point([7, 3, 7], UP)),
        step(300.0, Press(Tool::Add, Shape::Face, [7, 3, 7], UP)),
        step(300.0, Up),
        step(300.0, Point([7, 4, 7], UP)),
        step(300.0, Press(Tool::Add, Shape::Face, [7, 4, 7], UP)),
        step(300.0, Up),
        step(150.0, Color(ROOF)),
        step(300.0, Point([7, 5, 7], UP)),
        step(300.0, Press(Tool::Add, Shape::Face, [7, 5, 7], UP)),
        step(300.0, Up),
        step(1400.0, Wait),
    ]
}

fn line_steps() -> Vec<Step> {
    vec![
        step(300.0, Key("L", "L")),
        step(150.0, Color(STONE)),
        step(500.0, Point([3, 0, 11], UP)),
        step(250.0, Press(Tool::Add, Shape::Line, [3, 0, 11], UP)),
        step(1000.0, Drag([12, 1, 4])),
        step(300.0, Up),
        step(1400.0, Wait),
    ]
}

fn round_steps() -> Vec<Step> {
    vec![
        step(300.0, Key("C", "C")),
        step(150.0, Color(WATER)),
        step(500.0, Point([8, 0, 8], UP)),
        step(250.0, Press(Tool::Add, Shape::Circle, [8, 0, 8], UP)),
        step(700.0, Drag([12, 1, 8])),
        step(300.0, Up),
        step(300.0, Key("C again", "C again")),
        step(150.0, Color(PINK)),
        step(400.0, Point([8, 1, 8], UP)),
        step(250.0, Press(Tool::Add, Shape::Sphere, [8, 1, 8], UP)),
        step(600.0, Drag([8, 2, 10])),
        step(300.0, Up),
        step(1400.0, Wait),
    ]
}

fn mirror_steps() -> Vec<Step> {
    vec![
        step(300.0, Key("M", "M")),
        step(150.0, Mirror(true)),
        step(150.0, Color(PINK)),
        step(500.0, Point([4, 0, 4], UP)),
        step(250.0, Press(Tool::Add, Shape::Voxel, [4, 0, 4], UP)),
        step(500.0, Drag([4, 1, 11])),
        step(400.0, Drag([6, 1, 11])),
        step(300.0, Up),
        step(1400.0, Wait),
    ]
}

/// A player's GPU half: its map, its renderer (a sibling of the editor's) and the egui
/// texture of its image.
struct Tile {
    map: GpuMap,
    renderer: Renderer,
    texture: egui::TextureId,
    /// What the image shows, to march again only when something changed.
    shown: Option<Shown>,
    reshapes: u64,
}

#[derive(Clone, Copy, PartialEq)]
struct Shown {
    cam: [f32; 8],
    selection: u64,
    colors: vxr_render::ViewColors,
}

fn shown_of(p: &Player, colors: vxr_render::ViewColors) -> Shown {
    let c = &p.cam;
    Shown {
        cam: [c.target.x, c.target.y, c.target.z, c.yaw, c.pitch, c.distance, c.fov_y, c.ortho as u8 as f32],
        selection: p.scene.selection_version(),
        colors,
    }
}

/// The players and their renders, made when Help first shows and kept (a few hundred KB of
/// GPU memory) so opening it again costs nothing.
pub struct Tiles {
    pub players: Vec<Player>,
    tiles: Vec<Option<Tile>>,
    /// Pixel size of every tile's image.
    pub size: (u32, u32),
    /// Images marched in the last frame, for the checks.
    pub marched: usize,
}

impl Tiles {
    /// A player for every demo, at `size` pixels; renders come later, once a tile shows.
    pub fn new(size: (u32, u32)) -> Self {
        let aspect = size.0 as f32 / size.1.max(1) as f32;
        Self {
            players: (0..DEMOS.len()).map(|i| Player::new(i, aspect)).collect(),
            tiles: (0..DEMOS.len()).map(|_| None).collect(),
            size,
            marched: 0,
        }
    }

    /// The egui texture of demo `i`'s image, once it has one.
    pub fn texture(&self, i: usize) -> Option<egui::TextureId> {
        self.tiles.get(i)?.as_ref().map(|t| t.texture)
    }

    /// Move the players in `shown` to `now` (seconds, egui's clock).
    pub fn advance(&mut self, shown: &[usize], now: f64) {
        for &i in shown {
            if let Some(p) = self.players.get_mut(i) {
                p.advance(now);
            }
        }
    }

    /// Images of another size (the panel changed shape): the demos play on, framed again, and
    /// each render is resized the next time it draws.
    pub fn resize(&mut self, size: (u32, u32)) {
        let aspect = size.0 as f32 / size.1.max(1) as f32;
        for p in &mut self.players {
            p.set_aspect(aspect);
        }
        self.size = size;
    }

    /// Start every demo again from its first step (Help closed, so it opens on the start).
    pub fn rewind(&mut self) {
        for p in &mut self.players {
            p.rewind();
        }
    }

    /// March the images in `shown` whose scene, camera, selection or theme changed since
    /// they were last drawn; a tile shown for the first time gets its map and renderer.
    #[allow(clippy::too_many_arguments)]
    pub fn encode(
        &mut self,
        gpu: &vxr_render::Gpu,
        layout: &wgpu::BindGroupLayout,
        main: &Renderer,
        egui: &mut egui_wgpu::Renderer,
        enc: &mut wgpu::CommandEncoder,
        shown: &[usize],
        line: f32,
    ) {
        let (dev, queue) = (&gpu.device, &gpu.queue);
        self.marched = 0;
        for &i in shown {
            let Some(p) = self.players.get_mut(i) else { continue };
            let slot = &mut self.tiles[i];
            if slot.is_none() {
                p.scene.map.track_dirty(true);
                let Ok(map) = GpuMap::new(dev, queue, layout, &p.scene.map, &p.scene.palette) else { continue };
                let renderer = main.sibling(dev, self.size.0, self.size.1);
                let view = renderer.color.create_view(&Default::default());
                let texture = egui.register_native_texture(dev, &view, wgpu::FilterMode::Linear);
                *slot = Some(Tile { map, renderer, texture, shown: None, reshapes: p.scene.reshapes() });
            }
            let t = slot.as_mut().expect("made above");
            if (t.renderer.width, t.renderer.height) != self.size {
                t.renderer.resize(dev, self.size.0, self.size.1);
                let view = t.renderer.color.create_view(&Default::default());
                egui.update_egui_texture_from_wgpu_texture(dev, &view, wgpu::FilterMode::Linear, t.texture);
                t.shown = None;
            }
            // A loop builds a new scene, and a world switch a new map: upload it whole.
            if !p.scene.map.tracking() || t.reshapes != p.scene.reshapes() {
                p.scene.map.track_dirty(true);
                if t.map.reupload(dev, queue, &p.scene.map, &p.scene.palette).is_err() {
                    continue;
                }
                p.scene.map.take_dirty();
                p.scene.map.take_dirty_box();
                t.reshapes = p.scene.reshapes();
                t.shown = None;
            }
            let dirty = !p.scene.map.dirty_bricks.is_empty() || !p.scene.map.dirty_cells.is_empty();
            if dirty {
                if t.map.upload_dirty(dev, queue, &mut p.scene.map, &p.scene.palette).is_err() {
                    continue;
                }
                p.scene.map.take_dirty_box();
            }
            let now_shown = shown_of(p, main.colors);
            if !dirty && t.shown == Some(now_shown) {
                continue;
            }
            if t.shown.is_none_or(|s| s.selection != now_shown.selection) || dirty {
                t.renderer.set_selection(dev, queue, &p.scene.selection_mask());
            }
            t.shown = Some(now_shown);
            t.renderer.colors = main.colors;
            t.renderer.guides = vxr_render::Guides {
                bounds: p.scene.map.size_voxels(),
                floor: p.scene.floor,
                grid: false,
                ground: true,
                frame: false,
                line,
            };
            t.renderer.set_camera(queue, &p.cam, None);
            t.renderer.march(enc, &t.map, None);
            self.marched += 1;
        }
    }

    /// Free the egui textures (the editor is going away or the tiles are made again).
    pub fn free(&mut self, egui: &mut egui_wgpu::Renderer) {
        for t in self.tiles.iter_mut().filter_map(Option::take) {
            egui.free_texture(&t.texture);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Play demo `id` to the end of its first loop, a frame at a time.
    fn played(id: &str) -> Player {
        let mut p = Player::new(demo(id).expect("a demo"), 2.0);
        let mut t = 0.0;
        while t < p.length() - 0.05 {
            p.advance(t);
            t += 1.0 / 60.0;
        }
        p
    }

    #[test]
    fn every_demo_changes_its_scene_or_camera_and_loops_back() {
        for d in DEMOS {
            let start = Player::new(demo(d.id).unwrap(), 2.0);
            let mut p = played(d.id);
            let moved = p.scene.map.voxels != start.scene.map.voxels
                || p.scene.map.counts != start.scene.map.counts
                || p.scene.selection_version() != start.scene.selection_version();
            assert!(moved || d.id == "orbit", "{}: the demo shows no edit", d.id);
            p.advance(p.length() + 0.01);
            assert_eq!(p.loops, 1, "{}: one loop played", d.id);
            assert_eq!(p.scene.map.voxels, start.scene.map.voxels, "{}: the loop starts on its scene again", d.id);
        }
    }

    #[test]
    fn box_then_lift_places_a_box_six_by_five_and_four_high() {
        let p = played("box");
        let base = Player::new(demo("box").unwrap(), 2.0).scene.map.voxels;
        assert_eq!(p.scene.map.voxels - base, 6 * 6 * 4);
        assert!(p.scene.map.get(9, 4, 9) != 0 && p.scene.map.get(9, 5, 9) == 0);
    }

    #[test]
    fn select_and_move_moves_the_crate_on_the_ground_plane() {
        let p = played("select");
        assert_eq!(p.scene.map.get(5, 1, 6), 0, "the crate left its place");
        assert_ne!(p.scene.map.get(10, 1, 9), 0, "and is where the pointer let go");
        assert_eq!(p.scene.selected_count(), 27);
    }

    #[test]
    fn world_demo_edits_a_model_and_ends_in_the_world() {
        let p = played("world");
        assert_eq!(p.scene.at(), vxr_core::history::At::World);
        assert_eq!(p.scene.world().map(|w| w.objects().len()), Some(2));
    }

    #[test]
    fn orbit_ends_where_it_started() {
        let p = played("orbit");
        let home = Player::new(demo("orbit").unwrap(), 2.0).cam;
        assert!((p.cam.yaw - home.yaw).abs() < 1e-4 && (p.cam.distance - home.distance).abs() < 1e-3);
        assert!((p.cam.target - home.target).length() < 1e-3);
    }

    #[test]
    fn mirror_draws_both_sides() {
        let p = played("mirror");
        let m = p.scene.mirror([4, 1, 4], 0);
        assert_ne!(p.scene.map.get(4, 1, 4), 0);
        assert_ne!(p.scene.map.get(m[0], m[1], m[2]), 0);
    }

    #[test]
    fn search_finds_by_name_keys_and_words() {
        assert_eq!(search("box", false), vec![demo("box").unwrap()]);
        assert!(search("right-drag", false).contains(&demo("orbit").unwrap()));
        assert!(search("pinch", true).contains(&demo("orbit").unwrap()));
        assert!(search("tab", false).contains(&demo("world").unwrap()));
        assert_eq!(search("", false).len(), DEMOS.len());
        assert!(search("nothing like this", false).is_empty());
    }

    #[test]
    fn pointer_stays_on_the_tile() {
        for d in DEMOS {
            let mut p = Player::new(demo(d.id).unwrap(), 2.0);
            let mut t = 0.0;
            while t < p.length() {
                p.advance(t);
                if let Some([x, y]) = p.pointer() {
                    assert!(
                        (-0.02..=1.02).contains(&x) && (-0.02..=1.02).contains(&y),
                        "{} at {t:.2}s: {x}, {y}",
                        d.id
                    );
                }
                t += 1.0 / 30.0;
            }
        }
    }
}
