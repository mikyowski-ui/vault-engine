//! SPEC-010 §2.6 `.LIP` phoneme files: the mouth shapes for one dialogue
//! speech file. Every multi-byte integer is a big-endian s32. A version 2
//! file is a 44-byte header, P phoneme bytes and M eight-byte markers; a
//! marker's position is a byte offset into the decoded 16-bit mono PCM
//! (44100 bytes per second at 22050 Hz).
use crate::{MAX_ASSET_BYTES, sfx};

/// The version 2 header length.
pub const HEADER_V2: usize = 0x2C;
/// The version 1 header length; its phoneme table starts at `0x168`.
pub const HEADER_V1: usize = 0x168;
/// Flags bit 0: the audio keeps running past the final marker (§3.12).
pub const REPEAT_FLAG: i32 = 1;
/// Decoded speech PCM bytes per second: 22050 mono 16-bit samples.
pub const BYTES_PER_SECOND: u64 = 44100;
/// Phoneme numbers below this map to speech frames (SPEC-006 §3.11).
pub const PHONEMES: u8 = 42;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LipMarker {
    /// 0 or 1; checked when loading, otherwise unused.
    pub kind: i32,
    /// A byte offset into the decoded speech PCM.
    pub position: i32,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LipFile {
    pub version: i32,
    pub flags: i32,
    /// Speech length in bytes (version 2 `0x10`); not used at playback.
    pub length: i32,
    /// The companion ACM's name, without directory or extension: the
    /// stored 8 bytes up to the first zero byte.
    pub stem: Vec<u8>,
    pub phonemes: Vec<u8>,
    /// Phoneme *i* shows from marker *i* to marker *i* + 1; the last marker
    /// is the end-of-speech marker (`P = M - 1` when well formed).
    pub markers: Vec<LipMarker>,
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum LipError {
    /// §3.12: logged, then handled as a missing `.LIP`.
    #[error("LIP version {0} is neither 1 nor 2")]
    Version(i32),
    /// §3.12: the speech is abandoned.
    #[error("LIP file of {len} bytes is shorter than its header and counts require ({needed})")]
    Truncated { needed: u64, len: usize },
    /// A negative count (never in the corpus): abandoned like a short file.
    #[error("LIP {table} count {count} is negative")]
    Count { table: &'static str, count: i32 },
    #[error("LIP file exceeds the asset size limit")]
    TooLarge,
}

/// A §3.12 loading check. The checks only log; nothing is rejected.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LipWarning {
    /// A phoneme of 42 or more.
    Phoneme { index: usize, value: u8 },
    /// A marker type other than 0 or 1.
    MarkerKind { index: usize, kind: i32 },
    /// A nonzero position at marker 0.
    FirstPosition(i32),
    /// A position lower than the one before it.
    Decreasing { index: usize, position: i32 },
}

impl std::fmt::Display for LipWarning {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Phoneme { index, value } => write!(f, "phoneme {index} is {value} (42 or more)"),
            Self::MarkerKind { index, kind } => write!(f, "marker {index} has type {kind}"),
            Self::FirstPosition(position) => write!(f, "marker 0 is at position {position}"),
            Self::Decreasing { index, position } => {
                write!(f, "marker {index} position {position} decreases")
            }
        }
    }
}

fn word(bytes: &[u8], offset: usize) -> i32 {
    i32::from_be_bytes(
        bytes[offset..offset + 4]
            .try_into()
            .expect("four header bytes"),
    )
}

fn stem(bytes: &[u8]) -> Vec<u8> {
    let end = bytes.iter().position(|byte| *byte == 0).unwrap_or(8);
    bytes[..end].to_vec()
}

