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
}
