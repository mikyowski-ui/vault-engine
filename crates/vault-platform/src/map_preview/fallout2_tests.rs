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

/// A conversation with the synthetic map's scripted critter, made a
/// Fallout 2 party member with a control record whose allowed
/// dispositions are custom, defensive and aggressive and distances
/// stay_close, on_your_own and charge (§4.10's member).
fn party_talk() -> (tempfile::TempDir, MapPreview, vault_core::ObjectRef) {
    party_talk_with(&[])
}
/// [`party_talk`] with interface `art` written first and listed in
/// `intrface.lst`.
fn party_talk_with(
    art: &[(&str, u16, u16, u8)],
) -> (tempfile::TempDir, MapPreview, vault_core::ObjectRef) {
    let dir = corpus();
    write_art(dir.path(), art);
    let list = dir.path().join("art/intrface/intrface.lst");
    let mut rows = std::fs::read(&list).unwrap();
    for (name, ..) in art {
        rows.extend(format!("{name}\n").bytes());
    }
    std::fs::write(&list, rows).unwrap();
    std::fs::create_dir_all(dir.path().join("text/english/dialog")).unwrap();
    std::fs::write(
        dir.path().join("text/english/dialog/synthetic.msg"),
        b"{980}{}{Hng.}\n",
    )
    .unwrap();
    let mut preview = world(dir.path());
    preview.image().unwrap();
    preview.hud.mode = Mode::Arrow;
    assert!(preview.tap([336, 170]));
    assert!(preview.host.world.dialogue.open && preview.host.world.dialogue.waiting);
    let member = preview.host.world.dialogue.target;
    let pid = preview.host.world.get(member).unwrap().raw.pid;
    let party = format!(
        "[Party Member 0]\nparty_member_pid={pid}\ndisposition=custom, defensive, aggressive\n\
         distance=stay_close, on_your_own, charge\n"
    );
    let mut files: Vec<(&str, &str)> = vault_fixtures::fallout2::tables()
        .into_iter()
        .filter(|(path, _)| *path != "data/party.txt")
        .collect();
    let party: &'static str = Box::leak(party.into_boxed_str());
    files.push(("data/party.txt", party));
    let tables =
        Arc::new(Fallout2Tables::load(&mut vault_fixtures::fallout2::reader(files)).unwrap());
    preview.host.world.fallout2 = Some(Box::new(Fallout2World::new_game(tables, -1)));
    preview.host.world.get_mut(member).unwrap().raw.id = 18000;
    preview.host.world.party.insert(member);
    let record = preview
        .host
        .world
        .fallout2
        .as_deref()
        .unwrap()
        .joining_record(pid);
    assert_eq!(record.record, 0);
    preview
        .host
        .world
        .fallout2
        .as_deref_mut()
        .unwrap()
        .control
        .insert(18000, record);
    (dir, preview, member)
}

/// SPEC-019 §3.15.3 (Revision 2): the custom panel is a window the size of
/// its background, with window-relative rows and Done: a synthetic
/// 640-by-190 `custom.frm` sits at (0, 290), a 300-by-190 one where the
/// stand-in does, at (170, 145).
#[test]
fn spec019_revision2_the_custom_panel_takes_its_art_size() {
    use super::super::talk::TalkWindow;
    use vault_core::{
        fallout2_party::distance,
        ui::{fallout2 as ui2, talk},
    };
    use vault_formats::fallout2::party::ControlField;
    for ([width, height], origin) in [([640, 190], [0, 290]), ([300, 190], [170, 145])] {
        let (_dir, mut preview, member) = party_talk_with(&[("custom.frm", width, height, 91)]);
        preview.image().unwrap();
        let rect = talk::panel_button(ui2::COMBAT_CONTROL_BUTTON);
        assert!(preview.tap([rect.x + 7, rect.y + 7]));
        // Defensive: Distance is stay_close, and a row press makes it charge.
        assert!(preview.step(KeyCode::Digit3));
        let [ox, oy] = ui2::control::ORIGIN;
        let [cx, cy] = ui2::control::CUSTOM_BUTTON;
        assert!(preview.tap([ox + cx + 5, oy + cy + 5]));
        assert!(matches!(
            preview.talk,
            Some(TalkWindow::Control(ref panel)) if panel.window.custom
        ));
        let window = preview.custom_window();
        assert_eq!(
            [window.x, window.y, window.w, window.h],
            [origin[0], origin[1], i32::from(width), i32::from(height)]
        );
        // The art fills the window from its origin; the row's button is at
        // its window-relative place.
        let image = preview.image().unwrap();
        assert_eq!(pixel(&image, origin), 91);
        assert_eq!(
            pixel(&image, [origin[0] + i32::from(width) - 1, origin[1] + 2]),
            91
        );
        let [rx, ry] = ui2::custom::button(3);
        assert!(preview.tap([origin[0] + rx + 5, origin[1] + ry + 5]));
        assert_eq!(
            preview.host.control_of(member).unwrap().fields[ControlField::Distance as usize],
            distance::CHARGE,
            "{width} by {height}"
        );
        let [dx, dy] = ui2::custom::DONE_BUTTON;
        assert!(preview.tap([origin[0] + dx + 5, origin[1] + dy + 5]));
        assert!(matches!(
            preview.talk,
            Some(TalkWindow::Control(ref panel)) if !panel.window.custom
        ));
    }
    // Without the art, the stand-in's window.
    let (_dir, preview, _) = party_talk();
    let window = preview.custom_window();
    assert_eq!(
        [window.x, window.y, window.w, window.h],
        [170, 145, 300, 190]
    );
}

