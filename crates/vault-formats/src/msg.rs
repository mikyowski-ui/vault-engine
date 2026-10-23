//! Raw byte messages; no Unicode conversion, escapes, or comment-stripping pass.
//!
//! Loading follows SPEC-002 §3.4-§3.5 exactly: the scanner reads brace
//! fields three at a time, and the first malformed record, missing field,
//! delimiter error or resource limit ends the load. The records completed
//! before it remain loaded (there is no rollback). There is no recovery or
//! resynchronization past a bad record: the reference never loads the
//! records after one, so neither does the engine.
use crate::{
    ParseError,
    binary::{MAX_ENTRIES, Reader},
};
use std::collections::BTreeMap;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum TextMode {
    /// Raw/archive semantics, including bytes after 0x1A. CRLF is still translated.
    #[default]
    Archive,
    /// Original Windows loose text streams treat 0x1A as EOF.
    WindowsLoose,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Message {
    pub sound: Vec<u8>,
    pub text: Vec<u8>,
}

/// Records by numeric ID, plus the failure (if any) that ended the load
/// that built the catalog.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct MsgCatalog {
    messages: BTreeMap<i32, Message>,
    failure: Option<ParseError>,
}

fn decimal_syntax(id: &[u8]) -> bool {
    let sign = usize::from(id.first().is_some_and(|byte| matches!(byte, b'+' | b'-')));
    !id.is_empty() && id[sign..].iter().all(u8::is_ascii_digit)
}

/// SPEC-002 §3.5 ID validation: a nonempty optional sign plus decimal
/// digits, never trimmed (whitespace makes the ID invalid), leading zeroes
/// decimal, a sign alone the legacy ID 0. Bytes 0x80-0xFF are non-digits
/// and an ID outside i32 is rejected (both `[May-improve]`).
fn message_id(id: &[u8], offset: usize) -> Result<i32, ParseError> {
    if !decimal_syntax(id) {
        return Err(ParseError::new(
            "MSG",
            offset,
            "invalid or empty decimal message ID",
        ));
    }
    if matches!(id, b"+" | b"-") {
        return Ok(0);
    }
    std::str::from_utf8(id)
        .expect("validated ASCII digits")
        .parse()
        .map_err(|_| ParseError::new("MSG", offset, "message ID exceeds i32"))
}

struct Scanner<'a> {
    bytes: &'a [u8],
    position: usize,
    record_start: usize,
}

impl Scanner<'_> {
    /// SPEC-002 §3.4: seeking a field discards bytes until an opening brace
    /// (a closing brace first is a delimiter error; end of input is "no
    /// field"); inside a field every byte through the first closing brace is
    /// retained except line feeds, with the LF of a CRLF pair removed and a
    /// bare CR kept. End of input inside a field is an error.
    fn field(&mut self, required: bool) -> Result<Option<(usize, Vec<u8>)>, ParseError> {
        while let Some(&byte) = self.bytes.get(self.position) {
            let start = self.position;
            self.position += 1;
            if byte == b'}' {
                if !required {
                    self.record_start = start;
                }
                return Err(ParseError::new("MSG", start, "unmatched closing brace"));
            }
            if byte != b'{' {
                continue;
            }
            if !required {
                self.record_start = start;
            }
            let mut payload = Vec::new();
            while let Some(&byte) = self.bytes.get(self.position) {
                self.position += 1;
                match byte {
                    b'}' => {
                        // §3.5: storage and ID conversion see only the prefix
                        // before an embedded zero; scanning still consumed
                        // through the closing brace.
                        if let Some(nul) = payload.iter().position(|byte| *byte == 0) {
                            payload.truncate(nul);
                        }
                        return Ok(Some((start, payload)));
                    }
                    b'\n' => continue,
                    b'\r' if self.bytes.get(self.position) == Some(&b'\n') => {
                        self.position += 1;
                        continue;
                    }
                    _ => {}
                }
                // §3.5 [May-improve]: more than 1023 retained bytes is a load
                // failure instead of the reference's out-of-bounds write.
                if payload.len() == 1023 {
                    return Err(ParseError::new(
                        "MSG",
                        self.position - 1,
                        "field exceeds 1023 retained bytes",
                    ));
                }
                payload.push(byte);
            }
            return Err(ParseError::new("MSG", start, "unterminated field"));
        }
        Ok(None)
    }

    /// One record: ID, sound, text. End of input before an ID is the
    /// successful end; before a sound or text field it is a failure (§3.5).
    fn record(&mut self) -> Result<Option<(i32, Message)>, ParseError> {
        let Some((offset, id)) = self.field(false)? else {
            return Ok(None);
        };
        let id = message_id(&id, offset)?;
        let (_, sound) = self
            .field(true)?
            .ok_or_else(|| ParseError::new("MSG", self.position, "missing sound field"))?;
        let (_, text) = self
            .field(true)?
            .ok_or_else(|| ParseError::new("MSG", self.position, "missing text field"))?;
        Ok(Some((id, Message { sound, text })))
    }
}

