//! SPEC-024a §2.3 through the binary: `--mod-list` on `--replay-inputs`,
//! the one way a replay mounts layers, and the refusals of its `mod_stack`
//! record. The layer is a real folder on a real load order.
use std::{
    ffi::OsStr,
    path::Path,
    process::{Command, Output},
};
use vault_fixtures::{gallery, state, vm};

const N1: &str = include_str!("../../../tests/fixtures/spec024a/vault-mod.toml");
const PROGRAM: &str = include_str!("../../../tests/fixtures/spec024a/gl_cheapap.int.hex");
/// §2.3's example, with the digest of `N1`.
const RECORD: &str = r#"{"command_id":2,"kind":"mod_stack","api":1,"mods":[{"id":"cheap-ap","version":"1.0.0","manifest":"EE7C38513A262548","settings":{"ap_cost":"i:1"}}]}"#;
const SURFACE: &str = r#"{"command_id":1,"kind":"surface","width":640,"height":480}"#;

/// A folder of game data with the `N1` layer and its load order.
fn corpus(manifest: &str) -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    state::write_corpus(dir.path());
    gallery::write_hud(dir.path());
    std::fs::write(dir.path().join("scripts/core-fixture.int"), vm::program()).unwrap();
    let layer = dir.path().join("mods/cheap");
    std::fs::create_dir_all(layer.join("scripts")).unwrap();
    std::fs::write(layer.join("vault-mod.toml"), manifest).unwrap();
    let program: Vec<u8> = PROGRAM
        .lines()
        .filter_map(|line| line.split_once(": "))
        .flat_map(|(_, bytes)| bytes.split(' '))
        .map(|byte| u8::from_str_radix(byte, 16).unwrap())
        .collect();
    std::fs::write(layer.join("scripts/gl_cheapap.int"), program).unwrap();
    std::fs::write(
        dir.path().join("order.toml"),
        "[[layer]]\npath = \"mods/cheap\"\n",
    )
    .unwrap();
    dir
}
/// The surface, `record` if any, and two ticks.
fn log(record: Option<&str>) -> String {
    let mut lines = vec![SURFACE.to_owned()];
    lines.extend(record.map(str::to_owned));
    let first = lines.len() + 1;
    for (index, now) in [16, 32].iter().enumerate() {
        lines.push(format!(
            r#"{{"command_id":{},"kind":"tick","now_ms":{now}}}"#,
            first + index
        ));
    }
    lines.join("\n") + "\n"
}
fn replay(corpus: &Path, log: &str, extra: &[&OsStr]) -> Output {
    let path = corpus.join("inputs.jsonl");
    std::fs::write(&path, log).unwrap();
    Command::new(env!("CARGO_BIN_EXE_vault-platform"))
        .arg("--corpus-dir")
        .arg(corpus)
        .args(["--preview-map", "maps/synthetic.map", "--seed", "1"])
        .arg("--replay-inputs")
        .arg(path)
        .args(extra)
        .output()
        .unwrap()
}
fn with_order(corpus: &Path, log: &str) -> Output {
    let order = corpus.join("order.toml");
    replay(corpus, log, &["--mod-list".as_ref(), order.as_os_str()])
}
fn errors(output: &Output) -> String {
    String::from_utf8_lossy(&output.stderr).into_owned()
}

/// The record that matches the load order runs, and runs the same twice.
#[test]
fn a_matching_stack_replays_with_a_stable_digest() {
    let dir = corpus(N1);
    let first = with_order(dir.path(), &log(Some(RECORD)));
    assert_eq!(first.status.code(), Some(0), "{}", errors(&first));
    assert!(String::from_utf8_lossy(&first.stdout).contains("state_sha256"));
    let second = with_order(dir.path(), &log(Some(RECORD)));
    assert_eq!(first.stdout, second.stdout);
}

/// §10 test 10 at the command line: each way the stack can differ stops the
/// run with status 1, naming the mod, and no digest is printed.
#[test]
fn a_stack_that_differs_is_refused_by_the_binary() {
    let dir = corpus(N1);
    let refused = |output: Output, id: &str| {
        assert_eq!(output.status.code(), Some(1), "{}", errors(&output));
        let message = format!("mod stack mismatch: {id}");
        assert!(errors(&output).contains(&message), "{}", errors(&output));
        assert!(output.stdout.is_empty(), "{output:?}");
    };
    // The log has the record and the run has no layers.
    refused(replay(dir.path(), &log(Some(RECORD)), &[]), "cheap-ap");
    // The run has the layer and the log has no record.
    refused(with_order(dir.path(), &log(None)), "cheap-ap");
    // The manifest, its version, or a setting the manifest does not declare.
    let manifest = RECORD.replace("EE7C38513A262548", "0000000000000000");
    refused(with_order(dir.path(), &log(Some(&manifest))), "cheap-ap");
    let version = RECORD.replace("1.0.0", "1.0.1");
    refused(with_order(dir.path(), &log(Some(&version))), "cheap-ap");
    let setting = RECORD.replace("ap_cost", "other");
    refused(with_order(dir.path(), &log(Some(&setting))), "cheap-ap");
    // Another mod's name in the record.
    let other = RECORD.replace("cheap-ap", "other-mod");
    refused(with_order(dir.path(), &log(Some(&other))), "other-mod");
    // The folder on the load order changed since the log was written.
    let changed = corpus(&N1.replace("1.0.0", "2.0.0"));
    refused(with_order(changed.path(), &log(Some(RECORD))), "cheap-ap");
}

/// A load order that cannot be read stops the run and names the file.
#[test]
fn a_missing_load_order_is_refused_by_the_binary() {
    let dir = corpus(N1);
    let missing = dir.path().join("no-such-order.toml");
    let output = replay(
        dir.path(),
        &log(Some(RECORD)),
        &["--mod-list".as_ref(), missing.as_os_str()],
    );
    assert_eq!(output.status.code(), Some(1), "{}", errors(&output));
    assert!(errors(&output).contains("no-such-order.toml"), "{output:?}");
}
