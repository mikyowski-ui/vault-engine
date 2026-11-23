//! The autoplay bot: a deterministic [`AutoplayAgent`] that decides from the
//! world and its own random stream, and [`WorldHost::execute_bot`], which
//! carries an action out through the same rules the interface uses
//! (`host_player`). Actions are logged (`InputCommand::Bot`), so a replay of
//! the log repeats a run exactly. The agent never draws from `world.rng`: a
//! bot's thinking must not change what the game rolls.
//!
//! The agent has four parts, tried in order on each decision: screens and
//! dialogue, the combat tactician, the chaos audit (`chaos_audit` only), and
//! the goal spine, which explores a map and leaves it. A goal that makes no
//! progress is given up after [`STUCK_LIMIT`] decisions, so one dead end
//! cannot burn a run's budget.
use crate::{
    ObjectAction, ObjectRef, ParkMiller, RandomSource, Refusal, ScriptEvent, WorldError,
    assets::WorldAssets,
    clicks::{self, Click, Policy},
    combat::{Attack, damage_type},
    critical::{hit_mode, location},
    drugs,
    host::{TRACE_RETAINED, WorldHost},
    host_player::{ItemUse, PlayerAttack, PlayerReload},
    host_skills::BookRoute,
    queue::EventKind,
    routing::{route_hexes, step},
    skills::{self, SetTimer, skill},
    ui::automap::MOTION_SENSOR,
    world::{WorldObject, WorldTrace},
    worldmap,
};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet, VecDeque};
use vault_formats::{
    Fid, HexGrid,
    pro::{BasicFlags, Category, ItemData, LockFlags, PrototypeData, Scenery, SceneryData},
};

/// Decisions in a row that change nothing before a goal is given up.
const STUCK_LIMIT: u32 = 12;
/// Decisions spent waiting without any change before the wait counts as
/// stuck; a long fuse at 16 ms a frame stays well inside it.
const WAIT_LIMIT: u32 = 20_000;
/// The tactician heals below this share of maximum HP.
const HEAL_PERCENT: i32 = 40;
/// SPEC-013 §3.10: timed explosives reach critters and notify scripts this
/// far from the center hex.
const BLAST_RADIUS: i32 = 3;
/// The bot stands at least this far from its own bomb.
const SAFE_DISTANCE: i32 = 5;
/// Objects this close to a bomb are watched, so those just outside the blast
/// prove it stopped where it should.
const WATCH_RADIUS: i32 = 6;
/// The shortest fuse the Set Timer window accepts.
const FUSE_SECONDS: i32 = 10;
/// World ticks past a fuse's end before it counts as overdue.
const FUSE_SLACK_TICKS: u64 = 100;
/// Decisions an explosion may stay in progress before it counts as hung.
const EXPLOSION_DECISIONS: u32 = 2000;
/// How far around the player a hex counts as seen, in steps.
const SEEN_STEPS: usize = 5;
/// How many hexes one survey of the map walks.
const SURVEY_HEXES: usize = 3000;
/// Exploring hops per map.
const EXPLORE_HOPS: u32 = 10;
/// How close an unseen hex may be and still be worth a hop.
const EXPLORE_FROM: u32 = 6;
/// How many unseen hexes a hop chooses among.
const EXPLORE_CHOICES: usize = 6;
/// Actions the tactician takes in a turn before ending it.
const TURN_ACTIONS: u32 = 12;
/// Attacks that come to nothing (the rules refuse them, or their art is
/// missing) before the tactician gives up the weapon, and then the turn.
const DUDS_TO_FISTS: u32 = 2;
const DUDS_TO_END: u32 = 4;
/// Walks the tactician makes in a turn.
const TURN_MOVES: u32 = 3;
/// Choices in one dialogue before the bot leaves it.
const DIALOGUE_CHOICES: u32 = 64;
/// Bombs the audit spends on one map.
const BOMBS_PER_MAP: usize = 2;
/// Town-map markers tried before going back to the world map.
const ENTRANCE_TRIES: usize = 8;
/// Hexes walked at a time toward a goal too far for one route: the route
/// search's slots run out at some 25 hexes over open ground (SPEC-007 §3.1).
const HOP_STEPS: usize = 16;
/// Hops toward one goal before it is given up.
const WALK_HOPS: u32 = 40;
/// Attempts at reaching a city before choosing another.
const HEADING_TRIES: u32 = 3;
/// How often a screen is asked again to close, in decisions.
const CLOSE_EVERY: u32 = 8;
/// Maximum hit points, and the base of the explosion DT and DR stats.
const MAX_HP: usize = 7;
const THRESHOLDS: usize = 17;
const RESISTANCES: usize = 24;
/// The hexes a map holds.
const HEXES: i32 = 40000;

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
    /// Arm a carried explosive and drop it where the player stands.
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
    /// Out of combat, put a carried weapon in the active hand.
    Equip {
        item: u32,
    },
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

/// Decisions at the world map, with nowhere left to go, before the bot calls
/// the campaign finished.
const FINISHED_AFTER: u32 = 256;

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
    /// Nothing is left to do.
    pub finished: bool,
    /// A chaos check that failed, for the run's report: the first one.
    pub violation: Option<String>,
    /// Chaos checks that held.
    pub checks: u64,
    /// Chaos checks that failed, the first of them being `violation`.
    pub violations: u64,
    /// Goals given up as stuck.
    pub abandoned: u64,
}

/// One goal of the spine. Objects are named by `raw.id`, hexes by number.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum Goal {
    Pickup(u32),
    Talk(u32),
    Fight(u32),
    Door(u32),
    Explore(i32),
    Exit(i32),
    Climb(u32),
}

/// How a goal stands.
enum Pursuit {
    Act(BotAction),
    /// A hop on the way, which is no attempt at the goal.
    Hop(BotAction),
    Done,
    Failed,
}

/// How to get to a hex.
enum Way {
    /// One route gets there, or the player already stands by it.
    Direct,
    /// Too far for one route: this hex is on the way.
    Hop(i32),
    /// There is no way, short of opening something.
    Closed,
}

/// How a part of the agent that may have nothing to do came out.
enum Flow {
    Act(BotAction),
    Wait,
    Idle,
}

/// What the bot remembers of a map.
#[derive(Default)]
struct MapMemory {
    seen: BTreeSet<(u8, i32)>,
    /// Goals finished or given up.
    tried: BTreeSet<Goal>,
    hops: u32,
    /// Times each exit-grid hex was taken.
    exits: BTreeMap<i32, u32>,
    /// Upkeep attempts, by item.
    tended: BTreeMap<u32, u32>,
}

/// The tactician's memory of the player's turn.
#[derive(Default)]
struct Turn {
    round: Option<u32>,
    acts: u32,
    moves: u32,
    reloaded: bool,
    healed: bool,
    asked_end: bool,
    /// The AP when the last attack went out: unchanged, it did not happen.
    struck_at: Option<i32>,
    /// Attacks this turn that did not happen.
    duds: u32,
}

/// What must hold for a decision to count as progress.
#[derive(PartialEq, Eq)]
struct Progress {
    map: Option<String>,
    screen: BotScreen,
    hex: Option<i32>,
    elevation: u8,
    ap: i32,
    hp: i32,
    combat: bool,
    round: u32,
    dialogue: bool,
    said: usize,
    carried: usize,
    objects: usize,
}

impl Progress {
    fn of<A: WorldAssets>(host: &WorldHost<A>, view: &BotView) -> Self {
        let world = &host.world;
        let player = world.get(world.player);
        Self {
            map: world.current_map_name(),
            screen: view.screen,
            hex: player.and_then(|object| object.raw.hex),
            elevation: player.map_or(0, |object| object.raw.elevation),
            ap: world.action_points(world.player),
            hp: world.hit_points(world.player).unwrap_or(0),
            combat: world.combat,
            round: world.combat_state.round,
            dialogue: world.dialogue.open,
            said: world.dialogue.review.len(),
            carried: player.map_or(0, |object| object.inventory.len()),
            objects: world.iter().count(),
        }
    }
}

/// A check the chaos audit runs on a door or a container.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Check {
    /// SPEC-013 §3.8: the default does nothing.
    Lockpick,
    /// Only a script makes a Crowbar do anything.
    Crowbar,
    /// Use it, and check the passage changed.
    Swing,
    /// Arm an explosive beside it.
    Bomb,
}

/// The audit of one object.
struct Audit {
    id: u32,
    label: String,
    steps: VecDeque<Check>,
    /// Hops made toward the object.
    walks: u32,
    /// A check whose action went out and still has to be verified.
    sent: Option<Sent>,
}

struct Sent {
    check: Check,
    before: Frozen,
    probe: Option<[i32; 2]>,
    hex: i32,
    elevation: u8,
}

/// The parts of an object an audit compares.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Frozen {
    lock: Option<u32>,
    flags: u32,
    frame: usize,
    hex: Option<i32>,
}

impl Frozen {
    fn of(object: &WorldObject) -> Self {
        Self {
            lock: object.lock().map(|lock| lock.0),
            flags: object.raw.flags.0,
            frame: object.raw.frame,
            hex: object.raw.hex,
        }
    }
    fn open(&self) -> bool {
        self.lock
            .is_some_and(|lock| LockFlags(lock).contains(LockFlags::OPEN))
    }
}

/// Where the bomb job stands.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum BombStage {
    Approach,
    Retreat,
    Waiting,
}

/// The bomb the audit is placing.
struct Bomb {
    target: u32,
    label: String,
    item: u32,
    name: &'static str,
    stage: BombStage,
    tries: u32,
}

/// An object near a bomb, as it stood when the bomb was armed.
struct Neighbor {
    object: ObjectRef,
    id: u32,
    label: String,
    hex: i32,
    distance: i32,
    before: Frozen,
    script: Option<u32>,
    /// It holds a `damage_p_proc`.
    procedure: bool,
    /// Its `damage_p_proc` runs in the trace so far.
    notices: usize,
    probe: Option<[i32; 2]>,
}

/// A critter near a burning fuse, as last seen.
struct Bystander {
    critter: ObjectRef,
    id: u32,
    hp: i32,
    distance: i32,
}

/// A burning fuse and what is checked when it has fired.
struct Fuse {
    item: ObjectRef,
    id: u32,
    pid: u32,
    label: String,
    hex: i32,
    elevation: u8,
    deadline: u64,
    tiles: Vec<(u16, u16)>,
    neighbors: Vec<Neighbor>,
    bystanders: Vec<Bystander>,
    /// The item was seen armed and on the ground.
    dropped: bool,
    /// The explosion's victims, once it was seen in progress.
    victims: Option<Vec<(ObjectRef, i32)>>,
    /// Decisions since the explosive left the world.
    burned: u32,
}

/// The bot.
pub struct AutoplayAgent {
    rng: ParkMiller,
    chaos_audit: bool,
    status: BotStatus,
    /// The current map's working name.
    map: String,
    maps: BTreeMap<String, MapMemory>,
    place: Option<(i32, u8)>,
    goal: Option<Goal>,
    /// Actions sent for the goal in hand.
    attempts: u32,
    /// Hops made on the way to the goal in hand.
    walks: u32,
    progress: Option<Progress>,
    /// This decision is a wait, which is not being stuck.
    waiting: bool,
    /// Waiting decisions in a row with nothing changing.
    waited: u32,
    turn: Turn,
    /// Dialogue branches taken: speaker, reply, option.
    branches: BTreeSet<(u32, Vec<u8>, usize)>,
    choices: u32,
    /// Maps and elevations whose audit was planned.
    audited: BTreeSet<(String, u8)>,
    audits: VecDeque<Audit>,
    bomb: Option<Bomb>,
    fuse: Option<Fuse>,
    /// The town or city being travelled to, with the attempts so far.
    heading: Option<(usize, u32)>,
    visits: BTreeMap<usize, u32>,
    entrance: usize,
}

