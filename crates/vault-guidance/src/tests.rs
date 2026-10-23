//! SPEC-008 §3.8 tables and the §4.4 quest/holodisk vectors.
use super::*;

fn catalog(entries: &[(i32, &str)]) -> MsgCatalog {
    let text: String = entries
        .iter()
        .map(|(id, text)| format!("{{{id}}}{{}}{{{text}}}\n"))
        .collect();
    MsgCatalog::parse(text.as_bytes()).unwrap()
}

#[test]
fn spec008_fixed_disk_and_quest_tables() {
    assert_eq!(
        DISK_GLOBALS,
        [
            162, 163, 161, 164, 165, 166, 167, 209, 210, 211, 212, 234, 235, 258, 259, 260, 237,
            311
        ]
    );
    assert_eq!(QUESTS[0], [238, 309, 308, 101, 188, 0, 0, 0, 0]);
    assert_eq!(QUESTS[1], [0; 9]);
    assert_eq!(QUESTS[3], [144, 105, 143, 145, 104, 555, 0, 0, 0]);
    assert_eq!(QUESTS[10], [135, 127, 129, 133, 138, 130, 128, 0, 0]);
    assert_eq!(QUESTS[11], [0; 9]);
}

#[test]
fn spec008_fixture_quest_thresholds_water_chip_and_numbering() {
    let mut gvars = vec![0; 600];
    // Index 101 at zero: the water-chip quest is visible and active, which
    // also makes Vault 13's group visible.
    assert_eq!(quest_state(101, 0), QuestState::Active);
    assert_eq!(quest_state(101, 1), QuestState::Active);
    assert_eq!(quest_state(101, 2), QuestState::Completed);
    assert_eq!(quest_state(238, 0), QuestState::Hidden);
    assert_eq!(quest_state(238, -3), QuestState::Hidden);
    assert_eq!(quest_state(238, 1), QuestState::Active);
    assert_eq!(quest_state(238, 2), QuestState::Completed);
    assert_eq!(quest_locations(&gvars), [(0, 700)]);
    let vault = town_quests(0, &gvars);
    assert_eq!(vault.len(), 1);
    assert_eq!(
        vault[0],
        QuestEntry {
            number: 1,
            position: 3,
            global: 101,
            message: 704,
            state: QuestState::Active
        }
    );
    // Compact numbering with hidden intervening entries; ids keep positions.
    gvars[144] = 1;
    gvars[143] = 2;
    let junktown = town_quests(3, &gvars);
    assert_eq!(
        junktown
            .iter()
            .map(|q| (q.number, q.position, q.message, q.state))
            .collect::<Vec<_>>(),
        [
            (1, 0, 731, QuestState::Active),
            (2, 2, 733, QuestState::Completed)
        ]
    );
    assert_eq!(quest_locations(&gvars), [(0, 700), (3, 730)]);
    // Safe rule: the location pass stops at the zero sentinel like the
    // detail pass, so global zero cannot advertise empty groups.
    gvars[0] = 5;
    let listed: Vec<_> = quest_locations(&gvars).iter().map(|(t, _)| *t).collect();
    assert_eq!(listed, [0, 3]);
    assert!(town_quests(1, &gvars).is_empty());
    assert_eq!(town_quests(12, &gvars), []);
}