#[test]
fn spec019_vector_j_dialogue_panel_and_combat_control() {
    use super::super::talk::TalkWindow;
    use vault_core::{
        fallout2_party::{disposition, distance},
        ui::{fallout2 as ui2, talk},
    };
    use vault_formats::fallout2::party::ControlField;
    let (_dir, mut preview, member) = party_talk();
    preview.image().unwrap();
    // §3.8: `a` does nothing; the Combat Control button (Tell Me About's
    // place) opens the control panel.
    assert!(!preview.step(KeyCode::KeyA) || preview.talk.is_none());
    assert!(preview.talk.is_none());
    let rect = talk::panel_button(ui2::COMBAT_CONTROL_BUTTON);
    assert!(preview.tap([rect.x + 7, rect.y + 7]));
    assert!(matches!(preview.talk, Some(TalkWindow::Control(_))));
    preview.image().unwrap();
    let field = |preview: &MapPreview, field: ControlField| {
        preview.host.control_of(member).unwrap().fields[field as usize]
    };
    // Row "Defensive" (key 3): the preset.
    assert!(preview.step(KeyCode::Digit3));
    assert_eq!(
        field(&preview, ControlField::Disposition),
        disposition::DEFENSIVE
    );
    assert_eq!(
        field(&preview, ControlField::Distance),
        distance::STAY_CLOSE
    );
    // Key 2 (Coward) is disabled: nothing changes.
    assert!(preview.step(KeyCode::Digit2));
    assert_eq!(
        field(&preview, ControlField::Disposition),
        disposition::DEFENSIVE
    );
    // Custom: the Distance row cycles stay_close, charge, on_your_own.
    let [ox, oy] = ui2::control::ORIGIN;
    let [cx, cy] = ui2::control::CUSTOM_BUTTON;
    assert!(preview.tap([ox + cx + 5, oy + cy + 5]));
    assert!(matches!(
        preview.talk,
        Some(TalkWindow::Control(ref panel)) if panel.window.custom
    ));
    preview.image().unwrap();
    let [px, py] = ui2::custom::ORIGIN;
    let [rx, ry] = ui2::custom::button(3);
    let distance_row = [px + rx + 5, py + ry + 5];
    assert!(preview.tap(distance_row));
    assert_eq!(field(&preview, ControlField::Distance), distance::CHARGE);
    assert_eq!(
        field(&preview, ControlField::Disposition),
        disposition::CUSTOM
    );
    assert!(preview.tap(distance_row));
    assert_eq!(
        field(&preview, ControlField::Distance),
        distance::ON_YOUR_OWN
    );
    assert!(preview.tap(distance_row));
    assert_eq!(
        field(&preview, ControlField::Distance),
        distance::STAY_CLOSE
    );
    // Enter closes the custom panel; Use Best Weapon with nothing to wield
    // plays the error sound; `t` returns to dialogue.
    assert!(preview.step(KeyCode::Enter));
    assert!(matches!(
        preview.talk,
        Some(TalkWindow::Control(ref panel)) if !panel.window.custom
    ));
    assert!(preview.step(KeyCode::KeyU));
    assert!(preview.step(KeyCode::KeyT));
    assert!(preview.talk.is_none() && preview.host.world.dialogue.open);
    // `c` opens it too.
    assert!(preview.step(KeyCode::KeyC));
    assert!(matches!(preview.talk, Some(TalkWindow::Control(_))));
    assert!(preview.step(KeyCode::Escape));
    assert!(preview.talk.is_none());
    // §3.7: Barter opens without the prototype's barter flag (a trade),
    // and Offer moves both tables at once.
    assert!(preview.step(KeyCode::KeyB));
    preview.image().unwrap();
    let Screen::Barter(screen) = &preview.screen else {
        panic!("the trade window");
    };
    let session = screen.session.clone();
    let item = widget(&mut preview, member);
    preview
        .host
        .barter_move(member, session.table, item, 1)
        .unwrap();
    assert_eq!(
        preview.host.attempt_trade(&session).unwrap(),
        vault_core::barter::Trade::Accepted
    );
    let player = preview.host.world.player;
    assert!(
        preview
            .host
            .world
            .get(player)
            .unwrap()
            .inventory
            .iter()
            .any(|(entry, _)| *entry == item)
    );
}