impl AutoplayAgent {
    pub fn new(seed: i32, chaos_audit: bool) -> Result<Self, WorldError> {
        Ok(Self {
            rng: ParkMiller::seeded(seed.max(1))
                .map_err(|error| WorldError::Invalid(error.to_string()))?,
            chaos_audit,
            status: BotStatus::default(),
            map: String::new(),
            maps: BTreeMap::new(),
            place: None,
            goal: None,
            attempts: 0,
            walks: 0,
            progress: None,
            waiting: false,
            waited: 0,
            turn: Turn::default(),
            branches: BTreeSet::new(),
            choices: 0,
            audited: BTreeSet::new(),
            audits: VecDeque::new(),
            bomb: None,
            fuse: None,
            heading: None,
            visits: BTreeMap::new(),
            entrance: 0,
        })
    }

    /// The next action, or `None` to let the step pass. This reads the world
    /// and changes nothing in it.
    pub fn decide<A: WorldAssets>(
        &mut self,
        host: &WorldHost<A>,
        view: &BotView,
    ) -> Option<BotAction> {
        self.status.ticks += 1;
        self.enter_map(host);
        self.note_seen(host);
        self.watch_fuse(host);
        let now = Progress::of(host, view);
        let changed = self.progress.as_ref() != Some(&now);
        self.progress = Some(now);
        if changed {
            self.status.stuck = 0;
            self.waited = 0;
        }
        self.waiting = false;
        let decision = self.choose(host, view);
        if self.waiting {
            self.waited += 1;
        }
        if !changed && (!self.waiting || self.waited > WAIT_LIMIT) {
            self.status.stuck += 1;
        }
        if decision.is_some() {
            self.status.actions += 1;
        }
        decision
    }

    pub fn status(&self) -> &BotStatus {
        &self.status
    }

    /// Tell the agent what became of its last action. A refusal costs the
    /// goal in hand its remaining attempts, and is counted. An agent that is
    /// not told still moves on, by being stuck.
    pub fn note_outcome(&mut self, outcome: &BotOutcome) {
        if matches!(outcome, BotOutcome::Refused(_)) {
            self.status.refusals += 1;
            self.attempts = self.attempts.saturating_add(1);
            // Nothing happened, so there is nothing to verify.
            if let Some(audit) = self.audits.front_mut() {
                audit.sent = None;
            }
        }
    }

    fn say(&mut self, tactic: &str, goal: impl Into<String>) {
        self.status.tactic.clear();
        self.status.tactic.push_str(tactic);
        self.status.goal = goal.into();
    }

    fn wait(&mut self, tactic: &str, goal: impl Into<String>) {
        self.say(tactic, goal);
        self.waiting = true;
    }

    fn memory(&mut self) -> &mut MapMemory {
        self.maps.entry(self.map.clone()).or_default()
    }

    fn tried(&self, goal: &Goal) -> bool {
        self.maps
            .get(&self.map)
            .is_some_and(|memory| memory.tried.contains(goal))
    }

    fn roll(&mut self, count: usize) -> usize {
        let last = i32::try_from(count).unwrap_or(1).max(1) - 1;
        usize::try_from(self.rng.range(0, last).unwrap_or(0)).unwrap_or(0)
    }

    fn clear_target(&mut self) {
        self.status.target_hex = None;
        self.status.route.clear();
    }

    /// A check that held counts; the first that failed is the violation.
    fn expect(&mut self, holds: bool, failure: impl FnOnce() -> String) {
        if holds {
            self.status.checks += 1;
        } else {
            self.fail(failure());
        }
    }

    fn fail(&mut self, failure: String) {
        self.status.violations += 1;
        self.status.violation.get_or_insert(failure);
    }

    /// A new map starts over: nothing in hand survives a transition.
    fn enter_map<A: WorldAssets>(&mut self, host: &WorldHost<A>) {
        let map = host.world.current_map_name().unwrap_or_default();
        if map != self.map {
            self.map = map;
            self.goal = None;
            self.attempts = 0;
            self.audits.clear();
            self.bomb = None;
            self.fuse = None;
            self.place = None;
            self.clear_target();
        }
    }

    fn note_seen<A: WorldAssets>(&mut self, host: &WorldHost<A>) {
        let Some(place) = place_of(host) else {
            return;
        };
        if self.place == Some(place) {
            return;
        }
        self.place = Some(place);
        let (hex, elevation) = place;
        let memory = self.memory();
        memory.seen.insert((elevation, hex));
        let mut ring = vec![hex];
        for _ in 0..SEEN_STEPS {
            let mut next = Vec::new();
            for at in ring {
                for direction in 0..6 {
                    let hex = step(at, direction);
                    if memory.seen.insert((elevation, hex)) {
                        next.push(hex);
                    }
                }
            }
            ring = next;
        }
    }

    /// The part of the agent this decision belongs to.
    fn choose<A: WorldAssets>(&mut self, host: &WorldHost<A>, view: &BotView) -> Option<BotAction> {
        let world = &host.world;
        if world.is_dead(world.player) {
            self.say("Dead", "The player is dead");
            return None;
        }
        if view.dialog {
            self.say("Screens", "Dismissing a dialog box");
            return self
                .status
                .stuck
                .is_multiple_of(CLOSE_EVERY)
                .then_some(BotAction::DismissDialog);
        }
        if !view.input_open {
            self.wait("Screens", "Waiting for the session to take input");
            return None;
        }
        if view.screen != BotScreen::TownMap {
            self.entrance = 0;
        }
        match view.screen {
            BotScreen::Map => {}
            BotScreen::WorldMap => return self.on_world_map(host),
            BotScreen::TownMap => return self.on_town_map(),
            _ => {
                self.say("Screens", "Closing the screen in front");
                return self
                    .status
                    .stuck
                    .is_multiple_of(CLOSE_EVERY)
                    .then_some(BotAction::CloseScreen);
            }
        }
        if world.dialogue.open {
            return self.converse(host);
        }
        self.choices = 0;
        if world.combat || world.pending_combat.is_some() {
            return self.fight(host);
        }
        self.turn = Turn::default();
        if world.pending_transition.is_some() {
            self.wait("Explorer", "Waiting for the map to change");
            return None;
        }
        if self.busy(host) {
            self.wait(&self.status.tactic.clone(), self.status.goal.clone());
            return None;
        }
        if self.status.stuck > 0 && self.status.stuck.is_multiple_of(STUCK_LIMIT) {
            self.give_up();
        }
        if let Some(action) = self.tend(host) {
            return Some(action);
        }
        // Each pass ends a goal, so the passes run out.
        loop {
            if self.chaos_audit && self.goal.is_none() && self.next_pickup(host).is_none() {
                match self.audit(host) {
                    Flow::Act(action) => return Some(action),
                    Flow::Wait => return None,
                    Flow::Idle => {}
                }
            }
            match self.spine(host) {
                Flow::Act(action) => return Some(action),
                Flow::Wait => return None,
                Flow::Idle => {}
            }
        }
    }

    /// The player, or the object an audit is waiting on, is still in motion.
    fn busy<A: WorldAssets>(&self, host: &WorldHost<A>) -> bool {
        if host.owns_motion(host.world.player) {
            return true;
        }
        self.audits
            .front()
            .filter(|audit| audit.sent.is_some())
            .and_then(|audit| host.world.reference_for_id(audit.id))
            .is_some_and(|object| host.moving(object))
    }

    /// Stop trying what is in hand. A bomb that is waiting on its fuse is
    /// not stuck, and a fuse that burns stays watched.
    fn give_up(&mut self) {
        let dropped = if let Some(goal) = self.goal.take() {
            self.memory().tried.insert(goal);
            true
        } else if let Some(bomb) = &self.bomb {
            let moving = bomb.stage != BombStage::Waiting;
            if moving {
                self.bomb = None;
            }
            moving
        } else {
            self.audits.pop_front().is_some()
        };
        if dropped {
            self.status.abandoned += 1;
            self.attempts = 0;
            self.walks = 0;
            self.clear_target();
        }
    }

    // Screens.

    fn on_world_map<A: WorldAssets>(&mut self, host: &WorldHost<A>) -> Option<BotAction> {
        let world = &host.world;
        let (known, here, travelling) = match world.fallout2.as_deref() {
            Some(state) => (
                host.fallout2_known_cities()
                    .into_iter()
                    .filter(|city| !host.fallout2_city_locked(*city))
                    .collect(),
                host.fallout2_party_city(),
                state.travel.is_some(),
            ),
            None => {
                let map = &world.world_map;
                let towns: Vec<usize> = (0..worldmap::TOWNS.len())
                    .filter(|town| map.town_enabled(*town))
                    .collect();
                let here = map.cell().and_then(worldmap::town_at);
                (towns, here, map.travel.is_some())
            }
        };
        if let Some(town) = here {
            self.visits.entry(town).or_insert(1);
        }
        if travelling {
            self.wait("Traveller", "Crossing the world map");
            return None;
        }
        if let Some((place, tries)) = self.heading {
            if here == Some(place) {
                self.heading = None;
                *self.visits.entry(place).or_insert(0) += 1;
                self.say("Traveller", format!("Entering place {place}"));
                return Some(BotAction::EnterHere);
            }
            if tries < HEADING_TRIES {
                self.heading = Some((place, tries + 1));
                self.say("Traveller", format!("Heading for place {place}"));
                return Some(BotAction::TravelTo { place });
            }
            self.heading = None;
            *self.visits.entry(place).or_insert(0) += 1;
        }
        let mut places: Vec<(u32, usize)> = known
            .iter()
            .filter(|place| Some(**place) != here)
            .map(|place| (self.visits.get(place).copied().unwrap_or(0), *place))
            .collect();
        places.sort_unstable();
        let least = places.first().map(|(visits, _)| *visits);
        places.retain(|(visits, _)| Some(*visits) == least);
        if places.is_empty() {
            self.say("Traveller", "Entering where the party stands");
            // Everywhere has been seen and nothing here takes the party in.
            self.status.finished = self.status.stuck >= FINISHED_AFTER;
            return self
                .status
                .stuck
                .is_multiple_of(CLOSE_EVERY)
                .then_some(BotAction::EnterHere);
        }
        let place = places[self.roll(places.len())].1;
        self.heading = Some((place, 1));
        self.say("Traveller", format!("Heading for place {place}"));
        Some(BotAction::TravelTo { place })
    }

    /// Markers are tried in order: the shell alone knows how many there are,
    /// and a marker that is not there is refused.
    fn on_town_map(&mut self) -> Option<BotAction> {
        if self.entrance >= ENTRANCE_TRIES {
            self.entrance = 0;
            self.say("Traveller", "Leaving the town map");
            return Some(BotAction::CloseScreen);
        }
        let index = self.entrance;
        self.entrance += 1;
        self.say("Traveller", format!("Taking town entrance {index}"));
        Some(BotAction::TownEntrance { index })
    }

