//! SPEC-021 §4 tests T7, T9 (through the binary) and T11, on synthetic data.
use std::{
    path::{Path, PathBuf},
    process::{Command, Output},
    sync::Arc,
};

use vault_core::traces::{
    CombatEvent, combat_trace_host, read_combat_trace, run_combat_trace, run_combat_trace_recorded,
    trace_defender, write_combat_trace,
};
use vault_fixtures::{gallery, state, vm};
use vault_formats::LocalDirProvider;

fn fixtures() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tests/fixtures")
}
fn golden(name: &str) -> String {
    std::fs::read_to_string(fixtures().join("spec021").join(name)).unwrap()
}
fn audit(arguments: &[&std::ffi::OsStr]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_asset-audit"))
        .args(arguments)
        .output()
        .unwrap()
}
fn stdout(output: &Output) -> String {
    String::from_utf8(output.stdout.clone()).unwrap()
}
/// The first directory `cargo xtask fixtures --output DIR` writes.
fn bare_corpus() -> tempfile::TempDir {
    let temp = tempfile::tempdir().unwrap();
    state::write_corpus(temp.path());
    gallery::write_hud(temp.path());
    std::fs::write(temp.path().join("scripts/core-fixture.int"), vm::program()).unwrap();
    temp
}
/// SPEC-021 §3.7: the AI fixture (revision 2) and the critter art fixture
/// (revision 3), copied byte for byte into a traced corpus.
fn install_trace_fixtures(corpus: &Path) {
    let rev2 = fixtures().join("spec021/rev2");
    for (from, to) in [
        ("ai.txt", "data/ai.txt"),
        ("combatai.msg", "text/english/game/combatai.msg"),
        ("sampleaq.frm", "art/critters/sampleaq.frm"),
        ("sampleao.frm", "art/critters/sampleao.frm"),
        ("sampleba.frm", "art/critters/sampleba.frm"),
        ("samplecj.frm", "art/critters/samplecj.frm"),
    ] {
        let to = corpus.join(to);
        std::fs::create_dir_all(to.parent().unwrap()).unwrap();
        std::fs::copy(rev2.join(from), &to).unwrap();
    }
}
/// [`bare_corpus`] with all six rows of SPEC-021 §3.7's table installed:
/// every seeded combat trace runs on this (T7 and T11).
fn corpus() -> tempfile::TempDir {
    let temp = bare_corpus();
    install_trace_fixtures(temp.path());
    temp
}

/// T9 through the binary: the `.hex` fixture, the raw branch mutation, an
/// error row, `--trace-out`, and the exclusive-mode usage errors.
#[test]
fn the_vm_trace_dump_writes_the_goldens() {
    let hex = fixtures().join("vm/program.int.hex");
    let output = audit(&["--dump-vm-trace".as_ref(), hex.as_os_str()]);
    assert_eq!(output.status.code(), Some(0), "{output:?}");
    assert_eq!(stdout(&output), golden("vm-trace-program.tsv"));
    assert!(!stdout(&output).contains("Audit categories"));

    let temp = tempfile::tempdir().unwrap();
    let mut bytes = vm::program();
    bytes[0xAE..0xB2].copy_from_slice(&[0, 0, 0, 1]);
    let raw = temp.path().join("PROGRAM.INT");
    std::fs::write(&raw, &bytes).unwrap();
    let out = temp.path().join("trace.tsv");
    let output = audit(&[
        "--dump-vm-trace".as_ref(),
        raw.as_os_str(),
        "--trace-out".as_ref(),
        out.as_os_str(),
    ]);
    assert_eq!(output.status.code(), Some(0), "{output:?}");
    assert!(output.stdout.is_empty());
    assert_eq!(
        std::fs::read_to_string(&out).unwrap(),
        golden("vm-trace-program-branch.tsv")
    );

    // A jump with an empty value stack is a VM error: one error row, exit 2.
    bytes[0..2].copy_from_slice(&[0x80, 0x04]);
    std::fs::write(&raw, &bytes).unwrap();
    let output = audit(&["--dump-vm-trace".as_ref(), raw.as_os_str()]);
    assert_eq!(output.status.code(), Some(2), "{output:?}");
    assert_eq!(
        stdout(&output),
        format!(
            "{}\n0\tprogram.int\t00000000\t8004\t-\t-\t-\t-\t-\t-\t-\terror\n",
            vault_core::traces::VM_TRACE_HEADER
        )
    );
    // The step bound: exit 2 without an extra row.
    let output = audit(&[
        "--dump-vm-trace".as_ref(),
        hex.as_os_str(),
        "--max-steps".as_ref(),
        "3".as_ref(),
    ]);
    assert_eq!(output.status.code(), Some(2), "{output:?}");
    assert_eq!(stdout(&output).lines().count(), 4);

    for extra in [
        &["--validate-formats"][..],
        &["--report-script-stack"],
        &["--dump-combat-trace", "maps/synthetic.map"],
        &["--seed", "3"],
    ] {
        let mut arguments = vec!["--dump-vm-trace".as_ref(), hex.as_os_str()];
        arguments.extend(extra.iter().map(|a| std::ffi::OsStr::new(*a)));
        let output = audit(&arguments);
        assert_eq!(output.status.code(), Some(1), "{extra:?}: {output:?}");
        assert!(output.stdout.is_empty(), "{extra:?}");
    }
    // A load error writes no trace.
    let missing = temp.path().join("missing.int");
    let output = audit(&["--dump-vm-trace".as_ref(), missing.as_os_str()]);
    assert_eq!(output.status.code(), Some(1));
    assert!(output.stdout.is_empty());
}

