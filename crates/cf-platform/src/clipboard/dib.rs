//! Serialising an image as a `CF_DIBV5` clipboard payload (`BITMAPV5HEADER` + pixels).
//!
//! Built byte-by-byte instead of transmuting the Win32 struct, so it needs no `unsafe` and is
//! testable on its own. Windows synthesises `CF_DIB` / `CF_BITMAP` from it for older apps.

use cf_core::image::Image;

const HEADER_SIZE: u32 = 124;
const BI_BITFIELDS: u32 = 3;
const LCS_SRGB: u32 = 0x7352_4742; // 'sRGB'
const LCS_GM_IMAGES: u32 = 4;
const MASKS: [u32; 4] = [0x00FF_0000, 0x0000_FF00, 0x0000_00FF, 0xFF00_0000]; // R, G, B, A

/// The full `CF_DIBV5` payload: header followed by bottom-up BGRA rows.
pub fn dibv5_bytes(image: &Image) -> Vec<u8> {
    let image_size = u32::try_from(image.bgra().len()).unwrap_or(u32::MAX);
    let mut out = Vec::with_capacity(HEADER_SIZE as usize + image.bgra().len());
    write_header(&mut out, image, image_size);
    // Positive height in the header means bottom-up rows, the layout every reader supports.
    for row in image.bgra().chunks_exact(image.stride().max(1)).rev() {
        out.extend_from_slice(row);
    }
    out
}

fn write_header(out: &mut Vec<u8>, image: &Image, image_size: u32) {
    let u32le = |out: &mut Vec<u8>, v: u32| out.extend_from_slice(&v.to_le_bytes());
    let width = i32::try_from(image.width()).unwrap_or(i32::MAX);
    let height = i32::try_from(image.height()).unwrap_or(i32::MAX);

    u32le(out, HEADER_SIZE); // bV5Size
    out.extend_from_slice(&width.to_le_bytes()); // bV5Width
    out.extend_from_slice(&height.to_le_bytes()); // bV5Height
    out.extend_from_slice(&1u16.to_le_bytes()); // bV5Planes
    out.extend_from_slice(&32u16.to_le_bytes()); // bV5BitCount
    u32le(out, BI_BITFIELDS); // bV5Compression
    u32le(out, image_size); // bV5SizeImage
    out.extend_from_slice(&[0; 16]); // X/Y pels per meter, ClrUsed, ClrImportant
    for mask in MASKS {
        u32le(out, mask);
    }
    u32le(out, LCS_SRGB); // bV5CSType
    out.extend_from_slice(&[0; 36 + 12]); // bV5Endpoints, gamma R/G/B
    u32le(out, LCS_GM_IMAGES); // bV5Intent
    out.extend_from_slice(&[0; 12]); // ProfileData, ProfileSize, Reserved
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]

    use super::*;

    fn read_u32(bytes: &[u8], at: usize) -> u32 {
        u32::from_le_bytes(bytes[at..at + 4].try_into().unwrap())
    }

    #[test]
    fn header_is_124_bytes_with_correct_fields() {
        let image = Image::transparent(3, 2);
        let bytes = dibv5_bytes(&image);
        assert_eq!(bytes.len(), 124 + 3 * 2 * 4);
        assert_eq!(read_u32(&bytes, 0), 124);
        assert_eq!(read_u32(&bytes, 4), 3);
        assert_eq!(read_u32(&bytes, 8), 2);
        assert_eq!(read_u32(&bytes, 16), BI_BITFIELDS);
        assert_eq!(read_u32(&bytes, 52), 0xFF00_0000);
        assert_eq!(read_u32(&bytes, 56), LCS_SRGB);
        assert_eq!(read_u32(&bytes, 108), LCS_GM_IMAGES);
    }

    #[test]
    fn rows_are_stored_bottom_up() {
        let top = [1, 1, 1, 255];
        let bottom = [2, 2, 2, 255];
        let image = Image::from_bgra(1, 2, [top, bottom].concat()).unwrap();
        let bytes = dibv5_bytes(&image);
        assert_eq!(&bytes[124..128], &bottom);
        assert_eq!(&bytes[128..132], &top);
    }
}
