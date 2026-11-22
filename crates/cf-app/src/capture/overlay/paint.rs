//! Drawing one overlay window: frozen frame, dimming, selection, loupe and hints.
//! Inputs are virtual-desktop pixels; drawing is in the window's physical pixels, with UI
//! element sizes multiplied by the monitor's scale.

use cf_core::capture_mode::CaptureMode;
use cf_core::geometry::{Point, Rect};
use cf_render::{Canvas, Color, GpuImage, RectF};

/// Source pixels shown across the loupe (odd, so one pixel is centred).
pub const LOUPE_PIXELS: i32 = 15;
const LOUPE_CELL: f32 = 8.0;
const LOUPE_OFFSET: f32 = 24.0;
const HANDLE_SIZE: f32 = 7.0;
const BORDER: f32 = 2.0;

fn accent() -> Color {
    Color::rgba8(0x4F, 0xA3, 0xFF, 0xFF)
}

fn dim() -> Color {
    Color::rgba8(0, 0, 0, 120)
}

fn pill_bg() -> Color {
    Color::rgba8(18, 18, 22, 235)
}

fn white() -> Color {
    Color::rgba8(255, 255, 255, 255)
}

/// Which window is being painted.
pub struct Target<'a> {
    pub monitor: Rect,
    pub scale: f32,
    pub frozen: &'a GpuImage,
}

/// What to paint; identical for every window, each paints its own part.
#[derive(Debug, Clone)]
pub struct Scene {
    pub selection: Option<Rect>,
    pub show_handles: bool,
    pub pointer: Option<Point>,
    pub show_loupe: bool,
    /// BGRA pixel under the pointer.
    pub pointer_color: Option<[u8; 4]>,
    pub hint: String,
    pub mode: CaptureMode,
    /// The mode bar (hidden while dragging).
    pub show_bar: bool,
}

pub fn paint(canvas: &Canvas<'_>, target: &Target<'_>, scene: &Scene) {
    let (w, h) = canvas.size();
    let screen = RectF::new(0.0, 0.0, w, h);
    canvas.draw_image(target.frozen, screen, None, false);

    let hole = scene
        .selection
        .and_then(|s| s.intersect(target.monitor))
        .map(|s| to_local_rect(target.monitor, s));
    paint_dim(canvas, screen, hole);

    if let Some(selection) = scene.selection {
        paint_selection(canvas, target, selection, scene.show_handles);
    }
    if let Some(cursor) = scene.pointer.filter(|p| target.monitor.contains(*p)) {
        if scene.show_loupe {
            paint_loupe(canvas, target, cursor, scene.pointer_color);
        }
        if scene.show_bar {
            let local = to_local(target.monitor, cursor);
            super::modebar::paint(canvas, target.scale, scene.mode, Some(local), &scene.hint);
        }
    }
}

/// Darkens everything except `hole`, as up to four bands around it.
fn paint_dim(canvas: &Canvas<'_>, screen: RectF, hole: Option<RectF>) {
    let Some(hole) = hole else {
        canvas.fill_rect(screen, dim());
        return;
    };
    for band in dim_bands(screen, hole) {
        canvas.fill_rect(band, dim());
    }
}

/// The parts of `screen` outside `hole` (top, bottom, left, right), skipping empty ones.
fn dim_bands(screen: RectF, hole: RectF) -> impl Iterator<Item = RectF> {
    [
        RectF::new(0.0, 0.0, screen.w, hole.y),
        RectF::new(0.0, hole.bottom(), screen.w, screen.h - hole.bottom()),
        RectF::new(0.0, hole.y, hole.x, hole.h),
        RectF::new(hole.right(), hole.y, screen.w - hole.right(), hole.h),
    ]
    .into_iter()
    .filter(|b| b.is_positive())
}

fn paint_selection(canvas: &Canvas<'_>, target: &Target<'_>, selection: Rect, handles: bool) {
    let s = target.scale;
    let rect = to_local_rect(target.monitor, selection);
    canvas.stroke_outside(rect, (BORDER * s).round(), accent());
    if handles {
        let size = (HANDLE_SIZE * s).round();
        for (cx, cy) in handle_centers(rect) {
            let handle = RectF::new(cx - size / 2.0, cy - size / 2.0, size, size);
            canvas.fill_rect(handle, white());
            canvas.stroke_outside(handle, s.round().max(1.0), accent());
        }
    }
    if target.monitor.contains(selection.origin()) {
        let label = format!("{} × {}", selection.width, selection.height);
        let (at, anchor) = if rect.y > 30.0 * s {
            ((rect.x, rect.y - 6.0 * s), Anchor::BottomLeft)
        } else {
            ((rect.x + 6.0 * s, rect.y + 6.0 * s), Anchor::TopLeft)
        };
        pill(canvas, s, at, anchor, &label);
    }
}

fn handle_centers(r: RectF) -> [(f32, f32); 8] {
    let (cx, cy) = (r.x + r.w / 2.0, r.y + r.h / 2.0);
    [
        (r.x, r.y),
        (cx, r.y),
        (r.right(), r.y),
        (r.right(), cy),
        (r.right(), r.bottom()),
        (cx, r.bottom()),
        (r.x, r.bottom()),
        (r.x, cy),
    ]
}