impl LipFile {
    /// §2.6 layout. Version 1 is read by its 360-byte header; under the
    /// `[May-improve]` safe rule its four stored handle and address words
    /// (`0x0C`, `0x14`, `0x18`, `0x30`) are ignored. Bytes after the marker
    /// table are not read.
    pub fn parse(bytes: &[u8]) -> Result<Self, LipError> {
        if bytes.len() > MAX_ASSET_BYTES {
            return Err(LipError::TooLarge);
        }
        let short = |needed: u64| LipError::Truncated {
            needed,
            len: bytes.len(),
        };
        if bytes.len() < 4 {
            return Err(short(4));
        }
        let version = word(bytes, 0);
        // Header length, then the flags, length, P, M and stem offsets.
        let (header, flags, length, phonemes, markers, name) = match version {
            2 => (HEADER_V2, 0x08, Some(0x10), 0x14, 0x1C, 0x20),
            1 => (HEADER_V1, 0x08, None, 0x24, 0x2C, 0x50),
            other => return Err(LipError::Version(other)),
        };
        if bytes.len() < header {
            return Err(short(header as u64));
        }
        let count = |offset: usize, table: &'static str| {
            let count = word(bytes, offset);
            usize::try_from(count).map_err(|_| LipError::Count { table, count })
        };
        let (p, m) = (count(phonemes, "phoneme")?, count(markers, "marker")?);
        let needed = header as u64 + p as u64 + 8 * m as u64;
        if (bytes.len() as u64) < needed {
            return Err(short(needed));
        }
        let table = &bytes[header + p..];
        Ok(Self {
            version,
            flags: word(bytes, flags),
            length: length.map_or(0, |offset| word(bytes, offset)),
            stem: stem(&bytes[name..name + 8]),
            phonemes: bytes[header..header + p].to_vec(),
            markers: (0..m)
                .map(|index| LipMarker {
                    kind: word(table, 8 * index),
                    position: word(table, 8 * index + 4),
                })
                .collect(),
        })
    }
    /// The §3.12 missing-file safe rule: one phoneme 0 whose final marker
    /// lies after the audio end, so the head shows speech frame 0 until
    /// the audio finishes.
    pub fn silent() -> Self {
        Self {
            version: 2,
            flags: 0,
            length: 0,
            stem: Vec::new(),
            phonemes: vec![0],
            markers: vec![
                LipMarker {
                    kind: 1,
                    position: 0,
                },
                LipMarker {
                    kind: 1,
                    position: i32::MAX,
                },
            ],
        }
    }
    pub fn repeat(&self) -> bool {
        self.flags & REPEAT_FLAG != 0
    }
    /// §3.12 loading step 4: the checks that only log.
    pub fn check(&self) -> Vec<LipWarning> {
        let mut warnings = Vec::new();
        for (index, &value) in self.phonemes.iter().enumerate() {
            if value >= PHONEMES {
                warnings.push(LipWarning::Phoneme { index, value });
            }
        }
        for (index, marker) in self.markers.iter().enumerate() {
            if !matches!(marker.kind, 0 | 1) {
                warnings.push(LipWarning::MarkerKind {
                    index,
                    kind: marker.kind,
                });
            }
            if index == 0 && marker.position != 0 {
                warnings.push(LipWarning::FirstPosition(marker.position));
            }
            if index > 0 && marker.position < self.markers[index - 1].position {
                warnings.push(LipWarning::Decreasing {
                    index,
                    position: marker.position,
                });
            }
        }
        warnings
    }
    /// Phoneme *j* under the §3.12 safe rule: a phoneme of 42 or more, or
    /// an index past the phoneme table, is phoneme 0.
    pub fn phoneme(&self, index: usize) -> u8 {
        self.phonemes
            .get(index)
            .copied()
            .filter(|value| *value < PHONEMES)
            .unwrap_or(0)
    }
    /// Marker *j*'s position; a negative position counts as 0 (the §3.12
    /// start-skip safe rule), and past the table it never passes.
    pub fn position(&self, index: usize) -> u64 {
        self.markers
            .get(index)
            .map_or(u64::MAX, |marker| marker.position.max(0) as u64)
    }
}

/// §3.10 lip file: `sound/speech/<head>/<stem>.LIP`, with the speech stem
/// of the message's sound field. A head stem with a dot is rejected, as
/// for the audio path.
pub fn lip_path(head: &[u8], sound: &[u8]) -> Option<String> {
    if head.contains(&b'.') {
        return None;
    }
    Some(format!(
        "sound/speech/{}/{}.LIP",
        String::from_utf8_lossy(head),
        String::from_utf8_lossy(sfx::speech_stem(sound))
    ))
}

/// §3.12 start step 4: the speech plays at `trunc(0.69 x V)` of the
/// speech volume setting *V* (0-32767). §3.13's level table makes the
/// reference's double-precision product `[Must-match]`, so §3.12's
/// `floor(69 x V / 100)` safe rule is not used (they differ by one for
/// some multiples of 100, such as 300).
pub fn speech_volume(setting: i32) -> i32 {
    crate::sfx::speech_level(setting)
}
