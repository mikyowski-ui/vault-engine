//! SPEC-019's interface in the interactive world under the Fallout 2
//! profile, over the synthetic corpus with synthetic art (there is no
//! Fallout 2 corpus here: geometry, state and the drawn images' layout are
//! checked, not the real art).
use super::*;
use vault_core::{
    fallout2::{Fallout2Tables, Fallout2World},
    ui::cursor::Entry,
};

/// The SPEC-018 fixture tables.
fn tables() -> Arc<Fallout2Tables> {
    let mut read = vault_fixtures::fallout2::reader(vault_fixtures::fallout2::tables());
    Arc::new(Fallout2Tables::load(&mut read).unwrap())
}
/// Make `preview` a Fallout 2 game (its map stays the synthetic one).
fn fallout2(preview: &mut MapPreview) {
    preview.host.world.fallout2 = Some(Box::new(Fallout2World::new_game(tables(), -1)));
}
fn pixel(image: &vault_render::IndexedImage, [x, y]: [i32; 2]) -> u8 {
    image.pixels()[(y * image.width() as i32 + x) as usize]
}

#[test]
fn spec019_vector_a_and_b_bar_geometry_and_indicator_boxes() {
    let dir = corpus();
    write_art(
        dir.path(),
        &[("iface.frm", 640, 99, 77), ("warnbox.frm", 130, 21, 78)],
    );
    // Fallout 1 keeps SPEC-014's (0, 379) whatever the art's height.
    let mut vanilla = world(dir.path());
    let image = vanilla.image().unwrap();
    assert_eq!(pixel(&image, [5, 379]), 77);
    assert_eq!(vanilla.map_bottom_row(), 379);
    // Height 99: the bar at (0, 380); map clicks through y = 380.
    let mut preview = world(dir.path());
    fallout2(&mut preview);
    let geometry = preview.bar_geometry().unwrap();
    assert_eq!((geometry.height, geometry.origin()), (99, [0, 380]));
    assert_eq!(preview.map_bottom_row(), 380);
    assert!(preview.map_area([300, 380]) && !preview.map_area([300, 381]));
    // Sneak and poison lit: SNEAK then POISONED at y = 359, the engine
    // strings standing in for the missing labels.
    {
        let sheet = preview.host.world.player_sheet_mut().unwrap();
        sheet.flags |= 1;
        sheet.poison = 3;
    }
    let image = preview.image().unwrap();
    assert_eq!(pixel(&image, [5, 380]), 77);
    assert_ne!(pixel(&image, [5, 379]), 77);
    assert_eq!(pixel(&image, [0, 359]), 78, "box 0, 127 wide");
    assert_eq!(pixel(&image, [124, 359]), 78, "box 1 at 130 - 6");
    assert_ne!(pixel(&image, [256, 359]), 78, "no third box");
    // Height 50 is taken as 100.
    write_art(dir.path(), &[("iface.frm", 640, 50, 77)]);
    let mut short = world(dir.path());
    fallout2(&mut short);
    let geometry = short.bar_geometry().unwrap();
    assert_eq!((geometry.height, geometry.clamped), (100, true));
}

