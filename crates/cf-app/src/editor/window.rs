//! Creating and sizing the editor window.

use cf_core::geometry::Rect;
use winit::dpi::{PhysicalPosition, PhysicalSize};
use winit::event_loop::ActiveEventLoop;
use winit::window::Window;

use super::toolbar;

/// Space around the image inside the window, at 100 % scaling.
pub const MARGIN: f32 = 16.0;
/// Fraction of the work area the window may cover at most.
const MAX_SHARE: f32 = 0.9;

/// Creates the (hidden) editor window on the monitor around `near`, sized to show an image
/// of `image` pixels at 1:1 when it fits.
pub fn create(
    event_loop: &ActiveEventLoop,
    image: (u32, u32),
    near: Rect,
) -> anyhow::Result<Window> {
    let attributes = Window::default_attributes()
        .with_title("ClipFrame")
        .with_visible(false);
    let window = event_loop.create_window(attributes)?;
    let work_area = work_area_near(near);
    let scale = scale_of(&window);
    let size = window_size(image, scale, work_area);
    let _ = window.request_inner_size(size);
    #[allow(clippy::cast_possible_truncation, clippy::cast_possible_wrap)] // screen-sized
    let position = PhysicalPosition::new(
        work_area.x + (work_area.width - size.width as i32) / 2,
        work_area.y + (work_area.height - size.height as i32) / 2,
    );
    window.set_outer_position(position);
    let narrowest = toolbar::narrowest(scale);
    let tallest = toolbar::height(&toolbar::layout(scale, narrowest), scale);
    window.set_min_inner_size(Some(PhysicalSize::new(
        to_px(narrowest),
        to_px(tallest + 120.0 * scale),
    )));
    Ok(window)
}

/// The UI scale of `window`'s monitor (1.0 at 100 %).
pub fn scale_of(window: &Window) -> f32 {
    #[allow(clippy::cast_possible_truncation)] // scale factors are small (1.0–4.0)
    let scale = window.scale_factor() as f32;
    scale
}

fn window_size(image: (u32, u32), scale: f32, work_area: Rect) -> PhysicalSize<u32> {
    let chrome = 2.0 * MARGIN * scale;
    #[allow(clippy::cast_precision_loss)] // image and screen sizes
    let (max_w, max_h) = (
        work_area.width as f32 * MAX_SHARE,
        work_area.height as f32 * MAX_SHARE,
    );
    // Room for the toolbar on one row when the screen allows, else it wraps.
    let floor = toolbar::full_width(scale)
        .min(max_w)
        .max(toolbar::narrowest(scale));
    #[allow(clippy::cast_precision_loss)]
    let width = (image.0 as f32 + chrome).max(floor).min(max_w.max(floor));
    let bar = toolbar::height(&toolbar::layout(scale, width), scale);
    #[allow(clippy::cast_precision_loss)]
    let height = (image.1 as f32 + chrome + bar)
        .max(300.0 * scale)
        .min(max_h);
    PhysicalSize::new(to_px(width), to_px(height))
}

fn work_area_near(near: Rect) -> Rect {
    let center = cf_core::geometry::Point::new(near.x + near.width / 2, near.y + near.height / 2);
    let monitors = cf_platform::monitors::enumerate().unwrap_or_default();
    monitors
        .iter()
        .find(|m| m.rect.contains(center))
        .or_else(|| monitors.iter().find(|m| m.is_primary))
        .map_or(Rect::new(0, 0, 1280, 800), |m| m.work_area)
}

#[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)] // positive, screen-sized
fn to_px(v: f32) -> u32 {
    v.round().max(1.0) as u32
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn small_images_get_room_for_the_toolbar_and_large_ones_are_capped() {
        let work = Rect::new(0, 0, 2000, 1000);
        let small = window_size((100, 50), 1.0, work);
        assert!(small.width >= to_px(toolbar::full_width(1.0)));
        assert_eq!(small.height, 300);
        let large = window_size((5000, 5000), 1.0, work);
        assert_eq!((large.width, large.height), (1800, 900));
    }
}
