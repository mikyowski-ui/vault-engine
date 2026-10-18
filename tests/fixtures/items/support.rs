//! SPEC-011 §4 synthetic fixtures: the committed item prototypes, placeholder
//! message catalogs with the documented conversions, a test asset set, and
//! world builders.
use crate::{
    CharacterSheet, ObjectRef, ParkMiller, RandomSource, WorldError, WorldHost, WorldState,
    assets::WorldAssets,
};
use std::{collections::BTreeMap, sync::Arc};
use vault_formats::{MsgCatalog, Prototype};

pub(crate) use crate::fixture_assets as assets;
pub(crate) use crate::fixture_world as world;

pub const DRUG_A: u32 = 0x400;
pub const DRUG_B: u32 = 0x401;
pub const AMMO_X: u32 = 0x500;
pub const WEAPON_W: u32 = 0x501;
pub const ARMOR_A: u32 = 0x502;
pub const ARMOR_B: u32 = 0x503;
pub const CONTAINER: u32 = 0x504;
pub const MONEY: u32 = 41;

/// A committed fixture prototype's bytes.
pub fn bytes(pid: u32) -> Vec<u8> {
    assets::sparse(match pid {
        DRUG_A => include_str!("drug-A.pro.hex"),
        DRUG_B => include_str!("drug-B.pro.hex"),
        AMMO_X => include_str!("ammo-X.pro.hex"),
        WEAPON_W => include_str!("weapon-W.pro.hex"),
        ARMOR_A => include_str!("armor-A.pro.hex"),
        ARMOR_B => include_str!("armor-B.pro.hex"),
        CONTAINER => include_str!("container.pro.hex"),
        _ => panic!("no SPEC-011 fixture prototype {pid:#x}"),
    })
}
pub fn parse(bytes: &[u8]) -> Arc<Prototype> {
    Arc::new(Prototype::parse(bytes).expect("fixture prototype parses"))
}
pub fn prototype(pid: u32) -> Arc<Prototype> {
    parse(&bytes(pid))
}
/// A fixture prototype under another PID.
pub fn renumbered(pid: u32, as_pid: u32) -> Arc<Prototype> {
    let mut bytes = bytes(pid);
    world::word(&mut bytes, 0, as_pid as i32);
    parse(&bytes)
}
/// A plain misc item (no power cell) with size, weight, and cost.
pub fn misc(pid: u32, size: i32, weight: i32, cost: i32) -> Arc<Prototype> {
    let mut bytes = world::prototype(0, 5);
    world::word(&mut bytes, 0, pid as i32);
    world::word(&mut bytes, 0x28, size);
    world::word(&mut bytes, 0x2c, weight);
    world::word(&mut bytes, 0x30, cost);
    world::word(&mut bytes, 0x39, -1);
    parse(&bytes)
}
/// A misc item powered by `cell`.
pub fn powered(pid: u32, cell: u32) -> Arc<Prototype> {
    let mut bytes = world::prototype(0, 5);
    world::word(&mut bytes, 0, pid as i32);
    world::word(&mut bytes, 0x39, cell as i32);
    parse(&bytes)
}
/// SPEC-011 drug payload (file 0x39) from its 17 words.
pub fn drug(pid: u32, payload: [i32; 17]) -> Arc<Prototype> {
    let mut bytes = world::prototype(0, 2);
    world::word(&mut bytes, 0, pid as i32);
    for (index, value) in payload.into_iter().enumerate() {
        world::word(&mut bytes, 0x39 + 4 * index, value);
    }
    parse(&bytes)
}
/// A critter prototype: SPECIAL 5, the given base-stat overrides, body type,
/// and invested skill points.
pub fn critter(
    pid: u32,
    stats: &[(usize, i32)],
    body: i32,
    skills: &[(usize, i32)],
) -> Arc<Prototype> {
    let mut bytes = world::prototype(1, 0);
    world::word(&mut bytes, 0, pid as i32);
    for stat in 0..7 {
        world::word(&mut bytes, 48 + 4 * stat, 5);
    }
    world::word(&mut bytes, 48 + 4 * 7, 30);
    world::word(&mut bytes, 48 + 4 * 8, 8);
    world::word(&mut bytes, 48 + 4 * 12, 150);
    for (stat, value) in stats {
        world::word(&mut bytes, 48 + 4 * stat, *value);
    }
    for (skill, points) in skills {
        world::word(&mut bytes, 328 + 4 * skill, *points);
    }
    world::word(&mut bytes, 400, body);
    parse(&bytes)
}

