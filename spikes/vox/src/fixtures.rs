//! Fixtures for the MagicaVoxel check (`voxtool fixtures <dir>`). Each one targets a
//! corner of the format and says what MagicaVoxel should show.

use std::collections::BTreeMap;

use crate::build::{Builder, SceneNode, frame};
use crate::bytes::Dict;
use crate::default_palette::DEFAULT_PALETTE;
use crate::scene::{Affine, Rule, Scene, rotation_from_byte};
use crate::{Document, RawFile};

pub struct Fixture {
    pub name: &'static str,
    pub expect: String,
    pub builder: Builder,
}

// Palette indices used by the fixtures.
pub const WHITE: u8 = 1;
pub const RED: u8 = 2;
pub const GREEN: u8 = 3;
pub const BLUE: u8 = 4;
pub const YELLOW: u8 = 5;
pub const MAGENTA: u8 = 6;
pub const CYAN: u8 = 7;
pub const GREY: u8 = 8;
pub const ORANGE: u8 = 9;
pub const BROWN: u8 = 10;

fn palette() -> Vec<[u8; 4]> {
    let mut p = DEFAULT_PALETTE.to_vec();
    let set = [
        (WHITE, [255, 255, 255, 255]),
        (RED, [230, 40, 40, 255]),
        (GREEN, [40, 200, 60, 255]),
        (BLUE, [40, 80, 230, 255]),
        (YELLOW, [250, 220, 40, 255]),
        (MAGENTA, [220, 50, 220, 255]),
        (CYAN, [40, 220, 220, 255]),
        (GREY, [128, 128, 128, 255]),
        (ORANGE, [250, 140, 30, 255]),
        (BROWN, [120, 80, 40, 255]),
    ];
    for (i, c) in set {
        p[i as usize - 1] = c;
    }
    p
}

fn builder(version: i32) -> Builder {
    let mut b = Builder::new(version);
    b.palette = Some(palette());
    b.layers = (0..8).map(|i| Dict::default().with("_name", format!("{i}"))).collect();
    b
}

/// An axis marker filling a box of `size`: white voxel at the model's (0,0,0) corner,
/// red bar along +x, green along +y, blue along +z, yellow at the far corner.
fn axes(size: [i32; 3]) -> Vec<[u8; 4]> {
    let mut v = vec![[0, 0, 0, WHITE]];
    for x in 1..size[0] {
        v.push([x as u8, 0, 0, RED]);
    }
    for y in 1..size[1] {
        v.push([0, y as u8, 0, GREEN]);
    }
    for z in 1..size[2] {
        v.push([0, 0, z as u8, BLUE]);
    }
    let far = [size[0] - 1, size[1] - 1, size[2] - 1];
    if far != [0, 0, 0] {
        v.push([far[0] as u8, far[1] as u8, far[2] as u8, YELLOW]);
    }
    v
}

fn all_rotations() -> Vec<(u8, [[i32; 3]; 3])> {
    (0u8..128).filter_map(|b| rotation_from_byte(b).map(|r| (b, r))).collect()
}

/// Where model 0's white corner and red bar end up under both composition rules.
fn placement_note(b: &Builder) -> String {
    let doc = Document { raw: b.build() };
    let s = doc.scene().expect("fixture parses");
    let describe = |rule: Rule| -> String {
        let (inst, _) = s.instances(rule);
        inst.iter()
            .map(|i| {
                let m = &s.models[i.model];
                let corner = Scene::world_cell(&i.world, m.size, [0, 0, 0]);
                let red = [i.world.r[0][0], i.world.r[1][0], i.world.r[2][0]];
                format!("corner {corner:?}, red bar along {}", dir(red))
            })
            .collect::<Vec<_>>()
            .join("; ")
    };
    let a = describe(Rule::Compose);
    let d = describe(Rule::DotVoxExample);
    if a == d {
        format!("Expected: {a}.")
    } else {
        format!(
            "If MagicaVoxel composes transforms down the tree (ogt_vox rule): {a}. If it follows dot_vox's rule instead: {d}."
        )
    }
}

