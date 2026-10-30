//! SPEC-021 §4 tests T13 and T14: the checkpoint format and the comparison
//! rule, on the synthetic vectors of `tests/fixtures/spec021/rev3/`.
use super::*;
use crate::{CharacterSheet, WorldState};

const EXAMPLE: &str = include_str!("../../../tests/fixtures/spec021/rev3/checkpoints-example.tsv");
const CASES: &str = include_str!("../../../tests/fixtures/spec021/rev3/compare-cases.tsv");

/// §5.9 the rows of the example, written out by hand.
fn example_rows() -> Vec<Checkpoint> {
    [
        (0, 0, 0x0258_CFED_4EA8_A13E, None),
        (1, 2, 0x0258_CFED_4EA8_A13E, None),
        (2, 3, 0x04A0_AD69_522C_3565, None),
        (3, 3, 0x121B_BD7C_8E51_E03E, Some(0xCDC1_8383_8522_E1A7)),
        (4, 4, 0x1FF6_53DE_E3A2_23B3, Some(0xC256_48C5_07DC_28BE)),
        (5, 4, 0x02A2_EFB9_5101_DA7D, Some(0xA847_5FA8_866A_8966)),
        (6, 6, 0x04A0_AD69_522C_3565, None),
    ]
    .map(|(tick, input_index, state_hash, combat_hash)| Checkpoint {
        tick,
        input_index,
        state_hash,
        combat_hash,
    })
    .to_vec()
}

#[test]
fn t13_writing_the_rows_gives_the_example_and_reading_it_gives_them_back() {
    let rows = example_rows();
    assert_eq!(write_checkpoints(&rows), EXAMPLE);
    assert_eq!(read_checkpoints(EXAMPLE).unwrap(), rows);
    // The first two lines, exactly.
    let mut lines = EXAMPLE.lines();
    assert_eq!(lines.next(), Some("vault-checkpoints\t1\tVSH1\tVCH1"));
    assert_eq!(
        lines.next(),
        Some("tick\tinput_index\tstate_hash\tcombat_hash")
    );
    // The file rules: ASCII, LF line ends, a final LF, no blank line.
    assert!(EXAMPLE.is_ascii() && EXAMPLE.ends_with('\n') && !EXAMPLE.contains("\n\n"));
    assert!(!EXAMPLE.contains('\r'));
    assert!(read_checkpoints(EXAMPLE.trim_end()).is_err());
}

#[test]
fn t13_the_version_line_takes_its_magics_from_the_encoder() {
    let line = version_line();
    let fields: Vec<&str> = line.split('\t').collect();
    assert_eq!(fields[2].as_bytes(), crate::state_hash::WORLD_MAGIC);
    assert_eq!(fields[3].as_bytes(), crate::state_hash::COMBAT_MAGIC);
    assert_eq!(fields[..2], [CHECKPOINT_FORMAT, "1"]);
}

#[test]
fn t13_a_rows_combat_hash_is_written_exactly_while_a_combat_is_active() {
    let mut world = WorldState::new(CharacterSheet::default(), 1).unwrap();
    let before = world.state_hash();
    let calm = Checkpoint::of(&world, 0, 0);
    assert_eq!(calm.combat_hash, None);
    assert_eq!(calm.state_hash, before);
    assert!(calm.to_string().ends_with("\t-"));
    world.combat_state.flags |= crate::critical::state::ACTIVE;
    let fighting = Checkpoint::of(&world, 1, 0);
    assert_eq!(fighting.combat_hash, Some(world.combat_hash()));
    assert_ne!(fighting.combat_hash, Some(0x03A3_2523_40D6_7E8D));
    // The no-combat constant is never written.
    world.combat_state.flags &= !crate::critical::state::ACTIVE;
    assert_eq!(world.combat_hash(), 0x03A3_2523_40D6_7E8D);
    let over = Checkpoint::of(&world, 2, 0);
    assert_eq!(over.combat_hash, None);
    // Taking a row changes nothing.
    assert_eq!(world.state_hash(), before);
}

