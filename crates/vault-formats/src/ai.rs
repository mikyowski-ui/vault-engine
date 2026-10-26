//! SPEC-009 §2.2-2.4 `data/ai.txt`: the configuration reader's line and
//! section grammar, the 25 packet keys, the `hurt_too_much` token grammar,
//! and packet resolution over the case-folded, sorted packet table.
use crate::ParseError;

/// Every packet section must hold all of these file-format key literals.
pub const KEYS: [&str; 25] = [
    "packet_num",
    "max_dist",
    "min_to_hit",
    "min_hp",
    "aggression",
    "hurt_too_much",
    "secondary_freq",
    "called_freq",
    "font",
    "color",
    "outline_color",
    "chance",
    "run_start",
    "move_start",
    "attack_start",
    "miss_start",
    "hit_head_start",
    "hit_left_arm_start",
    "hit_right_arm_start",
    "hit_torso_start",
    "hit_right_leg_start",
    "hit_left_leg_start",
    "hit_eyes_start",
    "hit_groin_start",
    "last_msg",
];

/// SPEC-018 §4.7.1 (Revision 3, R14): the twelve range-end keys every
/// Fallout 2 packet holds, in [`Packet::ends`] order.
pub const END_KEYS: [&str; 12] = [
    "run_end",
    "move_end",
    "attack_end",
    "miss_end",
    "hit_head_end",
    "hit_left_arm_end",
    "hit_right_arm_end",
    "hit_torso_end",
    "hit_right_leg_end",
    "hit_left_leg_end",
    "hit_eyes_end",
    "hit_groin_end",
];

/// Damage-flag masks named by `hurt_too_much` tokens.
pub const HURT_TOKENS: [(&[u8], u32); 4] = [
    (b"blind", 0x40),
    (b"crippled", 0x3c),
    (b"crippled_legs", 0x0c),
    (b"crippled_arms", 0x30),
];

/// One reader line holds at most 255 data bytes (a 256-byte buffer).
const LINE_BYTES: usize = 255;

fn is_space(byte: u8) -> bool {
    byte == 0x20 || (0x09..=0x0d).contains(&byte)
}
fn trim(bytes: &[u8]) -> &[u8] {
    let start = bytes
        .iter()
        .position(|b| !is_space(*b))
        .unwrap_or(bytes.len());
    let end = bytes
        .iter()
        .rposition(|b| !is_space(*b))
        .map_or(start, |i| i + 1);
    &bytes[start..end.max(start)]
}
/// ASCII `A`-`Z` folded to lowercase; comparisons then use unsigned bytes.
pub fn fold(bytes: &[u8]) -> Vec<u8> {
    bytes.to_ascii_lowercase()
}

/// One section: its first-occurrence name and its (folded key, value) pairs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Section {
    pub name: Vec<u8>,
    pub entries: Vec<(Vec<u8>, Vec<u8>)>,
}
impl Section {
    pub fn get(&self, key: &[u8]) -> Option<&[u8]> {
        let key = fold(key);
        self.entries
            .iter()
            .find(|(k, _)| *k == key)
            .map(|(_, value)| value.as_slice())
    }
}

/// A parsed configuration file: sections sorted by case-folded name.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Config {
    pub sections: Vec<Section>,
}
impl Config {
    /// Lines of at most 255 bytes (ending after an LF), truncated at the first
    /// `;`. `[`...`]` selects a section (the line then ends); otherwise the
    /// first `=` splits a trimmed key and value. Sections exist once they hold
    /// a key; repeated sections merge and repeated keys keep the last value.
    /// Keys before any header belong to `unknown`. An embedded NUL byte is
    /// rejected rather than silently ending later parsing steps.
    pub fn parse(bytes: &[u8]) -> Result<Self, ParseError> {
        if let Some(at) = bytes.iter().position(|b| *b == 0) {
            return Err(ParseError::new("CONFIG", at, "embedded NUL byte"));
        }
        let mut config = Self::default();
        let mut current = b"unknown".to_vec();
        let mut pos = 0;
        while pos < bytes.len() {
            let window = &bytes[pos..(pos + LINE_BYTES).min(bytes.len())];
            let length = window
                .iter()
                .position(|b| *b == b'\n')
                .map_or(window.len(), |lf| lf + 1);
            let line = &bytes[pos..pos + length];
            pos += length;
            let line = &line[..line.iter().position(|b| *b == b';').unwrap_or(line.len())];
            if let Some(open) = line.iter().position(|b| *b == b'[')
                && let Some(close) = line[open + 1..].iter().position(|b| *b == b']')
            {
                current = trim(&line[open + 1..open + 1 + close]).to_vec();
                continue;
            }
            let Some(equals) = line.iter().position(|b| *b == b'=') else {
                continue;
            };
            config.insert(&current, trim(&line[..equals]), trim(&line[equals + 1..]));
        }
        Ok(config)
    }
    /// The section with this case-folded name.
    pub fn section(&self, name: &[u8]) -> Option<&Section> {
        let folded = fold(name);
        self.sections
            .binary_search_by(|section| fold(&section.name).cmp(&folded))
            .ok()
            .map(|index| &self.sections[index])
    }
    fn insert(&mut self, name: &[u8], key: &[u8], value: &[u8]) {
        let folded = fold(name);
        let index = match self
            .sections
            .binary_search_by(|section| fold(&section.name).cmp(&folded))
        {
            Ok(index) => index,
            Err(index) => {
                self.sections.insert(
                    index,
                    Section {
                        name: name.to_vec(),
                        entries: Vec::new(),
                    },
                );
                index
            }
        };
        let key = fold(key);
        let section = &mut self.sections[index];
        match section.entries.iter_mut().find(|(k, _)| *k == key) {
            Some(entry) => entry.1 = value.to_vec(),
            None => section.entries.push((key, value.to_vec())),
        }
    }
}

