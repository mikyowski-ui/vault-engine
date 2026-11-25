//! SPEC-024b sections 3, 4 and 7, in the VM: the table against the oracle,
//! the math bits and the string vectors of §7.2.
use crate::extensions::{ENHANCED_RANGE, opcode_in};
use crate::sfall::*;
use crate::sfall_arrays::{
    ArrayStore, Header, MAX_LIST_LENGTH, MAX_TOTAL_ELEMENTS, SIDECAR_TAG, SidecarError, Storage,
    parse_sidecar, write_stream,
};
use crate::stack_check::{
    Finding, Image, MAX_STEPS, MIN_STEPS, STEPS_PER_INSTRUCTION, Table, analyse, analyse_unit,
    check, summary,
};
use crate::{IntProgram, WordSet, sfall};

const ORACLE: &str = include_str!("../../../oracle/SPEC-024/sfall-opcodes.tsv");

fn int(value: i32) -> SfallValue {
    SfallValue::Int(value)
}
fn float(value: f32) -> SfallValue {
    SfallValue::float(value)
}

/// The oracle's rows that `keep` keeps, over its tab-split cells.
fn oracle_rows(keep: impl Fn(&[&str]) -> bool) -> usize {
    ORACLE
        .lines()
        .skip(1)
        .filter(|line| keep(&line.split('\t').collect::<Vec<_>>()))
        .count()
}

#[test]
fn every_oracle_row_is_in_the_table_with_its_contract() {
    let mut seen = 0;
    for line in ORACLE.lines().skip(1) {
        let cells: Vec<&str> = line.split('\t').collect();
        let pops = cells[4].parse::<u8>().ok();
        let push = cells[5] == "1";
        let row = match cells[0] {
            "word" => {
                let word = u16::from_str_radix(&cells[1][2..], 16).unwrap();
                *word_row(word).unwrap()
            }
            "sfall_func" => *SFALL_FUNCTIONS
                .iter()
                .find(|row| {
                    row.name == cells[2]
                        && row.pops == pops
                        && row.dispatch == u16::from_str_radix(&cells[3][2..], 16).unwrap()
                })
                .unwrap(),
            "metarule3" => *SFALL_METARULE3
                .iter()
                .find(|row| row.id.to_string() == cells[1])
                .unwrap(),
            _ => *SFALL_EXPLOSIONS
                .iter()
                .find(|row| row.id.to_string() == cells[1])
                .unwrap(),
        };
        assert_eq!((row.pops, row.push), (pops, push), "{line}");
        assert_eq!(format!("{:?}", row.tier), cells[9], "{line}");
        seen += 1;
    }
    assert_eq!(seen, 449);
    // Revision 3's word tiers, counted in the oracle and in the table.
    let oracle = |tier: &str| oracle_rows(|cells| cells[0] == "word" && cells[9] == tier);
    let count = |tier| SFALL_WORDS.iter().filter(|row| row.tier == tier).count();
    for (tier, name, expected) in [
        (Tier::P1, "P1", 133),
        (Tier::P2, "P2", 143),
        (Tier::X, "X", 18),
        (Tier::R, "R", 6),
    ] {
        assert_eq!(oracle(name), expected, "{name} in the oracle");
        assert_eq!(count(tier), expected, "{name} in the table");
    }
    // Revision 4: sfall_func has one row per argument count, so 33 behaviors
    // are 35 rows.
    assert_eq!(oracle_rows(|cells| cells[9] == "P1"), 193);
    assert_eq!(
        oracle_rows(|cells| cells[0] == "sfall_func" && cells[9] == "P1"),
        SFALL_FUNCTIONS
            .iter()
            .filter(|row| row.tier == Tier::P1)
            .count()
    );
}

#[test]
fn the_sfall_set_has_642_entries_and_keeps_the_rest_of_fallout2() {
    let set = WordSet::FALLOUT2_SFALL;
    assert_eq!(set.words.clone().count(), 642);
    assert!(set.fallout2() && set.sfall());
    assert!(WordSet::FALLOUT2.fallout2() && !WordSet::FALLOUT2.sfall());
    assert!(!WordSet::FALLOUT1.fallout2());
    // Below the overlap, the Fallout 2 rows stand.
    for word in [0x80b2, 0x80e1, 0x8109, 0x8155] {
        assert_eq!(
            opcode_in(&set, word).unwrap().name,
            opcode_in(&WordSet::FALLOUT2, word).unwrap().name
        );
    }
    assert_eq!(opcode_in(&set, 0x8156).unwrap().name, "read_byte");
    assert_eq!(
        opcode_in(&WordSet::FALLOUT2, 0x8156).unwrap().name,
        "inven_cmds"
    );
    assert_eq!(opcode_in(&set, 0x8200).unwrap().name, "fs_size");
    assert_eq!(opcode_in(&set, 0x8281).unwrap().name, "sfall_func8");
    assert!(opcode_in(&set, 0x8252).is_none(), "a reserved word");
    assert!(opcode_in(&set, 0x8282).is_none());
}

