//! Toolbar glyphs, drawn with the same primitives as the annotations (window pixels).

use cf_core::annotate::{Annotation, Dash, Rgba, Shape, Style, Tool, Vec2};
use cf_render::{Canvas, Color, RectF, Stroke};

use super::render;

/// Icon ink.
pub fn ink() -> Color {
    Color::rgba8(0xE6, 0xE6, 0xE6, 0xFF)
}

/// Draws the icon of `tool` centred on `(cx, cy)`, `half` pixels from centre to edge.
pub fn tool(canvas: &Canvas<'_>, tool: Tool, (cx, cy): (f32, f32), half: f32, line: &Stroke) {
    let pt = |dx: f32, dy: f32| (cx + dx * half, cy + dy * half);
    match tool {
        Tool::Select => {
            let pointer = [pt(-0.6, -0.9), pt(0.7, 0.3), pt(0.05, 0.35), pt(-0.3, 0.95)];
            canvas.fill_polygon(&pointer, line.color);
        }
        Tool::Text => label(canvas, "T", (cx, cy)),
        Tool::Step => {
            let badge = RectF::new(cx - half, cy - half, half * 2.0, half * 2.0);
            canvas.stroke_ellipse(badge, line);
            label(canvas, "1", (cx, cy));
        }
        Tool::Highlighter => {
            let marker = Stroke {
                width: half * 0.9,
                color: Color::rgba8(0xFD, 0xD8, 0x35, 0xFF),
                ..*line
            };
            canvas.stroke_line(pt(-0.7, 0.5), pt(0.7, -0.5), &marker);
        }
        Tool::Blur => blur(canvas, (cx, cy), half),
        Tool::Redact => pixels(canvas, (cx, cy), half),
        Tool::Spotlight => spotlight(canvas, (cx, cy), half),
        Tool::Magnify => {
            let lens = RectF::new(cx - half * 0.9, cy - half * 0.9, half * 1.3, half * 1.3);
            canvas.stroke_ellipse(lens, line);
            canvas.stroke_line(pt(0.3, 0.3), pt(0.9, 0.9), line);
        }
        Tool::Crop => {
            canvas.stroke_polyline(&[pt(-0.45, -0.95), pt(-0.45, 0.45), pt(0.95, 0.45)], line);
            canvas.stroke_polyline(&[pt(-0.95, -0.45), pt(0.45, -0.45), pt(0.45, 0.95)], line);
        }
        Tool::Pen | Tool::Line | Tool::Arrow | Tool::Rect | Tool::Ellipse => {
            let at = |dx: f32, dy: f32| Vec2::new(cx + dx * half, cy + dy * half);
            let style = Style {
                color: Rgba::new(0xE6, 0xE6, 0xE6, 0xFF),
                width: line.width,
                dash: Dash::Solid,
                filled: false,
            };
            let shape = sample(tool, at);
            render::annotation(canvas, &Annotation { shape, style }, 1);
        }
    }
}

/// A small sample of a drawing tool's shape, with points placed by `at`.
fn sample(tool: Tool, at: impl Fn(f32, f32) -> Vec2) -> Shape {
    match tool {
        Tool::Pen => Shape::Pen(vec![
            at(-0.9, 0.5),
            at(-0.3, -0.5),
            at(0.3, 0.5),
            at(0.9, -0.5),
        ]),
        Tool::Line => Shape::Line {
            from: at(-0.9, 0.9),
            to: at(0.9, -0.9),
        },
        Tool::Arrow => Shape::Arrow {
            from: at(-0.9, 0.9),
            to: at(0.9, -0.9),
        },
        Tool::Ellipse => Shape::Ellipse {
            a: at(-0.9, -0.7),
            b: at(0.9, 0.7),
        },
        _ => Shape::Rect {
            a: at(-0.9, -0.7),
            b: at(0.9, 0.7),
        },
    }
}

/// Soft dot: rings fading outwards.
fn blur(canvas: &Canvas<'_>, (cx, cy): (f32, f32), half: f32) {
    for (scale, alpha) in [(1.0, 0.25), (0.7, 0.5), (0.4, 1.0)] {
        let r = half * scale;
        let circle = RectF::new(cx - r, cy - r, r * 2.0, r * 2.0);
        canvas.fill_ellipse(circle, Color { a: alpha, ..ink() });
    }
}

/// A 3 × 3 checkerboard of pixels.
fn pixels(canvas: &Canvas<'_>, (cx, cy): (f32, f32), half: f32) {
    let cell = half * 2.0 / 3.0;
    for row in 0..3_u8 {
        for col in 0..3_u8 {
            let alpha = if (row + col) % 2 == 0 { 1.0 } else { 0.35 };
            let (x, y) = (
                cx - half + f32::from(col) * cell,
                cy - half + f32::from(row) * cell,
            );
            canvas.fill_rect(RectF::new(x, y, cell, cell), Color { a: alpha, ..ink() });
        }
    }
}

/// A dimmed square with a bright hole.
fn spotlight(canvas: &Canvas<'_>, (cx, cy): (f32, f32), half: f32) {
    let frame = RectF::new(cx - half, cy - half * 0.8, half * 2.0, half * 1.6);
    let hole = RectF::new(cx - half * 0.45, cy - half * 0.35, half * 0.9, half * 0.7);
    canvas.fill_outside(frame, &[hole], half * 0.15, Color { a: 0.6, ..ink() });
}

/// A symbol drawn larger than UI text.
pub fn glyph(canvas: &Canvas<'_>, text: &str, (cx, cy): (f32, f32), s: f32) {
    if let Some(block) = canvas.text_sized(text, 20.0 * s, true) {
        let (x, y) = (cx - block.width / 2.0, cy - block.height / 2.0);
        canvas.draw_text(&block, x, y, ink());
    }
}

/// UI text centred on `(cx, cy)`.
pub fn label(canvas: &Canvas<'_>, text: &str, (cx, cy): (f32, f32)) {
    if let Some(block) = canvas.text(text) {
        canvas.draw_text(
            &block,
            cx - block.width / 2.0,
            cy - block.height / 2.0,
            ink(),
        );
    }
}
