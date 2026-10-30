use std::collections::BTreeMap;

pub const MAX_PROGRAM_BYTES: usize = 64 * 1024 * 1024;
pub const TIMED: u32 = 1;
pub const CONDITIONAL: u32 = 2;
pub const IMPORT: u32 = 4;
pub const EXPORT: u32 = 8;
pub const CRITICAL: u32 = 16;

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("INT at byte {offset:#x}: {message}")]
pub struct IntError {
    pub offset: usize,
    pub message: String,
}
impl IntError {
    pub(crate) fn new(offset: usize, message: impl Into<String>) -> Self {
        Self {
            offset,
            message: message.into(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Procedure {
    pub name_offset: u32,
    pub name: Vec<u8>,
    pub flags: u32,
    pub deadline: u32,
    pub condition: u32,
    pub body: u32,
    pub arguments: u32,
}
#[derive(Debug, Clone)]
pub struct IntProgram {
    pub(crate) image: Vec<u8>,
    pub(crate) boundaries: Vec<bool>,
    pub(crate) procedures: Vec<Procedure>,
    identifiers: BTreeMap<u32, Vec<u8>>,
    literals: BTreeMap<u32, Vec<u8>>,
    pub identifier_origin: usize,
    pub literal_origin: usize,
    pub code_start: usize,
    /// Parsed with [`IntProgram::parse_enhanced`]: the versioned enhanced
    /// range is accepted as instruction words.
    enhanced: bool,
    /// The game word set it was checked against (SPEC-017 §2.1).
    words: WordSet,
}

fn word(bytes: &[u8], at: usize) -> Result<u32, IntError> {
    let end = at
        .checked_add(4)
        .ok_or_else(|| IntError::new(at, "offset overflow"))?;
    Ok(u32::from_be_bytes(
        bytes
            .get(at..end)
            .ok_or_else(|| IntError::new(at, "truncated word"))?
            .try_into()
            .expect("fixed word"),
    ))
}
fn count(bytes: &[u8], at: usize) -> Result<usize, IntError> {
    let n = word(bytes, at)? as i32;
    usize::try_from(n).map_err(|_| IntError::new(at, "negative count/offset"))
}
fn strings(
    bytes: &[u8],
    origin: usize,
    absent: bool,
) -> Result<(BTreeMap<u32, Vec<u8>>, usize), IntError> {
    if absent && word(bytes, origin)? == u32::MAX {
        return Ok((BTreeMap::new(), origin + 4));
    }
    let length = count(bytes, origin)?;
    let end = origin
        .checked_add(4)
        .and_then(|start| start.checked_add(length))
        .ok_or_else(|| IntError::new(origin, "table size overflow"))?;
    if end > bytes.len() {
        return Err(IntError::new(origin, "table exceeds image"));
    }
    let mut pos = origin + 4;
    let mut result = BTreeMap::new();
    while pos < end {
        if end - pos < 2 {
            return Err(IntError::new(pos, "truncated string length"));
        }
        let size = u16::from_be_bytes([bytes[pos], bytes[pos + 1]]) as usize;
        pos += 2;
        if size == 0 || size > end - pos {
            return Err(IntError::new(pos, "invalid string extent"));
        }
        let data = &bytes[pos..pos + size];
        let nul = data
            .iter()
            .position(|byte| *byte == 0)
            .ok_or_else(|| IntError::new(pos, "string is not NUL terminated"))?;
        result.insert((pos - origin) as u32, data[..nul].to_vec());
        pos += size;
    }
    if word(bytes, end)? != u32::MAX {
        return Err(IntError::new(end, "missing table terminator"));
    }
    Ok((result, end + 4))
}
pub(crate) fn instruction_length(word: u16) -> usize {
    if word & 0x3ff == 1 { 6 } else { 2 }
}
/// The instruction words a game's programs may use besides the slot-1
/// pushes: the SPEC-004 core words and the host words of the game's
/// profile (SPEC-017 §2.1). The versioned enhanced range belongs to
/// neither set (SPEC-017 §2.2).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WordSet {
    /// The core and host words, `0x8000` up to the last host word.
    pub words: std::ops::RangeInclusive<u16>,
}
impl WordSet {
    /// SPEC-004/005: `0x8000` to `0x8155`.
    pub const FALLOUT1: Self = Self {
        words: 0x8000..=0x8155,
    };
    /// SPEC-017 §2.1: `0x8000` to `0x8183`: SPEC-005 as amended by
    /// SPEC-017 §3, plus the 46 words of SPEC-017 §4 (four defined, 42
    /// reserved). Its dispatch table has 388 entries.
    pub const FALLOUT2: Self = Self {
        words: 0x8000..=0x8183,
    };
    /// SPEC-017 §2.1 and SPEC-024b §3.2: `0x8000` to `0x8281`, Fallout 2 with
    /// sfall mode on. Its table has 642 entries: `0x8156` up are SPEC-024b's
    /// words, whatever the preset.
    pub const FALLOUT2_SFALL: Self = Self {
        words: 0x8000..=0x8281,
    };
    /// Whether this is a Fallout 2 set, whose host words are SPEC-005's as
    /// amended by SPEC-017 ([`crate::extensions::opcode_in`]).
    pub fn fallout2(&self) -> bool {
        *self == Self::FALLOUT2 || self.sfall()
    }
    /// Whether this is the sfall mode set.
    pub fn sfall(&self) -> bool {
        *self == Self::FALLOUT2_SFALL
    }
    /// [`Self::accepts`], plus the versioned enhanced range
    /// ([`crate::extensions::ENHANCED_RANGE`]) when `enhanced` is set.
    pub fn accepts_in(&self, word: u16, enhanced: bool) -> bool {
        self.accepts(word) || (enhanced && crate::extensions::ENHANCED_RANGE.contains(&word))
    }
    /// SPEC-004 §2.5: bit 15 must be set. All 32 slot-1 words `0x8001 + n *
    /// 0x0400` are valid pushes that keep their full word as the tag (a
    /// non-standard tag such as `0x8401` fails later, at a type check).
    /// Other words with bits 10-14 set are rejected (the `[May-improve]`
    /// rule).
    pub fn accepts(&self, word: u16) -> bool {
        if word & 0x3ff == 1 {
            word & 0x8000 != 0
        } else {
            self.words.contains(&word)
        }
    }
}
impl IntProgram {
    /// Parse under SPEC-004: any word outside the defined set, including
    /// the enhanced range, is an invalid instruction.
    pub fn parse(bytes: &[u8]) -> Result<Self, IntError> {
        Self::parse_with(bytes, false)
    }
    /// Parse for an enhanced host: the versioned enhanced range is accepted
    /// as instruction words as well. Every other rule is unchanged.
    pub fn parse_enhanced(bytes: &[u8]) -> Result<Self, IntError> {
        Self::parse_with(bytes, true)
    }
    /// Parse under the Fallout 1 word set, with the enhanced range accepted
    /// when `enhanced` is set.
    pub fn parse_with(bytes: &[u8], enhanced: bool) -> Result<Self, IntError> {
        Self::parse_in(bytes, &WordSet::FALLOUT1, enhanced)
    }
    /// Parse under `words`, a game profile's word set (SPEC-017 §2.1): any
    /// other non-push word is an invalid instruction, except the enhanced
    /// range when `enhanced` is set. Every other rule is unchanged.
    pub fn parse_in(bytes: &[u8], words: &WordSet, enhanced: bool) -> Result<Self, IntError> {
        if bytes.len() > MAX_PROGRAM_BYTES {
            return Err(IntError::new(0, "program exceeds 64 MiB limit"));
        }
        let n = count(bytes, 42)?;
        if n > 65536 {
            return Err(IntError::new(42, "procedure count exceeds resource limit"));
        }
        let identifier_origin = 46 + n * 24;
        let (identifiers, literal_origin) = strings(bytes, identifier_origin, false)?;
        let (literals, code_start) = strings(bytes, literal_origin, true)?;
        let mut procedures = Vec::new();
        for index in 0..n {
            let at = 46 + 24 * index;
            let name_offset = word(bytes, at)?;
            let name = identifiers
                .get(&name_offset)
                .ok_or_else(|| IntError::new(at, "name reference is not a string start"))?
                .clone();
            let arguments = count(bytes, at + 20)?;
            if arguments > 341 {
                return Err(IntError::new(
                    at + 20,
                    "argument count exceeds stack capacity",
                ));
            }
            procedures.push(Procedure {
                name_offset,
                name,
                flags: word(bytes, at + 4)?,
                deadline: word(bytes, at + 8)?,
                condition: count(bytes, at + 12)? as u32,
                body: count(bytes, at + 16)? as u32,
                arguments: arguments as u32,
            });
        }
        let mut boundaries = vec![false; bytes.len()];
        for (start, end) in [(0, 42), (code_start, bytes.len())] {
            let mut at = start;
            while at < end {
                if end - at < 2 {
                    return Err(IntError::new(at, "truncated instruction"));
                }
                let op = u16::from_be_bytes([bytes[at], bytes[at + 1]]);
                if !words.accepts_in(op, enhanced) {
                    return Err(IntError::new(at, format!("invalid instruction {op:#06x}")));
                }
                let size = instruction_length(op);
                if end - at < size {
                    return Err(IntError::new(at, "truncated immediate"));
                }
                // Identifier-consuming instructions use the raw payload even
                // when it was pushed with a string tag. Resolve/bound references
                // at their consumer, rather than assuming every 9001 targets T.
                boundaries[at] = true;
                at += size;
            }
        }
        for (index, procedure) in procedures.iter().enumerate() {
            for (field, address) in [
                (16, Some(procedure.body)),
                (
                    12,
                    (procedure.condition != 0).then_some(procedure.condition),
                ),
            ] {
                let Some(address) = address else {
                    continue;
                };
                if address as usize != bytes.len()
                    && !boundaries.get(address as usize).copied().unwrap_or(false)
                {
                    return Err(IntError::new(
                        46 + 24 * index + field,
                        format!(
                            "procedure {} address {address:#x} is not an instruction boundary (image length {:#x})",
                            if field == 16 { "body" } else { "condition" },
                            bytes.len()
                        ),
                    ));
                }
            }
        }
        Ok(Self {
            image: bytes.to_vec(),
            boundaries,
            procedures,
            identifiers,
            literals,
            identifier_origin,
            literal_origin,
            code_start,
            enhanced,
            words: words.clone(),
        })
    }
    /// Whether the program was parsed with the enhanced range accepted.
    pub fn enhanced(&self) -> bool {
        self.enhanced
    }
    /// The game word set the program was parsed under.
    pub fn words(&self) -> &WordSet {
        &self.words
    }
    /// Whether the program may execute `word`: its word set, plus the
    /// enhanced range when it was parsed enhanced.
    pub fn accepts(&self, word: u16) -> bool {
        self.words.accepts_in(word, self.enhanced)
    }
    /// Whether `address` starts an instruction of the linear decode (the
    /// bootstrap area and the code after the string tables).
    pub fn boundary(&self, address: u32) -> bool {
        self.boundaries
            .get(address as usize)
            .copied()
            .unwrap_or(false)
    }
    pub fn image(&self) -> &[u8] {
        &self.image
    }
    pub fn procedures(&self) -> &[Procedure] {
        &self.procedures
    }
    pub fn identifier(&self, offset: u32) -> Option<&[u8]> {
        self.identifiers.get(&offset).map(Vec::as_slice)
    }
    pub fn literal(&self, offset: u32) -> Option<&[u8]> {
        self.literals.get(&offset).map(Vec::as_slice)
    }
    pub fn find_procedure(&self, name: &[u8], include_root: bool) -> Option<usize> {
        self.procedures
            .iter()
            .enumerate()
            .skip(usize::from(!include_root))
            .find(|(_, procedure)| procedure.name.eq_ignore_ascii_case(name))
            .map(|(index, _)| index)
    }
    pub fn lifecycle_entry(&self, event: Lifecycle) -> Result<Option<usize>, IntError> {
        let index = self.find_procedure(event.name(), true).unwrap_or(0);
        if index != 0 {
            return Ok(Some(index));
        }
        if matches!(event, Lifecycle::MapEnter | Lifecycle::MapUpdate) {
            return Ok(None);
        }
        if self.procedures.len() > 1 {
            Ok(Some(1))
        } else {
            Err(IntError::new(42, "missing lifecycle fallback procedure 1"))
        }
    }
    pub(crate) fn schedule(&mut self, index: usize, flags: u32, deadline: u32, condition: u32) {
        let procedure = &mut self.procedures[index];
        procedure.flags = flags;
        procedure.deadline = deadline;
        procedure.condition = condition;
        for (offset, value) in [(4, flags), (8, deadline), (12, condition)] {
            let at = 46 + 24 * index + offset;
            self.image[at..at + 4].copy_from_slice(&value.to_be_bytes());
        }
    }
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Lifecycle {
    Start,
    MapEnter,
    MapUpdate,
    Talk,
    LookAt,
    Description,
}
impl Lifecycle {
    pub fn name(self) -> &'static [u8] {
        match self {
            Self::Start => b"start",
            Self::MapEnter => b"map_enter_p_proc",
            Self::MapUpdate => b"map_update_p_proc",
            Self::Talk => b"talk_p_proc",
            Self::LookAt => b"look_at_p_proc",
            Self::Description => b"description_p_proc",
        }
    }
    pub fn event(self) -> u32 {
        match self {
            Self::Start => 1,
            Self::MapEnter => 15,
            Self::MapUpdate => 23,
            Self::Talk => 11,
            Self::LookAt => 21,
            Self::Description => 3,
        }
    }
}
