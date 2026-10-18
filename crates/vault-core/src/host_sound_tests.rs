//! SPEC-010 world integration: names from live prototypes (item 0x38,
//! weapon 0x79, scenery 0x28 sound bytes, target materials, art stems), the
//! lookup chain over the asset namespace, speech paths, and volume.
use crate::{
    CharacterSheet, ObjectRef, WorldError, WorldHost, WorldState,
    assets::WorldAssets,
    dialogue::{HeadDefinition, Mood, TalkingHead},
    fixture_world as fixture,
};
use std::{
    collections::{BTreeMap, BTreeSet},
    sync::Arc,
};
use vault_formats::{Fid, Prototype};

#[derive(Default)]
struct SoundAssets {
    /// `(category, art index)` to list token.
    tokens: BTreeMap<(u8, u16), Vec<u8>>,
    /// Effect files present, as upper-case stems.
    effects: BTreeSet<Vec<u8>>,
}
impl WorldAssets for SoundAssets {
    fn prototype(&self, _pid: u32) -> Result<Arc<Prototype>, WorldError> {
        Err(WorldError::Invalid("no prototypes".into()))
    }
    fn program(&self, _index: i32) -> Result<(String, vault_vm::IntProgram), WorldError> {
        Err(WorldError::Invalid("no programs".into()))
    }
    fn message(&self, _list: i32, _id: i32) -> Option<vault_formats::msg::Message> {
        None
    }
    fn art_token(&self, category: u8, index: u16) -> Option<Vec<u8>> {
        self.tokens.get(&(category, index)).cloned()
    }
    fn available(&self, path: &[u8]) -> bool {
        let stem = path
            .strip_prefix(b"sound/sfx/")
            .and_then(|name| name.strip_suffix(b".ACM"));
        stem.is_some_and(|stem| self.effects.contains(&stem.to_ascii_uppercase()))
    }
}

/// A prototype from the fixture template with words and bytes patched.
fn prototype(
    category: u8,
    subtype: usize,
    words: &[(usize, i32)],
    bytes: &[(usize, u8)],
) -> Arc<Prototype> {
    let mut data = fixture::prototype(category, subtype);
    fixture::word(&mut data, 0, ((category as i32) << 24) | 7);
    for (offset, value) in words {
        fixture::word(&mut data, *offset, *value);
    }
    for (offset, value) in bytes {
        data[*offset] = *value;
    }
    Arc::new(Prototype::parse(&data).unwrap())
}
fn place(world: &mut WorldState, prototype: Arc<Prototype>, hex: i32) -> ObjectRef {
    world.spawn(prototype, hex, 0).unwrap()
}

#[test]
fn spec010_prototype_sound_bytes_feed_the_name_builders() {
    let mut world = WorldState::new(CharacterSheet::default(), 1).unwrap();
    // Container: item sound byte `B` at 0x38. Door: scenery byte `C` at
    // 0x28. Weapon: sound byte `E` at 0x79, damage type at payload 0x0C.
    let container = place(&mut world, prototype(0, 1, &[], &[(0x38, b'B')]), 20300);
    let plain = place(&mut world, prototype(0, 1, &[], &[(0x38, 0)]), 20302);
    let door = place(&mut world, prototype(2, 0, &[], &[(0x28, b'C')]), 20304);
    let weapon = place(
        &mut world,
        prototype(0, 3, &[(0x45, 0)], &[(0x79, b'E')]),
        20306,
    );
    let blaster = place(
        &mut world,
        prototype(0, 3, &[(0x45, 6)], &[(0x79, b'H')]),
        20308,
    );
    // A metal (material 1) item target, and a critter target.
    let crate_ = place(&mut world, prototype(0, 5, &[(0x24, 1)], &[]), 20310);
    let wood = place(&mut world, prototype(3, 0, &[(0x20, 3)], &[]), 20312);
    let critter = place(&mut world, prototype(1, 0, &[], &[]), 20314);
    let mut host = WorldHost::new(world, SoundAssets::default());
    let name = |name: Option<Vec<u8>>| String::from_utf8(name.unwrap()).unwrap();

    assert_eq!(name(host.open_sound_name(container, 0)), "IOCNTNRB");
    assert_eq!(name(host.open_sound_name(container, 3)), "INCNTNRB");
    assert_eq!(name(host.open_sound_name(plain, 0)), "IOCNTNR");
    assert_eq!(name(host.open_sound_name(door, 1)), "SCDOORSC");
    assert_eq!(host.open_sound_name(door, 5), None);
    // A scenery object without its prototype uses `A`; a critter is
    // treated as an item with no item sound byte.
    host.world.get_mut(door).unwrap().prototype = None;
    assert_eq!(name(host.open_sound_name(door, 0)), "SODOORSA");
    assert_eq!(name(host.open_sound_name(critter, 0)), "IOCNTNR");

    assert_eq!(
        name(host.weapon_sound_name(4, weapon, 2, crate_)),
        "WHE1MXX1"
    );
    assert_eq!(
        name(host.weapon_sound_name(4, weapon, 5, crate_)),
        "WHE2MXX1"
    );
    assert_eq!(name(host.weapon_sound_name(4, weapon, 2, wood)), "WHE1WXX1");
    assert_eq!(
        name(host.weapon_sound_name(4, blaster, 0, critter)),
        "WHH1XXX1"
    );
    assert_eq!(
        name(host.weapon_sound_name(4, weapon, 0, critter)),
        "WHE1FXX1"
    );
    assert_eq!(name(host.weapon_sound_name(4, weapon, 2, 0)), "WHE1XXX1");
    assert_eq!(
        name(host.weapon_sound_name(1, weapon, 3, crate_)),
        "WAE2XXX1"
    );
    assert_eq!(name(host.weapon_sound_name(0, weapon, 3, 0)), "WRE1XXX1");
    assert_eq!(name(host.weapon_sound_name(1, 0, 2, 0)), "WA", "no weapon");
    assert_eq!(host.weapon_sound_name(9, weapon, 2, 0), None);
}

