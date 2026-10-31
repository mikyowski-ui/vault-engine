//! SPEC-024b on the shell: `HOOK_INVENTORYMOVE`'s target codes and the drop
//! to the ground, and what the barter window, the bar and the cursors show
//! under a hook (§6.6). A new Fallout 2 game in sfall mode, with the hook's
//! script in a mod layer, so the mod-script check passes it too.
use super::fallout2_combat_tests::{new_game_setup, passive_script, synthetic_install};
use super::*;
use std::sync::Arc;
use vault_core::{
    GameProfile,
    critical::hit_mode,
    host_sfall::game_mode,
    sfall::{GlobalKey, SfallValue},
    ui::{cursor, inventory::DropTarget},
};
use vault_fixtures::state as fixture;
use vault_formats::{LayeredVfs, LocalDirProvider, ModLayer, Vfs};
use winit::keyboard::KeyCode;

const INVENTORYMOVE: i32 = 24;
const MOVECOST: i32 = 11;
const CALCAPCOST: i32 = 2;
const TOHIT: i32 = 0;
const BARTERPRICE: i32 = 10;

const GET_ARG: u16 = 0x81e4;
const SET_RETURN: u16 = 0x81e5;
const REGISTER_PROC: u16 = 0x8262;
const SET_GLOBAL: u16 = 0x819d;
const GLOBAL_INT: u16 = 0x819e;
const ADD: u16 = 0x8039;
const SUBTRACT: u16 = 0x803a;
const MULTIPLY: u16 = 0x803b;
const GREATER: u16 = 0x8038;

/// A word or an int of a hook's body.
enum Op {
    I(i32),
    W(u16),
}
use Op::{I, W};

