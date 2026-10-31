//! SPEC-024b §4.11: each row Revision 3 promoted, under a script, on its
//! synthetic stack vectors (§7.1) and its rules.
use crate::{
    CharacterSheet, GameProfile, WorldEffect, WorldHost, WorldState,
    assets::{VfsWorldAssets, WorldAssets},
    fallout2_tests::new_game,
    fixture_items::{self as support, ItemAssets, WEAPON_W},
    sfall::{SfallWorld, UNIQUE_IDS, encounter_flag},
    spec024_tests::{I, ORACLE, Op, S, W, diagnostics, run, sfall_host},
    test_support::{TestAssets, npc_prototype},
    world::ObjectRef,
};
use std::sync::Arc;
use vault_formats::{Vfs, msg::MsgCatalog};
use vault_vm::{RunOutcome, VmStatus};

const LIST_LEN: u16 = 0x8231;
const LIST_GET: u16 = 0x822f;

fn hex(cell: &str) -> Vec<u8> {
    cell.split_whitespace()
        .map(|byte| u8::from_str_radix(byte, 16).unwrap())
        .collect()
}
/// A string of a vector's call, as a script's literal.
fn literal(text: &str) -> &'static [u8] {
    Box::leak(
        text.trim_matches('"')
            .as_bytes()
            .to_vec()
            .into_boxed_slice(),
    )
}
/// `0x10` or `5`.
fn number(cell: &str) -> i32 {
    match cell.strip_prefix("0x") {
        Some(hex) => u32::from_str_radix(hex, 16).unwrap() as i32,
        None => cell.parse().unwrap(),
    }
}
/// The push a row states, against what a word left on the stack.
fn pushed(cell: &str, ints: &[i32], cells: &[&str]) {
    match cell {
        "-" => assert!(ints.is_empty(), "{cells:?}"),
        cell => assert_eq!(
            ints,
            [number(cell.strip_prefix("I:").unwrap())],
            "{cells:?}"
        ),
    }
}