fn dir(v: [i32; 3]) -> &'static str {
    match v {
        [1, 0, 0] => "+X",
        [-1, 0, 0] => "-X",
        [0, 1, 0] => "+Y",
        [0, -1, 0] => "-Y",
        [0, 0, 1] => "+Z (up)",
        [0, 0, -1] => "-Z (down)",
        _ => "?",
    }
}

fn rot_z90() -> [[i32; 3]; 3] {
    [[0, -1, 0], [1, 0, 0], [0, 0, 1]]
}

fn rot_x90() -> [[i32; 3]; 3] {
    [[1, 0, 0], [0, 0, -1], [0, 1, 0]]
}

fn rot_y90() -> [[i32; 3]; 3] {
    [[0, 0, 1], [0, 1, 0], [-1, 0, 0]]
}

fn edges(size: i32) -> Vec<[u8; 4]> {
    let n = size - 1;
    let mut v = BTreeMap::new();
    for i in 0..size {
        for (a, b) in [(0, 0), (0, n), (n, 0), (n, n)] {
            v.insert([i, a, b], WHITE);
            v.insert([a, i, b], WHITE);
            v.insert([a, b, i], WHITE);
        }
    }
    v.insert([0, 0, 0], RED);
    v.insert([n, n, n], YELLOW);
    v.into_iter().map(|(p, c)| [p[0] as u8, p[1] as u8, p[2] as u8, c]).collect()
}

