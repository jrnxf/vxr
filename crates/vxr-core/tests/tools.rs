//! The editing tools: each brush on a small scene, mirror, and one undo step per gesture.

use vxr_core::Scene;
use vxr_core::tools::{Brush, Press, Shape, Target, Tool, plane_cell};

fn brush(tool: Tool, shape: Shape) -> Brush {
    Brush { tool, shape, mirror: [false; 3], lock: None, color: 7, size: 1 }
}

/// A 4x1x4 slab of colour 3 at y = 0, with a 1x1 bump of colour 5 on top at (0, 1, 0).
fn slab() -> Scene {
    let mut s = Scene::new([32, 32, 32]);
    let mut cells: Vec<_> = (0..4).flat_map(|x| (0..4).map(move |z| ([x, 0, z], 3))).collect();
    cells.push(([0, 1, 0], 5));
    s.edit(cells);
    s
}

fn top(voxel: [i32; 3], value: u8) -> Target {
    Target { voxel, normal: [0, 1, 0], value, ground: false }
}

fn drag(s: &mut Scene, b: Brush, t: Target, moves: &[[i32; 3]]) -> usize {
    let Press::Drag(mut g) = s.press(b, t) else { panic!("expected a drag") };
    for &p in moves {
        g.move_to(s, p);
    }
    g.release(s)
}

#[test]
fn target_hits_voxels_then_the_ground() {
    let s = slab();
    let t = s.target([2.5, 10.0, 2.5], [0.0, -1.0, 0.0]).unwrap();
    assert_eq!(t, top([2, 0, 2], 3));
    let g = s.target([20.5, 10.0, 20.5], [0.0, -1.0, 0.0]).unwrap();
    assert_eq!((g.voxel, g.front(), g.ground), ([20, -1, 20], [20, 0, 20], true));
    assert!(s.target([20.5, 10.0, 20.5], [0.0, 1.0, 0.0]).is_none(), "looking up misses");
}

#[test]
fn voxel_add_drag_fills_gaps_on_the_plane_as_one_step() {
    let mut s = slab();
    let before = s.map.voxels;
    let n = drag(&mut s, brush(Tool::Add, Shape::Voxel), top([1, 0, 1], 3), &[[1, 1, 3], [3, 1, 3]]);
    // (1,1,1) at press, then (1,1,2), (1,1,3), (2,1,3), (3,1,3).
    assert_eq!(n, 5);
    assert_eq!(s.map.get(1, 1, 2), 7);
    assert_eq!(s.map.voxels, before + 5);
    assert!(s.undo());
    assert_eq!(s.map.voxels, before, "the whole drag is one undo step");
}

#[test]
fn erase_and_paint_voxel_touch_only_filled_cells() {
    let mut s = slab();
    assert_eq!(drag(&mut s, brush(Tool::Paint, Shape::Voxel), top([2, 0, 2], 3), &[[2, 5, 2]]), 1);
    assert_eq!((s.map.get(2, 0, 2), s.map.get(2, 5, 2)), (7, 0));
    assert_eq!(drag(&mut s, brush(Tool::Erase, Shape::Voxel), top([3, 0, 3], 3), &[[3, 0, 2]]), 2);
    assert_eq!((s.map.get(3, 0, 3), s.map.get(3, 0, 2)), (0, 0));
}

#[test]
fn box_fills_the_rectangle_live() {
    let mut s = slab();
    let Press::Drag(mut g) = s.press(brush(Tool::Add, Shape::Box), top([0, 0, 1], 3)) else { panic!() };
    assert!(g.on_plane());
    assert_eq!((g.axis, g.layer), (1, 1));
    g.move_to(&mut s, [3, 1, 3]);
    assert_eq!(s.map.get(3, 1, 3), 7, "the box shows while dragging");
    g.move_to(&mut s, [1, 1, 2]);
    assert_eq!(s.map.get(3, 1, 3), 0, "shrinking the box takes the cells back");
    g.move_to(&mut s, [3, 1, 3]);
    assert_eq!(g.extent(), ([0, 1, 1], [3, 1, 3]));
    assert_eq!(g.release(&mut s), 12);
    assert_eq!(s.map.get(3, 1, 3), 7);
    // Box erase clears the filled cells of a 3D box.
    let n = drag(&mut s, brush(Tool::Erase, Shape::Box), top([0, 0, 0], 3), &[[3, 0, 3]]);
    assert_eq!(n, 16);
}

