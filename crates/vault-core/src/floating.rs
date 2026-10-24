//! SPEC-014 §3.46 floating text: short text drawn in the world over the hex
//! its owner stood on, created by `float_msg` (SPEC-005 `0x810A`) and by
//! combat taunts (SPEC-009 §3.17). This module holds the model: creation
//! refusals, the layout (wrap, trailing characters, box size), the
//! placement candidates, lifetimes on the real-time clock, replacement of
//! an owner's earlier text and removal at background updates. The shell
//! draws each box (vault-render `floating`) as the frame layer after the
//! outlines (§3.44).
use crate::world::ObjectRef;
use vault_formats::Font;

/// A creation is refused once 19 texts exist (the table has 20 slots).
pub const TEXT_LIMIT: usize = 19;
/// Lines wrap at 200 pixels with the shared word wrapper (§3.41.3).
pub const WRAP_WIDTH: i32 = 200;
/// The map window the placement candidates must fit in.
pub const WINDOW: [i32; 2] = [640, 380];
/// The fixed rise of the first candidate above the hex.
pub const RISE: i32 = 60;
/// `fallout.cfg` `[preferences] text_base_delay` default.
pub const DEFAULT_BASE_DELAY: f64 = 3.5;

/// A floating-text color.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Ink {
    /// An RGB555 key, resolved through the palette's color table.
    Key(u16),
    /// A raw palette index in the low byte, with the SPEC-002 §3.1 drawing
    /// flags (`0x10000` shadow, `0x20000` underline, `0x40000`
    /// monospaced) above it.
    Raw(u32),
}

/// One floating text.
#[derive(Debug, Clone, PartialEq)]
pub struct FloatingText {
    pub owner: ObjectRef,
    pub font: i32,
    /// The wrapped lines, after the trailing-character rules.
    pub lines: Vec<Vec<u8>>,
    /// The box size W by H, the outline's 2 pixels included.
    pub size: [i32; 2],
    pub color: Ink,
    /// The outline color; `None` draws no outline and adds no border.
    pub outline: Option<Ink>,
    /// The hex the owner stood on at creation, and the box's top-left
    /// offset from that hex's window position (the top-left of its 32-by-16
    /// cell). The text stays over this hex when the owner walks away.
    pub hex: i32,
    pub offset: [i32; 2],
    /// The time of the background update before the creation, and the
    /// lifetime in milliseconds of real time.
    pub created: u64,
    pub lifetime: u64,
    /// A newer text of the same owner replaces this one at the next
    /// background update; until then it is still drawn and still counts.
    pub marked: bool,
}

impl FloatingText {
    /// The box's top-left corner in the map window for the hex's current
    /// window position.
    pub fn origin(&self, hex_position: [i32; 2]) -> [i32; 2] {
        [
            hex_position[0].saturating_add(self.offset[0]),
            hex_position[1].saturating_add(self.offset[1]),
        ]
    }
    /// Whether the text expired at `now`: strictly more than its lifetime
    /// has passed since its creation.
    pub fn expired(&self, now: u64) -> bool {
        now.saturating_sub(self.created) > self.lifetime
    }
}

/// The world's floating texts, in creation order (the drawing order).
#[derive(Debug, Clone, PartialEq)]
pub struct FloatingTexts {
    pub texts: Vec<FloatingText>,
    /// `text_base_delay` in seconds, which the shell keeps in step with the
    /// preferences (SPEC-014 §3.18); the derived line delay is never read
    /// from `fallout.cfg`.
    pub base_delay: f64,
}

impl Default for FloatingTexts {
    fn default() -> Self {
        Self {
            texts: Vec::new(),
            base_delay: DEFAULT_BASE_DELAY,
        }
    }
}

