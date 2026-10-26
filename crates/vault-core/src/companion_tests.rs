//! The enhanced companion state's sidecar text and distance modes.
use super::*;

#[test]
fn the_sidecar_round_trips_and_names_bad_lines() {
    let mut distances = BTreeMap::new();
    distances.insert(18000, Distance::Close);
    distances.insert(18004, Distance::Charge);
    distances.insert(18009, Distance::Default);
    let text = write_sidecar(&distances);
    distances.remove(&18009);
    assert_eq!(parse_sidecar(&text).unwrap(), distances);
    assert!(parse_sidecar("").unwrap().is_empty());
    for (text, needle) in [
        ("18000 = \"close\"", "before `[companion_distance]`"),
        ("[companion_distance]\nx = \"close\"", "not an object id"),
        ("[companion_distance]\n18000 = \"near\"", "unknown distance"),
        ("[party]", "unknown table"),
        ("[companion_distance]\n18000", "expected"),
    ] {
        let error = parse_sidecar(text).unwrap_err();
        assert!(error.contains(needle), "{text}: {error}");
    }
}

#[test]
fn the_distance_modes_cycle_and_name_themselves() {
    let mut mode = Distance::default();
    let mut seen = Vec::new();
    for _ in 0..4 {
        seen.push(mode.name());
        assert_eq!(Distance::parse(mode.name()), Some(mode));
        mode = mode.next();
    }
    assert_eq!(seen, ["default", "close", "hold", "charge"]);
    assert_eq!(mode, Distance::Default);
}

use crate::{
    CharacterSheet, WorldState,
    fallout2::{Fallout2Tables, Fallout2World},
    fallout2_party as party,
    test_support::{TestAssets, patched, set_hp, special},
};
use std::sync::Arc;
use vault_formats::{FrmArchive, fallout2::save::MemberRecord};

/// A one-frame FRM, enough for an art existence test.
fn frm() -> Arc<FrmArchive> {
    let mut bytes = Vec::new();
    bytes.extend(4_u32.to_be_bytes());
    bytes.extend(10_u16.to_be_bytes());
    bytes.extend(0_u16.to_be_bytes());
    bytes.extend(1_u16.to_be_bytes());
    bytes.extend([0; 24]);
    for index in 0..6_u32 {
        bytes.extend((index * 13).to_be_bytes());
    }
    bytes.extend((6 * 13_u32).to_be_bytes());
    for _ in 0..6 {
        bytes.extend(1_u16.to_be_bytes());
        bytes.extend(1_u16.to_be_bytes());
        bytes.extend(1_u32.to_be_bytes());
        bytes.extend([0; 4]);
        bytes.push(1);
    }
    Arc::new(FrmArchive::parse(&bytes).unwrap())
}
/// A weapon prototype of `pid` with animation code `code`, or (without a
/// code) a miscellaneous item.
fn item(pid: u32, code: Option<i32>) -> Arc<vault_formats::Prototype> {
    match code {
        Some(code) => patched(0, 3, &[(0, pid as i32), (57, code)], &[]),
        None => patched(0, 5, &[(0, pid as i32)], &[]),
    }
}
/// The enhanced preset with a companion at 20300 (party ID 18000, 30 max
/// HP, 25 current) carrying a knife (code 1), a misc item and a spear
/// (code 4), in that order.
fn companion(assets: TestAssets) -> (WorldHost<TestAssets>, ObjectRef, [ObjectRef; 3]) {
    let world = WorldState::new(CharacterSheet::default(), 1).unwrap();
    let mut host = WorldHost::new(world, assets);
    host.set_preset(EnginePreset::Enhanced);
    let critter = patched(1, 0, &special(&[(7, 30), (8, 8)]), &[]);
    let member = host.world.spawn(critter, 20300, 0).unwrap();
    host.world.get_mut(member).unwrap().raw.id = 18000;
    host.world.get_mut(member).unwrap().name = b"Dogmeat".to_vec();
    set_hp(&mut host.world, member, 25);
    host.world.party.insert(member);
    let items = [(0x10, Some(1)), (0x11, None), (0x12, Some(4))].map(|(pid, code)| {
        let object = host.world.spawn(item(pid, code), 0, 0).unwrap();
        host.world.add_item(member, object, 1, true).unwrap();
        object
    });
    (host, member, items)
}
/// The companion's right-hand item and its art's weapon code.
fn held(host: &WorldHost<TestAssets>, critter: ObjectRef) -> (Option<ObjectRef>, u8) {
    let code = Fid(host.world.get(critter).unwrap().raw.art_id).weapon();
    (host.companion_weapon(critter), code)
}
fn inventory(host: &WorldHost<TestAssets>, owner: ObjectRef) -> Vec<ObjectRef> {
    let object = host.world.get(owner).unwrap();
    object.inventory.iter().map(|entry| entry.0).collect()
}

