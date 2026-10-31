//! SPEC-019 §3.12-§3.13 the Fallout 2 world and town map screens over the
//! synthetic corpus, with SPEC-018 tables and synthetic interface art
//! (there is no Fallout 2 corpus here: the layout, geometry and state are
//! checked, not the real art).
use super::{Pending2, World2Screen};
use crate::map_preview::{
    MapPreview, Screen, fixture,
    test_support::{interactive, synthetic_corpus},
    world::TownOrigin,
};
use std::path::Path;
use vault_core::{
    fallout2::{Fallout2World, Selected},
    ui::{ColorKey, fallout2::panel},
    worldmap::screen,
};
use winit::keyboard::KeyCode;

const CITY: &str = "[Area 00]\narea_name=Arroyo\nworld_pos=175,125\nstart_state=On\n\
    size=Large\ntownmap_art_idx=105\ntownmap_label_art_idx=106\n\
    entrance_0=On,100,200,Temple,-1,-1,0\nentrance_1=On,150,220,Test Village,-1,-1,0\n\
    entrance_2=Off,10,10,Test Village,-1,-1,0\n\
    [Area 01]\narea_name=Klamath\nworld_pos=500,400\nstart_state=On\nsize=Medium\n\
    entrance_0=On,10,10,Test Village,-1,-1,0\n\
    [Area 02]\narea_name=Secret\nworld_pos=300,100\nsize=Small\n\
    entrance_0=On,10,10,Test Village,-1,-1,0\n\
    [Area 03]\narea_name=Locked\nworld_pos=300,450\nstart_state=On\nlock_state=On\n\
    entrance_0=On,10,10,Test Village,-1,-1,0\n";
const WORLDMAP: &str = "[Data]\nterrain_types=Desert:1, Mountain:2\n\
    [Tile Data]\nnum_horizontal_tiles=2\n\
    [Tile 0]\nart_idx=104\n[Tile 1]\nart_idx=104\n[Tile 2]\nart_idx=104\n[Tile 3]\nart_idx=104\n\
    [Random Maps: Desert]\nmap_00=Desert Encounter 1\n";

/// The synthetic corpus with SPEC-018's tables (cities above, a 2-by-2
/// tile world of 700 by 600 pixels) and interface art: the world-map frame
/// around its hole, markers, the red buttons, digits and months, and rows
/// 104-109 of `intrface.lst` for the tile, Arroyo's picture and label, the
/// dial and the list arrows' second candidates.
fn corpus() -> tempfile::TempDir {
    let dir = synthetic_corpus();
    let root = dir.path();
    vault_fixtures::fallout2::write_tables(root);
    std::fs::write(root.join("data/city.txt"), CITY).unwrap();
    std::fs::write(root.join("data/worldmap.txt"), WORLDMAP).unwrap();
    let interface = root.join("art/intrface");
    let write = |name: &str, bytes: Vec<u8>| std::fs::write(interface.join(name), bytes).unwrap();
    let mut rows = vec!["shell.frm".to_string(); 104];
    rows[99] = "panel.frm".into();
    for name in [
        "wtile.frm",
        "wtown.frm",
        "wlabel.frm",
        "wmdial.frm",
        "uparwoff.frm",
        "dnarwoff.frm",
    ] {
        rows.push(name.into());
    }
    std::fs::write(interface.join("intrface.lst"), rows.join("\n") + "\n").unwrap();
    write("wtile.frm", fixture::frm(350, 300, 50));
    write("wtown.frm", fixture::frm(453, 444, 84));
    write("wlabel.frm", fixture::frm(60, 18, 80));
    write("wmdial.frm", fixture::frm(20, 20, 81));
    write("uparwoff.frm", fixture::frm(15, 16, 82));
    write("dnarwoff.frm", fixture::frm(15, 16, 83));
    let mut frame = fixture::frm(640, 480, 0);
    for y in 0..480 {
        for x in 0..640 {
            let hole = (22..472).contains(&x) && (21..463).contains(&y);
            frame[74 + y * 640 + x] = if hole { 0 } else { 9 };
        }
    }
    write("wmapbox.frm", frame);
    write("hotspot1.frm", fixture::frm(25, 13, 7));
    write("hotspot2.frm", fixture::frm(25, 13, 8));
    write("wmaploc.frm", fixture::frm(5, 5, 11));
    write("wmaptarg.frm", fixture::frm(11, 11, 12));
    write("wmaptrg2.frm", fixture::frm(11, 11, 13));
    write("wmapfgt0.frm", fixture::frm(7, 11, 14));
    write("wmapfgt1.frm", fixture::frm(7, 11, 15));
    write("lilredup.frm", fixture::frm(15, 16, 16));
    write("lilreddn.frm", fixture::frm(15, 16, 17));
    write("numbers.frm", fixture::frm(360, 17, 1));
    write("months.frm", fixture::frm(29, 179, 1));
    dir
}
/// A Fallout 2 game on `artemple.map` (map 2, in Arroyo).
fn preview(root: &Path) -> MapPreview {
    let mut preview = interactive(root);
    let tables = preview.host.load_fallout2_tables().unwrap();
    let start = tables.maps.match_name(b"artemple.map");
    assert_eq!(start, 2);
    preview.host.world.fallout2 = Some(Box::new(Fallout2World::new_game(tables, start)));
    preview.host.load_map("maps/artemple.map").unwrap();
    preview.image().unwrap();
    preview
}
fn world(preview: &MapPreview) -> &World2Screen {
    match &preview.screen {
        Screen::World2(world) => world,
        _ => panic!("the Fallout 2 world map is not open"),
    }
}
fn state(preview: &mut MapPreview) -> &mut Fallout2World {
    preview.host.world.fallout2.as_deref_mut().unwrap()
}
fn pixel(image: &vault_render::IndexedImage, [x, y]: [i32; 2]) -> u8 {
    image.pixels()[(y * image.width() as i32 + x) as usize]
}
fn click(preview: &mut MapPreview, point: [i32; 2]) {
    preview.click(point, false);
    preview.release(Some(point), false);
}
fn run(preview: &mut MapPreview, from: u64, limit: u64, done: impl Fn(&MapPreview) -> bool) -> u64 {
    let mut now = from;
    for _ in 0..limit {
        now += 41;
        preview.tick(now).unwrap();
        if done(preview) {
            break;
        }
    }
    now
}
fn open(preview: &mut MapPreview) {
    preview.host.request_transition(vault_core::Transition {
        map: -2,
        elevation: -1,
        hex: -1,
        facing: -1,
    });
    assert!(preview.host.process_transition().unwrap());
    preview.image().unwrap();
    assert!(matches!(preview.screen, Screen::World2(_)));
}

