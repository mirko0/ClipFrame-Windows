//! Annotation shapes and their styles.

use serde::{Deserialize, Serialize};

use super::area::Area;

/// A point or offset in image pixels.
#[derive(Debug, Clone, Copy, Default, PartialEq, Serialize, Deserialize)]
pub struct Vec2 {
    /// Horizontal.
    pub x: f32,
    /// Vertical.
    pub y: f32,
}

impl Vec2 {
    /// Creates a vector.
    pub const fn new(x: f32, y: f32) -> Self {
        Self { x, y }
    }

    /// `self - other`.
    #[must_use]
    pub fn minus(self, other: Self) -> Self {
        Self::new(self.x - other.x, self.y - other.y)
    }

    /// `self + other`.
    #[must_use]
    pub fn plus(self, other: Self) -> Self {
        Self::new(self.x + other.x, self.y + other.y)
    }

    /// Euclidean length.
    pub fn length(self) -> f32 {
        self.x.hypot(self.y)
    }
}

/// A straight-alpha colour; alpha is the annotation's opacity.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Rgba {
    /// Red.
    pub r: u8,
    /// Green.
    pub g: u8,
    /// Blue.
    pub b: u8,
    /// Opacity.
    pub a: u8,
}

impl Rgba {
    /// Creates a colour.
    pub const fn new(r: u8, g: u8, b: u8, a: u8) -> Self {
        Self { r, g, b, a }
    }

    /// The same colour with opacity `a`.
    #[must_use]
    pub const fn with_alpha(self, a: u8) -> Self {
        Self { a, ..self }
    }
}

/// Stroke pattern.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum Dash {
    /// Continuous.
    #[default]
    Solid,
    /// Long dashes.
    Dashed,
    /// Dots.
    Dotted,
}

impl Dash {
    /// The next pattern (for a toggle button).
    #[must_use]
    pub const fn next(self) -> Self {
        match self {
            Self::Solid => Self::Dashed,
            Self::Dashed => Self::Dotted,
            Self::Dotted => Self::Solid,
        }
    }
}

/// How an annotation is drawn.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Style {
    /// Stroke (and fill) colour.
    pub color: Rgba,
    /// Stroke width in image pixels.
    pub width: f32,
    /// Stroke pattern.
    pub dash: Dash,
    /// Rectangles and ellipses are filled instead of outlined.
    pub filled: bool,
}

impl Default for Style {
    fn default() -> Self {
        Self {
            color: Rgba::new(0xE5, 0x39, 0x35, 0xFF),
            width: 4.0,
            dash: Dash::Solid,
            filled: false,
        }
    }
}

impl Style {
    /// Text size for this style (text grows with the stroke width).
    pub fn text_size(&self) -> f32 {
        12.0 + self.width * 4.0
    }

    /// Radius of a step-counter badge.
    pub fn step_radius(&self) -> f32 {
        10.0 + self.width * 2.0
    }

    /// Length of an arrow head.
    pub fn arrow_head(&self) -> f32 {
        8.0 + self.width * 3.0
    }

    /// Width of a highlighter stroke.
    pub fn highlight_width(&self) -> f32 {
        8.0 + self.width * 4.0
    }

    /// Standard deviation of a blur.
    pub fn blur_sigma(&self) -> f32 {
        4.0 + self.width * 2.0
    }

    /// Edge length of a pixelation block, in whole pixels.
    pub fn pixel_block(&self) -> f32 {
        (6.0 + self.width * 2.5).round()
    }
}

/// How much a magnify inset enlarges its source.
pub const MAGNIFY_ZOOM: f32 = 2.0;

