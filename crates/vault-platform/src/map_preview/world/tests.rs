//! SPEC-014 Fixture I (§4.9) and the §3.39-§3.40 screen flows over the
//! synthetic corpus, with synthetic world-map art.
use super::{Pending, TownOrigin, WorldScreen};
use crate::map_preview::{MapPreview, Screen, fixture, state_fixture};
use std::{path::Path, sync::Arc};
use vault_core::{
    ui::ColorKey,
    worldmap::{Encounter, START},
};
use vault_formats::{LocalDirProvider, Vfs};
use winit::keyboard::KeyCode;

/// Pixel data starts after the 62-byte header and the 12-byte frame header.
const PIXELS: usize = 74;

/// A one-frame FRM whose pixel (x, y) is `color(x, y)`.
fn art(width: u16, height: u16, color: impl Fn(usize, usize) -> u8) -> Vec<u8> {
    let mut bytes = fixture::frm(width, height, 0);
    for y in 0..usize::from(height) {
        for x in 0..usize::from(width) {
            bytes[PIXELS + y * usize::from(width) + x] = color(x, y);
        }
    }
    bytes
}

/// The synthetic corpus with world-map art: the world in index 50, the
/// frame in 9 around its hole, solid markers, digit and month strips whose
/// colors give the digit or month, and town pictures for Vault 13 and
/// Vault 15. `V13ENT.MAP` is the fixture map renamed in its header (map
/// 35), and `VAULTBUR.MAP` likewise (map 8).
fn corpus() -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    state_fixture::write_corpus(root);
    let interface = root.join("art/intrface");
    let write = |name: &str, bytes: Vec<u8>| std::fs::write(interface.join(name), bytes).unwrap();
    write("worldmap.frm", fixture::frm(1400, 1500, 50));
    write(
        "wmapbox.frm",
        art(640, 480, |x, y| {
            let hole = (22..472).contains(&x) && (21..463).contains(&y);
            if hole { 0 } else { 9 }
        }),
    );
    write("hotspot1.frm", fixture::frm(25, 13, 7));
    write("hotspot2.frm", fixture::frm(25, 13, 8));
    write("wmaploc.frm", fixture::frm(5, 5, 11));
    write("wmaptarg.frm", fixture::frm(11, 11, 12));
    write("wmaptrg2.frm", fixture::frm(11, 11, 13));
    write("wmapfgt0.frm", fixture::frm(7, 11, 14));
    write("wmapfgt1.frm", fixture::frm(7, 11, 15));
    write("lilredup.frm", fixture::frm(15, 16, 16));
    write("lilreddn.frm", fixture::frm(15, 16, 17));
    write("wmaplabs.frm", art(82, 216, |_, y| 100 + (y / 18) as u8));
    write("numbers.frm", art(360, 17, |x, _| 1 + (x / 9) as u8));
    write("months.frm", art(29, 179, |_, y| 1 + (y / 15) as u8));
    write("twnmap00.frm", fixture::frm(453, 444, 60));
    write("twnmap01.frm", fixture::frm(453, 444, 61));
    let map = std::fs::read(root.join("maps/synthetic.map")).unwrap();
    for (file, header) in [
        ("v13ent.map", "V13ENT.MAP"),
        ("vaultbur.map", "VAULTBUR.MAP"),
    ] {
        let mut renamed = map.clone();
        let mut name = [0_u8; 16];
        name[..header.len()].copy_from_slice(header.as_bytes());
        renamed[4..20].copy_from_slice(&name);
        std::fs::write(root.join("maps").join(file), renamed).unwrap();
    }
    std::fs::write(root.join("maps/desert1.map"), &map).unwrap();
    let catalog = root.join("text/english/game/map.msg");
    let mut messages = std::fs::read(&catalog).unwrap();
    messages.extend_from_slice(b"{8}{}{VAULTBUR.MAP}\n{35}{}{V13ENT.MAP}\n");
    std::fs::write(&catalog, messages).unwrap();
    dir
}
fn preview(root: &Path) -> MapPreview {
    let vfs: Arc<dyn Vfs> = Arc::new(LocalDirProvider::mount(root).unwrap());
    let mut preview = MapPreview::load(
        vfs,
        "maps/synthetic.map",
        "english",
        Some("premade/synthetic.gcd"),
        1,
    )
    .unwrap();
    preview.image().unwrap();
    preview
}
fn world(preview: &MapPreview) -> &WorldScreen {
    match &preview.screen {
        Screen::World(world) => world,
        _ => panic!("the world map is not open"),
    }
}
fn pixel(image: &vault_render::IndexedImage, [x, y]: [i32; 2]) -> u8 {
    image.pixels()[(y * image.width() as i32 + x) as usize]
}
/// A left press and release at `point`.
fn click(preview: &mut MapPreview, point: [i32; 2]) {
    preview.click(point, false);
    preview.release(Some(point), false);
}
/// Ticks 41 milliseconds apart from `from` until `done` or `limit` passes.
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

