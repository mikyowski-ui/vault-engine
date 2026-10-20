//! SPEC-005 §3.8 the player's base art: the jumpsuit rows and the gender
//! update, armor art through `wield_obj_critter`, `rm_obj_from_inven` and
//! `move_obj_inven_to_obj`, and the inventory window's figure art with the
//! section's worked example.
use crate::{
    ObjectRef, WorldHost,
    fixture_items::{self as support, ItemAssets, WEAPON_W},
    fixture_vm as vm_fixture,
};
use vault_formats::{Fid, pro::BasicFlags};
use vault_vm::{
    IntProgram, Value, Vm,
    extensions::{self, Argument, HostCall},
};

/// Leather Armor (PID 1) with its corpus art fields: male `hmlthr` (12),
/// female `hflthr` (5).
const LEATHER: u32 = 1;
/// A Spear (PID 7), weapon code 4.
const SPEAR: u32 = 7;
/// An armor whose fields are -1 (no corpus armor has one).
const UNDRAWN: u32 = 0x5f0;

/// A player of the given gender after a player-sheet load, over a
/// 59-row critter list with the corpus jumpsuit rows: `hfjmps` 4 and
/// `hmjmps` 11.
fn player_host(female: bool) -> WorldHost<ItemAssets> {
    let mut host = support::host();
    let mut critters = vec!["reserv"; 59];
    critters[4] = "hfjmps";
    critters[11] = "HMJMPS";
    host.assets.critters = critters;
    for (pid, prototype) in [
        (LEATHER, support::armor(LEATHER, 0x0100_000C, 0x0100_0005)),
        (UNDRAWN, support::armor(UNDRAWN, -1, -1)),
        (SPEAR, {
            let mut bytes = support::bytes(WEAPON_W);
            support::world::word(&mut bytes, 0, SPEAR as i32);
            support::world::word(&mut bytes, 0x39, 4);
            support::parse(&bytes)
        }),
    ] {
        host.assets.prototypes.insert(pid, prototype);
    }
    host.world.player_sheet_mut().unwrap().base[34] = i32::from(female);
    host.update_player_gender();
    host
}
fn art(host: &WorldHost<ItemAssets>, object: ObjectRef) -> u32 {
    host.world.get(object).unwrap().raw.art_id
}
/// Run one host opcode with integer operands, in the handler's order.
fn call(host: &mut WorldHost<ItemAssets>, word: u16, operands: &[i32]) {
    let mut vm = Vm::new(IntProgram::parse(&vm_fixture::program()).unwrap());
    let call = HostCall {
        spec: extensions::opcode(word).unwrap(),
        args: operands
            .iter()
            .map(|value| Argument {
                value: Value::integer(*value),
                text: None,
                channel: None,
            })
            .collect(),
    };
    host.game_call(&mut vm, &call).unwrap();
}

#[test]
fn spec005_jumpsuit_rows_and_the_gender_update() {
    // The male jumpsuit row is matched without case; no armor and empty
    // hands give the jumpsuit, standing, weapon code 0.
    let male = player_host(false);
    assert_eq!(male.world.default_player_art, 11);
    assert_eq!(art(&male, male.world.player), 0x0100_000B);
    let female = player_host(true);
    assert_eq!(female.world.default_player_art, 4);
    assert_eq!(art(&female, female.world.player), 0x0100_0004);
    // A list without the jumpsuits leaves row 0.
    let mut bare = support::host();
    bare.world.get_mut(bare.world.player).unwrap().raw.art_id = 0x0100_0007;
    bare.update_player_gender();
    assert_eq!(bare.world.default_player_art, 0);
    assert_eq!(art(&bare, bare.world.player), 0x0100_0000);

    // A hand holding any item keeps the art's weapon code; worn armor
    // leaves the art alone while the default still follows the gender.
    let mut host = player_host(false);
    let player = host.world.player;
    let held = support::carry(&mut host, player, WEAPON_W, 1);
    host.world
        .equip(player, held, BasicFlags::RIGHT_HAND.0)
        .unwrap();
    host.world.get_mut(player).unwrap().raw.art_id = Fid::new(1, 2, 1, 5, 3).0;
    host.update_player_gender();
    assert_eq!(art(&host, player), Fid::new(1, 11, 0, 5, 0).0);
    let armor = support::carry(&mut host, player, LEATHER, 1);
    host.world.equip(player, armor, BasicFlags::WORN.0).unwrap();
    host.world.get_mut(player).unwrap().raw.art_id = 0x0100_000C;
    host.world.player_sheet_mut().unwrap().base[34] = 1;
    host.update_player_gender();
    assert_eq!(host.world.default_player_art, 4);
    assert_eq!(art(&host, player), 0x0100_000C, "armor keeps its art");
}

