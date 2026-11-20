//! Safe drawing primitives over a Direct2D device context, in physical pixels.

use windows::Win32::Graphics::Direct2D::Common::{D2D_RECT_F, D2D1_COLOR_F};
use windows::Win32::Graphics::Direct2D::{
    D2D1_DRAW_TEXT_OPTIONS_NONE, D2D1_INTERPOLATION_MODE_LINEAR,
    D2D1_INTERPOLATION_MODE_NEAREST_NEIGHBOR, D2D1_ROUNDED_RECT, ID2D1DeviceContext,
};
use windows::Win32::Graphics::DirectWrite::{
    DWRITE_TEXT_METRICS, IDWriteTextFormat, IDWriteTextLayout,
};

use crate::kit::Kit;
use crate::{Gpu, GpuImage};

mod shapes;

pub use shapes::{LineDash, Stroke};

/// A straight-alpha RGBA colour.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Color {
    /// Red, 0–1.
    pub r: f32,
    /// Green, 0–1.
    pub g: f32,
    /// Blue, 0–1.
    pub b: f32,
    /// Opacity, 0–1.
    pub a: f32,
}

impl Color {
    /// From 8-bit channels.
    pub fn rgba8(r: u8, g: u8, b: u8, a: u8) -> Self {
        let unit = |v: u8| f32::from(v) / 255.0;
        Self {
            r: unit(r),
            g: unit(g),
            b: unit(b),
            a: unit(a),
        }
    }

    fn d2d(self) -> D2D1_COLOR_F {
        D2D1_COLOR_F {
            r: self.r,
            g: self.g,
            b: self.b,
            a: self.a,
        }
    }
}

/// A rectangle in pixels.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct RectF {
    /// Left.
    pub x: f32,
    /// Top.
    pub y: f32,
    /// Width.
    pub w: f32,
    /// Height.
    pub h: f32,
}

impl RectF {
    /// Creates a rectangle.
    pub const fn new(x: f32, y: f32, w: f32, h: f32) -> Self {
        Self { x, y, w, h }
    }

    /// Right edge.
    pub fn right(self) -> f32 {
        self.x + self.w
    }

    /// Bottom edge.
    pub fn bottom(self) -> f32 {
        self.y + self.h
    }

    /// The rectangle grown by `by` on every side.
    #[must_use]
    pub fn inflate(self, by: f32) -> Self {
        Self::new(
            self.x - by,
            self.y - by,
            self.w + 2.0 * by,
            self.h + 2.0 * by,
        )
    }

    /// Whether it covers any area.
    pub fn is_positive(self) -> bool {
        self.w > 0.0 && self.h > 0.0
    }

    fn d2d(self) -> D2D_RECT_F {
        D2D_RECT_F {
            left: self.x,
            top: self.y,
            right: self.right(),
            bottom: self.bottom(),
        }
    }
}

/// Laid-out text, ready to measure and draw.
#[derive(Debug)]
pub struct TextBlock {
    layout: IDWriteTextLayout,
    /// Width in pixels.
    pub width: f32,
    /// Height in pixels.
    pub height: f32,
}

/// Drawing primitives for one frame. Obtained from [`crate::WindowSurface::draw`].
#[derive(Debug)]
pub struct Canvas<'a> {
    dc: &'a ID2D1DeviceContext,
    kit: &'a Kit,
    gpu: &'a Gpu,
    size: (u32, u32),
}

// SAFETY (applies to every method below): the context is between BeginDraw and EndDraw
// (guaranteed by `WindowSurface::draw`, the only constructor caller), and every argument is a
// valid value or COM object; Direct2D reports drawing errors at EndDraw.
impl<'a> Canvas<'a> {
    pub(crate) const fn new(
        dc: &'a ID2D1DeviceContext,
        kit: &'a Kit,
        gpu: &'a Gpu,
        size: (u32, u32),
    ) -> Self {
        Self { dc, kit, gpu, size }
    }

    /// Target size in pixels.
    #[allow(clippy::cast_precision_loss)] // window sizes are far below f32's exact range
    pub fn size(&self) -> (f32, f32) {
        (self.size.0 as f32, self.size.1 as f32)
    }

    /// Fills the whole target.
    pub fn clear(&self, color: Color) {
        let color = color.d2d();
        // SAFETY: see the impl-level comment.
        unsafe { self.dc.Clear(Some(&raw const color)) };
    }

