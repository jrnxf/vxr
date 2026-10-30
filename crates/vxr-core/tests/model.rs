//! Model tools: hollow and fill, of the whole model and of the selection, and resize and
//! crop of the model bounds, one undo step each.

use vxr_core::Scene;
use vxr_core::select::Mode;

/// A solid `n`^3 block of colour `v` with its low corner at `lo`.
fn block(lo: i32, n: i32, v: u8) -> Vec<([i32; 3], u8)> {
    let mut out = Vec::new();
    for x in lo..lo + n {
        for y in lo..lo + n {
            for z in lo..lo + n {
                out.push(([x, y, z], v));
            }
        }
    }
    out
}

/// A solid `n`^3 cube of colour 3 with its low corner at (4, 4, 4) in a 32^3 map.
fn cube(n: i32) -> Scene {
    let mut s = Scene::new([32, 32, 32]);
    s.edit(block(4, n, 3));
    s
}

#[test]
fn hollow_keeps_the_shell_and_fill_restores_it() {
    let mut s = cube(5);
    assert_eq!(s.map.voxels, 125);
    assert_eq!(s.hollow(), 27, "the 3^3 core goes");
    assert_eq!((s.map.voxels, s.map.get(6, 6, 6), s.map.get(4, 6, 6)), (98, 0, 3));
    assert_eq!(s.hollow(), 0, "a shell has nothing left to hollow");
    assert_eq!(s.fill_interior(7), 27);
    assert_eq!((s.map.voxels, s.map.counts[7]), (125, 27));
    assert_eq!(s.fill_interior(7), 0, "nothing enclosed is left");
    assert!(s.undo());
    assert_eq!(s.map.voxels, 98, "fill is one undo step");
    assert!(s.undo());
    assert_eq!(s.map.voxels, 125, "hollow is one undo step");
    assert!(s.redo());
    assert_eq!(s.map.voxels, 98);
}

#[test]
fn an_open_cup_is_not_filled_and_thin_walls_stay() {
    let mut s = cube(4);
    // Open the top: a 2x2 hole through the top layer exposes the core below it.
    s.edit([([5, 7, 5], 0), ([6, 7, 5], 0), ([5, 7, 6], 0), ([6, 7, 6], 0)]);
    assert_eq!(s.hollow(), 4, "the core's top layer is seen through the hole, its bottom is not");
    assert_eq!(s.map.get(5, 6, 5), 3);
    assert_eq!(s.hollow(), 0, "every voxel of a cup one voxel thick is seen");
    s.edit([([5, 6, 5], 0), ([6, 6, 5], 0), ([5, 6, 6], 0), ([6, 6, 6], 0)]);
    let v = s.map.voxels;
    assert_eq!(s.fill_interior(7), 0, "the cup's inside reaches the outside");
    assert_eq!(s.map.voxels, v);
}

#[test]
fn hollow_and_fill_work_on_the_selection_only() {
    let mut s = cube(5);
    // A second cube of colour 9 beside the first.
    s.edit(block(12, 3, 9));
    s.select_color(3, Mode::Replace);
    assert_eq!(s.hollow(), 27, "only the selected cube is hollowed");
    assert_eq!(s.map.get(13, 13, 13), 9, "the other cube keeps its core");
    assert_eq!(s.selected_count(), 98, "removed voxels leave the selection");
    assert_eq!(s.fill_interior(5), 27);
    s.deselect();
    assert_eq!(s.fill_interior(5), 0, "the whole model encloses nothing now");
    assert_eq!(s.map.counts[5], 27);
}

#[test]
fn fill_skips_cells_other_voxels_hold() {
    let mut s = cube(5);
    s.hollow();
    // An unselected voxel inside the shell, then select the shell only.
    s.edit([([6, 6, 6], 9)]);
    s.select_color(3, Mode::Replace);
    assert_eq!(s.fill_interior(5), 26);
    assert_eq!(s.map.get(6, 6, 6), 9);
}

#[test]
fn empty_scene_does_nothing() {
    let mut s = Scene::new([16, 16, 16]);
    assert_eq!((s.hollow(), s.fill_interior(3)), (0, 0));
    assert!(!s.undo());
}

/// Every voxel in world cells, sorted.
fn world(s: &Scene) -> Vec<([i32; 3], u8)> {
    let mut out = Vec::new();
    s.for_each_voxel(|p, v| out.push((s.to_world(p), v)));
    out.sort();
    out
}

