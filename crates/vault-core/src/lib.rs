//! Deterministic, platform-independent engine foundations.

pub mod handles;
pub mod save;
pub mod sequence;

pub use handles::{EntityId, EntityStore};
pub use sequence::{CommandId, EventId, SequenceExhausted, SequenceIds};