#[test]
fn face_brush_extrudes_peels_and_fills_the_connected_face() {
    let mut s = slab();
    // The slab's top face is the 15 cells of colour 3 with nothing above; the bump covers (0,0,0).
    assert_eq!(s.face_region(&top([2, 0, 2], 3)).len(), 15);
    let Press::Done(n) = s.press(brush(Tool::Add, Shape::Face), top([2, 0, 2], 3)) else { panic!() };
    assert_eq!(n, 15);
    assert_eq!((s.map.get(3, 1, 3), s.map.get(0, 2, 0)), (7, 0));
    let Press::Done(n) = s.press(brush(Tool::Paint, Shape::Face), top([0, 1, 0], 5)) else { panic!() };
    assert_eq!((n, s.map.get(0, 1, 0)), (1, 7), "paint fills only the bump, a different colour");
    let Press::Done(n) = s.press(brush(Tool::Erase, Shape::Face), top([2, 1, 2], 7)) else { panic!() };
    assert_eq!(n, 16, "peel takes the extruded layer and the repainted bump, both colour 7");
    assert!(s.undo() && s.undo() && s.undo());
    assert_eq!(s.map.voxels, 17);
}

#[test]
fn pick_and_the_ground() {
    let mut s = slab();
    assert!(matches!(s.press(brush(Tool::Pick, Shape::Box), top([0, 1, 0], 5)), Press::Picked(5)));
    let ground = s.target([20.5, 10.0, 20.5], [0.0, -1.0, 0.0]).unwrap();
    assert!(matches!(s.press(brush(Tool::Pick, Shape::Voxel), ground), Press::Nothing));
    assert!(matches!(s.press(brush(Tool::Erase, Shape::Voxel), ground), Press::Nothing));
    assert!(matches!(s.press(brush(Tool::Add, Shape::Face), ground), Press::Nothing));
    assert_eq!(drag(&mut s, brush(Tool::Add, Shape::Voxel), ground, &[]), 1);
    assert_eq!(s.map.get(20, 0, 20), 7);
}

#[test]
fn mirror_reflects_across_world_x_zero() {
    let mut s = Scene::new([32, 32, 32]);
    s.origin = [-16, 0, 0];
    let b = Brush { mirror: [true, false, false], ..brush(Tool::Add, Shape::Voxel) };
    // Map x 20 is world x 4, mirrored to world x -5, map x 11.
    assert_eq!(s.mirror_x([20, 0, 0]), [11, 0, 0]);
    assert_eq!(drag(&mut s, b, Target { voxel: [20, -1, 0], normal: [0, 1, 0], value: 0, ground: true }, &[]), 2);
    assert_eq!((s.map.get(20, 0, 0), s.map.get(11, 0, 0)), (7, 7));
    // The middle column (world x -1 and 0 mirror to each other; map 15 and 16).
    assert_eq!(s.mirror_x([16, 0, 0]), [15, 0, 0]);
}

#[test]
fn line_draws_from_the_press_to_the_pointer() {
    let mut s = slab();
    // From (0,1,1) to (3,1,3): four steps along x, rounded in z.
    let n = drag(&mut s, brush(Tool::Add, Shape::Line), top([0, 0, 1], 3), &[[2, 1, 2], [3, 1, 3]]);
    assert_eq!(n, 4);
    for p in [[0, 1, 1], [1, 1, 2], [2, 1, 2], [3, 1, 3]] {
        assert_eq!(s.map.get(p[0], p[1], p[2]), 7, "{p:?}");
    }
    assert_eq!(s.map.get(2, 1, 2), 7);
    assert!(s.undo());
    assert_eq!(s.map.voxels, 17, "one undo step, including the cells the line passed through first");
    // Painting a line touches only filled cells.
    assert_eq!(drag(&mut s, brush(Tool::Paint, Shape::Line), top([0, 0, 3], 3), &[[3, 0, 3]]), 4);
}

