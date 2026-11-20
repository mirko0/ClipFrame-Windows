//! Which annotation lies under a point (for selecting and moving).

use super::shape::{Annotation, Shape, Vec2};

/// Extra grab distance in image pixels, so thin lines are easy to pick.
const SLOP: f32 = 4.0;

/// Index of the topmost annotation under `p`.
pub fn topmost_at(items: &[Annotation], p: Vec2, slop_scale: f32) -> Option<usize> {
    items
        .iter()
        .rposition(|item| hits(item, p, SLOP * slop_scale))
}

fn hits(item: &Annotation, p: Vec2, slop: f32) -> bool {
    let reach = item.style.width / 2.0 + slop;
    match &item.shape {
        Shape::Pen(points) => points
            .windows(2)
            .any(|w| segment_distance(p, w[0], w[1]) <= reach),
        Shape::Line { from, to } | Shape::Arrow { from, to } => {
            segment_distance(p, *from, *to) <= reach
        }
        Shape::Rect { a, b } if !item.style.filled => rect_outline_hit(p, *a, *b, reach),
        Shape::Ellipse { a, b } if !item.style.filled => ellipse_outline_hit(p, *a, *b, reach),
        Shape::Rect { .. } | Shape::Ellipse { .. } | Shape::Text { .. } | Shape::Step { .. } => {
            let (min, max) = item.bounds();
            p.x >= min.x - slop && p.x <= max.x + slop && p.y >= min.y - slop && p.y <= max.y + slop
        }
    }
}

/// Distance from `p` to the segment `a`–`b`.
pub fn segment_distance(p: Vec2, a: Vec2, b: Vec2) -> f32 {
    let ab = b.minus(a);
    let len2 = ab.x * ab.x + ab.y * ab.y;
    if len2 <= f32::EPSILON {
        return p.minus(a).length();
    }
    let ap = p.minus(a);
    let t = ((ap.x * ab.x + ap.y * ab.y) / len2).clamp(0.0, 1.0);
    p.minus(Vec2::new(a.x + ab.x * t, a.y + ab.y * t)).length()
}

fn rect_outline_hit(p: Vec2, a: Vec2, b: Vec2, reach: f32) -> bool {
    let corners = [a, Vec2::new(b.x, a.y), b, Vec2::new(a.x, b.y)];
    (0..4).any(|i| segment_distance(p, corners[i], corners[(i + 1) % 4]) <= reach)
}

fn ellipse_outline_hit(p: Vec2, corner: Vec2, opposite: Vec2, reach: f32) -> bool {
    let center = Vec2::new(
        f32::midpoint(corner.x, opposite.x),
        f32::midpoint(corner.y, opposite.y),
    );
    let radius_x = (opposite.x - corner.x).abs() / 2.0;
    let radius_y = (opposite.y - corner.y).abs() / 2.0;
    if radius_x < 1.0 || radius_y < 1.0 {
        return segment_distance(p, corner, opposite) <= reach;
    }
    let offset = p.minus(center);
    // Normalised radius: 1.0 on the outline. Scale the tolerance by the smaller radius.
    let normalised = ((offset.x / radius_x).powi(2) + (offset.y / radius_y).powi(2)).sqrt();
    (normalised - 1.0).abs() * radius_x.min(radius_y) <= reach
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::annotate::shape::Style;

    fn item(shape: Shape, filled: bool) -> Annotation {
        Annotation {
            shape,
            style: Style {
                filled,
                ..Style::default()
            },
        }
    }

    #[test]
    fn segment_distance_clamps_to_the_ends() {
        let (a, b) = (Vec2::new(0.0, 0.0), Vec2::new(10.0, 0.0));
        assert!((segment_distance(Vec2::new(5.0, 3.0), a, b) - 3.0).abs() < 1e-5);
        assert!((segment_distance(Vec2::new(13.0, 4.0), a, b) - 5.0).abs() < 1e-5);
    }

    #[test]
    fn outlined_rect_is_hit_on_its_border_only() {
        let rect = Shape::Rect {
            a: Vec2::new(0.0, 0.0),
            b: Vec2::new(100.0, 100.0),
        };
        let items = [item(rect.clone(), false)];
        assert_eq!(topmost_at(&items, Vec2::new(1.0, 50.0), 1.0), Some(0));
        assert_eq!(topmost_at(&items, Vec2::new(50.0, 50.0), 1.0), None);
        let filled = [item(rect, true)];
        assert_eq!(topmost_at(&filled, Vec2::new(50.0, 50.0), 1.0), Some(0));
    }

    #[test]
    fn topmost_wins_and_ellipse_outline_counts() {
        let ellipse = Shape::Ellipse {
            a: Vec2::new(0.0, 0.0),
            b: Vec2::new(100.0, 50.0),
        };
        let step = Shape::Step {
            at: Vec2::new(100.0, 25.0),
        };
        let items = [item(ellipse, false), item(step, false)];
        assert_eq!(topmost_at(&items, Vec2::new(100.0, 25.0), 1.0), Some(1));
        assert_eq!(topmost_at(&items, Vec2::new(50.0, 0.5), 1.0), Some(0));
        assert_eq!(topmost_at(&items, Vec2::new(50.0, 25.0), 1.0), None);
    }
}
