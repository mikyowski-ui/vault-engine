//! SPEC-011 fixtures E and I and the inventory/equipment rules: weight,
//! value, ammunition stacks, reloads, unloading, carry/body/container/power
//! limits, stacking conditions, armor swaps, and opening the inventory in
//! combat.
use crate::{
    ObjectRef, Perk, WorldEffect, WorldHost,
    fixture_items::{
        self as support, AMMO_X, ARMOR_A, ARMOR_B, CONTAINER, ItemAssets, MONEY, WEAPON_W, messages,
    },
    items::Refusal,
};
use vault_formats::{
    map::{ObjectDetail, ObjectState},
    pro::BasicFlags,
};

fn set_detail(host: &mut WorldHost<ItemAssets>, item: ObjectRef, detail: ObjectDetail) {
    host.world.get_mut(item).unwrap().raw.state = ObjectState::Other { word: 0, detail };
}
fn rounds(host: &WorldHost<ItemAssets>, item: ObjectRef) -> i32 {
    match &host.world.get(item).unwrap().raw.state {
        ObjectState::Other {
            detail: ObjectDetail::Ammunition(rounds),
            ..
        } => *rounds,
        ObjectState::Other {
            detail: ObjectDetail::Weapon { rounds, .. },
            ..
        } => *rounds,
        _ => panic!("no round count"),
    }
}
fn entries(host: &WorldHost<ItemAssets>, owner: ObjectRef) -> Vec<(ObjectRef, i32)> {
    host.world.get(owner).unwrap().inventory.clone()
}
fn npc(host: &mut WorldHost<ItemAssets>, pid: u32, body: i32) -> ObjectRef {
    let prototype = support::critter(pid, &[], body, &[]);
    host.world.spawn(prototype, 20300, 0).unwrap()
}
fn loaded_w(host: &mut WorldHost<ItemAssets>, rounds: i32) -> ObjectRef {
    let weapon = support::spawn(host, WEAPON_W);
    set_detail(
        host,
        weapon,
        ObjectDetail::Weapon {
            rounds,
            ammo_pid: AMMO_X as i32,
        },
    );
    weapon
}

#[test]
fn spec011_fixture_e_weight_value_stacking_reload_and_unload() {
    let mut host = support::host();
    let owner = npc(&mut host, 0x0100_0201, 0);
    // The ammunition prototype becomes known with its first instance.
    let x = support::carry(&mut host, owner, AMMO_X, 3);
    set_detail(&mut host, x, ObjectDetail::Ammunition(5));
    let w = loaded_w(&mut host, 7);
    assert_eq!(host.world.item_weight(w), 6, "5 + 1 × ((7 - 1) / 20 + 1)");
    assert_eq!(host.world.item_value(w), 317, "300 + 7 × 50 / 20");
    assert_eq!(host.world.inventory_weight(owner), 3, "every pack counts");
    assert_eq!(host.world.entry_value(x, 3), 112, "50 × 2 + 50 × 5 / 20");
    assert_eq!(host.world.inventory_value(owner), 112);

    // Merge R' = 18: 5 + 18 > 20, so the front is 3 and Q becomes 4.
    let added = support::spawn(&mut host, AMMO_X);
    set_detail(&mut host, added, ObjectDetail::Ammunition(18));
    host.world.add_item(owner, added, 1, false).unwrap();
    assert_eq!(entries(&host, owner), [(added, 4)]);
    assert!(
        host.world.get(x).is_none(),
        "the old representative is destroyed"
    );
    assert_eq!(rounds(&host, added), 3);
    assert_eq!(rounds(&host, added) + 3 * 20, 5 + 2 * 20 + 18);
    // A merge that fits in the front pack: 3 + 10 ≤ 20, Q + 1 - 1.
    let small = support::spawn(&mut host, AMMO_X);
    set_detail(&mut host, small, ObjectDetail::Ammunition(10));
    host.world.add_item(owner, small, 1, false).unwrap();
    assert_eq!(entries(&host, owner), [(small, 4)]);
    assert_eq!(rounds(&host, small), 13);
    set_detail(&mut host, small, ObjectDetail::Ammunition(3));

    // Search reload of W (7/12): three rounds move, the empty front unit is
    // destroyed, and the rest keep full packs.
    assert!(host.world.search_reload(owner, w).unwrap());
    assert_eq!(rounds(&host, w), 10);
    let [(front, count)] = entries(&host, owner)[..] else {
        panic!("one ammunition entry")
    };
    assert_eq!((count, rounds(&host, front)), (3, 20));
    // Removing one pack: the partial pack leaves first and keeps its count.
    set_detail(&mut host, front, ObjectDetail::Ammunition(7));
    host.world.remove_item(owner, front, 1).unwrap();
    let [(rest, 2)] = entries(&host, owner)[..] else {
        panic!("two packs remain")
    };
    assert_eq!((rounds(&host, front), rounds(&host, rest)), (7, 20));
    assert_eq!(host.world.get(front).unwrap().raw.hex, Some(1));
    assert!(
        host.world.remove_item(owner, rest, 3).is_err(),
        "too few units"
    );

    // Unload W at 10/12: a new X object holding min(10, 20) rounds.
    let unloaded = host.world.unload_weapon(w).unwrap().unwrap();
    assert_eq!(host.world.get(unloaded).unwrap().raw.pid, AMMO_X);
    assert_eq!((rounds(&host, unloaded), rounds(&host, w)), (10, 0));
    assert!(host.world.unload_weapon(w).unwrap().is_none(), "empty");
    assert_eq!(host.world.item_weight(w), 5);
    assert_eq!(host.world.item_value(w), 300);
}

