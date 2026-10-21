//! SPEC-003 `.MAP` files, then the vault-core import that turns a parsed
//! map into world objects, scripts, and variable pools.
#![no_main]

#[path = "common.rs"]
mod common;

use libfuzzer_sys::fuzz_target;
use std::sync::Arc;
use vault_core::{WorldError, WorldState, assets::WorldAssets};
use vault_formats::{MapFile, Prototype};
use vault_vm::IntProgram;

/// No prototypes, programs, or messages: the import must fail cleanly or
/// fall back wherever a resource is missing.
struct NoAssets;
impl WorldAssets for NoAssets {
    fn prototype(&self, pid: u32) -> Result<Arc<Prototype>, WorldError> {
        Err(WorldError::Invalid(format!(
            "prototype {pid:#x} unavailable"
        )))
    }
    fn program(&self, index: i32) -> Result<(String, IntProgram), WorldError> {
        Err(WorldError::Invalid(format!("program {index} unavailable")))
    }
    fn message(&self, _list: i32, _id: i32) -> Option<vault_formats::msg::Message> {
        None
    }
}

fuzz_target!(|data: &[u8]| {
    let Ok(map) = MapFile::parse(data, &common::lookup) else {
        return;
    };
    if let Ok(mut world) = WorldState::empty(0) {
        let _ = world.import_map(map, &NoAssets);
    }
});
