//! Raw byte messages; no Unicode conversion, escapes, or comment-stripping pass.
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

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct MsgCatalog {
    messages: BTreeMap<i32, Message>,
    warnings: Vec<ParseError>,
    warning_count: usize,
}

const MAX_RETAINED_WARNINGS: usize = 128;

fn decimal_syntax(id: &[u8]) -> bool {
    let sign = usize::from(id.first().is_some_and(|byte| matches!(byte, b'+' | b'-')));
    !id.is_empty() && id[sign..].iter().all(u8::is_ascii_digit)
}

fn message_id(id: &[u8], offset: usize, compatible: bool) -> Result<i32, ParseError> {
    let id = if compatible { id.trim_ascii() } else { id };
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
    compatible: bool,
}

impl Scanner<'_> {
    /// A recovery boundary is an indented decimal ID followed by another opening
    /// field on the same line. A numeric sound field on its own line is not an ID.
    fn record_at(&self, position: usize) -> bool {
        if self.bytes.get(position) != Some(&b'{') {
            return false;
        }
        let mut before = position;
        while before > 0 && matches!(self.bytes[before - 1], b' ' | b'\t' | b'\r') {
            before -= 1;
        }
        if before != 0 && self.bytes[before - 1] != b'\n' {
            return false;
        }
        let tail = &self.bytes[position + 1..];
        let Some(end) = tail
            .iter()
            .take(1024)
            .position(|byte| matches!(byte, b'}' | b'\n'))
        else {
            return false;
        };
        tail[end] == b'}'
            && decimal_syntax(tail[..end].trim_ascii())
            && tail[end + 1..]
                .iter()
                .find(|byte| !matches!(byte, b' ' | b'\t' | b'\r'))
                == Some(&b'{')
    }

    fn recover(&mut self) {
        // Always advance beyond the failed record, even if a missing closing brace
        // made its scan encounter the beginning of the following valid record.
        self.position = (self.record_start + 1..self.bytes.len())
            .find(|position| self.record_at(*position))
            .unwrap_or(self.bytes.len());
    }

    fn field(&mut self, required: bool) -> Result<Option<(usize, Vec<u8>)>, ParseError> {
        while let Some(&byte) = self.bytes.get(self.position) {
            let start = self.position;
            if self.compatible && required && self.record_at(start) {
                return Err(ParseError::new(
                    "MSG",
                    start,
                    "missing field before the next record",
                ));
            }
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
                if self.compatible && self.record_at(self.position) {
                    return Err(ParseError::new(
                        "MSG",
                        start,
                        "unterminated field before the next record",
                    ));
                }
                self.position += 1;
                match byte {
                    b'}' => {
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

    fn record(&mut self) -> Result<Option<(i32, Message)>, ParseError> {
        let Some((offset, id)) = self.field(false)? else {
            return Ok(None);
        };
        let id = message_id(&id, offset, self.compatible)?;
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
    /// Corpus-compatible loading: trim IDs and skip malformed records with warnings.
    /// File-size and record-count resource limits still return errors.
    pub fn parse(bytes: &[u8]) -> Result<Self, ParseError> {
        Self::parse_with_mode(bytes, TextMode::Archive)
    }
    pub fn parse_with_mode(bytes: &[u8], mode: TextMode) -> Result<Self, ParseError> {
        let mut catalog = Self::default();
        catalog.load(bytes, mode)?;
        Ok(catalog)
    }
    /// Merge records in stream order. Complete prior records survive a later error.
    pub fn load(&mut self, bytes: &[u8], mode: TextMode) -> Result<(), ParseError> {
        self.load_with_policy(bytes, mode, true)
    }

    /// Original strict SPEC-002 grammar, useful for authoring and diagnostics.
    pub fn parse_strict(bytes: &[u8]) -> Result<Self, ParseError> {
        let mut catalog = Self::default();
        catalog.load_strict(bytes, TextMode::Archive)?;
        Ok(catalog)
    }

    pub fn load_strict(&mut self, bytes: &[u8], mode: TextMode) -> Result<(), ParseError> {
        self.load_with_policy(bytes, mode, false)
    }

    fn load_with_policy(
        &mut self,
        bytes: &[u8],
        mode: TextMode,
        compatible: bool,
    ) -> Result<(), ParseError> {
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
            compatible,
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
            let (id, message) = match scanner.record() {
                Ok(Some(record)) => record,
                Ok(None) => break,
                Err(error) if compatible => {
                    self.warning_count = self.warning_count.saturating_add(1);
                    if self.warnings.len() < MAX_RETAINED_WARNINGS {
                        self.warnings.push(error);
                    }
                    scanner.recover();
                    continue;
                }
                Err(error) => return Err(error),
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
    pub fn get(&self, id: i32) -> Option<&Message> {
        self.messages.get(&id)
    }
    pub fn entries(&self) -> &BTreeMap<i32, Message> {
        &self.messages
    }
    /// First 128 recoverable diagnostics, with original input byte offsets.
    pub fn warnings(&self) -> &[ParseError] {
        &self.warnings
    }
    pub fn warning_count(&self) -> usize {
        self.warning_count
    }
}
