//! The `enhanced` HUD's party pills and the WIELD buttons of the Gear and
//! party Trade windows, over the synthetic corpus with synthetic items.
use super::super::{
    barter::{BarterScreen, TRADE_WIELD},
    fixture,
    loot::WIELD,
    test_support::{interactive, npc, synthetic_corpus},
};
use super::*;
use crate::enhanced::{EnhancedSettings, ViewportMode};
use std::sync::Arc;
use vault_core::{
    EnginePreset,
    companion::Distance,
    fallout2::{Fallout2Tables, Fallout2World},
    fallout2_party as party,
};
use vault_formats::{HexGrid, Prototype, fallout2::save::MemberRecord};

/// The synthetic world, under the enhanced preset in the fixed viewport
/// (the frame's coordinates) when `enhanced`.
fn preview(dir: &std::path::Path, enhanced: bool) -> MapPreview {
    let mut preview = interactive(dir);
    if enhanced {
        preview.host.set_preset(EnginePreset::Enhanced);
        preview.set_enhanced(Some(EnhancedSettings {
            viewport: ViewportMode::Fixed,
            ..Default::default()
        }));
    }
    preview.tick(16).unwrap();
    preview
}
/// The synthetic NPC as a companion (party ID 18000) beside the player,
/// carrying a knife (code 1) and a spear (code 4).
fn join(preview: &mut MapPreview) -> (ObjectRef, [ObjectRef; 2]) {
    let member = npc(preview);
    let world = &mut preview.host.world;
    world.get_mut(member).unwrap().raw.id = 18000;
    world.party.insert(member);
    let hex = world.get(world.player).unwrap().raw.hex.unwrap();
    let elevation = world.get(world.player).unwrap().raw.elevation;
    world
        .move_object(member, HexGrid::step(hex, 1, 1).unwrap(), elevation, false)
        .unwrap();
    let weapons = [(0x10, 1, "Knife"), (0x11, 4, "Spear")].map(|(pid, code, name)| {
        let mut bytes = fixture::prototype(0, 3);
        fixture::word(&mut bytes, 0, pid);
        fixture::word(&mut bytes, 57, code);
        let prototype = Arc::new(Prototype::parse(&bytes).unwrap());
        let item = world.spawn(prototype, 0, 0).unwrap();
        world.get_mut(item).unwrap().name = name.as_bytes().to_vec();
        world.add_item(member, item, 1, true).unwrap();
        item
    });
    (member, weapons)
}
fn center(rect: Rect) -> [i32; 2] {
    [rect.x + rect.w / 2, rect.y + rect.h / 2]
}
fn pixel(image: &IndexedImage, [x, y]: [i32; 2]) -> u8 {
    image.pixels()[(y * image.width() as i32 + x) as usize]
}
fn amber(preview: &MapPreview) -> u8 {
    ColorKey::AMBER.resolve(&preview.palette)
}

#[test]
fn the_pills_are_laid_out_in_a_fixed_stack() {
    assert_eq!(pill(0), Rect::new(4, 4, 152, 16));
    assert_eq!(pill(5), Rect::new(4, 94, 152, 16));
    assert_eq!(
        regions(1),
        [Rect::new(4, 22, 118, 16), Rect::new(122, 22, 34, 16)]
    );
    // Above the indicator boxes (y 358) and left of the debug panel (x 352).
    let last = pill(MAX_PILLS - 1);
    assert!(last.y + last.h < 358 && last.x + last.w < 352);
}

