//! SPEC-012 §3.9-3.10 movie presentation in the 640 x 480 window: each
//! displayed frame is copied unscaled and centered (the recorded region's
//! offset in the frame, the region's size), and subtitles are drawn into a
//! band below it. The window keeps what earlier frames and subtitles drew.
use crate::{IndexedImage, TextStyle, dialogue::lines};
use vault_core::movie::{WINDOW, frame_origin, subtitle_band};
use vault_formats::{Font, Palette};

pub struct MovieWindow {
    pub image: IndexedImage,
    /// The last displayed frame's height and top.
    shown: [i32; 2],
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
    /// Draw one subtitle: fill the band with index 0, then wrap the text
    /// to the window width and center each line, the first line two pixels
    /// below the band top. `palette` is the subtitle color table and
    /// `color` the text color index.
    pub fn draw_subtitle(&mut self, font: &Font, palette: &Palette, color: u8, text: &[u8]) {
        let height = font.height() as i32;
        let band = subtitle_band(height, self.shown[0], self.shown[1]);
        self.image.fill_rect(band, 0);
        let style = TextStyle {
            foreground: color,
            ..Default::default()
        };
        for (line, (start, end)) in lines(font, text, WINDOW[0] as u32).into_iter().enumerate() {
            let piece = &text[start..end];
            let width = font.text_width(piece, false).min(WINDOW[0] as u64) as i32;
            let y = band[1] + 2 + line as i32 * font.line_height() as i32;
            self.image.draw_text(
                font,
                palette,
                piece,
                [(WINDOW[0] - width) / 2, y],
                WINDOW[0] as u32,
                style,
            );
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
        let band = subtitle_band(font.height() as i32, 320, 80);
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
    }
}
