//! SPEC-009 world integration: initialization, resolution, and fixtures C-J
//! through live world state, with draw accounting against a cloned
//! generator.
use crate::{
    CharacterSheet, ObjectRef, RandomSource, WorldEffect, WorldError, WorldHost, WorldState,
    ai::{Taunt, intent},
    assets::WorldAssets,
    combat::Resolution,
    combat_state::{CombatRequest, PacketAi},
    critical::{effect, hit_mode, location},
    fixture_world as fixture,
};
use std::{
    collections::{BTreeMap, BTreeSet},
    sync::Arc,
};
use vault_formats::{
    Fid, HexGrid, MsgCatalog, Prototype,
    map::{ObjectDetail, ObjectState},
    pro::BasicFlags,
};

/// One `ai.txt` section holding every key; `overrides` replace defaults.
fn section(name: &str, number: i32, overrides: &[(&str, &str)]) -> String {
    let number = number.to_string();
    let defaults = [
        ("packet_num", number.as_str()),
        ("max_dist", "8"),
        ("min_to_hit", "40"),
        ("min_hp", "0"),
        ("aggression", "0"),
        ("hurt_too_much", ""),
        ("secondary_freq", "4"),
        ("called_freq", "3"),
        ("font", "101"),
        ("color", "145"),
        ("outline_color", "-1"),
        ("chance", "100"),
        ("run_start", "1000"),
        ("move_start", "1010"),
        ("attack_start", "1020"),
        ("miss_start", "1030"),
        ("hit_head_start", "1040"),
        ("hit_left_arm_start", "1050"),
        ("hit_right_arm_start", "1060"),
        ("hit_torso_start", "1070"),
        ("hit_right_leg_start", "1080"),
        ("hit_left_leg_start", "1090"),
        ("hit_eyes_start", "1100"),
        ("hit_groin_start", "1110"),
        ("last_msg", "1119"),
    ];
    let mut text = format!("[{name}]\r\n");
    for (key, value) in defaults {
        let value = overrides
            .iter()
            .find(|(k, _)| *k == key)
            .map_or(value, |(_, v)| *v);
        text.push_str(&format!("{key}={value}\r\n"));
    }
    text
}
/// Packets: `Guard` (0), `Coward` (1, always flees), `Zeta` (2, chance 35),
/// `Brute` (3, the Fixture A groin range 2004..2003), `Rare` (4, chance 0).
fn packets() -> Vec<u8> {
    [
        section("Guard", 0, &[]),
        section("Coward", 1, &[("min_hp", "1000")]),
        section("Zeta", 2, &[("chance", "35")]),
        section(
            "Brute",
            3,
            &[
                ("hit_head_start", "2004"),
                ("hit_left_arm_start", "2004"),
                ("hit_right_arm_start", "2004"),
                ("hit_torso_start", "2004"),
                ("hit_right_leg_start", "2004"),
                ("hit_left_leg_start", "2004"),
                ("hit_eyes_start", "2004"),
                ("hit_groin_start", "2004"),
                ("last_msg", "2003"),
                ("hurt_too_much", "blind ,crippled"),
            ],
        ),
        section("Rare", 4, &[("chance", "0")]),
    ]
    .concat()
    .into_bytes()
}
const LONG_TAUNT: i32 = 1070;
fn taunts() -> Arc<MsgCatalog> {
    let mut text = String::new();
    for id in 1000..=1119 {
        let line = if id == LONG_TAUNT {
            "x".repeat(100)
        } else {
            format!("taunt {id}")
        };
        text.push_str(&format!("{{{id}}}{{}}{{{line}}}\n"));
    }
    text.push_str("{2004}{}{taunt 2004}\n");
    Arc::new(MsgCatalog::parse(text.as_bytes()).unwrap())
}
fn misc() -> Arc<MsgCatalog> {
    Arc::new(
        MsgCatalog::parse(b"{5000}{}{uses}\n{5001}{}{is out of ammo for}\n{5002}{}{reloads}\n")
            .unwrap(),
    )
}

#[derive(Clone)]
struct AiAssets {
    prototypes: BTreeMap<u32, Arc<Prototype>>,
    ai: Option<Vec<u8>>,
    taunts: Option<Arc<MsgCatalog>>,
    misc: Option<Arc<MsgCatalog>>,
    combat: Option<Arc<MsgCatalog>>,
    /// Critter animation codes without art.
    missing: BTreeSet<u8>,
    /// Loadable art by identifier.
    art: BTreeMap<u32, Arc<vault_formats::FrmArchive>>,
}
impl Default for AiAssets {
    fn default() -> Self {
        Self {
            prototypes: BTreeMap::new(),
            ai: Some(packets()),
            taunts: Some(taunts()),
            misc: Some(misc()),
            combat: None,
            missing: BTreeSet::new(),
            art: BTreeMap::new(),
        }
    }
}
impl WorldAssets for AiAssets {
    fn prototype(&self, pid: u32) -> Result<Arc<Prototype>, WorldError> {
        self.prototypes
            .get(&pid)
            .cloned()
            .ok_or_else(|| WorldError::Invalid("no prototype".into()))
    }
    fn program(&self, _index: i32) -> Result<(String, vault_vm::IntProgram), WorldError> {
        Err(WorldError::Invalid("no programs".into()))
    }
    fn message(&self, _list: i32, _id: i32) -> Option<vault_formats::msg::Message> {
        None
    }
    fn art_exists(&self, fid: u32) -> bool {
        !self.missing.contains(&Fid(fid).animation())
    }
    fn object_art(&self, fid: u32) -> Result<Arc<vault_formats::FrmArchive>, WorldError> {
        self.art
            .get(&fid)
            .cloned()
            .ok_or_else(|| WorldError::Invalid("object art unavailable".into()))
    }
    fn resource(&self, path: &str) -> Result<Vec<u8>, WorldError> {
        self.ai
            .clone()
            .filter(|_| path == "data/ai.txt")
            .ok_or_else(|| WorldError::Invalid(format!("{path} is missing")))
    }
    fn game_messages(&self, file: &str) -> Result<Arc<MsgCatalog>, WorldError> {
        match file {
            "combatai.msg" => self.taunts.clone(),
            "misc.msg" => self.misc.clone(),
            "combat.msg" => self.combat.clone(),
            _ => None,
        }
        .ok_or_else(|| WorldError::Invalid(format!("{file} is missing")))
    }
}

/// Player at 20100 facing 5, outside combat.
fn world() -> WorldState {
    let mut world = WorldState::new(CharacterSheet::default(), 1).unwrap();
    world.get_mut(world.player).unwrap().raw.orientation = 5;
    world
}
fn ai_host(world: WorldState, assets: AiAssets) -> WorldHost<AiAssets> {
    let mut host = WorldHost::new(world, assets);
    host.load_combat_ai().unwrap();
    host.world.effects.clear();
    host
}
/// A non-hidden NPC: SPECIAL 5, 30 max HP, 8 max AP, skill points 0, on
/// `team` with instance packet `packet`; `stats` override base stats.
fn npc(
    world: &mut WorldState,
    hex: i32,
    team: i32,
    packet: i32,
    stats: &[(usize, i32)],
) -> ObjectRef {
    let mut bytes = fixture::prototype(1, 0);
    for stat in 0..7 {
        fixture::word(&mut bytes, 48 + 4 * stat, 5);
    }
    fixture::word(&mut bytes, 48 + 4 * 7, 30);
    fixture::word(&mut bytes, 48 + 4 * 8, 8);
    for (stat, value) in stats {
        fixture::word(&mut bytes, 48 + 4 * stat, *value);
    }
    fixture::word(&mut bytes, 0x24, packet);
    fixture::word(&mut bytes, 0x28, team);
    let critter = world
        .spawn(Arc::new(Prototype::parse(&bytes).unwrap()), hex, 0)
        .unwrap();
    world.get_mut(critter).unwrap().raw.flags = BasicFlags(0);
    critter
}
fn set_skill(world: &mut WorldState, critter: ObjectRef, skill: usize, points: i32) {
    let sheet = world.get_mut(critter).unwrap().character.as_mut().unwrap();
    sheet.invested[skill] = points;
}
fn state(world: &mut WorldState, critter: ObjectRef) -> &mut vault_formats::map::CritterState {
    world.critter_state_mut(critter).unwrap()
}
fn set_hp(world: &mut WorldState, critter: ObjectRef, hp: i32) {
    state(world, critter).hp = hp;
    world
        .get_mut(critter)
        .unwrap()
        .character
        .as_mut()
        .unwrap()
        .current_hp = hp;
}

// Item prototype offsets (SPEC-003): extended flags, cost, and the weapon,
// ammunition, and drug payload fields.
const EXTENDED: usize = 0x18;
const COST: usize = 0x30;
const ANIMATION: usize = 57;
const MIN_DAMAGE: usize = 61;
const MAX_DAMAGE: usize = 65;
const RANGE: usize = 73;
const AP: usize = 89;
const PERK: usize = 101;
const CALIBER: usize = 109;
const CAPACITY: usize = 117;
const AMMO_CALIBER: usize = 57;
const AMMO_PACK: usize = 61;

fn item_bytes(kind: usize, pid: u32, fields: &[(usize, i32)]) -> Vec<u8> {
    let mut bytes = fixture::prototype(0, kind);
    fixture::word(&mut bytes, 0, pid as i32);
    for (offset, value) in fields {
        fixture::word(&mut bytes, *offset, *value);
    }
    bytes
}
fn item(kind: usize, pid: u32, fields: &[(usize, i32)]) -> Arc<Prototype> {
    Arc::new(Prototype::parse(&item_bytes(kind, pid, fields)).unwrap())
}
/// SPEC-010 weapon sound byte (file offset 0x79).
const WEAPON_SOUND: usize = 0x79;
/// A weapon: extended flags, AP for both modes, damage 1..`max`, both
/// ranges, caliber 1, capacity, cost, and sound byte `A`.
fn weapon(
    pid: u32,
    extended: i32,
    ap: i32,
    max: i32,
    range: i32,
    capacity: i32,
    cost: i32,
) -> Arc<Prototype> {
    let mut bytes = item_bytes(
        3,
        pid,
        &[
            (EXTENDED, extended),
            (COST, cost),
            (ANIMATION, 5),
            (MIN_DAMAGE, 1),
            (MAX_DAMAGE, max),
            (RANGE, range),
            (RANGE + 4, range),
            (AP, ap),
            (AP + 4, ap),
            (PERK, -1),
            (CALIBER, 1),
            (CAPACITY, capacity),
        ],
    );
    bytes[WEAPON_SOUND] = b'A';
    Arc::new(Prototype::parse(&bytes).unwrap())
}
fn ammo(pid: u32, cost: i32, pack: i32) -> Arc<Prototype> {
    item(
        4,
        pid,
        &[(COST, cost), (AMMO_CALIBER, 1), (AMMO_PACK, pack)],
    )
}
/// Carry `quantity` of a prototype with an instance detail.
fn carry(
    world: &mut WorldState,
    owner: ObjectRef,
    prototype: Arc<Prototype>,
    quantity: i32,
    detail: ObjectDetail,
) -> ObjectRef {
    let item = world.spawn(prototype, 0, 0).unwrap();
    world.get_mut(item).unwrap().raw.state = ObjectState::Other { word: 0, detail };
    world.add_item(owner, item, quantity, true).unwrap();
    item
}
fn loaded(rounds: i32, ammo_pid: i32) -> ObjectDetail {
    ObjectDetail::Weapon { rounds, ammo_pid }
}
fn rounds(world: &WorldState, weapon: ObjectRef) -> i32 {
    match &world.get(weapon).unwrap().raw.state {
        ObjectState::Other {
            detail: ObjectDetail::Weapon { rounds, .. },
            ..
        } => *rounds,
        _ => 0,
    }
}
fn hex(world: &WorldState, object: ObjectRef) -> i32 {
    world.get(object).unwrap().raw.hex.unwrap()
}
fn taunt_effects(host: &WorldHost<AiAssets>) -> Vec<(ObjectRef, Vec<u8>)> {
    host.world
        .effects
        .iter()
        .filter_map(|effect| match effect {
            WorldEffect::Taunt { object, text, .. } => Some((*object, text.clone())),
            _ => None,
        })
        .collect()
}
fn messages(host: &WorldHost<AiAssets>) -> Vec<Vec<u8>> {
    host.world
        .effects
        .iter()
        .filter_map(|effect| match effect {
            WorldEffect::Message(text) => Some(text.clone()),
            _ => None,
        })
        .collect()
}
fn line(id: i32) -> Vec<u8> {
    format!("taunt {id}").into_bytes()
}

#[test]
fn spec009_initialization_resolution_and_language_filter() {
    let mut world = world();
    let critter = npc(&mut world, 20300, 1, 99, &[]);
    let mut host = WorldHost::new(world, AiAssets::default());
    host.world.ai.language_filter = Some(2);
    host.load_combat_ai().unwrap();
    let packets = host.world.ai.packets.clone().unwrap();
    let names: Vec<_> = packets.packets.iter().map(|p| p.name.clone()).collect();
    assert_eq!(
        names,
        [
            b"Brute".to_vec(),
            b"Coward".into(),
            b"Guard".into(),
            b"Rare".into(),
            b"Zeta".into()
        ]
    );
    // The parser's token diagnostic reaches the world.
    assert!(host.world.effects.iter().any(|effect| matches!(
        effect,
        WorldEffect::Diagnostic(text) if text.contains("\"blind \"")
    )));
    // A nonzero preference at load filters; the remembered state is still
    // -1, so the first evaluation acts: 2 is not 1, so it reloads unfiltered.
    assert!(host.world.ai.filtered);
    host.reevaluate_language_filter().unwrap();
    assert!(!host.world.ai.filtered);
    assert_eq!(host.world.ai.filter_state, 2);
    host.world.ai.language_filter = Some(1);
    host.reevaluate_language_filter().unwrap();
    assert!(host.world.ai.filtered);
    host.reevaluate_language_filter().unwrap();
    assert!(host.world.ai.filtered, "an unchanged value does nothing");
    host.world.ai.language_filter = Some(0);
    host.reevaluate_language_filter().unwrap();
    assert!(!host.world.ai.filtered);

    // Resolution: an unknown number falls back to entry 0 (diagnosed), and
    // a script change takes effect at the next query (never cached).
    host.world.effects.clear();
    assert_eq!(host.packet(critter).unwrap().name, b"Brute");
    assert!(matches!(
        host.world.effects.front(),
        Some(WorldEffect::Diagnostic(text)) if text.contains("99")
    ));
    state(&mut host.world, critter).ai = 2;
    assert_eq!(host.packet(critter).unwrap().name, b"Zeta");
    let rock = host.world.spawn(item(5, 9, &[]), 1, 0).unwrap();
    assert!(host.packet(rock).is_none());

    // Missing resources fail initialization; combatai.msg is read first.
    for assets in [
        AiAssets {
            ai: None,
            ..AiAssets::default()
        },
        AiAssets {
            taunts: None,
            ..AiAssets::default()
        },
        AiAssets {
            ai: Some(b"[Empty]\r\n".to_vec()),
            ..AiAssets::default()
        },
    ] {
        let mut host = WorldHost::new(self::world(), assets);
        assert!(host.load_combat_ai().is_err());
        assert!(host.world.ai.packets.is_none());
    }
}