impl FloatingTexts {
    pub fn is_empty(&self) -> bool {
        self.texts.is_empty()
    }
    pub fn len(&self) -> usize {
        self.texts.len()
    }
    /// Remove every text: a dialogue start, a game reset, and (safe rules)
    /// a map load and an elevation change.
    pub fn clear(&mut self) {
        self.texts.clear();
    }
    /// One background update at `now`: the marked texts and the expired
    /// ones go. Returns whether any went.
    pub fn update(&mut self, now: u64) -> bool {
        let before = self.texts.len();
        self.texts.retain(|text| !text.marked && !text.expired(now));
        self.texts.len() != before
    }
    /// Add a laid-out text, marking the owner's earlier texts for removal.
    /// Refused (returning `false`, and the earlier texts stay) once 19
    /// texts exist, marked ones included.
    pub fn push(&mut self, text: FloatingText) -> bool {
        if self.texts.len() >= TEXT_LIMIT {
            return false;
        }
        for earlier in &mut self.texts {
            if earlier.owner == text.owner {
                earlier.marked = true;
            }
        }
        self.texts.push(text);
        true
    }
}

/// The lifetime of a text of `lines` lines: `base + lines × line`, with
/// base = `trunc(text_base_delay × 1000)` and line =
/// `trunc(min(max((text_base_delay - 1) × 0.4, 0), 2) × 1000)`.
pub fn lifetime(base_delay: f64, lines: usize) -> u64 {
    let base = (base_delay * 1000.0).trunc().max(0.0) as u64;
    let line = (((base_delay - 1.0) * 0.4).clamp(0.0, 2.0) * 1000.0).trunc() as u64;
    base.saturating_add(line.saturating_mul(lines as u64))
}

/// A laid-out text: its lines and box size.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Layout {
    pub lines: Vec<Vec<u8>>,
    pub size: [i32; 2],
}

/// The line pitch: the font's line height plus 1 (11 for font 101, 21 for
/// font 103). Without the font the safe rule is font 101's 11.
pub fn line_pitch(font: Option<&Font>) -> i32 {
    font.map_or(11, |font| font.line_height() as i32 + 1)
}

/// Lay out `text`: `None` for an empty string or a wrap of more than 63
/// lines. The lines are wrapped at 200 pixels in the font; a line that
/// ends at a break after a space loses the space, one after a hyphen keeps
/// it, and the last line keeps a trailing space only when there are two or
/// more lines. W is the widest line, H the line count times the pitch, and
/// an outline adds 2 to both.
///
/// Safe rule: without the font (it never fails to load in the game) the
/// text is one line of width 0, so it still times out and still gates
/// taunts.
pub fn layout(text: &[u8], font: Option<&Font>, outline: bool) -> Option<Layout> {
    if text.is_empty() {
        return None;
    }
    let mut lines = match font {
        Some(font) => crate::ui::pipboy::wrap(text, WRAP_WIDTH, |code| {
            font.text_width(&[code], false) as i32
        })?,
        None => vec![text.to_vec()],
    };
    let count = lines.len();
    for (index, line) in lines.iter_mut().enumerate() {
        let last = index + 1 == count;
        if (!last || count == 1) && line.last() == Some(&b' ') {
            line.pop();
        }
    }
    let width = font.map_or(0, |font| {
        lines
            .iter()
            .map(|line| font.text_width(line, false) as i32)
            .max()
            .unwrap_or(0)
    });
    let border = if outline { 2 } else { 0 };
    let height = (count as i32).saturating_mul(line_pitch(font));
    Some(Layout {
        lines,
        size: [width + border, height + border],
    })
}

