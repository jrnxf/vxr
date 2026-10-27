//! `render_view`: the scene from one side through the editor's ray marcher (`vxr_render`), with
//! the ground grid and bounds frame on, and world coordinates written along three bounds edges
//! in a 3x5 pixel font so the model can read positions off the picture.

use glam::{Mat4, Vec3, Vec4};
use vxr_core::Scene;
use vxr_render::{Gpu, GpuMap, Guides, OrbitCamera, Renderer, View, brickmap_layout};

pub const SIZE: (u32, u32) = (1000, 625);

pub struct Viewer {
    gpu: Gpu,
    layout: wgpu::BindGroupLayout,
    renderer: Renderer,
}

impl Viewer {
    pub fn new() -> Result<Viewer, String> {
        let instance = wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle());
        let gpu = pollster::block_on(Gpu::new(instance, None))?;
        let layout = brickmap_layout(&gpu.device);
        let renderer = Renderer::new(&gpu.device, &layout, wgpu::TextureFormat::Rgba8Unorm, SIZE.0, SIZE.1, false);
        Ok(Viewer { gpu, layout, renderer })
    }

    /// Render `scene` from `view`, framing `focus` (world cells) or the whole model. Returns the
    /// PNG and a line saying how the picture's axes run.
    pub fn render(
        &mut self,
        scene: &Scene,
        view: &str,
        focus: Option<([i32; 3], [i32; 3])>,
    ) -> Result<(Vec<u8>, String), String> {
        let (dev, queue) = (&self.gpu.device, &self.gpu.queue);
        let map = GpuMap::new(dev, queue, &self.layout, &scene.map, &scene.palette)?;
        let size = scene.map.size_voxels();
        self.renderer.guides =
            Guides { bounds: size, floor: scene.floor, grid: true, ground: true, frame: true, line: 1.0 };
        let v = |a: [i32; 3]| Vec3::new(a[0] as f32, a[1] as f32, a[2] as f32);
        let (lo, hi) = match focus {
            Some((a, b)) => {
                let (a, b) = (scene.to_map(a), scene.to_map(b));
                let lo: [i32; 3] = std::array::from_fn(|k| a[k].min(b[k]));
                let hi: [i32; 3] = std::array::from_fn(|k| a[k].max(b[k]) + 1);
                (v(lo), v(hi))
            }
            None => match scene.map.occupied_bounds() {
                Some((lo, hi)) => (v(lo.map(|x| x as i32)), v(hi.map(|x| x as i32))),
                None => {
                    let f = scene.floor.max(0);
                    (Vec3::new(0.0, f as f32, 0.0), Vec3::new(size[0] as f32, f as f32 + 1.0, size[2] as f32))
                }
            },
        };
        let aspect = SIZE.0 as f32 / SIZE.1 as f32;
        let mut cam = OrbitCamera::framing_aspect(lo, hi, aspect);
        let snap = match view {
            "persp" => None,
            "front" => Some(View::Front),
            "back" => Some(View::Back),
            "left" => Some(View::Left),
            "right" => Some(View::Right),
            "top" => Some(View::Top),
            "iso" => Some(View::Iso),
            v => return Err(format!("unknown view {v}")),
        };
        if let Some(s) = snap {
            cam.snap(s);
            cam.ortho = true;
            cam.fit(lo, hi, aspect, 0.85);
        } else {
            cam.fit(lo, hi, aspect, 0.85);
        }
        self.renderer.set_camera(queue, &cam, None);
        let mut enc = dev.create_command_encoder(&Default::default());
        self.renderer.march(&mut enc, &map, None);
        queue.submit([enc.finish()]);
        let mut px = pollster::block_on(vxr_render::gpu::read_texture(dev, queue, &self.renderer.color));
        for p in px.as_chunks_mut::<4>().0 {
            p[3] = 255;
        }
        let vp = cam.view_proj(aspect);
        label_edges(&mut px, scene, &cam, vp, size);
        let png = encode_png(&px, SIZE.0, SIZE.1)?;
        let (_, right, up) = cam.basis();
        let name = |d: Vec3| {
            let a = [d.x, d.y, d.z];
            let k = (0..3).max_by(|&i, &j| a[i].abs().total_cmp(&a[j].abs())).unwrap();
            format!("{}{}", if a[k] >= 0.0 { "+" } else { "-" }, ["x", "y", "z"][k])
        };
        let desc = format!(
            "{view} view{}: screen right is about {}, screen up about {}. Labels give world cell coordinates along the bounds edges; the ground grid has one line per cell.",
            if cam.ortho { " (orthographic)" } else { " (perspective)" },
            name(right),
            name(up)
        );
        Ok((png, desc))
    }

    /// A plain picture for judging: `scene` through `cam` at `dims`, with the ground and the
    /// bounds frame but no labels.
    pub fn picture(&mut self, scene: &Scene, cam: &OrbitCamera, dims: (u32, u32)) -> Result<Vec<u8>, String> {
        let (dev, queue) = (&self.gpu.device, &self.gpu.queue);
        let map = GpuMap::new(dev, queue, &self.layout, &scene.map, &scene.palette)?;
        self.renderer.resize(dev, dims.0, dims.1);
        self.renderer.guides = Guides {
            bounds: scene.map.size_voxels(),
            floor: scene.floor,
            grid: true,
            ground: true,
            frame: true,
            line: 1.0,
        };
        self.renderer.set_camera(queue, cam, None);
        let mut enc = dev.create_command_encoder(&Default::default());
        self.renderer.march(&mut enc, &map, None);
        queue.submit([enc.finish()]);
        let mut px = pollster::block_on(vxr_render::gpu::read_texture(dev, queue, &self.renderer.color));
        for p in px.as_chunks_mut::<4>().0 {
            p[3] = 255;
        }
        self.renderer.resize(dev, SIZE.0, SIZE.1);
        encode_png(&px, dims.0, dims.1)
    }
}