#[test]
fn spec008_fixture_holodisk_acquisition_pages_and_markers() {
    let mut gvars = vec![0; 400];
    assert!(listed_disks(&gvars).is_empty());
    gvars[162] = 1;
    assert_eq!(listed_disks(&gvars), [0]);
    assert_eq!(selected_disk(&gvars, 0), Some(0));
    // Safe rule: a negative value is neither listed nor selected.
    gvars[163] = -1;
    gvars[161] = 1;
    assert_eq!(listed_disks(&gvars), [0, 2]);
    assert_eq!(selected_disk(&gvars, 1), Some(2));
    assert_eq!(selected_disk(&gvars, 2), None);

    // Exactly 35 body records then **END-DISK**: under the safe rule the
    // last page index is 0 (the reference exposes an empty page 1).
    let mut entries = vec![(400, "Synthetic Disk".to_string())];
    for record in 0..35 {
        let text = if record == 3 {
            "**END-PAR**".to_string()
        } else {
            format!("line {record}")
        };
        entries.push((1000 + record, text));
    }
    entries.push((1035, "**END-DISK**".to_string()));
    let refs: Vec<_> = entries.iter().map(|(id, t)| (*id, t.as_str())).collect();
    let disk = holodisk(0, &catalog(&refs)).unwrap();
    assert_eq!(disk.title, b"Synthetic Disk");
    assert_eq!(disk.records.len(), 35);
    assert_eq!(disk.last_page, 0);
    assert_eq!(disk.page(0).len(), 35);
    assert!(disk.page(1).is_empty());
    // A page index whose record offset overflows is simply past the end.
    assert!(disk.page(usize::MAX).is_empty());
    assert_eq!(disk.records[3], DiskLine::Paragraph);
    assert_eq!(disk.records[0], DiskLine::Text(b"line 0".to_vec()));
    // 36 records: still page index 1, now non-empty.
    let mut longer = refs.clone();
    longer.pop();
    longer.push((1035, "line 35"));
    longer.push((1036, "**END-DISK**"));
    let disk = holodisk(0, &catalog(&longer)).unwrap();
    assert_eq!((disk.last_page, disk.page(1).len()), (1, 1));
    // Disk ordinal 1's window starts at 2000.
    let short = catalog(&[(401, "Two"), (2000, "**END-DISK**")]);
    let disk = holodisk(1, &short).unwrap();
    assert_eq!((disk.records.len(), disk.last_page), (0, 0));

    // Missing termination is diagnosed, and the disk shows the whole
    // 500-record window (the safe rule stops at the window bound).
    let mut open = vec![(400, "Open".to_string())];
    open.extend((1000..1500).map(|id| (id, "text".to_string())));
    open.push((1500, "**END-DISK**".to_string()));
    let refs: Vec<_> = open.iter().map(|(id, t)| (*id, t.as_str())).collect();
    let disk = holodisk(0, &catalog(&refs)).unwrap();
    assert_eq!((disk.records.len(), disk.last_page), (500, 14));
    assert_eq!(disk.page(14).len(), 10);
    assert_eq!(disk.diagnostics, [DiskDiagnostic::Unterminated(0)]);
    // A gap is neither skipped nor fatal: it reads as one `Error` record.
    let disk = holodisk(0, &catalog(&[(400, "Gap"), (1001, "**END-DISK**")])).unwrap();
    assert_eq!(disk.records, [DiskLine::Text(b"Error".to_vec())]);
    assert_eq!(disk.diagnostics, [DiskDiagnostic::MissingRecord(1000)]);
    let gap = holodisk(
        1,
        &catalog(&[(401, "Gap"), (2000, "a"), (2002, "**END-DISK**")]),
    )
    .unwrap();
    assert_eq!(
        gap.records,
        [
            DiskLine::Text(b"a".to_vec()),
            DiskLine::Text(b"Error".to_vec())
        ]
    );
    assert_eq!(
        gap.diagnostics[0].to_string(),
        "holodisk 1 content message 2001 is missing from the 2000-2499 window; showing Error"
    );
    // A gap counts toward the 35-record page budget.
    let mut gappy: Vec<(i32, String)> = vec![(400, "Paged".to_string())];
    gappy.extend(
        (1000..1040)
            .filter(|id| id % 2 == 0)
            .map(|id| (id, "x".to_string())),
    );
    gappy.push((1040, "**END-DISK**".to_string()));
    let refs: Vec<_> = gappy.iter().map(|(id, t)| (*id, t.as_str())).collect();
    let disk = holodisk(0, &catalog(&refs)).unwrap();
    assert_eq!(
        (disk.records.len(), disk.last_page, disk.page(1).len()),
        (40, 1, 5)
    );
    assert_eq!(disk.diagnostics.len(), 20);
    // A missing title shows the placeholder too; the window is still read.
    let disk = holodisk(0, &catalog(&[(1000, "**END-DISK**")])).unwrap();
    assert_eq!(disk.title, b"Error");
    assert_eq!(disk.diagnostics, [DiskDiagnostic::MissingTitle(400)]);
    assert_eq!(holodisk(18, &catalog(&[])), Err(DiskError::Ordinal(18)));
}
