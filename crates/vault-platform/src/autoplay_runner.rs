//! The headless autoplay harness: `AutoplayAgent` driving a session with no
//! window, no audio and no sleeping, with parity audits and failure bundles.
//!
//! [`autoplay_step`] is the one step the bot takes: decide, log, apply, log the
//! tick, run the tick. Watch Mode calls it too, so a headless run and a watched
//! run are the same game. Time is the count of ticks, each [`FRAME_MS`] long.
//! The wall clock is read for one line of `report.txt` and nothing else.
//!
//! The log a run writes is an ordinary timed input log, and its checkpoints are
//! the SPEC-021 stream a [`Replay`] of that log produces, row for row. A replay
//! runs no agent and no audit, so an audit must leave the live world as it
//! found it: [`LiveState`] puts back what `build_save` touches.
//!
//! A run stops at its first failure and writes the folder [`replay_bundle`]
//! reads. A clean run writes the same four files. A bundle replays through
//! [`start_world`], which starts the world as the run did: attacks play at once
//! and a Fallout 2 map is a new game. `--replay-inputs` does neither by itself.
use crate::{
    AppResult,
    input::{self, InputCommand},
    map_preview::MapPreview,
    preview::{AssetPreview, PreviewArgs},
    replay::{CheckpointOptions, FRAME_MS, INITIAL_SURFACE, Replay, interactive_world},
    session::{SessionConfig, SessionRuntime},
};
use serde::{Deserialize, Serialize};
use std::{
    any::Any,
    collections::{BTreeMap, BTreeSet},
    panic::{AssertUnwindSafe, catch_unwind},
    path::{Path, PathBuf},
};
use vault_core::{
    CharacterSheet, GameProfile, ObjectRef, ScriptEvent, SequenceIds, WorldHost, WorldState,
    assets::{VfsWorldAssets, WorldAssets},
    autoplay::{AutoplayAgent, BotAction, BotOutcome, BotStatus, BotView},
    checkpoints::{CheckpointStream, ExpectedStream},
    host_choreography::Playback,
    host_save::{SaveInfo, SlotImage},
    movie::MoviePolicy,
    queue::EventKind,
    skills::{EXPLOSION_EVENT, PREMATURE_EVENT},
    state_hash::{Participant, ParticipantState, StateSnapshot},
    world::WorldTrace,
};
use vault_formats::{
    AutomapDb, Fid,
    map::{MapObject, ObjectDetail, ObjectState},
    pro::BasicFlags,
};

/// The bundle's four files.
const INPUTS_FILE: &str = "inputs.jsonl";
const CHECKPOINTS_FILE: &str = "checkpoints.tsv";
const MANIFEST_FILE: &str = "manifest.json";
const REPORT_FILE: &str = "report.txt";
/// The manifest's format, and its version.
const BUNDLE_FORMAT: &str = "vault-autoplay-bundle";
const BUNDLE_VERSION: u32 = 1;
/// [`AutoplayOptions::stuck_limit`] unless a caller says otherwise: about half
/// a minute of frames. The agent gives a goal up long before this, and its
/// waits for a fuse do not count.
pub const STUCK_LIMIT: u32 = 2000;
/// How many lines of an audit's findings a report keeps.
const MAX_FINDINGS: usize = 32;

/// What decides for the bot: [`AutoplayAgent`], or a test's stand-in.
pub trait Decider {
    /// The next action, or `None` to let the step pass.
    fn decide(&mut self, host: &WorldHost<VfsWorldAssets>, view: &BotView) -> Option<BotAction>;
    /// What became of the last action.
    fn note_outcome(&mut self, _outcome: &BotOutcome) {}
    fn status(&self) -> &BotStatus;
}

impl Decider for AutoplayAgent {
    fn decide(&mut self, host: &WorldHost<VfsWorldAssets>, view: &BotView) -> Option<BotAction> {
        AutoplayAgent::decide(self, host, view)
    }
    fn note_outcome(&mut self, outcome: &BotOutcome) {
        AutoplayAgent::note_outcome(self, outcome);
    }
    fn status(&self) -> &BotStatus {
        AutoplayAgent::status(self)
    }
}

/// What one [`autoplay_step`] did.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct AutoplayStep {
    /// What the bot took, if anything.
    pub action: Option<BotAction>,
    /// What became of it. A world error is a refusal, with its text.
    pub outcome: Option<BotOutcome>,
    /// The frame changed: Watch Mode redraws.
    pub changed: bool,
    /// The session is over. A replay writes no row for such a tick.
    pub finished: bool,
}

/// One step of the bot, in the shell's order for `Debug` records: `decide`, log
/// the action, apply it, log the [`InputCommand::Tick`], run the tick at `now`.
/// `log` writes the record. A session showing no world only ticks.
///
/// Headless runs and Watch Mode both call this. `now` is the session's clock:
/// a headless run counts it, and Watch Mode passes one virtual 16 ms frame per
/// call. Nothing here reads the wall clock.
pub fn autoplay_step(
    preview: &mut AssetPreview,
    agent: &mut impl Decider,
    mut log: impl FnMut(&InputCommand) -> AppResult<()>,
    now: u64,
) -> AppResult<AutoplayStep> {
    let mut step = AutoplayStep::default();
    if let Some(world) = interactive_world(preview) {
        let view = world.bot_view();
        if let Some(action) = agent.decide(&world.host, &view) {
            log(&InputCommand::Bot {
                action: action.clone(),
            })?;
            // A refusal is part of the run: the world stands as it did.
            let outcome = world
                .apply_bot(&action)
                .unwrap_or_else(|error| BotOutcome::Refused(error.to_string()));
            agent.note_outcome(&outcome);
            step.outcome = Some(outcome);
            step.action = Some(action);
        }
    }
    log(&InputCommand::Tick { now_ms: now })?;
    step.changed = preview.tick(now)?;
    step.finished = preview.finished();
    Ok(step)
}

/// What [`run_headless_autoplay`] does.
#[derive(Debug, Clone)]
pub struct AutoplayOptions {
    /// Ticks to run, at most (`--ticks`).
    pub ticks: u64,
    /// The world's and the agent's seed (`--seed`). -1 asks for an ambient
    /// one, and the manifest records the seed it got.
    pub seed: i32,
    /// The chaos audit (`--chaos-doors-c4`).
    pub chaos: bool,
    /// The folder the run's files go in, made if missing.
    pub output: PathBuf,
    /// A stuck loop is this many decisions in a row that change nothing.
    pub stuck_limit: u32,
    /// The session: the corpus (`corpus_dir`, or `master_dat` and
    /// `critter_dat`), the edition (`game_profile`), the start map
    /// (`preview_map`), and `preset`, `sfall`, `mods` and `player_gcd`.
    pub session: PreviewArgs,
}

impl AutoplayOptions {
    /// A run over the loose `corpus`, from `map`.
    #[cfg(test)]
    pub fn new(
        corpus: impl Into<PathBuf>,
        edition: GameProfile,
        map: impl Into<String>,
        output: impl Into<PathBuf>,
    ) -> Self {
        Self::from_args(
            PreviewArgs {
                corpus_dir: Some(corpus.into()),
                game_profile: edition,
                preview_map: Some(map.into()),
                ..Default::default()
            },
            output,
        )
    }
    /// A run of the session the command line's flags describe.
    pub fn from_args(session: PreviewArgs, output: impl Into<PathBuf>) -> Self {
        Self {
            ticks: 50_000,
            seed: session.seed.unwrap_or(1),
            chaos: false,
            output: output.into(),
            stuck_limit: STUCK_LIMIT,
            session,
        }
    }
}

/// The kinds of failure a run stops at.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FailureKind {
    Panic,
    AuditMismatch,
    StaleIdIndex,
    BotViolation,
    StuckLoop,
    /// The session returned an error it could not go on from.
    Error,
}

/// Where in a tick a failure came.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Phase {
    #[default]
    Decide,
    Apply,
    Tick,
    /// After the tick: the row, the checks and the audit.
    Checks,
}

/// Why a run stopped short.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RunFailure {
    pub kind: FailureKind,
    pub tick: u64,
    pub phase: Phase,
    /// The panic's message, the audit's findings or the violation, a line
    /// each.
    pub detail: Vec<String>,
}

/// How a run came to its end.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RunEnd {
    /// It played every tick it was given.
    #[default]
    Budget,
    /// The session finished, as a confirmed quit does.
    SessionOver,
    /// The player died. Nothing is left to play, and that is no failure.
    PlayerDead,
    /// The bot ran out of things to do.
    Finished,
    /// [`AutoplayReport::failure`] says what.
    Failed,
}

/// What a run did. A run that found a failure still returns `Ok`: this report
/// names it, and the caller chooses the exit status.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct AutoplayReport {
    /// Ticks run, the failing one included.
    pub ticks: u64,
    /// Records in `inputs.jsonl`.
    pub records: u64,
    pub actions: u64,
    pub refusals: u64,
    pub tally: AuditTally,
    /// The agent as it ended: its goal, and the chaos checks it counted.
    pub bot: BotStatus,
    /// The world's `state_hash` at the last row, in the row's own digits.
    pub state_hash: String,
    /// The folder holding `inputs.jsonl` and the rest.
    pub output: PathBuf,
    /// Ticks a wall-clock second, for the report. Never read by the run.
    pub ticks_per_second: f64,
    pub ended: RunEnd,
    pub failure: Option<RunFailure>,
}

/// Why an audit ran.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum AuditReason {
    MapTransition,
    Explosion,
    CombatEnd,
}

impl AuditReason {
    pub fn name(self) -> &'static str {
        match self {
            Self::MapTransition => "map transition",
            Self::Explosion => "explosion",
            Self::CombatEnd => "combat end",
        }
    }
}

/// The count of what the auditor did.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct AuditTally {
    /// Audits that ran to a verdict, by why they ran.
    pub audits: BTreeMap<&'static str, u64>,
    /// Moments saving is refused at, by why.
    pub skipped: BTreeMap<&'static str, u64>,
    /// Differences an allowlist entry took, by entry.
    pub allowlisted: BTreeMap<&'static str, u64>,
}

impl AuditTally {
    pub fn total(&self) -> u64 {
        self.audits.values().sum()
    }
}

/// What an audit found.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AuditVerdict {
    Clean,
    /// Saving is refused now, for this reason.
    Skipped(&'static str),
    Mismatch(Vec<String>),
    StaleIds(Vec<String>),
}

/// Points where a test puts a fault in. A run has none.
pub trait Faults {
    /// A tick ended and its row was written.
    fn after_tick(&mut self, _world: &mut MapPreview, _tick: u64) {}
    /// A load of the audit ended in the second host: 0 for A, 1 for B.
    fn after_load(&mut self, _host: &mut WorldHost<VfsWorldAssets>, _load: u8) {}
}

/// No faults.
pub struct NoFaults;
impl Faults for NoFaults {}

/// Start a world for a run, its audit or a bundle's replay, all the same way:
/// no window, no storage of the user's, no audio, movies completed and attacks
/// played at once. A map under `fallout2` starts as a new game there, since a
/// plain map preview has no Fallout 2 state to save or travel with.
pub fn start_world(args: &PreviewArgs) -> AppResult<SessionRuntime> {
    let mut session = SessionRuntime::start(SessionConfig::replay(args.clone()))?
        .ok_or("autoplay needs a start map")?;
    if session.edition() == GameProfile::Fallout2 {
        let world = fallout2_game(&mut session, args)?;
        *session = AssetPreview::Map(Box::new(world));
    }
    let world = interactive_world(&mut session).ok_or("autoplay needs a map session")?;
    world.host.movie_policy = MoviePolicy::CompleteImmediately;
    world.host.playback = Playback::Instant;
    Ok(session)
}

