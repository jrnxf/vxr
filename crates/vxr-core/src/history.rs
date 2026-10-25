//! Undo and redo. Each step is one user action (a click, a drag, a recolour) stored as
//! the cells it changed with their old and new values, so undo is exact and cheap.

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

/// One undo step: the voxels and palette slots a single action changed. A recolour has
/// only colours, a swap of two slots has both (the colours and the voxels remapped to keep
/// their look).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Step {
    pub voxels: Vec<Change>,
    pub colors: Vec<ColorChange>,
}

impl Step {
    pub fn is_empty(&self) -> bool {
        self.voxels.is_empty() && self.colors.is_empty()
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

    pub fn clear(&mut self) {
        self.undo.clear();
        self.redo.clear();
    }
}