#[test]
fn spec005_worked_example_the_inventory_window_applies_the_figure_art() {
    for (female, armored, speared) in [
        (true, 0x0100_0005, 0x0100_4004),
        (false, 0x0100_000C, 0x0100_400B),
    ] {
        let mut host = player_host(female);
        let player = host.world.player;
        let armor = support::carry(&mut host, player, LEATHER, 1);
        host.world.equip(player, armor, BasicFlags::WORN.0).unwrap();
        // The art changes only when the window closes.
        let before = art(&host, player);
        assert_eq!(host.figure_art(player, None, 0), Some(armored));
        assert_eq!(art(&host, player), before);
        host.apply_figure_art(None);
        assert_eq!(art(&host, player), armored);
        // The armor goes back to the list and a Spear into the active hand.
        host.world.unequip(player, armor).unwrap();
        let spear = support::carry(&mut host, player, SPEAR, 1);
        host.world
            .equip(player, spear, BasicFlags::RIGHT_HAND.0)
            .unwrap();
        host.apply_figure_art(Some(spear));
        assert_eq!(art(&host, player), speared, "jumpsuit, weapon code 4");
        // A field of -1 shows the default player art.
        let undrawn = support::carry(&mut host, player, UNDRAWN, 1);
        host.world
            .equip(player, undrawn, BasicFlags::WORN.0)
            .unwrap();
        assert_eq!(
            host.figure_art(player, None, 0),
            Some(Fid::new(1, host.world.default_player_art, 0, 0, 0).0)
        );
    }
}

#[test]
fn spec005_wield_obj_critter_registers_the_armor_art() {
    let mut host = player_host(false);
    let player = host.world.player;
    let armor = support::carry(&mut host, player, LEATHER, 1);
    call(&mut host, 0x80da, &[armor as i32, player as i32]);
    // The art change is its own reserved sequence, whose set-art entry
    // takes effect as it registers (SPEC-007 §3.10.1), so nothing is left
    // busy; the protection applies at once too.
    assert_eq!(art(&host, player), 0x0100_000C);
    assert!(!host.animation_busy(player));
    assert_eq!(host.world.player_sheet().unwrap().bonus[9], 20);
    // Swapping armor moves the worn flag; under the safe rule a -1 field
    // shows the default player art (the reference takes base index 1).
    let undrawn = support::carry(&mut host, player, UNDRAWN, 1);
    call(&mut host, 0x80da, &[undrawn as i32, player as i32]);
    assert_eq!(art(&host, player), 0x0100_000B);
    assert!(
        !host
            .world
            .get(armor)
            .unwrap()
            .raw
            .flags
            .contains(BasicFlags::WORN)
    );
    assert_eq!(host.world.get(armor).unwrap().owner, Some(player));
    // Another critter only moves the worn flag.
    let npc = host
        .world
        .spawn(support::critter(0x0100_0201, &[], 0, &[]), 20300, 0)
        .unwrap();
    let worn = support::carry(&mut host, npc, LEATHER, 1);
    let npc_art = art(&host, npc);
    call(&mut host, 0x80da, &[worn as i32, npc as i32]);
    assert_eq!(art(&host, npc), npc_art);
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
fn spec005_scripted_removal_restores_the_jumpsuit() {
    // rm_obj_from_inven on worn armor: the default player art, keeping the
    // animation, weapon code and selector; the item is placed at hex 1.
    let mut host = player_host(false);
    let player = host.world.player;
    let armor = support::carry(&mut host, player, LEATHER, 1);
    host.world.equip(player, armor, BasicFlags::WORN.0).unwrap();
    host.world.get_mut(player).unwrap().raw.art_id = Fid::new(1, 12, 1, 5, 2).0;
    call(&mut host, 0x80d9, &[armor as i32, player as i32]);
    assert_eq!(art(&host, player), Fid::new(1, 11, 1, 5, 2).0);
    let removed = host.world.get(armor).unwrap();
    assert_eq!((removed.owner, removed.raw.hex), (None, Some(1)));
    assert_eq!(host.world.player_sheet().unwrap().bonus[9], 0);
    // Removing an unworn item, or from another owner, leaves the art.
    let spear = support::carry(&mut host, player, SPEAR, 1);
    host.world.get_mut(player).unwrap().raw.art_id = 0x0100_400C;
    call(&mut host, 0x80d9, &[spear as i32, player as i32]);
    assert_eq!(art(&host, player), 0x0100_400C);

    // move_obj_inven_to_obj from the player: everything moves, the armor's
    // effects come off and the gender update shows the jumpsuit with
    // weapon code 0.
    let mut host = player_host(true);
    let player = host.world.player;
    let armor = support::carry(&mut host, player, LEATHER, 1);
    call(&mut host, 0x80da, &[armor as i32, player as i32]);
    let spear = support::carry(&mut host, player, SPEAR, 1);
    host.world
        .equip(player, spear, BasicFlags::RIGHT_HAND.0)
        .unwrap();
    host.world.get_mut(player).unwrap().raw.art_id = 0x0100_4005;
    let chest = support::spawn(&mut host, support::CONTAINER);
    call(&mut host, 0x8147, &[chest as i32, player as i32]);
    assert_eq!(art(&host, player), 0x0100_0004);
    assert!(host.world.get(player).unwrap().inventory.is_empty());
    assert_eq!(host.world.get(chest).unwrap().inventory.len(), 2);
    assert_eq!(host.world.player_sheet().unwrap().bonus[9], 0);
}