#[test]
fn spec014_fixture_i_world_view_darkening_markers_and_date() {
    let dir = corpus();
    let mut preview = preview(dir.path());
    preview.host.world.world_map.reset_discovery();
    preview.host.world.clock.set(264_600);
    assert!(preview.step(KeyCode::KeyW));
    let screen = world(&preview);
    assert_eq!((screen.view, screen.follow), ([578, 0], true));
    let image = preview.image().unwrap();
    // Cell (16, 1) at (244, 71) is visited; the eight around it are dimmed;
    // every other visible cell is black (key 0).
    let black = ColorKey::BLACK.resolve(&preview.palette);
    let dim = preview.palette.intensity(50, 64);
    assert_eq!(pixel(&image, [245, 72]), 50);
    assert_eq!(pixel(&image, [195, 72]), dim);
    assert_eq!(pixel(&image, [300, 125]), dim);
    assert_eq!(pixel(&image, [350, 200]), black);
    assert_eq!(pixel(&image, [30, 30]), black);
    // The at-rest triangle at (257, 90), 25 by 13.
    assert_eq!(pixel(&image, [257, 90]), 7);
    assert_eq!(pixel(&image, [281, 102]), 7);
    assert_eq!(pixel(&image, [256, 90]), 50);
    // The frame over the margins, and the side panel.
    assert_eq!(pixel(&image, [10, 10]), 9);
    assert_eq!(pixel(&image, [508, 61]), 16);
    // Vault 13's name plate (row 0 of the plates) is known; Vault 15's is
    // not drawn.
    assert_eq!(pixel(&image, [531, 61]), 100);
    assert_eq!(pixel(&image, [531, 88]), 9);
    // Fixture I: tick 264600 is 07:21 on 05 DEC 2161: two day digits, the
    // month row, four year digits and HHMM 0721.
    let [month, day, year] = vault_core::host_save::game_date(264_600);
    assert_eq!([month, day, year], [12, 5, 2161]);
    let digits = |value: u16, count: u32| -> Vec<u8> {
        (0..count)
            .map(|k| (value / 10_u16.pow(count - 1 - k) % 10) as u8)
            .collect()
    };
    let cells = [
        (487, digits(day, 2)),
        (548, digits(year, 4)),
        (593, vec![0, 7, 2, 1]),
    ];
    for (left, values) in cells {
        for (k, digit) in values.into_iter().enumerate() {
            let x = left + 9 * k as i32;
            assert_eq!(pixel(&image, [x, 12]), 1 + digit, "x {x}");
            assert_eq!(pixel(&image, [x + 8, 28]), 1 + digit, "x {x}");
        }
    }
    assert_eq!(pixel(&image, [513, 12]), month as u8, "month row");
    assert_eq!(pixel(&image, [541, 25]), month as u8, "month row");
}