#[test]
fn the_stack_model_reads_the_oracle_under_sfall_with_or_without_enhanced() {
    for enhanced in [false, true] {
        let table = Table::with_enhanced(WordSet::FALLOUT2_SFALL, enhanced);
        for row in SFALL_WORDS.iter() {
            assert_eq!(
                table.signature(row.id),
                row.pops.map(|pops| (pops, u8::from(row.push))),
                "{}",
                row.name
            );
        }
    }
    // The overlap's enhanced rows stay under the plain Fallout 2 table.
    let table = Table::with_enhanced(WordSet::FALLOUT2, true);
    assert_eq!(table.row(0x8210).unwrap().name, "op_cam_pan_to");
    assert!(ENHANCED_RANGE.contains(&0x8210));
}

#[test]
fn p1_queries_are_limited_to_p1() {
    let names = p1_functions();
    assert_eq!(names.len(), 42);
    assert!(names.windows(2).all(|pair| pair[0] < pair[1]));
    // The list is the oracle's distinct P1 names, each one read from there.
    let mut oracle: Vec<&str> = ORACLE
        .lines()
        .skip(1)
        .map(|line| line.split('\t').collect::<Vec<_>>())
        .filter(|cells| cells[0] == "sfall_func" && cells[9] == "P1")
        .map(|cells| cells[2])
        .collect();
    oracle.sort_unstable();
    oracle.dedup();
    assert_eq!(names, oracle);
    assert!(names.contains(&"string_find"));
    assert!(names.contains(&"message_box"), "promoted by section 4.11");
    assert!(!names.contains(&"add_iface_tag"), "still tier P2");
    assert_eq!(function_tier(b"add_iface_tag"), Some(Tier::P2));
    assert_eq!(function_tier(b"string_find"), Some(Tier::P1));
    assert_eq!(function_tier(b"message_box"), Some(Tier::P1));
    assert_eq!(function_tier(b"no_such_fn"), None);
    assert!(p1_word(0x8261), "promoted by section 4.11");
    assert!(p1_word(0x8210));
    assert!(!p1_word(0x8200), "fs_size is P2");
    assert!(!p1_word(0x8252), "a reserved word");
    assert!(!p1_word(0x8156), "an X word");
    assert!(!p1_word(-1));
}

#[test]
fn coercion_follows_the_table_of_section_4_1() {
    let text = SfallValue::text("x");
    assert_eq!(float(-1.9).coerce(Class::I), Some(int(-1)));
    assert_eq!(float(3e10).coerce(Class::I), Some(int(i32::MAX)));
    assert_eq!(float(-3e10).coerce(Class::I), Some(int(i32::MIN)));
    assert_eq!(text.coerce(Class::I), None);
    assert_eq!(int(3).coerce(Class::F), Some(float(3.0)));
    assert_eq!(text.coerce(Class::F), None);
    assert_eq!(float(1.5).coerce(Class::N), Some(float(1.5)));
    assert_eq!(text.coerce(Class::N), None);
    assert_eq!(int(1).coerce(Class::S), None);
    assert_eq!(float(1.0).coerce(Class::S), None);
    assert_eq!(text.coerce(Class::S), Some(text.clone()));
    assert_eq!(int(1).coerce(Class::K), Some(int(1)));
    assert_eq!(float(1.0).coerce(Class::K), None);
    assert_eq!(text.coerce(Class::K), Some(text.clone()));
    assert_eq!(text.coerce(Class::A), Some(text.clone()));
    assert_eq!(float(1.0).coerce(Class::O), None);
    assert_eq!(text.coerce(Class::P), None);
    assert_eq!(int(5).coerce(Class::P), Some(int(5)));
}

#[test]
fn defaults_follow_the_return_class() {
    let row = |word| word_row(word).unwrap();
    assert_eq!(
        failed_default(row(0x824E)),
        Some(SfallValue::Text(Vec::new()))
    );
    assert_eq!(failed_default(row(0x8238)), Some(float(0.0)));
    assert_eq!(failed_default(row(0x8237)), Some(int(0)));
    assert_eq!(failed_default(row(0x815A)), None);
    assert_eq!(
        stub_default(row(0x8159)),
        Some(SfallValue::Text(Vec::new()))
    );
    assert_eq!(stub_default(row(0x8200)), Some(int(0)));
    assert_eq!(stub_default(row(0x81CF)), None);
}

