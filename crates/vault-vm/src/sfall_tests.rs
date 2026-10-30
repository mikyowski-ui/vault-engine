//! SPEC-024b sections 3, 4 and 7, in the VM: the table against the oracle,
//! the math bits and the string vectors of §7.2.
use crate::extensions::{ENHANCED_RANGE, opcode_in};
use crate::sfall::*;
use crate::stack_check::Table;
use crate::{WordSet, sfall};

const ORACLE: &str = include_str!("../../../oracle/SPEC-024/sfall-opcodes.tsv");

fn int(value: i32) -> SfallValue {
    SfallValue::Int(value)
}
fn float(value: f32) -> SfallValue {
    SfallValue::float(value)
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
    let count = |tier| SFALL_WORDS.iter().filter(|row| row.tier == tier).count();
    assert_eq!(
        (
            count(Tier::P1),
            count(Tier::P2),
            count(Tier::X),
            count(Tier::R)
        ),
        (99, 177, 18, 6)
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
    assert_eq!(names.len(), 18);
    assert!(names.windows(2).all(|pair| pair[0] < pair[1]));
    assert!(names.contains(&"string_find"));
    assert!(!names.contains(&"message_box"));
    assert_eq!(function_tier(b"string_find"), Some(Tier::P1));
    assert_eq!(function_tier(b"message_box"), Some(Tier::P2));
    assert_eq!(function_tier(b"no_such_fn"), None);
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
