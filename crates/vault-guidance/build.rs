//! Compile the SPEC-008 Section 3.8 holodisk and quest tables, from their
//! committed form under `oracle/SPEC-008/`. No build reads a spec:
//! `tests/table_sync.rs` holds these tables to the spec's text.
#[path = "../../tools/build-support/tsv_table.rs"]
mod tsv_table;

use std::{env, fs, path::PathBuf};
use tsv_table::build_input;

const SOURCE: Option<&str> = Some("SPEC-008 section 3.8");
/// The positions of one quest row, with trailing zero sentinels.
const QUEST_SLOTS: usize = 9;

fn main() {
    if let Err(error) = run() {
        eprintln!("{error}");
        std::process::exit(1);
    }
}

fn run() -> Result<(), String> {
    let disks = build_input(
        "oracle/SPEC-008/holodisks.tsv",
        SOURCE,
        &["ordinal", "global"],
    )?;
    let mut globals = Vec::new();
    for row in disks.rows() {
        let ordinal: usize = row.parse("ordinal")?;
        if ordinal != globals.len() {
            return Err(row.column_error("ordinal", "disk ordinals run 0 to 17 in order"));
        }
        globals.push(row.parse::<usize>("global")?);
    }
    disks.expect_rows(18)?;

    let table = build_input("oracle/SPEC-008/quests.tsv", SOURCE, &["town", "globals"])?;
    let mut quests = Vec::new();
    for row in table.rows() {
        let town: usize = row.parse("town")?;
        if town != quests.len() {
            return Err(row.column_error("town", "quest rows are in town order"));
        }
        let listed = row.list("globals")?;
        if listed.len() > QUEST_SLOTS {
            return Err(row.column_error(
                "globals",
                format_args!(
                    "{} quests, but a row has {QUEST_SLOTS} positions",
                    listed.len()
                ),
            ));
        }
        let mut positions = vec![0_usize; QUEST_SLOTS];
        for (slot, index) in listed.into_iter().enumerate() {
            positions[slot] = index
                .parse()
                .map_err(|error| row.column_error("globals", format_args!("{index:?}: {error}")))?;
        }
        quests.push(format!("{positions:?}"));
    }
    table.expect_rows(12)?;

    let source = format!(
        "pub static DISK_GLOBALS: [usize; 18] = {globals:?};\n\
         pub static QUESTS: [[usize; 9]; 12] = [{}];\n",
        quests.join(",")
    );
    let out = env::var_os("OUT_DIR").ok_or("OUT_DIR is not set")?;
    fs::write(PathBuf::from(out).join("pipboy_tables.rs"), source)
        .map_err(|error| format!("pipboy_tables.rs: {error}"))
}