/// The box's top-left corner for a hex at window position `hex` (the
/// top-left of its 32-by-16 cell): the first of the eight candidates whose
/// whole box lies inside the 640-by-380 window, else the first, clipped.
pub fn place(hex: [i32; 2], size: [i32; 2]) -> [i32; 2] {
    let [hx, hy] = hex;
    let [w, h] = size;
    let first = [hx + 16 - w / 2, hy - h - RISE];
    let second = [first[0] - w / 2, first[1]];
    let third = [second[0] + w, second[1]];
    let sixth = [hx + 16 - w / 2, hy];
    let seventh = [sixth[0] - w / 2, hy];
    let candidates = [
        first,
        second,
        third,
        [hx - 16 - w, hy - 16 - h],
        [hx + 48, hy - 16 - h],
        sixth,
        seventh,
        [seventh[0] + w, hy],
    ];
    candidates
        .into_iter()
        .find(|[x, y]| *x >= 0 && x + w <= WINDOW[0] && *y >= 0 && y + h <= WINDOW[1])
        .unwrap_or(first)
}

/// `float_msg` (SPEC-005 §3.5) style colors as RGB555 keys: style -2 and
/// 2 red, and so on; 0, 8 and any unrecognized style 32747.
pub fn style_color(style: i32) -> u16 {
    match style {
        -2 | 2 => 31744,
        1 | 5 | 10 => 10570,
        3 => 992,
        4 => 31,
        6 => 21140,
        7 => 32074,
        9 => 32767,
        11 => 8456,
        12 => 15855,
        _ => 32747,
    }
}

/// The style the shared cycle gives after `current` (2, 3, ... 12, 1, 2,
/// ...): stored style 1 at start, so the first -1 is style 2.
pub fn next_style(current: i32) -> i32 {
    current.rem_euclid(12) + 1
}

/// `float_msg` fonts: 103 for style -2, otherwise 101.
pub fn style_font(style: i32) -> i32 {
    if style == -2 { 103 } else { 101 }
}

