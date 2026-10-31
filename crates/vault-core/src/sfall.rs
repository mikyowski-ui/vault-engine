//! SPEC-024b: what sfall mode adds to the world. It exists only with the
//! mode on ([`crate::WorldState::sfall`]); with it off none of this does,
//! and nothing here reaches SPEC-021's `state_hash`.
use crate::state_hash::fnv1a64;
use std::collections::BTreeMap;
pub use vault_vm::sfall::SfallValue;
use vault_vm::sfall_arrays::{self, ArrayStore, SidecarError};

/// The save sidecar, beside the slot's `SAVE.DAT` (§5.8).
pub const SIDECAR: &str = "VAULTSFL.DAT";

/// A sfall global's key (§4.9): any int, or a string of exactly 8 bytes.
/// Numbers order before strings, as §5.6 asks of the sidecar.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub enum GlobalKey {
    Int(i32),
    Text([u8; 8]),
}
impl GlobalKey {
    /// The key of a class-`K` operand; a string of another length is a
    /// failed operand.
    pub fn of(value: &SfallValue) -> Option<Self> {
        match value {
            SfallValue::Int(key) => Some(Self::Int(*key)),
            SfallValue::Text(text) => text.as_slice().try_into().ok().map(Self::Text),
            SfallValue::Float(_) => None,
        }
    }
    fn value(&self) -> SfallValue {
        match self {
            Self::Int(key) => SfallValue::Int(*key),
            Self::Text(text) => SfallValue::text(text.as_slice()),
        }
    }
}

/// The world's sfall state.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SfallWorld {
    /// §4.9 each value keeps its type.
    pub globals: BTreeMap<GlobalKey, SfallValue>,
    /// §5: shared by all scripts.
    pub arrays: ArrayStore,
}
impl SfallWorld {
    fn global_records(&self) -> Vec<(SfallValue, SfallValue)> {
        self.globals
            .iter()
            .map(|(key, value)| (key.value(), value.clone()))
            .collect()
    }
    /// `VAULTSFL.DAT`: the globals and the saved arrays (§5.8).
    pub fn sidecar(&self) -> Vec<u8> {
        sfall_arrays::write_stream(
            sfall_arrays::SIDECAR_TAG,
            &self.arrays,
            &self.global_records(),
            true,
        )
    }
    /// The extension stream: the same layout with every live array (§5.9).
    pub fn extension_stream(&self) -> Vec<u8> {
        sfall_arrays::write_stream(
            sfall_arrays::STREAM_TAG,
            &self.arrays,
            &self.global_records(),
            false,
        )
    }
    /// A sidecar's state, parsed whole so that a bad file changes nothing.
    pub fn from_sidecar(bytes: &[u8]) -> Result<Self, SidecarError> {
        let (arrays, records) = sfall_arrays::parse_sidecar(bytes)?;
        let mut globals = BTreeMap::new();
        for (key, value) in records {
            let parsed =
                GlobalKey::of(&key).ok_or_else(|| SidecarError::GlobalKey(format!("{key:?}")))?;
            globals.insert(parsed, value);
        }
        Ok(Self { globals, arrays })
    }
}
impl crate::WorldState {
    /// SPEC-024b §5.9 the companion to `state_hash`: FNV-1a 64 of the
    /// extension stream. It takes `&self`, draws nothing and changes nothing,
    /// and with the mode off it is the empty stream's value (vector F3).
    pub fn sfall_hash(&self) -> u64 {
        fnv1a64(
            &self
                .sfall
                .as_deref()
                .cloned()
                .unwrap_or_default()
                .extension_stream(),
        )
    }
}
