//! SPEC-014 §3.46 floating-text boxes: the wrapped lines centered in a box
//! that starts as transparent index 0, glyphs blended over index 0
//! (SPEC-002 §3.2), and the two-pass outline. The map scene draws the boxes
//! as the layer after the outlines (§3.44).
use crate::{ImageError, IndexedImage, TextStyle};
use vault_formats::{Font, Palette};

/// One text's box, ready to compose.
#[derive(Debug, Clone, Copy)]
pub struct FloatingBox<'a> {
    pub lines: &'a [Vec<u8>],
    /// W by H, the outline's 2 pixels included.
    pub size: [i32; 2],
    /// The text color and drawing flags.
    pub style: TextStyle,
    /// The outline palette index, or none.
    pub outline: Option<u8>,
}

/// A composed box at its hex: drawn at the hex's window position (the
/// top-left of its 32-by-16 cell) plus `offset`.
#[derive(Debug, Clone)]
pub struct FloatingSprite {
    pub hex: i32,
    pub offset: [i32; 2],
    pub image: IndexedImage,
}

/// Compose a box: line k at x = `trunc((W - w) / 2)` and y = `k × (line
/// height + 1)`, plus 1 with an outline; then the outline passes.
pub fn compose(
    font: &Font,
    palette: &Palette,
    text: FloatingBox,
) -> Result<IndexedImage, ImageError> {
    let [width, height] = text.size;
    let mut image = IndexedImage::new(width.max(1) as u32, height.max(1) as u32)?;
    let inset = i32::from(text.outline.is_some());
    let pitch = font.line_height() as i32 + 1;
    for (index, line) in text.lines.iter().enumerate() {
        let w = font.text_width(line, false) as i32;
        let origin = [(width - w) / 2, index as i32 * pitch + inset];
        image.draw_text(font, palette, line, origin, width.max(0) as u32, text.style);
    }
    if let Some(color) = text.outline {
        outline(&mut image, color);
    }
    Ok(image)
}

/// The two outline passes in color `c`. Rows 1 to H - 2 are scanned left
/// to right from "outside": a nonzero pixel while outside paints the pixel
/// to its left and goes inside, a zero pixel while inside is painted and
/// goes outside. Then every column, rows 0 to H - 1, top to bottom, the
/// same way with the pixel above; the row pass's paint counts as nonzero,
/// so the outline covers the diagonal corners. Index 0 paints transparent
/// pixels.
pub fn outline(image: &mut IndexedImage, color: u8) {
    let (width, height) = (image.width() as usize, image.height() as usize);
    let pixels = image.pixels_mut();
    for y in 1..height.saturating_sub(1) {
        let mut inside = false;
        for x in 0..width {
            let at = y * width + x;
            if !inside && pixels[at] != 0 {
                if x > 0 {
                    pixels[at - 1] = color;
                }
                inside = true;
            } else if inside && pixels[at] == 0 {
                pixels[at] = color;
                inside = false;
            }
        }
    }
    for x in 0..width {
        let mut inside = false;
        for y in 0..height {
            let at = y * width + x;
            if !inside && pixels[at] != 0 {
                if y > 0 {
                    pixels[at - width] = color;
                }
                inside = true;
            } else if inside && pixels[at] == 0 {
                pixels[at] = color;
                inside = false;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fixture_assets as fixtures;

    /// A synthetic AAF: height 10, spacing 1, space width 3; every glyph 6
    /// by 10 at coverage 7 (advance 7, 4 for a space; pitch 11).
    fn font() -> Font {
        let mut bytes = b"AAFF".to_vec();
        for metric in [10_i16, 1, 3, 0] {
            bytes.extend(metric.to_be_bytes());
        }
        for _ in 0..256 {
            bytes.extend(6_i16.to_be_bytes());
            bytes.extend(10_i16.to_be_bytes());
            bytes.extend(0_i32.to_be_bytes());
        }
        bytes.extend([7; 60]);
        Font::parse_aaf(&bytes).unwrap()
    }

    #[test]
    fn spec014_fixture_u_outline_rings_a_lone_glyph_pixel() {
        // Box 5 by 3 whose only glyph pixel is (2, 1).
        let mut image = IndexedImage::new(5, 3).unwrap();
        image.pixels_mut()[5 + 2] = 9;
        outline(&mut image, 4);
        #[rustfmt::skip]
        let ring = [
            0, 4, 4, 4, 0,
            0, 4, 9, 4, 0,
            0, 4, 4, 4, 0,
        ];
        assert_eq!(image.pixels(), ring);
        // Outline index 0 paints transparent pixels: no visible outline.
        let mut image = IndexedImage::new(5, 3).unwrap();
        image.pixels_mut()[5 + 2] = 9;
        outline(&mut image, 0);
        assert_eq!(image.pixels().iter().filter(|p| **p != 0).count(), 1);
    }

    #[test]
    fn spec014_fixture_u_boxes_center_lines_over_index_zero() {
        let palette = Palette::parse(&fixtures::grayscale_palette()).unwrap();
        let font = font();
        let lines = [b"ab".to_vec(), b"c".to_vec()];
        let style = TextStyle {
            foreground: 28,
            ..Default::default()
        };
        // W = 14 + 2, H = 2 x 11 + 2: line 0 (w 14) at (1, 1), line 1
        // (w 7) at (4, 12).
        let image = compose(
            &font,
            &palette,
            FloatingBox {
                lines: &lines,
                size: [16, 24],
                style,
                outline: Some(5),
            },
        )
        .unwrap();
        let at = |x: usize, y: usize| image.pixels()[y * 16 + x];
        let full = palette.blend_coverage(7, 28, 0).unwrap();
        assert_eq!((at(1, 1), at(6, 10), at(8, 1)), (full, full, full));
        assert_eq!((at(0, 1), at(4, 12), at(3, 12)), (5, full, 5));
        // The spacing column between the glyphs is outlined, and so is the
        // corner above the row pass's paint; pixels clear of the text stay
        // transparent.
        assert_eq!((at(7, 1), at(0, 0), at(15, 23)), (5, 5, 0));
        // Without an outline the lines start at the top edge.
        let image = compose(
            &font,
            &palette,
            FloatingBox {
                lines: &lines[..1],
                size: [14, 11],
                style,
                outline: None,
            },
        )
        .unwrap();
        assert_eq!(image.pixels()[0], full);
        assert_eq!(image.pixels()[6], 0, "the spacing column");
    }
}
