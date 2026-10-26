//! Deterministic, platform-independent engine foundations.

pub mod ai;
pub mod assets;
pub mod barter;
pub mod camera;
pub mod choreography;
pub mod clicks;
pub mod combat;
pub mod combat_messages;
pub mod combat_state;
pub mod companion;
pub mod critical;
pub mod debug;
pub mod dialogue;
pub mod doors;
pub mod drugs;
pub mod fallout2;
pub mod fallout2_party;
pub mod floating;
pub mod handles;
pub mod host;
mod host_ai;
mod host_animation;
pub mod host_choreography;
mod host_combat;
mod host_drugs;
mod host_fallout2;
pub mod host_fallout2_party;
mod host_fallout2_words;
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
pub mod host_talk;
mod host_travel;
pub mod items;
pub mod library;
pub mod loot;
pub mod mod_export;
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
pub mod tables;
pub mod ui;
pub mod world;
pub mod worldmap;
pub use host::{VmObserver, WorldHost};
pub use host_combat::{Refusal, Spray, spray_budgets};
pub use host_fallout2::Fallout2Travel;
pub use host_movement::{Movement, StepOutcome};
pub use host_skills::BookRoute;
pub use host_travel::{TravelOutcome, WorldMapExit};
pub use world::{
    ActiveExplosion, EnginePreset, ExplosionRequest, GameProfile, LootRequest, ObjectAction,
    ObjectRef, ScriptEvent, Transition, WorldEffect, WorldRules, WorldState,
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
mod camera_tests;
#[cfg(test)]
mod choreography_tests;
#[cfg(test)]
mod clicks_tests;
#[cfg(test)]
mod combat_messages_tests;
#[cfg(test)]
mod combat_tests;
#[cfg(test)]
mod debug_tests;
#[cfg(test)]
mod dialogue_tests;
#[cfg(test)]
mod drugs_tests;
#[cfg(test)]
mod fallout2_tests;
#[cfg(test)]
mod spec019_tests;
#[cfg(test)]
pub(crate) use vault_fixtures::formats as fixture_assets;
#[cfg(test)]
#[path = "../../../tests/fixtures/items/support.rs"]
pub(crate) mod fixture_items;
#[cfg(test)]
pub(crate) use vault_fixtures::state as fixture_state;
#[cfg(test)]
pub(crate) use vault_fixtures::vm as fixture_vm;
#[cfg(test)]
pub(crate) use vault_fixtures::world as fixture_world;
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
mod script_request_tests;
#[cfg(test)]
mod self_stop_tests;
#[cfg(test)]
mod skills_tests;
#[cfg(test)]
mod spec017_tests;
#[cfg(test)]
mod state_tests;
#[cfg(test)]
mod tables_tests;
#[cfg(test)]
mod talk_tests;
#[cfg(test)]
mod test_support;
#[cfg(test)]
mod worldmap_tests;

pub use handles::{EntityId, EntityStore};
pub use sequence::{CommandId, EventId, SequenceExhausted, SequenceIds};
