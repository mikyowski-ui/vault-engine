//! SPEC-021 §4 tests T15 to T17 and T19: the replay's checkpoint stream, the
//! scenarios (a) and (b) of §3.11 on their committed logs, and both render
//! paths. Scenario (c) runs through `asset-audit` (T18); here it also runs
//! with a refresh after every step.
use super::*;
use crate::{
    map_preview::{Screen, SharedRenderer, test_support},
    preview::PreviewArgs,
    session::{SessionConfig, SessionRuntime},
};
use std::path::{Path, PathBuf};
use vault_core::{
    checkpoints::{Checkpoint, CheckpointFailure, read_checkpoints},
    traces::{CombatDriver, DriverStep},
};
use vault_fixtures::{gallery, state, vm};

/// `tests/fixtures/spec021/`.
fn fixtures() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tests/fixtures/spec021")
}
fn fixture(name: &str) -> String {
    std::fs::read_to_string(fixtures().join(name)).unwrap()
}

/// What `cargo xtask fixtures --output DIR` writes in DIR.
fn corpus() -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    state::write_corpus(dir.path());
    gallery::write_hud(dir.path());
    std::fs::write(dir.path().join("scripts/core-fixture.int"), vm::program()).unwrap();
    dir
}

/// §3.11's command line: `--corpus-dir DIR --preview-map maps/synthetic.map
/// --seed 1`, as a replay starts it.
fn session(root: &Path) -> SessionRuntime {
    SessionRuntime::start(SessionConfig::replay(PreviewArgs {
        corpus_dir: Some(root.into()),
        preview_map: Some("maps/synthetic.map".into()),
        seed: Some(1),
        headless: true,
        ..Default::default()
    }))
    .unwrap()
    .unwrap()
}

/// A run writing its stream to `dir`, and comparing with `expected` if given.
fn options(dir: &Path, expected: Option<&str>) -> CheckpointOptions {
    CheckpointOptions {
        out: Some(dir.join("stream.tsv")),
        expected: expected.map(|text| ExpectedStream::parse(text).unwrap()),
    }
}

/// The stream so far.
fn stream(replay: &Replay) -> String {
    replay.checkpoints.as_ref().unwrap().text.clone()
}

/// The rows so far.
fn rows(replay: &Replay) -> Vec<Checkpoint> {
    read_checkpoints(&stream(replay)).unwrap()
}

/// The world a replay shows.
fn world(replay: &mut Replay) -> &mut crate::map_preview::MapPreview {
    interactive_world(&mut replay.session).unwrap()
}

/// `log` replayed record by record over `root`; `at_step` runs after every
/// step, with the step's number. Returns the stream.
fn run_steps(
    root: &Path,
    log: &str,
    renderer: Option<SharedRenderer>,
    mut at_step: impl FnMut(u64, &mut Replay),
) -> String {
    let temp = tempfile::tempdir().unwrap();
    let mut session = session(root);
    if let Some(renderer) = renderer {
        interactive_world(&mut session)
            .unwrap()
            .set_scene_renderer(renderer);
    }
    let mut replay = Replay::with_checkpoints(session, options(temp.path(), None)).unwrap();
    let mut step = 0;
    for line in log.lines() {
        replay.run(line.as_bytes()).unwrap();
        if line.contains(r#""kind":"tick""#) {
            step += 1;
            at_step(step, &mut replay);
        }
    }
    let text = stream(&replay);
    replay.finish(None).unwrap();
    text
}

/// The first line of two streams that differs: its row's tick.
fn first_difference(a: &str, b: &str) -> Option<u64> {
    let rows = |text| read_checkpoints(text).unwrap();
    let (a, b) = (rows(a), rows(b));
    a.iter()
        .zip(&b)
        .find(|(a, b)| a != b)
        .map(|(a, _)| a.tick)
        .or((a.len() != b.len()).then(|| a.len().min(b.len()) as u64))
}

const EXPLORE: &str = "rev3/scenario-explore-talk.jsonl";
const TRAVEL: &str = "rev3/scenario-world-travel.jsonl";
const EXPLORE_GOLDEN: &str = "rev3/scenario-explore-talk.checkpoints.tsv";
const TRAVEL_GOLDEN: &str = "rev3/scenario-world-travel.checkpoints.tsv";
const COMBAT_GOLDEN: &str = "rev3/scenario-seeded-combat.checkpoints.tsv";

/// The first `count` records of scenario (a).
fn short_log(count: usize) -> String {
    fixture(EXPLORE)
        .lines()
        .take(count)
        .map(|line| format!("{line}\n"))
        .collect()
}

