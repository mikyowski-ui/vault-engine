//! The launcher's widgets: buttons, pills, switches, sliders, choices, text
//! fields, lists and badges, each a rectangle drawn with the launcher palette
//! and the launcher font. Text is cut to its rectangle before it is drawn, so
//! nothing here spills into a neighbour.
use super::font::{self, ADVANCE};
use super::view::color;
use vault_render::IndexedImage;
use winit::keyboard::KeyCode;

/// A frame rectangle: left, top, width, height.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Rect {
    pub x: i32,
    pub y: i32,
    pub w: i32,
    pub h: i32,
}

/// How far each of a corner's rows starts in from the edge, by radius.
const CORNERS: [&[i32]; 4] = [&[], &[1], &[2, 1], &[3, 1, 1]];

impl Rect {
    pub const fn new(x: i32, y: i32, w: i32, h: i32) -> Self {
        Self { x, y, w, h }
    }
    pub fn contains(self, [x, y]: [i32; 2]) -> bool {
        (self.x..self.x + self.w).contains(&x) && (self.y..self.y + self.h).contains(&y)
    }
    pub const fn right(self) -> i32 {
        self.x + self.w
    }
    pub const fn bottom(self) -> i32 {
        self.y + self.h
    }
    /// The rectangle `by` pixels smaller on every side.
    pub const fn inset(self, by: i32) -> Self {
        Self::new(self.x + by, self.y + by, self.w - 2 * by, self.h - 2 * by)
    }
    /// Whether the two share a pixel.
    #[cfg(test)]
    pub fn overlaps(self, other: Self) -> bool {
        self.x < other.right()
            && other.x < self.right()
            && self.y < other.bottom()
            && other.y < self.bottom()
    }
    /// Whether every pixel of it lies in `outer`.
    #[cfg(test)]
    pub fn within(self, outer: Self) -> bool {
        self.x >= outer.x
            && self.y >= outer.y
            && self.right() <= outer.right()
            && self.bottom() <= outer.bottom()
    }
    pub fn fill(self, image: &mut IndexedImage, color: u8) {
        image.fill_rect([self.x, self.y, self.w, self.h], color);
    }
    /// Filled, with its corners cut to `radius` (at most 3).
    pub fn rounded(self, image: &mut IndexedImage, color: u8, radius: usize) {
        let Self { x, y, w, h } = self;
        let corner = CORNERS[radius.min(3)];
        let rows = (corner.len() as i32).min(h / 2);
        for (row, inset) in (0..rows).zip(corner) {
            image.fill_rect([x + inset, y + row, w - 2 * inset, 1], color);
            image.fill_rect([x + inset, y + h - 1 - row, w - 2 * inset, 1], color);
        }
        image.fill_rect([x, y + rows, w, h - 2 * rows], color);
    }
    /// A one-pixel outline.
    pub fn outline(self, image: &mut IndexedImage, color: u8) {
        self.rounded_outline(image, color, 0);
    }
    /// A one-pixel outline that follows [`Self::rounded`]'s corners.
    pub fn rounded_outline(self, image: &mut IndexedImage, color: u8, radius: usize) {
        let Self { x, y, w, h } = self;
        let corner = CORNERS[radius.min(3)];
        let rows = (corner.len() as i32).min(h / 2);
        let first = corner.first().copied().unwrap_or(0);
        image.fill_rect([x + first, y, w - 2 * first, 1], color);
        image.fill_rect([x + first, y + h - 1, w - 2 * first, 1], color);
        for row in 1..rows {
            let inset = corner[row as usize];
            let span = (corner[row as usize - 1] - inset).max(1);
            for top in [y + row, y + h - 1 - row] {
                image.fill_rect([x + inset, top, span, 1], color);
                image.fill_rect([x + w - inset - span, top, span, 1], color);
            }
        }
        let top = rows.max(1);
        image.fill_rect([x, y + top, 1, h - 2 * top], color);
        image.fill_rect([x + w - 1, y + top, 1, h - 2 * top], color);
    }
    /// Text centred vertically, `inset` pixels from the left edge.
    fn text(self, image: &mut IndexedImage, inset: i32, text: &str, color: u8) {
        font::draw(
            image,
            [self.x + inset, self.y + (self.h - 7) / 2],
            text,
            color,
        );
    }
    /// Text cut to the rectangle and centred in it.
    fn centred(self, image: &mut IndexedImage, text: &str, color: u8) {
        let text = font::clip_end(text, font::columns(self.w - 2));
        self.text(image, (self.w - font::width(&text) + 1) / 2, &text, color);
    }
}

