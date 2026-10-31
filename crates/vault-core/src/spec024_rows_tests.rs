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

/// A font whose `a`, `b` and `c` advance 5, 6 and 7, as vector W14 says.
fn advances_font() -> vault_formats::Font {
    let width = |code: i32| match code {
        97 => 5,
        98 => 6,
        99 => 7,
        _ => 1,
    };
    let mut bytes = Vec::new();
    for value in [256, 1, 0, 0, 0] {
        bytes.extend(i32::to_le_bytes(value));
    }
    for code in 0..256 {
        bytes.extend(i32::to_le_bytes(width(code)));
        bytes.extend(i32::to_le_bytes(code));
    }
    bytes.extend([0; 256]);
    vault_formats::Font::parse_fon(&bytes).unwrap()
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
            "W14" => {
                host.assets.font = Some(Arc::new(advances_font()));
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
    /// `name(args)` as a sfall function: every call pushes one value.
    fn function(&mut self, name: &str, args: &[Op]) -> Vec<i32> {
        let mut ops = vec![S(literal(name))];
        ops.extend_from_slice(args);
        ops.push(W(0x8276 + args.len() as u16));
        run(&mut self.host, &ops).ints()
    }
    /// The one int argument of `call(n)` in `text`, if it has that call.
    fn argument(text: &str, call: &str) -> Option<i32> {
        let (_, rest) = text.split_once(&format!("{call}("))?;
        Some(number(rest.split([')', ',']).next()?))
    }
    fn session(&self) -> &crate::sfall::SessionSettings {
        &self.host.world.session
    }
    fn skill_row(&mut self, operation: &str, state: &str) {
        if let Some(value) = Self::argument(operation, "set_skill_max") {
            run(&mut self.host, &[I(value), W(0x81a2)]);
        }
        let player = self.host.world.player;
        let context = self.host.world.context(player);
        let sheet = self.host.world.player_sheet_mut().unwrap();
        let free = crate::StatContext {
            skill_cap: i32::MAX,
            ..context
        };
        sheet.invested[0] = 0;
        sheet.invested[0] = 250 - sheet.skill(0, free);
        let read = sheet.skill(0, context);
        let shown = format!("cap={}; raw 250 reads {read}", self.session().skill_cap);
        assert_eq!(shown, state);
    }
    /// W2's maximum of 90 is an example: stat 24 is the one with that table
    /// maximum, so it stands for the vector's stat 31.
    fn stat_row(&mut self, cells: &[&str]) {
        const STAT: usize = 24;
        let operation = cells[2];
        if let Some((_, rest)) = operation.split_once("set_pc_stat_max(") {
            let (stat, value) = rest.trim_end_matches(')').split_once(',').unwrap();
            let stat = if stat == "31" {
                STAT as i32
            } else {
                number(stat)
            };
            run(&mut self.host, &[I(stat), I(number(value)), W(0x81b7)]);
        }
        let player = self.host.world.player;
        let (npc, context) = (self.objects[0], self.host.world.context(player));
        let npc_context = self.host.world.context(npc);
        let read = |host: &mut WorldHost<TestAssets>, raw: i32| {
            let sheet = host.world.player_sheet_mut().unwrap();
            sheet.base[STAT] = raw;
            sheet.stat(STAT, context)
        };
        let maximum = read(&mut self.host, 100_000);
        if let Some(raw) = operation.strip_prefix("player stat 31, raw ") {
            let value = read(&mut self.host, number(raw));
            pushed(cells[3], &[value], cells);
        } else if operation.starts_with("NPC stat 31") {
            let mut sheet = crate::CharacterSheet {
                player: false,
                ..Default::default()
            };
            sheet.base[STAT] = 98;
            pushed(cells[3], &[sheet.stat(STAT, npc_context)], cells);
        } else if cells[4].contains("raw 98") {
            let value = read(&mut self.host, 98);
            let shown = format!("stat 31 maximum {maximum}; raw 98 reads {value}");
            assert_eq!(shown, cells[4]);
            return;
        }
        assert_eq!(format!("stat 31 maximum {maximum}"), cells[4]);
    }
    fn time_row(&mut self, cells: &[&str]) {
        let operation = cells[2];
        let (_, rest) = operation.split_once("(F:").unwrap();
        let multiplier: f32 = rest.trim_end_matches(')').parse().unwrap();
        run(&mut self.host, &[Op::F(multiplier), W(0x822a)]);
        let map_time = self.session().map_time;
        assert_eq!(format!("multi={map_time:?}"), cells[4]);
        let Some(ranks) = operation
            .split_once("Pathfinder ")
            .map(|(_, rest)| number(rest.split(',').next().unwrap()))
        else {
            assert_eq!(diagnostics(&mut self.host).len(), 1);
            return;
        };
        // Outdoorsman 50 gives the vector's Q of 9600.
        let speed = crate::worldmap::travel_speed_at(50, ranks as u32, map_time).unwrap();
        pushed(cells[3], &[speed.ticks as i32], cells);
    }
    fn hit_chance_row(&mut self, cells: &[&str]) {
        if let Some(value) = Self::argument(cells[2], "set_fo1_hit_chance") {
            assert_eq!(
                self.function("set_fo1_hit_chance", &[I(value)]),
                [0],
                "{cells:?}"
            );
            assert_eq!(self.session().fo1_hit_chance, value != 0);
        }
        // §3.5's is the only range term the engine has, in either state
        // (Decision D-58): r = max(10 - 2 x 6, -12) = -2 gives +8 over r = 0.
        let (host, [attacker, _, defender, _]) = crate::host_ai_tests::fixture_e("critter", 2);
        let right = crate::critical::hit_mode::RIGHT_PRIMARY;
        let mut attack = host
            .world
            .attack_profile(attacker, defender, right, 3, false)
            .unwrap();
        (attack.skill, attack.perception) = (40, 6);
        let chance = |attack: &mut crate::combat::Attack, distance: i32| {
            attack.distance = distance;
            attack.hit_chance(true).unwrap()
        };
        assert_eq!(chance(&mut attack, 10) - chance(&mut attack, 12), 8);
    }
    fn reaction_row(&mut self, cells: &[&str]) {
        if let Some(rest) = cells[2].strip_prefix("reaction ") {
            let (reaction, thresholds) = rest.split_once(", thresholds ").unwrap();
            let (neutral, good) = thresholds.split_once(',').unwrap();
            assert_eq!(self.session().reaction, [number(neutral), number(good)]);
            let mood =
                crate::dialogue::Mood::of_reaction(number(reaction), self.session().reaction);
            let shown = format!("{mood:?}").to_ascii_lowercase();
            assert_eq!(shown, cells[3], "{cells:?}");
            return;
        }
        let (_, args) = crate::spec024_arrays_tests::call(cells[2]);
        let args = [I(number(args[0])), I(number(args[1]))];
        assert_eq!(self.function("set_reaction_thresholds", &args), [0]);
        let [neutral, good] = self.session().reaction;
        assert_eq!(format!("neutral={neutral}; good={good}"), cells[4]);
    }
    fn tile_row(&mut self, cells: &[&str]) {
        let mut tiles = vec![vault_formats::map::TilePair::BLANK; 10000];
        tiles[5052] = vault_formats::map::TilePair {
            stored: 0x10a5_0123,
            word: 0x10a5_0123,
        };
        self.host.world.map.tiles[1] = Some(tiles);
        let (_, args) = crate::spec024_arrays_tests::call(cells[2]);
        let left = run(&mut self.host, &[I(number(args[0])), W(0x823a)]).ints();
        pushed(cells[3], &left, cells);
    }
    /// W10's cells stand for the fixture's cells 20 lower; a point is a pixel
    /// of that cell, as `get_terrain_name` reads world pixels (§4.11).
    fn terrain_row(&mut self, cells: &[&str]) {
        self.host
            .assets
            .game
            .insert("worldmap.msg".into(), messages("{1000}{}{Desert}\n"));
        let operation = cells[2];
        if operation == "load the slot" {
            self.host.sfall_restored();
        } else if let Some(rest) = operation.strip_prefix("get_terrain_name at cell (") {
            let (x, y) = rest.trim_end_matches(')').split_once(',').unwrap();
            let point = [(number(x) - 20) * 50 + 1, (number(y) - 20) * 50 + 1];
            let ops = [S(b"get_terrain_name"), I(point[0]), I(point[1]), W(0x8278)];
            let text = run(&mut self.host, &ops).text();
            let shown = format!("S:\"{}\"", String::from_utf8_lossy(&text));
            assert_eq!(shown, cells[3], "{cells:?}");
        } else {
            let (_, args) = crate::spec024_arrays_tests::call(operation);
            let x = number(args[0]);
            let x = if x == 9999 { x } else { x - 20 };
            let y = if x == 9999 { 0 } else { number(args[1]) - 20 };
            let name = S(literal(args[2]));
            assert_eq!(self.function("set_terrain_name", &[I(x), I(y), name]), [0]);
        }
        let overrides = self.session().terrain_names.len();
        assert_eq!(format!("overrides={overrides}"), cells[4], "{cells:?}");
    }
    /// W11's 5001 is an example, so the table holds it.
    fn critical_row(&mut self, cells: &[&str]) {
        if self.host.world.rules.tables.is_none() {
            let mut tables = crate::tables::CampaignTables::compiled().clone();
            tables.critical_success[13][0][0].failed_flags = 5001;
            self.host.world.rules.tables = Some(Arc::new(tables));
        }
        let call = cells[2].split(", then").next().unwrap();
        let (name, args) = crate::spec024_arrays_tests::call(call);
        let mut ops: Vec<Op> = args.iter().map(|arg| I(number(arg))).collect();
        ops.push(W(match name {
            "get_critical_table" => 0x81e2,
            "set_critical_table" => 0x81e1,
            _ => 0x81e3,
        }));
        let left = run(&mut self.host, &ops).ints();
        pushed(cells[3], &left, cells);
        if cells[2].ends_with("load the slot") {
            self.host.sfall_restored();
            let ops = [I(13), I(0), I(0), I(4), W(0x81e2)];
            assert_eq!(run(&mut self.host, &ops).ints(), [5001], "the base is back");
        }
        let overlay = self.host.sfall_changed_criticals();
        assert_eq!(format!("overlay={overlay}"), cells[4], "{cells:?}");
    }
    fn art_row(&mut self, cells: &[&str]) {
        // Animation 2's art is the absent file.
        self.host.assets.missing.insert(2);
        let fid = match cells[1] {
            "1" => 0x0100_0001,
            "2" => 0x0102_0001,
            "3" => 0x0102_0fff,
            _ => -1,
        };
        let left = run(&mut self.host, &[I(fid), W(0x8274)]).ints();
        pushed(cells[3], &left, cells);
    }
    fn fuel_row(&mut self, cells: &[&str]) {
        let car = &mut self.host.world.fallout2.as_deref_mut().unwrap().car;
        match cells[1] {
            "1" => (car.has_car, car.fuel) = (true, 5000),
            "3" => (car.has_car, car.fuel) = (false, 0),
            _ => {
                run(&mut self.host, &[I(32), I(90000), W(0x810b)]);
            }
        }
        let left = self.function("car_gas_amount", &[]);
        pushed(cells[3], &left, cells);
        let car = self.host.world.fallout2.as_deref().unwrap().car;
        let shown = if cells[1] == "3" {
            format!("has_car={}; car_fuel={}", i32::from(car.has_car), car.fuel)
        } else {
            format!("car_fuel={}", car.fuel)
        };
        assert_eq!(shown, cells[4]);
    }
    /// A level-up from 0 to 1 on a sheet whose R(IN) is 5.
    fn points_row(&mut self, cells: &[&str]) {
        let operation = cells[2];
        let value = Self::argument(operation, "mod_skill_points_per_level").unwrap();
        run(&mut self.host, &[I(value), W(0x8246)]);
        let gifted = operation.contains("Gifted 1");
        let player = self.host.world.player;
        let context = self.host.world.context(player);
        let sheet = self.host.world.player_sheet_mut().unwrap();
        sheet.traits = if gifted {
            [crate::Trait::Gifted as i32, -1]
        } else {
            [-1, -1]
        };
        sheet.base[4] = if gifted { 4 } else { 5 };
        (sheet.level, sheet.reconciled_level, sheet.unspent_skills) = (2, 1, 0);
        sheet.reconcile_levels(context);
        pushed(cells[3], &[sheet.unspent_skills], cells);
    }
    fn perk_row(&mut self, cells: &[&str]) {
        let operation = cells[2];
        let value = Self::argument(operation, "set_perk_freq").unwrap();
        run(&mut self.host, &[I(value), W(0x8247)]);
        let skilled = operation.contains("Skilled 1");
        let level = number(operation.rsplit("n=").next().unwrap());
        let player = self.host.world.player;
        let context = self.host.world.context(player);
        let sheet = self.host.world.player_sheet_mut().unwrap();
        sheet.traits = if skilled {
            [crate::Trait::Skilled as i32, -1]
        } else {
            [-1, -1]
        };
        (sheet.level, sheet.reconciled_level, sheet.pending_perk) = (level, level - 1, false);
        sheet.reconcile_levels(context);
        pushed(cells[3], &[i32::from(sheet.pending_perk)], cells);
    }
    /// W17's requirement of 6 is an example: perk 50's table row stands.
    fn perk_level_row(&mut self, cells: &[&str]) {
        let operation = cells[2];
        if let Some((_, rest)) = operation.split_once("set_perk_level(") {
            let (perk, level) = rest.split(')').next().unwrap().split_once(',').unwrap();
            run(
                &mut self.host,
                &[I(number(perk)), I(number(level)), W(0x817a)],
            );
        }
        let perk = crate::Perk::from_index(50).unwrap();
        let table = crate::progression::perk_row(perk).min_level;
        let player = self.host.world.player;
        let context = self.host.world.context(player);
        let sheet = self.host.world.player_sheet_mut().unwrap();
        (sheet.base[..7]).fill(10);
        sheet.invested = [300; 18];
        sheet.level = table.max(12);
        let eligible = sheet.perk_eligible(perk, context);
        if cells[3] != "-" {
            assert_eq!(eligible, cells[3] == "eligible", "{cells:?}");
        }
        let required = sheet.perk_levels.get(&perk).copied().unwrap_or(6);
        assert_eq!(format!("required={required}"), cells[4], "{cells:?}");
    }
    fn rest_row(&mut self, cells: &[&str]) {
        let operation = cells[2];
        let mut state = crate::fallout2_tests::new_game();
        let refused = (0..10).find(|map| !state.can_rest(*map, 0)).unwrap();
        let allowed = (0..10).find(|map| state.can_rest(*map, 0)).unwrap();
        state.position = [0, 0];
        self.host.world.fallout2 = Some(Box::new(state));
        if operation == "load the slot" {
            self.host.sfall_restored();
        } else {
            let mode = Self::argument(operation, "set_rest_mode").unwrap();
            self.function("set_rest_mode", &[I(mode)]);
            let refuses = operation.contains("can_rest_here 0");
            self.host.world.map.header.map_number = if refuses { refused } else { allowed };
            let allows = self.host.rest_allowed();
            assert_eq!(allows, cells[3].starts_with("allowed"), "{cells:?}");
            let player = self.host.world.player;
            let context = self.host.world.context(player);
            self.host.world.player_sheet_mut().unwrap().current_hp = 1;
            let sheet = self.host.world.get_mut(player).unwrap();
            if let vault_formats::map::ObjectState::Critter(state) = &mut sheet.raw.state {
                state.hp = 1;
            }
            let rate = self.host.world.player_sheet().unwrap().stat(14, context);
            assert!(rate > 0);
            self.host.credit_rest(crate::host_rest::REST_CREDIT_HEAL);
            let healed = self.host.world.player_sheet().unwrap().current_hp > 1;
            assert_eq!(healed, cells[3] != "allowed, no healing", "{cells:?}");
        }
        assert_eq!(format!("mode={}", self.session().rest_mode), cells[4]);
    }
    /// A sequence of `c` and a step of the program with ID 50, as `run` is.
    fn sequence_row(&mut self, cells: &[&str]) {
        let c = self.objects[0] as i32;
        let names = |host: &WorldHost<TestAssets>| -> String {
            let list = host.building_animation.as_ref();
            let commands = list.map_or(&[][..], |sequence| &sequence.commands[..]);
            let names: Vec<&str> = commands
                .iter()
                .map(|(word, values)| match (*word, values[values.len() - 2]) {
                    (0x827e, _) => "p",
                    (_, 0) => "stand",
                    _ => "animate",
                })
                .collect();
            names.join(",")
        };
        let hits = |host: &WorldHost<TestAssets>| crate::spec024_hook_tests::global_of(host, 3);
        match cells[1] {
            "1" => {
                let body = crate::spec024_hook_tests::callback_program(50);
                self.host.programs.insert(50, body);
                run(&mut self.host, &[I(1), I(1), W(0x810e)]);
            }
            "2" => drop(run(&mut self.host, &[I(c), I(16), I(0), W(0x810f)])),
            "3" => drop(run(&mut self.host, &[I(2), W(0x827e)])),
            "4" => drop(run(&mut self.host, &[I(c), I(0), I(0), W(0x810f)])),
            "5" => {
                assert_eq!(format!("sequence={}", names(&self.host)), cells[4]);
                run(&mut self.host, &[I(3), I(3), W(0x810e)]);
                return;
            }
            "6" => {
                for step in 0..400 {
                    self.host.tick(1000 + step * 50).unwrap();
                }
                assert_eq!(hits(&self.host), 1, "p ran once");
                return;
            }
            "7" => {
                // A forever animation holds the sequence before the step,
                // as the fixture's art has no frames to play through.
                run(&mut self.host, &[I(1), I(1), W(0x810e)]);
                run(&mut self.host, &[I(c), I(16), I(0), W(0x8126)]);
                run(&mut self.host, &[I(2), W(0x827e)]);
                run(&mut self.host, &[I(3), I(3), W(0x810e)]);
                self.host.motions.clear();
                for step in 0..50 {
                    self.host.tick(30_000 + step * 50).unwrap();
                }
                assert_eq!(hits(&self.host), 1, "the cancelled step did not run");
                return;
            }
            _ => {
                diagnostics(&mut self.host);
                run(&mut self.host, &[I(2), W(0x827e)]);
                assert_eq!(diagnostics(&mut self.host).len(), 1);
                return;
            }
        }
        assert_eq!(format!("sequence={}", names(&self.host)), cells[4]);
    }
    fn name_row(&mut self, cells: &[&str]) {
        let object = self.objects[0];
        if cells[1] == "1" {
            self.host.world.get_mut(object).unwrap().name = b"table".to_vec();
        }
        let set = |session: &mut Self, name: &str| {
            let ops = [
                I(object as i32),
                W(0x8206),
                S(b"set_scr_name"),
                S(literal(name)),
                W(0x8277),
            ];
            let left = run(&mut session.host, &ops).ints();
            pushed(cells[3], &left, cells);
        };
        let operation = cells[2];
        if operation.starts_with("obj_name") {
            let text = run(&mut self.host, &[I(object as i32), W(0x80a4)]).text();
            let shown = format!("S:\"{}\"", String::from_utf8_lossy(&text));
            assert_eq!(shown, cells[3], "{cells:?}");
        } else if operation.starts_with("leave the map") {
            self.host.sfall_reset_names();
        } else {
            let (_, args) = crate::spec024_arrays_tests::call(operation);
            set(self, args[0]);
        }
        let name = self.host.world.get(object).unwrap().name.clone();
        let shown = if name == b"table" {
            "override=none".to_owned()
        } else {
            format!("override=\"{}\"", String::from_utf8_lossy(&name))
        };
        assert_eq!(shown, cells[4], "{cells:?}");
    }
    fn spatial_row(&mut self, cells: &[&str]) {
        while self.host.assets.programs.len() < 85 {
            let name = format!("s{}.int", self.host.assets.programs.len());
            let program = crate::test_support::program(&[(b"start", Vec::new())]);
            self.host.assets.programs.push((name, program));
        }
        let (_, args) = crate::spec024_arrays_tests::call(cells[2]);
        let mut ops: Vec<Op> = args.iter().map(|arg| I(number(arg))).collect();
        ops.push(W(0x8273));
        let id = run(&mut self.host, &ops).ints()[0];
        if cells[3] == "I:0" {
            assert_eq!((id, cells[4]), (0, "no record"));
            return;
        }
        assert_ne!(id, 0, "a handle for the record");
        let script = &self.host.world.scripts[&(id as u32)];
        let (hex, elevation, radius) = script.spatial.unwrap();
        let shown = format!(
            "spatial record: script {}, location {}, radius {radius}",
            script.file + 1,
            hex + i32::from(elevation) * 536870912
        );
        assert_eq!(shown, cells[4]);
    }
    fn message_box_row(&mut self, cells: &[&str]) {
        self.host.world.dialogue.script_message = None;
        self.host.world.dialogue.open = !cells[2].ends_with("neither");
        diagnostics(&mut self.host);
        assert_eq!(self.function("dialog_message", &[S(b"x")]), [0]);
        let shown = self.host.world.dialogue.script_message.is_some();
        assert_eq!(shown, !cells[4].starts_with("nothing"), "{cells:?}");
        assert_eq!(diagnostics(&mut self.host).len(), usize::from(!shown));
    }
    fn script_row(&mut self, cells: &[&str]) {
        let object = self.objects[0];
        let argument = match cells[1] {
            "1" => {
                let program = crate::test_support::program(&[(b"start", Vec::new())]);
                self.host.assets.programs.push(("s.int".into(), program));
                let id = self.host.world.add_script(object, 84, 1, 0).unwrap();
                assert_eq!(self.host.world.script_for(object), Some(id));
                object as i32
            }
            "2" => self.objects[1] as i32,
            _ => 0,
        };
        let left = run(&mut self.host, &[I(argument), W(0x81f5)]).ints();
        pushed(cells[3], &left, cells);
    }
    fn cursor_row(&mut self, cells: &[&str]) {
        let value = Self::argument(cells[2], "set_cursor_mode").unwrap();
        assert_eq!(self.function("set_cursor_mode", &[I(value)]), [0]);
        let shown = match self.session().cursor_mode {
            Some(0) => "movement",
            Some(1) => "command",
            _ => "targeting",
        };
        assert_eq!(format!("mode={shown}"), cells[4]);
    }
    /// W28's point is outside the fixture's world, which wraps it in.
    fn position_row(&mut self, cells: &[&str]) {
        let [width, height] = self.host.world.fallout2.as_deref().unwrap().world_size();
        let (_, args) = crate::spec024_arrays_tests::call(cells[2]);
        let (x, y) = (number(args[0]), number(args[1]));
        let wrapped = |x: i32, y: i32| {
            if x < 0 {
                [x, y]
            } else {
                [x % width, y % height]
            }
        };
        let point = wrapped(x, y);
        run(&mut self.host, &[I(point[0]), I(point[1]), W(0x8172)]);
        let position = self.host.world.fallout2.as_deref().unwrap().position;
        assert_eq!(position, wrapped(772, 995), "{cells:?}");
    }
    fn tile_objects_row(&mut self, cells: &[&str]) {
        let [a, b] = self.objects;
        for object in [a, b] {
            self.host.world.get_mut(object).unwrap().raw.hex = Some(15000);
        }
        let (_, args) = crate::spec024_arrays_tests::call(cells[2]);
        let ops = [I(number(args[0])), I(number(args[1])), W(0x8270)];
        let id = run(&mut self.host, &ops).ints()[0];
        let names: Vec<&str> = list_of(&mut self.host, id)
            .iter()
            .map(|object| if *object == a as i32 { "A" } else { "B" })
            .collect();
        let shown = format!("temporary list [{}]", names.join(","));
        assert_eq!(shown, cells[3], "{cells:?}");
    }
    fn unwield_row(&mut self, cells: &[&str]) {
        use vault_formats::pro::BasicFlags;
        let player = self.host.world.player;
        let carried = |kind: usize, host: &mut WorldHost<TestAssets>, flag: u32| {
            let mut bytes = vault_fixtures::world::prototype(0, kind);
            vault_fixtures::world::word(&mut bytes, 0, 0x60 + kind as i32);
            let prototype = Arc::new(vault_formats::Prototype::parse(&bytes).unwrap());
            let item = host.world.spawn(prototype, 0, 0).unwrap();
            host.world.add_item(player, item, 1, true).unwrap();
            host.world.equip(player, item, flag).unwrap();
            item
        };
        diagnostics(&mut self.host);
        let held = match cells[1] {
            "1" => Some(carried(3, &mut self.host, BasicFlags::RIGHT_HAND.0)),
            "2" => Some(carried(0, &mut self.host, BasicFlags::WORN.0)),
            _ => None,
        };
        let (_, args) = crate::spec024_arrays_tests::call(cells[2]);
        let critter = if args[0] == "dude_obj" {
            player as i32
        } else {
            0
        };
        let args = [I(critter), I(number(args[1]))];
        assert_eq!(self.function("unwield_slot", &args), [0]);
        if let Some(item) = held {
            let object = self.host.world.get(item).unwrap();
            assert_eq!(object.owner, Some(player), "{cells:?}");
            let equipped = BasicFlags::WORN.0 | BasicFlags::RIGHT_HAND.0 | BasicFlags::LEFT_HAND.0;
            assert_eq!(object.raw.flags.0 & equipped, 0, "{cells:?}");
        }
        let expected = usize::from(matches!(cells[1], "4" | "5"));
        assert_eq!(diagnostics(&mut self.host).len(), expected, "{cells:?}");
    }
    /// One row of W1 to W30.
    fn word_row(&mut self, cells: &[&str]) {
        let flag = |text: &str, key: &str| text.contains(&format!("{key}=1"));
        match cells[0] {
            "W1" => self.skill_row(cells[2], cells[4]),
            "W2" => self.stat_row(cells),
            "W3" => self.time_row(cells),
            "W4" => {
                if let Some(value) = Self::argument(cells[2], "encounter_detection") {
                    self.function("encounter_detection", &[I(value)]);
                }
                let on = flag(cells[4], "detection");
                assert_eq!(self.session().encounter_detection, on, "{cells:?}");
            }
            "W5" => {
                if let Some(value) = Self::argument(cells[2], "npc_engine_level_up") {
                    self.function("npc_engine_level_up", &[I(value)]);
                }
                assert_eq!(self.session().npc_level_up, flag(cells[4], "engine"));
            }
            "W6" => {
                let before = self.host.world.state_hash();
                let value = Self::argument(cells[2], "remove_wm_town_names").unwrap();
                self.function("remove_wm_town_names", &[I(value)]);
                assert_eq!(self.session().town_names, flag(cells[4], "names"));
                assert_eq!(self.host.world.state_hash(), before);
            }
            "W7" => self.hit_chance_row(cells),
            "W8" => self.reaction_row(cells),
            "W9" => self.tile_row(cells),
            "W10" => self.terrain_row(cells),
            "W11" => self.critical_row(cells),
            "W12" => self.art_row(cells),
            "W13" => self.fuel_row(cells),
            "W14" => {
                let (_, args) = crate::spec024_arrays_tests::call(cells[2]);
                let left = self.function("get_text_width", &[S(literal(args[0]))]);
                pushed(cells[3], &left, cells);
            }
            "W15" => self.points_row(cells),
            "W16" => self.perk_row(cells),
            "W17" => self.perk_level_row(cells),
            "W18" => self.rest_row(cells),
            "W19" => {
                if let Some(value) = Self::argument(cells[2], "set_car_intface_art") {
                    self.function("set_car_intface_art", &[I(value)]);
                }
                assert_eq!(format!("car art={}", self.session().car_art), cells[4]);
            }
            "W20" => {
                let before = self.host.world.state_hash();
                let left = match cells[1] {
                    "1" => self.function("intface_redraw", &[]),
                    "2" => self.function("intface_redraw", &[I(-1)]),
                    _ => self.function("tile_refresh_display", &[]),
                };
                pushed(cells[3], &left, cells);
                assert_eq!(self.host.world.state_hash(), before);
            }
            "W21" => {
                let left = self.function("real_dude_obj", &[]);
                assert_eq!(left, [self.host.world.player as i32]);
            }
            "W22" => self.sequence_row(cells),
            "W23" => self.name_row(cells),
            "W24" => self.spatial_row(cells),
            "W25" => self.message_box_row(cells),
            "W26" => self.script_row(cells),
            "W27" => self.cursor_row(cells),
            "W28" => self.position_row(cells),
            "W29" => self.tile_objects_row(cells),
            "W30" => self.unwield_row(cells),
            other => panic!("no word vector {other}"),
        }
    }
    pub(crate) fn row(&mut self, cells: &[&str]) {
        match &cells[0][..1] {
            "V" => self.file_row(cells),
            "U" => self.id_row(cells),
            "M" => self.message_row(cells),
            "T" => self.tag_row(cells),
            "W" => self.word_row(cells),
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
    host.hook_host = Some(Box::default());
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