pub fn encode_png(rgba: &[u8], w: u32, h: u32) -> Result<Vec<u8>, String> {
    let mut out = Vec::new();
    let mut e = png::Encoder::new(&mut out, w, h);
    e.set_color(png::ColorType::Rgba);
    e.set_depth(png::BitDepth::Eight);
    e.write_header().and_then(|mut wr| wr.write_image_data(rgba)).map_err(|e| e.to_string())?;
    Ok(out)
}

fn project(vp: Mat4, p: Vec3) -> Option<(f32, f32)> {
    let c = vp * Vec4::new(p.x, p.y, p.z, 1.0);
    if c.w <= 1e-4 {
        return None;
    }
    let n = c / c.w;
    Some(((n.x + 1.0) * 0.5 * SIZE.0 as f32, (1.0 - n.y) * 0.5 * SIZE.1 as f32))
}

/// World coordinates along one bounds edge per axis: for x and z the floor edge nearest the
/// eye, for y the vertical edge nearest the eye. Edges seen end-on are skipped.
fn label_edges(px: &mut [u8], scene: &Scene, cam: &OrbitCamera, vp: Mat4, size: [u32; 3]) {
    let eye = cam.view_eye();
    let s = size.map(|v| v as f32);
    let center2 = project(vp, Vec3::new(s[0], s[1], s[2]) * 0.5).unwrap_or((SIZE.0 as f32 / 2.0, SIZE.1 as f32 / 2.0));
    for axis in 0..3 {
        // The two other axes take 0 or the size; y edges stand at floor corners.
        let (a, b) = ((axis + 1) % 3, (axis + 2) % 3);
        let mut best: Option<(f32, Vec3)> = None;
        for &ca in &[0.0, s[a]] {
            for &cb in &[0.0, s[b]] {
                let mut base = [0.0; 3];
                base[a] = ca;
                base[b] = cb;
                if axis != 1 && base[1] != 0.0 {
                    continue;
                }
                let mut mid = base;
                mid[axis] = s[axis] * 0.5;
                let d = (Vec3::from(mid) - eye).length();
                if best.is_none_or(|(bd, _)| d < bd - 1e-3) {
                    best = Some((d, Vec3::from(base)));
                }
            }
        }
        let Some((_, base)) = best else { continue };
        let mut end = base;
        end[axis] = s[axis];
        let (Some(p0), Some(p1)) = (project(vp, base), project(vp, end)) else { continue };
        let len = ((p1.0 - p0.0).powi(2) + (p1.1 - p0.1).powi(2)).sqrt();
        if len < 60.0 {
            continue;
        }
        let per_cell = len / s[axis];
        let step = [1, 2, 4, 8, 16, 32, 64, 128].into_iter().find(|&k| k as f32 * per_cell >= 44.0).unwrap_or(256);
        let lo = scene.to_world([0, 0, 0])[axis];
        let first = lo.div_euclid(step) * step;
        let mut w = first;
        while w < lo + size[axis] as i32 {
            if w >= lo {
                let mut p = base;
                p[axis] = (w - lo) as f32 + 0.5;
                if let Some((x, y)) = project(vp, p) {
                    // Push the label away from the bounds' middle on screen.
                    let (dx, dy) = (x - center2.0, y - center2.1);
                    let n = (dx * dx + dy * dy).sqrt().max(1.0);
                    let (lx, ly) = (x + dx / n * 14.0, y + dy / n * 14.0);
                    dot(px, x, y);
                    let t = format!("{}{}", ["x", "y", "z"][axis], w);
                    draw_text(px, &t, lx, ly);
                }
            }
            w += step;
        }
    }
}