    /// Dialogue: a branch not taken yet, whatever it is, else any; and out,
    /// when a session has run long or stopped waiting for a choice.
    fn converse<A: WorldAssets>(&mut self, host: &WorldHost<A>) -> Option<BotAction> {
        let dialogue = &host.world.dialogue;
        if !dialogue.waiting {
            self.say("Talker", "Waiting for the dialogue");
            return (self.status.stuck >= STUCK_LIMIT)
                .then_some(BotAction::Dialogue { choice: None });
        }
        let options = dialogue.options.len();
        self.choices += 1;
        if options == 0 || self.choices > DIALOGUE_CHOICES {
            self.say("Talker", "Leaving the dialogue");
            return Some(BotAction::Dialogue { choice: None });
        }
        let speaker = host
            .world
            .get(dialogue.target)
            .map_or(0, |object| object.raw.id);
        let reply = dialogue.resolved_reply.clone();
        let fresh: Vec<usize> = (0..options)
            .filter(|option| !self.branches.contains(&(speaker, reply.clone(), *option)))
            .collect();
        let choice = if fresh.is_empty() {
            self.roll(options)
        } else {
            fresh[self.roll(fresh.len())]
        };
        self.branches.insert((speaker, reply, choice));
        self.say("Talker", format!("Choosing option {choice}"));
        Some(BotAction::Dialogue {
            choice: Some(choice),
        })
    }

    // The combat tactician.

    fn fight<A: WorldAssets>(&mut self, host: &WorldHost<A>) -> Option<BotAction> {
        let world = &host.world;
        let player = world.player;
        if !world.combat {
            self.wait("Tactician", "Combat is starting");
            return None;
        }
        let combat = &world.combat_state;
        if !combat.player_turn
            || clicks::input_locked(&host.click_state())
            || host.presenting()
            || host.owns_motion(player)
            || host.player_turn_over()
        {
            self.wait("Tactician", "Waiting for the player's turn");
            return None;
        }
        if self.turn.round != Some(combat.round) {
            self.turn = Turn {
                round: Some(combat.round),
                ..Turn::default()
            };
        }
        self.turn.acts += 1;
        if self.turn.acts > TURN_ACTIONS || self.status.stuck >= STUCK_LIMIT {
            self.say("Tactician", "Ending a turn that goes nowhere");
            return Some(BotAction::EndTurn);
        }
        let ap = world.action_points(player);
        if self.turn.struck_at.take() == Some(ap) {
            self.turn.duds += 1;
        }
        if !self.turn.healed
            && ap >= 2
            && hurt(host)
            && let Some(item) = healing_item(host, false)
        {
            self.turn.healed = true;
            self.say("Tactician", "Healing below 40% HP");
            return Some(BotAction::UseItem { item });
        }
        let Some(target) = pick_target(host) else {
            if !self.turn.asked_end {
                self.turn.asked_end = true;
                self.say("Tactician", "Asking to end combat: no one left to fight");
                return Some(BotAction::EndCombat);
            }
            self.say("Tactician", "Ending the turn: no target");
            return Some(BotAction::EndTurn);
        };
        let label = label_of(host, target);
        self.say("Tactician", format!("Fighting {label}"));
        self.note_target(host, target);
        if self.turn.duds >= DUDS_TO_END {
            if !self.turn.asked_end {
                self.turn.asked_end = true;
                self.say("Tactician", "Asking to end combat: no attack lands");
                return Some(BotAction::EndCombat);
            }
            return Some(BotAction::EndTurn);
        }
        if self.turn.duds >= DUDS_TO_FISTS {
            self.status.tactic = "Dropping to fists: the weapon does not fire".to_owned();
            return self.fists(host, target);
        }
        let (mode, weapon) = attack_mode(host);
        let Ok(attack) = world.attack_profile(player, target, mode, location::UNCALLED, false)
        else {
            return Some(BotAction::EndTurn);
        };
        match world.preflight(player, target, &attack) {
            Ok(()) => Some(self.strike(host, target, mode, &attack)),
            Err(Refusal::NoAmmunition) => {
                if let Some(weapon) = weapon
                    && ap >= 2
                    && !self.turn.reloaded
                    && world.first_compatible_ammo(player, weapon).is_some()
                {
                    self.turn.reloaded = true;
                    self.status.tactic = "Reloading an empty weapon".to_owned();
                    return world.get(weapon).map(|object| BotAction::Reload {
                        weapon: object.raw.id,
                    });
                }
                self.fists(host, target)
            }
            // A weapon that does not reach the next hex is no weapon.
            Err(Refusal::Range) if attack.distance <= 1 => self.fists(host, target),
            Err(Refusal::Range | Refusal::Obstructed) => Some(self.close_in(host, target, &attack)),
            Err(_) => {
                self.status.tactic = "Ending the turn: the attack is refused".to_owned();
                Some(BotAction::EndTurn)
            }
        }
    }

    /// An empty hand when the weapon cannot fire.
    fn fists<A: WorldAssets>(
        &mut self,
        host: &WorldHost<A>,
        target: ObjectRef,
    ) -> Option<BotAction> {
        let world = &host.world;
        let player = world.player;
        let mode = hit_mode::PUNCH;
        let Ok(attack) = world.attack_profile(player, target, mode, location::UNCALLED, false)
        else {
            return Some(BotAction::EndTurn);
        };
        Some(match world.preflight(player, target, &attack) {
            Ok(()) => self.strike(host, target, mode, &attack),
            Err(Refusal::Range) => self.close_in(host, target, &attack),
            Err(_) => BotAction::EndTurn,
        })
    }

    /// The attack: now and then a called shot, when it is on offer and the
    /// AP run to it.
    fn strike<A: WorldAssets>(
        &mut self,
        host: &WorldHost<A>,
        target: ObjectRef,
        mode: u8,
        attack: &Attack,
    ) -> BotAction {
        let world = &host.world;
        let player = world.player;
        let id = world.get(target).map_or(0, |object| object.raw.id);
        let cost = attack.ap_cost();
        self.turn.struck_at = Some(world.action_points(player));
        let aimed = world
            .attack_profile(player, target, mode, location::UNCALLED, true)
            .is_ok_and(|aimed| {
                aimed.aimed_shot_available() && world.action_points(player) >= aimed.ap_cost()
            });
        let location = (aimed && self.roll(4) == 0).then(|| self.roll(8) as u8);
        self.status.tactic = format!(
            "Attacking for {cost} AP at range {} of {}",
            attack.distance,
            attack.range()
        );
        BotAction::Attack {
            target: id,
            hit_mode: mode,
            location,
        }
    }

    /// Walk into reach, keeping the attack's AP when they run to it.
    fn close_in<A: WorldAssets>(
        &mut self,
        host: &WorldHost<A>,
        target: ObjectRef,
        attack: &Attack,
    ) -> BotAction {
        let world = &host.world;
        let player = world.player;
        self.turn.moves += 1;
        let Some(hex) = world.get(target).and_then(|object| object.raw.hex) else {
            return BotAction::EndTurn;
        };
        let (Some(movement), Some((start, _))) =
            (host.plan_to_tile(player, hex, usize::MAX), place_of(host))
        else {
            return BotAction::EndTurn;
        };
        if self.turn.moves > TURN_MOVES || movement.directions.is_empty() {
            self.status.tactic = "Ending the turn: no way into reach".to_owned();
            return BotAction::EndTurn;
        }
        let route = route_hexes(start, &movement.directions);
        let ap = usize::try_from(world.action_points(player)).unwrap_or(0);
        let spare = ap.saturating_sub(usize::try_from(attack.ap_cost()).unwrap_or(0));
        let need = usize::try_from(attack.distance - attack.range())
            .unwrap_or(1)
            .max(1);
        let steps = match spare {
            0 => need.min(ap),
            spare => need.min(spare),
        }
        .clamp(1, route.len() - 1);
        let stop = route[steps];
        self.status.target_hex = Some(stop);
        self.status.route = route[..=steps].to_vec();
        self.status.tactic = format!("Closing {steps} hexes");
        BotAction::Walk {
            hex: stop,
            run: false,
        }
    }

    fn note_target<A: WorldAssets>(&mut self, host: &WorldHost<A>, target: ObjectRef) {
        self.status.target_hex = host.world.get(target).and_then(|object| object.raw.hex);
        self.status.route.clear();
    }

    // Upkeep between fights.

    fn tend<A: WorldAssets>(&mut self, host: &WorldHost<A>) -> Option<BotAction> {
        let world = &host.world;
        let player = world.player;
        if hurt(host)
            && let Some(item) = healing_item(host, true)
            && self.tend_once(item)
        {
            self.say("Upkeep", "Healing below 40% HP");
            return Some(BotAction::UseItem { item });
        }
        let (mode, weapon) = attack_mode(host);
        let Some(weapon) = weapon else {
            let item = carried_weapon(host)?;
            if !self.tend_once(item) {
                return None;
            }
            self.say("Upkeep", "Taking up a weapon");
            return Some(BotAction::Equip { item });
        };
        let empty = world
            .weapon_profile(weapon, mode)
            .is_some_and(|profile| profile.capacity > 0 && profile.rounds == 0);
        let id = world.get(weapon)?.raw.id;
        if empty && world.first_compatible_ammo(player, weapon).is_some() && self.tend_once(id) {
            self.say("Upkeep", "Reloading");
            return Some(BotAction::Reload { weapon: id });
        }
        None
    }

    /// Whether `item` may have another try: each gets two on a map.
    fn tend_once(&mut self, item: u32) -> bool {
        let tries = self.memory().tended.entry(item).or_insert(0);
        *tries += 1;
        *tries <= 2
    }

    // The goal spine.

    /// One pass over the goal in hand: `Idle` when it has just ended, `Wait`
    /// when there is nothing left to want.
    fn spine<A: WorldAssets>(&mut self, host: &WorldHost<A>) -> Flow {
        if self.goal.is_none() {
            self.goal = self.next_goal(host);
            self.attempts = 0;
            self.walks = 0;
        }
        let Some(goal) = self.goal else {
            self.say("Explorer", "Nothing left to do on this map");
            self.clear_target();
            return Flow::Wait;
        };
        match self.pursue(host, goal) {
            Pursuit::Act(action) => {
                self.attempts += 1;
                return Flow::Act(action);
            }
            Pursuit::Hop(action) if self.walks < WALK_HOPS => {
                self.walks += 1;
                return Flow::Act(action);
            }
            Pursuit::Hop(_) | Pursuit::Done | Pursuit::Failed => {}
        }
        self.memory().tried.insert(goal);
        self.goal = None;
        Flow::Idle
    }

    fn next_pickup<A: WorldAssets>(&self, host: &WorldHost<A>) -> Option<Goal> {
        let world = &host.world;
        self.nearest(host, |object, reference| {
            (wanted(object) && world.add_refusal(world.player, reference, 1).is_none())
                .then_some(Goal::Pickup(object.raw.id))
        })
    }

    /// The closest object on the player's elevation, on the ground, that
    /// `goal` makes a goal not yet tried.
    fn nearest<A: WorldAssets>(
        &self,
        host: &WorldHost<A>,
        goal: impl Fn(&WorldObject, ObjectRef) -> Option<Goal>,
    ) -> Option<Goal> {
        let world = &host.world;
        let player = world.player;
        let (_, elevation) = place_of(host)?;
        world
            .iter()
            .filter(|(reference, object)| {
                *reference != player
                    && object.owner.is_none()
                    && object.raw.hex.is_some()
                    && object.raw.elevation == elevation
            })
            .filter_map(|(reference, object)| {
                let goal = goal(object, reference)?;
                (!self.tried(&goal)).then(|| (world.distance(player, reference), goal))
            })
            .min()
            .map(|(_, goal)| goal)
    }

