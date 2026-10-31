//! SPEC-024b §4.11: each row Revision 3 promoted, under a script, on its
//! synthetic stack vectors (§7.1) and its rules.
use crate::{
    WorldEffect, WorldHost,
    fixture_items::{self as support, ItemAssets, WEAPON_W},
    sfall::{UNIQUE_IDS, encounter_flag},
    spec024_tests::{I, Op, S, W, diagnostics, run, sfall_host},
    test_support::{TestAssets, npc_prototype},
};
use std::sync::Arc;
use vault_formats::msg::MsgCatalog;
use vault_vm::{RunOutcome, VmStatus};

const LIST_LEN: u16 = 0x8231;
const LIST_GET: u16 = 0x822f;

fn list_of(host: &mut WorldHost<TestAssets>, id: i32) -> Vec<i32> {
    let length = run(host, &[I(id), W(LIST_LEN)]).ints()[0];
    (0..length)
        .map(|index| run(host, &[I(id), I(index), W(LIST_GET)]).ints()[0])
        .collect()
}
fn messages(text: &str) -> Arc<MsgCatalog> {
    Arc::new(MsgCatalog::parse(text.as_bytes()).unwrap())
}

#[test]
fn explosive_damage_names_four_pids_and_any_other_weapon_reads_its_prototype() {
    let (mut host, _) = sfall_host();
    for (pid, expected) in [
        (51, [30, 50]),
        (206, [30, 50]),
        (85, [40, 80]),
        (209, [40, 80]),
    ] {
        let id = run(&mut host, &[I(6), I(pid), I(0), W(0x8261)]).ints()[0];
        assert_eq!(list_of(&mut host, id), expected, "PID {pid}");
    }
    let weapon = support::parse(&support::bytes(WEAPON_W));
    let pid = weapon.pid;
    host.assets.prototypes.insert(pid, weapon.clone());
    let id = run(&mut host, &[I(6), I(pid as i32), I(0), W(0x8261)]).ints()[0];
    let vault_formats::pro::PrototypeData::Item(item) = &weapon.data else {
        panic!("a weapon fixture");
    };
    let vault_formats::pro::ItemData::Weapon(data) = &item.data else {
        panic!("a weapon fixture");
    };
    assert_eq!(list_of(&mut host, id), data.damage);
    // S16: 3 pops, 1 push, and a PID with no weapon gives [0, 0].
    let done = run(&mut host, &[I(6), I(41), I(0), W(0x8261)]);
    assert_eq!(done.ints().len(), 1);
    assert_eq!(list_of(&mut host, done.ints()[0]), [0, 0]);
    // S17: sub-function 2 is still a stub, and an unknown one gives 0.
    assert_eq!(run(&mut host, &[I(2), I(0), I(0), W(0x8261)]).ints(), [0]);
    diagnostics(&mut host);
    assert_eq!(run(&mut host, &[I(99), I(0), I(0), W(0x8261)]).ints(), [0]);
    assert!(
        diagnostics(&mut host)
            .iter()
            .any(|line| line.contains("no sub-function 99"))
    );
    host.sfall_end_of_step();
    assert_eq!(run(&mut host, &[I(id), W(LIST_LEN)]).ints(), [-1]);
}

#[test]
fn party_member_list_has_two_modes_and_the_player_comes_first() {
    let (mut host, me) = sfall_host();
    let hidden = host.world.spawn(npc_prototype(), 20001, 0).unwrap();
    let dead = host.world.spawn(npc_prototype(), 20002, 0).unwrap();
    let player = host.world.player;
    host.world.party.extend([me, hidden, dead]);
    host.world.get_mut(hidden).unwrap().raw.flags.0 |= 0x01;
    if let vault_formats::map::ObjectState::Critter(state) =
        &mut host.world.get_mut(dead).unwrap().raw.state
    {
        state.results.0 |= crate::critical::effect::DEAD;
    }
    let id = |value: u32| value as i32;
    let all = run(&mut host, &[I(1), W(0x8271)]).ints()[0];
    assert_eq!(
        list_of(&mut host, all),
        [id(player), id(me), id(hidden), id(dead)]
    );
    // Any value but 0 acts as 1.
    let other = run(&mut host, &[I(7), W(0x8271)]).ints()[0];
    assert_eq!(list_of(&mut host, other).len(), 4);
    let shown = run(&mut host, &[I(0), W(0x8271)]).ints()[0];
    let shown = list_of(&mut host, shown);
    assert_eq!(*shown.last().unwrap(), id(me));
    assert!(!shown.contains(&id(hidden)) && !shown.contains(&id(dead)));
}