#[test]
fn wield_cycles_the_weapons_in_inventory_order_then_none() {
    let (mut host, member, [knife, misc, spear]) = companion(TestAssets::all_art());
    assert_eq!(
        Fid(host.world.get(member).unwrap().raw.art_id).category(),
        1
    );
    assert_eq!(host.companion_weapons(member), [knife, spear]);
    assert_eq!(held(&host, member), (None, 0));
    let cycle = |host: &mut WorldHost<TestAssets>| host.cycle_companion_weapon(member).unwrap();
    assert_eq!(cycle(&mut host), Some(Some(knife)));
    assert_eq!(held(&host, member), (Some(knife), 1));
    assert_eq!(cycle(&mut host), Some(Some(spear)));
    assert_eq!(held(&host, member), (Some(spear), 4));
    let right = |host: &WorldHost<TestAssets>, item: ObjectRef| {
        host.world
            .get(item)
            .unwrap()
            .raw
            .flags
            .contains(BasicFlags::RIGHT_HAND)
    };
    assert!(!right(&host, knife) && !right(&host, misc) && right(&host, spear));
    assert_eq!(cycle(&mut host), Some(None));
    assert_eq!(held(&host, member), (None, 0));
    assert!(!right(&host, spear));
    // The entries stay in their places.
    assert_eq!(inventory(&host, member), [knife, misc, spear]);
    assert_eq!(cycle(&mut host), Some(Some(knife)));
    // Refused in combat, under Vanilla and for a stranger.
    host.world.combat = true;
    assert_eq!(cycle(&mut host), None);
    host.world.combat = false;
    host.set_preset(EnginePreset::Vanilla);
    assert_eq!(cycle(&mut host), None);
    assert_eq!(held(&host, member), (Some(knife), 1));
    host.set_preset(EnginePreset::Enhanced);
    host.world.party.remove(&member);
    assert_eq!(cycle(&mut host), None);
}

#[test]
fn the_wield_cycle_wraps_through_none() {
    let (host, _, [knife, misc, spear]) = companion(TestAssets::all_art());
    drop(host);
    let weapons = [knife, spear];
    assert_eq!(next_weapon(&[], None), None);
    assert_eq!(next_weapon(&weapons, None), Some(knife));
    assert_eq!(next_weapon(&weapons, Some(knife)), Some(spear));
    assert_eq!(next_weapon(&weapons, Some(spear)), None);
    // A held item that is not among the weapons (a flare, say).
    assert_eq!(next_weapon(&weapons, Some(misc)), Some(knife));
}

