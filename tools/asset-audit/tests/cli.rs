use std::process::Command;

#[allow(dead_code)]
#[path = "../../../tests/fixtures/formats/support.rs"]
mod fixture_assets;
use fixture_assets as fixtures;

#[allow(dead_code)]
#[path = "../../../tests/fixtures/vm/support.rs"]
mod vm;
#[allow(dead_code)]
#[path = "../../../tests/fixtures/world/support.rs"]
mod world;

#[test]
fn validates_maps_with_prototype_lists_and_int_images() {
    let temp = tempfile::tempdir().unwrap();
    world::write_corpus(temp.path());
    std::fs::write(temp.path().join("scripts/synthetic.int"), vm::program()).unwrap();
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
    assert!(String::from_utf8_lossy(&output.stdout).contains("Validation: 40 passed, 0 failed"));
    std::fs::write(
        temp.path().join("proto/items/synthetic.pro"),
        &world::container()[..64],
    )
    .unwrap();
    let mut bad = vm::program();
    bad[0x56..0x5a].copy_from_slice(&0x200_u32.to_be_bytes());
    std::fs::write(temp.path().join("scripts/synthetic.int"), bad).unwrap();
    let output = run();
    assert!(!output.status.success());
    let errors = String::from_utf8_lossy(&output.stderr);
    assert!(errors.contains("FAIL maps/synthetic.map: MAP"));
    assert!(errors.contains("FAIL proto/items/synthetic.pro: PRO"));
    assert!(errors.contains("FAIL scripts/synthetic.int: INT"));
    assert!(String::from_utf8_lossy(&output.stdout).contains("Validation: 37 passed, 3 failed"));
}

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