#[test]
fn spec011_fixture_e_carry_body_container_and_power_limits() {
    let mut host = support::host();
    // A biped with effective carry weight 150 holding 148.
    let carrier = npc(&mut host, 0x0100_0202, 0);
    host.assets
        .prototypes
        .insert(0x600, support::misc(0x600, 1, 148, 0));
    host.assets
        .prototypes
        .insert(0x601, support::misc(0x601, 3, 3, 0));
    host.assets
        .prototypes
        .insert(0x602, support::misc(0x602, 2, 2, 0));
    support::carry(&mut host, carrier, 0x600, 1);
    let heavy = support::spawn(&mut host, 0x601);
    let light = support::spawn(&mut host, 0x602);
    assert_eq!(
        host.world.add_refusal(carrier, heavy, 1),
        Some(Refusal::TooHeavy)
    );
    assert!(
        host.world.add_item(carrier, heavy, 1, false).is_err(),
        "151 > 150"
    );
    assert_eq!(
        host.world.add_refusal(carrier, light, 1),
        None,
        "150 is not > 150"
    );
    assert!(
        host.world.add_item(carrier, heavy, 0, true).is_err(),
        "Q < 1"
    );
    // Forced transfers exceed the limit.
    host.world.add_item(carrier, heavy, 1, true).unwrap();
    // Non-biped critters refuse everything.
    let dog = npc(&mut host, 0x0100_0203, 1);
    assert_eq!(
        host.world.add_refusal(dog, light, 1),
        Some(Refusal::WrongBody)
    );
    // Container of size 10 holding 7: size 3 reaches it (refused), 2 fits.
    let container = support::spawn(&mut host, CONTAINER);
    host.assets
        .prototypes
        .insert(0x603, support::misc(0x603, 7, 0, 0));
    support::carry(&mut host, container, 0x603, 1);
    assert_eq!(
        host.world.add_refusal(container, heavy, 1),
        Some(Refusal::TooBig)
    );
    assert_eq!(host.world.add_refusal(container, light, 1), None);
    // Inside a carried container the outermost carrier's limit applies
    // too: 146 plus the empty bag's 2 leaves room for 2, not 3.
    host.assets
        .prototypes
        .insert(0x605, support::misc(0x605, 1, 146, 0));
    let bag = support::spawn(&mut host, CONTAINER);
    let porter = npc(&mut host, 0x0100_0204, 0);
    support::carry(&mut host, porter, 0x605, 1);
    host.world.add_item(porter, bag, 1, true).unwrap();
    assert_eq!(host.world.item_weight(bag), 2);
    assert_eq!(host.world.add_refusal(bag, light, 1), None);
    assert_eq!(
        host.world.add_refusal(bag, heavy, 1),
        Some(Refusal::TooHeavy)
    );
    // A misc item accepts only its power cell; other items accept nothing.
    host.assets
        .prototypes
        .insert(0x604, support::powered(0x604, 0x602));
    let lamp = support::spawn(&mut host, 0x604);
    assert_eq!(host.world.add_refusal(lamp, light, 1), None);
    assert_eq!(
        host.world.add_refusal(lamp, heavy, 1),
        Some(Refusal::NotAccepted)
    );
    let gun = loaded_w(&mut host, 0);
    assert_eq!(
        host.world.add_refusal(gun, light, 1),
        Some(Refusal::NotAccepted)
    );
    // The player is a biped critter with a carry limit; scenery and other
    // owners never refuse.
    let player = host.world.player;
    host.world.player_sheet_mut().unwrap().base[12] = 0;
    assert_eq!(
        host.world.add_refusal(player, light, 1),
        Some(Refusal::TooHeavy)
    );
    let mut scenery = support::world::prototype(2, 5);
    support::world::word(&mut scenery, 0, 0x0200_0001);
    let shelf = host.world.spawn(support::parse(&scenery), 0, 0).unwrap();
    assert_eq!(host.world.add_refusal(shelf, heavy, 99), None);
}

