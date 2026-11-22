//! CPU-side images and stitching of per-monitor captures.

use crate::geometry::{Point, Rect};

/// A BGRA8 image (top-down rows, tightly packed: stride = `width * 4`).
///
/// BGRA is what Windows capture APIs and `CF_DIBV5` use natively, so no channel swizzle is
/// needed between capture, GPU upload (`Bgra8Unorm`) and the clipboard.
#[derive(Clone, PartialEq, Eq)]
pub struct Image {
    width: u32,
    height: u32,
    pixels: Vec<u8>,
}

impl std::fmt::Debug for Image {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Image({}x{})", self.width, self.height)
    }
}

impl Image {
    /// Wraps existing BGRA pixels; `None` when the buffer length doesn't match.
    pub fn from_bgra(width: u32, height: u32, pixels: Vec<u8>) -> Option<Self> {
        (pixels.len() == byte_len(width, height)).then_some(Self {
            width,
            height,
            pixels,
        })
    }

    /// A fully transparent image.
    pub fn transparent(width: u32, height: u32) -> Self {
        Self {
            width,
            height,
            pixels: vec![0; byte_len(width, height)],
        }
    }

    /// Width in pixels.
    pub const fn width(&self) -> u32 {
        self.width
    }

    /// Height in pixels.
    pub const fn height(&self) -> u32 {
        self.height
    }

    /// Raw BGRA bytes.
    pub fn bgra(&self) -> &[u8] {
        &self.pixels
    }

    /// Bytes per row.
    pub const fn stride(&self) -> usize {
        self.width as usize * 4
    }

    /// The BGRA value at `(x, y)`, or `None` outside the image.
    pub fn pixel(&self, x: u32, y: u32) -> Option<[u8; 4]> {
        if x >= self.width || y >= self.height {
            return None;
        }
        let i = y as usize * self.stride() + x as usize * 4;
        self.pixels.get(i..i + 4).and_then(|p| p.try_into().ok())
    }

    /// The pixels converted to RGBA (for encoders that need it).
    pub fn to_rgba(&self) -> Vec<u8> {
        let mut out = self.pixels.clone();
        for px in out.as_chunks_mut::<4>().0 {
            px.swap(0, 2);
        }
        out
    }

    /// A copy scaled down (never up) to fit `max_w` × `max_h`, each output pixel the
    /// average of the source pixels it covers. For previews, so it favours speed.
    #[must_use]
    pub fn thumbnail(&self, max_w: u32, max_h: u32) -> Self {
        let (w, h) = fit(self.width, self.height, max_w.max(1), max_h.max(1));
        let mut pixels = Vec::with_capacity(byte_len(w, h));
        for ty in 0..h {
            let (y0, y1) = span(ty, h, self.height);
            for tx in 0..w {
                let (x0, x1) = span(tx, w, self.width);
                pixels.extend_from_slice(&self.average(x0..x1, y0..y1));
            }
        }
        Self {
            width: w,
            height: h,
            pixels,
        }
    }

    fn average(&self, xs: std::ops::Range<u32>, ys: std::ops::Range<u32>) -> [u8; 4] {
        let mut sum = [0u64; 4];
        let mut count = 0u64;
        // Sample at most 4 × 4 pixels per cell: plenty for a preview.
        let step = |r: &std::ops::Range<u32>| ((r.end - r.start) / 4).max(1) as usize;
        let samples = ys
            .clone()
            .step_by(step(&ys))
            .flat_map(|y| xs.clone().step_by(step(&xs)).map(move |x| (x, y)));
        for px in samples.filter_map(|(x, y)| self.pixel(x, y)) {
            for (s, v) in sum.iter_mut().zip(px) {
                *s += u64::from(v);
            }
            count += 1;
        }
        sum.map(|s| u8::try_from(s / count.max(1)).unwrap_or(u8::MAX))
    }
}

/// `(w, h)` scaled down to fit `max_w` × `max_h`, keeping the aspect ratio (at least 1 × 1).
fn fit(w: u32, h: u32, max_w: u32, max_h: u32) -> (u32, u32) {
    if w <= max_w && h <= max_h {
        return (w.max(1), h.max(1));
    }
    let (w64, h64) = (u64::from(w), u64::from(h));
    let (mw, mh) = (u64::from(max_w), u64::from(max_h));
    let (out_w, out_h) = if w64 * mh > h64 * mw {
        (mw, (h64 * mw / w64.max(1)).max(1))
    } else {
        ((w64 * mh / h64.max(1)).max(1), mh)
    };
    (
        u32::try_from(out_w).unwrap_or(max_w),
        u32::try_from(out_h).unwrap_or(max_h),
    )
}

/// The source range covered by output cell `i` of `out` cells across `len` source pixels.
fn span(i: u32, out: u32, len: u32) -> (u32, u32) {
    let scale = |v: u32| {
        u32::try_from(u64::from(v) * u64::from(len) / u64::from(out.max(1))).unwrap_or(len)
    };
    let start = scale(i);
    (start, scale(i + 1).max(start + 1).min(len.max(1)))
}

fn byte_len(width: u32, height: u32) -> usize {
    width as usize * height as usize * 4
}

/// One monitor's frozen capture and where it sits on the virtual desktop.
#[derive(Debug, Clone)]
pub struct Frame {
    /// Monitor bounds in virtual-desktop pixels.
    pub rect: Rect,
    /// Captured pixels; same size as `rect`.
    pub image: Image,
}

