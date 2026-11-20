//! PNG encoding of captures.

use crate::image::Image;

/// Why encoding failed.
#[derive(Debug, thiserror::Error)]
#[error("PNG encoding failed")]
pub struct EncodeError(#[from] png::EncodingError);

/// Encodes `image` as PNG.
///
/// Fully opaque images (every normal screenshot) are written as RGB, which is a quarter less
/// data to compress and gives smaller files; images with transparency keep their alpha.
/// Uses fast compression: screenshots compress well anyway and latency matters more.
///
/// # Errors
/// [`EncodeError`] if the encoder fails (practically only on allocation failure).
pub fn png(image: &Image) -> Result<Vec<u8>, EncodeError> {
    let opaque = image.bgra().chunks_exact(4).all(|px| px[3] == 255);
    let (color, data) = if opaque {
        (png::ColorType::Rgb, bgra_to_rgb(image.bgra()))
    } else {
        (png::ColorType::Rgba, image.to_rgba())
    };
    let mut out = Vec::with_capacity(data.len() / 4);
    let mut encoder = png::Encoder::new(&mut out, image.width(), image.height());
    encoder.set_color(color);
    encoder.set_depth(png::BitDepth::Eight);
    encoder.set_compression(png::Compression::Fast);
    encoder.set_source_srgb(png::SrgbRenderingIntent::Perceptual);
    let mut writer = encoder.write_header()?;
    writer.write_image_data(&data)?;
    writer.finish()?;
    Ok(out)
}

fn bgra_to_rgb(bgra: &[u8]) -> Vec<u8> {
    let mut rgb = Vec::with_capacity(bgra.len() / 4 * 3);
    for px in bgra.chunks_exact(4) {
        rgb.extend_from_slice(&[px[2], px[1], px[0]]);
    }
    rgb
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]

    use super::*;

    fn decode(bytes: &[u8]) -> (png::ColorType, Vec<u8>) {
        let mut reader = png::Decoder::new(std::io::Cursor::new(bytes))
            .read_info()
            .unwrap();
        let mut buf = vec![0; reader.output_buffer_size().unwrap()];
        let info = reader.next_frame(&mut buf).unwrap();
        buf.truncate(info.buffer_size());
        (info.color_type, buf)
    }

    #[test]
    fn opaque_image_is_written_as_rgb() {
        let image = Image::from_bgra(2, 1, vec![10, 20, 30, 255, 40, 50, 60, 255]).unwrap();
        let (color, data) = decode(&png(&image).unwrap());
        assert_eq!(color, png::ColorType::Rgb);
        assert_eq!(data, vec![30, 20, 10, 60, 50, 40]);
    }

    #[test]
    fn transparent_image_keeps_alpha() {
        let image = Image::from_bgra(1, 1, vec![10, 20, 30, 0]).unwrap();
        let (color, data) = decode(&png(&image).unwrap());
        assert_eq!(color, png::ColorType::Rgba);
        assert_eq!(data, vec![30, 20, 10, 0]);
    }
}
