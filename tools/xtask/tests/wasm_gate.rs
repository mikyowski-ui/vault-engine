//! SPEC-023 T17 and T19: the compile gate as a user runs it, and the
//! dependency graph it keeps to, on a machine with or without the target.
use std::{
    collections::BTreeSet,
    path::{Path, PathBuf},
    process::Command,
};

const GRAPH: &str = include_str!("../../../tests/fixtures/spec023/wasm-graph.tsv");

fn root() -> &'static Path {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .unwrap()
}

/// T19: this needs no installed target, so it runs everywhere, and catches a
/// new dependency on a machine that skips the gate.
#[test]
fn t19_each_gated_crate_has_exactly_its_allowed_dependencies() {
    for line in GRAPH.lines().skip(1) {
        let [name, workspace, allowed] = line.split('\t').collect::<Vec<_>>()[..] else {
            panic!("{line:?}");
        };
        let output = Command::new(env!("CARGO"))
            .args(["tree", "-e", "normal", "--target", "wasm32-unknown-unknown"])
            .args(["--no-default-features", "-p", name])
            .args(["--prefix", "none", "--locked"])
            .current_dir(root())
            .output()
            .unwrap();
        let tree = String::from_utf8(output.stdout).unwrap();
        assert!(output.status.success(), "{name}: {tree}");
        let found: BTreeSet<&str> = tree
            .lines()
            .filter_map(|line| line.split_whitespace().next())
            .collect();
        let mut expected: BTreeSet<&str> = allowed.split(',').collect();
        expected.extend(workspace.split(',').filter(|crate_name| *crate_name != "-"));
        expected.insert(name);
        assert_eq!(found, expected, "{name}");
    }
}

/// A `rustc` that names a missing library folder, as a distribution's does,
/// and hands every other call to the real compiler.
#[cfg(unix)]
fn missing_target_rustc(dir: &Path) -> PathBuf {
    use std::os::unix::fs::PermissionsExt;
    let real = std::env::var("RUSTC").unwrap_or_else(|_| "rustc".to_owned());
    let script = format!(
        "#!/bin/sh\n\
         for argument in \"$@\"; do\n\
         \x20 if [ \"$argument\" = target-libdir ]; then\n\
         \x20   echo /nonexistent/rustlib/lib\n\
         \x20   exit 0\n\
         \x20 fi\n\
         done\n\
         exec {real} \"$@\"\n"
    );
    let path = dir.join("rustc-without-wasm");
    std::fs::write(&path, script).unwrap();
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
    path
}

#[cfg(unix)]
#[test]
fn t17_a_missing_target_skips_offline_and_a_required_one_fails() {
    let dir = Path::new(env!("CARGO_TARGET_TMPDIR")).join("wasm-gate");
    std::fs::create_dir_all(&dir).unwrap();
    let rustc = missing_target_rustc(&dir);
    let gate = |require: Option<&str>, target: &Path| {
        let mut command = Command::new(env!("CARGO_BIN_EXE_xtask"));
        command
            .arg("wasm-check")
            .env("RUSTC", &rustc)
            .env("CARGO_NET_OFFLINE", "true")
            .env("CARGO_TARGET_DIR", target)
            .env_remove("VAULT_REQUIRE_WASM");
        if let Some(value) = require {
            command.env("VAULT_REQUIRE_WASM", value);
        }
        command.output().unwrap()
    };

    // This folder outlives a run, so the one that shows no check ran starts
    // empty each time.
    let unused = dir.join("target-unused");
    let _ = std::fs::remove_dir_all(&unused);
    let required = gate(Some("1"), &unused);
    let stdout = String::from_utf8_lossy(&required.stdout);
    assert_eq!(required.status.code(), Some(1), "{stdout}");
    assert_eq!(
        stdout.trim_end(),
        "error: wasm32-unknown-unknown target not installed"
    );
    // It ran no check.
    assert!(!unused.exists());

    let skipped = gate(None, &dir.join("target"));
    let stdout = String::from_utf8_lossy(&skipped.stdout);
    let stderr = String::from_utf8_lossy(&skipped.stderr);
    assert_eq!(skipped.status.code(), Some(0), "{stdout}{stderr}");
    let lines: Vec<&str> = stdout.lines().collect();
    assert_eq!(
        lines.first(),
        Some(&"skipped: wasm32-unknown-unknown target not installed")
    );
    assert_eq!(lines.last(), Some(&"host check ok: 4 crates"));
    assert_eq!(lines.len(), 3, "{stdout}");
}
