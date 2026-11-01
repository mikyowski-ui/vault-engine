//! Watch Mode's banner and its overlay on the map: what the bot is doing, how
//! fast, and the way back to the player.
use super::{MapPreview, MapView};
use vault_render::IndexedImage;
use winit::keyboard::KeyCode;

impl MapPreview {
    /// A key Watch Mode takes. Space hands control to the player.
    pub(crate) fn autoplay_key(&mut self, _key: KeyCode) -> bool {
        false
    }
    /// The release of a key Watch Mode took.
    pub(crate) fn autoplay_key_up(&mut self, _key: KeyCode) -> bool {
        false
    }
    /// A press on the banner, or a manual action that takes control back.
    pub(crate) fn autoplay_click(&mut self, _point: [i32; 2]) -> bool {
        false
    }
    /// The banner, over everything.
    pub(crate) fn draw_autoplay_hud(&self, _image: &mut IndexedImage) {}
    /// The target hex and route, on the world image.
    pub(crate) fn draw_autoplay_world(
        &self,
        _image: &mut IndexedImage,
        _view: MapView,
        _scale: i32,
    ) {
    }
}
