//! Model tools: hollow and fill, of the whole model and of the selection, one undo step each.

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
