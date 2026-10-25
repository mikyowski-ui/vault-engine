//! SPEC-018 §3.3-§3.5: the sectioned key/value grammar of the Fallout 2
//! data tables, the value forms, and the line grammar of §4.6.
//!
//! Text is read as bytes and kept as single-byte text: each byte becomes the
//! `char` of the same value (bytes `0x80`-`0xFF` included), so no Unicode
//! decoding is done and [`to_bytes`] gives the bytes back.
use std::collections::BTreeMap;

/// SPEC-015's member limit, which bounds a table file.
pub const MAX_FILE: usize = crate::MAX_ASSET_BYTES;
/// §3.3 rule 2: the longest accepted line.
pub const MAX_LINE: usize = 4096;

/// A diagnostic that never stops play (§3.5): the file, the line (1-based;
/// 0 for the file as a whole) and what was dropped or defaulted.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Diagnostic {
    pub file: String,
    pub line: usize,
    pub message: String,
}

impl std::fmt::Display for Diagnostic {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{} line {}: {}", self.file, self.line, self.message)
    }
}

/// A refused file (§3.5 "File refused"): Play stops, naming the file and
/// the line.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("{file} line {line}: {message}")]
pub struct TableError {
    pub file: String,
    pub line: usize,
    pub message: String,
}

impl TableError {
    pub fn new(file: &str, line: usize, message: impl Into<String>) -> Self {
        Self {
            file: file.to_owned(),
            line,
            message: message.into(),
        }
    }
}

/// Where a table's diagnostics are collected.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Diagnostics(pub Vec<Diagnostic>);

impl Diagnostics {
    pub fn push(&mut self, file: &str, line: usize, message: impl Into<String>) {
        self.0.push(Diagnostic {
            file: file.to_owned(),
            line,
            message: message.into(),
        });
    }
}

/// Single-byte text back to its bytes (see the module comment).
pub fn to_bytes(text: &str) -> Vec<u8> {
    text.chars().map(|c| c as u32 as u8).collect()
}

fn latin1(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| char::from(*byte)).collect()
}

/// §3.3 rule 4: space, tab and CR are trimmed.
fn trim(text: &str) -> &str {
    text.trim_matches([' ', '\t', '\r'])
}

/// §3.3 rules 1-2: the lines of a file, numbered from 1, with the file's
/// structural faults refused.
fn lines<'a>(file: &str, bytes: &'a [u8]) -> Result<Vec<(usize, &'a [u8])>, TableError> {
    if bytes.len() > MAX_FILE {
        return Err(TableError::new(file, 0, "larger than 64 MiB"));
    }
    let mut out = Vec::new();
    let mut parts: Vec<&[u8]> = bytes.split(|byte| *byte == b'\n').collect();
    // A final LF ends the last line; it does not start an empty one.
    if bytes.ends_with(b"\n") {
        parts.pop();
    }
    for (index, line) in parts.into_iter().enumerate() {
        let number = index + 1;
        if line.contains(&0) {
            return Err(TableError::new(file, number, "a NUL byte"));
        }
        if line.len() > MAX_LINE {
            return Err(TableError::new(
                file,
                number,
                format!("a line of {} bytes, over {MAX_LINE}", line.len()),
            ));
        }
        out.push((number, line));
    }
    Ok(out)
}

/// The text of a line before its first comment marker, trimmed.
fn content(line: &[u8], markers: &[u8]) -> String {
    let end = line
        .iter()
        .position(|byte| markers.contains(byte))
        .unwrap_or(line.len());
    trim(&latin1(&line[..end])).to_owned()
}

/// A name or key folded for comparison, with §3.3 rule 11's numbered forms
/// made canonical: `Map 012` and `map 12` are `map 12`, `enc_07` and
/// `enc_7` are `enc_7`.
fn canonical_section(name: &str) -> String {
    let folded = name.to_ascii_lowercase();
    if let Some((prefix, number)) = numbered(&folded) {
        return format!("{prefix} {number}");
    }
    folded
}

/// A section name that is a prefix, one or more spaces and a decimal
/// number: the prefix (folded and trimmed) and the number.
fn numbered(name: &str) -> Option<(String, u32)> {
    let digits = name.len() - name.trim_end_matches(|c: char| c.is_ascii_digit()).len();
    if digits == 0 {
        return None;
    }
    let (prefix, number) = name.split_at(name.len() - digits);
    if !prefix.ends_with(' ') || trim(prefix).is_empty() {
        return None;
    }
    Some((trim(prefix).to_ascii_lowercase(), decimal(number)?))
}

