//! Compile the approved opcode ABI table into exhaustive, checked metadata.
use std::{collections::BTreeMap, env, fs, path::PathBuf};
fn rule(operand: &str) -> &'static str {
    if operand.contains("S/0/-1") {
        "ScrollFourth"
    } else if operand.contains("S or I zero") {
        "Previous"
    } else if operand.contains("I or S") || operand.contains("S or I") {
        "IntegerOrString"
    } else {
        match operand.split_whitespace().last().unwrap_or("") {
            "S" => "String",
            "F" => "Float",
            "U" => "Unchecked",
            "O" => "Optional",
            "N" => "Filename",
            "C" => "FillColor",
            "T" => "TextColor",
            _ => "Integer",
        }
    }
}
fn main() {
    let spec = PathBuf::from(env::var_os("CARGO_MANIFEST_DIR").unwrap())
        .join("../../specs/approved/SPEC-005-host-opcodes.md");
    println!("cargo:rerun-if-changed={}", spec.display());
    let text = fs::read_to_string(spec).expect("approved SPEC-005 must be present");
    let mut entries = BTreeMap::new();
    for line in text.lines().filter(|line| line.starts_with("| `0x")) {
        let cells: Vec<_> = line.split('|').map(str::trim).collect();
        if cells.len() < 7 {
            continue;
        }
        let word = cells[1].trim_matches('`').trim_start_matches("0x");
        let Ok(opcode) = u16::from_str_radix(word, 16) else {
            continue;
        };
        if !(0x804c..=0x8155).contains(&opcode) {
            continue;
        }
        let name = cells[2].trim_matches('`');
        let operands = if cells[3] == "none" {
            Vec::new()
        } else {
            cells[3]
                .split(',')
                .map(|operand| format!("Rule::{}", rule(operand)))
                .collect::<Vec<_>>()
        };
        let push = if cells[4] == "none" {
            "None"
        } else if cells[4].starts_with("D") {
            "Text"
        } else {
            "Value"
        };
        let row = format!(
            "OpcodeSpec {{word:0x{opcode:04x},name:{name:?},operands:&[{}],push:PushKind::{push},registered:{},variable:{}}}",
            operands.join(","),
            name != "Unregistered",
            opcode == 0x807f
        );
        assert!(entries.insert(opcode, row).is_none(), "duplicate opcode");
    }
    assert_eq!(
        entries.len(),
        266,
        "the complete catalog must have all 266 slots"
    );
    for opcode in 0x804c..=0x8155 {
        assert!(entries.contains_key(&opcode), "missing opcode");
    }
    let source = format!(
        "pub const EXTENSIONS: [OpcodeSpec;266] = [\n{}\n];\n",
        entries.values().cloned().collect::<Vec<_>>().join(",\n")
    );
    fs::write(
        PathBuf::from(env::var_os("OUT_DIR").unwrap()).join("extension_catalog.rs"),
        source,
    )
    .unwrap();
    sfall_catalog();
}
fn class(text: &str) -> String {
    assert!(matches!(
        text,
        "I" | "F" | "N" | "S" | "K" | "A" | "O" | "P"
    ));
    format!("Class::{text}")
}
/// One row of `oracle/SPEC-024/sfall-opcodes.tsv`, as a `SfallRow`.
fn sfall_row(cells: &[&str]) -> String {
    let id = cells[1].strip_prefix("0x").map_or_else(
        || cells[1].parse::<u16>().unwrap_or(0),
        |word| u16::from_str_radix(word, 16).expect("a word"),
    );
    let dispatch = u16::from_str_radix(cells[3].trim_start_matches("0x"), 16).expect("a word");
    let pops = match cells[4] {
        "-" => "None".to_owned(),
        count => format!("Some({})", count.parse::<u8>().expect("a count")),
    };
    let args: Vec<String> = cells[6]
        .split(',')
        .filter(|class| *class != "-")
        .map(class)
        .collect();
    let ret = match cells[7] {
        "-" => "None".to_owned(),
        text => format!("Some({})", class(text)),
    };
    format!(
        "SfallRow {{id:0x{id:04x},name:{:?},dispatch:0x{dispatch:04x},pops:{pops},push:{},\
         args:&[{}],ret:{ret},tier:Tier::{}}}",
        cells[2],
        cells[5] == "1",
        args.join(","),
        cells[9],
    )
}
/// SPEC-024b section 3: the oracle table, compiled in and counted, so a word
/// cannot drift from the published contract. A reserved word has a row with
/// no pop count, and no `OpcodeSpec` for the stack model.
fn sfall_catalog() {
    let table = PathBuf::from(env::var_os("CARGO_MANIFEST_DIR").unwrap())
        .join("../../oracle/SPEC-024/sfall-opcodes.tsv");
    println!("cargo:rerun-if-changed={}", table.display());
    let text = fs::read_to_string(table).expect("the SPEC-024 oracle must be present");
    let mut words = BTreeMap::new();
    let mut specs = Vec::new();
    let (mut functions, mut metarule3, mut explosions) = (Vec::new(), Vec::new(), Vec::new());
    for line in text.lines().skip(1) {
        let cells: Vec<&str> = line.split('\t').collect();
        assert_eq!(cells.len(), 11, "an oracle row has eleven columns");
        let row = sfall_row(&cells);
        match cells[0] {
            "word" => {
                let word = u16::from_str_radix(cells[1].trim_start_matches("0x"), 16).unwrap();
                assert!(words.insert(word, row).is_none(), "duplicate word");
                if cells[9] != "R" {
                    let pops: usize = cells[4].parse().expect("a pop count");
                    specs.push(format!(
                        "OpcodeSpec {{word:0x{word:04x},name:{:?},operands:&[{}],\
                         push:PushKind::{},registered:true,variable:false}}",
                        cells[2],
                        vec!["Rule::Unchecked"; pops].join(","),
                        if cells[5] == "1" { "Value" } else { "None" },
                    ));
                }
            }
            "sfall_func" => functions.push(row),
            "metarule3" => metarule3.push(row),
            "metarule2_explosions" => explosions.push(row),
            kind => panic!("unknown oracle row {kind}"),
        }
    }
    assert_eq!(words.len(), 300, "every word 0x8156..=0x8281");
    for word in 0x8156..=0x8281 {
        assert!(words.contains_key(&word), "missing word {word:#06x}");
    }
    assert_eq!(specs.len(), 294, "every word but the six reserved");
    assert_eq!(functions.len(), 133);
    assert_eq!(metarule3.len(), 7);
    assert_eq!(explosions.len(), 9);
    let table = |name: &str, kind: &str, rows: &[String]| {
        format!(
            "pub static {name}: [{kind};{}] = [\n{}\n];\n",
            rows.len(),
            rows.join(",\n")
        )
    };
    let source = [
        table(
            "SFALL_WORDS",
            "SfallRow",
            &words.into_values().collect::<Vec<_>>(),
        ),
        table("SFALL_SPECS", "OpcodeSpec", &specs),
        table("SFALL_FUNCTIONS", "SfallRow", &functions),
        table("SFALL_METARULE3", "SfallRow", &metarule3),
        table("SFALL_EXPLOSIONS", "SfallRow", &explosions),
    ]
    .concat();
    fs::write(
        PathBuf::from(env::var_os("OUT_DIR").unwrap()).join("sfall_catalog.rs"),
        source,
    )
    .unwrap();
}