    /// What to do next: take what lies around, talk, fight what cannot talk,
    /// open the way, look around, and leave.
    fn next_goal<A: WorldAssets>(&mut self, host: &WorldHost<A>) -> Option<Goal> {
        let world = &host.world;
        let player = world.player;
        if let Some(goal) = self.next_pickup(host) {
            return Some(goal);
        }
        let team = world.critter_state(player).map_or(0, |state| state.team);
        let talker = self.nearest(host, |object, reference| {
            (is_critter(object)
                && !world.is_dead(reference)
                && !world.party.contains(&reference)
                && world.script_for(reference).is_some())
            .then_some(Goal::Talk(object.raw.id))
        });
        if talker.is_some() {
            return talker;
        }
        let foe = self.nearest(host, |object, reference| {
            let other = world
                .critter_state(reference)
                .is_some_and(|state| state.team != team);
            (is_critter(object)
                && other
                && !world.is_dead(reference)
                && !world.party.contains(&reference)
                && world.script_for(reference).is_none())
            .then_some(Goal::Fight(object.raw.id))
        });
        if foe.is_some() {
            return foe;
        }
        let survey = survey(host);
        for door in &survey.doors {
            let goal = Goal::Door(world.get(*door)?.raw.id);
            if !self.tried(&goal) {
                return Some(goal);
            }
        }
        if self.memory().hops < EXPLORE_HOPS {
            let (_, elevation) = place_of(host)?;
            let seen = &self.memory().seen;
            let hexes: Vec<i32> = survey
                .hexes
                .iter()
                .filter(|(hex, depth)| *depth >= EXPLORE_FROM && !seen.contains(&(elevation, *hex)))
                .take(EXPLORE_CHOICES)
                .map(|(hex, _)| *hex)
                .collect();
            if !hexes.is_empty() {
                self.memory().hops += 1;
                return Some(Goal::Explore(hexes[self.roll(hexes.len())]));
            }
        }
        self.next_exit(host).or_else(|| {
            self.nearest(host, |object, _| {
                is_ladder(object).then_some(Goal::Climb(object.raw.id))
            })
        })
    }

    /// The exit grid used least, one to a map before one to the world map.
    fn next_exit<A: WorldAssets>(&mut self, host: &WorldHost<A>) -> Option<Goal> {
        let world = &host.world;
        let (_, elevation) = place_of(host)?;
        let memory = self.maps.get(&self.map);
        world
            .iter()
            .filter(|(_, object)| {
                skills::is_exit_grid(object.raw.pid)
                    && object.raw.elevation == elevation
                    && object.owner.is_none()
            })
            .filter_map(|(_, object)| {
                let hex = object.raw.hex?;
                let uses = memory.and_then(|memory| memory.exits.get(&hex).copied());
                let goal = Goal::Exit(hex);
                (!self.tried(&goal)).then_some((uses.unwrap_or(0), world_exit(object), hex, goal))
            })
            .min()
            .map(|(.., goal)| goal)
    }

    /// One step of the goal in hand.
    fn pursue<A: WorldAssets>(&mut self, host: &WorldHost<A>, goal: Goal) -> Pursuit {
        let world = &host.world;
        let object_of = |id: u32| world.reference_for_id(id);
        match goal {
            Goal::Pickup(id) => {
                let Some(item) = object_of(id) else {
                    return Pursuit::Done;
                };
                if world.get(item).is_none_or(|object| object.owner.is_some()) {
                    return Pursuit::Done;
                }
                if self.attempts >= 2 {
                    return Pursuit::Failed;
                }
                let label = label_of(host, item);
                self.say("Explorer", format!("Picking up {label}"));
                if let Some(pursuit) = self.go_to(host, item) {
                    return pursuit;
                }
                Pursuit::Act(BotAction::Interact {
                    target: id,
                    verb: BotVerb::Pickup,
                })
            }
            Goal::Talk(id) => {
                let Some(critter) = object_of(id) else {
                    return Pursuit::Done;
                };
                if self.attempts >= 1 {
                    return Pursuit::Done;
                }
                let label = label_of(host, critter);
                self.say("Explorer", format!("Talking to {label}"));
                if let Some(pursuit) = self.go_to(host, critter) {
                    return pursuit;
                }
                Pursuit::Act(BotAction::Interact {
                    target: id,
                    verb: BotVerb::Talk,
                })
            }
            Goal::Fight(id) => self.pick_fight(host, id),
            Goal::Door(id) => {
                let Some(door) = object_of(id) else {
                    return Pursuit::Done;
                };
                if self.attempts >= 2
                    || world
                        .get(door)
                        .and_then(WorldObject::lock)
                        .is_some_and(|lock| lock.contains(LockFlags::OPEN))
                {
                    return Pursuit::Done;
                }
                let label = label_of(host, door);
                self.say("Explorer", format!("Opening {label}"));
                if let Some(pursuit) = self.go_to(host, door) {
                    return pursuit;
                }
                Pursuit::Act(BotAction::Interact {
                    target: id,
                    verb: BotVerb::Use,
                })
            }
            Goal::Explore(hex) => {
                if self.attempts >= 2 || place_of(host).is_some_and(|(at, _)| at == hex) {
                    return Pursuit::Done;
                }
                self.say("Explorer", format!("Exploring toward hex {hex}"));
                match self.way_to(host, hex) {
                    Way::Direct => Pursuit::Act(BotAction::Walk { hex, run: true }),
                    Way::Hop(hex) => Pursuit::Hop(BotAction::Walk { hex, run: true }),
                    Way::Closed => Pursuit::Failed,
                }
            }
            Goal::Exit(hex) => {
                if self.attempts >= 2 {
                    return Pursuit::Failed;
                }
                self.say("Explorer", format!("Leaving by the exit grid at hex {hex}"));
                match self.way_to(host, hex) {
                    Way::Direct => {
                        *self.memory().exits.entry(hex).or_insert(0) += 1;
                        Pursuit::Act(BotAction::Walk { hex, run: true })
                    }
                    Way::Hop(hex) => Pursuit::Hop(BotAction::Walk { hex, run: true }),
                    Way::Closed => Pursuit::Failed,
                }
            }
            Goal::Climb(id) => {
                let Some(ladder) = object_of(id) else {
                    return Pursuit::Done;
                };
                if self.attempts >= 1 {
                    return Pursuit::Done;
                }
                let label = label_of(host, ladder);
                self.say("Explorer", format!("Climbing {label}"));
                if let Some(pursuit) = self.go_to(host, ladder) {
                    return pursuit;
                }
                Pursuit::Act(BotAction::Interact {
                    target: id,
                    verb: BotVerb::Use,
                })
            }
        }
    }

    /// Start a fight with something that cannot be talked to.
    fn pick_fight<A: WorldAssets>(&mut self, host: &WorldHost<A>, id: u32) -> Pursuit {
        let world = &host.world;
        let player = world.player;
        let Some(target) = world.reference_for_id(id) else {
            return Pursuit::Done;
        };
        if world.is_dead(target) {
            return Pursuit::Done;
        }
        if self.attempts >= 6 {
            return Pursuit::Failed;
        }
        let (mode, _) = attack_mode(host);
        let Ok(attack) = world.attack_profile(player, target, mode, location::UNCALLED, false)
        else {
            return Pursuit::Failed;
        };
        let label = label_of(host, target);
        self.say("Explorer", format!("Picking a fight with {label}"));
        self.aim_at(host, target);
        match world.preflight(player, target, &attack) {
            Ok(()) => Pursuit::Act(BotAction::Attack {
                target: id,
                hit_mode: mode,
                location: None,
            }),
            Err(Refusal::Range) if attack.distance <= 1 => Pursuit::Act(BotAction::Attack {
                target: id,
                hit_mode: hit_mode::PUNCH,
                location: None,
            }),
            Err(Refusal::Range | Refusal::Obstructed) => {
                match world.get(target).and_then(|object| object.raw.hex) {
                    Some(hex) => Pursuit::Act(BotAction::Walk { hex, run: true }),
                    None => Pursuit::Failed,
                }
            }
            Err(_) => Pursuit::Failed,
        }
    }

    /// Show the way to `hex`, and whether there is one.
    fn plan<A: WorldAssets>(&mut self, host: &WorldHost<A>, hex: i32) -> bool {
        self.status.target_hex = Some(hex);
        self.status.route.clear();
        let Some((start, _)) = place_of(host) else {
            return false;
        };
        let Some(movement) = host.plan_to_tile(host.world.player, hex, usize::MAX) else {
            return false;
        };
        self.status.route = route_hexes(start, &movement.directions);
        !movement.directions.is_empty() || start == hex
    }

    /// Show the way to an object; the action walks there by itself.
    fn aim_at<A: WorldAssets>(&mut self, host: &WorldHost<A>, object: ObjectRef) {
        if let Some(hex) = host.world.get(object).and_then(|object| object.raw.hex) {
            self.plan(host, hex);
        }
    }

    /// How to get to `hex`, shown as the plan.
    fn way_to<A: WorldAssets>(&mut self, host: &WorldHost<A>, hex: i32) -> Way {
        let Some((start, _)) = place_of(host) else {
            return Way::Closed;
        };
        if self.plan(host, hex) || HexGrid::angular_distance(start, hex).is_ok_and(|d| d <= 1) {
            return Way::Direct;
        }
        let Some(path) = walking_path(host, start, hex) else {
            return Way::Closed;
        };
        let stop = HOP_STEPS.min(path.len().saturating_sub(2)).max(1);
        match path.get(stop) {
            Some(hop) => {
                self.plan(host, *hop);
                Way::Hop(*hop)
            }
            None => Way::Closed,
        }
    }

    /// What a goal on `object` does before its action: `None` when one
    /// action can reach it from here.
    fn go_to<A: WorldAssets>(&mut self, host: &WorldHost<A>, object: ObjectRef) -> Option<Pursuit> {
        let hex = host.world.get(object).and_then(|object| object.raw.hex)?;
        match self.way_to(host, hex) {
            Way::Direct => None,
            Way::Hop(hex) => Some(Pursuit::Hop(BotAction::Walk { hex, run: true })),
            Way::Closed => Some(Pursuit::Failed),
        }
    }

    // The chaos audit.

    /// One map and elevation's audit, planned once. Doors and containers in
    /// `raw.id` order, then the bombs, on doors a script can destroy first.
    fn plan_audits<A: WorldAssets>(&mut self, host: &WorldHost<A>) {
        let Some((_, elevation)) = place_of(host) else {
            return;
        };
        if !self.audited.insert((self.map.clone(), elevation)) {
            return;
        }
        let world = &host.world;
        let mut targets: Vec<(u32, ObjectRef)> = world
            .iter()
            .filter(|(_, object)| {
                object.owner.is_none()
                    && object.raw.hex.is_some()
                    && object.raw.elevation == elevation
                    && (is_door(object) || is_container(object))
            })
            .map(|(reference, object)| (object.raw.id, reference))
            .collect();
        targets.sort_unstable();
        let mut bombs = Vec::new();
        for (id, reference) in targets {
            let Some(object) = world.get(reference) else {
                continue;
            };
            let lock = object.lock().unwrap_or(LockFlags(0));
            let sealed = lock.contains(LockFlags::LOCKED) || lock.contains(LockFlags::JAMMED);
            let mut steps = VecDeque::new();
            if sealed {
                steps.extend([Check::Lockpick, Check::Crowbar]);
            }
            steps.push_back(Check::Swing);
            // A locked container stays shut, so there is nothing to undo.
            if is_door(object) || !lock.contains(LockFlags::LOCKED) {
                steps.push_back(Check::Swing);
            }
            let label = label_of(host, reference);
            self.audits.push_back(Audit {
                id,
                label,
                steps,
                walks: 0,
                sent: None,
            });
            if is_door(object) {
                let destructible = world
                    .script_for(reference)
                    .is_some_and(|script| host.has_procedure(script, ScriptEvent::Damage));
                bombs.push((!destructible, id, label_of(host, reference)));
            }
        }
        bombs.sort_unstable();
        bombs.truncate(BOMBS_PER_MAP);
        for (_, id, label) in bombs {
            self.audits.push_back(Audit {
                id,
                label,
                steps: VecDeque::from([Check::Bomb]),
                walks: 0,
                sent: None,
            });
        }
    }

