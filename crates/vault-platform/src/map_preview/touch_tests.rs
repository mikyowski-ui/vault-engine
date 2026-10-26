//! The touch UI's pill bar over the synthetic corpus: its layout, when it
//! draws, and that each button does what its key does.
use super::super::{
    party::{MAX_PILLS, pill},
    test_support::{interactive, synthetic_corpus},
};
use super::*;
use crate::enhanced::{EnhancedSettings, PointerAt, PointerButton, RoofPeek, ViewportMode};
use vault_render::EnhancedLayout;

/// The synthetic world, with the touch UI on or off.
fn preview(dir: &std::path::Path, touch: bool) -> MapPreview {
    let mut preview = interactive(dir);
    preview.set_touch_ui(touch);
    preview.tick(16).unwrap();
    preview
}
fn center(rect: Rect) -> [i32; 2] {
    [rect.x + rect.w / 2, rect.y + rect.h / 2]
}
fn rect_of(preview: &MapPreview, button: BarButton) -> Rect {
    preview
        .touch_bar()
        .into_iter()
        .find(|(shown, _)| *shown == button)
        .map(|(_, rect)| rect)
        .unwrap_or_else(|| panic!("{button:?} is not shown"))
}
fn pixel(image: &IndexedImage, [x, y]: [i32; 2]) -> u8 {
    image.pixels()[(y * image.width() as i32 + x) as usize]
}
fn overlaps(a: Rect, b: Rect) -> bool {
    a.x < b.x + b.w && b.x < a.x + a.w && a.y < b.y + b.h && b.y < a.y + a.h
}

#[test]
fn the_bar_is_thumb_sized_inside_the_frame_and_clear_of_the_party_pills() {
    for peek in [true, false] {
        let bar = layout(peek);
        let names: Vec<_> = bar.iter().map(|(button, _)| *button).collect();
        let expected: Vec<_> = BarButton::ALL
            .into_iter()
            .filter(|button| peek || *button != BarButton::Peek)
            .collect();
        assert_eq!(names, expected);
        for (k, (_, rect)) in bar.iter().enumerate() {
            assert!(rect.w >= MIN_TARGET && rect.h >= MIN_TARGET, "{rect:?}");
            // Inside the frame, above the indicator boxes (y 358) and the
            // bar, in the top-right corner.
            assert!(
                rect.x >= 0 && rect.x + rect.w <= 640 && rect.y >= 0,
                "{rect:?}"
            );
            assert!(rect.y + rect.h < 358, "{rect:?}");
            assert_eq!(rect.y, 12);
            // Clear of every party pill, and of each other.
            for slot in 0..MAX_PILLS {
                assert!(!overlaps(*rect, pill(slot)), "{rect:?} and pill {slot}");
            }
            for (_, other) in &bar[k + 1..] {
                assert!(!overlaps(*rect, *other));
                assert!(other.x - (rect.x + rect.w) >= GAP);
            }
        }
        let last = bar.last().unwrap().1;
        assert_eq!(last.x + last.w, 636, "right-aligned");
        assert!(bar[0].1.x > pill(0).x + pill(0).w);
    }
}

