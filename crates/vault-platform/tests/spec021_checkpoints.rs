//! SPEC-021 §3.10 through the binary: `--checkpoints-out` and
//! `--checkpoints-expect` on `--replay-inputs`, and their exit statuses.
use std::{
    path::{Path, PathBuf},
    process::{Command, Output},
};
use vault_fixtures::{gallery, state, vm};

const GOLDEN: &str = "rev3/scenario-explore-talk.checkpoints.tsv";

fn fixture(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../tests/fixtures/spec021")
        .join(name)
}

/// What `cargo xtask fixtures --output DIR` writes in DIR.
fn corpus() -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    state::write_corpus(dir.path());
    gallery::write_hud(dir.path());
    std::fs::write(dir.path().join("scripts/core-fixture.int"), vm::program()).unwrap();
    dir
}

/// Scenario (a) as §3.11 runs it, with `extra` flags.
fn replay(corpus: &Path, extra: &[&std::ffi::OsStr]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_vault-platform"))
        .arg("--corpus-dir")
        .arg(corpus)
        .args(["--preview-map", "maps/synthetic.map", "--seed", "1"])
        .arg("--replay-inputs")
        .arg(fixture("rev3/scenario-explore-talk.jsonl"))
        .args(extra)
        .output()
        .unwrap()
}

fn digest(output: &Output) -> String {
    String::from_utf8(output.stdout.clone()).unwrap()
}

#[test]
fn the_binary_writes_the_golden_and_keeps_its_digest() {
    let corpus = corpus();
    let temp = tempfile::tempdir().unwrap();
    let out = temp.path().join("stream.tsv");
    let plain = replay(corpus.path(), &[]);
    assert_eq!(plain.status.code(), Some(0), "{plain:?}");
    let with = replay(
        corpus.path(),
        &["--checkpoints-out".as_ref(), out.as_os_str()],
    );
    assert_eq!(with.status.code(), Some(0), "{with:?}");
    assert_eq!(digest(&plain), digest(&with));
    assert!(digest(&plain).contains("state_sha256"));
    assert_eq!(
        std::fs::read(&out).unwrap(),
        std::fs::read(fixture(GOLDEN)).unwrap()
    );
    // A stream that matches to the end exits as without the flag.
    let golden = fixture(GOLDEN);
    let matched = replay(
        corpus.path(),
        &["--checkpoints-expect".as_ref(), golden.as_os_str()],
    );
    assert_eq!(matched.status.code(), Some(0), "{matched:?}");
    assert_eq!(digest(&matched), digest(&plain));
}

#[test]
fn a_changed_row_stops_the_run_with_status_2_and_no_digest() {
    let corpus = corpus();
    let temp = tempfile::tempdir().unwrap();
    let golden = std::fs::read_to_string(fixture(GOLDEN)).unwrap();
    let row = golden.lines().nth(120 + 2).unwrap();
    let hash = row.split('\t').nth(2).unwrap();
    let flipped = format!(
        "{}{}",
        if hash.starts_with('0') { '1' } else { '0' },
        &hash[1..]
    );
    let changed = temp.path().join("changed.tsv");
    std::fs::write(
        &changed,
        golden.replace(row, &row.replacen(hash, &flipped, 1)),
    )
    .unwrap();
    let out = temp.path().join("stream.tsv");
    let output = replay(
        corpus.path(),
        &[
            "--checkpoints-expect".as_ref(),
            changed.as_os_str(),
            "--checkpoints-out".as_ref(),
            out.as_os_str(),
        ],
    );
    assert_eq!(output.status.code(), Some(2), "{output:?}");
    assert!(output.stdout.is_empty(), "no digest line");
    let stderr = String::from_utf8_lossy(&output.stderr);
    let report =
        format!("checkpoint mismatch at tick 120: state_hash expected {flipped}, actual {hash}");
    assert!(stderr.lines().any(|line| line == report), "{stderr}");
    // No step past that row.
    let written = std::fs::read_to_string(&out).unwrap();
    assert_eq!(written.lines().count(), 2 + 121);
}

#[test]
fn usage_errors_and_an_unreadable_stream_exit_1_before_the_run() {
    let corpus = corpus();
    let temp = tempfile::tempdir().unwrap();
    let missing = temp.path().join("missing.tsv");
    let output = replay(
        corpus.path(),
        &["--checkpoints-expect".as_ref(), missing.as_os_str()],
    );
    assert_eq!(output.status.code(), Some(1), "{output:?}");
    assert!(output.stdout.is_empty());
    let garbage = temp.path().join("garbage.tsv");
    std::fs::write(&garbage, "not a stream").unwrap();
    let output = replay(
        corpus.path(),
        &["--checkpoints-expect".as_ref(), garbage.as_os_str()],
    );
    assert_eq!(output.status.code(), Some(1), "{output:?}");
    // Without --replay-inputs the flag is a usage error, and it exits 1
    // because 2 means a failed comparison.
    let output = Command::new(env!("CARGO_BIN_EXE_vault-platform"))
        .args(["--checkpoints-out", "stream.tsv"])
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(1), "{output:?}");
}
