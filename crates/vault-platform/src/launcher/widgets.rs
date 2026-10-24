//! The launcher's widgets: buttons, toggles, text fields and lists, each a
//! rectangle drawn with the launcher palette and the launcher font.
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

impl Rect {
    pub const fn new(x: i32, y: i32, w: i32, h: i32) -> Self {
        Self { x, y, w, h }
    }
    pub fn contains(self, [x, y]: [i32; 2]) -> bool {
        (self.x..self.x + self.w).contains(&x) && (self.y..self.y + self.h).contains(&y)
    }
    fn fill(self, image: &mut IndexedImage, color: u8) {
        image.fill_rect([self.x, self.y, self.w, self.h], color);
    }
    /// A one-pixel outline.
    pub fn outline(self, image: &mut IndexedImage, color: u8) {
        let Self { x, y, w, h } = self;
        for edge in [
            [x, y, w, 1],
            [x, y + h - 1, w, 1],
            [x, y, 1, h],
            [x + w - 1, y, 1, h],
        ] {
            image.fill_rect(edge, color);
        }
    }
    /// Text centred vertically, `inset` pixels from the left edge.
    fn text(self, image: &mut IndexedImage, inset: i32, text: &str, color: u8) {
        font::draw(
            image,
            [self.x + inset, self.y + (self.h - 8) / 2],
            text,
            color,
        );
    }
}

/// How a widget shows: focused (keyboard), hovered (pointer), or usable.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Look {
    pub focused: bool,
    pub hovered: bool,
    pub disabled: bool,
}

fn frame(image: &mut IndexedImage, rect: Rect, look: Look, fill: u8) {
    let fill = if look.hovered && !look.disabled {
        color::HOVER
    } else {
        fill
    };
    rect.fill(image, fill);
    let border = if look.focused {
        color::FOCUS
    } else {
        color::BORDER
    };
    rect.outline(image, border);
}

/// A push button with its label centred.
pub fn button(image: &mut IndexedImage, rect: Rect, label: &str, look: Look) {
    frame(image, rect, look, color::BUTTON);
    let text = if look.disabled {
        color::DIM
    } else {
        color::TEXT
    };
    rect.text(image, (rect.w - font::width(label)) / 2, label, text);
}

/// A toggle or choice: its current value, `[x] on` or `< value >`.
pub fn toggle(image: &mut IndexedImage, rect: Rect, value: &str, look: Look) {
    frame(image, rect, look, color::FIELD);
    rect.text(image, 6, value, color::ACCENT);
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
    ((rect.w - 8) / ADVANCE).max(1) as usize
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

/// One list row: its text and colour, and an optional right-hand note.
pub struct Row {
    pub text: String,
    pub color: u8,
    pub note: Option<(char, u8, String)>,
}

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
    let columns = ((note_x - 8) / ADVANCE).max(1) as usize;
    let note_columns = ((rect.w - note_x - 16) / ADVANCE).max(1) as usize;
    for (line, index) in (0..List::rows(rect)).zip(list.scroll..rows.len()) {
        let row = &rows[index];
        let top = rect.y + 2 + line as i32 * ROW;
        if index == list.selected {
            let fill = if look.focused {
                color::SELECTED
            } else {
                color::BUTTON
            };
            image.fill_rect([rect.x + 2, top, rect.w - 4, ROW], fill);
        }
        let text = font::clip_start(&row.text, columns);
        font::draw(image, [rect.x + 6, top + 2], &text, row.color);
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
    if rows.is_empty() {
        font::draw(image, [rect.x + 6, rect.y + 4], "(empty)", color::DIM);
    }
}
