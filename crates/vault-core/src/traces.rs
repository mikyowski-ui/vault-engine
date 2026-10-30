//! SPEC-021 §3.5 to §3.7 the two trace oracles: the VM opcode trace and
//! the seeded combat hash trace, their TSV text, and the drivers that the
//! `asset-audit` dump modes and the tests share.
//!
//! Implementation notes:
//!
//! - The VM driver steps one program. An instruction that blocks it on a
//!   child program or an external call ends the trace there (the headless
//!   driver has nothing to resume it with), so child and host-initiated
//!   rows are not interleaved in revision 1's driver.
//! - A row whose address lies outside the code writes `-` for the word,
//!   since there is no word to fetch.
use crate::{
    CharacterSheet, EnginePreset, GameProfile, ObjectRef, WorldError, WorldHost, WorldState,
    assets::{VfsWorldAssets, WorldAssets},
    checkpoints::{CheckpointFailure, CheckpointStream},
    combat_state::CombatRequest,
    state_hash::{Handle, StateSnapshot},
};
use std::{fmt, sync::Arc};
use vault_formats::Vfs;
use vault_vm::{RunOutcome, Vm, VmHost, VmStatus};

/// The VM trace's header line (without its LF).
pub const VM_TRACE_HEADER: &str =
    "step\tprogram\tpc\tword\timm\tnext_pc\tvsp\trsp\tfp\tgp\ttop\tstatus";
/// The combat trace's header line (without its LF).
pub const COMBAT_TRACE_HEADER: &str = "seq\tevent\tround\tturn\tactor\tcombat_hash\tstate_hash";
/// `--max-steps` default.
pub const DEFAULT_MAX_STEPS: u64 = 100_000;
/// `--max-rounds` default.
pub const DEFAULT_MAX_ROUNDS: u32 = 10;
/// The frame step of the headless stepping replays use.
pub const TRACE_FRAME_MS: u64 = 16;
/// Frames a combat trace may take before it is refused as stalled.
pub const TRACE_FRAME_LIMIT: u64 = 200_000;