#[test]
fn spec011_stacking_conditions_and_caps() {
    let mut host = support::host();
    let owner = npc(&mut host, 0x0100_0205, 0);
    // Weapons with different loaded rounds never stack; equal ones do.
    let a = loaded_w(&mut host, 7);
    let b = loaded_w(&mut host, 6);
    let c = loaded_w(&mut host, 7);
    for item in [a, b, c] {
        host.world.add_item(owner, item, 1, true).unwrap();
    }
    assert_eq!(entries(&host, owner), [(c, 2), (b, 1)]);
    // Equipped, used/armed, contents-holding, and container items never
    // stack.
    host.world
        .equip(owner, c, BasicFlags::RIGHT_HAND.0)
        .unwrap();
    let d = loaded_w(&mut host, 7);
    host.world.add_item(owner, d, 1, true).unwrap();
    assert_eq!(entries(&host, owner).len(), 3);
    let primed = support::spawn(&mut host, 0x500);
    host.world.get_mut(primed).unwrap().raw.flags.0 |= BasicFlags::USED.0;
    let plain = support::spawn(&mut host, 0x500);
    assert!(!host.world.stackable(primed, plain));
    let bag = support::spawn(&mut host, CONTAINER);
    let other_bag = support::spawn(&mut host, CONTAINER);
    assert!(!host.world.stackable(bag, other_bag));
    // Removing exactly the quantity deletes the entry; removal clears the
    // equipped flags.
    host.world.remove_item(owner, c, 2).unwrap();
    assert_eq!(host.world.get(c).unwrap().raw.flags.0 & 0x0700_0000, 0);
    assert_eq!(entries(&host, owner).len(), 2);

    // Money: amounts are ignored for stacking; caps count quantities,
    // including money inside containers.
    let wallet = support::carry(&mut host, owner, CONTAINER, 1);
    support::carry(&mut host, wallet, MONEY, 30);
    let coins = support::carry(&mut host, owner, MONEY, 12);
    host.world.get_mut(coins).unwrap().money = Some(5);
    let more = support::spawn(&mut host, MONEY);
    host.world.add_item(owner, more, 8, true).unwrap();
    assert_eq!(host.world.caps(owner), 50);
    assert_eq!(host.world.inventory_value(wallet), 30, "caps at face value");
    assert!(host.world.get(coins).is_none(), "merged into the new money");
}

