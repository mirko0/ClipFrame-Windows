//! The controls of a settings row: toggle, choice and text/shortcut/folder box.

use cf_render::{Canvas, Color, LineDash, RectF, Stroke};

use super::model::{Kind, Value};
use super::view::{RowLook, accent, centered, field, field_hover, ink, muted};

pub fn toggle(canvas: &Canvas<'_>, control: RectF, on: bool, s: f32) {
    let (w, h) = (46.0 * s, 24.0 * s);
    let pill = RectF::new(control.right() - w, control.y + (control.h - h) / 2.0, w, h);
    canvas.fill_rounded(pill, h / 2.0, if on { accent() } else { field_hover() });
    let knob = h - 8.0 * s;
    let x = if on {
        pill.right() - 4.0 * s - knob
    } else {
        pill.x + 4.0 * s
    };
    canvas.fill_ellipse(RectF::new(x, pill.y + 4.0 * s, knob, knob), ink());
    let text = if on { "On" } else { "Off" };
    if let Some(block) = canvas.text_sized(text, 13.0 * s, false) {
        canvas.draw_text(
            &block,
            pill.x - block.width - 10.0 * s,
            pill.y + (h - block.height) / 2.0,
            muted(),
        );
    }
}

pub fn choice(canvas: &Canvas<'_>, control: RectF, options: &[&str], look: &RowLook<'_>, s: f32) {
    canvas.fill_rounded(
        control,
        6.0 * s,
        if look.hovered { field_hover() } else { field() },
    );
    let index = match look.value {
        Value::Choice(i) => *i,
        _ => 0,
    };
    let text = options.get(index).copied().unwrap_or("");
    centered(canvas, text, control, 14.0 * s, ink());
    for (glyph, x) in [
        ("‹", control.x + 16.0 * s),
        ("›", control.right() - 16.0 * s),
    ] {
        if let Some(block) = canvas.text_sized(glyph, 20.0 * s, true) {
            let y = control.y + (control.h - block.height) / 2.0 - 1.0 * s;
            canvas.draw_text(&block, x - block.width / 2.0, y, muted());
        }
    }
}

pub fn boxed(canvas: &Canvas<'_>, control: RectF, look: &RowLook<'_>, s: f32) {
    let editing = look.editing.is_some();
    canvas.fill_rounded(
        control,
        6.0 * s,
        if look.hovered || editing {
            field_hover()
        } else {
            field()
        },
    );
    if editing {
        let frame = Stroke {
            width: 1.5 * s,
            color: accent(),
            dash: LineDash::Solid,
        };
        canvas.stroke_rect(control.inflate(-0.75 * s), &frame);
    }
    let inner = control.inflate(-12.0 * s);
    let (text, color) = display_text(look);
    let Some(block) = canvas.text_sized(&text, 14.0 * s, false) else {
        return;
    };
    let mut width = inner.w;
    if let Kind::Folder = look.kind
        && let Some(change) = canvas.text_sized("Change…", 13.0 * s, true)
    {
        width -= change.width + 12.0 * s;
        let y = control.y + (control.h - change.height) / 2.0;
        canvas.draw_text(&change, inner.right() - change.width, y, accent());
    }
    let y = control.y + (control.h - block.height) / 2.0;
    canvas.push_clip(RectF::new(inner.x, control.y, width, control.h));
    // Long text shows its end (the part being typed, or the folder's own name).
    let x = if block.width > width {
        inner.x + width - block.width
    } else {
        inner.x
    };
    canvas.draw_text(&block, x, y, color);
    if let (Some(_), Kind::Text) = (look.editing, look.kind) {
        let caret = (x + block.width + 1.0 * s).min(inner.x + width - 1.0 * s);
        let stroke = Stroke {
            width: 1.5 * s,
            color: accent(),
            dash: LineDash::Solid,
        };
        canvas.stroke_line((caret, y), (caret, y + block.height), &stroke);
    }
    canvas.pop_clip();
}

fn display_text(look: &RowLook<'_>) -> (String, Color) {
    match (look.kind, look.editing, look.value) {
        (Kind::Hotkey { optional }, Some(_), _) => {
            let hint = if optional {
                "Press a shortcut… (Backspace clears)"
            } else {
                "Press a shortcut… (Esc cancels)"
            };
            (hint.to_owned(), accent())
        }
        (Kind::Text, Some(text), _) => (text.to_owned(), ink()),
        (Kind::Hotkey { .. }, None, Value::Hotkey(Some(key))) => (key.to_string(), ink()),
        (Kind::Hotkey { .. }, None, Value::Hotkey(None)) => ("Not set".to_owned(), muted()),
        (_, _, Value::Text(text)) => (text.clone(), ink()),
        _ => (String::new(), ink()),
    }
}
