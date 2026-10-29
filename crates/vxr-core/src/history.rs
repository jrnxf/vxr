//! Undo and redo. Each step is one user action (a click, a drag, a recolour) stored as
//! the cells it changed with their old and new values, so undo is exact and cheap.

use vxr_vox::{Camera, Material};

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

/// One saved camera that an edit changed (the id is the same in both).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct CameraChange {
    pub old: Camera,
    pub new: Camera,
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

/// Where a step's map cells are: the map of a scene without a world, the world view, or
/// the model of one object (see [`crate::scene::Scene::edit_object`]).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum At {
    #[default]
    Map,
    World,
    Object(usize),
}

/// A world edit (move, copy, group, hide, ...): the tree before and after, and the models
/// it added (a duplicate's own model), which undo drops and redo puts back.
#[derive(Clone)]
pub struct WorldChange {
    pub old: vxr_vox::Tree,
    pub new: vxr_vox::Tree,
    /// How many models there were before.
    pub models: usize,
    pub added: Vec<crate::BrickMap>,
}

impl std::fmt::Debug for WorldChange {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("WorldChange").field("models", &self.models).field("added", &self.added.len()).finish()
    }
}

impl PartialEq for WorldChange {
    fn eq(&self, o: &Self) -> bool {
        self.old == o.old && self.new == o.new && self.models == o.models && self.added.len() == o.added.len()
    }
}

/// One undo step: the voxels, palette colours and materials a single action changed. A
/// recolour has only colours, a swap of two slots has all three (the colours and materials
/// swapped, and the voxels remapped to keep their look). Steps that move a selection (move,
/// rotate, paste, delete) also record it, so undo puts the selection back with the voxels.
/// A resize or crop records the bounds, with the voxels it dropped. A world edit records
/// the tree. A camera edit has only cameras. `at` says which map the cells belong to; undo
/// and redo go there first.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Step {
    pub voxels: Vec<Change>,
    pub colors: Vec<ColorChange>,
    pub materials: Vec<MaterialChange>,
    pub cameras: Vec<CameraChange>,
    pub selection: Option<Box<SelectionChange>>,
    pub bounds: Option<BoundsChange>,
    pub world: Option<Box<WorldChange>>,
    pub at: At,
    /// What the step is called, when it is more than an edit by hand ("AI: make the roof red").
    pub label: Option<String>,
}

impl Step {
    pub fn is_empty(&self) -> bool {
        self.voxels.is_empty()
            && self.colors.is_empty()
            && self.materials.is_empty()
            && self.cameras.is_empty()
            && self.selection.is_none()
            && self.bounds.is_none()
            && self.world.is_none()
    }

    /// Whether the step changes anything that belongs to the map at `at` (not only the palette).
    pub fn needs_map(&self) -> bool {
        !self.voxels.is_empty() || self.selection.is_some() || self.bounds.is_some() || self.world.is_some()
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
    undo: Vec<(u64, Step)>,
    redo: Vec<(u64, Step)>,
    /// The id the last pushed step got.
    last: u64,
}

impl History {
    /// Record a new step. Any redo steps are dropped, as in every editor.
    pub fn push(&mut self, step: Step) {
        self.last += 1;
        self.undo.push((self.last, step));
        self.redo.clear();
    }

    /// The step to revert, moved onto the redo stack.
    pub fn undo(&mut self) -> Option<&Step> {
        let s = self.undo.pop()?;
        self.redo.push(s);
        self.redo.last().map(|(_, s)| s)
    }

    /// The step to apply again, moved back onto the undo stack.
    pub fn redo(&mut self) -> Option<&Step> {
        let s = self.redo.pop()?;
        self.undo.push(s);
        self.undo.last().map(|(_, s)| s)
    }

    pub fn can_undo(&self) -> bool {
        !self.undo.is_empty()
    }

    pub fn can_redo(&self) -> bool {
        !self.redo.is_empty()
    }

    /// The name of the step undo would revert next, if it has one.
    pub fn undo_label(&self) -> Option<&str> {
        self.undo.last().and_then(|(_, s)| s.label.as_deref())
    }

    /// The name of the step redo would apply next, if it has one.
    pub fn redo_label(&self) -> Option<&str> {
        self.redo.last().and_then(|(_, s)| s.label.as_deref())
    }

    /// Where the history stands: the id of the last step applied, 0 before any. Two equal
    /// positions mean the same edits are applied, so the scene is the same.
    pub fn position(&self) -> u64 {
        self.undo.last().map_or(0, |(id, _)| *id)
    }

    /// The id of the step redo would apply next, if any.
    pub fn redo_id(&self) -> Option<u64> {
        self.redo.last().map(|(id, _)| *id)
    }

    /// Whether the step that got `id` is applied (on the undo stack).
    pub fn applied(&self, id: u64) -> bool {
        self.step(id).is_some()
    }

    /// The applied step that got `id`, if it is still on the undo stack.
    pub fn step(&self, id: u64) -> Option<&Step> {
        self.undo.iter().rev().find(|(i, _)| *i == id).map(|(_, s)| s)
    }

    /// Add `d` to every map cell in every step at `at`, after that map moved.
    pub fn translate(&mut self, at: At, d: [i32; 3]) {
        self.undo
            .iter_mut()
            .chain(self.redo.iter_mut())
            .map(|(_, s)| s)
            .filter(|s| s.at == at)
            .for_each(|s| s.translate(d));
    }

    pub fn clear(&mut self) {
        self.undo.clear();
        self.redo.clear();
    }
}
