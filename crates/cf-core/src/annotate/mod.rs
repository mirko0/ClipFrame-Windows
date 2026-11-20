//! The editor's document: annotations drawn over a captured image, with undo/redo.
//!
//! Coordinates are image pixels as `f32` (sub-pixel pen strokes). Rendering lives in the
//! app; this module only holds data and decides how input changes it.

mod document;
mod editor;
mod hit;
mod shape;

pub use document::{Document, Edit};
pub use editor::{EditorState, Preview, Tool};
pub use shape::{Annotation, Dash, Rgba, Shape, Style, Vec2};
