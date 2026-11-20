//! Vector shapes for annotations: strokes with dash patterns, polylines, polygons,
//! ellipses, plus view transforms and clipping.

use windows::Win32::Graphics::Direct2D::Common::{
    D2D1_FIGURE_BEGIN_FILLED, D2D1_FIGURE_BEGIN_HOLLOW, D2D1_FIGURE_END_CLOSED,
    D2D1_FIGURE_END_OPEN,
};
use windows::Win32::Graphics::Direct2D::{
    D2D1_ANTIALIAS_MODE_ALIASED, D2D1_ELLIPSE, ID2D1PathGeometry, ID2D1StrokeStyle,
};
use windows_numerics::{Matrix3x2, Vector2};

use super::{Canvas, Color, RectF};

/// Stroke pattern.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum LineDash {
    /// Continuous.
    #[default]
    Solid,
    /// Dashes.
    Dashed,
    /// Dots.
    Dotted,
}

/// How a line is drawn: round caps and joins, `width` pixels in the current transform.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Stroke {
    /// Width.
    pub width: f32,
    /// Colour.
    pub color: Color,
    /// Pattern.
    pub dash: LineDash,
}

// SAFETY (every method): as for the `Canvas` impl in the parent module — the context is
// between BeginDraw and EndDraw and every argument is valid for the duration of the call.
impl Canvas<'_> {
    /// Maps drawing coordinates to pixels as `pixel = point * scale + offset`.
    pub fn set_transform(&self, scale: f32, offset: (f32, f32)) {
        let matrix = Matrix3x2 {
            M11: scale,
            M12: 0.0,
            M21: 0.0,
            M22: scale,
            M31: offset.0,
            M32: offset.1,
        };
        // SAFETY: see above.
        unsafe { self.dc.SetTransform(&raw const matrix) };
    }

    /// Back to plain pixel coordinates.
    pub fn reset_transform(&self) {
        self.set_transform(1.0, (0.0, 0.0));
    }

    /// Restricts drawing to `rect` (in current coordinates) until [`Self::pop_clip`].
    pub fn push_clip(&self, rect: RectF) {
        let rect = rect.d2d();
        // SAFETY: see above; must be balanced by `pop_clip` before EndDraw.
        unsafe {
            self.dc
                .PushAxisAlignedClip(&raw const rect, D2D1_ANTIALIAS_MODE_ALIASED);
        }
    }

    /// Ends the latest [`Self::push_clip`].
    pub fn pop_clip(&self) {
        // SAFETY: see above.
        unsafe { self.dc.PopAxisAlignedClip() };
    }

    /// A straight line.
    pub fn stroke_line(&self, from: (f32, f32), to: (f32, f32), stroke: &Stroke) {
        self.set_color(stroke.color);
        // SAFETY: see above.
        unsafe {
            self.dc.DrawLine(
                vector(from),
                vector(to),
                &self.kit.brush,
                stroke.width,
                self.style(stroke.dash),
            );
        }
    }

    /// Connected line segments through `points` (a single stroke, so translucent
    /// self-overlaps don't darken).
    pub fn stroke_polyline(&self, points: &[(f32, f32)], stroke: &Stroke) {
        let Some(path) = self.path(points, false) else {
            return;
        };
        self.set_color(stroke.color);
        // SAFETY: see above.
        unsafe {
            self.dc.DrawGeometry(
                &path,
                &self.kit.brush,
                stroke.width,
                self.style(stroke.dash),
            );
        }
    }

    /// Fills the closed polygon through `points`.
    pub fn fill_polygon(&self, points: &[(f32, f32)], color: Color) {
        let Some(path) = self.path(points, true) else {
            return;
        };
        self.set_color(color);
        // SAFETY: see above.
        unsafe { self.dc.FillGeometry(&path, &self.kit.brush, None) };
    }

    /// Outlines `rect` centred on its edges.
    pub fn stroke_rect(&self, rect: RectF, stroke: &Stroke) {
        self.set_color(stroke.color);
        let rect = rect.d2d();
        // SAFETY: see above.
        unsafe {
            self.dc.DrawRectangle(
                &raw const rect,
                &self.kit.brush,
                stroke.width,
                self.style(stroke.dash),
            );
        }
    }

    /// Outlines the ellipse inscribed in `rect`.
    pub fn stroke_ellipse(&self, rect: RectF, stroke: &Stroke) {
        self.set_color(stroke.color);
        let ellipse = ellipse(rect);
        // SAFETY: see above.
        unsafe {
            self.dc.DrawEllipse(
                &raw const ellipse,
                &self.kit.brush,
                stroke.width,
                self.style(stroke.dash),
            );
        }
    }

    /// Fills the ellipse inscribed in `rect`.
    pub fn fill_ellipse(&self, rect: RectF, color: Color) {
        self.set_color(color);
        let ellipse = ellipse(rect);
        // SAFETY: see above.
        unsafe { self.dc.FillEllipse(&raw const ellipse, &self.kit.brush) };
    }

    fn style(&self, dash: LineDash) -> &ID2D1StrokeStyle {
        match dash {
            LineDash::Solid => &self.kit.solid,
            LineDash::Dashed => &self.kit.dashed,
            LineDash::Dotted => &self.kit.dotted,
        }
    }

    fn path(&self, points: &[(f32, f32)], closed: bool) -> Option<ID2D1PathGeometry> {
        let (first, rest) = points.split_first()?;
        let rest: Vec<Vector2> = rest.iter().copied().map(vector).collect();
        let (begin, end) = if closed {
            (D2D1_FIGURE_BEGIN_FILLED, D2D1_FIGURE_END_CLOSED)
        } else {
            (D2D1_FIGURE_BEGIN_HOLLOW, D2D1_FIGURE_END_OPEN)
        };
        // SAFETY: a fresh geometry is opened, filled with one figure and closed; `rest`
        // outlives the AddLines call.
        unsafe {
            let path = self.gpu.factory.CreatePathGeometry().ok()?;
            let sink = path.Open().ok()?;
            sink.BeginFigure(vector(*first), begin);
            sink.AddLines(&rest);
            sink.EndFigure(end);
            sink.Close().ok()?;
            Some(path.into())
        }
    }
}

const fn vector(p: (f32, f32)) -> Vector2 {
    Vector2 { X: p.0, Y: p.1 }
}

fn ellipse(rect: RectF) -> D2D1_ELLIPSE {
    D2D1_ELLIPSE {
        point: Vector2 {
            X: rect.x + rect.w / 2.0,
            Y: rect.y + rect.h / 2.0,
        },
        radiusX: rect.w.abs() / 2.0,
        radiusY: rect.h.abs() / 2.0,
    }
}