/// How a widget shows: focused (keyboard), hovered (pointer), or usable.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Look {
    pub focused: bool,
    pub hovered: bool,
    pub disabled: bool,
}

impl Look {
    fn live_hover(self) -> bool {
        self.hovered && !self.disabled
    }
    /// The widget's fill: `fill` at rest, lighter under the pointer.
    fn fill(self, fill: u8) -> u8 {
        if self.live_hover() {
            color::HOVER
        } else {
            fill
        }
    }
    /// The widget's edge: `edge` at rest, the accent under the pointer and
    /// the focus ring over both.
    fn edge(self, edge: u8) -> u8 {
        if self.focused {
            color::FOCUS
        } else if self.live_hover() {
            color::ACCENT
        } else {
            edge
        }
    }
    fn ink(self) -> u8 {
        if self.disabled {
            color::DIM
        } else {
            color::TEXT
        }
    }
}

const BUTTON_RADIUS: usize = 2;

fn frame(image: &mut IndexedImage, rect: Rect, look: Look, fill: u8) {
    rect.rounded(image, look.fill(fill), BUTTON_RADIUS);
    rect.rounded_outline(image, look.edge(color::BORDER), BUTTON_RADIUS);
}

/// A push button with its label centred.
pub fn button(image: &mut IndexedImage, rect: Rect, label: &str, look: Look) {
    frame(image, rect, look, color::BUTTON);
    rect.centred(image, label, look.ink());
}

/// A button for a choice among several, marked while it is the one in force.
pub fn chip(image: &mut IndexedImage, rect: Rect, label: &str, on: bool, look: Look) {
    if !on {
        return button(image, rect, label, look);
    }
    rect.rounded(image, color::SELECTED, BUTTON_RADIUS);
    let edge = if look.focused {
        color::FOCUS
    } else {
        color::ACCENT
    };
    rect.rounded_outline(image, edge, BUTTON_RADIUS);
    rect.centred(image, label, color::BRIGHT);
}

/// The screen's main action: filled with the accent.
pub fn primary(image: &mut IndexedImage, rect: Rect, label: &str, look: Look) {
    if look.disabled {
        return button(image, rect, label, look);
    }
    let fill = if look.hovered {
        color::BRIGHT
    } else {
        color::ACCENT
    };
    rect.rounded(image, fill, BUTTON_RADIUS);
    if look.focused {
        rect.rounded_outline(image, color::FOCUS, BUTTON_RADIUS);
    }
    rect.centred(image, label, color::BACKGROUND);
}

/// A tab: a pill, filled while it is the current one.
pub fn pill(image: &mut IndexedImage, rect: Rect, label: &str, current: bool, look: Look) {
    let radius = 3;
    let fill = match (current, look.live_hover()) {
        (true, _) => color::SELECTED,
        (false, true) => color::HOVER,
        (false, false) => color::BACKGROUND,
    };
    rect.rounded(image, fill, radius);
    if look.focused {
        rect.rounded_outline(image, color::FOCUS, radius);
    } else if current {
        rect.rounded_outline(image, color::ACCENT, radius);
    }
    let ink = if current { color::BRIGHT } else { color::TEXT };
    rect.centred(image, label, ink);
}

/// A small label on a filled tag at `origin`; returns its right edge.
pub fn badge(image: &mut IndexedImage, origin: [i32; 2], text: &str, fill: u8, ink: u8) -> i32 {
    let rect = Rect::new(origin[0], origin[1], badge_width(text), BADGE_HEIGHT);
    rect.rounded(image, fill, 1);
    rect.text(image, 3, text, ink);
    rect.right()
}

pub const BADGE_HEIGHT: i32 = 11;

pub fn badge_width(text: &str) -> i32 {
    font::width(text) + 5
}

/// A settings row: its whole width takes the pointer, and it lights up
/// under it.
pub fn row(image: &mut IndexedImage, rect: Rect, look: Look) {
    if look.live_hover() {
        rect.rounded(image, color::HOVER, BUTTON_RADIUS);
    }
    if look.focused {
        rect.rounded_outline(image, color::FOCUS, BUTTON_RADIUS);
    }
}