#[test]
fn party_pills_show_and_route_under_fallout1_and_never_under_vanilla() {
    let dir = synthetic_corpus();
    // Vanilla: no pill, and the frame is unchanged by a companion.
    let mut vanilla = preview(dir.path(), false);
    let (member, _) = join(&mut vanilla);
    assert!(vanilla.party_pills().is_empty());
    assert_eq!(vanilla.party_pill_at(center(pill(0))), None);
    let with = vanilla.image().unwrap();
    vanilla.host.world.party.remove(&member);
    assert_eq!(vanilla.image().unwrap().pixels(), with.pixels());
    // Enhanced without a companion: nothing either.
    let mut preview = preview(dir.path(), true);
    let (member, [knife, _]) = join(&mut preview);
    preview.host.world.party.remove(&member);
    let plain = preview.image().unwrap();
    assert!(preview.party_pills().is_empty());
    assert!(!preview.party_pill_click(center(pill(0))));
    // A companion: its pill, drawn in amber.
    preview.host.world.party.insert(member);
    let pills = preview.party_pills();
    assert_eq!(pills.len(), 1);
    assert_eq!(pills[0].critter, member);
    assert_eq!(pills[0].mode, Some("DEF"));
    let name = preview.host.world.get(member).unwrap().name.clone();
    assert_eq!(pills[0].name, name);
    let drawn = preview.image().unwrap();
    assert_ne!(drawn.pixels(), plain.pixels());
    assert_eq!(pixel(&drawn, [5, 4]), amber(&preview));
    assert_eq!(pixel(&drawn, [130, 12]), amber(&preview), "the mode region");
    // The mode region cycles the enhanced Distance.
    let [window, mode] = regions(0);
    assert!(preview.tap(center(mode)));
    assert_eq!(preview.host.companion_distance(member), Distance::Close);
    assert_eq!(preview.party_pills()[0].mode, Some("CLS"));
    assert!(matches!(preview.screen, Screen::Map));
    // The name/HP region opens the Gear window.
    assert!(preview.tap(center(window)));
    let Screen::Loot(screen) = &preview.screen else {
        panic!("the Gear window");
    };
    assert!(screen.session.companion && screen.session.root == member);
    // The window hides the pills; WIELD takes the knife in hand.
    let [ox, oy] = vault_core::skills::LOOT_ORIGIN;
    let button = [ox + WIELD.x + 10, oy + WIELD.y + 10];
    preview.image().unwrap();
    assert!(preview.tap(button));
    assert_eq!(preview.host.companion_weapon(member), Some(knife));
    let Screen::Loot(screen) = &preview.screen else {
        panic!("the Gear window");
    };
    let pane = preview.host.loot_items(&screen.session, true);
    assert!(pane.iter().all(|(item, _)| *item != knife));
    preview.image().unwrap();
    assert!(preview.step(winit::keyboard::KeyCode::Escape));
    assert!(matches!(preview.screen, Screen::Map));
    // In combat both regions refuse, with the status line.
    preview.host.world.combat = true;
    assert!(preview.party_pill_click(center(window)));
    assert!(matches!(preview.screen, Screen::Map));
    assert_eq!(preview.status, REFUSED);
    preview.status.clear();
    assert!(preview.party_pill_click(center(mode)));
    assert_eq!(preview.status, REFUSED);
    assert_eq!(preview.host.companion_distance(member), Distance::Close);
}

#[test]
fn an_ordinary_loot_window_has_no_wield_button() {
    let dir = synthetic_corpus();
    let mut preview = preview(dir.path(), true);
    let (member, _) = join(&mut preview);
    preview.host.world.party.remove(&member);
    preview.host.world.pending_loot = Some(vault_core::LootRequest {
        looter: preview.host.world.player,
        target: member,
        steal: false,
    });
    preview.image().unwrap();
    let Screen::Loot(screen) = &preview.screen else {
        panic!("the loot window");
    };
    assert!(!screen.session.companion);
    let [ox, oy] = vault_core::skills::LOOT_ORIGIN;
    preview.tap([ox + WIELD.x + 10, oy + WIELD.y + 10]);
    assert_eq!(preview.host.companion_weapon(member), None);
}

/// Make `preview` a Fallout 2 game whose party record 0 allows
/// `stay_close`, `charge` and `stay`, with the member's control record
/// (disposition `aggressive`).
fn fallout2(preview: &mut MapPreview) {
    let mut files = vault_fixtures::fallout2::tables();
    for file in &mut files {
        if file.0 == "data/party.txt" {
            file.1 = "[Party Member 0]\nparty_member_pid=16777300\n\
                distance=stay_close, charge, stay\n";
        }
    }
    let mut read = vault_fixtures::fallout2::reader(files);
    let tables = Arc::new(Fallout2Tables::load(&mut read).unwrap());
    let mut state = Fallout2World::new_game(tables, -1);
    let mut fields = party::neutral();
    fields[0] = party::disposition::AGGRESSIVE;
    let record = MemberRecord {
        record: 0,
        stage: 0,
        fields,
    };
    state.control.insert(18000, record);
    preview.host.world.fallout2 = Some(Box::new(state));
}

