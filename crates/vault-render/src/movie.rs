//! SPEC-012 §3.9-3.10 movie presentation in the 640 x 480 window: each
//! displayed frame is copied unscaled and centered (the recorded region's
//! offset in the frame, the region's size), and subtitles are drawn into a
//! band below it. The window keeps what earlier frames and subtitles drew.
use crate::{IndexedImage, TextStyle};
use vault_core::movie::{WINDOW, frame_origin, subtitle_band_lines, subtitle_lines};
use vault_formats::{Font, Palette};

pub struct MovieWindow {
    pub image: IndexedImage,
    /// The last displayed frame's height and top.
    shown: [i32; 2],
    /// The band the last subtitle filled.
    band: Option<[i32; 4]>,
}
impl Default for MovieWindow {
    fn default() -> Self {
        Self::new()
    }
}
impl MovieWindow {
    pub fn new() -> Self {
        Self {
            image: IndexedImage::new(WINDOW[0] as u32, WINDOW[1] as u32)
                .expect("the movie window is a valid image"),
            shown: [0, 0],
            band: None,
        }
    }
    /// Copy a displayed frame of `size` pixels; `region` is the recorded
    /// decode region `[x, y, w, h]` in blocks. The destination is the
    /// centered origin, with the region's size.
    pub fn show_frame(&mut self, pixels: &[u8], size: [usize; 2], region: [usize; 4]) {
        let origin = frame_origin(size);
        let [x, y, w, h] = region.map(|blocks| blocks * 8);
        for row in 0..h {
            let source = (y + row) * size[0] + x;
            let Some(line) = pixels.get(source..source + w) else {
                break;
            };
            let target_y = origin[1] + row as i32;
            if !(0..WINDOW[1]).contains(&target_y) {
                continue;
            }
            for (col, pixel) in line.iter().enumerate() {
                let target_x = origin[0] + col as i32;
                if (0..WINDOW[0]).contains(&target_x) {
                    self.image.pixels_mut()[(target_y * WINDOW[0] + target_x) as usize] = *pixel;
                }
            }
        }
        self.shown = [h as i32, origin[1]];
    }
    /// Draw one subtitle (SPEC-012 §3.10 steps 5-6): fill the band with
    /// index 0, then wrap the text for the 640-pixel width and draw each
    /// line i transparently at `y0 + i × th`, centered at
    /// `trunc((640 - w) / 2)` for its width w including the final letter
    /// spacing. `palette` is the subtitle color table and `color` the text
    /// color index.
    ///
    /// The reference band is one text height plus 4 pixels, so only line 0
    /// of a wrapped entry is drawn and the rest is lost. `[May-improve]`
    /// safe rule: the band grows downward by one text height per extra
    /// line, to the window bottom, and every line whose bottom fits in it
    /// is drawn. Nothing is drawn below the band, and the next entry's
    /// fill also clears what is left of a grown band below the frame, so a
    /// wrapped entry leaves no ghost lines under a shorter one.
    pub fn draw_subtitle(&mut self, font: &Font, palette: &Palette, color: u8, text: &[u8]) {
        let height = font.height() as i32;
        let lines = subtitle_lines(font, text);
        let band = subtitle_band_lines(height, self.shown[0], self.shown[1], lines.len());
        if let Some([_, top, width, rows]) = self.band.replace(band) {
            let from = top.max(self.shown[0] + self.shown[1]);
            let to = top + rows;
            if to > from {
                self.image.fill_rect([0, from, width, to - from], 0);
            }
        }
        self.image.fill_rect(band, 0);
        let style = TextStyle {
            foreground: color,
            ..Default::default()
        };
        let bottom = band[1] + band[3];
        for (line, (start, end)) in lines.into_iter().enumerate() {
            let y = band[1] + line as i32 * height;
            if y + height > bottom {
                break;
            }
            let piece = &text[start..end];
            let width = font.text_width(piece, false).min(u64::from(u32::MAX)) as u32;
            let x = (WINDOW[0] - width.min(i32::MAX as u32) as i32) / 2;
            self.image
                .draw_text(font, palette, piece, [x, y], width, style);
        }
    }
}