#[test]
fn spec011_fixture_i_armor_swap_for_the_player_only() {
    let mut host = support::host();
    let player = host.world.player;
    let a = support::carry(&mut host, player, ARMOR_A, 1);
    let b = support::carry(&mut host, player, ARMOR_B, 1);
    let bonus = |host: &WorldHost<ItemAssets>| {
        let sheet = host.world.player_sheet().unwrap();
        [9, 24, 29, 17, 0, 31].map(|stat| sheet.bonus[stat])
    };
    host.wield_object(player, a).unwrap();
    assert_eq!(bonus(&host), [20, 30, 500, 4, 0, 0]);
    // SPEC-005 §3.8: the armor's art follows on the next animation step.
    host.step_animations(0).unwrap();
    assert_eq!(
        host.world.get(player).unwrap().raw.art_id & 0xfff,
        10,
        "male art"
    );
    let carry = host.world.player_stat(12);
    host.wield_object(player, b).unwrap();
    host.step_animations(100).unwrap();
    assert_eq!(bonus(&host), [25, 40, 500, 12, 3, 30]);
    assert_eq!(host.world.player_stat(29), 100, "EMP DR clamps at 100");
    let base = host.world.player_sheet().unwrap().base[24];
    assert_eq!(host.world.player_stat(24), 40 + base);
    assert_eq!(
        host.world.player_stat(12),
        carry + 75,
        "ST +3 recomputes carry"
    );
    assert_eq!(host.world.get(player).unwrap().raw.art_id & 0xfff, 12);
    host.world.remove_item(player, b, 1).unwrap();
    assert_eq!(bonus(&host), [0; 6]);
    // The female art selector; under the SPEC-005 §3.8 safe rule -1 shows
    // the default player art (the reference falls back to art index 1).
    host.world.player_sheet_mut().unwrap().base[34] = 1;
    host.world.default_player_art = 4;
    host.world.add_item(player, b, 1, true).unwrap();
    host.wield_object(player, b).unwrap();
    host.step_animations(200).unwrap();
    assert_eq!(host.world.get(player).unwrap().raw.art_id & 0xfff, 13);
    host.wield_object(player, a).unwrap();
    host.step_animations(300).unwrap();
    assert_eq!(host.world.get(player).unwrap().raw.art_id & 0xfff, 4);

    // NPC: the worn flag only; no AC/DR/DT/perk change.
    let npc = npc(&mut host, 0x0100_0206, 0);
    let worn = support::carry(&mut host, npc, ARMOR_B, 1);
    let before = host.world.get(npc).unwrap().character.clone().unwrap();
    host.wield_object(npc, worn).unwrap();
    let object = host.world.get(npc).unwrap();
    assert_eq!(object.character.as_ref().unwrap().bonus, before.bonus);
    assert!(
        host.world
            .get(worn)
            .unwrap()
            .raw
            .flags
            .contains(BasicFlags::WORN)
    );
}

#[test]
fn spec011_inventory_in_combat_and_drag_reload() {
    let mut host = support::host();
    let player = host.world.player;
    assert!(host.open_inventory(), "free outside combat");
    host.world.combat = true;
    host.world.critter_state_mut(player).unwrap().ap = 3;
    host.world.effects.clear();
    assert!(!host.open_inventory(), "4 AP > 3");
    assert_eq!(messages(&host), ["no time"]);
    host.world
        .player_sheet_mut()
        .unwrap()
        .perks
        .insert(Perk::QuickPockets, 1);
    assert!(host.open_inventory());
    assert_eq!(host.world.critter_state(player).unwrap().ap, 0);

    // Drag reload from a three-pack entry of 5 + 20 + 20 rounds onto an
    // empty 12-round magazine: the first pack empties and is destroyed,
    // the second is left non-empty, and the ready sound plays.
    let weapon = loaded_w(&mut host, 0);
    host.world.add_item(player, weapon, 1, true).unwrap();
    let ammo = support::carry(&mut host, player, AMMO_X, 3);
    set_detail(&mut host, ammo, ObjectDetail::Ammunition(5));
    host.world.effects.clear();
    assert!(host.drag_reload(player, weapon, ammo, 3).unwrap());
    assert_eq!(rounds(&host, weapon), 12);
    let ammo_entry: Vec<_> = entries(&host, player)
        .into_iter()
        .filter(|(item, _)| host.world.get(*item).unwrap().raw.pid == AMMO_X)
        .collect();
    let [(front, 2)] = ammo_entry[..] else {
        panic!("two packs remain")
    };
    assert_eq!(rounds(&host, front), 13);
    assert!(
        host.world.get(weapon).unwrap().owner == Some(player),
        "re-added"
    );
    assert!(
        host.world
            .effects
            .iter()
            .any(|effect| matches!(effect, WorldEffect::WeaponSound { effect: 0, .. }))
    );
}

