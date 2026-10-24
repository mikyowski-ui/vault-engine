//! Live-corpus regressions for `--game` behavior, run with
//! `cargo test -p vault-platform -- --ignored` beside a `local-corpus`.
use super::{MapPreview, test_support::local_corpus};
use vault_core::{ObjectRef, WorldEffect};
use vault_formats::{
    map::{ObjectDetail, ObjectState},
    pro::Category,
};

fn map(name: &str) -> Option<MapPreview> {
    let vfs = local_corpus()?;
    let mut preview = MapPreview::load(vfs, &format!("maps/{name}"), "english", None, 1).unwrap();
    preview.host.world.effects.clear();
    Some(preview)
}

fn hex(preview: &MapPreview, object: ObjectRef) -> Option<i32> {
    preview.host.world.get(object)?.raw.hex
}

/// Tick the host in 50 ms frames until `done` or `limit_ms`; returns the
/// time reached.
fn run(preview: &mut MapPreview, limit_ms: u64, mut done: impl FnMut(&MapPreview) -> bool) -> u64 {
    let mut now = 1000;
    while now < 1000 + limit_ms && !done(preview) {
        preview.host.tick(now).unwrap();
        now += 50;
    }
    now - 1000
}

/// A hostile animal's critter_p_proc runs on the background tick and
/// attacks a player beside it, which starts combat.
fn animals_start_combat(name: &str) {
    let Some(mut preview) = map(name) else {
        return;
    };
    let player = preview.host.world.player;
    let animal = preview
        .host
        .world
        .iter()
        .filter(|(reference, object)| {
            *reference != player
                && object.raw.hex.is_some()
                && object.raw.elevation == preview.host.world.elevation
                && Category::from_pid(object.raw.pid).ok() == Some(Category::Critter)
        })
        .map(|(reference, _)| reference)
        .next()
        .expect("an animal");
    let beside = vault_core::routing::step(hex(&preview, animal).unwrap(), 0);
    let elevation = preview.host.world.elevation;
    preview
        .host
        .world
        .move_object(player, beside, elevation, false)
        .unwrap();
    let elapsed = run(&mut preview, 5000, |preview| preview.host.world.combat);
    assert!(
        preview.host.world.combat,
        "{name}: no combat after {elapsed} ms"
    );
}

#[test]
#[ignore = "needs the local corpus"]
fn v13ent_cave_rats_start_combat_when_the_player_comes_close() {
    animals_start_combat("v13ent.map");
}

#[test]
#[ignore = "needs the local corpus"]
fn v13ent_walking_across_the_cave_exit_grid_leaves_on_that_step() {
    let Some(mut preview) = map("v13ent.map") else {
        return;
    };
    let player = preview.host.world.player;
    let elevation = preview.host.world.elevation;
    // The cave mouth's grids lead to the world map (-2).
    let grids: Vec<i32> = preview
        .host
        .world
        .iter()
        .filter(|(_, object)| object.raw.elevation == elevation)
        .filter_map(|(_, object)| match object.raw.state {
            ObjectState::Other {
                detail: ObjectDetail::ExitGrid { map: -2, .. },
                ..
            } => object.raw.hex,
            _ => None,
        })
        .collect();
    assert!(!grids.is_empty());
    // Stand two hexes before a grid and walk to two hexes past it, so the
    // grid hex lies inside a multi-hex walk.
    let start = hex(&preview, player).unwrap();
    let step = vault_core::routing::step;
    let (before, grid, beyond) = grids
        .iter()
        .flat_map(|grid| (0u8..6).map(move |direction| (*grid, direction)))
        .find_map(|(grid, direction)| {
            let back = (direction + 3) % 6;
            let before = step(step(grid, back), back);
            let beyond = step(step(grid, direction), direction);
            let world = &preview.host.world;
            let clear = |at: i32| world.movement_blocker(at, elevation, player).is_none();
            let route = world.route(player, before, beyond, true);
            (clear(before)
                && !grids.contains(&before)
                && !world.route(player, start, before, true).is_empty()
                && route.len() == 4
                && route.iter().all(|turn| *turn == direction))
            .then_some((before, grid, beyond))
        })
        .expect("a straight walk across a grid");
    preview
        .host
        .world
        .move_object(player, before, elevation, false)
        .unwrap();
    preview.host.world.effects.clear();
    assert!(preview.host.walk_toward(player, beyond, false, 0).unwrap());
    let mut left = false;
    let elapsed = run(&mut preview, 20000, |preview| {
        left |= preview.host.world.effects.contains(&WorldEffect::WorldMap);
        left || !preview.host.moving(preview.host.world.player)
    });
    assert!(
        left,
        "no world map after {elapsed} ms walking {before} -> {beyond}"
    );
    // The walk stops on the first grid hex of its path.
    let direction = vault_core::routing::rotation(before, grid).unwrap();
    let first = std::iter::successors(Some(before), |at| Some(step(*at, direction)))
        .take(5)
        .find(|at| grids.contains(at));
    assert_eq!(hex(&preview, player), first, "the walk stops on the grid");
}