#[test]
fn spec009_fixture_c_danger_through_live_state() {
    // Fixture C on a straight southeast line from A (direction 2, so
    // `dist` is the step count); A faces northwest: rear limit 2P = 10.
    let mut world = world();
    let player = world.player;
    world.get_mut(player).unwrap().raw.hex = Some(39000);
    let at = |steps: i32| 20100 + 200 * steps;
    let a = npc(&mut world, at(0), 1, 0, &[]);
    let b = npc(&mut world, at(5), 2, 0, &[]);
    let c = npc(&mut world, at(2), 1, 0, &[]);
    let d = npc(&mut world, at(12), 2, 0, &[]);
    let e = npc(&mut world, at(3), 2, 0, &[]);
    let f = npc(&mut world, at(9), 1, 0, &[]);
    let x = npc(&mut world, at(4), 2, 0, &[]);
    world.get_mut(a).unwrap().raw.orientation = 5;
    for (critter, attacker) in [(a, x), (b, a), (c, d), (e, f)] {
        world.get_mut(critter).unwrap().opponent = attacker;
    }
    state(&mut world, x).results.0 = effect::DEAD;
    set_hp(&mut world, x, 0);
    world.combat = true;
    world.ai.snapshot = vec![a, b, c, d, e, f, x];
    let mut host = ai_host(world, AiAssets::default());
    assert_eq!(host.danger(a), Some(e));
    // The distance sort persists in the snapshot (stable for ties).
    assert_eq!(host.world.ai.snapshot, [a, c, e, x, b, f, d]);

    // X alive returns at once, with no perception or team test.
    state(&mut host.world, x).results.0 = 0;
    set_hp(&mut host.world, x, 30);
    host.world.critter_state_mut(x).unwrap().team = 1;
    assert_eq!(host.danger(a), Some(x));
    state(&mut host.world, x).results.0 = effect::DEAD;
    set_hp(&mut host.world, x, 0);
    host.world.critter_state_mut(x).unwrap().team = 2;
    // Perception 1 (limit 2): no candidate is perceived.
    host.world
        .get_mut(a)
        .unwrap()
        .character
        .as_mut()
        .unwrap()
        .base[1] = 1;
    assert_eq!(host.danger(a), None);
    host.world
        .get_mut(a)
        .unwrap()
        .character
        .as_mut()
        .unwrap()
        .base[1] = 5;
    // Removing E swap-removes it from the snapshot; the result becomes B.
    host.world.erase(e);
    assert!(!host.world.ai.snapshot.contains(&e));
    assert_eq!(host.danger(a), Some(b));
}

#[test]
fn spec009_fixture_h_setter_retaliation_and_onlookers() {
    let mut world = world();
    let player = world.player;
    let v = npc(&mut world, 20300, 1, 0, &[]);
    // Q: Melee Damage 3, right-hand maximum 12, AC 10 -> 22.
    let q = npc(&mut world, 20500, 2, 0, &[(11, 3), (9, 10)]);
    let club = carry(
        &mut world,
        q,
        weapon(20, 0x03, 3, 12, 1, 0, 10),
        1,
        loaded(0, -1),
    );
    world.equip(q, club, BasicFlags::RIGHT_HAND.0).unwrap();
    // R1: 5, none, AC 20 -> 25. R2: 2, left 20, AC 0 -> 20. R3 knocked out.
    let r1 = npc(&mut world, 20700, 2, 0, &[(11, 5), (9, 20)]);
    let r2 = npc(&mut world, 20900, 2, 0, &[(11, 2), (9, 0)]);
    let sword = carry(
        &mut world,
        r2,
        weapon(21, 0x03, 3, 20, 1, 0, 10),
        1,
        loaded(0, -1),
    );
    world.equip(r2, sword, BasicFlags::LEFT_HAND.0).unwrap();
    let r3 = npc(&mut world, 21100, 2, 0, &[(11, 9), (9, 40)]);
    state(&mut world, r3).results.0 = effect::KNOCKED_OUT;
    let rock = world.spawn(item(5, 22, &[]), 21300, 0).unwrap();
    let mut host = ai_host(world, AiAssets::default());
    let ratings: Vec<_> = [q, r1, r2, r3, 0, rock]
        .iter()
        .map(|critter| host.world.retaliation_rating(*critter))
        .collect();
    // R2's rule value is max(2, 20) + 0 = 20; the fixture table lists 22,
    // a tie. Both keep Q.
    assert_eq!(ratings, [22, 25, 20, 0, 0, 0]);
    host.set_last_attacker(v, q);
    for (candidate, expected) in [(r2, q), (r3, q), (r1, r1)] {
        host.retaliate(v, candidate);
        assert_eq!(host.world.get(v).unwrap().opponent, expected);
    }

    // The setter refuses non-critter art; the player writes reaction 1
    // into script local 0 (nothing is written without a script).
    host.set_last_attacker(v, rock);
    assert_eq!(host.world.get(v).unwrap().opponent, r1);
    let script = host.world.add_script(v, 0, 1, 2).unwrap();
    host.set_last_attacker(v, player);
    assert_eq!(host.world.get(v).unwrap().opponent, player);
    assert_eq!(host.world.local(Some(script), 0), 1);
    host.set_last_attacker(q, player);
    assert_eq!(host.world.get(q).unwrap().opponent, player);

    // Damage writers (SPEC-007 §3.6): a same-team victim makes the rating
    // check only as the attack's intended target, so a same-team unintended
    // victim or extra keeps its reference; another team always makes it; a
    // knocked-out victim references the attacker.
    let ally = npc(&mut host.world, 21500, 1, 0, &[(11, 50)]);
    host.set_last_attacker(v, 0);
    host.after_damage(ally, v, false);
    assert_eq!(host.world.get(v).unwrap().opponent, 0);
    host.after_damage(ally, v, true);
    assert_eq!(host.world.get(v).unwrap().opponent, ally);
    host.after_damage(r1, v, false);
    assert_eq!(host.world.get(v).unwrap().opponent, ally, "25 < 50 + 5");
    state(&mut host.world, v).results.0 = effect::KNOCKED_OUT;
    host.after_damage(q, v, false);
    assert_eq!(host.world.get(v).unwrap().opponent, q);

    // Onlookers: every snapshot entry perceiving the damaged critter
    // (itself and dead entries included) becomes alert.
    let far = npc(&mut host.world, 39000, 3, 0, &[]);
    state(&mut host.world, r2).results.0 = effect::DEAD;
    host.world.combat = true;
    host.world.ai.snapshot = vec![v, r2, far];
    host.after_damage(q, v, true);
    let alert = |host: &WorldHost<AiAssets>, critter| {
        host.world.critter_state(critter).unwrap().maneuvers & intent::ALERT != 0
    };
    assert!(alert(&host, v) && alert(&host, r2) && !alert(&host, far));
}

#[test]
fn spec007_intended_target_rule_covers_unintended_victims() {
    // SPEC-007 §3.6: the hit lands on V instead of the intended target D.
    // Onlookers hear of D, though D took nothing, then of V; a surviving
    // V on the attacker's team keeps its reference. As the intended target
    // the same V would make the rating check.
    let mut world = world();
    let attacker = npc(&mut world, 20100, 1, 0, &[(11, 5)]);
    let intended = npc(&mut world, 20104, 2, 0, &[]);
    let victim = npc(&mut world, 20102, 1, 0, &[]);
    let attack = world
        .attack_profile(attacker, victim, hit_mode::PUNCH, location::UNCALLED, false)
        .unwrap();
    let mut host = ai_host(world, AiAssets::default());
    host.world.combat = true;
    host.world.ai.snapshot = vec![attacker, intended, victim];
    let resolution = Resolution {
        code: 2,
        damage: 1,
        defender_flags: effect::HIT,
        ..Default::default()
    };
    let hp = |host: &WorldHost<AiAssets>, critter| host.world.hit_points(critter).unwrap();
    let before = hp(&host, intended);
    host.apply_attack_to(attacker, intended, victim, &attack, &resolution)
        .unwrap();
    assert_eq!(
        hp(&host, intended),
        before,
        "the intended target takes nothing"
    );
    assert_eq!(host.world.get(victim).unwrap().opponent, 0);
    let alert = |host: &WorldHost<AiAssets>, critter| {
        host.world.critter_state(critter).unwrap().maneuvers & intent::ALERT != 0
    };
    assert!(alert(&host, intended) && alert(&host, victim));
    host.apply_attack_to(attacker, victim, victim, &attack, &resolution)
        .unwrap();
    assert_eq!(host.world.get(victim).unwrap().opponent, attacker);
    // A victim already marked dead takes no status, damage or writers.
    state(&mut host.world, intended).results.0 = effect::DEAD;
    let hp_dead = hp(&host, intended);
    host.apply_attack_to(attacker, intended, intended, &attack, &resolution)
        .unwrap();
    assert_eq!(hp(&host, intended), hp_dead);
    assert_eq!(host.world.get(intended).unwrap().opponent, 0);
}

#[test]
fn spec014_combat_cleanup_wakes_a_knocked_out_player() {
    // A knockout during combat is recovered: cleanup wakes the living
    // player and removes the pending wake-up event (SPEC-014 §3.5).
    let world = world();
    let player = world.player;
    let mut host = ai_host(world, AiAssets::default());
    host.damage_critter(player, None, 0, effect::KNOCKED_OUT)
        .unwrap();
    let wake = |host: &WorldHost<AiAssets>| {
        host.world
            .queue
            .events()
            .any(|event| event.object == player && event.kind == crate::queue::EventKind::Knockout)
    };
    assert!(wake(&host));
    host.world.combat = true;
    host.end_combat().unwrap();
    assert_eq!(
        host.world.critter_state(player).unwrap().results.0 & effect::KNOCKED_OUT,
        0
    );
    assert!(!wake(&host));
}

#[test]
fn spec009_fixture_d_weapon_usability_preference_and_cost() {
    let mut world = world();
    let critter = npc(&mut world, 20300, 1, 0, &[]);
    state(&mut world, critter).ap = 5;
    let mut assets = AiAssets::default();
    assets.prototypes.insert(30, ammo(30, 0, 10));
    assets.prototypes.insert(31, ammo(31, 50, 20));
    assets.prototypes.insert(32, ammo(32, 50, 0));
    let two_handed = vault_formats::pro::ExtendedFlags::TWO_HANDED.0 as i32;
    let entries = [
        (weapon(40, 0x03, 3, 4, 1, 0, 40), loaded(0, -1)),
        (weapon(41, 0x06, 4, 8, 20, 10, 250), loaded(0, -1)),
        (weapon(42, 0x05, 3, 3, 10, 0, 0), loaded(0, -1)),
        (
            weapon(43, 0x06 | two_handed, 7, 9, 30, 10, 1000),
            loaded(5, 30),
        ),
        (weapon(44, 0x06, 5, 9, 20, 10, 1000), loaded(10, 30)),
        (weapon(45, 0x06, 5, 9, 20, 10, 1000), loaded(10, 30)),
    ];
    let weapons: Vec<_> = entries
        .into_iter()
        .map(|(prototype, detail)| carry(&mut world, critter, prototype, 1, detail))
        .collect();
    let mut host = ai_host(world, assets);
    assert_eq!(
        host.inventory_search(critter, true),
        Some(weapons[4]),
        "SMG-A"
    );
    assert_eq!(
        host.inventory_search(critter, false),
        Some(weapons[3]),
        "rifle"
    );
    // The right-hand entry is skipped; the next fold keeps SMG-B.
    host.world
        .equip(critter, weapons[4], BasicFlags::RIGHT_HAND.0)
        .unwrap();
    assert_eq!(host.inventory_search(critter, true), Some(weapons[5]));
    // Quadrupeds never switch weapons.
    host.world
        .get_mut(critter)
        .unwrap()
        .character
        .as_mut()
        .unwrap()
        .body = 1;
    assert_eq!(host.inventory_search(critter, false), None);
    host.world
        .get_mut(critter)
        .unwrap()
        .character
        .as_mut()
        .unwrap()
        .body = 0;

    // Cost 200 + floor(10 x 50 / 20) = 225; a zero pack adds 0 (diagnosed).
    let gun = carry(
        &mut host.world,
        critter,
        weapon(46, 0x06, 4, 8, 20, 10, 200),
        1,
        loaded(10, 31),
    );
    assert_eq!(host.item_cost(gun), 225);
    let zero = carry(
        &mut host.world,
        critter,
        weapon(47, 0x06, 4, 8, 20, 10, 200),
        1,
        loaded(10, 32),
    );
    host.world.effects.clear();
    assert_eq!(host.item_cost(zero), 200);
    assert!(matches!(
        host.world.effects.front(),
        Some(WorldEffect::Diagnostic(_))
    ));

    // Usability: a crippled left arm rules out the two-handed rifle only;
    // Small Guns 39 below min_to_hit 40 rules out every small gun.
    state(&mut host.world, critter).results.0 = effect::CRIPPLED_LEFT_ARM;
    assert!(!host.usable(critter, weapons[3], hit_mode::RIGHT_PRIMARY, 40));
    assert!(host.usable(critter, weapons[1], hit_mode::RIGHT_PRIMARY, 40));
    state(&mut host.world, critter).results.0 = effect::CRIPPLED_ARMS;
    assert!(!host.usable(critter, weapons[1], hit_mode::RIGHT_PRIMARY, 40));
    state(&mut host.world, critter).results.0 = 0;
    host.world
        .get_mut(critter)
        .unwrap()
        .character
        .as_mut()
        .unwrap()
        .base[5] = 4;
    assert!(!host.usable(critter, weapons[1], hit_mode::RIGHT_PRIMARY, 40));
    assert!(host.usable(critter, weapons[1], hit_mode::RIGHT_PRIMARY, 39));
    // Missing attack art (single fire, 45) makes it unusable too.
    host.assets.missing.insert(45);
    assert!(!host.usable(critter, weapons[1], hit_mode::RIGHT_PRIMARY, 0));
}

