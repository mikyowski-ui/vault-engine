//! SPEC-002 byte-code fonts. There is spacing, but no pair-kerning table.
use crate::{ParseError, binary::Reader};

const MAX_DIMENSION: usize = 4096;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FontKind {
    Bitmap,
    Antialiased,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Glyph {
    pub width: u32,
    pub height: u32,
    pub offset: usize,
}

#[derive(Debug, Clone)]
pub struct Font {
    kind: FontKind,
    height: u32,
    spacing: u32,
    space_width: u32,
    line_spacing: u32,
    glyphs: Vec<Glyph>,
    data: Vec<u8>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PositionedGlyph {
    pub code: u8,
    pub x: i64,
    pub y: u32,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TextLayout {
    pub width: u32,
    pub glyphs: Vec<PositionedGlyph>,
}

fn metric(reader: &Reader<'_>, value: i32, name: &str) -> Result<u32, ParseError> {
    let value = reader.nonnegative(value, name)?;
    if value > MAX_DIMENSION {
        return Err(reader.error(format!("{name} exceeds {MAX_DIMENSION} pixels")));
    }
    Ok(value as u32)
}

impl Font {
    pub fn parse_fon(bytes: &[u8]) -> Result<Self, ParseError> {
        let mut reader = Reader::new(bytes, "FON")?;
        let count = reader.le_i32()?;
        if !(1..=256).contains(&count) {
            return Err(reader.error("glyph count must be 1..=256"));
        }
        let height = reader.le_i32()?;
        let height = metric(&reader, height, "font height")?;
        let spacing = reader.le_i32()?;
        let spacing = metric(&reader, spacing, "letter spacing")?;
        if height == 0 {
            return Err(reader.error("font height must be positive"));
        }
        reader.take(8)?; // Opaque words, never addresses.
        let mut glyphs = Vec::new();
        for _ in 0..count {
            let width = reader.le_i32()?;
            let width = metric(&reader, width, "glyph width")?;
            let offset = reader.le_i32()?;
            let offset = reader.nonnegative(offset, "glyph offset")?;
            glyphs.push(Glyph {
                width,
                height,
                offset,
            });
        }
        let last = glyphs.last().expect("validated positive count");
        let length = last
            .offset
            .checked_add(height as usize * last.width.div_ceil(8) as usize)
            .ok_or_else(|| reader.error("bitmap length overflow"))?;
        let origin = reader.pos;
        let data = reader.take(length)?;
        Self::validate_extents(&reader, &glyphs, data, origin, FontKind::Bitmap)?;
        Ok(Self {
            kind: FontKind::Bitmap,
            height,
            spacing,
            space_width: 0,
            line_spacing: 0,
            glyphs,
            data: data.to_vec(),
        })
    }

    pub fn parse_aaf(bytes: &[u8]) -> Result<Self, ParseError> {
        let mut reader = Reader::new(bytes, "AAF")?;
        if reader.take(4)? != b"AAFF" {
            return Err(ParseError::new("AAF", 0, "expected AAFF signature"));
        }
        let height = reader.be_i16()?;
        let height = metric(&reader, i32::from(height), "font height")?;
        let spacing = reader.be_i16()?;
        let spacing = metric(&reader, i32::from(spacing), "letter spacing")?;
        let space_width = reader.be_i16()?;
        let space_width = metric(&reader, i32::from(space_width), "space width")?;
        let line_spacing = reader.be_i16()?;
        let line_spacing = metric(&reader, i32::from(line_spacing), "line spacing")?;
        if height == 0 {
            return Err(reader.error("font height must be positive"));
        }
        let mut glyphs = Vec::new();
        for _ in 0..256 {
            let width = reader.be_i16()?;
            let width = metric(&reader, i32::from(width), "glyph width")?;
            let glyph_height = reader.be_i16()?;
            let glyph_height = metric(&reader, i32::from(glyph_height), "glyph height")?;
            if glyph_height > height {
                return Err(reader.error("glyph height exceeds font height"));
            }
            let offset = reader.be_i32()?;
            let offset = reader.nonnegative(offset, "glyph offset")?;
            glyphs.push(Glyph {
                width,
                height: glyph_height,
                offset,
            });
        }
        let origin = reader.pos;
        let data = &bytes[origin..];
        if data.is_empty() {
            return Err(reader.error("missing bitmap data"));
        }
        Self::validate_extents(&reader, &glyphs, data, origin, FontKind::Antialiased)?;
        Ok(Self {
            kind: FontKind::Antialiased,
            height,
            spacing,
            space_width,
            line_spacing,
            glyphs,
            data: data.to_vec(),
        })
    }

    fn validate_extents(
        reader: &Reader<'_>,
        glyphs: &[Glyph],
        data: &[u8],
        origin: usize,
        kind: FontKind,
    ) -> Result<(), ParseError> {
        let mut ranges = Vec::new();
        for (code, glyph) in glyphs.iter().enumerate() {
            let stride = if kind == FontKind::Bitmap {
                glyph.width.div_ceil(8)
            } else {
                glyph.width
            };
            let size = stride as usize * glyph.height as usize;
            if size == 0 {
                continue;
            }
            let end = glyph
                .offset
                .checked_add(size)
                .ok_or_else(|| reader.error("glyph extent overflow"))?;
            if end > data.len() {
                return Err(reader.error(format!(
                    "glyph {code:#04x} extends beyond the loaded bitmap block"
                )));
            }
            ranges.push((glyph.offset, end));
        }
        if kind == FontKind::Antialiased {
            // Scan overlapping/shared buffers once, leaving unreferenced bytes alone.
            ranges.sort_unstable();
            let mut checked = 0;
            for (start, end) in ranges {
                let start = start.max(checked);
                if start < end {
                    if let Some(offset) = data[start..end].iter().position(|level| *level >= 14) {
                        return Err(ParseError::new(
                            "AAF",
                            origin + start + offset,
                            "undefined coverage level (expected 0..=13)",
                        ));
                    }
                    checked = end;
                }
            }
        }
        Ok(())
    }

    pub fn kind(&self) -> FontKind {
        self.kind
    }
    pub fn height(&self) -> u32 {
        self.height
    }
    pub fn line_height(&self) -> u32 {
        self.height + self.line_spacing
    }
    pub fn spacing(&self) -> u32 {
        self.spacing
    }
    pub fn glyph_count(&self) -> usize {
        self.glyphs.len()
    }
    pub fn bitmap_bytes(&self) -> &[u8] {
        &self.data
    }
    pub fn glyph(&self, code: u8) -> Option<&Glyph> {
        self.glyphs.get(usize::from(code))
    }
    pub fn character_width(&self, code: u8) -> Option<u32> {
        self.glyph(code).map(|glyph| {
            if self.kind == FontKind::Antialiased && code == b' ' {
                self.space_width
            } else {
                glyph.width
            }
        })
    }
    pub fn max_cell_width(&self) -> u32 {
        match self.kind {
            FontKind::Bitmap => {
                self.glyphs
                    .iter()
                    .map(|glyph| glyph.width)
                    .max()
                    .unwrap_or(0)
                    + self.spacing
            }
            // H + S; SPEC-002 §3.1 safe rule for a zero or negative Q, which
            // the reference measures as H + L.
            FontKind::Antialiased => self.height + self.spacing,
        }
    }
    /// Stop at NUL; high-bit codes are handled safely as unsigned byte indices.
    pub fn text_width(&self, text: &[u8], monospaced: bool) -> u64 {
        text.iter()
            .take_while(|code| **code != 0)
            .fold(0_u64, |width, code| {
                let advance = if monospaced {
                    self.max_cell_width()
                } else {
                    self.character_width(*code)
                        .map_or(0, |width| width + self.spacing)
                };
                width.saturating_add(u64::from(advance))
            })
    }
    pub fn buffer_size(&self, text: &[u8], monospaced: bool) -> u64 {
        self.text_width(text, monospaced)
            .saturating_mul(u64::from(self.line_height()))
    }
    /// Complete advances must fit. Unsupported FON codes do not draw or advance.
    pub fn layout(&self, text: &[u8], limit: u32, monospaced: bool) -> TextLayout {
        let mut layout = TextLayout {
            width: 0,
            glyphs: Vec::new(),
        };
        for code in text.iter().copied().take_while(|code| *code != 0) {
            let Some(glyph) = self.glyph(code) else {
                continue;
            };
            let width = self.character_width(code).expect("existing glyph");
            let advance = if monospaced {
                self.max_cell_width()
            } else {
                width + self.spacing
            };
            let end = u64::from(layout.width) + u64::from(advance);
            if end > u64::from(limit) {
                break;
            }
            let offset = if monospaced {
                (i64::from(advance) - i64::from(width) - i64::from(self.spacing)) / 2
            } else {
                0
            };
            layout.glyphs.push(PositionedGlyph {
                code,
                x: i64::from(layout.width) + offset,
                y: self.height - glyph.height,
            });
            layout.width = end as u32;
        }
        layout
    }
    /// Bitmap bits become coverage 0/7; AAF returns its original level, including 8..13.
    pub fn coverage(&self, code: u8, x: u32, y: u32) -> Option<u8> {
        let glyph = self.glyph(code)?;
        if x >= glyph.width || y >= glyph.height {
            return None;
        }
        match self.kind {
            FontKind::Bitmap => {
                let byte = self.data[glyph.offset
                    + y as usize * glyph.width.div_ceil(8) as usize
                    + (x / 8) as usize];
                Some(if byte & (0x80 >> (x % 8)) != 0 { 7 } else { 0 })
            }
            FontKind::Antialiased => {
                Some(self.data[glyph.offset + y as usize * glyph.width as usize + x as usize])
            }
        }
    }
}

/// FON startup search suffixes; selection numbers equal the suffix.
pub const FON_SUFFIXES: std::ops::RangeInclusive<u8> = 0..=9;
/// AAF startup search suffixes.
pub const AAF_SUFFIXES: std::ops::RangeInclusive<u8> = 0..=15;
/// Registered AAF selection numbers; number `100 + n` selects `font<n>.aaf`.
pub const AAF_SELECTION: std::ops::RangeInclusive<i32> = 100..=110;

/// SPEC-002 §3.1 font subsystems. Fonts resolve at the asset root. Each
/// subsystem defaults to its lowest successfully loaded suffix; an absent or
/// invalid file is never synthesized from another one.
#[derive(Debug, Clone, Default)]
pub struct FontSet {
    fon: std::collections::BTreeMap<u8, std::sync::Arc<Font>>,
    aaf: std::collections::BTreeMap<u8, std::sync::Arc<Font>>,
    current: Option<i32>,
}

impl FontSet {
    /// Try every startup suffix; files that are absent or fail to parse are
    /// simply not loaded.
    pub fn load(vfs: &dyn crate::Vfs) -> Self {
        let present: std::collections::BTreeSet<&str> =
            vfs.files().map(crate::VfsPath::as_str).collect();
        let mut set = Self::default();
        let load = |name: String, parse: fn(&[u8]) -> Result<Font, ParseError>| {
            present
                .contains(name.as_str())
                .then(|| vfs.read(&name).ok())
                .flatten()
                .filter(|bytes| bytes.len() <= crate::MAX_ASSET_BYTES)
                .and_then(|bytes| parse(&bytes).ok())
                .map(std::sync::Arc::new)
        };
        for suffix in FON_SUFFIXES {
            if let Some(font) = load(format!("font{suffix}.fon"), Font::parse_fon) {
                set.fon.insert(suffix, font);
            }
        }
        for suffix in AAF_SUFFIXES {
            if let Some(font) = load(format!("font{suffix}.aaf"), Font::parse_aaf) {
                set.aaf.insert(suffix, font);
            }
        }
        set
    }
    pub fn insert(&mut self, kind: FontKind, suffix: u8, font: Font) {
        let fonts = match kind {
            FontKind::Bitmap => &mut self.fon,
            FontKind::Antialiased => &mut self.aaf,
        };
        fonts.insert(suffix, std::sync::Arc::new(font));
    }
    /// The subsystem's initial choice, its lowest loaded suffix; `None`
    /// reports that subsystem's initialization failure.
    pub fn default_font(&self, kind: FontKind) -> Option<(u8, &std::sync::Arc<Font>)> {
        match kind {
            FontKind::Bitmap => &self.fon,
            FontKind::Antialiased => &self.aaf,
        }
        .iter()
        .next()
        .map(|(suffix, font)| (*suffix, font))
    }
    /// A loaded font by selection number: FON 0-9, or registered AAF 100-110.
    pub fn get(&self, number: i32) -> Option<&std::sync::Arc<Font>> {
        if let Ok(suffix) = u8::try_from(number)
            && FON_SUFFIXES.contains(&suffix)
        {
            self.fon.get(&suffix)
        } else if AAF_SELECTION.contains(&number) {
            self.aaf.get(&((number - 100) as u8))
        } else {
            None
        }
    }
    /// Select a font number. Under the §3.1 safe rule, selecting an unloaded
    /// number is a no-op: the current number, subsystem, bitmap, and metrics
    /// stay unchanged (the reference switches subsystem and reported number
    /// inside a registered range), and `false` is returned.
    pub fn select(&mut self, number: i32) -> bool {
        let available = self.get(number).is_some();
        if available {
            self.current = Some(number);
        }
        available
    }
    pub fn current_number(&self) -> Option<i32> {
        self.current
    }
    pub fn current(&self) -> Option<&std::sync::Arc<Font>> {
        self.current.and_then(|number| self.get(number))
    }
}

/// Numeric text-drawing word: low eight bits select the foreground palette
/// index; `0x10000` shadow, `0x20000` underline, `0x40000` monospaced.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TextFlags {
    pub foreground: u8,
    pub shadow: bool,
    pub underline: bool,
    pub monospaced: bool,
}
impl TextFlags {
    pub const SHADOW: u32 = 0x10000;
    pub const UNDERLINE: u32 = 0x20000;
    pub const MONOSPACED: u32 = 0x40000;
    pub fn from_word(word: u32) -> Self {
        Self {
            foreground: (word & 0xff) as u8,
            shadow: word & Self::SHADOW != 0,
            underline: word & Self::UNDERLINE != 0,
            monospaced: word & Self::MONOSPACED != 0,
        }
    }
}
