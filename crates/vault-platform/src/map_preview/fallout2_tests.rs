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
