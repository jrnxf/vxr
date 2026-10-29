//! The app icon: the vxr logo, a 2x2x2 voxel cube with its top front voxel out, in neutral
//! greys on a dark rounded square (`assets/logo/vxr-app-icon.svg` is the same drawing), drawn
//! here so no image is kept in git. `cargo run --example icon -p vxr-app -- <dir.iconset>`
//! writes the ten PNGs `iconutil -c icns` turns into `vxr.icns` (`crates/vxr-app/bundle`).

use glam::{Vec2, Vec3};

/// A quad on the 1024 grid and its colour.
type Quad = ([Vec2; 4], [u8; 3]);

/// The cube's visible faces on a unit-high box 0.866 wide, (tone, corners): tone 0 is the top,
/// 1 the left and 2 the right side, as in the top bar's logo (`ui.rs`).
const FACES: [(usize, [[f32; 2]; 4]); 9] = [
    (0, [[0.433, 0.0], [0.866, 0.25], [0.6495, 0.375], [0.2165, 0.125]]),
    (0, [[0.2165, 0.125], [0.433, 0.25], [0.2165, 0.375], [0.0, 0.25]]),
    (1, [[0.0, 0.25], [0.2165, 0.375], [0.2165, 0.875], [0.0, 0.75]]),
    (2, [[0.866, 0.25], [0.866, 0.75], [0.6495, 0.875], [0.6495, 0.375]]),
    (1, [[0.433, 0.25], [0.6495, 0.375], [0.6495, 0.625], [0.433, 0.5]]),
    (2, [[0.433, 0.25], [0.433, 0.5], [0.2165, 0.625], [0.2165, 0.375]]),
    (0, [[0.433, 0.5], [0.6495, 0.625], [0.433, 0.75], [0.2165, 0.625]]),
    (1, [[0.2165, 0.625], [0.433, 0.75], [0.433, 1.0], [0.2165, 0.875]]),
    (2, [[0.6495, 0.625], [0.6495, 0.875], [0.433, 1.0], [0.433, 0.75]]),
];
/// shadcn Neutral's greys: the top, left and right faces, and the square behind them.
const TONES: [[u8; 3]; 3] = [[0xfa, 0xfa, 0xfa], [0xa1, 0xa1, 0xa1], [0x52, 0x52, 0x52]];
const BACKGROUND: [u8; 3] = [0x17, 0x17, 0x17];

/// The cube's faces on the 1024 grid: 486 high, centred a little above the middle.
fn shapes() -> Vec<Quad> {
    let h = 486.0;
    let origin = Vec2::new(512.0 - 0.433 * h, 506.0 - 0.5 * h);
    FACES.iter().map(|(tone, q)| (q.map(|[x, y]| origin + Vec2::new(x, y) * h), TONES[*tone])).collect()
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

/// The signed distance to the rounded square of macOS's icon grid: 824 wide, corner radius 185.
fn background_distance(pt: Vec2) -> f32 {
    let q = (pt - Vec2::splat(512.0)).abs() - Vec2::splat(412.0 - 185.0);
    q.max(Vec2::ZERO).length() + q.x.max(q.y).min(0.0) - 185.0
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
                    let d = background_distance(pt);
                    if d > 0.0 {
                        continue;
                    }
                    let mut color = Vec3::from(BACKGROUND.map(f32::from));
                    // A white 10% rim, as on the dark theme's panels.
                    if d > -2.0 {
                        color = color.lerp(Vec3::splat(255.0), 0.1);
                    }
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