/// Decimal-prefix integer: leading whitespace, an optional sign, digits up
/// to the first non-digit (none gives 0). Out-of-range values saturate to
/// the signed 32-bit range; the flag reports that for a diagnostic.
pub fn decimal_prefix(value: &[u8]) -> (i32, bool) {
    let mut rest = value
        .iter()
        .copied()
        .skip_while(|b| is_space(*b))
        .peekable();
    let negative = match rest.peek() {
        Some(b'-') => {
            rest.next();
            true
        }
        Some(b'+') => {
            rest.next();
            false
        }
        _ => false,
    };
    let limit = i64::from(i32::MAX) + 1;
    let mut magnitude = 0_i64;
    let mut overflow = false;
    for digit in rest.take_while(u8::is_ascii_digit) {
        magnitude = magnitude * 10 + i64::from(digit - b'0');
        if magnitude > limit {
            magnitude = limit + 1;
            overflow = true;
        }
    }
    let signed = if negative { -magnitude } else { magnitude };
    let clamped = signed.clamp(i64::from(i32::MIN), i64::from(i32::MAX));
    (clamped as i32, overflow || clamped != signed)
}

/// `hurt_too_much`: lowercase the value, then per token skip leading spaces
/// (`0x20` only), take bytes up to the next `,` (trailing spaces kept), and
/// OR the exact token's mask. Unrecognized tokens, including empty ones, add
/// nothing and are returned for diagnostics. An empty value gives 0.
pub fn hurt_mask(value: &[u8]) -> (u32, Vec<Vec<u8>>) {
    let value = value.to_ascii_lowercase();
    let (mut mask, mut unknown) = (0, Vec::new());
    if value.is_empty() {
        return (mask, unknown);
    }
    let mut pos = 0;
    loop {
        while value.get(pos) == Some(&b' ') {
            pos += 1;
        }
        let comma = value[pos..]
            .iter()
            .position(|b| *b == b',')
            .map(|i| pos + i);
        let token = &value[pos..comma.unwrap_or(value.len())];
        match HURT_TOKENS.iter().find(|(name, _)| *name == token) {
            Some((_, bits)) => mask |= bits,
            None => unknown.push(token.to_vec()),
        }
        match comma {
            Some(comma) => pos = comma + 1,
            None => return (mask, unknown),
        }
    }
}

/// One `ai.txt` packet. `hit_start` follows hit-location numbering 0-7:
/// head, left arm, right arm, torso, right leg, left leg, eyes, groin.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Packet {
    pub name: Vec<u8>,
    pub packet_num: i32,
    pub max_dist: i32,
    pub min_to_hit: i32,
    pub min_hp: i32,
    /// Parsed and required, but consulted by no behavior.
    pub aggression: i32,
    pub hurt_too_much: u32,
    pub secondary_freq: i32,
    pub called_freq: i32,
    pub font: i32,
    pub color: i32,
    pub outline_color: i32,
    pub chance: i32,
    pub run_start: i32,
    pub move_start: i32,
    pub attack_start: i32,
    pub miss_start: i32,
    pub hit_start: [i32; 8],
    /// 0 under Fallout 2, which does not read it (SPEC-018 §4.7.1).
    pub last_msg: i32,
    /// SPEC-018 §4.7.1: under Fallout 2, each range's own end, in
    /// [`END_KEYS`] order; `None` for a Fallout 1 packet, whose ranges end
    /// at the next start.
    pub ends: Option<[i32; 12]>,
}