#[test]
fn spec019_the_action_menu_push_and_no_enhanced_companion_entries() {
    use crate::enhanced::{EnhancedSettings, ViewportMode};
    let dir = corpus();
    let open = |preview: &mut MapPreview, target, now: u64| {
        let hex = preview.host.world.get(target).unwrap().raw.hex.unwrap();
        let point = on_hex(preview, hex);
        preview.hud.mode = Mode::Arrow;
        preview.hover(point);
        assert!(preview.click(point, false));
        preview.tick(now + 300).unwrap();
        (
            point,
            preview
                .pointer
                .menu
                .as_ref()
                .expect("the action menu")
                .entries
                .clone(),
        )
    };
    // Under Fallout 2, even with the enhanced preset: Push right after
    // Talk, and no enhanced Push or Gear.
    let mut preview = world(dir.path());
    preview.host.set_preset(vault_core::EnginePreset::Enhanced);
    preview.set_enhanced(Some(EnhancedSettings {
        viewport: ViewportMode::Fixed,
        ..Default::default()
    }));
    fallout2(&mut preview);
    let member = npc(&preview);
    preview.host.world.get_mut(member).unwrap().raw.id = 18000;
    preview.host.world.party.insert(member);
    assert!(!preview.host.is_companion(member));
    assert!(!preview.host.can_push(member, true));
    preview.tick(16).unwrap();
    preview.image().unwrap();
    let (point, entries) = open(&mut preview, member, 16);
    assert_eq!(
        entries,
        [
            Entry::Talk,
            Entry::Push,
            Entry::Look,
            Entry::Inventory,
            Entry::Skill,
            Entry::Cancel
        ]
    );
    // The push icon pair is missing: the engine-drawn icon stands in.
    preview.image().unwrap();
    // Push: the talk approach, then the step away from the player.
    let before = preview.host.world.get(member).unwrap().raw.hex.unwrap();
    preview.pointer.menu.as_mut().unwrap().tracker.highlight = 1;
    assert!(preview.release(Some(point), false));
    let mut now = 400;
    while (preview.host.moving(member) || preview.host.moving(preview.host.world.player))
        && now < 30_000
    {
        now += 16;
        preview.tick(now).unwrap();
    }
    let player = preview.host.world.player;
    let player = preview.host.world.get(player).unwrap().raw.hex.unwrap();
    let after = preview.host.world.get(member).unwrap().raw.hex.unwrap();
    assert_ne!(after, before);
    // The player walked up for the talk approach; the member then
    // stepped directly away from the player's new hex.
    let player_hex = player;
    let pushed = vault_formats::HexGrid::lattice_distance(player_hex, after).unwrap();
    assert_eq!(pushed, 2, "one step past the adjacent hex");
    // A stranger gets no Push under Fallout 2, `--debug` or not.
    let mut preview = world(dir.path());
    fallout2(&mut preview);
    preview.debug_session = true;
    let stranger = npc(&preview);
    preview.tick(16).unwrap();
    preview.image().unwrap();
    let (_, entries) = open(&mut preview, stranger, 16);
    assert!(!entries.contains(&Entry::Push) && !entries.contains(&Entry::Gear));
}

#[test]
fn spec019_character_karma_and_kills_folders() {
    use vault_core::ui::{
        character::{Folder, Title, info_card},
        editor::InfoLine,
    };
    let dir = corpus();
    let mut files = vault_fixtures::fallout2::tables();
    files.push(("data/genrep.txt", vault_fixtures::fallout2::GENREP));
    files.push((
        "data/karmavar.txt",
        "3, 104, 2000, 2100\n4, -1, 2001, 2101\n",
    ));
    let tables =
        Arc::new(Fallout2Tables::load(&mut vault_fixtures::fallout2::reader(files)).unwrap());
    let mut preview = world(dir.path());
    preview.host.world.fallout2 = Some(Box::new(Fallout2World::new_game(tables, -1)));
    preview.host.world.gvars = Some(vec![0, 0, 0, 1, 0]);
    preview.host.world.player_sheet_mut().unwrap().karma = 10;
    preview.host.world.kills[1] = 2;
    preview.host.world.fallout2.as_deref_mut().unwrap().kills[3] = 1;
    let view = preview.folder_view(Folder::Karma);
    let folders = view.fallout2.clone().unwrap();
    // Karma 10: the greatest threshold not above it is 0 (first in file
    // order, title 1002); karma variable 0's global (3) is set, 1's not.
    assert_eq!(folders.karma, 10);
    assert_eq!(folders.reputation, Some(1002));
    assert_eq!(folders.variables, [(104, 2000, 2100)]);
    // Kill types 1 and 18 (Big Bad Boss, by the English fallback), by name.
    assert_eq!(folders.kills, [(18, 1), (1, 2)]);
    assert_eq!(preview.kill_name(18), b"Big Bad Boss");
    // The cards: row 1 the title, row 2 the variable with its interface
    // row; the kills from `proto.msg` 1450 + t and 1469 + t.
    let reputation = info_card(InfoLine::Folder(1), Some(&view));
    assert_eq!(reputation.title, Title::Message("editor.msg", 1002));
    let variable = info_card(InfoLine::Folder(2), Some(&view));
    assert_eq!((variable.picture, variable.interface), (104, true));
    assert_eq!(variable.description, ("editor.msg", 2100));
    let kills = preview.folder_view(Folder::Kills);
    let card = info_card(InfoLine::Folder(0), Some(&kills));
    assert_eq!(card.title, Title::Kill2(18));
    assert_eq!(card.description, ("proto.msg", 1487));
    // Fallout 1 keeps its folders.
    preview.host.world.fallout2 = None;
    assert!(preview.folder_view(Folder::Karma).fallout2.is_none());
}