/// A program's state after a traced instruction (SPEC-021 §3.5).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VmTraceStatus {
    Ok,
    Yield,
    Wait,
    Blocked,
    Stopped,
    Ended,
    Error,
}
impl VmTraceStatus {
    pub fn name(self) -> &'static str {
        match self {
            Self::Ok => "ok",
            Self::Yield => "yield",
            Self::Wait => "wait",
            Self::Blocked => "blocked",
            Self::Stopped => "stopped",
            Self::Ended => "ended",
            Self::Error => "error",
        }
    }
}
/// The registers and stacks after an instruction that raised no error.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct VmTraceAfter {
    pub next_pc: u32,
    pub vsp: u32,
    pub rsp: u32,
    pub fp: i32,
    pub gp: i32,
    /// The value-stack top as (tag, payload).
    pub top: Option<(u16, u32)>,
}
/// One executed instruction.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VmTraceRow {
    pub step: u64,
    pub program: String,
    pub pc: u32,
    pub word: Option<u16>,
    pub imm: Option<u32>,
    /// `None` on an error row.
    pub after: Option<VmTraceAfter>,
    pub status: VmTraceStatus,
}
impl fmt::Display for VmTraceRow {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}\t{}\t{:08X}\t", self.step, self.program, self.pc)?;
        match self.word {
            Some(word) => write!(f, "{word:04X}\t")?,
            None => f.write_str("-\t")?,
        }
        match self.imm {
            Some(imm) => write!(f, "{imm:08X}\t")?,
            None => f.write_str("-\t")?,
        }
        match self.after {
            Some(after) => {
                write!(
                    f,
                    "{:08X}\t{}\t{}\t{}\t{}\t",
                    after.next_pc, after.vsp, after.rsp, after.fp, after.gp
                )?;
                match after.top {
                    Some((tag, payload)) => write!(f, "{tag:04X}:{payload:08X}\t")?,
                    None => f.write_str("-\t")?,
                }
            }
            None => f.write_str("-\t-\t-\t-\t-\t-\t")?,
        }
        f.write_str(self.status.name())
    }
}
/// Why a VM trace ended.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VmTraceEnd {
    Ended,
    Stopped,
    Error,
    Blocked,
    Waiting,
    StepLimit,
}
impl VmTraceEnd {
    /// The `--dump-vm-trace` exit status: 0 for an ended or stopped
    /// program, 2 otherwise.
    pub fn exit_code(self) -> u8 {
        match self {
            Self::Ended | Self::Stopped => 0,
            _ => 2,
        }
    }
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VmTrace {
    pub rows: Vec<VmTraceRow>,
    pub end: VmTraceEnd,
}
impl VmTrace {
    /// The TSV text: header, then one line per row, each ending in LF.
    pub fn text(&self) -> String {
        tsv(VM_TRACE_HEADER, &self.rows)
    }
}
fn tsv<T: fmt::Display>(header: &str, rows: &[T]) -> String {
    let mut text = String::with_capacity(64 * (rows.len() + 1));
    text.push_str(header);
    text.push('\n');
    for row in rows {
        text.push_str(&row.to_string());
        text.push('\n');
    }
    text
}

/// The `program` column for a load path: its last component, lowercase,
/// without a trailing `.hex`.
pub fn trace_program_name(path: &str) -> String {
    let name = path.rsplit(['/', '\\']).next().unwrap_or(path);
    let name = name.to_ascii_lowercase();
    match name.strip_suffix(".hex") {
        Some(stem) => stem.to_string(),
        None => name,
    }
}
/// An offset-addressed hex dump (lines `OOOO: XX XX ...`, offsets
/// contiguous from 0, blank lines ignored) as bytes.
pub fn parse_hex_dump(text: &str) -> Result<Vec<u8>, WorldError> {
    let mut bytes = Vec::new();
    for (number, line) in text.lines().enumerate() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        let bad = |what: &str| WorldError::Invalid(format!("hex dump line {}: {what}", number + 1));
        let (offset, data) = line.split_once(':').ok_or_else(|| bad("no offset"))?;
        let offset = usize::from_str_radix(offset.trim(), 16).map_err(|_| bad("bad offset"))?;
        if offset != bytes.len() {
            return Err(bad("offset is not contiguous"));
        }
        for byte in data.split_whitespace() {
            if byte.len() != 2 {
                return Err(bad("bad byte"));
            }
            bytes.push(u8::from_str_radix(byte, 16).map_err(|_| bad("bad byte"))?);
        }
    }
    Ok(bytes)
}

fn word_at(code: &[u8], at: u32) -> Option<u16> {
    let at = usize::try_from(at).ok()?;
    let bytes = code.get(at..at.checked_add(2)?)?;
    Some(u16::from_be_bytes([bytes[0], bytes[1]]))
}
fn payload_at(code: &[u8], at: u32) -> Option<u32> {
    let at = usize::try_from(at).ok()?.checked_add(2)?;
    let bytes = code.get(at..at.checked_add(4)?)?;
    Some(u32::from_be_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]))
}
/// A constant push: bit 15 set and dispatch slot 1 (SPEC-004 §2.5).
fn constant_push(word: u16) -> bool {
    word & 0x8000 != 0 && word & 0x03FF == 1
}
/// The wait word (`0x8047`, with the dispatch-ignored bits 10-14 free).
fn installs_wait(word: u16) -> bool {
    word & 0x8000 != 0 && word & 0x03FF == 0x047
}
fn height(bytes: usize) -> u32 {
    u32::try_from(bytes).unwrap_or(u32::MAX)
}