#[test]
fn spec014_fixture_i_destination_click_dots_scrolling_and_keys() {
    let dir = corpus();
    let mut preview = preview(dir.path());
    assert!(preview.step(KeyCode::KeyW));
    // A click that closes the pause window does nothing else.
    assert!(preview.step(KeyCode::KeyP));
    click(&mut preview, [400, 300]);
    assert!(!world(&preview).paused);
    assert!(preview.host.world.world_map.travel.is_none());
    // A destination click at (400, 300): world (956, 279), no jitter;
    // follow mode stays on (the party was at rest).
    click(&mut preview, [400, 300]);
    let travel = preview.host.world.world_map.travel.unwrap();
    assert_eq!(travel.destination, [956, 279]);
    assert!(world(&preview).follow);
    // The first pass marks the party's pixel; the plus sign and crosshair
    // replace the triangle.
    preview.tick(1000).unwrap();
    let start = preview.host.world.world_map.position();
    assert_ne!(start, START);
    let image = preview.image().unwrap();
    let screen = world(&preview);
    // Mountain travel stepped once in the first of two iterations, so the
    // marked pixel is the party's: the dot is painted over the plus sign.
    let party = vault_core::worldmap::screen::window(start, screen.view);
    assert!(screen.marked(start));
    assert_eq!(
        pixel(&image, party),
        ColorKey(vault_core::worldmap::screen::DOT_KEY).resolve(&preview.palette)
    );
    assert_eq!(pixel(&image, [party[0] + 2, party[1] + 2]), 11);
    let target = vault_core::worldmap::screen::window([956, 279], screen.view);
    assert_eq!(pixel(&image, target), 12);
    // A click during travel retargets and switches follow mode off.
    click(&mut preview, [100, 100]);
    assert!(!world(&preview).follow);
    // Esc does nothing; A toggles follow; Home and End scroll.
    assert!(preview.step(KeyCode::Escape));
    assert!(matches!(preview.screen, Screen::World(_)));
    assert!(preview.step(KeyCode::KeyA));
    assert!(world(&preview).follow);
    assert!(preview.step(KeyCode::End));
    assert_eq!(world(&preview).view[1], 1058);
    assert!(!world(&preview).follow);
    assert!(preview.step(KeyCode::KeyC));
    let centered = vault_core::worldmap::screen::centered(preview.host.world.world_map.position());
    assert_eq!(world(&preview).view, centered);
    // The scroll cursors: east blocked at vx 950.
    if let Screen::World(world) = &mut preview.screen {
        world.view = [950, 300];
    }
    assert_eq!(preview.world_cursor([639, 240]), "screx.frm");
    if let Screen::World(world) = &mut preview.screen {
        world.view = [900, 0];
    }
    assert_eq!(preview.world_cursor([639, 0]), "scrneast.frm");
    assert_eq!(preview.world_cursor([300, 300]), "stdarrow.frm");
}

#[test]
fn spec014_entering_a_known_town_opens_its_town_map_and_a_marker_loads() {
    let dir = corpus();
    let mut preview = preview(dir.path());
    assert!(preview.step(KeyCode::KeyW));
    // Vault 13's town button while at rest in its cell: the pressed
    // triangle shows for 300 ms, then its town map opens.
    let now = 1000;
    preview.tick(now).unwrap();
    click(&mut preview, [510, 63]);
    assert!(matches!(
        world(&preview).pending,
        Some(Pending::Enter { town: Some(0), .. })
    ));
    let image = preview.image().unwrap();
    assert_eq!(pixel(&image, [257, 90]), 8, "the pressed triangle");
    preview.tick(now + 299).unwrap();
    assert!(matches!(preview.screen, Screen::World(_)));
    preview.tick(now + 300).unwrap();
    let Screen::Town(town) = &preview.screen else {
        panic!("town map");
    };
    assert_eq!((town.town, town.origin), (0, TownOrigin::Entered));
    // Before tick 864000 only the Cavern Entrance shows, its triangle at
    // (214, 309) over the picture at (20, 20).
    assert_eq!(town.buttons.len(), 1);
    let image = preview.image().unwrap();
    assert_eq!(pixel(&image, [214, 309]), 7);
    assert_eq!(pixel(&image, [30, 30]), 60);
    // Outside the triangle's pixels nothing reacts; on them the press
    // shows the filled triangle and the release loads the entrance.
    click(&mut preview, [213, 309]);
    assert!(matches!(preview.screen, Screen::Town(_)));
    preview.click([220, 312], false);
    preview.pointer.at = Some([220, 312]);
    let image = preview.image().unwrap();
    assert_eq!(pixel(&image, [214, 309]), 8);
    preview.release(Some([220, 312]), false);
    assert!(
        matches!(preview.screen, Screen::Map),
        "{}",
        String::from_utf8_lossy(&preview.status)
    );
    let gvars = preview.host.world.gvars.as_ref();
    assert_eq!(gvars.and_then(|g| g.get(32)).copied().unwrap_or(0), 0);
    assert_eq!(
        preview.host.world.current_map_name().as_deref(),
        Some("V13ENT.SAV")
    );
    assert_eq!(preview.host.world.world_map.saved.town, 0);
}