#[test]
fn spec009_fixture_e_hit_mode_and_called_shot_draws_through_the_world() {
    let mut world = world();
    let player = world.player;
    // Intelligence 6; a spear (thrust primary, throw secondary), no other
    // weapon; the player stands adjacent.
    let critter = npc(&mut world, 20300, 1, 0, &[(4, 6)]);
    set_skill(&mut world, critter, 4, 100);
    let spear = carry(
        &mut world,
        critter,
        weapon(50, 0x54, 3, 6, 1, 0, 80),
        1,
        loaded(0, -1),
    );
    world
        .equip(critter, spear, BasicFlags::RIGHT_HAND.0)
        .unwrap();
    state(&mut world, critter).ap = 8;
    world.get_mut(player).unwrap().raw.hex = Some(20500);
    let mut host = ai_host(world, AiAssets::default());
    let mut outcomes = BTreeSet::new();
    for seed in 1..400 {
        host.world.rng = crate::ParkMiller::seeded(seed).unwrap();
        let mut expected = host.world.rng.clone();
        let mode = if expected.range(1, 4).unwrap() != 1 || expected.range(1, 10).unwrap() <= 6 {
            hit_mode::RIGHT_PRIMARY
        } else {
            hit_mode::RIGHT_SECONDARY
        };
        assert_eq!(host.select_mode(critter, Some(spear)).unwrap(), mode);
        assert_eq!(host.world.rng, expected, "seed {seed}: draw count");
        outcomes.insert(mode);
        // Called shot: gate draw 1..3, then location 0..8 against the
        // range-aware hit chance (no draws for its comparison).
        let mut expected = host.world.rng.clone();
        let place = if expected.range(1, 3).unwrap() != 1 {
            location::TORSO
        } else {
            let place = expected.range(0, 8).unwrap() as u8;
            let chance = host
                .world
                .attack_profile(critter, player, hit_mode::RIGHT_PRIMARY, place, false)
                .unwrap()
                .hit_chance(true)
                .unwrap();
            if chance < 40 { location::TORSO } else { place }
        };
        let chosen = host
            .select_location(critter, player, hit_mode::RIGHT_PRIMARY)
            .unwrap();
        assert_eq!(chosen, place, "seed {seed}");
        assert_eq!(host.world.rng, expected);
        outcomes.insert(10 + chosen);
    }
    assert!(
        outcomes.contains(&hit_mode::RIGHT_SECONDARY)
            && outcomes.contains(&hit_mode::RIGHT_PRIMARY)
    );
    assert!(outcomes.contains(&(10 + location::TORSO)) && outcomes.len() > 5);
    // A second weapon skips the Intelligence draw: one draw only.
    carry(
        &mut host.world,
        critter,
        weapon(51, 0x03, 3, 6, 1, 0, 10),
        1,
        loaded(0, -1),
    );
    for seed in 1..50 {
        host.world.rng = crate::ParkMiller::seeded(seed).unwrap();
        let mut expected = host.world.rng.clone();
        let mode = if expected.range(1, 4).unwrap() == 1 {
            hit_mode::RIGHT_SECONDARY
        } else {
            hit_mode::RIGHT_PRIMARY
        };
        assert_eq!(host.select_mode(critter, Some(spear)).unwrap(), mode);
        assert_eq!(host.world.rng, expected);
    }
    // Intelligence below the normal-difficulty minimum 5: the gate draw
    // only; unaffordable aimed AP: no draws at all.
    host.world
        .get_mut(critter)
        .unwrap()
        .character
        .as_mut()
        .unwrap()
        .base[4] = 4;
    let mut expected = host.world.rng.clone();
    let gate = expected.range(1, 3).unwrap();
    assert_eq!(
        host.select_location(critter, player, hit_mode::RIGHT_PRIMARY)
            .unwrap(),
        location::TORSO
    );
    assert_eq!(host.world.rng, expected, "gate {gate}");
    state(&mut host.world, critter).ap = 3;
    let expected = host.world.rng.clone();
    assert_eq!(
        host.select_location(critter, player, hit_mode::RIGHT_PRIMARY)
            .unwrap(),
        location::TORSO
    );
    assert_eq!(host.world.rng, expected);
    // No weapon: punch, without a draw.
    assert_eq!(host.select_mode(critter, None).unwrap(), hit_mode::PUNCH);
    assert_eq!(host.world.rng, expected);
}

#[test]
fn spec009_fixture_f_flee_movement_and_run_taunt() {
    // Player at 20100; the coward 7 hexes southeast (D = 2) with AP 4.
    let mut world = world();
    let critter = npc(&mut world, 20100 + 7 * 200, 1, 1, &[]);
    state(&mut world, critter).ap = 4;
    world.combat = true;
    let mut host = ai_host(world, AiAssets::default());
    let mut expected = host.world.rng.clone();
    let _chance = expected.range(1, 100).unwrap();
    let id = expected.range(1000, 1010).unwrap();
    host.ai_turn(critter, None).unwrap();
    let maneuvers = host.world.critter_state(critter).unwrap().maneuvers;
    assert_eq!(maneuvers, intent::FLEE, "forced flee: no healing or attack");
    assert_eq!(
        hex(&host.world, critter),
        HexGrid::step(21500, 2, 4).unwrap()
    );
    assert_eq!(host.world.critter_state(critter).unwrap().ap, 0);
    assert_eq!(taunt_effects(&host), [(critter, line(id))]);
    assert_eq!(host.world.rng, expected);

    // A blocked first candidate takes (4, D + 1).
    let mut world = self::world();
    let critter = npc(&mut world, 21500, 1, 1, &[]);
    state(&mut world, critter).ap = 4;
    let blocker = npc(&mut world, HexGrid::step(21500, 2, 4).unwrap(), 1, 0, &[]);
    let mut host = ai_host(world, AiAssets::default());
    host.ai_flee(critter).unwrap();
    assert_eq!(
        hex(&host.world, critter),
        HexGrid::step(21500, 3, 4).unwrap()
    );
    assert_ne!(hex(&host.world, blocker), hex(&host.world, critter));

    // At max_dist: stop only. With AP 0: fleeing, but no movement or taunt.
    let mut world = self::world();
    let far = npc(&mut world, 20100 + 8 * 200, 1, 1, &[]);
    let tired = npc(&mut world, 20100 + 6 * 200, 1, 1, &[]);
    state(&mut world, far).ap = 4;
    let mut host = ai_host(world, AiAssets::default());
    let expected = host.world.rng.clone();
    host.ai_flee(far).unwrap();
    host.ai_flee(tired).unwrap();
    assert_eq!(
        host.world.critter_state(far).unwrap().maneuvers,
        intent::STOP
    );
    assert_eq!(
        host.world.critter_state(tired).unwrap().maneuvers,
        intent::FLEE
    );
    assert_eq!(hex(&host.world, far), 21700);
    assert_eq!(hex(&host.world, tired), 21300);
    assert!(taunt_effects(&host).is_empty());
    assert_eq!(host.world.rng, expected, "no taunt draws");

    // hurt_too_much: Brute's mask 0x3C triggers on either crippled leg.
    let mut world = self::world();
    let brute = npc(&mut world, 21300, 1, 3, &[]);
    state(&mut world, brute).results.0 = effect::CRIPPLED_RIGHT_LEG;
    let mut host = ai_host(world, AiAssets::default());
    host.ai_turn(brute, None).unwrap();
    assert_eq!(
        host.world.critter_state(brute).unwrap().maneuvers,
        intent::FLEE
    );
}

#[test]
fn spec009_fixture_g_taunt_requests_buffers_and_display() {
    let mut world = world();
    let player = world.player;
    let zeta = npc(&mut world, 20300, 1, 2, &[]);
    let guard = npc(&mut world, 20500, 1, 0, &[]);
    let brute = npc(&mut world, 20700, 1, 3, &[]);
    let mut host = ai_host(world, AiAssets::default());
    // The chance draw must be at most 35, then the line draw is inclusive.
    let mut seen = BTreeSet::new();
    for seed in 1..200 {
        host.world.rng = crate::ParkMiller::seeded(seed).unwrap();
        host.world.ai.callbacks.clear();
        let mut expected = host.world.rng.clone();
        let passed = expected.range(1, 100).unwrap() <= 35;
        let id = passed.then(|| expected.range(1080, 1090).unwrap());
        assert_eq!(host.request_taunt(zeta, Taunt::Hit(4)).unwrap(), passed);
        assert_eq!(host.world.rng, expected);
        if let Some(id) = id {
            assert_eq!(host.world.ai.taunts[1], line(id));
            seen.insert(id);
        }
    }
    assert!(
        seen.contains(&1080) && seen.contains(&1090),
        "both endpoints"
    );
    // No draws: the player, a dead or knocked-out speaker, taunts disabled,
    // or a non-critter.
    let expected = host.world.rng.clone();
    host.world.ai.callbacks.clear();
    assert!(!host.request_taunt(player, Taunt::Attack).unwrap());
    state(&mut host.world, guard).results.0 = effect::KNOCKED_OUT;
    assert!(!host.request_taunt(guard, Taunt::Attack).unwrap());
    state(&mut host.world, guard).results.0 = 0;
    host.world.ai.combat_taunts = Some(0);
    assert!(!host.request_taunt(guard, Taunt::Attack).unwrap());
    host.world.ai.combat_taunts = None;
    assert_eq!(host.world.rng, expected);
    // An empty range (2004..2003) consumes only the chance draw.
    let mut expected = host.world.rng.clone();
    expected.range(1, 100).unwrap();
    assert!(!host.request_taunt(brute, Taunt::Hit(7)).unwrap());
    assert_eq!(host.world.rng, expected);
    // Head: 2004..2004.
    assert!(host.request_taunt(brute, Taunt::Hit(0)).unwrap());
    assert_eq!(host.world.ai.taunts[1], line(2004));

    // Display: buffer text at callback time; nothing while floating text
    // exists; a later same-class request replaces the earlier text.
    host.world.ai.callbacks.clear();
    host.world.effects.clear();
    host.request_taunt(guard, Taunt::Run).unwrap();
    host.request_taunt(guard, Taunt::Attack).unwrap();
    let attack_text = host.world.ai.taunts[0].clone();
    host.run_taunts();
    assert_eq!(taunt_effects(&host), [(guard, attack_text)]);
    assert!(matches!(
        host.world.effects.back(),
        Some(WorldEffect::Taunt {
            font: 101,
            color: 145,
            outline: -1,
            ..
        })
    ));
    let expiry = host.world.floating[0].1;
    assert_eq!(expiry, crate::ai::floating_lifetime(1), "no font: one line");
    host.request_taunt(guard, Taunt::Miss).unwrap();
    host.run_taunts();
    assert_eq!(taunt_effects(&host).len(), 1, "a floating text exists");
    host.world.elapsed_ms = expiry;
    host.request_taunt(guard, Taunt::Hit(LOCATION_TORSO))
        .unwrap();
    host.run_taunts();
    assert_eq!(taunt_effects(&host).len(), 2);
    // Buffers keep 79 bytes.
    host.world.ai.callbacks.clear();
    let mut seed = 1;
    loop {
        host.world.rng = crate::ParkMiller::seeded(seed).unwrap();
        let mut probe = host.world.rng.clone();
        probe.range(1, 100).unwrap();
        if probe.range(1070, 1080).unwrap() == LONG_TAUNT {
            break;
        }
        seed += 1;
    }
    assert!(
        host.request_taunt(guard, Taunt::Hit(LOCATION_TORSO))
            .unwrap()
    );
    assert_eq!(host.world.ai.taunts[1], vec![b'x'; 79]);

    // Request sites: a ranged hit at location 4 makes the defender draw
    // 1080-1090; a melee miss makes the attacker draw its miss range.
    host.world.ai.callbacks.clear();
    let mut attack = host
        .world
        .attack_profile(guard, zeta, hit_mode::PUNCH, location::RIGHT_LEG, false)
        .unwrap();
    attack.weapon = host
        .world
        .attack_profile(player, zeta, hit_mode::PUNCH, location::RIGHT_LEG, false)
        .unwrap()
        .weapon;
    let gun = carry(
        &mut host.world,
        guard,
        weapon(60, 0x06, 4, 6, 20, 10, 10),
        1,
        loaded(5, 30),
    );
    host.world
        .equip(guard, gun, BasicFlags::RIGHT_HAND.0)
        .unwrap();
    let ranged = host
        .world
        .attack_profile(
            guard,
            zeta,
            hit_mode::RIGHT_PRIMARY,
            location::RIGHT_LEG,
            true,
        )
        .unwrap();
    let hit = Resolution {
        defender_flags: effect::HIT,
        ..Resolution::default()
    };
    for seed in 1..100 {
        host.world.rng = crate::ParkMiller::seeded(seed).unwrap();
        host.world.ai.callbacks.clear();
        let mut expected = host.world.rng.clone();
        expected.range(1, 100).unwrap();
        let attack_id = expected.range(1020, 1030).unwrap();
        let passed = expected.range(1, 100).unwrap() <= 35;
        let hit_id = passed.then(|| expected.range(1080, 1090).unwrap());
        host.request_taunt(guard, crate::ai::Taunt::Attack).unwrap();
        host.request_result_taunt(guard, zeta, &ranged, &hit)
            .unwrap();
        assert_eq!(host.world.rng, expected);
        assert_eq!(host.world.ai.taunts[0], line(attack_id));
        let speakers: Vec<_> = host.world.ai.callbacks.iter().map(|c| c.speaker).collect();
        match hit_id {
            Some(id) => {
                assert_eq!(speakers, [guard, zeta]);
                assert_eq!(host.world.ai.taunts[1], line(id));
            }
            None => assert_eq!(speakers, [guard]),
        }
    }
    let miss = Resolution::default();
    host.world.ai.callbacks.clear();
    host.request_taunt(guard, crate::ai::Taunt::Attack).unwrap();
    host.request_result_taunt(guard, zeta, &attack, &miss)
        .unwrap();
    let speakers: Vec<_> = host.world.ai.callbacks.iter().map(|c| c.speaker).collect();
    assert_eq!(speakers, [guard, guard], "melee: the attacker speaks");
    // A hit that kills requests no hit taunt.
    let killed = Resolution {
        defender_flags: effect::HIT | effect::DEAD,
        ..Resolution::default()
    };
    host.world.ai.callbacks.clear();
    host.request_taunt(guard, crate::ai::Taunt::Attack).unwrap();
    host.request_result_taunt(guard, zeta, &attack, &killed)
        .unwrap();
    assert_eq!(host.world.ai.callbacks.len(), 1);
}
const LOCATION_TORSO: u8 = location::TORSO;

fn stimpak_count(world: &WorldState, critter: ObjectRef) -> i32 {
    world
        .get(critter)
        .unwrap()
        .inventory
        .iter()
        .filter(|(item, _)| world.get(*item).unwrap().raw.pid == crate::ai::STIMPAK)
        .map(|(_, count)| *count)
        .sum()
}
/// SPEC-011 drug payload (file 0x39): selectors, then immediate amounts;
/// both delayed stages and the addiction chance stay zero.
fn drug(pid: u32, selectors: [i32; 3], immediate: [i32; 3]) -> Arc<Prototype> {
    let fields: Vec<_> = (0..3)
        .flat_map(|slot| {
            [
                (0x39 + 4 * slot, selectors[slot]),
                (0x45 + 4 * slot, immediate[slot]),
            ]
        })
        .collect();
    item(2, pid, &fields)
}

