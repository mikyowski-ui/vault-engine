//! SPEC-003 `.MAP` files, then the vault-core import that turns a parsed
//! map into world objects, scripts, and variable pools.
#![no_main]

use libfuzzer_sys::fuzz_target;
use std::sync::Arc;
use vault_core::{WorldError, WorldState, assets::WorldAssets};
use vault_formats::{
    MapFile, ParseError, Prototype,
    pro::{ItemSubtype, PrototypeInfo, PrototypeKind, ScenerySubtype},
};
use vault_vm::IntProgram;

const ITEMS: [ItemSubtype; 7] = [
    ItemSubtype::Armor,
    ItemSubtype::Container,
    ItemSubtype::Drug,
    ItemSubtype::Weapon,
    ItemSubtype::Ammunition,
    ItemSubtype::Misc,
    ItemSubtype::Key,
];
const SCENERY: [ScenerySubtype; 6] = [
    ScenerySubtype::Door,
    ScenerySubtype::Stairs,
    ScenerySubtype::Elevator,
    ScenerySubtype::LadderUp,
    ScenerySubtype::LadderDown,
    ScenerySubtype::Generic,
];

/// Every PID resolves from its own bits, so the fuzzer reaches each
/// prototype-dependent object layout without a prototype corpus.
fn lookup(pid: u32) -> Result<PrototypeInfo, ParseError> {
    let index = (pid & 0xff) as usize;
    let kind = match pid >> 24 {
        0 => PrototypeKind::Item(ITEMS[index % ITEMS.len()]),
        1 => PrototypeKind::Critter,
        2 => PrototypeKind::Scenery(SCENERY[index % SCENERY.len()]),
        3 => PrototypeKind::Wall,
        4 => PrototypeKind::Tile,
        5 => PrototypeKind::Misc,
        _ => {
            return Err(ParseError {
                format: "PRO",
                offset: 0,
                message: "unknown prototype type".into(),
            });
        }
    };
    let weapon_defaults =
        matches!(kind, PrototypeKind::Item(ItemSubtype::Weapon)).then_some((-1, 0));
    Ok(PrototypeInfo {
        kind,
        weapon_defaults,
    })
}

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
    let Ok(map) = MapFile::parse(data, &lookup) else {
        return;
    };
    if let Ok(mut world) = WorldState::empty(0) {
        let _ = world.import_map(map, &NoAssets);
    }
});
