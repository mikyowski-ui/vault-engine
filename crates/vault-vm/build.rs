//! Compile the opcode ABI into exhaustive, checked metadata, from the committed
//! tables under `oracle/SPEC-005/` and `oracle/SPEC-024/`. No build reads a
//! spec: `tests/table_sync.rs` holds the SPEC-005 table to the spec's text.
#[path = "../../tools/build-support/tsv_table.rs"]
mod tsv_table;

use std::{collections::BTreeMap, env, fs, path::PathBuf};
use tsv_table::{Row, build_input, parse_word};

/// The `Rule` variants a row of `host-opcodes.tsv` can name. Anything else
/// fails the build at its row: nothing falls back to `Integer`.
const RULES: [&str; 11] = [
    "Integer",
    "String",
    "Float",
    "Unchecked",
    "Optional",
    "Filename",
    "FillColor",
    "TextColor",
    "IntegerOrString",
    "Previous",
    "ScrollFourth",
];
const FIRST_HOST_WORD: u16 = 0x804c;
const HOST_WORDS: usize = 266;

/// A file of OUT_DIR.
fn output(name: &str, contents: String) -> Result<(), String> {
    let out = env::var_os("OUT_DIR").ok_or("OUT_DIR is not set")?;
    fs::write(PathBuf::from(out).join(name), contents).map_err(|error| format!("{name}: {error}"))
}

/// One row of `host-opcodes.tsv`, as an `OpcodeSpec`.
fn host_row(row: &Row, word: u16) -> Result<String, String> {
    let name = row.text("name")?;
    let mut operands = Vec::new();
    for operand in row.list("operands")? {
        if !RULES.contains(&operand) {
            return Err(row.column_error(
                "operands",
                format_args!("unknown operand type {operand:?}, expected one of {RULES:?}"),
            ));
        }
        operands.push(format!("Rule::{operand}"));
    }
    // The verbatim cell has one comma-separated item per operand.
    let stated = match row.text("spec_operands")? {
        "none" => 0,
        cell => cell.split(',').count(),
    };
    if stated != operands.len() {
        return Err(row.error(format_args!(
            "{} operand types for {stated} operands in `spec_operands`",
            operands.len()
        )));
    }
    let push = row.one_of("push", &["None", "Value", "Text"])?;
    Ok(format!(
        "OpcodeSpec {{word:0x{word:04x},name:{name:?},operands:&[{}],push:PushKind::{push},\
         registered:{},variable:{}}}",
        operands.join(","),
        row.flag("registered")?,
        row.flag("variable")?,
    ))
}

fn main() {
    if let Err(error) = run() {
        eprintln!("{error}");
        std::process::exit(1);
    }
}