/// SPEC-021 §3.5: run `vm` from its current state one instruction at a
/// time until it ends, stops, errs, can no longer be resumed, or
/// `max_steps` rows were written.
pub fn trace_vm(vm: &mut Vm, host: &mut impl VmHost, program: &str, max_steps: u64) -> VmTrace {
    let mut rows = Vec::new();
    let end = loop {
        if rows.len() as u64 >= max_steps {
            break VmTraceEnd::StepLimit;
        }
        let status = vm.status;
        if status.contains(VmStatus::ERROR) {
            break VmTraceEnd::Error;
        }
        if status.contains(VmStatus::ENDED) {
            break VmTraceEnd::Ended;
        }
        if status.contains(VmStatus::STOPPED) {
            break VmTraceEnd::Stopped;
        }
        if status.contains(VmStatus::ATTACHED | VmStatus::SPAWN_BLOCKED) {
            break VmTraceEnd::Blocked;
        }
        if status.contains(VmStatus::WAITING)
            && vm.wait_predicate != 1
            && !matches!(
                host.wait_ready(vm.wait_predicate, vm.wait_start, vm.wait_deadline),
                Ok(true)
            )
        {
            break VmTraceEnd::Waiting;
        }
        let pc = vm.pc;
        let word = word_at(vm.code(), pc);
        let imm = word
            .filter(|word| constant_push(*word))
            .and_then(|_| payload_at(vm.code(), pc));
        let result = vm.step(host);
        let (after, status) = match result {
            Err(_) => (None, VmTraceStatus::Error),
            Ok(outcome) => {
                let flags = vm.status;
                let status = if flags.contains(VmStatus::ERROR) {
                    VmTraceStatus::Error
                } else if flags.contains(VmStatus::ENDED) {
                    VmTraceStatus::Ended
                } else if flags.contains(VmStatus::STOPPED) {
                    VmTraceStatus::Stopped
                } else if flags.contains(VmStatus::ATTACHED | VmStatus::SPAWN_BLOCKED) {
                    VmTraceStatus::Blocked
                } else if flags.contains(VmStatus::WAITING) || word.is_some_and(installs_wait) {
                    VmTraceStatus::Wait
                } else if outcome == RunOutcome::Yielded {
                    VmTraceStatus::Yield
                } else {
                    VmTraceStatus::Ok
                };
                let after = (status != VmTraceStatus::Error).then(|| VmTraceAfter {
                    next_pc: vm.pc,
                    vsp: height(vm.value_stack.byte_len()),
                    rsp: height(vm.return_stack.byte_len()),
                    fp: vm.f,
                    gp: vm.g,
                    top: vm.value_stack.top().ok().map(|v| (v.tag, v.payload)),
                });
                (after, status)
            }
        };
        rows.push(VmTraceRow {
            step: rows.len() as u64,
            program: program.to_string(),
            pc,
            word,
            imm,
            after,
            status,
        });
        if status == VmTraceStatus::Error {
            break VmTraceEnd::Error;
        }
    };
    VmTrace { rows, end }
}

