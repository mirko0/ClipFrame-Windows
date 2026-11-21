//! Text annotations: typing, committing, re-editing, and resizing by dragging a corner (the
//! font size scales with the distance from the text's top-left corner).

use super::{EditorState, Gesture, Preview, Tool};
use crate::annotate::document::Edit;
use crate::annotate::hit;
use crate::annotate::shape::{Annotation, Shape, Style, Vec2};

/// Smallest and largest font size, in image pixels.
const MIN_SIZE: f32 = 8.0;
const MAX_SIZE: f32 = 400.0;

/// Text being typed; becomes an annotation when committed.
#[derive(Debug, Clone, PartialEq)]
pub struct TextDraft {
    at: Vec2,
    text: String,
    size: f32,
    style: Style,
    /// The existing text annotation being edited, if any.
    editing: Option<(usize, Annotation)>,
}

impl TextDraft {
    pub(super) const fn set_style(&mut self, style: Style) {
        self.style = style;
    }

    fn annotation(&self) -> Annotation {
        Annotation {
            shape: Shape::Text {
                at: self.at,
                text: self.text.clone(),
                size: self.size,
            },
            style: self.style,
        }
    }

    /// Sets the size to `size` scaled like [`resized`] does.
    pub(super) fn resize(&mut self, size: f32, grab: Vec2, p: Vec2) {
        self.size = scaled(size, self.at, grab, p);
    }

    pub(super) fn preview(&self) -> Preview {
        Preview {
            replaces: self.editing.as_ref().map(|(i, _)| *i),
            item: self.annotation(),
            typing: true,
        }
    }
}

impl EditorState {
    /// Types `text` into the text being edited.
    pub fn type_text(&mut self, text: &str) {
        if let Some(draft) = &mut self.draft {
            draft.text.extend(text.chars().filter(|c| !c.is_control()));
        }
    }

    /// Starts a new line in the text being edited.
    pub fn new_line(&mut self) {
        if let Some(draft) = &mut self.draft {
            draft.text.push('\n');
        }
    }

    /// Deletes the last character of the text being edited.
    pub fn backspace(&mut self) {
        if let Some(draft) = &mut self.draft {
            draft.text.pop();
        }
    }

    /// Finishes the text being typed (empty text is dropped, or deletes the edited text).
    pub fn commit_text(&mut self) {
        let Some(draft) = self.draft.take() else {
            return;
        };
        let item = draft.annotation();
        let keep = !item.shape.is_degenerate();
        match (draft.editing, keep) {
            (Some((index, before)), true) if before != item => self.doc.apply(Edit::Replace {
                index,
                before,
                after: item,
            }),
            (Some((index, before)), false) => self.doc.apply(Edit::Remove {
                index,
                item: before,
            }),
            (None, true) => self.insert(item),
            _ => {}
        }
    }

    /// Starts dragging the resize handle of text `index` from `grab` (normally its
    /// bottom-right corner). Returns `false` if `index` isn't text.
    pub fn begin_text_resize(&mut self, index: usize, grab: Vec2) -> bool {
        self.commit_text();
        let Some(item) = self.doc.items().get(index).cloned() else {
            return false;
        };
        if !matches!(item.shape, Shape::Text { .. }) {
            return false;
        }
        self.selected = Some(index);
        self.gesture = Some(Gesture::Resizing {
            index,
            grab,
            before: item.clone(),
            current: item,
        });
        true
    }

    /// Starts dragging the resize handle of the text being typed, from `grab`; typing
    /// carries on afterwards. Returns `false` if no text is being typed.
    pub fn begin_typing_resize(&mut self, grab: Vec2) -> bool {
        let Some(draft) = &self.draft else {
            return false;
        };
        self.gesture = Some(Gesture::ResizingDraft {
            grab,
            size: draft.size,
        });
        true
    }

    /// The text being typed and the position of the item it replaces, if any (for drawing
    /// its frame).
    pub fn typing_item(&self) -> Option<Annotation> {
        self.draft.as_ref().map(TextDraft::annotation)
    }

    pub(super) fn start_text(&mut self, p: Vec2, slop_scale: f32) {
        let existing = hit::topmost_at(self.doc.items(), p, slop_scale)
            .filter(|&i| matches!(self.doc.items()[i].shape, Shape::Text { .. }));
        let style = self.styles[Tool::Text.index()];
        self.draft = Some(match existing {
            Some(index) => {
                let item = self.doc.items()[index].clone();
                let Shape::Text { at, text, size } = &item.shape else {
                    unreachable!("filtered to text above");
                };
                TextDraft {
                    at: *at,
                    text: text.clone(),
                    size: *size,
                    style: item.style,
                    editing: Some((index, item)),
                }
            }
            None => TextDraft {
                at: p,
                text: String::new(),
                size: style.text_size(),
                style,
                editing: None,
            },
        });
    }
}

/// `before` (text) with its size scaled by how far `p` is from the top-left corner, compared
/// with where the drag started (`grab`).
pub(super) fn resized(before: &Annotation, grab: Vec2, p: Vec2) -> Annotation {
    let Shape::Text { at, text, size } = &before.shape else {
        return before.clone();
    };
    Annotation {
        shape: Shape::Text {
            at: *at,
            text: text.clone(),
            size: scaled(*size, *at, grab, p),
        },
        style: before.style,
    }
}

/// `size` scaled by the distance of `p` from `at`, compared with that of `grab`.
fn scaled(size: f32, at: Vec2, grab: Vec2, p: Vec2) -> f32 {
    let start = grab.minus(at).length().max(1.0);
    let now = p.minus(at).length();
    (size * now / start).clamp(MIN_SIZE, MAX_SIZE)
}
