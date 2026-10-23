//! Drawing helpers for the SPEC-013/SPEC-014 interface windows: interface
//! art from `art/intrface/` (transparent, opaque and sub-region blits),
//! text in a numbered font and RGB555 color key, message lookups, and the
//! `bignum.frm` and `numbers.frm` digit strips.
use std::sync::Arc;
use vault_core::{
    assets::{VfsWorldAssets, WorldAssets},
    ui::{ColorKey, Rect},
};
use vault_formats::{Font, FrmArchive, Palette};
use vault_render::{IndexedImage, TextStyle};

pub(crate) struct Painter<'a> {
    pub assets: &'a VfsWorldAssets,
    pub palette: &'a Palette,
}

impl Painter<'_> {
    /// Interface art by file name (`art/intrface/<name>`) or full path.
    pub fn art(&self, name: &str) -> Option<Arc<FrmArchive>> {
        let path = if name.contains('/') {
            name.to_owned()
        } else {
            format!("art/intrface/{name}")
        };
        self.assets.frame(&path).ok()
    }
    /// Frame 0's size.
    pub fn size(&self, name: &str) -> Option<[i32; 2]> {
        let art = self.art(name)?;
        let frame = art.frame(0, 0)?;
        Some([i32::from(frame.width), i32::from(frame.height)])
    }
    /// A transparent blit of `frame`; index 0 leaves the destination.
    pub fn blit(
        &self,
        image: &mut IndexedImage,
        name: &str,
        frame: usize,
        origin: [i32; 2],
    ) -> bool {
        let Some(art) = self.art(name) else {
            return false;
        };
        let Some(frame) = art.frame(0, frame) else {
            return false;
        };
        image.blit(frame, [i64::from(origin[0]), i64::from(origin[1])]);
        true
    }
    /// Cursor art with its SPEC-001 §3.5 hotspot, `(trunc(w / 2) - direction
    /// X shift, h - 1 - direction Y shift)`, at `point` (SPEC-014 §3.26).
    pub fn cursor(&self, image: &mut IndexedImage, name: &str, point: [i32; 2]) -> bool {
        self.cursor_frame(image, name, 0, point)
    }
    /// Frame `index` of an animated cursor (SPEC-014 §3.26), with the same
    /// hotspot rule.
    pub fn cursor_frame(
        &self,
        image: &mut IndexedImage,
        name: &str,
        index: usize,
        point: [i32; 2],
    ) -> bool {
        let Some(art) = self.art(name) else {
            return false;
        };
        let Some(frame) = art.frame(0, index).or_else(|| art.frame(0, 0)) else {
            return false;
        };
        let [sx, sy] = art.shifts[0].map(i64::from);
        let hotspot = [
            i64::from(frame.width / 2) - sx,
            i64::from(frame.height) - 1 - sy,
        ];
        image.blit(
            frame,
            [
                i64::from(point[0]) - hotspot[0],
                i64::from(point[1]) - hotspot[1],
            ],
        );
        true
    }
    /// A transparent blit of frame 0 with every pixel darkened to intensity
    /// level `level` (128 leaves colors unchanged).
    pub fn blit_lit(&self, image: &mut IndexedImage, name: &str, origin: [i32; 2], level: u8) {
        let Some(art) = self.art(name) else {
            return;
        };
        let Some(frame) = art.frame(0, 0) else {
            return;
        };
        let (width, height) = (i32::from(frame.width), i32::from(frame.height));
        let stride = image.width() as i32;
        for y in 0..height {
            for x in 0..width {
                let color = frame.pixels()[(y * width + x) as usize];
                let (dx, dy) = (origin[0] + x, origin[1] + y);
                if color != 0
                    && (0..stride).contains(&dx)
                    && (0..image.height() as i32).contains(&dy)
                {
                    image.pixels_mut()[(dy * stride + dx) as usize] =
                        self.palette.intensity(color, level);
                }
            }
        }
    }
    /// An opaque blit: every pixel, index 0 included.
    pub fn blit_opaque(
        &self,
        image: &mut IndexedImage,
        name: &str,
        frame: usize,
        origin: [i32; 2],
    ) -> bool {
        let Some(art) = self.art(name) else {
            return false;
        };
        let Some(frame) = art.frame(0, frame) else {
            return false;
        };
        image.copy_frame(frame, origin);
        true
    }
    /// Copy the `source` rectangle of frame 0 to `origin`, transparently or
    /// not.
    pub fn blit_region(
        &self,
        image: &mut IndexedImage,
        name: &str,
        source: Rect,
        origin: [i32; 2],
        opaque: bool,
    ) {
        let Some(art) = self.art(name) else {
            return;
        };
        let Some(frame) = art.frame(0, 0) else {
            return;
        };
        let (width, height) = (i32::from(frame.width), i32::from(frame.height));
        for y in 0..source.h {
            for x in 0..source.w {
                let (sx, sy) = (source.x + x, source.y + y);
                if !(0..width).contains(&sx) || !(0..height).contains(&sy) {
                    continue;
                }
                let color = frame.pixels()[(sy * width + sx) as usize];
                let (dx, dy) = (origin[0] + x, origin[1] + y);
                if (opaque || color != 0)
                    && (0..image.width() as i32).contains(&dx)
                    && (0..image.height() as i32).contains(&dy)
                {
                    let stride = image.width() as usize;
                    image.pixels_mut()[dy as usize * stride + dx as usize] = color;
                }
            }
        }
    }
    pub fn color(&self, key: ColorKey) -> u8 {
        key.resolve(self.palette)
    }
    pub fn font(&self, number: i32) -> Option<Arc<Font>> {
        self.assets.font_number(number)
    }
    pub fn width(&self, font: i32, text: &[u8]) -> i32 {
        self.font(font)
            .map_or(0, |font| font.text_width(text, false) as i32)
    }
    pub fn line_height(&self, font: i32) -> i32 {
        self.font(font).map_or(10, |font| font.line_height() as i32)
    }
    /// Draw `text`; returns its width.
    pub fn text(
        &self,
        image: &mut IndexedImage,
        font: i32,
        key: ColorKey,
        text: &[u8],
        origin: [i32; 2],
        limit: i32,
    ) -> i32 {
        let Some(font) = self.font(font) else {
            return 0;
        };
        image.draw_text(
            &font,
            self.palette,
            text,
            origin,
            limit.max(0) as u32,
            TextStyle {
                foreground: self.color(key),
                ..Default::default()
            },
        ) as i32
    }
    /// Draw text centered in `width` from `origin`.
    pub fn centered(
        &self,
        image: &mut IndexedImage,
        font: i32,
        key: ColorKey,
        text: &[u8],
        origin: [i32; 2],
        width: i32,
    ) {
        let w = self.width(font, text);
        self.text(
            image,
            font,
            key,
            text,
            [origin[0] + (width - w) / 2, origin[1]],
            width.max(w),
        );
    }
    /// A `text/<language>/game/<file>` message, or empty.
    pub fn message(&self, file: &str, id: i32) -> Vec<u8> {
        self.assets
            .game_messages(file)
            .ok()
            .and_then(|catalog| catalog.get(id).map(|message| message.text.clone()))
            .unwrap_or_default()
    }
    /// One `bignum.frm` cell (14 by 24; 0-9 digits, 11 rolling; red set at
    /// x offset 168).
    pub fn big_digit(&self, image: &mut IndexedImage, cell: u8, origin: [i32; 2], red: bool) {
        let x = 14 * i32::from(cell) + if red { 168 } else { 0 };
        self.blit_region(image, "bignum.frm", Rect::new(x, 0, 14, 24), origin, true);
    }
}

