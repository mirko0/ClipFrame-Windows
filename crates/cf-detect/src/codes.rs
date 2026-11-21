//! QR codes and barcodes in a capture.

use cf_core::annotate::{Area, Vec2};
use cf_core::image::Image;
use rxing::helpers::detect_multiple_in_luma;

/// Margin added around a code's detected corner points, in pixels.
const PAD: f32 = 4.0;

/// One decoded code.
#[derive(Debug, Clone, PartialEq)]
pub struct Code {
    /// Symbology, e.g. `qrcode` or `code 128`.
    pub format: String,
    /// The decoded content.
    pub text: String,
    /// Where it is in the image, when known.
    pub area: Option<Area>,
}

/// Every distinct code in `image` (empty when none is found). Can take a while on large
/// images: run it on a worker thread.
pub fn scan(image: &Image) -> Vec<Code> {
    if image.width() == 0 || image.height() == 0 {
        return Vec::new();
    }
    let Ok(results) = detect_multiple_in_luma(luma(image), image.width(), image.height()) else {
        return Vec::new();
    };
    let mut codes: Vec<Code> = Vec::new();
    for result in results {
        let text = result.getText().to_owned();
        if text.is_empty() || codes.iter().any(|c| c.text == text) {
            continue;
        }
        let corners = result.getPoints().iter().map(|p| Vec2::new(p.x, p.y));
        codes.push(Code {
            format: result.getBarcodeFormat().to_string(),
            text,
            area: bounds(corners),
        });
    }
    codes
}

/// Grey levels (Rec. 601 weights) of a BGRA image.
fn luma(image: &Image) -> Vec<u8> {
    image
        .bgra()
        .as_chunks::<4>()
        .0
        .iter()
        .map(|px| {
            let (b, g, r) = (u32::from(px[0]), u32::from(px[1]), u32::from(px[2]));
            u8::try_from((r * 299 + g * 587 + b * 114) / 1000).unwrap_or(u8::MAX)
        })
        .collect()
}

fn bounds(points: impl Iterator<Item = Vec2>) -> Option<Area> {
    let mut points = points.peekable();
    let first = *points.peek()?;
    let area = points.fold(Area::from_corners(first, first), |area, p| Area {
        min: Vec2::new(area.min.x.min(p.x), area.min.y.min(p.y)),
        max: Vec2::new(area.max.x.max(p.x), area.max.y.max(p.y)),
    });
    Some(Area {
        min: Vec2::new(area.min.x - PAD, area.min.y - PAD),
        max: Vec2::new(area.max.x + PAD, area.max.y + PAD),
    })
}

#[cfg(test)]
mod tests {
    use rxing::{BarcodeFormat, MultiFormatWriter, Writer};

    use super::*;

    /// A white image with a QR code of `text` drawn at `(left, top)`, 4 pixels per module.
    fn image_with_qr(text: &str, left: u32, top: u32) -> Image {
        let matrix = MultiFormatWriter
            .encode(text, &BarcodeFormat::QR_CODE, 0, 0)
            .expect("encodable");
        let (w, h) = (400, 300);
        let mut pixels = vec![255_u8; (w * h * 4) as usize];
        let (cols, rows) = (matrix.width() * 4, matrix.height() * 4);
        let dark = (0..rows)
            .flat_map(|y| (0..cols).map(move |x| (x, y)))
            .filter(|&(x, y)| matrix.get(x / 4, y / 4));
        for (x, y) in dark {
            let i = (((top + y) * w + left + x) * 4) as usize;
            pixels[i..i + 3].fill(0);
        }
        Image::from_bgra(w, h, pixels).expect("sized")
    }

    #[test]
    fn finds_a_qr_code_and_where_it_is() {
        let codes = scan(&image_with_qr("https://example.com/a?b=1", 120, 60));
        assert_eq!(codes.len(), 1);
        assert_eq!(codes[0].text, "https://example.com/a?b=1");
        assert!(codes[0].format.to_lowercase().contains("qr"));
        let area = codes[0].area.expect("located");
        assert!(area.min.x > 100.0 && area.min.y > 40.0 && area.max.x < 300.0);
    }

    #[test]
    fn a_blank_image_has_no_codes() {
        let blank = Image::from_bgra(64, 64, vec![255; 64 * 64 * 4]).expect("sized");
        assert_eq!(scan(&blank), []);
        assert_eq!(scan(&Image::transparent(0, 0)), []);
    }
}