#[test]
fn the_bar_draws_and_takes_clicks_only_with_the_touch_ui_on() {
    let dir = synthetic_corpus();
    let mut off = preview(dir.path(), false);
    let plain = off.image().unwrap();
    assert!(off.touch_bar().is_empty());
    let escape = layout(false)[4].1;
    assert_eq!(off.touch_button_at(center(escape)), None);
    assert!(!off.touch_bar_press(center(escape)));
    // On: drawn amber on black over the map, without PEEK in Vanilla.
    let mut on = preview(dir.path(), true);
    let bar = on.touch_bar();
    assert_eq!(bar, layout(false));
    let drawn = on.image().unwrap();
    assert_ne!(drawn.pixels(), plain.pixels());
    let amber = ColorKey::AMBER.resolve(&on.palette);
    let black = ColorKey::BLACK.resolve(&on.palette);
    let mode = rect_of(&on, BarButton::Mode);
    assert_eq!(pixel(&drawn, [mode.x + 1, mode.y]), amber);
    assert_eq!(pixel(&drawn, [mode.x + 4, mode.y + 4]), black);
    // Off again: the Vanilla frame exactly.
    on.set_touch_ui(false);
    assert_eq!(on.image().unwrap().pixels(), plain.pixels());
    on.set_touch_ui(true);
    // A held button draws inverted until its release.
    assert!(on.touch_bar_press(center(mode)));
    let held = on.image().unwrap();
    assert_eq!(pixel(&held, [mode.x + 4, mode.y + 4]), amber);
    assert!(on.release(Some([10, 300]), false), "released elsewhere");
    assert!(!on.touch_bar_held());
    assert_eq!(on.image().unwrap().pixels(), drawn.pixels());
    // Not over the debug overlay's panel, a window or a conversation.
    on.set_debug(true);
    assert!(on.touch_bar().is_empty());
    on.set_debug(false);
    assert!(on.tap(center(rect_of(&on, BarButton::Inventory))));
    assert!(matches!(on.screen, Screen::Inventory(_)));
    assert!(on.touch_bar().is_empty());
    assert_eq!(on.touch_button_at(center(escape)), None);
}

/// Each button against its key: two worlds with the touch UI on, one
/// tapping the button and one pressing and releasing the key, end the
/// same.
#[test]
fn each_button_does_what_its_key_does() {
    let dir = synthetic_corpus();
    for (button, key) in [
        (BarButton::Inventory, KeyCode::KeyI),
        (BarButton::PipBoy, KeyCode::KeyP),
        (BarButton::Escape, KeyCode::Escape),
        (BarButton::Turn, KeyCode::Space),
    ] {
        let mut tapped = preview(dir.path(), true);
        let mut typed = preview(dir.path(), true);
        assert!(tapped.tap(center(rect_of(&tapped, button))), "{button:?}");
        typed.step(key);
        typed.key_up(key);
        assert_eq!(
            std::mem::discriminant(&tapped.screen),
            std::mem::discriminant(&typed.screen),
            "{button:?}"
        );
        assert_eq!(tapped.title(), typed.title(), "{button:?}");
        assert_eq!(
            tapped.image().unwrap().pixels(),
            typed.image().unwrap().pixels(),
            "{button:?}"
        );
    }
    // INV and ESC open their windows.
    let mut world = preview(dir.path(), true);
    assert!(world.tap(center(rect_of(&world, BarButton::Escape))));
    assert!(matches!(world.screen, Screen::Options(_)));
    // MODE is a right click in the map: the cursor mode cycles.
    let mut tapped = preview(dir.path(), true);
    let mut clicked = preview(dir.path(), true);
    let before = tapped.hud.mode;
    assert!(tapped.tap(center(rect_of(&tapped, BarButton::Mode))));
    assert!(clicked.click([320, 200], true));
    clicked.release(Some([320, 200]), true);
    assert_ne!(tapped.hud.mode, before);
    assert_eq!(tapped.hud.mode, clicked.hud.mode);
    // A release away from the button pressed does nothing.
    let mut world = preview(dir.path(), true);
    let inventory = rect_of(&world, BarButton::Inventory);
    assert!(world.click(center(inventory), false));
    let escape = rect_of(&world, BarButton::Escape);
    assert!(world.release(Some(center(escape)), false));
    assert!(matches!(world.screen, Screen::Map));
}