#[test]
fn spec014_fixture_i_hotspot_in_another_town_travels_without_entering() {
    let dir = corpus();
    let mut preview = preview(dir.path());
    // Vault 15 known with its slot 2; the party one cell west of it.
    let mut gvars = vec![0; 700];
    gvars[10] = 30;
    gvars[70] = 1;
    gvars[564] = 1;
    preview.host.world.gvars = Some(gvars);
    preview.host.world.world_map.set_position([1225, 75]);
    assert!(preview.step(KeyCode::KeyW));
    // The Town/World button opens the current town's (Vault 13) map.
    click(&mut preview, [522, 441]);
    let Screen::Town(town) = &preview.screen else {
        panic!("town map");
    };
    assert_eq!((town.town, town.origin), (0, TownOrigin::TownWorld));
    // Vault 15's side button switches to its picture and markers.
    click(&mut preview, [510, 90]);
    let Screen::Town(town) = &preview.screen else {
        panic!("town map");
    };
    assert_eq!(town.town, 1);
    assert_eq!(preview.host.world.world_map.saved.town, 0);
    let slot = town.buttons.iter().position(|b| b.slot == 2).unwrap();
    assert_eq!(town.buttons[slot].map, "VAULTBUR.MAP");
    // Slot 2 (triangle at (310, 193)): the world map opens with the party
    // travelling to Vault 15's jittered center, follow mode on.
    click(&mut preview, [322, 199]);
    let screen = world(&preview);
    assert!(screen.follow);
    assert_eq!(screen.journey, Some(1));
    let travel = preview.host.world.world_map.travel.unwrap();
    assert_eq!(
        vault_core::worldmap::cell_of(travel.destination),
        Some([25, 1])
    );
    // It stops there at rest; VAULTBUR.MAP does not load.
    run(&mut preview, 1000, 400, |p| {
        p.host.world.world_map.travel.is_none()
    });
    assert!(matches!(preview.screen, Screen::World(_)));
    assert_eq!(preview.host.world.world_map.cell(), Some([25, 1]));
    assert_eq!(world(&preview).journey, None);
    assert_ne!(
        preview.host.world.current_map_name().as_deref(),
        Some("VAULTBUR.SAV")
    );
}

#[test]
fn spec014_town_world_button_returns_at_rest_and_the_world_button_from_an_exit_grid() {
    let dir = corpus();
    let mut preview = preview(dir.path());
    assert!(preview.step(KeyCode::KeyW));
    click(&mut preview, [400, 300]);
    assert!(preview.host.world.world_map.travel.is_some());
    // Town/World stops the journey where the party is.
    click(&mut preview, [522, 441]);
    assert!(matches!(preview.screen, Screen::Town(_)));
    assert!(preview.host.world.world_map.travel.is_none());
    // Its World button returns at rest, follow mode off.
    click(&mut preview, [522, 441]);
    assert!(!world(&preview).follow);
    // The town map of an exit grid holding map -1: its World button opens
    // the world map normally, follow mode on.
    preview.open_town_map();
    let Screen::Town(town) = &preview.screen else {
        panic!("town map");
    };
    assert_eq!(town.origin, TownOrigin::Transition);
    assert!(preview.step(KeyCode::Escape), "no Esc, but swallowed");
    assert!(matches!(preview.screen, Screen::Town(_)));
    click(&mut preview, [522, 441]);
    assert!(world(&preview).follow);
    // A town map that fails leaves the world map open (safe rule).
    preview.host.world.world_map.saved.town = 4;
    click(&mut preview, [522, 441]);
    assert!(matches!(preview.screen, Screen::World(_)));
}

