//! Procedurally drawn tray icon (four corner brackets around a dot).
//! Placeholder until real artwork lands with the installer (M7).

/// Icon width and height in pixels.
pub const SIZE: usize = 32;
const INSET: usize = 2;
const STROKE: usize = 4;
const ARM: usize = 12;
const DOT_START: usize = 13;
const COLOR: [u8; 4] = [0x4F, 0xA3, 0xFF, 0xFF];

/// RGBA8 pixels, row-major, `SIZE * SIZE * 4` bytes.
pub fn rgba() -> Vec<u8> {
    let mut pixels = vec![0u8; SIZE * SIZE * 4];
    for (index, pixel) in pixels.chunks_exact_mut(4).enumerate() {
        if is_lit(index % SIZE, index / SIZE) {
            pixel.copy_from_slice(&COLOR);
        }
    }
    pixels
}

fn is_lit(x: usize, y: usize) -> bool {
    // Distance to the nearest edge makes the shape symmetric in all four corners.
    let dx = x.min(SIZE - 1 - x);
    let dy = y.min(SIZE - 1 - y);
    let in_stroke = |d: usize| (INSET..INSET + STROKE).contains(&d);
    let in_arm = |d: usize| (INSET..INSET + ARM).contains(&d);
    let bracket = (in_stroke(dx) && in_arm(dy)) || (in_stroke(dy) && in_arm(dx));
    let dot = dx >= DOT_START && dy >= DOT_START;
    bracket || dot
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn has_expected_byte_length() {
        assert_eq!(rgba().len(), SIZE * SIZE * 4);
    }

    #[test]
    fn is_symmetric() {
        for y in 0..SIZE {
            for x in 0..SIZE {
                assert_eq!(is_lit(x, y), is_lit(SIZE - 1 - x, y));
                assert_eq!(is_lit(x, y), is_lit(x, SIZE - 1 - y));
            }
        }
    }

    #[test]
    fn corners_lit_and_border_transparent() {
        assert!(is_lit(INSET, INSET));
        assert!(!is_lit(0, 0));
        assert!(is_lit(SIZE / 2, SIZE / 2));
    }
}