#[test]
fn circle_and_sphere_grow_from_the_centre() {
    let mut s = Scene::new([32, 32, 32]);
    let ground = Target { voxel: [16, -1, 16], normal: [0, 1, 0], value: 0, ground: true };
    // Radius 1 is a plus; radius 2 a 21-cell disc on the ground layer.
    assert_eq!(drag(&mut s, brush(Tool::Add, Shape::Circle), ground, &[[17, 0, 16]]), 5);
    let Press::Drag(mut g) = s.press(brush(Tool::Add, Shape::Circle), ground) else { panic!() };
    g.move_to(&mut s, [18, 0, 16]);
    assert_eq!((g.radius(), g.extent()), (2, ([14, 0, 14], [18, 0, 18])));
    assert_eq!(g.release(&mut s), 21 - 5, "cells of the first circle are already filled");
    assert_eq!(s.map.get(16, 1, 16), 0, "a circle stays on its layer");
    // A ball of radius 2: 57 cells.
    let mut s = Scene::new([32, 32, 32]);
    let n = drag(&mut s, brush(Tool::Add, Shape::Sphere), Target { voxel: [16, 9, 16], ..ground }, &[[16, 10, 18]]);
    assert_eq!(n, 57);
    assert_eq!((s.map.get(16, 12, 16), s.map.get(16, 8, 16), s.map.get(16, 13, 16)), (7, 7, 0));
    // Erase with a sphere carves only filled cells.
    let erase = Target { voxel: [16, 10, 16], normal: [0, 1, 0], value: 7, ground: false };
    assert_eq!(drag(&mut s, brush(Tool::Erase, Shape::Sphere), erase, &[[16, 10, 17]]), 7);
}

#[test]
fn mirror_on_every_axis_and_a_new_scene_mirrors_inside_the_map() {
    let mut s = Scene::new([32, 32, 32]);
    // A new scene is centred on x = z = 0, and mirrors y across the middle of its height.
    assert_eq!(s.mirror([20, 3, 6], 0), [11, 3, 6]);
    assert_eq!(s.mirror([20, 3, 6], 1), [20, 28, 6]);
    assert_eq!(s.mirror([20, 3, 6], 2), [20, 3, 25]);
    let b = Brush { mirror: [true, true, true], ..brush(Tool::Add, Shape::Voxel) };
    let ground = Target { voxel: [20, 2, 6], normal: [0, 1, 0], value: 0, ground: true };
    assert_eq!(drag(&mut s, b, ground, &[]), 8);
    assert_eq!(s.map.get(11, 28, 25), 7);
    // On a mirror plane the images coincide and are set once.
    let b = Brush { mirror: [false, false, true], ..b };
    assert_eq!(drag(&mut s, b, Target { voxel: [5, 2, 15], ..ground }, &[]), 2);
    assert_eq!(drag(&mut s, b, Target { voxel: [5, 4, 15], ..ground }, &[[5, 5, 16]]), 2, "15 and 16 mirror");
}

#[test]
fn axis_lock_keeps_the_drag_across_that_axis() {
    let mut s = slab();
    // Pressing the top face would draw on the y layer; locked to z it draws a wall at z = 1.
    let b = Brush { lock: Some(2), ..brush(Tool::Add, Shape::Box) };
    let Press::Drag(mut g) = s.press(b, top([0, 0, 1], 3)) else { panic!() };
    assert_eq!((g.axis, g.layer), (2, 1));
    g.move_to(&mut s, [2, 3, 1]);
    assert_eq!(g.release(&mut s), 9);
    assert_eq!(s.map.get(2, 3, 1), 7);
    // Locked erase drags stay on the layer instead of following the surface.
    let b = Brush { lock: Some(1), ..brush(Tool::Erase, Shape::Voxel) };
    let Press::Drag(g) = s.press(b, top([3, 0, 3], 3)) else { panic!() };
    assert!(g.on_plane() && g.axis == 1 && g.layer == 0);
}

