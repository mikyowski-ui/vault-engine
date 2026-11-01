//! The autoplay bot: a deterministic [`AutoplayAgent`] that decides from the
//! world and its own random stream, and [`WorldHost::execute_bot`], which
//! carries an action out through the same rules the interface uses
//! (`host_player`). Actions are logged (`InputCommand::Bot`), so a replay of
//! the log repeats a run exactly. The agent never draws from `world.rng`: a
//! bot's thinking must not change what the game rolls.
use crate::{ParkMiller, WorldError, assets::WorldAssets, host::WorldHost};
use serde::{Deserialize, Serialize};

/// How the bot interacts with an object.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BotVerb {
    Use,
    Talk,
    Pickup,
    Look,
}

/// One thing the bot does, as logged. Objects are named by `raw.id`, never by
/// a slot handle, as [`crate::debug::DebugCommand`] names them.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "op", rename_all = "snake_case")]
pub enum BotAction {
    Walk {
        hex: i32,
        run: bool,
    },
    Attack {
        target: u32,
        hit_mode: u8,
        location: Option<u8>,
    },
    Reload {
        weapon: u32,
    },
    UseItem {
        item: u32,
    },
    UseItemOn {
        item: u32,
        target: u32,
    },
    UseSkill {
        skill: i32,
        target: u32,
    },
    Interact {
        target: u32,
        verb: BotVerb,
    },
    ArmExplosive {
        item: u32,
        seconds: i32,
    },
    Dialogue {
        choice: Option<usize>,
    },
    EndTurn,
    EndCombat,
    /// World map: head for town or city `place`, or enter it if the party is
    /// there.
    TravelTo {
        place: usize,
    },
    /// World map: enter where the party stands.
    EnterHere,
    /// Town map: take entrance marker `index`.
    TownEntrance {
        index: usize,
    },
    /// Leave the screen in front.
    CloseScreen,
    /// Accept the dialog box in front.
    DismissDialog,
}

/// What became of an action.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BotOutcome {
    Done,
    /// The rules said no; nothing is wrong with the world, and the agent
    /// should try something else.
    Refused(String),
    /// A screen action: only the shell can carry it out
    /// (`MapPreview::apply_bot`).
    NeedsShell,
}

/// The screen in front, as far as the bot cares.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum BotScreen {
    #[default]
    Map,
    WorldMap,
    TownMap,
    Movie,
    Inventory,
    Loot,
    Barter,
    Elevator,
    Other,
}

/// What only the shell knows.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct BotView {
    pub screen: BotScreen,
    /// Whether the session takes input now: no fade, roll, dialog or lock.
    pub input_open: bool,
    /// A dialog box waits in front.
    pub dialog: bool,
    pub now_ms: u64,
}

/// What the bot is about, for the Watch Mode banner and the run's report.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct BotStatus {
    /// e.g. `Testing Door #142 with C4`.
    pub goal: String,
    pub tactic: String,
    pub ticks: u64,
    pub actions: u64,
    pub refusals: u64,
    /// Decisions in a row that changed nothing.
    pub stuck: u32,
    /// The hex the bot is making for.
    pub target_hex: Option<i32>,
    /// The planned route, as hexes.
    pub route: Vec<i32>,
    /// A chaos check that failed, for the run's report.
    pub violation: Option<String>,
}

/// The bot.
pub struct AutoplayAgent {
    rng: ParkMiller,
    chaos_audit: bool,
    status: BotStatus,
}

impl AutoplayAgent {
    pub fn new(seed: i32, chaos_audit: bool) -> Result<Self, WorldError> {
        Ok(Self {
            rng: ParkMiller::seeded(seed.max(1))
                .map_err(|error| WorldError::Invalid(error.to_string()))?,
            chaos_audit,
            status: BotStatus::default(),
        })
    }

    /// The next action, or `None` to let the step pass.
    pub fn decide<A: WorldAssets>(
        &mut self,
        _host: &WorldHost<A>,
        _view: &BotView,
    ) -> Option<BotAction> {
        let _ = (&mut self.rng, self.chaos_audit);
        None
    }

    pub fn status(&self) -> &BotStatus {
        &self.status
    }
}

impl<A: WorldAssets> WorldHost<A> {
    /// Carry out `action` as far as the world goes. Screen actions come back
    /// [`BotOutcome::NeedsShell`].
    pub fn execute_bot(&mut self, _action: &BotAction) -> Result<BotOutcome, WorldError> {
        Ok(BotOutcome::Refused("not implemented".to_owned()))
    }
}