    /// Draws `src` (whole image when `None`) of `image` into `dest`.
    /// `smooth = false` uses nearest-neighbour sampling (crisp pixels, e.g. a magnifier).
    pub fn draw_image(&self, image: &GpuImage, dest: RectF, src: Option<RectF>, smooth: bool) {
        let mode = if smooth {
            D2D1_INTERPOLATION_MODE_LINEAR
        } else {
            D2D1_INTERPOLATION_MODE_NEAREST_NEIGHBOR
        };
        let dest = dest.d2d();
        let src = src.map(RectF::d2d);
        // SAFETY: see the impl-level comment; the rect pointers outlive the call.
        unsafe {
            self.dc.DrawBitmap(
                &image.bitmap,
                Some(&raw const dest),
                1.0,
                mode,
                src.as_ref().map(std::ptr::from_ref),
                None,
            );
        }
    }

    /// Fills a rectangle.
    pub fn fill_rect(&self, rect: RectF, color: Color) {
        self.set_color(color);
        let rect = rect.d2d();
        // SAFETY: see the impl-level comment.
        unsafe { self.dc.FillRectangle(&raw const rect, &self.kit.brush) };
    }

    /// Fills a rounded rectangle.
    pub fn fill_rounded(&self, rect: RectF, radius: f32, color: Color) {
        self.set_color(color);
        let rounded = D2D1_ROUNDED_RECT {
            rect: rect.d2d(),
            radiusX: radius,
            radiusY: radius,
        };
        // SAFETY: see the impl-level comment.
        unsafe {
            self.dc
                .FillRoundedRectangle(&raw const rounded, &self.kit.brush);
        }
    }

    /// Strokes a band of `width` pixels just **outside** `rect`, so integer rectangles get
    /// crisp, unblurred edges and never cover selected pixels.
    pub fn stroke_outside(&self, rect: RectF, width: f32, color: Color) {
        self.set_color(color);
        let path = rect.inflate(width / 2.0).d2d();
        // SAFETY: see the impl-level comment.
        unsafe {
            self.dc
                .DrawRectangle(&raw const path, &self.kit.brush, width, None);
        }
    }

    /// Lays out `text` in the surface's UI font.
    pub fn text(&self, text: &str) -> Option<TextBlock> {
        self.layout(text, &self.kit.ui_text)
    }

    /// Lays out `text` (line breaks allowed) at `px` pixels, bold or regular.
    pub fn text_sized(&self, text: &str, px: f32, bold: bool) -> Option<TextBlock> {
        let format = self.kit.text_format(self.gpu, px, bold).ok()?;
        self.layout(text, &format)
    }

    fn layout(&self, text: &str, format: &IDWriteTextFormat) -> Option<TextBlock> {
        let wide: Vec<u16> = text.encode_utf16().collect();
        // SAFETY: `wide` outlives the call; the format is valid.
        let layout = unsafe {
            self.gpu
                .dwrite
                .CreateTextLayout(&wide, format, f32::MAX, f32::MAX)
        }
        .ok()?;
        let mut metrics = DWRITE_TEXT_METRICS::default();
        // SAFETY: `metrics` is a valid out-pointer.
        unsafe { layout.GetMetrics(&raw mut metrics) }.ok()?;
        Some(TextBlock {
            layout,
            width: metrics.width,
            height: metrics.height,
        })
    }

    /// Draws a laid-out text block with its top-left at `(x, y)`.
    pub fn draw_text(&self, block: &TextBlock, x: f32, y: f32, color: Color) {
        self.set_color(color);
        // SAFETY: see the impl-level comment.
        unsafe {
            self.dc.DrawTextLayout(
                windows_numerics::Vector2 { X: x, Y: y },
                &block.layout,
                &self.kit.brush,
                D2D1_DRAW_TEXT_OPTIONS_NONE,
            );
        }
    }

    pub(crate) fn set_color(&self, color: Color) {
        let color = color.d2d();
        // SAFETY: setting a brush colour has no preconditions.
        unsafe { self.kit.brush.SetColor(&raw const color) };
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rgba8_maps_to_unit_range() {
        let c = Color::rgba8(255, 0, 51, 255);
        assert!((c.r - 1.0).abs() < f32::EPSILON && c.g == 0.0 && (c.b - 0.2).abs() < 1e-6);
    }

    #[test]
    fn inflate_grows_every_side() {
        assert_eq!(
            RectF::new(10.0, 10.0, 4.0, 2.0).inflate(1.0),
            RectF::new(9.0, 9.0, 6.0, 4.0)
        );
    }
}