/// A combat trace event (SPEC-021 §3.6).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CombatEvent {
    Start,
    Turn,
    End,
}
impl CombatEvent {
    pub fn name(self) -> &'static str {
        match self {
            Self::Start => "start",
            Self::Turn => "turn",
            Self::End => "end",
        }
    }
}
/// One combat trace row.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CombatTraceRow {
    pub seq: u64,
    pub event: CombatEvent,
    pub round: Option<u32>,
    pub turn: Option<u32>,
    pub actor: Option<Handle>,
    pub combat_hash: u64,
    pub state_hash: u64,
}
impl CombatTraceRow {
    /// The row for `event` at the moment `snapshot` was taken. `actor` is
    /// written on a `turn` row only.
    pub fn from_snapshot(
        seq: u64,
        event: CombatEvent,
        actor: Option<Handle>,
        snapshot: &StateSnapshot,
    ) -> Self {
        let combat = snapshot.combat.as_ref();
        let turn_row = event == CombatEvent::Turn;
        Self {
            seq,
            event,
            round: (event != CombatEvent::End)
                .then(|| combat.map(|c| c.round))
                .flatten(),
            turn: turn_row.then(|| combat.and_then(|c| c.current)).flatten(),
            actor: actor.filter(|_| turn_row),
            combat_hash: snapshot.combat_hash(),
            state_hash: snapshot.state_hash(),
        }
    }
}
fn dash<T: fmt::Display>(value: Option<T>) -> String {
    value.map_or_else(|| "-".to_string(), |value| value.to_string())
}
impl fmt::Display for CombatTraceRow {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{}\t{}\t{}\t{}\t{}\t{:016X}\t{:016X}",
            self.seq,
            self.event.name(),
            dash(self.round),
            dash(self.turn),
            dash(self.actor),
            self.combat_hash,
            self.state_hash
        )
    }
}
impl std::str::FromStr for CombatTraceRow {
    type Err = String;
    fn from_str(line: &str) -> Result<Self, Self::Err> {
        let fields: Vec<&str> = line.split('\t').collect();
        let [seq, event, round, turn, actor, combat_hash, state_hash] = fields[..] else {
            return Err(format!("combat trace row {line:?} does not have 7 fields"));
        };
        fn optional<T: std::str::FromStr>(field: &str) -> Result<Option<T>, String> {
            if field == "-" {
                return Ok(None);
            }
            field
                .parse()
                .map(Some)
                .map_err(|_| format!("bad combat trace field {field:?}"))
        }
        let hash = |field: &str| {
            (field.len() == 16
                && field
                    .bytes()
                    .all(|b| matches!(b, b'0'..=b'9' | b'A'..=b'F')))
            .then(|| u64::from_str_radix(field, 16).ok())
            .flatten()
            .ok_or_else(|| format!("bad combat trace hash {field:?}"))
        };
        Ok(Self {
            seq: seq
                .parse()
                .map_err(|_| format!("bad combat trace seq {seq:?}"))?,
            event: match event {
                "start" => CombatEvent::Start,
                "turn" => CombatEvent::Turn,
                "end" => CombatEvent::End,
                _ => return Err(format!("bad combat trace event {event:?}")),
            },
            round: optional(round)?,
            turn: optional(turn)?,
            actor: optional(actor)?,
            combat_hash: hash(combat_hash)?,
            state_hash: hash(state_hash)?,
        })
    }
}
/// The combat trace's TSV text.
pub fn write_combat_trace(rows: &[CombatTraceRow]) -> String {
    tsv(COMBAT_TRACE_HEADER, rows)
}
/// Read a combat trace written by [`write_combat_trace`].
pub fn read_combat_trace(text: &str) -> Result<Vec<CombatTraceRow>, String> {
    let body = text
        .strip_suffix('\n')
        .ok_or("a combat trace ends with LF")?;
    let mut lines = body.split('\n');
    if lines.next() != Some(COMBAT_TRACE_HEADER) {
        return Err("the combat trace header differs".into());
    }
    lines.map(str::parse).collect()
}

