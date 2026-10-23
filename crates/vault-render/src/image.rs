//! Bounded indexed composition for sprite previews and byte-code bitmap text.
use vault_formats::{Font, FontKind, FrmArchive, Palette, frm::FrmFrame};

#[cfg(test)]
use crate::fixture_assets as fixtures;

#[derive(Debug, thiserror::Error)]
pub enum ImageError {
    #[error("image dimensions must be 1..=4096 and match the pixel buffer")]
    Dimensions,
    #[error("sprite direction or frame is out of range")]
    Frame,
}

#[derive(Debug, Clone)]
pub struct IndexedImage {
    pub(crate) width: u32,
    pub(crate) height: u32,
    pub(crate) pixels: Vec<u8>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TextStyle {
    pub foreground: u8,
    pub monospaced: bool,
    pub underline: bool,
    pub shadow: bool,
}

impl Default for TextStyle {
    fn default() -> Self {
        Self {
            foreground: 255,
            monospaced: false,
            underline: false,
            shadow: false,
        }
    }
}

/// SPEC-002 numeric drawing word: low byte color, shadow/underline/monospaced bits.
impl From<vault_formats::TextFlags> for TextStyle {
    fn from(flags: vault_formats::TextFlags) -> Self {
        Self {
            foreground: flags.foreground,
            monospaced: flags.monospaced,
            underline: flags.underline,
            shadow: flags.shadow,
        }
    }
}

impl IndexedImage {
    pub fn new(width: u32, height: u32) -> Result<Self, ImageError> {
        if width == 0 || height == 0 || width > 4096 || height > 4096 {
            return Err(ImageError::Dimensions);
        }
        Ok(Self {
            width,
            height,
            pixels: vec![0; width as usize * height as usize],
        })
    }
    pub fn from_pixels(width: u32, height: u32, pixels: Vec<u8>) -> Result<Self, ImageError> {
        if width == 0
            || height == 0
            || width > 4096
            || height > 4096
            || width as usize * height as usize != pixels.len()
        {
            return Err(ImageError::Dimensions);
        }
        Ok(Self {
            width,
            height,
            pixels,
        })
    }
    pub fn width(&self) -> u32 {
        self.width
    }
    pub fn height(&self) -> u32 {
        self.height
    }
    pub fn pixels(&self) -> &[u8] {
        &self.pixels
    }
    pub fn pixels_mut(&mut self) -> &mut [u8] {
        &mut self.pixels
    }
    pub fn fill_rect(&mut self, rect: [i32; 4], color: u8) {
        let [x, y, width, height] = rect;
        let start = i64::from(x).max(0);
        let end = (i64::from(x) + i64::from(width)).clamp(0, i64::from(self.width));
        if start >= end {
            return;
        }
        let stride = self.width as usize;
        for row in
            i64::from(y).max(0)..(i64::from(y) + i64::from(height)).clamp(0, i64::from(self.height))
        {
            let row = row as usize * stride;
            self.pixels[row + start as usize..row + end as usize].fill(color);
        }
    }
    pub fn copy_image(&mut self, source: &IndexedImage, origin: [i32; 2]) {
        self.copy_rows(&source.pixels, [source.width, source.height], origin);
    }
    /// An opaque copy of a frame (index 0 included), clipped.
    pub fn copy_frame(&mut self, frame: &FrmFrame, origin: [i32; 2]) {
        self.copy_rows(
            frame.pixels(),
            [u32::from(frame.width), u32::from(frame.height)],
            origin,
        );
    }
    /// Copy a `width` by `height` pixel block row by row, clipped.
    fn copy_rows(&mut self, pixels: &[u8], [width, height]: [u32; 2], origin: [i32; 2]) {
        let [left, top] = origin.map(i64::from);
        let start = left.max(0);
        let end = (left + i64::from(width)).min(i64::from(self.width));
        if start >= end {
            return;
        }
        let stride = self.width as usize;
        for y in top.max(0)..(top + i64::from(height)).min(i64::from(self.height)) {
            let source = ((y - top) * i64::from(width) + start - left) as usize;
            let row = y as usize * stride;
            self.pixels[row + start as usize..row + end as usize]
                .copy_from_slice(&pixels[source..source + (end - start) as usize]);
        }
    }
    fn pixel_mut(&mut self, x: i64, y: i64) -> Option<&mut u8> {
        if x < 0 || y < 0 || x >= i64::from(self.width) || y >= i64::from(self.height) {
            return None;
        }
        self.pixels
            .get_mut(y as usize * self.width as usize + x as usize)
    }