/// The session of vectors V, U, M, T and E, row by row.
pub(crate) struct RowSession {
    host: WorldHost<TestAssets>,
    objects: [ObjectRef; 2],
}
impl RowSession {
    pub(crate) fn for_group(group: &str) -> Self {
        let (mut host, a) = sfall_host();
        let b = host.world.spawn(npc_prototype(), 20001, 0).unwrap();
        match group {
            // V1's first state holds the file's bytes.
            "V1" | "V2" | "V3" | "V4" => {
                let cell = crate::spec024_arrays_tests::vector_cell("V1", "1", 4);
                let bytes = hex(cell.split_once("bytes=").unwrap().1);
                host.assets.resources.insert("art/x.frm".into(), bytes);
            }
            // The ordinary IDs U1 shows, and its allocator's 900.
            "U1" => {
                host.world.set_object_id(a, 5);
                host.world.set_object_id(b, 6);
                host.world.set_next_object_id(0x384);
            }
            "M1" => {
                let text = "{5}{}{text}\n";
                host.assets.game.insert("a.msg".into(), messages(text));
                host.assets.game.insert("b.msg".into(), messages(text));
            }
            "E1" => {
                let weapon = support::parse(&support::bytes(WEAPON_W));
                let mut proto = (*weapon).clone();
                proto.pid = 159;
                host.assets.prototypes.insert(159, Arc::new(proto));
                let mut plain = (*support::parse(&support::bytes(WEAPON_W))).clone();
                plain.data = vault_formats::pro::PrototypeData::Misc;
                plain.pid = 41;
                host.assets.prototypes.insert(41, Arc::new(plain));
            }
            _ => {}
        }
        Self {
            host,
            objects: [a, b],
        }
    }
    fn object(&self, cell: &str) -> i32 {
        self.objects[usize::from(cell != "A")] as i32
    }
    fn file_state(&mut self, cells: &[&str]) {
        if cells[4] == "-" {
            return;
        }
        let (position, bytes) = self.host.virtual_file_for_test(1);
        let state = cells[4].strip_prefix("pos=").unwrap();
        let (at, shown) = state.split_once(" bytes=").unwrap();
        assert_eq!(position, at.parse::<usize>().unwrap(), "{cells:?}");
        assert_eq!(bytes, hex(shown), "{cells:?}");
    }
    /// One `fs_*` call, and what it left on the stack.
    fn file_call(&mut self, call: &str) -> Vec<i32> {
        let (name, args) = crate::spec024_arrays_tests::call(call);
        let int = |index: usize| I(number(args[index]));
        let ops = match name {
            "fs_copy" => vec![S(literal(args[0])), S(literal(args[1])), W(0x81f8)],
            "fs_seek" => vec![int(0), int(1), W(0x8202)],
            "fs_read_short" => vec![int(0), W(0x820a)],
            "fs_write_short" => vec![int(0), int(1), W(0x81fb)],
            other => panic!("{other}"),
        };
        run(&mut self.host, &ops).ints()
    }
    fn file_row(&mut self, cells: &[&str]) {
        // "a at 0, then b at 0": a seek to 0 before each call.
        let (calls, seeks): (Vec<&str>, bool) = match cells[2].split_once(" at 0, then ") {
            Some((first, second)) => (vec![first, second.trim_end_matches(" at 0")], true),
            None => (cells[2].split("; ").collect(), false),
        };
        let mut left = Vec::new();
        for call in calls {
            if seeks {
                self.file_call("fs_seek(1,0)");
            }
            left = self.file_call(call);
        }
        pushed(cells[3], &left, cells);
        self.file_state(cells);
    }
    fn id_row(&mut self, cells: &[&str]) {
        let (_, args) = crate::spec024_arrays_tests::call(cells[2]);
        let object = self.object(args[0]);
        let mut ops = vec![S(b"set_unique_id"), I(object)];
        match args.get(1) {
            Some(flag) => ops.extend([I(number(flag)), W(0x8278)]),
            None => ops.push(W(0x8277)),
        }
        let done = run(&mut self.host, &ops);
        pushed(cells[3], &done.ints(), cells);
        let host = &self.host;
        let next = host.world.sfall.as_deref().unwrap().next_unique;
        let shown = format!(
            "A={:#x} B={:#x} next={next:#x}",
            host.object_id_for_test(self.objects[0]),
            host.object_id_for_test(self.objects[1])
        );
        assert_eq!(shown, cells[4], "{cells:?}");
    }
    fn message_row(&mut self, cells: &[&str]) {
        let (name, args) = crate::spec024_arrays_tests::call(cells[2]);
        if name == "add_extra_msg_file" {
            let ops = [S(b"add_extra_msg_file"), S(literal(args[0])), W(0x8277)];
            let done = run(&mut self.host, &ops);
            pushed(cells[3], &done.ints(), cells);
            let next = self.host.next_message_file_for_test();
            assert_eq!(format!("next={next:#x}"), cells[4], "{cells:?}");
            return;
        }
        let ops = [I(number(args[0])), I(number(args[1])), W(0x826b)];
        let text = run(&mut self.host, &ops).text();
        assert_eq!(
            format!("S:\"{}\"", String::from_utf8_lossy(&text)),
            cells[3],
            "{cells:?}"
        );
    }
    fn tag_row(&mut self, cells: &[&str]) {
        // Rows 5 and 6 are about an addicted player.
        if cells[2].contains("player addicted") {
            let sheet = self.host.world.player_sheet_mut().unwrap();
            sheet.flags |= crate::progression::ADDICTED_FLAG;
        }
        let (name, args) = crate::spec024_arrays_tests::call(cells[2]);
        let tag = I(number(args[0]));
        let word = match name {
            "show_iface_tag" => 0x81dc,
            "hide_iface_tag" => 0x81dd,
            _ => 0x81de,
        };
        let done = run(&mut self.host, &[tag, W(word)]);
        pushed(cells[3], &done.ints(), cells);
        let lit: Vec<String> = (0..=9)
            .filter(|tag| run(&mut self.host, &[I(*tag), W(0x81de)]).ints() == [1])
            .map(|tag| tag.to_string())
            .collect();
        match cells[4] {
            "-" => {}
            "box stays lit" => assert_eq!(lit, ["4"], "{cells:?}"),
            state => assert_eq!(format!("lit=[{}]", lit.join(",")), state, "{cells:?}"),
        }
    }
    fn explosive_row(&mut self, cells: &[&str]) {
        let (_, args) = crate::spec024_arrays_tests::call(cells[2]);
        let ops: Vec<Op> = args
            .iter()
            .map(|arg| I(number(arg)))
            .chain([W(0x8261)])
            .collect();
        let id = run(&mut self.host, &ops).ints()[0];
        let shown = list_of(&mut self.host, id);
        let expected = if cells[3].contains("proto min") {
            let weapon = support::parse(&support::bytes(WEAPON_W));
            let vault_formats::pro::PrototypeData::Item(item) = &weapon.data else {
                panic!("a weapon fixture");
            };
            let vault_formats::pro::ItemData::Weapon(data) = &item.data else {
                panic!("a weapon fixture");
            };
            format!("temporary list [{},{}]", data.damage[0], data.damage[1])
        } else {
            cells[3].to_owned()
        };
        assert_eq!(
            format!("temporary list [{},{}]", shown[0], shown[1]),
            expected,
            "{cells:?}"
        );
        // A temporary list: the step's end frees it.
        self.host.sfall_end_of_step();
        assert_eq!(run(&mut self.host, &[I(id), W(LIST_LEN)]).ints(), [-1]);
    }
    pub(crate) fn row(&mut self, cells: &[&str]) {
        match &cells[0][..1] {
            "V" => self.file_row(cells),
            "U" => self.id_row(cells),
            "M" => self.message_row(cells),
            "T" => self.tag_row(cells),
            _ => self.explosive_row(cells),
        }
    }
}

