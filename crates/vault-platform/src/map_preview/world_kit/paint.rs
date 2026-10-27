//! The drawing helpers both world-map editions and the bar share.
use crate::{interface::Painter, map_preview::MapPreview};
use vault_core::{
    ui::{ColorKey, Rect, bar},
    worldmap::{
        VIEWPORT_ORIGIN, VIEWPORT_SIZE,
        screen::{self, Marker},
    },
};
use vault_render::{IndexedImage, TextStyle};

const MONTHS_ART: &str = "months.frm";
const DIGITS_ART: &str = "numbers.frm";
const BUTTON_UP_ART: &str = "lilredup.frm";
const BUTTON_DOWN_ART: &str = "lilreddn.frm";
pub(super) const TRIANGLE_ART: &str = "hotspot1.frm";
pub(super) const PRESSED_TRIANGLE_ART: &str = "hotspot2.frm";

/// The world-map view's window rectangle, as a clip.
pub(crate) const VIEW_CLIP: [i64; 4] = [
    VIEWPORT_ORIGIN[0] as i64,
    VIEWPORT_ORIGIN[1] as i64,
    VIEWPORT_SIZE[0] as i64,
    VIEWPORT_SIZE[1] as i64,
];

/// Whether `point` lies in `rect` (`[x, y, w, h]`).
pub(crate) fn inside(rect: [i32; 4], point: [i32; 2]) -> bool {
    (rect[0]..rect[0] + rect[2]).contains(&point[0])
        && (rect[1]..rect[1] + rect[3]).contains(&point[1])
}

/// A 15-row red side-panel button at `rect`, down or up, written opaque
/// from the art's top-left corner.
pub(crate) fn red_button(painter: &Painter, image: &mut IndexedImage, rect: [i32; 4], down: bool) {
    let art = if down { BUTTON_DOWN_ART } else { BUTTON_UP_ART };
    painter.blit_region(
        image,
        art,
        Rect::new(0, 0, rect[2], rect[3]),
        [rect[0], rect[1]],
        false,
    );
}

/// The town-map triangle art, pressed or not.
pub(crate) fn triangle_art(pressed: bool) -> &'static str {
    if pressed {
        PRESSED_TRIANGLE_ART
    } else {
        TRIANGLE_ART
    }
}

/// SPEC-014 §3.25 / SPEC-019 §3.5.2 indicator box `k` of the row at
/// `origin`: the box art (the first one from its fourth column), and its
/// label centered in font 101.
pub(crate) fn draw_indicator_box(
    painter: &Painter,
    image: &mut IndexedImage,
    k: usize,
    [ox, oy]: [i32; 2],
    label: &[u8],
    key: ColorKey,
) {
    let x = ox + bar::indicator_x(k);
    if k == 0 {
        painter.blit_region(
            image,
            bar::INDICATOR_BOX.0,
            Rect::new(3, 0, 127, 21),
            [x, oy],
            true,
        );
    } else {
        painter.blit_opaque(image, bar::INDICATOR_BOX.0, 0, [x, oy]);
    }
    let width = if k == 0 { 127 } else { 130 };
    painter.centered(
        image,
        101,
        key,
        label,
        [x, oy + bar::INDICATOR_LABEL_Y],
        width,
    );
}