fn bytes(ops: &[Op]) -> Vec<u8> {
    ops.iter()
        .flat_map(|op| match op {
            I(value) => fixture::push(*value),
            W(word) => fixture::instruction(*word),
        })
        .collect()
}
/// Count the run in global 1, and leave argument `index`'s value in global
/// `10 + index`, for `count` arguments.
fn recording(count: i32) -> Vec<Op> {
    let mut ops = vec![I(1), I(1), W(GLOBAL_INT), I(1), W(ADD), W(SET_GLOBAL)];
    for index in 0..count {
        ops.extend([I(10 + index), W(GET_ARG), W(SET_GLOBAL)]);
    }
    ops
}
/// A game in sfall mode whose `gl_test.int` registers `body` on `hook`. A
/// passive critter stands next to the player.
struct Game {
    world: MapPreview,
    _install: tempfile::TempDir,
    _layer: tempfile::TempDir,
}
fn game(hook: i32, body: &[Op]) -> Game {
    let script = fixture::corpus_program(
        &[],
        &[
            (
                b"start",
                fixture::returning(bytes(&[I(hook), I(2), W(REGISTER_PROC)])),
            ),
            (b"member", fixture::returning(bytes(body))),
        ],
    );
    with_script(script)
}
/// A game in sfall mode whose `gl_test.int` is `script`.
fn with_script(script: Vec<u8>) -> Game {
    let (install, base) = synthetic_install(passive_script());
    let layer = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(layer.path().join("scripts")).unwrap();
    std::fs::write(layer.path().join("scripts/gl_test.int"), script).unwrap();
    // What the displays draw with: the interface font, and a digit strip
    // whose cells differ, as the corpus's one-color art does not.
    std::fs::write(layer.path().join("font1.aaf"), fixture::preview_aaf()).unwrap();
    let art = layer.path().join("art/intrface");
    std::fs::create_dir_all(&art).unwrap();
    let (width, height) = (100, 12);
    let mut digits = vault_fixtures::world::frm(width, height, 1);
    let pixels = digits.len() - usize::from(width) * usize::from(height);
    for (at, pixel) in digits[pixels..].iter_mut().enumerate() {
        *pixel = 1 + (at % usize::from(width) / 10) as u8;
    }
    std::fs::write(art.join("mvenum.frm"), digits).unwrap();
    let loose = LocalDirProvider::mount(layer.path()).unwrap();
    let vfs: Arc<dyn Vfs> = Arc::new(LayeredVfs::new(
        base,
        vec![ModLayer::new("mods/test", Some(loose), None, None)],
    ));
    let mut world = new_game_setup(
        vfs,
        "premade/synthetic.gcd",
        EnginePreset::Vanilla,
        |host| host.enable_sfall().unwrap(),
    );
    assert_eq!(host_profile(&world), GameProfile::Fallout2);
    world.tick(16).unwrap();
    Game {
        world,
        _install: install,
        _layer: layer,
    }
}
fn host_profile(world: &MapPreview) -> GameProfile {
    use vault_core::assets::WorldAssets;
    world.host.assets.game_profile()
}
fn global(world: &MapPreview, key: i32) -> i32 {
    world
        .host
        .world
        .sfall
        .as_ref()
        .unwrap()
        .globals
        .get(&GlobalKey::Int(key))
        .map_or(0, SfallValue::int)
}
fn set_global(world: &mut MapPreview, key: i32, value: i32) {
    let state = world.host.world.sfall.as_mut().unwrap();
    state
        .globals
        .insert(GlobalKey::Int(key), SfallValue::Int(value));
}
fn item(world: &mut MapPreview, kind: usize, quantity: i32) -> ObjectRef {
    let player = world.host.world.player;
    let mut bytes = vault_fixtures::world::prototype(0, kind);
    vault_fixtures::world::word(&mut bytes, 0, 0x40 + kind as i32);
    let prototype = Arc::new(vault_formats::Prototype::parse(&bytes).unwrap());
    let item = world.host.world.spawn(prototype, 0, 0).unwrap();
    world
        .host
        .world
        .add_item(player, item, quantity, true)
        .unwrap();
    item
}
/// The inventory is open, and the hook answers global 2's value.
fn inventory(ret: i32) -> Game {
    let mut body = recording(3);
    body.extend([I(2), W(GLOBAL_INT), W(SET_RETURN)]);
    let mut game = game(INVENTORYMOVE, &body);
    set_global(&mut game.world, 2, ret);
    game.world.image().unwrap();
    assert!(game.world.step(KeyCode::KeyI));
    assert!(matches!(game.world.screen, Screen::Inventory(_)));
    game
}
fn empty_hands(world: &mut MapPreview) {
    let player = world.host.world.player;
    let carried = world.host.world.get(player).unwrap().inventory.clone();
    for (item, quantity) in carried {
        world.host.world.drop_item(player, item, quantity).unwrap();
    }
}
/// What the hook saw last: target, item, replaced, and how often it ran.
fn seen(world: &MapPreview) -> [i32; 4] {
    [
        global(world, 10),
        global(world, 11),
        global(world, 12),
        global(world, 1),
    ]
}
/// How many items the player carries, by stack quantity.
fn carried(world: &MapPreview) -> i32 {
    let player = world.host.world.player;
    let inventory = &world.host.world.get(player).unwrap().inventory;
    inventory.iter().map(|(_, quantity)| *quantity).sum()
}