/// SPEC-018 §6.6 step 3's hand-off to the session's start map.
fn fallout2_game(session: &mut SessionRuntime, args: &PreviewArgs) -> AppResult<MapPreview> {
    let mods = session.mod_session();
    let world = interactive_world(session).ok_or("autoplay needs a map session")?;
    let tables = world
        .host
        .load_fallout2_tables()
        .map_err(|error| error.to_string())?;
    let assets = VfsWorldAssets::new(world.host.assets.vfs.clone(), &world.host.assets.language)?;
    assets.set_game_profile(GameProfile::Fallout2);
    let sheet = match &args.player_gcd {
        Some(path) => CharacterSheet::from_gcd(&assets.gcd(path)?, Default::default()),
        None => CharacterSheet::default(),
    };
    let map = args.preview_map.as_deref().unwrap_or_default();
    let mut game = MapPreview::with_setup(
        assets,
        map,
        sheet,
        args.resolved_seed(),
        args.preset,
        |host| {
            crate::session::start_sfall(host, args.sfall);
            crate::session::start_mods(host, mods);
            host.begin_fallout2_game_on(tables, map);
        },
    )?;
    game.save_root = world.save_root.clone();
    game.storage = world.storage.clone();
    Ok(game)
}

/// One object as a slot holds it, keyed by `raw.id` in a [`Ledger`].
#[derive(Debug, Clone, PartialEq, Eq)]
struct ObjectRow {
    pid: u32,
    hex: Option<i32>,
    elevation: u8,
    orientation: u8,
    art_id: u32,
    flags: u32,
    state: ObjectState,
    /// The holder's `raw.id`.
    holder: Option<u32>,
    /// Held objects by `raw.id`, with their quantities.
    contents: BTreeMap<u32, u32>,
}

impl ObjectRow {
    fn of(raw: &MapObject, holder: Option<u32>) -> Self {
        Self {
            pid: raw.pid,
            hex: raw.hex,
            elevation: raw.elevation,
            orientation: raw.orientation,
            art_id: raw.art_id,
            flags: raw.flags.0,
            state: raw.state.clone(),
            holder,
            contents: raw
                .inventory
                .iter()
                .map(|entry| (entry.object.id, entry.quantity))
                .collect(),
        }
    }
    /// The fields a scripted object's own Start or MapEnter may change.
    fn scripted_fields(&self) -> impl PartialEq + use<> {
        (
            self.hex,
            self.elevation,
            self.orientation,
            self.art_id,
            self.flags,
            self.state.clone(),
        )
    }
    /// What no script's placement touches: a critter's hit points, poison and
    /// radiation.
    fn vitals(&self) -> Option<[i32; 3]> {
        match &self.state {
            ObjectState::Critter(state) => Some([state.hp, state.poison, state.radiation]),
            ObjectState::Other { .. } => None,
        }
    }
}

/// One timed event, with handles gone: its owner by `raw.id`, -2 for none.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
struct QueueRow {
    due: u64,
    kind: i32,
    owner: i64,
    payload: String,
}

/// The state an audit compares, with every handle replaced by `raw.id`.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
struct Ledger {
    objects: BTreeMap<u32, ObjectRow>,
    gvars: Vec<i32>,
    mvars: Vec<i32>,
    /// By script record, without the records a load makes anew.
    lvars: BTreeMap<u32, Vec<i32>>,
    /// Script records of temporary objects, which a load cannot bind.
    dropped_scripts: BTreeSet<u32>,
    clock: u64,
    queue: Vec<QueueRow>,
    /// Identifiers from here on were made after the save.
    next_id: u32,
    /// The player's `raw.id`.
    player: u32,
}

impl Ledger {
    fn of(world: &WorldState) -> Self {
        let mut objects = BTreeMap::new();
        for (reference, object) in world.iter() {
            if object.owner.is_none()
                && kept_by_a_slot(world, reference)
                && let Some(tree) = world.object_tree(reference)
            {
                flatten(&tree, None, &mut objects);
            }
        }
        let temporary = |object: ObjectRef| {
            world
                .get(object)
                .is_some_and(|found| found.raw.flags.contains(BasicFlags::TEMPORARY))
        };
        let mut lvars = BTreeMap::new();
        let mut dropped_scripts = BTreeSet::new();
        for script in world.scripts.values() {
            if Some(script.id) == world.map_script || script.owner == world.player {
                continue;
            }
            if temporary(script.owner) {
                dropped_scripts.insert(script.id);
                continue;
            }
            let values = usize::try_from(script.local_base)
                .ok()
                .and_then(|base| {
                    let end = base.saturating_add(script.local_count);
                    world.lvars.get(base..end.min(world.lvars.len()))
                })
                .unwrap_or_default();
            lvars.insert(script.id, values.to_vec());
        }
        let owner = |object: ObjectRef| -> i64 {
            match world.get(object) {
                Some(found) if !temporary(object) => i64::from(found.raw.id),
                _ => -2,
            }
        };
        // A load gives the player a new script record and drops the old
        // one's events (SPEC-006 §3.5).
        let players = |event: &&vault_core::queue::TimedEvent| match event.kind {
            EventKind::Script { script, .. } => world
                .scripts
                .get(&script)
                .is_some_and(|script| script.owner == world.player),
            _ => false,
        };
        let mut queue: Vec<QueueRow> = world
            .queue
            .events()
            .filter(|event| !players(event))
            .map(|event| QueueRow {
                due: event.due,
                kind: event.kind.type_number(),
                owner: owner(event.object),
                payload: format!("{:?}", event.kind),
            })
            .collect();
        queue.sort();
        Self {
            objects,
            gvars: world.gvars.clone().unwrap_or_default(),
            mvars: world.mvars.clone(),
            lvars,
            dropped_scripts,
            clock: world.clock.ticks,
            queue,
            next_id: world.next_object_id(),
            player: world.get(world.player).map_or(0, |player| player.raw.id),
        }
    }
}

/// Whether a save keeps this top-level object: the player, centrally, and what
/// `export_map` writes, which leaves out temporary objects, objects with no
/// hex and objects past elevation 2.
fn kept_by_a_slot(world: &WorldState, reference: ObjectRef) -> bool {
    reference == world.player
        || world.get(reference).is_some_and(|object| {
            object.raw.hex.is_some()
                && object.raw.elevation <= 2
                && !object.raw.flags.contains(BasicFlags::TEMPORARY)
        })
}

fn flatten(raw: &MapObject, holder: Option<u32>, out: &mut BTreeMap<u32, ObjectRow>) {
    out.insert(raw.id, ObjectRow::of(raw, holder));
    for entry in &raw.inventory {
        flatten(&entry.object, Some(raw.id), out);
    }
}

/// What ran during a load: Start, MapEnter and the load's own map update.
#[derive(Debug, Default)]
struct LoadScripts {
    records: BTreeSet<u32>,
    /// The owners of those records, by `raw.id`.
    owners: BTreeSet<u32>,
}

impl LoadScripts {
    fn of(world: &WorldState) -> Self {
        let mut ran = Self::default();
        for entry in &world.trace {
            if let WorldTrace::Script { id, event, .. } = entry
                && matches!(
                    event,
                    ScriptEvent::Start | ScriptEvent::MapEnter | ScriptEvent::MapUpdate
                )
            {
                ran.records.insert(*id);
                if let Some(owner) = world
                    .scripts
                    .get(id)
                    .and_then(|script| world.get(script.owner))
                {
                    ran.owners.insert(owner.raw.id);
                }
            }
        }
        ran
    }
}

/// The audit's allowlist. Each entry is a difference a load is entitled to
/// make, and each one is counted in the report. Everything else that differs
/// is a mismatch.
mod allow {
    /// A global changed while a Start, MapEnter or map update ran in the load.
    pub const GLOBAL: &str = "global written by a load's scripts";
    /// A map variable, likewise.
    pub const MAP_VARIABLE: &str = "map variable written by a load's scripts";
    /// A script's own locals, when that script ran.
    pub const LOCAL: &str = "local written by its own script in a load";
    /// A scripted object's place, look or lock, when its own script ran.
    pub const SCRIPTED_OBJECT: &str = "scripted object moved by its own script in a load";
    /// An object no save held: its identifier is past the saved allocator, so
    /// a script made it during the load.
    pub const NEW_OBJECT: &str = "object a load's scripts made";
    /// A script record no save held, made while the load ran.
    pub const NEW_SCRIPT: &str = "script record a load's scripts made";
    /// SPEC-003 §3.3 `[Must-match]`: reading an exit grid that leads to the
    /// town or world map moves its low art index up by 16, on every read,
    /// saved maps too. Shipped maps never qualify; the fixture's does.
    pub const EXIT_ART: &str = "exit-grid art a load moved";
    /// SPEC-005 §3.8: a load ends in the gender update, which rebuilds the
    /// player's art without its pose, unless armor is worn.
    pub const PLAYER_ART: &str = "player art rebuilt at the end of a load";
    /// `state_hash` names entities by slot, and a load fills the slots in hex
    /// order. A script that moves an object during load A changes the order
    /// load B reads, so the same state lands in other slots. It is taken only
    /// when a load did move something and nothing but the handles differs.
    pub const RENUMBERED: &str = "same state under other handles after a load's scripts moved it";
    /// Load B differs from load A, by the hash, in nothing but what the
    /// entries above allow a load's scripts to change: a critter a script
    /// places afresh on every load, say. Saving does not settle those.
    pub const UNSETTLED: &str = "load B differs from load A only by its scripts";
}

/// Differences between a world and what a load of it gave, as findings.
fn diff_ledgers(
    live: &Ledger,
    loaded: &Ledger,
    ran: &LoadScripts,
    allowed: &mut BTreeMap<&'static str, u64>,
) -> Vec<String> {
    let mut findings = Vec::new();
    let mut allow = |entry: &'static str| *allowed.entry(entry).or_default() += 1;
    let scripts_ran = !ran.records.is_empty();
    for (id, row) in &live.objects {
        let Some(other) = loaded.objects.get(id) else {
            findings.push(format!("object {id} ({:#010x}) is gone", row.pid));
            continue;
        };
        if row == other {
            continue;
        }
        let moved = if shifted_exit_art(row, other) {
            Some(allow::EXIT_ART)
        } else if *id == live.player && rebuilt_player_art(row, other) {
            Some(allow::PLAYER_ART)
        } else {
            None
        };
        if let Some(entry) = moved {
            allow(entry);
        }
        let other = &ObjectRow {
            art_id: moved.map_or(other.art_id, |_| row.art_id),
            ..other.clone()
        };
        if row == other {
            continue;
        }
        let same_contents = same_held(&row.contents, &other.contents, live.next_id);
        if row.pid != other.pid || row.holder != other.holder || !same_contents {
            findings.push(format!("object {id}: {row:?} became {other:?}"));
        } else if row.scripted_fields() == other.scripted_fields() {
            // Only made-in-the-load contents differ.
            allow(allow::NEW_OBJECT);
        } else if ran.owners.contains(id) && row.vitals() == other.vitals() {
            allow(allow::SCRIPTED_OBJECT);
        } else {
            findings.push(format!("object {id}: {row:?} became {other:?}"));
        }
    }
    for (id, row) in &loaded.objects {
        if !live.objects.contains_key(id) {
            if *id >= live.next_id {
                allow(allow::NEW_OBJECT);
            } else {
                findings.push(format!("object {id} ({:#010x}) appeared", row.pid));
            }
        }
    }
    let mut variables = |name: &str, entry, live: &[i32], loaded: &[i32]| {
        if live.len() != loaded.len() {
            findings.push(format!(
                "{name}: {} values became {}",
                live.len(),
                loaded.len()
            ));
            return;
        }
        for (index, (was, now)) in live.iter().zip(loaded).enumerate() {
            if was != now {
                if scripts_ran {
                    allow(entry);
                } else {
                    findings.push(format!("{name} {index}: {was} became {now}"));
                }
            }
        }
    };
    variables("global", allow::GLOBAL, &live.gvars, &loaded.gvars);
    variables(
        "map variable",
        allow::MAP_VARIABLE,
        &live.mvars,
        &loaded.mvars,
    );
    for (id, values) in &live.lvars {
        match loaded.lvars.get(id) {
            None => findings.push(format!("script {id:#x} is gone")),
            Some(other) if other == values => {}
            Some(_) if ran.records.contains(id) => allow(allow::LOCAL),
            Some(other) => findings.push(format!(
                "script {id:#x} locals: {values:?} became {other:?}"
            )),
        }
    }
    for id in loaded.lvars.keys() {
        if !live.lvars.contains_key(id) && !live.dropped_scripts.contains(id) {
            if ran.records.contains(id) {
                allow(allow::NEW_SCRIPT);
            } else {
                findings.push(format!("script {id:#x} appeared"));
            }
        }
    }
    if live.clock != loaded.clock {
        findings.push(format!("clock: {} became {}", live.clock, loaded.clock));
    }
    if live.queue != loaded.queue {
        findings.extend(queue_findings(&live.queue, &loaded.queue));
    }
    findings
}