/// An item host with the mode's world state in place: the test fixture's
/// profile is Fallout 1, and stacking reads only that state.
fn item_host() -> WorldHost<ItemAssets> {
    let mut host = support::host();
    host.sfall = Some(Box::default());
    host.world.sfall = Some(Box::default());
    host
}

#[test]
fn a_unique_id_is_kept_released_and_never_reused_and_it_stops_stacking() {
    let mut host = item_host();
    let a = support::spawn(&mut host, 0x500);
    let b = support::spawn(&mut host, 0x500);
    assert!(host.world.stackable(a, b));
    let first = *UNIQUE_IDS.start() as i32;
    assert_eq!(host.set_unique_id(a, 0), first);
    assert_eq!(host.set_unique_id(a, 0), first, "an assigned ID is kept");
    assert!(!host.world.stackable(a, b), "§4.11 stacking");
    assert_eq!(host.set_unique_id(0, 0), 0, "S14: a null object");
    // A flag other than -1 is ignored.
    assert_eq!(host.set_unique_id(a, 5), first);
    let released = host.set_unique_id(a, -1);
    assert!(!UNIQUE_IDS.contains(&(released as u32)));
    assert!(host.world.stackable(a, b), "it stacks again");
    assert_eq!(host.set_unique_id(b, -1), host.object_id_for_test(b));
    assert_eq!(host.set_unique_id(a, 0), first + 1, "not reused");
    // At the range's end, nothing is assigned.
    host.world.sfall.as_deref_mut().unwrap().next_unique = *UNIQUE_IDS.end() + 1;
    let plain = host.object_id_for_test(b);
    assert_eq!(host.set_unique_id(b, 0), plain);
    assert!(host.world.effects.iter().any(|effect| matches!(
        effect,
        WorldEffect::Diagnostic(text) if text.contains("used up")
    )));
}

#[test]
fn message_str_game_reads_each_id_range_and_says_error_otherwise() {
    let (mut host, _) = sfall_host();
    host.assets
        .game
        .insert("combatai.msg".into(), messages("{5}{}{combat ai}\n"));
    host.assets
        .game
        .insert("editor.msg".into(), messages("{5}{}{editor}\n"));
    host.assets
        .game
        .insert("pro_misc.msg".into(), messages("{5}{}{misc proto}\n"));
    host.assets
        .game
        .insert("first.msg".into(), messages("{5}{}{first}\n"));
    host.assets
        .game
        .insert("third.msg".into(), messages("{5}{}{third}\n"));
    host.assets
        .game
        .insert("next.msg".into(), messages("{5}{}{next}\n"));
    host.assets.resources.insert(
        "ddraw.ini".into(),
        b"[Misc]\nExtraGameMsgFileList=first,third:2,next\n".to_vec(),
    );
    for (file, expected) in [
        (1, &b"combat ai"[..]),
        (20, b"editor"),
        (0x1005, b"misc proto"),
        (0x2000, b"first"),
        (0x2002, b"third"),
        (0x2003, b"next"),
    ] {
        assert_eq!(
            run(&mut host, &[I(file), I(5), W(0x826b)]).text(),
            expected,
            "file {file:#x}"
        );
    }
    for (file, number) in [(1, 6), (1, 0), (1, -1), (0x2001, 5), (21, 5), (0x3000, 5)] {
        assert_eq!(
            run(&mut host, &[I(file), I(number), W(0x826b)]).text(),
            b"Error",
            "file {file:#x}, message {number}"
        );
    }
}

#[test]
fn add_extra_msg_file_falls_back_to_english_and_clears_at_a_load() {
    let (mut host, _) = sfall_host();
    host.assets
        .game
        .insert("a.msg".into(), messages("{5}{}{local}\n"));
    host.assets.resources.insert(
        "text/english/game/b.msg".into(),
        b"{5}{}{english}\n".to_vec(),
    );
    let add = |host: &mut WorldHost<TestAssets>, name: &'static [u8]| {
        run(host, &[S(b"add_extra_msg_file"), S(name), W(0x8277)]).ints()[0]
    };
    assert_eq!(add(&mut host, b"a.msg"), 0x3000);
    assert_eq!(add(&mut host, b"b.msg"), 0x3001);
    assert_eq!(add(&mut host, b"A.MSG"), 0x3000);
    assert_eq!(add(&mut host, b"none.msg"), -1);
    assert_eq!(
        run(&mut host, &[I(0x3001), I(5), W(0x826b)]).text(),
        b"english"
    );
    // The table's end: an ID past 0x3FFF gives -1 and uses none.
    for index in 2..=0xfff {
        let name: &'static str = Box::leak(format!("m{index}.msg").into_boxed_str());
        host.assets.game.insert(name.into(), messages("{1}{}{x}\n"));
        assert_eq!(add(&mut host, name.as_bytes()), 0x3000 + index);
    }
    host.assets
        .game
        .insert("last.msg".into(), messages("{1}{}{x}\n"));
    assert_eq!(add(&mut host, b"last.msg"), -1);
    host.sfall_restored();
    assert_eq!(
        run(&mut host, &[I(0x3000), I(5), W(0x826b)]).text(),
        b"Error"
    );
}