#[test]
fn the_published_string_vectors_of_section_7_2() {
    assert_eq!(substr(b"test", -2, 2), b"st");
    assert_eq!(substr(b"test", 0, -2), b"te");
    assert_eq!(substr(b"test", 1, 0), b"est");
    assert_eq!(substr(b"test", 9, 2), b"");
    assert_eq!(substr(b"test", -9, 2), b"te");
    assert_eq!(substr(b"test", 3, -3), b"");
    assert_eq!(split(b"a,b", b","), [b"a".to_vec(), b"b".to_vec()]);
    assert_eq!(split(b"ab", b""), [b"a".to_vec(), b"b".to_vec()]);
    assert_eq!(split(b"", b","), [Vec::<u8>::new()]);
    assert_eq!(split(b"a,,", b","), [b"a".to_vec(), vec![], vec![]]);
}

#[test]
fn atoi_and_atof_read_up_to_the_first_byte_that_does_not_fit() {
    assert_eq!(atoi(b"  -12x"), -12);
    assert_eq!(atoi(b"+7"), 7);
    assert_eq!(atoi(b"abc"), 0);
    assert_eq!(atoi(b"-"), 0);
    assert_eq!(atoi(b"99999999999"), i32::MAX);
    assert_eq!(atoi(b"-99999999999"), i32::MIN);
    assert_eq!(atof(b" 1.5e2x"), 150.0);
    assert_eq!(atof(b"-.5"), -0.5);
    assert_eq!(atof(b"5."), 5.0);
    assert_eq!(atof(b"2e"), 2.0);
    assert_eq!(atof(b"2e+"), 2.0);
    assert_eq!(atof(b"."), 0.0);
    assert_eq!(atof(b"x"), 0.0);
}

#[test]
fn string_find_case_and_compare() {
    assert_eq!(find(b"abc", b"c", 0), 2);
    assert_eq!(find(b"abcabc", b"b", 2), 4);
    assert_eq!(find(b"abcabc", b"a", -3), 3);
    assert_eq!(find(b"abc", b"z", 0), -1);
    assert_eq!(find(b"abc", b"a", 9), -1);
    assert_eq!(to_case(b"aB\xC9", 1), b"AB\xC9");
    assert_eq!(to_case(b"aB\xE9", 0), b"ab\xE9");
    assert!(compare(b"ABC", b"abc"));
    assert!(!compare(b"\xC9", b"\xE9"), "bytes 128-255 compare exactly");
}

fn formatted(text: &str, values: &[SfallValue]) -> String {
    String::from_utf8(format(text.as_bytes(), values).unwrap()).unwrap()
}

#[test]
fn the_format_conversions_are_cs() {
    assert_eq!(formatted("%d", &[int(-7)]), "-7");
    assert_eq!(
        formatted("%i|%5d|%-5d|%05d", &[int(1), int(2), int(3), int(4)]),
        "1|    2|3    |00004"
    );
    assert_eq!(formatted("%+d % d", &[int(1), int(1)]), "+1  1");
    assert_eq!(formatted("%.3d", &[int(5)]), "005");
    assert_eq!(formatted("%u", &[int(-1)]), "4294967295");
    assert_eq!(
        formatted(
            "%x %X %#x %o %#o",
            &[int(255), int(255), int(255), int(8), int(8)]
        ),
        "ff FF 0xff 10 010"
    );
    assert_eq!(formatted("%c", &[int(65)]), "A");
    assert_eq!(
        formatted(
            "%s|%5s|%-5s|%.1s",
            &[
                SfallValue::text("ab"),
                SfallValue::text("ab"),
                SfallValue::text("ab"),
                SfallValue::text("ab")
            ]
        ),
        "ab|   ab|ab   |a"
    );
    assert_eq!(formatted("%f", &[float(1.5)]), "1.500000");
    assert_eq!(formatted("%.2f", &[float(1.5)]), "1.50");
    assert_eq!(formatted("%e", &[float(1500.0)]), "1.500000e+03");
    assert_eq!(formatted("%E", &[float(0.015)]), "1.500000E-02");
    assert_eq!(
        formatted("%g %g %g", &[float(100000.0), float(1e6), float(0.0001)]),
        "100000 1e+06 0.0001"
    );
    assert_eq!(formatted("%G", &[float(1e-5)]), "1E-05");
    assert_eq!(formatted("%g", &[float(0.0)]), "0");
    assert_eq!(formatted("100%%", &[]), "100%");
}

#[test]
fn format_coercions_are_the_safe_rule_of_section_4_6() {
    assert_eq!(formatted("%d", &[float(2.9)]), "2");
    assert_eq!(formatted("%.1f", &[int(2)]), "2.0");
    assert_eq!(formatted("%s", &[int(12)]), "12");
    assert_eq!(formatted("%s", &[float(1.5)]), "1.50000");
    assert_eq!(format(b"%d", &[SfallValue::text("x")]), None);
    assert_eq!(format(b"%f", &[SfallValue::text("x")]), None);
    assert_eq!(format(b"%c", &[SfallValue::text("x")]), None);
    // A missing value leaves the conversion text empty.
    assert_eq!(formatted("a%db%sc", &[]), "abc");
    assert_eq!(formatted("%d-%d", &[int(1)]), "1-");
    assert_eq!(formatted("%y", &[int(1)]), "%y");
    assert_eq!(formatted("%f", &[float(f32::INFINITY)]), "inf");
    assert_eq!(formatted("%F", &[float(f32::NAN)]), "NAN");
}

