//! SPEC-014 §3.2 the help screen (F1): with no fade, `helpscrn.frm` (list
//! index 297) is drawn under `art/intrface/helpscrn.pal` until any key or
//! click; then the game palette returns. Palette cycling is suspended
//! meanwhile (the help palette replaces the display palette).
use super::{MapPreview, Screen};
use crate::{AppResult, interface::Painter};
use vault_core::ui::ColorKey;
use vault_formats::{MAX_ASSET_BYTES, Palette};
use vault_render::IndexedImage;

const ART: &str = "helpscrn.frm";
const PALETTE: &str = "art/intrface/helpscrn.pal";

pub(crate) struct HelpScreen {
    /// The help palette, or black without one.
    palette: Option<Palette>,
}

impl MapPreview {
    pub(crate) fn open_help(&mut self) {
        let palette = self
            .host
            .assets
            .vfs
            .read_bounded(PALETTE, MAX_ASSET_BYTES)
            .ok()
            .and_then(|bytes| Palette::parse(&bytes).ok());
        self.screen = Screen::Help(Box::new(HelpScreen { palette }));
    }
    pub(crate) fn close_help(&mut self) -> bool {
        if matches!(self.screen, Screen::Help(_)) {
            self.screen = Screen::Map;
        }
        true
    }
    pub(crate) fn help_image(&self) -> AppResult<IndexedImage> {
        let painter = Painter {
            assets: &self.host.assets,
            palette: &self.palette,
        };
        let mut image = IndexedImage::new(640, 480)?;
        image.fill_rect([0, 0, 640, 480], painter.color(ColorKey::BLACK));
        painter.blit_opaque(&mut image, ART, 0, [0, 0]);
        Ok(image)
    }
    /// The help palette shown in place of the game's.
    pub(crate) fn help_display(&self, gamma: f64) -> Option<[[u8; 4]; 256]> {
        let Screen::Help(screen) = &self.screen else {
            return None;
        };
        Some(match &screen.palette {
            Some(palette) => palette.display_rgba8(gamma),
            None => [[0, 0, 0, 255]; 256],
        })
    }
}