#[test]
fn get_terrain_name_reads_worldmap_msg_at_a_point_or_the_party() {
    let (mut host, _) = sfall_host();
    host.assets
        .game
        .insert("worldmap.msg".into(), messages("{1000}{}{Desert}\n"));
    let state = host.world.fallout2.as_deref_mut().unwrap();
    state.position = [10, 10];
    assert_eq!(
        run(&mut host, &[S(b"get_terrain_name"), W(0x8276)]).text(),
        b"Desert"
    );
    assert_eq!(
        run(
            &mut host,
            &[S(b"get_terrain_name"), I(10), I(10), W(0x8278)]
        )
        .text(),
        b"Desert"
    );
    assert_eq!(
        run(
            &mut host,
            &[S(b"get_terrain_name"), I(-1), I(10), W(0x8278)]
        )
        .text(),
        b"",
        "outside the world"
    );
}

#[test]
fn set_car_current_town_writes_the_area_with_or_without_a_car() {
    let (mut host, _) = sfall_host();
    run(&mut host, &[I(3), W(0x81b6)]);
    let car = |host: &WorldHost<TestAssets>| host.world.fallout2.as_deref().unwrap().car;
    assert!(!car(&host).has_car);
    assert_eq!(car(&host).area, 3);
    run(&mut host, &[I(4000), W(0x81b6)]);
    assert_eq!(car(&host).area, 3);
    assert!(
        diagnostics(&mut host)
            .iter()
            .any(|line| line.contains("no area"))
    );
}

#[test]
fn a_forced_encounter_is_stored_replaced_and_locked() {
    let (mut host, _) = sfall_host();
    let pending = |host: &WorldHost<TestAssets>| {
        let state = host.world.sfall.as_deref().unwrap();
        (state.forced_map, state.forced_flags)
    };
    assert_eq!(pending(&host), (-1, 0));
    // S19: flags on top. Map 0 is valid.
    assert!(run(&mut host, &[I(2), I(16), W(0x8229)]).ints().is_empty());
    assert_eq!(pending(&host), (2, 16));
    run(&mut host, &[I(0), W(0x8171)]);
    assert_eq!(pending(&host), (0, 0), "a later call replaces it");
    run(&mut host, &[I(9999), W(0x8171)]);
    assert_eq!(pending(&host), (0, 0), "a map with no row changes nothing");
    assert!(
        diagnostics(&mut host)
            .iter()
            .any(|line| line.contains("no row"))
    );
    run(&mut host, &[I(3), I(2), W(0x8229)]);
    run(&mut host, &[I(4), W(0x8171)]);
    assert_eq!(
        pending(&host),
        (3, 2),
        "ENCOUNTER_FLAG_LOCK ignores the call"
    );
    assert_eq!(encounter_flag::LOCK, 2);
    let selected = host.fallout2_check_encounter().unwrap().unwrap();
    assert_eq!((selected.map, selected.forced), (3, Some(2)));
    assert_eq!(pending(&host), (-1, 0), "the encounter occurred");
    run(&mut host, &[I(4), W(0x8171)]);
    assert_eq!(pending(&host), (4, 0), "the lock ended with it");
}

#[test]
fn virtual_file_words_copy_seek_read_and_write_big_endian() {
    let (mut host, _) = sfall_host();
    host.assets
        .resources
        .insert("art/x.frm".into(), vec![0, 0, 0, 4, 0, 10, 0, 0, 0, 25]);
    // S15: an absent source gives -1, in 2 pops and 1 push.
    assert_eq!(
        run(
            &mut host,
            &[S(b"art/none.frm"), S(b"art/none.frm"), W(0x81f8)]
        )
        .ints(),
        [-1]
    );
    let path = br"art\x.frm";
    let handle = run(&mut host, &[S(path), S(path), W(0x81f8)]).ints()[0];
    assert_eq!(handle, 1, "handles start at 1");
    run(&mut host, &[I(handle), I(8), W(0x8202)]);
    assert_eq!(run(&mut host, &[I(handle), W(0x820a)]).ints(), [25]);
    run(&mut host, &[I(handle), I(4), W(0x8202)]);
    run(&mut host, &[I(handle), I(-2), W(0x81fb)]);
    run(&mut host, &[I(handle), I(4), W(0x8202)]);
    assert_eq!(
        run(&mut host, &[I(handle), W(0x820a)]).ints(),
        [-2],
        "a read sign-extends"
    );
    // An unknown handle is a failed operand: 0, with a diagnostic.
    diagnostics(&mut host);
    assert_eq!(run(&mut host, &[I(9), W(0x820a)]).ints(), [0]);
    assert!(
        diagnostics(&mut host)
            .iter()
            .any(|line| line.contains("fs_read_short: failed operand"))
    );
    // The other fs_* words stay tier P2 and do not touch the table.
    assert_eq!(run(&mut host, &[S(b"art/x.frm"), W(0x8200)]).ints(), [0]);
}