#[test]
fn the_weapon_code_needs_the_critters_art() {
    // Only the companion's standing art with the knife (code 1) exists.
    let (mut host, member, [knife, _, spear]) = companion(TestAssets::default());
    let base = Fid(host.world.get(member).unwrap().raw.art_id).base();
    host.assets.art.insert(Fid::new(1, base, 0, 1, 0).0, frm());
    assert_eq!(host.held_weapon_code(member, 1), 1);
    assert_eq!(host.held_weapon_code(member, 4), 0);
    host.cycle_companion_weapon(member).unwrap();
    assert_eq!(held(&host, member), (Some(knife), 1));
    // The spear is held but drawn unarmed: the base art, code 0.
    host.cycle_companion_weapon(member).unwrap();
    assert_eq!(held(&host, member), (Some(spear), 0));
    assert_eq!(Fid(host.world.get(member).unwrap().raw.art_id).base(), base);
}

#[test]
fn the_gear_window_hides_the_weapon_taken_in_hand() {
    let (mut host, member, [knife, misc, spear]) = companion(TestAssets::all_art());
    let mut session = host.open_companion_loot(member).unwrap().unwrap();
    let pane = |host: &WorldHost<TestAssets>, session: &crate::loot::LootSession| {
        let items = host.loot_items(session, true);
        items.iter().map(|entry| entry.0).collect::<Vec<_>>()
    };
    assert_eq!(pane(&host, &session), [knife, misc, spear]);
    assert_eq!(host.loot_wield(&mut session).unwrap(), Some(Some(knife)));
    assert_eq!(pane(&host, &session), [misc, spear]);
    assert_eq!(host.loot_wield(&mut session).unwrap(), Some(Some(spear)));
    assert_eq!(pane(&host, &session), [knife, misc]);
    assert_eq!(host.loot_wield(&mut session).unwrap(), Some(None));
    assert_eq!(pane(&host, &session), [knife, misc, spear]);
    // An ordinary loot session has no WIELD.
    session.companion = false;
    assert_eq!(host.loot_wield(&mut session).unwrap(), None);
}

/// A Fallout 2 game whose party table's record 0 allows `stay_close`,
/// `charge` and `stay` for `distance`.
fn fallout2(host: &mut WorldHost<TestAssets>) {
    let mut files = vault_fixtures::fallout2::tables();
    for file in &mut files {
        if file.0 == "data/party.txt" {
            file.1 = "[Party Member 0]\nparty_member_pid=16777300\n\
                distance=stay_close, charge, stay\n";
        }
    }
    let mut read = vault_fixtures::fallout2::reader(files);
    let tables = Arc::new(Fallout2Tables::load(&mut read).unwrap());
    host.world.fallout2 = Some(Box::new(Fallout2World::new_game(tables, -1)));
}
/// The companion's control record: party record 0 with `fields`.
fn control(host: &mut WorldHost<TestAssets>, fields: [i32; 7]) {
    let state = host.world.fallout2.as_deref_mut().unwrap();
    let record = MemberRecord {
        record: 0,
        stage: 0,
        fields,
    };
    state.control.insert(18000, record);
}

#[test]
fn the_party_trade_window_wields_and_hides_the_held_weapon_again() {
    let (mut host, member, [knife, misc, spear]) = companion(TestAssets::all_art());
    // Under Fallout 2 the Gear window is not offered; Trade needs a
    // control record.
    fallout2(&mut host);
    assert!(!host.is_companion(member));
    assert!(!host.can_wield_for(member));
    control(&mut host, party::neutral());
    assert!(host.can_wield_for(member));
    // Unarmed: §3.17 hides the first weapon for the session.
    let mut session = host.begin_barter(member).unwrap();
    assert_eq!(inventory(&host, member), [misc, spear]);
    assert_eq!(host.barter_held_weapon(&session), None);
    assert_eq!(host.barter_wield(&mut session).unwrap(), Some(Some(knife)));
    assert_eq!(held(&host, member), (None, 1), "hidden, its code shown");
    assert_eq!(host.barter_held_weapon(&session), Some(knife));
    assert_eq!(inventory(&host, member), [misc, spear]);
    assert_eq!(host.barter_wield(&mut session).unwrap(), Some(Some(spear)));
    assert_eq!(host.barter_held_weapon(&session), Some(spear));
    assert_eq!(inventory(&host, member), [knife, misc]);
    assert_eq!(host.barter_wield(&mut session).unwrap(), Some(None));
    assert_eq!(host.barter_held_weapon(&session), None);
    assert_eq!(inventory(&host, member), [misc, spear]);
    assert_eq!(held(&host, member).1, 0);
    host.barter_wield(&mut session).unwrap();
    host.barter_wield(&mut session).unwrap();
    // Leaving puts the spear back in the right hand.
    host.end_barter(session).unwrap();
    assert_eq!(inventory(&host, member), [knife, misc, spear]);
    assert_eq!(held(&host, member), (Some(spear), 4));
    // Vanilla Fallout 2 trade has no WIELD.
    host.set_preset(EnginePreset::Vanilla);
    let mut session = host.begin_barter(member).unwrap();
    assert_eq!(host.barter_wield(&mut session).unwrap(), None);
    host.end_barter(session).unwrap();
    assert_eq!(held(&host, member), (Some(spear), 4));
}