pub fn all() -> Vec<Fixture> {
    let mut out = Vec::new();
    let mut add = |name: &'static str, expect: String, builder: Builder| out.push(Fixture { name, expect, builder });

    // 01: the smallest possible scene.
    let mut b = builder(200);
    let m = b.model([1, 1, 1], vec![[0, 0, 0, RED]]);
    b.root.push(SceneNode::shape(m, None, [0, 0, 0]));
    add(
        "01-single-voxel",
        "One red voxel. Its world cell is (0, 0, 0): pivot of a 1x1x1 model is (0, 0, 0).".into(),
        b,
    );

    // 02-04: pivot placement for odd, even and mixed sizes.
    for (name, size) in
        [("02-axes-odd-5x5x5", [5, 5, 5]), ("03-axes-even-4x4x4", [4, 4, 4]), ("04-axes-mixed-3x4x5", [3, 4, 5])]
    {
        let mut b = builder(200);
        let m = b.model(size, axes(size));
        b.root.push(SceneNode::shape(m, None, [0, 0, 0]));
        let note = placement_note(&b);
        add(
            name,
            format!(
                "Axis marker {}x{}x{}: white corner, red bar +X, green +Y, blue up, yellow far corner. Pivot is floor(size/2). {note}",
                size[0], size[1], size[2]
            ),
            b,
        );
    }

    // 05/06: all 24 rotations and all 24 mirrors of the mixed-size marker.
    for (name, mirror) in [("05-rotations-24", false), ("06-mirrors-24", true)] {
        let mut b = builder(200);
        let size = [3, 4, 5];
        let m = b.model(size, axes(size));
        let rots: Vec<_> =
            all_rotations().into_iter().filter(|(_, r)| (Affine { r: *r, t: [0; 3] }.det() < 0) == mirror).collect();
        for (k, (byte, r)) in rots.iter().enumerate() {
            let t = [(k as i32 % 6) * 12, (k as i32 / 6) * 12, 0];
            b.root.push(SceneNode::shape(m, Some(*r), t).named(&format!("r{byte}")));
        }
        let hand = if mirror { "left-handed (mirror images)" } else { "right-handed" };
        add(
            name,
            format!(
                "24 copies of the 3x4x5 marker on a 6 x 4 grid, 12 apart, each named after its _r byte. All 24 differ, and red, green, blue always form a {hand} set."
            ),
            b,
        );
    }

    // 07: the composition question, as simply as possible.
    let mut b = builder(200);
    let size = [3, 4, 5];
    let m = b.model(size, axes(size));
    b.root.push(
        SceneNode::group(Some(rot_z90()), [0, 0, 0], vec![SceneNode::shape(m, None, [20, 0, 0])]).named("group z90"),
    );
    let note = placement_note(&b);
    add(
        "07-nested-rotation",
        format!("A group turned 90 degrees about Z holds the marker, moved +20 on X inside the group. {note}"),
        b,
    );

    // 08: three levels with different axes and translations.
    let mut b = builder(200);
    let m = b.model(size, axes(size));
    let inner = SceneNode::shape(m, Some(rot_y90()), [6, 0, 0]);
    let mid = SceneNode::group(Some(rot_x90()), [0, 10, 0], vec![inner]);
    b.root.push(SceneNode::group(Some(rot_z90()), [0, 0, 8], vec![mid]).named("outer"));
    let note = placement_note(&b);
    add(
        "08-nested-three-levels",
        format!("Three nested transforms: outer Z90 + (0,0,8), middle X90 + (0,10,0), shape Y90 + (6,0,0). {note}"),
        b,
    );

    // 09/10: the largest model sizes.
    for (name, n) in [("09-size-255", 255), ("10-size-256", 256)] {
        let mut b = builder(200);
        let m = b.model([n, n, n], edges(n));
        b.root.push(SceneNode::shape(m, None, [0, 0, n / 2]));
        add(
            name,
            format!(
                "Wireframe cube, {n} voxels on each edge, red corner at the bottom (-{0}, -{0}, 0), yellow at the top. Some readers fail at 256 but not 255.",
                n / 2
            ),
            b,
        );
    }

    // 11: a scene bigger than one model, split into 256^3 blocks.
    let mut b = builder(200);
    let mut w = BTreeMap::new();
    for x in 0..520 {
        for y in 0..300 {
            let h = (8.0 + 6.0 * ((x as f64) / 37.0).sin() + 5.0 * ((y as f64) / 23.0).cos()) as i32;
            w.insert([x - 260, y - 150, h], GREEN);
            w.insert([x - 260, y - 150, h - 1], BROWN);
        }
    }
    // A thin tower that crosses z = 256, so the split happens on every axis.
    for z in 0..300 {
        for (dx, dy) in [(0, 0), (1, 0), (0, 1), (1, 1)] {
            w.insert([dx, dy, z], if z % 50 == 49 { RED } else { WHITE });
        }
    }
    b.add_world(&w);
    let n = b.models.len();
    add(
        "11-over-256-split",
        format!(
            "A 520 x 300 rolling terrain (green top, brown under it) and a 300-tall white tower with red bands every 50, split into {n} models of at most 256^3. No seams, steps or gaps where the models meet; the tower is straight."
        ),
        b,
    );

    // 12: layers with names, colours and one hidden.
    let mut b = builder(200);
    b.layers = vec![
        Dict::default().with("_name", "ground"),
        Dict::default().with("_name", "props").with("_color", "255 0 0"),
        Dict::default().with("_name", "secret").with("_hidden", "1"),
        Dict::default().with("_name", "blue layer").with("_color", "0 0 255"),
    ];
    for (l, c) in [(0, GREY), (1, RED), (2, MAGENTA), (3, BLUE)] {
        let m = b.model([4, 4, 4], (0..64).map(|i| [(i % 4) as u8, ((i / 4) % 4) as u8, (i / 16) as u8, c]).collect());
        b.root.push(SceneNode::shape(m, None, [l * 8, 0, 2]).on_layer(l));
    }
    add("12-layers", "Four layers named ground, props (red tag), secret (hidden), blue layer (blue tag). Grey, red and blue cubes show; the magenta cube is on the hidden layer.".into(), b);

    // 13: a hidden node.
    let mut b = builder(200);
    let m = b.model([3, 3, 3], (0..27).map(|i| [(i % 3) as u8, ((i / 3) % 3) as u8, (i / 9) as u8, CYAN]).collect());
    b.root.push(SceneNode::shape(m, None, [0, 0, 1]).named("shown"));
    b.root.push(SceneNode::shape(m, None, [6, 0, 1]).named("hidden one").hidden());
    add("13-hidden-node", "Two cyan cubes share one model; only \"shown\" is visible, \"hidden one\" (at x = 6) is hidden in the outliner.".into(), b);

    // 14: every material type.
    let mut b = builder(200);
    let mats: [(u8, Dict); 6] = [
        (
            GREY,
            Dict::default()
                .with("_type", "_diffuse")
                .with("_weight", "1")
                .with("_rough", "0.1")
                .with("_spec", "0.5")
                .with("_ior", "0.3"),
        ),
        (
            YELLOW,
            Dict::default()
                .with("_type", "_metal")
                .with("_weight", "1")
                .with("_rough", "0.2")
                .with("_spec", "1")
                .with("_ior", "0.3")
                .with("_metal", "1"),
        ),
        (
            CYAN,
            Dict::default()
                .with("_type", "_glass")
                .with("_weight", "1")
                .with("_rough", "0")
                .with("_ior", "0.5")
                .with("_trans", "0.9")
                .with("_alpha", "0.9"),
        ),
        (
            ORANGE,
            Dict::default()
                .with("_type", "_emit")
                .with("_weight", "1")
                .with("_emit", "1")
                .with("_flux", "2")
                .with("_ldr", "0"),
        ),
        (
            MAGENTA,
            Dict::default()
                .with("_type", "_blend")
                .with("_weight", "1")
                .with("_rough", "0.3")
                .with("_metal", "0.5")
                .with("_trans", "0.3"),
        ),
        (
            WHITE,
            Dict::default()
                .with("_type", "_media")
                .with("_weight", "1")
                .with("_d", "0.05")
                .with("_media_type", "_cloud"),
        ),
    ];
    for i in 1..=255i32 {
        let d = mats
            .iter()
            .find(|(c, _)| *c as i32 == i)
            .map(|(_, d)| d.clone())
            .unwrap_or_else(|| Dict::default().with("_type", "_diffuse").with("_weight", "1"));
        b.materials.push((i, d));
    }
    for (k, (c, _)) in mats.iter().enumerate() {
        let m = b.model([4, 4, 4], (0..64).map(|i| [(i % 4) as u8, ((i / 4) % 4) as u8, (i / 16) as u8, *c]).collect());
        b.root.push(SceneNode::shape(m, None, [k as i32 * 7, 0, 2]));
    }
    add("14-materials", "Six cubes, left to right: grey diffuse, yellow metal, cyan glass, orange emissive, magenta blend, white cloud. Check each in the material panel and in the render tab.".into(), b);

    // 15: a camera.
    let mut b = builder(200);
    let size = [3, 4, 5];
    let m = b.model(size, axes(size));
    b.root.push(SceneNode::shape(m, None, [0, 0, 0]));
    b.cameras.push((
        0,
        Dict::default()
            .with("_mode", "pers")
            .with("_focus", "0 0 2")
            .with("_angle", "30 45 0")
            .with("_radius", "40")
            .with("_frustum", "0.4")
            .with("_fov", "45"),
    ));
    add(
        "15-camera",
        "Camera 1 is perspective, looking at (0, 0, 2) from 30 degrees up and 45 around, radius 40, field of view 45."
            .into(),
        b,
    );

    // 16: palette display order (IMAP) and row names (NOTE).
    let mut b = builder(200);
    let m = b.model([10, 1, 1], (0..10).map(|i| [i as u8, 0, 0, i as u8 + 1]).collect());
    b.root.push(SceneNode::shape(m, None, [0, 0, 0]));
    let mut imap: Vec<u8> = (0..=255u8).collect();
    imap.reverse();
    b.imap = Some(imap);
    b.note = Some((0..32).map(|i| format!("row {i}").into_bytes()).collect());
    add("16-palette-imap-note", "A row of 10 voxels: white, red, green, blue, yellow, magenta, cyan, grey, orange, brown. The palette panel shows colours in reversed order (IMAP) with rows named \"row 0\" to \"row 31\"; voxel colours do not change.".into(), b);

    // 17: animation keyframes, shape frames and META.
    let mut b = builder(200);
    b.meta = Some(Dict::default().with("_anim_range", "0 20"));
    let m0 = b.model([3, 3, 3], (0..27).map(|i| [(i % 3) as u8, ((i / 3) % 3) as u8, (i / 9) as u8, RED]).collect());
    let m1 = b.model([3, 3, 3], (0..27).map(|i| [(i % 3) as u8, ((i / 3) % 3) as u8, (i / 9) as u8, BLUE]).collect());
    let frames = vec![
        frame(None, [0, 0, 1]).with("_f", "0"),
        frame(None, [10, 0, 1]).with("_f", "10"),
        frame(Some(rot_z90()), [10, 10, 1]).with("_f", "20"),
    ];
    b.root.push(SceneNode::Shape {
        attrs: Dict::default().with("_name", "mover").with("_loop", "1"),
        layer: 0,
        frames,
        models: vec![(m0, Dict::default().with("_f", "0")), (m1, Dict::default().with("_f", "10"))],
    });
    add("17-animation", "Animation range 0 to 20, looping. A cube slides from (0,0) to (10,0) by frame 10, where it turns from red to blue, then to (10,10) by frame 20 while turning 90 degrees.".into(), b);

    // 18/19: overlapping instances; which one wins depends on the version.
    for (name, v, winner) in
        [("18-overlap-v200", 200, "red (the first instance)"), ("19-overlap-v150", 150, "blue (the second instance)")]
    {
        let mut b = builder(v);
        let a =
            b.model([4, 4, 4], (0..64).map(|i| [(i % 4) as u8, ((i / 4) % 4) as u8, (i / 16) as u8, RED]).collect());
        let c =
            b.model([4, 4, 4], (0..64).map(|i| [(i % 4) as u8, ((i / 4) % 4) as u8, (i / 16) as u8, BLUE]).collect());
        b.root.push(SceneNode::shape(a, None, [0, 0, 2]).named("first red"));
        b.root.push(SceneNode::shape(c, None, [2, 0, 2]).named("second blue"));
        add(
            name,
            format!(
                "Version {v}. Two 4x4x4 cubes overlap by half. Where they overlap, {winner} should win (visible, and kept if you merge them)."
            ),
            b,
        );
    }

    // 20: far positions, to find MagicaVoxel's world clamp.
    let mut b = builder(200);
    let size = [3, 4, 5];
    let m = b.model(size, axes(size));
    for x in [-1300, -1000, 0, 999, 1200] {
        b.root.push(SceneNode::shape(m, None, [x, 0, 0]).named(&format!("x {x}")));
    }
    add("20-far-positions", "Five markers along X at -1300, -1000, 0, 999 and 1200 (named after their x). Records whether MagicaVoxel clamps positions beyond about +-1000.".into(), b);

    // 21: base transform keys from 0.99.7.2 next to keyframes.
    let mut b = builder(200);
    let m = b.model(size, axes(size));
    b.root.push(SceneNode::Shape {
        attrs: Dict::default().with("_name", "base").with("_frm_t", "30 0 0"),
        layer: 0,
        frames: vec![frame(None, [0, 0, 0]).with("_f", "0"), frame(None, [0, 20, 0]).with("_f", "10")],
        models: vec![(m, Dict::default())],
    });
    add("21-frm-base-transform", "One marker whose node has a base translation _frm_t = (30, 0, 0) and keyframes at (0,0,0) on frame 0 and (0,20,0) on frame 10. ogt_vox (and we) place it at x = 30 when not animating; records what MagicaVoxel does.".into(), b);

    out
}

/// For tests: every fixture parses and round-trips.
pub fn check_all() -> Result<(), String> {
    for f in all() {
        let bytes = f.builder.build().write();
        let doc = Document::read(&bytes).map_err(|e| format!("{}: {e}", f.name))?;
        if doc.write() != bytes || RawFile::parse(&bytes).is_err() {
            return Err(format!("{} does not round-trip", f.name));
        }
    }
    Ok(())
}