/// The art change SPEC-003 makes when an exit grid to the town or world map is
/// read: the same art with its index 16 higher.
fn shifted_exit_art(live: &ObjectRow, loaded: &ObjectRow) -> bool {
    let town_or_world = matches!(
        live.state,
        ObjectState::Other { detail: ObjectDetail::ExitGrid { map, .. }, .. } if map <= 0
    );
    let (from, to) = (Fid(live.art_id), Fid(loaded.art_id));
    town_or_world
        && (0x0500_0010..=0x0500_0017).contains(&live.pid)
        && from.base() < 33
        && to == Fid::new(from.category(), from.base() + 16, from.animation(), 0, 0)
}

/// The art SPEC-005 §3.8 gives the player: the same critter art, with no pose
/// and no selector, keeping the weapon code only while something is held.
fn rebuilt_player_art(live: &ObjectRow, loaded: &ObjectRow) -> bool {
    let (from, to) = (Fid(live.art_id), Fid(loaded.art_id));
    to.category() == 1
        && to.base() == from.base()
        && to.animation() == 0
        && to.selector() == 0
        && (to.weapon() == from.weapon() || to.weapon() == 0)
}

/// Held objects match but for objects made after the save.
fn same_held(live: &BTreeMap<u32, u32>, loaded: &BTreeMap<u32, u32>, next_id: u32) -> bool {
    let kept = |held: &BTreeMap<u32, u32>| -> BTreeMap<u32, u32> {
        held.iter()
            .filter(|(id, _)| **id < next_id)
            .map(|(id, quantity)| (*id, *quantity))
            .collect()
    };
    kept(live) == kept(loaded)
}

/// The events only one side holds.
fn queue_findings(live: &[QueueRow], loaded: &[QueueRow]) -> Vec<String> {
    let mut left: Vec<&QueueRow> = loaded.iter().collect();
    let mut findings = Vec::new();
    for row in live {
        match left.iter().position(|other| *other == row) {
            Some(at) => {
                left.swap_remove(at);
            }
            None => findings.push(format!("queue event {row:?} is gone")),
        }
    }
    findings.extend(
        left.into_iter()
            .map(|row| format!("queue event {row:?} appeared")),
    );
    findings
}

/// `build_save` joins the current map to the working set: it marks the map
/// visited, works off the seen hexes and captures the automap. This puts all of
/// that back, so that an audited run and its unaudited replay stay one game.
struct LiveState {
    maps: BTreeMap<String, Vec<u8>>,
    automap: AutomapDb,
    map_flags: vault_formats::pro::MapFlags,
    last_visit: u32,
    visited: Vec<u8>,
    flags: Vec<(ObjectRef, BasicFlags)>,
}

impl LiveState {
    fn of(world: &WorldState) -> Self {
        Self {
            maps: world.persistence.maps.clone(),
            automap: world.persistence.automap.clone(),
            map_flags: world.map.header.flags,
            last_visit: world.map.header.last_visit,
            visited: world.visited.clone(),
            flags: world
                .iter()
                .map(|(reference, object)| (reference, object.raw.flags))
                .collect(),
        }
    }
    fn restore(self, world: &mut WorldState) {
        world.persistence.maps = self.maps;
        world.persistence.automap = self.automap;
        world.map.header.flags = self.map_flags;
        world.map.header.last_visit = self.last_visit;
        world.visited = self.visited;
        for (reference, flags) in self.flags {
            if let Some(object) = world.get_mut(reference) {
                object.raw.flags = flags;
            }
        }
    }
}

/// The parity auditor, with its second host.
pub struct ParityAuditor {
    session: SessionRuntime,
    tally: AuditTally,
}

impl ParityAuditor {
    /// A second host set up as `args` sets up the run's: the same corpus,
    /// preset, sfall mode, profile and layers.
    pub fn new(args: &PreviewArgs) -> AppResult<Self> {
        Ok(Self {
            session: start_world(args)?,
            tally: AuditTally::default(),
        })
    }
    #[cfg(test)]
    pub fn tally(&self) -> &AuditTally {
        &self.tally
    }

    /// World, to an image, to load A in the second host, to an image, to load
    /// B: `state_hash(A)` must equal `state_hash(B)`, and world must match A by
    /// `raw.id`, over objects, variables, clock and queue. The RNG is not
    /// saved, so it is copied in before and after each load: before, so that a
    /// script drawing in a load draws what the other load drew. A moment saving
    /// is refused at is skipped. The live world comes out unchanged.
    pub fn audit(
        &mut self,
        world: &mut MapPreview,
        why: &[AuditReason],
        faults: &mut impl Faults,
    ) -> AuditVerdict {
        if let Some(reason) = refused_moment(&world.host) {
            *self.tally.skipped.entry(reason).or_default() += 1;
            return AuditVerdict::Skipped(reason);
        }
        for reason in why {
            *self.tally.audits.entry(reason.name()).or_default() += 1;
        }
        let stale = stale_ids(&world.host.world);
        if !stale.is_empty() {
            return AuditVerdict::StaleIds(stale);
        }
        let live = &mut world.host;
        let before = live.world.state_hash();
        let state = LiveState::of(&live.world);
        let image = live.slot_image(&SaveInfo::default());
        let ledger = Ledger::of(&live.world);
        state.restore(&mut live.world);
        let mut findings = Vec::new();
        if live.world.state_hash() != before {
            findings.push("the audit's save changed the live world".to_owned());
        }
        let rng = live.world.rng.clone();
        match image {
            Ok(image) => self.two_loads(image, &ledger, &rng, faults, &mut findings),
            Err(error) => findings.push(format!("the save refused: {error}")),
        }
        if findings.is_empty() {
            AuditVerdict::Clean
        } else {
            AuditVerdict::Mismatch(findings)
        }
    }

    fn two_loads(
        &mut self,
        mut image: SlotImage,
        ledger: &Ledger,
        rng: &vault_core::ParkMiller,
        faults: &mut impl Faults,
        findings: &mut Vec<String>,
    ) {
        let Some(second) = interactive_world(&mut self.session) else {
            findings.push("the second host shows no world".to_owned());
            return;
        };
        let host = &mut second.host;
        let mut load = |host: &mut WorldHost<VfsWorldAssets>, image: &mut SlotImage, name: u8| {
            host.world.rng = rng.clone();
            let loaded = host.load_slot(image);
            host.world.rng = rng.clone();
            host.world.effects.clear();
            faults.after_load(host, name);
            loaded
        };
        if let Err(error) = load(host, &mut image, 0) {
            findings.push(format!("load A refused the slot: {error}"));
            return;
        }
        let ran = LoadScripts::of(&host.world);
        let first = Ledger::of(&host.world);
        let snapshot = host.world.state_snapshot();
        let hash = snapshot.state_hash();
        findings.extend(diff_ledgers(
            ledger,
            &first,
            &ran,
            &mut self.tally.allowlisted,
        ));
        let mut again = match host.slot_image(&SaveInfo::default()) {
            Ok(image) => image,
            Err(error) => {
                findings.push(format!("the save of load A refused: {error}"));
                return;
            }
        };
        if let Err(error) = load(host, &mut again, 1) {
            findings.push(format!("load B refused the slot: {error}"));
            return;
        }
        let last = host.world.state_snapshot();
        let other = last.state_hash();
        if hash == other {
            return;
        }
        let allowed = &mut self.tally.allowlisted;
        if moved_in_load(ledger, &first) && only_handles_differ(&snapshot, &last) {
            *allowed.entry(allow::RENUMBERED).or_default() += 1;
            return;
        }
        let second_load = Ledger::of(&host.world);
        let ran = LoadScripts::of(&host.world);
        let mut taken = BTreeMap::new();
        let left = diff_ledgers(&first, &second_load, &ran, &mut taken);
        if left.is_empty() && !taken.is_empty() {
            *allowed.entry(allow::UNSETTLED).or_default() += 1;
            for (entry, count) in taken {
                *allowed.entry(entry).or_default() += count;
            }
            return;
        }
        findings.push(format!(
            "state_hash(A) {hash:016X} differs from state_hash(B) {other:016X}"
        ));
        findings.extend(left);
        findings.extend(hashed_parts(&snapshot, &last));
    }
}

/// A snapshot's entities without their handles, as sorted text.
fn entities_unhandled(snapshot: &StateSnapshot) -> Vec<String> {
    let mut rows: Vec<String> = snapshot
        .entities
        .iter()
        .map(|entity| {
            let mut inventory = entity.inventory.clone();
            inventory.sort_unstable();
            format!(
                "{:#010x} {:?} {} {} {} {:?}",
                entity.pid, entity.position, entity.orientation, entity.hp, entity.ap, inventory
            )
        })
        .collect();
    rows.sort();
    rows
}

/// A combat record without its handles: the round, the turn, and each
/// participant's state in list order.
fn combat_unhandled(snapshot: &StateSnapshot) -> Option<impl PartialEq + use<>> {
    let live = |list: &[Participant]| -> Vec<Option<ParticipantState>> {
        list.iter().map(|participant| participant.live).collect()
    };
    let combat = snapshot.combat.as_ref()?;
    Some((
        combat.round,
        combat.current,
        live(&combat.active),
        live(&combat.waiting),
    ))
}

/// Two snapshots hold the same state under other entity handles: everything
/// the hash covers matches, but for which slot each entity landed in.
fn only_handles_differ(first: &StateSnapshot, second: &StateSnapshot) -> bool {
    entities_unhandled(first) == entities_unhandled(second)
        && first.rng == second.rng
        && first.gvars == second.gvars
        && first.mvars == second.mvars
        && first.lvars == second.lvars
        && combat_unhandled(first) == combat_unhandled(second)
}

/// Which covered parts of two snapshots differ, for a report.
fn hashed_parts(first: &StateSnapshot, second: &StateSnapshot) -> Vec<String> {
    let mut findings = Vec::new();
    let (one, other) = (entities_unhandled(first), entities_unhandled(second));
    if one != other {
        findings.push(format!("entities: {} became {}", one.len(), other.len()));
        for row in one.iter().filter(|row| !other.contains(row)).take(8) {
            findings.push(format!("  only in A: {row}"));
        }
        for row in other.iter().filter(|row| !one.contains(row)).take(8) {
            findings.push(format!("  only in B: {row}"));
        }
    }
    let parts = [
        ("the globals", first.gvars != second.gvars),
        ("the map variables", first.mvars != second.mvars),
        ("the locals", first.lvars != second.lvars),
        ("the combat record", first.combat != second.combat),
    ];
    for (part, differs) in parts {
        if differs {
            findings.push(format!("{part} differ"));
        }
    }
    if findings.is_empty() {
        findings.push("the same entities under other handles".to_owned());
    }
    findings
}

/// Whether a load's scripts moved anything: an object it kept stands on
/// another hex than it did in the save.
fn moved_in_load(live: &Ledger, loaded: &Ledger) -> bool {
    live.objects.iter().any(|(id, row)| {
        loaded
            .objects
            .get(id)
            .is_some_and(|other| other.hex != row.hex)
    })
}