    /// Unscaled palette-keyed blit, clipped before iteration. Map projection
    /// supplies the image corner; tile footprints never rescale the art.
    pub fn blit(&mut self, frame: &FrmFrame, origin: [i64; 2]) {
        self.blit_within(
            frame,
            origin,
            [0, 0, i64::from(self.width), i64::from(self.height)],
        );
    }
    /// A transparent blit clipped to `clip` (`[x, y, width, height]`) as
    /// well as to the image.
    pub fn blit_within(&mut self, frame: &FrmFrame, origin: [i64; 2], clip: [i64; 4]) {
        let [left, top] = origin;
        let width = i64::from(frame.width);
        let height = i64::from(frame.height);
        let start = left.max(clip[0]).max(0);
        let end = (left + width)
            .min(clip[0] + clip[2])
            .min(i64::from(self.width));
        let first = top.max(clip[1]).max(0);
        let last = (top + height)
            .min(clip[1] + clip[3])
            .min(i64::from(self.height));
        if start >= end {
            return;
        }
        let stride = self.width as usize;
        for y in first..last {
            let source = ((y - top) * width + start - left) as usize;
            let source = &frame.pixels()[source..source + (end - start) as usize];
            let row = y as usize * stride;
            let target = &mut self.pixels[row + start as usize..row + end as usize];
            for (pixel, color) in target.iter_mut().zip(source) {
                if *color != 0 {
                    *pixel = *color;
                }
            }
        }
    }

    /// UI rectangle centering: no world shifts or accumulated animation movement.
    /// Nearest-neighbor fit, with at most 4x magnification.
    pub fn draw_frame(&mut self, frame: &FrmFrame, origin: [u32; 2], size: [u32; 2]) {
        if size.contains(&0) || frame.width == 0 || frame.height == 0 {
            return;
        }
        let fw = u64::from(frame.width);
        let fh = u64::from(frame.height);
        let scale = (u64::from(size[0]) * 1024 / fw)
            .min(u64::from(size[1]) * 1024 / fh)
            .min(4096);
        let width = (fw * scale / 1024).max(1);
        let height = (fh * scale / 1024).max(1);
        let left = i64::from(origin[0]) + (i64::from(size[0]) - width as i64) / 2;
        let top = i64::from(origin[1]) + (i64::from(size[1]) - height as i64) / 2;
        // Clip loop bounds as well as writes, even for caller-supplied huge boxes.
        let start_x = (-left).max(0) as u64;
        let start_y = (-top).max(0) as u64;
        let end_x = width.min((i64::from(self.width) - left).max(0) as u64);
        let end_y = height.min((i64::from(self.height) - top).max(0) as u64);
        for y in start_y..end_y {
            for x in start_x..end_x {
                let index = frame.pixels()[(y * fh / height * fw + x * fw / width) as usize];
                if index != 0
                    && let Some(pixel) = self.pixel_mut(left + x as i64, top + y as i64)
                {
                    *pixel = index;
                }
            }
        }
    }

    pub fn draw_text(
        &mut self,
        font: &Font,
        palette: &Palette,
        text: &[u8],
        origin: [i32; 2],
        limit: u32,
        style: TextStyle,
    ) -> u32 {
        if style.shadow {
            let preserve_flags = font.kind() == FontKind::Antialiased;
            self.draw_text(
                font,
                palette,
                text,
                [origin[0].saturating_add(1), origin[1].saturating_add(1)],
                limit,
                TextStyle {
                    foreground: palette.lookup_rgb5([0, 0, 0]).expect("five-bit color"),
                    monospaced: preserve_flags && style.monospaced,
                    underline: preserve_flags && style.underline,
                    shadow: false,
                },
            );
        }
        let layout = font.layout(text, limit, style.monospaced);
        for positioned in &layout.glyphs {
            let glyph = font
                .glyph(positioned.code)
                .expect("layout only contains supported codes");
            for y in 0..glyph.height {
                let dy = i64::from(origin[1]) + i64::from(positioned.y + y);
                if dy < 0 || dy >= i64::from(self.height) {
                    continue;
                }
                for x in 0..glyph.width {
                    let dx = i64::from(origin[0]) + positioned.x + i64::from(x);
                    let Some(destination) = self.pixel_mut(dx, dy) else {
                        continue;
                    };
                    let coverage = font
                        .coverage(positioned.code, x, y)
                        .expect("validated glyph extent");
                    if font.kind() == FontKind::Bitmap {
                        if coverage != 0 {
                            *destination = style.foreground;
                        }
                    } else {
                        *destination = palette
                            .blend_coverage(coverage, style.foreground, *destination)
                            .expect("validated coverage");
                    }
                }
            }
        }
        if style.underline {
            let y = i64::from(origin[1]) + i64::from(font.height()) - 1;
            let start = i64::from(origin[0]).max(0);
            let end = (i64::from(origin[0]) + i64::from(layout.width)).min(i64::from(self.width));
            for x in start..end {
                if let Some(pixel) = self.pixel_mut(x, y) {
                    *pixel = style.foreground;
                }
            }
        }
        layout.width
    }
}

pub fn compose_preview(
    archive: &FrmArchive,
    direction: usize,
    frame: usize,
    palette: &Palette,
    font: &Font,
    message: &[u8],
) -> Result<IndexedImage, ImageError> {
    let frame = archive.frame(direction, frame).ok_or(ImageError::Frame)?;
    let mut image = IndexedImage::new(640, 480)?;
    let text_height = font
        .line_height()
        .saturating_mul(2)
        .saturating_add(32)
        .min(240);
    image.draw_frame(frame, [16, 16], [608, 448 - text_height]);
    let foreground = (1_u16..256)
        .map(|index| index as u8)
        .max_by_key(|index| palette.rgb6(*index).into_iter().map(u16::from).sum::<u16>())
        .unwrap_or(1);
    let style = TextStyle {
        foreground,
        ..Default::default()
    };
    let top = 480 - text_height as i32;
    image.draw_text(font, palette, b"AB abc 0123", [16, top], 608, style);
    image.draw_text(
        font,
        palette,
        message,
        [16, top + font.line_height() as i32 + 4],
        608,
        style,
    );
    Ok(image)
}

#[cfg(test)]
mod tests {
    use super::*;
    use vault_formats::frm::FrameCursor;

