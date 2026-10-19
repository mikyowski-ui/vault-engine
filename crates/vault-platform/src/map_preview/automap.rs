//! The automap overlay (Tab unless Alt is held, or the bar's MAP button;
//! SPEC-014 §3.19): the current map and elevation's SPEC-008 §2.8 raster,
//! extracted live from the seen walls and scenery as a capture would store
//! it. Four two-bit cells per byte, most-significant pair first; values 1
//! (wall) and 2 (scenery) draw, 0 and 3 do not; each cell covers 2 by 2
//! pixels, as in the Pip-Boy view, for a 400-by-400 picture.
//!
//! No approved spec gives the in-game automap window's art, layout,
//! colors or buttons, so this presentation is provisional: a black screen
//! with the picture centered, walls in key 992, scenery in key 21204, the
//! player's hex in white and a caption. Any key or click closes it.
use super::{MapPreview, Screen};
use crate::{AppResult, interface::Painter};
use vault_core::ui::ColorKey;
use vault_formats::automap;
use vault_render::IndexedImage;

pub(crate) struct AutomapScreen {
    pub elevation: u8,
}

/// The picture's top-left corner.
const ORIGIN: [i32; 2] = [120, 40];

/// The raster position of hex `tile`: the writer's `X = 200 - column`, so
/// column 0 lands at X 0 of the following row.
fn cell_of(tile: i32) -> Option<[i32; 2]> {
    if !(0..40000).contains(&tile) {
        return None;
    }
    let (row, x) = (tile / 200, 200 - tile % 200);
    Some(if x == 200 { [0, row + 1] } else { [x, row] })
}

impl MapPreview {
    pub(crate) fn open_automap(&mut self) {
        if matches!(self.screen, Screen::Map) {
            self.screen = Screen::Automap(AutomapScreen {
                elevation: self.host.world.elevation,
            });
        }
    }
    pub(crate) fn close_automap(&mut self) -> bool {
        if !matches!(self.screen, Screen::Automap(_)) {
            return false;
        }
        self.screen = Screen::Map;
        true
    }
    pub(crate) fn automap_image(&self) -> AppResult<IndexedImage> {
        let Screen::Automap(screen) = &self.screen else {
            return Err("no automap".into());
        };
        let painter = Painter {
            assets: &self.host.assets,
            palette: &self.palette,
        };
        let mut image = IndexedImage::new(640, 480)?;
        image.fill_rect([0, 0, 640, 480], painter.color(ColorKey::BLACK));
        let raster = self.host.world.automap_raster(screen.elevation);
        let colors = [
            None,
            Some(painter.color(ColorKey::GREEN)),
            Some(painter.color(ColorKey::GREY_GREEN)),
            None,
        ];
        let [ox, oy] = ORIGIN;
        for row in 0..200 {
            for column in 0..200 {
                if let Some(color) = colors[usize::from(automap::cell(&raster, row, column))] {
                    image.fill_rect([ox + 2 * column as i32, oy + 2 * row as i32, 2, 2], color);
                }
            }
        }
        let player = self.host.world.player;
        if let Some(object) = self.host.world.get(player)
            && object.raw.elevation == screen.elevation
            && let Some([x, y]) = object.raw.hex.and_then(cell_of)
        {
            image.fill_rect(
                [ox + 2 * x - 1, oy + 2 * y - 1, 4, 4],
                painter.color(ColorKey::WHITE),
            );
        }
        let caption = format!(
            "{} - elevation {} - any key closes",
            self.path,
            screen.elevation + 1
        );
        painter.centered(
            &mut image,
            101,
            ColorKey::GREEN,
            caption.as_bytes(),
            [0, oy + 410],
            640,
        );
        Ok(image)
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn hex_cells_follow_the_raster_writer() {
        // Tile 199: X = 1 on row 0; tile 0 spills to X 0 of row 1.
        assert_eq!(super::cell_of(199), Some([1, 0]));
        assert_eq!(super::cell_of(0), Some([0, 1]));
        assert_eq!(super::cell_of(201), Some([199, 1]));
        assert_eq!(super::cell_of(40000), None);
    }
}