#[test]
fn spec014_fixture_i_encounter_flash_then_load() {
    let dir = corpus();
    let mut preview = preview(dir.path());
    let mut gvars = vec![0; 700];
    gvars[10] = 30;
    preview.host.world.gvars = Some(gvars);
    assert!(preview.step(KeyCode::KeyW));
    preview.tick(1000).unwrap();
    if let Screen::World(world) = &mut preview.screen {
        world.view = [0, 0];
        world.pending = Some(Pending::Flash {
            start: 1000,
            encounter: Encounter::Terrain {
                map: "DESERT1.MAP",
                region: 2,
                context: 2,
            },
        });
    }
    // The view recenters; the bolt alternates at (266, 91).
    let image = preview.image().unwrap();
    assert_eq!(pixel(&image, [266, 91]), 14);
    preview.tick(1000 + 199).unwrap();
    assert_eq!(pixel(&preview.image().unwrap(), [266, 91]), 15);
    preview.tick(1000 + 2785).unwrap();
    assert!(matches!(preview.screen, Screen::World(_)));
    preview.tick(1000 + 2786).unwrap();
    assert!(
        matches!(preview.screen, Screen::Map),
        "{}",
        String::from_utf8_lossy(&preview.status)
    );
    let gvars = preview.host.world.gvars.as_ref().unwrap();
    assert_eq!((gvars.get(65), gvars.get(32)), (Some(&2), Some(&2)));
}

#[test]
fn spec014_the_water_check_quits_from_the_world_map() {
    let dir = corpus();
    let mut preview = preview(dir.path());
    let mut gvars = vec![0; 700];
    gvars[101] = 1;
    preview.host.world.gvars = Some(gvars);
    assert!(preview.step(KeyCode::KeyW));
    preview.tick(1000).unwrap();
    assert!(preview.quit_requested);
    assert!(matches!(preview.screen, Screen::Map));
}

#[test]
fn m_w2_a_vault13_campaign_exit_leaves_no_stale_travel_and_follow_on() {
    let dir = corpus();
    let mut preview = preview(dir.path());
    // The vats countdown (index 147) is due and the cathedral is gone
    // (index 18): the pass moves the party to Vault 13 and loads V13ENT.
    let mut gvars = vec![0; 700];
    gvars[10] = 30;
    gvars[147] = 1;
    gvars[18] = 1;
    preview.host.world.gvars = Some(gvars);
    preview.host.world.clock.set(10_000);
    assert!(preview.step(KeyCode::KeyW));
    click(&mut preview, [400, 300]);
    assert!(preview.host.world.world_map.travel.is_some());
    preview.tick(1000).unwrap();
    assert!(
        matches!(preview.screen, Screen::Map),
        "{}",
        String::from_utf8_lossy(&preview.status)
    );
    let map = &preview.host.world.world_map;
    assert!(
        map.travel.is_none() && map.mark.is_none(),
        "no stale journey"
    );
    // Reopened, the party is at rest with follow mode on, and a destination
    // click keeps it on (the party was at rest).
    assert!(preview.step(KeyCode::KeyW));
    assert!(world(&preview).follow);
    click(&mut preview, [400, 300]);
    assert!(world(&preview).follow);
}

#[test]
fn h14_a_dialog_answered_on_the_world_map_does_not_click_the_view() {
    let dir = corpus();
    let mut preview = preview(dir.path());
    let mut gvars = vec![0; 700];
    gvars[10] = 30;
    preview.host.world.gvars = Some(gvars);
    assert!(preview.step(KeyCode::KeyW));
    preview.tick(1000).unwrap();
    assert!(preview.step(KeyCode::F10));
    assert!(preview.dialog.is_some());
    // No is the red button at (169 + 182, 117 + 102), inside the view.
    let no = [351, 219];
    assert!(vault_core::worldmap::screen::view_click(no));
    click(&mut preview, no);
    assert!(preview.dialog.is_none() && !preview.quit_requested);
    assert!(
        preview.host.world.world_map.travel.is_none(),
        "the release did not start travel"
    );
    assert!(matches!(preview.screen, Screen::World(_)));
    // The next view click is a destination click again.
    click(&mut preview, [400, 300]);
    assert!(preview.host.world.world_map.travel.is_some());
}

