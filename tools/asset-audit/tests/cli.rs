use std::process::Command;

use vault_fixtures::formats as fixtures;
use vault_fixtures::{state, vm, world};

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
    // A value-less assignment stores zero under the SPEC-006 safe rule.
    assert!(!errors.contains("vault13.gam"));
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
    // The fixture corpus's `map.msg` (SPEC-008 §3.10) validates as well.
    assert!(String::from_utf8_lossy(&output.stdout).contains("Validation: 44 passed, 0 failed"));
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
    assert!(String::from_utf8_lossy(&output.stdout).contains("Validation: 41 passed, 3 failed"));
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
fn a_message_file_stopping_early_is_reported_not_failed() {
    // SPEC-002 §4.5: `{1}{}{ok}}` fails after inserting ID 1, which stays.
    let temp = tempfile::tempdir().unwrap();
    fixtures::write_corpus(temp.path());
    let dir = temp.path().join("text/english/game");
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("early.msg"), b"{1}{}{ok}}{2}{}{lost}").unwrap();
    let (success, _, stdout, stderr) = audit(temp.path(), &["--validate-formats"]);
    assert!(success, "{stderr}");
    assert!(
        stdout.contains("Validation: 17 passed, 0 failed"),
        "{stdout}"
    );
    assert!(stdout.contains("MSG STOPS EARLY text/english/game/early.msg: 1 record(s) kept"));
    assert!(stdout.contains("Message files stopping early (SPEC-002 §3.5): 1"));
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
    // SPEC-008 §3.10: a map's number comes from its header name matched
    // against `map.msg`. The dialogue map is a copy of the fixture map, so
    // it gets its own header name and number; a third map `map.msg` does
    // not list is unnumbered and never joins the working set.
    let named = |name: &str, number: i32| {
        let mut map = state::interactive_map();
        map[4..20].fill(0);
        map[4..4 + name.len()].copy_from_slice(name.as_bytes());
        world::word(&mut map, 0x34, number);
        map
    };
    std::fs::write(
        temp.path().join("maps/dialogue.map"),
        named("DIALOGUE.MAP", 8),
    )
    .unwrap();
    std::fs::write(
        temp.path().join("maps/zunlistd.map"),
        named("ZUNLISTD.MAP", -1),
    )
    .unwrap();
    std::fs::write(
        temp.path().join("text/english/game/map.msg"),
        "{7}{}{SYNTH.MAP}\n{8}{}{DIALOGUE.MAP}\n",
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
        text.contains("Saves: 3 maps entered, 2 working saves, 0 failed"),
        "{text}"
    );
    assert!(
        text.contains("Save slot round trip: restored SYNTH.SAV"),
        "{text}"
    );
}