#[test]
fn plane_cell_crosses_the_layer_middle() {
    assert_eq!(plane_cell([0.2, 5.0, 0.7], [1.0, -1.0, 0.0], 1, 2), Some([2, 2, 0]));
    assert_eq!(plane_cell([0.2, 5.0, 0.7], [1.0, 0.0, 0.0], 1, 2), None);
    assert_eq!(plane_cell([0.2, 5.0, 0.7], [0.0, 1.0, 0.0], 1, 2), None, "behind the ray");
}

#[test]
fn a_new_edit_after_a_drag_drops_redo_and_undo_commits_an_open_stroke() {
    let mut s = slab();
    let Press::Drag(mut g) = s.press(brush(Tool::Add, Shape::Voxel), top([1, 0, 1], 3)) else { panic!() };
    g.move_to(&mut s, [2, 1, 1]);
    // Undo during a drag closes the stroke first, then undoes it.
    assert!(s.undo());
    assert_eq!(s.map.get(1, 1, 1), 0);
    assert!(s.history().can_redo());
    s.edit([([9, 9, 9], 1)]);
    assert!(!s.history().can_redo());
}

#[test]
fn a_sphere_dragged_out_and_back_matches_one_drawn_at_once() {
    let cells = |s: &Scene| {
        let mut v = Vec::new();
        s.for_each_voxel(|p, c| v.push((p, c)));
        v
    };
    let ground = Target { voxel: [16, -1, 16], normal: [0, 1, 0], value: 0, ground: true };
    let b = Brush { mirror: [true, false, true], ..brush(Tool::Add, Shape::Sphere) };
    let mut once = Scene::new([48, 48, 48]);
    let n = drag(&mut once, b, ground, &[[22, 0, 16]]);
    // Grown a cell at a time, past the end, and back.
    let mut grown = Scene::new([48, 48, 48]);
    let path: Vec<_> = (17..=26).chain((22..26).rev()).map(|x| [x, 0, 16]).collect();
    assert_eq!(drag(&mut grown, b, ground, &path), n);
    assert_eq!(cells(&grown), cells(&once));
    assert!(grown.undo() && grown.map.voxels == 0, "one undo step");
}

#[test]
fn voxel_brush_size_stamps_a_cube_out_of_the_face_for_add_and_into_it_for_erase_and_paint() {
    // Add, size 3, on the slab's top at (2, 0, 2): a 3x3x3 cube resting on the face, centred
    // on the cell above it.
    let mut s = slab();
    let before = s.map.voxels;
    let b = Brush { size: 3, ..brush(Tool::Add, Shape::Voxel) };
    assert_eq!(drag(&mut s, b, top([2, 0, 2], 3), &[]), 27);
    assert_eq!(s.map.voxels, before + 27);
    for (x, y, z) in [(1, 1, 1), (3, 3, 3), (2, 2, 2)] {
        assert_eq!(s.map.get(x, y, z), 7, "{x},{y},{z}");
    }
    assert_eq!((s.map.get(2, 4, 2), s.map.get(4, 1, 2), s.map.get(2, 0, 2)), (0, 0, 3), "nothing outside");
    // A drag on the plane stamps at every cell it crosses, as one undo step.
    assert!(s.undo());
    assert_eq!(drag(&mut s, b, top([2, 0, 2], 3), &[[4, 1, 2]]), 27 + 2 * 9);
    assert!(s.undo());
    assert_eq!(s.map.voxels, before);
    // Even sizes reach one cell further on the high side.
    let b2 = Brush { size: 2, ..b };
    assert_eq!(drag(&mut s, b2, top([2, 0, 2], 3), &[]), 8);
    assert_eq!((s.map.get(3, 2, 3), s.map.get(1, 1, 1)), (7, 0));
    assert!(s.undo());
    // Erase and paint go into the face and touch only filled cells: a size 3 erase on the
    // slab's top removes the 3x3 under it (the slab is one layer deep), and a size 2 paint on
    // the bump paints it and the slab under it.
    let e = Brush { size: 3, ..brush(Tool::Erase, Shape::Voxel) };
    assert_eq!(drag(&mut s, e, top([2, 0, 2], 3), &[]), 9);
    assert!(s.undo());
    let p = Brush { size: 2, ..brush(Tool::Paint, Shape::Voxel) };
    assert_eq!(drag(&mut s, p, top([0, 1, 0], 5), &[]), 5, "the bump and the 2x2 of slab below");
    assert_eq!((s.map.get(0, 1, 0), s.map.get(1, 0, 1), s.map.get(2, 0, 2)), (7, 7, 3));
    // Mirror applies to every cell of the cube.
    assert!(s.undo());
    let m = Brush { mirror: [true, false, false], ..b };
    let n = drag(&mut s, m, top([2, 0, 2], 3), &[]);
    assert!(n > 27, "mirror adds the cube's image too: {n}");
}