#[test]
fn interface_tags_set_script_flags_and_the_engine_condition_still_lights_a_box() {
    let (mut host, _) = sfall_host();
    let lit =
        |host: &mut WorldHost<TestAssets>, tag: i32| run(host, &[I(tag), W(0x81de)]).ints()[0];
    run(&mut host, &[I(4), W(0x81dc)]);
    assert_eq!(lit(&mut host, 4), 1);
    run(&mut host, &[I(1), W(0x81dc)]);
    assert_eq!(lit(&mut host, 1), 0, "POISONED is read-only");
    host.world.player_sheet_mut().unwrap().poison = 3;
    assert_eq!(lit(&mut host, 1), 1, "the poison level lights it");
    run(&mut host, &[I(9), W(0x81dc)]);
    assert_eq!(lit(&mut host, 9), 1, "the last custom tag");
    run(&mut host, &[I(10), W(0x81dc)]);
    assert_eq!(lit(&mut host, 10), 0, "past BoxBarCount's 5");
    assert_eq!(host.sfall_custom_tags(), [9]);
    run(&mut host, &[I(4), W(0x81dd)]);
    assert_eq!(lit(&mut host, 4), 0);
    host.world.player_sheet_mut().unwrap().flags |= crate::progression::ADDICTED_FLAG;
    assert_eq!(lit(&mut host, 4), 1, "hide cannot hide an engine box");
    host.assets
        .resources
        .insert("ddraw.ini".into(), b"[Misc]\nBoxBarCount=1\n".to_vec());
    let (mut host, _) = {
        let (mut fresh, me) = sfall_host();
        fresh.assets.resources = host.assets.resources.clone();
        (fresh, me)
    };
    run(&mut host, &[I(6), W(0x81dc)]);
    assert_eq!(lit(&mut host, 6), 0, "ddraw.ini sets the count");
    run(&mut host, &[I(5), W(0x81dc)]);
    assert_eq!(lit(&mut host, 5), 1);
    host.sfall_restored();
    assert_eq!(lit(&mut host, 5), 0, "flags clear at a load");
}

#[test]
fn screen_size_words_are_constants_and_set_hero_style_only_stores() {
    let (mut host, _) = sfall_host();
    assert_eq!(run(&mut host, &[W(0x8220)]).ints(), [640], "S12");
    assert_eq!(run(&mut host, &[W(0x8221)]).ints(), [480]);
    let art = host.world.get(host.world.player).unwrap().raw.art_id;
    assert_eq!(host.hero_style_for_test(), 0);
    run(&mut host, &[I(3), W(0x8215)]);
    assert_eq!(host.hero_style_for_test(), 3);
    assert_eq!(host.world.get(host.world.player).unwrap().raw.art_id, art);
}

#[test]
fn refresh_pc_art_applies_the_player_art_rule_at_no_cost() {
    let (mut host, _) = sfall_host();
    let player = host.world.player;
    host.world.default_player_art = 11;
    let before = host.world.action_points(player);
    host.world.get_mut(player).unwrap().raw.art_id = 0x0100_0001;
    host.world.get_mut(player).unwrap().raw.frame = 3;
    assert!(run(&mut host, &[W(0x8227)]).ints().is_empty());
    let object = host.world.get(player).unwrap();
    assert_eq!(object.raw.art_id & 0xfff, 11);
    assert_eq!(object.raw.frame, 0);
    assert_eq!(host.world.action_points(player), before);
}

