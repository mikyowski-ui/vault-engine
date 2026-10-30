//! SPEC-024b: what sfall mode adds to the world. It exists only with the
//! mode on ([`crate::WorldState::sfall`]); with it off none of this does,
//! and nothing here reaches SPEC-021's `state_hash`.
use std::collections::BTreeMap;
use vault_vm::sfall::SfallValue;

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
}

/// The world's sfall state.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SfallWorld {
    /// §4.9 each value keeps its type.
    pub globals: BTreeMap<GlobalKey, SfallValue>,
}