    #[test]
    fn spec002_numeric_flag_word_selects_drawing_options() {
        let style = TextStyle::from(vault_formats::TextFlags::from_word(0x0006_0109));
        assert_eq!(
            style,
            TextStyle {
                foreground: 9,
                monospaced: true,
                underline: true,
                shadow: false,
            }
        );
        // Drawing with the decoded word matches drawing with explicit options.
        let palette = Palette::parse(&fixtures::palette()).unwrap();
        let font = Font::parse_fon(&fixtures::fon()).unwrap();
        let draw = |style: TextStyle| {
            let mut image = IndexedImage::new(40, 20).unwrap();
            image.draw_text(&font, &palette, b"AB", [1, 1], 38, style);
            image
        };
        assert_eq!(
            draw(style).pixels(),
            draw(TextStyle {
                foreground: 9,
                monospaced: true,
                underline: true,
                shadow: false,
            })
            .pixels()
        );
    }

    #[test]
    fn spec001_fixtures_b_and_c_draw_through_world_rectangles() {
        let at = |image: &IndexedImage, x: usize, y: usize| image.pixels()[y * 200 + x];
        let canvas = || IndexedImage::from_pixels(200, 200, vec![9; 40000]).unwrap();
        // Fixture B: index 0 leaves the destination unchanged; index 1 draws.
        let archive = FrmArchive::parse(&fixtures::shared_frm()).unwrap();
        let frame = archive.frame(0, 0).unwrap();
        let rect = frame.world_rect([100, 100], archive.shifts[0], [0, 0]);
        let mut image = canvas();
        image.blit(frame, [rect.left, rect.top]);
        assert_eq!((at(&image, 101, 99), at(&image, 102, 99)), (9, 1));
        assert_eq!(
            image.pixels().iter().filter(|pixel| **pixel != 9).count(),
            1
        );
        // Fixture C: after entering frame 1, the top pixel is transparent and
        // the lower pixel is index 3.
        let archive = FrmArchive::parse(&fixtures::animated_frm()).unwrap();
        let mut cursor = FrameCursor::default();
        assert!(archive.advance(&mut cursor, 0) && archive.advance(&mut cursor, 0));
        let frame = archive.frame(0, 1).unwrap();
        let rect = frame.world_rect([100, 100], archive.shifts[0], cursor.displacement);
        let mut image = canvas();
        image.blit(frame, [rect.left, rect.top]);
        assert_eq!((at(&image, 103, 99), at(&image, 103, 100)), (9, 3));
        // Directions 2 and 3 share records but not header shifts.
        for (direction, [x, y]) in [(2, [98, 103]), (3, [100, 100])] {
            for (index, color) in [(0, 4), (1, 5)] {
                let frame = archive.frame(direction, index).unwrap();
                let rect = frame.world_rect([100, 100], archive.shifts[direction], [0, 0]);
                let mut image = canvas();
                image.blit(frame, [rect.left, rect.top]);
                assert_eq!(at(&image, x, y), color);
            }
        }
    }
    #[test]
    fn aaf_composites_spec002_grayscale_and_underlines_the_height_band() {
        let palette = Palette::parse(&fixtures::grayscale_palette()).unwrap();
        let font = Font::parse_aaf(&fixtures::aaf()).unwrap();
        let mut image = IndexedImage::new(3, 4).unwrap();
        image.draw_text(
            &font,
            &palette,
            b"A",
            [0, 0],
            4,
            TextStyle {
                foreground: 28,
                ..Default::default()
            },
        );
        assert_eq!(image.pixels(), &[0, 0, 0, 0, 4, 28, 27, 12, 0, 0, 0, 0]);
        image.draw_text(
            &font,
            &palette,
            b"A",
            [0, 0],
            4,
            TextStyle {
                foreground: 28,
                underline: true,
                ..Default::default()
            },
        );
        assert_eq!(&image.pixels()[6..9], &[28; 3]);
        assert_eq!(&image.pixels()[9..12], &[0; 3]);
    }
    /// SPEC-002 §3.2 destination sampling: each glyph pixel reads the index
    /// already at the pixel it writes, with no snapshot, so a later string
    /// blends over an earlier one, and the shadow pass is drawn first at
    /// (1, 1).
    #[test]
    fn spec002_aaf_blends_read_the_destination_being_written() {
        let palette = Palette::parse(&fixtures::grayscale_palette()).unwrap();
        let font = Font::parse_aaf(&fixtures::aaf()).unwrap();
        let style = TextStyle {
            foreground: 28,
            ..Default::default()
        };
        let blend = |coverage: u8, destination: u8| {
            palette.blend_coverage(coverage, 28, destination).unwrap()
        };
        let mut image = IndexedImage::new(5, 5).unwrap();
        image.draw_text(&font, &palette, b"A", [0, 0], 4, style);
        image.draw_text(&font, &palette, b"A", [0, 0], 4, style);
        // Coverage 1 at (1, 1) and 3 at (1, 2) blend over the first pass.
        assert_eq!(image.pixels()[5 + 1], blend(1, blend(1, 0)));
        assert_eq!(image.pixels()[10 + 1], blend(3, blend(3, 0)));
        assert_eq!(image.pixels()[5 + 2], 28);
        // With a shadow: the black pass at (1, 1) first, then the main pass
        // over it. (2, 2) and (3, 2) hold only shadow pixels; (0, 2) the
        // main coverage 8 over the background.
        let black = palette.lookup_rgb5([0, 0, 0]).unwrap();
        let mut image = IndexedImage::from_pixels(5, 5, vec![20; 25]).unwrap();
        image.draw_text(
            &font,
            &palette,
            b"A",
            [0, 0],
            4,
            TextStyle {
                shadow: true,
                ..style
            },
        );
        let shadow = |coverage: u8| palette.blend_coverage(coverage, black, 20).unwrap();
        assert_eq!(image.pixels()[10 + 2], shadow(1));
        assert_eq!(image.pixels()[10 + 3], shadow(7));
        assert_eq!(image.pixels()[10], blend(8, 20));
        // (1, 2): the main coverage 3 over the shadow's untouched pixel.
        assert_eq!(image.pixels()[10 + 1], blend(3, 20));
    }

