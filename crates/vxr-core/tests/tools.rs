//! The editing tools: each brush on a small scene, mirror, and one undo step per gesture.

use proptest::prelude::*;
use vxr_core::Scene;
use vxr_core::select::Mode;
use vxr_core::tools::{Brush, Press, Shape, Target, Tool, plane_cell};

fn brush(tool: Tool, shape: Shape) -> Brush {
    Brush { tool, shape, mirror: [false; 3], lock: None, color: 7, size: 1, inside: false }
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
/// cells (and in i64 at a pointer saturated to i32::MIN): a panic in a debug build, and in
/// release a radius of 0, so the sphere vanished.
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
    assert_eq!((r, far.clone()), sphere([16 + MAX_RADIUS, 0, 16]));
    // A ray almost along the ground from a zoomed-out camera saturates the cell to i32::MIN,
    // where two squared offsets overflow even i64.
    assert_eq!(sphere([i32::MIN, 0, i32::MIN]).0, MAX_RADIUS);
    assert_eq!(sphere([i32::MAX, 0, i32::MAX]).0, MAX_RADIUS);
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
    // Saturated pointer cells (a ray almost along the ground) on both ends of i32.
    assert_eq!(run(brush(Tool::Add, Shape::Line), [i32::MAX, 0, i32::MAX]), line);
    assert_eq!(run(brush(Tool::Add, Shape::Line), [i32::MIN, 0, 0]), [([0, 0, 0], 7)]);

    let stroke = Brush { size: 16, ..brush(Tool::Add, Shape::Voxel) };
    let far = run(stroke, [100_000, 0, 100_000]);
    assert_eq!(far, run(stroke, [40, 0, 40]), "past 40 no 16-cube reaches the map");
    assert!(far.len() > 32 * 16);
}

/// Catches: a mirrored paint coloured the mirror image of a voxel even where that cell was
/// empty, so painting one side of a lopsided model grew voxels on the other.
#[test]
fn a_mirrored_paint_or_erase_never_fills_an_empty_mirror_cell() {
    let mut s = slab();
    // The slab spans x 0..4, so the mirror of x 0 across the pivot is outside it.
    let image = s.mirror_x([0, 0, 2]);
    assert_eq!(s.map.get(image[0], image[1], image[2]), 0);
    let before = s.map.voxels;
    let b = Brush { mirror: [true, false, false], ..brush(Tool::Paint, Shape::Voxel) };
    assert_eq!(drag(&mut s, b, top([0, 0, 2], 3), &[]), 1);
    assert_eq!((s.map.get(0, 0, 2), s.map.get(image[0], image[1], image[2])), (7, 0));
    assert_eq!(s.map.voxels, before);
}

/// The face brush with the region option: erase takes only the selected part of the face,
/// add still builds on the whole face, and the erased voxels leave the selection.
#[test]
fn the_face_brush_erases_only_selected_voxels_and_adds_everywhere() {
    let mut s = slab();
    s.select([[1, 0, 1], [2, 0, 1]], Mode::Replace);
    let inside = |tool| Brush { inside: true, ..brush(tool, Shape::Face) };
    let Press::Done(n) = s.press(inside(Tool::Add), top([3, 0, 3], 3)) else { panic!() };
    assert_eq!(n, 15, "add covers the face's 15 open cells, selected or not");
    assert!(s.undo());
    let Press::Done(n) = s.press(inside(Tool::Erase), top([3, 0, 3], 3)) else { panic!() };
    assert_eq!(n, 2);
    assert_eq!((s.map.get(1, 0, 1), s.map.get(2, 0, 1), s.map.get(3, 0, 3)), (0, 0, 3));
    assert_eq!(s.selected_count(), 0);
    assert!(s.undo());
    assert_eq!((s.map.voxels, s.selected_count()), (17, 2), "one undo step gives back voxels and selection");
}