impl MsgCatalog {
    /// Runtime loading. A failure ends the load with the earlier records
    /// kept (SPEC-002 §3.5: no rollback) and is reported by [`failure`];
    /// `Err` only when the input itself exceeds the asset size limit.
    ///
    /// [`failure`]: MsgCatalog::failure
    pub fn parse(bytes: &[u8]) -> Result<Self, ParseError> {
        Self::parse_with_mode(bytes, TextMode::Archive)
    }
    pub fn parse_with_mode(bytes: &[u8], mode: TextMode) -> Result<Self, ParseError> {
        Reader::new(bytes, "MSG")?;
        let mut catalog = Self::default();
        catalog.failure = catalog.load(bytes, mode).err();
        Ok(catalog)
    }
    /// Merge records in stream order into this catalog (§3.5: loading into
    /// an existing list merges rather than clearing it). On failure the
    /// records completed before it are already merged and the error is
    /// returned; it is not recorded for [`failure`].
    ///
    /// [`failure`]: MsgCatalog::failure
    pub fn load(&mut self, bytes: &[u8], mode: TextMode) -> Result<(), ParseError> {
        Reader::new(bytes, "MSG")?;
        let end = if mode == TextMode::WindowsLoose {
            bytes
                .iter()
                .position(|byte| *byte == 0x1a)
                .unwrap_or(bytes.len())
        } else {
            bytes.len()
        };
        let mut scanner = Scanner {
            bytes: &bytes[..end],
            position: 0,
            record_start: 0,
        };
        let mut count = 0;
        while scanner.position < scanner.bytes.len() {
            count += 1;
            if count > MAX_ENTRIES {
                return Err(ParseError::new(
                    "MSG",
                    scanner.position,
                    "record limit exceeded",
                ));
            }
            let Some((id, message)) = scanner.record()? else {
                break;
            };
            if self.messages.len() == MAX_ENTRIES && !self.messages.contains_key(&id) {
                return Err(ParseError::new(
                    "MSG",
                    scanner.record_start,
                    "catalog entry limit exceeded",
                ));
            }
            self.messages.insert(id, message);
        }
        Ok(())
    }

    /// The same grammar as [`parse`], surfacing a load failure as `Err`
    /// (the partial catalog is discarded): for authoring tools and asset
    /// validation.
    ///
    /// [`parse`]: MsgCatalog::parse
    pub fn parse_strict(bytes: &[u8]) -> Result<Self, ParseError> {
        let mut catalog = Self::default();
        catalog.load(bytes, TextMode::Archive)?;
        Ok(catalog)
    }
    /// The same as [`load`].
    ///
    /// [`load`]: MsgCatalog::load
    pub fn load_strict(&mut self, bytes: &[u8], mode: TextMode) -> Result<(), ParseError> {
        self.load(bytes, mode)
    }