#[test]
fn spec019_pipboy_status_holodisks_automaps_archives_and_availability() {
    let dir = corpus();
    let game = dir.path().join("text/english/game");
    std::fs::write(
        game.join("pipboy.msg"),
        "{201}{}{BACK}\n{202}{}{STATUS}\n{210}{}{Status}\n{211}{}{DATA}\n{212}{}{of}\n\
         {200}{}{MORE}\n{214}{}{DONE}\n{400}{}{Disk A}\n{1000}{}{Line one}\n\
         {1001}{}{**END-DISK**}\n{502}{}{Elder}\n{505}{}{Destroyed}\n",
    )
    .unwrap();
    std::fs::write(
        game.join("quests.msg"),
        "{100}{}{Kill bugs}\n{101}{}{Find Vic}\n",
    )
    .unwrap();
    let mut map = std::fs::read(game.join("map.msg")).unwrap();
    map.extend_from_slice(b"{1500}{}{Arroyo}\n{1501}{}{Den}\n");
    std::fs::write(game.join("map.msg"), map).unwrap();
    let mut preview = world(dir.path());
    fallout2(&mut preview);
    preview.image().unwrap();
    preview.host.world.gvars = Some(vec![0; 20]);
    let gvars = preview.host.world.gvars.as_mut().unwrap();
    // Quest 0 active, quest 1 completed, quest 2 active at threshold 0;
    // holodisk 0 held, 1 not.
    gvars[7] = 1;
    gvars[8] = 2;
    gvars[12] = 1;
    // §3.10: a map whose record has `pipboy_active=No` (map 2) keeps it
    // shut; map 1 opens it.
    preview.host.world.map.header.map_number = 2;
    assert!(!preview.pipboy_active());
    assert!(preview.step(KeyCode::KeyP));
    assert!(matches!(preview.screen, Screen::Map));
    preview.host.world.map.header.map_number = 1;
    assert!(preview.pipboy_active());
    assert!(preview.step(KeyCode::KeyP));
    assert!(matches!(preview.screen, Screen::PipBoy(_)));
    assert_eq!(
        preview.status_locations(),
        [(0, b"Arroyo".to_vec()), (1, b"Den".to_vec())]
    );
    assert_eq!(preview.status_disks(), [(0, b"Disk A".to_vec())]);
    let (title, quests) = preview.status_detail(0);
    assert_eq!(title, b"Arroyo");
    assert_eq!(
        quests,
        [
            (1, b"Kill bugs".to_vec(), false),
            (2, b"Find Vic".to_vec(), true)
        ]
    );
    // A missing quest text shows SPEC-002's placeholder.
    assert_eq!(preview.status_detail(1).1, [(1, b"Error".to_vec(), false)]);
    let disk = preview.read_disk(0).unwrap();
    assert_eq!(disk.title, b"Disk A");
    assert_eq!(disk.records.len(), 1);
    // The Status page draws; the water note is never drawn.
    assert!(preview.tap([60, 345]), "Status");
    let image = preview.image().unwrap();
    assert_ne!(pixel(&image, [32, 83]), 33);
    // Video Archives: indexes 2 through 15 that were played.
    let state = preview.host.world.fallout2.as_deref_mut().unwrap();
    state.movies[1] = 1;
    state.movies[2] = 1;
    state.movies[5] = 1;
    assert_eq!(preview.archive_list(), [2, 5]);
    // Automaps by city: map 1 (Test Village, area 0) and map 0 (no city).
    preview.host.world.persistence.automap = vault_formats::AutomapDb::fallout2(|_| false);
    let raster = vec![0; 10000];
    let db = &mut preview.host.world.persistence.automap;
    assert!(db.store(1, 0, &raster).unwrap());
    assert!(db.store(1, 2, &raster).unwrap());
    assert!(db.store(0, 1, &raster).unwrap());
    let areas = preview.fallout2_automap_areas();
    assert_eq!(areas, [1000, 0], "Desert Encounter 1 sorts first");
    assert_eq!(preview.automap_name(0), b"Test Village");
    assert_eq!(preview.fallout2_automap_entries(0), [(1, 0), (1, 2)]);
    assert_eq!(preview.automap_entry(1, 2), b"Test Village 3");
    assert_eq!(preview.fallout2_automap_entries(1000), [(0, 1)]);
}