    fn audit<A: WorldAssets>(&mut self, host: &WorldHost<A>) -> Flow {
        match self.run_bomb(host) {
            Flow::Idle => {}
            flow => return flow,
        }
        self.plan_audits(host);
        let elevation = place_of(host).map_or(0, |(_, elevation)| elevation);
        while let Some(mut audit) = self.audits.pop_front() {
            let Some(reference) = host.world.reference_for_id(audit.id) else {
                continue;
            };
            if host
                .world
                .get(reference)
                .is_none_or(|object| object.raw.elevation != elevation)
            {
                continue;
            }
            if let Some(sent) = audit.sent.take()
                && !self.verify(host, reference, &audit, &sent)
            {
                continue;
            }
            if audit.steps.is_empty() {
                continue;
            }
            let Some(hex) = host.world.get(reference).and_then(|object| object.raw.hex) else {
                continue;
            };
            match self.way_to(host, hex) {
                Way::Hop(stop) if audit.walks < WALK_HOPS => {
                    audit.walks += 1;
                    self.say("Chaos", format!("Walking to {}", audit.label));
                    self.audits.push_front(audit);
                    return Flow::Act(BotAction::Walk {
                        hex: stop,
                        run: true,
                    });
                }
                Way::Direct => {}
                _ => continue,
            }
            while let Some(check) = audit.steps.pop_front() {
                if let Some(action) = self.start_check(host, reference, &mut audit, check) {
                    self.audits.push_front(audit);
                    return Flow::Act(action);
                }
            }
        }
        self.clear_target();
        Flow::Idle
    }

    /// Send a check's action, or skip a check that cannot run.
    fn start_check<A: WorldAssets>(
        &mut self,
        host: &WorldHost<A>,
        reference: ObjectRef,
        audit: &mut Audit,
        check: Check,
    ) -> Option<BotAction> {
        let world = &host.world;
        let object = world.get(reference)?;
        let (hex, elevation) = (object.raw.hex?, object.raw.elevation);
        let before = Frozen::of(object);
        let label = audit.label.clone();
        let (action, probe) = match check {
            Check::Lockpick => {
                self.say("Chaos", format!("Picking the lock of {label}"));
                let action = BotAction::UseSkill {
                    skill: skill::LOCKPICK,
                    target: audit.id,
                };
                (action, None)
            }
            Check::Crowbar => {
                let item = carried_crowbar(host)?;
                self.say("Chaos", format!("Forcing {label} with a Crowbar"));
                let action = BotAction::UseItemOn {
                    item,
                    target: audit.id,
                };
                (action, None)
            }
            Check::Swing => {
                // A door that permits no use refuses the action.
                if is_door(object)
                    && !object
                        .prototype
                        .as_ref()
                        .is_some_and(|prototype| prototype.permits_use())
                {
                    return None;
                }
                self.say("Chaos", format!("Swinging {label}"));
                let probe = is_door(object)
                    .then(|| door_probe(host, hex, elevation))
                    .flatten();
                if is_door(object) && !before.open() {
                    self.check_passage(host, reference, &label, hex, elevation, probe);
                }
                let action = BotAction::Interact {
                    target: audit.id,
                    verb: BotVerb::Use,
                };
                (action, probe)
            }
            Check::Bomb => {
                return match self.start_bomb(host, audit) {
                    Flow::Act(action) => Some(action),
                    _ => None,
                };
            }
        };
        self.aim_at(host, reference);
        audit.sent = Some(Sent {
            check,
            before,
            probe,
            hex,
            elevation,
        });
        Some(action)
    }

    /// Whether the check's action reached its object; if not, the rest of
    /// the audit of that object cannot run either.
    fn verify<A: WorldAssets>(
        &mut self,
        host: &WorldHost<A>,
        reference: ObjectRef,
        audit: &Audit,
        sent: &Sent,
    ) -> bool {
        let world = &host.world;
        let label = &audit.label;
        if world.distance(world.player, reference) > 1 {
            self.say("Chaos", format!("Could not reach {label}"));
            return false;
        }
        let Some(object) = world.get(reference) else {
            return false;
        };
        let now = Frozen::of(object);
        let scripted = world.script_for(reference).is_some();
        match sent.check {
            Check::Lockpick if !scripted => self.expect(now == sent.before, || {
                format!("Lockpick changed {label} with no script: {now:?}")
            }),
            Check::Crowbar if !scripted => self.expect(now == sent.before, || {
                format!("a Crowbar changed {label} with no script: {now:?}")
            }),
            Check::Swing if is_door(object) => {
                // A door with no art has no frames: its open bit stays set
                // and SPEC-007 §3.11.4 gives its next use nothing to do.
                let stuck_open = sent.before.open() && sent.before.frame == 0;
                if !scripted && !stuck_open {
                    self.expect(now.open() != sent.before.open(), || {
                        format!("{label} did not swing when used")
                    });
                }
                self.check_passage(host, reference, label, sent.hex, sent.elevation, sent.probe);
            }
            Check::Swing => self.verify_container(host, reference, label, &now, sent),
            _ => {}
        }
        true
    }

    /// A locked container stays as it was; an unlocked one with no script
    /// and art to play swings.
    fn verify_container<A: WorldAssets>(
        &mut self,
        host: &WorldHost<A>,
        reference: ObjectRef,
        label: &str,
        now: &Frozen,
        sent: &Sent,
    ) {
        let locked = now
            .lock
            .is_some_and(|lock| LockFlags(lock).contains(LockFlags::LOCKED));
        let frames = host
            .world
            .get(reference)
            .and_then(|object| host.assets.object_art(object.raw.art_id).ok())
            .map_or(0, |art| art.frame_count());
        if locked {
            self.expect(now.frame == sent.before.frame, || {
                format!("locked {label} swung when used")
            });
        } else if host.world.script_for(reference).is_none() && frames > 1 {
            self.expect(now.frame != sent.before.frame, || {
                format!("{label} did not swing when used")
            });
        }
    }

    /// A closed door blocks its hex and a route through it, unless the
    /// `enhanced` player may open it; an open one does neither.
    fn check_passage<A: WorldAssets>(
        &mut self,
        host: &WorldHost<A>,
        door: ObjectRef,
        label: &str,
        hex: i32,
        elevation: u8,
        probe: Option<[i32; 2]>,
    ) {
        let world = &host.world;
        let player = world.player;
        let Some(object) = world.get(door) else {
            return;
        };
        let open = Frozen::of(object).open();
        let blocker = world.movement_blocker(hex, elevation, player);
        if open {
            self.expect(blocker != Some(door), || {
                format!("{label} is open but blocks hex {hex}")
            });
        } else if !object.raw.flags.contains(BasicFlags::NONBLOCKING) {
            self.expect(blocker.is_some(), || {
                format!("{label} is closed but nothing blocks hex {hex}")
            });
        }
        let Some([from, to]) = probe else {
            return;
        };
        let through = route_hexes(from, &world.route(player, from, to, false)).contains(&hex);
        if open && blocker.is_none() {
            self.expect(through, || {
                format!("no route takes open {label} at hex {hex}")
            });
        } else if !open && !world.may_auto_open(player, door) {
            self.expect(!through, || {
                format!("a player route passes closed {label} at hex {hex}")
            });
        }
    }

    fn start_bomb<A: WorldAssets>(&mut self, host: &WorldHost<A>, audit: &Audit) -> Flow {
        let Some((item, name)) = carried_explosive(host) else {
            return Flow::Idle;
        };
        self.bomb = Some(Bomb {
            target: audit.id,
            label: audit.label.clone(),
            item,
            name,
            stage: BombStage::Approach,
            tries: 0,
        });
        self.run_bomb(host)
    }

    fn run_bomb<A: WorldAssets>(&mut self, host: &WorldHost<A>) -> Flow {
        let Some(mut bomb) = self.bomb.take() else {
            return Flow::Idle;
        };
        let world = &host.world;
        let player = world.player;
        self.say(
            "Chaos",
            format!("Testing {} with {}", bomb.label, bomb.name),
        );
        if bomb.stage == BombStage::Approach {
            let Some(door) = world.reference_for_id(bomb.target) else {
                return Flow::Idle;
            };
            if world.distance(player, door) <= 1 {
                let Some(item) = world.reference_for_id(bomb.item) else {
                    return Flow::Idle;
                };
                let Some((hex, elevation)) = place_of(host) else {
                    return Flow::Idle;
                };
                self.fuse = Some(light_fuse(host, item, &bomb, hex, elevation));
                let action = BotAction::ArmExplosive {
                    item: bomb.item,
                    seconds: FUSE_SECONDS,
                };
                bomb.stage = BombStage::Retreat;
                bomb.tries = 0;
                self.bomb = Some(bomb);
                return Flow::Act(action);
            }
            let Some(hex) = world.get(door).and_then(|object| object.raw.hex) else {
                return Flow::Idle;
            };
            bomb.tries += 1;
            if bomb.tries > WALK_HOPS {
                return Flow::Idle;
            }
            let stop = match self.way_to(host, hex) {
                Way::Direct => hex,
                Way::Hop(stop) => stop,
                Way::Closed => return Flow::Idle,
            };
            self.bomb = Some(bomb);
            return Flow::Act(BotAction::Walk {
                hex: stop,
                run: true,
            });
        }
        let Some(fuse) = &self.fuse else {
            return Flow::Idle;
        };
        let bomb_hex = fuse.hex;
        if bomb.stage == BombStage::Retreat {
            let clear = |at: i32| {
                HexGrid::angular_distance(at, bomb_hex)
                    .is_ok_and(|distance| distance >= SAFE_DISTANCE)
            };
            bomb.tries += 1;
            if bomb.tries > 3 || place_of(host).is_none_or(|(at, _)| clear(at)) {
                bomb.stage = BombStage::Waiting;
            } else if let Some((hex, _)) =
                survey(host).hexes.into_iter().find(|(hex, _)| clear(*hex))
                && self.plan(host, hex)
            {
                self.bomb = Some(bomb);
                return Flow::Act(BotAction::Walk { hex, run: true });
            } else {
                bomb.stage = BombStage::Waiting;
            }
        }
        self.bomb = Some(bomb);
        self.waiting = true;
        Flow::Wait
    }

