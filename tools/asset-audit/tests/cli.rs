use std::process::Command;

#[allow(dead_code)]
#[path = "../../../tests/fixtures/formats/support.rs"]
mod fixtures;

#[test]
fn validates_all_formats_and_reports_multiple_failures_without_stopping() {
    let temp = tempfile::tempdir().unwrap();
    fixtures::write_corpus(temp.path());
    let run = || {
        Command::new(env!("CARGO_BIN_EXE_asset-audit"))
            .arg("--corpus-dir")
            .arg(temp.path())
            .arg("--validate-formats")
            .output()
            .unwrap()
    };
    let output = run();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(String::from_utf8_lossy(&output.stdout).contains("Validation: 16 passed, 0 failed"));
    std::fs::write(temp.path().join("color.pal"), &fixtures::palette()[..768]).unwrap();
    std::fs::write(temp.path().join("font0.aaf"), b"bad font").unwrap();
    let output = run();
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stdout).contains("Validation: 14 passed, 2 failed"));
    let errors = String::from_utf8_lossy(&output.stderr);
    assert!(errors.contains("FAIL color.pal: PAL at byte"));
    assert!(errors.contains("FAIL font0.aaf: AAF at byte"));
}

#[test]
fn audits_the_mixed_case_loose_fixture() {
    let output = Command::new(env!("CARGO_BIN_EXE_asset-audit"))
        .args([
            "--corpus-dir",
            concat!(env!("CARGO_MANIFEST_DIR"), "/../../tests/fixtures/loose"),
        ])
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        String::from_utf8(output.stdout).unwrap(),
        include_str!("../../../tests/fixtures/audit-expected.txt")
    );
}

#[test]
fn missing_argument_and_invalid_mount_exit_unsuccessfully() {
    assert!(
        !Command::new(env!("CARGO_BIN_EXE_asset-audit"))
            .output()
            .unwrap()
            .status
            .success()
    );
    let output = Command::new(env!("CARGO_BIN_EXE_asset-audit"))
        .args([
            "--corpus-dir",
            concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/../../tests/fixtures/loose/COLOR.PAL"
            ),
        ])
        .output()
        .unwrap();
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("filesystem error"));
}
