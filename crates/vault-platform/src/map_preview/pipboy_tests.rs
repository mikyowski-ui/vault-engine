//! SPEC-014 §3.41 the Pip-Boy over the synthetic corpus, following Fixture
//! J where the synthetic font allows (its widths are the corpus font's in
//! the vault-core layout tests).
use super::{AutomapView, Page, PipBoy, StatusView, Tab};
use crate::map_preview::{
    MapPreview, Screen, state_fixture,
    test_support::{interactive, local_corpus, npc, synthetic_corpus},
};
use vault_core::ui::{automap, saveload::MAP_SHORT_NAMES};
use winit::keyboard::KeyCode;

fn corpus() -> tempfile::TempDir {
    let dir = synthetic_corpus();
    std::fs::write(dir.path().join("font1.aaf"), state_fixture::preview_aaf()).unwrap();
    dir
}
fn load(dir: &std::path::Path) -> MapPreview {
    let mut preview = interactive(dir);
    preview.image().unwrap();
    preview.host.world.gvars = Some(vec![0; 620]);
    preview
}
fn pipboy(preview: &MapPreview) -> &PipBoy {
    match &preview.screen {
        Screen::PipBoy(pipboy) => pipboy,
        _ => panic!("the Pip-Boy is not open"),
    }
}
fn gvar(preview: &mut MapPreview, index: usize, value: i32) {
    preview.host.world.gvars.as_mut().unwrap()[index] = value;
}
/// Let a 200 ms flash run out.
fn settle(preview: &mut MapPreview, now: &mut u64) {
    *now += 200;
    preview.tick(*now).unwrap();
}
fn pixel(preview: &mut MapPreview, [x, y]: [i32; 2]) -> u8 {
    let image = preview.image().unwrap();
    image.pixels()[(y * 640 + x) as usize]
}

#[test]
fn spec014_pipboy_opens_on_its_art_and_closes() {
    let dir = corpus();
    let mut preview = load(dir.path());
    let mut now = 1000;
    preview.tick(now).unwrap();
    gvar(&mut preview, 10, 150);
    preview.host.world.rest_credit = 100;
    assert!(preview.step(KeyCode::KeyP));
    assert_eq!(preview.host.world.rest_credit, 0, "opening resets credit");
    assert_eq!(pipboy(&preview).tab(), None, "the start screen");
    assert!(!preview.palette_cycling());
    // pip.frm under everything, the logo, the note and its ones digit, the
    // date and month cells and the alarm button.
    assert_eq!(pixel(&mut preview, [600, 470]), 3);
    assert_eq!(pixel(&mut preview, [323, 156]), 38);
    assert_eq!(pixel(&mut preview, [32, 83]), 33);
    assert_eq!(pixel(&mut preview, [92, 180]), 35);
    assert_eq!(pixel(&mut preview, [20, 17]), 32);
    assert_eq!(pixel(&mut preview, [46, 18]), 34);
    assert_eq!(pixel(&mut preview, [124, 13]), 36);
    // The water chip found: the note is gone.
    gvar(&mut preview, 101, 2);
    assert_eq!(pixel(&mut preview, [32, 83]), 3);
    // A held page button shows its down art while the pointer is over it.
    preview.hover([60, 345]);
    assert!(preview.click([60, 345], false));
    assert_eq!(pixel(&mut preview, [60, 345]), 31);
    assert!(preview.release(Some([60, 345]), false));
    assert_eq!(pipboy(&preview).tab(), Some(Tab::Status));
    assert_eq!(pixel(&mut preview, [60, 345]), 30);
    // Unhandled keys do nothing; `p` closes.
    assert!(preview.step(KeyCode::ArrowLeft));
    assert!(preview.step(KeyCode::Digit1));
    assert!(matches!(preview.screen, Screen::PipBoy(_)));
    assert!(preview.step(KeyCode::KeyP));
    assert!(matches!(preview.screen, Screen::Map));
    // The Close button and Enter close too.
    assert!(preview.step(KeyCode::KeyP));
    assert!(preview.tap([60, 455]));
    assert!(matches!(preview.screen, Screen::Map));
    assert!(preview.step(KeyCode::KeyP));
    assert!(preview.step(KeyCode::Enter));
    assert!(matches!(preview.screen, Screen::Map));
    // F10 asks to quit; the Pip-Boy closes whatever the answer.
    assert!(preview.step(KeyCode::KeyP));
    assert!(preview.step(KeyCode::F10));
    assert!(preview.dialog.is_some());
    assert!(preview.step(KeyCode::Escape), "No");
    now += 50;
    preview.tick(now).unwrap();
    assert!(matches!(preview.screen, Screen::Map));
    assert!(!preview.quit_requested);
}