/// An indexed key, `base_N`: the base and N.
fn indexed(key: &str) -> Option<(&str, u32)> {
    let (base, number) = key.rsplit_once('_')?;
    if base.is_empty() || number.is_empty() || !number.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    Some((base, decimal(number)?))
}

/// Decimal digits with optional leading zeros, as a number that fits.
fn decimal(digits: &str) -> Option<u32> {
    let digits = digits.trim_start_matches('0');
    if digits.is_empty() {
        return Some(0);
    }
    (digits.len() <= 9).then(|| digits.parse().ok()).flatten()
}

fn canonical_key(key: &str) -> String {
    let folded = key.to_ascii_lowercase();
    match indexed(&folded) {
        Some((base, number)) => format!("{base}_{number}"),
        None => folded,
    }
}

/// One value and the line it was read from.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Value {
    pub text: String,
    pub line: usize,
}

/// One section: its pairs by canonical key, the later of a repeated key
/// kept (§3.3 rule 9).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Section {
    /// The name as first written, trimmed.
    pub name: String,
    /// The line of its first header.
    pub line: usize,
    pairs: BTreeMap<String, Value>,
}

impl Section {
    /// The value of `key` (case folded; `enc_07` finds `enc_7`).
    pub fn get(&self, key: &str) -> Option<&Value> {
        self.pairs.get(&canonical_key(key))
    }
    /// Every `base_N` key with N in `range`, in ascending N.
    pub fn indexed(&self, base: &str, range: std::ops::RangeInclusive<u32>) -> Vec<(u32, &Value)> {
        let base = base.to_ascii_lowercase();
        let mut out: Vec<(u32, &Value)> = self
            .pairs
            .iter()
            .filter_map(|(key, value)| {
                let (b, n) = indexed(key)?;
                (b == base && range.contains(&n)).then_some((n, value))
            })
            .collect();
        out.sort_by_key(|(n, _)| *n);
        out
    }
    /// Every pair, by canonical key.
    pub fn pairs(&self) -> impl Iterator<Item = (&str, &Value)> {
        self.pairs.iter().map(|(key, value)| (key.as_str(), value))
    }
}

/// A file in the §3.3 sectioned grammar.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SectionedText {
    pub file: String,
    /// By canonical name.
    sections: BTreeMap<String, Section>,
}

impl SectionedText {
    /// Parse `bytes` as `file`, collecting the grammar's diagnostics.
    pub fn parse(file: &str, bytes: &[u8], notes: &mut Diagnostics) -> Result<Self, TableError> {
        let mut sections: BTreeMap<String, Section> = BTreeMap::new();
        let mut current: Option<String> = None;
        for (number, line) in lines(file, bytes)? {
            let text = content(line, b";");
            if text.is_empty() {
                continue;
            }
            if text.starts_with('[') && text.ends_with(']') && text.len() >= 2 {
                let name = trim(&text[1..text.len() - 1]).to_owned();
                if name.is_empty() {
                    notes.push(file, number, "an empty section name; its lines are ignored");
                    current = None;
                    continue;
                }
                let key = canonical_section(&name);
                sections.entry(key.clone()).or_insert_with(|| Section {
                    name,
                    line: number,
                    pairs: BTreeMap::new(),
                });
                current = Some(key);
                continue;
            }
            let Some((key, value)) = text.split_once('=') else {
                notes.push(file, number, format!("ignored line {text:?}"));
                continue;
            };
            let (key, value) = (trim(key), trim(value));
            if key.is_empty() {
                notes.push(file, number, "a pair without a key is ignored");
                continue;
            }
            let Some(section) = current.as_ref().and_then(|name| sections.get_mut(name)) else {
                notes.push(
                    file,
                    number,
                    format!("{key} outside any section is ignored"),
                );
                continue;
            };
            let previous = section.pairs.insert(
                canonical_key(key),
                Value {
                    text: value.to_owned(),
                    line: number,
                },
            );
            if previous.is_some() {
                notes.push(
                    file,
                    number,
                    format!(
                        "{key} repeated in [{}]; the later value is kept",
                        section.name
                    ),
                );
            }
        }
        Ok(Self {
            file: file.to_owned(),
            sections,
        })
    }
    /// The section named `name` (case folded, numbered names canonical).
    pub fn section(&self, name: &str) -> Option<&Section> {
        self.sections.get(&canonical_section(name))
    }
    /// Every numbered section `prefix N`, in ascending N, whatever N is.
    pub fn numbered(&self, prefix: &str) -> Vec<(u32, &Section)> {
        let prefix = prefix.to_ascii_lowercase();
        let mut out: Vec<(u32, &Section)> = self
            .sections
            .iter()
            .filter_map(|(key, section)| {
                let (p, n) = numbered(key)?;
                (p == prefix).then_some((n, section))
            })
            .collect();
        out.sort_by_key(|(n, _)| *n);
        out
    }
    /// Every section whose folded name starts with `prefix` (for example
    /// `random maps:`), with the rest of the name trimmed and folded.
    pub fn with_prefix(&self, prefix: &str) -> Vec<(String, &Section)> {
        let prefix = prefix.to_ascii_lowercase();
        self.sections
            .iter()
            .filter_map(|(key, section)| {
                let rest = key.strip_prefix(&prefix)?;
                Some((trim(rest).to_owned(), section))
            })
            .collect()
    }
}

