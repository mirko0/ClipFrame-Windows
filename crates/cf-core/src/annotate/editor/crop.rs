//! Cropping: dragging out a crop and applying it as an undoable edit.

use super::{EditorState, Gesture};
use crate::annotate::area::Area;
use crate::annotate::document::Edit;
use crate::annotate::shape::Vec2;

/// Crops smaller than this (a click) are ignored.
const MIN_CROP: f32 = 8.0;

impl EditorState {
    /// The crop to show: the one being dragged, else the document's.
    pub fn crop_preview(&self) -> Option<Area> {
        match &self.gesture {
            Some(Gesture::Cropping { from, to }) => Some(self.crop_area(*from, *to)),
            _ => self.doc.crop(),
        }
    }

    /// The crop dragged from `from` to `to`, inside the image and on whole pixels.
    pub(super) fn crop_area(&self, from: Vec2, to: Vec2) -> Area {
        let area = Area::from_corners(from, to);
        let area = if self.canvas.x > 0.0 {
            area.clamp_to(self.canvas)
        } else {
            area
        };
        area.round()
    }

    pub(super) fn finish_crop(&mut self, from: Vec2, to: Vec2) {
        let area = self.crop_area(from, to);
        if area.width() < MIN_CROP || area.height() < MIN_CROP {
            return;
        }
        let whole = Area::from_corners(Vec2::default(), self.canvas);
        self.set_crop((area != whole).then_some(area));
    }

    pub(super) fn set_crop(&mut self, crop: Option<Area>) {
        let before = self.doc.crop();
        if before != crop {
            self.doc.apply(Edit::Crop {
                before,
                after: crop,
            });
        }
    }
}