    pub fn get(&self, id: i32) -> Option<&Message> {
        self.messages.get(&id)
    }
    pub fn entries(&self) -> &BTreeMap<i32, Message> {
        &self.messages
    }
    /// The failure that ended the [`parse`] that built this catalog, with
    /// its input byte offset; `None` when the whole input loaded.
    ///
    /// [`parse`]: MsgCatalog::parse
    pub fn failure(&self) -> Option<&ParseError> {
        self.failure.as_ref()
    }
    /// [`failure`] as a slice (empty or one entry).
    ///
    /// [`failure`]: MsgCatalog::failure
    pub fn warnings(&self) -> &[ParseError] {
        self.failure.as_slice()
    }
    pub fn warning_count(&self) -> usize {
        self.warnings().len()
    }
    pub fn is_empty(&self) -> bool {
        self.messages.is_empty()
    }
    /// SPEC-002 §3.7: rewrite every record's text in place, in ascending ID
    /// order, from cursor `k` (the caller's one `random(1, 29)` draw minus
    /// 1); returns the cursor after the application. Sound fields and IDs
    /// are never filtered.
    pub fn apply_language_filter(&mut self, words: &BadWords, mut k: usize) -> usize {
        k %= FILTER_ALPHABET.len();
        for message in self.messages.values_mut() {
            k = words.filter_text(&mut message.text, k);
        }
        k
    }
}

/// SPEC-002 §3.7 language-filter replacement alphabet.
pub const FILTER_ALPHABET: &[u8; 29] = b"!@#$%&*@#*!&$%#&%#*%!$&%@*$@&";
/// Bytes per `badwords.txt` read: a longer line becomes several entries.
const BAD_WORD_CHUNK: usize = 78;

/// SPEC-002 §3.7 word list, `data/badwords.txt`: case-folded entries in
/// file order.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct BadWords {
    entries: Vec<Vec<u8>>,
}

/// ASCII letters only; the end of the text and bytes `0x80`-`0xFF` are
/// non-letters (§3.7 `[May-improve]`).
fn filter_letter(byte: Option<&u8>) -> bool {
    byte.is_some_and(u8::is_ascii_alphabetic)
}

impl BadWords {
    /// Text-mode lines (CRLF becomes LF, one trailing LF per line removed),
    /// read in chunks of at most 78 bytes, untrimmed, without comments, and
    /// case-folded (`a`-`z` only). A final line without LF is still an
    /// entry. `[May-improve]`: zero-length entries, which make the
    /// reference filter loop without end, are ignored.
    pub fn parse(bytes: &[u8]) -> Self {
        let mut text = Vec::with_capacity(bytes.len());
        for (index, &byte) in bytes.iter().enumerate() {
            if byte != b'\r' || bytes.get(index + 1) != Some(&b'\n') {
                text.push(byte);
            }
        }
        let mut entries = Vec::new();
        let mut start = 0;
        while start < text.len() {
            let chunk = &text[start..text.len().min(start + BAD_WORD_CHUNK)];
            let (entry, consumed) = match chunk.iter().position(|byte| *byte == b'\n') {
                Some(end) => (&chunk[..end], end + 1),
                None => (chunk, chunk.len()),
            };
            start += consumed;
            if !entry.is_empty() {
                entries.push(entry.to_ascii_uppercase());
            }
        }
        Self { entries }
    }
    pub fn entries(&self) -> &[Vec<u8>] {
        &self.entries
    }
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
    /// One §3.7 pass over `text` from cursor `k` (0-28), returning the
    /// cursor after it. Matching uses an unchanged case-folded copy, so
    /// every occurrence is judged against the original text. After each
    /// find the search restarts one byte after its previous start (not
    /// after the find), so a qualifying occurrence is rewritten once per
    /// start position up to it, using L cursor positions each time, and
    /// the last rewrite's bytes remain.
    pub fn filter_text(&self, text: &mut [u8], mut k: usize) -> usize {
        let folded = text.to_ascii_uppercase();
        for word in &self.entries {
            let mut from = 0;
            while let Some(found) = folded
                .get(from..)
                .and_then(|rest| rest.windows(word.len()).position(|w| w == word.as_slice()))
            {
                let start = from + found;
                let end = start + word.len();
                // An occurrence at offset 0 qualifies without testing the
                // byte after it.
                if start == 0
                    || !filter_letter(folded.get(start - 1)) && !filter_letter(folded.get(end))
                {
                    // The searches from `from` through `start` all find
                    // this occurrence; only the last rewrite stays visible.
                    let skipped = (start - from) * word.len();
                    k = (k + skipped) % FILTER_ALPHABET.len();
                    for byte in &mut text[start..end] {
                        *byte = FILTER_ALPHABET[k];
                        k = (k + 1) % FILTER_ALPHABET.len();
                    }
                }
                from = start + 1;
            }
        }
        k
    }
}