    #[test]
    fn fon_drawing_copies_foreground_directly_and_clips_safely() {
        let palette = Palette::parse(&fixtures::palette()).unwrap();
        let font = Font::parse_fon(&fixtures::fon()).unwrap();
        let mut image = IndexedImage::new(3, 2).unwrap();
        image.draw_text(
            &font,
            &palette,
            b"B",
            [0, 0],
            4,
            TextStyle {
                foreground: 2,
                ..Default::default()
            },
        );
        assert_eq!(image.pixels(), &[2, 0, 2, 0, 2, 0]);
        image.draw_text(
            &font,
            &palette,
            b"A",
            [-100, -100],
            100,
            TextStyle {
                shadow: true,
                ..Default::default()
            },
        );
        assert_eq!(image.pixels(), &[2, 0, 2, 0, 2, 0]);
    }
    #[test]
    fn preview_supports_every_direction_and_animation_frame() {
        let archive = FrmArchive::parse(&fixtures::animated_frm()).unwrap();
        let palette = Palette::parse(&fixtures::grayscale_palette()).unwrap();
        let font = Font::parse_fon(&fixtures::fon()).unwrap();
        for direction in 0..6 {
            for frame in 0..2 {
                let image =
                    compose_preview(&archive, direction, frame, &palette, &font, b"AB").unwrap();
                let expected = archive
                    .frame(direction, frame)
                    .unwrap()
                    .pixels()
                    .iter()
                    .copied()
                    .find(|pixel| *pixel != 0)
                    .unwrap();
                assert!(image.pixels().contains(&expected));
            }
        }
        assert!(compose_preview(&archive, 6, 0, &palette, &font, b"AB").is_err());
    }
}
