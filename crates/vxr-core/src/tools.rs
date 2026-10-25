//! The editing tools, as in the prototype editor (`prototype/src/editor.html`) and
//! MagicaVoxel: add, erase, paint and pick, each with the voxel, box, face, line or center
//! (circle or sphere) brush, an optional mirror on any of the three axes ([`Scene::pivot`])
//! and an optional axis lock that keeps drags on a plane across that axis. A gesture is a
//! press, any number of moves and a release; the whole gesture is one undo step. All
//! positions are map cells.

use crate::Scene;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Tool {
    Add,
    Erase,
    Paint,
    Pick,
    /// Choose voxels for [`crate::select`]; presses do nothing to the scene.
    Select,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Shape {
    Voxel,
    Box,
    Face,
    /// A straight line from the press to the pointer.
    Line,
    /// A disc on the drag plane, centred on the press, out to the pointer.
    Circle,
    /// A ball centred on the press, out to the pointer.
    Sphere,
}

impl Shape {
    /// Shapes drawn whole from the press to the pointer, redrawn live as the pointer moves.
    pub fn is_span(self) -> bool {
        matches!(self, Shape::Box | Shape::Line | Shape::Circle | Shape::Sphere)
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Brush {
    pub tool: Tool,
    pub shape: Shape,
    /// Mirror across the plane of each axis ([`Scene::pivot`]).
    pub mirror: [bool; 3],
    /// Keep drags on the plane across this axis through the pressed cell, whatever face was
    /// clicked (MagicaVoxel's brush "axis" option).
    pub lock: Option<usize>,
    /// Palette index that add and paint use.
    pub color: u8,
}

/// What a ray points at: a voxel face, or the ground plane under the model.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Target {
    /// The voxel hit. On the ground it is the empty cell just below the floor layer.
    pub voxel: [i32; 3],
    /// Unit normal of the face hit.
    pub normal: [i32; 3],
    /// Palette index of the voxel, 0 on the ground.
    pub value: u8,
    pub ground: bool,
}

impl Target {
    /// The empty cell in front of the face, where add puts a voxel.
    pub fn front(&self) -> [i32; 3] {
        add(self.voxel, self.normal)
    }
}

/// Most cells a face brush changes in one click.
pub const MAX_FACE: usize = 120_000;

/// Largest circle or sphere radius, in voxels (a ball of about 1.1 million cells).
pub const MAX_RADIUS: i32 = 64;

/// What a press did.
#[derive(Debug)]
pub enum Press {
    /// Nothing to do there (for example erasing the ground).
    Nothing,
    /// Pick took this palette index.
    Picked(u8),
    /// The edit is done (face brush): one undo step.
    Done(usize),
    /// A drag started; feed it moves, then release it.
    Drag(Gesture),
}

/// A drag in progress. Add, span-shape and locked drags stay on the plane of the face first
/// pressed (or across the locked axis); erase and paint voxel drags follow the surface
/// under the pointer.
#[derive(Clone, Debug)]
pub struct Gesture {
    brush: Brush,
    /// Axis and layer of the drag plane.
    pub axis: usize,
    pub layer: i32,
    pub start: [i32; 3],
    pub end: [i32; 3],
    last: [i32; 3],
    /// The radius a circle or sphere was last drawn at.
    drawn: i32,
}

impl Scene {
    /// The voxel face or ground point a ray in map space points at.
    pub fn target(&self, ro: [f32; 3], dir: [f32; 3]) -> Option<Target> {
        if let Some(h) = self.map.raycast(ro, dir, 1e5) {
            return Some(Target { voxel: h.voxel, normal: h.normal, value: h.value, ground: false });
        }
        if dir[1] >= -1e-6 {
            return None;
        }
        let t = (self.floor as f32 - ro[1]) / dir[1];
        if t < 0.0 {
            return None;
        }
        let x = (ro[0] + dir[0] * t).floor() as i32;
        let z = (ro[2] + dir[2] * t).floor() as i32;
        let size = self.map.size_voxels();
        if x < 0 || z < 0 || x >= size[0] as i32 || z >= size[2] as i32 {
            return None;
        }
        Some(Target { voxel: [x, self.floor - 1, z], normal: [0, 1, 0], value: 0, ground: true })
    }

    /// Mirror of a map cell across the plane of `axis` ([`Scene::pivot`]).
    pub fn mirror(&self, p: [i32; 3], axis: usize) -> [i32; 3] {
        let mut m = p;
        m[axis] = self.pivot[axis] - 1 - p[axis] - 2 * self.origin[axis];
        m
    }

    /// Mirror of a map cell across world x = 0.
    pub fn mirror_x(&self, p: [i32; 3]) -> [i32; 3] {
        self.mirror(p, 0)
    }

    /// Set a cell and its mirror images: up to eight cells with all three axes on.
    fn put_brush(&mut self, brush: &Brush, p: [i32; 3], v: u8) {
        let mut cells = [p; 8];
        let mut n = 1;
        for a in 0..3 {
            if brush.mirror[a] {
                for i in 0..n {
                    cells[n + i] = self.mirror(cells[i], a);
                }
                n *= 2;
            }
        }
        for (i, &c) in cells[..n].iter().enumerate() {
            if !cells[..i].contains(&c) {
                self.put(c, v);
            }
        }
    }

    /// One cell of a voxel or box stroke: add fills, erase and paint touch only filled cells.
    fn apply_voxel(&mut self, brush: &Brush, p: [i32; 3]) {
        match brush.tool {
            Tool::Add => self.put_brush(brush, p, brush.color),
            Tool::Erase | Tool::Paint if self.map.get(p[0], p[1], p[2]) != 0 => {
                let v = if brush.tool == Tool::Erase { 0 } else { brush.color };
                self.put_brush(brush, p, v);
            }
            _ => {}
        }
    }

    /// The face under a target: connected voxels of the same colour in its layer whose face
    /// along the normal is open, up to [`MAX_FACE`] cells.
    pub fn face_region(&self, t: &Target) -> Vec<[i32; 3]> {
        let n = t.normal;
        let a = axis_of(n);
        let (u, v) = ((a + 1) % 3, (a + 2) % 3);
        let get = |p: [i32; 3]| self.map.get(p[0], p[1], p[2]);
        let open = |p: [i32; 3]| get(p) == t.value && get(add(p, n)) == 0;
        let mut seen = std::collections::HashSet::new();
        let (mut out, mut stack) = (Vec::new(), vec![t.voxel]);
        while let Some(p) = stack.pop() {
            if out.len() >= MAX_FACE {
                break;
            }
            if !seen.insert(p) || !open(p) {
                continue;
            }
            out.push(p);
            for (du, dv) in [(1, 0), (-1, 0), (0, 1), (0, -1)] {
                let mut q = p;
                q[u] += du;
                q[v] += dv;
                stack.push(q);
            }
        }
        out
    }

    /// Press with a brush on a target.
    pub fn press(&mut self, brush: Brush, t: Target) -> Press {
        if brush.tool == Tool::Select {
            return Press::Nothing;
        }
        if brush.tool == Tool::Pick {
            return if t.value != 0 { Press::Picked(t.value) } else { Press::Nothing };
        }
        if t.ground && brush.tool != Tool::Add {
            return Press::Nothing;
        }
        self.commit();
        if brush.shape == Shape::Face {
            if t.ground {
                return Press::Nothing;
            }
            self.begin();
            for p in self.face_region(&t) {
                match brush.tool {
                    Tool::Add => self.put_brush(&brush, add(p, t.normal), brush.color),
                    Tool::Erase => self.put_brush(&brush, p, 0),
                    _ => self.put_brush(&brush, p, brush.color),
                }
            }
            return Press::Done(self.commit());
        }
        let start = if brush.tool == Tool::Add { t.front() } else { t.voxel };
        let axis = brush.lock.unwrap_or(axis_of(t.normal));
        self.begin();
        let g = Gesture { brush, axis, layer: start[axis], start, end: start, last: start, drawn: 0 };
        if brush.shape == Shape::Voxel {
            self.apply_voxel(&brush, start);
        } else {
            g.draw(self);
        }
        Press::Drag(g)
    }
}

impl Gesture {
    /// Whether moves should be picked on the drag plane ([`plane_cell`]) rather than on the
    /// surface ([`Scene::target`], taking its `voxel`).
    pub fn on_plane(&self) -> bool {
        self.brush.tool == Tool::Add || self.brush.shape != Shape::Voxel || self.brush.lock.is_some()
    }

    pub fn brush(&self) -> Brush {
        self.brush
    }

    /// The pointer moved over cell `p`. Voxel strokes fill the gap from the last cell so fast
    /// moves stay continuous; span shapes (box, line, circle, sphere) are redrawn from the
    /// press to `p`, so the scene shows the shape live. A circle or sphere is redrawn only
    /// when its radius changes, and a growing one only adds the new shell.
    pub fn move_to(&mut self, scene: &mut Scene, p: [i32; 3]) {
        if p == self.last {
            return;
        }
        if self.brush.shape == Shape::Voxel {
            let l = self.last;
            let steps = if self.on_plane() { (0..3).map(|a| (p[a] - l[a]).abs()).max().unwrap() } else { 1 };
            for q in line(l, p, steps).skip(1) {
                scene.apply_voxel(&self.brush, q);
            }
        }
        self.end = p;
        self.last = p;
        if matches!(self.brush.shape, Shape::Circle | Shape::Sphere) {
            let r = self.radius();
            if r > self.drawn {
                self.draw_round(scene, Some(self.drawn));
            } else if r < self.drawn {
                scene.revert_voxels();
                self.draw(scene);
            }
            self.drawn = r;
        } else if self.brush.shape.is_span() {
            scene.revert_voxels();
            self.draw(scene);
        }
    }

    /// Radius of a circle or sphere: the distance from the press to the pointer, rounded.
    pub fn radius(&self) -> i32 {
        let d2: i32 = (0..3).map(|a| (self.end[a] - self.start[a]).pow(2)).sum();
        ((d2 as f32).sqrt().round() as i32).min(MAX_RADIUS)
    }

    /// The bounds of what the drag draws, inclusive.
    pub fn extent(&self) -> ([i32; 3], [i32; 3]) {
        match self.brush.shape {
            Shape::Circle | Shape::Sphere => {
                let r = self.radius();
                let flat = |a: usize| self.brush.shape == Shape::Circle && a == self.axis;
                (
                    std::array::from_fn(|a| if flat(a) { self.start[a] } else { self.start[a] - r }),
                    std::array::from_fn(|a| if flat(a) { self.start[a] } else { self.start[a] + r }),
                )
            }
            _ => (
                std::array::from_fn(|a| self.start[a].min(self.end[a])),
                std::array::from_fn(|a| self.start[a].max(self.end[a])),
            ),
        }
    }

    /// Apply a span shape from the press to the current end.
    fn draw(&self, scene: &mut Scene) {
        let (s, b) = (self.start, &self.brush);
        match b.shape {
            Shape::Box => {
                let (lo, hi) = self.extent();
                for z in lo[2]..=hi[2] {
                    for y in lo[1]..=hi[1] {
                        for x in lo[0]..=hi[0] {
                            scene.apply_voxel(b, [x, y, z]);
                        }
                    }
                }
            }
            Shape::Line => {
                let steps = (0..3).map(|a| (self.end[a] - s[a]).abs()).max().unwrap();
                for q in line(s, self.end, steps) {
                    scene.apply_voxel(b, q);
                }
            }
            Shape::Circle | Shape::Sphere => self.draw_round(scene, None),
            Shape::Voxel | Shape::Face => {}
        }
    }

    /// Apply a circle or sphere at the current radius, leaving out the cells of radius
    /// `inside` when it is already drawn. Cells within r^2 + r/2 of the press: radius 1 is a
    /// plus, radius 2 a round disc.
    fn draw_round(&self, scene: &mut Scene, inside: Option<i32>) {
        let reach = |r: i32| (r * r) as f32 + r as f32 / 2.0;
        let (s, b) = (self.start, &self.brush);
        let r2 = reach(self.radius());
        let skip = inside.map_or(-1.0, reach);
        let (lo, hi) = self.extent();
        for z in lo[2]..=hi[2] {
            for y in lo[1]..=hi[1] {
                for x in lo[0]..=hi[0] {
                    let d = [x - s[0], y - s[1], z - s[2]];
                    let d2 = (d[0] * d[0] + d[1] * d[1] + d[2] * d[2]) as f32;
                    if d2 <= r2 && d2 > skip {
                        scene.apply_voxel(b, [x, y, z]);
                    }
                }
            }
        }
    }

    /// End the drag and close the undo step. Returns how many voxels the whole gesture changed.
    pub fn release(self, scene: &mut Scene) -> usize {
        scene.commit()
    }
}

/// The cells from `a` to `b` in `steps` equal steps, rounded, both ends included.
fn line(a: [i32; 3], b: [i32; 3], steps: i32) -> impl Iterator<Item = [i32; 3]> {
    (0..=steps.max(0)).map(move |s| {
        if steps == 0 {
            return a;
        }
        std::array::from_fn(|k| (a[k] as f32 + (b[k] - a[k]) as f32 * s as f32 / steps as f32).round() as i32)
    })
}

/// The cell where a ray crosses the middle of layer `layer` on `axis`, for plane drags.
pub fn plane_cell(ro: [f32; 3], dir: [f32; 3], axis: usize, layer: i32) -> Option<[i32; 3]> {
    if dir[axis].abs() < 1e-6 {
        return None;
    }
    let t = (layer as f32 + 0.5 - ro[axis]) / dir[axis];
    if t < 0.0 {
        return None;
    }
    Some(std::array::from_fn(|a| if a == axis { layer } else { (ro[a] + dir[a] * t).floor() as i32 }))
}

fn add(a: [i32; 3], b: [i32; 3]) -> [i32; 3] {
    [a[0] + b[0], a[1] + b[1], a[2] + b[2]]
}

fn axis_of(n: [i32; 3]) -> usize {
    if n[0] != 0 {
        0
    } else if n[1] != 0 {
        1
    } else {
        2
    }
}