/// A choice among named values, read as `< value >`.
pub fn choice(image: &mut IndexedImage, rect: Rect, value: &str, look: Look) {
    frame(image, rect, look, color::FIELD);
    rect.text(image, 4, "<", color::DIM);
    rect.text(image, rect.w - 9, ">", color::DIM);
    Rect::new(rect.x + 10, rect.y, rect.w - 20, rect.h).centred(image, value, color::TEXT);
}

const SWITCH_WIDTH: i32 = 22;
const SWITCH_HEIGHT: i32 = 12;

/// An on/off switch at the right of `rect`, with the state written beside it.
pub fn switch(image: &mut IndexedImage, rect: Rect, on: bool) {
    let track = Rect::new(
        rect.right() - SWITCH_WIDTH,
        rect.y + (rect.h - SWITCH_HEIGHT) / 2,
        SWITCH_WIDTH,
        SWITCH_HEIGHT,
    );
    track.rounded(image, if on { color::ACCENT } else { color::BORDER }, 3);
    let knob = Rect::new(
        if on { track.right() - 10 } else { track.x + 2 },
        track.y + 2,
        8,
        8,
    );
    knob.rounded(image, if on { color::BACKGROUND } else { color::DIM }, 2);
    let (state, ink) = if on {
        ("On", color::ACCENT)
    } else {
        ("Off", color::DIM)
    };
    let x = track.x - 5 - font::width(state);
    font::draw(image, [x, rect.y + (rect.h - 7) / 2], state, ink);
}

/// The stop nearest a frame x on a slider's track.
pub fn slider_stop(track: Rect, x: i32, stops: usize) -> usize {
    if stops < 2 {
        return 0;
    }
    let span = (track.w - 1).max(1);
    let last = stops as i32 - 1;
    let at = ((x - track.x).clamp(0, span) * last + span / 2) / span;
    at as usize
}

/// A slider over `stops` values with `stop` chosen: a tick for each stop,
/// the track filled up to the knob.
pub fn slider(image: &mut IndexedImage, track: Rect, stop: usize, stops: usize, look: Look) {
    let span = track.w - 1;
    let last = (stops.max(2) - 1) as i32;
    let at = |stop: i32| track.x + stop * span / last;
    let middle = track.y + track.h / 2;
    let ink = if look.disabled {
        color::DIM
    } else {
        color::ACCENT
    };
    image.fill_rect([track.x, middle - 1, track.w, 2], color::BORDER);
    for tick in 0..=last {
        image.fill_rect([at(tick), middle - 3, 1, 6], color::BORDER);
    }
    let knob = at((stop as i32).min(last));
    image.fill_rect([track.x, middle - 1, knob - track.x, 2], ink);
    Rect::new(knob - 2, middle - 5, 5, 10).rounded(image, ink, 1);
}

/// A small up or down arrow button.
pub fn arrow_button(image: &mut IndexedImage, rect: Rect, up: bool, look: Look) {
    frame(image, rect, look, color::BUTTON);
    let middle = rect.x + rect.w / 2;
    let top = rect.y + (rect.h - 4) / 2;
    for row in 0..4 {
        let half = if up { row } else { 3 - row };
        image.fill_rect([middle - half, top + row, 2 * half + 1, 1], look.ink());
    }
}

/// A 9 x 9 checkbox with its top-left at `origin`.
pub fn checkbox(image: &mut IndexedImage, origin: [i32; 2], on: bool) {
    let rect = Rect::new(origin[0], origin[1], 9, 9);
    if on {
        rect.rounded(image, color::ACCENT, 1);
        font::draw(
            image,
            [rect.x + 2, rect.y],
            &font::CHECK.to_string(),
            color::BACKGROUND,
        );
    } else {
        rect.rounded_outline(image, color::DIM, 1);
    }
}

/// A thin scroll bar in `track`, for `total` items of which `shown` from
/// `first` show. Nothing when they all do.
pub fn scrollbar(image: &mut IndexedImage, track: Rect, first: usize, shown: usize, total: usize) {
    if total <= shown {
        return;
    }
    track.fill(image, color::BORDER);
    let h = (track.h * shown as i32 / total as i32).max(6).min(track.h);
    let travel = track.h - h;
    let range = (total - shown) as i32;
    let y = track.y + travel * (first as i32).min(range) / range;
    Rect::new(track.x, y, track.w, h).fill(image, color::DIM);
}