#[test]
fn crop_fits_the_bounds_to_the_voxels_and_undo_restores_them() {
    let mut s = cube(5);
    s.edit([([20, 4, 9], 5)]);
    let (before, cells) = (s.bounds(), world(&s));
    assert_eq!(before.size, [32, 32, 32]);
    assert!(s.crop());
    let b = s.bounds();
    assert_eq!((b.lo, b.size), (s.to_world([0, 0, 0]), [17, 5, 6]));
    assert_eq!(b.lo, [before.lo[0] + 4, before.lo[1] + 4, before.lo[2] + 4]);
    assert_eq!(world(&s), cells, "crop keeps every voxel where it was in the world");
    assert_eq!(b.floor, b.lo[1], "the ground moves up to the cropped bottom");
    assert!(!s.map.set(0, 5, 0, 1), "no cell outside the bounds");
    assert!(!s.crop(), "already cropped");
    let back = Scene::from_vox(&s.to_vox().unwrap()).unwrap();
    assert_eq!(world(&back), cells, "a cropped scene saves and opens the same");
    // An edit after the crop, in the new map's cells, undoes and redoes in place.
    s.edit([([16, 0, 0], 9)]);
    assert!(s.undo() && s.undo());
    assert_eq!((s.bounds(), world(&s)), (before, cells.clone()), "crop is one undo step");
    assert!(s.redo());
    assert_eq!(s.bounds(), b);
    assert!(s.redo());
    assert_eq!(s.map.get(16, 0, 0), 9, "the later edit redoes in the cropped map");
}

#[test]
fn resize_keeps_the_bottom_and_centre_and_drops_what_falls_outside() {
    let mut s = Scene::new([16, 16, 16]);
    let lo = s.bounds().lo;
    s.edit([([0, 0, 0], 1), ([8, 3, 8], 2), ([15, 15, 15], 3)]);
    s.select_all();
    let (cells, version) = (world(&s), s.reshapes());
    assert_eq!(s.resize([20, 16, 24]), Ok(0));
    let b = s.bounds();
    assert_eq!((b.lo, b.size), ([lo[0] - 2, lo[1], lo[2] - 4], [20, 16, 24]));
    assert_eq!((world(&s), s.reshapes()), (cells.clone(), version + 1));
    assert_eq!(s.selected_count(), 3, "the selection moves with the map");
    // Shrinking by an odd amount takes the extra cell from the high side.
    assert_eq!(s.resize([9, 9, 9]), Ok(2), "two corners fall outside");
    let b = s.bounds();
    assert_eq!((b.lo, b.size), ([lo[0] + 3, lo[1], lo[2] + 3], [9, 9, 9]));
    assert_eq!(world(&s), vec![([lo[0] + 8, lo[1] + 3, lo[2] + 8], 2)]);
    assert_eq!(s.selected_count(), 1);
    assert!(s.undo());
    assert_eq!((world(&s), s.selected_count()), (cells.clone(), 3), "resize is one undo step");
    assert!(s.undo());
    assert_eq!((s.bounds().lo, s.bounds().size, world(&s)), (lo, [16, 16, 16], cells));
    assert!(s.resize([0, 4, 4]).is_err());
    assert_eq!(s.resize([16, 16, 16]), Ok(0), "same size is no step");
    assert!(s.redo(), "the redo stack survives a no-op");
}

#[test]
fn crop_of_an_empty_scene_does_nothing() {
    let mut s = Scene::new([16, 16, 16]);
    assert!(!s.crop());
    assert!(!s.undo());
}

#[test]
fn a_resize_inside_an_open_edit_is_one_step_with_its_edits() {
    let mut s = Scene::new([16, 16, 16]);
    let lo = s.bounds().lo;
    s.edit([([0, 0, 0], 1), ([5, 5, 5], 4)]);
    s.select([[5, 5, 5]], Mode::Replace);
    let (bounds, cells) = (s.bounds(), world(&s));
    let w = |m: [i32; 3]| [lo[0] + m[0], lo[1] + m[1], lo[2] + m[2]];
    s.begin();
    s.put([1, 0, 0], 2);
    // Taller: nothing moves, and a voxel goes above the old top.
    assert_eq!(s.put_bounds(lo, [16, 24, 16]), Ok((0, [0, 0, 0])));
    assert!(s.put([1, 20, 0], 3));
    // One narrower from the low x side: map cells move one down x and (0, 0, 0) falls out.
    assert_eq!(s.put_bounds([lo[0] + 1, lo[1], lo[2]], [15, 24, 16]), Ok((1, [-1, 0, 0])));
    assert_eq!((s.map.get(0, 0, 0), s.map.get(0, 20, 0)), (2, 3), "the open edit's cells moved with the map");
    assert_eq!(s.selected_count(), 1);
    let after = world(&s);
    assert_eq!(after, vec![(w([1, 0, 0]), 2), (w([1, 20, 0]), 3), (w([5, 5, 5]), 4)]);
    // Reverted, the bounds come back with the voxels.
    s.revert_voxels();
    assert_eq!((s.bounds(), world(&s)), (bounds, cells.clone()));
    // Again, closed as one step: undo and redo move the bounds and the voxels together, the
    // voxel above the old top included.
    let position = s.history().position();
    s.put([1, 0, 0], 2);
    s.put_bounds(lo, [16, 24, 16]).unwrap();
    s.put([1, 20, 0], 3);
    s.put_bounds([lo[0] + 1, lo[1], lo[2]], [15, 24, 16]).unwrap();
    assert_eq!(s.commit(), 3);
    assert_eq!(s.history().position(), position + 1);
    let b = s.bounds();
    assert!(s.undo());
    assert_eq!((s.bounds(), world(&s), s.selected_count()), (bounds, cells, 1));
    assert!(s.redo());
    assert_eq!((s.bounds(), world(&s), s.selected_count()), (b, after, 1));
}
