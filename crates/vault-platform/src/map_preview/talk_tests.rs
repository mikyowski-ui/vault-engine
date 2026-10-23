//! SPEC-014 §3.45 the dialogue panel, Review and Ask About windows over a
//! synthetic conversation.
use super::{TalkWindow, ask_buttons};
use crate::map_preview::{MapPreview, Screen, state_fixture};
use std::sync::Arc;
use vault_core::{WorldEffect, ui::cursor::Mode, ui::talk};
use vault_formats::{LocalDirProvider, Vfs};
use winit::keyboard::KeyCode;

/// The synthetic conversation ("Hello" with option "Continue"), its
/// script's message file (list 1) with one fallback answer, and
/// `proto.msg`.
fn talking() -> (tempfile::TempDir, MapPreview) {
    let dir = tempfile::tempdir().unwrap();
    state_fixture::write_corpus(dir.path());
    std::fs::create_dir_all(dir.path().join("text/english/dialog")).unwrap();
    std::fs::write(
        dir.path().join("text/english/dialog/synthetic.msg"),
        b"{980}{}{Hng.}\n",
    )
    .unwrap();
    std::fs::write(
        dir.path().join("text/english/game/proto.msg"),
        b"{903}{}{This person will not barter with you.}\n\
          {904}{}{This person has nothing to tell you about.}\n",
    )
    .unwrap();
    let vfs: Arc<dyn Vfs> = Arc::new(LocalDirProvider::mount(dir.path()).unwrap());
    let mut preview = MapPreview::load(
        vfs,
        "maps/synthetic.map",
        "english",
        Some("premade/synthetic.gcd"),
        1,
    )
    .unwrap();
    preview.image().unwrap();
    preview.hud.mode = Mode::Arrow;
    assert!(preview.tap([336, 170]));
    assert!(preview.host.world.dialogue.open && preview.host.world.dialogue.waiting);
    (dir, preview)
}

/// A press and release in the middle of a screen rectangle.
fn press(preview: &mut MapPreview, rect: vault_core::ui::Rect) {
    assert!(preview.tap([rect.x + rect.w / 2, rect.y + rect.h / 2]));
}

#[test]
fn spec014_review_window() {
    let (_dir, mut preview) = talking();
    // The option's callback builds the same node again: two entries.
    preview.host.choose_dialogue(Some(0)).unwrap();
    assert_eq!(preview.host.world.dialogue.review.len(), 2);
    preview.image().unwrap();
    preview.host.world.effects.clear();
    // No key opens Review; its button does, stopping speech and the head
    // updater.
    preview.host.world.dialogue.speech = Some(b"voice".to_vec());
    press(&mut preview, talk::panel_button(talk::REVIEW));
    assert!(matches!(
        preview.talk,
        Some(TalkWindow::Review(ref review)) if review.top == 0
    ));
    assert!(!preview.host.world.dialogue.updater);
    assert!(preview.host.world.dialogue.speech.is_none());
    preview.image().unwrap();
    // Down stops at the last entry; Up stops at entry 0; other keys are
    // read and ignored, digits included.
    let top = |preview: &MapPreview| match &preview.talk {
        Some(TalkWindow::Review(review)) => review.top,
        _ => panic!("Review is open"),
    };
    assert!(preview.step(KeyCode::ArrowDown));
    assert!(preview.step(KeyCode::ArrowDown));
    assert_eq!(top(&preview), 1);
    press(&mut preview, talk::SCROLL_UP.0);
    assert_eq!(top(&preview), 0);
    assert!(preview.step(KeyCode::Digit1));
    assert!(preview.host.world.dialogue.waiting);
    assert!(preview.step(KeyCode::KeyQ));
    // Done closes it and the updater resumes.
    press(&mut preview, talk::DONE.0);
    assert!(preview.talk.is_none());
    assert!(preview.host.world.dialogue.updater);
    assert!(preview.host.world.dialogue.open);
    press(&mut preview, talk::panel_button(talk::REVIEW));
    assert!(preview.step(KeyCode::Escape));
    assert!(preview.talk.is_none() && preview.host.world.dialogue.open);
}

