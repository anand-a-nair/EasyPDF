//! Undoable mutations.
//!
//! Every change to a [`Document`] goes through a [`Command`]. This is the
//! mechanism behind undo/redo, and it also makes each mutation testable in
//! isolation from the UI. See `ideas/02-architecture.md`.

use crate::document::{Document, PageIndex, Rotation};
use crate::error::Result;

/// A reversible mutation.
///
/// `invert` must restore exactly the state that existed before `apply`. A
/// command that cannot describe its own inverse does not belong here.
pub trait Command: std::fmt::Debug + Send {
    /// Applies the change.
    fn apply(&self, doc: &mut Document) -> Result<()>;

    /// Reverses a previously applied change.
    fn invert(&self, doc: &mut Document) -> Result<()>;

    /// Short description for the undo menu ("Rotate page 3").
    fn label(&self) -> String;
}

/// Rotates a single page.
#[derive(Debug, Clone, Copy)]
pub struct RotatePage {
    /// Page to rotate.
    pub page: PageIndex,
    /// Rotation to apply, relative to the page's current rotation.
    pub by: Rotation,
}

impl Command for RotatePage {
    fn apply(&self, doc: &mut Document) -> Result<()> {
        let page = doc.page_mut(self.page)?;
        page.rotation = page.rotation.then(self.by);
        Ok(())
    }

    fn invert(&self, doc: &mut Document) -> Result<()> {
        // Rotations form a cyclic group of order 4, so the inverse of `by` is
        // whatever completes the turn back to zero.
        let inverse = Rotation::from_degrees(-self.by.degrees())?;
        let page = doc.page_mut(self.page)?;
        page.rotation = page.rotation.then(inverse);
        Ok(())
    }

    fn label(&self) -> String {
        format!("Rotate page {}", self.page.0 + 1)
    }
}

/// Moves a page to a different position.
#[derive(Debug, Clone, Copy)]
pub struct MovePage {
    /// Current position.
    pub from: PageIndex,
    /// Target position.
    pub to: PageIndex,
}

impl Command for MovePage {
    fn apply(&self, doc: &mut Document) -> Result<()> {
        doc.move_page(self.from, self.to)
    }

    fn invert(&self, doc: &mut Document) -> Result<()> {
        doc.move_page(self.to, self.from)
    }

    fn label(&self) -> String {
        format!("Move page {} to {}", self.from.0 + 1, self.to.0 + 1)
    }
}

/// Undo/redo history.
#[derive(Debug, Default)]
pub struct CommandStack {
    done: Vec<Box<dyn Command>>,
    undone: Vec<Box<dyn Command>>,
}

impl CommandStack {
    /// Creates an empty stack.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Applies a command and records it for undo.
    ///
    /// A command that fails is not recorded, so a failed operation can never
    /// leave an un-undoable entry in the history.
    pub fn apply(&mut self, doc: &mut Document, command: Box<dyn Command>) -> Result<()> {
        command.apply(doc)?;
        self.done.push(command);
        // A new action invalidates the redo branch — standard linear history.
        self.undone.clear();
        Ok(())
    }

    /// Reverses the most recent command. Returns its label.
    pub fn undo(&mut self, doc: &mut Document) -> Result<Option<String>> {
        let Some(command) = self.done.pop() else {
            return Ok(None);
        };
        command.invert(doc)?;
        let label = command.label();
        self.undone.push(command);
        Ok(Some(label))
    }

    /// Re-applies the most recently undone command. Returns its label.
    pub fn redo(&mut self, doc: &mut Document) -> Result<Option<String>> {
        let Some(command) = self.undone.pop() else {
            return Ok(None);
        };
        command.apply(doc)?;
        let label = command.label();
        self.done.push(command);
        Ok(Some(label))
    }

    /// Whether there is anything to undo.
    #[must_use]
    pub fn can_undo(&self) -> bool {
        !self.done.is_empty()
    }

    /// Whether there is anything to redo.
    #[must_use]
    pub fn can_redo(&self) -> bool {
        !self.undone.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::document::{Page, PageSize};

    fn doc(n: usize) -> Document {
        Document::from_pages(
            (0..n).map(|_| Page { size: PageSize::A4, rotation: Rotation::None }).collect(),
        )
    }

    #[test]
    fn undo_restores_exact_prior_state() {
        let mut d = doc(3);
        let mut stack = CommandStack::new();
        let before = d.clone();

        stack
            .apply(&mut d, Box::new(RotatePage { page: PageIndex(1), by: Rotation::Clockwise90 }))
            .unwrap();
        assert_ne!(d.page(PageIndex(1)).unwrap(), before.page(PageIndex(1)).unwrap());

        stack.undo(&mut d).unwrap();
        assert_eq!(d.page(PageIndex(1)).unwrap(), before.page(PageIndex(1)).unwrap());
    }

    #[test]
    fn redo_reapplies_the_change() {
        let mut d = doc(2);
        let mut stack = CommandStack::new();
        stack
            .apply(&mut d, Box::new(RotatePage { page: PageIndex(0), by: Rotation::Half }))
            .unwrap();
        stack.undo(&mut d).unwrap();
        stack.redo(&mut d).unwrap();
        assert_eq!(d.page(PageIndex(0)).unwrap().rotation, Rotation::Half);
    }

    #[test]
    fn round_trip_through_four_rotations_returns_to_origin() {
        let mut d = doc(1);
        let mut stack = CommandStack::new();
        for _ in 0..4 {
            stack
                .apply(
                    &mut d,
                    Box::new(RotatePage { page: PageIndex(0), by: Rotation::Clockwise90 }),
                )
                .unwrap();
        }
        assert_eq!(d.page(PageIndex(0)).unwrap().rotation, Rotation::None);
    }

    #[test]
    fn move_page_undo_restores_order() {
        let mut d = doc(4);
        d.page_mut(PageIndex(0)).unwrap().rotation = Rotation::Half;
        let mut stack = CommandStack::new();

        stack.apply(&mut d, Box::new(MovePage { from: PageIndex(0), to: PageIndex(3) })).unwrap();
        assert_eq!(d.page(PageIndex(3)).unwrap().rotation, Rotation::Half);

        stack.undo(&mut d).unwrap();
        assert_eq!(d.page(PageIndex(0)).unwrap().rotation, Rotation::Half);
    }

    #[test]
    fn failed_command_is_not_recorded_in_history() {
        // Otherwise undo would try to reverse something that never happened.
        let mut d = doc(2);
        let mut stack = CommandStack::new();
        let result =
            stack.apply(&mut d, Box::new(RotatePage { page: PageIndex(99), by: Rotation::Half }));
        assert!(result.is_err());
        assert!(!stack.can_undo());
    }

    #[test]
    fn new_action_clears_the_redo_branch() {
        let mut d = doc(2);
        let mut stack = CommandStack::new();
        stack
            .apply(&mut d, Box::new(RotatePage { page: PageIndex(0), by: Rotation::Half }))
            .unwrap();
        stack.undo(&mut d).unwrap();
        assert!(stack.can_redo());

        stack
            .apply(&mut d, Box::new(RotatePage { page: PageIndex(1), by: Rotation::Half }))
            .unwrap();
        assert!(!stack.can_redo(), "redo branch must not survive a new action");
    }

    #[test]
    fn undo_on_empty_stack_is_not_an_error() {
        let mut d = doc(1);
        let mut stack = CommandStack::new();
        assert_eq!(stack.undo(&mut d).unwrap(), None);
    }
}