#[test]
fn create_win_acts_as_createwin_with_its_default_flags() {
    let (mut host, _) = sfall_host();
    let created = run(
        &mut host,
        &[
            S(b"create_win"),
            S(b"Win"),
            I(1),
            I(2),
            I(30),
            I(40),
            W(0x827b),
        ],
    );
    assert_eq!(created.ints(), [0]);
    let window = &host.library.windows[&b"win"[..]];
    assert_eq!(window.flags, 0x4);
    assert_eq!(window.rectangle[2..], [30, 40]);
    run(
        &mut host,
        &[
            S(b"create_win"),
            S(b"Two"),
            I(1),
            I(2),
            I(30),
            I(40),
            I(0x8),
            W(0x827c),
        ],
    );
    assert_eq!(host.library.windows[&b"two"[..]].flags, 0x8);
    // A name in use is refused, as createwin refuses it, with no VM error.
    let again = run(
        &mut host,
        &[
            S(b"create_win"),
            S(b"Win"),
            I(1),
            I(2),
            I(30),
            I(40),
            W(0x827b),
        ],
    );
    assert_eq!(again.result, Ok(RunOutcome::FragmentEnd));
    assert_eq!(again.ints(), [0]);
    assert!(
        diagnostics(&mut host)
            .iter()
            .any(|line| line.contains("create_win"))
    );
}

#[test]
fn dialog_obj_gives_the_critter_the_player_talks_to() {
    let (mut host, me) = sfall_host();
    let query: &[Op] = &[S(b"dialog_obj"), W(0x8276)];
    assert_eq!(run(&mut host, query).ints(), [0], "S13");
    host.world.dialogue.open = true;
    host.world.dialogue.target = me;
    assert_eq!(run(&mut host, query).ints(), [me as i32]);
}

#[test]
fn message_box_waits_for_its_key_and_gives_the_result() {
    let (mut host, _) = sfall_host();
    let box_ops: &[Op] = &[S(b"message_box"), S(b"one\ntwo"), W(0x8277), I(7)];
    let waiting = run(&mut host, box_ops);
    assert_eq!(waiting.result, Ok(RunOutcome::Waiting));
    assert!(waiting.ints().is_empty(), "the result waits for the box");
    let open = host.sfall_open_message_box().unwrap();
    assert_eq!(open.lines, [b"one".to_vec(), b"two".to_vec()]);
    assert_eq!(open.flags, 0x11, "absent flags");
    assert_eq!(open.colors, [145, 145]);
    // A second box cannot be created.
    assert_eq!(run(&mut host, box_ops).result, Ok(RunOutcome::FragmentEnd));
    let mut vm = waiting.vm;
    assert!(
        vm.run(&mut host, 1000) == Ok(RunOutcome::Waiting),
        "it holds"
    );
    host.programs.insert(vm.id, vm);
    host.sfall_close_message_box(1).unwrap();
    assert!(host.sfall_open_message_box().is_none());
    let mut vm = host.programs.remove(&50).unwrap();
    assert!(!vm.status.contains(VmStatus::WAITING));
    assert_eq!(vm.run(&mut host, 1000), Ok(RunOutcome::FragmentEnd));
    let (seven, one) = (vm.value_stack.pop().unwrap(), vm.value_stack.pop().unwrap());
    assert_eq!((one.payload, seven.payload), (1, 7));
    // Flags -1, the colors, and the flags and colors given.
    let shown: &[Op] = &[S(b"message_box"), S(b"x"), I(-1), I(1), I(2), W(0x827a)];
    run(&mut host, shown);
    let open = host.sfall_open_message_box().unwrap();
    assert_eq!((open.flags, open.colors), (0x11, [1, 2]));
}

#[test]
fn signal_close_game_ends_the_session_after_the_step() {
    let (mut host, _) = sfall_host();
    assert_eq!(
        run(&mut host, &[S(b"signal_close_game"), W(0x8276)]).ints(),
        [0]
    );
    assert!(
        host.world
            .effects
            .iter()
            .any(|effect| matches!(effect, WorldEffect::EndGame))
    );
    assert!(!host.sfall_session_closed(), "the step is still running");
    host.sfall_end_of_step();
    assert!(host.sfall_session_closed());
}

#[test]
fn tap_key_queues_a_scan_code_for_the_next_input_phase() {
    let (mut host, _) = sfall_host();
    assert!(run(&mut host, &[I(30), W(0x8162)]).ints().is_empty());
    run(&mut host, &[I(0), W(0x8162)]);
    run(&mut host, &[I(256), W(0x8162)]);
    assert_eq!(diagnostics(&mut host).len(), 2);
    assert_eq!(host.sfall_take_keys(), [30]);
    assert!(host.sfall_take_keys().is_empty());
    // metarule3 201 empties the queue, a tapped key included.
    run(&mut host, &[I(31), W(0x8162)]);
    run(&mut host, &[I(201), I(0), I(0), I(0), W(0x80e1)]);
    assert!(host.sfall_take_keys().is_empty());
}