#[test]
fn spec009_fixture_i_healing_loop_through_inventory() {
    let mut world = world();
    // Effective max HP 41, so B = 20; HP 15, AP 5.
    let critter = npc(&mut world, 20300, 1, 0, &[(7, 41)]);
    set_hp(&mut world, critter, 15);
    state(&mut world, critter).ap = 5;
    carry(
        &mut world,
        critter,
        item(2, 0x35, &[]),
        1,
        ObjectDetail::Plain,
    );
    // A stimpak heals 8 through the SPEC-011 §3.7 consumption contract.
    carry(
        &mut world,
        critter,
        drug(crate::ai::STIMPAK, [35, -1, -1], [8, 0, 0]),
        2,
        ObjectDetail::Plain,
    );
    let mut host = ai_host(world, AiAssets::default());
    let mut expected = host.world.rng.clone();
    expected.range(1, 100).unwrap();
    host.ai_heal(critter).unwrap();
    let state = host.world.critter_state(critter).unwrap();
    assert_eq!((state.hp, state.ap), (23, 3));
    assert_eq!(stimpak_count(&host.world, critter), 1);
    assert_eq!(
        host.world.rng.state(),
        expected.state(),
        "only the 0% addiction draw"
    );
    let name = String::from_utf8(host.world.get(critter).unwrap().name.clone()).unwrap();
    assert_eq!(
        messages(&host),
        [format!("{name} uses Object 00000028.").into_bytes()]
    );
    assert!(
        host.world
            .effects
            .iter()
            .any(|effect| matches!(effect, WorldEffect::CritterAnimation { animation: 11, .. }))
    );
}

/// An NPC with a loaded pistol (primary 4 AP, damage 1, range 20) facing
/// the player, who is its living last attacker.
fn duel(distance: i32, skill: i32) -> (WorldHost<AiAssets>, ObjectRef, ObjectRef) {
    let mut world = world();
    let player = world.player;
    set_hp(&mut world, player, 1000);
    let critter = npc(&mut world, 20100 + distance * 200, 1, 0, &[(4, 1)]);
    set_skill(&mut world, critter, 0, skill);
    let pistol = carry(
        &mut world,
        critter,
        weapon(70, 0x06, 4, 1, 20, 10, 100),
        1,
        loaded(10, 30),
    );
    world
        .equip(critter, pistol, BasicFlags::RIGHT_HAND.0)
        .unwrap();
    world.get_mut(critter).unwrap().opponent = player;
    state(&mut world, critter).ap = 8;
    world.combat = true;
    world.ai.snapshot = vec![player, critter];
    (ai_host(world, AiAssets::default()), critter, pistol)
}

#[test]
fn spec009_fixture_j_attack_loop_through_the_world() {
    let (mut host, critter, pistol) = duel(3, 100);
    let player = host.world.player;
    let chance = host
        .world
        .attack_profile(
            critter,
            player,
            hit_mode::RIGHT_PRIMARY,
            location::UNCALLED,
            false,
        )
        .unwrap()
        .hit_chance(true)
        .unwrap();
    assert!(chance >= 40);
    assert_eq!(host.ai_turn(critter, None).unwrap(), Some(player));
    // Two attacks (8 -> 4 -> 0); then cost 4 > 0 ends the loop, and AP 0
    // skips the max-distance rule.
    assert_eq!(rounds(&host.world, pistol), 8);
    assert_eq!(host.world.critter_state(critter).unwrap().ap, 0);
    assert_eq!(
        host.world.critter_state(critter).unwrap().maneuvers & intent::STOP,
        0
    );
    // Facing the target, and still referencing it through the setter.
    assert_eq!(host.world.get(critter).unwrap().opponent, player);
    // The player's onlooker alert came from damage application.
    assert_ne!(
        host.world.critter_state(player).unwrap().maneuvers & intent::ALERT,
        0
    );

    // Range-aware chance below 40 but no-range chance at least 40: approach
    // (with a move taunt), spending the AP on movement.
    let (mut host, critter, pistol) = duel(19, 40);
    let profile = host
        .world
        .attack_profile(
            critter,
            player,
            hit_mode::RIGHT_PRIMARY,
            location::UNCALLED,
            false,
        )
        .unwrap();
    let (aware, plain) = (
        profile.hit_chance(true).unwrap(),
        profile.hit_chance(false).unwrap(),
    );
    assert!(aware < 40 && plain >= 40, "{aware} {plain}");
    let before = host.world.object_distance(critter, player);
    host.ai_turn(critter, None).unwrap();
    assert!(host.world.object_distance(critter, player) < before);
    assert_eq!(rounds(&host.world, pistol), 10);
    assert!(
        taunt_effects(&host)
            .iter()
            .any(|(who, text)| *who == critter && text.starts_with(b"taunt 101"))
    );
    // A no-range chance below 40 flees at once (within max_dist 8).
    let (mut host, critter, _) = duel(6, -100);
    host.ai_turn(critter, None).unwrap();
    assert_ne!(
        host.world.critter_state(critter).unwrap().maneuvers & intent::FLEE,
        0
    );
    assert!(host.world.object_distance(critter, player) > 6);

    // Out of ammunition with none to find: sound, hands, message 5001,
    // unwield, and weapon switching (nothing found: punch).
    let (mut host, critter, pistol) = duel(3, 100);
    host.world.get_mut(pistol).unwrap().raw.state = ObjectState::Other {
        word: 0,
        detail: loaded(0, 30),
    };
    host.ai_turn(critter, None).unwrap();
    assert!(host.world.effects.iter().any(|effect| matches!(
        effect,
        WorldEffect::WeaponSound { effect: 2, name, .. } if name == b"WOA1XXX1"
    )));
    let name = String::from_utf8(host.world.get(critter).unwrap().name.clone()).unwrap();
    assert!(
        messages(&host)
            .contains(&format!("{name} is out of ammo for Object 00000046.").into_bytes())
    );
    assert_eq!(host.world.wielded(critter, hit_mode::RIGHT_PRIMARY), None);

    // Reloading from inventory: 2 AP with no prior check; an emptied unit
    // is destroyed while the rest of the stack keeps a full count.
    let (mut host, critter, pistol) = duel(3, 100);
    host.assets.prototypes.insert(30, ammo(30, 10, 6));
    host.world.get_mut(pistol).unwrap().raw.state = ObjectState::Other {
        word: 0,
        detail: loaded(0, 30),
    };
    let clip = carry(
        &mut host.world,
        critter,
        ammo(30, 10, 6),
        3,
        ObjectDetail::Ammunition(4),
    );
    state(&mut host.world, critter).ap = 1;
    let mut engagement_ap = host.world.critter_state(critter).unwrap().ap;
    host.ai_reload(critter, player, Some(pistol)).unwrap();
    assert_eq!(rounds(&host.world, pistol), 4);
    engagement_ap = (engagement_ap - 2).max(0);
    assert_eq!(host.world.critter_state(critter).unwrap().ap, engagement_ap);
    assert!(
        host.world.get(clip).is_none(),
        "the emptied unit is destroyed"
    );
    let (rest, count) = host.world.get(critter).unwrap().inventory[1];
    assert_eq!(count, 2);
    assert_eq!(
        host.world.get(rest).unwrap().raw.state,
        ObjectState::Other {
            word: 0,
            detail: ObjectDetail::Ammunition(6)
        }
    );
    assert!(
        messages(&host)
            .last()
            .unwrap()
            .ends_with(b"reloads Object 00000046.")
    );
    assert!(host.world.effects.iter().any(|effect| matches!(
        effect,
        WorldEffect::WeaponSound { effect: 0, name, .. } if name == b"WRA1XXX1"
    )));
    // A full magazine transfers nothing but still reloads (2 AP).
    host.world.get_mut(pistol).unwrap().raw.state = ObjectState::Other {
        word: 0,
        detail: loaded(10, 30),
    };
    assert_eq!(host.reload_weapon(pistol, rest), 6);
    // A different loaded PID is not permitted.
    let other = carry(
        &mut host.world,
        critter,
        ammo(31, 10, 6),
        1,
        ObjectDetail::Ammunition(6),
    );
    assert_eq!(host.reload_weapon(pistol, other), -1);
}

#[test]
fn spec009_weapon_switching_pickup_and_unarmed_upgrade() {
    // Unarmed against a target that visibly holds a weapon: switch to the
    // inventory pistol (wielded, art weapon code 5) and shoot.
    let mut world = world();
    let player = world.player;
    set_hp(&mut world, player, 1000);
    let art = world.get(player).unwrap().raw.art_id;
    world.get_mut(player).unwrap().raw.art_id = Fid(art).0 | (5 << 12);
    let critter = npc(&mut world, 20700, 1, 0, &[(4, 1)]);
    set_skill(&mut world, critter, 0, 100);
    let pistol = carry(
        &mut world,
        critter,
        weapon(70, 0x06, 4, 1, 20, 10, 100),
        1,
        loaded(10, 30),
    );
    state(&mut world, critter).ap = 8;
    world.combat = true;
    world.ai.snapshot = vec![player, critter];
    let mut host = ai_host(world, AiAssets::default());
    host.ai_attack(critter, player).unwrap();
    assert_eq!(
        host.world.wielded(critter, hit_mode::RIGHT_PRIMARY),
        Some(pistol)
    );
    assert_eq!(Fid(host.world.get(critter).unwrap().raw.art_id).weapon(), 5);
    assert_eq!(rounds(&host.world, pistol), 8);
    assert!(
        host.world
            .effects
            .iter()
            .any(|effect| matches!(effect, WorldEffect::CritterAnimation { animation: 38, .. }))
    );

    // Environment search and retrieval: walk with the AP budget, need
    // adjacency, pay 3 AP; out of range or unaffordable fails.
    let mut world = self::world();
    let critter = npc(&mut world, 20500, 1, 0, &[]);
    state(&mut world, critter).ap = 8;
    let near = world
        .spawn(weapon(71, 0x03, 3, 4, 1, 0, 30), 21100, 0)
        .unwrap();
    let far = world
        .spawn(weapon(72, 0x06, 4, 4, 20, 10, 300), 20500 + 11 * 200, 0)
        .unwrap();
    world.combat = true;
    let mut host = ai_host(world, AiAssets::default());
    // Perception 5 + 5 = 10: the farther (better) weapon is out of range.
    assert_eq!(
        host.environment_search(critter, vault_formats::pro::ItemSubtype::Weapon),
        Some(near)
    );
    let (switched, found) = host.switch_weapon(critter, host.world.player).unwrap();
    assert_eq!(found, Some(near));
    assert!(switched.success && switched.weapon);
    assert_eq!(host.world.get(near).unwrap().owner, Some(critter));
    // Two hexes walked (stopping adjacent), then 3 AP for the pick-up.
    assert_eq!(host.world.critter_state(critter).unwrap().ap, 8 - 2 - 3);
    let _ = far;
    state(&mut host.world, critter).ap = 2;
    let spare = host
        .world
        .spawn(
            weapon(73, 0x03, 3, 4, 1, 0, 30),
            hex(&host.world, critter) + 200,
            0,
        )
        .unwrap();
    assert_eq!(
        host.retrieve(critter, spare).unwrap(),
        None,
        "fewer than 3 AP"
    );
    // Quadrupeds and robots never use the environment.
    host.world
        .get_mut(critter)
        .unwrap()
        .character
        .as_mut()
        .unwrap()
        .body = 2;
    assert_eq!(
        host.environment_search(critter, vault_formats::pro::ItemSubtype::Weapon),
        None
    );
}

#[test]
fn spec009_turn_tree_requested_target_join_stop_and_party_follow() {
    // A scripted request's first turn: the requested target is attacked
    // with the request accuracy added to the threshold comparisons.
    let (mut host, critter, pistol) = duel(19, 40);
    let player = host.world.player;
    host.world.combat = false;
    host.world.ai.snapshot.clear();
    host.world.pending_combat = Some(CombatRequest {
        attacker: critter,
        defender: player,
        accuracy: 60,
        damage: [0, 1],
        flags: None,
    });
    host.combat_ai = Box::new(PacketAi);
    host.tick(0).unwrap();
    assert!(host.world.combat_state.player_turn);
    assert!(
        rounds(&host.world, pistol) < 10,
        "attacked instead of approaching"
    );
    assert_eq!(host.world.ai.accuracy, 0);
    assert!(host.world.ai.requested.is_none());
    // The snapshot holds the collected combat list (hex order).
    let mut by_hex = host.world.ai.snapshot.clone();
    by_hex.sort_by_key(|c| hex(&host.world, *c));
    assert_eq!(host.world.ai.snapshot, by_hex);
    assert!(host.world.ai.snapshot.contains(&player));
    // Combat start referenced the pair through the setter.
    assert_eq!(host.world.get(player).unwrap().opponent, critter);

    // Wants-to-stop: AP at maximum, or a danger source it cannot perceive.
    host.end_player_turn();
    host.end_combat().unwrap();
    assert!(host.world.ai.snapshot.is_empty());

    // Party members follow the player beyond 5 hexes (no taunt).
    let mut world = self::world();
    let player = world.player;
    let member = npc(&mut world, 20100 + 9 * 200, 0, 0, &[]);
    world.get_mut(member).unwrap().raw.id = 18000;
    world.party.insert(member);
    state(&mut world, member).ap = 3;
    let mut host = ai_host(world, AiAssets::default());
    assert!(host.world.party_member(member));
    host.ai_turn(member, None).unwrap();
    assert_eq!(host.world.object_distance(member, player), 6);
    assert!(taunt_effects(&host).is_empty());
    // Below 18000 nothing is a party member, even in the party list.
    host.world.get_mut(member).unwrap().raw.id = 17999;
    assert!(!host.world.party_member(member));

    // Hurt and unopposed (non-party): flee when the living last attacker is
    // C itself and damage was taken last turn.
    let mut world = self::world();
    let critter = npc(&mut world, 20100 + 3 * 200, 1, 0, &[]);
    world.get_mut(critter).unwrap().opponent = critter;
    state(&mut world, critter).damage = 4;
    state(&mut world, critter).ap = 2;
    let mut host = ai_host(world, AiAssets::default());
    assert_eq!(host.ai_turn(critter, None).unwrap(), None);
    assert_ne!(
        host.world.critter_state(critter).unwrap().maneuvers & intent::FLEE,
        0
    );
}