#[test]
fn spec014_pipboy_stays_shut_without_its_art() {
    let dir = corpus();
    std::fs::remove_file(dir.path().join("art/intrface/bomb1.frm")).unwrap();
    let mut preview = load(dir.path());
    assert!(preview.step(KeyCode::KeyP));
    assert!(matches!(preview.screen, Screen::Map));
    assert!(preview.dialog.is_none(), "no message");
}

#[test]
fn spec014_fixture_j_status_details_and_holodisk_pages() {
    let dir = corpus();
    // FEV Experiment Tape: 94 records, three pages.
    let mut text = String::from(
        "{201}{}{BACK}\n{202}{}{STATUS}\n{210}{}{Status}\n{211}{}{DATA}\n{212}{}{of}\n\
         {200}{}{MORE}\n{214}{}{DONE}\n{400}{}{FEV Experiment Tape}\n{700}{}{Vault 13}\n\
         {704}{}{Find the Water Chip.}\n",
    );
    for record in 0..94 {
        text += &format!("{{{}}}{{}}{{Record {record}}}\n", 1000 + record);
    }
    text += "{1094}{}{**END-DISK**}\n";
    std::fs::write(dir.path().join("text/english/game/pipboy.msg"), text).unwrap();
    let mut preview = load(dir.path());
    let mut now = 1000;
    preview.tick(now).unwrap();
    assert!(preview.step(KeyCode::KeyP));
    assert!(preview.tap([60, 345]), "Status");
    // Only the water-chip quest: Vault 13 on row 1; row 2 is below the
    // last location and ignored under the safe rule.
    assert!(preview.tap([300, 90]));
    assert!(pipboy(&preview).flash.is_none());
    assert!(preview.tap([300, 70]));
    assert!(pipboy(&preview).flash.is_some(), "Vault 13 shows yellow");
    preview.image().unwrap();
    // Input waits for the flash.
    assert!(preview.tap([300, 70]));
    settle(&mut preview, &mut now);
    assert!(matches!(
        pipboy(&preview).page,
        Some(Page::Status(StatusView::Detail(0)))
    ));
    preview.image().unwrap();
    // Completed: drawn struck through.
    gvar(&mut preview, 101, 2);
    preview.image().unwrap();
    // BACK on line 40 returns to the list.
    assert!(preview.tap([413, 450]));
    settle(&mut preview, &mut now);
    assert!(matches!(
        pipboy(&preview).page,
        Some(Page::Status(StatusView::List))
    ));
    // Holodisk 0 (global 162): the right column's row 1.
    gvar(&mut preview, 162, 1);
    preview.image().unwrap();
    assert!(preview.tap([500, 70]));
    settle(&mut preview, &mut now);
    let page = |preview: &MapPreview| {
        pipboy(preview)
            .disk()
            .map(|(disk, page)| (disk.records.len(), disk.last_page, page))
    };
    assert_eq!(page(&preview), Some((94, 2, 0)));
    preview.image().unwrap();
    // MORE twice, then on the last page PageDown and the gap do nothing
    // and x <= 395 goes back.
    for expected in [1, 2] {
        assert!(preview.tap([500, 450]));
        settle(&mut preview, &mut now);
        assert_eq!(page(&preview), Some((94, 2, expected)));
    }
    preview.image().unwrap();
    assert!(preview.step(KeyCode::PageDown));
    assert!(preview.tap([420, 450]));
    assert!(pipboy(&preview).flash.is_none());
    assert!(preview.tap([380, 450]));
    settle(&mut preview, &mut now);
    assert_eq!(page(&preview), Some((94, 2, 1)));
    assert!(preview.step(KeyCode::PageDown));
    settle(&mut preview, &mut now);
    assert_eq!(page(&preview), Some((94, 2, 2)));
    // DONE on the last page returns to the list.
    assert!(preview.tap([500, 450]));
    settle(&mut preview, &mut now);
    assert!(matches!(
        pipboy(&preview).page,
        Some(Page::Status(StatusView::List))
    ));
    // PageUp from page 0 also returns; on the list page keys do nothing.
    assert!(preview.tap([500, 70]));
    settle(&mut preview, &mut now);
    assert!(preview.step(KeyCode::PageUp));
    settle(&mut preview, &mut now);
    assert!(pipboy(&preview).disk().is_none());
    assert!(preview.step(KeyCode::PageDown));
    assert!(pipboy(&preview).flash.is_none());
}