#[test]
fn inventory_move_gives_each_target_its_code_and_the_item_it_replaces() {
    const LIST: DropTarget = DropTarget::List(0);
    // Target, the kind of item at the target, the code, whether it replaces.
    // Prototype kinds: 1 container, 3 weapon, 4 ammunition, 5 misc.
    let cases: [(DropTarget, Option<usize>, usize, i32, bool); 8] = [
        (LIST, None, 5, 0, false),
        (DropTarget::LeftHand, None, 5, 1, false),
        (DropTarget::RightHand, None, 5, 2, false),
        (DropTarget::Armor, None, 5, 3, false),
        (DropTarget::Figure, None, 5, 8, false),
        (DropTarget::RightHand, Some(3), 4, 4, true),
        (DropTarget::RightHand, Some(1), 5, 5, true),
        (DropTarget::LeftHand, Some(5), 5, 1, true),
    ];
    for (target, occupant, kind, code, replaces) in cases {
        let mut game = inventory(1);
        let world = &mut game.world;
        empty_hands(world);
        let player = world.host.world.player;
        let lifted = item(world, kind, 1);
        let other = occupant.map(|kind| {
            let other = item(world, kind, 1);
            let flag = match target {
                DropTarget::LeftHand => vault_formats::pro::BasicFlags::LEFT_HAND,
                _ => vault_formats::pro::BasicFlags::RIGHT_HAND,
            };
            world.host.world.equip(player, other, flag.0).unwrap();
            other
        });
        world.inventory_drop(inventory::Slot::List, lifted, target);
        let replaced = other.filter(|_| replaces).map_or(0, |other| other as i32);
        assert_eq!(
            seen(world),
            [code, lifted as i32, replaced, 1],
            "{target:?}"
        );
    }
}

#[test]
fn inventory_move_sees_a_list_row_it_lands_on() {
    let mut game = inventory(1);
    let world = &mut game.world;
    empty_hands(world);
    // A bag on the row: the item goes in, so its code is the container's.
    let bag = item(world, 1, 1);
    let lifted = item(world, 5, 1);
    world.inventory_drop(inventory::Slot::List, lifted, DropTarget::List(0));
    assert_eq!(seen(world), [5, lifted as i32, bag as i32, 1]);
}

#[test]
fn a_drop_on_nowhere_runs_no_chain() {
    let mut game = inventory(1);
    let lifted = item(&mut game.world, 5, 1);
    game.world
        .inventory_drop(inventory::Slot::List, lifted, DropTarget::Nowhere);
    assert_eq!(seen(&game.world)[3], 0);
}

#[test]
fn a_prevented_move_leaves_the_item_and_minus_1_lets_the_engine_handle_it() {
    let right = |world: &MapPreview| {
        world
            .host
            .world
            .wielded(world.host.world.player, hit_mode::RIGHT_PRIMARY)
    };
    let mut game = inventory(0);
    empty_hands(&mut game.world);
    let lifted = item(&mut game.world, 5, 1);
    game.world
        .inventory_drop(inventory::Slot::List, lifted, DropTarget::RightHand);
    assert_eq!(right(&game.world), None, "any other value prevents it");

    let mut game = inventory(-1);
    empty_hands(&mut game.world);
    let lifted = item(&mut game.world, 5, 1);
    game.world
        .inventory_drop(inventory::Slot::List, lifted, DropTarget::RightHand);
    assert_eq!(right(&game.world), Some(lifted));
}

#[test]
fn a_drop_to_the_ground_asks_once_per_item_and_leaves_only_what_it_allows() {
    let mut game = inventory(-1);
    let world = &mut game.world;
    empty_hands(world);
    let stack = item(world, 5, 3);
    world.hooked_drop(stack, 3);
    assert_eq!(seen(world), [6, stack as i32, 0, 3]);
    assert_eq!(carried(world), 0);

    // Refused from the third run on: one stays.
    let mut body = recording(3);
    // (runs > 2) * 2 - 1: -1 lets the first two go, and 1 refuses.
    body.extend([
        I(1),
        W(GLOBAL_INT),
        I(2),
        W(GREATER),
        I(2),
        W(MULTIPLY),
        I(1),
        W(SUBTRACT),
        W(SET_RETURN),
    ]);
    let mut game = self::game(INVENTORYMOVE, &body);
    let world = &mut game.world;
    world.image().unwrap();
    assert!(world.step(KeyCode::KeyI));
    empty_hands(world);
    let stack = item(world, 5, 3);
    world.hooked_drop(stack, 3);
    assert_eq!(seen(world)[3], 3);
    assert_eq!(carried(world), 1);
}

