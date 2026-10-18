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

#[test]
fn validates_spec008_working_saves_and_slot_round_trip() {
    let temp = tempfile::tempdir().unwrap();
    state::write_corpus(temp.path());
    // The fixed SPEC-008 tables reach campaign index 600.
    let campaign: String = (0..618)
        .map(|index| format!("global_{index} = {index};\n"))
        .collect();
    std::fs::write(
        temp.path().join("data/vault13.gam"),
        format!("GAME_GLOBAL_VARS:\n{campaign}"),
    )
    .unwrap();
    // Manifest names are bounded to twelve bytes (8.3 map names).
    std::fs::rename(
        temp.path().join("maps/synthetic.map"),
        temp.path().join("maps/synth.map"),
    )
    .unwrap();
    let output = Command::new(env!("CARGO_BIN_EXE_asset-audit"))
        .arg("--corpus-dir")
        .arg(temp.path())
        .arg("--validate-saves")
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let text = String::from_utf8_lossy(&output.stdout);
    assert!(text.contains("highest fixed SPEC-008 index: 600"), "{text}");
    assert!(
        text.contains("Saves: 2 maps entered, 2 working saves, 0 failed"),
        "{text}"
    );
    assert!(
        text.contains("Save slot round trip: restored SYNTH.SAV"),
        "{text}"
    );
}

#[test]
fn validates_spec009_packets_critter_resolution_and_taunt_ranges() {
    let temp = tempfile::tempdir().unwrap();
    state::write_corpus(temp.path());
    let section: String = vault_formats::ai::KEYS
        .iter()
        .enumerate()
        .map(|(index, key)| match *key {
            "packet_num" => "packet_num=0\r\n".to_string(),
            "hurt_too_much" => "hurt_too_much=blind ,crippled\r\n".to_string(),
            // Range starts 1000, 1010, ..., last_msg 1119.
            _ => format!("{key}={}\r\n", 1000 + 10 * (index as i32 - 12).max(0)),
        })
        .collect();
    std::fs::write(
        temp.path().join("data/ai.txt"),
        format!("[Synthetic]\r\n{section}"),
    )
    .unwrap();
    std::fs::create_dir_all(temp.path().join("text/english/game")).unwrap();
    std::fs::write(
        temp.path().join("text/english/game/combatai.msg"),
        b"{1000}{}{run}\n{1075}{}{torso}\n",
    )
    .unwrap();
    // A second critter prototype names an undefined packet.
    let mut other = world::prototype(1, 0);
    world::word(&mut other, 0x24, 9);
    std::fs::write(temp.path().join("proto/critters/other.pro"), other).unwrap();
    let run = || {
        Command::new(env!("CARGO_BIN_EXE_asset-audit"))
            .arg("--corpus-dir")
            .arg(temp.path())
            .arg("--validate-ai")
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
        text.contains("AI packets: 1 parsed, 1 token diagnostics"),
        "{text}"
    );
    assert!(
        text.contains("Critter AI packets: 1 resolved, 1 fall back to entry 0"),
        "{text}"
    );
    assert!(
        text.contains("Taunt ranges: 2 with catalog lines, 10 without"),
        "{text}"
    );
    // A packet missing a key fails initialization.
    std::fs::write(
        temp.path().join("data/ai.txt"),
        format!("[Synthetic]\r\n{}", section.replace("last_msg", "last")),
    )
    .unwrap();
    let output = run();
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("last_msg"));
}

#[allow(dead_code)]
#[path = "../../../tests/fixtures/sound/support.rs"]
mod sound;