#[test]
fn spec014_fixture_j_alarm_clock_rest_and_refusal() {
    let dir = corpus();
    let mut preview = load(dir.path());
    let mut now = 1000;
    preview.tick(now).unwrap();
    assert!(preview.step(KeyCode::KeyP));
    assert!(preview.tap([130, 20]), "the Alarm Clock button");
    assert_eq!(pipboy(&preview).tab(), Some(Tab::Alarm));
    preview.image().unwrap();
    // Option 0 (row 4, y = 96): ten minutes in 50 ms steps.
    let start = preview.host.world.clock.ticks;
    assert!(preview.tap([300, 100]));
    assert!(pipboy(&preview).rest.is_some());
    assert!(preview.pipboy_watch().is_some(), "the watch cursor");
    preview.image().unwrap();
    // Other input is discarded while resting.
    assert!(preview.tap([60, 345]));
    assert!(preview.step(KeyCode::KeyP));
    while pipboy(&preview).rest.is_some() {
        now += 50;
        preview.tick(now).unwrap();
        assert!(now < 100_000, "the rest finishes");
    }
    assert_eq!(preview.host.world.clock.ticks - start, 6000);
    assert_eq!(pipboy(&preview).tab(), Some(Tab::Alarm));
    // Esc stops a rest and the Pip-Boy stays open.
    assert!(preview.tap([300, 140]), "option 2, an hour");
    for _ in 0..3 {
        now += 50;
        preview.tick(now).unwrap();
    }
    assert!(preview.step(KeyCode::Escape));
    assert!(pipboy(&preview).rest.is_none());
    assert!(matches!(preview.screen, Screen::PipBoy(_)));
    let rested = preview.host.world.clock.ticks - start - 6000;
    assert!(rested > 0 && rested < 36000);
    // Refused (an enemy who was attacked by the player): the box, and the
    // previous page stays current under the safe rule.
    assert!(preview.tap([60, 345]));
    let player = preview.host.world.player;
    let npc = npc(&preview);
    preview.host.world.get_mut(npc).unwrap().opponent = player;
    assert!(!preview.host.rest_allowed());
    assert!(preview.tap([130, 20]));
    assert!(preview.dialog.is_some());
    assert_eq!(pipboy(&preview).tab(), Some(Tab::Status));
    assert!(preview.step(KeyCode::Enter));
    assert!(preview.dialog.is_none());
    // The rest intent (`z`) refused: the start screen and the box.
    assert!(preview.step(KeyCode::Escape));
    preview.open_pipboy_on(Tab::Alarm);
    assert_eq!(pipboy(&preview).tab(), None);
    assert!(preview.dialog.is_some());
    assert!(preview.step(KeyCode::Enter));
    assert!(matches!(preview.screen, Screen::PipBoy(_)));
    // Allowed: the Alarm Clock page at once.
    assert!(preview.step(KeyCode::Escape));
    preview.host.world.get_mut(npc).unwrap().opponent = npc;
    preview.open_pipboy_on(Tab::Alarm);
    assert_eq!(pipboy(&preview).tab(), Some(Tab::Alarm));
}