/// A magnified view of the pixels around the cursor, with coordinates and colour.
fn paint_loupe(canvas: &Canvas<'_>, target: &Target<'_>, cursor: Point, color: Option<[u8; 4]>) {
    let s = target.scale;
    let cell = (LOUPE_CELL * s).round();
    #[allow(clippy::cast_precision_loss)]
    let side = LOUPE_PIXELS as f32 * cell;
    let rect = loupe_rect(canvas.size(), to_local(target.monitor, cursor), side, s);
    canvas.fill_rect(rect, Color::rgba8(0, 0, 0, 255));
    canvas.draw_image(
        target.frozen,
        rect,
        Some(loupe_source(target.monitor, cursor)),
        false,
    );
    let center = RectF::new(
        rect.x + (side - cell) / 2.0,
        rect.y + (side - cell) / 2.0,
        cell,
        cell,
    );
    canvas.stroke_outside(center, 1.0, Color::rgba8(0, 0, 0, 255));
    canvas.stroke_outside(center.inflate(1.0), 1.0, white());
    canvas.stroke_outside(rect, (1.5 * s).round(), accent());

    let info = match color {
        Some([b, g, r, _]) => format!("{}, {}   #{r:02X}{g:02X}{b:02X}", cursor.x, cursor.y),
        None => format!("{}, {}", cursor.x, cursor.y),
    };
    pill(
        canvas,
        s,
        (rect.x, rect.bottom() + 6.0 * s),
        Anchor::TopLeft,
        &info,
    );
}

/// Places the loupe below-right of the cursor, flipping when it would leave the screen.
fn loupe_rect(screen: (f32, f32), cursor: (f32, f32), side: f32, s: f32) -> RectF {
    let offset = LOUPE_OFFSET * s;
    let info_height = 32.0 * s;
    let mut x = cursor.0 + offset;
    let mut y = cursor.1 + offset;
    if x + side > screen.0 {
        x = cursor.0 - offset - side;
    }
    if y + side + info_height > screen.1 {
        y = cursor.1 - offset - side - info_height;
    }
    RectF::new(x, y, side, side)
}

/// The `LOUPE_PIXELS²` source square centred on `cursor`, in the frozen bitmap's pixels.
fn loupe_source(monitor: Rect, cursor: Point) -> RectF {
    let half = LOUPE_PIXELS / 2;
    let (x, y) = to_local(monitor, Point::new(cursor.x - half, cursor.y - half));
    #[allow(clippy::cast_precision_loss)]
    let side = LOUPE_PIXELS as f32;
    RectF::new(x, y, side, side)
}

#[derive(Debug, Clone, Copy)]
enum Anchor {
    TopLeft,
    BottomLeft,
}

/// Text on a dark rounded background, positioned by `anchor` at `at`.
fn pill(canvas: &Canvas<'_>, scale: f32, at: (f32, f32), anchor: Anchor, text: &str) {
    let Some(block) = canvas.text(text) else {
        return;
    };
    let (pad_x, pad_y) = (8.0 * scale, 4.0 * scale);
    let width = block.width + 2.0 * pad_x;
    let height = block.height + 2.0 * pad_y;
    let (left, top) = match anchor {
        Anchor::TopLeft => at,
        Anchor::BottomLeft => (at.0, at.1 - height),
    };
    let rect = RectF::new(left.round(), top.round(), width.round(), height.round());
    canvas.fill_rounded(rect, 5.0 * scale, pill_bg());
    canvas.draw_text(&block, rect.x + pad_x, rect.y + pad_y, white());
}

#[allow(clippy::cast_precision_loss)] // monitor-local pixel offsets are small
fn to_local(monitor: Rect, p: Point) -> (f32, f32) {
    ((p.x - monitor.x) as f32, (p.y - monitor.y) as f32)
}

#[allow(clippy::cast_precision_loss)]
fn to_local_rect(monitor: Rect, r: Rect) -> RectF {
    let (x, y) = to_local(monitor, r.origin());
    RectF::new(x, y, r.width as f32, r.height as f32)
}

#[cfg(test)]
mod tests {
    use super::*;

    const MONITOR: Rect = Rect::new(-1920, 0, 1920, 1080);

    #[test]
    fn local_coordinates_are_relative_to_monitor() {
        assert_eq!(
            to_local(MONITOR, Point::new(-1920 + 300, 150)),
            (300.0, 150.0)
        );
    }

    #[test]
    fn loupe_source_is_centred_on_cursor() {
        let src = loupe_source(MONITOR, Point::new(-960, 540));
        assert_eq!(src, RectF::new(953.0, 533.0, 15.0, 15.0));
    }

    #[test]
    fn loupe_flips_near_bottom_right_edge() {
        let r = loupe_rect((1920.0, 1080.0), (1900.0, 1070.0), 120.0, 1.0);
        assert!(r.right() < 1900.0 && r.bottom() < 1070.0);
        let r = loupe_rect((1920.0, 1080.0), (100.0, 100.0), 120.0, 1.0);
        assert!(r.x > 100.0 && r.y > 100.0);
    }

    #[test]
    fn dim_bands_surround_hole_without_overlap() {
        let screen = RectF::new(0.0, 0.0, 100.0, 100.0);
        let bands: Vec<_> = dim_bands(screen, RectF::new(10.0, 20.0, 30.0, 40.0)).collect();
        let area: f32 = bands.iter().map(|b| b.w * b.h).sum();
        assert_eq!(bands.len(), 4);
        assert!((area - (100.0 * 100.0 - 30.0 * 40.0)).abs() < f32::EPSILON);
    }

    #[test]
    fn dim_bands_skip_empty_sides() {
        let screen = RectF::new(0.0, 0.0, 100.0, 100.0);
        assert_eq!(
            dim_bands(screen, RectF::new(0.0, 0.0, 50.0, 100.0)).count(),
            1
        );
    }

    #[test]
    fn handles_cover_corners_and_edge_midpoints() {
        let centers = handle_centers(RectF::new(0.0, 0.0, 10.0, 20.0));
        assert!(centers.contains(&(5.0, 0.0)));
        assert!(centers.contains(&(10.0, 20.0)));
        assert!(centers.contains(&(0.0, 10.0)));
    }
}
