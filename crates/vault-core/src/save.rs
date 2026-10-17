//! Engine-owned save envelope; this is not a legacy game save format.
//!
//! All integers are little-endian. The fixed 24-byte header contains:
//! `VAULTSAV` (8 bytes), version (u16), header size (u16), reserved zero flags
//! (u32), and payload length (u64). Version 1 leaves payload encoding to callers.

pub const SAVE_MAGIC: [u8; 8] = *b"VAULTSAV";
pub const SAVE_VERSION: u16 = 1;
pub const SAVE_HEADER_LEN: usize = 24;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SaveHeader {
    payload_len: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum SaveError {
    #[error("truncated save header: expected {SAVE_HEADER_LEN} bytes, got {0}")]
    TruncatedHeader(usize),
    #[error("invalid save magic")]
    InvalidMagic,
    #[error("unsupported save version {0}")]
    UnsupportedVersion(u16),
    #[error("unsupported save header length {0}")]
    InvalidHeaderLength(u16),
    #[error("unsupported save flags {0:#x}")]
    UnsupportedFlags(u32),
    #[error("save payload length mismatch: header declares {declared}, got {actual}")]
    PayloadLength { declared: u64, actual: usize },
}

impl SaveHeader {
    pub const fn new(payload_len: u64) -> Self {
        Self { payload_len }
    }

    pub const fn version(self) -> u16 {
        SAVE_VERSION
    }

    pub const fn payload_len(self) -> u64 {
        self.payload_len
    }

    pub fn encode(self) -> [u8; SAVE_HEADER_LEN] {
        let mut bytes = [0; SAVE_HEADER_LEN];
        bytes[..8].copy_from_slice(&SAVE_MAGIC);
        bytes[8..10].copy_from_slice(&SAVE_VERSION.to_le_bytes());
        bytes[10..12].copy_from_slice(&(SAVE_HEADER_LEN as u16).to_le_bytes());
        bytes[16..24].copy_from_slice(&self.payload_len.to_le_bytes());
        bytes
    }

    /// Decode the header prefix without allocating or interpreting the payload.
    pub fn decode(bytes: &[u8]) -> Result<Self, SaveError> {
        let header = bytes
            .get(..SAVE_HEADER_LEN)
            .ok_or(SaveError::TruncatedHeader(bytes.len()))?;
        if header[..8] != SAVE_MAGIC {
            return Err(SaveError::InvalidMagic);
        }
        let version = u16::from_le_bytes([header[8], header[9]]);
        if version != SAVE_VERSION {
            return Err(SaveError::UnsupportedVersion(version));
        }
        let header_len = u16::from_le_bytes([header[10], header[11]]);
        if usize::from(header_len) != SAVE_HEADER_LEN {
            return Err(SaveError::InvalidHeaderLength(header_len));
        }
        let flags = u32::from_le_bytes(header[12..16].try_into().expect("fixed header slice"));
        if flags != 0 {
            return Err(SaveError::UnsupportedFlags(flags));
        }
        let payload_len =
            u64::from_le_bytes(header[16..24].try_into().expect("fixed header slice"));
        Ok(Self::new(payload_len))
    }
}

/// Validate the entire envelope, rejecting truncated payloads and trailing data.
///
/// Never allocate based on the untrusted length in the header.
pub fn decode_envelope(bytes: &[u8]) -> Result<(SaveHeader, &[u8]), SaveError> {
    let header = SaveHeader::decode(bytes)?;
    let payload = &bytes[SAVE_HEADER_LEN..];
    if u64::try_from(payload.len()).ok() != Some(header.payload_len) {
        return Err(SaveError::PayloadLength {
            declared: header.payload_len,
            actual: payload.len(),
        });
    }
    Ok((header, payload))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture() -> Vec<u8> {
        include_str!("../../../tests/fixtures/core/save-v1.hex")
            .split_whitespace()
            .map(|byte| u8::from_str_radix(byte, 16).unwrap())
            .collect()
    }

    #[test]
    fn matches_the_documented_wire_format() {
        let bytes = fixture();
        let (header, payload) = decode_envelope(&bytes).unwrap();
        assert_eq!(header.version(), 1);
        assert_eq!(header.payload_len(), 4);
        assert_eq!(payload, b"TEST");
        assert_eq!(header.encode(), bytes[..SAVE_HEADER_LEN]);
        assert_eq!(
            SaveHeader::decode(&SaveHeader::new(u64::MAX).encode()).unwrap(),
            SaveHeader::new(u64::MAX)
        );
        assert!(decode_envelope(&SaveHeader::new(0).encode()).is_ok());
    }

    #[test]
    fn every_truncated_header_is_rejected() {
        let bytes = fixture();
        for length in 0..SAVE_HEADER_LEN {
            assert_eq!(
                SaveHeader::decode(&bytes[..length]),
                Err(SaveError::TruncatedHeader(length))
            );
        }
    }

    #[test]
    fn unknown_magic_version_header_size_and_flags_are_rejected() {
        let cases = [
            (0, 0, SaveError::InvalidMagic),
            (8, 2, SaveError::UnsupportedVersion(2)),
            (10, 25, SaveError::InvalidHeaderLength(25)),
            (12, 1, SaveError::UnsupportedFlags(1)),
        ];
        for (offset, value, expected) in cases {
            let mut bytes = fixture();
            bytes[offset] = value;
            assert_eq!(SaveHeader::decode(&bytes), Err(expected));
        }
    }

    #[test]
    fn truncated_trailing_and_extreme_payload_lengths_are_rejected() {
        let mut bytes = fixture();
        bytes.pop();
        assert!(matches!(
            decode_envelope(&bytes),
            Err(SaveError::PayloadLength {
                declared: 4,
                actual: 3
            })
        ));
        bytes.extend_from_slice(b"XX");
        assert!(matches!(
            decode_envelope(&bytes),
            Err(SaveError::PayloadLength {
                declared: 4,
                actual: 5
            })
        ));
        assert!(matches!(
            decode_envelope(&SaveHeader::new(u64::MAX).encode()),
            Err(SaveError::PayloadLength {
                declared: u64::MAX,
                actual: 0
            })
        ));
    }
}