#[test]
fn spec014_tell_me_about_and_ask_about_window() {
    let (_dir, mut preview) = talking();
    let target = preview.host.world.dialogue.target;
    let sid = preview.host.world.script_for(target).unwrap();
    let base = preview.host.world.ensure_locals(sid).unwrap();
    // Reaction 20: safe rule, `proto.msg` 904 in the reply area.
    preview.host.world.lvars[base..base + 4].copy_from_slice(&[20, 1, 1, 0]);
    press(&mut preview, talk::panel_button(talk::TELL_ME_ABOUT));
    assert!(preview.talk.is_none());
    assert_eq!(
        preview.host.world.dialogue.resolved_reply,
        b"This person has nothing to tell you about."
    );
    preview.host.world.dialogue.resolved_reply = b"Hello".to_vec();
    // A neutral reaction: `a` opens the window, hiding the reply and the
    // options, and stops speech (safe rule).
    preview.host.world.lvars[base] = 60;
    preview.host.world.dialogue.speech = Some(b"voice".to_vec());
    assert!(preview.step(KeyCode::KeyA));
    assert!(matches!(preview.talk, Some(TalkWindow::AskAbout(_))));
    assert!(preview.host.world.dialogue.speech.is_none());
    preview.image().unwrap();
    assert!(preview.choices.is_empty(), "the option window is hidden");
    assert_eq!(preview.host.world.dialogue.resolved_reply, b"Hello");
    // Typing: letters and punctuation; Up arrow enters nothing (safe
    // rule); Backspace removes; an empty submit does nothing.
    let typed = |preview: &MapPreview| match &preview.talk {
        Some(TalkWindow::AskAbout(ask)) => ask.field.text.clone(),
        _ => panic!("Ask About is open"),
    };
    assert!(preview.step(KeyCode::Enter));
    for key in [
        KeyCode::KeyX,
        KeyCode::KeyY,
        KeyCode::ArrowUp,
        KeyCode::KeyZ,
        KeyCode::Period,
        KeyCode::Backspace,
    ] {
        assert!(preview.step(key));
    }
    assert_eq!(typed(&preview), b"xyz");
    // The cursor blinks on polls more than 333 ms apart.
    let clock = preview.clock;
    assert!(preview.talk_tick(clock + 334));
    // Enter submits: no keyword, no name, the fallback answer 980 shows in
    // the reply window, and the field clears.
    assert!(preview.step(KeyCode::Enter));
    assert_eq!(typed(&preview), b"");
    assert_eq!(preview.host.world.dialogue.resolved_reply, b"Hng.");
    assert!(matches!(
        preview.talk,
        Some(TalkWindow::AskAbout(ref ask)) if ask.answered
    ));
    assert_eq!(preview.talk_hides(), (false, true));
    preview.image().unwrap();
    // CANCEL stops speech and closes: the saved reply and the options
    // return. Ask About replies are not in the review history.
    preview.host.world.dialogue.speech = Some(b"voice".to_vec());
    preview.host.world.effects.clear();
    let [_, cancel] = ask_buttons();
    press(&mut preview, cancel);
    assert!(preview.talk.is_none());
    assert!(preview.host.world.dialogue.speech.is_none());
    assert_eq!(preview.host.world.dialogue.resolved_reply, b"Hello");
    assert_eq!(preview.host.world.dialogue.review.len(), 1);
    preview.image().unwrap();
    assert_eq!(preview.choices.len(), 1);
    // Esc closes it too; the conversation stays open.
    assert!(preview.step(KeyCode::KeyA));
    assert!(preview.step(KeyCode::Escape));
    assert!(preview.talk.is_none() && preview.host.world.dialogue.open);
    assert!(
        !preview
            .host
            .world
            .effects
            .iter()
            .any(|effect| matches!(effect, WorldEffect::Speech(_)))
    );
}

#[test]
fn spec013_barter_key_and_button_refusal() {
    let (_dir, mut preview) = talking();
    // The synthetic merchant lacks prototype flag 0x02: `b` shows
    // `proto.msg` 903 and keeps the script modifier.
    preview.host.world.dialogue.barter_modifier = 7;
    assert!(preview.step(KeyCode::KeyB));
    assert_eq!(
        preview.host.world.dialogue.resolved_reply,
        b"This person will not barter with you."
    );
    assert_eq!(preview.host.world.dialogue.barter_modifier, 7);
    preview.host.world.dialogue.resolved_reply.clear();
    press(&mut preview, talk::panel_button(talk::BARTER));
    assert!(!preview.host.world.dialogue.resolved_reply.is_empty());
    assert!(!preview.host.world.dialogue.pending_barter);
    preview.image().unwrap();
    assert!(matches!(preview.screen, Screen::Map));
}