#[test]
fn spec009_team_switching_and_random_retarget() {
    let mut world = world();
    let a = npc(&mut world, 20300, 1, 0, &[]);
    let b = npc(&mut world, 20500, 2, 0, &[]);
    world.get_mut(a).unwrap().opponent = b;
    let mut host = ai_host(world, AiAssets::default());
    host.set_team(a, 3);
    assert_eq!(host.world.get(a).unwrap().opponent, b);
    host.set_team(a, 2);
    assert_eq!(host.world.critter_state(a).unwrap().team, 2);
    assert_eq!(
        host.world.get(a).unwrap().opponent,
        0,
        "cleared: now a teammate"
    );

    // Random retarget: no draw for an empty snapshot; otherwise one draw
    // and a wrapping scan for an eligible forward-sector entry.
    let mut world = self::world();
    let player = world.player;
    let attacker = npc(&mut world, 20100 + 2 * 200, 1, 0, &[]);
    world.get_mut(attacker).unwrap().raw.orientation = 2;
    let ahead = npc(&mut world, 20100 + 3 * 200, 2, 0, &[]);
    let behind = npc(&mut world, 20100 + 200, 2, 0, &[]);
    state(&mut world, attacker).ap = 8;
    let mut host = ai_host(world, AiAssets::default());
    let mut rng = crate::ParkMiller::seeded(3).unwrap();
    let before = rng.clone();
    assert_eq!(
        host.random_target(attacker, player, hit_mode::PUNCH, &mut rng)
            .unwrap(),
        None
    );
    assert_eq!(rng, before);
    host.world.ai.snapshot = vec![behind, attacker, player, ahead];
    for seed in 1..20 {
        let mut rng = crate::ParkMiller::seeded(seed).unwrap();
        let mut expected = rng.clone();
        expected.range(0, 3).unwrap();
        assert_eq!(
            host.random_target(attacker, player, hit_mode::PUNCH, &mut rng)
                .unwrap(),
            Some(ahead)
        );
        assert_eq!(rng, expected);
    }
    // A dead candidate fails preflight: nothing qualifies.
    state(&mut host.world, ahead).results.0 = effect::DEAD;
    let mut rng = crate::ParkMiller::seeded(1).unwrap();
    assert_eq!(
        host.random_target(attacker, player, hit_mode::PUNCH, &mut rng)
            .unwrap(),
        None
    );
}

/// Scripted outcomes in draw order; each must lie in its requested range.
struct Draws(std::collections::VecDeque<i32>);
impl Draws {
    fn new(values: &[i32]) -> Self {
        Self(values.iter().copied().collect())
    }
}
impl RandomSource for Draws {
    fn range(&mut self, first: i32, last: i32) -> Result<i32, WorldError> {
        let value = self.0.pop_front().expect("an outcome for every draw");
        assert!(
            (first..=last).contains(&value),
            "{value} outside {first}..={last}"
        );
        Ok(value)
    }
}

/// SPEC-007 Fixture E geometry, on one hex column: the attacker, a living
/// critter X1, the defender, then X2 (DT 2, DR 20) or a wall, with a
/// single-fire weapon of range 20 and damage 1-20. The clock is before the
/// first day, so no critical-upgrade draws are made.
fn fixture_e(blocker: &str, x1_team: i32) -> (WorldHost<AiAssets>, [ObjectRef; 4]) {
    let mut world = world();
    let player = world.player;
    world.get_mut(player).unwrap().raw.hex = Some(0);
    let attacker = npc(&mut world, 20100 + 2 * 200, 1, 0, &[]);
    world.get_mut(attacker).unwrap().raw.orientation = 2;
    let x1 = npc(&mut world, 20100 + 3 * 200, x1_team, 0, &[]);
    let defender = npc(&mut world, 20100 + 5 * 200, 2, 0, &[]);
    let far = match blocker {
        "wall" => {
            let wall = world
                .spawn(
                    Arc::new(Prototype::parse(&fixture::prototype(3, 0)).unwrap()),
                    20100 + 6 * 200,
                    0,
                )
                .unwrap();
            world.get_mut(wall).unwrap().raw.flags = BasicFlags(0);
            wall
        }
        _ => npc(&mut world, 20100 + 6 * 200, 2, 0, &[(17, 2), (24, 20)]),
    };
    let gun = carry(
        &mut world,
        attacker,
        weapon(60, 0x06, 4, 20, 20, 10, 100),
        1,
        loaded(10, 30),
    );
    world
        .equip(attacker, gun, BasicFlags::RIGHT_HAND.0)
        .unwrap();
    state(&mut world, attacker).ap = 8;
    let mut assets = AiAssets::default();
    assets.prototypes.insert(30, ammo(30, 0, 10));
    (ai_host(world, assets), [attacker, x1, defender, far])
}
fn miss(
    host: &WorldHost<AiAssets>,
    attacker: ObjectRef,
    defender: ObjectRef,
    outcomes: &[i32],
) -> (crate::Spray, Option<crate::host_ai::Victim>) {
    let attack = host
        .world
        .attack_profile(
            attacker,
            defender,
            hit_mode::RIGHT_PRIMARY,
            location::UNCALLED,
            false,
        )
        .unwrap();
    let mut draws = Draws::new(outcomes);
    let (spray, victim, _) = host
        .resolve_attack(attacker, defender, &attack, &mut draws)
        .unwrap();
    assert!(draws.0.is_empty(), "unused outcomes {:?}", draws.0);
    (spray, victim)
}
fn line_of_fire_chance(host: &WorldHost<AiAssets>, attacker: ObjectRef, critter: ObjectRef) -> i32 {
    host.world
        .attack_profile(
            attacker,
            critter,
            hit_mode::RIGHT_PRIMARY,
            location::TORSO,
            false,
        )
        .unwrap()
        .hit_chance(true)
        .unwrap()
        / 3
}

#[test]
fn spec007_fixture_e_missed_shot_bystanders() {
    use crate::host_ai::Victim;
    // E1 under the Stage A safe rule: X1's line-of-fire draw selects it, it
    // is committed, and its one round (DT 0, DR 0) is the damage draw.
    let (host, [attacker, x1, defender, x2]) = fixture_e("critter", 2);
    let chance = line_of_fire_chance(&host, attacker, x1);
    assert!(chance >= 1);
    let (spray, victim) = miss(&host, attacker, defender, &[100, chance, 12]);
    assert_eq!(spray.main.code, 1);
    assert_ne!(spray.main.defender_flags & effect::HIT, 0);
    assert_eq!((spray.main.damage, spray.main.multiplier), (12, 2));
    assert!(matches!(victim, Some(Victim::Bystander(victim, _)) if victim == x1));
    // E2: X1's draw fails, the defender is passed with no draw, X2's fails;
    // Stage B's ray from the defender reports X2: 12 - 2 = 10, less 20%.
    let x2_chance = line_of_fire_chance(&host, attacker, x2);
    let (spray, victim) = miss(
        &host,
        attacker,
        defender,
        &[100, chance + 1, x2_chance + 1, 12],
    );
    assert_eq!(spray.main.damage, 8);
    assert!(matches!(victim, Some(Victim::Bystander(victim, _)) if victim == x2));
    // A projectile-through X2 is never hit: no candidate, no damage draw.
    let mut host = host;
    host.world.get_mut(x2).unwrap().raw.flags = BasicFlags::PROJECTILE_THROUGH;
    let (spray, victim) = miss(&host, attacker, defender, &[100, chance + 1]);
    assert!(victim.is_none());
    assert_eq!(spray.main.defender_flags & effect::HIT, 0);
    // E3: a wall in X2's place is an obstruction for Stage A and Stage B's
    // victim: the hit bit, 0 damage, and no damage draw.
    let (host, [attacker, _, defender, wall]) = fixture_e("wall", 2);
    let (spray, victim) = miss(&host, attacker, defender, &[100, chance + 1]);
    assert_eq!(victim, Some(Victim::Object(wall)));
    assert_ne!(spray.main.defender_flags & effect::HIT, 0);
    assert_eq!(spray.main.damage, 0);
}

#[test]
fn spec007_missed_shot_at_the_extension_hex_hits_the_defender_at_numerator_2() {
    use crate::host_ai::Victim;
    // Range 3 at distance 3: E is the defender's own hex. With no other
    // blocker the "miss" selects the intended defender for one round.
    let mut world = world();
    let player = world.player;
    world.get_mut(player).unwrap().raw.hex = Some(0);
    let attacker = npc(&mut world, 20100 + 2 * 200, 1, 0, &[]);
    let defender = npc(&mut world, 20100 + 5 * 200, 2, 0, &[]);
    let gun = carry(
        &mut world,
        attacker,
        weapon(60, 0x06, 4, 20, 3, 10, 100),
        1,
        loaded(10, 30),
    );
    world
        .equip(attacker, gun, BasicFlags::RIGHT_HAND.0)
        .unwrap();
    let mut assets = AiAssets::default();
    assets.prototypes.insert(30, ammo(30, 0, 10));
    let host = ai_host(world, assets);
    let (spray, victim) = miss(&host, attacker, defender, &[100, 9]);
    assert!(matches!(victim, Some(Victim::Bystander(victim, _)) if victim == defender));
    assert_eq!((spray.main.damage, spray.main.multiplier), (9, 2));
    assert_eq!(spray.main.attacker_flags & effect::CRITICAL, 0);
}

#[test]
fn spec007_bystander_messages_and_same_team_retaliation() {
    // A same-team bystander does not retaliate; another team's does.
    for (team, retaliates) in [(1, false), (2, true)] {
        let (mut host, [attacker, x1, defender, _]) = fixture_e("critter", team);
        for (object, name) in [(attacker, "Gunner"), (x1, "Bob"), (defender, "Target")] {
            host.world.get_mut(object).unwrap().name = name.as_bytes().to_vec();
        }
        host.assets.combat = Some(Arc::new(
            MsgCatalog::parse(b"{506}{}{You}\n{608}{}{Oops! %s was hit instead of you!}\n{609}{}{Oops! %s was hit instead of %s!}\n{615}{}{%s missed}\n")
                .unwrap(),
        ));
        host.world.ai.combat_taunts = Some(0);
        let main = host
            .world
            .attack_profile(
                attacker,
                defender,
                hit_mode::RIGHT_PRIMARY,
                location::UNCALLED,
                false,
            )
            .unwrap()
            .hit_chance(true)
            .unwrap();
        let chance = line_of_fire_chance(&host, attacker, x1);
        host.world.rng = crate::fixture_items::seed_where(&[(1, 100), (1, 100)], |v| {
            v[0] > main && v[1] <= chance
        });
        let result = host
            .perform_attack(
                attacker,
                defender,
                hit_mode::RIGHT_PRIMARY,
                location::UNCALLED,
                false,
            )
            .unwrap()
            .unwrap();
        assert_ne!(result.defender_flags & effect::HIT, 0);
        assert!(messages(&host).contains(&b"Oops! Bob was hit instead of Target!".to_vec()));
        let opponent = host.world.get(x1).unwrap().opponent;
        assert_eq!(opponent == attacker, retaliates, "team {team}");
    }
    // A wall impact shows the attacker's "missed" text (615, male NPC).
    let (mut host, [attacker, x1, defender, _]) = fixture_e("wall", 2);
    host.world.get_mut(attacker).unwrap().name = b"Gunner".to_vec();
    host.assets.combat = Some(Arc::new(
        MsgCatalog::parse(b"{615}{}{%s missed}\n").unwrap(),
    ));
    host.world.ai.combat_taunts = Some(0);
    let main = host
        .world
        .attack_profile(
            attacker,
            defender,
            hit_mode::RIGHT_PRIMARY,
            location::UNCALLED,
            false,
        )
        .unwrap()
        .hit_chance(true)
        .unwrap();
    let chance = line_of_fire_chance(&host, attacker, x1);
    host.world.rng =
        crate::fixture_items::seed_where(&[(1, 100), (1, 100)], |v| v[0] > main && v[1] > chance);
    let result = host
        .perform_attack(
            attacker,
            defender,
            hit_mode::RIGHT_PRIMARY,
            location::UNCALLED,
            false,
        )
        .unwrap()
        .unwrap();
    assert_ne!(result.defender_flags & effect::HIT, 0, "recorded hit bit");
    assert_eq!(messages(&host), [b"Gunner missed".to_vec()]);
}

#[test]
fn spec007_fixture_d_punch_to_kick_draw() {
    // A bare-handed NPC and a defender beyond punch range; `missing` lists
    // animations without art. The generator is positioned so the first
    // D(1, 4) gives `first`.
    let build = |first: i32, missing: &[u8]| {
        let mut world = world();
        let attacker = npc(&mut world, 20100 + 2 * 200, 1, 0, &[]);
        let far = npc(&mut world, 20100 + 9 * 200, 2, 0, &[]);
        state(&mut world, attacker).ap = 8;
        let mut assets = AiAssets::default();
        assets.missing.extend(missing);
        let mut host = ai_host(world, assets);
        host.world.rng = crate::fixture_items::seed_where(&[(1, 4)], |v| v[0] == first);
        (host, attacker, far)
    };
    let after_one = |mut rng: crate::ParkMiller| {
        rng.range(1, 4).unwrap();
        rng
    };
    // D1: draw 1 with kick art (animation 17) switches to kick, after
    // exactly one draw.
    let (mut host, attacker, _) = build(1, &[]);
    let expected = after_one(host.world.rng.clone());
    assert_eq!(
        host.kick_substitution(attacker, hit_mode::PUNCH).unwrap(),
        hit_mode::KICK
    );
    assert_eq!(host.world.rng, expected);
    // D2: the same draw without kick art keeps punch.
    let (mut host, attacker, _) = build(1, &[17]);
    assert_eq!(
        host.kick_substitution(attacker, hit_mode::PUNCH).unwrap(),
        hit_mode::PUNCH
    );
    assert_eq!(host.world.rng, expected);
    // D3: results 2-4 keep punch.
    for draw in 2..=4 {
        let (mut host, attacker, _) = build(draw, &[]);
        assert_eq!(
            host.kick_substitution(attacker, hit_mode::PUNCH).unwrap(),
            hit_mode::PUNCH
        );
    }
    // D4: a direct kick, or any other mode, makes no draw.
    let (mut host, attacker, _) = build(1, &[]);
    let before = host.world.rng.clone();
    for mode in [hit_mode::KICK, hit_mode::RIGHT_PRIMARY] {
        assert_eq!(host.kick_substitution(attacker, mode).unwrap(), mode);
    }
    assert_eq!(host.world.rng, before);
    // D5: the kick draw precedes the range check and stays consumed when the
    // attack is refused; a kick request is refused with no draw at all.
    let (mut host, attacker, far) = build(4, &[]);
    let expected = after_one(host.world.rng.clone());
    let refused = host
        .perform_attack(attacker, far, hit_mode::PUNCH, location::UNCALLED, false)
        .unwrap();
    assert_eq!(refused, Err(crate::Refusal::Range));
    assert_eq!(host.world.rng, expected);
    let refused = host
        .perform_attack(attacker, far, hit_mode::KICK, location::UNCALLED, false)
        .unwrap();
    assert_eq!(refused, Err(crate::Refusal::Range));
    assert_eq!(host.world.rng, expected);
}

