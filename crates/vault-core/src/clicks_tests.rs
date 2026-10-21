//! SPEC-014 §3.26-§3.27 the click policy while sequences play.
use crate::clicks::{
    Click, ClickState, Policy, busy_policy, click_policy, input_locked, interrupt_walk,
};

fn state(busy: bool, walking: bool, standing: bool) -> ClickState {
    ClickState {
        combat: false,
        player_turn: false,
        presenting: false,
        busy,
        walking,
        standing,
    }
}

#[test]
fn spec014_clicks_while_the_player_is_busy_cancel_or_do_nothing() {
    let idle = state(false, false, true);
    let walking = state(true, true, false);
    let reaching = state(true, false, false);
    // A movement click interrupts a busy player, whatever it is doing,
    // only with `interrupt_walk` on; off, it does nothing. Nothing queues.
    for interrupt in [false, true] {
        assert_eq!(
            busy_policy(Click::Movement, &idle, interrupt),
            Policy::Proceed
        );
    }
    assert_eq!(busy_policy(Click::Movement, &walking, true), Policy::Cancel);
    assert_eq!(
        busy_policy(Click::Movement, &reaching, true),
        Policy::Cancel
    );
    assert_eq!(
        busy_policy(Click::Movement, &walking, false),
        Policy::Ignore
    );
    assert_eq!(
        busy_policy(Click::Movement, &reaching, false),
        Policy::Ignore
    );
    // An action cancels only a walking or running player, by its art; in
    // any other pose it does nothing.
    assert_eq!(busy_policy(Click::Action, &idle, false), Policy::Proceed);
    assert_eq!(busy_policy(Click::Action, &walking, false), Policy::Cancel);
    assert_eq!(busy_policy(Click::Action, &reaching, true), Policy::Ignore);
    // A near talk is callbacks only and registers while busy; rotating
    // needs the standing art.
    assert_eq!(
        busy_policy(Click::NearTalk, &reaching, false),
        Policy::Proceed
    );
    assert_eq!(busy_policy(Click::Rotate, &idle, true), Policy::Proceed);
    assert_eq!(busy_policy(Click::Rotate, &walking, true), Policy::Ignore);
    assert_eq!(busy_policy(Click::Rotate, &reaching, true), Policy::Ignore);
    // Standing inside a sequence: an action does nothing, rotating works.
    let standing = state(true, false, true);
    assert_eq!(busy_policy(Click::Action, &standing, true), Policy::Ignore);
    assert_eq!(busy_policy(Click::Rotate, &standing, true), Policy::Proceed);
}

#[test]
fn spec014_combat_locks_input_outside_the_player_turn_and_while_sequences_play() {
    let mut combat = state(false, false, true);
    combat.combat = true;
    assert!(input_locked(&combat), "not the player's turn");
    combat.player_turn = true;
    assert!(!input_locked(&combat));
    combat.presenting = true;
    assert!(
        input_locked(&combat),
        "until every running sequence has ended"
    );
    assert_eq!(click_policy(Click::Movement, &combat, true), Policy::Ignore);
    assert_eq!(click_policy(Click::Rotate, &combat, true), Policy::Ignore);
    combat.presenting = false;
    assert_eq!(click_policy(Click::Rotate, &combat, true), Policy::Proceed);
    // In combat a movement click never interrupts a busy player.
    let busy = ClickState {
        busy: true,
        walking: true,
        standing: false,
        ..combat
    };
    assert_eq!(busy_policy(Click::Movement, &busy, true), Policy::Ignore);
    assert_eq!(click_policy(Click::Action, &busy, true), Policy::Cancel);
    // Out of combat nothing is locked.
    assert!(!input_locked(&state(true, true, false)));
}

#[test]
fn spec014_interrupt_walk_is_a_system_key_defaulting_to_on() {
    let parse = |text: &str| interrupt_walk(&vault_formats::cfg::ConfigFile::parse(text));
    assert!(parse(""));
    assert!(parse("[system]\ninterrupt_walk=1\n"));
    assert!(!parse("[system]\ninterrupt_walk=0\n"));
    assert!(parse("[preferences]\ninterrupt_walk=0\n"));
}