    /// Follow the fuse in hand. A fuse stopped by combat or dialogue is not
    /// overdue, however long that takes.
    fn watch_fuse<A: WorldAssets>(&mut self, host: &WorldHost<A>) {
        let Some(mut fuse) = self.fuse.take() else {
            return;
        };
        let world = &host.world;
        let burning = world
            .get(fuse.item)
            .filter(|object| object.raw.id == fuse.id);
        if let Some(object) = burning {
            if !fuse.dropped {
                if !object.raw.flags.contains(BasicFlags::USED) {
                    // Still in the pack: the action was refused.
                    return;
                }
                let on_ground = object.owner.is_none() && object.raw.hex == Some(fuse.hex);
                let label = fuse.label.clone();
                self.expect(on_ground, || {
                    format!("the explosive for {label} is not where it was dropped")
                });
                fuse.dropped = true;
            }
            fuse.bystanders = bystanders(host, fuse.hex, fuse.elevation);
            let ticks = world.clock.ticks;
            if world.explosion.is_some() || !world.shows.is_empty() {
                let retry = skills::EXPLOSION_RETRY_TICKS;
                fuse.deadline = fuse.deadline.max(ticks + retry + FUSE_SLACK_TICKS);
            }
            // The queue stands still, while a combat round still adds to
            // the clock: the fuse gets its chance when they are over.
            if world.combat || world.dialogue.open {
                fuse.deadline = fuse.deadline.max(ticks + FUSE_SLACK_TICKS);
            }
            if !world.queue.events().any(|event| event.object == fuse.item) {
                self.fail(format!(
                    "the armed explosive for {} has no event queued",
                    fuse.label
                ));
            } else if ticks > fuse.deadline {
                self.fail(format!(
                    "the fuse for {} is overdue at tick {ticks}",
                    fuse.label
                ));
            } else {
                self.fuse = Some(fuse);
            }
            return;
        }
        if let Some(explosion) = world
            .explosion
            .as_ref()
            .filter(|explosion| explosion.hex == fuse.hex)
        {
            if fuse.victims.is_none() {
                fuse.victims = Some(
                    explosion
                        .victims
                        .iter()
                        .map(|(victim, damage, _)| (*victim, *damage))
                        .collect(),
                );
                fuse.bystanders = bystanders(host, fuse.hex, fuse.elevation);
            }
            fuse.burned += 1;
            if fuse.burned > EXPLOSION_DECISIONS {
                self.fail(format!("the explosion at {} never ended", fuse.label));
                return;
            }
            self.fuse = Some(fuse);
            return;
        }
        self.verify_blast(host, &fuse);
    }

    /// Everything a finished explosion must have left behind (SPEC-013
    /// §3.10): the explosive and its event gone, no tile changed, the
    /// critters in reach hurt and those beyond it not, every script in reach
    /// notified, and every other object as it was.
    fn verify_blast<A: WorldAssets>(&mut self, host: &WorldHost<A>, fuse: &Fuse) {
        let world = &host.world;
        let label = &fuse.label;
        self.expect(world.reference_for_id(fuse.id).is_none(), || {
            format!("the explosive for {label} outlived its explosion")
        });
        self.expect(world.entity_key(fuse.item).is_none(), || {
            format!("the explosive for {label} left a ghost reference")
        });
        self.expect(
            !world.queue.events().any(|event| {
                event.object == fuse.item
                    || matches!(
                        event.kind,
                        EventKind::Other(skills::EXPLOSION_EVENT | skills::PREMATURE_EVENT)
                    ) && world.get(event.object).is_none()
            }),
            || format!("an armed event for {label} outlived its explosive"),
        );
        self.expect(fuse.tiles == tile_art(host, fuse.elevation), || {
            format!("the explosion at {label} changed tiles")
        });
        self.verify_victims(host, fuse);
        for neighbor in &fuse.neighbors {
            self.verify_neighbor(host, fuse, neighbor);
        }
    }

    fn verify_victims<A: WorldAssets>(&mut self, host: &WorldHost<A>, fuse: &Fuse) {
        let world = &host.world;
        let label = &fuse.label;
        let before = |critter: ObjectRef| {
            fuse.bystanders
                .iter()
                .find(|bystander| bystander.critter == critter)
        };
        let hurt_by = |critter: ObjectRef, amount: i32, hp: i32| {
            world.get(critter).is_none()
                || world.is_dead(critter)
                || world
                    .hit_points(critter)
                    .is_some_and(|now| now <= hp - amount)
        };
        if let Some(victims) = &fuse.victims {
            let reached: BTreeSet<ObjectRef> = fuse
                .bystanders
                .iter()
                .filter(|bystander| bystander.distance <= BLAST_RADIUS)
                .map(|bystander| bystander.critter)
                .collect();
            let listed: BTreeSet<ObjectRef> = victims.iter().map(|victim| victim.0).collect();
            self.expect(reached == listed, || {
                format!("the blast at {label} chose {listed:?}, not {reached:?}")
            });
            for (victim, damage) in victims {
                if *damage > 0
                    && let Some(bystander) = before(*victim)
                {
                    let (id, hp) = (bystander.id, bystander.hp);
                    self.expect(hurt_by(*victim, *damage, hp), || {
                        format!("critter #{id} near {label} took no blast damage")
                    });
                }
            }
        } else {
            // The explosion came and went between decisions, so positions
            // are a tick old: one hex of slack, and the least it could do.
            let (least, _) = skills::explosion_range(fuse.pid);
            for bystander in &fuse.bystanders {
                if bystander.distance < BLAST_RADIUS {
                    let critter = bystander.critter;
                    let index = damage_type::EXPLOSION as usize;
                    let threshold = world.effective(critter, THRESHOLDS + index);
                    let resistance = world.effective(critter, RESISTANCES + index);
                    let multihex = world.multihex(critter);
                    let (damage, _) =
                        skills::explosion_damage(least, threshold, resistance, multihex);
                    if damage > 0 {
                        let (id, hp) = (bystander.id, bystander.hp);
                        self.expect(hurt_by(critter, 1, hp), || {
                            format!("critter #{id} near {label} took no blast damage")
                        });
                    }
                }
            }
        }
        // Combat that followed may have hurt anyone.
        if !world.combat && world.pending_combat.is_none() {
            for bystander in &fuse.bystanders {
                if bystander.distance > BLAST_RADIUS + 1
                    && let Some(hp) = world.hit_points(bystander.critter)
                {
                    let id = bystander.id;
                    self.expect(hp >= bystander.hp, || {
                        format!("critter #{id} outside the blast at {label} was hurt")
                    });
                }
            }
        }
    }

    fn verify_neighbor<A: WorldAssets>(
        &mut self,
        host: &WorldHost<A>,
        fuse: &Fuse,
        neighbor: &Neighbor,
    ) {
        let world = &host.world;
        let player = world.player;
        let name = &neighbor.label;
        let near = neighbor.distance <= BLAST_RADIUS;
        let notices = damage_notices(host, neighbor.script);
        let object = world
            .get(neighbor.object)
            .filter(|object| object.raw.id == neighbor.id);
        if neighbor.procedure && near {
            // The trace is bounded: a full one may have dropped older runs.
            let ran = notices > neighbor.notices
                || (neighbor.notices > 0 && world.trace.len() >= TRACE_RETAINED);
            self.expect(ran, || format!("damage_p_proc of {name} did not run"));
        } else {
            self.expect(notices <= neighbor.notices, || {
                format!("damage_p_proc of {name} ran from outside the blast")
            });
            let locked = |frozen: &Frozen| {
                frozen
                    .lock
                    .map(|lock| lock & LockFlags::LOCKED.0 == LockFlags::LOCKED.0)
            };
            let same = object.is_some_and(|object| {
                let now = Frozen::of(object);
                now.hex == neighbor.before.hex && locked(&now) == locked(&neighbor.before)
            });
            self.expect(same, || {
                format!("the blast changed {name}, which nothing notified")
            });
        }
        if object.is_some() {
            return;
        }
        self.expect(world.reference_for_id(neighbor.id).is_none(), || {
            format!("destroyed {name} still answers to its id")
        });
        self.expect(world.entity_key(neighbor.object).is_none(), || {
            format!("destroyed {name} left a ghost reference")
        });
        if let Some(script) = neighbor.script {
            self.expect(!world.scripts.contains_key(&script), || {
                format!("destroyed {name} left its script behind")
            });
        }
        let blocker = world.movement_blocker(neighbor.hex, fuse.elevation, player);
        self.expect(
            blocker != Some(neighbor.object) && blocker.is_none_or(|b| world.get(b).is_some()),
            || format!("destroyed {name} still blocks hex {}", neighbor.hex),
        );
        if let (None, Some([from, to])) = (blocker, neighbor.probe) {
            let route = route_hexes(from, &world.route(player, from, to, false));
            self.expect(route.contains(&neighbor.hex), || {
                format!("no route takes the hex of destroyed {name}")
            });
        }
    }
}

/// The player's hex and elevation, on the map.
fn place_of<A: WorldAssets>(host: &WorldHost<A>) -> Option<(i32, u8)> {
    let object = host.world.get(host.world.player)?;
    let hex = object.raw.hex.filter(|hex| (0..HEXES).contains(hex))?;
    Some((hex, object.raw.elevation))
}

fn is_door(object: &WorldObject) -> bool {
    matches!(
        object.prototype.as_deref().map(|prototype| &prototype.data),
        Some(PrototypeData::Scenery(Scenery {
            data: SceneryData::Door { .. },
            ..
        }))
    )
}

fn is_ladder(object: &WorldObject) -> bool {
    matches!(
        object.prototype.as_deref().map(|prototype| &prototype.data),
        Some(PrototypeData::Scenery(Scenery {
            data: SceneryData::LadderUp { .. }
                | SceneryData::LadderDown { .. }
                | SceneryData::Stairs { .. },
            ..
        }))
    )
}

fn is_container(object: &WorldObject) -> bool {
    matches!(
        object.prototype.as_deref().map(|prototype| &prototype.data),
        Some(PrototypeData::Item(item)) if matches!(item.data, ItemData::Container { .. })
    )
}

fn is_critter(object: &WorldObject) -> bool {
    Fid(object.raw.art_id).category() == 1
}

/// Whether the bot wants this item from the ground. A fuse the bot lit stays
/// where it is.
fn wanted(object: &WorldObject) -> bool {
    let Some(PrototypeData::Item(item)) = object.prototype.as_deref().map(|p| &p.data) else {
        return false;
    };
    object.category() == Category::Item
        && !object.raw.flags.contains(BasicFlags::USED)
        && !matches!(
            object.raw.pid,
            skills::DYNAMITE_ARMED | skills::PLASTIC_ARMED
        )
        && !matches!(item.data, ItemData::Container { .. })
}

/// Whether an exit grid leads off the maps, to the world or town map.
fn world_exit(object: &WorldObject) -> bool {
    matches!(
        object.raw.state,
        vault_formats::map::ObjectState::Other {
            detail: vault_formats::map::ObjectDetail::ExitGrid { map, .. },
            ..
        } if map <= 0
    )
}

/// A short name for the banner: what it is, and its `raw.id`.
fn label_of<A: WorldAssets>(host: &WorldHost<A>, reference: ObjectRef) -> String {
    let Some(object) = host.world.get(reference) else {
        return format!("object {reference}");
    };
    let id = object.raw.id;
    if is_door(object) {
        return format!("Door #{id}");
    }
    if is_container(object) {
        return format!("Container #{id}");
    }
    if object.name.is_empty() {
        return if is_critter(object) {
            format!("Critter #{id}")
        } else {
            format!("Object #{id}")
        };
    }
    format!("{} #{id}", String::from_utf8_lossy(&object.name))
}

/// Whether the player is below [`HEAL_PERCENT`] of maximum HP.
fn hurt<A: WorldAssets>(host: &WorldHost<A>) -> bool {
    let world = &host.world;
    let player = world.player;
    let hp = world.hit_points(player).unwrap_or(0);
    hp.saturating_mul(100) < world.effective(player, MAX_HP).saturating_mul(HEAL_PERCENT)
}