/// The packet table in sorted section order, plus parse diagnostics.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Packets {
    pub packets: Vec<Packet>,
    pub diagnostics: Vec<String>,
}
impl Packets {
    /// Build every section's packet; a section lacking any of the 25 keys
    /// fails the whole table. A zero-packet table is also an error.
    pub fn parse(bytes: &[u8]) -> Result<Self, ParseError> {
        Self::parse_for(bytes, crate::GameProfile::Fallout1)
    }
    /// [`Packets::parse`] in `game`'s key set. Under Fallout 2 (SPEC-018
    /// §4.7.1, Revision 3) a packet needs SPEC-009's keys except `last_msg`
    /// (not read) and `hurt_too_much` (optional: absent is mask 0, with no
    /// diagnostic), and also the twelve [`END_KEYS`]; a packet lacking a
    /// required key still fails the whole table.
    pub fn parse_for(bytes: &[u8], game: crate::GameProfile) -> Result<Self, ParseError> {
        let fallout2 = game == crate::GameProfile::Fallout2;
        let config = Config::parse(bytes)?;
        let mut diagnostics = Vec::new();
        let mut packets = Vec::with_capacity(config.sections.len());
        for section in &config.sections {
            let label = String::from_utf8_lossy(&section.name).into_owned();
            let lacks =
                |key: &str| ParseError::new("AI", 0, format!("packet [{label}] lacks key {key}"));
            let mut values = [0_i32; 25];
            let mut hurt = 0;
            for (index, key) in KEYS.iter().enumerate() {
                let value = match section.get(key.as_bytes()) {
                    Some(value) => value,
                    None if fallout2 && (*key == "hurt_too_much" || *key == "last_msg") => continue,
                    None => return Err(lacks(key)),
                };
                if fallout2 && *key == "last_msg" {
                    continue;
                }
                if *key == "hurt_too_much" {
                    let (mask, unknown) = hurt_mask(value);
                    hurt = mask;
                    for token in unknown {
                        diagnostics.push(format!(
                            "packet [{label}]: unrecognized hurt_too_much token {:?}",
                            String::from_utf8_lossy(&token)
                        ));
                    }
                    continue;
                }
                let (number, saturated) = decimal_prefix(value);
                if saturated {
                    diagnostics.push(format!("packet [{label}]: {key} saturated to {number}"));
                }
                values[index] = number;
            }
            let ends = if fallout2 {
                let mut ends = [0_i32; 12];
                for (end, key) in ends.iter_mut().zip(END_KEYS) {
                    let value = section.get(key.as_bytes()).ok_or_else(|| lacks(key))?;
                    let (number, saturated) = decimal_prefix(value);
                    if saturated {
                        diagnostics.push(format!("packet [{label}]: {key} saturated to {number}"));
                    }
                    *end = number;
                }
                Some(ends)
            } else {
                None
            };
            packets.push(Packet {
                name: section.name.clone(),
                packet_num: values[0],
                max_dist: values[1],
                min_to_hit: values[2],
                min_hp: values[3],
                aggression: values[4],
                hurt_too_much: hurt,
                secondary_freq: values[6],
                called_freq: values[7],
                font: values[8],
                color: values[9],
                outline_color: values[10],
                chance: values[11],
                run_start: values[12],
                move_start: values[13],
                attack_start: values[14],
                miss_start: values[15],
                hit_start: std::array::from_fn(|location| values[16 + location]),
                last_msg: values[24],
                ends,
            });
        }
        if packets.is_empty() {
            return Err(ParseError::new("AI", 0, "ai.txt defines no packets"));
        }
        Ok(Self {
            packets,
            diagnostics,
        })
    }
    pub fn len(&self) -> usize {
        self.packets.len()
    }
    pub fn is_empty(&self) -> bool {
        self.packets.is_empty()
    }
    /// The first sorted packet numbered `number`; otherwise table entry 0
    /// (the alphabetically first section), reported by the `false` flag.
    pub fn resolve(&self, number: i32) -> (&Packet, bool) {
        match self.packets.iter().find(|p| p.packet_num == number) {
            Some(packet) => (packet, true),
            None => (&self.packets[0], false),
        }
    }
    /// Section name for a packet number. The range guard compares the
    /// number with the table length, so a packet numbered at or above the
    /// count cannot be named.
    pub fn name(&self, number: i32) -> Option<&[u8]> {
        if number < 0 || number as usize >= self.packets.len() {
            return None;
        }
        self.packets
            .iter()
            .find(|p| p.packet_num == number)
            .map(|p| p.name.as_slice())
    }
}
