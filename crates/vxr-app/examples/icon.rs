//! The app icon: a 2x2x2 voxel cube in the editor's indigo on a dark rounded square, drawn
//! here so no image is kept in git. `cargo run --example icon -p vxr-app -- <dir.iconset>`
//! writes the ten PNGs `iconutil -c icns` turns into `vxr.icns` (`crates/vxr-app/bundle`).

use glam::{Vec2, Vec3};

/// A quad on the 1024 grid and its colour.
type Quad = ([Vec2; 4], [u8; 3]);
/// A face of the cube: two coordinates in [-0.5, 0.5] to a point on it.
type Face = fn(f32, f32) -> Vec3;

/// Each visible cell of the cube's faces, on the 1024 grid.
fn shapes() -> Vec<Quad> {
    let c = Vec2::new(512.0, 522.0);
    let s = 330.0;
    let (cos, sin) = (30f32.to_radians().cos(), 30f32.to_radians().sin());
    let p = |v: Vec3| c + v.x * Vec2::new(cos, sin) * s + v.y * Vec2::new(-cos, sin) * s + v.z * Vec2::new(0.0, -s);
    let mut quads = Vec::new();
    // Top (z = 0.5), front left (y = 0.5) and front right (x = 0.5), lightest to darkest.
    let faces: [(Face, [u8; 3]); 3] = [
        (|a, b| Vec3::new(a, b, 0.5), [0xc7, 0xd2, 0xfe]),
        (|a, b| Vec3::new(a, 0.5, b), [0x81, 0x8c, 0xf8]),
        (|a, b| Vec3::new(0.5, a, b), [0x5b, 0x5f, 0xe0]),
    ];
    let gap = 0.035;
    for (face, color) in faces {
        for i in 0..2 {
            for j in 0..2 {
                let (a0, b0) = (-0.5 + 0.5 * i as f32 + gap, -0.5 + 0.5 * j as f32 + gap);
                let (a1, b1) = (a0 + 0.5 - 2.0 * gap, b0 + 0.5 - 2.0 * gap);
                quads.push(([p(face(a0, b0)), p(face(a1, b0)), p(face(a1, b1)), p(face(a0, b1))], color));
            }
        }
    }
    quads
}

fn inside(q: &[Vec2; 4], pt: Vec2) -> bool {
    let sides = (0..4).map(|i| (q[(i + 1) % 4] - q[i]).perp_dot(pt - q[i]));
    let (mut pos, mut neg) = (false, false);
    for d in sides {
        pos |= d > 0.0;
        neg |= d < 0.0;
    }
    !(pos && neg)
}

/// Inside the rounded square of macOS's icon grid: 824 wide, corner radius 185.
fn in_background(pt: Vec2) -> bool {
    let q = (pt - Vec2::splat(512.0)).abs() - Vec2::splat(412.0 - 185.0);
    q.max(Vec2::ZERO).length() + q.x.max(q.y).min(0.0) <= 185.0
}

fn render(size: u32, quads: &[Quad]) -> Vec<u8> {
    const N: u32 = 4;
    let mut rgba = Vec::with_capacity((size * size * 4) as usize);
    for y in 0..size {
        for x in 0..size {
            let (mut sum, mut cover) = (Vec3::ZERO, 0.0);
            for sy in 0..N {
                for sx in 0..N {
                    let pt =
                        Vec2::new(x as f32 + (sx as f32 + 0.5) / N as f32, y as f32 + (sy as f32 + 0.5) / N as f32)
                            * (1024.0 / size as f32);
                    if !in_background(pt) {
                        continue;
                    }
                    // A dark vertical gradient behind the cube.
                    let t = (pt.y - 100.0) / 824.0;
                    let mut color = Vec3::new(30.0, 31.0, 46.0).lerp(Vec3::new(17.0, 18.0, 26.0), t);
                    if let Some((_, c)) = quads.iter().find(|(q, _)| inside(q, pt)) {
                        color = Vec3::new(c[0] as f32, c[1] as f32, c[2] as f32);
                    }
                    sum += color;
                    cover += 1.0;
                }
            }
            let alpha = cover / (N * N) as f32;
            let color = if cover > 0.0 { sum / cover } else { Vec3::ZERO };
            rgba.extend([color.x, color.y, color.z].map(|v| v.round() as u8));
            rgba.push((alpha * 255.0).round() as u8);
        }
    }
    rgba
}

fn main() {
    let dir = std::path::PathBuf::from(std::env::args().nth(1).expect("usage: icon <dir.iconset>"));
    std::fs::create_dir_all(&dir).expect("make the iconset folder");
    let quads = shapes();
    for base in [16, 32, 128, 256, 512] {
        for scale in [1, 2] {
            let size = base * scale;
            let name =
                if scale == 1 { format!("icon_{base}x{base}.png") } else { format!("icon_{base}x{base}@2x.png") };
            let file = std::fs::File::create(dir.join(name)).expect("write the icon");
            let mut png = png::Encoder::new(std::io::BufWriter::new(file), size, size);
            png.set_color(png::ColorType::Rgba);
            png.set_depth(png::BitDepth::Eight);
            png.write_header().and_then(|mut w| w.write_image_data(&render(size, &quads))).expect("write the icon");
        }
    }
}
