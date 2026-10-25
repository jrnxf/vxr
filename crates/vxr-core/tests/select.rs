//! Selection and transform: box and colour select, move (live drag and by offset), copy,
//! cut, paste, delete, fill, rotate, flip and loop, of the selection and the whole model,
//! each one undo step that restores the selection too.

use std::collections::BTreeMap;

use vxr_core::Scene;
use vxr_core::select::{Mode, Region, Transform};
use vxr_core::tools::Target;

/// An L of colour 3 on y = 0 ((0..3, 0, 0) and (0, 0, 1)), plus one voxel of colour 5 at
/// (5, 0, 5), all offset into the middle of a 32^3 map.
const O: i32 = 10;
fn scene() -> Scene {
    let mut s = Scene::new([32, 32, 32]);
    s.edit([([O, O, O], 3), ([O + 1, O, O], 3), ([O + 2, O, O], 3), ([O, O, O + 1], 3), ([O + 5, O, O + 5], 5)]);
    s
}

fn voxels(s: &Scene) -> BTreeMap<[i32; 3], u8> {
    let mut m = BTreeMap::new();
    s.for_each_voxel(|p, v| {
        m.insert(p, v);
    });
    m
}

fn sel(s: &Scene) -> Vec<[i32; 3]> {
    let mut v: Vec<_> = s.selection().collect();
    v.sort();
    v
}

#[test]
fn select_by_box_and_colour_with_modes() {
    let mut s = scene();
    s.select([[O, O, O], [O + 1, O, O], [O + 9, O, O]], Mode::Replace);
    assert_eq!(sel(&s), vec![[O, O, O], [O + 1, O, O]], "empty cells are not selected");
    s.select_color(5, Mode::Add);
    assert_eq!(s.selected_count(), 3);
    s.select([[O, O, O]], Mode::Subtract);
    assert_eq!(sel(&s), vec![[O + 1, O, O], [O + 5, O, O + 5]]);
    s.select_color(3, Mode::Replace);
    assert_eq!(s.selected_count(), 4);
    let v = s.selection_version();
    s.deselect();
    assert_eq!((s.selected_count(), s.selection_version() > v), (0, true));
    s.select_all();
    assert_eq!(s.selected_count(), 5);
    s.select_color(3, Mode::Replace);
    s.invert_selection();
    assert_eq!(sel(&s), vec![[O + 5, O, O + 5]], "invert keeps only the voxels not selected");
    s.invert_selection();
    assert_eq!(s.selected_count(), 4, "inverting twice gives the selection back");
    s.deselect();
    s.invert_selection();
    assert_eq!(s.selected_count(), 5, "inverting nothing selects every voxel");
    assert!(s.undo(), "the scene's own edit");
    assert!(!s.undo(), "selecting is not an undo step");
}

#[test]
fn a_click_selects_the_colour_the_face_or_the_volume() {
    // The L, a voxel of colour 4 on top of its corner, and the far voxel of colour 5.
    let mut s = scene();
    s.edit([([O, O + 1, O], 4)]);
    let top = |p: [i32; 3], value| Target { voxel: p, normal: [0, 1, 0], value, ground: false };
    let t = top([O + 1, O, O], 3);
    s.select_region(&t, Region::Color, Mode::Replace);
    assert_eq!(s.selected_count(), 4, "every voxel of colour 3");
    s.select_region(&t, Region::Face, Mode::Replace);
    assert_eq!(sel(&s), vec![[O + 1, O, O], [O + 2, O, O]], "the covered corner splits the face");
    s.select_region(&t, Region::Volume, Mode::Replace);
    assert_eq!(s.selected_count(), 5, "the L and the voxel on it, not the far voxel");
    s.select_region(&top([O + 5, O, O + 5], 5), Region::Volume, Mode::Add);
    assert_eq!(s.selected_count(), 6);
    s.select_region(&top([O, O + 1, O], 4), Region::Face, Mode::Subtract);
    assert_eq!(s.selected_count(), 5);
    assert!(!s.is_selected([O, O + 1, O]));
    let ground = Target { voxel: [O, O - 1, O], normal: [0, 1, 0], value: 0, ground: true };
    s.select_region(&ground, Region::Volume, Mode::Replace);
    assert_eq!(s.selected_count(), 5, "the ground selects nothing");
    assert_eq!(s.volume([O + 9, O, O]), Vec::<[i32; 3]>::new(), "an empty cell has no volume");
}

