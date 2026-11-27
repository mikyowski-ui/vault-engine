//! The `VTLX` tile remap sidecar of a mod layer
//! (`docs/extended-art-namespaces.md` section 3.5).
//!
//! A map's tile words keep their 12-bit floor and roof indexes (SPEC-003
//! §2.8); a `[[tile_remap]]` sidecar maps some of those local indexes to
//! rows of the resolved tile list, which may lie past row 4095. Layout,
//! big-endian:
//!
//! | Offset | Size | Field |
//! | ---: | ---: | --- |
//! | 0 | 4 | magic `56 54 4C 58` (`VTLX`) |
//! | 4 | 2 | version, 1 |
//! | 6 | 2 | entry count N, at most 4096 |
//! | 8 | 6 × N | local index (2 bytes, 0-4095, never 1) and row (4 bytes, at most 16,777,215) |
//!
//! Entries ascend by local index without repeats, and nothing follows the
//! last one. A listed local index draws its mapped row; an unlisted one
//! draws its own index, and index 1 stays the blank tile.
use crate::ParseError;

/// The sidecar's magic bytes.
pub const MAGIC: [u8; 4] = *b"VTLX";
/// The one layout version.
pub const VERSION: u16 = 1;
/// The most entries a sidecar holds.
pub const MAX_ENTRIES: usize = 4096;
/// The largest local index (a 12-bit tile word index).
pub const MAX_LOCAL: u16 = 4095;
/// The blank tile's local index, which a sidecar may not list.
pub const BLANK_TILE: u16 = 1;
/// The largest mapped row.
pub const MAX_ROW: u32 = 0x00ff_ffff;
const FORMAT: &str = "TLX";
const HEADER: usize = 8;
const ENTRY: usize = 6;

/// A parsed tile remap: (local index, row) pairs ascending by local index.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct TileRemap {
    entries: Vec<(u16, u32)>,
}

impl TileRemap {
    /// Parse a sidecar; any deviation from the layout is an error naming the
    /// byte offset.
    pub fn parse(bytes: &[u8]) -> Result<Self, ParseError> {
        if bytes.len() > crate::MAX_ASSET_BYTES {
            return Err(ParseError::new(FORMAT, 0, "asset exceeds 64 MiB limit"));
        }
        if bytes.len() < HEADER {
            return Err(ParseError::new(
                FORMAT,
                bytes.len(),
                format!("{} bytes is shorter than the 8-byte header", bytes.len()),
            ));
        }
        if bytes[..4] != MAGIC {
            return Err(ParseError::new(FORMAT, 0, "magic is not VTLX"));
        }
        let version = u16::from_be_bytes([bytes[4], bytes[5]]);
        if version != VERSION {
            return Err(ParseError::new(
                FORMAT,
                4,
                format!("version {version} is not {VERSION}"),
            ));
        }
        let count = usize::from(u16::from_be_bytes([bytes[6], bytes[7]]));
        if count > MAX_ENTRIES {
            return Err(ParseError::new(
                FORMAT,
                6,
                format!("{count} entries is more than {MAX_ENTRIES}"),
            ));
        }
        let end = HEADER + count * ENTRY;
        if bytes.len() < end {
            return Err(ParseError::new(
                FORMAT,
                bytes.len(),
                format!(
                    "{count} entries need {end} bytes, and the sidecar has {}",
                    bytes.len()
                ),
            ));
        }
        if bytes.len() > end {
            return Err(ParseError::new(
                FORMAT,
                end,
                format!("{} trailing bytes after the last entry", bytes.len() - end),
            ));
        }
        let pairs = bytes[HEADER..end]
            .as_chunks::<ENTRY>()
            .0
            .iter()
            .map(|entry| {
                (
                    u16::from_be_bytes([entry[0], entry[1]]),
                    u32::from_be_bytes([entry[2], entry[3], entry[4], entry[5]]),
                )
            });
        Self::validated(pairs, |at| HEADER + at * ENTRY)
    }

    /// A remap from (local index, row) pairs, under the same rules as
    /// [`Self::parse`]: at most 4096, ascending by local index without
    /// repeats, local index 0-4095 and not 1, row at most 16,777,215.
    pub fn from_pairs(pairs: impl IntoIterator<Item = (u16, u32)>) -> Result<Self, ParseError> {
        Self::validated(pairs, |_| 0)
    }

    /// Check `pairs`; `offset` gives entry `at`'s byte offset for an error.
    fn validated(
        pairs: impl IntoIterator<Item = (u16, u32)>,
        offset: impl Fn(usize) -> usize,
    ) -> Result<Self, ParseError> {
        let mut entries: Vec<(u16, u32)> = Vec::new();
        for (at, (local, row)) in pairs.into_iter().enumerate() {
            let fail = |message: String| Err(ParseError::new(FORMAT, offset(at), message));
            if at == MAX_ENTRIES {
                return fail(format!("more than {MAX_ENTRIES} entries"));
            }
            if local > MAX_LOCAL {
                return fail(format!(
                    "entry {at}: local index {local} is outside 0..={MAX_LOCAL}"
                ));
            }
            if local == BLANK_TILE {
                return fail(format!(
                    "entry {at}: local index {BLANK_TILE} is the blank tile and cannot be remapped"
                ));
            }
            if let Some(&(previous, _)) = entries.last()
                && local <= previous
            {
                return fail(format!(
                    "entry {at}: local index {local} does not ascend past {previous}"
                ));
            }
            if row > MAX_ROW {
                return fail(format!("entry {at}: row {row} is above {MAX_ROW}"));
            }
            entries.push((local, row));
        }
        Ok(Self { entries })
    }

    /// The sidecar bytes, which [`Self::parse`] reads back to this remap.
    pub fn to_bytes(&self) -> Vec<u8> {
        let mut bytes = Vec::with_capacity(HEADER + self.entries.len() * ENTRY);
        bytes.extend_from_slice(&MAGIC);
        bytes.extend_from_slice(&VERSION.to_be_bytes());
        let count = u16::try_from(self.entries.len()).expect("at most 4096 entries");
        bytes.extend_from_slice(&count.to_be_bytes());
        for (local, row) in &self.entries {
            bytes.extend_from_slice(&local.to_be_bytes());
            bytes.extend_from_slice(&row.to_be_bytes());
        }
        bytes
    }

    /// The resolved tile list row local index `local` draws: its mapped row,
    /// or `local` itself when unlisted.
    pub fn row(&self, local: u16) -> u32 {
        self.entries
            .binary_search_by_key(&local, |(listed, _)| *listed)
            .map_or(u32::from(local), |at| self.entries[at].1)
    }

    /// The (local index, row) pairs, ascending by local index.
    pub fn entries(&self) -> &[(u16, u32)] {
        &self.entries
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
}

#[cfg(test)]
#[path = "tile_remap_tests.rs"]
mod tests;