/// A synthetic AAF for floating-text tests: height 10, letter spacing 1,
/// space width 3, no line spacing; every glyph 6 by 10 and fully covered.
/// Advances are 7, and 4 for a space; the line pitch is 11, as for font
/// 101.
#[cfg(test)]
pub(crate) fn test_font() -> Font {
    let mut bytes = b"AAFF".to_vec();
    for metric in [10_i16, 1, 3, 0] {
        bytes.extend(metric.to_be_bytes());
    }
    for _ in 0..256 {
        bytes.extend(6_i16.to_be_bytes());
        bytes.extend(10_i16.to_be_bytes());
        bytes.extend(0_i32.to_be_bytes());
    }
    bytes.extend([7; 60]);
    Font::parse_aaf(&bytes).expect("synthetic font")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn text(owner: ObjectRef, created: u64, lifetime: u64) -> FloatingText {
        FloatingText {
            owner,
            font: 101,
            lines: vec![b"x".to_vec()],
            size: [9, 13],
            color: Ink::Key(32747),
            outline: Some(Ink::Key(0)),
            hex: 20100,
            offset: [0, 0],
            created,
            lifetime,
            marked: false,
        }
    }

    #[test]
    fn spec014_fixture_u_layout_wraps_at_200_and_sizes_the_box() {
        let font = test_font();
        let font = Some(&font);
        // 173 pixels fit and the next word does not: two lines, the first
        // without its trailing space.
        let two = layout(b"Hello stranger. Welcome to Shady Sands.", font, true).unwrap();
        assert_eq!(
            two.lines,
            [
                b"Hello stranger. Welcome to".to_vec(),
                b"Shady Sands.".to_vec()
            ]
        );
        assert_eq!(two.size, [173 + 2, 2 * 11 + 2]);
        // Without an outline the box has no border.
        assert_eq!(layout(b"Die, scum!", font, false).unwrap().size, [67, 11]);
        assert_eq!(layout(b"Die, scum!", font, true).unwrap().size, [69, 13]);
        // A one-line text loses a trailing space; the last line of two or
        // more keeps it; a hyphen stays at a line's end.
        assert_eq!(
            layout(b"Die, scum! ", font, false).unwrap().lines,
            [b"Die, scum!".to_vec()]
        );
        let long = [&[b'a'; 20][..], b"-", &[b'b'; 10], b" "].concat();
        assert_eq!(
            layout(&long, font, false).unwrap().lines,
            [
                [&[b'a'; 20][..], b"-"].concat(),
                [&[b'b'; 10][..], b" "].concat()
            ]
        );
        // A line feed is a break point drawn as a glyph, not a forced break.
        assert_eq!(
            layout(b"a\nb", font, false).unwrap().lines,
            [b"a\nb".to_vec()]
        );
        // Refusals: an empty string, and a wrap of more than 63 lines.
        assert_eq!(layout(b"", font, true), None);
        assert_eq!(layout(&[b'a'; 2000], font, true), None);
        assert_eq!(
            layout(&[b'a'; 28 * 63], font, true).unwrap().lines.len(),
            63
        );
        // Safe rule without the font: one line of width 0.
        assert_eq!(layout(b"Hi", None, true).unwrap().size, [2, 13]);
    }

    #[test]
    fn spec014_fixture_u_placement_candidates() {
        // Candidate 1 fits: box at (239, 116), offset (-65, -84).
        assert_eq!(place([304, 200], [162, 24]), [239, 116]);
        // Candidates 1-3 fail (y = -34, then x + W = 778); 4 fits.
        assert_eq!(place([600, 50], [162, 24]), [422, 10]);
        // Candidate 5, right of the hex.
        assert_eq!(place([20, 50], [60, 13]), [68, 21]);
        // Candidate 6, below the hex.
        assert_eq!(place([304, 5], [162, 24]), [239, 5]);
        // None fits: candidate 1, clipped.
        assert_eq!(place([304, 200], [700, 24]), [-30, 116]);
        // The text stays over its creation hex wherever that hex scrolls.
        let floating = FloatingText {
            offset: [-65, -84],
            ..text(1, 0, 0)
        };
        assert_eq!(floating.origin([304, 200]), [239, 116]);
        assert_eq!(floating.origin([336, 212]), [271, 128]);
    }

    #[test]
    fn spec014_fixture_u_lifetimes_and_styles() {
        assert_eq!(lifetime(3.5, 2), 5500);
        assert_eq!(lifetime(3.5, 1), 4500);
        assert_eq!(lifetime(1.0, 3), 1000);
        assert_eq!(lifetime(6.0, 1), 8000);
        // Style -1 three times from the start of the session: 2, 3, 4.
        let mut style = 1;
        let mut colors = Vec::new();
        for _ in 0..3 {
            style = next_style(style);
            colors.push(style_color(style));
        }
        assert_eq!(colors, [31744, 992, 31]);
        assert_eq!(next_style(12), 1);
        assert_eq!((style_font(13), style_color(13)), (101, 32747));
        assert_eq!((style_font(-2), style_color(-2)), (103, 31744));
        assert_eq!(line_pitch(Some(&test_font())), 11);
    }

    #[test]
    fn spec014_fixture_u_replacement_limit_and_removal() {
        let mut floating = FloatingTexts::default();
        assert!(floating.push(text(1, 0, 5500)));
        // A new text of the same owner marks the old one: it is still drawn
        // and counted until the next background update.
        assert!(floating.push(text(1, 100, 5500)));
        assert_eq!(floating.len(), 2);
        assert!(floating.texts[0].marked && !floating.texts[1].marked);
        assert!(floating.update(100));
        assert_eq!(floating.texts.len(), 1);
        assert_eq!(floating.texts[0].created, 100);
        // Removed at the first update strictly after its lifetime.
        assert!(!floating.update(5600));
        assert!(floating.update(5601));
        assert!(floating.is_empty());
        // 19 texts: a new creation is refused, and an earlier text of the
        // same owner stays.
        for owner in 0..19 {
            assert!(floating.push(text(owner, 0, 5500)));
        }
        assert!(!floating.push(text(3, 0, 5500)));
        assert_eq!(floating.len(), 19);
        assert!(floating.texts.iter().all(|text| !text.marked));
        floating.clear();
        assert!(floating.is_empty());
    }
}