#[test]
fn t15_n_steps_give_n_plus_one_rows_and_the_index_counts_the_records() {
    let dir = corpus();
    let log = short_log(40);
    let ticks = log.matches(r#""kind":"tick""#).count() as u64;
    let last_tick = log
        .lines()
        .enumerate()
        .filter(|(_, line)| line.contains("tick"))
        .last()
        .unwrap()
        .0 as u64;
    let temp = tempfile::tempdir().unwrap();
    let mut replay =
        Replay::with_checkpoints(session(dir.path()), options(temp.path(), None)).unwrap();
    // Row 0 is written before the first step, whatever the session did.
    assert_eq!(rows(&replay).len(), 1);
    assert_eq!(
        (rows(&replay)[0].tick, rows(&replay)[0].input_index),
        (0, 0)
    );
    replay.run(log.as_bytes()).unwrap();
    let rows = rows(&replay);
    assert_eq!(rows.len() as u64, ticks + 1);
    for (tick, row) in rows.iter().enumerate() {
        assert_eq!(row.tick, tick as u64);
    }
    // `surface`, then a tick: one step of two records.
    assert_eq!(rows[1].input_index, 2);
    assert_eq!(rows.last().unwrap().input_index, last_tick + 1);
}

#[test]
fn t15_a_simulation_change_appears_in_its_own_step_and_presentation_reads_that_row() {
    let dir = corpus();
    let mut changes = 0;
    let mut last = None;
    let mut presented = 0;
    let log = fixture(EXPLORE);
    run_steps(dir.path(), &log, None, |step, replay| {
        let row = *rows(replay).last().unwrap();
        assert_eq!(row.tick, step);
        // Whatever the step's simulation changed is in row S, not row S + 1:
        // nothing covered moves between the row and the end of the step.
        let hash = world(replay).host.world.state_hash();
        assert_eq!(row.state_hash, hash, "step {step}");
        changes += u32::from(last.is_some_and(|last| last != hash));
        last = Some(hash);
        // Presentation starts from the state the row describes.
        if replay.presented.len() > presented {
            presented = replay.presented.len();
            assert_eq!(
                replay.presented.last(),
                Some(&row.state_hash),
                "step {step}"
            );
        }
    });
    assert!(changes > 0, "the scenario changes the world");
    assert!(presented > 0, "the scenario presents frames");
}

#[test]
fn t15_the_stream_changes_no_digest() {
    let dir = corpus();
    let temp = tempfile::tempdir().unwrap();
    let log = fixture(EXPLORE);
    let plain = replay(session(dir.path()), log.as_bytes(), None).unwrap();
    let with = replay_with_checkpoints(
        session(dir.path()),
        log.as_bytes(),
        None,
        options(temp.path(), None),
    )
    .unwrap();
    assert_eq!(plain, with);
    // Without a flag no row is computed.
    let mut quiet = Replay::new(session(dir.path())).unwrap();
    quiet.run(log.as_bytes()).unwrap();
    assert!(quiet.checkpoints.is_none());
    assert_eq!(quiet.finish(None).unwrap(), plain);
}

/// A golden with row `tick`'s `state_hash` changed.
fn with_changed_row(golden: &str, tick: u64) -> String {
    let prefix = format!("{tick}\t");
    golden
        .lines()
        .map(|line| {
            let mut line = line.to_owned();
            if line.starts_with(&prefix) {
                let hash = line.split('\t').nth(2).unwrap().to_owned();
                let flipped = if hash.starts_with('0') { '1' } else { '0' };
                line = line.replacen(&hash, &format!("{flipped}{}", &hash[1..]), 1);
            }
            line + "\n"
        })
        .collect()
}

#[test]
fn t15_expecting_a_changed_row_stops_at_that_step() {
    let dir = corpus();
    let temp = tempfile::tempdir().unwrap();
    let golden = fixture(EXPLORE_GOLDEN);
    let changed = with_changed_row(&golden, 200);
    let error = replay_with_checkpoints(
        session(dir.path()),
        fixture(EXPLORE).as_bytes(),
        None,
        options(temp.path(), Some(&changed)),
    )
    .unwrap_err();
    let report = error.downcast_ref::<CheckpointsDiverged>().unwrap();
    assert!(
        report
            .0
            .starts_with("checkpoint mismatch at tick 200: state_hash expected "),
        "{report}"
    );
    // The run makes no step past that row: its stream ends there.
    let written = std::fs::read_to_string(temp.path().join("stream.tsv")).unwrap();
    assert_eq!(read_checkpoints(&written).unwrap().len(), 201);
    // A stream that runs out before the log is too long at the next row,
    // and a log that runs out first ended early.
    let cut: String = golden
        .lines()
        .take(12)
        .map(|line| format!("{line}\n"))
        .collect();
    let error = replay_with_checkpoints(
        session(dir.path()),
        fixture(EXPLORE).as_bytes(),
        None,
        options(temp.path(), Some(&cut)),
    )
    .unwrap_err();
    assert_eq!(
        error.downcast_ref::<CheckpointsDiverged>().unwrap().0,
        "checkpoint stream too long: unexpected tick 10"
    );
    let error = replay_with_checkpoints(
        session(dir.path()),
        short_log(40).as_bytes(),
        None,
        options(temp.path(), Some(&golden)),
    )
    .unwrap_err();
    assert!(
        error
            .downcast_ref::<CheckpointsDiverged>()
            .unwrap()
            .0
            .starts_with("checkpoint stream ended early: expected tick ")
    );
    // Another format stops before the first step.
    let other = golden.replace("VSH1", "VSH2");
    let replay =
        Replay::with_checkpoints(session(dir.path()), options(temp.path(), Some(&other))).unwrap();
    assert!(replay.stopped && replay.diverged());
}

#[test]
fn an_untimed_log_writes_one_row_per_command() {
    let dir = corpus();
    let temp = tempfile::tempdir().unwrap();
    let log = concat!(
        r#"{"command_id":1,"kind":"focus","focused":true}"#,
        "\n",
        r#"{"command_id":2,"kind":"pointer_moved","x":496.5,"y":250.5}"#,
        "\n",
        r#"{"command_id":3,"kind":"focus","focused":true}"#,
        "\n",
        r#"{"command_id":4,"kind":"quit"}"#,
        "\n",
    );
    let mut replay =
        Replay::with_checkpoints(session(dir.path()), options(temp.path(), None)).unwrap();
    replay.run(log.as_bytes()).unwrap();
    let written = rows(&replay);
    // Three commands, and the quit writes no row.
    assert_eq!(written.len(), 4);
    assert_eq!(
        written
            .iter()
            .map(|row| row.input_index)
            .collect::<Vec<_>>(),
        [0, 1, 2, 3]
    );
}

/// §3.11's assertions on (a), read from the live session.
#[test]
fn t16_scenario_a_replays_to_its_golden_and_its_assertions_hold() {
    let dir = corpus();
    let mut hexes = [None; 2];
    let mut open = [None; 2];
    let text = run_steps(dir.path(), &fixture(EXPLORE), None, |step, replay| {
        let map = world(replay);
        let player = map.host.world.player;
        let hex = map.host.world.get(player).unwrap().raw.hex;
        match step {
            10 => hexes[0] = Some(hex),
            133 => hexes[1] = Some(hex),
            261 => open[0] = Some(map.host.world.dialogue.open),
            387 => open[1] = Some(map.host.world.dialogue.open),
            _ => {}
        }
    });
    assert_eq!(text, fixture(EXPLORE_GOLDEN));
    let rows = read_checkpoints(&text).unwrap();
    assert_eq!(rows.len(), 388);
    assert_ne!(hexes[0], hexes[1], "the player walked");
    assert_ne!(rows[10].state_hash, rows[133].state_hash);
    assert_eq!(open, [Some(true), Some(false)], "the conversation");
    assert!(rows.iter().all(|row| row.combat_hash.is_none()));
}

/// §3.11's assertions on (b), and its uncovered final row.
#[test]
fn t17_scenario_b_replays_to_its_golden_and_its_assertions_hold() {
    let dir = corpus();
    let mut opened = None;
    let mut finished = None;
    let mut map_at_start = None;
    let text = run_steps(dir.path(), &fixture(TRAVEL), None, |step, replay| {
        let map = world(replay);
        if step == 1 {
            map_at_start = Some(map.host.world.map.header.name);
        }
        let on_world_map = matches!(map.screen, Screen::World(_));
        match step {
            12 => opened = Some(on_world_map),
            615 => {
                let world = &map.host.world;
                finished = Some((
                    on_world_map,
                    world.world_map.saved.position,
                    world.world_map.travel.is_none(),
                    world.map.header.name == map_at_start.unwrap(),
                    world.clock.ticks,
                ));
            }
            _ => {}
        }
    });
    assert_eq!(text, fixture(TRAVEL_GOLDEN));
    let rows = read_checkpoints(&text).unwrap();
    assert_eq!(rows.len(), 616);
    assert_eq!(opened, Some(true), "the world map is open at step 12");
    let (on_world_map, position, arrived, same_map, ticks) = finished.unwrap();
    assert!(on_world_map, "arrival never enters a town");
    assert_ne!(position, [825, 75], "the party moved");
    assert!(arrived, "travel has finished");
    assert!(same_map, "no encounter changed the map");
    let hashes: std::collections::BTreeSet<u64> =
        rows[12..].iter().map(|row| row.state_hash).collect();
    assert!(hashes.len() >= 2, "a check drew dice on the way");
    assert!(rows.iter().all(|row| row.combat_hash.is_none()));
    let final_row = format!(
        "x\ty\tgame_tick\n{}\t{}\t{ticks}\n",
        position[0], position[1]
    );
    assert_eq!(final_row, fixture("rev3/scenario-world-travel.final.tsv"));
}

/// Scenario `log` drawn by `renderer` gives its golden, with nothing refused.
fn assert_stream_on(root: &Path, log: &str, golden: &str, renderer: SharedRenderer) {
    let text = run_steps(root, &fixture(log), Some(renderer.clone()), |_, _| {});
    assert_eq!(text, fixture(golden), "{log}");
    let totals = renderer.session_totals();
    assert!(totals.instanced > 0, "{log}: {totals:?}");
    assert_eq!(totals.refused, 0, "{log}: {totals:?}");
    assert!(renderer.logged_causes().is_empty(), "{log}");
}

/// T19: the instanced path on the CPU executor, on any host.
#[test]
fn t19_both_logged_scenarios_give_their_goldens_on_the_cpu_executor() {
    let dir = corpus();
    for (log, golden) in [(EXPLORE, EXPLORE_GOLDEN), (TRAVEL, TRAVEL_GOLDEN)] {
        assert_stream_on(dir.path(), log, golden, test_support::cpu_renderer());
    }
}

/// T19 on the GPU (a GPU test: it skips without an adapter unless
/// `VAULT_REQUIRE_GPU=1`).
#[test]
fn t19_both_logged_scenarios_give_their_goldens_on_the_gpu() {
    let _gpu = test_support::gpu_lock();
    let dir = corpus();
    let Some(gpu) = test_support::gpu_renderer() else {
        return;
    };
    let renderer = SharedRenderer::from_renderer(gpu);
    for (log, golden) in [(EXPLORE, EXPLORE_GOLDEN), (TRAVEL, TRAVEL_GOLDEN)] {
        assert_stream_on(dir.path(), log, golden, renderer.clone());
    }
}

/// §3.7's table: the AI fixture and the art fixture, in a traced corpus.
fn install_trace_fixtures(root: &Path) {
    for (from, to) in [
        ("ai.txt", "data/ai.txt"),
        ("combatai.msg", "text/english/game/combatai.msg"),
        ("sampleaq.frm", "art/critters/sampleaq.frm"),
        ("sampleao.frm", "art/critters/sampleao.frm"),
        ("sampleba.frm", "art/critters/sampleba.frm"),
        ("samplecj.frm", "art/critters/samplecj.frm"),
    ] {
        let to = root.join(to);
        std::fs::create_dir_all(to.parent().unwrap()).unwrap();
        std::fs::copy(fixtures().join("rev2").join(from), to).unwrap();
    }
}

/// Scenario (c): §3.7's driver over a map session, with a refresh drawn by
/// `renderer` after every row, which is how the render path could reach the
/// stream. Returns the stream.
fn seeded_combat_on(root: &Path, renderer: SharedRenderer) -> String {
    let mut session = session(root);
    let map = interactive_world(&mut session).unwrap();
    map.set_scene_renderer(renderer);
    let mut stream = CheckpointStream::new(None);
    let mut text = stream.head().unwrap();
    let mut driver = CombatDriver::new(&mut map.host, 5).unwrap();
    let row = |stream: &mut CheckpointStream, text: &mut String, map: &mut MapPreview, input| {
        text.push_str(&stream.row(&map.host.world, input).unwrap());
        map.image().unwrap();
    };
    row(&mut stream, &mut text, map, driver.commands());
    loop {
        match driver.step(&mut map.host).unwrap() {
            DriverStep::Running => row(&mut stream, &mut text, map, driver.commands()),
            DriverStep::Ended(_) => {
                row(&mut stream, &mut text, map, driver.commands());
                break;
            }
            DriverStep::RoundLimit(_) => break,
        }
    }
    text
}

#[test]
fn scenario_c_gives_its_golden_with_a_refresh_after_every_step_on_each_path() {
    let _gpu = test_support::gpu_lock();
    let dir = corpus();
    install_trace_fixtures(dir.path());
    let golden = fixture(COMBAT_GOLDEN);
    assert_eq!(
        seeded_combat_on(dir.path(), SharedRenderer::default()),
        golden
    );
    let cpu = test_support::cpu_renderer();
    assert_eq!(seeded_combat_on(dir.path(), cpu.clone()), golden);
    let totals = cpu.session_totals();
    assert!(totals.instanced > 0 && totals.refused == 0, "{totals:?}");
    if let Some(gpu) = test_support::gpu_renderer() {
        let gpu = SharedRenderer::from_renderer(gpu);
        assert_eq!(seeded_combat_on(dir.path(), gpu.clone()), golden);
        let totals = gpu.session_totals();
        assert!(totals.instanced > 0 && totals.refused == 0, "{totals:?}");
    }
}

/// `log` with the record that holds `old` replaced by `new` (`None` drops
/// it), keeping every other record's bytes.
fn changed_log(log: &str, old: &str, new: Option<&str>) -> String {
    assert_eq!(log.matches(old).count(), 1, "{old}");
    log.lines()
        .filter_map(|line| {
            if !line.contains(old) {
                return Some(format!("{line}\n"));
            }
            new.map(|new| format!("{}\n", line.replace(old, new)))
        })
        .collect()
}

/// The step a record belongs to: one past the ticks before it.
fn step_of(log: &str, record: &str) -> u64 {
    let at = log.find(record).unwrap();
    log[..at].matches(r#""kind":"tick""#).count() as u64 + 1
}

/// `changed` replayed against `golden`: the failure, and the rows written.
fn compare_with(root: &Path, changed: &str, golden: &str) -> (String, u64) {
    let temp = tempfile::tempdir().unwrap();
    let error = replay_with_checkpoints(
        session(root),
        changed.as_bytes(),
        None,
        options(temp.path(), Some(golden)),
    )
    .unwrap_err();
    let report = error
        .downcast_ref::<CheckpointsDiverged>()
        .unwrap()
        .0
        .clone();
    let written = std::fs::read_to_string(temp.path().join("stream.tsv")).unwrap();
    (report, read_checkpoints(&written).unwrap().len() as u64)
}

/// A dropped record shifts `input_index` in the very step that took it.
#[test]
fn dropping_one_input_record_fails_at_the_step_that_took_it() {
    let dir = corpus();
    for (log, golden, record) in [
        (EXPLORE, EXPLORE_GOLDEN, r#""x":352.0,"y":161.0"#),
        (TRAVEL, TRAVEL_GOLDEN, r#""x":407.0,"y":96.0"#),
    ] {
        let log = fixture(log);
        let step = step_of(&log, record);
        let golden = fixture(golden);
        let changed = changed_log(&log, record, None);
        let (report, written) = compare_with(dir.path(), &changed, &golden);
        assert!(
            report.starts_with(&format!(
                "checkpoint mismatch at tick {step}: input_index expected "
            )),
            "{report}"
        );
        assert_eq!(written, step + 1);
    }
}

/// A changed record keeps every `input_index`: the comparison fails at the
/// first row whose covered state differs, which a full run of the changed
/// log finds on its own.
#[test]
fn changing_one_input_record_fails_at_the_first_row_that_differs() {
    let dir = corpus();
    for (log, golden, record, new) in [
        (
            EXPLORE,
            EXPLORE_GOLDEN,
            r#""x":288.0,"y":205.0"#,
            r#""x":352.0,"y":237.0"#,
        ),
        (
            EXPLORE,
            EXPLORE_GOLDEN,
            r#""x":352.0,"y":161.0"#,
            r#""x":200.0,"y":100.0"#,
        ),
        (
            TRAVEL,
            TRAVEL_GOLDEN,
            r#""x":407.0,"y":96.0"#,
            r#""x":287.0,"y":176.0"#,
        ),
    ] {
        let log = fixture(log);
        let golden = fixture(golden);
        let changed = changed_log(&log, record, Some(new));
        let full = run_steps(dir.path(), &changed, None, |_, _| {});
        let tick = first_difference(&golden, &full).expect("the record changed the run");
        assert!(tick >= step_of(&log, record), "{record}: tick {tick}");
        let (report, written) = compare_with(dir.path(), &changed, &golden);
        assert!(
            report.starts_with(&format!("checkpoint mismatch at tick {tick}: state_hash")),
            "{record}: {report}"
        );
        assert_eq!(written, tick + 1, "{record}");
    }
}

#[test]
fn the_failure_is_the_row_of_the_comparison() {
    let failure = CheckpointFailure::TooLong { tick: 3 };
    assert_eq!(
        CheckpointsDiverged(failure.to_string()).to_string(),
        "checkpoint stream too long: unexpected tick 3"
    );
}