proptest! {
    #![proptest_config(ProptestConfig {
        cases: std::env::var("PROPTEST_CASES").ok().and_then(|v| v.parse().ok()).unwrap_or(256),
        ..ProptestConfig::default()
    })]

    /// The region option on any erase or paint drag (every shape, any mirror, any brush
    /// size): each selected cell ends as the same drag without the option leaves it and
    /// each other cell is untouched. With nothing selected, and for add, the option
    /// changes nothing.
    #[test]
    fn a_limited_brush_changes_selected_cells_as_the_free_brush_does_and_no_others(
        // Cells on both sides of the mirror planes and a brick edge (8); the flag is "selected".
        model in prop::collection::btree_map((4i32..12, 4i32..12, 4i32..12), (1u8..=6, any::<bool>()), 1..160),
        none_selected in prop::bool::weighted(0.15),
        tool in prop::sample::select(vec![Tool::Erase, Tool::Paint, Tool::Add]),
        shape in prop::sample::select(vec![Shape::Voxel, Shape::Box, Shape::Line, Shape::Circle, Shape::Sphere]),
        mirror in any::<[bool; 3]>(),
        size in 1u8..=3,
        from in (4i32..12, 4i32..12, 4i32..12),
        moves in prop::collection::vec((2i32..14, 2i32..14, 2i32..14), 0..4),
    ) {
        let run = |inside: bool| {
            let mut s = Scene::new([16, 16, 16]);
            // Every axis mirrors q to 15 - q.
            (s.origin, s.pivot) = ([0; 3], [16; 3]);
            s.edit(model.iter().map(|(&(x, y, z), &(v, _))| ([x, y, z], v)));
            if !none_selected {
                s.select(model.iter().filter(|c| c.1.1).map(|(&(x, y, z), _)| [x, y, z]), Mode::Replace);
            }
            let selected: Vec<[i32; 3]> = s.selection().collect();
            let before = cells(&s);
            let b = Brush { mirror, size, inside, ..brush(tool, shape) };
            let from = [from.0, from.1, from.2];
            let t = Target { voxel: from, normal: [0, 1, 0], value: s.map.get(from[0], from[1], from[2]), ground: false };
            let moves: Vec<[i32; 3]> = moves.iter().map(|&(x, y, z)| [x, y, z]).collect();
            drag(&mut s, b, t, &moves);
            let after = (0..16).flat_map(|x| (0..16).flat_map(move |y| (0..16).map(move |z| [x, y, z])));
            let after: Vec<u8> = after.map(|p| s.map.get(p[0], p[1], p[2])).collect();
            (selected, before, after, s)
        };
        let (_, _, free, _) = run(false);
        let (selected, before, limited, mut s) = run(true);
        let all = (0..16).flat_map(|x| (0..16).flat_map(move |y| (0..16).map(move |z| [x, y, z])));
        for (k, p) in all.enumerate() {
            let was = before.iter().find(|c| c.0 == p).map_or(0, |c| c.1);
            let want = if tool == Tool::Add || selected.is_empty() || selected.contains(&p) { free[k] } else { was };
            prop_assert_eq!(limited[k], want, "cell {:?}", p);
        }
        // The limited drag is still one undo step.
        if cells(&s) != before {
            prop_assert!(s.undo());
        }
        prop_assert_eq!(cells(&s), before);
    }
}

/// A 4x4x4 block of colour 3 at 10..14 on every axis, with an L of three voxels on the
/// clipboard: 8 at its low corner, 9 beside it on +x and 6 above it (a 2x2x1 box).
fn block_with_an_l_copied() -> Scene {
    let mut s = Scene::new([32, 32, 32]);
    s.edit([([20, 0, 20], 8), ([21, 0, 20], 9), ([20, 1, 20], 6)]);
    s.select_all();
    assert_eq!(s.copy_selection(), 3);
    s.delete_selection();
    let block = (10..14).flat_map(|x| (10..14).flat_map(move |y| (10..14).map(move |z| ([x, y, z], 3))));
    s.edit(block);
    s
}

/// What differs from the 4x4x4 block: each changed cell with its slot now.
fn off_block(s: &Scene) -> Vec<([i32; 3], u8)> {
    let mut out = Vec::new();
    for x in 0..32 {
        for y in 0..32 {
            for z in 0..32 {
                let block = [x, y, z].iter().all(|k| (10..14).contains(k));
                let v = s.map.get(x, y, z);
                if v != if block { 3 } else { 0 } {
                    out.push(([x, y, z], v));
                }
            }
        }
    }
    out
}

/// Catches: the stamp set on the wrong side of the face (inside the wall for add, in the
/// air for erase), not centred, or keyed to one colour instead of the copy's own.
#[test]
fn pattern_brush_stamps_the_copy_against_the_pressed_face() {
    let face = |voxel, normal| Target { voxel, normal, value: 3, ground: false };
    let rows = [
        // Add stands the copy on a top face, where Paste would put it.
        (Tool::Add, face([12, 13, 12], [0, 1, 0]), vec![([11, 14, 12], 8), ([11, 15, 12], 6), ([12, 14, 12], 9)]),
        // On a side face it grows out of the wall, not turned, centred on the pressed cell.
        (Tool::Add, face([10, 11, 12], [-1, 0, 0]), vec![([8, 10, 12], 8), ([8, 11, 12], 6), ([9, 10, 12], 9)]),
        (Tool::Add, face([12, 11, 13], [0, 0, 1]), vec![([11, 10, 14], 8), ([11, 11, 14], 6), ([12, 10, 14], 9)]),
        // Erase and paint go into the face.
        (Tool::Erase, face([12, 13, 12], [0, 1, 0]), vec![([11, 12, 12], 0), ([11, 13, 12], 0), ([12, 12, 12], 0)]),
        (Tool::Paint, face([12, 13, 12], [0, 1, 0]), vec![([11, 12, 12], 8), ([11, 13, 12], 6), ([12, 12, 12], 9)]),
        // At an edge, erase and paint leave the empty cells the copy covers empty.
        (Tool::Paint, face([10, 13, 12], [0, 1, 0]), vec![([10, 12, 12], 9)]),
        (Tool::Erase, face([10, 13, 12], [0, 1, 0]), vec![([10, 12, 12], 0)]),
    ];
    for (tool, t, want) in rows {
        let mut s = block_with_an_l_copied();
        let n = drag(&mut s, brush(tool, Shape::Pattern), t, &[]);
        assert_eq!((off_block(&s), n), (want.clone(), want.len()), "{tool:?} at {t:?}");
        assert!(s.undo());
        assert_eq!(off_block(&s), [], "{tool:?}: one undo step");
    }
}