/// Within one binary32 step of the true value, which binary64 gives here.
fn close(ours: f32, truth: f64) {
    if truth.is_nan() {
        assert!(ours.is_nan());
        return;
    }
    let step = f64::from(f32::EPSILON) * truth.abs().max(1e-30);
    assert!(
        (f64::from(ours) - truth).abs() <= step,
        "{ours} against {truth}"
    );
}

#[test]
fn the_software_math_agrees_with_the_true_values_to_a_binary32_step() {
    let mut x = -40.0f64;
    while x < 40.0 {
        let value = x as f32;
        let wide = f64::from(value);
        close(sine(value), wide.sin());
        close(cosine(value), wide.cos());
        if wide.cos().abs() > 1e-3 {
            close(tangent(value), wide.tan());
        }
        if wide > 0.0 {
            close(logarithm(value), wide.ln());
        }
        close(exponent(value), wide.exp());
        close(arctan(value, 1.0), wide.atan());
        close(arctan(value, -2.5), wide.atan2(-2.5));
        x += 0.137;
    }
    for value in [1e-30f32, 1e-5, 0.3, 1.0, 2.0, 1e10, 1e30, f32::MIN_POSITIVE] {
        close(logarithm(value), f64::from(value).ln());
    }
    close(logarithm(1e-45), f64::from(1e-45f32).ln());
    for (base, power_of) in [(2.0f32, 0.5f32), (10.0, 2.5), (0.5, 3.25), (7.0, -1.5)] {
        let SfallValue::Float(bits) = power(&float(base), &float(power_of)) else {
            panic!("a float");
        };
        close(
            f32::from_bits(bits),
            f64::from(base).powf(f64::from(power_of)),
        );
    }
}

#[test]
fn the_math_edges_are_fixed() {
    assert_eq!(sfall::sine(0.0).to_bits(), 0);
    assert_eq!(cosine(0.0), 1.0);
    assert_eq!(logarithm(0.0), f32::NEG_INFINITY);
    assert_eq!(logarithm(-1.0).to_bits(), f32::NAN.to_bits());
    assert_eq!(logarithm(f32::INFINITY), f32::INFINITY);
    assert_eq!(exponent(1000.0), f32::INFINITY);
    assert_eq!(exponent(-1000.0), 0.0);
    assert_eq!(square_root(-1.0).to_bits(), f32::NAN.to_bits());
    assert_eq!(sine(f32::INFINITY).to_bits(), f32::NAN.to_bits());
    close(arctan(1.0, 0.0), std::f64::consts::FRAC_PI_2);
    close(arctan(-1.0, 0.0), -std::f64::consts::FRAC_PI_2);
    close(arctan(0.0, -1.0), std::f64::consts::PI);
    assert_eq!(arctan(0.0, 1.0), 0.0);
    close(
        arctan(f32::INFINITY, f32::INFINITY),
        std::f64::consts::FRAC_PI_4,
    );
    // A huge argument stays finite and the same everywhere.
    assert!(sine(1e30).abs() <= 1.0);
    assert_eq!(ceil(1.1), 2);
    assert_eq!(ceil(-1.1), -1);
    assert_eq!(round(2.5), 3);
    assert_eq!(round(-2.5), -3);
    assert_eq!(floor(-1.5), -2);
    assert_eq!(ceil(3e10), i32::MAX);
    assert_eq!(round(f32::NAN), 0);
}

#[test]
fn the_math_bits_are_pinned() {
    // The golden bits of the software functions: any change here is a replay
    // break on every platform at once, so it has to be deliberate.
    for (value, bits) in [
        (sine(1.0), 0x3f57_6aa4u32),
        (cosine(1.0), 0x3f0a_5140),
        (tangent(1.0), 0x3fc7_5923),
        (arctan(1.0, 1.0), 0x3f49_0fdb),
        (logarithm(10.0), 0x4013_5d8e),
        (exponent(1.0), 0x402d_f854),
        (sine(100.0), 0xbf01_a12e),
        (cosine(-7.5), 0x3eb1_7a2f),
    ] {
        assert_eq!(value.to_bits(), bits, "{value}");
    }
}