/// §4.6 the line grammar: each record line split into its List items, with
/// its line number. `#` and `;` start comments.
pub fn record_lines(file: &str, bytes: &[u8]) -> Result<Vec<(usize, Vec<String>)>, TableError> {
    let mut out = Vec::new();
    for (number, line) in lines(file, bytes)? {
        let text = content(line, b"#;");
        if text.is_empty() {
            continue;
        }
        out.push((number, list(&text)));
    }
    Ok(out)
}

/// §3.4 Integer: an optional sign, then 1 to 10 decimal digits, within the
/// signed 32-bit range; nothing else.
pub fn integer(text: &str) -> Option<i32> {
    let digits = text.strip_prefix(['+', '-']).unwrap_or(text);
    if digits.is_empty() || digits.len() > 10 || !digits.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    let value: i64 = digits.parse().ok()?;
    let value = if text.starts_with('-') { -value } else { value };
    i32::try_from(value).ok()
}

/// An Integer within `range`.
pub fn integer_in(text: &str, range: std::ops::RangeInclusive<i32>) -> Option<i32> {
    integer(text).filter(|value| range.contains(value))
}

/// §3.4 Percent: an Integer 0-100, optionally followed by `%`.
pub fn percent(text: &str) -> Option<i32> {
    integer_in(text.strip_suffix('%').unwrap_or(text), 0..=100)
}

/// §3.4 Boolean, case folded.
pub fn boolean(text: &str) -> Option<bool> {
    match text.to_ascii_lowercase().as_str() {
        "yes" | "on" | "true" | "1" => Some(true),
        "no" | "off" | "false" | "0" => Some(false),
        _ => None,
    }
}

/// §3.4 List: items separated by `,` at parenthesis depth 0, each
/// trimmed, empty items dropped.
pub fn list(text: &str) -> Vec<String> {
    let mut items = Vec::new();
    let mut depth = 0i32;
    let mut item = String::new();
    for c in text.chars() {
        match c {
            '(' => depth += 1,
            ')' => depth -= 1,
            ',' if depth <= 0 => {
                items.push(std::mem::take(&mut item));
                continue;
            }
            _ => {}
        }
        item.push(c);
    }
    items.push(item);
    items
        .iter()
        .map(|item| trim(item).to_owned())
        .filter(|item| !item.is_empty())
        .collect()
}

/// §3.4 Stem: 1 to 8 bytes of letters, digits, `_` and `-`, after a final
/// `.map` (case folded) is removed. The case is kept.
pub fn stem(text: &str) -> Option<String> {
    let stem = if text.len() >= 4 && text[text.len() - 4..].eq_ignore_ascii_case(".map") {
        &text[..text.len() - 4]
    } else {
        text
    };
    let ok = (1..=8).contains(&stem.len())
        && stem
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-');
    ok.then(|| stem.to_owned())
}

/// §3.4 Name: 1 to 64 bytes without `,`, `(`, `)`, `=` or `;`.
pub fn name(text: &str) -> Option<String> {
    let ok = (1..=64).contains(&text.chars().count()) && !text.contains([',', '(', ')', '=', ';']);
    ok.then(|| text.to_owned())
}

/// A `keyword:value` field: the keyword folded and the value trimmed.
pub fn field(text: &str) -> Option<(String, &str)> {
    let (keyword, value) = text.split_once(':')?;
    Some((trim(keyword).to_ascii_lowercase(), trim(value)))
}

/// A name folded for comparison (§3.3 rule 8).
pub fn fold(text: &str) -> String {
    text.to_ascii_lowercase()
}