#[test]
fn box_height_lifts_the_base_along_the_drag_axis_as_one_step() {
    // Add: a 2x1x3 base on the slab's top, lifted 3 layers high.
    let mut s = slab();
    let before = s.map.voxels;
    let Press::Drag(mut g) = s.press(brush(Tool::Add, Shape::Box), top([1, 0, 1], 3)) else { panic!() };
    g.move_to(&mut s, [2, 1, 3]);
    assert_eq!(s.map.voxels, before + 6, "the base");
    g.lift();
    assert!(g.lifting);
    // Looking along -z at the far corner's column: the ray's height picks the layer.
    let ro = [2.5, 3.5, 20.0];
    assert_eq!(g.lift_cell(ro, [0.0, 0.0, -1.0]), Some(3));
    assert_eq!(g.lift_cell([2.5, 1.5, 20.0], [0.0, 0.0, -1.0]), Some(1), "at release the box is one layer high");
    assert_eq!(g.lift_cell([2.5, 9.0, 3.5], [0.0, -1.0, 0.0]), None, "a ray along the axis says nothing");
    g.lift_to(&mut s, 3);
    assert_eq!(g.extent(), ([1, 1, 1], [2, 3, 3]));
    assert_eq!(s.map.voxels, before + 18);
    g.lift_to(&mut s, 2);
    assert_eq!(s.map.voxels, before + 12, "lowering redraws");
    assert_eq!(g.release(&mut s), 12);
    assert!(s.undo());
    assert_eq!(s.map.voxels, before, "base and height are one undo step");

    // Erase: a 4x1x2 base on the top layer of a 3-high block, pushed down into it.
    let mut s = Scene::new([32, 32, 32]);
    s.edit((0..4).flat_map(|x| (0..3).flat_map(move |y| (0..2).map(move |z| ([x, y, z], 3)))).collect::<Vec<_>>());
    let Press::Drag(mut g) = s.press(brush(Tool::Erase, Shape::Box), top([0, 2, 0], 3)) else { panic!() };
    g.move_to(&mut s, [3, 2, 1]);
    g.lift();
    let h = g.lift_cell([3.5, 0.2, 20.0], [0.0, 0.0, -1.0]).unwrap();
    assert_eq!(h, 0);
    g.lift_to(&mut s, h);
    assert_eq!(g.release(&mut s), 24);
    assert_eq!(s.map.voxels, 0);

    // Other shapes never lift.
    let mut s = slab();
    let Press::Drag(mut g) = s.press(brush(Tool::Add, Shape::Line), top([1, 0, 1], 3)) else { panic!() };
    g.lift();
    assert!(!g.lifting);
    g.lift_to(&mut s, 5);
    assert_eq!(g.extent(), ([1, 1, 1], [1, 1, 1]));
}

