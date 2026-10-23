//! Deterministic, platform-independent engine foundations.

pub mod ai;
pub mod assets;
pub mod barter;
pub mod choreography;
pub mod clicks;
pub mod combat;
pub mod combat_state;
pub mod critical;
pub mod dialogue;
pub mod drugs;
pub mod handles;
pub mod host;
mod host_ai;
mod host_animation;
pub mod host_choreography;
mod host_combat;
mod host_drugs;
mod host_fidget;
mod host_game;
mod host_items;
mod host_motion;
mod host_movement;
mod host_movie;
pub mod host_rest;
mod host_revisit;
pub mod host_save;
mod host_skills;
mod host_sound;
mod host_travel;
pub mod items;
pub mod library;
pub mod loot;
pub mod motion;
pub mod movie;
pub mod outline;
pub mod persistence;
pub mod progression;
pub mod queue;
pub mod rng;
pub mod roof;
pub mod routing;
pub mod sequence;
pub mod skills;
pub mod stats;
pub mod ui;
pub mod world;
pub mod worldmap;
pub use host::{VmObserver, WorldHost};
pub use host_combat::{Refusal, Spray, spray_budgets};
pub use host_movement::{Movement, StepOutcome};
pub use host_skills::BookRoute;
pub use host_travel::{TravelOutcome, WorldMapExit};
pub use world::{
    ActiveExplosion, ExplosionRequest, LootRequest, ObjectAction, ObjectRef, ScriptEvent,
    Transition, WorldEffect, WorldRules, WorldState,
};

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
mod ai_tests;
#[cfg(test)]
mod barter_tests;
#[cfg(test)]
mod choreography_tests;
#[cfg(test)]
mod clicks_tests;
#[cfg(test)]
mod combat_tests;
#[cfg(test)]
mod dialogue_tests;
#[cfg(test)]
mod drugs_tests;
#[cfg(test)]
#[allow(dead_code)]
#[path = "../../../tests/fixtures/formats/support.rs"]
pub(crate) mod fixture_assets;
#[cfg(test)]
#[allow(dead_code)]
#[path = "../../../tests/fixtures/items/support.rs"]
pub(crate) mod fixture_items;
#[cfg(test)]
#[allow(dead_code)]
#[path = "../../../tests/fixtures/state/support.rs"]
pub(crate) mod fixture_state;
#[cfg(test)]
#[allow(dead_code)]
#[path = "../../../tests/fixtures/vm/support.rs"]
pub(crate) mod fixture_vm;
#[cfg(test)]
#[allow(dead_code)]
#[path = "../../../tests/fixtures/world/support.rs"]
pub(crate) mod fixture_world;
#[cfg(test)]
mod host_ai_tests;
#[cfg(test)]
mod host_sound_tests;
#[cfg(test)]
mod items_tests;
#[cfg(test)]
mod lighting_tests;
#[cfg(test)]
mod motion_tests;
#[cfg(test)]
mod movie_tests;
#[cfg(test)]
mod persistence_tests;
#[cfg(test)]
mod player_art_tests;
#[cfg(test)]
mod progression_tests;
#[cfg(test)]
mod roof_tests;
#[cfg(test)]
mod script_cycle_tests;
#[cfg(test)]
mod skills_tests;
#[cfg(test)]
mod state_tests;
#[cfg(test)]
mod worldmap_tests;

pub use handles::{EntityId, EntityStore};
pub use sequence::{CommandId, EventId, SequenceExhausted, SequenceIds};