fn carried<A: WorldAssets>(host: &WorldHost<A>) -> impl Iterator<Item = (ObjectRef, &WorldObject)> {
    let world = &host.world;
    world
        .get(world.player)
        .into_iter()
        .flat_map(|player| player.inventory.iter())
        .filter_map(|(item, _)| Some((*item, world.get(*item)?)))
}

/// A carried item that heals: a drug raising current HP at once, or, out of
/// combat, a kit (SPEC-013 §3.13 refuses it in combat). Named by `raw.id`.
fn healing_item<A: WorldAssets>(host: &WorldHost<A>, kits: bool) -> Option<u32> {
    carried(host)
        .find(|(_, object)| {
            let heals = match object.prototype.as_deref().map(|p| &p.data) {
                Some(PrototypeData::Item(item)) => match &item.data {
                    ItemData::Drug(drug) => {
                        let range = drug.stats[0] == drugs::RANGE_FORM;
                        drug.stats.iter().zip(drug.immediate).any(|(stat, amount)| {
                            *stat == drugs::CURRENT_HP && (amount > 0 || range)
                        })
                    }
                    _ => false,
                },
                _ => false,
            };
            heals || (kits && host.kit_skill(object.raw.pid) == Some(skill::FIRST_AID))
        })
        .map(|(_, object)| object.raw.id)
}

fn carried_weapon<A: WorldAssets>(host: &WorldHost<A>) -> Option<u32> {
    let mut weapons: Vec<u32> = carried(host)
        .filter(|(_, object)| {
            matches!(
                object.prototype.as_deref().map(|p| &p.data),
                Some(PrototypeData::Item(item)) if matches!(item.data, ItemData::Weapon(_))
            )
        })
        .map(|(_, object)| object.raw.id)
        .collect();
    weapons.sort_unstable();
    weapons.first().copied()
}

/// A carried Crowbar. It has no PID in the rules, so it goes by its name.
fn carried_crowbar<A: WorldAssets>(host: &WorldHost<A>) -> Option<u32> {
    carried(host)
        .find(|(_, object)| {
            object
                .name
                .to_ascii_lowercase()
                .windows(7)
                .any(|word| word == b"crowbar")
        })
        .map(|(_, object)| object.raw.id)
}

/// A carried explosive that is not armed, with the name the banner gives it.
fn carried_explosive<A: WorldAssets>(host: &WorldHost<A>) -> Option<(u32, &'static str)> {
    carried(host).find_map(|(_, object)| {
        if object.raw.flags.contains(BasicFlags::USED) {
            return None;
        }
        match object.raw.pid {
            skills::DYNAMITE => Some((object.raw.id, "Dynamite")),
            skills::PLASTIC => Some((object.raw.id, "C4")),
            _ => None,
        }
    })
}

/// The active hand's attack mode, and the weapon in it; bare hands punch.
fn attack_mode<A: WorldAssets>(host: &WorldHost<A>) -> (u8, Option<ObjectRef>) {
    let world = &host.world;
    let mode = if world.active_hand != 0 {
        hit_mode::RIGHT_PRIMARY
    } else {
        hit_mode::LEFT_PRIMARY
    };
    world
        .wielded(world.player, mode)
        .filter(|item| world.weapon_profile(*item, mode).is_some())
        .map_or((hit_mode::PUNCH, None), |item| (mode, Some(item)))
}

/// The combatant to go for: one fighting the player first, then the nearest
/// on another team, by `raw.id` when they tie.
fn pick_target<A: WorldAssets>(host: &WorldHost<A>) -> Option<ObjectRef> {
    let world = &host.world;
    let player = world.player;
    let team = |critter: ObjectRef| world.critter_state(critter).map_or(0, |state| state.team);
    let combat = &world.combat_state;
    combat
        .list
        .iter()
        .take(combat.active)
        .copied()
        .filter(|critter| {
            *critter != player && !world.is_dead(*critter) && team(*critter) != team(player)
        })
        .min_by_key(|critter| {
            let object = world.get(*critter);
            (
                object.is_none_or(|object| object.opponent != player),
                world.distance(player, *critter),
                object.map_or(0, |object| object.raw.id),
            )
        })
}

/// What can be walked to from the player without opening anything.
struct Survey {
    /// Hexes in walking order, with their steps from the player.
    hexes: Vec<(i32, u32)>,
    /// Closed doors at its edge, nearest first.
    doors: Vec<ObjectRef>,
}

fn survey<A: WorldAssets>(host: &WorldHost<A>) -> Survey {
    let world = &host.world;
    let player = world.player;
    let mut found = Survey {
        hexes: Vec::new(),
        doors: Vec::new(),
    };
    let Some((start, elevation)) = place_of(host) else {
        return found;
    };
    let occupancy = world.occupancy(elevation);
    let mut seen = vec![false; HEXES as usize];
    seen[start as usize] = true;
    let mut queue = VecDeque::from([(start, 0)]);
    while let Some((at, depth)) = queue.pop_front() {
        found.hexes.push((at, depth));
        if found.hexes.len() >= SURVEY_HEXES {
            break;
        }
        for direction in 0..6 {
            let next = step(at, direction);
            if !(0..HEXES).contains(&next) || seen[next as usize] {
                continue;
            }
            seen[next as usize] = true;
            match world.movement_blocker_in(&occupancy, next, player) {
                None => queue.push_back((next, depth + 1)),
                Some(blocker) => {
                    let closed = world
                        .get(blocker)
                        .is_some_and(|object| is_door(object) && !Frozen::of(object).open());
                    if closed && !found.doors.contains(&blocker) {
                        found.doors.push(blocker);
                    }
                }
            }
        }
    }
    found
}

/// The hexes from `start` to `goal`, both included, by the fewest steps that
/// open nothing. `goal` itself may be blocked, as a route's goal may.
fn walking_path<A: WorldAssets>(host: &WorldHost<A>, start: i32, goal: i32) -> Option<Vec<i32>> {
    let world = &host.world;
    let (_, elevation) = place_of(host)?;
    let occupancy = world.occupancy(elevation);
    let mut parents = vec![-1; HEXES as usize];
    parents[start as usize] = start;
    let mut queue = VecDeque::from([start]);
    while let Some(at) = queue.pop_front() {
        if at == goal {
            let mut path = vec![goal];
            while let Some(last) = path.last().copied().filter(|hex| *hex != start) {
                path.push(parents[last as usize]);
            }
            path.reverse();
            return Some(path);
        }
        for direction in 0..6 {
            let next = step(at, direction);
            if !(0..HEXES).contains(&next) || parents[next as usize] != -1 {
                continue;
            }
            let free = next == goal
                || world
                    .movement_blocker_in(&occupancy, next, world.player)
                    .is_none();
            if free {
                parents[next as usize] = at;
                queue.push_back(next);
            }
        }
    }
    None
}

/// Two hexes either side of a door, both free, so that the way between them
/// is through it when it is open. `None` when the door has no such pair.
fn door_probe<A: WorldAssets>(host: &WorldHost<A>, hex: i32, elevation: u8) -> Option<[i32; 2]> {
    let world = &host.world;
    let occupancy = world.occupancy(elevation);
    let free = |at: i32| {
        at != hex
            && world
                .movement_blocker_in(&occupancy, at, world.player)
                .is_none()
    };
    (0..3).find_map(|direction| {
        let (from, to) = (step(hex, direction), step(hex, direction + 3));
        (free(from) && free(to)).then_some([from, to])
    })
}

/// The living critters a bomb at `hex` may reach or just miss.
fn bystanders<A: WorldAssets>(host: &WorldHost<A>, hex: i32, elevation: u8) -> Vec<Bystander> {
    let world = &host.world;
    world
        .iter()
        .filter(|(reference, object)| {
            is_critter(object) && object.raw.elevation == elevation && !world.is_dead(*reference)
        })
        .filter_map(|(critter, object)| {
            let distance = HexGrid::angular_distance(object.raw.hex?, hex).ok()?;
            (distance <= WATCH_RADIUS).then(|| Bystander {
                critter,
                id: object.raw.id,
                hp: world.hit_points(critter).unwrap_or(0),
                distance,
            })
        })
        .collect()
}

/// How many `damage_p_proc` runs of `script` the trace holds.
fn damage_notices<A: WorldAssets>(host: &WorldHost<A>, script: Option<u32>) -> usize {
    let Some(script) = script else {
        return 0;
    };
    host.world
        .trace
        .iter()
        .filter(|entry| {
            matches!(entry, WorldTrace::Script { id, event, .. }
                if *id == script && *event == ScriptEvent::Damage)
        })
        .count()
}

/// An elevation's floor and roof art. Hidden roofs follow the player, so
/// they are not part of it.
fn tile_art<A: WorldAssets>(host: &WorldHost<A>, elevation: u8) -> Vec<(u16, u16)> {
    host.world.map.tiles[usize::from(elevation)]
        .iter()
        .flatten()
        .map(|tile| (tile.floor(), tile.roof()))
        .collect()
}

/// The record of a bomb about to be armed at `hex`.
fn light_fuse<A: WorldAssets>(
    host: &WorldHost<A>,
    item: ObjectRef,
    bomb: &Bomb,
    hex: i32,
    elevation: u8,
) -> Fuse {
    let world = &host.world;
    let neighbors = world
        .iter()
        .filter(|(reference, object)| {
            *reference != item
                && object.owner.is_none()
                && object.raw.elevation == elevation
                && matches!(object.category(), Category::Item | Category::Scenery)
                && (is_door(object) || world.script_for(*reference).is_some())
        })
        .filter_map(|(reference, object)| {
            let at = object.raw.hex?;
            let distance = HexGrid::angular_distance(at, hex).ok()?;
            let script = world.script_for(reference);
            (distance <= WATCH_RADIUS).then(|| Neighbor {
                object: reference,
                id: object.raw.id,
                label: label_of(host, reference),
                hex: at,
                distance,
                before: Frozen::of(object),
                script,
                procedure: script
                    .is_some_and(|script| host.has_procedure(script, ScriptEvent::Damage)),
                notices: damage_notices(host, script),
                probe: door_probe(host, at, elevation),
            })
        })
        .collect();
    Fuse {
        item,
        id: bomb.item,
        pid: world.get(item).map_or(0, |object| object.raw.pid),
        label: bomb.label.clone(),
        hex,
        elevation,
        deadline: world.clock.ticks + 10 * FUSE_SECONDS as u64 + FUSE_SLACK_TICKS,
        tiles: tile_art(host, elevation),
        neighbors,
        bystanders: bystanders(host, hex, elevation),
        dropped: false,
        victims: None,
        burned: 0,
    }
}

/// A rule's `Invalid` answer, given before anything changed, is a refusal.
fn ruled(result: Result<(), WorldError>) -> Result<BotOutcome, WorldError> {
    match result {
        Ok(()) => Ok(BotOutcome::Done),
        Err(WorldError::Invalid(reason)) => Ok(BotOutcome::Refused(reason)),
        Err(error) => Err(error),
    }
}

fn refused(reason: impl Into<String>) -> Result<BotOutcome, WorldError> {
    Ok(BotOutcome::Refused(reason.into()))
}