#[test]
fn spec019_world_view_tiles_cities_and_side_panel() {
    let dir = corpus();
    let mut preview = preview(dir.path());
    assert_eq!(state(&mut preview).position, [175, 125]);
    open(&mut preview);
    // The view centers on (175, 125), clamped to 0..=250 and 0..=158.
    assert_eq!(
        (world(&preview).view, world(&preview).follow),
        ([0, 0], true)
    );
    let image = preview.image().unwrap();
    let black = ColorKey::BLACK.resolve(&preview.palette);
    let green = ColorKey::GREEN.resolve(&preview.palette);
    let dim = preview.palette.intensity(50, 64);
    // The party's subtile (3, 2) is clear tile art; (2, 2) is dimmed; an
    // unknown subtile is black.
    assert_eq!(pixel(&image, [22 + 160, 21 + 105]), 50);
    assert_eq!(pixel(&image, [22 + 120, 21 + 110]), dim);
    assert_eq!(pixel(&image, [22 + 420, 21 + 10]), black);
    // Arroyo's circle (Large, r = 20) in key 992; unknown Secret has none.
    assert_eq!(pixel(&image, [22 + 175 + 20, 21 + 125]), green);
    assert_eq!(pixel(&image, [22 + 175, 21 + 125 - 20]), green);
    assert_ne!(pixel(&image, [22 + 300 + 7, 21 + 100]), green);
    // The at-rest triangle at the party.
    assert_eq!(pixel(&image, [22 + 175 - 12, 21 + 125 - 6]), 7);
    // The frame; the town list: Arroyo's label art, the red buttons; the
    // dial, the arrows' second candidates and the Town/World button.
    assert_eq!(pixel(&image, [10, 10]), 9);
    assert_eq!(pixel(&image, [508, 138]), 16);
    assert_eq!(pixel(&image, [531, 138]), 80);
    assert_eq!(pixel(&image, panel::DIAL), 81);
    assert_eq!(pixel(&image, [590, 112]), 82);
    assert_eq!(pixel(&image, [590, 330]), 83);
    assert_eq!(pixel(&image, [520, 439]), 16);
    // No gauge while walking.
    assert_eq!(pixel(&image, [493, 300]), 9);
    // The town list holds the three known cities.
    assert_eq!(preview.known_cities(), [0, 1, 3]);
    // Keys: Esc does nothing, End takes vy to 158, arrows scroll and stop
    // at the limits.
    assert!(preview.step(KeyCode::Escape));
    assert!(matches!(preview.screen, Screen::World2(_)));
    assert!(preview.step(KeyCode::End));
    assert_eq!(world(&preview).view, [0, 158]);
    assert!(!world(&preview).follow);
    assert!(preview.step(KeyCode::ArrowRight));
    assert_eq!(world(&preview).view, [16, 158]);
    for _ in 0..30 {
        preview.step(KeyCode::ArrowRight);
    }
    assert_eq!(world(&preview).view, [250, 158]);
    assert_eq!(preview.world2_cursor([639, 240]), "screx.frm");
    assert!(preview.step(KeyCode::KeyC));
    assert_eq!(world(&preview).view, [0, 0]);
}

