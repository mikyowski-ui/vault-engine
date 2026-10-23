//! SPEC-014 §3.26-§3.27 clicks while sequences play: the combat input lock
//! and the pure policy for a player's map click, action-menu entry, bar
//! press or game hotkey. The interface applies it before acting (showing
//! the animated watch while combat locks input), and the world applies its
//! busy rules again as it registers (`WorldHost::walk_toward_with` and the
//! interaction sequences). Nothing is ever queued: a click either cancels
//! the player's current sequence and starts its own, or does nothing. The
//! rules come from SPEC-007 §3.10.1 "One sequence per owner".

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
    /// In combat, whether it is the player's turn: from its start, before
    /// the player's combat script and stand-up, to its end.
    pub player_turn: bool,
    /// The combat lock a running sequence engaged during the player's turn
    /// ([`CombatLock`]).
    pub engaged: bool,
    /// A sequence holding the player has not ended: a non-callback entry
    /// of the player's, started or not, in a sequence other than an idle
    /// fidget (`WorldHost::owns_motion`).
    pub busy: bool,
    /// The player's current art is walk (1) or run (19).
    pub walking: bool,
    /// The player's current art is standing (0).
    pub standing: bool,
}

/// §3.27 the combat input lock: from combat start the game mouse, the
/// keyboard and the interface bar are disabled (edge scrolling stays on)
/// under the animated watch `wait2.frm` (list index 295), except during
/// the player's turn while [`CombatLock`] is not engaged. Every map click,
/// bar press and game hotkey is gated on this; clicks are discarded and
/// keys held (§3.26-§3.27).
pub fn input_locked(state: &ClickState) -> bool {
    state.combat && (!state.player_turn || state.engaged)
}

/// SPEC-014 §3.27 the combat-wide count of running sequences and the lock
/// it drives.
///
/// - Every sequence that registers while combat is active counts until it
///   ends, normally, by failing while it plays or by being cancelled,
///   whatever it holds: the player, an NPC, a door, a projectile or an
///   extra victim's reaction. A sequence whose registration fails never
///   counts, and neither does one registered before combat began.
/// - During the player's turn, the count rising from 0 engages the lock
///   and its falling back to 0 releases it. A sequence that starts while
///   another still runs extends the lock. Sequences registered outside the
///   player's turn change the count but not the lock.
/// - The player's turn start releases the lock and flushes held keys
///   (`turns` counts the starts); between player turns the input stays
///   locked, and the end of combat releases everything without a flush.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct CombatLock {
    /// Running sequences registered while combat was active.
    pub count: usize,
    /// The player's turn has started (before its combat script and any
    /// stand-up) and not ended.
    pub turn: bool,
    /// A sequence engaged the lock during the player's turn.
    pub engaged: bool,
    /// Player turns started: each one flushes the held keys.
    pub turns: u64,
}
impl CombatLock {
    /// The player's turn starts: the input comes back and held keys are
    /// flushed.
    pub fn begin_turn(&mut self) {
        self.turn = true;
        self.engaged = false;
        self.turns += 1;
    }
    /// The player's turn ended, or combat started or ended: while combat
    /// lasts the input stays locked until the next player turn.
    pub fn end_turn(&mut self) {
        self.turn = false;
        self.engaged = false;
    }
    /// The count is now `count`; `player_turn` is whether the player's turn
    /// is under way.
    pub fn counted(&mut self, count: usize, player_turn: bool) {
        if player_turn {
            if self.count == 0 && count > 0 {
                self.engaged = true;
            } else if count == 0 {
                self.engaged = false;
            }
        }
        self.count = count;
    }
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