#[test]
fn spec009_explosion_attribution_and_noticed_drops() {
    let mut world = world();
    let player = world.player;
    let source = npc(&mut world, 20300, 1, 0, &[]);
    let hurt = npc(&mut world, 20500, 1, 0, &[]);
    let dead = npc(&mut world, 20700, 2, 0, &[]);
    state(&mut world, dead).results.0 = effect::DEAD;
    set_hp(&mut world, dead, 0);
    let ally = npc(&mut world, 20900, 0, 0, &[]);
    let mut host = ai_host(world, AiAssets::default());
    host.attribute_explosion(source, &[source, hurt, dead]);
    assert_eq!(host.world.get(hurt).unwrap().opponent, source);
    assert_eq!(host.world.get(dead).unwrap().opponent, 0);
    assert_eq!(host.world.get(source).unwrap().opponent, 0);
    // One D10 Perception check per eligible critter (not the player or its
    // team), in set order; the first success starts a combat request.
    let mut expected = host.world.rng.clone();
    let checks: Vec<bool> = (0..2)
        .map(|_| expected.range(1, 10).unwrap() <= 5)
        .collect();
    host.notice_explosive_drop(&[player, ally, hurt, dead])
        .unwrap();
    assert_eq!(host.world.rng, expected);
    assert_eq!(host.world.get(hurt).unwrap().opponent == player, checks[0]);
    let first = [hurt, dead].into_iter().zip(checks).find(|(_, ok)| *ok);
    assert_eq!(
        host.world.pending_combat.map(|request| request.attacker),
        first.map(|(critter, _)| critter)
    );
}

#[test]
fn spec009_join_and_stop_predicates_use_live_danger() {
    let mut world = world();
    let player = world.player;
    let fighter = npc(&mut world, 20100 + 3 * 200, 1, 0, &[]);
    let waiter = npc(&mut world, 20100 + 5 * 200, 1, 0, &[]);
    let mut host = ai_host(world, AiAssets::default());
    host.start_combat(player, fighter).unwrap();
    let mut ai = PacketAi;
    // The fighter's living last attacker (the player) is perceived and its
    // AP is below maximum: it does not want to stop.
    state(&mut host.world, fighter).ap = 3;
    assert!(!host.request_end_combat(&mut ai));
    // At maximum AP it would stop, but the waiter's attacker search finds
    // the player (its teammate's attacker): it wants to join.
    state(&mut host.world, fighter).ap = 8;
    assert!(!host.request_end_combat(&mut ai));
    // Facing away with Perception 1 (rear limit 2), the waiter loses track.
    host.world.get_mut(waiter).unwrap().raw.orientation = 2;
    host.world
        .get_mut(waiter)
        .unwrap()
        .character
        .as_mut()
        .unwrap()
        .base[1] = 1;
    assert!(host.request_end_combat(&mut ai));
}

/// A one-pixel-per-frame FRM with `frames` frames in a direction shared by
/// all six, its fps and its action frame (SPEC-001 layout).
fn animation_frm(frames: u16, fps: u16, action: u16) -> Vec<u8> {
    let mut bytes = Vec::new();
    bytes.extend(4_u32.to_be_bytes());
    bytes.extend(fps.to_be_bytes());
    bytes.extend(action.to_be_bytes());
    bytes.extend(frames.to_be_bytes());
    bytes.extend([0; 12 + 12 + 24]);
    bytes.extend((u32::from(frames) * 13).to_be_bytes());
    for _ in 0..frames {
        bytes.extend(1_u16.to_be_bytes());
        bytes.extend(1_u16.to_be_bytes());
        bytes.extend(1_u32.to_be_bytes());
        bytes.extend([0; 4]);
        bytes.push(1);
    }
    bytes
}
fn loaded_art(frames: u16, fps: u16, action: u16) -> Arc<vault_formats::FrmArchive> {
    Arc::new(vault_formats::FrmArchive::parse(&animation_frm(frames, fps, action)).unwrap())
}
/// A show's entries: owner, delay, and what each does.
fn labels(
    sequence: &crate::choreography::Sequence,
) -> Vec<(crate::choreography::Owner, i32, String)> {
    use crate::choreography::Action;
    let art = |fid: u32| {
        let fid = Fid(fid);
        if fid.category() == 1 {
            fid.animation().to_string()
        } else {
            format!("{:#x}", fid.0)
        }
    };
    sequence
        .entries()
        .iter()
        .map(|entry| {
            let label = match &entry.action {
                Action::Turn { orientation, .. } => format!("turn {orientation}"),
                Action::Sound { name, .. } => format!("sound {}", String::from_utf8_lossy(name)),
                Action::Taunt(_) => "taunt".into(),
                Action::Release(index) => format!("release {index}"),
                Action::Hide => "hide".into(),
                Action::Unhide => "unhide".into(),
                Action::SetArt { art: fid } => format!("art {}", art(*fid)),
                Action::Place { .. } => "place".into(),
                Action::Remove => "remove".into(),
                Action::Corpse { .. } => "corpse".into(),
                Action::Animate { art: fid, hide, .. } => {
                    format!("animate {}{}", art(*fid), if *hide { " hide" } else { "" })
                }
                Action::Move { .. } => "move".into(),
            };
            (entry.owner, entry.delay, label)
        })
        .collect()
}
/// The last node of a show's projectile flight.
fn flight_end(host: &WorldHost<AiAssets>) -> i32 {
    host.world
        .shows
        .front()
        .unwrap()
        .main
        .entries()
        .iter()
        .find_map(|entry| match &entry.action {
            crate::choreography::Action::Move { nodes, .. } => nodes.last().map(|node| node.0),
            _ => None,
        })
        .expect("a flight")
}
/// Play every queued show, a pass per 100 ms; returns the sounds started.
fn play(host: &mut WorldHost<AiAssets>) -> Vec<Vec<u8>> {
    host.world.effects.clear();
    let mut now = 100;
    while host.presenting() {
        host.step_shows(now);
        now += 100;
        assert!(now < 1_000_000, "the shows end");
    }
    host.world
        .effects
        .iter()
        .filter_map(|effect| match effect {
            WorldEffect::SoundEffect { name, .. } => Some(name.clone()),
            _ => None,
        })
        .collect()
}

#[test]
fn spec013_weapon_explosions_play_where_the_projectile_ends() {
    use crate::{choreography::Owner, host_choreography::Playback, skills::explosion_art};
    use vault_formats::pro::{ItemData, PrototypeData};
    // Fixture E's gun given a PID, a damage type, an attack mode and a
    // projectile with art; the shows wait in the queue.
    let explosive_gun = |pid: u32, damage_type: i32, extended: i32| {
        let (mut host, [attacker, _, defender, _]) = fixture_e("critter", 2);
        let gun = host
            .world
            .wielded(attacker, hit_mode::RIGHT_PRIMARY)
            .unwrap();
        let mut prototype = (*weapon(pid, extended, 4, 20, 20, 10, 100)).clone();
        if let PrototypeData::Item(item) = &mut prototype.data
            && let ItemData::Weapon(weapon) = &mut item.data
        {
            weapon.damage_type = damage_type;
            weapon.projectile_pid = 0x0500_0009;
        }
        let object = host.world.get_mut(gun).unwrap();
        object.raw.pid = pid;
        object.prototype = Some(Arc::new(prototype));
        let mut rocket = fixture::prototype(5, 0);
        fixture::word(&mut rocket, 0, 0x0500_0009);
        fixture::word(&mut rocket, 8, 0x0500_0009);
        host.assets
            .prototypes
            .insert(0x0500_0009, Arc::new(Prototype::parse(&rocket).unwrap()));
        host.world.ai.combat_taunts = Some(0);
        host.playback = Playback::RealTime;
        (host, attacker, defender)
    };
    let attack = |host: &mut WorldHost<AiAssets>, attacker, defender, hits: bool| {
        let chance = host
            .world
            .attack_profile(
                attacker,
                defender,
                hit_mode::RIGHT_PRIMARY,
                location::UNCALLED,
                false,
            )
            .unwrap()
            .hit_chance(true)
            .unwrap();
        host.world.rng = crate::fixture_items::seed_where(&[(1, 100)], |v| {
            if hits {
                v[0] <= chance.min(90)
            } else {
                v[0] > chance && v[0] < 96
            }
        });
        host.perform_attack(
            attacker,
            defender,
            hit_mode::RIGHT_PRIMARY,
            location::UNCALLED,
            false,
        )
        .unwrap()
        .unwrap()
    };
    // The explosion entries: the projectile turns into the center art, and
    // the six adjacent arts follow with direction 0's start delay.
    let blasts = |host: &WorldHost<AiAssets>, art: u32| {
        let show = host.world.shows.front().unwrap();
        labels(&show.main)
            .into_iter()
            .filter(|(_, _, label)| *label == format!("animate {art:#x} hide"))
            .map(|(owner, delay, _)| (owner, delay))
            .collect::<Vec<_>>()
    };
    // Explosion damage, not thrown (a Rocket Launcher): roktxpd.frm where
    // the rocket ends, the defender's hex on a hit.
    let (mut host, attacker, defender) = explosive_gun(13, 6, 0x06);
    let result = attack(&mut host, attacker, defender, true);
    assert_ne!(result.defender_flags & effect::HIT, 0);
    assert!(host.world.shows.front().unwrap().explosive);
    assert_eq!(flight_end(&host), hex(&host.world, defender));
    let rocket = blasts(&host, explosion_art::ROCKET);
    assert_eq!(rocket.len(), 7);
    assert!(matches!(rocket[0].0, Owner::Shown(_)), "a created rocket");
    assert_eq!(
        rocket.iter().map(|(_, delay)| *delay).collect::<Vec<_>>(),
        [0, 2, 0, 0, 0, 0, 0]
    );
    let show = host.world.shows.front().unwrap().clone();
    let center = hex(&host.world, defender);
    for (owner, _) in &rocket[1..] {
        let place = show.main.display(*owner).unwrap().hex.unwrap();
        assert_eq!(
            vault_formats::HexGrid::lattice_distance(center, place).unwrap(),
            1
        );
    }
    play(&mut host);
    assert!(!host.presenting());
    // Thrown plasma damage (the Plasma Grenade): expp.frm, delay 4.
    let (mut host, attacker, defender) = explosive_gun(26, 3, 0x05);
    attack(&mut host, attacker, defender, true);
    let plasma = blasts(&host, explosion_art::PLASMA);
    assert_eq!((plasma.len(), plasma[1].1), (7, 4));
    // Plasma damage that is not thrown never explodes: the projectile is
    // erased, then the hit sound plays on the weapon.
    let (mut host, attacker, defender) = explosive_gun(15, 3, 0x06);
    attack(&mut host, attacker, defender, true);
    let show = host.world.shows.front().unwrap();
    assert!(!show.explosive);
    let entries = labels(&show.main);
    let erase = entries
        .iter()
        .position(|(owner, delay, label)| {
            matches!(owner, Owner::Shown(_)) && *delay == -1 && label == "remove"
        })
        .expect("the projectile's erase");
    assert!(entries[erase + 1].2.starts_with("sound WH"));
    // A miss explodes where the projectile ends, the impact hex.
    let (mut host, attacker, defender) = explosive_gun(13, 6, 0x06);
    let result = attack(&mut host, attacker, defender, false);
    if result.code != 0 {
        assert!(host.world.shows.front().unwrap().explosive);
        assert_ne!(flight_end(&host), hex(&host.world, attacker));
    }
    play(&mut host);
    assert!(!host.presenting());
}

// SPEC-007 §3.9 thrown weapons: the weapon payload's damage type (file
// 0x45) and projectile PID (file 0x51), and the projectile prototype.
const DAMAGE_TYPE: usize = 69;
const PROJECTILE: usize = 81;
const PROJECTILE_PID: u32 = 0x0500_0010;
/// The projectile prototype's art (`spear.frm`, `misc.lst` 13).
const PROJECTILE_ART: u32 = 0x0500_000D;

