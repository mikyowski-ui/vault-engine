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
pub(crate) use world as fixture_world;
#[allow(dead_code)]
#[path = "../../../tests/fixtures/state/support.rs"]
mod state;

#[test]
fn validates_gam_and_gcd_and_rejects_undefined_initializers() {
    let temp = tempfile::tempdir().unwrap();
    state::write_corpus(temp.path());
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
    let text = String::from_utf8_lossy(&output.stdout);
    assert!(text.contains(".gam\t2\t0"));
    assert!(text.contains(".gcd\t1\t0"));
    std::fs::write(
        temp.path().join("data/vault13.gam"),
        b"GAME_GLOBAL_VARS:\nbad=;\n",
    )
    .unwrap();
    std::fs::write(
        temp.path().join("premade/synthetic.gcd"),
        &state::gcd()[..427],
    )
    .unwrap();
    let output = run();
    assert!(!output.status.success());
    let errors = String::from_utf8_lossy(&output.stderr);
    assert!(errors.contains("FAIL data/vault13.gam: GAM"));
    assert!(errors.contains("FAIL premade/synthetic.gcd: GCD"));
}

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
    assert!(String::from_utf8_lossy(&output.stdout).contains("Validation: 41 passed, 0 failed"));
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
    assert!(String::from_utf8_lossy(&output.stdout).contains("Validation: 38 passed, 3 failed"));
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

#[test]
fn resolves_art_identifiers_and_separates_missing_from_invalid() {
    let temp = tempfile::tempdir().unwrap();
    world::write_corpus(temp.path());
    let run = || {
        Command::new(env!("CARGO_BIN_EXE_asset-audit"))
            .arg("--corpus-dir")
            .arg(temp.path())
            .arg("--validate-art-ids")
            .output()
            .unwrap()
    };
    let output = run();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let text = String::from_utf8_lossy(&output.stdout);
    assert!(text.contains(".pro\t8\t0\t0"), "{text}");
    assert!(text.contains(".map\t5\t0\t0"), "{text}");
    assert!(text.contains("Art identifiers: 13 resolved, 0 missing, 0 invalid"));

    // A decodable identifier whose file is absent is reported, not fatal.
    std::fs::remove_file(temp.path().join("art/critters/sampleaa.frm")).unwrap();
    let output = run();
    assert!(output.status.success());
    let errors = String::from_utf8_lossy(&output.stderr);
    assert!(errors.contains(
        "MISSING proto/critters/synthetic.pro: art 0x01000000: art/critters/sampleaa.frm"
    ));
    assert!(errors.contains("MISSING maps/synthetic.map: object art 0x01000000"));
    assert!(
        String::from_utf8_lossy(&output.stdout)
            .contains("Art identifiers: 11 resolved, 2 missing, 0 invalid")
    );

    // Weapon code 11 fails the codec before suffix selection.
    let mut critter = world::prototype(1, 0);
    world::word(&mut critter, 8, 0x0100B000);
    std::fs::write(temp.path().join("proto/critters/synthetic.pro"), critter).unwrap();
    let output = run();
    assert!(!output.status.success());
    let errors = String::from_utf8_lossy(&output.stderr);
    assert!(
        errors.contains(
            "FAIL proto/critters/synthetic.pro: art 0x0100b000: critter weapon-animation code 11"
        ),
        "{errors}"
    );
    assert!(
        String::from_utf8_lossy(&output.stdout)
            .contains("Art identifiers: 11 resolved, 1 missing, 1 invalid")
    );
}

#[test]
fn round_trips_prototypes_and_maps_through_the_spec003_writers() {
    let temp = tempfile::tempdir().unwrap();
    world::write_corpus(temp.path());
    let run = || {
        Command::new(env!("CARGO_BIN_EXE_asset-audit"))
            .arg("--corpus-dir")
            .arg(temp.path())
            .arg("--validate-writers")
            .output()
            .unwrap()
    };
    let output = run();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let text = String::from_utf8_lossy(&output.stdout);
    assert!(
        text.contains(".pro\t6\t0") && text.contains(".map\t1\t0"),
        "{text}"
    );
    assert!(text.contains("Writers: 7 passed, 0 failed"));
    std::fs::write(
        temp.path().join("proto/walls/synthetic.pro"),
        &world::prototype(3, 0)[..20],
    )
    .unwrap();
    let output = run();
    assert!(!output.status.success());
    assert!(
        String::from_utf8_lossy(&output.stderr).contains("FAIL proto/walls/synthetic.pro: PRO")
    );
    assert!(String::from_utf8_lossy(&output.stdout).contains("Writers: 6 passed, 1 failed"));
}
