//! The colour menu: a small panel of swatches under the toolbar's colour button. Keys 1–8
//! pick the same colours; Esc or a click elsewhere closes it.

use cf_core::annotate::Rgba;
use cf_render::{Canvas, Color, LineDash, RectF, Stroke};

use super::command::{COLORS, Command};
use super::toolbar::{self, Button};
use super::{Editor, EditorOutcome, render};

const PAD: f32 = 8.0;
const CELL: f32 = 30.0;
const DOT: f32 = 22.0;
const COLUMNS: usize = 4;

/// Where the panel and its swatches are, in window pixels.
pub struct Placed {
    pub panel: RectF,
    pub swatches: Vec<RectF>,
}

/// Places the panel under `button`, kept inside a window `window_w` pixels wide.
pub fn place(button: RectF, window_w: f32, s: f32) -> Placed {
    let rows = COLORS.len().div_ceil(COLUMNS);
    #[allow(clippy::cast_precision_loss)] // a few cells
    let (width, height) = (
        (PAD * 2.0 + CELL * COLUMNS as f32) * s,
        (PAD * 2.0 + CELL * rows as f32) * s,
    );
    let x = button.x.clamp(0.0, (window_w - width).max(0.0));
    let y = button.bottom() + 6.0 * s;
    let swatches = (0..COLORS.len())
        .map(|i| {
            #[allow(clippy::cast_precision_loss)]
            let (col, row) = ((i % COLUMNS) as f32, (i / COLUMNS) as f32);
            RectF::new(
                x + (PAD + col * CELL) * s,
                y + (PAD + row * CELL) * s,
                CELL * s,
                CELL * s,
            )
        })
        .collect();
    Placed {
        panel: RectF::new(x, y, width, height),
        swatches,
    }
}

fn inside(r: RectF, (x, y): (f32, f32)) -> bool {
    x >= r.x && x < r.right() && y >= r.y && y < r.bottom()
}

/// Index of the swatch under `p`.
pub fn hit(placed: &Placed, p: (f32, f32)) -> Option<usize> {
    placed.swatches.iter().position(|r| inside(*r, p))
}

/// Draws the panel; `current` gets a ring, `hover` a highlight.
pub fn paint(canvas: &Canvas<'_>, placed: &Placed, current: Rgba, hover: Option<usize>, s: f32) {
    canvas.fill_rounded(placed.panel, 8.0 * s, toolbar::bar());
    for (index, (cell, color)) in placed.swatches.iter().zip(COLORS).enumerate() {
        if hover == Some(index) {
            canvas.fill_rounded(*cell, 6.0 * s, toolbar::hovered());
        }
        let (cx, cy) = (cell.x + cell.w / 2.0, cell.y + cell.h / 2.0);
        let d = DOT * s;
        let dot = RectF::new(cx - d / 2.0, cy - d / 2.0, d, d);
        canvas.fill_ellipse(dot, render::color(color));
        if (color.r, color.g, color.b) == (current.r, current.g, current.b) {
            let ring = Stroke {
                width: 2.0 * s,
                color: Color::rgba8(0xFF, 0xFF, 0xFF, 0xFF),
                dash: LineDash::Solid,
            };
            canvas.stroke_ellipse(dot.inflate(3.0 * s), &ring);
        }
    }
}

impl Editor {
    /// The open menu's placement (under the colour button).
    fn placed_colors(&self, buttons: &[Button]) -> Option<Placed> {
        if !self.colors_open {
            return None;
        }
        let button = buttons.iter().find(|b| b.command == Command::ColorMenu)?;
        Some(place(button.rect, self.window_size().0, self.scale()))
    }

    /// A press while the menu is open: picks a colour, or closes the menu. Returns whether
    /// the press was used up (a click on the image only closes the menu; a click on
    /// another toolbar button still runs it).
    pub(super) fn colors_press(&mut self, p: (f32, f32)) -> Option<EditorOutcome> {
        let buttons = self.buttons();
        let placed = self.placed_colors(&buttons)?;
        if let Some(index) = hit(&placed, p) {
            self.colors_open = false;
            let color = COLORS[index];
            let style = Command::Color(color).restyle(self.state.style())?;
            self.state.set_style(style);
            return Some(EditorOutcome::Continue);
        }
        if inside(placed.panel, p) {
            return Some(EditorOutcome::Continue);
        }
        self.colors_open = false;
        let on_button =
            toolbar::hit(&buttons, p).is_some_and(|i| buttons[i].command == Command::ColorMenu);
        (on_button || p.1 >= self.toolbar_height()).then_some(EditorOutcome::Continue)
    }

    /// Whether the pointer is over the open menu (no tooltip, no drawing cursor).
    pub(super) fn over_colors(&self, p: (f32, f32)) -> bool {
        self.placed_colors(&self.buttons())
            .is_some_and(|placed| inside(placed.panel, p))
    }

    pub(super) fn paint_colors(&self, canvas: &Canvas<'_>, buttons: &[Button]) {
        if let Some(placed) = self.placed_colors(buttons) {
            let hover = hit(&placed, self.input.pointer);
            paint(
                canvas,
                &placed,
                self.state.style().color,
                hover,
                self.scale(),
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn swatches_sit_in_the_panel_under_the_button() {
        let button = RectF::new(780.0, 8.0, 32.0, 32.0);
        let placed = place(button, 800.0, 1.0);
        assert!(placed.panel.y > button.bottom());
        assert!(placed.panel.right() <= 800.0, "kept inside the window");
        assert_eq!(placed.swatches.len(), COLORS.len());
        for (i, r) in placed.swatches.iter().enumerate() {
            assert!(inside(placed.panel, (r.x + 1.0, r.y + 1.0)));
            assert_eq!(hit(&placed, (r.x + 2.0, r.y + 2.0)), Some(i));
        }
        assert_eq!(hit(&placed, (0.0, 0.0)), None);
    }
}