fn sha256(bytes: &[u8]) -> String {
    use sha2::Digest;
    sha2::Sha256::digest(bytes)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

#[test]
fn validates_spec010_acm_hashes_and_sfx_resolution_against_oracle_tables() {
    let temp = tempfile::tempdir().unwrap();
    let corpus = temp.path().join("corpus");
    state::write_corpus(&corpus);
    // Item sound byte `B` (0x38) on the container; the door's scenery
    // byte (0x28) is 0.
    for (path, offset, byte) in [
        ("proto/items/synthetic.pro", 0x38, b'B'),
        ("proto/scenery/synthetic.pro", 0x28, 0),
    ] {
        let mut bytes = std::fs::read(corpus.join(path)).unwrap();
        bytes[offset] = byte;
        std::fs::write(corpus.join(path), bytes).unwrap();
    }
    std::fs::create_dir_all(corpus.join("sound/sfx")).unwrap();
    // Vectors A, B, and H, plus A again as the only open/close name.
    let files = [
        ("a.acm", 'A', &[300, -400, 0, 100][..], 1, 0),
        ("b.acm", 'B', &[150, 300, 50, -200, -100, 0, 0], 2, 0),
        ("h.acm", 'H', &[7, -7, 0, 0], 1, 1),
        ("IOCNTNRB.acm", 'A', &[300, -400, 0, 100], 1, 0),
    ];
    let oracle = temp.path().join("oracle");
    std::fs::create_dir_all(&oracle).unwrap();
    let mut rows = String::from("# path\tsource sha256\t...\n");
    for (name, vector, samples, blocks, past) in files {
        let bytes = sound::acm(vector);
        std::fs::write(corpus.join("sound/sfx").join(name), &bytes).unwrap();
        let pcm: Vec<u8> = samples
            .iter()
            .flat_map(|sample: &i16| sample.to_le_bytes())
            .collect();
        let list = |samples: &[i16]| {
            samples
                .iter()
                .map(i16::to_string)
                .collect::<Vec<_>>()
                .join(",")
        };
        let header = (bytes[12] as u16) | (bytes[13] as u16) << 8;
        rows.push_str(&format!(
            "sound/sfx/{}\t{}\t{}\t{}\t1\t22050\t{}\t{}\t{blocks}\t{past}\t{}\t{}\t{}\n",
            name.to_ascii_lowercase(),
            sha256(&bytes),
            bytes.len(),
            samples.len(),
            header & 15,
            header >> 4,
            sha256(&pcm),
            list(&samples[..samples.len().min(8)]),
            list(&samples[samples.len().saturating_sub(4)..]),
        ));
    }
    std::fs::write(oracle.join("acm-corpus.tsv"), &rows).unwrap();
    std::fs::write(
        oracle.join("acm-wrap.tsv"),
        "# path\tindex\tshifted\temitted\n",
    )
    .unwrap();
    std::fs::write(
        oracle.join("sfx-resolution.tsv"),
        "# kind\tcontext\trequested\tresolved\tattempts\n\
         literal\tbutton\tiocntnrb\tIOCNTNRB\tIOCNTNRB\n\
         alias\tprefix\tMALIEUAB\t-\tMALIEUAB > MAMTNTAB\n\
         character\tlist 0\tSAMPLEAA\t-\tSAMPLEAA\n\
         weapon\tcode A\tWRA1XXX1\t-\tWRA1XXX1\n",
    )
    .unwrap();
    std::fs::write(
        oracle.join("sfx-proto-bytes.tsv"),
        "# category\tsubtype\tsound byte\tcount\tnames\n\
         items\t1\tB\t1\tIOCNTNRB\n\
         scenery\t0\t0\t1\t-\n\
         # weapon sound code (file offset 0x79) distribution: \n",
    )
    .unwrap();
    let run = |flag: &str| {
        Command::new(env!("CARGO_BIN_EXE_asset-audit"))
            .arg("--corpus-dir")
            .arg(&corpus)
            .arg("--oracle-dir")
            .arg(&oracle)
            .arg(flag)
            .output()
            .unwrap()
    };
    let output = run("--validate-acm");
    let text = String::from_utf8_lossy(&output.stdout);
    assert!(
        output.status.success(),
        "{text}{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(
        text.contains("ACM files: 4 in corpus, 4 oracle rows"),
        "{text}"
    );
    assert!(
        text.contains("ACM decoding: 4 passed, 0 failed; 0 wrapped samples matched"),
        "{text}"
    );
    let output = run("--validate-sfx");
    let text = String::from_utf8_lossy(&output.stdout);
    assert!(
        output.status.success(),
        "{text}{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(
        text.contains("SFX resolution: 4 rows, 4 passed, 0 failed"),
        "{text}"
    );
    assert!(
        text.contains("SFX prototype sound bytes: 0 failures"),
        "{text}"
    );

    // A wrong PCM hash, or a request the builders cannot produce, fails.
    let corrupted = rows.replacen(
        &sha256(&[44, 1, 112, 254, 0, 0, 100, 0]),
        &"0".repeat(64),
        1,
    );
    std::fs::write(oracle.join("acm-corpus.tsv"), corrupted).unwrap();
    let output = run("--validate-acm");
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("PCM SHA-256"));
    std::fs::write(
        oracle.join("sfx-resolution.tsv"),
        "weapon\tcode A\tWRB1XXX1\t-\tWRB1XXX1\n",
    )
    .unwrap();
    assert!(!run("--validate-sfx").status.success());
}