fn run() -> Result<(), String> {
    let table = build_input(
        "oracle/SPEC-005/host-opcodes.tsv",
        Some("SPEC-005 sections 2.3 to 2.10"),
        &[
            "word",
            "section",
            "name",
            "operands",
            "spec_operands",
            "push",
            "spec_return",
            "registered",
            "variable",
        ],
    )?;
    let mut entries = Vec::new();
    for (row, wanted) in table.rows().zip(FIRST_HOST_WORD..) {
        let word = row.word("word")?;
        if word != wanted {
            return Err(row.column_error(
                "word",
                format_args!("the words count up, so this one must be {wanted:#06x}"),
            ));
        }
        entries.push(host_row(&row, word)?);
    }
    table.expect_rows(HOST_WORDS)?;
    output(
        "extension_catalog.rs",
        format!(
            "pub const EXTENSIONS: [OpcodeSpec;266] = [\n{}\n];\n",
            entries.join(",\n")
        ),
    )?;
    sfall_catalog()
}
/// The `Class` of an operand or return (SPEC-024b section 4.1). There is no
/// default: an unknown class fails at its row.
fn class(row: &Row, column: &str, text: &str) -> Result<String, String> {
    if matches!(text, "I" | "F" | "N" | "S" | "K" | "A" | "O" | "P") {
        return Ok(format!("Class::{text}"));
    }
    Err(row.column_error(column, format_args!("unknown operand class {text:?}")))
}
/// A word cell. `SfallRow` holds it in lower case, as a Rust literal.
fn word_in(row: &Row, column: &str, text: &str) -> Result<u16, String> {
    parse_word(text)
        .ok_or_else(|| row.column_error(column, format_args!("{text:?} is not a 0xNNNN word")))
}
/// The contract columns `sfall-opcodes.tsv` and `mod-words.tsv` share, as a
/// `SfallRow`. `kind` says what the `id` cell holds, and only a reserved row of
/// `tier` `R` may leave `pops` and `push` as `-`.
fn sfall_row(
    row: &Row,
    kind: &str,
    id: &str,
    dispatch: &str,
    tier: &str,
) -> Result<String, String> {
    let id = match kind {
        "word" => word_in(row, "id", id)?,
        "metarule3" | "metarule2_explosions" => row.parse("id")?,
        // A function's `id` is its name string: it has no number.
        "sfall_func" => 0,
        kind => return Err(row.error(format_args!("unknown oracle row {kind:?}"))),
    };
    let dispatch = word_in(row, "dispatch_word", dispatch)?;
    let reserved = tier == "R";
    let pops = match row.text("pops")? {
        "-" if reserved => "None".to_owned(),
        _ => format!("Some({})", row.parse::<u8>("pops")?),
    };
    let push = match row.text("push")? {
        "-" if reserved => false,
        _ => row.flag("push")?,
    };
    let args = row
        .list("arg_classes")?
        .into_iter()
        .map(|text| class(row, "arg_classes", text))
        .collect::<Result<Vec<_>, _>>()?;
    let ret = match row.text("ret_class")? {
        "-" => "None".to_owned(),
        text => format!("Some({})", class(row, "ret_class", text)?),
    };
    Ok(format!(
        "SfallRow {{id:0x{id:04x},name:{:?},dispatch:0x{dispatch:04x},pops:{pops},push:{push},\
         args:&[{}],ret:{ret},tier:Tier::{tier}}}",
        row.text("mnemonic")?,
        args.join(","),
    ))
}
fn hook_types(row: &Row, column: &str) -> Result<String, String> {
    let types = row
        .list(column)?
        .into_iter()
        .map(|name| match name {
            "int" => Ok("HookType::Int"),
            "critter" => Ok("HookType::Critter"),
            "item" => Ok("HookType::Item"),
            "obj" => Ok("HookType::Object"),
            "mixed" => Ok("HookType::Mixed"),
            "string" => Ok("HookType::Text"),
            other => Err(row.column_error(column, format_args!("unknown hook type {other:?}"))),
        })
        .collect::<Result<Vec<_>, _>>()?;
    Ok(types.join(","))
}
/// `oracle/SPEC-024/sfall-hooks.tsv`: the 50 hooks, by ID. The reserved range
/// has one row of tier `R`, which makes no hook.
fn sfall_hooks() -> Result<Vec<String>, String> {
    let table = build_input(
        "oracle/SPEC-024/sfall-hooks.tsv",
        None,
        &[
            "hook_id",
            "hook_name",
            "script_file",
            "tier",
            "arg_count",
            "arg_types",
            "ret_count",
            "ret_types",
            "deprecated",
            "tag",
        ],
    )?;
    let mut rows = Vec::new();
    for row in table.rows() {
        let tier = row.one_of("tier", &["P1", "P2", "R"])?;
        if tier == "R" {
            continue;
        }
        rows.push(format!(
            "HookRow {{id:{},name:{:?},file:{:?},fires:{},args:&[{}],rets:&[{}]}}",
            row.parse::<i32>("hook_id")?,
            row.text("hook_name")?,
            row.text("script_file")?,
            tier == "P1",
            hook_types(&row, "arg_types")?,
            hook_types(&row, "ret_types")?,
        ));
    }
    if rows.len() != 50 {
        return Err(format!("{}: {} hooks, not 50", table.file(), rows.len()));
    }
    Ok(rows)
}
/// SPEC-024a section 7.3: `tests/fixtures/spec024a/mod-words.tsv`, as rows of
/// the value model and as the stack model's rows.
fn mod_words() -> Result<(Vec<String>, Vec<String>), String> {
    let table = build_input(
        "tests/fixtures/spec024a/mod-words.tsv",
        None,
        &[
            "word",
            "mnemonic",
            "pops",
            "push",
            "arg_classes",
            "ret_class",
            "pop_order",
        ],
    )?;
    let (mut rows, mut specs) = (Vec::new(), Vec::new());
    for row in table.rows() {
        let word_text = row.text("word")?;
        let word = row.word("word")?;
        let pops: usize = row.parse("pops")?;
        rows.push(sfall_row(&row, "word", word_text, word_text, "P1")?);
        specs.push(format!(
            "OpcodeSpec {{word:0x{word:04x},name:{:?},operands:&[{}],\
             push:PushKind::{},registered:true,variable:false}}",
            row.text("mnemonic")?,
            vec!["Rule::Unchecked"; pops].join(","),
            if row.flag("push")? { "Value" } else { "None" },
        ));
    }
    table.expect_rows(11)?;
    Ok((rows, specs))
}
/// SPEC-024b section 3: the oracle table, compiled in and counted, so a word
/// cannot drift from the published contract. A reserved word has a row with
/// no pop count, and no `OpcodeSpec` for the stack model.
fn sfall_catalog() -> Result<(), String> {
    let oracle = build_input(
        "oracle/SPEC-024/sfall-opcodes.tsv",
        None,
        &[
            "kind",
            "id",
            "mnemonic",
            "dispatch_word",
            "pops",
            "push",
            "arg_classes",
            "ret_class",
            "category",
            "tier",
            "tag",
        ],
    )?;
    let mut words = BTreeMap::new();
    let mut specs = Vec::new();
    let (mut functions, mut metarule3, mut explosions) = (Vec::new(), Vec::new(), Vec::new());
    for row in oracle.rows() {
        let kind = row.text("kind")?;
        let tier = row.one_of("tier", &["P1", "P2", "X", "R"])?;
        let text = sfall_row(
            &row,
            kind,
            row.text("id")?,
            row.text("dispatch_word")?,
            tier,
        )?;
        match kind {
            "word" => {
                let word = row.word("id")?;
                if words.insert(word, text).is_some() {
                    return Err(row.column_error("id", "a duplicate word"));
                }
                if tier != "R" {
                    let pops: usize = row.parse("pops")?;
                    specs.push(format!(
                        "OpcodeSpec {{word:0x{word:04x},name:{:?},operands:&[{}],\
                         push:PushKind::{},registered:true,variable:false}}",
                        row.text("mnemonic")?,
                        vec!["Rule::Unchecked"; pops].join(","),
                        if row.flag("push")? { "Value" } else { "None" },
                    ));
                }
            }
            "sfall_func" => functions.push(text),
            "metarule3" => metarule3.push(text),
            _ => explosions.push(text),
        }
    }
    let counted = |what: &str, count: usize, wanted: usize| {
        if count == wanted {
            Ok(())
        } else {
            Err(format!("{}: {count} {what}, not {wanted}", oracle.file()))
        }
    };
    counted("words", words.len(), 300)?;
    for word in 0x8156..=0x8281 {
        if !words.contains_key(&word) {
            return Err(format!("{}: no word {word:#06x}", oracle.file()));
        }
    }
    counted("unreserved words", specs.len(), 294)?;
    counted("sfall_func rows", functions.len(), 133)?;
    counted("metarule3 rows", metarule3.len(), 7)?;
    counted("metarule2_explosions rows", explosions.len(), 9)?;
    let table = |name: &str, kind: &str, rows: &[String]| {
        format!(
            "pub static {name}: [{kind};{}] = [\n{}\n];\n",
            rows.len(),
            rows.join(",\n")
        )
    };
    let hooks = sfall_hooks()?;
    let (mod_rows, mod_specs) = mod_words()?;
    let source = [
        table("MOD_ROWS", "SfallRow", &mod_rows),
        table("MOD_SPECS", "OpcodeSpec", &mod_specs),
        table("SFALL_HOOKS", "HookRow", &hooks),
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
    output("sfall_catalog.rs", source)
}