#[test]
fn caps_ask_once_however_many_drop_and_a_refusal_keeps_them_all() {
    for (ret, dropped) in [(-1, true), (0, false)] {
        let mut game = inventory(ret);
        let world = &mut game.world;
        empty_hands(world);
        let caps = item(world, 5, 100);
        world.host.world.get_mut(caps).unwrap().money = Some(100);
        world.hooked_drop(caps, 100);
        assert_eq!(seen(world)[3], 1);
        assert_eq!(carried(world), if dropped { 0 } else { 100 });
    }
}

/// The player's hex, a hex the player can walk to, and that path's length.
fn walkable(world: &mut MapPreview) -> (i32, i32) {
    let player = world.host.world.player;
    let from = world.host.world.get(player).unwrap().raw.hex.unwrap();
    [
        from - 4,
        from + 4,
        from - 400,
        from + 400,
        from - 2,
        from + 2,
    ]
    .into_iter()
    .find_map(|hex| {
        let plan = world.host.plan_to_tile(player, hex, usize::MAX);
        plan.map(|plan| (hex, plan.directions.len() as i32))
            .filter(|(_, length)| *length > 0)
    })
    .expect("a walkable hex")
}
fn point_of(world: &MapPreview, hex: i32) -> [i32; 2] {
    let [x, y] = world.view.grid.hex_center(hex).unwrap();
    [x as i32, y as i32 - 5]
}
fn fighting(world: &mut MapPreview, ap: i32, free: i32) {
    let player = world.host.world.player;
    world.host.world.combat = true;
    world.host.world.free_movement = free;
    world.host.world.critter_state_mut(player).unwrap().ap = ap;
    world.hud.mode = cursor::Mode::Movement;
}

#[test]
fn the_movement_badge_shows_the_hooked_cost_less_the_free_pool() {
    let mut body = recording(3);
    body.extend([I(9), W(SET_RETURN)]);
    let mut game = game(MOVECOST, &body);
    let world = &mut game.world;
    let (hex, hexes) = walkable(world);
    let at = point_of(world, hex);
    fighting(world, 20, 2);
    let feedback = world.hover_feedback(at);
    assert_eq!(
        feedback,
        Some(pointer::Feedback::Cost(b"7".to_vec(), cursor::COST_COLOR))
    );
    // The cursor draws that cost.
    world.hover(at);
    world.pointer.feedback = feedback.clone();
    let hooked = world.pointer_frame().unwrap();
    world.pointer.feedback = Some(pointer::Feedback::Cost(b"1".to_vec(), cursor::COST_COLOR));
    assert_ne!(world.pointer_frame().unwrap().pixels(), hooked.pixels());
    // A planning query: the hexes, and their cost at the per-hex rate.
    assert_eq!(global(world, 11), hexes);
    assert_eq!(global(world, 12), hexes);
    assert_eq!(global(world, 1), 1);
    // The same hover runs nothing more.
    assert_eq!(world.hover_feedback(at), feedback);
    assert_eq!(global(world, 1), 1);
    // More than the AP cover is a red `X`.
    world.host.world.free_movement = 0;
    let player = world.host.world.player;
    world.host.world.critter_state_mut(player).unwrap().ap = 8;
    assert_eq!(
        world.hover_feedback(at),
        Some(pointer::Feedback::Cost(
            b"X".to_vec(),
            cursor::BLOCKED_COLOR
        ))
    );
    // Crippled legs raise the rate the query is given.
    let flags = &mut world.host.world.critter_state_mut(player).unwrap().results;
    flags.0 |= 0x04;
    world.hover_feedback(at);
    assert_eq!(global(world, 12), hexes * 4);
}