#[test]
fn power_and_div_follow_section_4_7() {
    assert_eq!(power(&int(2), &int(3)), int(8));
    assert_eq!(power(&float(-8.0), &int(2)), float(64.0));
    assert!(
        matches!(power(&float(-8.0), &float(0.5)), SfallValue::Float(bits) if bits == f32::NAN.to_bits())
    );
    assert_eq!(power(&int(2), &int(31)), int(i32::MIN), "wraps at 32 bits");
    assert_eq!(power(&int(2), &int(-1)), int(0));
    assert_eq!(power(&int(1), &int(-9)), int(1));
    assert_eq!(power(&int(-1), &int(-9)), int(-1));
    assert_eq!(power(&int(-1), &int(-8)), int(1));
    assert_eq!(power(&int(0), &int(0)), int(1));
    assert_eq!(power(&float(2.0), &int(-2)), float(0.25));
    assert_eq!(power(&float(0.0), &float(2.0)), float(0.0));
    assert_eq!(divide(&int(7), &int(2)).unwrap(), int(3));
    assert_eq!(divide(&int(-1), &int(2)).unwrap(), int(i32::MAX));
    assert_eq!(divide(&int(-1), &float(2.0)).unwrap(), float(-0.5));
    assert!(divide(&int(1), &int(0)).is_err());
    assert!(divide(&float(1.0), &float(0.0)).is_err());
}

#[test]
fn decision_d60_a_list_past_the_cap_is_not_created() {
    let mut store = ArrayStore::default();
    for size in [i32::MAX, MAX_LIST_LENGTH + 1] {
        for storage in [Storage::Permanent, Storage::Temporary] {
            assert_eq!(store.create(size, 0, storage), 0, "{size}");
        }
    }
    assert!(store.is_empty());
    assert_eq!(store.next_id(), 1, "a refusal takes no ID");
    // A refusal is no creation, so `arrayexpr` keeps its last array.
    let first = store.create(1, 0, Storage::Permanent);
    assert_eq!(store.create(i32::MAX, 0, Storage::Permanent), 0);
    store.expression(int(0), int(5));
    assert_eq!(store.get(first, &int(0)), int(5));
    // The cap itself is a size like any other.
    let largest = store.create(MAX_LIST_LENGTH, 2, Storage::Temporary);
    assert_eq!(store.len(largest), 1 << 20);
    // A map has no size to refuse, and -2 stays refused.
    assert_ne!(store.create(-1, 0, Storage::Permanent), 0);
    assert_eq!(store.create(-2, 0, Storage::Permanent), 0);
}

#[test]
fn decision_d60_a_resize_past_the_cap_changes_nothing() {
    fn resize(store: &mut ArrayStore, id: i32, size: i32) -> bool {
        store.resize(id, size, |_| Ok::<_, ()>(0)).unwrap()
    }
    let mut store = ArrayStore::default();
    let list = store.create(2, 0, Storage::Permanent);
    store.set(list, int(1), int(7));
    for size in [i32::MAX, MAX_LIST_LENGTH + 1] {
        assert!(!resize(&mut store, list, size), "names no operation");
        assert_eq!(store.len(list), 2);
        assert_eq!(store.get(list, &int(1)), int(7));
    }
    assert!(resize(&mut store, list, MAX_LIST_LENGTH));
    assert_eq!(store.len(list), 1 << 20);
    assert_eq!(store.get(list, &int(1)), int(7));
    assert!(resize(&mut store, list, 3), "a shrink is always allowed");
    assert_eq!(store.len(list), 3);
    // Above its length a map changes nothing, as ever, with no diagnostic.
    let map = store.create(-1, 0, Storage::Permanent);
    assert!(resize(&mut store, map, i32::MAX));
    assert_eq!(store.len(map), 0);
}

/// A sidecar of `revision` whose one saved array is #3, next ID 4, with the
/// next ID then replaced by `next_id`.
/// Each of these tests holds about 100 MB: one at a time.
static TOTAL_CAP_TESTS: std::sync::Mutex<()> = std::sync::Mutex::new(());
fn one_at_a_time() -> std::sync::MutexGuard<'static, ()> {
    TOTAL_CAP_TESTS
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}
/// A store whose lists hold `4 << 20` elements, the cap exactly.
fn store_at_the_total_cap() -> (ArrayStore, [i32; 4]) {
    let mut store = ArrayStore::default();
    let lists = [(); 4].map(|_| store.create(MAX_LIST_LENGTH, 0, Storage::Permanent));
    assert!(lists.iter().all(|id| *id != 0));
    assert_eq!(store.total_elements(), MAX_TOTAL_ELEMENTS);
    (store, lists)
}
fn resized(store: &mut ArrayStore, id: i32, size: i32) -> bool {
    store.resize(id, size, |_| Ok::<_, ()>(0)).unwrap()
}