#[test]
fn t13_a_row_is_at_most_56_bytes_for_ten_digit_numbers() {
    let row = Checkpoint {
        tick: 9_999_999_999,
        input_index: 9_999_999_999,
        state_hash: u64::MAX,
        combat_hash: Some(u64::MAX),
    };
    assert_eq!(row.to_string().len() + 1, 56);
}

/// A copy of the example with one case's change applied. `line` is the
/// one-based line of the file, `\t` in `value` stands for a tab.
fn apply_case(case: &std::collections::BTreeMap<&str, &str>) -> String {
    let value = case["value"].replace("\\t", "\t");
    let mut lines: Vec<String> = EXAMPLE.lines().map(str::to_owned).collect();
    let column = COLUMNS.iter().position(|name| *name == case["column"]);
    let set = |line: &mut String, column: usize, value: &str| {
        let mut fields: Vec<&str> = line.split('\t').collect();
        fields[column] = value;
        *line = fields.join("\t");
    };
    let line = case["line"].parse::<usize>().ok().map(|line| line - 1);
    match case["change"] {
        "none" => {}
        "set" => set(&mut lines[line.unwrap()], column.unwrap(), &value),
        "set2" => {
            set(&mut lines[line.unwrap()], column.unwrap(), &value);
            set(&mut lines[6], 2, &value);
        }
        "setboth" => {
            let at = line.unwrap();
            let state = lines[at].split('\t').nth(2).unwrap().to_owned();
            set(&mut lines[at], column.unwrap(), &value);
            set(&mut lines[at], 3, &state);
        }
        "truncate" => lines.truncate(line.unwrap() + 1),
        "append" => lines.push(value),
        "line1" => lines[0] = value,
        "line2" => lines[1] = value,
        "drop" => {
            let at = line.unwrap();
            let mut fields: Vec<&str> = lines[at].split('\t').collect();
            fields.remove(column.unwrap());
            lines[at] = fields.join("\t");
        }
        other => panic!("change {other}"),
    }
    lines.join("\n") + "\n"
}

#[test]
fn t14_every_comparison_case_gives_its_verdict_and_its_exact_report() {
    let expected = ExpectedStream::parse(EXAMPLE).unwrap();
    let mut rows = CASES.lines();
    let header: Vec<&str> = rows.next().unwrap().split('\t').collect();
    let mut seen = 0;
    for row in rows {
        let case: std::collections::BTreeMap<&str, &str> =
            header.iter().copied().zip(row.split('\t')).collect();
        let actual = apply_case(&case);
        let result = compare_checkpoints(&expected, &actual);
        let verdict = match &result {
            Ok(_) => "match",
            Err(CheckpointFailure::Format { .. }) => "format",
            Err(CheckpointFailure::Mismatch { .. }) => "mismatch",
            Err(CheckpointFailure::EndedEarly { .. }) => "short",
            Err(CheckpointFailure::TooLong { .. }) => "long",
            Err(CheckpointFailure::Malformed { .. }) => "malformed",
        };
        assert_eq!(verdict, case["verdict"], "{}", case["case"]);
        assert_eq!(
            comparison_report(&result),
            case["report"],
            "{}",
            case["case"]
        );
        seen += 1;
    }
    assert_eq!(seen, 15);
}

#[test]
fn t14_the_comparison_stops_at_the_first_failure() {
    let expected = ExpectedStream::parse(EXAMPLE).unwrap();
    let mut comparison = CheckpointComparison::new(expected);
    let mut lines = EXAMPLE.lines();
    comparison.line(lines.next().unwrap()).unwrap();
    comparison.line(lines.next().unwrap()).unwrap();
    comparison.line(lines.next().unwrap()).unwrap();
    comparison.line(lines.next().unwrap()).unwrap();
    // Tick 2 differs: the report names it, whatever follows.
    let tick_2 = lines
        .next()
        .unwrap()
        .replace("04A0AD69522C3565", "45A5547A2010CCB5");
    let failure = comparison.line(&tick_2).unwrap_err();
    assert_eq!(
        failure.to_string(),
        "checkpoint mismatch at tick 2: state_hash expected 04A0AD69522C3565, actual \
         45A5547A2010CCB5"
    );
}