fn list_of(host: &mut WorldHost<TestAssets>, id: i32) -> Vec<i32> {
    let length = run(host, &[I(id), W(LIST_LEN)]).ints()[0];
    (0..length)
        .map(|index| run(host, &[I(id), I(index), W(LIST_GET)]).ints()[0])
        .collect()
}
pub(crate) fn messages(text: &str) -> Arc<MsgCatalog> {
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
fn extra_game_msg_file_list_is_read_from_misc_only() {
    let (mut host, _) = sfall_host();
    host.assets
        .game
        .insert("first.msg".into(), messages("{5}{}{first}\n"));
    host.assets.resources.insert(
        "ddraw.ini".into(),
        b"[Other]\nExtraGameMsgFileList=first\n".to_vec(),
    );
    assert_eq!(
        run(&mut host, &[I(0x2000), I(5), W(0x826b)]).text(),
        b"Error"
    );
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

/// §7.3 test 10, through the real assets: `art/x.frm` in a VFS.
#[test]
fn t10_a_load_reads_the_patched_rate_and_a_new_game_or_a_load_removes_the_file() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(dir.path().join("art")).unwrap();
    let frm = vault_fixtures::world::frm(1, 1, 1);
    std::fs::write(dir.path().join("art/x.frm"), &frm).unwrap();
    let vfs: Arc<dyn Vfs> = Arc::new(vault_formats::LocalDirProvider::mount(dir.path()).unwrap());
    let assets = VfsWorldAssets::new(vfs, "english").unwrap();
    assets.set_game_profile(GameProfile::Fallout2);
    let world = WorldState::new(CharacterSheet::default(), 1).unwrap();
    let mut host = WorldHost::new(world, assets);
    host.world.fallout2 = Some(Box::new(new_game()));
    host.enable_sfall().unwrap();
    let rate = |host: &WorldHost<VfsWorldAssets>| host.assets.frame("art/x.frm").unwrap().fps;
    let path: &'static [u8] = b"art/x.frm";
    let write = |host: &mut WorldHost<VfsWorldAssets>| {
        let handle = run(host, &[S(path), S(path), W(0x81f8)]).ints()[0];
        assert_eq!(handle, 1);
        run(host, &[I(handle), I(4), W(0x8202)]);
        run(host, &[I(handle), I(25), W(0x81fb)]);
    };
    let plain = rate(&host);
    assert_ne!(plain, 25);
    // V1's write, over a decode the cache already holds.
    write(&mut host);
    assert_eq!(rate(&host), 25, "the write dropped the cached decode");
    host.sfall_restored();
    assert_eq!(rate(&host), plain, "a load removes the virtual file");
    write(&mut host);
    assert_eq!(rate(&host), 25);
    host.enable_sfall().unwrap();
    assert_eq!(rate(&host), plain, "a new game removes it too");
    // The handle went with it.
    assert_eq!(run(&mut host, &[I(1), W(0x820a)]).ints(), [0]);
}

/// §7.3 test 11, on the fixture tables' encounter.
#[test]
fn t11_a_forced_encounter_draws_nothing_and_survives_a_save() {
    let (mut host, _) = sfall_host();
    host.assets.any_map = Some(crate::world::empty_map());
    let mut state = new_game();
    state.position = [10, 10];
    host.world.fallout2 = Some(Box::new(state));
    let counters = |host: &WorldHost<TestAssets>| {
        let state = host.world.fallout2.as_deref().unwrap();
        (state.counter(0, 1), state.counter(0, 3))
    };
    let before = counters(&host);
    run(&mut host, &[I(2), I(0x8), W(0x8229)]);
    // Saved between the call and the check, it occurs after a load.
    let sidecar = host.world.sfall.as_deref().unwrap().sidecar();
    host.world.sfall = Some(Box::new(SfallWorld::from_sidecar(&sidecar).unwrap()));
    let drawn = host.world.rng.clone();
    let selected = host.fallout2_check_encounter().unwrap().unwrap();
    assert_eq!(host.world.rng, drawn, "no random number is drawn");
    assert_eq!(counters(&host), before, "no counter changes");
    assert_eq!((selected.map, selected.special), (2, true));
    host.fallout2_enter_encounter(selected).unwrap();
    let others = host
        .world
        .iter()
        .filter(|(object, _)| *object != host.world.player);
    assert_eq!(others.count(), 0, "no group is spawned");
    let state = host.world.sfall.as_deref().unwrap();
    assert_eq!(state.forced_map, -1, "the pending encounter cleared");
}

#[test]
fn forced_encounter_flags_leave_the_car_and_fade_to_black() {
    // Driving, the car comes along; with `ENCOUNTER_FLAG_NO_CAR` it stays.
    for (flags, moves) in [(0, true), (encounter_flag::NO_CAR, false)] {
        let (mut host, _) = sfall_host();
        host.assets.any_map = Some(crate::world::empty_map());
        let mut state = new_game();
        (state.car.has_car, state.car.area, state.car.map) = (true, 5, 5);
        (state.driving, state.world_map_open) = (true, true);
        host.world.fallout2 = Some(Box::new(state));
        run(&mut host, &[I(2), I(flags as i32), W(0x8229)]);
        let selected = host.fallout2_check_encounter().unwrap().unwrap();
        host.fallout2_enter_encounter(selected).unwrap();
        let car = host.world.fallout2.as_deref().unwrap().car;
        assert_eq!(car.map != 5, moves, "flags {flags}");
    }
    let (mut host, _) = sfall_host();
    host.assets.any_map = Some(crate::world::empty_map());
    host.world.fallout2 = Some(Box::new(new_game()));
    let fades = |host: &WorldHost<TestAssets>| {
        host.world
            .effects
            .iter()
            .filter(|effect| matches!(effect, WorldEffect::Fade { out: true, .. }))
            .count()
    };
    run(
        &mut host,
        &[I(2), I(encounter_flag::FADEOUT as i32), W(0x8229)],
    );
    let selected = host.fallout2_check_encounter().unwrap().unwrap();
    host.fallout2_enter_encounter(selected).unwrap();
    assert_eq!(fades(&host), 1);
    assert!(host.library.faded_out, "the load does not fade back in");
}

/// §7.3 test 12.
#[test]
fn t12_a_unique_id_does_not_stack_and_a_released_one_stacks_again() {
    let mut host = item_host();
    let owner = host.world.spawn(npc_prototype(), 20000, 0).unwrap();
    let (a, b) = (
        support::spawn(&mut host, 0x500),
        support::spawn(&mut host, 0x500),
    );
    assert!(host.world.stackable(a, b), "otherwise identical");
    host.set_unique_id(a, 0);
    assert!(!host.world.stackable(a, b));
    host.world.add_item(owner, a, 1, true).unwrap();
    host.world.add_item(owner, b, 1, true).unwrap();
    assert_eq!(host.world.get(owner).unwrap().inventory.len(), 2);
    host.set_unique_id(a, -1);
    assert!(host.world.stackable(a, b));
    host.world.drop_item(owner, a, 1).unwrap();
    host.world.add_item(owner, a, 1, true).unwrap();
    assert_eq!(host.world.get(owner).unwrap().inventory.len(), 1);
}

/// §7.3 test 13.
#[test]
fn t13_opcode_exists_0x823b_gives_1_under_sfall_mode() {
    let (mut host, _) = sfall_host();
    assert_eq!(
        run(&mut host, &[S(b"opcode_exists"), I(0x823b), W(0x8277)]).ints(),
        [1]
    );
}

/// §7.3 test 14.
#[test]
fn t14_every_row_the_audit_promoted_is_tier_p1_in_its_table() {
    const USAGE: &str = include_str!("../../../oracle/SPEC-024/mod-opcode-usage.tsv");
    const HOOKS: &str = include_str!("../../../oracle/SPEC-024/sfall-hooks.tsv");
    let header: Vec<&str> = USAGE.lines().next().unwrap().split('\t').collect();
    let column = |name: &str| header.iter().position(|cell| *cell == name).unwrap();
    let (kind, id, tier) = (column("kind"), column("id"), column("rev3_tier"));
    let mut promoted = 0;
    for line in USAGE.lines().skip(1) {
        let cells: Vec<&str> = line.split('\t').collect();
        if cells[tier] != "P1" {
            continue;
        }
        promoted += 1;
        let row = if cells[kind] == "hook" {
            HOOKS
                .lines()
                .map(|row| row.split('\t').collect::<Vec<_>>())
                .find(|row| row[0] == cells[id])
                .map(|row| row[3].to_owned())
        } else {
            ORACLE
                .lines()
                .map(|row| row.split('\t').collect::<Vec<_>>())
                .find(|row| row[0] == cells[kind] && row[1] == cells[id])
                .map(|row| row[9].to_owned())
        };
        assert_eq!(row.as_deref(), Some("P1"), "{line}");
    }
    assert_eq!(promoted, 32, "26 opcode rows and 6 hooks");
}