#[test]
fn spec019_town_list_travel_entry_and_locked_cities() {
    let dir = corpus();
    let mut preview = preview(dir.path());
    let globals = preview.host.world.gvars.clone();
    open(&mut preview);
    // Row 1 is Klamath: travel toward its jittered position (four draws).
    let mut expected = preview.host.world.rng.clone();
    use vault_core::RandomSource;
    for range in [(0, 16), (0, 1), (0, 16), (0, 1)] {
        expected.range(range.0, range.1).unwrap();
    }
    click(&mut preview, [510, 140 + 27]);
    let travel = state(&mut preview).travel.expect("travelling");
    assert!((484..=516).contains(&travel.destination[0]), "{travel:?}");
    assert!((384..=416).contains(&travel.destination[1]), "{travel:?}");
    assert_eq!(preview.host.world.rng.state(), expected.state());
    run(&mut preview, 1000, 4000, |preview| {
        preview
            .host
            .world
            .fallout2
            .as_deref()
            .unwrap()
            .travel
            .is_none()
    });
    // At rest at the end of the line (SPEC-018 §5.4's stepper).
    let at = state(&mut preview).position;
    assert!(
        (at[0] - travel.destination[0]).abs() <= 1 && (at[1] - travel.destination[1]).abs() <= 1,
        "{at:?} {travel:?}"
    );
    // Row 2 (Locked) does nothing.
    let at = state(&mut preview).position;
    click(&mut preview, [510, 140 + 2 * 27]);
    assert!(state(&mut preview).travel.is_none());
    assert_eq!(state(&mut preview).position, at);
    // At Klamath's center, a click on the party: the pressed triangle for
    // 300 ms, then the first entry loads its lowest known entrance.
    state(&mut preview).position = [500, 400];
    preview.tick(200_000).unwrap();
    let party = screen::window([500, 400], world(&preview).view);
    click(&mut preview, party);
    assert!(matches!(
        world(&preview).pending,
        Some(Pending2::Enter { city: Some(1), .. })
    ));
    preview.tick(200_299).unwrap();
    assert!(matches!(preview.screen, Screen::World2(_)));
    preview.tick(200_300).unwrap();
    assert!(matches!(preview.screen, Screen::Map));
    assert_eq!(preview.host.world.map.header.map_number, 1);
    assert_eq!(state(&mut preview).areas[1].known, 2);
    // No campaign global was written.
    assert_eq!(preview.host.world.gvars, globals);
}

#[test]
fn spec019_town_map_markers_labels_and_entering() {
    let dir = corpus();
    let mut preview = preview(dir.path());
    open(&mut preview);
    // Town/World at rest: the current city's town map (Arroyo).
    click(&mut preview, [522, 441]);
    let Screen::Town2(town) = &preview.screen else {
        panic!("town map");
    };
    assert_eq!((town.city, town.origin), (0, TownOrigin::TownWorld));
    // The two known entrances; their anchors on the picture at (20, 20).
    assert_eq!(town.markers, [(0, [120, 220]), (1, [170, 240])]);
    let image = preview.image().unwrap();
    assert_eq!(pixel(&image, [30, 30]), 84, "the picture");
    assert_eq!(pixel(&image, [132, 226]), 7, "entrance 0's triangle");
    // The town map's Town/World returns to the world map, follow off.
    click(&mut preview, [522, 441]);
    assert!(matches!(preview.screen, Screen::World2(_)));
    assert!(!world(&preview).follow);
    click(&mut preview, [522, 441]);
    // Entrance 1 (Test Village, the header's entrance) while in Arroyo.
    preview.click([190, 250], false);
    preview.pointer.at = Some([190, 250]);
    preview.release(Some([190, 250]), false);
    assert!(matches!(preview.screen, Screen::Map));
    assert_eq!(preview.host.world.map.header.map_number, 1);
    assert_eq!(state(&mut preview).entrance, 1);
}