#[test]
fn erased_voxels_leave_the_selection() {
    let mut s = scene();
    s.select_color(3, Mode::Replace);
    assert_eq!(s.selected_count(), 4);
    s.put([O, O, O], 0);
    assert_eq!(s.selected_count(), 3, "the count follows an open edit");
    s.put([O + 1, O, O], 0);
    s.commit();
    assert_eq!(sel(&s), vec![[O, O, O + 1], [O + 2, O, O]]);
    s.edit([([O, O, O], 4)]);
    assert!(!s.is_selected([O, O, O]), "a voxel added where one was erased is not selected");
    assert!(s.undo());
    assert!(s.undo(), "the erase");
    assert_eq!(s.selected_count(), 4, "undoing the erase selects the voxels again");
    assert!(s.redo());
    assert_eq!(s.selected_count(), 2);
}

#[test]
fn voxels_emptied_by_undo_leave_the_selection() {
    let mut s = scene();
    let v = [O + 9, O, O];
    s.edit([(v, 4)]);
    s.select_all();
    assert!(s.is_selected(v));
    assert!(s.undo(), "the add");
    s.edit([(v, 4)]);
    assert!(!s.is_selected(v), "a voxel added where undo emptied a selected cell is not selected");
    assert_eq!(s.selected_count(), 5);
    s.delete_selection();
    assert_eq!(voxels(&s).get(&v), Some(&4), "delete leaves the unselected voxel");
}

#[test]
fn move_drag_is_live_and_one_undo_step() {
    let mut s = scene();
    let before = voxels(&s);
    s.select_color(3, Mode::Replace);
    let mut g = s.start_move(1, [O, O, O]).unwrap();
    g.move_to(&mut s, [O + 3, O, O]);
    assert_eq!(s.map.get(O + 3, O, O), 3, "shown while dragging");
    g.move_to(&mut s, [O, O, O + 4]);
    assert_eq!((s.map.get(O + 3, O, O), s.map.get(O, O, O + 4)), (0, 3), "redrawn from the start");
    g.move_to(&mut s, [O + 1, O + 7, O + 2]);
    assert_eq!(g.offset, [1, 0, 2], "stays on the drag plane");
    g.release(&mut s);
    let after = voxels(&s);
    assert_eq!(after.len(), before.len());
    assert_eq!(after.get(&[O + 1, O, O + 2]), Some(&3));
    assert_eq!(sel(&s), vec![[O + 1, O, O + 2], [O + 1, O, O + 3], [O + 2, O, O + 2], [O + 3, O, O + 2]]);
    assert!(s.undo());
    assert_eq!(voxels(&s), before, "one undo step");
    assert_eq!(sel(&s), vec![[O, O, O], [O, O, O + 1], [O + 1, O, O], [O + 2, O, O]], "selection restored");
    assert!(s.redo());
    assert_eq!(voxels(&s), after);
    assert_eq!(s.selected_count(), 4);
}

#[test]
fn copy_cut_paste_delete_fill() {
    let mut s = scene();
    s.select_color(3, Mode::Replace);
    assert_eq!(s.copy_selection(), 4);
    let clip = s.clipboard.clone().unwrap();
    assert_eq!((clip.size, clip.from), ([3, 1, 2], [O, O, O]));
    // Beside the original on +X, then at a cell (bottom centre).
    let c = s.paste_corner(None).unwrap();
    assert_eq!(c, [O + 3, O, O]);
    assert_eq!(s.paste(c), 4);
    assert_eq!(s.map.get(O + 5, O, O), 3);
    assert_eq!(s.selected_count(), 4, "the pasted copy is selected");
    let c = s.paste_corner(Some([O + 10, O + 1, O + 10])).unwrap();
    assert_eq!(c, [O + 9, O + 1, O + 9]);
    s.paste(c);
    assert_eq!(s.map.get(O + 9, O + 1, O + 9), 3);
    assert_eq!(voxels(&s).len(), 13);
    assert!(s.undo());
    assert_eq!(voxels(&s).len(), 9);
    assert_eq!(sel(&s)[0], [O + 3, O, O], "undo restores the first paste's selection");
    // Fill, delete, cut.
    assert_eq!(s.fill_selection(9), 4);
    assert_eq!(s.map.get(O + 3, O, O), 9);
    assert_eq!(s.delete_selection(), 4);
    assert_eq!((s.map.get(O + 3, O, O), s.selected_count()), (0, 0));
    assert!(s.undo());
    assert_eq!((s.map.get(O + 3, O, O), s.selected_count()), (9, 4), "undo of delete reselects");
    s.select_color(5, Mode::Replace);
    assert_eq!(s.cut_selection(), 1);
    assert_eq!(s.map.get(O + 5, O, O + 5), 0);
    assert_eq!(s.clipboard.as_ref().unwrap().voxels, vec![([0, 0, 0], 5)]);
}