#[test]
fn pattern_brush_moves_with_the_drag_and_needs_a_copy() {
    // Catches: a drag leaving a stamp at every cell it crossed (it is one stamp, moved).
    let mut s = block_with_an_l_copied();
    let t = Target { voxel: [12, 13, 12], normal: [0, 1, 0], value: 3, ground: false };
    let Press::Drag(mut g) = s.press(brush(Tool::Add, Shape::Pattern), t) else { panic!("expected a drag") };
    assert_eq!(g.extent(), ([11, 14, 12], [12, 15, 12]));
    g.move_to(&mut s, [11, 14, 11]);
    g.move_to(&mut s, [20, 14, 10]);
    assert_eq!(g.extent(), ([19, 14, 10], [20, 15, 10]));
    assert_eq!(g.release(&mut s), 3);
    assert_eq!(off_block(&s), [([19, 14, 10], 8), ([19, 15, 10], 6), ([20, 14, 10], 9)]);
    // The region option limits an erase to the selected voxels.
    s.undo();
    s.select([[11, 12, 12]], Mode::Replace);
    let inside = Brush { inside: true, ..brush(Tool::Erase, Shape::Pattern) };
    assert_eq!(drag(&mut s, inside, t, &[]), 1);
    assert_eq!(off_block(&s), [([11, 12, 12], 0)]);
    // With an empty clipboard the press says so and changes nothing.
    let mut s = slab();
    let before = cells(&s);
    assert!(matches!(s.press(brush(Tool::Add, Shape::Pattern), top([1, 0, 1], 3)), Press::NoPattern));
    assert_eq!(cells(&s), before);
}

proptest! {
    /// Any copy stamped on any face of an empty scene: the stamp is the copy, voxel for
    /// voxel, it lies wholly in front of the face and touches it, and the pressed cell is
    /// at its middle across the face (within the one cell an even size leaves).
    #[test]
    fn a_stamp_is_the_copy_set_in_front_of_the_face(
        copy in prop::collection::btree_map((0i32..5, 0i32..5, 0i32..5), 1u8..=6, 1..40),
        voxel in (10i32..20, 10i32..20, 10i32..20),
        axis in 0usize..3,
        positive in any::<bool>(),
    ) {
        let mut s = Scene::new([32, 32, 32]);
        s.edit(copy.iter().map(|(&(x, y, z), &v)| ([x, y, z], v)));
        s.select_all();
        s.copy_selection();
        s.delete_selection();
        let low = |cells: Vec<([i32; 3], u8)>| {
            let lo: [i32; 3] = std::array::from_fn(|a| cells.iter().map(|c| c.0[a]).min().unwrap());
            let hi: [i32; 3] = std::array::from_fn(|a| cells.iter().map(|c| c.0[a]).max().unwrap());
            let rel: Vec<_> = cells.iter().map(|&(p, v)| (std::array::from_fn::<i32, 3, _>(|a| p[a] - lo[a]), v)).collect();
            (rel, lo, hi)
        };
        let (want, _, _) = low(copy.iter().map(|(&(x, y, z), &v)| ([x, y, z], v)).collect());
        let voxel = [voxel.0, voxel.1, voxel.2];
        let mut normal = [0; 3];
        normal[axis] = if positive { 1 } else { -1 };
        let t = Target { voxel, normal, value: 0, ground: false };
        prop_assert_eq!(drag(&mut s, brush(Tool::Add, Shape::Pattern), t, &[]), copy.len());
        let (got, lo, hi) = low(cells(&s));
        prop_assert_eq!(got, want);
        for a in 0..3 {
            if a == axis {
                let (near, far) = if positive { (lo[a], hi[a]) } else { (hi[a], lo[a]) };
                prop_assert_eq!(near, voxel[a] + normal[a], "touches the face");
                prop_assert!((far - voxel[a]) * normal[a] >= 1, "in front of the face");
            } else {
                let (below, above) = (voxel[a] - lo[a], hi[a] - voxel[a]);
                prop_assert!(below >= 0 && above >= 0 && (below - above).abs() <= 1, "centred on axis {}: {} {}", a, below, above);
            }
        }
    }
}