fn ground(voxel: [i32; 3]) -> Target {
    Target { voxel, normal: [0, 1, 0], value: 0, ground: true }
}

/// The filled cells, as (cell, slot).
fn cells(s: &Scene) -> Vec<([i32; 3], u8)> {
    let mut out = Vec::new();
    s.for_each_voxel(|p, c| out.push((p, c)));
    out.sort();
    out
}

/// Catches: a box dragged toward the horizon visited every cell out to the pointer, inside
/// the map or not, on each move: 3.4 s a move at 16,000 cells out, which froze the app.
#[test]
fn a_box_dragged_far_off_the_map_fills_only_the_map_and_stays_fast() {
    let mut s = Scene::new([32, 32, 32]);
    let t = std::time::Instant::now();
    let n = drag(&mut s, brush(Tool::Add, Shape::Box), ground([0, -1, 0]), &[[100_000, 0, 100_000]]);
    assert!(t.elapsed().as_secs_f32() < 1.0, "{:?}", t.elapsed());
    assert_eq!(n, 32 * 32, "the whole ground layer, nothing else");

    // Mirrored, the cells off the map whose images land on it still count: with the mirror
    // plane at map x = 3.5, x = -1 to -24 mirror to 8 to 31.
    let run = |to: i32| {
        let mut s = Scene::new([32, 32, 32]);
        s.origin = [-4, 0, 0];
        let b = Brush { mirror: [true, false, false], ..brush(Tool::Add, Shape::Box) };
        drag(&mut s, b, ground([0, -1, 0]), &[[to, 0, 0]]);
        cells(&s)
    };
    let near = run(-24);
    assert_eq!(near.len(), 26, "x = 0 and 7 to 31");
    assert_eq!(run(-100_000), near);
}

/// Catches: the radius summed squared offsets in i32, which overflowed past about 46,341
/// cells: a panic in a debug build, and in release a radius of 0, so the sphere vanished.
#[test]
fn a_sphere_dragged_far_off_the_map_draws_at_the_largest_radius() {
    use vxr_core::tools::MAX_RADIUS;
    let sphere = |to: [i32; 3]| {
        let mut s = Scene::new([32, 32, 32]);
        let Press::Drag(mut g) = s.press(brush(Tool::Add, Shape::Sphere), ground([16, -1, 16])) else { panic!() };
        g.move_to(&mut s, to);
        let r = g.radius();
        g.release(&mut s);
        (r, cells(&s))
    };
    let (r, far) = sphere([50_000, 0, 16]);
    assert_eq!(r, MAX_RADIUS);
    assert!(!far.is_empty());
    assert_eq!((r, far), sphere([16 + MAX_RADIUS, 0, 16]));
}

/// Catches: a voxel stroke and the line brush visited every step out to the pointer, inside
/// the map or not: a size-16 stroke toward the horizon stamped a 4,096-cell cube at each of
/// 100,000 steps on one move, which froze the app as the far box did.
#[test]
fn a_stroke_or_line_dragged_far_off_the_map_changes_only_the_map_and_stays_fast() {
    let run = |b: Brush, to: [i32; 3]| {
        let mut s = Scene::new([32, 32, 32]);
        let t = std::time::Instant::now();
        drag(&mut s, b, ground([0, -1, 0]), &[to]);
        assert!(t.elapsed().as_secs_f32() < 1.0, "{:?}", t.elapsed());
        cells(&s)
    };
    let line = run(brush(Tool::Add, Shape::Line), [100_000, 0, 100_000]);
    assert_eq!(line, (0..32).map(|k| ([k, 0, k], 7)).collect::<Vec<_>>(), "the diagonal across the map");

    let stroke = Brush { size: 16, ..brush(Tool::Add, Shape::Voxel) };
    let far = run(stroke, [100_000, 0, 100_000]);
    assert_eq!(far, run(stroke, [40, 0, 40]), "past 40 no 16-cube reaches the map");
    assert!(far.len() > 32 * 16);
}