/// Why saving is refused now, if it is. No map is loaded on the world map, and
/// a debugger's VM holds the world until it ends.
fn refused_moment(host: &WorldHost<VfsWorldAssets>) -> Option<&'static str> {
    if !host.world.map_loaded() {
        Some("no map is loaded")
    } else if host.debug_vm.is_some() {
        Some("a debugger VM is running")
    } else {
        None
    }
}

/// Live objects that break the unique-identifier rule: identifier 0, or one
/// held by two objects. The index itself is private to `vault-core`, so these
/// are the entries that must be stale, seen from outside.
fn stale_ids(world: &WorldState) -> Vec<String> {
    let mut seen: BTreeMap<u32, ObjectRef> = BTreeMap::new();
    let mut findings = Vec::new();
    for (reference, object) in world.iter() {
        let id = object.raw.id;
        if id == 0 {
            findings.push(format!("object reference {reference} has identifier 0"));
        } else if let Some(other) = seen.insert(id, reference) {
            findings.push(format!(
                "identifier {id} is held by references {other} and {reference}"
            ));
        }
    }
    for id in world.stale_object_ids() {
        findings.push(format!("the id index holds retired identifier {id}"));
    }
    findings
}

/// What the world looks like to the audit triggers.
#[derive(Debug, Default)]
struct Triggers {
    /// The working name of the loaded map.
    place: Option<String>,
    combat: bool,
    /// An explosion is in progress.
    blast: bool,
    /// Explosive events waiting.
    charges: usize,
}

impl Triggers {
    fn of(world: &WorldState) -> Self {
        Self {
            place: world
                .map_loaded()
                .then(|| world.current_map_name())
                .flatten(),
            combat: world.combat,
            blast: world.explosion.is_some()
                || world.pending_explosion.is_some()
                || world.shows.iter().any(|show| show.explosive),
            charges: world
                .queue
                .events()
                .filter(|event| {
                    matches!(event.kind, EventKind::Other(kind)
                        if kind == EXPLOSION_EVENT || kind == PREMATURE_EVENT)
                })
                .count(),
        }
    }
    /// Why an audit is due, moving on to `self`. An explosion is over when its
    /// progress ends, or when an unanimated one took its event with no
    /// progress to see.
    fn next(&mut self, world: &WorldState) -> Vec<AuditReason> {
        let now = Self::of(world);
        let mut due = Vec::new();
        if now.place.is_some() && now.place != self.place {
            due.push(AuditReason::MapTransition);
        }
        if self.combat && !now.combat {
            due.push(AuditReason::CombatEnd);
        }
        if !now.blast && (self.blast || now.charges < self.charges) {
            due.push(AuditReason::Explosion);
        }
        *self = now;
        due
    }
}

/// The run's records, its stream and its counts.
struct Run {
    log: Vec<u8>,
    ids: SequenceIds,
    records: u64,
    checkpoints: String,
    stream: CheckpointStream,
    phase: Phase,
    triggers: Triggers,
    /// The last row's `state_hash` digits, and the map it was on.
    last: (String, String),
    /// Actions in a row that left the state as it was. The clock is no
    /// measure: it runs on while an action is refused.
    frozen: u32,
    stuck_limit: u32,
    actions: u64,
    refusals: u64,
    ticks: u64,
}

/// How a tick left the run.
enum Flow {
    Going,
    Over(RunEnd),
    Failed(FailureKind, Vec<String>),
}

fn record(
    log: &mut Vec<u8>,
    ids: &mut SequenceIds,
    records: &mut u64,
    command: &InputCommand,
) -> AppResult<()> {
    input::write_command(log, ids, command)?;
    *records += 1;
    Ok(())
}

impl Run {
    fn new(stuck_limit: u32) -> Self {
        Self {
            log: Vec::new(),
            ids: SequenceIds::default(),
            records: 0,
            checkpoints: String::new(),
            stream: CheckpointStream::new(None),
            phase: Phase::Decide,
            triggers: Triggers::default(),
            last: Default::default(),
            frozen: 0,
            stuck_limit,
            actions: 0,
            refusals: 0,
            ticks: 0,
        }
    }
    fn push(&mut self, command: &InputCommand) -> AppResult<()> {
        record(&mut self.log, &mut self.ids, &mut self.records, command)
    }
    /// The stream's head and row 0: what a replay writes before its first
    /// record.
    fn open(&mut self, world: &mut MapPreview) -> AppResult<()> {
        self.triggers = Triggers::of(&world.host.world);
        let head = self.stream.head().map_err(|failure| failure.to_string())?;
        self.checkpoints.push_str(&head);
        self.row(&world.host.world, 0)?;
        Ok(())
    }
    /// The row of the record count `applied`, and whether the world stood
    /// still since the last one.
    fn row(&mut self, world: &WorldState, applied: u64) -> AppResult<bool> {
        let line = self
            .stream
            .row(world, applied)
            .map_err(|(_, failure)| failure.to_string())?;
        self.checkpoints.push_str(&line);
        let hash = line.split('\t').nth(2).unwrap_or_default().to_owned();
        let place = self.triggers.place.clone().unwrap_or_default();
        let now = (hash, place);
        let same = now == self.last;
        self.last = now;
        Ok(same)
    }

    fn step(
        &mut self,
        session: &mut SessionRuntime,
        agent: &mut impl Decider,
        auditor: &mut ParityAuditor,
        faults: &mut impl Faults,
        tick: u64,
    ) -> AppResult<Flow> {
        self.ticks = tick;
        self.phase = Phase::Decide;
        let (log, ids, records, phase) = (
            &mut self.log,
            &mut self.ids,
            &mut self.records,
            &mut self.phase,
        );
        let step = autoplay_step(
            session,
            agent,
            |command| {
                *phase = match command {
                    InputCommand::Tick { .. } => Phase::Tick,
                    _ => Phase::Apply,
                };
                record(log, ids, records, command)
            },
            tick * FRAME_MS,
        )?;
        self.phase = Phase::Checks;
        if step.action.is_some() {
            self.actions += 1;
        }
        if matches!(step.outcome, Some(BotOutcome::Refused(_))) {
            self.refusals += 1;
        }
        let world = interactive_world(session).ok_or("the session shows no world")?;
        if step.finished {
            // A replay writes no row for the tick that ends the session.
            self.last.0 = format!("{:016X}", world.host.world.state_hash());
            return Ok(Flow::Over(RunEnd::SessionOver));
        }
        let due = self.triggers.next(&world.host.world);
        let still = self.row(&world.host.world, self.records)?;
        faults.after_tick(world, tick);
        let status = agent.status();
        if let Some(violation) = &status.violation {
            return Ok(Flow::Failed(
                FailureKind::BotViolation,
                vec![violation.clone()],
            ));
        }
        if status.finished {
            return Ok(Flow::Over(RunEnd::Finished));
        }
        self.frozen = match (step.action.is_some(), still) {
            (true, true) => self.frozen + 1,
            (false, true) => self.frozen,
            (_, false) => 0,
        };
        let moves = status.stuck.max(self.frozen);
        if moves >= self.stuck_limit {
            return Ok(Flow::Failed(
                FailureKind::StuckLoop,
                vec![format!(
                    "{moves} decisions changed nothing: goal {:?}, tactic {:?}",
                    status.goal, status.tactic
                )],
            ));
        }
        if !due.is_empty() {
            match auditor.audit(world, &due, faults) {
                AuditVerdict::Clean | AuditVerdict::Skipped(_) => {}
                AuditVerdict::Mismatch(findings) => {
                    return Ok(Flow::Failed(FailureKind::AuditMismatch, findings));
                }
                AuditVerdict::StaleIds(findings) => {
                    return Ok(Flow::Failed(FailureKind::StaleIdIndex, findings));
                }
            }
        }
        if world.host.world.is_dead(world.host.world.player) {
            return Ok(Flow::Over(RunEnd::PlayerDead));
        }
        Ok(Flow::Going)
    }
}

/// A panic's message.
fn panic_text(payload: &(dyn Any + Send)) -> String {
    payload
        .downcast_ref::<&str>()
        .map(|text| (*text).to_owned())
        .or_else(|| payload.downcast_ref::<String>().cloned())
        .unwrap_or_else(|| "a panic with no message".to_owned())
}

/// `run_headless_autoplay` for `options`: the bot plays until the tick budget
/// or a failure, and the folder `options.output` gets the run's files. A failed
/// run returns `Ok` with [`AutoplayReport::failure`] set; `Err` is a run that
/// could not start or could not write its files.
pub fn run_headless_autoplay(options: AutoplayOptions) -> AppResult<AutoplayReport> {
    let seed = resolved_seed(&options);
    let agent = AutoplayAgent::new(seed, options.chaos)?;
    run_with(AutoplayOptions { seed, ..options }, agent, NoFaults)
}

fn resolved_seed(options: &AutoplayOptions) -> i32 {
    PreviewArgs {
        seed: Some(options.seed),
        ..Default::default()
    }
    .resolved_seed()
}

/// [`run_headless_autoplay`] with `agent`'s decisions and `faults`.
pub fn run_with(
    options: AutoplayOptions,
    mut agent: impl Decider,
    mut faults: impl Faults,
) -> AppResult<AutoplayReport> {
    let session_args = PreviewArgs {
        seed: Some(options.seed),
        ..options.session.clone()
    };
    let mut session = start_world(&session_args)?;
    let mut auditor = ParityAuditor::new(&session_args)?;
    let started = std::time::Instant::now();
    let mut run = Run::new(options.stuck_limit);
    let [width, height] = INITIAL_SURFACE;
    run.push(&InputCommand::Surface { width, height })?;
    if let Some(mods) = session.mod_session() {
        run.push(&input::mod_stack_record(&mods))?;
    }
    let world = interactive_world(&mut session).ok_or("the session shows no world")?;
    run.open(world)?;
    let manifest = BundleManifest::of(&session_args, &options, &session)?;
    let mut failure = None;
    let mut ended = RunEnd::Budget;
    for tick in 1..=options.ticks {
        let result = catch_unwind(AssertUnwindSafe(|| {
            run.step(&mut session, &mut agent, &mut auditor, &mut faults, tick)
        }));
        let (kind, detail) = match result {
            Ok(Ok(Flow::Going)) => continue,
            Ok(Ok(Flow::Over(end))) => {
                ended = end;
                break;
            }
            Ok(Ok(Flow::Failed(kind, detail))) => (kind, detail),
            Ok(Err(error)) => (FailureKind::Error, vec![error.to_string()]),
            Err(payload) => (FailureKind::Panic, vec![panic_text(payload.as_ref())]),
        };
        failure = Some(RunFailure {
            kind,
            tick,
            phase: run.phase,
            detail,
        });
        ended = RunEnd::Failed;
        break;
    }
    let seconds = started.elapsed().as_secs_f64();
    let report = AutoplayReport {
        ticks: run.ticks,
        records: run.records,
        actions: run.actions,
        refusals: run.refusals,
        tally: auditor.tally.clone(),
        bot: agent.status().clone(),
        state_hash: run.last.0.clone(),
        output: options.output.clone(),
        ticks_per_second: run.ticks as f64 / seconds.max(f64::EPSILON),
        ended,
        failure,
    };
    let manifest = BundleManifest {
        current_map: run.triggers.place.clone(),
        ticks: report.ticks,
        state_hash: report.state_hash.clone(),
        failure: report.failure.clone(),
        ..manifest
    };
    write_bundle(&options.output, &run, &manifest, &report)?;
    Ok(report)
}