#[test]
fn the_movement_badge_is_the_plain_cost_with_no_return_and_out_of_combat_no_chain_runs() {
    let mut game = game(MOVECOST, &recording(3));
    let world = &mut game.world;
    let (hex, hexes) = walkable(world);
    let at = point_of(world, hex);
    world.hud.mode = cursor::Mode::Movement;
    assert_eq!(world.hover_feedback(at), None);
    assert_eq!(global(world, 1), 0, "movement costs no AP here");
    fighting(world, 20, 0);
    assert_eq!(
        world.hover_feedback(at),
        Some(pointer::Feedback::Cost(
            hexes.to_string().into_bytes(),
            cursor::COST_COLOR
        ))
    );
    assert_eq!(global(world, 1), 1);
}

#[test]
fn the_bar_shows_the_hooked_ap_cost_once_a_step_has_queried_it() {
    let mut body = recording(5);
    body.extend([I(6), W(SET_RETURN)]);
    let mut game = game(CALCAPCOST, &body);
    let world = &mut game.world;
    let (mode, aimed) = world.slot_attack().expect("the slot attacks");
    world.tick(32).unwrap();
    assert_eq!(world.slot_attack_cost(mode, aimed), 6);
    let runs = global(world, 1);
    assert!(runs >= 1);
    // A step with the same inputs asks nothing, and drawing never does.
    world.tick(48).unwrap();
    world.image().unwrap();
    assert_eq!(global(world, 1), runs);
    assert_eq!(world.slot_attack_cost(mode, aimed), 6);
    // The bar draws that cost: the same game with no return draws another.
    let hooked = world.image().unwrap();
    let mut plain = self::game(CALCAPCOST, &recording(5));
    plain.world.tick(32).unwrap();
    assert_ne!(plain.world.image().unwrap().pixels(), hooked.pixels());
    // Other inputs show the unhooked cost until a step queries them.
    let other = u8::from(mode == 0);
    let plain = {
        let world = &world.host.world;
        world
            .attack_profile(world.player, world.player, other, 3, aimed)
            .map(|attack| attack.ap_cost())
    };
    if let Ok(plain) = plain {
        assert_eq!(world.slot_attack_cost(other, aimed), plain);
    }
}

#[test]
fn the_crosshair_shows_the_hooked_chance_in_the_step_after_the_hover() {
    let mut game = game(TOHIT, &[I(77), W(SET_RETURN)]);
    let world = &mut game.world;
    let target = super::test_support::npc(world);
    let player = world.host.world.player;
    world.host.world.combat = true;
    world.host.world.critter_state_mut(player).unwrap().ap = 10;
    world.hud.mode = cursor::Mode::Crosshair;
    world.pointer.looked = Some(target);
    world.tick(32).unwrap();
    assert_eq!(world.hover_hit_chance(target), Some(77));
    let hex = world.host.world.get(target).unwrap().raw.hex.unwrap();
    let at = point_of(world, hex);
    let feedback = world.hover_feedback(at);
    let expected = cursor::hit_text(Some(77));
    assert!(
        matches!(&feedback, Some(pointer::Feedback::Hit(text, _)) if *text == expected),
        "{feedback:?}"
    );
    // The cursor draws what the hover holds.
    world.hover(at);
    world.pointer.feedback = feedback;
    let hooked = world.pointer_frame().unwrap();
    world.pointer.feedback = Some(pointer::Feedback::Hit(
        cursor::hit_text(Some(12)),
        cursor::COST_COLOR,
    ));
    assert_ne!(world.pointer_frame().unwrap().pixels(), hooked.pixels());
}

#[test]
fn the_barter_window_shows_the_stored_price_and_offer() {
    let body = [I(90), W(SET_RETURN), I(200), W(SET_RETURN)];
    let mut game = game(BARTERPRICE, &body);
    let world = &mut game.world;
    let merchant = super::test_support::npc(world);
    let session = world.host.begin_barter(merchant).unwrap();
    let plain = world.host.barter_price(&session);
    world.screen = Screen::Barter(Box::new(barter::BarterScreen::new(session, false)));
    assert_eq!(
        world.barter_shown_price(),
        plain,
        "before a step, the plain price"
    );
    let unhooked = world.barter_image().unwrap();
    world.tick(32).unwrap();
    assert_eq!(world.barter_shown_price(), 90);
    assert_eq!(world.barter_offer_value(), 200);
    assert_ne!(plain, 90);
    // The stored totals are what is drawn.
    assert_ne!(world.barter_image().unwrap().pixels(), unhooked.pixels());
}