/// T11: the seeded combat golden on the synthetic corpus.
#[test]
fn the_seeded_combat_trace_matches_its_golden() {
    let corpus = corpus();
    let output = audit(&[
        "--corpus-dir".as_ref(),
        corpus.path().as_os_str(),
        "--dump-combat-trace".as_ref(),
        "maps/synthetic.map".as_ref(),
        "--seed".as_ref(),
        "1".as_ref(),
        "--max-rounds".as_ref(),
        "5".as_ref(),
    ]);
    assert_eq!(output.status.code(), Some(0), "{output:?}");
    let text = stdout(&output);
    assert_eq!(text, golden("combat-trace-synthetic-seed1.tsv"));
    let rows = read_combat_trace(&text).unwrap();
    assert_eq!(rows[0].event, CombatEvent::Start);
    // An NPC turn row: a turn ended by an actor other than the player's
    // `1:1`.
    assert!(rows.iter().any(|row| row.event == CombatEvent::Turn
        && row.actor.is_some_and(|actor| actor.to_string() != "1:1")));
    // Five complete rounds traced, no sixth round's row.
    assert!(rows.iter().all(|row| row.round.is_none_or(|r| r < 5)));
    assert_eq!(rows.iter().filter_map(|row| row.round).max(), Some(4));

    // Usage: no corpus, or a dump combined with a validation.
    let output = audit(&[
        "--dump-combat-trace".as_ref(),
        "maps/synthetic.map".as_ref(),
    ]);
    assert_eq!(output.status.code(), Some(1), "{output:?}");
    let output = audit(&[
        "--corpus-dir".as_ref(),
        corpus.path().as_os_str(),
        "--dump-combat-trace".as_ref(),
        "maps/synthetic.map".as_ref(),
        "--validate-formats".as_ref(),
    ]);
    assert_eq!(output.status.code(), Some(1), "{output:?}");
    assert!(output.stdout.is_empty());
}