#[test]
fn t14_a_format_mismatch_reads_no_row() {
    let expected = ExpectedStream::parse(EXAMPLE).unwrap();
    // Every row is broken too, and only the format is reported.
    let actual = EXAMPLE.replace("VSH1", "VSH2").replace("0258", "xxxx");
    assert!(matches!(
        compare_checkpoints(&expected, &actual),
        Err(CheckpointFailure::Format { line: 1, .. })
    ));
}

#[test]
fn t14_well_formed_means_canonical_numbers_and_hashes() {
    let expected = ExpectedStream::parse(EXAMPLE).unwrap();
    let head = stream_head();
    for row in [
        "00\t0\t0258CFED4EA8A13E\t-",
        "+0\t0\t0258CFED4EA8A13E\t-",
        "0\t01\t0258CFED4EA8A13E\t-",
        "0\t0\t0258CFED4EA8A13\t-",
        "0\t0\t0258CFED4EA8A13E\tnone",
        "0\t0\t0258CFED4EA8A13E\t-\textra",
        "1\t0\t0258CFED4EA8A13E\t-",
        "0\t1\t0258CFED4EA8A13E\t-",
    ] {
        assert_eq!(
            compare_checkpoints(&expected, &format!("{head}{row}\n")),
            Err(CheckpointFailure::Malformed { line: 3 }),
            "{row}"
        );
    }
    // A stream without its final LF.
    assert_eq!(
        compare_checkpoints(&expected, EXAMPLE.trim_end()),
        Err(CheckpointFailure::Malformed { line: 9 })
    );
    // An empty stream is a format mismatch at line 1, and a stream without
    // a row ended early.
    assert!(matches!(
        compare_checkpoints(&expected, ""),
        Err(CheckpointFailure::Format { line: 1, .. })
    ));
    assert_eq!(
        compare_checkpoints(&expected, &head),
        Err(CheckpointFailure::EndedEarly {
            expected: 0,
            after: None
        })
    );
}

#[test]
fn t14_an_expected_stream_that_is_no_stream_is_refused() {
    assert!(ExpectedStream::parse("").is_err());
    assert!(ExpectedStream::parse("vault-checkpoints\t1\tVSH1\tVCH1\n").is_err());
    assert!(ExpectedStream::parse(&format!("{}1\t0\tzz\t-\n", stream_head())).is_err());
    // Another format is still a stream: the comparison reports it.
    let other = EXAMPLE.replace("VSH1", "VSH2");
    assert!(ExpectedStream::parse(&other).is_ok());
}

#[test]
fn a_run_writes_row_0_then_one_row_per_step_and_stops_on_the_first_failure() {
    let world = WorldState::new(CharacterSheet::default(), 1).unwrap();
    let mut run = CheckpointStream::new(None);
    let mut text = run.head().unwrap();
    for index in [0, 2, 3] {
        text.push_str(&run.row(&world, index).unwrap());
    }
    assert_eq!(run.finish(), Ok(None));
    let rows = read_checkpoints(&text).unwrap();
    assert_eq!(rows.len(), 3);
    assert_eq!(rows[2].tick, 2);
    assert_eq!(rows[2].input_index, 3);

    // The same run against that stream matches; one changed row stops it.
    let expected = ExpectedStream::parse(&text).unwrap();
    let mut run = CheckpointStream::new(Some(expected.clone()));
    run.head().unwrap();
    for index in [0, 2, 3] {
        run.row(&world, index).unwrap();
    }
    assert_eq!(run.finish(), Ok(Some(3)));
    let mut run = CheckpointStream::new(Some(expected.clone()));
    run.head().unwrap();
    run.row(&world, 0).unwrap();
    let (line, failure) = run.row(&world, 9).unwrap_err();
    assert!(line.starts_with("1\t9\t"));
    assert!(matches!(
        failure,
        CheckpointFailure::Mismatch {
            tick: 1,
            column: "input_index",
            ..
        }
    ));
    // A run that stops short ended early.
    let mut run = CheckpointStream::new(Some(expected));
    run.head().unwrap();
    run.row(&world, 0).unwrap();
    assert_eq!(
        run.finish(),
        Err(CheckpointFailure::EndedEarly {
            expected: 1,
            after: Some(0)
        })
    );
}
