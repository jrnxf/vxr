//! The editing tools, as in the prototype editor (`prototype/src/editor.html`) and
//! MagicaVoxel: add, erase, paint and pick, each with the voxel, box, face, line or center
//! (circle or sphere) brush (the voxel brush in sizes up to [`MAX_SIZE`]), an optional mirror on any of the three axes ([`Scene::pivot`])
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
    /// Edge of the voxel brush's cube, 1 to [`MAX_SIZE`]: centred on the cell across the
    /// pressed face and as deep as it is wide, out of the face for add and into it for erase
    /// and paint.
    pub size: u8,
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

/// Most layers a box brush's height drag spans.
pub const MAX_HEIGHT: i32 = 256;

/// Largest voxel brush size.
pub const MAX_SIZE: u8 = 16;

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
    /// Normal of the face first pressed: the voxel brush's cube grows along it.
    normal: [i32; 3],
    /// The radius a circle or sphere was last drawn at.
    drawn: i32,
    /// A box whose base is drawn and whose height now follows the pointer along the drag
    /// axis ([`Gesture::lift`]), until a click or Enter places it.
    pub lifting: bool,
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

    /// One step of a voxel stroke at `p`: a single cell, or the brush's cube grown from it
    /// along `normal` (outward for add, inward for erase and paint).
    fn stamp(&mut self, brush: &Brush, p: [i32; 3], normal: [i32; 3]) {
        let n = brush.size.clamp(1, MAX_SIZE) as i32;
        if n == 1 {
            return self.apply_voxel(brush, p);
        }
        let a = axis_of(normal);
        let out = if brush.tool == Tool::Add { normal[a].signum() } else { -normal[a].signum() };
        let lo = -(n - 1) / 2;
        for i in 0..n {
            for j in lo..lo + n {
                for k in lo..lo + n {
                    let mut q = p;
                    q[a] += i * out;
                    q[(a + 1) % 3] += j;
                    q[(a + 2) % 3] += k;
                    self.apply_voxel(brush, q);
                }
            }
        }
    }

    /// The face under a target: connected voxels of the same colour in its layer whose face
    /// along the normal is open, up to [`MAX_FACE`] cells.
    pub fn face_region(&self, t: &Target) -> Vec<[i32; 3]> {
        face_cells(|p| self.map.get(p[0], p[1], p[2]), t.voxel, t.normal)
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
        let normal = t.normal;
        let g = Gesture {
            brush,
            axis,
            layer: start[axis],
            start,
            end: start,
            last: start,
            normal,
            drawn: 0,
            lifting: false,
        };
        if brush.shape == Shape::Voxel {
            self.stamp(&brush, start, normal);
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
            let steps = if self.on_plane() { line_steps(l, p) } else { 1 };
            let (lo, hi) = self.reach(scene);
            let n = self.brush.size.clamp(1, MAX_SIZE) as i32;
            let r = steps_near(l, p, steps, lo.map(|k| k - n), hi.map(|k| k + n));
            for s in (*r.start()).max(1)..=*r.end() {
                scene.stamp(&self.brush, line_cell(l, p, steps, s), self.normal);
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

    /// Box brush: the base rectangle is drawn; from now on the pointer sets the box's height
    /// along the drag axis ([`Gesture::lift_cell`], [`Gesture::lift_to`]) until the host
    /// releases the gesture, which places the box as one undo step with its base.
    pub fn lift(&mut self) {
        self.lifting = self.brush.shape == Shape::Box;
    }

    /// While lifting: the layer on the drag axis nearest a ray, measured on the line along
    /// that axis through the centre of the base's far corner (where the pointer let go, so
    /// the box starts one layer high). `None` when the ray runs along the axis, so the
    /// height cannot be read from it (a straight-on view).
    pub fn lift_cell(&self, ro: [f32; 3], dir: [f32; 3]) -> Option<i32> {
        let a = self.axis;
        let p: [f32; 3] =
            std::array::from_fn(|k| if k == a { self.layer as f32 + 0.5 } else { self.end[k] as f32 + 0.5 });
        let w: [f32; 3] = std::array::from_fn(|k| p[k] - ro[k]);
        let dot = |u: [f32; 3], v: [f32; 3]| u[0] * v[0] + u[1] * v[1] + u[2] * v[2];
        let (b, c) = (dir[a], dot(dir, dir));
        let den = c - b * b;
        if den < 1e-4 * c {
            return None;
        }
        // Closest points of the line p + s * axis and the ray ro + t * dir.
        let s = (b * dot(dir, w) - c * w[a]) / den;
        let h = (p[a] + s).floor() as i32;
        Some(h.clamp(self.layer - MAX_HEIGHT + 1, self.layer + MAX_HEIGHT - 1))
    }

    /// While lifting: make the box reach layer `h` on the drag axis, redrawing it live.
    pub fn lift_to(&mut self, scene: &mut Scene, h: i32) {
        if !self.lifting || self.end[self.axis] == h {
            return;
        }
        self.end[self.axis] = h;
        self.last = self.end;
        scene.revert_voxels();
        self.draw(scene);
    }

    /// Radius of a circle or sphere: the distance from the press to the pointer, rounded.
    pub fn radius(&self) -> i32 {
        // In i64: a pointer far out on the ground plane overflows the squares in i32.
        let d2: i64 = (0..3).map(|a| (self.end[a] as i64 - self.start[a] as i64).pow(2)).sum();
        ((d2 as f64).sqrt().round() as i64).min(MAX_RADIUS as i64) as i32
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

    /// The cells a brush can change, inclusive: the map's and, on a mirrored axis, their
    /// mirror images'. A drag toward the horizon reaches tens of thousands of cells out, and
    /// visiting each one froze the app, so every drawn shape and stroke is cut to these.
    fn reach(&self, scene: &Scene) -> ([i32; 3], [i32; 3]) {
        let size = scene.map.size_voxels();
        let (mut lo, mut hi) = ([0; 3], [0; 3]);
        for a in 0..3 {
            let (mut min, mut max) = (0, size[a] as i32 - 1);
            if self.brush.mirror[a] {
                let (m0, m1) = (scene.mirror([0; 3], a)[a], scene.mirror([max; 3], a)[a]);
                (min, max) = (min.min(m0).min(m1), max.max(m0).max(m1));
            }
            (lo[a], hi[a]) = (min, max);
        }
        (lo, hi)
    }

    /// [`Gesture::extent`] cut to [`Gesture::reach`]. None when nothing is left.
    fn drawn_extent(&self, scene: &Scene) -> Option<([i32; 3], [i32; 3])> {
        let (mut lo, mut hi) = self.extent();
        let (min, max) = self.reach(scene);
        for a in 0..3 {
            (lo[a], hi[a]) = (lo[a].max(min[a]), hi[a].min(max[a]));
            if lo[a] > hi[a] {
                return None;
            }
        }
        Some((lo, hi))
    }

    /// Apply a span shape from the press to the current end.
    fn draw(&self, scene: &mut Scene) {
        let (s, b) = (self.start, &self.brush);
        match b.shape {
            Shape::Box => {
                let Some((lo, hi)) = self.drawn_extent(scene) else { return };
                for p in box_cells(lo, hi, false) {
                    scene.apply_voxel(b, p);
                }
            }
            Shape::Line => {
                let (steps, (lo, hi)) = (line_steps(s, self.end), self.reach(scene));
                for k in steps_near(s, self.end, steps, lo, hi) {
                    scene.apply_voxel(b, line_cell(s, self.end, steps, k));
                }
            }
            Shape::Circle | Shape::Sphere => self.draw_round(scene, None),
            Shape::Voxel | Shape::Face => {}
        }
    }

    /// Apply a circle or sphere at the current radius, leaving out the cells of radius
    /// `inside` when it is already drawn ([`ball_cells`]).
    fn draw_round(&self, scene: &mut Scene, inside: Option<i32>) {
        let Some((lo, hi)) = self.drawn_extent(scene) else { return };
        let flat = (self.brush.shape == Shape::Circle).then_some(self.axis);
        for p in ball_cells(self.start, self.radius(), flat, inside) {
            if (0..3).all(|a| (lo[a]..=hi[a]).contains(&p[a])) {
                scene.apply_voxel(&self.brush, p);
            }
        }
    }

    /// End the drag and close the undo step. Returns how many voxels the whole gesture changed.
    pub fn release(self, scene: &mut Scene) -> usize {
        scene.commit()
    }
}

/// The face at voxel `p` along unit normal `n`, with `get` reading a cell's slot: the
/// connected voxels of `p`'s colour in its layer whose face along `n` is open, up to
/// [`MAX_FACE`] cells ([`Scene::face_region`]). Empty when `p` is empty or that face is covered.
pub fn face_cells(get: impl Fn([i32; 3]) -> u8, p: [i32; 3], n: [i32; 3]) -> Vec<[i32; 3]> {
    let value = get(p);
    if value == 0 {
        return Vec::new();
    }
    let a = axis_of(n);
    let (u, v) = ((a + 1) % 3, (a + 2) % 3);
    let open = |p: [i32; 3]| get(p) == value && get(add(p, n)) == 0;
    let mut seen = std::collections::HashSet::new();
    let (mut out, mut stack) = (Vec::new(), vec![p]);
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

/// Equal steps from `a` to `b` along the longest axis: the number of cells after the first.
pub fn line_steps(a: [i32; 3], b: [i32; 3]) -> i64 {
    (0..3).map(|k| (b[k] as i64 - a[k] as i64).abs()).max().unwrap_or(0)
}

/// The cells of the line brush from `a` to `b`, both ends included: [`line_steps`] equal
/// steps, rounded half up. The brush and the agents' `line` tool both draw these.
pub fn line_cells(a: [i32; 3], b: [i32; 3]) -> impl Iterator<Item = [i32; 3]> {
    let steps = line_steps(a, b);
    (0..=steps).map(move |s| line_cell(a, b, steps, s))
}

/// Step `s` of the cells from `a` to `b` in `steps` equal steps, rounded.
fn line_cell(a: [i32; 3], b: [i32; 3], steps: i64, s: i64) -> [i32; 3] {
    if steps == 0 {
        return a;
    }
    // Half up, not `round`'s half away from zero: the same line in map cells (never
    // negative) and in world cells (which are) must come out the same.
    std::array::from_fn(|k| (a[k] as f64 + (b[k] as f64 - a[k] as f64) * s as f64 / steps as f64 + 0.5).floor() as i32)
}

/// The steps of [`line_cell`] from `a` to `b` whose cells can fall between `lo` and `hi`
/// (inclusive; a step or so more on each end), so a far line visits only those.
fn steps_near(a: [i32; 3], b: [i32; 3], steps: i64, lo: [i32; 3], hi: [i32; 3]) -> std::ops::RangeInclusive<i64> {
    let steps = steps.max(0);
    let (mut from, mut to) = (0, steps);
    for k in 0..3 {
        let d = b[k] as f64 - a[k] as f64;
        let (l, h) = (lo[k] as f64 - 1.0 - a[k] as f64, hi[k] as f64 + 1.0 - a[k] as f64);
        if d == 0.0 || steps == 0 {
            if l > 0.0 || h < 0.0 {
                from = steps + 1;
            }
            continue;
        }
        let (s0, s1) = (l * steps as f64 / d, h * steps as f64 / d);
        from = from.max(s0.min(s1).floor() as i64);
        to = to.min(s0.max(s1).ceil() as i64);
    }
    from..=to
}

/// The cells of the box brush between corners `lo` and `hi` (inclusive, `lo <= hi` on every
/// axis): all of them, or with `hollow` only the ones on its surface.
pub fn box_cells(lo: [i32; 3], hi: [i32; 3], hollow: bool) -> impl Iterator<Item = [i32; 3]> {
    (lo[2]..=hi[2]).flat_map(move |z| {
        (lo[1]..=hi[1]).flat_map(move |y| {
            (lo[0]..=hi[0])
                .map(move |x| [x, y, z])
                .filter(move |p| !hollow || (0..3).any(|k| p[k] == lo[k] || p[k] == hi[k]))
        })
    })
}

/// The cells of the center brush around `c`: the ball of radius `r`, or with `flat` the disc
/// across that axis. A cell is in when its squared distance from `c` is within r^2 + r/2
/// (radius 1 is a plus, radius 2 a round disc); with `inside` the cells of that smaller
/// radius, already drawn, are left out.
pub fn ball_cells(c: [i32; 3], r: i32, flat: Option<usize>, inside: Option<i32>) -> impl Iterator<Item = [i32; 3]> {
    let reach = |r: i32| (r * r) as f32 + r as f32 / 2.0;
    let (r2, skip) = (reach(r), inside.map_or(-1.0, reach));
    let span = move |k: usize| if flat == Some(k) { 0 } else { r };
    (-span(2)..=span(2)).flat_map(move |z| {
        (-span(1)..=span(1)).flat_map(move |y| {
            (-span(0)..=span(0)).filter_map(move |x| {
                let d2 = (x * x + y * y + z * z) as f32;
                (d2 <= r2 && d2 > skip).then_some([c[0] + x, c[1] + y, c[2] + z])
            })
        })
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