#[test]
fn party_pills_name_hp_and_mode_per_profile() {
    let (mut host, member, _) = companion(TestAssets::all_art());
    let status = PartyStatus {
        critter: member,
        name: b"Dogmeat".to_vec(),
        hp: 25,
        max_hp: 30,
        mode: Some("DEF"),
    };
    assert_eq!(host.party_status(), [status]);
    for label in ["CLS", "HLD", "CHG", "DEF"] {
        assert!(host.cycle_party_distance(member));
        assert_eq!(host.party_status()[0].mode, Some(label));
    }
    host.world.combat = true;
    assert!(!host.cycle_party_distance(member));
    host.world.combat = false;
    // No pill for the player, a non-critter entry, the hidden or the dead.
    let player = host.world.player;
    host.world.party.insert(player);
    let trunk = host.world.spawn(item(0x13, None), 20302, 0).unwrap();
    host.world.get_mut(trunk).unwrap().raw.id = 18001;
    host.world.party.insert(trunk);
    assert_eq!(host.party_status().len(), 1);
    host.world.get_mut(member).unwrap().raw.flags.0 |= BasicFlags::HIDDEN.0;
    assert!(host.party_status().is_empty());
    host.world.get_mut(member).unwrap().raw.flags.0 &= !BasicFlags::HIDDEN.0;
    host.world.critter_state_mut(member).unwrap().results.0 |= effect::DEAD;
    assert!(host.party_status().is_empty());
    host.world.critter_state_mut(member).unwrap().results.0 &= !effect::DEAD;
    // Nothing under Vanilla.
    host.set_preset(EnginePreset::Vanilla);
    assert!(host.party_status().is_empty());
    assert!(!host.cycle_party_distance(member));
    host.set_preset(EnginePreset::Enhanced);
    // Fallout 2: the control record's `distance`, cycled through the
    // allowed set (stay_close, charge, stay) from `on_your_own`, making
    // the disposition `custom`; without a record there is no mode.
    fallout2(&mut host);
    assert_eq!(host.party_status()[0].mode, None);
    let mut fields = party::neutral();
    fields[0] = party::disposition::AGGRESSIVE;
    control(&mut host, fields);
    assert_eq!(host.party_status()[0].mode, Some("DEF"));
    for label in ["HLD", "CLS", "CHG", "HLD"] {
        assert!(host.cycle_party_distance(member));
        assert_eq!(host.party_status()[0].mode, Some(label));
    }
    let record = host.control_of(member).unwrap();
    assert_eq!(record.fields[0], party::disposition::CUSTOM);
    // The enhanced Distance is a separate value, left alone.
    assert_eq!(host.companion_distance(member), Distance::Default);
    fields[4] = party::distance::SNIPE;
    control(&mut host, fields);
    assert_eq!(host.party_status()[0].mode, Some("SNP"));
}