#[test]
fn a_list_past_the_total_cap_is_not_created() {
    let _turn = one_at_a_time();
    let (mut store, _) = store_at_the_total_cap();
    let next = store.next_id();
    for storage in [Storage::Permanent, Storage::Temporary] {
        assert_eq!(store.create(1, 0, storage), 0, "create_array or temp_array");
    }
    assert_eq!(store.next_id(), next, "no ID is taken");
    assert_eq!(store.arrays().len(), 4, "no array is made");
    assert_eq!(store.total_elements(), MAX_TOTAL_ELEMENTS);

    // What holds no element still fits, and takes its ID.
    assert_eq!(store.create(0, 0, Storage::Permanent), next as i32);
    let map = store.create(-1, 0, Storage::Permanent);
    assert_eq!(map, next as i32 + 1);

    // The last created array stays what it was, as with the list cap.
    assert_eq!(store.create(1, 0, Storage::Permanent), 0);
    store.expression(int(5), int(6));
    assert_eq!(store.get(map, &int(5)), int(6));
}

#[test]
fn a_resize_past_the_total_cap_changes_nothing() {
    let _turn = one_at_a_time();
    let (mut store, lists) = store_at_the_total_cap();
    assert!(resized(&mut store, lists[0], 8));
    let small = store.create(2, 0, Storage::Permanent);
    assert_ne!(small, 0);
    let room = MAX_LIST_LENGTH - 8 - 2;
    assert!(resized(&mut store, small, 2 + room));
    assert_eq!(store.total_elements(), MAX_TOTAL_ELEMENTS);

    let before = store.clone();
    assert!(!resized(&mut store, lists[0], 9));
    assert!(!resized(&mut store, small, 2 + room + 1));
    assert_eq!(store, before);

    // The size it has, and a shrink, are no growth.
    assert!(resized(&mut store, small, 2 + room));
    assert!(resized(&mut store, small, 2));
    assert_eq!(
        store.total_elements(),
        MAX_TOTAL_ELEMENTS - room as usize,
        "a shrink gives its elements back"
    );
    assert!(resized(&mut store, lists[0], 8 + room));
    assert_eq!(store.total_elements(), MAX_TOTAL_ELEMENTS);
}

#[test]
fn the_total_follows_every_way_elements_leave_or_arrive() {
    let _turn = one_at_a_time();
    let (mut store, lists) = store_at_the_total_cap();
    store.free(lists[0]);
    assert_eq!(store.total_elements(), 3 * MAX_LIST_LENGTH as usize);
    store.free(lists[0]);
    assert_eq!(store.total_elements(), 3 * MAX_LIST_LENGTH as usize, "gone");
    assert_ne!(store.create(MAX_LIST_LENGTH, 0, Storage::Temporary), 0);
    assert_eq!(store.total_elements(), MAX_TOTAL_ELEMENTS);
    store.end_of_step();
    assert_eq!(store.total_elements(), 3 * MAX_LIST_LENGTH as usize);

    // A map counts its pairs as it holds them, one each.
    let map = store.create(-1, 0, Storage::Permanent);
    store.set(map, int(1), int(5));
    store.set(map, int(2), int(6));
    store.set(map, int(3), int(7));
    store.set(map, int(3), int(8));
    store.set(map, int(9), int(0));
    let pairs = 3 * MAX_LIST_LENGTH as usize + 3;
    assert_eq!(store.total_elements(), pairs);
    store.set(map, int(2), int(0));
    assert_eq!(store.total_elements(), pairs - 1);
    assert!(resized(&mut store, map, 1));
    assert_eq!(store.total_elements(), pairs - 2);
    assert!(resized(&mut store, map, 50));
    assert_eq!(store.total_elements(), pairs - 2, "a map does not grow");

    // Lists and maps the engine builds are counted, though only the two
    // script words above refuse.
    let values = store.temp_list(vec![int(1), int(2)]);
    let kept = store.map_of(vec![(int(1), int(1))], Storage::Permanent);
    assert_eq!(store.total_elements(), pairs + 1);
    store.free(values);
    store.free(kept);
    store.free(map);
    assert_eq!(store.total_elements(), 3 * MAX_LIST_LENGTH as usize);
}

/// A sidecar whose saved list holds `count` values, written byte by byte: no
/// script word builds a store above the cap this cheaply.
fn sidecar_with_list(revision: u32, count: u32) -> Vec<u8> {
    let mut out = SIDECAR_TAG.to_vec();
    out.extend(revision.to_le_bytes());
    out.extend(2u32.to_le_bytes());
    if revision == 2 {
        out.extend(0x1000_0000u32.to_le_bytes());
        out.extend((-1i32).to_le_bytes());
        out.extend(0u32.to_le_bytes());
    }
    out.extend(0u32.to_le_bytes());
    out.extend(1u32.to_le_bytes());
    out.extend(1u32.to_le_bytes());
    out.extend([0, 0, Storage::Saved as u8, 0]);
    out.push(0);
    out.extend(count.to_le_bytes());
    for _ in 0..count {
        out.push(1);
        out.extend(7u32.to_le_bytes());
    }
    out
}

