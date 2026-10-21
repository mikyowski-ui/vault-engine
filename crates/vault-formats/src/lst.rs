//! Consumer-specific bounded line reads; blank and comment-like lines keep IDs.
use crate::{
    ParseError, VfsPath,
    binary::{MAX_ENTRIES, Reader},
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LstKind {
    Raw,
    Art,
    Script,
    Prototype,
}

impl LstKind {
    pub fn for_path(path: &VfsPath) -> Self {
        if path.as_str().starts_with("art/") {
            Self::Art
        } else if path.as_str() == "scripts/scripts.lst" {
            Self::Script
        } else if path.as_str().starts_with("proto/") {
            Self::Prototype
        } else {
            Self::Raw
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LstEntry {
    pub raw: Vec<u8>,
    pub name: Option<Vec<u8>>,
    pub alias: Option<i32>,
    /// The reference head-list counts: the first count after the comma,
    /// repeated for all three moods.
    pub head_counts: Option<[i32; 3]>,
    /// The first, second, and third comma-separated counts (good, neutral,
    /// bad) for the SPEC-006 §3.11 safe rule; a missing or overflowing later
    /// column repeats the first count.
    pub head_columns: Option<[i32; 3]>,
    pub local_vars: Option<i32>,
}

impl LstEntry {
    pub fn critter_alias(&self, default_art_index: i32) -> i32 {
        self.alias.unwrap_or(default_art_index)
    }
}

#[derive(Debug, Clone)]
pub struct LstTable {
    kind: LstKind,
    entries: Vec<LstEntry>,
    /// Per-line defects (an embedded NUL, overflowing decimal metadata):
    /// the line keeps its position without a name or metadata, and the
    /// rest of the list still loads.
    diagnostics: Vec<String>,
}

fn decimal_prefix(bytes: &[u8], offset: usize) -> Result<i32, ParseError> {
    let start = bytes
        .iter()
        .position(|byte| !byte.is_ascii_whitespace())
        .unwrap_or(bytes.len());
    let bytes = &bytes[start..];
    let sign = usize::from(
        bytes
            .first()
            .is_some_and(|byte| matches!(byte, b'+' | b'-')),
    );
    let length = bytes[sign..]
        .iter()
        .take_while(|byte| byte.is_ascii_digit())
        .count();
    if length == 0 {
        return Ok(0);
    }
    std::str::from_utf8(&bytes[..sign + length])
        .expect("ASCII decimal")
        .parse()
        .map_err(|_| ParseError::new("LST", offset, "decimal metadata exceeds i32"))
}

impl LstTable {
    pub fn parse(bytes: &[u8], kind: LstKind) -> Result<Self, ParseError> {
        Reader::new(bytes, "LST")?;
        let limit = match kind {
            LstKind::Art => 199,
            LstKind::Script => 259,
            LstKind::Prototype => 255,
            LstKind::Raw => usize::MAX,
        };
        let mut entries = Vec::new();
        let mut diagnostics = Vec::new();
        let mut position = 0;
        while position < bytes.len() {
            if entries.len() == MAX_ENTRIES {
                return Err(ParseError::new("LST", position, "entry limit exceeded"));
            }
            let offset = position;
            let line = entries.len() + 1;
            let mut raw = Vec::new();
            let mut nul = None;
            while position < bytes.len() && raw.len() < limit {
                let mut byte = bytes[position];
                position += 1;
                if byte == b'\r' && bytes.get(position) == Some(&b'\n') {
                    byte = b'\n';
                    position += 1;
                }
                if byte == 0 && nul.is_none() {
                    nul = Some(position - 1);
                }
                raw.push(byte);
                if byte == b'\n' {
                    break;
                }
            }
            // SPEC-001 §3.6: malformed text has no dependable compatibility
            // result and must not pass for a supported mapping. Safe rule:
            // a line with an embedded NUL keeps its position (so later
            // positions stay aligned) with no name or metadata, and the
            // rest of the list still loads.
            if let Some(at) = nul {
                diagnostics.push(format!(
                    "line {line} at byte {at:#x}: embedded NUL; position kept without a name"
                ));
                entries.push(LstEntry {
                    raw,
                    name: None,
                    alias: None,
                    head_counts: None,
                    head_columns: None,
                    local_vars: None,
                });
                continue;
            }
            // Overflowing decimal metadata likewise withholds that value
            // for the line instead of failing the whole list.
            let mut metadata = |result: Result<i32, ParseError>| match result {
                Ok(value) => Some(value),
                Err(error) => {
                    diagnostics.push(format!("line {line}: {error}; metadata withheld"));
                    None
                }
            };
            let name = match kind {
                LstKind::Raw => None,
                LstKind::Art => Some(
                    raw.iter()
                        .copied()
                        .take_while(|byte| !b" ,;\t\n".contains(byte))
                        .take(12)
                        .collect(),
                ),
                LstKind::Prototype => Some(
                    raw.iter()
                        .copied()
                        .take_while(|byte| !b" \n".contains(byte))
                        .collect(),
                ),
                LstKind::Script => raw.iter().position(|byte| *byte == b'.').map(|dot| {
                    let mut name = raw[..dot].to_vec();
                    name.extend_from_slice(b".int");
                    name
                }),
            };
            let alias = if kind == LstKind::Art {
                raw.iter().position(|byte| *byte == b',').and_then(|comma| {
                    metadata(decimal_prefix(&raw[comma + 1..], offset + comma + 1))
                })
            } else {
                None
            };
            let local_vars = if kind == LstKind::Script {
                raw.iter()
                    .position(|byte| *byte == b'#')
                    .and_then(|hash| {
                        raw[hash + 1..]
                            .windows(11)
                            .position(|window| window == b"local_vars=")
                            .map(|at| hash + 1 + at + 11)
                    })
                    .and_then(|start| metadata(decimal_prefix(&raw[start..], offset + start)))
            } else {
                None
            };
            let head_columns = alias.map(|first| {
                let mut columns = [first; 3];
                let mut rest = raw.splitn(4, |byte| *byte == b',').skip(2);
                for column in &mut columns[1..] {
                    if let Some(count) = rest
                        .next()
                        .and_then(|text| decimal_prefix(text, offset).ok())
                    {
                        *column = count;
                    }
                }
                columns
            });
            entries.push(LstEntry {
                raw,
                name,
                alias,
                head_counts: alias.map(|count| [count; 3]),
                head_columns,
                local_vars,
            });
        }
        Ok(Self {
            kind,
            entries,
            diagnostics,
        })
    }

    pub fn entries(&self) -> &[LstEntry] {
        &self.entries
    }
    /// Lines whose name or metadata was withheld as malformed, in order.
    pub fn diagnostics(&self) -> &[String] {
        &self.diagnostics
    }
    /// The entry count, derived from successful bounded reads for every
    /// kind (SPEC-001 §3.4 safe rule; the reference's script and prototype
    /// bookkeeping counts LF + 1 and LF + 2 are not reproduced).
    pub fn bookkeeping_len(&self) -> usize {
        self.entries.len()
    }
    pub fn art_id(&self, id: u32) -> Option<&LstEntry> {
        (self.kind == LstKind::Art)
            .then(|| self.entries.get((id & 0xfff) as usize))
            .flatten()
    }
    pub fn script_id(&self, id: u32) -> Option<&LstEntry> {
        (self.kind == LstKind::Script)
            .then(|| self.entries.get((id & 0xffffff) as usize))
            .flatten()
    }
    pub fn map_script_reference(&self, reference: i32) -> Option<&LstEntry> {
        if self.kind != LstKind::Script || reference <= 0 {
            return None;
        }
        self.entries.get((reference - 1) as usize)
    }
    pub fn prototype_id(&self, id: u32) -> Option<&LstEntry> {
        if self.kind != LstKind::Prototype {
            return None;
        }
        self.entries.get((id & 0xffffff).checked_sub(1)? as usize)
    }
}