#[test]
#[ignore = "needs the local corpus"]
fn egg_mask_matches_spec014_and_the_cave_view_renders() {
    let Some(mut preview) = map("v13ent.map") else {
        return;
    };
    let egg = preview.egg_mask().expect("egg.frm");
    let frame = egg.frame(0, 0).unwrap();
    assert_eq!((frame.width, frame.height), (129, 98));
    let pixels = frame.pixels();
    assert_eq!(pixels.iter().filter(|weight| **weight == 1).count(), 4716);
    assert_eq!(pixels.iter().copied().max(), Some(122));
    assert_eq!(egg.shifts, [[0, 25]; 6]);
    // Inside the cave mouth the rock wall is drawn over the player (object
    // order) and the cutout reveals the player through it. `VAULT_VIEW_PPM`
    // saves the view for inspection.
    let player = preview.host.world.player;
    preview
        .host
        .world
        .move_object(player, 23353, 0, false)
        .unwrap();
    preview.view.grid = vault_formats::HexGrid::centered(23353, 640, 380).unwrap();
    let image = preview.map_view_image().unwrap();
    preview.egg = Some(None);
    let plain = preview.map_view_image().unwrap();
    let changed = image
        .pixels()
        .iter()
        .zip(plain.pixels())
        .filter(|(a, b)| a != b)
        .count();
    assert!(changed > 1000, "the cutout changed only {changed} pixels");
    if let Some(path) = std::env::var_os("VAULT_VIEW_PPM") {
        let rgba = preview.palette.rgba8();
        let mut ppm = format!("P6 {} {} 255\n", image.width(), image.height()).into_bytes();
        for index in image.pixels() {
            ppm.extend(&rgba[usize::from(*index)][..3]);
        }
        std::fs::write(path, ppm).unwrap();
    }
}

/// SPEC-014 §3.38 over the corpus: `called.frm` is 424 by 309, every `na`
/// picture reached through the critter-list alias is 170 by 225 with one
/// frame, and the labels follow the alias (Fixture H: `hmmaxx`, row 13,
/// aliases 11; `mabrom`, row 15, labels 1150-1157).
#[test]
#[ignore = "needs the local corpus"]
fn called_shot_art_and_labels_follow_the_alias() {
    use vault_core::assets::WorldAssets;
    let Some(preview) = map("v13ent.map") else {
        return;
    };
    let painter = crate::interface::Painter {
        assets: &preview.host.assets,
        palette: &preview.palette,
    };
    assert_eq!(painter.size("called.frm"), Some([424, 309]));
    let catalog = preview.host.assets.art_catalog().unwrap();
    assert_eq!(catalog.critter_alias(13), Some(11));
    assert_eq!(catalog.critter_alias(0), Some(0));
    let mut pictures = 0;
    for base in 0..100u16 {
        if catalog.critter_alias(base).is_none() {
            break;
        }
        let fid = vault_formats::Fid::new(1, base, 64, 0, 0).0;
        if let Ok(art) = preview.host.assets.object_art(fid) {
            let frame = art.frame(0, 0).unwrap();
            assert_eq!((frame.width, frame.height), (170, 225), "row {base}");
            assert_eq!(art.frame_count(), 1);
            pictures += 1;
        }
    }
    assert!(pictures > 0);
    let combat = preview.host.assets.game_messages("combat.msg").unwrap();
    let text = |id| combat.get(id).map(|message| message.text.clone());
    assert_eq!(text(1110).as_deref(), Some(b"the head".as_slice()));
    assert_eq!(text(1113).as_deref(), Some(b"the torso".as_slice()));
    assert_eq!(text(1155).as_deref(), Some(b"the left foreleg".as_slice()));
    assert_eq!(text(1000), None);
}

/// SPEC-014 §3.45 and Fixture R over the corpus: the panel and window art
/// sizes, the font widths and title centering, the messages the windows
/// show, and map 35 (`V13ENT`), where no one has anything to tell.
#[test]
#[ignore = "needs the local corpus"]
fn dialogue_review_and_ask_about_resources() {
    use vault_core::ui::talk;
    let Some(preview) = map("v13ent.map") else {
        return;
    };
    assert_eq!(
        preview.host.world.map.header.map_number,
        vault_core::host_talk::SILENT_MAP
    );
    let painter = crate::interface::Painter {
        assets: &preview.host.assets,
        palette: &preview.palette,
    };
    for (art, size) in [
        (talk::PANEL, [640, 190]),
        (talk::REVIEW_BACKGROUND, [640, 480]),
        (talk::ASK_BACKGROUND, talk::ASK_SIZE),
        (talk::SMALL_ART.0, [14, 14]),
        (talk::REVIEW_ART.0, [51, 29]),
        (talk::SCROLL_UP.1.0, [35, 35]),
        (talk::SCROLL_DOWN.1.0, [35, 37]),
        (talk::DONE.1.0, [82, 46]),
    ] {
        assert_eq!(painter.size(art), Some(size), "{art}");
    }
    assert_eq!(painter.width(101, b"Abel:"), 29);
    assert_eq!(painter.width(101, b"_"), 8);
    assert_eq!(painter.width(101, b"Water"), 35);
    let title = painter.message(talk::ASK_TITLE.0, talk::ASK_TITLE.1);
    assert_eq!(title, b"Ask about what?");
    let width = painter.width(talk::ASK_FONT, &title);
    assert_eq!(width, 107);
    assert_eq!(vault_core::ui::centered(talk::ASK_SIZE[0], width), 91);
    assert_eq!(painter.message("proto.msg", 650), b"[Done]");
    assert_eq!(
        painter.message("proto.msg", 903),
        b"This person will not barter with you."
    );
    assert_eq!(
        painter.message("proto.msg", 904),
        b"This person has nothing to tell you about."
    );
    assert_eq!(painter.message("dbox.msg", 100), b"DONE");
    assert_eq!(painter.message("dbox.msg", 103), b"CANCEL");
}