#[test]
fn rotate_flip_loop_selection_and_whole_model() {
    let mut s = scene();
    let before = voxels(&s);
    s.select_color(3, Mode::Replace);
    // The L's bounds are 3 x 1 x 2 at (O, O, O). Rotating about Y turns it to 2 x 1 x 3,
    // centred: x starts at O + (3 - 2) / 2 = O, z at O + (2 - 3) / 2 = O.
    s.transform(Transform::Rotate(1));
    let mut cells = sel(&s);
    cells.sort();
    assert_eq!(cells.len(), 4);
    let (lo, hi) = s.selection_bounds().unwrap();
    assert_eq!(([hi[0] - lo[0], hi[1] - lo[1], hi[2] - lo[2]]), [1, 0, 2]);
    for _ in 0..3 {
        s.transform(Transform::Rotate(1));
    }
    assert_eq!(voxels(&s), before, "four turns are the identity");
    for a in 0..3 {
        s.transform(Transform::Flip(a));
        s.transform(Transform::Flip(a));
    }
    assert_eq!(voxels(&s), before, "two flips are the identity");
    s.transform(Transform::Flip(0));
    assert_eq!((s.map.get(O + 2, O, O + 1), s.map.get(O, O, O + 1)), (3, 0));
    s.transform(Transform::Flip(0));
    // Loop: the 3-wide row shifts and wraps inside its bounds.
    s.transform(Transform::Loop(0, 1));
    assert_eq!((s.map.get(O + 1, O, O + 1), s.map.get(O, O, O + 1)), (3, 0));
    s.transform(Transform::Loop(0, -1));
    assert_eq!(voxels(&s), before);
    // Nothing selected: the whole model (bounds 6 x 1 x 6).
    s.deselect();
    s.transform(Transform::Flip(2));
    assert_eq!((s.map.get(O + 5, O, O), s.map.get(O, O, O + 5), s.map.get(O, O, O + 4)), (5, 3, 3));
    assert_eq!(s.selected_count(), 0, "a whole-model transform selects nothing");
    s.transform(Transform::Move([0, 2, 0]));
    assert_eq!(s.map.get(O + 5, O + 2, O), 5);
    assert!(s.undo() && s.undo());
    assert_eq!(voxels(&s), before);
}

#[test]
fn a_transform_that_leaves_the_map_changes_nothing() {
    // A row along x on the floor of a 32^3 map: turned upright about Z, it would reach y = -14.
    let mut s = Scene::new([32, 32, 32]);
    s.edit((1..31).map(|x| ([x, 0, 5], 3)));
    let before = voxels(&s);
    assert_eq!(s.transform(Transform::Rotate(2)), None);
    assert_eq!(s.transform(Transform::Move([2, 0, 0])), None, "x = 31 fits, 32 does not");
    assert_eq!(voxels(&s), before);
    assert!(s.undo() && !s.undo(), "only the scene's own edit is a step");
    // A drag stops at the edge instead of dropping what crosses it.
    let mut s = scene();
    s.select_color(5, Mode::Replace);
    let mut g = s.start_move(1, [O + 5, O, O + 5]).unwrap();
    g.move_to(&mut s, [O + 8, O, O + 5]);
    g.move_to(&mut s, [O + 40, O, O + 5]);
    assert_eq!(g.offset, [3, 0, 0]);
    g.release(&mut s);
    assert_eq!(s.map.get(O + 8, O, O + 5), 5);
}

#[test]
fn moving_onto_other_voxels_overwrites_and_undo_restores_them() {
    let mut s = scene();
    let before = voxels(&s);
    s.select_color(5, Mode::Replace);
    s.transform(Transform::Move([-5, 0, -5]));
    assert_eq!((s.map.get(O, O, O), s.map.get(O + 5, O, O + 5)), (5, 0));
    assert!(s.undo());
    assert_eq!(voxels(&s), before);
}