/// An actor's action points at the moment its `turn` row was written: what
/// SPEC-021 revision 2 test T11 reads to see that the NPC acted. Not part
/// of the trace text.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TurnAp {
    /// The `seq` of the `turn` row.
    pub seq: u64,
    pub actor: Handle,
    /// Current AP at the end of the turn.
    pub ap: i32,
    /// Effective maximum AP (the refill at the turn's start).
    pub maximum: i32,
}
/// The rows a host writes while combat events happen (SPEC-021 §3.6 row
/// moments). Rows are numbered in recording order.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct CombatTraceRecorder {
    pub rows: Vec<CombatTraceRow>,
    /// One entry per `turn` row whose actor is still an entity.
    pub turn_ap: Vec<TurnAp>,
}
impl CombatTraceRecorder {
    pub fn record(&mut self, world: &WorldState, event: CombatEvent, actor: Option<ObjectRef>) {
        let seq = self.rows.len() as u64;
        let handle = actor.map(|actor| world.state_handle(actor));
        let row = CombatTraceRow::from_snapshot(seq, event, handle, &world.state_snapshot());
        self.rows.push(row);
        if let (CombatEvent::Turn, Some(actor), Some(handle)) = (event, actor, handle)
            && let Some(state) = world.critter_state(actor)
        {
            self.turn_ap.push(TurnAp {
                seq,
                actor: handle,
                ap: state.ap,
                maximum: world.maximum_ap(actor),
            });
        }
    }
}
impl<A: WorldAssets> WorldHost<A> {
    /// Record a combat event when a combat trace is attached.
    pub(crate) fn record_combat_event(&mut self, event: CombatEvent, actor: Option<ObjectRef>) {
        if let Some(recorder) = &mut self.combat_trace {
            recorder.record(&self.world, event, actor);
        }
    }
}