impl Frame {
    /// The BGRA pixel at virtual-desktop point `p`, if this frame covers it.
    pub fn pixel_at(&self, p: Point) -> Option<[u8; 4]> {
        if !self.rect.contains(p) {
            return None;
        }
        let x = u32::try_from(p.x - self.rect.x).ok()?;
        let y = u32::try_from(p.y - self.rect.y).ok()?;
        self.image.pixel(x, y)
    }
}

/// Copies `region` out of the frames, stitching across monitors.
///
/// Areas no monitor covers (gaps between differently sized monitors) stay transparent.
/// Returns `None` for an empty region.
pub fn crop(frames: &[Frame], region: Rect) -> Option<Image> {
    if region.is_empty() {
        return None;
    }
    let mut out = Image::transparent(dim(region.width), dim(region.height));
    for frame in frames {
        if let Some(overlap) = frame.rect.intersect(region) {
            copy_rows(frame, region, overlap, &mut out);
        }
    }
    Some(out)
}

fn copy_rows(frame: &Frame, region: Rect, overlap: Rect, out: &mut Image) {
    let row_bytes = idx(overlap.width) * 4;
    let src_stride = frame.image.stride();
    let dst_stride = out.stride();
    for y in overlap.y..overlap.bottom() {
        let src = idx(y - frame.rect.y) * src_stride + idx(overlap.x - frame.rect.x) * 4;
        let dst = idx(y - region.y) * dst_stride + idx(overlap.x - region.x) * 4;
        out.pixels[dst..dst + row_bytes].copy_from_slice(&frame.image.pixels[src..src + row_bytes]);
    }
}

/// Non-negative `i32` → `u32` (geometry guarantees non-negative sizes/offsets here).
fn dim(v: i32) -> u32 {
    u32::try_from(v).unwrap_or(0)
}

fn idx(v: i32) -> usize {
    usize::try_from(v).unwrap_or(0)
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]

    use super::*;

    /// A frame whose every pixel is `[b, g, r, 255]` with `b` = x and `g` = y (mod 256).
    fn gradient_frame(rect: Rect) -> Frame {
        let mut pixels = Vec::new();
        for y in 0..rect.height {
            for x in 0..rect.width {
                pixels.extend_from_slice(&[x.to_le_bytes()[0], y.to_le_bytes()[0], 0, 255]);
            }
        }
        Frame {
            rect,
            image: Image::from_bgra(dim(rect.width), dim(rect.height), pixels).unwrap(),
        }
    }

    #[test]
    fn from_bgra_rejects_wrong_length() {
        assert!(Image::from_bgra(2, 2, vec![0; 15]).is_none());
    }

    #[test]
    fn crop_inside_one_frame() {
        let frame = gradient_frame(Rect::new(0, 0, 50, 50));
        let out = crop(&[frame], Rect::new(10, 20, 5, 5)).unwrap();
        assert_eq!((out.width(), out.height()), (5, 5));
        assert_eq!(out.pixel(0, 0), Some([10, 20, 0, 255]));
        assert_eq!(out.pixel(4, 4), Some([14, 24, 0, 255]));
    }

    #[test]
    fn crop_stitches_across_monitors_and_leaves_gaps_transparent() {
        let left = gradient_frame(Rect::new(-10, 0, 10, 10));
        let right = gradient_frame(Rect::new(0, 0, 10, 5));
        let out = crop(&[left, right], Rect::new(-2, 0, 4, 8)).unwrap();
        assert_eq!(out.pixel(0, 0), Some([8, 0, 0, 255])); // from left monitor, x = 8
        assert_eq!(out.pixel(2, 0), Some([0, 0, 0, 255])); // from right monitor, x = 0
        assert_eq!(out.pixel(3, 7), Some([0, 0, 0, 0])); // below the shorter right monitor
    }

    #[test]
    fn crop_of_empty_region_is_none() {
        let frame = gradient_frame(Rect::new(0, 0, 4, 4));
        assert!(crop(&[frame], Rect::new(1, 1, 0, 3)).is_none());
    }

    #[test]
    fn frame_pixel_at_uses_virtual_coordinates() {
        let frame = gradient_frame(Rect::new(-10, 0, 10, 10));
        assert_eq!(frame.pixel_at(Point::new(-7, 3)), Some([3, 3, 0, 255]));
        assert_eq!(frame.pixel_at(Point::new(0, 0)), None);
    }

    #[test]
    fn to_rgba_swaps_red_and_blue() {
        let image = Image::from_bgra(1, 1, vec![1, 2, 3, 4]).unwrap();
        assert_eq!(image.to_rgba(), vec![3, 2, 1, 4]);
    }

    #[test]
    fn thumbnails_fit_the_box_and_average_colours() {
        let mut pixels = Vec::new();
        for _ in 0..(100 * 50) {
            pixels.extend_from_slice(&[10, 20, 30, 255]);
        }
        let image = Image::from_bgra(100, 50, pixels).unwrap();
        let thumb = image.thumbnail(20, 20);
        assert_eq!((thumb.width(), thumb.height()), (20, 10));
        assert_eq!(thumb.pixel(5, 5), Some([10, 20, 30, 255]));
        let small = image.thumbnail(500, 500);
        assert_eq!(
            (small.width(), small.height()),
            (100, 50),
            "never scaled up"
        );
    }
}
