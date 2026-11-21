//! Text recognition with the OCR engine built into Windows (`Windows.Media.Ocr`): no model
//! files, no extra memory while idle, and it reads the languages installed in Windows.
//!
//! [`recognize`] blocks (typically 50–500 ms): call it on a worker thread.

use cf_core::image::Image;
use cf_core::ocr::{Line, Recognized, Word};
use windows::Graphics::Imaging::{BitmapPixelFormat, SoftwareBitmap};
use windows::Media::Ocr::{OcrEngine, OcrLine};
use windows::Storage::Streams::DataWriter;

use crate::com;

/// Why text could not be read.
#[derive(Debug, thiserror::Error)]
pub enum OcrError {
    /// No installed Windows language has an OCR engine.
    #[error("no OCR language is installed in Windows")]
    NoLanguage,
    /// A Windows Runtime call failed.
    #[error("OCR failed: {0}")]
    Windows(#[from] windows::core::Error),
}

/// Reads all text in `image`. Images larger than the engine's limit are scaled down for
/// recognition; positions are reported in `image`'s own pixels either way.
///
/// # Errors
/// [`OcrError`] when no OCR language is installed or a Windows call fails.
pub fn recognize(image: &Image) -> Result<Recognized, OcrError> {
    com::keep_mta()?;
    let engine =
        OcrEngine::TryCreateFromUserProfileLanguages().map_err(|_| OcrError::NoLanguage)?;
    let limit = OcrEngine::MaxImageDimension()?.max(1);
    let factor = image.width().max(image.height()).div_ceil(limit).max(1);
    let scaled = (factor > 1).then(|| shrink(image, factor));
    let source = scaled.as_ref().unwrap_or(image);
    let bitmap = software_bitmap(source)?;
    let result = engine.RecognizeAsync(&bitmap)?.join()?;
    let lines = result.Lines()?;
    let mut out = Recognized::default();
    for index in 0..lines.Size()? {
        let line = read_line(&lines.GetAt(index)?, factor)?;
        if !line.words.is_empty() {
            out.lines.push(line);
        }
    }
    Ok(out)
}

fn software_bitmap(image: &Image) -> windows::core::Result<SoftwareBitmap> {
    let writer = DataWriter::new()?;
    writer.WriteBytes(image.bgra())?;
    let buffer = writer.DetachBuffer()?;
    let (width, height) = (
        i32::try_from(image.width()).unwrap_or(i32::MAX),
        i32::try_from(image.height()).unwrap_or(i32::MAX),
    );
    SoftwareBitmap::CreateCopyFromBuffer(&buffer, BitmapPixelFormat::Bgra8, width, height)
}

fn read_line(line: &OcrLine, factor: u32) -> windows::core::Result<Line> {
    #[allow(clippy::cast_precision_loss)] // small scale factors
    let scale = factor as f32;
    let words = line.Words()?;
    let mut out = Line::default();
    for index in 0..words.Size()? {
        let word = words.GetAt(index)?;
        let rect = word.BoundingRect()?;
        out.words.push(Word::new(
            word.Text()?.to_string_lossy(),
            rect.X * scale,
            rect.Y * scale,
            rect.Width * scale,
            rect.Height * scale,
        ));
    }
    Ok(out)
}

/// Averages `factor × factor` blocks into single pixels.
fn shrink(image: &Image, factor: u32) -> Image {
    let (width, height) = (image.width() / factor, image.height() / factor);
    let mut pixels = Vec::with_capacity(width as usize * height as usize * 4);
    for y in 0..height {
        for x in 0..width {
            pixels.extend(block_average(image, x * factor, y * factor, factor));
        }
    }
    Image::from_bgra(width, height, pixels).unwrap_or_else(|| Image::transparent(0, 0))
}

/// Mean BGRA of the `size × size` block with its top-left at `(left, top)`.
fn block_average(image: &Image, left: u32, top: u32, size: u32) -> [u8; 4] {
    let mut sum = [0_u32; 4];
    let pixels = (0..size).flat_map(|dy| (0..size).map(move |dx| (left + dx, top + dy)));
    for (x, y) in pixels {
        let px = image.pixel(x, y).unwrap_or_default();
        sum.iter_mut().zip(px).for_each(|(s, v)| *s += u32::from(v));
    }
    sum.map(|s| u8::try_from(s / (size * size)).unwrap_or(u8::MAX))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shrink_averages_blocks() {
        let pixels = [0, 0, 0, 255, 200, 200, 200, 255].repeat(2);
        let image = Image::from_bgra(2, 2, pixels).unwrap_or_else(|| Image::transparent(0, 0));
        let small = shrink(&image, 2);
        assert_eq!((small.width(), small.height()), (1, 1));
        assert_eq!(small.pixel(0, 0), Some([100, 100, 100, 255]));
    }
}