/// An editable line of text with a caret.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct TextField {
    pub text: String,
    /// The caret, in characters.
    pub cursor: usize,
}

impl TextField {
    pub fn new(text: impl Into<String>) -> Self {
        let text = text.into();
        let cursor = text.chars().count();
        Self { text, cursor }
    }
    fn byte(&self, at: usize) -> usize {
        self.text
            .char_indices()
            .nth(at)
            .map_or(self.text.len(), |(index, _)| index)
    }
    /// Apply an editing key, or the byte it types; returns whether the
    /// text changed (`None`: the key is not an editing key).
    pub fn edit(&mut self, key: KeyCode, typed: Option<u8>) -> Option<bool> {
        let count = self.text.chars().count();
        match key {
            KeyCode::ArrowLeft => self.cursor = self.cursor.saturating_sub(1),
            KeyCode::ArrowRight => self.cursor = (self.cursor + 1).min(count),
            KeyCode::Home => self.cursor = 0,
            KeyCode::End => self.cursor = count,
            KeyCode::Backspace if self.cursor > 0 => {
                self.cursor -= 1;
                self.text.remove(self.byte(self.cursor));
                return Some(true);
            }
            KeyCode::Delete if self.cursor < count => {
                self.text.remove(self.byte(self.cursor));
                return Some(true);
            }
            KeyCode::Backspace | KeyCode::Delete => {}
            _ => {
                let typed = typed?;
                self.text.insert(self.byte(self.cursor), char::from(typed));
                self.cursor += 1;
                return Some(true);
            }
        }
        Some(false)
    }
    /// The first character shown in `columns` glyphs, so the caret stays
    /// in view.
    pub fn first_shown(&self, columns: usize) -> usize {
        self.cursor.saturating_sub(columns.saturating_sub(1))
    }
    /// Place the caret at a frame x inside the field drawn at `rect`.
    pub fn click(&mut self, rect: Rect, x: i32) {
        let columns = field_columns(rect);
        let column = ((x - rect.x - 4 + ADVANCE / 2) / ADVANCE).max(0) as usize;
        let count = self.text.chars().count();
        self.cursor = (self.first_shown(columns) + column).min(count);
    }
}

fn field_columns(rect: Rect) -> usize {
    font::columns(rect.w - 8).max(1)
}

/// A text field, with a dim placeholder while empty.
pub fn text_field(
    image: &mut IndexedImage,
    rect: Rect,
    field: &TextField,
    placeholder: &str,
    look: Look,
) {
    frame(image, rect, look, color::FIELD);
    let columns = field_columns(rect);
    if field.text.is_empty() && !look.focused {
        rect.text(image, 4, &font::clip_end(placeholder, columns), color::DIM);
        return;
    }
    let first = field.first_shown(columns);
    let shown: String = field.text.chars().skip(first).take(columns).collect();
    rect.text(image, 4, &shown, color::TEXT);
    if look.focused {
        let x = rect.x + 4 + (field.cursor - first) as i32 * ADVANCE - 1;
        image.fill_rect([x, rect.y + 3, 1, rect.h - 6], color::FOCUS);
    }
}

/// The selection and scroll of a list.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct List {
    pub selected: usize,
    pub scroll: usize,
}

/// The height of one list row, in pixels.
pub const ROW: i32 = 12;

impl List {
    /// Rows a list drawn at `rect` shows.
    pub fn rows(rect: Rect) -> usize {
        ((rect.h - 4) / ROW).max(1) as usize
    }
    /// Move the selection by `delta` among `len` items, keeping it shown.
    pub fn step(&mut self, delta: isize, len: usize, rows: usize) {
        if len == 0 {
            *self = Self::default();
            return;
        }
        self.selected = self.selected.saturating_add_signed(delta).min(len - 1);
        self.show(len, rows);
    }
    /// Keep the selection inside `len` items and its row in view.
    pub fn show(&mut self, len: usize, rows: usize) {
        self.selected = self.selected.min(len.saturating_sub(1));
        if self.selected < self.scroll {
            self.scroll = self.selected;
        } else if self.selected >= self.scroll + rows {
            self.scroll = self.selected + 1 - rows;
        }
        self.scroll = self.scroll.min(len.saturating_sub(rows));
    }
    /// The item under a frame y in a list drawn at `rect`.
    pub fn row_at(&self, rect: Rect, y: i32, len: usize) -> Option<usize> {
        let row = ((y - rect.y - 2) / ROW) as usize;
        let item = self.scroll + row;
        (y >= rect.y + 2 && row < Self::rows(rect) && item < len).then_some(item)
    }
}