impl MapPreview {
    /// Darken the view by discovery cell (SPEC-014 §3.39 step 2): unknown
    /// cells black, seen cells through intensity level 64 (indices 0 and
    /// 229-255 unchanged), visited cells unchanged.
    pub(in crate::map_preview) fn darken_cells(
        &self,
        painter: &Painter,
        image: &mut IndexedImage,
        view: [i32; 2],
        [columns, rows]: [usize; 2],
        discovery: impl Fn([usize; 2]) -> u8,
    ) {
        let black = painter.color(ColorKey::BLACK);
        let dim: Vec<u8> = (0..=255_u8)
            .map(|index| {
                if index == 0 || index >= 229 {
                    index
                } else {
                    self.palette.intensity(index, 64)
                }
            })
            .collect();
        let stride = image.width() as i32;
        for row in 0..rows {
            for column in 0..columns {
                let Some(rect) = screen::cell_rect([column, row], view) else {
                    continue;
                };
                match discovery([column, row]) {
                    0 => image.fill_rect(rect, black),
                    1 => {
                        let pixels = image.pixels_mut();
                        for y in rect[1]..rect[1] + rect[3] {
                            for x in rect[0]..rect[0] + rect[2] {
                                let pixel = &mut pixels[(y * stride + x) as usize];
                                *pixel = dim[usize::from(*pixel)];
                            }
                        }
                    }
                    _ => {}
                }
            }
        }
    }
    /// The travel dots under the view, in key `DOT_KEY`.
    pub(in crate::map_preview) fn draw_travel_dots(
        &self,
        painter: &Painter,
        image: &mut IndexedImage,
        view: [i32; 2],
        marked: impl Fn([i32; 2]) -> bool,
    ) {
        let dot = painter.color(ColorKey(screen::DOT_KEY));
        let stride = image.width() as i32;
        let pixels = image.pixels_mut();
        for y in 0..VIEWPORT_SIZE[1] {
            for x in 0..VIEWPORT_SIZE[0] {
                if marked([view[0] + x, view[1] + y]) {
                    let at = (VIEWPORT_ORIGIN[1] + y) * stride + VIEWPORT_ORIGIN[0] + x;
                    pixels[at as usize] = dot;
                }
            }
        }
    }
    /// G-24 / SPEC-014 §3.17: the world-map region (22, 21) 450 by 442
    /// converted through the grey lookup table.
    pub(in crate::map_preview) fn grey_view(&self, image: &mut IndexedImage) {
        let [x, y] = VIEWPORT_ORIGIN;
        let [w, h] = VIEWPORT_SIZE;
        let stride = image.width() as i32;
        for row in y..y + h {
            for column in x..x + w {
                if let Some(pixel) = image.pixels_mut().get_mut((row * stride + column) as usize) {
                    *pixel = self.grey[usize::from(*pixel)];
                }
            }
        }
    }
    /// A marker, skipped outside its drawing window and clipped to the view.
    pub(in crate::map_preview) fn draw_marker(
        &self,
        painter: &Painter,
        image: &mut IndexedImage,
        art: &str,
        marker: Marker,
        at: [i32; 2],
    ) {
        let origin = marker.origin(at);
        if !marker.shown(origin) {
            return;
        }
        if let Some(art) = painter.art(art)
            && let Some(frame) = art.frame(0, 0)
        {
            image.blit_within(frame, origin.map(i64::from), VIEW_CLIP);
        }
    }
    /// Text in font 101, key 992, with shadow, written only within its
    /// w-by-10 rectangle at `origin` and within `clip`.
    pub(in crate::map_preview) fn draw_label(
        &self,
        painter: &Painter,
        image: &mut IndexedImage,
        text: &[u8],
        origin: [i32; 2],
        width: i32,
        clip: [i64; 4],
    ) {
        let Some(font) = painter.font(screen::LABEL_FONT) else {
            return;
        };
        let Ok(mut label) = IndexedImage::new(width.max(1) as u32, 10) else {
            return;
        };
        label.draw_text(
            &font,
            &self.palette,
            text,
            [0, 0],
            width as u32,
            TextStyle {
                foreground: painter.color(ColorKey(screen::LABEL_KEY)),
                shadow: true,
                ..Default::default()
            },
        );
        let stride = image.width() as i64;
        let height = i64::from(image.height());
        let pixels = image.pixels_mut();
        for (index, color) in label.pixels().iter().enumerate() {
            let (x, y) = (
                i64::from(origin[0]) + index as i64 % i64::from(width),
                i64::from(origin[1]) + index as i64 / i64::from(width),
            );
            if *color != 0
                && (clip[0]..clip[0] + clip[2]).contains(&x)
                && (clip[1]..clip[1] + clip[3]).contains(&y)
                && (0..stride).contains(&x)
                && (0..height).contains(&y)
            {
                pixels[(y * stride + x) as usize] = *color;
            }
        }
    }
    /// The party's hover label (SPEC-014 §3.39): `text` centered over the
    /// triangle at `party`, drawn only inside its window.
    pub(in crate::map_preview) fn draw_party_label(
        &self,
        painter: &Painter,
        image: &mut IndexedImage,
        text: &[u8],
        party: [i32; 2],
    ) {
        let width = painter.width(screen::LABEL_FONT, text);
        let origin = screen::label_origin(party, width);
        if width <= 0 || !screen::label_shown(origin, width) {
            return;
        }
        self.draw_label(painter, image, text, origin, width, VIEW_CLIP);
    }
    /// A town-map marker's label (SPEC-014 §3.40, SPEC-019 §3.13): `text`
    /// placed by its width in the marker's button `rect` and clipped to it.
    pub(in crate::map_preview) fn draw_marker_label(
        &self,
        painter: &Painter,
        image: &mut IndexedImage,
        text: &[u8],
        [x, y, w, h]: [i32; 4],
    ) {
        let width = painter.width(screen::LABEL_FONT, text);
        if width > 0 {
            let origin = [x + screen::marker_label_x(width), y];
            let clip = [x, y, w, h].map(i64::from);
            self.draw_label(painter, image, text, origin, width.min(w), clip);
        }
    }
    /// The town-map marker whose triangle pixels are under `point`, of the
    /// triangles at `origins`: only the opaque pixels of `hotspot1.frm`
    /// count (the whole 25-by-13 image when the art is missing).
    pub(in crate::map_preview) fn triangle_at(
        &self,
        origins: impl IntoIterator<Item = [i32; 2]>,
        point: [i32; 2],
    ) -> Option<usize> {
        let art = self.painter().art(TRIANGLE_ART);
        let frame = art.as_ref().and_then(|art| art.frame(0, 0));
        origins.into_iter().position(|[x, y]| {
            let local = [point[0] - x, point[1] - y];
            let [w, h] = screen::MARKER_SIZE;
            if !(0..w).contains(&local[0]) || !(0..h).contains(&local[1]) {
                return false;
            }
            frame.is_none_or(|frame| {
                let width = i32::from(frame.width);
                local[0] < width
                    && local[1] < i32::from(frame.height)
                    && frame.pixels()[(local[1] * width + local[0]) as usize] != 0
            })
        })
    }
    /// The date and time from the game clock (SPEC-005 calendar), written
    /// opaque: day, month row, year, and HHMM.
    pub(in crate::map_preview) fn draw_date(&self, painter: &Painter, image: &mut IndexedImage) {
        let world = &self.host.world;
        let [month, day, year] = world.game_date();
        let digits = |image: &mut IndexedImage, value: i32, count: u32, origin: [i32; 2]| {
            for k in 0..count {
                let digit = value / 10_i32.pow(count - 1 - k) % 10;
                painter.blit_region(
                    image,
                    DIGITS_ART,
                    Rect::new(
                        screen::DIGIT[0] * digit,
                        0,
                        screen::DIGIT[0],
                        screen::DIGIT[1],
                    ),
                    [origin[0] + screen::DIGIT[0] * k as i32, origin[1]],
                    true,
                );
            }
        };
        digits(image, day as i32, 2, screen::DAY);
        let [w, h] = screen::MONTH_SIZE;
        painter.blit_region(
            image,
            MONTHS_ART,
            Rect::new(0, 15 * (month as i32 - 1), w, h),
            screen::MONTH,
            true,
        );
        digits(image, year as i32, 4, screen::YEAR);
        digits(image, world.clock.hhmm(), 4, screen::TIME);
    }
}