/// A primary-throw weapon (PID `pid`, weapon code `code`) whose projectile
/// is `PROJECTILE_PID`.
fn thrown(pid: u32, code: i32, damage_type: i32) -> Arc<Prototype> {
    let mut bytes = item_bytes(
        3,
        pid,
        &[
            (EXTENDED, 0x05),
            (COST, 10),
            (ANIMATION, code),
            (MIN_DAMAGE, 1),
            (MAX_DAMAGE, 4),
            (DAMAGE_TYPE, damage_type),
            (RANGE, 10),
            (RANGE + 4, 10),
            (PROJECTILE, PROJECTILE_PID as i32),
            (AP, 2),
            (AP + 4, 2),
            (PERK, -1),
        ],
    );
    bytes[WEAPON_SOUND] = b'A';
    Arc::new(Prototype::parse(&bytes).unwrap())
}
/// Fixture F: the player (right hand active) at 20100 holds `held` units
/// of a thrown weapon, with `spare` more unequipped, and throws at an NPC
/// four hexes south.
fn fixture_f(
    pid: u32,
    code: i32,
    damage_type: i32,
    held: i32,
    spare: i32,
) -> (WorldHost<AiAssets>, ObjectRef, ObjectRef, Option<ObjectRef>) {
    let mut world = world();
    let player = world.player;
    let defender = npc(&mut world, 20100 + 4 * 200, 1, 0, &[]);
    let prototype = thrown(pid, code, damage_type);
    let hand = carry(&mut world, player, prototype.clone(), held, loaded(0, -1));
    world.equip(player, hand, BasicFlags::RIGHT_HAND.0).unwrap();
    let spare = (spare > 0).then(|| carry(&mut world, player, prototype, spare, loaded(0, -1)));
    state(&mut world, player).ap = 8;
    let mut assets = AiAssets::default();
    let mut projectile = fixture::prototype(5, 0);
    fixture::word(&mut projectile, 0, PROJECTILE_PID as i32);
    fixture::word(&mut projectile, 8, PROJECTILE_ART as i32);
    assets.prototypes.insert(
        PROJECTILE_PID,
        Arc::new(Prototype::parse(&projectile).unwrap()),
    );
    let mut host = ai_host(world, assets);
    host.world.ai.combat_taunts = Some(0);
    host.playback = crate::host_choreography::Playback::RealTime;
    (host, hand, defender, spare)
}
/// The player's throw, hitting or (not critically) missing.
fn throw(host: &mut WorldHost<AiAssets>, defender: ObjectRef, hits: bool) -> Resolution {
    let player = host.world.player;
    let chance = host
        .world
        .attack_profile(
            player,
            defender,
            hit_mode::RIGHT_PRIMARY,
            location::UNCALLED,
            false,
        )
        .unwrap()
        .hit_chance(true)
        .unwrap();
    host.world.rng = crate::fixture_items::seed_where(&[(1, 100)], |v| {
        if hits {
            v[0] <= chance.min(90)
        } else {
            v[0] > chance && v[0] < 96
        }
    });
    let result = host
        .perform_attack(
            player,
            defender,
            hit_mode::RIGHT_PRIMARY,
            location::UNCALLED,
            false,
        )
        .unwrap()
        .unwrap();
    assert_eq!(result.defender_flags & effect::HIT != 0, hits);
    result
}
/// Inventory entries: item and quantity.
type Entries = Vec<(ObjectRef, i32)>;
/// The player's inventory: hand-flagged entries, then the others.
fn hands(host: &WorldHost<AiAssets>) -> (Entries, Entries) {
    let world = &host.world;
    world
        .get(world.player)
        .unwrap()
        .inventory
        .iter()
        .partition(|(item, _)| {
            world.get(*item).unwrap().raw.flags.0
                & (BasicFlags::LEFT_HAND.0 | BasicFlags::RIGHT_HAND.0)
                != 0
        })
}
/// The player's art weapon code and animation.
fn hand_art(host: &WorldHost<AiAssets>) -> (u8, u8) {
    let art = Fid(host.world.get(host.world.player).unwrap().raw.art_id);
    (art.weapon(), art.animation())
}
#[test]
fn spec007_fixture_f_thrown_weapon_consumption() {
    // F1: a Throwing Knife from a hand stack of 3 hits. A copy with a new
    // ID keeps the other 2 in the hand; the thrown unit lies at the
    // defender's hex with its item art; under the safe rule the hand's
    // weapon code (1) is taken out.
    let (mut host, knife, defender, _) = fixture_f(45, 1, 0, 3, 0);
    let id = host.world.get(knife).unwrap().raw.id;
    throw(&mut host, defender, true);
    let (hand, other) = hands(&host);
    assert_eq!((hand.len(), hand[0].1, other.len()), (1, 2, 0));
    assert_ne!(hand[0].0, knife);
    assert_ne!(host.world.get(hand[0].0).unwrap().raw.id, id);
    let landed = host.world.get(knife).unwrap();
    assert_eq!(landed.owner, None);
    assert_eq!(landed.raw.hex, Some(hex(&host.world, defender)));
    assert_eq!(landed.raw.art_id, landed.prototype.as_ref().unwrap().art_id);
    // §3.9: the requested radius 9 is set at the knife's own intensity, 0.
    assert_eq!(
        (landed.raw.light.radius, landed.raw.light.intensity),
        (0, 0)
    );
    assert_eq!(landed.raw.flags.0 & crate::items::EQUIPPED, 0);
    assert_eq!(hand_art(&host), (1, 0));

    // F2: a Frag Grenade misses at impact hex T. The hand is refilled from
    // the unequipped stack of 3; the grenade explodes at T and is erased.
    let (mut host, grenade, defender, spare) = fixture_f(25, 0, 6, 1, 3);
    throw(&mut host, defender, false);
    // The unit taken from the stack is its old representative; a copy
    // keeps the other 2.
    let (hand, other) = hands(&host);
    assert_eq!(hand, vec![(spare.unwrap(), 1)]);
    assert_eq!(
        other.iter().map(|(_, count)| *count).collect::<Vec<_>>(),
        [2]
    );
    assert!(host.world.get(grenade).is_none(), "erased");
    // It flies to the impact hex and explodes there, after its flight.
    assert!(host.world.shows.front().unwrap().explosive);
    let arrival = flight_end(&host);
    assert_ne!(arrival, hex(&host.world, defender), "a miss");
    play(&mut host);
    assert_eq!(hand_art(&host), (0, 0));

    // F3: a Spear hits; the unequipped Spear refills the hand and its
    // weapon code (4) is taken out.
    let (mut host, spear, defender, spare) = fixture_f(7, 4, 0, 1, 1);
    throw(&mut host, defender, true);
    let (hand, other) = hands(&host);
    assert_eq!((hand, other.len()), (vec![(spare.unwrap(), 1)], 0));
    assert_eq!(
        host.world.get(spear).unwrap().raw.hex,
        Some(hex(&host.world, defender))
    );
    assert_eq!(hand_art(&host), (4, 0));

    // F4: a critical failure that keeps its effect launches nothing.
    let (host, _, defender, _) = fixture_f(19, 0, 0, 1, 0);
    let player = host.world.player;
    let attack = host
        .world
        .attack_profile(
            player,
            defender,
            hit_mode::RIGHT_PRIMARY,
            location::UNCALLED,
            false,
        )
        .unwrap();
    let failure = Resolution {
        attacker_flags: effect::CRITICAL | effect::DROP,
        ..Default::default()
    };
    assert!(host.projectile_launch(player, &attack, &failure).is_none());
    let ordinary = Resolution::default();
    assert!(host.projectile_launch(player, &attack, &ordinary).is_some());

    // F5: a hand stack of 2 with 1 more unequipped. Under the safe rule a
    // hand that keeps units is not refilled (the reference leaves two
    // hand entries), and its weapon code is taken out.
    let (mut host, _, defender, spare) = fixture_f(45, 1, 0, 2, 1);
    throw(&mut host, defender, true);
    let (hand, other) = hands(&host);
    assert_eq!((hand.len(), hand[0].1), (1, 1));
    assert_eq!(other, vec![(spare.unwrap(), 1)]);
    assert_eq!(hand_art(&host), (1, 0));

    // F6: a Flare misses at T: the hand is empty and the Flare lies at T.
    let (mut host, flare, defender, _) = fixture_f(79, 0, 0, 1, 0);
    throw(&mut host, defender, false);
    let (hand, other) = hands(&host);
    assert!(hand.is_empty() && other.is_empty());
    let arrival = flight_end(&host);
    assert_eq!(host.world.get(flare).unwrap().raw.hex, Some(arrival));
    let light = host.world.get(flare).unwrap().raw.light;
    assert_eq!((light.radius, light.intensity), (0, 0), "no light");
    assert_eq!(hand_art(&host), (0, 0));

    // F7: a lit flare (radius 8, intensity 65536) hits. It lands lit, its
    // burn-out event still queued; under the safe rule the thrower's light
    // returns to radius 4 (the reference keeps the raised radius 8).
    let (mut host, lit, defender, _) = fixture_f(205, 0, 0, 1, 0);
    let player = host.world.player;
    host.world.get_mut(lit).unwrap().raw.light = vault_formats::pro::Light {
        radius: 8,
        intensity: 65536,
    };
    host.world.get_mut(player).unwrap().raw.light = vault_formats::pro::Light {
        radius: 8,
        intensity: 65536,
    };
    host.world
        .queue
        .insert(0, 18000, lit, crate::queue::EventKind::Other(7))
        .unwrap();
    throw(&mut host, defender, true);
    let landed = host.world.get(lit).unwrap();
    assert_eq!(landed.raw.hex, Some(hex(&host.world, defender)));
    assert_eq!(
        (landed.raw.light.radius, landed.raw.light.intensity),
        (8, 65536)
    );
    assert!(host.world.queue.events().any(|event| event.object == lit));
    let light = host.world.get(player).unwrap().raw.light;
    assert_eq!((light.radius, light.intensity), (4, 65536));
}

#[test]
fn spec007_projectile_flight_from_the_adjacent_hex_to_the_target() {
    use crate::choreography::{Action, Owner};
    let (mut host, knife, defender, _) = fixture_f(45, 1, 0, 1, 0);
    let player = host.world.player;
    host.world.get_mut(player).unwrap().raw.orientation = 5;
    // The throw art for the player's base and its art's weapon code (0,
    // `as`): 12 frames at 10 fps, action frame 4.
    host.assets
        .art
        .insert(Fid::new(1, 0, 18, 0, 0).0, loaded_art(12, 0, 4));
    throw(&mut host, defender, true);
    // The attacker turns toward the defender (direction 2, straight
    // south) at once; the knife waits hidden on the adjacent hex.
    assert_eq!(host.world.get(player).unwrap().raw.orientation, 2);
    let show = host.world.shows.front().unwrap().clone();
    let projectile = Owner::World(knife);
    let start = show.main.display(projectile).unwrap();
    assert_eq!(
        (start.hex, start.orientation, start.art, start.hidden),
        (Some(20300), 2, PROJECTILE_ART, true)
    );
    let entries = labels(&show.main);
    let rows: Vec<_> = entries
        .iter()
        .filter(|(owner, ..)| *owner == projectile)
        .map(|(_, delay, label)| (*delay, label.as_str()))
        .collect();
    // Unhide on the throw's action frame, flight sound, flight, hit sound
    // N = nodes - 1 steps on, then the knife's art comes back.
    let nodes = show
        .main
        .entries()
        .iter()
        .find_map(|entry| match &entry.action {
            Action::Move { nodes, .. } => Some(nodes.clone()),
            _ => None,
        })
        .unwrap();
    let grid = vault_formats::HexGrid::default();
    let ([ax, ay], [bx, by]) = (
        grid.hex_center(20300).unwrap(),
        grid.hex_center(hex(&host.world, defender)).unwrap(),
    );
    let l = (bx - ax).abs().max((by - ay).abs());
    assert_eq!(nodes.len() as i64, l / 32 + 1);
    assert_eq!(nodes.last().unwrap().0, hex(&host.world, defender));
    let n = nodes.len() as i32 - 1;
    assert_eq!(rows[0], (4, "unhide"));
    assert!(rows[1].1.starts_with("sound WF") && rows[1].0 == 0);
    assert_eq!(rows[2], (0, "move"));
    assert!(rows[3].1.starts_with("sound WH") && rows[3].0 == n);
    assert_eq!(rows.last().unwrap().0, -1);
    // In flight the thrown item is drawn as its projectile only.
    let drawn = |host: &WorldHost<AiAssets>, art: u32| {
        host.world.render_snapshot().objects.elevations[0]
            .iter()
            .filter(|record| record.art_id == art)
            .count()
    };
    let item_art = host.world.get(knife).unwrap().raw.art_id;
    assert_eq!(
        (drawn(&host, PROJECTILE_ART), drawn(&host, item_art)),
        (0, 0)
    );
    host.world.effects.clear();
    for pass in 0..=3 {
        host.step_shows(100 * pass);
    }
    assert_eq!(
        drawn(&host, PROJECTILE_ART),
        0,
        "hidden until the action frame"
    );
    host.step_shows(400);
    assert_eq!(
        (drawn(&host, PROJECTILE_ART), drawn(&host, item_art)),
        (1, 0)
    );
    // The flight and hit sounds start on and after the action frame.
    let early: Vec<Vec<u8>> = host
        .world
        .effects
        .iter()
        .filter_map(|effect| match effect {
            WorldEffect::SoundEffect { name, .. } => Some(name.clone()),
            _ => None,
        })
        .collect();
    let sounds = [early, play(&mut host)].concat();
    assert!(sounds.iter().any(|name| name.starts_with(b"WF")));
    assert!(sounds.iter().any(|name| name.starts_with(b"WH")));
    assert_eq!(
        (drawn(&host, PROJECTILE_ART), drawn(&host, item_art)),
        (0, 1)
    );

    // A miss's hit sound waits no steps.
    let (mut host, knife, defender, _) = fixture_f(45, 1, 0, 1, 0);
    throw(&mut host, defender, false);
    let show = host.world.shows.front().unwrap();
    let hit = labels(&show.main)
        .into_iter()
        .find(|(owner, _, label)| *owner == Owner::World(knife) && label.starts_with("sound WH"))
        .unwrap();
    assert_eq!(hit.1, 0);
}

/// SPEC-007 §3.10 scenes: the player at 20100 and an NPC adjacent to the
/// south (20300), both with critter base 0, combat taunts off, and the
/// player's punch art (11 frames, action frame 3) loadable.
fn scene_pair() -> (WorldHost<AiAssets>, ObjectRef, ObjectRef) {
    let mut world = world();
    let player = world.player;
    let defender = npc(&mut world, 20300, 1, 0, &[]);
    state(&mut world, player).ap = 8;
    let mut host = ai_host(world, AiAssets::default());
    host.world.ai.combat_taunts = Some(0);
    host.assets
        .art
        .insert(Fid::new(1, 0, 16, 0, 0).0, loaded_art(11, 0, 3));
    (host, player, defender)
}
/// A resolved punch against `defender` with defender flags `flags`.
fn punch_scene(
    host: &WorldHost<AiAssets>,
    player: ObjectRef,
    defender: ObjectRef,
    flags: u32,
    knockback: i32,
    damage: i32,
) -> (crate::combat::Attack, crate::Spray) {
    let attack = host
        .world
        .attack_profile(player, defender, hit_mode::PUNCH, location::TORSO, false)
        .unwrap();
    let spray = crate::Spray {
        main: Resolution {
            code: if flags & effect::HIT != 0 { 2 } else { 1 },
            defender_flags: flags,
            damage,
            knockback,
            ..Resolution::default()
        },
        extras: Vec::new(),
    };
    (attack, spray)
}
fn build(
    host: &mut WorldHost<AiAssets>,
    player: ObjectRef,
    defender: ObjectRef,
    attack: &crate::combat::Attack,
    spray: &crate::Spray,
) -> crate::host_choreography::Built {
    let scene = crate::host_choreography::Scene {
        attacker: player,
        defender,
        attack,
        victim: attack,
        spray,
        hit: spray.main.defender_flags & effect::HIT != 0,
        impact: None,
        explosion: None,
    };
    host.build_show(&scene).unwrap()
}
/// A sequence's rows as `(object, delay, label)`.
fn rows(sequence: &crate::choreography::Sequence) -> Vec<(ObjectRef, i32, String)> {
    labels(sequence)
        .into_iter()
        .map(|(owner, delay, label)| match owner {
            crate::choreography::Owner::World(object) => (object, delay, label),
            crate::choreography::Owner::Shown(_) => (0, delay, label),
        })
        .collect()
}
fn row(object: ObjectRef, delay: i32, label: &str) -> (ObjectRef, i32, String) {
    (object, delay, label.to_string())
}

#[test]
fn spec007_melee_contact_and_dodges_follow_the_action_frames() {
    // §3.10.3 a punch hit: the contact sound waits the punch art's action
    // frame (3); the defender, hit from the front, flinches (14) then.
    let (mut host, player, defender) = scene_pair();
    let (attack, hit) = punch_scene(&host, player, defender, effect::HIT, 0, 1);
    let built = build(&mut host, player, defender, &attack, &hit);
    assert_eq!(
        rows(&built.show.main),
        [
            row(player, 0, "turn 2"),
            row(player, 0, "sound "),
            row(player, 0, "animate 16"),
            row(player, 3, "sound "),
            row(defender, 0, "sound "),
            row(defender, 0, "animate 14"),
        ]
    );
    // A miss with dodge art: B = 1 <= A, so the dodge waits A - B = 2.
    host.assets
        .art
        .insert(Fid::new(1, 0, 13, 0, 0).0, loaded_art(8, 0, 1));
    let (attack, miss) = punch_scene(&host, player, defender, 0, 0, 0);
    let built = build(&mut host, player, defender, &attack, &miss);
    assert_eq!(
        rows(&built.show.main)[1..],
        [
            row(player, -1, "sound "),
            row(player, 0, "animate 16"),
            row(defender, 2, "sound "),
            row(defender, 0, "animate 13"),
        ]
    );
    // B = 5 > A: the attack waits B - A = 2 behind the dodge.
    host.assets
        .art
        .insert(Fid::new(1, 0, 13, 0, 0).0, loaded_art(8, 0, 5));
    let built = build(&mut host, player, defender, &attack, &miss);
    assert_eq!(
        rows(&built.show.main)[1..],
        [
            row(defender, -1, "sound "),
            row(defender, 0, "animate 13"),
            row(player, 2, "sound "),
            row(player, 0, "animate 16"),
        ]
    );
    // No dodge art: under the safe rule the attacker still swings.
    host.assets.missing.insert(13);
    let built = build(&mut host, player, defender, &attack, &miss);
    assert_eq!(
        rows(&built.show.main)[1..],
        [row(player, -1, "sound "), row(player, 0, "animate 16")]
    );
}