/// Placeholder catalogs keeping the SPEC-011 message numbers and their
/// conversions; no original text is reproduced.
fn catalog(text: &str) -> Arc<MsgCatalog> {
    Arc::new(MsgCatalog::parse(text.as_bytes()).expect("fixture catalog parses"))
}
pub fn catalogs() -> BTreeMap<&'static str, Arc<MsgCatalog>> {
    let mut stat = String::new();
    for index in 0..38 {
        stat.push_str(&format!("{{{}}}{{}}{{stat{index}}}\n", 100 + index));
    }
    stat.push_str("{600}{}{level up}\n");
    let mut perk = String::new();
    for index in 0..64 {
        // Names sort in reverse index order, unlike the perk numbers.
        perk.push_str(&format!(
            "{{{}}}{{}}{{perk {:02}}}\n",
            101 + index,
            99 - index
        ));
        perk.push_str(&format!("{{{}}}{{}}{{about {index}}}\n", 201 + index));
    }
    BTreeMap::from([
        (
            "item.msg",
            catalog(
                "{1}{}{gained %d %s}\n{2}{}{lost %d %s}\n{3}{}{withdrawal ends}\n{10}{}{nothing}\n",
            ),
        ),
        ("stat.msg", catalog(&stat)),
        ("perk.msg", catalog(&perk)),
        (
            "proto.msg",
            catalog(
                "{581}{}{use %s on %s}\n{582}{}{no use}\n{583}{}{dead 0}\n{584}{}{dead 1}\n{585}{}{dead 2}\n{586}{}{dead 3}\n",
            ),
        ),
        (
            "inventry.msg",
            catalog("{19}{}{no time}\n{27}{}{deal}\n{28}{}{no deal}\n"),
        ),
        ("misc.msg", catalog("{5000}{}{uses}\n")),
        ("combat.msg", catalog("{107}{}{too heavy}\n")),
    ])
}

#[derive(Clone)]
pub struct ItemAssets {
    pub prototypes: BTreeMap<u32, Arc<Prototype>>,
    pub catalogs: BTreeMap<&'static str, Arc<MsgCatalog>>,
}
impl Default for ItemAssets {
    fn default() -> Self {
        let prototypes = [
            DRUG_A, DRUG_B, AMMO_X, WEAPON_W, ARMOR_A, ARMOR_B, CONTAINER,
        ]
        .into_iter()
        .map(|pid| (pid, prototype(pid)))
        .chain([(MONEY, misc(MONEY, 0, 0, 1))])
        .collect();
        Self {
            prototypes,
            catalogs: catalogs(),
        }
    }
}
impl WorldAssets for ItemAssets {
    fn prototype(&self, pid: u32) -> Result<Arc<Prototype>, WorldError> {
        self.prototypes
            .get(&pid)
            .cloned()
            .ok_or_else(|| WorldError::Invalid(format!("no prototype {pid:#x}")))
    }
    fn program(&self, _index: i32) -> Result<(String, vault_vm::IntProgram), WorldError> {
        Err(WorldError::Invalid("no programs".into()))
    }
    fn message(&self, _list: i32, _id: i32) -> Option<vault_formats::msg::Message> {
        None
    }
    fn game_messages(&self, file: &str) -> Result<Arc<MsgCatalog>, WorldError> {
        self.catalogs
            .get(file)
            .cloned()
            .ok_or_else(|| WorldError::Invalid(format!("{file} is missing")))
    }
}

/// A host whose player has the default sheet, with campaign variables.
pub fn host() -> WorldHost<ItemAssets> {
    let mut world = WorldState::new(CharacterSheet::default(), 1).unwrap();
    world.gvars = Some(vec![0; 1200]);
    WorldHost::new(world, ItemAssets::default())
}
/// Place a new instance of a known prototype at hex 0.
pub fn spawn(host: &mut WorldHost<ItemAssets>, pid: u32) -> ObjectRef {
    let prototype = host.assets.prototype(pid).unwrap();
    host.world.spawn(prototype, 0, 0).unwrap()
}
/// A new instance carried by `owner` as an entry of `quantity` (forced).
pub fn carry(
    host: &mut WorldHost<ItemAssets>,
    owner: ObjectRef,
    pid: u32,
    quantity: i32,
) -> ObjectRef {
    let item = spawn(host, pid);
    host.world.add_item(owner, item, quantity, true).unwrap();
    item
}
/// A generator state (a position in one seeded stream) whose next range
/// draws satisfy `accept`, so a fixture can follow the branch an explicit
/// draw names.
pub fn seed_where(draws: &[(i32, i32)], accept: impl Fn(&[i32]) -> bool) -> ParkMiller {
    let mut stream = ParkMiller::seeded(12345).unwrap();
    for _ in 0..100_000 {
        let mut probe = stream.clone();
        let values: Vec<_> = draws
            .iter()
            .map(|(low, high)| probe.range(*low, *high).unwrap())
            .collect();
        if accept(&values) {
            return stream;
        }
        stream.range(1, 100).unwrap();
    }
    panic!("no stream position gives the requested draws")
}
/// Messages emitted since the last clear.
pub fn messages(host: &WorldHost<ItemAssets>) -> Vec<String> {
    host.world
        .effects
        .iter()
        .filter_map(|effect| match effect {
            crate::WorldEffect::Message(text) => Some(String::from_utf8_lossy(text).into_owned()),
            _ => None,
        })
        .collect()
}