#[test]
fn spec014_fixture_j_automaps_and_archives() {
    let dir = corpus();
    let names = [
        (MAP_SHORT_NAMES[3], "Necropolis"),
        (MAP_SHORT_NAMES[6], "Vault 13"),
        (MAP_SHORT_NAMES[36], "Hub"),
        (136, "Hub"),
        (106, "Vault 13"),
        (308, "Entrance"),
        (218, "Entrance"),
        (219, "Living Quarters"),
        (305, "Cave Entrance"),
        (209, "Sewers"),
    ];
    let mut text = String::from("{7}{}{SYNTH.MAP}\n");
    for (id, name) in names {
        text += &format!("{{{id}}}{{}}{{{name}}}\n");
    }
    std::fs::write(dir.path().join("text/english/game/map.msg"), text).unwrap();
    let mut preview = load(dir.path());
    let mut now = 1000;
    preview.tick(now).unwrap();
    // A wall cell at row 0, column 0 of map 6 elevation 1.
    let mut wall = vec![0; 10000];
    wall[0] = 0b0100_0000;
    let db = &mut preview.host.world.persistence.automap;
    for (map, elevation) in [(6, 0), (35, 0), (36, 0), (3, 0)] {
        assert!(db.store(map, elevation, &[0; 10000]).unwrap());
    }
    assert!(db.store(6, 1, &wall).unwrap());
    assert!(preview.step(KeyCode::KeyP));
    assert!(preview.tap([60, 400]), "Automaps");
    // Sorted by name: Hub, Necropolis, Vault 13.
    assert_eq!(preview.automap_areas(), [36, 3, 6]);
    preview.image().unwrap();
    assert!(preview.tap([300, 70]));
    // The core view keys an area by its index in the sorted area list.
    let area = preview.automap_areas().iter().position(|area| *area == 36);
    assert!(matches!(
        pipboy(&preview).page,
        Some(Page::Automaps(AutomapView::Elevations {
            area: index,
            entry: 0
        })) if Some(index) == area
    ));
    assert_eq!(preview.automap_entries(36), [(36, 0)], "safe rule");
    preview.image().unwrap();
    // The title line (row -1) returns to the list.
    assert!(preview.tap([300, 50]));
    assert!(matches!(
        pipboy(&preview).page,
        Some(Page::Automaps(AutomapView::List))
    ));
    // Vault 13: its two elevations, then the cave entrance (safe rule).
    assert!(preview.tap([300, 110]));
    assert_eq!(preview.automap_entries(6), [(6, 0), (6, 1), (35, 0)]);
    // The description line (row 0) does nothing; row 2 selects elevation 1.
    assert!(preview.tap([300, 70]));
    assert!(preview.tap([300, 110]));
    // The core view keys an area by its index in the sorted area list.
    let area = preview.automap_areas().iter().position(|area| *area == 6);
    assert!(matches!(
        pipboy(&preview).page,
        Some(Page::Automaps(AutomapView::Elevations {
            area: index,
            entry: 1
        })) if Some(index) == area
    ));
    // Its wall cell paints (238, 105) and (239, 105) but not row 106.
    let color = automap::BRIGHT_GREEN.resolve(&preview.palette);
    let image = preview.image().unwrap();
    let at = |x: i32, y: i32| image.pixels()[(y * 640 + x) as usize];
    assert_eq!([at(238, 105), at(239, 105)], [color, color]);
    assert_eq!(at(238, 106), 3);
    // Back: the list; leaving the page takes the raster with it.
    assert!(preview.tap([413, 450]));
    assert!(matches!(
        pipboy(&preview).page,
        Some(Page::Automaps(AutomapView::List))
    ));
    assert_eq!(pixel(&mut preview, [238, 105]), 3);
    // Archives: indexes 3 and 5 played list two rows.
    preview.host.world.persistence.movies[3] = 1;
    preview.host.world.persistence.movies[5] = 1;
    assert!(preview.tap([60, 430]), "Archives");
    assert_eq!(pipboy(&preview).tab(), Some(Tab::Archives));
    assert_eq!(preview.zone_layout(), (2, 2, false));
    preview.image().unwrap();
    now += 50;
    preview.tick(now).unwrap();
}

