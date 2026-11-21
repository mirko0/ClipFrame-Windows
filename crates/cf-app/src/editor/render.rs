//! Drawing annotations with Direct2D. Coordinates are image pixels: the caller sets the
//! view transform (on screen) or none (export), so both paths draw identically.
//!
//! Layers, bottom to top: the image, region effects (blur, pixelate, redact), the single
//! spotlight dim layer, then every other annotation (magnify insets stay bright).

use cf_core::annotate::{Annotation, Area, Dash, Document, Preview, Rgba, Shape, Vec2, inset_size};
use cf_render::{Canvas, Color, GpuImage, LineDash, RectF, Stroke};

/// Rounding of spotlight holes, in image pixels.
const SPOTLIGHT_RADIUS: f32 = 6.0;

/// Selection and caret colour.
pub fn accent() -> Color {
    Color::rgba8(0x4F, 0xA3, 0xFF, 0xFF)
}

/// What image effects draw from.
pub struct Backdrop<'a> {
    /// The captured image.
    pub base: &'a GpuImage,
    /// Opacity of the spotlight dim layer.
    pub dim: f32,
}

/// Every annotation, skipping the one the preview replaces, then the preview on top.
pub fn document(
    canvas: &Canvas<'_>,
    backdrop: &Backdrop<'_>,
    doc: &Document,
    preview: Option<&Preview>,
) {
    let hidden = preview.and_then(|p| p.replaces);
    let mut items: Vec<(&Annotation, usize)> = doc
        .items()
        .iter()
        .enumerate()
        .filter(|(index, _)| Some(*index) != hidden)
        .map(|(index, item)| (item, doc.step_number(index)))
        .collect();
    if let Some(preview) = preview {
        let number = hidden.map_or_else(|| doc.next_step_number(), |i| doc.step_number(i));
        items.push((&preview.item, number));
    }
    for (item, _) in &items {
        region_effect(canvas, backdrop.base, item);
    }
    spotlight(canvas, backdrop, &items);
    for (item, step) in &items {
        if let Shape::Magnify { .. } = item.shape {
            magnify(canvas, backdrop.base, item);
        } else {
            annotation(canvas, item, *step);
        }
    }
    if let Some(preview) = preview.filter(|p| p.typing) {
        caret(canvas, &preview.item);
    }
}

/// While cropping: dims everything outside `crop` and frames it.
pub fn crop_overlay(canvas: &Canvas<'_>, image: RectF, crop: Area, zoom: f32) {
    let area = rect(crop.min, crop.max);
    canvas.fill_outside(image, &[area], 0.0, Color::rgba8(0, 0, 0, 0x99));
    let stroke = Stroke {
        width: 1.5 / zoom,
        color: accent(),
        dash: LineDash::Dashed,
    };
    canvas.stroke_rect(area, &stroke);
}

/// Blur, pixelate or solid redaction. Pixelation that fails to render falls back to a
/// solid box, so a redaction never shows the original pixels.
fn region_effect(canvas: &Canvas<'_>, base: &GpuImage, item: &Annotation) {
    let style = &item.style;
    let (area, drawn) = match item.shape {
        Shape::Blur { a, b } => {
            let area = rect(a, b);
            (area, canvas.draw_blurred(base, area, style.blur_sigma()))
        }
        Shape::Redact { a, b } if !style.filled => {
            let area = rect(a, b);
            (area, canvas.draw_pixelated(base, area, style.pixel_block()))
        }
        Shape::Redact { a, b } => (rect(a, b), false),
        _ => return,
    };
    if !drawn {
        canvas.fill_rect(area, Color::rgba8(0x10, 0x10, 0x10, 0xFF));
    }
}

/// The single dim layer with every spotlight as a hole.
fn spotlight(canvas: &Canvas<'_>, backdrop: &Backdrop<'_>, items: &[(&Annotation, usize)]) {
    let holes: Vec<RectF> = items
        .iter()
        .filter_map(|(item, _)| match item.shape {
            Shape::Spotlight { a, b } => Some(rect(a, b)),
            _ => None,
        })
        .collect();
    if holes.is_empty() {
        return;
    }
    let image = image_rect(backdrop.base);
    let dim = Color::rgba8(0, 0, 0, 0xFF);
    canvas.fill_outside(
        image,
        &holes,
        SPOTLIGHT_RADIUS,
        Color {
            a: backdrop.dim,
            ..dim
        },
    );
}

/// A magnified copy of the source area, framed, plus a thin frame around the source.
fn magnify(canvas: &Canvas<'_>, base: &GpuImage, item: &Annotation) {
    let Shape::Magnify { a, b, at } = item.shape else {
        return;
    };
    let image = image_rect(base);
    let source = Area::from_corners(a, b).clamp_to(Vec2::new(image.w, image.h));
    let size = inset_size(source.min, source.max);
    let inset = RectF::new(at.x, at.y, size.x, size.y);
    let source = rect(source.min, source.max);
    if !source.is_positive() {
        return;
    }
    canvas.draw_image(base, inset, Some(source), false);
    let style = &item.style;
    let frame = Stroke {
        width: style.width,
        color: color(style.color),
        dash: dash(style.dash),
    };
    canvas.stroke_rect(inset, &frame);
    let thin = Stroke {
        width: (style.width / 2.0).max(1.5),
        ..frame
    };
    canvas.stroke_rect(source, &thin);
}

#[allow(clippy::cast_precision_loss)] // image sizes
fn image_rect(base: &GpuImage) -> RectF {
    RectF::new(0.0, 0.0, base.width() as f32, base.height() as f32)
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
        Shape::Text { at, text, size } => {
            if let Some(block) = canvas.text_sized(text, *size, true) {
                canvas.draw_text(&block, at.x, at.y, color);
            }
        }
        Shape::Step { at } => step_badge(canvas, *at, style.step_radius(), color, step),
        Shape::Highlight(points) => {
            let points: Vec<(f32, f32)> = points.iter().map(|p| (p.x, p.y)).collect();
            canvas.stroke_highlight(&points, style.highlight_width(), color);
        }
        // Drawn in their own layers (they need the image).
        Shape::Blur { .. }
        | Shape::Redact { .. }
        | Shape::Spotlight { .. }
        | Shape::Magnify { .. } => {}
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
    let Shape::Text { at, text, size } = &item.shape else {
        return;
    };
    let size = *size;
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

/// Measured frame of text `item` (image pixels); empty text gets a narrow frame one line high.
pub fn text_frame(canvas: &Canvas<'_>, item: &Annotation) -> Option<RectF> {
    let Shape::Text { at, text, size } = &item.shape else {
        return None;
    };
    let block = canvas.text_sized(if text.is_empty() { "M" } else { text }, *size, true)?;
    let width = if text.is_empty() { 0.0 } else { block.width };
    Some(RectF::new(at.x, at.y, width.max(size * 0.6), block.height))
}