impl<A: WorldAssets> WorldHost<A> {
    /// Carry out `action` as far as the world goes. Screen actions come back
    /// [`BotOutcome::NeedsShell`]. A refused action changes nothing; `Err`
    /// is a failure of the world, not of the bot's request. A use the rules
    /// allow but which then fails comes back `Done`, as the interface's does:
    /// its AP are spent.
    pub fn execute_bot(&mut self, action: &BotAction) -> Result<BotOutcome, WorldError> {
        if matches!(
            action,
            BotAction::TravelTo { .. }
                | BotAction::EnterHere
                | BotAction::TownEntrance { .. }
                | BotAction::CloseScreen
                | BotAction::DismissDialog
        ) {
            return Ok(BotOutcome::NeedsShell);
        }
        if let BotAction::Dialogue { choice } = action {
            return self.bot_dialogue(*choice);
        }
        if self.world.dialogue.open {
            return refused("a dialogue is open");
        }
        if self.world.is_dead(self.world.player) {
            return refused("the player is dead");
        }
        // SPEC-014 §3.27: the combat lock discards every click.
        if clicks::input_locked(&self.click_state()) {
            return refused("combat has the input locked");
        }
        match *action {
            BotAction::Walk { hex, run } => self.bot_walk(hex, run),
            BotAction::Attack {
                target,
                hit_mode,
                location,
            } => self.bot_attack(target, hit_mode, location),
            BotAction::Reload { weapon } => self.bot_reload(weapon),
            BotAction::UseItem { item } => self.bot_use_item(item),
            BotAction::UseItemOn { item, target } => self.bot_use_item_on(item, target),
            BotAction::UseSkill { skill, target } => self.bot_use_skill(skill, target),
            BotAction::Interact { target, verb } => self.bot_interact(target, verb),
            BotAction::ArmExplosive { item, seconds } => self.bot_arm(item, seconds),
            BotAction::Equip { item } => self.bot_equip(item),
            BotAction::EndTurn => {
                if !self.world.combat || !self.world.combat_state.player_turn {
                    return refused("it is not the player's turn");
                }
                self.end_player_turn();
                Ok(BotOutcome::Done)
            }
            BotAction::EndCombat => self.bot_end_combat(),
            _ => refused("no rule carries this out"),
        }
    }

    /// The object `raw.id` names.
    fn bot_object(&self, id: u32) -> Option<ObjectRef> {
        self.world.reference_for_id(id)
    }

    /// The player's own carried item `raw.id` names.
    fn bot_carried(&self, id: u32) -> Option<ObjectRef> {
        self.bot_object(id).filter(|item| {
            self.world
                .get(*item)
                .is_some_and(|object| object.owner == Some(self.world.player))
        })
    }

    /// SPEC-014 §3.27: a click the busy player's policy ignores.
    fn bot_ignored(&self, click: Click) -> bool {
        clicks::click_policy(click, &self.click_state(), true) == Policy::Ignore
    }

    fn bot_walk(&mut self, hex: i32, run: bool) -> Result<BotOutcome, WorldError> {
        if !(0..HEXES).contains(&hex) {
            return refused(format!("hex {hex} is off the map"));
        }
        if self.bot_ignored(Click::Movement) {
            return refused("the player is busy");
        }
        let player = self.world.player;
        if self.walk_toward_with(player, hex, run, 0, true)? {
            Ok(BotOutcome::Done)
        } else {
            refused("the walk did not register")
        }
    }

    fn bot_attack(
        &mut self,
        id: u32,
        mode: u8,
        place: Option<u8>,
    ) -> Result<BotOutcome, WorldError> {
        let player = self.world.player;
        let Some(target) = self.bot_object(id).filter(|target| *target != player) else {
            return refused(format!("object {id} is gone"));
        };
        if self
            .world
            .get(target)
            .is_none_or(|object| !is_critter(object))
        {
            return refused(format!("object {id} is no critter"));
        }
        if mode > hit_mode::KICK || place.is_some_and(|place| place > location::UNCALLED) {
            return refused(format!("hit mode {mode} at {place:?} is no attack"));
        }
        // SPEC-007 §3.10: the player acts again once every attack has played.
        if self.world.combat && self.presenting() {
            return refused("an attack is still playing");
        }
        let aimed = place.is_some();
        if aimed {
            let attack =
                self.world
                    .attack_profile(player, target, mode, location::UNCALLED, true)?;
            if !attack.aimed_shot_available() {
                return refused("this mode takes no called shot");
            }
        }
        match self.player_attack(target, mode, aimed)? {
            PlayerAttack::Gated => refused("the input gate is shut"),
            PlayerAttack::Refused { refusal, cost } => {
                refused(format!("{refusal:?}, at {cost} AP"))
            }
            PlayerAttack::CombatRequested | PlayerAttack::Attacked => Ok(BotOutcome::Done),
            PlayerAttack::Aimed => {
                let place = place.unwrap_or(location::UNCALLED);
                match self.perform_attack(player, target, mode, place, true)? {
                    Ok(_) => Ok(BotOutcome::Done),
                    Err(refusal) => refused(format!("{refusal:?}")),
                }
            }
        }
    }

    fn bot_reload(&mut self, id: u32) -> Result<BotOutcome, WorldError> {
        let Some(weapon) = self.bot_carried(id) else {
            return refused(format!("the player carries no object {id}"));
        };
        let player = self.world.player;
        if self.world.first_compatible_ammo(player, weapon).is_none() {
            return refused("no ammunition for it");
        }
        match self.player_reload(weapon) {
            PlayerReload::ShortOfAp => refused("short of AP"),
            PlayerReload::Ran { .. } => Ok(BotOutcome::Done),
        }
    }

    /// The item slot's use (SPEC-014 §3.23). An explosive takes
    /// [`BotAction::ArmExplosive`], and a Motion Sensor has only its screen.
    fn bot_use_item(&mut self, id: u32) -> Result<BotOutcome, WorldError> {
        let Some(item) = self.bot_carried(id) else {
            return refused(format!("the player carries no object {id}"));
        };
        let pid = self.world.get(item).map_or(0, |object| object.raw.pid);
        if skills::is_explosive(pid) || pid == MOTION_SENSOR {
            return refused("this item's use is a window");
        }
        if !self.player_spend_use_ap(item) {
            return refused("short of AP");
        }
        match self.player_use_item(item, BookRoute::ItemSlot, false)? {
            ItemUse::OpenContainer | ItemUse::OpenTimer | ItemUse::MotionSensor => {
                refused("this item's use is a window")
            }
            ItemUse::Done | ItemUse::Failed(_) => Ok(BotOutcome::Done),
        }
    }

    /// The use crosshair (SPEC-014 §3.27): in combat 2 AP, spent once the
    /// click is allowed.
    fn bot_use_item_on(&mut self, id: u32, target: u32) -> Result<BotOutcome, WorldError> {
        let Some(item) = self.bot_carried(id) else {
            return refused(format!("the player carries no object {id}"));
        };
        let Some(target) = self.bot_object(target) else {
            return refused(format!("object {target} is gone"));
        };
        let usable = self
            .world
            .get(item)
            .and_then(|object| object.prototype.as_ref())
            .is_none_or(|prototype| prototype.permits_use_on());
        if !usable {
            return refused("the item permits no use on an object");
        }
        if self.bot_ignored(Click::Action) {
            return refused("the player is busy");
        }
        if !self.player_spend_use_ap(item) {
            return refused("short of AP");
        }
        let player = self.world.player;
        ruled(self.action(player, target, ObjectAction::UseObject(item)))
    }

    fn bot_use_skill(&mut self, index: i32, target: u32) -> Result<BotOutcome, WorldError> {
        let Some(target) = self.bot_object(target) else {
            return refused(format!("object {target} is gone"));
        };
        if !(0..18).contains(&index) {
            return refused(format!("skill {index} does not exist"));
        }
        if self.bot_ignored(Click::Action) {
            return refused("the player is busy");
        }
        let player = self.world.player;
        if self.use_skill_on(player, target, index)? {
            Ok(BotOutcome::Done)
        } else {
            refused("the skill request was rejected")
        }
    }

    fn bot_interact(&mut self, id: u32, verb: BotVerb) -> Result<BotOutcome, WorldError> {
        let Some(target) = self.bot_object(id) else {
            return refused(format!("object {id} is gone"));
        };
        let action = match verb {
            BotVerb::Use => ObjectAction::Use,
            BotVerb::Talk => ObjectAction::Talk,
            BotVerb::Pickup => ObjectAction::Pickup,
            BotVerb::Look => ObjectAction::LookAt,
        };
        let click = if verb == BotVerb::Talk && self.near_talk(target) {
            Click::NearTalk
        } else {
            Click::Action
        };
        if verb != BotVerb::Look && self.bot_ignored(click) {
            return refused("the player is busy");
        }
        let player = self.world.player;
        ruled(self.action(player, target, action))
    }

    /// The item slot's use of an explosive and its Set Timer window
    /// (SPEC-013 §3.10): the use's AP in combat, Traps for the fuse, and the
    /// item leaves the pack where the player stands.
    fn bot_arm(&mut self, id: u32, seconds: i32) -> Result<BotOutcome, WorldError> {
        let Some(item) = self.bot_carried(id) else {
            return refused(format!("the player carries no object {id}"));
        };
        let Some(object) = self.world.get(item) else {
            return refused(format!("the player carries no object {id}"));
        };
        if !skills::is_explosive(object.raw.pid) {
            return refused("the item is no explosive");
        }
        if object.raw.flags.contains(BasicFlags::USED) {
            return refused("the explosive is already armed");
        }
        if !(SetTimer { seconds }).accepts() {
            return refused(format!("the timer takes no {seconds} seconds"));
        }
        if !self.player_spend_use_ap(item) {
            return refused("short of AP");
        }
        if self.player_use_item(item, BookRoute::ItemSlot, false)? != ItemUse::OpenTimer {
            // A use hook took it.
            return Ok(BotOutcome::Done);
        }
        self.arm_explosive(item, seconds)?;
        let player = self.world.player;
        self.world.drop_item(player, item, 1)?;
        Ok(BotOutcome::Done)
    }

    /// The inventory window's hand slot (SPEC-014 §3.34), out of combat.
    fn bot_equip(&mut self, id: u32) -> Result<BotOutcome, WorldError> {
        let Some(item) = self.bot_carried(id) else {
            return refused(format!("the player carries no object {id}"));
        };
        if self.world.combat {
            return refused("the bot keeps out of its pack in combat");
        }
        let flag = if self.world.active_hand != 0 {
            BasicFlags::RIGHT_HAND
        } else {
            BasicFlags::LEFT_HAND
        };
        let player = self.world.player;
        self.world.equip_slot(player, item, flag.0)?;
        // The window's close gives the player the weapon's art.
        self.apply_figure_art(Some(item));
        Ok(BotOutcome::Done)
    }

    fn bot_dialogue(&mut self, choice: Option<usize>) -> Result<BotOutcome, WorldError> {
        let dialogue = &self.world.dialogue;
        if !dialogue.open {
            return refused("no dialogue is open");
        }
        if !dialogue.waiting {
            if choice.is_some() {
                return refused("the dialogue is not waiting for a choice");
            }
            // Esc, which ends a session that is not waiting.
            self.close_dialogue()?;
            return Ok(BotOutcome::Done);
        }
        if choice.is_some_and(|choice| choice >= dialogue.options.len()) {
            return refused(format!("no option {choice:?}"));
        }
        self.choose_dialogue(choice)?;
        Ok(BotOutcome::Done)
    }

    fn bot_end_combat(&mut self) -> Result<BotOutcome, WorldError> {
        if !self.world.combat {
            return refused("no combat to end");
        }
        let mut ai = std::mem::replace(&mut self.combat_ai, Box::new(crate::combat_state::NoAi));
        let accepted = self.request_end_combat(&mut *ai);
        self.combat_ai = ai;
        if accepted {
            Ok(BotOutcome::Done)
        } else {
            refused("an opponent fights on")
        }
    }
}