/// The manifest of a bundle: what a replay needs to start the same session,
/// and what the run ended on.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BundleManifest {
    pub format: String,
    pub version: u32,
    pub seed: i32,
    /// The start map.
    pub map: String,
    /// The map loaded at the end, if one was.
    pub current_map: Option<String>,
    pub preset: String,
    pub profile: String,
    /// The mod layers in order, and the load order they came from, if any.
    pub layers: Vec<String>,
    pub load_order: Option<String>,
    pub sfall: bool,
    pub chaos: bool,
    pub corpus_dir: Option<String>,
    pub master_dat: Option<String>,
    pub critter_dat: Option<String>,
    pub player_gcd: Option<String>,
    pub language: Option<String>,
    /// Ticks run, the failing one included.
    pub ticks: u64,
    /// The last row's `state_hash`.
    pub state_hash: String,
    pub failure: Option<RunFailure>,
}

fn path_text(path: &Option<PathBuf>) -> Option<String> {
    path.as_ref().map(|path| path.display().to_string())
}

impl BundleManifest {
    fn of(
        args: &PreviewArgs,
        options: &AutoplayOptions,
        session: &SessionRuntime,
    ) -> AppResult<Self> {
        let layers = match &args.mods {
            Some(mods) => crate::mods::enabled_layer_dirs(mods)
                .map_err(|error| format!("mod_list: {error}"))?
                .iter()
                .map(|path| path.display().to_string())
                .collect(),
            None => Vec::new(),
        };
        let load_order = match &args.mods {
            Some(crate::config::ModList::File(path)) => Some(path.display().to_string()),
            _ => None,
        };
        Ok(Self {
            format: BUNDLE_FORMAT.to_owned(),
            version: BUNDLE_VERSION,
            seed: options.seed,
            map: args.preview_map.clone().unwrap_or_default(),
            current_map: None,
            preset: match session.preset() {
                crate::config::Preset::Vanilla => "vanilla",
                crate::config::Preset::Enhanced => "enhanced",
            }
            .to_owned(),
            profile: crate::config::game_name(session.edition()).to_owned(),
            layers,
            load_order,
            sfall: args.sfall,
            chaos: options.chaos,
            corpus_dir: path_text(&args.corpus_dir),
            master_dat: path_text(&args.master_dat),
            critter_dat: path_text(&args.critter_dat),
            player_gcd: args.player_gcd.clone(),
            language: args.language.clone(),
            ticks: 0,
            state_hash: String::new(),
            failure: None,
        })
    }
    /// The session this bundle's run started.
    fn session_args(&self) -> AppResult<PreviewArgs> {
        if self.format != BUNDLE_FORMAT || self.version != BUNDLE_VERSION {
            return Err(format!(
                "a bundle of format {} {}, not {BUNDLE_FORMAT} {BUNDLE_VERSION}",
                self.format, self.version
            )
            .into());
        }
        let preset = match self.preset.as_str() {
            "vanilla" => vault_core::EnginePreset::Vanilla,
            "enhanced" => vault_core::EnginePreset::Enhanced,
            other => return Err(format!("a bundle of preset {other}").into()),
        };
        let mods = match (&self.load_order, self.layers.is_empty()) {
            (Some(path), _) => Some(crate::config::ModList::File(path.into())),
            (None, false) => Some(crate::config::ModList::Paths(
                self.layers.iter().map(PathBuf::from).collect(),
            )),
            (None, true) => None,
        };
        Ok(PreviewArgs {
            preset,
            game_profile: crate::session::parse_edition(&self.profile)?,
            mods,
            sfall: self.sfall,
            corpus_dir: self.corpus_dir.as_ref().map(PathBuf::from),
            master_dat: self.master_dat.as_ref().map(PathBuf::from),
            critter_dat: self.critter_dat.as_ref().map(PathBuf::from),
            preview_map: Some(self.map.clone()),
            player_gcd: self.player_gcd.clone(),
            language: self.language.clone(),
            seed: Some(self.seed),
            ..Default::default()
        })
    }
}

/// `report.txt`: tick counts, but for its one throughput line.
fn report_text(manifest: &BundleManifest, report: &AutoplayReport) -> String {
    let mut text = String::new();
    let mut line = |row: String| {
        text.push_str(&row);
        text.push('\n');
    };
    line(format!(
        "result: {}",
        if report.failure.is_some() {
            "FAILED"
        } else {
            "clean"
        }
    ));
    if let Some(failure) = &report.failure {
        line(format!(
            "failure: {:?} at tick {} ({:?})",
            failure.kind, failure.tick, failure.phase
        ));
        for finding in failure.detail.iter().take(MAX_FINDINGS) {
            line(format!("  {finding}"));
        }
        if failure.detail.len() > MAX_FINDINGS {
            line(format!(
                "  and {} more",
                failure.detail.len() - MAX_FINDINGS
            ));
        }
    }
    line(format!(
        "seed {} map {} preset {} profile {} layers {:?}",
        manifest.seed, manifest.map, manifest.preset, manifest.profile, manifest.layers
    ));
    line(format!(
        "ended: {:?}, on {:?}",
        report.ended, manifest.current_map
    ));
    line(format!(
        "ticks {} records {} actions {} refusals {}",
        report.ticks, report.records, report.actions, report.refusals
    ));
    line(format!("state_hash {}", report.state_hash));
    line(format!(
        "bot: goal {:?}, tactic {:?}, {} checks held, {} failed, {} goals given up",
        report.bot.goal,
        report.bot.tactic,
        report.bot.checks,
        report.bot.violations,
        report.bot.abandoned
    ));
    let rows = |name: &str, counts: &BTreeMap<&'static str, u64>, line: &mut dyn FnMut(String)| {
        for (entry, count) in counts {
            line(format!("{name} {entry}: {count}"));
        }
    };
    rows("audited at", &report.tally.audits, &mut line);
    rows("skipped because", &report.tally.skipped, &mut line);
    rows("allowlisted", &report.tally.allowlisted, &mut line);
    line(format!(
        "throughput: {:.0} ticks per wall-clock second",
        report.ticks_per_second
    ));
    text
}

fn write_bundle(
    folder: &Path,
    run: &Run,
    manifest: &BundleManifest,
    report: &AutoplayReport,
) -> AppResult<()> {
    std::fs::create_dir_all(folder).map_err(|error| format!("{}: {error}", folder.display()))?;
    let write = |name: &str, bytes: &[u8]| -> AppResult<()> {
        let path = folder.join(name);
        std::fs::write(&path, bytes).map_err(|error| format!("{}: {error}", path.display()))?;
        Ok(())
    };
    write(INPUTS_FILE, &run.log)?;
    write(CHECKPOINTS_FILE, run.checkpoints.as_bytes())?;
    write(
        MANIFEST_FILE,
        serde_json::to_string_pretty(manifest)?.as_bytes(),
    )?;
    write(REPORT_FILE, report_text(manifest, report).as_bytes())
}

/// What a bundle's replay showed. It reached the run's state, row by row:
/// anything else is the `Err` [`crate::replay::CheckpointsDiverged`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BundleReplay {
    /// The rows of the bundle, each of which the replay matched.
    pub rows: usize,
    /// The last row's `state_hash`, as the replayed world holds it. `None`
    /// after a panic, when the world is not to be read.
    pub state_hash: Option<String>,
    /// The panic, or the error the run failed with, that the replay ended in.
    pub stopped_by: Option<String>,
    /// For an audit's failure: the audit again on the replayed world.
    pub audit: Option<AuditVerdict>,
}

/// Replay the bundle in `folder` from its manifest and compare every row.
pub fn replay_bundle(folder: &Path) -> AppResult<BundleReplay> {
    replay_bundle_with(folder, &mut NoFaults)
}

