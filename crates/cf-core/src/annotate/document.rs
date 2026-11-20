//! The annotation list and its undo/redo history. Every change is an [`Edit`], so undo is
//! unlimited and exact.

use super::shape::{Annotation, Shape};

/// One reversible change to the annotation list.
#[derive(Debug, Clone, PartialEq)]
pub enum Edit {
    /// Insert `item` at `index`.
    Insert {
        /// Position in z-order (0 = bottom).
        index: usize,
        /// The new annotation.
        item: Annotation,
    },
    /// Remove the annotation at `index` (which equals `item`).
    Remove {
        /// Position in z-order.
        index: usize,
        /// The removed annotation, kept for undo.
        item: Annotation,
    },
    /// Replace the annotation at `index`.
    Replace {
        /// Position in z-order.
        index: usize,
        /// Previous value.
        before: Annotation,
        /// New value.
        after: Annotation,
    },
}

impl Edit {
    /// The edit that undoes this one.
    #[must_use]
    pub fn inverse(self) -> Self {
        match self {
            Self::Insert { index, item } => Self::Remove { index, item },
            Self::Remove { index, item } => Self::Insert { index, item },
            Self::Replace {
                index,
                before,
                after,
            } => Self::Replace {
                index,
                before: after,
                after: before,
            },
        }
    }
}

/// Annotations over an image of `width × height` pixels, bottom-most first.
#[derive(Debug, Clone, Default)]
pub struct Document {
    items: Vec<Annotation>,
    undo: Vec<Edit>,
    redo: Vec<Edit>,
    /// Bumped on every change, so callers can tell when to re-export.
    revision: u64,
}

impl Document {
    /// An empty document.
    pub fn new() -> Self {
        Self::default()
    }

    /// The annotations, bottom-most first.
    pub fn items(&self) -> &[Annotation] {
        &self.items
    }

    /// Changes whenever the content changes (including undo and redo).
    pub const fn revision(&self) -> u64 {
        self.revision
    }

    /// Applies `edit` and records it for undo (clearing the redo history).
    pub fn apply(&mut self, edit: Edit) {
        if self.perform(edit.clone()) {
            self.undo.push(edit);
            self.redo.clear();
        }
    }

    /// Reverts the latest edit. Returns `false` when there is nothing to undo.
    pub fn undo(&mut self) -> bool {
        let Some(edit) = self.undo.pop() else {
            return false;
        };
        self.perform(edit.clone().inverse());
        self.redo.push(edit);
        true
    }

    /// Re-applies the latest undone edit. Returns `false` when there is nothing to redo.
    pub fn redo(&mut self) -> bool {
        let Some(edit) = self.redo.pop() else {
            return false;
        };
        self.perform(edit.clone());
        self.undo.push(edit);
        true
    }

    /// The 1-based number of the step badge at `index` (its order among all steps).
    pub fn step_number(&self, index: usize) -> usize {
        self.items[..=index.min(self.items.len().saturating_sub(1))]
            .iter()
            .filter(|a| matches!(a.shape, Shape::Step { .. }))
            .count()
    }

    /// The number the next step badge would get.
    pub fn next_step_number(&self) -> usize {
        self.items
            .iter()
            .filter(|a| matches!(a.shape, Shape::Step { .. }))
            .count()
            + 1
    }

    fn perform(&mut self, edit: Edit) -> bool {
        let done = match edit {
            Edit::Insert { index, item } if index <= self.items.len() => {
                self.items.insert(index, item);
                true
            }
            Edit::Remove { index, .. } if index < self.items.len() => {
                self.items.remove(index);
                true
            }
            Edit::Replace { index, after, .. } if index < self.items.len() => {
                self.items[index] = after;
                true
            }
            _ => false,
        };
        if done {
            self.revision += 1;
        }
        done
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::annotate::shape::{Style, Vec2};

    fn step(x: f32) -> Annotation {
        Annotation {
            shape: Shape::Step {
                at: Vec2::new(x, 0.0),
            },
            style: Style::default(),
        }
    }

    fn insert(doc: &mut Document, item: Annotation) {
        let index = doc.items().len();
        doc.apply(Edit::Insert { index, item });
    }

    #[test]
    fn undo_and_redo_restore_exact_states() {
        let mut doc = Document::new();
        insert(&mut doc, step(1.0));
        insert(&mut doc, step(2.0));
        doc.apply(Edit::Replace {
            index: 0,
            before: step(1.0),
            after: step(5.0),
        });
        assert_eq!(doc.items(), &[step(5.0), step(2.0)]);
        assert!(doc.undo());
        assert_eq!(doc.items(), &[step(1.0), step(2.0)]);
        assert!(doc.undo() && doc.undo() && !doc.undo());
        assert!(doc.items().is_empty());
        assert!(doc.redo() && doc.redo() && doc.redo() && !doc.redo());
        assert_eq!(doc.items(), &[step(5.0), step(2.0)]);
    }

    #[test]
    fn a_new_edit_clears_redo() {
        let mut doc = Document::new();
        insert(&mut doc, step(1.0));
        doc.undo();
        insert(&mut doc, step(2.0));
        assert!(!doc.redo());
    }

    #[test]
    fn steps_renumber_when_one_is_removed() {
        let mut doc = Document::new();
        for x in [1.0, 2.0, 3.0] {
            insert(&mut doc, step(x));
        }
        assert_eq!(doc.step_number(2), 3);
        doc.apply(Edit::Remove {
            index: 0,
            item: step(1.0),
        });
        assert_eq!(doc.step_number(1), 2);
        assert_eq!(doc.next_step_number(), 3);
    }

    #[test]
    fn revision_tracks_changes_and_ignores_invalid_edits() {
        let mut doc = Document::new();
        doc.apply(Edit::Remove {
            index: 3,
            item: step(0.0),
        });
        assert_eq!(doc.revision(), 0);
        assert!(!doc.undo());
        insert(&mut doc, step(1.0));
        doc.undo();
        assert_eq!(doc.revision(), 2);
    }
}