#[test]
fn spec019_the_fuel_gauge_and_running_dry() {
    let dir = corpus();
    let mut preview = preview(dir.path());
    // The car is in Arroyo, the map just left: the party drives.
    state(&mut preview).car = vault_core::fallout2::Car {
        has_car: true,
        area: 0,
        map: 2,
        fuel: 40000,
    };
    open(&mut preview);
    assert!(state(&mut preview).driving);
    let image = preview.image().unwrap();
    let green = ColorKey::GREEN.resolve(&preview.palette);
    let black = ColorKey::BLACK.resolve(&preview.palette);
    // 94 rows filled from the bottom of (493, 138) 6 by 189.
    assert_eq!(pixel(&image, [493, 138 + 189 - 1]), green);
    assert_eq!(pixel(&image, [498, 138 + 189 - 94]), green);
    assert_eq!(pixel(&image, [493, 138 + 189 - 95]), black);
    // The last 100 fuel: the first iteration empties the tank; the large
    // Done box shows and travel stops at rest.
    state(&mut preview).car.fuel = 100;
    click(&mut preview, [400, 300]);
    preview.tick(1000).unwrap();
    assert_eq!(state(&mut preview).car.fuel, 0);
    assert!(state(&mut preview).travel.is_none());
    assert!(preview.dialog.is_some());
    // Car steps: 4 pixels on difficulty 1 in that one iteration.
    let moved = state(&mut preview).position;
    assert_eq!(
        (moved[0] - 175).abs().max((moved[1] - 125).abs()),
        4,
        "{moved:?}"
    );
}

#[test]
fn spec024_a_forced_encounter_flashes_unless_its_flags_say_no_icon() {
    let selected = |forced| Selected {
        table: 0,
        entry: 0,
        map: 0,
        special: false,
        forced,
    };
    assert!(<super::Fallout2 as super::WorldEdition>::flashes(selected(
        None
    )));
    assert!(<super::Fallout2 as super::WorldEdition>::flashes(selected(
        Some(0)
    )));
    assert!(<super::Fallout2 as super::WorldEdition>::flashes(selected(
        Some(0x8)
    )));
    assert!(!<super::Fallout2 as super::WorldEdition>::flashes(
        selected(Some(0x4))
    ));
}

#[test]
fn spec019_encounter_flash_and_terrain_drop() {
    let dir = corpus();
    let mut preview = preview(dir.path());
    open(&mut preview);
    // An ordinary encounter's flash shows the bolt, then its map loads.
    if let Screen::World2(world) = &mut preview.screen {
        world.pending = Some(Pending2::Flash {
            start: 5000,
            selected: Selected {
                table: 0,
                entry: 0,
                map: 0,
                special: false,
                forced: None,
            },
        });
    }
    preview.tick(5000).unwrap();
    let image = preview.image().unwrap();
    assert_eq!(pixel(&image, [22 + 175 - 3, 21 + 125 - 5]), 14, "the bolt");
    preview.tick(5000 + 14 * 199).unwrap();
    assert!(matches!(preview.screen, Screen::Map));
    assert_eq!(preview.host.world.map.header.map_number, 0);
    // Outside every city a click on the party drops to terrain.
    open(&mut preview);
    state(&mut preview).position = [400, 300];
    if let Screen::World2(world) = &mut preview.screen {
        world.view = [200, 100];
        world.follow = false;
    }
    preview.tick(9000).unwrap();
    let party = screen::window([400, 300], [200, 100]);
    click(&mut preview, party);
    assert!(matches!(
        world(&preview).pending,
        Some(Pending2::Enter { city: None, .. })
    ));
    preview.tick(9300).unwrap();
    assert!(matches!(preview.screen, Screen::Map));
    assert_eq!(preview.host.world.map.header.map_number, 0);
}

#[test]
fn spec019_fallout1_keeps_its_own_world_map() {
    let dir = corpus();
    let mut preview = interactive(dir.path());
    preview.open_world_map();
    assert!(matches!(preview.screen, Screen::World(_)));
}