/// SPEC-014 §3.19 F12: write `image` (the composed 640-by-480 screen) as
/// the §2.5 bitmap under `palette`, the installed palette before brightness
/// (levels `6-bit × 4`, as a neutral-gamma display palette gives them). The
/// name is the first of `scr00000.bmp` to `scr99999.bmp` in `directory`
/// (the process's working directory in the game) that cannot be opened
/// for reading, so a gap in the numbering is filled first.
///
/// The reference reports a failure only when every name exists or the
/// file cannot be created; a write error after that still counts as
/// saved. `[May-improve]` safe rule: a failed write is reported as an
/// error too.
pub(crate) fn save_screenshot(
    directory: &std::path::Path,
    image: &IndexedImage,
    palette: &[[u8; 4]; 256],
) -> Result<std::path::PathBuf, String> {
    let levels = palette.map(|[r, g, b, _]| [r / 4, g / 4, b / 4]);
    let bytes =
        vault_formats::screenshot::write(image.width(), image.height(), image.pixels(), &levels)
            .ok_or("the screenshot does not fit a bitmap")?;
    let path = (0..100_000)
        .map(|number| directory.join(format!("scr{number:05}.bmp")))
        .find(|path| std::fs::File::open(path).is_err())
        .ok_or("no free screenshot name")?;
    std::fs::write(&path, bytes).map_err(|error| format!("{}: {error}", path.display()))?;
    tracing::info!(path = %path.display(), "screenshot saved");
    Ok(path)
}

#[cfg(test)]
mod tests {
    #[test]
    fn fixture_s_screenshots_fill_the_first_gap_in_the_numbering() {
        let dir = tempfile::tempdir().unwrap();
        for name in ["scr00000.bmp", "scr00002.bmp"] {
            std::fs::write(dir.path().join(name), b"").unwrap();
        }
        let image = vault_render::IndexedImage::new(640, 480).unwrap();
        let mut palette = [[0, 0, 0, 255]; 256];
        palette[5] = [252, 128, 0, 255];
        let path = super::save_screenshot(dir.path(), &image, &palette).unwrap();
        assert_eq!(path, dir.path().join("scr00001.bmp"));
        let bytes = std::fs::read(&path).unwrap();
        assert_eq!(bytes.len(), 308_278);
        assert_eq!(bytes[0x36 + 20..0x36 + 24], [0x00, 0x80, 0xFC, 0x00]);
        // Safe rule: a file that cannot be written is an error.
        let missing = dir.path().join("missing");
        assert!(super::save_screenshot(&missing, &image, &palette).is_err());
    }
}