/// [`replay_bundle`], with `faults` in the audit it repeats.
pub fn replay_bundle_with(folder: &Path, faults: &mut impl Faults) -> AppResult<BundleReplay> {
    let read = |name: &str| -> AppResult<String> {
        let path = folder.join(name);
        Ok(std::fs::read_to_string(&path)
            .map_err(|error| format!("{}: {error}", path.display()))?)
    };
    let manifest: BundleManifest = serde_json::from_str(&read(MANIFEST_FILE)?)?;
    let args = manifest.session_args()?;
    let log = read(INPUTS_FILE)?;
    let checkpoints = read(CHECKPOINTS_FILE)?;
    let expected = ExpectedStream::parse(&checkpoints)?;
    let rows = checkpoints.lines().count().saturating_sub(2);
    let session = start_world(&args)?;
    let mut replay = Replay::with_checkpoints(
        session,
        CheckpointOptions {
            out: None,
            expected: Some(expected),
        },
    )?;
    let replayed = catch_unwind(AssertUnwindSafe(|| replay.run(log.as_bytes())));
    let failed = manifest.failure.as_ref().map(|failure| failure.kind);
    let stopped_by = match replayed {
        Ok(Ok(())) => None,
        Ok(Err(error)) if failed == Some(FailureKind::Error) => Some(error.to_string()),
        Ok(Err(error)) => return Err(error),
        Err(payload) => Some(panic_text(payload.as_ref())),
    };
    if stopped_by.is_some() {
        return Ok(BundleReplay {
            rows,
            state_hash: None,
            stopped_by,
            audit: None,
        });
    }
    let world = replay.world().ok_or("the replay shows no world")?;
    let state_hash = format!("{:016X}", world.host.world.state_hash());
    let audited = matches!(
        failed,
        Some(FailureKind::AuditMismatch | FailureKind::StaleIdIndex)
    );
    let audit = if audited {
        let mut auditor = ParityAuditor::new(&args)?;
        Some(auditor.audit(world, &[], faults))
    } else {
        None
    };
    // The verdict on the stream: a failed comparison or a short run.
    replay.finish(None)?;
    if state_hash != manifest.state_hash {
        return Err(format!(
            "the replay ended on state_hash {state_hash}, the run on {}",
            manifest.state_hash
        )
        .into());
    }
    Ok(BundleReplay {
        rows,
        state_hash: Some(state_hash),
        stopped_by: None,
        audit,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        map_preview::test_support::synthetic_corpus,
        replay::{CheckpointsDiverged, replay_with_checkpoints},
    };
    use vault_core::{autoplay::BotVerb, critical::state::END_REQUESTED};
    use vault_fixtures::campaign::{self, Edition};

    const EDITIONS: [(Edition, GameProfile); 2] = [
        (Edition::Fallout1, GameProfile::Fallout1),
        (Edition::Fallout2, GameProfile::Fallout2),
    ];

    /// A hex beside the first map's foe, and one that a walk to it from the
    /// entrance reaches without crossing the exit grid in between.
    const FOE_SIDE: i32 = 20901;
    const CLEAR_OF_THE_EXIT: i32 = 20920;

    /// A corpus holding the campaign, and where a run writes.
    struct Fixture {
        corpus: tempfile::TempDir,
        output: tempfile::TempDir,
        edition: Edition,
        profile: GameProfile,
    }
    impl Fixture {
        fn new((edition, profile): (Edition, GameProfile)) -> Self {
            let corpus = synthetic_corpus();
            campaign::write_campaign(corpus.path(), edition);
            Self {
                corpus,
                output: tempfile::tempdir().unwrap(),
                edition,
                profile,
            }
        }
        fn options(&self, ticks: u64) -> AutoplayOptions {
            let map = campaign::start(self.edition);
            let mut options =
                AutoplayOptions::new(self.corpus.path(), self.profile, map, self.output.path());
            options.session.player_gcd = Some("premade/synthetic.gcd".into());
            options.ticks = ticks;
            options.seed = 42;
            options
        }
        fn args(&self) -> PreviewArgs {
            let options = self.options(0);
            PreviewArgs {
                seed: Some(options.seed),
                ..options.session
            }
        }
        fn read(&self, name: &str) -> String {
            std::fs::read_to_string(self.output.path().join(name)).unwrap()
        }
        fn manifest(&self) -> BundleManifest {
            serde_json::from_str(&self.read(MANIFEST_FILE)).unwrap()
        }
    }

    /// A stand-in for the agent: `plan`'s actions on their ticks, and
    /// `repeat` on every other tick.
    #[derive(Default)]
    struct Script {
        plan: BTreeMap<u64, BotAction>,
        repeat: Option<BotAction>,
        tick: u64,
        violation_at: Option<u64>,
        panic_at: Option<u64>,
        outcomes: Vec<BotOutcome>,
        status: BotStatus,
    }
    impl Script {
        fn plan(actions: impl IntoIterator<Item = (u64, BotAction)>) -> Self {
            Self {
                plan: actions.into_iter().collect(),
                ..Self::default()
            }
        }
    }
    impl Decider for Script {
        fn decide(&mut self, _: &WorldHost<VfsWorldAssets>, _: &BotView) -> Option<BotAction> {
            self.tick += 1;
            if self.panic_at == Some(self.tick) {
                panic!("the stand-in fell over");
            }
            if self.violation_at == Some(self.tick) {
                self.status.violation = Some("a jammed door opened".to_owned());
            }
            self.plan.remove(&self.tick).or(self.repeat.clone())
        }
        fn note_outcome(&mut self, outcome: &BotOutcome) {
            self.outcomes.push(outcome.clone());
        }
        fn status(&self) -> &BotStatus {
            &self.status
        }
    }

    /// A fault run on a session's tick.
    type TickFault = (u64, fn(&mut MapPreview));
    /// A fault run on the load with this number.
    type LoadFault = (u8, fn(&mut WorldHost<VfsWorldAssets>));

    /// Faults: a panic after `panic_at`, and `load` run after a load.
    #[derive(Default)]
    struct Injected {
        panic_at: Option<u64>,
        tick: Option<TickFault>,
        load: Option<LoadFault>,
    }
    impl Faults for Injected {
        fn after_tick(&mut self, world: &mut MapPreview, tick: u64) {
            if self.panic_at == Some(tick) {
                panic!("an injected fault");
            }
            if let Some((at, fault)) = self.tick
                && at == tick
            {
                fault(world);
            }
        }
        fn after_load(&mut self, host: &mut WorldHost<VfsWorldAssets>, load: u8) {
            if let Some((at, fault)) = self.load
                && at == load
            {
                fault(host);
            }
        }
    }

    fn walk(hex: i32) -> BotAction {
        BotAction::Walk { hex, run: true }
    }
    /// A walk onto each map's exit grid, far enough apart to load.
    fn exits() -> Script {
        Script::plan([20, 60, 100].map(|tick| (tick, walk(campaign::EXIT_HEX))))
    }
    /// Pick the dynamite up, arm it on the spot, and walk well clear.
    fn bomb() -> Script {
        Script::plan([
            (
                5,
                BotAction::Interact {
                    target: campaign::DYNAMITE,
                    verb: BotVerb::Pickup,
                },
            ),
            (
                10,
                BotAction::ArmExplosive {
                    item: campaign::DYNAMITE,
                    seconds: 10,
                },
            ),
            (15, walk(campaign::ENTRY + 5 * 200)),
        ])
    }
    /// Walk up to the foe and hit it, then pass every turn.
    fn fight() -> Script {
        let punch = BotAction::Attack {
            target: campaign::FOE,
            hit_mode: vault_core::critical::hit_mode::PUNCH,
            location: None,
        };
        Script {
            repeat: Some(BotAction::EndTurn),
            ..Script::plan([
                (3, walk(CLEAR_OF_THE_EXIT)),
                (5, walk(FOE_SIDE)),
                (10, punch),
            ])
        }
    }
    fn rows(text: &str) -> usize {
        text.lines().count() - 2
    }

    /// `autoplay_step` takes the shell's order: the action is logged before it
    /// applies, the tick is logged before it runs, the agent hears the
    /// outcome, and a step that decides nothing logs only its tick.
    #[test]
    fn autoplay_step_logs_each_record_before_it_takes_effect() {
        let fixture = Fixture::new(EDITIONS[0]);
        let mut session = start_world(&fixture.args()).unwrap();
        let mut agent = Script::plan([(1, walk(campaign::ENTRY + 2)), (3, BotAction::EndTurn)]);
        let hex = |session: &mut SessionRuntime| {
            let world = interactive_world(session).unwrap();
            world
                .host
                .world
                .get(world.host.world.player)
                .unwrap()
                .raw
                .hex
        };
        let start = hex(&mut session);
        let mut seen = Vec::new();
        let world = std::cell::RefCell::new(&mut session);
        let step = |agent: &mut Script, now: u64, seen: &mut Vec<String>| {
            let mut session = world.borrow_mut();
            autoplay_step(
                &mut session,
                agent,
                |command| {
                    let text = serde_json::to_value(command).unwrap();
                    seen.push(text["kind"].as_str().unwrap().to_owned());
                    Ok(())
                },
                now,
            )
            .unwrap()
        };
        let first = step(&mut agent, 16, &mut seen);
        assert_eq!(first.action, Some(walk(campaign::ENTRY + 2)));
        assert_eq!(
            first.outcome,
            Some(BotOutcome::Done),
            "{:?}",
            agent.outcomes
        );
        let idle = step(&mut agent, 32, &mut seen);
        assert_eq!((idle.action, idle.outcome), (None, None));
        let refused = step(&mut agent, 48, &mut seen);
        assert!(matches!(refused.outcome, Some(BotOutcome::Refused(_))));
        assert_eq!(seen, ["bot", "tick", "tick", "bot", "tick"]);
        assert_eq!(agent.outcomes.len(), 2, "the agent hears each outcome");
        assert_ne!(hex(world.into_inner()), start, "the walk went out");
    }

    /// The same seed twice: the same log and the same checkpoints, row for
    /// row, with the real agent and its chaos audit on both editions.
    /// The Wave 2 sweep: every seed plays the campaign to its end on both
    /// editions with the chaos audit on, no failure, and the doors and fights
    /// really happen. Each run's bundle replays to its own checkpoints.
    #[test]
    fn autoplay_the_campaign_sweep_finds_no_failure() {
        for edition in EDITIONS {
            let (mut checks, mut fought) = (0, false);
            for seed in 1..=8 {
                let fixture = Fixture::new(edition);
                let mut options = fixture.options(50_000);
                options.seed = seed;
                options.session.seed = Some(seed);
                options.chaos = true;
                let report = run_headless_autoplay(options).unwrap();
                assert_eq!(report.failure, None, "{edition:?} seed {seed}");
                assert!(
                    matches!(report.ended, RunEnd::Finished | RunEnd::PlayerDead),
                    "{edition:?} seed {seed}: {:?} at tick {}",
                    report.ended,
                    report.ticks
                );
                assert!(report.tally.total() > 0, "{edition:?} seed {seed}");
                checks += report.bot.checks;
                fought |= fixture.read(INPUTS_FILE).contains(r#""op":"attack""#);
                replay_bundle(fixture.output.path()).unwrap();
            }
            assert!(checks > 0, "{edition:?}: the chaos audit never ran");
            assert!(fought, "{edition:?}: the bot never fought");
        }
    }

    #[test]
    fn autoplay_the_same_seed_gives_identical_checkpoints() {
        for edition in EDITIONS {
            let run = || {
                let fixture = Fixture::new(edition);
                let mut options = fixture.options(1200);
                options.chaos = true;
                let report = run_headless_autoplay(options).unwrap();
                (
                    fixture.read(CHECKPOINTS_FILE),
                    fixture.read(INPUTS_FILE),
                    report,
                )
            };
            let first = run();
            assert_eq!(
                rows(&first.0) as u64,
                first.2.ticks + 1,
                "row 0, then one a tick"
            );
            assert!(first.1.contains(r#""kind":"bot""#), "the bot acted");
            let second = run();
            assert_eq!(first.0, second.0, "{edition:?}");
            assert_eq!(first.1, second.1, "{edition:?}");
            assert_eq!(first.2.failure, second.2.failure, "{edition:?}");
        }
    }

    /// A clean run reports no failure, and its folder is a bundle with none.
    #[test]
    fn autoplay_a_clean_run_reports_no_failure() {
        for edition in EDITIONS {
            let fixture = Fixture::new(edition);
            let report = run_with(fixture.options(160), exits(), NoFaults).unwrap();
            assert_eq!(report.failure, None, "{edition:?}");
            assert_eq!(report.ticks, 160);
            assert_eq!(report.actions, 3, "{edition:?}");
            assert_eq!(report.records, 164, "the surface, three actions, 160 ticks");
            assert!(report.tally.audits["map transition"] >= 1, "{edition:?}");
            assert!(report.ticks_per_second > 0.0);
            let text = fixture.read(REPORT_FILE);
            assert!(text.starts_with("result: clean\n"), "{text}");
            let manifest = fixture.manifest();
            assert_eq!((manifest.seed, manifest.failure), (42, None));
            assert_eq!(manifest.map, campaign::start(fixture.edition));
            assert_eq!(manifest.preset, "vanilla");
            assert_eq!(manifest.profile, crate::config::game_name(fixture.profile));
            assert!(manifest.layers.is_empty());
        }
    }

    /// Every map change, explosion and combat end is audited: the triggers
    /// fire on what the game really does, through logged actions only.
    #[test]
    fn autoplay_audits_each_map_change_and_explosion_and_combat_end() {
        for edition in EDITIONS {
            let fixture = Fixture::new(edition);
            let report = run_with(fixture.options(160), exits(), NoFaults).unwrap();
            assert_eq!(report.failure, None);
            let maps = campaign::maps(fixture.edition).len() as u64;
            assert_eq!(report.tally.audits["map transition"], maps - 1);

            let fixture = Fixture::new(edition);
            let report = run_with(fixture.options(800), bomb(), NoFaults).unwrap();
            assert_eq!(report.failure, None, "{edition:?}");
            assert_eq!(report.tally.audits["explosion"], 1, "{edition:?}");

            // SPEC-013 §3.22: a request ends the fight as a transition's does.
            let fixture = Fixture::new(edition);
            let ending = Injected {
                tick: Some((40, |world| {
                    assert!(world.host.world.combat, "the fight is on");
                    world.host.world.combat_state.flags |= END_REQUESTED;
                })),
                ..Injected::default()
            };
            let report = run_with(fixture.options(200), fight(), ending).unwrap();
            assert_eq!(report.failure, None, "{edition:?}");
            assert_eq!(report.tally.audits["combat end"], 1, "{edition:?}");
        }
    }

    /// An audit leaves no trace: two sessions, one of them audited, go on to
    /// save the same bytes, map and automap included.
    #[test]
    fn autoplay_an_audit_leaves_the_live_world_as_it_found_it() {
        for edition in EDITIONS {
            let fixture = Fixture::new(edition);
            let args = fixture.args();
            let advance = |session: &mut SessionRuntime, from: u64, to: u64| {
                for tick in from..to {
                    session.tick(tick * FRAME_MS).unwrap();
                }
            };
            let start = |args: &PreviewArgs| {
                let mut session = start_world(args).unwrap();
                let world = interactive_world(&mut session).unwrap();
                let hex = campaign::ENTRY + 4;
                world.apply_bot(&walk(hex)).unwrap();
                session
            };
            let (mut plain, mut audited) = (start(&args), start(&args));
            let mut auditor = ParityAuditor::new(&args).unwrap();
            advance(&mut plain, 1, 40);
            advance(&mut audited, 1, 20);
            let world = interactive_world(&mut audited).unwrap();
            let verdict = auditor.audit(world, &[AuditReason::CombatEnd], &mut NoFaults);
            assert_eq!(verdict, AuditVerdict::Clean);
            advance(&mut audited, 20, 40);
            let saved = |session: &mut SessionRuntime| {
                let world = interactive_world(session).unwrap();
                world.host.slot_image(&SaveInfo::default()).unwrap()
            };
            assert_eq!(saved(&mut plain), saved(&mut audited), "{edition:?}");
        }
    }

    /// A fight is saved and resumed: an audit in the middle of one holds too.
    #[test]
    fn autoplay_an_audit_in_combat_is_clean() {
        for edition in EDITIONS {
            let fixture = Fixture::new(edition);
            let args = fixture.args();
            let mut session = start_world(&args).unwrap();
            let mut auditor = ParityAuditor::new(&args).unwrap();
            let mut script = fight();
            for tick in 1..40 {
                autoplay_step(&mut session, &mut script, |_| Ok(()), tick * FRAME_MS).unwrap();
            }
            let world = interactive_world(&mut session).unwrap();
            assert!(world.host.world.combat, "{edition:?}");
            let verdict = auditor.audit(world, &[AuditReason::Explosion], &mut NoFaults);
            assert_eq!(verdict, AuditVerdict::Clean, "{edition:?}");
        }
    }

    /// Saving is refused with no map loaded and while a debugger VM runs, and
    /// the audit skips those moments: neither is a failure.
    #[test]
    fn autoplay_an_audit_skips_the_moments_saving_is_refused() {
        for edition in EDITIONS {
            let fixture = Fixture::new(edition);
            let args = fixture.args();
            let mut session = start_world(&args).unwrap();
            let mut auditor = ParityAuditor::new(&args).unwrap();
            let why = [AuditReason::MapTransition];
            let world = interactive_world(&mut session).unwrap();
            world.host.world.unloaded = true;
            assert_eq!(
                auditor.audit(world, &why, &mut NoFaults),
                AuditVerdict::Skipped("no map is loaded")
            );
            world.host.world.unloaded = false;
            world
                .apply_debug(&vault_core::debug::DebugCommand::VmBegin {
                    object: campaign::SCRIPTED_DOOR,
                    event: ScriptEvent::Damage.code(),
                })
                .unwrap();
            assert!(world.host.debug_vm.is_some());
            assert_eq!(
                auditor.audit(world, &why, &mut NoFaults),
                AuditVerdict::Skipped("a debugger VM is running")
            );
            assert_eq!(auditor.tally().total(), 0);
            assert_eq!(auditor.tally().skipped.values().sum::<u64>(), 2);
        }
    }

    /// What differs and the allowlist does not take is a finding, and what it
    /// takes is counted.
    #[test]
    fn autoplay_the_allowlist_takes_only_its_own_differences() {
        let live = Ledger {
            mvars: vec![1, 2],
            next_id: 100,
            clock: 7,
            ..Ledger::default()
        };
        let mut loaded = live.clone();
        loaded.mvars[1] = 3;
        let mut allowed = BTreeMap::new();
        let none = LoadScripts::default();
        let findings = diff_ledgers(&live, &loaded, &none, &mut allowed);
        assert_eq!(findings, ["map variable 1: 2 became 3"], "no script ran");
        assert!(allowed.is_empty());
        let ran = LoadScripts {
            records: BTreeSet::from([0x0400_0001]),
            owners: BTreeSet::new(),
        };
        assert!(diff_ledgers(&live, &loaded, &ran, &mut allowed).is_empty());
        assert_eq!(allowed[allow::MAP_VARIABLE], 1);
        // Nothing allows the clock, whatever ran.
        loaded.clock = 8;
        let findings = diff_ledgers(&live, &loaded, &ran, &mut allowed);
        assert_eq!(findings, ["clock: 7 became 8"]);
    }

    /// An object's own script may move it, but not another's, and only an
    /// identifier past the saved allocator is an object the load made.
    #[test]
    fn autoplay_the_allowlist_knows_whose_script_ran() {
        let row = ObjectRow {
            pid: 0x0200_0001,
            hex: Some(100),
            elevation: 0,
            orientation: 0,
            art_id: 1,
            flags: 0,
            state: ObjectState::Other {
                word: 0,
                detail: ObjectDetail::Plain,
            },
            holder: None,
            contents: BTreeMap::new(),
        };
        let live = Ledger {
            objects: BTreeMap::from([(7, row.clone())]),
            next_id: 100,
            ..Ledger::default()
        };
        let mut loaded = live.clone();
        loaded.objects.get_mut(&7).unwrap().hex = Some(101);
        loaded.objects.insert(100, row.clone());
        let mut allowed = BTreeMap::new();
        let mut ran = LoadScripts::default();
        let findings = diff_ledgers(&live, &loaded, &ran, &mut allowed);
        assert_eq!(findings.len(), 1, "{findings:?}");
        assert!(findings[0].starts_with("object 7: "), "{findings:?}");
        assert_eq!(allowed[allow::NEW_OBJECT], 1);
        ran.owners.insert(7);
        assert!(diff_ledgers(&live, &loaded, &ran, &mut allowed).is_empty());
        assert_eq!(allowed[allow::SCRIPTED_OBJECT], 1);
        // A script may place a critter, but its hit points stay.
        let critter = |hp: i32, hex: i32| ObjectRow {
            pid: 0x0100_0001,
            hex: Some(hex),
            state: ObjectState::Critter(vault_formats::map::CritterState {
                reaction: 0,
                damage: 0,
                maneuvers: 0,
                ap: 0,
                results: Default::default(),
                ai: 0,
                team: 0,
                attacker: -1,
                hp,
                radiation: 0,
                poison: 0,
            }),
            ..row.clone()
        };
        let mut hurt = (live.clone(), loaded.clone());
        hurt.0.objects.insert(7, critter(30, 100));
        hurt.1.objects.insert(7, critter(29, 100));
        let findings = diff_ledgers(&hurt.0, &hurt.1, &ran, &mut allowed);
        assert!(findings[0].starts_with("object 7: "), "{findings:?}");
        hurt.1.objects.insert(7, critter(30, 101));
        assert!(diff_ledgers(&hurt.0, &hurt.1, &ran, &mut allowed).is_empty());
        // An object gone is never taken.
        loaded.objects.remove(&7);
        let findings = diff_ledgers(&live, &loaded, &ran, &mut allowed);
        assert!(findings[0].contains("is gone"), "{findings:?}");
        // An identifier below the allocator did not come from the load.
        loaded.objects.insert(7, row.clone());
        loaded.objects.insert(50, row.clone());
        let findings = diff_ledgers(&live, &loaded, &ran, &mut allowed);
        assert!(findings[0].contains("appeared"), "{findings:?}");
    }

    /// A Start that moves an object past another makes load B fill its slots
    /// in another order. The hash differs, the state does not: that is taken
    /// for a scripted object, and an unscripted one's move is still a finding.
    #[test]
    fn autoplay_a_scripted_move_only_renumbers_the_handles() {
        for edition in EDITIONS {
            let fixture = Fixture::new(edition);
            let args = fixture.args();
            let mut session = start_world(&args).unwrap();
            let mut auditor = ParityAuditor::new(&args).unwrap();
            let world = interactive_world(&mut session).unwrap();
            let why = [AuditReason::Explosion];
            let mut faults = Injected {
                load: Some((0, |host| {
                    let door = host.world.reference_for_id(campaign::SCRIPTED_DOOR);
                    let door = host.world.get_mut(door.unwrap()).unwrap();
                    door.raw.hex = Some(campaign::ENTRY - 500);
                })),
                ..Injected::default()
            };
            assert_eq!(auditor.audit(world, &why, &mut faults), AuditVerdict::Clean);
            let allowed = &auditor.tally().allowlisted;
            assert_eq!(allowed[allow::RENUMBERED], 1, "{allowed:?}");
            assert_eq!(allowed[allow::SCRIPTED_OBJECT], 1, "{allowed:?}");
            let mut faults = Injected {
                load: Some((0, |host| {
                    let door = host.world.reference_for_id(campaign::PLAIN_DOOR);
                    let door = host.world.get_mut(door.unwrap()).unwrap();
                    door.raw.hex = Some(campaign::ENTRY - 500);
                })),
                ..Injected::default()
            };
            let AuditVerdict::Mismatch(findings) = auditor.audit(world, &why, &mut faults) else {
                panic!("an unscripted move is a finding");
            };
            assert!(findings[0].starts_with("object 10: "), "{findings:?}");
            // A script that places its object afresh on each load never
            // settles: load B is a finding only for what scripts may not do.
            let mut faults = Injected {
                load: Some((1, |host| {
                    let door = host.world.reference_for_id(campaign::SCRIPTED_DOOR);
                    let door = host.world.get_mut(door.unwrap()).unwrap();
                    door.raw.hex = Some(campaign::ENTRY - 500);
                })),
                ..Injected::default()
            };
            assert_eq!(auditor.audit(world, &why, &mut faults), AuditVerdict::Clean);
            assert_eq!(auditor.tally().allowlisted[allow::UNSETTLED], 1);
            let mut faults = Injected {
                load: Some((1, |host| {
                    let door = host.world.reference_for_id(campaign::PLAIN_DOOR);
                    let door = host.world.get_mut(door.unwrap()).unwrap();
                    door.raw.hex = Some(campaign::ENTRY - 500);
                })),
                ..Injected::default()
            };
            let AuditVerdict::Mismatch(findings) = auditor.audit(world, &why, &mut faults) else {
                panic!("load B moved what no script of its own moves");
            };
            assert!(findings[0].starts_with("state_hash(A) "), "{findings:?}");
        }
    }

    /// Handles alone may differ only with everything else the hash covers
    /// the same, and a swapped entity's state is not the same state.
    #[test]
    fn autoplay_only_handles_differ_means_only_handles() {
        let fixture = Fixture::new(EDITIONS[0]);
        let mut session = start_world(&fixture.args()).unwrap();
        let world = interactive_world(&mut session).unwrap();
        let first = world.host.world.state_snapshot();
        let mut second = first.clone();
        let (left, right) = second.entities.split_at_mut(1);
        std::mem::swap(&mut left[0].handle, &mut right[0].handle);
        assert_ne!(first.state_hash(), second.state_hash());
        assert!(only_handles_differ(&first, &second));
        assert_eq!(
            hashed_parts(&first, &second),
            ["the same entities under other handles"]
        );
        second.entities[0].hp += 1;
        assert!(!only_handles_differ(&first, &second));
        let mut third = first.clone();
        third.mvars = Some(vec![1]);
        assert!(!only_handles_differ(&first, &third));
    }

    /// The load's scripts are read from its trace, by record and owner.
    #[test]
    fn autoplay_a_load_names_the_scripts_it_ran() {
        struct Saw(Option<LoadScripts>);
        impl Faults for &mut Saw {
            fn after_load(&mut self, host: &mut WorldHost<VfsWorldAssets>, load: u8) {
                if load == 0 {
                    self.0 = Some(LoadScripts::of(&host.world));
                }
            }
        }
        let fixture = Fixture::new(EDITIONS[0]);
        let args = fixture.args();
        let mut session = start_world(&args).unwrap();
        let mut auditor = ParityAuditor::new(&args).unwrap();
        let mut saw = Saw(None);
        let world = interactive_world(&mut session).unwrap();
        let why = [AuditReason::Explosion];
        assert_eq!(
            auditor.audit(world, &why, &mut &mut saw),
            AuditVerdict::Clean
        );
        let ran = saw.0.unwrap();
        assert!(ran.owners.contains(&campaign::SCRIPTED_DOOR), "{ran:?}");
        assert!(ran.owners.contains(&campaign::FRIEND), "{ran:?}");
        assert!(!ran.owners.contains(&campaign::FOE), "{ran:?}");
    }

    /// SPEC-003 moves a grid to the world map up by 16 on every read. That is
    /// the only art a load may change.
    #[test]
    fn autoplay_only_a_grid_out_of_the_map_changes_its_art() {
        let grid = |map: i32, art_id: u32| ObjectRow {
            pid: 0x0500_0010,
            hex: Some(100),
            elevation: 0,
            orientation: 0,
            art_id,
            flags: 0,
            state: ObjectState::Other {
                word: 0,
                detail: ObjectDetail::ExitGrid {
                    map,
                    hex: 0,
                    elevation: 0,
                    orientation: 0,
                },
            },
            holder: None,
            contents: BTreeMap::new(),
        };
        let art = |index: u16| Fid::new(5, index, 0, 0, 0).0;
        assert!(shifted_exit_art(&grid(-2, art(16)), &grid(-2, art(32))));
        assert!(!shifted_exit_art(&grid(-2, art(16)), &grid(-2, art(33))));
        assert!(!shifted_exit_art(&grid(-2, art(33)), &grid(-2, art(49))));
        assert!(!shifted_exit_art(&grid(3, art(16)), &grid(3, art(32))));
    }

    /// The load's gender update takes the player's pose away and leaves the
    /// rest of the art.
    #[test]
    fn autoplay_only_the_pose_of_the_player_art_may_go() {
        let row = |art: Fid| ObjectRow {
            pid: 0x0100_0000,
            hex: Some(100),
            elevation: 0,
            orientation: 0,
            art_id: art.0,
            flags: 0,
            state: ObjectState::Other {
                word: 0,
                detail: ObjectDetail::Plain,
            },
            holder: None,
            contents: BTreeMap::new(),
        };
        let posed = row(Fid::new(1, 11, 0x22, 0, 6));
        assert!(rebuilt_player_art(&posed, &row(Fid::new(1, 11, 0, 0, 0))));
        assert!(!rebuilt_player_art(&posed, &row(Fid::new(1, 12, 0, 0, 0))));
        assert!(!rebuilt_player_art(&posed, &row(Fid::new(1, 11, 1, 0, 0))));
        let armed = row(Fid::new(1, 11, 0, 3, 0));
        assert!(!rebuilt_player_art(&armed, &row(Fid::new(1, 11, 0, 4, 0))));
    }

    /// A dead player ends the run, and that is no failure: nothing is left
    /// to play, and an idle bot would only look stuck.
    #[test]
    fn autoplay_the_players_death_ends_a_run_cleanly() {
        for edition in EDITIONS {
            let fixture = Fixture::new(edition);
            let kill = Injected {
                tick: Some((10, |world| {
                    let player = world.host.world.player;
                    world.host.world.set_hit_points(player, 0);
                })),
                ..Injected::default()
            };
            let report = run_with(fixture.options(160), Script::default(), kill).unwrap();
            assert_eq!(report.ended, RunEnd::PlayerDead, "{edition:?}");
            assert_eq!((report.failure, report.ticks), (None, 10), "{edition:?}");
        }
    }

    /// Each kind of failure writes the four files, cut at its tick.
    #[test]
    fn autoplay_an_injected_fault_writes_a_bundle() {
        // An audit that finds the load changed the clock.
        let fixture = Fixture::new(EDITIONS[0]);
        let faults = Injected {
            load: Some((0, |host| {
                let ticks = host.world.clock.ticks;
                host.world.clock.set(ticks + 1);
            })),
            ..Injected::default()
        };
        let report = run_with(fixture.options(160), exits(), faults).unwrap();
        let failure = report.failure.unwrap();
        assert_eq!(failure.kind, FailureKind::AuditMismatch);
        assert_eq!((failure.tick, failure.phase), (20, Phase::Checks));
        assert!(failure.detail[0].starts_with("clock: "), "{failure:?}");
        let log = fixture.read(INPUTS_FILE);
        assert!(log.ends_with("\"now_ms\":320}\n"), "cut at tick 20: {log}");
        assert_eq!(rows(&fixture.read(CHECKPOINTS_FILE)), 21);
        assert!(
            fixture
                .read(REPORT_FILE)
                .contains("AuditMismatch at tick 20")
        );
        assert_eq!(fixture.manifest().ticks, 20);

        // Load B parting from load A.
        let fixture = Fixture::new(EDITIONS[1]);
        let faults = Injected {
            load: Some((1, |host| host.world.mvars.push(1))),
            ..Injected::default()
        };
        let report = run_with(fixture.options(160), exits(), faults).unwrap();
        let failure = report.failure.unwrap();
        assert_eq!(failure.kind, FailureKind::AuditMismatch);
        assert!(
            failure.detail[0].starts_with("state_hash(A) "),
            "{failure:?}"
        );

        // Two objects holding one identifier.
        let fixture = Fixture::new(EDITIONS[0]);
        let faults = Injected {
            tick: Some((20, |world| {
                let world = &mut world.host.world;
                let id = world.get(world.player).unwrap().raw.id;
                let door = world.iter().find(|(_, o)| o.raw.pid >> 24 == 2).unwrap().0;
                world.get_mut(door).unwrap().raw.id = id;
            })),
            ..Injected::default()
        };
        let report = run_with(fixture.options(160), exits(), faults).unwrap();
        assert_eq!(report.failure.unwrap().kind, FailureKind::StaleIdIndex);

        // A panic in the tick's checks, and one in the agent.
        let fixture = Fixture::new(EDITIONS[0]);
        let faults = Injected {
            panic_at: Some(30),
            ..Injected::default()
        };
        let report = run_with(fixture.options(160), exits(), faults).unwrap();
        let failure = report.failure.unwrap();
        assert_eq!(failure.kind, FailureKind::Panic);
        assert_eq!((failure.tick, failure.phase), (30, Phase::Checks));
        assert_eq!(failure.detail, ["an injected fault"]);
        assert_eq!(rows(&fixture.read(CHECKPOINTS_FILE)), 31);
        let fixture = Fixture::new(EDITIONS[1]);
        let script = Script {
            panic_at: Some(30),
            ..exits()
        };
        let report = run_with(fixture.options(160), script, NoFaults).unwrap();
        let failure = report.failure.unwrap();
        assert_eq!((failure.tick, failure.phase), (30, Phase::Decide));
        assert_eq!(rows(&fixture.read(CHECKPOINTS_FILE)), 30, "no row for it");

        // A bot violation, and a loop of actions that change nothing.
        let fixture = Fixture::new(EDITIONS[1]);
        let script = Script {
            violation_at: Some(12),
            ..exits()
        };
        let report = run_with(fixture.options(160), script, NoFaults).unwrap();
        let failure = report.failure.unwrap();
        assert_eq!(failure.kind, FailureKind::BotViolation);
        assert_eq!(failure.tick, 12);
        assert_eq!(failure.detail, ["a jammed door opened"]);
        let fixture = Fixture::new(EDITIONS[0]);
        let mut options = fixture.options(200);
        options.stuck_limit = 25;
        let script = Script {
            repeat: Some(BotAction::EndTurn),
            ..Script::default()
        };
        let report = run_with(options, script, NoFaults).unwrap();
        assert_eq!(report.failure.unwrap().kind, FailureKind::StuckLoop);
        assert_eq!(report.refusals, 25);
    }

    /// A bundle replays to the state it was cut at, under the checkpoints it
    /// holds, on both editions: a clean run's, a failed run's and one the
    /// bot acted in throughout. Its log needs no agent.
    #[test]
    fn autoplay_a_bundle_replays_to_the_same_state() {
        for edition in EDITIONS {
            let fixture = Fixture::new(edition);
            let mut options = fixture.options(1200);
            options.chaos = true;
            let report = run_headless_autoplay(options).unwrap();
            let replayed = replay_bundle(fixture.output.path()).unwrap();
            assert_eq!(replayed.rows as u64, report.ticks + 1);
            assert_eq!(replayed.state_hash, Some(report.state_hash));
            assert_eq!(replayed.stopped_by, None);

            let fixture = Fixture::new(edition);
            let script = Script {
                violation_at: Some(70),
                ..exits()
            };
            let report = run_with(fixture.options(160), script, NoFaults).unwrap();
            assert_eq!(report.ticks, 70);
            let replayed = replay_bundle(fixture.output.path()).unwrap();
            assert_eq!(replayed.rows, 71);
            assert_eq!(replayed.state_hash, Some(report.state_hash));

            // A bomb, and a fight: the fuse's and the turns' rows match too.
            let fixture = Fixture::new(edition);
            let report = run_with(fixture.options(800), bomb(), NoFaults).unwrap();
            assert_eq!(report.failure, None);
            let replayed = replay_bundle(fixture.output.path()).unwrap();
            assert_eq!(replayed.state_hash, Some(report.state_hash));
            let fixture = Fixture::new(edition);
            let report = run_with(fixture.options(200), fight(), NoFaults).unwrap();
            let replayed = replay_bundle(fixture.output.path()).unwrap();
            assert_eq!(replayed.state_hash, Some(report.state_hash));
        }
    }

    /// The audit's failure is found again on the replayed world, by the same
    /// findings: the bundle holds what it takes to see the fault.
    #[test]
    fn autoplay_a_replayed_bundle_finds_its_audit_failure_again() {
        let fixture = Fixture::new(EDITIONS[1]);
        let fault = || Injected {
            load: Some((0, |host| {
                let ticks = host.world.clock.ticks;
                host.world.clock.set(ticks + 1);
            })),
            ..Injected::default()
        };
        let report = run_with(fixture.options(160), exits(), fault()).unwrap();
        let failure = report.failure.unwrap();
        let replayed = replay_bundle_with(fixture.output.path(), &mut fault()).unwrap();
        assert_eq!(replayed.audit, Some(AuditVerdict::Mismatch(failure.detail)));
        let healed = replay_bundle(fixture.output.path()).unwrap();
        assert_eq!(
            healed.audit,
            Some(AuditVerdict::Clean),
            "no fault, no finding"
        );
    }

    /// The comparison bites: an edited row is a divergence, and so is a
    /// stream the replay ends short of.
    #[test]
    fn autoplay_a_bundle_that_does_not_replay_is_refused() {
        let fixture = Fixture::new(EDITIONS[0]);
        run_with(fixture.options(60), exits(), NoFaults).unwrap();
        let path = fixture.output.path().join(CHECKPOINTS_FILE);
        let text = fixture.read(CHECKPOINTS_FILE);
        let mut lines: Vec<&str> = text.lines().collect();
        let row = lines[30].replacen('\t', "\tX", 2).replace("\tX", "\t");
        let edited = row
            .split('\t')
            .enumerate()
            .map(|(column, field)| match column {
                2 => "0123456789ABCDEF",
                _ => field,
            })
            .collect::<Vec<_>>()
            .join("\t");
        lines[30] = &edited;
        std::fs::write(&path, lines.join("\n") + "\n").unwrap();
        let error = replay_bundle(fixture.output.path()).unwrap_err();
        assert!(
            error.downcast_ref::<CheckpointsDiverged>().is_some(),
            "{error}"
        );
        assert!(error.to_string().contains("mismatch at tick 28"), "{error}");

        let longer = format!("{text}{}", text.lines().last().unwrap());
        let mut parts: Vec<String> = longer.lines().map(str::to_owned).collect();
        let last = parts.len() - 1;
        parts[last] = parts[last].replacen("60\t", "61\t", 1);
        std::fs::write(&path, parts.join("\n") + "\n").unwrap();
        let error = replay_bundle(fixture.output.path()).unwrap_err();
        assert!(error.to_string().contains("ended early"), "{error}");
    }

    /// The log a run writes is an ordinary log: the plain replay path takes
    /// it, from the same start, and matches the run's rows.
    #[test]
    fn autoplay_a_fallout1_log_replays_on_the_plain_path() {
        let fixture = Fixture::new(EDITIONS[0]);
        run_with(fixture.options(160), exits(), NoFaults).unwrap();
        let session = start_world(&fixture.manifest().session_args().unwrap()).unwrap();
        let expected = ExpectedStream::parse(&fixture.read(CHECKPOINTS_FILE)).unwrap();
        let log = fixture.read(INPUTS_FILE);
        let report = replay_with_checkpoints(
            session,
            log.as_bytes(),
            None,
            CheckpointOptions {
                out: None,
                expected: Some(expected),
            },
        )
        .unwrap();
        assert_eq!(report.commands, 164);
        assert_eq!(report.elapsed_ms, 160 * FRAME_MS);
    }
}