#[test]
fn spec011_minimum_strength_warns_only_the_player() {
    use crate::critical::{hit_mode, location};
    let mut host = support::host();
    let player = host.world.player;
    let target = npc(&mut host, 0x0100_0207, 0);
    host.world.move_object(target, 20102, 0, false).unwrap();
    let weapon = support::carry(&mut host, player, WEAPON_W, 1);
    host.world
        .equip(player, weapon, BasicFlags::RIGHT_HAND.0)
        .unwrap();
    let attack = |host: &mut WorldHost<ItemAssets>, attacker, defender| {
        host.world.critter_state_mut(attacker).unwrap().ap = 10;
        host.world.effects.clear();
        let outcome = host
            .perform_attack(
                attacker,
                defender,
                hit_mode::RIGHT_PRIMARY,
                location::UNCALLED,
                false,
            )
            .unwrap();
        assert!(outcome.is_ok(), "{outcome:?}");
        messages(host).contains(&"too heavy".to_string())
    };
    assert!(attack(&mut host, player, target), "ST 5 < 6");
    host.world.player_sheet_mut().unwrap().base[0] = 6;
    assert!(!attack(&mut host, player, target), "ST 6 meets it");
    // An NPC attacker is never warned.
    let gun = support::carry(&mut host, target, WEAPON_W, 1);
    host.world
        .equip(target, gun, BasicFlags::RIGHT_HAND.0)
        .unwrap();
    assert!(!attack(&mut host, target, player));
}

#[test]
fn spec014_inventory_slot_moves_split_swap_and_drop() {
    let mut host = support::host();
    let player = host.world.player;
    let worn = |host: &WorldHost<ItemAssets>, item, flag: BasicFlags| {
        host.world.get(item).unwrap().raw.flags.contains(flag)
    };
    let entries = |host: &WorldHost<ItemAssets>| host.world.get(player).unwrap().inventory.clone();
    // From a stack of three, a hand takes one unit as its own entry.
    let stack = support::carry(&mut host, player, AMMO_X, 3);
    let held = host
        .world
        .equip_slot(player, stack, BasicFlags::LEFT_HAND.0)
        .unwrap();
    assert!(worn(&host, held, BasicFlags::LEFT_HAND));
    let mut quantities: Vec<_> = entries(&host).iter().map(|(_, q)| *q).collect();
    quantities.sort();
    assert_eq!(quantities, [1, 2]);
    // The other hand takes it over; the first hand lets go.
    host.world
        .equip_slot(player, held, BasicFlags::RIGHT_HAND.0)
        .unwrap();
    assert!(worn(&host, held, BasicFlags::RIGHT_HAND));
    assert!(!worn(&host, held, BasicFlags::LEFT_HAND));
    // Back to the list: it merges into the stack again.
    host.world.unequip(player, held).unwrap();
    assert_eq!(entries(&host).len(), 1);
    assert_eq!(entries(&host)[0].1, 3);
    // Armor worn through the slot applies its AC; unequipping removes it.
    let armor = support::carry(&mut host, player, ARMOR_A, 1);
    host.world
        .equip_slot(player, armor, BasicFlags::WORN.0)
        .unwrap();
    assert_eq!(host.world.player_sheet().unwrap().bonus[9], 20);
    host.world.unequip(player, armor).unwrap();
    assert_eq!(host.world.player_sheet().unwrap().bonus[9], 0);
    // A container takes items within its limits; a drop lands on the
    // player's hex.
    let bag = support::carry(&mut host, player, CONTAINER, 1);
    let first = entries(&host)[0].0;
    assert!(host.world.move_into(player, first, bag, 1).unwrap());
    assert_eq!(host.world.get(bag).unwrap().inventory.len(), 1);
    host.world.move_object(player, 20100, 0, false).unwrap();
    host.world.drop_item(player, armor, 1).unwrap();
    let dropped = host.world.get(armor).unwrap();
    assert_eq!((dropped.owner, dropped.raw.hex), (None, Some(20100)));
}