#[test]
fn m_w3_no_travel_progresses_under_the_quit_confirmation() {
    let dir = corpus();
    let mut preview = preview(dir.path());
    let mut gvars = vec![0; 700];
    gvars[10] = 30;
    preview.host.world.gvars = Some(gvars);
    assert!(preview.step(KeyCode::KeyW));
    click(&mut preview, [400, 300]);
    let mut now = run(&mut preview, 1000, 3, |_| false);
    assert!(preview.host.world.world_map.travel.is_some());
    assert!(preview.step(KeyCode::F10));
    let (position, ticks) = (
        preview.host.world.world_map.position(),
        preview.host.world.clock.ticks,
    );
    now = run(&mut preview, now, 20, |_| false);
    assert_eq!(preview.host.world.world_map.position(), position);
    assert_eq!(preview.host.world.clock.ticks, ticks);
    assert!(preview.dialog.is_some());
    // Esc answers No; the journey resumes.
    assert!(preview.step(KeyCode::Escape));
    run(&mut preview, now, 3, |_| false);
    assert_ne!(preview.host.world.world_map.position(), position);
}

#[test]
fn spec014_an_arrow_key_at_the_scroll_limit_keeps_follow_mode() {
    let dir = corpus();
    let mut preview = preview(dir.path());
    assert!(preview.step(KeyCode::KeyW));
    if let Screen::World(world) = &mut preview.screen {
        world.view = [0, 300];
        world.follow = true;
    }
    assert!(preview.step(KeyCode::ArrowLeft));
    assert!(world(&preview).follow);
    assert_eq!(world(&preview).view, [0, 300]);
    assert!(preview.step(KeyCode::ArrowRight));
    assert!(!world(&preview).follow);
    assert_eq!(world(&preview).view, [16, 300]);
}

#[test]
#[ignore = "needs the local corpus"]
fn corpus_world_and_town_map_art_and_seen_cell_dimming() {
    let dir = std::env::var_os("VAULT_CORPUS")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| Path::new(env!("CARGO_MANIFEST_DIR")).join("../../../local-corpus"))
        .join("fallout1-unpacked");
    if !dir.is_dir() {
        eprintln!("skipping: no local corpus at {}", dir.display());
        return;
    }
    let vfs: Arc<dyn Vfs> = Arc::new(LocalDirProvider::mount(&dir).unwrap());
    let preview = MapPreview::load(vfs, "maps/v13ent.map", "english", None, 1).unwrap();
    let painter = preview.painter();
    // SPEC-014 §3.39-§3.40 art sizes.
    for (name, size) in [
        ("worldmap.frm", [1400, 1500]),
        ("wmapbox.frm", [640, 480]),
        ("wmaplabs.frm", [82, 216]),
        ("wmaploc.frm", [5, 5]),
        ("wmaptarg.frm", [11, 11]),
        ("wmaptrg2.frm", [11, 11]),
        ("wmapfgt0.frm", [7, 11]),
        ("wmapfgt1.frm", [7, 11]),
        ("hotspot1.frm", [25, 13]),
        ("hotspot2.frm", [25, 13]),
        ("months.frm", [29, 179]),
        ("numbers.frm", [360, 17]),
        ("lilredup.frm", [15, 16]),
        ("lilreddn.frm", [15, 16]),
    ] {
        assert_eq!(painter.size(name), Some(size), "{name}");
    }
    for town in 0..12 {
        let name = vault_core::worldmap::screen::town_picture(town).unwrap();
        assert_eq!(painter.size(&name), Some([453, 444]), "{name}");
    }
    // Fixture I: seen-cell pixels 50, 150 and 100 become 52, 153 and 228.
    assert_eq!(
        [50, 150, 100].map(|index| preview.palette.intensity(index, 64)),
        [52, 153, 228]
    );
}
