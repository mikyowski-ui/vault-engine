//! The autoplay bot's way into a session: what it can see of the screens, and
//! the actions only a screen can carry out. Both the headless runner and Watch
//! Mode come through here, so they are the same game.
use super::{MapPreview, Screen};
use vault_core::{
    WorldError,
    autoplay::{AutoplayAgent, BotAction, BotOutcome, BotScreen, BotView},
};
use winit::keyboard::KeyCode;

/// How fast Watch Mode plays: virtual 16 ms frames per shown frame.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum BotSpeed {
    #[default]
    X1,
    X2,
    X4,
    X10,
    /// As many as fit in a shown frame.
    Max,
}

impl BotSpeed {
    pub const ALL: [Self; 5] = [Self::X1, Self::X2, Self::X4, Self::X10, Self::Max];
    /// Frames per shown frame; `None` for [`Self::Max`].
    pub fn frames(self) -> Option<u32> {
        match self {
            Self::X1 => Some(1),
            Self::X2 => Some(2),
            Self::X4 => Some(4),
            Self::X10 => Some(10),
            Self::Max => None,
        }
    }
    pub fn label(self) -> &'static str {
        match self {
            Self::X1 => "1x",
            Self::X2 => "2x",
            Self::X4 => "4x",
            Self::X10 => "10x",
            Self::Max => "Max",
        }
    }
}

/// Watch Mode's state in a visible session.
pub struct Watch {
    pub agent: AutoplayAgent,
    /// The player has control; the bot waits.
    pub paused: bool,
    pub speed: BotSpeed,
    /// The Space that took control over, until its key comes up.
    pub swallow_space: bool,
}

impl Watch {
    pub fn new(agent: AutoplayAgent) -> Self {
        Self {
            agent,
            paused: false,
            speed: BotSpeed::default(),
            swallow_space: false,
        }
    }
}

impl MapPreview {
    /// A replay's half of `InputCommand::Autoplay`. It runs no agent, since
    /// the log holds the bot's actions, but presses must route as they did:
    /// a banner button takes its press, not the map.
    pub fn replay_autoplay(&mut self, on: bool) -> Result<(), WorldError> {
        match &mut self.watch {
            Some(watch) => watch.paused = !on,
            None if on => {
                self.watch = Some(Box::new(Watch::new(AutoplayAgent::new(1, false)?)));
            }
            None => {}
        }
        Ok(())
    }

    /// What the bot sees of the session.
    pub fn bot_view(&self) -> BotView {
        let screen = match &self.screen {
            Screen::Map => BotScreen::Map,
            Screen::World(_) | Screen::World2(_) => BotScreen::WorldMap,
            Screen::Town(_) | Screen::Town2(_) => BotScreen::TownMap,
            Screen::Movie(_) => BotScreen::Movie,
            Screen::Inventory(_) => BotScreen::Inventory,
            Screen::Loot(_) => BotScreen::Loot,
            Screen::Barter(_) => BotScreen::Barter,
            Screen::Elevator(_) => BotScreen::Elevator,
            _ => BotScreen::Other,
        };
        let dialog = self.dialog.is_some();
        BotView {
            screen,
            input_open: !dialog && !self.fade_blocks() && !self.bar_blocks(),
            dialog,
            now_ms: self.clock,
        }
    }

    /// Carry out `action`, world rules first, then screens.
    pub fn apply_bot(&mut self, action: &BotAction) -> Result<BotOutcome, WorldError> {
        let outcome = self.host.execute_bot(action)?;
        if outcome != BotOutcome::NeedsShell {
            self.drain_effects();
            return Ok(outcome);
        }
        Ok(match *action {
            BotAction::TravelTo { place } => self.bot_head_for(place),
            BotAction::EnterHere => self.bot_enter_here(),
            BotAction::TownEntrance { index } => self.bot_town_entrance(index),
            BotAction::CloseScreen => {
                self.bot_key(KeyCode::Escape);
                BotOutcome::Done
            }
            BotAction::DismissDialog => {
                self.bot_key(KeyCode::Enter);
                BotOutcome::Done
            }
            _ => BotOutcome::Refused(format!("no screen carries out {action:?}")),
        })
    }

    fn bot_head_for(&mut self, place: usize) -> BotOutcome {
        match &self.screen {
            Screen::World(_) => self.bot_head_for_town(place),
            Screen::World2(_) => self.bot_head_for_city(place),
            _ => BotOutcome::Refused("no world map is open".to_owned()),
        }
    }

    fn bot_enter_here(&mut self) -> BotOutcome {
        match &self.screen {
            Screen::World(_) => self.bot_enter_town(),
            Screen::World2(_) => self.bot_enter_city(),
            _ => BotOutcome::Refused("no world map is open".to_owned()),
        }
    }

    fn bot_town_entrance(&mut self, index: usize) -> BotOutcome {
        match &self.screen {
            Screen::Town(_) => self.bot_take_town_marker(index),
            Screen::Town2(_) => self.bot_take_city_marker(index),
            _ => BotOutcome::Refused("no town map is open".to_owned()),
        }
    }
}
