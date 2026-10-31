//! SPEC-024a §10: the manifest hook API on the engine's chain. The order
//! vectors of `tests/fixtures/spec024a/` run member by member, and §9.1's
//! program runs through the loader. What needs a session, a replay or a
//! manifest on disk is in `vault_platform`.
use crate::{
    CharacterSheet, GameProfile, ObjectAction, WorldHost, WorldState,
    host_mods::{ModGlobal, ModHook, ModSession, ScriptedMod},
    host_sfall_boundaries::{HOOK_ADJUSTFID, HOOK_AMMOCOST, HOOK_CARTRAVEL, HOOK_WITHINPERCEPTION},
    host_sfall_hooks::{
        HOOK_CALCAPCOST, HOOK_COMBATDAMAGE, HOOK_ENCOUNTER, HOOK_GAMEMODECHANGE, HOOK_SETGLOBALVAR,
    },
    sfall::SfallValue,
    spec024_hook_tests::{
        ADD, DISCARD, DUPLICATE, GET_ARG, REGISTER_PROC, REGISTER_SPEC, SET_ARG, SET_RETURN, SWAP,
        install, script_in, set_up,
    },
    spec024_tests::{
        Op::{self, I, S, W},
        diagnostics, run_as,
    },
    state_hash::fnv1a64,
    test_support::{ArtPolicy, TestAssets},
    world::ObjectRef,
};
use std::{collections::BTreeMap, sync::Arc};
use vault_formats::{
    ModManifest,
    mod_manifest::{ModSetting, hook_point},
};
use vault_vm::{IntProgram, RunOutcome, VmError, WordSet, stack_check};

const ORDER: &str = include_str!("../../../tests/fixtures/spec024a/hook-order.tsv");
const ORDER_REV4: &str = include_str!("../../../tests/fixtures/spec024a/hook-order-rev4.tsv");
const POINTS: &str = include_str!("../../../tests/fixtures/spec024a/hook-points.tsv");
const WORDS: &str = include_str!("../../../tests/fixtures/spec024a/mod-words.tsv");
const N1: &str = include_str!("../../../tests/fixtures/spec024a/vault-mod.toml");
const N2: &str = include_str!("../../../tests/fixtures/spec024a/vault-mod-row.toml");
const PROGRAM: &str = include_str!("../../../tests/fixtures/spec024a/gl_cheapap.int.hex");
const ORACLE_HOOKS: &str = include_str!("../../../oracle/SPEC-024/sfall-hooks.tsv");

const API: u16 = 0x8300;
const GET_SETTING: u16 = 0x8301;
const SET_SETTING: u16 = 0x8302;
const LOG: u16 = 0x8303;
const REGISTER: u16 = 0x8304;
const HOOK_ID: u16 = 0x8305;
const ARG: u16 = 0x8306;
const SET_HOOK_ARG: u16 = 0x8307;
const RETURN: u16 = 0x8308;
const MODE: u16 = 0x8309;
const SET_GLOBAL_VAR: u16 = 0x80c6;