#[test]
#[ignore = "needs the local corpus"]
fn spec014_fixture_j_corpus_art_sizes_and_font_widths() {
    use vault_core::{
        assets::{VfsWorldAssets, WorldAssets},
        ui::pipboy,
    };
    let Some(vfs) = local_corpus() else {
        return;
    };
    let assets = VfsWorldAssets::new(vfs.clone(), "english").unwrap();
    for (name, size) in [
        ("pip.frm", [640, 480]),
        ("lilredup.frm", [15, 16]),
        ("lilreddn.frm", [15, 16]),
        ("numbers.frm", [360, 17]),
        ("pip2.frm", [148, 227]),
        ("months.frm", [29, 179]),
        ("notenums.frm", [120, 13]),
        ("alarmout.frm", [28, 24]),
        ("alarmin.frm", [28, 24]),
        ("pipx.frm", [204, 141]),
        ("bomb1.frm", [65, 65]),
    ] {
        assert!(pipboy::ART.contains(&name));
        let art = assets.frame(&format!("art/intrface/{name}")).unwrap();
        let frame = art.frame(0, 0).unwrap();
        assert_eq!([frame.width, frame.height], size, "{name}");
    }
    assert!(assets.frame("art/intrface/pipboy.frm").is_err());
    let font = assets.font_number(101).unwrap();
    let messages = assets.game_messages("pipboy.msg").unwrap();
    let width = |text: &[u8]| font.text_width(text, false);
    for (id, expected) in [
        (202, 47),
        (211, 33),
        (201, 32),
        (200, 31),
        (214, 31),
        (205, 65),
        (206, 99),
        (300, 83),
        (212, 13),
        (400, 127),
        (700, 50),
        (107, 101),
        (215, 209),
    ] {
        let text = &messages.get(id).unwrap().text;
        assert_eq!(width(text), expected, "pipboy.msg {id}");
    }
    assert_eq!(width(b"Hit Points 25/30"), 103);
    assert_eq!(width(b"Hub"), 22);
    // "1. Find the Water Chip." strikes from x = 262 through 396.
    let quest = [b"1. ".as_slice(), &messages.get(704).unwrap().text].concat();
    assert_eq!(width(&quest), 135);
    // The widest shipped quest fits one line.
    let advance = |code: u8| {
        font.character_width(code)
            .map_or(0, |width| (width + font.spacing()) as i32)
    };
    let widest = [b"9. ".as_slice(), &messages.get(751).unwrap().text].concat();
    assert_eq!(width(&widest), 301);
    assert_eq!(pipboy::wrap(&widest, 350, advance).unwrap().len(), 1);
    // The Pip-Boy opens over a real map and draws every page.
    let mut preview = MapPreview::load(vfs, "maps/v13ent.map", "english", None, 1).unwrap();
    preview.image().unwrap();
    preview.tick(1000).unwrap();
    assert!(preview.step(KeyCode::KeyP));
    assert!(matches!(preview.screen, Screen::PipBoy(_)));
    preview.host.world.gvars.as_mut().unwrap()[162] = 1;
    preview.host.world.persistence.movies[5] = 1;
    preview.image().unwrap();
    for button in [[60, 345], [60, 400], [60, 430], [130, 20]] {
        assert!(preview.tap(button));
        preview.image().unwrap();
        // Resting may be refused here: the box closes.
        preview.dialog = None;
    }
    // The tape's reading view, then Vault 13's status.
    assert!(preview.tap([60, 345]));
    assert!(preview.tap([500, 70]));
    preview.tick(1300).unwrap();
    assert_eq!(
        pipboy(&preview).disk().map(|(disk, _)| disk.page_count()),
        Some(3)
    );
    preview.image().unwrap();
    assert!(preview.tap([380, 450]));
    preview.tick(1600).unwrap();
    assert!(preview.tap([300, 70]));
    preview.tick(1900).unwrap();
    assert!(matches!(
        pipboy(&preview).page,
        Some(Page::Status(StatusView::Detail(0)))
    ));
    preview.image().unwrap();
}

#[test]
fn spec014_pipboy_screen_saver_runs_after_two_idle_minutes() {
    let dir = corpus();
    let mut preview = load(dir.path());
    let mut now = 1000;
    preview.tick(now).unwrap();
    preview.hover([300, 300]);
    // The idle timer starts at the opening.
    assert!(preview.step(KeyCode::KeyP));
    now += 120_000;
    preview.tick(now).unwrap();
    assert!(!pipboy(&preview).saver_running(), "120 s is not more");
    now += 1;
    preview.tick(now).unwrap();
    assert!(pipboy(&preview).saver_running());
    assert!(pipboy(&preview).saver.as_ref().unwrap().state.warm_up == 0);
    // The screen area is bare pip.frm plus bombs; the logo is gone.
    assert_ne!(pixel(&mut preview, [330, 160]), 38);
    for _ in 0..40 {
        now += 50;
        preview.tick(now).unwrap();
    }
    // A key ends it and is consumed: Esc does not close the Pip-Boy.
    assert!(preview.step(KeyCode::Escape));
    assert!(matches!(preview.screen, Screen::PipBoy(_)));
    assert!(!pipboy(&preview).saver_running());
    assert_eq!(pixel(&mut preview, [330, 160]), 38);
    // Pointer movement ends it too.
    now += 120_001;
    preview.tick(now).unwrap();
    assert!(pipboy(&preview).saver_running());
    preview.hover([301, 300]);
    now += 50;
    preview.tick(now).unwrap();
    assert!(!pipboy(&preview).saver_running());
}
