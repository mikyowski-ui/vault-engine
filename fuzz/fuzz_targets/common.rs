//! Shared by the targets that read object records: a prototype lookup that
//! resolves every PID from its own bits, so each prototype-dependent record
//! layout is reachable without a prototype corpus.
use vault_formats::{
    ParseError,
    pro::{ItemSubtype, PrototypeInfo, PrototypeKind, ScenerySubtype},
};

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

/// The type byte selects the category and the low byte the subtype.
pub fn lookup(pid: u32) -> Result<PrototypeInfo, ParseError> {
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
