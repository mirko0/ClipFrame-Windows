//! Drawing annotations with Direct2D. Coordinates are image pixels: the caller sets the
//! view transform (on screen) or none (export), so both paths draw identically.

use cf_core::annotate::{Annotation, Dash, Document, Preview, Rgba, Shape, Vec2};
use cf_render::{Canvas, Color, LineDash, RectF, Stroke};

/// Selection and caret colour.
pub fn accent() -> Color {
    Color::rgba8(0x4F, 0xA3, 0xFF, 0xFF)
}

/// Every annotation, skipping the one the preview replaces, then the preview on top.
pub fn document(canvas: &Canvas<'_>, doc: &Document, preview: Option<&Preview>) {
    let hidden = preview.and_then(|p| p.replaces);
    for (index, item) in doc.items().iter().enumerate() {
        if Some(index) != hidden {
            annotation(canvas, item, doc.step_number(index));
        }
    }
    if let Some(preview) = preview {
        let number = hidden.map_or_else(|| doc.next_step_number(), |i| doc.step_number(i));
        annotation(canvas, &preview.item, number);
        if preview.typing {
            caret(canvas, &preview.item);
        }
    }
}

/// A dashed frame around `item` (the selection), `px` thick on screen at `zoom`.
pub fn selection_frame(canvas: &Canvas<'_>, item: &Annotation, zoom: f32) {
    let (min, max) = item.bounds();
    let pad = 4.0 / zoom;
    let rect = RectF::new(
        min.x - pad,
        min.y - pad,
        max.x - min.x + 2.0 * pad,
        max.y - min.y + 2.0 * pad,
    );
    let stroke = Stroke {
        width: 1.5 / zoom,
        color: accent(),
        dash: LineDash::Dashed,
    };
    canvas.stroke_rect(rect, &stroke);
}

/// One annotation; `step` is its number when it is a step badge.
pub fn annotation(canvas: &Canvas<'_>, item: &Annotation, step: usize) {
    let style = &item.style;
    let color = color(style.color);
    let stroke = Stroke {
        width: style.width,
        color,
        dash: dash(style.dash),
    };
    match &item.shape {
        Shape::Pen(points) => {
            let points: Vec<(f32, f32)> = points.iter().map(|p| (p.x, p.y)).collect();
            canvas.stroke_polyline(&points, &stroke);
        }
        Shape::Line { from, to } => canvas.stroke_line(xy(*from), xy(*to), &stroke),
        Shape::Arrow { from, to } => arrow(canvas, *from, *to, style.arrow_head(), &stroke),
        Shape::Rect { a, b } if style.filled => canvas.fill_rect(rect(*a, *b), color),
        Shape::Rect { a, b } => canvas.stroke_rect(rect(*a, *b), &stroke),
        Shape::Ellipse { a, b } if style.filled => canvas.fill_ellipse(rect(*a, *b), color),
        Shape::Ellipse { a, b } => canvas.stroke_ellipse(rect(*a, *b), &stroke),
        Shape::Text { at, text } => {
            if let Some(block) = canvas.text_sized(text, style.text_size(), true) {
                canvas.draw_text(&block, at.x, at.y, color);
            }
        }
        Shape::Step { at } => step_badge(canvas, *at, style.step_radius(), color, step),
    }
}

fn arrow(canvas: &Canvas<'_>, from: Vec2, to: Vec2, head: f32, stroke: &Stroke) {
    let delta = to.minus(from);
    let length = delta.length();
    if length < f32::EPSILON {
        return;
    }
    let head = head.min(length);
    let dir = Vec2::new(delta.x / length, delta.y / length);
    let normal = Vec2::new(-dir.y, dir.x);
    let base = Vec2::new(to.x - dir.x * head, to.y - dir.y * head);
    // End the shaft inside the head so its round cap doesn't poke out of the tip.
    let shaft_end = Vec2::new(to.x - dir.x * head * 0.6, to.y - dir.y * head * 0.6);
    canvas.stroke_line(xy(from), xy(shaft_end), stroke);
    let half = head * 0.55;
    let tip = [
        xy(to),
        (base.x + normal.x * half, base.y + normal.y * half),
        (base.x - normal.x * half, base.y - normal.y * half),
    ];
    canvas.fill_polygon(&tip, stroke.color);
}

fn step_badge(canvas: &Canvas<'_>, at: Vec2, radius: f32, color: Color, number: usize) {
    let circle = RectF::new(at.x - radius, at.y - radius, radius * 2.0, radius * 2.0);
    canvas.fill_ellipse(circle, color);
    let label = number.to_string();
    if let Some(block) = canvas.text_sized(&label, radius * 1.15, true) {
        let ink = if is_light(color) {
            Color::rgba8(0x10, 0x10, 0x10, 0xFF)
        } else {
            Color::rgba8(0xFF, 0xFF, 0xFF, 0xFF)
        };
        canvas.draw_text(
            &block,
            at.x - block.width / 2.0,
            at.y - block.height / 2.0,
            ink,
        );
    }
}

/// A caret after the last character of the text being typed.
fn caret(canvas: &Canvas<'_>, item: &Annotation) {
    let Shape::Text { at, text } = &item.shape else {
        return;
    };
    let size = item.style.text_size();
    let lines = text.split('\n').count().max(1);
    let last = text.rsplit('\n').next().unwrap_or("");
    let width = canvas.text_sized(last, size, true).map_or(0.0, |b| b.width);
    let line_height = canvas
        .text_sized("Ag", size, true)
        .map_or(size * 1.3, |b| b.height);
    #[allow(clippy::cast_precision_loss)] // a handful of lines
    let top = at.y + (lines - 1) as f32 * line_height;
    let x = at.x + width + 1.0;
    let stroke = Stroke {
        width: (size / 14.0).max(1.5),
        color: accent(),
        dash: LineDash::Solid,
    };
    canvas.stroke_line((x, top), (x, top + line_height), &stroke);
}

fn is_light(color: Color) -> bool {
    0.299 * color.r + 0.587 * color.g + 0.114 * color.b > 0.6
}

fn rect(a: Vec2, b: Vec2) -> RectF {
    RectF::new(
        a.x.min(b.x),
        a.y.min(b.y),
        (b.x - a.x).abs(),
        (b.y - a.y).abs(),
    )
}

const fn xy(p: Vec2) -> (f32, f32) {
    (p.x, p.y)
}

pub fn color(c: Rgba) -> Color {
    Color::rgba8(c.r, c.g, c.b, c.a)
}

const fn dash(d: Dash) -> LineDash {
    match d {
        Dash::Solid => LineDash::Solid,
        Dash::Dashed => LineDash::Dashed,
        Dash::Dotted => LineDash::Dotted,
    }
}