const GLYPHS: &[(char, [u8; 5])] = &[
    ('0', [0b111, 0b101, 0b101, 0b101, 0b111]),
    ('1', [0b010, 0b110, 0b010, 0b010, 0b111]),
    ('2', [0b111, 0b001, 0b111, 0b100, 0b111]),
    ('3', [0b111, 0b001, 0b111, 0b001, 0b111]),
    ('4', [0b101, 0b101, 0b111, 0b001, 0b001]),
    ('5', [0b111, 0b100, 0b111, 0b001, 0b111]),
    ('6', [0b111, 0b100, 0b111, 0b101, 0b111]),
    ('7', [0b111, 0b001, 0b010, 0b010, 0b010]),
    ('8', [0b111, 0b101, 0b111, 0b101, 0b111]),
    ('9', [0b111, 0b101, 0b111, 0b001, 0b111]),
    ('-', [0b000, 0b000, 0b111, 0b000, 0b000]),
    ('x', [0b000, 0b101, 0b010, 0b101, 0b000]),
    ('y', [0b101, 0b101, 0b010, 0b010, 0b010]),
    ('z', [0b000, 0b111, 0b010, 0b100, 0b111]),
];

const SCALE: i32 = 2;

fn put(px: &mut [u8], x: i32, y: i32, c: [u8; 3]) {
    if x < 0 || y < 0 || x >= SIZE.0 as i32 || y >= SIZE.1 as i32 {
        return;
    }
    let i = (y as usize * SIZE.0 as usize + x as usize) * 4;
    px[i..i + 3].copy_from_slice(&c);
}

fn dot(px: &mut [u8], x: f32, y: f32) {
    for dy in -2i32..=2 {
        for dx in -2i32..=2 {
            let c = if dx.abs() == 2 || dy.abs() == 2 { [0, 0, 0] } else { [255, 220, 0] };
            put(px, x as i32 + dx, y as i32 + dy, c);
        }
    }
}

/// White text with a black outline, centred on (x, y).
fn draw_text(px: &mut [u8], t: &str, x: f32, y: f32) {
    let w = t.chars().count() as i32 * 4 * SCALE - SCALE;
    let (x0, y0) = (x as i32 - w / 2, y as i32 - 5 * SCALE / 2);
    for pass in 0..2 {
        for (i, ch) in t.chars().enumerate() {
            let Some((_, g)) = GLYPHS.iter().find(|(c, _)| *c == ch) else { continue };
            for (row, bits) in g.iter().enumerate() {
                for col in 0..3 {
                    if bits & (0b100 >> col) == 0 {
                        continue;
                    }
                    let (bx, by) = (x0 + (i as i32 * 4 + col) * SCALE, y0 + row as i32 * SCALE);
                    for dy in 0..SCALE {
                        for dx in 0..SCALE {
                            if pass == 0 {
                                for oy in -1..=1 {
                                    for ox in -1..=1 {
                                        put(px, bx + dx + ox, by + dy + oy, [0, 0, 0]);
                                    }
                                }
                            } else {
                                put(px, bx + dx, by + dy, [255, 255, 255]);
                            }
                        }
                    }
                }
            }
        }
    }
}
