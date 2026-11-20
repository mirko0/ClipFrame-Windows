//! Growing a shape while it is dragged out.

use crate::annotate::area::Area;
use crate::annotate::shape::{Shape, Vec2, inset_size};

/// Pen points closer than this to the previous one are skipped.
const PEN_MIN_STEP: f32 = 1.0;
/// Space between a magnify source and its inset.
const INSET_GAP: f32 = 16.0;

pub fn extend(shape: &mut Shape, p: Vec2, canvas: Vec2) {
    match shape {
        Shape::Pen(points) | Shape::Highlight(points) => {
            if points
                .last()
                .is_none_or(|last| p.minus(*last).length() >= PEN_MIN_STEP)
            {
                points.push(p);
            }
        }
        Shape::Line { to, .. } | Shape::Arrow { to, .. } => *to = p,
        Shape::Rect { b, .. }
        | Shape::Ellipse { b, .. }
        | Shape::Blur { b, .. }
        | Shape::Redact { b, .. }
        | Shape::Spotlight { b, .. } => *b = p,
        Shape::Magnify { a, b, at } => {
            *b = p;
            *at = place_inset(*a, p, canvas);
        }
        Shape::Text { .. } | Shape::Step { .. } => {}
    }
}

/// Where a magnify inset goes: right of its source, else left, kept inside `canvas`
/// (when known).
fn place_inset(a: Vec2, b: Vec2, canvas: Vec2) -> Vec2 {
    let source = Area::from_corners(a, b);
    let size = inset_size(a, b);
    let mut x = source.max.x + INSET_GAP;
    let mut y = source.min.y;
    if canvas.x > 0.0 && x + size.x > canvas.x {
        x = source.min.x - INSET_GAP - size.x;
        if x < 0.0 {
            x = (canvas.x - size.x).max(0.0);
        }
    }
    if canvas.y > 0.0 {
        y = y.min(canvas.y - size.y).max(0.0);
    }
    Vec2::new(x, y)
}