/// One list row: its text and colour, a checkbox if it has one, and an
/// optional right-hand note.
pub struct Row {
    pub text: String,
    pub color: u8,
    pub check: Option<bool>,
    pub note: Option<(char, u8, String)>,
}

/// The pixels a list row's checkbox takes before its text.
const CHECK_COLUMN: i32 = 14;
/// The scroll bar's width, with a pixel clear of the frame.
const BAR: i32 = 3;

/// A list of rows with the selection highlighted; the note column starts
/// `note_x` pixels from the left edge.
pub fn list(
    image: &mut IndexedImage,
    rect: Rect,
    list: &List,
    rows: &[Row],
    note_x: i32,
    look: Look,
) {
    frame(
        image,
        rect,
        Look {
            hovered: false,
            ..look
        },
        color::FIELD,
    );
    let shown = List::rows(rect);
    let text_end = note_x.min(rect.w - BAR - 2);
    let note_columns = font::columns(rect.w - note_x - 2 * ADVANCE - BAR - 4).max(1);
    for (line, index) in (0..shown).zip(list.scroll..rows.len()) {
        let row = &rows[index];
        let top = rect.y + 2 + line as i32 * ROW;
        if index == list.selected {
            let fill = if look.focused {
                color::SELECTED
            } else {
                color::BUTTON
            };
            image.fill_rect([rect.x + 2, top, rect.w - BAR - 5, ROW], fill);
        }
        let mut x = rect.x + 6;
        if let Some(on) = row.check {
            checkbox(image, [x, top + 1], on);
            x += CHECK_COLUMN;
        }
        let columns = font::columns(rect.x + text_end - 2 - x).max(1);
        let text = font::clip_start(&row.text, columns);
        font::draw(image, [x, top + 2], &text, row.color);
        if let Some((glyph, color, note)) = &row.note {
            let x = font::draw(
                image,
                [rect.x + note_x, top + 2],
                &glyph.to_string(),
                *color,
            );
            let note = font::clip_end(note, note_columns);
            font::draw(image, [x + ADVANCE, top + 2], &note, color::DIM);
        }
    }
    scrollbar(
        image,
        Rect::new(rect.right() - BAR - 2, rect.y + 2, BAR, rect.h - 4),
        list.scroll,
        shown,
        rows.len(),
    );
    if rows.is_empty() {
        font::draw(image, [rect.x + 6, rect.y + 4], "(empty)", color::DIM);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn blank() -> IndexedImage {
        IndexedImage::new(64, 48).unwrap()
    }

    fn inked(image: &IndexedImage) -> Vec<[i32; 2]> {
        let width = image.width() as i32;
        (0..)
            .zip(image.pixels())
            .filter(|(_, pixel)| **pixel != 0)
            .map(|(at, _): (i32, _)| [at % width, at / width])
            .collect()
    }

    #[test]
    fn rounded_corners_stay_inside_and_are_cut() {
        let rect = Rect::new(4, 4, 20, 12);
        for radius in 0..=3 {
            let mut filled = blank();
            rect.rounded(&mut filled, 5, radius);
            let mut outlined = blank();
            rect.rounded_outline(&mut outlined, 5, radius);
            for image in [&filled, &outlined] {
                assert!(inked(image).iter().all(|point| rect.contains(*point)));
            }
            // Every outline pixel is on the shape it follows.
            let shape = inked(&filled);
            assert!(inked(&outlined).iter().all(|point| shape.contains(point)));
            assert_eq!(shape.contains(&[4, 4]), radius == 0);
        }
    }

    #[test]
    fn rects_report_their_overlaps() {
        let rect = Rect::new(10, 10, 10, 10);
        assert!(rect.overlaps(Rect::new(19, 19, 4, 4)));
        assert!(!rect.overlaps(Rect::new(20, 10, 4, 4)));
        assert!(!rect.overlaps(Rect::new(10, 20, 4, 4)));
        assert!(Rect::new(12, 12, 2, 2).within(rect));
        assert!(!Rect::new(12, 12, 9, 2).within(rect));
        assert_eq!(rect.inset(2), Rect::new(12, 12, 6, 6));
    }

    #[test]
    fn widgets_draw_inside_their_rectangles() {
        let rect = Rect::new(6, 6, 40, 18);
        let long = "a label far too long for its rectangle";
        let looks = [
            Look::default(),
            Look {
                focused: true,
                ..Look::default()
            },
            Look {
                hovered: true,
                ..Look::default()
            },
            Look {
                disabled: true,
                ..Look::default()
            },
        ];
        for look in looks {
            let draws: [&dyn Fn(&mut IndexedImage); 9] = [
                &|image| chip(image, rect, long, true, look),
                &|image| button(image, rect, long, look),
                &|image| primary(image, rect, long, look),
                &|image| pill(image, rect, long, true, look),
                &|image| pill(image, rect, long, false, look),
                &|image| row(image, rect, look),
                &|image| choice(image, rect, long, look),
                &|image| switch(image, rect, true),
                &|image| arrow_button(image, rect, false, look),
            ];
            for draw in draws {
                let mut image = blank();
                draw(&mut image);
                assert!(inked(&image).iter().all(|point| rect.contains(*point)));
            }
        }
    }

    #[test]
    fn a_switch_shows_its_state_by_knob_and_by_word() {
        let rect = Rect::new(0, 0, 64, 18);
        let draw = |on| {
            let mut image = blank();
            switch(&mut image, rect, on);
            image
        };
        assert_ne!(draw(true).pixels(), draw(false).pixels());
    }

    #[test]
    fn a_slider_maps_its_stops_both_ways() {
        let track = Rect::new(100, 0, 71, 12);
        assert_eq!(slider_stop(track, 0, 8), 0);
        assert_eq!(slider_stop(track, 100, 8), 0);
        assert_eq!(slider_stop(track, 170, 8), 7);
        assert_eq!(slider_stop(track, 900, 8), 7);
        assert_eq!(slider_stop(track, 150, 8), 5);
        assert_eq!(slider_stop(track, 135, 3), 1);
        assert_eq!(slider_stop(track, 135, 1), 0);
        // The knob moves with the stop, and stays on the track.
        let draw = |stop| {
            let mut image = IndexedImage::new(200, 20).unwrap();
            slider(&mut image, track, stop, 8, Look::default());
            image
        };
        assert_ne!(draw(0).pixels(), draw(7).pixels());
        let knob = Rect::new(98, 0, 76, 12);
        for stop in 0..8 {
            assert!(inked(&draw(stop)).iter().all(|point| knob.contains(*point)));
        }
    }

    #[test]
    fn a_scrollbar_shows_only_when_something_is_hidden() {
        let track = Rect::new(10, 4, 3, 40);
        let draw = |first, shown, total| {
            let mut image = blank();
            scrollbar(&mut image, track, first, shown, total);
            image
        };
        assert!(inked(&draw(0, 5, 5)).is_empty());
        assert!(inked(&draw(0, 5, 2)).is_empty());
        for first in 0..=35 {
            assert!(
                inked(&draw(first, 5, 40))
                    .iter()
                    .all(|p| track.contains(*p))
            );
        }
        assert_ne!(draw(0, 5, 40).pixels(), draw(35, 5, 40).pixels());
    }

    #[test]
    fn long_list_rows_stay_inside_the_list() {
        let rect = Rect::new(2, 2, 60, 40);
        let rows: Vec<Row> = (0..9)
            .map(|at| Row {
                text: format!("row {at} with a name far too long for the list"),
                color: color::TEXT,
                check: Some(at % 2 == 0),
                note: Some(('x', color::OK, "a note far too long as well".into())),
            })
            .collect();
        let mut image = blank();
        let scrolled = List {
            selected: 8,
            scroll: 6,
        };
        list(&mut image, rect, &scrolled, &rows, 30, Look::default());
        assert!(inked(&image).iter().all(|point| rect.contains(*point)));
    }
}