#[test]
fn spec010_character_names_lookup_speech_and_volume() {
    let mut world = WorldState::new(CharacterSheet::default(), 1).unwrap();
    let player = world.player;
    // A female critter whose art index 1 lists `hmjmps`.
    let critter = place(
        &mut world,
        prototype(1, 0, &[(48 + 4 * 34, 1)], &[]),
        20100 + 6 * 200,
    );
    world.get_mut(critter).unwrap().raw.art_id = Fid::new(1, 1, 0, 0, 0).0;
    let mut assets = SoundAssets::default();
    assets.tokens.insert((1, 1), b"hmjmps".to_vec());
    for stem in ["HFXXXXAA", "HMXXXXAN", "MAMTNTAN", "HMJMPSZQ"] {
        assets.effects.insert(stem.as_bytes().to_vec());
    }
    let mut host = WorldHost::new(world, assets);
    let name = |name: Option<Vec<u8>>| String::from_utf8(name.unwrap()).unwrap();
    assert_eq!(name(host.character_sound_name(critter, 16, 4)), "HMJMPSZQ");
    assert_eq!(name(host.character_sound_name(critter, 38, 5)), "HMJMPSHC");
    assert_eq!(host.character_sound_name(critter, 39, 0), None);
    host.world.get_mut(critter).unwrap().raw.art_id = Fid::new(1, 2, 0, 0, 0).0;
    assert_eq!(
        host.character_sound_name(critter, 16, 4),
        None,
        "beyond the list"
    );

    // The gender alias needs a critter owner; direct playback has none.
    let file = |host: &WorldHost<SoundAssets>, name: &[u8], owner| host.effect_file(name, owner);
    assert_eq!(
        file(&host, b"HAPOWRAA", Some(critter)).as_deref(),
        Some("sound/sfx/HFXXXXAA.ACM")
    );
    assert_eq!(file(&host, b"HAPOWRAA", None), None);
    assert_eq!(
        file(&host, b"HFCMBTAN", Some(critter)).as_deref(),
        Some("sound/sfx/HMXXXXAN.ACM")
    );
    assert_eq!(
        file(&host, b"MALIEUAN", None).as_deref(),
        Some("sound/sfx/MAMTNTAN.ACM")
    );
    assert_eq!(
        file(&host, b"hmjmpszq", None).as_deref(),
        Some("sound/sfx/hmjmpszq.ACM"),
        "case-insensitive file match"
    );

    // Speech: the active head's stem and the message sound field.
    assert_eq!(host.speech_file(b"vic_12"), None, "no talking head");
    let definition = HeadDefinition {
        index: 0,
        stem: "aradesh".into(),
        counts: [1; 3],
    };
    host.world.dialogue.head =
        Some(TalkingHead::new(definition, Mood::Neutral, 0, &mut host.world.rng).unwrap());
    assert_eq!(
        host.speech_file(b"ara_1001.lip").as_deref(),
        Some("sound/speech/aradesh/ara_1001.ACM")
    );

    // Volume: Perception 5 at distance 6 is 28399; a carried item uses its
    // owner; on screen is full; other categories and no object are full.
    assert_eq!(host.world.effect_volume(Some(critter), false), 28399);
    assert_eq!(host.world.effect_volume(Some(critter), true), 32767);
    let held = place(&mut host.world, prototype(0, 5, &[], &[]), 0);
    host.world.add_item(critter, held, 1, true).unwrap();
    assert_eq!(host.world.effect_volume(Some(held), false), 28399);
    let wall = place(&mut host.world, prototype(3, 0, &[], &[]), 39000);
    assert_eq!(host.world.effect_volume(Some(wall), false), 32767);
    assert_eq!(host.world.effect_volume(None, false), 32767);
    host.world.get_mut(critter).unwrap().raw.hex = Some(20100 + 9 * 200);
    assert_eq!(host.world.effect_volume(Some(held), false), 15292);
    assert_eq!(host.world.effect_volume(Some(player), false), 32767);
}

