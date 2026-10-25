//! Undo and redo. Each step is one user action (a click, a drag, a recolour) stored as
//! the cells it changed with their old and new values, so undo is exact and cheap.

use vxr_vox::Material;

/// One voxel that an edit changed.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Change {
    pub pos: [i32; 3],
    pub old: u8,
    pub new: u8,
}

/// One palette slot that an edit recoloured.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ColorChange {
    pub index: u8,
    pub old: [u8; 4],
    pub new: [u8; 4],
}

/// One palette slot whose material an edit changed.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct MaterialChange {
    pub index: u8,
    pub old: Material,
    pub new: Material,
}

/// The selection before and after a step that moved or changed it (map cells).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct SelectionChange {
    pub old: Vec<[i32; 3]>,
    pub new: Vec<[i32; 3]>,
}

/// The model bounds: the world cell of the low corner, the size in voxels, and the world
/// layer the ground plane sits on.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Bounds {
    pub lo: [i32; 3],
    pub size: [u32; 3],
    pub floor: i32,
}

/// The bounds before and after a resize or crop.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BoundsChange {
    pub old: Bounds,
    pub new: Bounds,
}

/// One undo step: the voxels, palette colours and materials a single action changed. A
/// recolour has only colours, a swap of two slots has all three (the colours and materials
/// swapped, and the voxels remapped to keep their look). Steps that move a selection (move,
/// rotate, paste, delete) also record it, so undo puts the selection back with the voxels.
/// A resize or crop records the bounds, with the voxels it dropped.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Step {
    pub voxels: Vec<Change>,
    pub colors: Vec<ColorChange>,
    pub materials: Vec<MaterialChange>,
    pub selection: Option<Box<SelectionChange>>,
    pub bounds: Option<BoundsChange>,
}

impl Step {
    pub fn is_empty(&self) -> bool {
        self.voxels.is_empty()
            && self.colors.is_empty()
            && self.materials.is_empty()
            && self.selection.is_none()
            && self.bounds.is_none()
    }

    /// Add `d` to every map cell the step names, after the map moved.
    pub fn translate(&mut self, d: [i32; 3]) {
        let shift = |p: &mut [i32; 3]| *p = std::array::from_fn(|a| p[a] + d[a]);
        self.voxels.iter_mut().for_each(|c| shift(&mut c.pos));
        if let Some(s) = &mut self.selection {
            s.old.iter_mut().chain(s.new.iter_mut()).for_each(shift);
        }
    }
}

#[derive(Clone, Debug, Default)]
pub struct History {
    undo: Vec<Step>,
    redo: Vec<Step>,
}

impl History {
    /// Record a new step. Any redo steps are dropped, as in every editor.
    pub fn push(&mut self, step: Step) {
        self.undo.push(step);
        self.redo.clear();
    }

    /// The step to revert, moved onto the redo stack.
    pub fn undo(&mut self) -> Option<&Step> {
        let s = self.undo.pop()?;
        self.redo.push(s);
        self.redo.last()
    }

    /// The step to apply again, moved back onto the undo stack.
    pub fn redo(&mut self) -> Option<&Step> {
        let s = self.redo.pop()?;
        self.undo.push(s);
        self.undo.last()
    }

    pub fn can_undo(&self) -> bool {
        !self.undo.is_empty()
    }

    pub fn can_redo(&self) -> bool {
        !self.redo.is_empty()
    }

    /// Add `d` to every map cell in every step, after the map moved.
    pub fn translate(&mut self, d: [i32; 3]) {
        self.undo.iter_mut().chain(self.redo.iter_mut()).for_each(|s| s.translate(d));
    }

    pub fn clear(&mut self) {
        self.undo.clear();
        self.redo.clear();
    }
}
