//! Deterministic, platform-independent engine foundations.

pub mod assets;
pub mod dialogue;
pub mod handles;
pub mod host;
mod host_game;
pub mod library;
pub mod queue;
pub mod rng;
pub mod save;
pub mod sequence;
pub mod stats;
pub mod world;
pub use host::WorldHost;
pub use world::{ObjectAction, ObjectRef, ScriptEvent, WorldEffect, WorldRules, WorldState};

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum WorldError {
    #[error("{0}")]
    Invalid(String),
    #[error("world arithmetic exceeds defined range")]
    Arithmetic,
}
impl From<vault_formats::ParseError> for WorldError {
    fn from(error: vault_formats::ParseError) -> Self {
        Self::Invalid(error.to_string())
    }
}
impl From<vault_vm::IntError> for WorldError {
    fn from(error: vault_vm::IntError) -> Self {
        Self::Invalid(error.to_string())
    }
}
impl From<vault_vm::VmError> for WorldError {
    fn from(error: vault_vm::VmError) -> Self {
        Self::Invalid(error.to_string())
    }
}
impl From<WorldError> for vault_vm::VmError {
    fn from(error: WorldError) -> Self {
        Self::Host(error.to_string())
    }
}
pub use rng::{ParkMiller, RandomSource, RngState};
pub use stats::{CharacterSheet, Perk, StatContext, Trait};

#[cfg(test)]
#[allow(dead_code)]
#[path = "../../../tests/fixtures/formats/support.rs"]
pub(crate) mod fixture_assets;
#[cfg(test)]
#[allow(dead_code)]
#[path = "../../../tests/fixtures/world/support.rs"]
pub(crate) mod fixture_world;
#[cfg(test)]
mod state_tests;

pub use handles::{EntityId, EntityStore};
pub use sequence::{CommandId, EventId, SequenceExhausted, SequenceIds};