fn effects(host: &mut WorldHost<SoundAssets>) -> Vec<(String, Option<ObjectRef>)> {
    host.world
        .effects
        .drain(..)
        .filter_map(|effect| match effect {
            crate::WorldEffect::SoundEffect { name, owner } => {
                Some((String::from_utf8(name).unwrap(), owner))
            }
            _ => None,
        })
        .collect()
}

#[test]
fn spec010_cues_for_doors_containers_attacks_and_movement() {
    use vault_formats::{
        map::{ObjectDetail, ObjectState},
        pro::LockFlags,
    };
    let mut world = WorldState::new(CharacterSheet::default(), 1).unwrap();
    let player = world.player;
    // A usable door (extended flag 0x800) west of the player, and a
    // container north-east of it.
    let door = place(
        &mut world,
        prototype(2, 0, &[(0x18, 0x800)], &[(0x28, b'C')]),
        20101,
    );
    world.get_mut(door).unwrap().raw.state = ObjectState::Other {
        word: 0,
        detail: ObjectDetail::Door(LockFlags(0)),
    };
    let chest = place(&mut world, prototype(0, 1, &[], &[(0x38, b'B')]), 20099);
    // A melee club (swing, sound byte `E`) and an adjacent critter.
    let club = place(
        &mut world,
        prototype(
            0,
            3,
            &[(0x18, 0x03), (61, 1), (65, 2), (73, 1), (89, 3), (101, -1)],
            &[(0x79, b'E')],
        ),
        0,
    );
    world.add_item(player, club, 1, true).unwrap();
    let critter = place(&mut world, prototype(1, 0, &[(48 + 4 * 7, 50)], &[]), 20300);
    let far = place(&mut world, prototype(0, 5, &[], &[]), 20900);
    if let Some(ObjectState::Critter(state)) = world.get_mut(player).map(|o| &mut o.raw.state) {
        state.ap = 20;
    }
    let mut assets = SoundAssets::default();
    assets.tokens.insert((1, 0), b"hmjmps".to_vec());
    let mut host = WorldHost::new(world, assets);

    // Use opens, then closes, the door; using the container opens it.
    host.action(player, door, crate::ObjectAction::Use).unwrap();
    host.action(player, door, crate::ObjectAction::Use).unwrap();
    host.action(player, chest, crate::ObjectAction::Use)
        .unwrap();
    assert_eq!(
        effects(&mut host),
        [
            ("SODOORSC".to_string(), Some(door)),
            ("SCDOORSC".to_string(), Some(door)),
            ("IOCNTNRB".to_string(), Some(chest)),
        ]
    );
    // Walking toward a distant object requests the walk effect.
    host.action(player, far, crate::ObjectAction::Pickup)
        .unwrap();
    assert_eq!(effects(&mut host), [("HMJMPSAB".to_string(), Some(player))]);

    // Unarmed: the punch effect, and its contact effect on a hit.
    let mode = crate::critical::hit_mode::PUNCH;
    let result = host
        .perform_attack(
            player,
            critter,
            mode,
            crate::critical::location::TORSO,
            false,
        )
        .unwrap()
        .unwrap();
    let hit = result.defender_flags & crate::critical::effect::HIT != 0;
    let mut expected = vec![("HMJMPSAQ".to_string(), Some(player))];
    if hit {
        expected.push(("HMJMPSZQ".to_string(), Some(player)));
    }
    assert_eq!(effects(&mut host), expected);
    // Armed: the weapon's attack effect, then its hit effect on the critter
    // target (no material: `F`), owned by the target.
    host.world
        .equip(player, club, vault_formats::pro::BasicFlags::RIGHT_HAND.0)
        .unwrap();
    let mode = crate::critical::hit_mode::RIGHT_PRIMARY;
    let result = host
        .perform_attack(
            player,
            critter,
            mode,
            crate::critical::location::TORSO,
            false,
        )
        .unwrap()
        .unwrap();
    let hit = result.defender_flags & crate::critical::effect::HIT != 0;
    let mut expected = vec![("WAE1XXX1".to_string(), Some(player))];
    if hit {
        expected.push(("WHE1FXX1".to_string(), Some(critter)));
    }
    assert_eq!(effects(&mut host), expected);
}