#[test]
fn a_sidecar_over_the_total_cap_loads_and_only_refuses_growth() {
    let _turn = one_at_a_time();
    let count = MAX_TOTAL_ELEMENTS as u32 + 1;
    for revision in [1, 2] {
        let (mut store, ..) = parse_sidecar(&sidecar_with_list(revision, count)).unwrap();
        assert_eq!(
            store.total_elements(),
            count as usize,
            "revision {revision}"
        );
        assert_eq!(store.len(1), count as i32);
        assert_eq!(store.get(1, &int(count as i32 - 1)), int(7));

        let next = store.next_id();
        assert_eq!(store.create(1, 0, Storage::Permanent), 0);
        assert_eq!(store.next_id(), next);
        let empty = store.create(0, 0, Storage::Permanent);
        assert_ne!(empty, 0);
        assert!(!resized(&mut store, empty, 1));

        // Scripts can bring it back under, where growth works again.
        assert!(resized(&mut store, 1, 1));
        assert_eq!(store.total_elements(), 1);
        assert!(resized(&mut store, empty, 1));
        store.free(1);
        assert_eq!(store.total_elements(), 1);
    }
}

fn sidecar_with_next_id(revision: u32, next_id: Option<u32>) -> Vec<u8> {
    let mut store = ArrayStore::default();
    for _ in 0..3 {
        store.create(1, 0, Storage::Permanent);
    }
    store.save(SfallValue::text("kept"), 3);
    let mut bytes = write_stream(SIDECAR_TAG, &store, Header::default(), &[], true);
    assert_eq!(bytes[8..12], 4u32.to_le_bytes());
    if revision == 1 {
        // Revision 1's header stops at offset 12 (§5.8).
        bytes[4..8].copy_from_slice(&1u32.to_le_bytes());
        bytes.drain(12..24);
    }
    if let Some(next_id) = next_id {
        bytes[8..12].copy_from_slice(&next_id.to_le_bytes());
    }
    bytes
}

#[test]
fn a_saved_next_array_id_must_continue_the_series() {
    for revision in [1, 2] {
        let parsed = |next_id| {
            parse_sidecar(&sidecar_with_next_id(revision, Some(next_id))).map(|(store, ..)| store)
        };
        // An untouched sidecar loads as before.
        let bytes = sidecar_with_next_id(revision, None);
        let (store, ..) = parse_sidecar(&bytes).unwrap();
        assert_eq!(store.next_id(), 4);
        assert!(store.exists(3));
        // Never 0; never an ID held; never below one, which the series would
        // reach; never past what a script can hold, up to u32::MAX.
        for next_id in [0, 1, 2, 3, 0x8000_0001, u32::MAX - 1, u32::MAX] {
            assert_eq!(
                parsed(next_id),
                Err(SidecarError::NextId(next_id)),
                "revision {revision}"
            );
        }
        assert_eq!(parsed(5).unwrap().next_id(), 5);
    }
    // With nothing saved, any start in the series will do.
    let empty = write_stream(
        SIDECAR_TAG,
        &ArrayStore::default(),
        Header::default(),
        &[],
        true,
    );
    assert_eq!(parse_sidecar(&empty).unwrap().0.next_id(), 1);
    let mut zero = empty;
    zero[8..12].fill(0);
    assert_eq!(parse_sidecar(&zero), Err(SidecarError::NextId(0)));
}

#[test]
fn the_last_array_id_stops_creation_and_the_save_still_loads() {
    let bytes = sidecar_with_next_id(2, Some(i32::MAX as u32));
    let (mut store, ..) = parse_sidecar(&bytes).unwrap();
    assert_eq!(store.create(0, 0, Storage::Permanent), i32::MAX);
    // The IDs have run out: no array, no reused ID, and no overflow.
    assert_eq!(store.next_id(), 0x8000_0000);
    assert_eq!(store.create(0, 0, Storage::Permanent), 0);
    assert_eq!(store.temp_list(vec![int(1)]), 0);
    assert_eq!(store.map_of(Vec::new(), Storage::Permanent), 0);
    assert_eq!(store.next_id(), 0x8000_0000);
    assert_eq!(store.arrays().len(), 2);
    // What the engine can reach, it can write and load back.
    let saved = write_stream(SIDECAR_TAG, &store, Header::default(), &[], true);
    assert_eq!(parse_sidecar(&saved).unwrap().0.next_id(), 0x8000_0000);
}