#[cfg(test)]
use crate::fixture_assets as fixtures;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn frames_are_centered_and_subtitles_fill_their_band() {
        let mut window = MovieWindow::new();
        let size = [432, 320];
        let pixels: Vec<u8> = (0..size[0] * size[1])
            .map(|i| (i % 251) as u8 + 1)
            .collect();
        window.show_frame(&pixels, size, [0, 0, 54, 40]);
        let at =
            |window: &MovieWindow, x: i32, y: i32| window.image.pixels()[(y * 640 + x) as usize];
        assert_eq!(at(&window, 104, 80), pixels[0]);
        assert_eq!(at(&window, 535, 399), pixels[size[0] * size[1] - 1]);
        assert_eq!(at(&window, 103, 80), 0);
        assert_eq!(at(&window, 104, 400), 0);
        // A partial region copies only the region, from its offset.
        let mut partial = MovieWindow::new();
        partial.show_frame(&pixels, size, [2, 1, 2, 1]);
        assert_eq!(at(&partial, 104, 80), pixels[8 * size[0] + 16]);
        assert_eq!(at(&partial, 120, 80), 0);
        // The band below the frame is filled with 0 and holds the text.
        let palette = Palette::parse(&fixtures::palette()).unwrap();
        let font = Font::parse_fon(&fixtures::fon()).unwrap();
        window.image.fill_rect([0, 400, 640, 80], 9);
        window.draw_subtitle(&font, &palette, 5, b"AB");
        let band = vault_core::movie::subtitle_band(font.height() as i32, 320, 80);
        let rows = band[1]..band[1] + band[3];
        let colors: Vec<u8> = rows
            .clone()
            .flat_map(|y| (0..640).map(move |x| (x, y)))
            .map(|(x, y)| at(&window, x, y))
            .collect();
        assert!(colors.contains(&5), "text drawn");
        assert!(!colors.contains(&9), "band filled first");
        assert_eq!(at(&window, 0, band[1] - 1), 9, "outside the band");
        // Text is centered: as many columns of text on each side of 320.
        let columns: Vec<i32> = rows
            .flat_map(|y| (0..640).map(move |x| (x, y)))
            .filter(|(x, y)| at(&window, *x, *y) == 5)
            .map(|(x, _)| x)
            .collect();
        let (left, right) = (columns.iter().min().unwrap(), columns.iter().max().unwrap());
        assert!((left + right - 639).abs() <= font.text_width(b"A", false) as i32);
        // SPEC-012 §3.10 safe rule (Fixture S): an entry wider than 640
        // pixels shows its second line at y0 + th in a band grown to
        // 2 th + 4, and nothing below that.
        let long: Vec<u8> = b"AB ".iter().copied().cycle().take(900).collect();
        let wrapped = subtitle_lines(&font, &long);
        assert!(wrapped.len() > 1);
        let th = font.height() as i32;
        let grown = subtitle_band_lines(th, 320, 80, wrapped.len());
        assert_eq!(grown[1], band[1]);
        assert_eq!(grown[3], (wrapped.len() as i32 * th + 4).min(480 - band[1]));
        window.image.fill_rect([0, 400, 640, 80], 9);
        window.draw_subtitle(&font, &palette, 5, &long);
        let second = band[1] + th..band[1] + 2 * th;
        assert!(
            second
                .clone()
                .any(|y| (0..640).any(|x| at(&window, x, y) == 5)),
            "the second line is drawn"
        );
        let below = grown[1] + grown[3];
        assert!((below..480).all(|y| (0..640).all(|x| at(&window, x, y) == 9)));
        // The next, shorter entry clears the grown band: no ghost lines.
        window.draw_subtitle(&font, &palette, 5, b"");
        assert!((band[1]..below).all(|y| (0..640).all(|x| at(&window, x, y) == 0)));
    }
}