/// The geometry of one annotation.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum Shape {
    /// Freehand stroke.
    Pen(Vec<Vec2>),
    /// Straight line.
    Line {
        /// Start.
        from: Vec2,
        /// End.
        to: Vec2,
    },
    /// Line with a head at `to`.
    Arrow {
        /// Tail.
        from: Vec2,
        /// Head.
        to: Vec2,
    },
    /// Rectangle between two corners.
    Rect {
        /// One corner.
        a: Vec2,
        /// The opposite corner.
        b: Vec2,
    },
    /// Ellipse inscribed in the rectangle between two corners.
    Ellipse {
        /// One corner.
        a: Vec2,
        /// The opposite corner.
        b: Vec2,
    },
    /// Text with its top-left corner at `at`; may contain line breaks.
    Text {
        /// Top-left corner.
        at: Vec2,
        /// Content.
        text: String,
    },
    /// Numbered badge; the number is its position among all steps.
    Step {
        /// Centre.
        at: Vec2,
    },
    /// Freehand highlighter, drawn "under the ink" so dark text stays readable.
    Highlight(Vec<Vec2>),
    /// Blurred region (looks redacted but is not safe for sensitive data).
    Blur {
        /// One corner.
        a: Vec2,
        /// The opposite corner.
        b: Vec2,
    },
    /// Pixelated region, or a solid box when the style is filled.
    Redact {
        /// One corner.
        a: Vec2,
        /// The opposite corner.
        b: Vec2,
    },
    /// A hole in the single, global spotlight dim layer.
    Spotlight {
        /// One corner.
        a: Vec2,
        /// The opposite corner.
        b: Vec2,
    },
    /// The source rectangle `a`â€“`b` shown [`MAGNIFY_ZOOM`] times larger, with the inset's
    /// top-left at `at`.
    Magnify {
        /// One source corner.
        a: Vec2,
        /// The opposite source corner.
        b: Vec2,
        /// Top-left of the inset.
        at: Vec2,
    },
}

impl Shape {
    /// Moves the shape by `d`.
    pub fn translate(&mut self, d: Vec2) {
        match self {
            Self::Pen(points) | Self::Highlight(points) => {
                for p in points {
                    *p = p.plus(d);
                }
            }
            Self::Line { from, to } | Self::Arrow { from, to } => {
                *from = from.plus(d);
                *to = to.plus(d);
            }
            Self::Rect { a, b }
            | Self::Ellipse { a, b }
            | Self::Blur { a, b }
            | Self::Redact { a, b }
            | Self::Spotlight { a, b } => {
                *a = a.plus(d);
                *b = b.plus(d);
            }
            // A magnify inset moves on its own; its source stays put.
            Self::Text { at, .. } | Self::Step { at } | Self::Magnify { at, .. } => {
                *at = at.plus(d);
            }
        }
    }

    /// Whether the shape is too small to keep (a click without dragging).
    pub fn is_degenerate(&self) -> bool {
        const MIN: f32 = 2.0;
        match self {
            Self::Pen(points) | Self::Highlight(points) => points.len() < 2,
            Self::Line { from, to } | Self::Arrow { from, to } => to.minus(*from).length() < MIN,
            Self::Rect { a, b }
            | Self::Ellipse { a, b }
            | Self::Blur { a, b }
            | Self::Redact { a, b }
            | Self::Spotlight { a, b }
            | Self::Magnify { a, b, .. } => {
                let d = b.minus(*a);
                d.x.abs() < MIN || d.y.abs() < MIN
            }
            Self::Text { text, .. } => text.trim().is_empty(),
            Self::Step { .. } => false,
        }
    }
}

/// A shape with its style.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Annotation {
    /// Geometry.
    pub shape: Shape,
    /// Appearance.
    pub style: Style,
}

