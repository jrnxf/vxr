//! The editing tools, as in the prototype editor (`prototype/src/editor.html`): add, erase,
//! paint and pick, each with the voxel, box or face brush, and an optional mirror across
//! world x = 0. A gesture is a press, any number of moves and a release; the whole gesture
//! is one undo step. All positions are map cells.

use crate::Scene;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Tool {
    Add,
    Erase,
    Paint,
    Pick,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Shape {
    Voxel,
    Box,
    Face,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Brush {
    pub tool: Tool,
    pub shape: Shape,
    pub mirror: bool,
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

/// A drag in progress. Add and box drags stay on the plane of the face first pressed;
/// erase and paint voxel drags follow the surface under the pointer.
#[derive(Clone, Debug)]
pub struct Gesture {
    brush: Brush,
    /// Axis and layer of the drag plane.
    pub axis: usize,
    pub layer: i32,
    pub start: [i32; 3],
    pub end: [i32; 3],
    last: [i32; 3],
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

    /// Mirror of a map cell across world x = 0.
    pub fn mirror_x(&self, p: [i32; 3]) -> [i32; 3] {
        [-1 - p[0] - 2 * self.origin[0], p[1], p[2]]
    }

    fn put_brush(&mut self, brush: &Brush, p: [i32; 3], v: u8) {
        self.put(p, v);
        if brush.mirror {
            let m = self.mirror_x(p);
            if m != p {
                self.put(m, v);
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
        let axis = axis_of(t.normal);
        self.begin();
        if brush.shape == Shape::Voxel {
            self.apply_voxel(&brush, start);
        }
        Press::Drag(Gesture { brush, axis, layer: start[axis], start, end: start, last: start })
    }
}

impl Gesture {
    /// Whether moves should be picked on the drag plane ([`plane_cell`]) rather than on the
    /// surface ([`Scene::target`], taking its `voxel`).
    pub fn on_plane(&self) -> bool {
        self.brush.tool == Tool::Add || self.brush.shape == Shape::Box
    }

    pub fn brush(&self) -> Brush {
        self.brush
    }

    /// The pointer moved over cell `p`. Voxel strokes fill the gap from the last cell so fast
    /// moves stay continuous; box strokes only move their far corner.
    pub fn move_to(&mut self, scene: &mut Scene, p: [i32; 3]) {
        if p == self.last {
            return;
        }
        if self.brush.shape == Shape::Voxel {
            let l = self.last;
            let steps =
                if self.brush.tool == Tool::Add { (0..3).map(|a| (p[a] - l[a]).abs()).max().unwrap() } else { 1 };
            for s in 1..=steps {
                let q = std::array::from_fn(|a| {
                    (l[a] as f32 + (p[a] - l[a]) as f32 * s as f32 / steps as f32).round() as i32
                });
                scene.apply_voxel(&self.brush, q);
            }
        }
        self.end = p;
        self.last = p;
    }

    /// The box from the start to the far corner, inclusive.
    pub fn extent(&self) -> ([i32; 3], [i32; 3]) {
        (
            std::array::from_fn(|a| self.start[a].min(self.end[a])),
            std::array::from_fn(|a| self.start[a].max(self.end[a])),
        )
    }

    /// End the drag: fill the box if it is one, and close the undo step. Returns how many
    /// voxels the whole gesture changed.
    pub fn release(self, scene: &mut Scene) -> usize {
        if self.brush.shape == Shape::Box {
            let (lo, hi) = self.extent();
            for z in lo[2]..=hi[2] {
                for y in lo[1]..=hi[1] {
                    for x in lo[0]..=hi[0] {
                        scene.apply_voxel(&self.brush, [x, y, z]);
                    }
                }
            }
        }
        scene.commit()
    }
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