fn words() -> WordSet {
    WordSet::FALLOUT2_SFALL.with_mods(true)
}
fn program(strings: &[&[u8]], start: &[Op], procedures: &[(&'static str, &[Op])]) -> IntProgram {
    script_in(&words(), strings, start, procedures)
}
/// A Fallout 2 host, with sfall mode as `sfall` says.
fn host(sfall: bool) -> WorldHost<TestAssets> {
    let world = WorldState::new(CharacterSheet::default(), 1).unwrap();
    let assets = TestAssets {
        profile: GameProfile::Fallout2,
        ..TestAssets::all_art()
    };
    let mut host = WorldHost::new(world, assets);
    host.world.fallout2 = Some(Box::new(crate::fallout2_tests::new_game()));
    if sfall {
        host.enable_sfall().unwrap();
    }
    host.world.effects.clear();
    host
}
fn row(hook: i32, file: &str, procedure: &str) -> ModHook {
    ModHook {
        hook,
        file: file.to_owned(),
        procedure: procedure.to_owned(),
    }
}
fn mod_of(id: &str, globals: &[&str], hooks: Vec<ModHook>) -> ScriptedMod {
    ScriptedMod {
        id: id.to_owned(),
        version: String::new(),
        manifest: 0,
        globals: globals
            .iter()
            .map(|file| ModGlobal {
                file: (*file).to_owned(),
                repeat: 0,
                kind: 0,
            })
            .collect(),
        hooks,
        defaults: BTreeMap::new(),
    }
}
/// `mods` on `host`, set up.
fn enable(host: &mut WorldHost<TestAssets>, mods: Vec<ScriptedMod>) -> Arc<ModSession> {
    let session = Arc::new(ModSession::new(mods, true));
    host.enable_mod_scripts(session.clone());
    set_up(host);
    session
}

/// A row of `hook-order.tsv`.
#[derive(Debug)]
struct Turn {
    case: String,
    sfall: bool,
    member: String,
    block: String,
    outcome: String,
    read: Option<i32>,
    after: i32,
    ret: Option<i32>,
    result: Option<i32>,
}
fn turns() -> Vec<Turn> {
    ORDER
        .lines()
        .skip(1)
        .map(|line| {
            let cells: Vec<&str> = line.split('\t').collect();
            Turn {
                case: cells[0].to_owned(),
                sfall: cells[1] == "on",
                member: cells[3].to_owned(),
                block: cells[4].to_owned(),
                outcome: cells[5].to_owned(),
                read: cells[6].parse().ok(),
                after: cells[7].parse().unwrap(),
                ret: cells[8].parse().ok(),
                result: cells[10].parse().ok(),
            }
        })
        .collect()
}
/// A member's body for its row: it reads arg3, and every write is that value
/// plus a constant, so a member that reads the wrong value writes the wrong
/// one. Sfall members use sfall's words, and mod members the mod words.
fn body(turn: &Turn, previous_ret: Option<i32>) -> Vec<Op> {
    let sfall = turn.block == "sfall";
    let read = turn.read.unwrap_or(0);
    let mut ops = Vec::new();
    if sfall {
        for _ in 0..3 {
            ops.extend([GET_ARG, DISCARD]);
        }
        ops.push(GET_ARG);
    } else {
        ops.extend([I(3), W(ARG)]);
    }
    if turn.after != read {
        ops.extend([DUPLICATE, I(3), SWAP, I(turn.after - read), ADD]);
        ops.push(if sfall { SET_ARG } else { W(SET_HOOK_ARG) });
    }
    match turn.ret.filter(|ret| Some(*ret) != previous_ret) {
        Some(ret) => {
            ops.extend([I(ret - read), ADD]);
            if sfall {
                ops.push(SET_RETURN);
            } else {
                ops.extend([I(0), SWAP, W(RETURN)]);
            }
        }
        None => ops.push(DISCARD),
    }
    match turn.outcome.as_str() {
        "vm-error" => ops.push(W(0x8252)),
        "suspended" => ops.extend([I(1), W(0x8047)]),
        "return-unconverted" => ops.extend([I(0), S(b"x"), W(RETURN)]),
        _ => {}
    }
    ops
}
/// The file of a member: `alpha:r1` is `alpha_r1.int`.
fn file_of(member: &str) -> String {
    format!("{}.int", member.replace(':', "_"))
}
/// `start`, and the procedures, of `member` running `ops`.
fn member_program(turn: &Turn, ops: &[Op]) -> IntProgram {
    let strings: &[&[u8]] = &[b"x"];
    let hook = HOOK_CALCAPCOST;
    let member = turn.member.as_str();
    if member.starts_with("hs_") {
        return program(strings, ops, &[]);
    }
    let start = match member {
        "gl_a" | "gl_b" => vec![I(hook), I(2), REGISTER_PROC],
        "gl_c" | "gl_d" => vec![I(hook), I(2), REGISTER_SPEC],
        "alpha:word" => vec![I(hook), I(2), W(REGISTER)],
        _ => Vec::new(),
    };
    program(strings, &start, &[("m", ops)])
}
/// The mods the turns of a case name, `alpha` then `beta`.
fn case_mods(queries: &[&[&Turn]]) -> Vec<ScriptedMod> {
    ["alpha", "beta"]
        .iter()
        .filter_map(|id| {
            let mut hooks = Vec::new();
            let mut globals = Vec::new();
            for turn in queries.iter().flat_map(|query| query.iter()) {
                let name = &turn.member;
                if turn.block != "mod" || !name.starts_with(id) {
                    continue;
                }
                let file = file_of(name);
                if name.ends_with(":word") {
                    globals.push(file);
                } else if !hooks.iter().any(|row: &ModHook| row.file == file) {
                    hooks.push(row(HOOK_CALCAPCOST, &file, "m"));
                }
            }
            if hooks.is_empty() && globals.is_empty() {
                return None;
            }
            let globals: Vec<&str> = globals.iter().map(String::as_str).collect();
            Some(mod_of(id, &globals, hooks))
        })
        .collect()
}
/// Each query of case group `name`: `O5` is `O5a` then `O5b`.
fn run_group(name: &str, rows: &[Turn]) {
    let in_group: Vec<&Turn> = rows
        .iter()
        .filter(|turn| turn.case.trim_end_matches(['a', 'b']) == name)
        .collect();
    let mut cases: Vec<&str> = in_group.iter().map(|turn| turn.case.as_str()).collect();
    cases.dedup();
    let queries: Vec<Vec<&Turn>> = cases
        .iter()
        .map(|case| {
            let query = in_group.iter().filter(|turn| turn.case == *case);
            query.copied().collect()
        })
        .collect();
    let bodies = |query: &[&Turn]| {
        let mut previous = None;
        let programs: Vec<(String, IntProgram)> = query
            .iter()
            .map(|turn| {
                let ops = body(turn, previous);
                previous = turn.ret.or(previous);
                (file_of(&turn.member), member_program(turn, &ops))
            })
            .collect();
        programs
    };
    let mut host = host(in_group[0].sfall);
    for (file, program) in bodies(&queries[0]) {
        install(&mut host, &file, program);
    }
    let slices: Vec<&[&Turn]> = queries.iter().map(Vec::as_slice).collect();
    let mods = case_mods(&slices);
    if !mods.is_empty() {
        enable(&mut host, mods);
    } else {
        set_up(&mut host);
    }
    let ids = host.hooks().unwrap().files.clone();
    for (index, query) in queries.iter().enumerate() {
        if index > 0 {
            for (file, program) in bodies(query) {
                if let Some(id) = ids.get(file.as_bytes()) {
                    host.replace_program_for_test(*id, program);
                }
            }
        }
        host.hooks().unwrap().turns.clear();
        diagnostics(&mut host);
        let cost = host.calc_ap_cost(101, 0, false, 5, 0);
        let ran = std::mem::take(&mut host.hooks().unwrap().turns);
        let expected: Vec<&&Turn> = query.iter().filter(|t| t.outcome != "skipped").collect();
        assert_eq!(ran.len(), expected.len(), "{}: turns", query[0].case);
        for (turn, (program, arg3, ret0)) in expected.iter().zip(ran) {
            let at = format!("{} {}", turn.case, turn.member);
            let file = file_of(&turn.member);
            assert_eq!(Some(&program), ids.get(file.as_bytes()), "{at}");
            assert_eq!(arg3, Some(SfallValue::Int(turn.after)), "{at}");
            assert_eq!(ret0, turn.ret.map(SfallValue::Int), "{at}");
        }
        let problems = diagnostics(&mut host);
        let has = |word: &str| problems.iter().any(|line| line.contains(word));
        for turn in query {
            match turn.outcome.as_str() {
                "vm-error" => assert!(has("unloaded"), "{problems:?}"),
                "suspended" => assert!(has("ends this member"), "{problems:?}"),
                "return-unconverted" => assert!(has("cannot take"), "{problems:?}"),
                _ => {}
            }
        }
        let last = query.last().unwrap();
        assert_eq!(Some(cost), last.result, "{}", last.case);
    }
}
/// §10 test 1: every row of `hook-order.tsv`, on the engine's chain.
#[test]
fn t1_the_order_vectors_o1_to_o11() {
    let rows = turns();
    let names = [
        "O1", "O2", "O3", "O4", "O5", "O6", "O7", "O8", "O9", "O10", "O11",
    ];
    for name in names {
        run_group(name, &rows);
    }
    let covered: usize = rows.len();
    assert_eq!(covered, ORDER.lines().count() - 1);
}

/// One member of a rev4 row: it writes ret0 when the row's value changes.
fn rev4_body(value: &str, changed: bool, outcome: &str, sfall: bool) -> Vec<Op> {
    if !changed && outcome == "ok" {
        return Vec::new();
    }
    let number = value.strip_prefix("0x").map_or_else(
        || value.parse().ok(),
        |hex| i32::from_str_radix(hex, 16).ok(),
    );
    let written = match (outcome, number) {
        ("return-unconverted", _) => I(7),
        (_, Some(number)) => I(number),
        (_, None) => S(b"Old"),
    };
    if sfall {
        vec![written, SET_RETURN]
    } else {
        vec![I(0), written, W(RETURN)]
    }
}
fn art(host: &mut WorldHost<TestAssets>, fids: &[u32]) {
    host.assets.art_policy = ArtPolicy::Loads;
    for fid in fids {
        let frm = vault_fixtures::world::frm(1, 1, 1);
        let frm = vault_formats::FrmArchive::parse(&frm).unwrap();
        host.assets.art.insert(*fid, Arc::new(frm));
    }
}
/// §10 test 1: `hook-order-rev4.tsv`, each through its point's boundary.
#[test]
fn t1_the_order_vectors_o12_to_o15() {
    let rows: Vec<Vec<&str>> = ORDER_REV4
        .lines()
        .skip(1)
        .map(|line| line.split('\t').collect())
        .collect();
    assert_eq!(rows.len(), 10);
    for case in ["O12", "O13", "O14", "O15"] {
        let turns: Vec<&Vec<&str>> = rows.iter().filter(|row| row[0] == case).collect();
        let hook = hook_point(turns[0][1]).unwrap();
        let sfall = turns.iter().any(|row| row[4] == "sfall");
        let mut host = host(sfall);
        let mut mods = Vec::new();
        let mut previous = "unset";
        for turn in &turns {
            let (member, block, outcome, ret) = (turn[3], turn[4], turn[5], turn[6]);
            let ops = rev4_body(ret, ret != previous, outcome, block == "sfall");
            previous = ret;
            let file = file_of(member);
            if block == "sfall" {
                install(&mut host, &file, program(&[b"Old"], &ops, &[]));
            } else {
                install(&mut host, &file, program(&[b"Old"], &[], &[("m", &ops)]));
                mods.push(mod_of(member, &[], vec![row(hook, &file, "m")]));
            }
        }
        enable(&mut host, mods);
        let ids = host.hooks().unwrap().files.clone();
        art(&mut host, &[0x0100_0001, 0x0100_000c, 0x0100_000e]);
        let player = host.world.player;
        host.hooks().unwrap().turns.clear();
        let result = match case {
            "O12" | "O13" => {
                host.sfall_figure(0x0100_0001);
                format!("{:#010X}", host.sfall_figure_shown(0x0100_0001)).replace("0X", "0x")
            }
            "O14" => {
                let object = spawn_npc(&mut host);
                host.world.get_mut(object).unwrap().description = b"base text".to_vec();
                host.world.effects.clear();
                host.action(player, object, ObjectAction::Description)
                    .unwrap();
                let shown = host.world.effects.iter().find_map(|effect| match effect {
                    crate::WorldEffect::Message(text) => Some(text.clone()),
                    _ => None,
                });
                format!("\"{}\"", String::from_utf8_lossy(&shown.unwrap()))
            }
            _ => {
                let skipped = host.hook_turn_start(player, false);
                (if skipped { "turn skipped" } else { "turn runs" }).to_owned()
            }
        };
        let ran = std::mem::take(&mut host.hooks().unwrap().turns);
        assert_eq!(ran.len(), turns.len(), "{case}");
        for (turn, (member, _, ret0)) in turns.iter().zip(ran) {
            let file = file_of(turn[3]);
            assert_eq!(Some(&member), ids.get(file.as_bytes()), "{case}");
            let shown = match ret0 {
                Some(SfallValue::Int(value)) if case != "O15" => format!("{value:#010X}"),
                Some(SfallValue::Int(value)) => value.to_string(),
                Some(SfallValue::Text(text)) => {
                    format!("\"{}\"", String::from_utf8_lossy(&text))
                }
                other => format!("{other:?}"),
            };
            assert_eq!(shown.replace("0X", "0x"), turn[6], "{case} {}", turn[3]);
        }
        assert_eq!(result, turns.last().unwrap()[8], "{case}");
    }
}

/// §9.1's program, from its `OFFSET: bytes` lines.
fn fixture_bytes() -> Vec<u8> {
    PROGRAM
        .lines()
        .filter_map(|line| line.split_once(": "))
        .flat_map(|(_, bytes)| bytes.split(' '))
        .map(|byte| u8::from_str_radix(byte, 16).unwrap())
        .collect()
}
/// `manifest` as the platform maps it.
fn scripted(manifest: &str) -> ScriptedMod {
    let parsed = ModManifest::parse(manifest).unwrap();
    ScriptedMod {
        id: parsed.id.clone().unwrap(),
        version: parsed.version.clone().unwrap_or_default(),
        manifest: fnv1a64(manifest.as_bytes()),
        globals: parsed
            .global_scripts
            .iter()
            .map(|script| ModGlobal {
                file: script.file.clone(),
                repeat: script.repeat as i32,
                kind: script.kind as i32,
            })
            .collect(),
        hooks: parsed
            .hooks
            .iter()
            .map(|hook| {
                row(
                    hook_point(&hook.point).unwrap(),
                    &hook.file,
                    &hook.procedure,
                )
            })
            .collect(),
        defaults: parsed
            .settings
            .iter()
            .map(|(key, setting)| {
                let value = match setting {
                    ModSetting::Int(value) => SfallValue::Int(*value),
                    ModSetting::Float(value) => SfallValue::float(*value),
                    ModSetting::Text(text) => SfallValue::text(text.as_bytes()),
                };
                (key.clone(), value)
            })
            .collect(),
    }
}
/// A host with `manifest` and §9.1's program, set up.
fn cheap_ap(manifest: &str, sfall: bool) -> (WorldHost<TestAssets>, Arc<ModSession>) {
    let mut host = host(sfall);
    let bytes = fixture_bytes();
    assert_eq!(bytes.len(), 250);
    let loaded = IntProgram::parse_in(&bytes, &words(), false).unwrap();
    install(&mut host, "gl_cheapap.int", loaded);
    let session = enable(&mut host, vec![scripted(manifest)]);
    (host, session)
}
/// §10 test 2.
#[test]
fn t2_the_fixture_program_through_the_loader_on_n1_and_n2() {
    for manifest in [N1, N2] {
        for sfall in [false, true] {
            let (mut host, session) = cheap_ap(manifest, sfall);
            let player = host.world.player;
            assert_eq!(host.members_for_test(HOOK_CALCAPCOST), 1, "one member");
            assert_eq!(host.hook_fixed_cost(player, 5, 0), 1);
            assert!(session.restore(0, "ap_cost", SfallValue::Int(3)));
            assert_eq!(host.hook_fixed_cost(player, 5, 0), 3);
        }
    }
    assert_eq!(fnv1a64(N1.as_bytes()), 0xEE7C_3851_3A26_2548);
    assert_eq!(N1.len(), 264);
    let every = WordSet {
        words: 0x8000..=0xffff,
        mods: false,
    };
    let decoded = IntProgram::parse_in(&fixture_bytes(), &every, false).unwrap();
    for table in [
        WordSet::FALLOUT1,
        WordSet::FALLOUT2,
        WordSet::FALLOUT2_SFALL,
    ] {
        let table = stack_check::Table::new(table.with_mods(true));
        let findings = stack_check::check("gl_cheapap.int", &decoded, &table, &[]);
        assert!(findings.is_empty(), "{findings:?}");
    }
}
/// §10 test 3, with sfall mode off: one counter, and the exception.
#[test]
fn t3_vector_d1_eight_chains_run_and_nine_writes_store() {
    let mut host = host(false);
    let hook = HOOK_SETGLOBALVAR;
    host.world.gvars = Some(vec![0; 16]);
    let body = [I(0), W(ARG), I(1), ADD, I(1), W(SET_GLOBAL_VAR)];
    install(&mut host, "nest.int", program(&[], &[], &[("m", &body)]));
    enable(
        &mut host,
        vec![mod_of("nest", &[], vec![row(hook, "nest.int", "m")])],
    );
    diagnostics(&mut host);
    let done = run_as(&mut host, &[I(0), I(1), W(SET_GLOBAL_VAR)], &words(), 50);
    assert_eq!(done.result, Ok(RunOutcome::FragmentEnd));
    let stored = host.world.gvars.clone().unwrap();
    assert_eq!(stored[..10], [1, 1, 1, 1, 1, 1, 1, 1, 1, 0]);
    let skipped = diagnostics(&mut host);
    let past = skipped.iter().filter(|line| line.contains("past 8 nested"));
    assert_eq!(past.count(), 1, "{skipped:?}");
}

/// The word vectors with no chain, by §9.4's table. Stacks run bottom first.
#[test]
fn t4_the_word_vectors_outside_a_chain() {
    let (mut host, session) = cheap_ap(N1, false);
    let owner = host.hooks().unwrap().files[&b"gl_cheapap.int".to_vec()];
    let set = words();
    let word = |host: &mut WorldHost<TestAssets>, ops: &[Op], id: u32| {
        diagnostics(host);
        let run = run_as(host, ops, &set, id);
        let found = diagnostics(host).len();
        (run, found)
    };
    let key: &[u8] = b"ap_cost";
    let (w1, _) = word(&mut host, &[W(API)], owner);
    assert_eq!(w1.ints(), [1], "W1");
    let w2 = [S(key), I(9), W(GET_SETTING)];
    assert_eq!(word(&mut host, &w2, owner).0.ints(), [1], "W2");
    let (w3, found) = word(&mut host, &[S(b"absent"), I(9), W(GET_SETTING)], owner);
    assert_eq!((w3.ints(), found), (vec![9], 1), "W3");
    let (w3, found) = word(&mut host, &[S(b"absent"), I(9), W(GET_SETTING)], owner);
    assert_eq!(
        (w3.ints(), found),
        (vec![9], 0),
        "one diagnostic per program"
    );
    let (w4, found) = word(&mut host, &[I(7), I(9), W(GET_SETTING)], owner);
    assert_eq!((w4.ints(), found), (vec![0], 1), "W4");
    let (w5, found) = word(&mut host, &w2, 77);
    assert_eq!((w5.ints(), found), (vec![9], 1), "W5");
    let (w6, _) = word(&mut host, &[S(key), I(3), W(SET_SETTING)], owner);
    assert!(w6.ints().is_empty(), "W6");
    assert_eq!(word(&mut host, &w2, owner).0.ints(), [3], "W6 then W2");
    word(&mut host, &[S(key), Op::F(2.5), W(SET_SETTING)], owner);
    assert_eq!(word(&mut host, &w2, owner).0.ints(), [2], "W7");
    let (_, found) = word(&mut host, &[S(key), S(b"x"), W(SET_SETTING)], owner);
    assert_eq!(found, 1, "W8");
    assert_eq!(
        word(&mut host, &w2, owner).0.ints(),
        [2],
        "W8 changed nothing"
    );
    word(&mut host, &[S(b"hi"), W(LOG)], owner);
    assert_eq!(session.log_lines().last().unwrap(), "cheap-ap: hi", "W9");
    // N1's `start` registered, so W10's "nothing registered" follows a removal.
    word(&mut host, &[I(2), I(0), W(REGISTER)], owner);
    assert_eq!(host.members_for_test(HOOK_CALCAPCOST), 0);
    let (_, found) = word(&mut host, &[I(2), I(0), W(REGISTER)], owner);
    assert_eq!(found, 0, "W10: no effect, and no diagnostic");
    assert_eq!(host.members_for_test(HOOK_CALCAPCOST), 0, "W10");
    let (_, found) = word(&mut host, &[I(50), I(2), W(REGISTER)], owner);
    assert_eq!(found, 1, "W11");
    assert_eq!(word(&mut host, &[W(HOOK_ID)], owner).0.ints(), [-1], "W12");
    assert_eq!(word(&mut host, &[I(3), W(ARG)], owner).0.ints(), [0], "W13");
    assert_eq!(word(&mut host, &[W(MODE)], owner).0.ints(), [2], "W18");
    let (w19, _) = word(&mut host, &[W(LOG)], owner);
    assert_eq!(w19.result, Err(VmError::Underflow), "W19");
    // W5's line for a caller with no owner, and §7.5's bytes.
    word(&mut host, &[S(b"a\x01b"), W(LOG)], 79);
    assert_eq!(session.log_lines().last().unwrap(), "-: a?b");
    let long: &[u8] = &[b'x'; 250];
    word(&mut host, &[S(long), W(LOG)], owner);
    assert_eq!(session.log_lines().last().unwrap().len(), 210, "cut to 200");
    for _ in 0..300 {
        word(&mut host, &[S(b"again"), W(LOG)], owner);
    }
    assert_eq!(session.log_lines().len(), 256, "the newest 256");
}
/// W14 to W17 in O1's chain, and W20 to W22 in O14's and O12's.
#[test]
fn t4_the_word_vectors_inside_a_chain() {
    let hook = HOOK_CALCAPCOST;
    let chain = |target: i32, body: &[Op], args: Vec<SfallValue>| {
        let mut host = host(false);
        install(&mut host, "m.int", program(&[b"Old"], &[], &[("m", body)]));
        enable(
            &mut host,
            vec![mod_of("m", &[], vec![row(target, "m.int", "m")])],
        );
        diagnostics(&mut host);
        let rets = host.run_hook(target, args).unwrap();
        (rets, diagnostics(&mut host).len())
    };
    let args: Vec<SfallValue> = [101, 0, 0, 5, 0].map(SfallValue::Int).to_vec();
    let int = |value| Some(SfallValue::Int(value));
    let w14 = chain(hook, &[I(0), I(3), W(ARG), W(RETURN)], args.clone());
    assert_eq!(w14.0, [int(5)], "W14");
    let w15 = [I(3), I(4), W(SET_HOOK_ARG), I(0), I(3), W(ARG), W(RETURN)];
    assert_eq!(chain(hook, &w15, args.clone()).0, [int(4)], "W15");
    assert_eq!(
        chain(hook, &[I(0), I(2), W(RETURN)], args.clone()).0,
        [int(2)],
        "W16"
    );
    assert_eq!(
        chain(hook, &[I(1), I(2), W(RETURN)], args.clone()).0,
        [None],
        "W17"
    );
    let text = crate::host_sfall_boundaries::HOOK_DESCRIPTIONOBJ;
    let old = Some(SfallValue::text(&b"Old"[..]));
    let object = vec![SfallValue::Int(1)];
    let w20 = [I(0), S(b"Old"), W(RETURN)];
    assert_eq!(
        chain(text, &w20, object.clone()).0,
        vec![old.clone()],
        "W20"
    );
    let w21 = [I(0), S(b"Old"), W(RETURN), I(0), I(7), W(RETURN)];
    assert_eq!(chain(text, &w21, object), (vec![old], 1), "W21");
    let w22 = [I(0), W(HOOK_ID), W(RETURN)];
    let figure: Vec<SfallValue> = [1, 1].map(SfallValue::Int).to_vec();
    assert_eq!(chain(HOOK_ADJUSTFID, &w22, figure).0, [int(26)], "W22");
}
/// §9.4's load-time vectors.
#[test]
fn t4_the_load_time_vectors() {
    let load = |word: u16, set: &WordSet| {
        let mut bytes =
            crate::fixture_state::program(&[], &[(b"body", word.to_be_bytes().to_vec())]);
        bytes.extend(0x8000u16.to_be_bytes());
        IntProgram::parse_in(&bytes, set, false).is_ok()
    };
    let (off, on, sfall) = (
        WordSet::FALLOUT2,
        WordSet::FALLOUT2.with_mods(true),
        WordSet::FALLOUT2_SFALL.with_mods(true),
    );
    for (word, in_off, in_on, in_sfall) in [
        (0x8300, false, true, true),
        (0x8309, false, true, true),
        (0x830a, false, false, false),
        (0x83ff, false, false, false),
        (0x8281, false, false, true),
    ] {
        let found = (load(word, &off), load(word, &on), load(word, &sfall));
        assert_eq!(found, (in_off, in_on, in_sfall), "{word:#x}");
    }
    assert!(load(0x8300, &WordSet::FALLOUT1.with_mods(true)));
    // `opcode_exists` reports SPEC-024b's rows, and `engine_api_version`
    // keeps its own version.
    let table = stack_check::Table::new(WordSet::FALLOUT2.with_mods(true));
    for line in WORDS.lines().skip(1) {
        let cells: Vec<&str> = line.split('\t').collect();
        let word = u16::from_str_radix(&cells[0][2..], 16).unwrap();
        let row = table.row(word).unwrap();
        assert_eq!(row.operands.len().to_string(), cells[2], "{line}");
        assert_eq!(usize::from(row.name == cells[1]), 1, "{line}");
    }
    assert!(table.row(0x830a).is_none());
}
/// N13 and N14: accepted, one diagnostic at setup, and nothing runs.
#[test]
fn t4_a_row_with_no_boundary_or_no_procedure_runs_nothing() {
    let n13 = N2.replace("HOOK_CALCAPCOST", "HOOK_ONDEATH");
    let (mut host, _) = cheap_ap(&n13, false);
    let setup = diagnostics(&mut host);
    assert_eq!(
        setup
            .iter()
            .filter(|line| line.contains("no boundary"))
            .count(),
        1
    );
    assert_eq!(host.run_hook(6, vec![SfallValue::Int(1)]), None, "N13");
    let n14 = N2.replace("procedure = \"ap_cost\"", "procedure = \"absent\"");
    let (mut host, _) = cheap_ap(&n14, false);
    let setup = diagnostics(&mut host);
    assert_eq!(
        setup.iter().filter(|line| line.contains("inert")).count(),
        1
    );
    let player = host.world.player;
    assert_eq!(host.hook_fixed_cost(player, 5, 0), 5, "N14: nothing runs");
}
/// §10 test 7, and §1's third invariant, over all 30 points.
#[test]
fn t7_a_mod_that_writes_nothing_changes_no_hash_column() {
    let body: &[Op] = &[];
    for sfall in [false, true] {
        let plain = crate::spec024_hook_tests::sequence_with(body, false, sfall, false);
        let attached = crate::spec024_hook_tests::sequence_with(body, false, sfall, true);
        assert_eq!(attached, plain, "sfall {sfall}");
    }
}
/// §10 test 8: a member that draws, on one platform here; CI runs the rest.
#[test]
fn t8_two_runs_of_a_member_that_draws_give_identical_streams() {
    let body: &[Op] = &[I(1), I(100), W(0x80b4), DISCARD];
    let first = crate::spec024_hook_tests::sequence_with(body, false, false, true);
    assert_eq!(
        first,
        crate::spec024_hook_tests::sequence_with(body, false, false, true)
    );
    let quiet = crate::spec024_hook_tests::sequence_with(&[], false, false, true);
    assert_ne!(first.0, quiet.0, "the draws move the gameplay sequence");
}
/// §10 test 12's core half: both blocks, the final FID, and no player art.
#[test]
fn t12_the_figure_takes_the_final_fid_and_only_on_a_change() {
    let mut host = host(true);
    let write = |fid: i32| vec![I(0), I(fid), W(RETURN)];
    install(
        &mut host,
        "hs_adjustfid.int",
        program(&[], &[I(0x0100_000c), SET_RETURN], &[]),
    );
    install(
        &mut host,
        "m.int",
        program(&[], &[], &[("m", &write(0x0100_000e))]),
    );
    enable(
        &mut host,
        vec![mod_of("m", &[], vec![row(HOOK_ADJUSTFID, "m.int", "m")])],
    );
    art(&mut host, &[0x0100_0001, 0x0100_000c, 0x0100_000e]);
    let player = host.world.player;
    let figure = host.world.get(player).unwrap().raw.art_id;
    host.hooks().unwrap().turns.clear();
    assert_eq!(host.sfall_figure(0x0100_0001), Some(0x0100_000e));
    assert_eq!(host.hooks().unwrap().turns.len(), 2, "both blocks");
    assert_eq!(host.sfall_figure(0x0100_0001), Some(0x0100_000e));
    assert_eq!(host.hooks().unwrap().turns.len(), 2, "the same inputs");
    assert_eq!(host.world.get(player).unwrap().raw.art_id, figure);
}
/// §10 test 12's other half: neither block sees the withdrawn types.
#[test]
fn t12_ammo_cost_type_1_and_perception_type_2_run_neither_block() {
    for hook in [HOOK_AMMOCOST, HOOK_WITHINPERCEPTION] {
        let (mut plain, [attacker, _, defender, _]) = crate::host_ai_tests::fixture_e("critter", 2);
        plain.assets.programs.clear();
        let body = [I(0), I(3), W(ARG), W(RETURN)];
        install(&mut plain, "m.int", program(&[], &[], &[("m", &body)]));
        enable(
            &mut plain,
            vec![mod_of("m", &[], vec![row(hook, "m.int", "m")])],
        );
        plain.hooks().unwrap().turns.clear();
        let right = crate::spec024_hook_tests::RIGHT;
        plain
            .perform_attack(attacker, defender, right, 8, false)
            .unwrap()
            .unwrap();
        plain.perceives(attacker, defender, 0);
        for (_, arg3, _) in std::mem::take(&mut plain.hooks().unwrap().turns) {
            let kind = arg3.unwrap().int();
            assert_ne!(kind, if hook == HOOK_AMMOCOST { 1 } else { 2 });
        }
    }
}
/// §5.4: membership is fixed at the query.
#[test]
fn t13_a_registration_made_during_a_chain_counts_from_the_next_query() {
    let hook = HOOK_CALCAPCOST;
    let mut host = host(true);
    let first = [I(0), I(1), W(RETURN), I(hook), I(3), W(REGISTER)];
    let second = [I(0), I(2), W(RETURN)];
    let scripted = program(&[], &[], &[("one", &first), ("two", &second)]);
    install(&mut host, "m.int", scripted);
    enable(
        &mut host,
        vec![mod_of("m", &[], vec![row(hook, "m.int", "one")])],
    );
    assert_eq!(
        host.calc_ap_cost(101, 0, false, 5, 0),
        1,
        "the old procedure"
    );
    assert_eq!(
        host.calc_ap_cost(101, 0, false, 5, 0),
        2,
        "from the next query"
    );
    assert_eq!(host.members_for_test(hook), 1, "and it kept its place");
}
/// §5.4: a member that left during this query is passed over at its turn.
#[test]
fn t13_a_member_that_left_during_a_chain_is_skipped_at_its_turn() {
    let hook = HOOK_CALCAPCOST;
    let mut host = host(false);
    host.world.gvars = Some(vec![0; 4]);
    let first = [I(0), I(1), W(RETURN), I(0), I(1), W(SET_GLOBAL_VAR)];
    install(&mut host, "a.int", program(&[], &[], &[("m", &first)]));
    let bad = [W(0x8252)];
    let late = [I(0), I(9), W(RETURN)];
    install(
        &mut host,
        "b.int",
        program(&[], &[], &[("bad", &bad), ("late", &late)]),
    );
    enable(
        &mut host,
        vec![
            mod_of("a", &[], vec![row(hook, "a.int", "m")]),
            mod_of(
                "b",
                &[],
                vec![
                    row(hook, "b.int", "late"),
                    row(HOOK_SETGLOBALVAR, "b.int", "bad"),
                ],
            ),
        ],
    );
    assert_eq!(
        host.calc_ap_cost(101, 0, false, 5, 0),
        1,
        "b never got its turn"
    );
    assert_eq!(
        host.members_for_test(HOOK_SETGLOBALVAR),
        0,
        "it left every chain"
    );
    assert_eq!(host.members_for_test(hook), 1);
}
/// §5.4: a program running on an outer chain is skipped, and the chain goes
/// on. Its own `set_global_var` is the exception, which D1 pins.
#[test]
fn t13_a_member_already_running_is_skipped_with_a_diagnostic() {
    let hook = HOOK_CALCAPCOST;
    let lighting = crate::host_sfall_boundaries::HOOK_SETLIGHTING;
    let mut host = host(false);
    let object = spawn_npc(&mut host);
    let outer = [
        I(0),
        I(1),
        W(RETURN),
        I(object as i32),
        I(50),
        I(4),
        W(0x8107),
    ];
    let inner = [I(0), I(0), W(RETURN)];
    let other = [I(0), I(123), W(RETURN)];
    install(
        &mut host,
        "a.int",
        program(&[], &[], &[("outer", &outer), ("inner", &inner)]),
    );
    install(&mut host, "b.int", program(&[], &[], &[("m", &other)]));
    enable(
        &mut host,
        vec![
            mod_of(
                "a",
                &[],
                vec![row(hook, "a.int", "outer"), row(lighting, "a.int", "inner")],
            ),
            mod_of("b", &[], vec![row(lighting, "b.int", "m")]),
        ],
    );
    diagnostics(&mut host);
    assert_eq!(host.calc_ap_cost(101, 0, false, 5, 0), 1);
    let light = host.world.get(object).unwrap().raw.light;
    assert_eq!(light.intensity, 123, "b ran after a was skipped");
    let lines = diagnostics(&mut host);
    assert!(
        lines.iter().any(|line| line.contains("is running")),
        "{lines:?}"
    );
}
/// §5.3: the instruction limit is a VM error, and a member that erred
/// outside a chain has left its chains the same way.
#[test]
fn t13_running_out_of_instructions_and_erring_elsewhere_leave_every_chain() {
    let hook = HOOK_CALCAPCOST;
    let mut host = host(false);
    let probe = program(&[], &[], &[("m", &[I(0), W(0x8004)])]);
    let at = probe.procedures()[2].body as i32;
    let forever = [I(at), W(0x8004)];
    let scripted = program(&[], &[], &[("m", &forever)]);
    install(&mut host, "a.int", scripted);
    let after = [I(0), I(4), W(RETURN)];
    install(&mut host, "b.int", program(&[], &[], &[("m", &after)]));
    let bad = [W(0x8252)];
    let fine = [I(0), I(6), W(RETURN)];
    let events = program(&[], &[], &[("m", &fine), ("map_update_p_proc", &bad)]);
    install(&mut host, "c.int", events);
    enable(
        &mut host,
        vec![
            mod_of("a", &[], vec![row(hook, "a.int", "m")]),
            mod_of("b", &[], vec![row(hook, "b.int", "m")]),
            mod_of("c", &["c.int"], vec![row(COMBAT, "c.int", "m")]),
        ],
    );
    assert_eq!(
        host.calc_ap_cost(101, 0, false, 5, 0),
        4,
        "the chain went on"
    );
    assert_eq!(host.members_for_test(hook), 1, "it left its chains");
    assert_eq!(host.members_for_test(COMBAT), 1);
    host.sfall_global_event(b"map_update_p_proc");
    assert_eq!(host.members_for_test(COMBAT), 0);
}
const COMBAT: i32 = HOOK_COMBATDAMAGE;
/// §4.3: a `gl` file a manifest names is a manifest script only, and global
/// scripts run sfall's first, then each mod's.
#[test]
fn global_scripts_load_once_in_their_order() {
    let mut host = host(true);
    let note = |digit: i32| {
        [
            I(3),
            I(3),
            W(0x819e),
            I(10),
            W(0x803b),
            I(digit),
            ADD,
            W(0x819d),
        ]
    };
    install(
        &mut host,
        "gl_b.int",
        program(&[], &note(2), &[("map_enter_p_proc", &note(5))]),
    );
    install(
        &mut host,
        "gl_m.int",
        program(&[], &note(3), &[("map_enter_p_proc", &note(6))]),
    );
    install(
        &mut host,
        "other.int",
        program(&[], &note(4), &[("map_enter_p_proc", &note(7))]),
    );
    enable(
        &mut host,
        vec![mod_of("m", &["gl_m.int", "other.int"], Vec::new())],
    );
    assert_eq!(crate::spec024_hook_tests::global_of(&host, 3), 234);
    host.sfall_global_event(b"map_enter_p_proc");
    assert_eq!(crate::spec024_hook_tests::global_of(&host, 3), 234_567);
}
/// §3.2 and §10 test 15: no boundary for these under `fallout1`.
#[test]
fn t15_fallout1_runs_neither_fallout2_point() {
    let world = WorldState::new(CharacterSheet::default(), 1).unwrap();
    let mut host = WorldHost::new(world, TestAssets::all_art());
    let body = [I(0), I(1), W(RETURN)];
    let scripted = script_in(
        &WordSet::FALLOUT1.with_mods(true),
        &[],
        &[],
        &[("m", &body)],
    );
    install(&mut host, "m.int", scripted);
    let rows = vec![
        row(HOOK_CARTRAVEL, "m.int", "m"),
        row(HOOK_ENCOUNTER, "m.int", "m"),
        row(HOOK_CALCAPCOST, "m.int", "m"),
    ];
    enable(&mut host, vec![mod_of("m", &[], rows)]);
    assert_eq!(host.members_for_test(HOOK_CARTRAVEL), 1, "the row is kept");
    assert_eq!(host.run_hook(HOOK_CARTRAVEL, vec![]), None);
    assert_eq!(host.run_hook(HOOK_ENCOUNTER, vec![]), None);
    assert_eq!(host.calc_ap_cost(101, 0, false, 5, 0), 1, "the others fire");
}
/// §3.2: with sfall mode off the mode comparison still runs.
#[test]
fn the_game_mode_chain_runs_with_sfall_mode_off() {
    let mut host = host(false);
    host.world.gvars = Some(vec![0; 4]);
    let write = [I(1), W(MODE), W(SET_GLOBAL_VAR)];
    install(&mut host, "m.int", program(&[], &[], &[("m", &write)]));
    enable(
        &mut host,
        vec![mod_of(
            "m",
            &[],
            vec![row(HOOK_GAMEMODECHANGE, "m.int", "m")],
        )],
    );
    host.sfall_end_of_step();
    assert_eq!(host.world.gvars.as_ref().unwrap()[1], 0, "nothing changed");
    host.set_sfall_screens(crate::host_sfall::game_mode::INVENTORY);
    host.sfall_end_of_step();
    let bits = crate::host_sfall::game_mode::INVENTORY | crate::host_sfall::game_mode::LOCAL_MAP;
    assert_eq!(host.world.gvars.as_ref().unwrap()[1], bits as i32);
}
/// §10 test 14.
#[test]
fn t14_the_points_are_the_oracle_rows_and_fire_exactly_for_tier_p1() {
    let oracle: Vec<Vec<&str>> = ORACLE_HOOKS
        .lines()
        .skip(1)
        .map(|line| line.split('\t').collect())
        .collect();
    let points: Vec<Vec<&str>> = POINTS
        .lines()
        .skip(1)
        .map(|line| line.split('\t').collect())
        .collect();
    assert_eq!(points.len(), oracle.len());
    assert_eq!(points.len(), 51);
    for (point, row) in points.iter().zip(&oracle) {
        assert_eq!(point[..2], row[..2], "{row:?}");
        assert_eq!(point[3] == "yes", row[3] == "P1", "{row:?}");
        assert_eq!((point[5], point[6]), (row[5], row[7]), "{row:?}");
    }
    assert_eq!(points.iter().filter(|point| point[3] == "yes").count(), 30);
    let fallout2 = points.iter().filter(|point| point[4] == "fallout2");
    let names: Vec<&str> = fallout2.map(|point| point[1]).collect();
    assert_eq!(names, ["HOOK_CARTRAVEL", "HOOK_ENCOUNTER"]);
}
/// A manifest script may call `register_hook`, which keeps SPEC-024b's block
/// (§4.2), and a sfall member and a mod member share one context (§5.2).
#[test]
fn the_word_chooses_the_block_and_both_families_share_a_context() {
    let hook = HOOK_CALCAPCOST;
    let mut host = host(true);
    let sfall_member = [I(0), I(1), W(RETURN), I(7), SET_RETURN];
    let start = [I(hook), I(2), REGISTER_PROC];
    let both = program(&[], &start, &[("m", &sfall_member)]);
    install(&mut host, "s.int", both);
    let last = [GET_ARG, DISCARD, I(1), W(RETURN)];
    install(&mut host, "t.int", program(&[], &[], &[("m", &last)]));
    enable(
        &mut host,
        vec![
            mod_of("t", &[], vec![row(hook, "t.int", "m")]),
            mod_of("s", &["s.int"], Vec::new()),
        ],
    );
    // `s`'s word put it in the sfall block, so it runs before `t`'s row,
    // though it is the higher layer. `t` writes by index and the cursor it
    // read with does not matter.
    host.hooks().unwrap().turns.clear();
    host.calc_ap_cost(101, 0, false, 5, 0);
    let ran = std::mem::take(&mut host.hooks().unwrap().turns);
    assert_eq!(ran.len(), 2);
    let files = host.hooks().unwrap().files.clone();
    assert_eq!(ran[0].0, files[&b"s.int".to_vec()]);
}
fn spawn_npc(host: &mut WorldHost<TestAssets>) -> ObjectRef {
    let prototype = crate::test_support::npc_prototype();
    host.world.spawn(prototype, 20000, 0).unwrap()
}
/// §4.3: a manifest's `repeat` and `type`, with sfall mode off.
#[test]
fn a_repeating_manifest_script_runs_its_start_every_n_steps() {
    let mut host = host(false);
    host.world.gvars = Some(vec![0; 4]);
    let count = [I(1), I(1), W(0x80c5), I(1), ADD, W(SET_GLOBAL_VAR)];
    install(&mut host, "tick.int", program(&[], &count, &[]));
    let mut scripted = mod_of("tick", &["tick.int"], Vec::new());
    scripted.globals[0].repeat = 2;
    enable(&mut host, vec![scripted]);
    let counted = |host: &WorldHost<TestAssets>| host.world.gvars.as_ref().unwrap()[1];
    assert_eq!(counted(&host), 1, "setup runs start once");
    for _ in 0..4 {
        host.sfall_end_of_step();
    }
    assert_eq!(counted(&host), 3, "then every second step");
}
/// §7.1: `opcode_exists` reports SPEC-024b's rows, not these words.
#[test]
fn opcode_exists_gives_0_for_the_mod_words() {
    let (mut host, _) = cheap_ap(N1, true);
    let ops = [S(b"opcode_exists"), I(0x8300), W(0x8277)];
    assert_eq!(run_as(&mut host, &ops, &words(), 50).ints(), [0]);
}
/// §4.2: a registration from a program with no owner is ignored.
#[test]
fn a_program_with_no_owner_cannot_register() {
    let (mut host, _) = cheap_ap(N1, false);
    diagnostics(&mut host);
    let before = host.members_for_test(HOOK_COMBATDAMAGE);
    let ops = [I(HOOK_COMBATDAMAGE), I(2), W(REGISTER)];
    run_as(&mut host, &ops, &words(), 77);
    assert_eq!(host.members_for_test(HOOK_COMBATDAMAGE), before);
    assert_eq!(diagnostics(&mut host).len(), 1);
}
/// §4.2: procedure 0 removes a row until the next setup.
#[test]
fn procedure_0_removes_a_row_until_the_next_setup() {
    let (mut host, _) = cheap_ap(N2, false);
    let owner = host.hooks().unwrap().files[&b"gl_cheapap.int".to_vec()];
    assert_eq!(host.members_for_test(HOOK_CALCAPCOST), 1);
    let ops = [I(HOOK_CALCAPCOST), I(0), W(REGISTER)];
    run_as(&mut host, &ops, &words(), owner);
    assert_eq!(host.members_for_test(HOOK_CALCAPCOST), 0);
    set_up(&mut host);
    assert_eq!(
        host.members_for_test(HOOK_CALCAPCOST),
        1,
        "setup rebuilt it"
    );
}
/// §5.2 and §6.2: a load rebuilds the registry and keeps the settings.
#[test]
fn a_load_rebuilds_the_chains_and_changes_no_setting() {
    let (mut host, session) = cheap_ap(N1, false);
    session.restore(0, "ap_cost", SfallValue::Int(3));
    host.sfall_restored();
    host.sfall_setup_if_due();
    let player = host.world.player;
    assert_eq!(host.members_for_test(HOOK_CALCAPCOST), 1);
    assert_eq!(host.hook_fixed_cost(player, 5, 0), 3);
}