/// SPEC-021 §3.7 step 1: a new Fallout 1 game on `map` with the default
/// player sheet, the default language and `seed`, set up as the desktop's
/// map session does (with the replays' immediate movies), its map entered.
pub fn combat_trace_host(
    vfs: Arc<dyn Vfs>,
    map: &str,
    seed: i32,
) -> Result<WorldHost<VfsWorldAssets>, WorldError> {
    let assets = VfsWorldAssets::new(vfs, "english")?;
    assets.set_game_profile(GameProfile::Fallout1);
    let mut host = WorldHost::new(WorldState::new(CharacterSheet::default(), seed)?, assets);
    host.set_preset(EnginePreset::Vanilla);
    host.update_player_gender();
    host.interactive_dialogue = true;
    host.movie_policy = crate::movie::MoviePolicy::CompleteImmediately;
    host.playback = crate::host_choreography::Playback::RealTime;
    // As the desktop does, a corpus without campaign globals still runs;
    // the missing part is reported.
    if let Err(error) = host.load_campaign() {
        host.world
            .diagnostic(format!("campaign initializer unavailable: {error}"));
    }
    host.load_language_filter();
    // SPEC-021 revision 2 §3.7 step 1: the combat AI must initialize with a
    // nonempty packet table, so a golden is never recorded with idle NPCs.
    host.load_combat_ai()
        .map_err(|error| WorldError::Invalid(format!("combat AI unavailable: {error}")))?;
    require_packets(&host.world)?;
    host.load_map(map)?;
    Ok(host)
}
/// SPEC-021 revision 2 §3.7 step 1: a loaded, nonempty packet table.
fn require_packets(world: &WorldState) -> Result<(), WorldError> {
    match world.ai.packets.as_deref() {
        Some(packets) if !packets.packets.is_empty() => Ok(()),
        Some(_) => Err(WorldError::Invalid(
            "combat AI unavailable: data/ai.txt has no packet".into(),
        )),
        None => Err(WorldError::Invalid(
            "combat AI unavailable: no packet table is loaded".into(),
        )),
    }
}
/// SPEC-021 §3.7 step 2: the live non-player critter on the player's
/// elevation, not dead, with the smallest handle.
pub fn trace_defender(world: &WorldState) -> Option<ObjectRef> {
    let elevation = world.get(world.player)?.raw.elevation;
    world
        .iter()
        .filter(|(reference, object)| {
            *reference != world.player
                && object.raw.pid >> 24 == 1
                && object.raw.elevation == elevation
                && !world.is_dead(*reference)
        })
        .min_by_key(|(reference, _)| world.state_handle(*reference))
        .map(|(reference, _)| reference)
}
/// Where a combat trace stops, if it does.
#[derive(Clone, Copy)]
enum TraceCut {
    /// Through this row count, the last one an `end` row.
    Ended(usize),
    /// `max_rounds` complete rounds are traced (SPEC-021 §3.7 step 6): the
    /// rows of those rounds, this many.
    RoundLimit(usize),
}
/// `round` is the combat state's counter now: it reaches `max_rounds` in
/// the step that writes the last turn's row of the last complete round.
fn trace_cut(rows: &[CombatTraceRow], round: u32, max_rounds: u32) -> Option<TraceCut> {
    rows.iter()
        .enumerate()
        .find_map(|(index, row)| {
            if row.event == CombatEvent::End {
                Some(TraceCut::Ended(index + 1))
            } else if row.round.is_some_and(|round| round >= max_rounds) {
                Some(TraceCut::RoundLimit(index))
            } else {
                None
            }
        })
        .or((round >= max_rounds).then_some(TraceCut::RoundLimit(rows.len())))
}
/// What one step of the [`CombatDriver`] did.
#[derive(Debug)]
pub enum DriverStep {
    /// The step finished: it writes a checkpoint row, and the run goes on.
    Running,
    /// Combat ended in this step, which finished: it writes a row, and the
    /// run stops with the kept rows.
    Ended(CombatTraceRecorder),
    /// The round limit stopped the run in this step, which writes no row
    /// (SPEC-021 §3.10 rule 5).
    RoundLimit(CombatTraceRecorder),
}
/// SPEC-021 §3.7 steps 2 to 6, one headless step at a time. A step takes the
/// driver's commands since the last step (§3.10): the combat request first,
/// then each end of a player turn. Then the world advances one frame.
#[derive(Debug)]
pub struct CombatDriver {
    defender: ObjectRef,
    max_rounds: u32,
    now: u64,
    frames: u64,
    commands: u64,
}
impl CombatDriver {
    /// A driver for an entered map. It refuses to run without a loaded,
    /// nonempty packet table (§3.7 step 1) or a defender.
    pub fn new<A: WorldAssets>(
        host: &mut WorldHost<A>,
        max_rounds: u32,
    ) -> Result<Self, WorldError> {
        require_packets(&host.world)?;
        let defender = trace_defender(&host.world)
            .ok_or_else(|| WorldError::Invalid("no defender for the combat trace".into()))?;
        host.combat_trace = Some(CombatTraceRecorder::default());
        Ok(Self {
            defender,
            max_rounds,
            now: host.world.elapsed_ms,
            frames: 0,
            commands: 0,
        })
    }
    /// The driver's commands so far: a checkpoint row's `input_index`.
    pub fn commands(&self) -> u64 {
        self.commands
    }
    /// One headless step, up to the moment a row is written.
    pub fn step<A: WorldAssets>(
        &mut self,
        host: &mut WorldHost<A>,
    ) -> Result<DriverStep, WorldError> {
        if self.frames == TRACE_FRAME_LIMIT {
            host.combat_trace = None;
            return Err(WorldError::Invalid(format!(
                "the combat trace did not finish within {TRACE_FRAME_LIMIT} frames"
            )));
        }
        if self.commands == 0 {
            host.world.pending_combat = Some(CombatRequest {
                attacker: host.world.player,
                defender: self.defender,
                accuracy: 0,
                damage: [0, i32::MAX],
                flags: None,
            });
            self.commands = 1;
        } else if host.world.combat
            && host.world.combat_state.player_turn
            && !host.player_turn_over()
        {
            host.end_player_turn();
            self.commands += 1;
        }
        self.frames += 1;
        self.now += TRACE_FRAME_MS;
        if let Err(error) = host.tick(self.now) {
            host.combat_trace = None;
            return Err(error);
        }
        let rows = host.combat_trace.as_ref().map_or(&[][..], |r| &r.rows[..]);
        let round = host.world.combat_state.round;
        let Some(cut) = trace_cut(rows, round, self.max_rounds) else {
            return Ok(DriverStep::Running);
        };
        let (TraceCut::Ended(kept) | TraceCut::RoundLimit(kept)) = cut;
        let mut recorder = host.combat_trace.take().unwrap_or_default();
        recorder.rows.truncate(kept);
        recorder.turn_ap.retain(|turn| turn.seq < kept as u64);
        Ok(match cut {
            TraceCut::Ended(_) => DriverStep::Ended(recorder),
            TraceCut::RoundLimit(_) => DriverStep::RoundLimit(recorder),
        })
    }
}
/// SPEC-021 §3.7 steps 2 to 6 on an entered map: request combat between
/// the player and the defender, end every player turn at once, step the
/// world frame by frame, and return the rows.
pub fn run_combat_trace<A: WorldAssets>(
    host: &mut WorldHost<A>,
    max_rounds: u32,
) -> Result<Vec<CombatTraceRow>, WorldError> {
    run_combat_trace_recorded(host, max_rounds).map(|recorder| recorder.rows)
}
/// [`run_combat_trace`], also returning each kept `turn` row's actor AP
/// (SPEC-021 revision 2 test T11). It refuses to run without a loaded,
/// nonempty packet table (§3.7 step 1).
pub fn run_combat_trace_recorded<A: WorldAssets>(
    host: &mut WorldHost<A>,
    max_rounds: u32,
) -> Result<CombatTraceRecorder, WorldError> {
    let mut driver = CombatDriver::new(host, max_rounds)?;
    loop {
        match driver.step(host)? {
            DriverStep::Running => {}
            DriverStep::Ended(recorder) | DriverStep::RoundLimit(recorder) => return Ok(recorder),
        }
    }
}
/// How a checkpointed combat trace ended.
#[derive(Debug)]
pub enum CheckpointedTrace {
    /// The run finished; the stream matched, if one was expected.
    Finished(CombatTraceRecorder),
    /// The run stopped at the step of this comparison failure.
    Diverged(CheckpointFailure),
}
/// One row of `stream` for `world`, appended to `text` even when it is the
/// row a comparison stops at.
fn write_row(
    stream: &mut CheckpointStream,
    text: &mut String,
    world: &WorldState,
    commands: u64,
) -> Result<(), CheckpointFailure> {
    match stream.row(world, commands) {
        Ok(line) => {
            text.push_str(&line);
            Ok(())
        }
        Err((line, failure)) => {
            text.push_str(&line);
            Err(failure)
        }
    }
}
/// SPEC-021 §3.10 the combat trace with a checkpoint row per step, appended
/// to `text`: the stream's head and row 0 before the first step, then one
/// row after each step's simulation. A comparison failure stops the run at
/// that step.
pub fn run_combat_checkpoints<A: WorldAssets>(
    host: &mut WorldHost<A>,
    max_rounds: u32,
    stream: &mut CheckpointStream,
    text: &mut String,
) -> Result<CheckpointedTrace, WorldError> {
    // Refused before any row, as the trace itself is.
    let mut driver = CombatDriver::new(host, max_rounds)?;
    let diverged = |host: &mut WorldHost<A>, failure| {
        host.combat_trace = None;
        Ok(CheckpointedTrace::Diverged(failure))
    };
    match stream.head() {
        Ok(head) => text.push_str(&head),
        Err(failure) => return diverged(host, failure),
    }
    if let Err(failure) = write_row(stream, text, &host.world, driver.commands()) {
        return diverged(host, failure);
    }
    let recorder = loop {
        let step = driver.step(host)?;
        let ended = match step {
            DriverStep::RoundLimit(recorder) => break recorder,
            DriverStep::Running => None,
            DriverStep::Ended(recorder) => Some(recorder),
        };
        if let Err(failure) = write_row(stream, text, &host.world, driver.commands()) {
            return diverged(host, failure);
        }
        if let Some(recorder) = ended {
            break recorder;
        }
    };
    Ok(match stream.finish() {
        Ok(_) => CheckpointedTrace::Finished(recorder),
        Err(failure) => CheckpointedTrace::Diverged(failure),
    })
}
