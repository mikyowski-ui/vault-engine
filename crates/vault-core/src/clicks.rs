//! SPEC-014 §3.26-§3.27 clicks while sequences play: the pure policy for
//! a player's map click, action-menu entry, bar press or game hotkey. The
//! interface applies it before acting (showing the animated watch while
//! combat locks input), and the world applies its busy rules again as it
//! registers (`WorldHost::walk_toward_with` and the interaction
//! sequences). Nothing is ever queued: a click either cancels the player's
//! current sequence and starts its own, or does nothing. The rules come
//! from SPEC-007 §3.10.1 "One sequence per owner".

/// What the player asked for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Click {
    /// A movement-mode click on a hex (§3.27 "Movement").
    Movement,
    /// An arrow click or an action-menu entry: pick up, use, loot, a talk
    /// with an approach, or a skill use.
    Action,
    /// Talk out of combat at object distance below 9 with an unblocked
    /// line: callbacks only, so it registers even while the player is
    /// busy, and its dialogue request is made while the earlier animation
    /// plays.
    NearTalk,
    /// Rotate the player: an arrow click on the player, or the action
    /// menu's Rotate.
    Rotate,
}

/// What the click does.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Policy {
    /// Register the click's own sequence, or act at once.
    Proceed,
    /// Cancel the player's current sequence (SPEC-007 §3.10.1), then
    /// register the click's own.
    Cancel,
    /// Discard the click: no sequence, no message.
    Ignore,
}

/// What a click finds (`WorldHost::click_state`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct ClickState {
    pub combat: bool,
    /// In combat, whether it is the player's turn.
    pub player_turn: bool,
    /// An attack show is playing, or in combat any sequence still runs.
    pub presenting: bool,
    /// A sequence holding the player has not ended: a non-callback entry
    /// of the player's, started or not, in a sequence other than an idle
    /// fidget (`WorldHost::owns_motion`).
    pub busy: bool,
    /// The player's current art is walk (1) or run (19).
    pub walking: bool,
    /// The player's current art is standing (0).
    pub standing: bool,
}

/// §3.27: in combat, the first sequence registered on the player's turn
/// disables the game mouse, the keyboard and the interface bar until every
/// running sequence has ended, and clicks made meanwhile are discarded;
/// outside the player's turn nothing is accepted either. Every map click,
/// bar press and game hotkey is gated on this, and the animated watch
/// `wait2.frm` shows while it holds (§3.26).
pub fn input_locked(state: &ClickState) -> bool {
    state.combat && (!state.player_turn || state.presenting)
}

/// §3.27 "Clicks while the player is busy", for a click that passed the
/// combat lock.
///
/// - A movement click out of combat with `interrupt_walk` on first cancels
///   the player's current sequence, whatever it is doing: a reach or a
///   container use in progress is abandoned, and its unstarted use,
///   take-out and loot request are dropped. With it off, the new walk
///   cannot register while the player is busy, so the click does nothing.
/// - An action cancels the current sequence only while the player's art is
///   walk or run: a click during the approach of an earlier action replaces
///   that whole action. In any other pose (a reach, a put-away, a take-out,
///   standing inside a sequence) the new action's first player-owned entry
///   cannot register, so the click does nothing and shows no message.
/// - A near talk is callbacks only and always registers.
/// - Rotating works only while the player's art is standing.
pub fn busy_policy(click: Click, state: &ClickState, interrupt_walk: bool) -> Policy {
    match click {
        Click::Movement if !state.busy => Policy::Proceed,
        Click::Movement if interrupt_walk && !state.combat => Policy::Cancel,
        Click::Movement => Policy::Ignore,
        Click::Action if !state.busy => Policy::Proceed,
        Click::Action if state.walking => Policy::Cancel,
        Click::Action => Policy::Ignore,
        Click::NearTalk => Policy::Proceed,
        Click::Rotate if state.standing => Policy::Proceed,
        Click::Rotate => Policy::Ignore,
    }
}

/// The whole §3.26-§3.27 policy: the combat lock, then the busy rules.
pub fn click_policy(click: Click, state: &ClickState, interrupt_walk: bool) -> Policy {
    if input_locked(state) {
        Policy::Ignore
    } else {
        busy_policy(click, state, interrupt_walk)
    }
}

/// `fallout.cfg` `[system] interrupt_walk` (§3.27), on by default.
pub fn interrupt_walk(config: &vault_formats::cfg::ConfigFile) -> bool {
    config.get_int("system", "interrupt_walk").unwrap_or(1) != 0
}