#[test]
fn validates_spec008_map_numbers_against_map_msg_and_the_fixed_table() {
    use vault_core::worldmap::MAP_TABLE;
    let temp = tempfile::tempdir().unwrap();
    state::write_corpus(temp.path());
    std::fs::remove_file(temp.path().join("maps/synthetic.map")).unwrap();
    std::fs::remove_file(temp.path().join("maps/dialogue.map")).unwrap();
    // The 66 numbered maps and the six unlisted ones, each a header whose
    // name field and word 0x34 follow SPEC-008 §3.10.
    let header = |name: &str, number: i32| {
        let mut map = world::empty_map();
        map[4..20].fill(0);
        map[4..4 + name.len()].copy_from_slice(name.as_bytes());
        world::word(&mut map, 0x34, number);
        map
    };
    let unlisted = [
        "CARAVAN.MAP",
        "JUNKDEMO.MAP",
        "TEMPLAT1.MAP",
        "ZDESERT1.MAP",
        "ZDESERT2.MAP",
        "ZDESERT3.MAP",
    ];
    for (number, entry) in MAP_TABLE.iter().enumerate() {
        std::fs::write(
            temp.path()
                .join(format!("maps/{}", entry.name.to_ascii_lowercase())),
            header(entry.name, number as i32),
        )
        .unwrap();
    }
    for name in unlisted {
        std::fs::write(
            temp.path()
                .join(format!("maps/{}", name.to_ascii_lowercase())),
            header(name, -1),
        )
        .unwrap();
    }
    let catalog: String = MAP_TABLE
        .iter()
        .enumerate()
        .map(|(number, entry)| format!("{{{number}}}{{}}{{{}}}\n", entry.name))
        .collect();
    std::fs::write(temp.path().join("text/english/game/map.msg"), &catalog).unwrap();
    let run = || {
        Command::new(env!("CARGO_BIN_EXE_asset-audit"))
            .arg("--corpus-dir")
            .arg(temp.path())
            .arg("--validate-map-numbers")
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
        text.contains("Map numbers map.msg: 66 passed, 0 failed"),
        "{text}"
    );
    assert!(
        text.contains("Map numbers maps present: 73 passed, 0 failed"),
        "{text}"
    );
    assert!(
        text.contains("Map numbers header word 0x34: 72 passed, 0 failed"),
        "{text}"
    );
    assert!(
        text.contains("Map numbers SPEC-008 §4.5 vectors: 6 passed, 0 failed"),
        "{text}"
    );
    // A map whose word disagrees with the number its name takes, an extra
    // map, and a `map.msg` text that differs from the table all fail.
    std::fs::write(temp.path().join("maps/hubent.map"), header("HUBENT.MAP", 7)).unwrap();
    std::fs::write(temp.path().join("maps/extra.map"), header("EXTRA.MAP", -1)).unwrap();
    std::fs::write(
        temp.path().join("text/english/game/map.msg"),
        catalog.replace("{0}{}{DESERT1.MAP}", "{0}{}{DESERT9.MAP}"),
    )
    .unwrap();
    let output = run();
    assert!(!output.status.success());
    let errors = String::from_utf8_lossy(&output.stderr);
    assert!(
        errors.contains("FAIL header word 0x34: HUBENT.MAP: header word 7 != 36"),
        "{errors}"
    );
    assert!(
        errors.contains("FAIL maps present: EXTRA.MAP: not a corpus map"),
        "{errors}"
    );
    assert!(errors.contains("FAIL map.msg: message 0:"), "{errors}");
    assert!(
        errors.contains("FAIL maps present: count: 73 maps, expected 72"),
        "{errors}"
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
    // The taunt ranges come from the packet, so an enormous range
    // (here the last hit range up to i32::MAX) is looked up, not scanned.
    std::fs::write(
        temp.path().join("data/ai.txt"),
        format!(
            "[Synthetic]\r\n{}",
            section.replace("last_msg=1119", "last_msg=2147483647")
        ),
    )
    .unwrap();
    let output = run();
    assert!(output.status.success());
    let text = String::from_utf8_lossy(&output.stdout);
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

use vault_fixtures::sound;

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

#[test]
fn validates_spec011_item_invariants_and_reports_violations() {
    let temp = tempfile::tempdir().unwrap();
    let items = temp.path().join("proto/items");
    std::fs::create_dir_all(&items).unwrap();
    let write = |pid: u32, bytes: &[u8]| {
        std::fs::write(items.join(format!("{pid:08}.pro")), bytes).unwrap();
    };
    let drug = |pid: u32, selectors: [i32; 3], addiction: i32, perk: i32| {
        let mut bytes = world::prototype(0, 2);
        world::word(&mut bytes, 0, pid as i32);
        for (slot, selector) in selectors.into_iter().enumerate() {
            world::word(&mut bytes, 0x39 + 4 * slot, selector);
        }
        world::word(&mut bytes, 0x45, 5);
        world::word(&mut bytes, 0x71, addiction);
        world::word(&mut bytes, 0x75, perk);
        bytes
    };
    for pid in [40, 144] {
        write(pid, &drug(pid, [35, -1, -1], 0, -1));
    }
    for pid in [106, 87, 53, 110, 48] {
        write(pid, &drug(pid, [0, -1, -1], 10, 54));
    }
    for pid in [124, 125] {
        write(pid, &drug(pid, [0, -1, -1], 0, -1));
    }
    let mut money = world::prototype(0, 5);
    world::word(&mut money, 0, 41);
    world::word(&mut money, 0x30, 1);
    write(41, &money);
    for (pid, text) in [
        (
            0x400,
            include_str!("../../../tests/fixtures/items/drug-A.pro.hex"),
        ),
        (
            0x401,
            include_str!("../../../tests/fixtures/items/drug-B.pro.hex"),
        ),
        (
            0x500,
            include_str!("../../../tests/fixtures/items/ammo-X.pro.hex"),
        ),
        (
            0x501,
            include_str!("../../../tests/fixtures/items/weapon-W.pro.hex"),
        ),
        (
            0x502,
            include_str!("../../../tests/fixtures/items/armor-A.pro.hex"),
        ),
        (
            0x503,
            include_str!("../../../tests/fixtures/items/armor-B.pro.hex"),
        ),
        (
            0x504,
            include_str!("../../../tests/fixtures/items/container.pro.hex"),
        ),
    ] {
        write(pid, &fixtures::sparse(text));
    }
    // The item list maps each one-based PID index to its file.
    let list: String = (1..=0x504).map(|pid| format!("{pid:08}.pro\n")).collect();
    std::fs::write(items.join("items.lst"), list).unwrap();
    let game = temp.path().join("text/english/game");
    std::fs::create_dir_all(&game).unwrap();
    let perks: String = (101..=164)
        .chain(201..=264)
        .map(|id| format!("{{{id}}}{{}}{{perk text {id}}}\n"))
        .collect();
    std::fs::write(game.join("perk.msg"), perks).unwrap();
    // The engine check consumes each drug with campaign variables and the
    // stage messages installed.
    std::fs::create_dir_all(temp.path().join("data")).unwrap();
    std::fs::write(
        temp.path().join("data/vault13.gam"),
        include_bytes!("../../../tests/fixtures/state/campaign.gam"),
    )
    .unwrap();
    std::fs::write(
        game.join("item.msg"),
        "{1}{}{gained %d %s}\n{2}{}{lost %d %s}\n{3}{}{over}\n{10}{}{nothing}\n",
    )
    .unwrap();
    let stats: String = (100..=137)
        .map(|id| format!("{{{id}}}{{}}{{stat {id}}}\n"))
        .collect();
    std::fs::write(game.join("stat.msg"), stats).unwrap();
    let run = || {
        Command::new(env!("CARGO_BIN_EXE_asset-audit"))
            .arg("--corpus-dir")
            .arg(temp.path())
            .arg("--validate-items")
            .output()
            .unwrap()
    };
    let output = run();
    let text = String::from_utf8_lossy(&output.stdout);
    assert!(
        output.status.success(),
        "{text}{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(
        text.contains("Item invariants: 18 passed, 0 failed"),
        "{text}"
    );
    assert!(
        text.contains("Item prototypes: 17 (armor 2, container 1, drug 11, weapon 1, ammunition 1, misc 1, key 0)"),
        "{text}"
    );
    assert!(
        text.contains("range-form drugs: 1 (§4.12 survey reported 2)"),
        "{text}"
    );
    assert!(
        text.contains("Drug consumption: 11 consumed, 2 delayed stages queued, 3 healed, 0 failed"),
        "{text}"
    );
    // EMP DR at or below 100 and an addictive drug without a withdrawal
    // perk both violate the invariants.
    let mut armour = fixtures::sparse(include_str!(
        "../../../tests/fixtures/items/armor-A.pro.hex"
    ));
    world::word(&mut armour, 0x39 + 4 * (1 + 5), 50);
    write(0x502, &armour);
    write(106, &drug(106, [0, -1, -1], 10, -1));
    let output = run();
    assert!(!output.status.success());
    let errors = String::from_utf8_lossy(&output.stderr);
    assert!(
        errors.contains("FAIL armor DR is 0-90 (EMP above 100)"),
        "{errors}"
    );
    assert!(
        errors.contains("FAIL addictive drugs name withdrawal perks 53-57"),
        "{errors}"
    );
    assert!(errors.contains("2 item invariant(s) failed"), "{errors}");
    // Without armor the three armor invariants have nothing to check: they
    // stay in the table, marked, instead of dropping out of it.
    for pid in [0x502, 0x503] {
        std::fs::remove_file(items.join(format!("{pid:08}.pro"))).unwrap();
    }
    let output = run();
    assert!(!output.status.success());
    let text = String::from_utf8_lossy(&output.stdout);
    for line in [
        "armor perks are -1, 62, or 63\t0\t0",
        "NOT EXERCISED armor DT is 0-100",
        "Item invariants: 14 passed, 1 failed, 3 not exercised",
    ] {
        assert!(text.contains(line), "{line}\n{text}");
    }
}

#[test]
fn interaction_and_ui_audits_report_categories_and_fail_on_a_partial_corpus() {
    let temp = tempfile::tempdir().unwrap();
    state::write_corpus(temp.path());
    for flag in ["--validate-interactions", "--validate-ui"] {
        let output = Command::new(env!("CARGO_BIN_EXE_asset-audit"))
            .arg("--corpus-dir")
            .arg(temp.path())
            .arg(flag)
            .output()
            .unwrap();
        // The synthetic corpus has no interface art or catalogs: every
        // category reports its counts, the failures are listed, and the run
        // fails.
        assert!(!output.status.success(), "{flag}");
        let text = String::from_utf8_lossy(&output.stdout);
        let title = if flag == "--validate-ui" {
            "UI"
        } else {
            "Interactions"
        };
        assert!(text.contains(&format!("{title} messages:")), "{text}");
        assert!(text.contains(&format!("{title} art:")), "{text}");
        let errors = String::from_utf8_lossy(&output.stderr);
        assert!(errors.contains("FAIL messages:"), "{errors}");
        assert!(errors.contains("check(s) failed"), "{errors}");
    }
}

#[test]
fn validates_spec014_roof_words_and_stored_hide_bits_across_the_corpus_maps() {
    use vault_core::worldmap::MAP_TABLE;
    let temp = tempfile::tempdir().unwrap();
    state::write_corpus(temp.path());
    std::fs::remove_file(temp.path().join("maps/synthetic.map")).unwrap();
    std::fs::remove_file(temp.path().join("maps/dialogue.map")).unwrap();
    // A map with an elevation-0 tile block whose first `roofed` squares
    // carry roof art `index`, the first `hidden` of them with the stored
    // hide bit and the first `barrier` with the barrier bit.
    let roofed_map = |name: &str, roofed: u32, index: u32, hidden: u32, barrier: u32| {
        let mut map = world::empty_map();
        map.truncate(248);
        map[4..20].fill(0);
        map[4..4 + name.len()].copy_from_slice(name.as_bytes());
        world::word(&mut map, 0x28, 0x0D);
        for square in 0..10000_u32 {
            let word = if square < roofed {
                (index << 16)
                    | 1
                    | if square < hidden { 0x1000_0000 } else { 0 }
                    | if square < barrier { 0x2000_0000 } else { 0 }
            } else {
                0x0001_0001
            };
            map.extend(word.to_be_bytes());
        }
        map.extend(world::words(&[0; 5]));
        map.extend(world::words(&[0; 4]));
        map
    };
    let unlisted = [
        "CARAVAN.MAP",
        "JUNKDEMO.MAP",
        "TEMPLAT1.MAP",
        "ZDESERT1.MAP",
        "ZDESERT2.MAP",
        "ZDESERT3.MAP",
    ];
    let names: Vec<&str> = MAP_TABLE
        .iter()
        .map(|entry| entry.name)
        .chain(unlisted)
        .collect();
    assert_eq!(names.len(), 72);
    // 71 maps with 208 roofed squares and LAGUNRUN.MAP with 288, 257 of
    // them stored hidden: 15056 in all.
    for name in &names {
        let (roofed, hidden) = if *name == "LAGUNRUN.MAP" {
            (288, 257)
        } else {
            (208, 0)
        };
        std::fs::write(
            temp.path()
                .join(format!("maps/{}", name.to_ascii_lowercase())),
            roofed_map(name, roofed, 5, hidden, 0),
        )
        .unwrap();
    }
    let run = || {
        Command::new(env!("CARGO_BIN_EXE_asset-audit"))
            .arg("--corpus-dir")
            .arg(temp.path())
            .arg("--validate-roofs")
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
    for line in [
        "Roofs: 72 maps, 15056 roofed squares, 257 stored hide bits",
        "Roofs roof index 0: 72 passed, 0 failed",
        "Roofs barrier bit: 72 passed, 0 failed",
        "Roofs stored hide bits: 72 passed, 0 failed",
        "Roofs hidden after parse: 72 passed, 0 failed",
        "Roofs roofed squares: 1 passed, 0 failed",
    ] {
        assert!(text.contains(line), "{line}\n{text}");
    }
    // Roof index 0, a barrier bit, a stored hide bit outside LAGUNRUN.MAP
    // and a changed total all fail.
    std::fs::write(
        temp.path().join("maps/hubent.map"),
        roofed_map("HUBENT.MAP", 209, 0, 1, 1),
    )
    .unwrap();
    let output = run();
    assert!(!output.status.success());
    let errors = String::from_utf8_lossy(&output.stderr);
    for line in [
        "FAIL roof index 0: HUBENT.MAP: 209 roofed square(s) with index 0",
        "FAIL barrier bit: HUBENT.MAP: 1 square(s) with the barrier bit",
        "FAIL stored hide bits: HUBENT.MAP: 1 stored hide bit(s), expected 0",
        "FAIL roofed squares: total: 15057 != 15056",
    ] {
        assert!(errors.contains(line), "{line}\n{errors}");
    }
}

/// SPEC-010 §2.6: every `.LIP` against its layout and companion ACM.
#[test]
fn validates_spec010_lip_files_against_their_audio() {
    let temp = tempfile::tempdir().unwrap();
    state::write_corpus(temp.path());
    let speech = temp.path().join("sound/speech/synth");
    std::fs::create_dir_all(&speech).unwrap();
    // Vector H: 4 samples, so 2N = 8.
    std::fs::write(speech.join("hello.acm"), sound::acm('H')).unwrap();
    let mut lip = Vec::new();
    for word in [2, 0x5800, 0, 0, 9, 2, 0, 3] {
        lip.extend_from_slice(&i32::to_be_bytes(word));
    }
    lip.extend_from_slice(b"HELLO\0\0\0VOC\0");
    lip.extend_from_slice(&[0, 9]);
    for (kind, position) in [(1, 0), (0, 2), (1, 10)] {
        lip.extend_from_slice(&i32::to_be_bytes(kind));
        lip.extend_from_slice(&i32::to_be_bytes(position));
    }
    std::fs::write(speech.join("hello.lip"), &lip).unwrap();
    std::fs::write(speech.join("short.lip"), &lip[..50]).unwrap();
    let (ok, _, stdout, stderr) = audit(temp.path(), &["--validate-lip"]);
    assert!(!ok);
    assert!(
        stdout.contains("LIP files: 2, 1 passed, 1 failed; 0 end before the audio"),
        "{stdout}"
    );
    assert!(
        stderr.contains("FAIL sound/speech/synth/short.lip"),
        "{stderr}"
    );
    std::fs::remove_file(speech.join("short.lip")).unwrap();
    // A final marker before the audio end is counted.
    let last = lip.len() - 1;
    lip[last] = 6;
    std::fs::write(speech.join("hello.lip"), &lip).unwrap();
    let (ok, _, stdout, stderr) = audit(temp.path(), &["--validate-lip"]);
    assert!(ok, "{stdout}{stderr}");
    assert!(
        stdout.contains("LIP files: 1, 1 passed, 0 failed; 1 end before the audio"),
        "{stdout}"
    );
}

fn audit(corpus: &std::path::Path, flags: &[&str]) -> (bool, Option<i32>, String, String) {
    let output = Command::new(env!("CARGO_BIN_EXE_asset-audit"))
        .arg("--corpus-dir")
        .arg(corpus)
        .args(flags)
        .output()
        .unwrap();
    (
        output.status.success(),
        output.status.code(),
        String::from_utf8_lossy(&output.stdout).into_owned(),
        String::from_utf8_lossy(&output.stderr).into_owned(),
    )
}

#[test]
fn every_requested_category_runs_and_reports_before_the_failing_exit() {
    let temp = tempfile::tempdir().unwrap();
    world::write_corpus(temp.path());
    std::fs::write(
        temp.path().join("proto/walls/synthetic.pro"),
        &world::prototype(3, 0)[..20],
    )
    .unwrap();
    // The movie category stops at its missing oracle; the others still run
    // and print their tallies, and one bad PRO does not end the art audit.
    let (success, code, text, errors) = audit(
        temp.path(),
        &[
            "--validate-movies",
            "--movie-oracle-dir",
            "no-such-oracle",
            "--validate-formats",
            "--validate-writers",
            "--validate-art-ids",
        ],
    );
    assert!(!success && code == Some(1), "{errors}");
    for line in [
        "Validation: 42 passed, 1 failed",
        "Writers: 6 passed, 1 failed",
        "Art identifiers: 12 resolved, 0 missing, 0 invalid",
        "Art-id source files: 1 unreadable",
        "Audit categories: 4 run, 4 failed",
    ] {
        assert!(text.contains(line), "{line}\n{text}");
    }
    for line in [
        // (the path separator is the platform's)
        "asset-audit: Movies: no-such-oracle",
        "mve-summary.tsv: ",
        "asset-audit: 1 asset(s) in format validation failed",
        "asset-audit: 1 writer round trip(s) failed",
        "asset-audit: 1 art identifier check(s) failed",
        "asset-audit: 4 of 4 categories failed",
    ] {
        assert!(errors.contains(line), "{line}\n{errors}");
    }
}

#[test]
fn validates_spec008_archive_members_against_the_loose_corpus() {
    let temp = tempfile::tempdir().unwrap();
    let corpus = temp.path().join("game");
    std::fs::create_dir_all(&corpus).unwrap();
    // SPEC-008 §4.1: the synthetic archive holds PACK.TXT and PLAIN.TXT.
    std::fs::write(
        temp.path().join("MASTER.DAT"),
        fixtures::sparse(include_str!(
            "../../../tests/fixtures/archive/synthetic.dat.hex"
        )),
    )
    .unwrap();
    std::fs::write(corpus.join("pack.txt"), b"ABCABCABC").unwrap();
    std::fs::write(corpus.join("plain.txt"), b"XYZ").unwrap();
    std::fs::write(corpus.join("loose.txt"), b"only loose").unwrap();
    let master = temp.path().join("MASTER.DAT");
    let master = master.to_str().unwrap();

    // The default CRITTER.DAT beside the corpus is absent: that archive
    // fails, and the master archive is still audited.
    let (success, _, text, errors) = audit(&corpus, &["--validate-dat"]);
    assert!(!success);
    assert!(
        text.contains("MASTER.DAT: 2 members, 2 matched, 0 differ, 0 missing loose, 0 failed"),
        "{text}"
    );
    assert!(
        text.contains("CRITTER.DAT: 0 members, 0 matched, 0 differ, 0 missing loose, 1 failed"),
        "{text}"
    );
    assert!(text.contains("Loose files: 3, 1 supplied by no archive"));
    assert!(errors.contains("FAIL "), "{errors}");
    assert!(errors.contains("asset-audit: 1 archive member check(s) failed"));

    let both = ["--validate-dat", "--critter-dat", master];
    let (success, _, text, errors) = audit(&corpus, &both);
    assert!(success, "{errors}");
    assert!(text.contains("Loose files: 3, 1 supplied by no archive"));

    // A loose copy that differs, and a member with no loose file.
    std::fs::write(corpus.join("plain.txt"), b"XYW").unwrap();
    std::fs::remove_file(corpus.join("pack.txt")).unwrap();
    let (success, _, text, errors) = audit(&corpus, &both);
    assert!(!success);
    assert!(
        text.contains("2 members, 0 matched, 1 differ, 1 missing loose, 0 failed"),
        "{text}"
    );
    assert!(errors.contains("DIFF "), "{errors}");
    assert!(errors.contains("MISSING "), "{errors}");
    assert!(errors.contains("asset-audit: 4 archive member check(s) failed"));
}

/// SPEC-012 oracle tables for the synthetic movie A, built from its
/// expected-hash table (`oracle/SPEC-012/synthetic`), with `summary` as the
/// summary row's values after the file name.
fn movie_oracle(dir: &std::path::Path, summary: &[String], subtitles: &str) {
    let (_, expected, _) = movie::synthetic('A');
    let value = |cell: &str| cell.split_once('=').unwrap().1.to_owned();
    let (mut frames, mut audio) = (String::from("header\n"), String::from("header\n"));
    let mut index = 0;
    for line in expected.lines() {
        let cells: Vec<&str> = line.split('\t').collect();
        match cells[0] {
            "frame" => {
                let (start, count) = value(cells[4])
                    .split_once('+')
                    .map(|(a, b)| (a.to_owned(), b.to_owned()))
                    .unwrap();
                frames += &format!(
                    "synthetic-a.mve\t{}\t{}\t{}\t{start}\t{count}\t{}\t{}\t\n",
                    cells[1],
                    value(cells[2]),
                    value(cells[3]),
                    &value(cells[5])[..16],
                    &value(cells[6])[..16],
                );
            }
            "audio" => {
                index += 1;
                audio += &format!(
                    "synthetic-a.mve\t{index}\t{}\t{}\t{}\n",
                    value(cells[1]),
                    value(cells[2]),
                    &value(cells[3])[..16],
                );
            }
            _ => {}
        }
    }
    std::fs::create_dir_all(dir).unwrap();
    std::fs::write(
        dir.join("mve-summary.tsv"),
        format!("header\nsynthetic-a.mve\t{}\n", summary.join("\t")),
    )
    .unwrap();
    std::fs::write(dir.join("mve-frames.tsv"), frames).unwrap();
    std::fs::write(dir.join("mve-audio.tsv"), audio).unwrap();
    std::fs::write(dir.join("moviefx.tsv"), "header\n").unwrap();
    std::fs::write(
        dir.join("sve-subtitles.tsv"),
        format!("header\n{subtitles}"),
    )
    .unwrap();
}

use vault_fixtures::movie;

#[test]
fn validates_spec012_movies_fades_and_subtitles_against_oracle_tables() {
    let temp = tempfile::tempdir().unwrap();
    let corpus = temp.path().join("game");
    let oracle = temp.path().join("oracle");
    std::fs::create_dir_all(corpus.join("art/cuts")).unwrap();
    std::fs::create_dir_all(corpus.join("text/english/cuts")).unwrap();
    let (bytes, expected, _) = movie::synthetic('A');
    std::fs::write(corpus.join("art/cuts/synthetic-a.mve"), bytes).unwrap();
    // Fixture G has no movie of its name: parsed, reported, and skipped.
    std::fs::write(corpus.join("art/cuts/fixture-g.cfg"), movie::fades()).unwrap();
    std::fs::write(
        corpus.join("text/english/cuts/synthetic-a.sve"),
        b"1:Hello\r\n3:World\r\n",
    )
    .unwrap();
    let subtitles = format!(
        "english\tsynthetic-a.sve\t1\t1\t1\t5\t{}\nenglish\tsynthetic-a.sve\t2\t3\t3\t5\t{}\n",
        &sha256(b"Hello")[..16],
        &sha256(b"World")[..16],
    );
    let pcm = expected
        .lines()
        .find(|line| line.starts_with("pcm_total"))
        .unwrap();
    let pcm: Vec<&str> = pcm
        .split('\t')
        .map(|cell| cell.split_once('=').map_or(cell, |(_, v)| v))
        .collect();
    // Size, file hash, frame size, and PCM come from the fixture; the
    // decoder's own counts (`?`) are filled in from its mismatch reports,
    // one field per run, and only those fields may be filled that way.
    let mut summary: Vec<String> = [
        bytes.len().to_string(),
        sha256(bytes),
        "64".into(),
        "64".into(),
    ]
    .into_iter()
    .chain(std::iter::repeat_n("?".to_string(), 10))
    .chain([pcm[1].to_owned(), pcm[2].to_owned()])
    .chain(std::iter::repeat_n("?".to_string(), 2))
    .collect();
    let unknown = [
        "chunks",
        "frames",
        "video frames",
        "timer rate",
        "timer subdivision",
        "audio rate",
        "stereo",
        "16-bit",
        "compressed",
        "audio buffer",
        "block counts",
        "opcode counts",
    ];
    let flags = [
        "--validate-movies",
        "--movie-oracle-dir",
        oracle.to_str().unwrap(),
    ];
    let mut filled = 0;
    let (text, errors) = loop {
        movie_oracle(&oracle, &summary, &subtitles);
        let (success, _, text, errors) = audit(&corpus, &flags);
        if success {
            break (text, errors);
        }
        let report = errors
            .lines()
            .find_map(|line| line.strip_prefix("FAIL art/cuts/synthetic-a.mve: "))
            .unwrap_or_else(|| panic!("{errors}"));
        let (name, rest) = report.split_once(": ").unwrap();
        let (value, wanted) = rest.split_once(" != ").unwrap();
        assert!(unknown.contains(&name) && wanted == "?", "{report}");
        let index = [
            "bytes",
            "file SHA-256",
            "width",
            "height",
            "chunks",
            "frames",
            "video frames",
            "timer rate",
            "timer subdivision",
            "audio rate",
            "stereo",
            "16-bit",
            "compressed",
            "audio buffer",
            "PCM bytes",
            "PCM SHA-256",
            "block counts",
            "opcode counts",
        ]
        .iter()
        .position(|field| *field == name)
        .unwrap();
        summary[index] = value.to_owned();
        filled += 1;
        assert!(filled <= unknown.len());
    };
    assert_eq!(filled, 12, "{errors}");
    for line in [
        "MVE files: 1 in corpus; summaries 1 passed, 0 failed",
        "MVE frames: 5 passed, 0 failed; audio opcodes: 6 passed, 0 failed",
        "Fade effects: 0 passed, 0 failed; 1 fade file(s) without a movie",
        "SVE files: 1 in corpus; entries 2 passed, 0 failed",
    ] {
        assert!(text.contains(line), "{line}\n{text}");
    }

    // A fade file that does not parse is one failed effect check,
    // and the movie and subtitles are still compared.
    std::fs::write(corpus.join("art/cuts/broken.cfg"), b"[info]\0\n").unwrap();
    let (success, _, text, errors) = audit(&corpus, &flags);
    assert!(!success);
    assert!(text.contains("Fade effects: 0 passed, 1 failed"), "{text}");
    assert!(text.contains("entries 2 passed, 0 failed"), "{text}");
    assert!(
        errors.contains("FAIL art/cuts/broken.cfg: CONFIG"),
        "{errors}"
    );
    std::fs::remove_file(corpus.join("art/cuts/broken.cfg")).unwrap();

    // A changed oracle hash fails that frame only.
    let frames = std::fs::read_to_string(oracle.join("mve-frames.tsv")).unwrap();
    let mut rows: Vec<String> = frames.lines().map(str::to_owned).collect();
    rows[2] = rows[2].replace("\t1\t0\t", "\t1\t1\t");
    std::fs::write(oracle.join("mve-frames.tsv"), rows.join("\n") + "\n").unwrap();
    let (success, _, text, _) = audit(&corpus, &flags);
    assert!(!success);
    assert!(text.contains("MVE frames: 4 passed, 1 failed"), "{text}");

    // A short user-supplied oracle row is an error, not a panic.
    movie_oracle(&oracle, &summary, "english\n");
    let (success, code, _, errors) = audit(&corpus, &flags);
    assert!(!success && code == Some(1), "{errors}");
    assert!(
        errors.contains("sve-subtitles.tsv line 2: 1 column(s), expected at least 2"),
        "{errors}"
    );
}

/// The first non-push instruction word at or after the code start of
/// `bytes`, by byte offset.
fn first_statement(bytes: &[u8]) -> usize {
    let program = vault_vm::IntProgram::parse(bytes).unwrap();
    let mut at = program.code_start as u32;
    loop {
        let instruction = vault_debug::decode::decode(program.image(), at).unwrap();
        if instruction.immediate.is_none() {
            return at as usize;
        }
        at = instruction.next_address();
    }
}

#[test]
fn validates_script_words_references_and_the_lifecycle_fallback() {
    let temp = tempfile::tempdir().unwrap();
    state::write_corpus(temp.path());
    let (success, _, text, errors) = audit(temp.path(), &["--validate-scripts"]);
    assert!(success, "{errors}");
    for line in [
        "Scripts: 2 files,",
        "0 unknown word(s)",
        "Scripts instruction words: 2 passed, 0 failed",
        "Audit categories: 1 run, 0 failed",
    ] {
        assert!(text.contains(line), "{line}\n{text}");
    }
    // 0x807D is an unregistered library slot (SPEC-005): the dispatcher
    // raises a VM error on it, so the audit lists it.
    let mut bad = vm::program();
    let at = first_statement(&bad);
    bad[at..at + 2].copy_from_slice(&0x807d_u16.to_be_bytes());
    std::fs::write(temp.path().join("scripts/bad.int"), bad).unwrap();
    // A map whose script index names no `scripts.lst` row.
    let mut map = std::fs::read(temp.path().join("maps/synthetic.map")).unwrap();
    map[0x24..0x28].copy_from_slice(&40_i32.to_be_bytes());
    std::fs::write(temp.path().join("maps/broken.map"), map).unwrap();
    let (success, _, text, errors) = audit(temp.path(), &["--validate-scripts"]);
    assert!(!success);
    assert!(text.contains("Unknown word 0x807d"), "{text}");
    assert!(
        text.contains("Scripts instruction words: 2 passed, 1 failed"),
        "{text}"
    );
    assert!(
        errors.contains("FAIL instruction words: scripts/bad.int: unknown word 0x807d x1"),
        "{errors}"
    );
    assert!(
        errors.contains("FAIL map script index: BROKEN.MAP: script index 39 missing"),
        "{errors}"
    );
}

/// The world fixture's map (number 7) with an exit grid leading back to map
/// 7 at `hex`, elevation 0.
fn map_with_exit_grid(hex: i32) -> Vec<u8> {
    let mut bytes = world::preview_map();
    let counts = 248 + 3 * 40_000 + 20;
    for offset in [counts, counts + 4] {
        world::word(&mut bytes, offset, 6);
    }
    let mut grid = world::object(5, 20500, 6, &[7, hex, 0, 2]);
    // Exit grids are miscellaneous PIDs 0x05000010-0x05000017 (SPEC-003).
    world::word(&mut grid, 0x2c, 0x0500_0010);
    let end = bytes.len() - 8;
    bytes.splice(end..end, grid);
    bytes
}

#[test]
fn validates_exit_grid_destinations_against_tiles_and_blockers() {
    let temp = tempfile::tempdir().unwrap();
    world::write_corpus(temp.path());
    // The wall stands on 20101.
    let write = |hex| {
        let map = map_with_exit_grid(hex);
        std::fs::write(temp.path().join("maps/synthetic.map"), &map).unwrap();
        std::fs::write(temp.path().join("maps/synth.map"), &map).unwrap();
    };
    write(20101);
    let (success, _, text, errors) = audit(temp.path(), &["--validate-exits"]);
    assert!(!success);
    assert!(
        text.contains("Exits exit grids: 0 passed, 2 failed"),
        "{text}\n{errors}"
    );
    assert!(
        errors.contains(
            "grid 20500/0 -> map 7 hex 20101/0: hex 20101/0 is blocked by a wall or scenery"
        ),
        "{errors}"
    );
    // The fixture lists no elevator map, so those destinations fail too.
    assert!(
        errors.contains("FAIL elevator destinations: elevator 0"),
        "{errors}"
    );
    write(20500);
    let (_, _, text, errors) = audit(temp.path(), &["--validate-exits"]);
    assert!(
        text.contains("Exits exit grids: 2 passed, 0 failed"),
        "{text}\n{errors}"
    );
    assert!(text.contains("Exits: 2 exit grids"), "{text}");
    // Off the map.
    write(40_000);
    let (_, _, _, errors) = audit(temp.path(), &["--validate-exits"]);
    assert!(
        errors.contains("hex 40000/0 is outside 0-39999"),
        "{errors}"
    );
}

#[test]
fn transition_sequences_report_each_missing_campaign_map() {
    let temp = tempfile::tempdir().unwrap();
    state::write_corpus(temp.path());
    let map = std::fs::read(temp.path().join("maps/synthetic.map")).unwrap();
    std::fs::write(temp.path().join("maps/v13ent.map"), &map).unwrap();
    let (success, _, text, errors) = audit(temp.path(), &["--validate-transitions"]);
    assert!(!success);
    // The fresh start runs and is checked; every sequence then stops at its
    // first campaign map, which the fixture lacks.
    for line in [
        "Transition new game V13ENT.MAP:",
        "Transitions symbol faults: 1 passed, 0 failed",
        "Transitions reached: 0 passed, 3 failed",
        "Audit categories: 1 run, 1 failed",
    ] {
        assert!(text.contains(line), "{line}\n{text}\n{errors}");
    }
    assert!(
        errors.contains("FAIL reached: Vault 15 ladder 1"),
        "{errors}"
    );
    assert!(errors.contains("FAIL reached: Campaign 1"), "{errors}");
}

#[test]
fn reports_the_spec017_script_stack_model_for_both_tables() {
    use vault_fixtures::dat2::{Member, archive};
    // Listed scripts: a clean one, one using the Fallout 2 word 0x8156,
    // one using the reserved 0x815A, and one the mount lacks.
    let script = |words: &[u16]| {
        let mut body = vec![state::instruction(0x80bc), state::push(13), state::push(0)];
        body.extend(words.iter().map(|word| state::instruction(*word)));
        body.push(state::instruction(0x801a));
        state::program(
            &[],
            &[(b"start".as_slice(), state::returning(body.concat()))],
        )
    };
    let scripts = [
        ("clean.int", script(&[0x8033, 0x8033])),
        ("words2.int", script(&[0x8156])),
        ("reserved.int", script(&[0x815a])),
    ];
    let list = b"clean.int # local_vars=0\nwords2.int # local_vars=0\n\
                 reserved.int # local_vars=0\nabsent.int # local_vars=0\n";
    let temp = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(temp.path().join("scripts")).unwrap();
    std::fs::write(temp.path().join("scripts/scripts.lst"), list).unwrap();
    let mut members = vec![Member::stored("scripts\\scripts.lst", list)];
    for (name, bytes) in &scripts {
        std::fs::write(temp.path().join("scripts").join(name), bytes).unwrap();
        members.push(Member::stored(&format!("scripts\\{name}"), bytes));
    }
    let install = tempfile::tempdir().unwrap();
    std::fs::write(install.path().join("MASTER.DAT"), archive(&members)).unwrap();
    std::fs::write(install.path().join("CRITTER.DAT"), archive(&[])).unwrap();
    let (success, _, text, errors) = audit(
        temp.path(),
        &[
            "--report-script-stack",
            "--fallout2-dir",
            install.path().to_str().unwrap(),
        ],
    );
    // A report, not a category: it never fails the run.
    assert!(success, "{errors}");
    assert!(!text.contains("Audit categories"), "{text}");
    // The fallout1 table's recorded exception names a script this mount
    // lacks: it is stale, and counts as a finding.
    for line in [
        "Script stack (fallout1 table): 3 scripts, 1 listed but absent, 0 undecodable",
        "Script stack findings: 3 (underflow 0, overflow 0, merge conflict 0, unresolved 0, \
         refused word 2, reserved word 0, unexpected frame 0, data-dependent 0, \
         stale exception 1)",
        "Script stack units with leaked results: 0",
        "Script stack stale exception: gatedemo.int underflow at word 0x80de recorded x1, \
         script not in the mount",
        "Script stack finding: refused word at word 0x8156 x1",
        "Script stack finding: refused word at word 0x815a x1",
        "Script stack (fallout2 table): 3 scripts, 1 listed but absent, 0 undecodable",
        "Script stack findings: 1 (underflow 0, overflow 0, merge conflict 0, unresolved 0, \
         refused word 0, reserved word 1, unexpected frame 0, data-dependent 0, \
         stale exception 0)",
        "Script stack finding: reserved word at word 0x815a x1",
        "Script stack word 0x8156 (inven_cmds): 1 occurrences, table pops 3 push 1, consistent: ",
        "Script stack word 0x80e6 (set_exit_grids): 0 occurrences, table pops 5 push 0, \
         consistent: not inferred",
        "Script stack stub 0x80ad: 0 occurrences",
        "Script stack reserved-range occurrences: 1",
    ] {
        assert!(text.contains(line), "{line}\n{text}");
    }
    // The inferred candidates include the table's signature.
    let inferred = text
        .lines()
        .find(|line| line.starts_with("Script stack word 0x8156"))
        .unwrap();
    assert!(inferred.contains("pops 3 push 1"), "{inferred}");
    // The model check is the report's final line.
    assert_eq!(
        text.lines()
            .rfind(|line| line.starts_with("Script stack"))
            .unwrap(),
        "Script stack model check: failed"
    );
}

#[test]
fn the_spec017_model_check_passes_with_a_matched_exception_and_leaked_results() {
    // A statement call of self_obj whose result is left on the stack where
    // a branch rejoins at body offset 0x12, and gatedemo.int's
    // four-operand start_gdialog.
    let leaking = |target: i32| {
        state::returning(
            [
                state::push(target),
                state::push(1),
                state::instruction(0x802f),
                state::instruction(0x80bc),
                state::instruction(0x8000),
            ]
            .concat(),
        )
    };
    // The body is the last thing in the image.
    let probe = state::program(&[], &[(b"start".as_slice(), leaking(0))]);
    let body = (probe.len() - leaking(0).len()) as i32;
    let scripts = [
        (
            "leak.int",
            state::program(&[], &[(b"start".as_slice(), leaking(body + 0x12))]),
        ),
        (
            "gatedemo.int",
            state::program(
                &[],
                &[(
                    b"start".as_slice(),
                    state::returning(
                        [
                            state::push(0),
                            state::push(0),
                            state::push(0),
                            state::push(0),
                            state::instruction(0x80de),
                        ]
                        .concat(),
                    ),
                )],
            ),
        ),
    ];
    let temp = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(temp.path().join("scripts")).unwrap();
    std::fs::write(
        temp.path().join("scripts/scripts.lst"),
        b"leak.int # local_vars=0\nGATEDEMO.INT # local_vars=0\n",
    )
    .unwrap();
    for (name, bytes) in &scripts {
        std::fs::write(temp.path().join("scripts").join(name), bytes).unwrap();
    }
    let (success, _, text, errors) = audit(temp.path(), &["--report-script-stack"]);
    assert!(success, "{errors}");
    for line in [
        "Script stack (fallout1 table): 2 scripts, 0 listed but absent, 0 undecodable",
        "Script stack findings: 0 (underflow 0, overflow 0, merge conflict 0, unresolved 0, \
         refused word 0, reserved word 0, unexpected frame 0, data-dependent 0, \
         stale exception 0)",
        "Script stack units with leaked results: 1",
        "Script stack recorded exception: gatedemo.int underflow at word 0x80de x1",
    ] {
        assert!(text.contains(line), "{line}\n{text}");
    }
    assert!(!text.contains("Script stack finding:"), "{text}");
    assert_eq!(
        text.lines()
            .rfind(|line| line.starts_with("Script stack"))
            .unwrap(),
        "Script stack model check: passed"
    );
}

#[test]
fn reports_which_spec019_interface_art_names_an_install_lists() {
    use vault_fixtures::dat2::{Member, archive};
    let temp = tempfile::tempdir().unwrap();
    world::write_corpus(temp.path());
    let list = b"IFACE.FRM\nwmdial.frm\ntakeallu.frm\n";
    let install = tempfile::tempdir().unwrap();
    std::fs::write(
        install.path().join("MASTER.DAT"),
        archive(&[Member::stored("art\\intrface\\intrface.lst", list)]),
    )
    .unwrap();
    std::fs::write(install.path().join("CRITTER.DAT"), archive(&[])).unwrap();
    let (success, _, text, errors) = audit(
        temp.path(),
        &[
            "--report-fallout2-art",
            "--fallout2-dir",
            install.path().to_str().unwrap(),
        ],
    );
    assert!(success, "{errors}");
    for line in [
        "Fallout 2 interface art iface.frm: iface.frm",
        "Fallout 2 interface art wmdial.frm: wmdial.frm",
        "Fallout 2 interface art invmaup.frm / takeallu.frm: takeallu.frm",
        "Fallout 2 interface art control.frm: missing",
        "Fallout 2 interface art: 3 of 12 roles listed",
    ] {
        assert!(text.contains(line), "{line}\n{text}");
    }
}

#[test]
fn reports_a_fallout2_install_with_a_smoke_run() {
    // The synthetic corpus with SPEC-018's tables, its start map's script
    // (`scripts.lst` row 3) faulting in `map_enter_p_proc` on a division by
    // zero, packed as a SPEC-015 install with a loose `data` folder.
    let corpus = tempfile::tempdir().unwrap();
    state::write_corpus(corpus.path());
    vault_fixtures::fallout2::write_tables(corpus.path());
    let enter = [state::push(1), state::push(0), state::instruction(0x803c)].concat();
    std::fs::write(
        corpus.path().join("scripts/f2enter.int"),
        state::program(&[], &[(b"map_enter_p_proc", state::returning(enter))]),
    )
    .unwrap();
    let list = corpus.path().join("scripts/scripts.lst");
    let mut rows = std::fs::read(&list).unwrap();
    rows.extend(b"f2enter.int # local_vars=0\n");
    std::fs::write(&list, rows).unwrap();
    let map = std::fs::read(corpus.path().join("maps/synthetic.map")).unwrap();
    std::fs::write(
        corpus.path().join("maps/artemple.map"),
        vault_fixtures::fallout2::named_map(map, "artemple.map", 3, 20100, 2),
    )
    .unwrap();
    // SPEC-018 §4.7.1 (Revision 3): a Fallout 2 `ai.txt` packet, with no
    // `last_msg` or `hurt_too_much` and with the twelve range ends.
    let mut packet = String::from(
        "[Synthetic]\npacket_num=0\nmax_dist=8\nmin_to_hit=40\nmin_hp=12\naggression=50\n\
         secondary_freq=4\ncalled_freq=3\nfont=101\ncolor=145\noutline_color=-1\nchance=35\n",
    );
    for (index, key) in [
        "run",
        "move",
        "attack",
        "miss",
        "hit_head",
        "hit_left_arm",
        "hit_right_arm",
        "hit_torso",
        "hit_right_leg",
        "hit_left_leg",
        "hit_eyes",
        "hit_groin",
    ]
    .iter()
    .enumerate()
    {
        let start = 1000 + 10 * index;
        packet.push_str(&format!("{key}_start={start}\n{key}_end={}\n", start + 5));
    }
    std::fs::write(corpus.path().join("data/ai.txt"), packet).unwrap();
    std::fs::create_dir_all(corpus.path().join("text/english/game")).unwrap();
    std::fs::write(
        corpus.path().join("text/english/game/combatai.msg"),
        b"{1000}{}{Run}\n",
    )
    .unwrap();
    let install = tempfile::tempdir().unwrap();
    vault_fixtures::dat2::fallout2_install(corpus.path(), install.path(), 2);
    std::fs::create_dir_all(install.path().join("data")).unwrap();
    std::fs::write(install.path().join("data/note.txt"), b"loose\n").unwrap();
    let (success, _, text, errors) = audit(
        corpus.path(),
        &[
            "--report-fallout2-install",
            "--fallout2-dir",
            install.path().to_str().unwrap(),
        ],
    );
    // A report, not a category: it never fails the run.
    assert!(success, "{errors}");
    assert!(!text.contains("Audit categories"), "{text}");
    for line in [
        "Fallout 2 install: MASTER.DAT, CRITTER.DAT\n",
        "Fallout 2 data root: data (1 files)",
        "Fallout 2 required members: 7 of 7 present",
        "Fallout 2 tables: loaded: 3 maps, 1 areas, 1 encounter tables, 2 encounter groups, \
         1 party records",
        "Fallout 2 table diagnostic: 1 x data/worldmap.txt: enc_N dropped (lines 21)",
        "Fallout 2 parsing: ",
        "Fallout 2 critter prototypes read at 412 bytes (SPEC-016 Decision 9): 0\n",
        "Fallout 2 writers: .map ",
        "Fallout 2 smoke run: ARTEMPLE.SAV (map 2), 1200 ticks of 50 ms",
        "Fallout 2 word set: true",
        "Fallout 2 smoke script: 0x00000001 scripts/f2enter.int: 0 locals declared, a slice of 0",
        // SPEC-018 §6.5 (Revision 3): the player's record has entry 0's slice.
        "Fallout 2 smoke script: 0x04004650 scripts/synthetic.int: 4 locals declared, a slice of 4",
        "Fallout 2 smoke VM errors: 1 (1 distinct)",
        "Fallout 2 smoke VM error: 1 x scripts/f2enter.int map_enter_p_proc word 0x803c: \
         division by zero",
        "Fallout 2 smoke VM error kind: 1 x division by zero",
        "Fallout 2 install check: passed",
    ] {
        assert!(text.contains(line), "{line}\n{text}");
    }
    // Every member of a parsed extension parses and round-trips.
    let parsing = text
        .lines()
        .find(|line| line.starts_with("Fallout 2 parsing: "))
        .unwrap();
    assert!(parsing.ends_with(" passed, 0 failed"), "{parsing}");
    assert!(text.contains(".map\t5\t0"), "{text}");
    assert!(text.contains(", 0 failed; .pro "), "{text}");
    // The Fallout 2 packet table loads, so combat AI is available.
    assert!(!text.contains("combat AI unavailable"), "{text}");
}