#[test]
fn party_pills_open_the_trade_window_and_cycle_the_control_distance_under_fallout2() {
    let dir = synthetic_corpus();
    let mut preview = preview(dir.path(), true);
    let (member, [knife, _]) = join(&mut preview);
    fallout2(&mut preview);
    preview.tick(32).unwrap();
    assert_eq!(preview.party_pills()[0].mode, Some("DEF"));
    // The mode region: the next allowed value after `on_your_own`, and
    // the disposition becomes `custom`.
    let [window, mode] = regions(0);
    assert!(preview.tap(center(mode)));
    assert_eq!(preview.party_pills()[0].mode, Some("HLD"));
    let record = preview.host.control_of(member).unwrap();
    assert_eq!(record.fields[4], party::distance::STAY);
    assert_eq!(record.fields[0], party::disposition::CUSTOM);
    assert_eq!(preview.host.companion_distance(member), Distance::Default);
    // The name/HP region opens the Trade window without a conversation.
    assert!(preview.tap(center(window)));
    let Screen::Barter(screen) = &preview.screen else {
        panic!("the Trade window");
    };
    assert!(screen.direct && screen.session.merchant == member);
    assert!(!preview.host.world.dialogue.open);
    let image = preview.image().unwrap();
    let plate = [TRADE_WIELD.x + 1, 289 + TRADE_WIELD.y];
    assert_eq!(pixel(&image, plate), amber(&preview));
    // WIELD takes the knife in hand; it stays hidden from the trade.
    let button = center(Rect::new(
        TRADE_WIELD.x,
        289 + TRADE_WIELD.y,
        TRADE_WIELD.w,
        10,
    ));
    assert!(preview.tap(button));
    let Screen::Barter(screen) = &preview.screen else {
        panic!("the Trade window");
    };
    assert_eq!(
        preview.host.barter_held_weapon(&screen.session),
        Some(knife)
    );
    preview.image().unwrap();
    // Leaving returns to the map, the knife in the right hand.
    assert!(preview.step(winit::keyboard::KeyCode::Escape));
    assert!(matches!(preview.screen, Screen::Map));
    assert_eq!(preview.host.companion_weapon(member), Some(knife));
    // Out of reach: the talk exception, and no window.
    let world = &mut preview.host.world;
    let hex = world.get(world.player).unwrap().raw.hex.unwrap();
    let far = HexGrid::step(hex, 1, 12).unwrap();
    world.move_object(member, far, 0, false).unwrap();
    assert!(preview.tap(center(window)));
    assert!(matches!(preview.screen, Screen::Map));
}

#[test]
fn vanilla_fallout2_trade_has_no_wield_button() {
    let dir = synthetic_corpus();
    let mut preview = preview(dir.path(), false);
    let (member, _) = join(&mut preview);
    fallout2(&mut preview);
    // Under Vanilla the pill does not exist and the host refuses the
    // direct Trade window.
    assert!(preview.party_pills().is_empty());
    assert!(!preview.open_party_trade(member));
    // A Trade window (as a conversation opens it) draws no WIELD and
    // takes no press there.
    let session = preview.host.begin_barter(member).unwrap();
    preview.screen = Screen::Barter(Box::new(BarterScreen::new(session, false)));
    let image = preview.image().unwrap();
    let plate = [TRADE_WIELD.x + 1, 289 + TRADE_WIELD.y];
    assert_ne!(pixel(&image, plate), amber(&preview));
    let button = center(Rect::new(
        TRADE_WIELD.x,
        289 + TRADE_WIELD.y,
        TRADE_WIELD.w,
        10,
    ));
    preview.tap(button);
    let Screen::Barter(screen) = &preview.screen else {
        panic!("the Trade window");
    };
    assert_eq!(preview.host.barter_held_weapon(&screen.session), None);
}

#[test]
fn a_pill_takes_its_press_on_the_unlocked_canvas() {
    use crate::enhanced::{PointerAt, PointerButton};
    use vault_render::EnhancedLayout;
    let dir = synthetic_corpus();
    let mut preview = interactive(dir.path());
    preview.host.set_preset(EnginePreset::Enhanced);
    preview.set_enhanced(Some(EnhancedSettings {
        smooth_camera: false,
        ..Default::default()
    }));
    preview.tick(16).unwrap();
    let (member, _) = join(&mut preview);
    let layout = EnhancedLayout::compute([1920, 1080], None, preview.enhanced_zoom()).unwrap();
    preview.enhanced_frame(&layout).unwrap().unwrap();
    // The surface point at the middle of the mode region's frame pixel.
    let [x, y] = center(regions(0)[1]);
    let scale = f64::from(layout.ui_scale);
    let [left, top, ..] = layout.ui_rect.map(f64::from);
    let point = [
        left + (f64::from(x) + 0.5) * scale,
        top + (f64::from(y) + 0.5) * scale,
    ];
    let at = PointerAt {
        ui: layout.ui_pixel(point),
        world: layout.world_point(point),
        side: layout.in_side_panel(point),
    };
    assert_eq!(at.ui, Some([x, y]));
    assert!(preview.enhanced_button(at, PointerButton::Left, true));
    preview.enhanced_button(at, PointerButton::Left, false);
    assert_eq!(preview.host.companion_distance(member), Distance::Close);
    assert!(matches!(preview.screen, Screen::Map));
}
