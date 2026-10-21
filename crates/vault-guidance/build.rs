//! Compile the approved SPEC-008 Section 3.8 holodisk and quest tables.
use std::{env, fs, path::PathBuf};

fn main() {
    let spec = PathBuf::from(env::var_os("CARGO_MANIFEST_DIR").unwrap())
        .join("../../specs/approved/SPEC-008-dat-saves-and-worldmap.md");
    println!("cargo:rerun-if-changed={}", spec.display());
    let text = fs::read_to_string(spec).expect("approved SPEC-008 must be present");
    // The section is the lines after the one heading whose number is exactly
    // 3.8 ("### 3.8 Title"), up to the next heading of level 3 or higher;
    // a heading such as "### 3.80" or a "3.8" mention elsewhere never matches.
    let is_heading = |line: &str| line.starts_with("### ") || line.starts_with("## ");
    let headings: Vec<usize> = text
        .lines()
        .enumerate()
        .filter(|(_, line)| {
            line.strip_prefix("### 3.8")
                .is_some_and(|rest| rest.is_empty() || rest.starts_with(' '))
        })
        .map(|(index, _)| index)
        .collect();
    assert_eq!(
        headings.len(),
        1,
        "exactly one SPEC-008 section 3.8 heading"
    );
    let section: String = text
        .lines()
        .skip(headings[0] + 1)
        .take_while(|line| !is_heading(line))
        .collect::<Vec<_>>()
        .join("\n");
    let section = section.as_str();

    let numbers = |prefix: &str| -> Vec<usize> {
        let line = section
            .lines()
            .find(|line| line.starts_with(prefix))
            .unwrap_or_else(|| panic!("{prefix} line"));
        line[prefix.len()..]
            .split_whitespace()
            .map(|value| value.parse().expect("decimal value"))
            .collect()
    };
    let ordinals = numbers("Ordinal:");
    let globals = numbers("Global:");
    assert_eq!(ordinals, (0..18).collect::<Vec<_>>(), "disk ordinals 0-17");
    assert_eq!(globals.len(), 18, "one controlling global per disk");

    let mut quests = Vec::new();
    for line in section.lines() {
        let cells: Vec<_> = line.split('|').map(str::trim).collect();
        if cells.len() != 4 {
            continue;
        }
        let Ok(town) = cells[1].parse::<usize>() else {
            continue;
        };
        assert_eq!(town, quests.len(), "quest rows are in town order");
        let mut row = vec![0_usize; 9];
        if cells[2] != "None" {
            for (slot, index) in cells[2].split(',').enumerate() {
                row[slot] = index.parse().expect("quest campaign index");
            }
        }
        quests.push(format!("{row:?}"));
    }
    assert_eq!(quests.len(), 12, "quest rows for towns 0-11");
    let source = format!(
        "pub static DISK_GLOBALS: [usize; 18] = {globals:?};\n\
         pub static QUESTS: [[usize; 9]; 12] = [{}];\n",
        quests.join(",")
    );
    fs::write(
        PathBuf::from(env::var_os("OUT_DIR").unwrap()).join("pipboy_tables.rs"),
        source,
    )
    .unwrap();
}