/// Under the combat lock the keyboard is not read: the buttons' keys are
/// held like typed ones. TURN sends Space, or Enter when held 500 ms.
#[test]
fn turn_is_space_and_held_it_is_enter_under_the_combat_lock_as_typed() {
    let dir = synthetic_corpus();
    let lock = |preview: &mut MapPreview| {
        preview.image().unwrap();
        preview.host.world.combat = true;
        preview.hud.panel = super::super::hud::Panel::settled(true, false);
        assert!(preview.keyboard_locked());
    };
    let mut tapped = preview(dir.path(), true);
    let mut typed = preview(dir.path(), true);
    lock(&mut tapped);
    lock(&mut typed);
    let turn = center(rect_of(&tapped, BarButton::Turn));
    // A short press: Space.
    assert!(tapped.click(turn, false));
    tapped.clock += LONG_PRESS_MS - 1;
    assert!(tapped.release(Some(turn), false));
    // A long one: Enter.
    assert!(tapped.click(turn, false));
    tapped.clock += LONG_PRESS_MS;
    assert!(tapped.release(Some(turn), false));
    for key in [KeyCode::Space, KeyCode::Enter] {
        typed.step(key);
        typed.key_up(key);
    }
    assert_eq!(tapped.hud.held_keys, typed.hud.held_keys);
    let modifiers = [false; 3];
    assert_eq!(
        Vec::from(tapped.hud.held_keys.clone()),
        [
            super::super::hud::HeldKey::Press(KeyCode::Space, modifiers),
            super::super::hud::HeldKey::Release,
            super::super::hud::HeldKey::Press(KeyCode::Enter, modifiers),
            super::super::hud::HeldKey::Release,
        ]
    );
}

/// The enhanced unlocked view: the bar is part of the interface frame,
/// reached through the shell's enhanced routing, and PEEK is Alt+R.
#[test]
fn peek_cycles_the_roof_peek_as_alt_r_does_in_the_unlocked_view() {
    let dir = synthetic_corpus();
    let unlocked = |touch: bool| {
        let mut preview = interactive(dir.path());
        preview.set_enhanced(Some(EnhancedSettings {
            smooth_camera: false,
            ..Default::default()
        }));
        preview.set_touch_ui(touch);
        preview.tick(16).unwrap();
        preview
    };
    let mut tapped = unlocked(true);
    let layout = EnhancedLayout::compute([1280, 960], None, tapped.enhanced_zoom()).unwrap();
    let frame = tapped.enhanced_frame(&layout).unwrap().unwrap();
    let bar = tapped.touch_bar();
    assert_eq!(bar, super::layout(true));
    // Drawn on the interface frame, opaque over the world.
    let peek = rect_of(&tapped, BarButton::Peek);
    let amber = ColorKey::AMBER.resolve(&tapped.palette);
    assert_eq!(pixel(&frame.ui, [peek.x + 1, peek.y]), amber);
    assert_ne!(frame.ui_transparent, Some(amber));
    // The shell's pointer at the button's middle.
    let scale = f64::from(layout.ui_scale);
    let [px, py] = center(peek);
    let point = [
        f64::from(layout.ui_rect[0]) + (f64::from(px) + 0.5) * scale,
        f64::from(layout.ui_rect[1]) + (f64::from(py) + 0.5) * scale,
    ];
    let at = PointerAt {
        ui: layout.ui_pixel(point),
        world: layout.world_point(point),
        side: layout.in_side_panel(point),
    };
    assert_eq!(at.ui, Some([px, py]));
    tapped.enhanced_pointer(at);
    assert!(tapped.enhanced_button(at, PointerButton::Left, true));
    assert!(tapped.touch_bar_held());
    assert!(tapped.enhanced_button(at, PointerButton::Left, false));
    let mut typed = unlocked(true);
    typed.set_modifiers([false, false, true]);
    assert!(typed.step(KeyCode::KeyR));
    typed.set_modifiers([false; 3]);
    let roof = |preview: &MapPreview| preview.enhanced().unwrap().roof_peek;
    assert_eq!(roof(&tapped), RoofPeek::CursorXray);
    assert_eq!(roof(&tapped), roof(&typed));
    assert_eq!(tapped.hud.modifiers, [false; 3], "Alt is not left held");
    // The fixed viewport has no peek: the button hides.
    tapped.set_enhanced(Some(EnhancedSettings {
        viewport: ViewportMode::Fixed,
        ..Default::default()
    }));
    assert_eq!(tapped.touch_bar(), super::layout(false));
}