#[test]
fn spec007_bullets_show_no_projectile_and_no_hit_sound() {
    // §3.10.4 a single shot from a bullet weapon (projectile PID -1, as
    // every corpus bullet weapon): point, the shot, then the reactions on
    // the shot's action frame, with no hit sound, and unpoint.
    let (mut host, [attacker, _, defender, _]) = fixture_e("critter", 2);
    host.world.ai.combat_taunts = Some(0);
    host.assets
        .art
        .insert(Fid::new(1, 0, 45, 5, 0).0, loaded_art(5, 0, 1));
    let gun = host
        .world
        .wielded(attacker, hit_mode::RIGHT_PRIMARY)
        .unwrap();
    host.world.get_mut(attacker).unwrap().raw.art_id = Fid::new(1, 0, 0, 5, 0).0;
    let attack = host
        .world
        .attack_profile(
            attacker,
            defender,
            hit_mode::RIGHT_PRIMARY,
            location::TORSO,
            false,
        )
        .unwrap();
    let spray = crate::Spray {
        main: Resolution {
            code: 2,
            defender_flags: effect::HIT,
            damage: 3,
            ..Resolution::default()
        },
        extras: Vec::new(),
    };
    let built = build(&mut host, attacker, defender, &attack, &spray);
    let labels = rows(&built.show.main);
    assert_eq!(labels[1], row(attacker, -1, "animate 43"));
    assert_eq!(labels[3], row(attacker, 0, "animate 45"));
    assert_eq!(labels[4], row(defender, 1, "sound "), "reaction on A = 1");
    assert_eq!(labels.last().unwrap(), &row(attacker, -1, "animate 44"));
    assert!(
        !labels
            .iter()
            .any(|(_, _, label)| label.starts_with("sound WH")),
        "no hit sound"
    );
    assert!(!labels.iter().any(|(_, _, label)| label == "move"));
    // A miss: the defender dodges on the action frame, with no sound.
    let spray = crate::Spray {
        main: Resolution {
            code: 1,
            ..Resolution::default()
        },
        extras: Vec::new(),
    };
    let built = build(&mut host, attacker, defender, &attack, &spray);
    let labels = rows(&built.show.main);
    assert_eq!(labels[4], row(defender, 1, "animate 13"));
    assert!(host.world.get(gun).is_some());
}

#[test]
fn spec007_reactions_fall_slide_get_up_die_and_bleed() {
    // Knocked down from the front: fall back (20) and stay down.
    let (mut host, player, defender) = scene_pair();
    let (attack, spray) = punch_scene(
        &host,
        player,
        defender,
        effect::HIT | effect::KNOCKED_DOWN,
        0,
        1,
    );
    let built = build(&mut host, player, defender, &attack, &spray);
    assert_eq!(
        rows(&built.show.main)[4..],
        [row(defender, 0, "sound "), row(defender, 0, "animate 20")]
    );
    // Knockback without a knockdown: the slide (20 from the front), then
    // back on its feet (37); the world takes the final hex.
    let (attack, spray) = punch_scene(&host, player, defender, effect::HIT, 2, 1);
    let built = build(&mut host, player, defender, &attack, &spray);
    assert_eq!(
        rows(&built.show.main)[4..],
        [row(defender, 0, "move"), row(defender, -1, "animate 37")]
    );
    let slid = 20300 + 2 * 200;
    assert_eq!(
        built.effects,
        [crate::host_choreography::Effect::Move {
            object: defender,
            hex: slid,
            elevation: 0
        }]
    );
    // A lethal hit under 15 damage: the die sound, the plain fall, blood
    // after it (34 after 20), and the corpse callback.
    let (attack, spray) = punch_scene(&host, player, defender, effect::HIT | effect::DEAD, 0, 10);
    let built = build(&mut host, player, defender, &attack, &spray);
    assert_eq!(
        rows(&built.show.main)[4..],
        [
            row(defender, 0, "sound "),
            row(defender, 0, "animate 20"),
            row(defender, -1, "animate 34"),
            row(defender, -1, "corpse"),
        ]
    );
    // Through a real attack the corpse keeps its blood art once the show
    // is done, flat and nonblocking.
    host.world.rng =
        crate::fixture_items::seed_where(&[(1, 4), (1, 100)], |v| v[0] != 1 && v[1] <= 50);
    set_hp(&mut host.world, defender, 1);
    let result = host
        .perform_attack(player, defender, hit_mode::PUNCH, location::TORSO, false)
        .unwrap()
        .unwrap();
    if result.defender_flags & effect::DEAD != 0 {
        let corpse = host.world.get(defender).unwrap();
        assert_eq!(Fid(corpse.raw.art_id).animation(), 34);
        assert!(corpse.raw.flags.contains(BasicFlags::NONBLOCKING));
    }
    // A blocker behind the defender reverses its fall to the front (21),
    // whose blood is 35.
    let (mut host, player, defender) = scene_pair();
    host.world.get_mut(defender).unwrap().raw.orientation = 5;
    let behind = crate::routing::step(20300, 2);
    npc(&mut host.world, behind, 1, 0, &[]);
    let (attack, spray) = punch_scene(&host, player, defender, effect::HIT | effect::DEAD, 0, 10);
    let built = build(&mut host, player, defender, &attack, &spray);
    let labels = rows(&built.show.main);
    assert!(
        labels.contains(&row(defender, 0, "animate 21")),
        "{labels:?}"
    );
    assert!(labels.contains(&row(defender, -1, "animate 35")));
}

#[test]
fn spec007_fire_dance_runs_then_burns_and_on_fire_returns_to_standing() {
    use vault_formats::pro::{ItemData, PrototypeData};
    let (mut host, player, defender) = scene_pair();
    host.world.violence = Some(3);
    // A flamer: fire damage (2) at maximum violence and 45+ damage picks
    // the fire dance (33).
    let mut flamer = (*weapon(70, 0x06, 4, 60, 10, 0, 100)).clone();
    if let PrototypeData::Item(item) = &mut flamer.data
        && let ItemData::Weapon(gun) = &mut item.data
    {
        gun.damage_type = 2;
        gun.projectile_pid = -1;
    }
    let gun = carry(&mut host.world, player, Arc::new(flamer), 1, loaded(5, 30));
    host.world
        .equip(player, gun, BasicFlags::RIGHT_HAND.0)
        .unwrap();
    let attack = host
        .world
        .attack_profile(
            player,
            defender,
            hit_mode::RIGHT_PRIMARY,
            location::TORSO,
            false,
        )
        .unwrap();
    let spray = crate::Spray {
        main: Resolution {
            code: 2,
            defender_flags: effect::HIT | effect::DEAD,
            damage: 60,
            ..Resolution::default()
        },
        extras: Vec::new(),
    };
    let before = host.world.rng.clone();
    let built = build(&mut host, player, defender, &attack, &spray);
    let labels: Vec<_> = rows(&built.show.main)
        .into_iter()
        .filter(|(object, ..)| *object == defender)
        .map(|(_, delay, label)| (delay, label))
        .collect();
    // The dance in place, a turn and a run (when a path is free), then
    // burned to nothing (29) and the corpse callback; two draws.
    assert_eq!(labels[0], (0, "sound ".to_string()));
    assert_eq!(labels[1], (0, "animate 33".to_string()));
    assert_eq!(labels[labels.len() - 2], (0, "animate 29".to_string()));
    assert_eq!(labels.last().unwrap().1, "corpse");
    let mut expected = before;
    expected.range(2, 5).unwrap();
    expected.range(0, 5).unwrap();
    assert_eq!(host.world.rng, expected);
    if labels.iter().any(|(_, label)| label == "move") {
        assert!(matches!(
            built.effects[0],
            crate::host_choreography::Effect::Move { object, .. } if object == defender
        ));
    }
    // Set on fire and surviving: the dance (33) with no sound, then its
    // standing art.
    let spray = crate::Spray {
        main: Resolution {
            code: 2,
            defender_flags: effect::HIT | effect::ON_FIRE,
            damage: 5,
            ..Resolution::default()
        },
        extras: Vec::new(),
    };
    let built = build(&mut host, player, defender, &attack, &spray);
    let labels: Vec<_> = rows(&built.show.main)
        .into_iter()
        .filter(|(object, ..)| *object == defender)
        .collect();
    assert_eq!(
        labels,
        [row(defender, 0, "animate 33"), row(defender, -1, "art 0")]
    );
}

#[test]
fn spec007_extras_react_when_the_main_sequence_reaches_their_placeholders() {
    // A burst with two critter extras: placeholders before the recorded
    // defender's reaction (the first with the reaction delay), each extra
    // in its own sequence, held until released.
    let (mut host, [attacker, x1, defender, far]) = fixture_e("critter", 2);
    host.world.ai.combat_taunts = Some(0);
    host.playback = crate::host_choreography::Playback::RealTime;
    let attack = host
        .world
        .attack_profile(
            attacker,
            defender,
            hit_mode::RIGHT_PRIMARY,
            location::TORSO,
            false,
        )
        .unwrap();
    let extra = |object| {
        (
            object,
            attack.clone(),
            Resolution {
                code: 2,
                defender_flags: effect::HIT,
                damage: 2,
                ..Resolution::default()
            },
        )
    };
    let spray = crate::Spray {
        main: Resolution {
            code: 2,
            defender_flags: effect::HIT,
            damage: 3,
            ..Resolution::default()
        },
        extras: vec![extra(x1), extra(far)],
    };
    let built = build(&mut host, attacker, defender, &attack, &spray);
    let main = rows(&built.show.main);
    let first = main
        .iter()
        .position(|(_, _, label)| label == "release 0")
        .unwrap();
    assert_eq!(main[first + 1].2, "release 1");
    assert_eq!(main[first + 1].1, 0);
    assert_eq!(main[first + 2].0, defender, "then the defender reacts");
    assert_eq!(main[first + 2].1, 0, "its delay is 0 after placeholders");
    assert_eq!(built.show.extras.len(), 2);
    assert_eq!(rows(&built.show.extras[0])[0].0, x1);
    let mut show = built.show;
    show.pass(0);
    assert!(!show.extras[0].registered(), "held");
    show.run_to_end(0);
    assert!(
        show.extras
            .iter()
            .all(|extra| extra.registered() && extra.finished())
    );
}

#[test]
fn spec007_an_attack_whose_sequence_cannot_register_does_nothing() {
    // §3.10.1: without the punch art the sequence is refused before the AP
    // debit; nothing applies and nothing plays.
    let (mut host, player, defender) = scene_pair();
    host.assets.missing.insert(16);
    host.assets.missing.insert(17);
    let hp = host.world.hit_points(defender);
    let refused = host
        .perform_attack(player, defender, hit_mode::PUNCH, location::TORSO, false)
        .unwrap();
    assert_eq!(refused, Err(crate::Refusal::Sequence));
    assert_eq!(host.world.critter_state(player).unwrap().ap, 8);
    assert_eq!(host.world.hit_points(defender), hp);
    assert!(!host.presenting());
}

#[test]
fn spec007_critical_failure_effects_drop_or_destroy_the_weapon() {
    // A critical failure with the drop bit: the attacker's own reaction
    // (delay -1, from the front), then the weapon drops at its hex.
    let (mut host, [attacker, _, defender, _]) = fixture_e("critter", 2);
    host.world.ai.combat_taunts = Some(0);
    let gun = host
        .world
        .wielded(attacker, hit_mode::RIGHT_PRIMARY)
        .unwrap();
    let attack = host
        .world
        .attack_profile(
            attacker,
            defender,
            hit_mode::RIGHT_PRIMARY,
            location::TORSO,
            false,
        )
        .unwrap();
    let failure = |flags| crate::Spray {
        main: Resolution {
            code: 0,
            attacker_flags: effect::CRITICAL | flags,
            ..Resolution::default()
        },
        extras: Vec::new(),
    };
    let spray = failure(effect::DROP);
    let built = build(&mut host, attacker, defender, &attack, &spray);
    let labels = rows(&built.show.main);
    assert!(labels.contains(&row(attacker, -1, "sound ")));
    assert!(labels.contains(&row(attacker, 0, "animate 14")));
    assert!(labels.contains(&row(gun, -1, "place")));
    host.apply_effects(&built.effects).unwrap();
    let dropped = host.world.get(gun).unwrap();
    assert_eq!(
        (dropped.owner, dropped.raw.hex),
        (None, Some(hex(&host.world, attacker)))
    );
    // The destroy bit erases it.
    let (mut host, [attacker, _, defender, _]) = fixture_e("critter", 2);
    host.world.ai.combat_taunts = Some(0);
    let gun = host
        .world
        .wielded(attacker, hit_mode::RIGHT_PRIMARY)
        .unwrap();
    let attack = host
        .world
        .attack_profile(
            attacker,
            defender,
            hit_mode::RIGHT_PRIMARY,
            location::TORSO,
            false,
        )
        .unwrap();
    let built = build(
        &mut host,
        attacker,
        defender,
        &attack,
        &failure(effect::DESTROY),
    );
    assert!(rows(&built.show.main).contains(&row(gun, -1, "remove")));
    host.apply_effects(&built.effects).unwrap();
    assert!(host.world.get(gun).is_none());
}

#[test]
fn spec007_combat_waits_while_attacks_play() {
    // In real time a queued show holds the combat request pass until it
    // has played.
    let (mut host, player, defender) = scene_pair();
    host.playback = crate::host_choreography::Playback::RealTime;
    host.world.rng = crate::fixture_items::seed_where(&[(1, 4)], |v| v[0] != 1);
    host.perform_attack(player, defender, hit_mode::PUNCH, location::TORSO, false)
        .unwrap()
        .unwrap();
    assert!(host.presenting());
    host.world.pending_combat = Some(CombatRequest {
        attacker: player,
        defender,
        accuracy: 0,
        damage: [0, i32::MAX],
        flags: None,
    });
    host.tick(100).unwrap();
    assert!(host.world.pending_combat.is_some(), "combat waits");
    play(&mut host);
    host.tick(100_000).unwrap();
    assert!(host.world.pending_combat.is_none());
}
