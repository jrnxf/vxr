//! Undo and redo. Each step is one user action (a click, a drag, a recolour) stored as
//! the cells it changed with their old and new values, so undo is exact and cheap.

/// One voxel that an edit changed.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Change {
    pub pos: [i32; 3],
    pub old: u8,
    pub new: u8,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Step {
    Voxels(Vec<Change>),
    Palette { index: u8, old: [u8; 4], new: [u8; 4] },
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