#[test]
fn the_screens_report_their_game_mode_bits() {
    let mut game = inventory(-1);
    let world = &mut game.world;
    world.tick(32).unwrap();
    let list = world.host.world.get(world.host.world.player).unwrap();
    assert!(list.character.is_some());
    assert_eq!(world.sfall_screens(), game_mode::INVENTORY);
}

/// The literals of [`spoken`]: `message_box`, `hi` and `signal_close_game`.
const STRINGS: [&[u8]; 3] = [b"message_box", b"hi", b"signal_close_game"];
const MESSAGE_BOX: u32 = 6;
const BOX_TEXT: u32 = MESSAGE_BOX + 14;
const CLOSE_GAME: u32 = BOX_TEXT + 6;
const SET_REPEAT: u16 = 0x816a;
const TAP_KEY: u16 = 0x8162;
const FUNC0: u16 = 0x8276;
const FUNC1: u16 = 0x8277;
/// A global script whose `start` is `body`, and which `start` makes repeat
/// every step.
fn spoken(body: Vec<u8>) -> Game {
    let mut start = body;
    start.extend(bytes(&[I(1), W(SET_REPEAT)]));
    with_script(fixture::corpus_program(
        &STRINGS,
        &[(b"start", fixture::returning(start))],
    ))
}
fn message_box(global: i32) -> Vec<u8> {
    let mut body = fixture::push(global);
    body.extend(fixture::literal(MESSAGE_BOX));
    body.extend(fixture::literal(BOX_TEXT));
    body.extend(fixture::instruction(FUNC1));
    body.extend(fixture::instruction(SET_GLOBAL));
    body
}

#[test]
fn message_box_is_modal_and_its_key_gives_the_script_the_result() {
    let mut game = spoken(message_box(5));
    assert_eq!(global(&game.world, 5), -1, "session setup cannot wait");
    // The repeat opens the box, and the next step shows it.
    game.world.tick(32).unwrap();
    game.world.tick(48).unwrap();
    assert!(game.world.dialog.is_some());
    let before = game.world.host.world.clock.ticks;
    game.world.tick(64).unwrap();
    assert_eq!(game.world.host.world.clock.ticks, before, "the world waits");
    assert_eq!(global(&game.world, 5), -1, "no result yet");
    game.world.image().unwrap();
    assert!(game.world.step(KeyCode::Enter));
    assert!(game.world.dialog.is_none());
    assert_eq!(global(&game.world, 5), 1, "Enter is Yes or Done");
    assert!(game.world.host.sfall_open_message_box().is_none());
    // The next repeat opens another, and Escape is No.
    game.world.tick(80).unwrap();
    game.world.tick(96).unwrap();
    assert!(game.world.step(KeyCode::Escape));
    assert_eq!(global(&game.world, 5), 0);
}

#[test]
fn a_tapped_key_reaches_the_screen_at_the_next_input_phase() {
    let mut game = spoken(bytes(&[I(0x17), W(TAP_KEY)]));
    game.world.image().unwrap();
    game.world.tick(32).unwrap();
    assert!(matches!(game.world.screen, Screen::Inventory(_)));
}

#[test]
fn signal_close_game_ends_the_session_with_no_prompt() {
    let mut body = fixture::push(5);
    body.extend(fixture::literal(CLOSE_GAME));
    body.extend(fixture::instruction(FUNC0));
    body.extend(fixture::instruction(SET_GLOBAL));
    let game = spoken(body);
    assert!(game.world.quit_requested);
    assert!(game.world.dialog.is_none());
}