impl Annotation {
    /// Approximate bounds `(min, max)` including stroke width; text is estimated from its
    /// character count (exact layout is a rendering concern). A magnify item's bounds are
    /// its inset (what gets selected and moved).
    pub fn bounds(&self) -> (Vec2, Vec2) {
        let half = match self.shape {
            Shape::Highlight(_) => self.style.highlight_width() / 2.0,
            Shape::Blur { .. } | Shape::Redact { .. } | Shape::Spotlight { .. } => 0.0,
            _ => self.style.width / 2.0,
        };
        let (min, max) = match &self.shape {
            Shape::Pen(points) | Shape::Highlight(points) => span(points.iter().copied()),
            Shape::Line { from, to }
            | Shape::Arrow { from, to }
            | Shape::Rect { a: from, b: to }
            | Shape::Ellipse { a: from, b: to }
            | Shape::Blur { a: from, b: to }
            | Shape::Redact { a: from, b: to }
            | Shape::Spotlight { a: from, b: to } => span([*from, *to]),
            Shape::Magnify { a, b, at } => (*at, at.plus(inset_size(*a, *b))),
            Shape::Text { at, text } => {
                let size = self.style.text_size();
                let lines = text.lines().count().max(1);
                let longest = text.lines().map(|l| l.chars().count()).max().unwrap_or(0);
                #[allow(clippy::cast_precision_loss)] // small counts
                let extent = Vec2::new(longest as f32 * size * 0.6, lines as f32 * size * 1.3);
                (*at, at.plus(extent))
            }
            Shape::Step { at } => {
                let r = self.style.step_radius();
                (at.minus(Vec2::new(r, r)), at.plus(Vec2::new(r, r)))
            }
        };
        (
            min.minus(Vec2::new(half, half)),
            max.plus(Vec2::new(half, half)),
        )
    }
}

/// Size of the inset that magnifies the source rectangle `a`â€“`b`.
pub fn inset_size(a: Vec2, b: Vec2) -> Vec2 {
    let area = Area::from_corners(a, b);
    Vec2::new(area.width() * MAGNIFY_ZOOM, area.height() * MAGNIFY_ZOOM)
}

fn span(points: impl IntoIterator<Item = Vec2>) -> (Vec2, Vec2) {
    let mut min = Vec2::new(f32::MAX, f32::MAX);
    let mut max = Vec2::new(f32::MIN, f32::MIN);
    for p in points {
        min = Vec2::new(min.x.min(p.x), min.y.min(p.y));
        max = Vec2::new(max.x.max(p.x), max.y.max(p.y));
    }
    if min.x > max.x {
        return (Vec2::default(), Vec2::default());
    }
    (min, max)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn line(x2: f32) -> Annotation {
        Annotation {
            shape: Shape::Line {
                from: Vec2::new(0.0, 0.0),
                to: Vec2::new(x2, 10.0),
            },
            style: Style::default(),
        }
    }

    #[test]
    fn bounds_include_half_the_stroke() {
        let (min, max) = line(20.0).bounds();
        assert_eq!(min, Vec2::new(-2.0, -2.0));
        assert_eq!(max, Vec2::new(22.0, 12.0));
    }

    #[test]
    fn translate_moves_every_point() {
        let mut shape = Shape::Pen(vec![Vec2::new(1.0, 1.0), Vec2::new(2.0, 3.0)]);
        shape.translate(Vec2::new(10.0, -1.0));
        assert_eq!(
            shape,
            Shape::Pen(vec![Vec2::new(11.0, 0.0), Vec2::new(12.0, 2.0)])
        );
    }

    #[test]
    fn tiny_shapes_are_degenerate() {
        let dot = Shape::Line {
            from: Vec2::new(5.0, 5.0),
            to: Vec2::new(5.5, 6.0),
        };
        assert!(dot.is_degenerate());
        assert!(!line(20.0).shape.is_degenerate());
        let text = Shape::Text {
            at: Vec2::default(),
            text: "  ".into(),
        };
        assert!(text.is_degenerate());
    }

    #[test]
    fn magnify_moves_and_bounds_its_inset_only() {
        let mut item = Annotation {
            shape: Shape::Magnify {
                a: Vec2::new(10.0, 10.0),
                b: Vec2::new(30.0, 20.0),
                at: Vec2::new(100.0, 0.0),
            },
            style: Style::default(),
        };
        item.shape.translate(Vec2::new(5.0, 5.0));
        let Shape::Magnify { a, at, .. } = item.shape else {
            unreachable!("still a magnify item")
        };
        assert_eq!((a, at), (Vec2::new(10.0, 10.0), Vec2::new(105.0, 5.0)));
        let (min, max) = item.bounds();
        assert_eq!(min, Vec2::new(103.0, 3.0));
        assert_eq!(max, Vec2::new(147.0, 27.0));
    }

    #[test]
    fn dash_cycles() {
        assert_eq!(Dash::Solid.next().next().next(), Dash::Solid);
    }
}