/// Code at address `start` that leaves `depth` host results, then runs a
/// loop of `no_ops` no-ops that drops one result per pass. Each shallower
/// arrival at the head explores the loop again, until the discard underflows.
fn shrinking_loop(depth: usize, no_ops: usize, start: u32) -> Vec<u8> {
    let mut code = [0x80, 0xbc].repeat(depth);
    let head = start + code.len() as u32;
    code.extend([0x80, 0x00].repeat(no_ops));
    code.extend([0x80, 0x1a, 0xc0, 0x01]);
    code.extend(head.to_be_bytes());
    code.extend([0x80, 0x04]);
    code
}

/// [`shrinking_loop`] as a bare procedure body, after its frame word.
fn looping_body(depth: usize, no_ops: usize) -> Image {
    let code = [vec![0x80, 0x2b], shrinking_loop(depth, no_ops, 2)].concat();
    Image::body(&code, 0)
}

fn findings(report: &crate::stack_check::UnitReport) -> Vec<(Finding, u16)> {
    report.findings.clone()
}

#[test]
fn a_cyclic_program_stops_at_its_step_budget_with_one_answer() {
    let table = Table::new(WordSet::FALLOUT1);
    // Within the budget, the loop ends as §8.2 says it does.
    let small = looping_body(20, 20);
    let report = analyse_unit(&small, &table, small.units[0]);
    assert_eq!(findings(&report), [(Finding::Underflow, 0x801a)]);
    // 30 passes of 10,003 steps overrun the floor, and the unit stops.
    let large = looping_body(30, 10_000);
    assert_eq!(large.step_budget(), MIN_STEPS);
    let first = analyse(&large, &table);
    assert_eq!(findings(&first[0]), [(Finding::StepBudget, 0)]);
    // Steps, not time: a second run agrees, here as a unit on its own.
    let alone = analyse_unit(&large, &table, large.units[0]);
    assert_eq!(findings(&alone), findings(&first[0]));
}

#[test]
fn a_program_over_budget_is_refused_by_the_load_check() {
    use vault_fixtures::state::{program, returning};
    // The jump needs the body's address, so parse once to learn it. The body
    // is `main`'s successor, and starts with `returning`'s frame word.
    let build = |body: u32| {
        let code = shrinking_loop(30, 10_000, body + 2);
        program(&[], &[(b"start".as_slice(), returning(code))])
    };
    let every = WordSet {
        words: 0x8000..=0xffff,
        mods: false,
    };
    let parsed = |image: &[u8]| IntProgram::parse_in(image, &every, false).unwrap();
    let body = parsed(&build(0)).procedures()[1].body;
    let hostile = parsed(&build(body));
    let table = Table::new(WordSet::FALLOUT1);
    let found = check("hostile.int", &hostile, &table, &[]);
    assert_eq!(summary(&found), "step budget x1");
    assert_eq!(found, check("hostile.int", &hostile, &table, &[]));
}

/// A body of `count` instructions that run straight through.
fn straight_line(count: usize) -> Image {
    let mut code = vec![0x80, 0x2b];
    code.extend([0x80, 0x00].repeat(count - 2));
    code.extend([0x80, 0x1c]);
    Image::body(&code, 0)
}

#[test]
fn the_step_budget_is_shared_by_a_programs_units() {
    // 16 passes of 20,000 instructions spend the budget exactly: all 16 are
    // clean, and each of the other 24 reports that it was not finished.
    let mut image = straight_line(20_000);
    assert_eq!(image.step_budget(), 20_000 * STEPS_PER_INSTRUCTION);
    image.units = vec![image.units[0]; 40];
    let table = Table::new(WordSet::FALLOUT1);
    let reports = analyse(&image, &table);
    assert_eq!(reports.len(), 40);
    assert!(
        reports[..16]
            .iter()
            .all(|report| report.findings.is_empty())
    );
    for report in &reports[16..] {
        assert_eq!(findings(report), [(Finding::StepBudget, 0)]);
    }
    // One such unit on its own is clean.
    assert!(
        analyse_unit(&image, &table, image.units[0])
            .findings
            .is_empty()
    );
}

#[test]
fn the_step_budget_scales_with_the_code_between_its_limits() {
    let per = STEPS_PER_INSTRUCTION as usize;
    assert_eq!(straight_line(2).step_budget(), MIN_STEPS);
    let floor = MIN_STEPS as usize / per;
    assert_eq!(straight_line(floor).step_budget(), MIN_STEPS);
    assert_eq!(
        straight_line(floor + 1).step_budget(),
        MIN_STEPS + STEPS_PER_INSTRUCTION
    );
    let ceiling = MAX_STEPS as usize / per;
    assert_eq!(straight_line(ceiling).step_budget(), MAX_STEPS);
    assert_eq!(straight_line(ceiling + 1).step_budget(), MAX_STEPS);
    // A clean program past the floor still passes: a flat budget would
    // refuse it for its size alone.
    let long = straight_line(MIN_STEPS as usize + 50_000);
    let table = Table::new(WordSet::FALLOUT1);
    assert!(analyse(&long, &table)[0].findings.is_empty());
}