/// T11, revisions 2 and 3: in process, the NPC acts. At the end of at
/// least one of the defender's traced turns its AP is below its effective
/// maximum, because it moved or attacked (SPEC-009 §3.12).
#[test]
fn the_defender_spends_action_points_in_the_seeded_trace() {
    let corpus = corpus();
    // Revision 3: the art fixture names base `sample`, the only row of the
    // corpus's critter list.
    assert_eq!(
        std::fs::read_to_string(corpus.path().join("art/critters/critters.lst"))
            .unwrap()
            .trim_end(),
        "sample,0"
    );
    let vfs = Arc::new(LocalDirProvider::mount(corpus.path()).unwrap());
    let mut host = combat_trace_host(vfs, "maps/synthetic.map", 1).unwrap();
    assert!(
        host.world
            .ai
            .packets
            .as_ref()
            .is_some_and(|table| !table.packets.is_empty())
    );
    let defender = trace_defender(&host.world).unwrap();
    // Its art identifier names base index 0 (`sample`), as the player's does.
    for critter in [defender, host.world.player] {
        assert_eq!(host.world.get(critter).unwrap().raw.art_id & 0xFFF, 0);
    }
    let defender = host.world.state_handle(defender);
    let player = host.world.state_handle(host.world.player);
    let recorder = run_combat_trace_recorded(&mut host, 5).unwrap();
    assert_eq!(
        write_combat_trace(&recorder.rows),
        golden("combat-trace-synthetic-seed1.tsv")
    );
    let turns: Vec<_> = recorder
        .turn_ap
        .iter()
        .filter(|turn| turn.actor == defender)
        .collect();
    assert!(!turns.is_empty(), "the trace has no NPC turn row");
    assert!(
        recorder
            .rows
            .iter()
            .any(|row| row.event == CombatEvent::Turn && row.actor == Some(defender))
    );
    assert!(
        turns.iter().any(|turn| turn.ap < turn.maximum),
        "the NPC spent no AP in any traced turn: {turns:?}"
    );
    // The player ends every turn at once, so it never spends AP.
    assert!(
        recorder
            .turn_ap
            .iter()
            .filter(|turn| turn.actor == player)
            .all(|turn| turn.ap == turn.maximum)
    );
}

/// SPEC-021 revision 2 §3.7 step 1: without `data/ai.txt` (the bare
/// synthetic corpus), or with a table of no packet, the combat dump
/// exits 1 with a diagnostic and writes no trace.
#[test]
fn the_combat_dump_refuses_to_run_without_combat_ai() {
    let bare = bare_corpus();
    let arguments = |dir: &Path| {
        audit(&[
            "--corpus-dir".as_ref(),
            dir.as_os_str(),
            "--dump-combat-trace".as_ref(),
            "maps/synthetic.map".as_ref(),
        ])
    };
    let output = arguments(bare.path());
    assert_eq!(output.status.code(), Some(1), "{output:?}");
    assert!(output.stdout.is_empty());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("combat AI unavailable"), "{stderr}");
    let vfs = Arc::new(LocalDirProvider::mount(bare.path()).unwrap());
    assert!(combat_trace_host(vfs, "maps/synthetic.map", 1).is_err());

    // A file with no packet section.
    install_trace_fixtures(bare.path());
    std::fs::write(bare.path().join("data/ai.txt"), "; no packet\n").unwrap();
    let output = arguments(bare.path());
    assert_eq!(output.status.code(), Some(1), "{output:?}");
    assert!(output.stdout.is_empty());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("combat AI unavailable"), "{stderr}");
}

/// T7: identical seeds give identical traces, in combat and out of it; a
/// different seed's trace differs.
#[test]
fn identical_seeds_trace_identically() {
    let corpus = corpus();
    let vfs = Arc::new(LocalDirProvider::mount(corpus.path()).unwrap());
    let trace = |seed: i32| {
        let mut host = combat_trace_host(vfs.clone(), "maps/synthetic.map", seed).unwrap();
        run_combat_trace(&mut host, 5).unwrap()
    };
    let first = trace(1);
    let second = trace(1);
    assert_eq!(first, second);
    assert!(
        first.iter().any(|row| row.round == Some(4))
            || first
                .last()
                .is_some_and(|row| row.event == CombatEvent::End)
    );
    assert_eq!(
        write_combat_trace(&first),
        golden("combat-trace-synthetic-seed1.tsv")
    );
    let other = trace(2);
    assert_eq!(other.len(), first.len());
    assert_ne!(other, first);

    // Ordinary stepping outside combat, 200 frames, hash after each.
    let mut hosts = [1, 1].map(|seed| combat_trace_host(vfs.clone(), "maps/synthetic.map", seed));
    let [Ok(a), Ok(b)] = &mut hosts else {
        panic!("the synthetic map loads");
    };
    assert_eq!(a.world.state_hash(), b.world.state_hash());
    for frame in 1..=200_u64 {
        let now = frame * 16;
        a.tick(now).unwrap();
        b.tick(now).unwrap();
        assert!(!a.world.combat);
        assert_eq!(a.world.state_hash(), b.world.state_hash(), "frame {frame}");
    }
}
