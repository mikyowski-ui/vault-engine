//! SPEC-014 §2.5 the F12 screenshot file: an uncompressed 8-bit Windows
//! bitmap of palette indexes, every multi-byte field little-endian.
//!
//! | Offset | Field |
//! | --- | --- |
//! | `0x0000` | `BM` |
//! | `0x0002` | file size, `width × height + 1078` |
//! | `0x0006` | 0 (two reserved words) |
//! | `0x000A` | 1078, the pixel data offset |
//! | `0x000E` | 40, the information header size |
//! | `0x0012` | width, then height (positive: rows bottom-up) |
//! | `0x001A` | 1 plane, 8 bits per pixel |
//! | `0x001E` | six 32-bit zeros |
//! | `0x0036` | 256 entries of blue, green, red, 0; each level `6-bit × 4` |
//! | `0x0436` | palette indexes, bottom row first, rows unpadded |

/// The header and palette before the pixels.
pub const PIXEL_OFFSET: usize = 1078;
const INFO_HEADER: u32 = 40;

/// The file for `width` by `height` palette indexes in `pixels` (row-major
/// from the top row) under `palette`, six-bit levels per entry. The rows are
/// stored without padding, as the reference writes them (a width that is
/// not a multiple of 4 gives a file other readers may reject; the game's
/// screen is 640 wide). Returns `None` when `pixels` is not `width × height`
/// long or the size does not fit the 32-bit fields.
pub fn write(width: u32, height: u32, pixels: &[u8], palette: &[[u8; 3]; 256]) -> Option<Vec<u8>> {
    let area = u64::from(width) * u64::from(height);
    if pixels.len() as u64 != area {
        return None;
    }
    let size = u32::try_from(area + PIXEL_OFFSET as u64).ok()?;
    let mut bytes = Vec::with_capacity(size as usize);
    bytes.extend_from_slice(b"BM");
    bytes.extend_from_slice(&size.to_le_bytes());
    bytes.extend_from_slice(&0u32.to_le_bytes());
    bytes.extend_from_slice(&(PIXEL_OFFSET as u32).to_le_bytes());
    bytes.extend_from_slice(&INFO_HEADER.to_le_bytes());
    bytes.extend_from_slice(&width.to_le_bytes());
    bytes.extend_from_slice(&height.to_le_bytes());
    bytes.extend_from_slice(&1u16.to_le_bytes());
    bytes.extend_from_slice(&8u16.to_le_bytes());
    bytes.extend_from_slice(&[0; 24]);
    for [r, g, b] in palette {
        bytes.extend_from_slice(&[b.wrapping_mul(4), g.wrapping_mul(4), r.wrapping_mul(4), 0]);
    }
    if width > 0 {
        for row in pixels.chunks_exact(width as usize).rev() {
            bytes.extend_from_slice(row);
        }
    }
    Some(bytes)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fixture_s_screenshot_header_palette_and_row_order() {
        // SPEC-014 §4.19 Fixture S: a 640-by-480 screenshot.
        let mut pixels = vec![0u8; 640 * 480];
        pixels[479 * 640] = 7; // screen (0, 479)
        pixels[0] = 9; // screen (0, 0)
        let mut palette = [[0u8; 3]; 256];
        palette[5] = [63, 32, 0];
        let file = write(640, 480, &pixels, &palette).unwrap();
        assert_eq!(file.len(), 308_278);
        let header = [
            0x42, 0x4D, 0x36, 0xB4, 0x04, 0x00, 0x00, 0x00, 0x00, 0x00, 0x36, 0x04, 0x00, 0x00,
            0x28, 0x00, 0x00, 0x00, 0x80, 0x02, 0x00, 0x00, 0xE0, 0x01, 0x00, 0x00, 0x01, 0x00,
            0x08, 0x00,
        ];
        assert_eq!(file[..30], header);
        assert_eq!(file[30..54], [0; 24]);
        // Palette entry for 6-bit levels (63, 32, 0): `00 80 FC 00`.
        assert_eq!(file[0x36 + 4 * 5..0x36 + 4 * 6], [0x00, 0x80, 0xFC, 0x00]);
        // Screen (0, 479) is the first pixel; (0, 0) starts the last row.
        assert_eq!(file[0x0436], 7);
        assert_eq!(0x0436 + 479 * 640, 0x4B1B6);
        assert_eq!(file[0x4B1B6], 9);
        // The pixel count must match the size.
        assert_eq!(write(640, 480, &pixels[1..], &palette), None);
    }
}
