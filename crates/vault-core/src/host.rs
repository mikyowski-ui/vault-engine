use crate::{
    ObjectAction, ObjectRef, Perk, ScriptEvent, WorldEffect, WorldError, WorldRules, WorldState,
    assets::WorldAssets,
    dialogue::{DialogueText, HeadDefinition, Mood, TalkingHead, npc_reaction},
    library::LibraryState,
    queue::EventKind,
    world::{ScriptRequest, WorldObject, WorldTrace, empty_map, object_record},
};
use std::collections::{BTreeMap, BTreeSet, VecDeque};
use vault_formats::{
    GamSection,
    map::ObjectState,
    pro::{BasicFlags, Category, ItemData, Light, LockFlags, PrototypeData},
};
use vault_vm::{
    BasicHost, ExportedProcedure, ExternalValue, LaunchMode, RunOutcome, Value, Vm, VmError,
    VmHost, VmStatus,
    extensions::{self, PushKind},
};

pub(crate) type HostResult = Result<Option<ExternalValue>, VmError>;

/// An observer of script execution: a debugger's live instruction trace.
/// While one is attached to a host, its scripts run one instruction per
/// `Vm::run` call (the same scheduling as larger budgets) and every step is
/// reported before and after it executes. Nested script runs are reported
/// too; the observer is never held across a step.
pub trait VmObserver: Send {
    fn before_step(&mut self, vm: &Vm);
    fn after_step(&mut self, vm: &Vm, result: &Result<RunOutcome, VmError>);
}
#[derive(Debug, Clone)]
pub struct CallTrace {
    pub opcode: u16,
    pub operands: Vec<Value>,
}
#[derive(Debug, Clone, Default)]
pub struct AnimationSequence {
    /// Request flags: 1 unreserved, 2 reserved, 4 suppress standing, 512
    /// insignificant.
    pub flags: i32,
    pub commands: Vec<(u16, Vec<i32>)>,
    /// Sound-cue names by command index.
    pub sounds: BTreeMap<usize, Vec<u8>>,
}
#[derive(Debug, Clone, Copy, Default)]
pub struct DispatchResult {
    pub overridden: bool,
    pub result: i32,
    pub pending: bool,
}
struct PresentationState {
    font: i32,
    mouse_hidden: bool,
    scrolling: bool,
    palette_cycling: bool,
    music_paused: bool,
}

pub struct WorldHost<A: WorldAssets = Box<dyn WorldAssets>> {
    pub world: WorldState,
    pub assets: A,
    pub library: LibraryState,
    pub registry: BasicHost,
    pub programs: BTreeMap<u32, Vm>,
    pub calls: Vec<CallTrace>,
    pub interactive_dialogue: bool,
    /// Whether `playmovie` waits for a presentation layer (headless runs
    /// complete it immediately by default).
    pub movie_policy: crate::movie::MoviePolicy,
    pub(crate) callbacks: VecDeque<(u32, u32)>,
    program_scripts: BTreeMap<u32, u32>,
    active: BTreeSet<u32>,
    next_program: u32,
    /// NPC combat policy: the built-in SPEC-009 packet AI by default.
    pub combat_ai: Box<dyn crate::combat_state::CombatAi>,
    pub building_animation: Option<AnimationSequence>,
    /// SPEC-007 §3.10-3.11 how attack shows and the world's own sequences
    /// play: at once (the default for headless runs) or over time, with
    /// combat waiting for them.
    pub playback: crate::host_choreography::Playback,
    pub floating_style: i32,
    dialogue_waiters: Vec<u32>,
    dialogue_callbacks: Vec<u32>,
    /// SPEC-007 §3.11 the world's own sequences: walking, interactions,
    /// doors and containers.
    pub motions: Vec<crate::motion::MotionSequence>,
    /// The sequences being played (a callback can register another).
    pub(crate) running_motions: Vec<crate::host_motion::Running>,
    pub(crate) next_motion: u64,
    /// The scheduler time of the world's sequences.
    pub(crate) motion_time: u64,
    /// SPEC-007 §3.11.2 the idle-fidget background task, while it runs.
    pub(crate) fidget: Option<crate::host_fidget::Fidget>,
    presentation: Option<PresentationState>,
    /// An interaction's approach has just ended: its action runs as the
    /// sequence's proximity step (SPEC-013 §3.2), not a new approach.
    pub(crate) arriving: bool,
    /// A debugger's live trace of script execution, when attached.
    pub vm_observer: Option<std::sync::Arc<std::sync::Mutex<dyn VmObserver>>>,
    /// Presentation-only draws (a dead critter's look-at line), apart from
    /// the world's generator so looking never moves its sequence.
    pub presentation_rng: crate::ParkMiller,
}
impl<T: WorldAssets + ?Sized> WorldAssets for Box<T> {
    fn prototype(&self, pid: u32) -> Result<std::sync::Arc<vault_formats::Prototype>, WorldError> {
        (**self).prototype(pid)
    }
    fn program_by_name(&self, name: &[u8]) -> Result<(String, vault_vm::IntProgram), WorldError> {
        (**self).program_by_name(name)
    }
    fn program(&self, index: i32) -> Result<(String, vault_vm::IntProgram), WorldError> {
        (**self).program(index)
    }
    fn local_count(&self, index: i32) -> usize {
        (**self).local_count(index)
    }
    fn message(&self, list: i32, id: i32) -> Option<vault_formats::msg::Message> {
        (**self).message(list, id)
    }
    fn prototype_text(&self, p: &vault_formats::Prototype, d: bool) -> Option<Vec<u8>> {
        (**self).prototype_text(p, d)
    }
    fn head(&self, index: i32) -> Result<crate::dialogue::HeadDefinition, WorldError> {
        (**self).head(index)
    }
    fn frame(&self, path: &str) -> Result<std::sync::Arc<vault_formats::FrmArchive>, WorldError> {
        (**self).frame(path)
    }
    fn background_path(&self, index: i32) -> Result<String, WorldError> {
        (**self).background_path(index)
    }
    fn interface_path(&self, index: i32) -> Result<String, WorldError> {
        (**self).interface_path(index)
    }
    fn font(&self) -> Result<std::sync::Arc<vault_formats::Font>, WorldError> {
        (**self).font()
    }
    fn gam(
        &self,
        path: &str,
        section: GamSection,
    ) -> Result<Option<vault_formats::GamFile>, WorldError> {
        (**self).gam(path, section)
    }
    fn map(&self, path: &str) -> Result<vault_formats::MapFile, WorldError> {
        (**self).map(path)
    }
    fn map_index(&self, name: &[u8]) -> Option<i32> {
        (**self).map_index(name)
    }
    fn map_path(&self, number: i32) -> Option<String> {
        (**self).map_path(number)
    }
    fn available(&self, path: &[u8]) -> bool {
        (**self).available(path)
    }
    fn language(&self) -> &str {
        (**self).language()
    }
    fn files(&self, pattern: &[u8]) -> Vec<Vec<u8>> {
        (**self).files(pattern)
    }
    fn dialogue_label(&self, done: bool) -> Vec<u8> {
        (**self).dialogue_label(done)
    }
    fn release_dialogue_art(&self) {
        (**self).release_dialogue_art();
    }
    fn art_token(&self, category: u8, index: u16) -> Option<Vec<u8>> {
        (**self).art_token(category, index)
    }
    fn art_row(&self, category: u8, name: &[u8]) -> Option<u16> {
        (**self).art_row(category, name)
    }
    fn object_art(
        &self,
        fid: u32,
    ) -> Result<std::sync::Arc<vault_formats::FrmArchive>, WorldError> {
        (**self).object_art(fid)
    }
    fn construct_fid(
        &self,
        category: u8,
        base: u16,
        animation: u8,
        weapon: u8,
        selector: u8,
    ) -> u32 {
        (**self).construct_fid(category, base, animation, weapon, selector)
    }
    fn font_number(&self, number: i32) -> Option<std::sync::Arc<vault_formats::Font>> {
        (**self).font_number(number)
    }
    fn palette(&self, name: &[u8]) -> Result<vault_formats::Palette, WorldError> {
        (**self).palette(name)
    }
    fn art_exists(&self, fid: u32) -> bool {
        (**self).art_exists(fid)
    }
    fn resource(&self, path: &str) -> Result<Vec<u8>, WorldError> {
        (**self).resource(path)
    }
    fn game_messages(
        &self,
        file: &str,
    ) -> Result<std::sync::Arc<vault_formats::MsgCatalog>, WorldError> {
        (**self).game_messages(file)
    }
}
impl<A: WorldAssets> WorldHost<A> {
    pub fn new(world: WorldState, assets: A) -> Self {
        Self {
            world,
            assets,
            library: LibraryState::default(),
            registry: BasicHost::default(),
            programs: BTreeMap::new(),
            calls: Vec::new(),
            interactive_dialogue: false,
            movie_policy: crate::movie::MoviePolicy::default(),
            callbacks: VecDeque::new(),
            program_scripts: BTreeMap::new(),
            active: BTreeSet::new(),
            next_program: 1,
            combat_ai: Box::new(crate::combat_state::PacketAi),
            building_animation: None,
            playback: Default::default(),
            floating_style: 1,
            dialogue_waiters: Vec::new(),
            dialogue_callbacks: Vec::new(),
            motions: Vec::new(),
            running_motions: Vec::new(),
            next_motion: 0,
            motion_time: 0,
            fidget: None,
            presentation: None,
            arriving: false,
            vm_observer: None,
            presentation_rng: crate::ParkMiller::seeded(1).expect("a positive seed"),
        }
    }
    pub fn rules(&mut self, rules: WorldRules) {
        self.world.rules = rules;
    }
    pub fn bind_program(&mut self, vm: &Vm, script: u32) {
        self.program_scripts.insert(vm.id, script);
        if let Some(instance) = self.world.scripts.get_mut(&script) {
            instance.program = vm.id;
            instance.initialized = true;
        }
    }
    pub(crate) fn sid(&self, vm: &Vm) -> Option<u32> {
        self.program_scripts.get(&vm.id).copied()
    }
    pub(crate) fn self_object(&self, vm: &Vm) -> ObjectRef {
        self.sid(vm)
            .and_then(|sid| self.world.scripts.get(&sid))
            .map_or(0, |s| s.owner)
    }
    pub(crate) fn target(&self, vm: &Vm) -> ObjectRef {
        self.sid(vm)
            .and_then(|sid| self.world.scripts.get(&sid))
            .map_or(0, |s| s.target)
    }
    /// A campaign initializer that opens replaces the array (a missing marker
    /// yields an empty array); an open failure reports failure and leaves any
    /// prior array intact.
    pub fn load_campaign(&mut self) -> Result<(), WorldError> {
        self.world.trace.push(WorldTrace::InventoryRefresh);
        let gam = self
            .assets
            .gam("data/vault13.gam", GamSection::Campaign)?
            .ok_or_else(|| {
                WorldError::Invalid("campaign initializer data/vault13.gam did not open".into())
            })?;
        self.world.gvars = Some(gam.values);
        Ok(())
    }
    /// SPEC-014 §3.35 leaving a map, before another loads or the town or
    /// world map opens: every animation stops; the map-exit handlers and
    /// `map_exit_p_proc` run; the map is marked saved with the current
    /// game time as its last visit; then it joins the working map set (an
    /// excluded map's working copy is deleted instead). Returns whether the
    /// map's save, when one was due, succeeded.
    pub(crate) fn leave_map(&mut self) -> Result<bool, WorldError> {
        self.stop_motions();
        self.building_animation = None;
        // SPEC-013 §2.5 map-exit handlers for explosives and sneak.
        self.skill_map_exit()?;
        let leaving = self.world.map_exit_cleanup(|_| true);
        self.leave_map_events(leaving)?;
        if let Some(script) = self.world.map_script {
            // Like the entry requests, a failed exit request (for example a
            // missing compiled script) is reported; leaving still proceeds.
            let result = self.dispatch(ScriptRequest {
                script,
                event: ScriptEvent::MapExit,
                source: 0,
                target: 0,
                fixed: 0,
                action: -1,
                only_present: false,
            });
            self.report_load_dispatch(result);
        }
        // SPEC-014 §3.35 step 3: the game-time (midnight) event is queued
        // again, and the roof over the player's position is restored.
        self.world.queue.insert(
            self.world.clock.ticks,
            self.world.clock.midnight_delay(),
            0,
            crate::queue::EventKind::Midnight,
        )?;
        self.world.restore_roofs();
        self.world.map.header.flags.0 |= 1;
        self.world.map.header.last_visit = self.world.clock.ticks as u32;
        Ok(self.world.save_left_map())
    }
    /// The left map's programs and presentation stop before a load.
    fn teardown_map(&mut self) -> Result<(), WorldError> {
        self.motions.clear();
        self.world.background_enabled = false;
        self.world.scrolling_enabled = false;
        self.world.rendering_enabled = false;
        self.world.scripts_enabled = false;
        self.world.map_script = None;
        for program in self.programs.keys().copied().collect::<Vec<_>>() {
            self.registry.purge(program)?;
            self.library.cleanup(program);
        }
        self.programs.clear();
        self.program_scripts.clear();
        self.world.loading = true;
        if let Some(player) = self.world.get_mut(self.world.player) {
            player.raw.light = Light {
                radius: 0,
                intensity: 0,
            };
        }
        Ok(())
    }
    /// SPEC-013 §3.22 leaving for the town map or the world map: the map is
    /// left and saved (SPEC-014 §3.35), then cleared, so no map is loaded
    /// until a location is entered. Returns whether the save succeeded.
    pub(crate) fn unload_map(&mut self) -> Result<bool, WorldError> {
        let saved = self.leave_map()?;
        self.teardown_map()?;
        self.world.import_map(empty_map(), &self.assets)?;
        self.world.persistence.current_map = None;
        self.world.loading = false;
        self.world.unloaded = true;
        Ok(saved)
    }
    pub fn load_map(&mut self, path: &str) -> Result<(), WorldError> {
        self.world.trace.push(WorldTrace::LoadStep(1));
        self.leave_map()?;
        self.teardown_map()?;
        self.world.trace.push(WorldTrace::LoadStep(2));
        let (map, saved, load_error) = match self.read_map(path) {
            Ok((map, saved)) => (map, saved, None),
            Err(error) => (empty_map(), false, Some(error)),
        };
        self.world.persistence.current_map = load_error
            .is_none()
            .then(|| {
                let file = path.rsplit('/').next().unwrap_or(path);
                crate::persistence::saved_map_name(file.as_bytes())
            })
            .flatten();
        self.world.restoring = map.header.flags.0 & 1 != 0;
        for message in &map.diagnostics {
            self.world.diagnostic(message.clone());
        }
        self.world.trace.push(WorldTrace::LoadStep(3));
        self.world.import_map(map, &self.assets)?;
        self.world.unloaded = false;
        // SPEC-014 §3.35: a map loaded fresh clears the player's last
        // attacker.
        if !saved && let Some(player) = self.world.get_mut(self.world.player) {
            player.opponent = 0;
        }
        self.world.trace.push(WorldTrace::LoadStep(4));
        self.world.elevation = self.world.map.header.entry_elevation;
        // SPEC-014 §3.29: an entry hex outside the map border fails the load.
        let entry = self.world.map.header.entry_hex;
        if !self.world.set_view_center(entry) {
            return Err(WorldError::Invalid(format!(
                "map entry hex {entry} is outside the map border"
            )));
        }
        self.world.trace.push(WorldTrace::LoadStep(5));
        self.world.set_ambient(65536);
        let (hex, elevation, orientation) = (
            self.world.camera_hex,
            self.world.elevation,
            self.world.map.header.entry_orientation,
        );
        if let Some(player) = self.world.get_mut(self.world.player) {
            player.raw.hex = Some(hex);
            player.raw.elevation = elevation;
            player.raw.orientation = orientation;
        }
        if !self.world.restoring {
            self.world.trace.push(WorldTrace::InventoryRefresh);
            let name = &self.world.map.header.name;
            let name = &name[..name.iter().position(|b| *b == 0).unwrap_or(name.len())];
            if let Ok(name) = std::str::from_utf8(name) {
                let lower = name.to_ascii_lowercase();
                let stem = lower.strip_suffix(".map").unwrap_or(&lower);
                match self
                    .assets
                    .gam(&format!("maps/{stem}.gam"), GamSection::Map)
                {
                    Ok(Some(gam)) => self.world.mvars = gam.values,
                    Ok(None) => {}
                    Err(error) => self.world.diagnostic(error.to_string()),
                }
            }
        }
        self.world.trace.push(WorldTrace::LoadStep(6));
        self.world.scripts_enabled = true;
        let fixed = i32::from(!self.world.restoring);
        if let Some(file) = self.world.map.header.map_script_index() {
            let mut raw = object_record(0x05000001, 0);
            raw.flags = BasicFlags(BasicFlags::HIDDEN.0 | BasicFlags::TEMPORARY.0);
            let owner = self.world.insert(WorldObject {
                raw,
                prototype: None,
                character: None,
                name: Vec::new(),
                description: Vec::new(),
                owner: None,
                inventory: Vec::new(),
                money: None,
                active: true,
                opponent: 0,
            })?;
            let sid = self.world.add_script(
                owner,
                file as i32,
                0,
                self.assets.local_count(file as i32),
            )?;
            self.world.map_script = Some(sid);
            self.world.spatial_enabled = false;
            let result = self.dispatch(ScriptRequest {
                script: sid,
                event: ScriptEvent::MapEnter,
                source: 0,
                target: 0,
                fixed,
                action: -1,
                only_present: false,
            });
            self.world.spatial_enabled = true;
            self.report_load_dispatch(result);
        }
        self.world.trace.push(WorldTrace::LoadStep(7));
        if let Some(player) = self.world.get_mut(self.world.player) {
            player.raw.light = Light {
                radius: 4,
                intensity: 65536,
            };
        }
        self.world.rebuild_lighting();
        self.world.background_enabled = true;
        self.world.trace.push(WorldTrace::LoadStep(8));
        for script in self.world.script_order.clone() {
            let result = self.dispatch(ScriptRequest {
                script,
                event: ScriptEvent::Start,
                source: 0,
                target: 0,
                fixed: 0,
                action: -1,
                only_present: false,
            });
            self.report_load_dispatch(result);
        }
        self.world.spatial_enabled = false;
        for script in self.world.script_order.clone() {
            if Some(script) != self.world.map_script {
                let result = self.dispatch(ScriptRequest {
                    script,
                    event: ScriptEvent::MapEnter,
                    source: 0,
                    target: 0,
                    fixed,
                    action: -1,
                    only_present: true,
                });
                self.report_load_dispatch(result);
            }
        }
        self.world.spatial_enabled = true;
        self.world.trace.push(WorldTrace::LoadStep(9));
        self.map_update()?;
        self.world.trace.push(WorldTrace::LoadStep(10));
        self.world.rendering_enabled = true;
        self.world.scrolling_enabled = true;
        self.world.enqueue_map_timers()?;
        self.world.loading = false;
        self.world.restoring = false;
        // SPEC-014 §3.35: once a saved copy has loaded, with its map
        // scripts, the revisit rules age it; then the city music starts.
        if saved && load_error.is_none() {
            self.revisit()?;
        }
        if let Some(track) = crate::worldmap::city_music(self.world.map.header.map_number) {
            self.world.emit(WorldEffect::Music(track.to_string()));
        }
        self.drain_requests()?;
        if let Some(error) = load_error {
            self.world.diagnostic(error.to_string());
            return Err(error);
        }
        Ok(())
    }
    pub(crate) fn clear_program_bindings(&mut self) {
        self.program_scripts.clear();
        self.active.clear();
        self.motions.clear();
    }
    /// A working saved-map `.SAV` takes precedence over the asset lookup;
    /// the flag tells which was read.
    fn read_map(&self, path: &str) -> Result<(vault_formats::MapFile, bool), WorldError> {
        let saved = path
            .rsplit('/')
            .next()
            .and_then(|file| crate::persistence::saved_map_name(file.as_bytes()));
        if let Some(bytes) = saved.and_then(|name| self.world.persistence.maps.get(&name)) {
            let lookup = |pid| {
                self.assets
                    .prototype(pid)
                    .map(|prototype| prototype.info())
                    .map_err(|error| vault_formats::ParseError {
                        format: "PRO",
                        offset: 0,
                        message: error.to_string(),
                    })
            };
            return Ok((vault_formats::MapFile::parse(bytes, &lookup)?, true));
        }
        Ok((self.assets.map(path)?, false))
    }
    /// Whether a loaded script's program defines the event's procedure.
    pub(crate) fn has_procedure(&self, script: u32, event: ScriptEvent) -> bool {
        self.world
            .scripts
            .get(&script)
            .filter(|instance| instance.initialized)
            .and_then(|instance| self.programs.get(&instance.program))
            .is_some_and(|vm| vm.program.find_procedure(event.name(), true).is_some())
    }
    pub fn dispatch(&mut self, request: ScriptRequest) -> Result<DispatchResult, WorldError> {
        if !self.world.scripts_enabled {
            return Ok(DispatchResult::default());
        }
        let Some(instance) = self.world.scripts.get(&request.script).cloned() else {
            return Ok(DispatchResult::default());
        };
        if self.active.contains(&instance.program) && instance.program != 0 {
            self.world.request(request);
            return Ok(DispatchResult {
                pending: true,
                ..Default::default()
            });
        }
        let mut vm = if !instance.initialized {
            let (filename, program) = self.assets.program(instance.file)?;
            let mut vm = Vm::new(program);
            vm.id = self.next_program;
            self.next_program = self
                .next_program
                .checked_add(1)
                .ok_or(WorldError::Arithmetic)?;
            self.registry.register_program(vm.id, filename.as_bytes());
            self.program_scripts.insert(vm.id, instance.id);
            let script = self.world.scripts.get_mut(&instance.id).expect("script");
            script.program = vm.id;
            script.initialized = true;
            vm
        } else {
            self.programs
                .remove(&instance.program)
                .ok_or_else(|| WorldError::Invalid("script program unavailable".into()))?
        };
        let procedure = vm.program.find_procedure(request.event.name(), true);
        if instance.initialized && request.only_present && procedure.is_none() {
            self.programs.insert(vm.id, vm);
            return Ok(DispatchResult::default());
        }
        let target = if request.target == 0 {
            instance.owner
        } else {
            request.target
        };
        let event = request
            .event
            .documented_code()
            .or_else(|| self.world.rules.event_codes.get(&request.event).copied())
            .unwrap_or(0);
        let script = self.world.scripts.get_mut(&instance.id).expect("script");
        script.overridden = false;
        script.event = event;
        script.source = request.source;
        script.target = target;
        script.fixed = request.fixed;
        script.action = request.action;
        self.world.trace.push(WorldTrace::Script {
            id: instance.id,
            event: request.event,
            source: request.source,
            target,
            fixed: request.fixed,
            player_light: self
                .world
                .get(self.world.player)
                .map_or(0, |p| p.raw.light.intensity),
        });
        let preparation = if instance.initialized {
            vm.prepare_host_call(procedure.filter(|index| *index != 0).unwrap_or(1), true)
        } else {
            Ok(())
        };
        let run = preparation.and_then(|_| self.run_vm(&mut vm));
        let pending = matches!(run, Ok(RunOutcome::Blocked | RunOutcome::Waiting));
        if let Err(error) = run {
            self.world
                .diagnostic(format!("script {}: {error}", instance.file));
        }
        self.programs.insert(vm.id, vm);
        let result =
            self.world
                .scripts
                .get_mut(&instance.id)
                .map_or(DispatchResult::default(), |script| {
                    script.source = 0;
                    DispatchResult {
                        overridden: script.overridden,
                        result: script.result,
                        pending,
                    }
                });
        Ok(result)
    }
    fn run_vm(&mut self, vm: &mut Vm) -> Result<RunOutcome, VmError> {
        self.active.insert(vm.id);
        let mut outcome = Err(VmError::InstructionLimit);
        match self.vm_observer.clone() {
            // The same total budget, one observed instruction at a time.
            Some(observer) => {
                let lock = || {
                    observer
                        .lock()
                        .unwrap_or_else(std::sync::PoisonError::into_inner)
                };
                for _ in 0..1_000_000 {
                    lock().before_step(vm);
                    let result = vm.run(self, 1);
                    lock().after_step(vm, &result);
                    match result {
                        Ok(RunOutcome::Budget) => {}
                        result => {
                            outcome = result;
                            break;
                        }
                    }
                }
            }
            None => {
                for _ in 0..1000 {
                    match vm.run(self, 1000) {
                        Ok(RunOutcome::Budget) => {}
                        result => {
                            outcome = result;
                            break;
                        }
                    }
                }
            }
        }
        self.active.remove(&vm.id);
        outcome
    }
    pub fn drain_requests(&mut self) -> Result<(), WorldError> {
        let mut count = 0;
        while let Some(request) = self.world.requests.pop_front() {
            count += 1;
            if count > 10000 {
                return Err(WorldError::Invalid("script request limit exceeded".into()));
            }
            let active = self
                .world
                .scripts
                .get(&request.script)
                .is_some_and(|script| self.active.contains(&script.program));
            if active {
                self.world.requests.push_front(request);
                break;
            }
            let spatial = request.event == ScriptEvent::Spatial;
            let previous = self.world.spatial_enabled;
            if spatial {
                self.world.spatial_enabled = false;
            }
            let result = self.dispatch(request);
            self.world.spatial_enabled = previous;
            if let Err(error) = result {
                self.world.diagnostic(error.to_string());
            }
        }
        Ok(())
    }
    /// A script that cannot be loaded/dispatched during map entry is reported;
    /// the remaining load stages (startup broadcasts, updates, timers) still run.
    fn report_load_dispatch(&mut self, result: Result<DispatchResult, WorldError>) {
        if let Err(error) = result {
            self.world
                .diagnostic(format!("map-load script request: {error}"));
        }
    }
    /// Talking-head speech playback completed (the presentation layer owns
    /// audio): release it, show speech frame zero, and allow a new fidget.
    pub fn speech_finished(&mut self) {
        if self.world.dialogue.speech.take().is_some()
            && let Some(head) = &mut self.world.dialogue.head
        {
            head.speech_end();
        }
    }
    pub fn map_update(&mut self) -> Result<(), WorldError> {
        let previous = self.world.spatial_enabled;
        self.world.spatial_enabled = false;
        self.world.broadcast(ScriptEvent::MapUpdate, 0);
        let result = self.drain_requests();
        self.world.spatial_enabled = previous;
        result
    }
    /// SPEC-006 queue pass: snapshot the clock once, remove each due event
    /// before handling it, and stop the pass at the first nonzero handler
    /// result (later due events stay pending). Returns that result, or zero.
    pub fn process_queue(&mut self) -> Result<i32, WorldError> {
        let snapshot = self.world.clock.ticks;
        let mut count = 0;
        while let Some(event) = self.world.queue.pop_due(snapshot) {
            count += 1;
            if count > 100_000 {
                return Err(WorldError::Invalid("queue dispatch limit exceeded".into()));
            }
            self.world.trace.push(WorldTrace::QueueFired(event.id));
            let result = self.handle_event(event)?;
            if result != 0 {
                return Ok(result);
            }
        }
        Ok(0)
    }
    /// Handler results: the specs define the stop rule but no handler-specific
    /// nonzero values. This host reports 1 when a script timer's dispatch
    /// requested a map transition (the condition under which SPEC-008 says a
    /// nonzero handler returns travel/rest to a map), for every withdrawal
    /// event of the player, and for a drug stage that left the player dead
    /// (SPEC-011 §3.3, §3.8); all others report 0.
    fn handle_event(&mut self, event: crate::queue::TimedEvent) -> Result<i32, WorldError> {
        match event.kind {
            EventKind::Script { script, parameter } => {
                if self.world.scripts.contains_key(&script) {
                    let transitions = self.map_transitions();
                    self.world.request(ScriptRequest {
                        script,
                        event: ScriptEvent::Timed,
                        source: 0,
                        target: 0,
                        fixed: parameter,
                        action: -1,
                        only_present: false,
                    });
                    self.drain_requests()?;
                    if self.map_transitions() > transitions {
                        return Ok(1);
                    }
                }
            }
            EventKind::MapUpdate => {
                self.map_update()?;
                self.world
                    .queue
                    .retain(|event| event.kind != EventKind::MapUpdate);
                self.world
                    .queue
                    .insert(self.world.clock.ticks, 600, 0, EventKind::MapUpdate)?;
            }
            EventKind::Midnight => {
                // SPEC-013 §3.20: a movie already playing skips the whole
                // handler, including the next midnight.
                if self.library.movie_playing {
                    return Ok(0);
                }
                let refs: Vec<_> = self.world.iter().map(|(r, _)| r).collect();
                for reference in refs {
                    if let Some(object) = self.world.get_mut(reference)
                        && let Some(mut lock) = object.lock()
                    {
                        lock.0 &= !LockFlags::JAMMED.0;
                        object.set_lock(lock);
                    }
                }
                self.water_countdown();
                // The radiation check follows the other work and precedes
                // the next midnight.
                self.radiation_check()?;
                self.world.queue.retain(|event| {
                    !matches!(event.kind, EventKind::MapUpdate | EventKind::Midnight)
                });
                self.world.enqueue_map_timers()?;
            }
            EventKind::Knockout => {
                // SPEC-007 §3.11.5: waking replaces a living critter's
                // knockout with a knockdown. Outside combat it gets up at
                // once, which clears the knockdown; in combat it gets up on
                // its turn.
                use crate::critical::effect::{DEAD, KNOCKED_DOWN, KNOCKED_OUT};
                let combat = self.world.combat;
                if let Some(ObjectState::Critter(state)) =
                    self.world.get_mut(event.object).map(|o| &mut o.raw.state)
                    && state.results.0 & DEAD == 0
                {
                    state.results.0 = (state.results.0 & !KNOCKED_OUT) | KNOCKED_DOWN;
                    if !combat {
                        state.results.0 &= !KNOCKED_DOWN;
                        self.stand_up(event.object);
                    }
                }
            }
            // SPEC-011 §3.3 and §3.8 handlers report their own stop rule.
            EventKind::Drug { stats, modifiers } => {
                return self.drug_event(event.object, stats, modifiers);
            }
            EventKind::Withdrawal(payload) => return self.withdrawal_event(event.object, payload),
            // SPEC-013 §3.19-3.20 and §2.5.
            EventKind::Poison | EventKind::Radiation { .. } => {
                return self.poison_or_radiation_event(event.object, event.kind);
            }
            EventKind::Other(kind) => return self.skill_event(event.object, kind),
        }
        Ok(0)
    }
    fn map_transitions(&self) -> usize {
        self.world
            .effects
            .iter()
            .filter(|effect| matches!(effect, WorldEffect::MapTransition { .. }))
            .count()
    }
    /// `game_time_advance`: events are processed after each full day and
    /// once more after the signed remainder. Under the SPEC-005 §3.7 safe
    /// rule the clock arithmetic wraps as 32-bit two's complement, so a
    /// negative remainder moves the clock back.
    pub fn advance_time(&mut self, ticks: i32) -> Result<(), WorldError> {
        let mut ticks = ticks;
        let advance = |host: &mut Self, delta: i32| {
            let wrapped = (host.world.clock.ticks as u32).wrapping_add_signed(delta);
            host.world.clock.set(u64::from(wrapped));
        };
        while ticks >= 864000 {
            advance(self, 864000);
            self.process_queue()?;
            ticks -= 864000;
        }
        advance(self, ticks);
        // Each pass's nonzero stop only ends that pass; time still advances.
        self.process_queue().map(|_| ())
    }
    /// SPEC-004 interpreter pass: each loaded program, in identifier order,
    /// polls its timed/conditional procedures (firing at most one, as a
    /// deferred host call) and then runs its ordinary ten-instruction budget.
    /// Programs blocked on a child or a wait predicate do not advance here;
    /// ended/stopped flows stay ended unless a deferred call was prepared.
    pub fn update_programs(&mut self) -> Result<(), WorldError> {
        for id in self.programs.keys().copied().collect::<Vec<_>>() {
            if self.active.contains(&id) {
                continue;
            }
            let Some(mut vm) = self.programs.remove(&id) else {
                continue;
            };
            if vm
                .status
                .contains(VmStatus::ATTACHED | VmStatus::SPAWN_BLOCKED)
            {
                self.programs.insert(id, vm);
                continue;
            }
            self.active.insert(id);
            let result = if vm.status.contains(VmStatus::WAITING) {
                vm.update(self).map(|_| ())
            } else {
                vm.poll_scheduled(self, u32::MAX)
                    .and_then(|_| vm.update(self))
                    .map(|_| ())
            };
            self.active.remove(&id);
            self.programs.insert(id, vm);
            if let Err(error) = result {
                self.world.diagnostic(format!("program {id}: {error}"));
            }
        }
        self.drain_requests()
    }
    pub fn tick(&mut self, now_ms: u64) -> Result<bool, WorldError> {
        self.world.elapsed_ms = now_ms;
        self.update_programs()?;
        let mut changed_by_animation = false;
        // SPEC-007 §3.10 attack shows play in order; SPEC-013 §3.10: a timed
        // or scripted explosion's art plays at its own rate, and its damage
        // step follows its last art.
        if self.step_shows(now_ms) {
            changed_by_animation = true;
        }
        if self.step_explosions(now_ms)? {
            changed_by_animation = true;
        }
        // SPEC-007 §3.11 the world's own sequences (walking, interactions,
        // doors) pause while a dialogue is open.
        if !self.world.dialogue.open && self.step_motions(now_ms) {
            changed_by_animation = true;
            self.drain_requests()?;
        }
        // SPEC-007 §3.11.2 the idle-fidget background task.
        if !self.world.dialogue.open && self.idle_fidget(now_ms)? {
            changed_by_animation = true;
        }
        // SPEC-007: service scripted combat requests and advance turns until
        // the player must act (host-configured AI drives NPC turns). Combat
        // waits while an attack still plays.
        if !self.presenting() && (self.service_combat_request()? || self.world.combat) {
            let mut ai =
                std::mem::replace(&mut self.combat_ai, Box::new(crate::combat_state::NoAi));
            let progress = self.advance_combat(&mut *ai);
            self.combat_ai = ai;
            progress?;
        }
        let (queue, update) = self.world.clock.background(
            now_ms,
            self.world.combat,
            self.world.dialogue.open,
            self.world.mode,
        )?;
        if queue {
            self.process_queue()?;
        }
        if update {
            self.map_update()?;
        }
        self.callbacks.extend(self.library.callbacks());
        self.drain_requests()?;
        let mut changed = changed_by_animation;
        // SPEC-006 §3.11 safe rule: missing head art keeps the current frame
        // and is diagnosed once, rather than failing the update.
        if let Some(head) = &mut self.world.dialogue.head {
            let art = head.path().and_then(|path| self.assets.frame(&path));
            let mut diagnostic = head.diagnostic.take();
            match art {
                Ok(art) => {
                    changed = head.tick(now_ms, art.frame_count(), art.fps, &mut self.world.rng)?;
                    diagnostic = diagnostic.or_else(|| head.diagnostic.take());
                }
                Err(error) if head.missing_art.is_none() => {
                    head.missing_art = Some(error.to_string());
                    diagnostic = Some(format!("talking-head art unavailable: {error}"));
                }
                Err(_) => {}
            }
            if let Some(message) = diagnostic {
                self.world.diagnostic(message);
            }
        }
        if self.world.dialogue.waiting
            && self.world.dialogue.page_offsets.len() > 1
            && now_ms.saturating_sub(self.world.dialogue.page_ms) >= 10000
        {
            self.world.dialogue.page =
                (self.world.dialogue.page + 1).min(self.world.dialogue.page_offsets.len() - 1);
            self.world.dialogue.page_ms = now_ms;
            changed = true;
        }
        while let Some((program, procedure)) = self.callbacks.pop_front() {
            if procedure == 0 {
                continue;
            }
            if self.active.contains(&program) {
                self.callbacks.push_front((program, procedure));
                break;
            }
            self.callback(program, procedure)?;
            self.library.release_named(program, procedure);
            changed = true;
        }
        // SPEC-013 §3.22: a pending transition runs in the main loop.
        if self.process_transition()? {
            changed = true;
        }
        // SPEC-013 §3.10: the request pass services a scripted explosion.
        if self.service_explosion_request()? {
            changed = true;
        }
        Ok(changed)
    }
    pub fn key_callback(&mut self, key: i32) -> bool {
        if let Some(binding) = self
            .library
            .any_key
            .or_else(|| self.library.keys.get(&key).copied())
        {
            self.callbacks.push_back(binding);
            true
        } else {
            false
        }
    }
    fn callback(&mut self, program: u32, procedure: u32) -> Result<RunOutcome, WorldError> {
        let mut vm = self
            .programs
            .remove(&program)
            .ok_or_else(|| WorldError::Invalid("callback program unavailable".into()))?;
        let result = vm
            .prepare_host_call(procedure as usize, true)
            .and_then(|_| self.run_vm(&mut vm));
        self.programs.insert(program, vm);
        Ok(result?)
    }
    pub(crate) fn text_resource(
        &mut self,
        text: &DialogueText,
        option: bool,
        first: bool,
    ) -> Result<(Vec<u8>, Vec<u8>), WorldError> {
        match text {
            DialogueText::Literal(text) => Ok((text.clone(), Vec::new())),
            DialogueText::Message {
                list: -2,
                message: -2,
            } => Ok((self.assets.dialogue_label(true), Vec::new())),
            DialogueText::Message {
                list: -1,
                message: -1,
            } => Ok((
                if option && first && self.world.player_stat(4) < 4 {
                    self.assets.dialogue_label(false)
                } else {
                    Vec::new()
                },
                Vec::new(),
            )),
            DialogueText::Message { list, message } if *list > 0 => {
                let value = self.assets.message(*list, *message).ok_or_else(|| {
                    WorldError::Invalid(format!("missing dialogue message {list}:{message}"))
                })?;
                Ok((value.text, value.sound))
            }
            _ => Err(WorldError::Invalid(
                "invalid dialogue message selectors".into(),
            )),
        }
    }
    pub(crate) fn realize_dialogue(&mut self) -> Result<(), WorldError> {
        let reply = self
            .world
            .dialogue
            .reply
            .clone()
            .ok_or_else(|| WorldError::Invalid("no dialogue reply".into()))?;
        let (mut text, sound) = self.text_resource(&reply, false, false)?;
        text.truncate(899);
        self.world.dialogue.resolved_reply = text;
        self.world.dialogue.resolved_options.clear();
        for (index, option) in self.world.dialogue.options.clone().iter().enumerate() {
            let (mut text, _) = self.text_resource(&option.text, true, index == 0)?;
            text.truncate(897);
            self.world
                .dialogue
                .resolved_options
                .push([b"\x95 ".as_slice(), text.as_slice()].concat());
        }
        if self.world.dialogue.head.is_some() && !sound.is_empty() {
            self.world.dialogue.speech = Some(sound.clone());
            self.world.emit(WorldEffect::Speech(sound));
            if let Some(head) = &mut self.world.dialogue.head {
                head.phoneme(0)?;
            }
        }
        Ok(())
    }
    pub(crate) fn begin_dialogue_loop(&mut self, vm: &mut Vm) -> Result<(), VmError> {
        vm.status.0 |= 0x20;
        let process = self
            .world
            .dialogue
            .begin(self.world.elapsed_ms)
            .map_err(|e| VmError::Host(e.to_string()))?;
        if process {
            self.realize_dialogue()
                .map_err(|e| VmError::Host(e.to_string()))?;
        }
        if process && self.interactive_dialogue {
            self.dialogue_waiters.push(vm.id);
        } else {
            vm.status.0 &= !0x20;
        }
        Ok(())
    }
    pub fn choose_dialogue(&mut self, index: Option<usize>) -> Result<(), WorldError> {
        self.world.emit(WorldEffect::StopSpeech);
        let callback = self.world.dialogue.choose(index)?;
        if let Some((program, procedure)) = callback {
            self.dialogue_callbacks.push(program);
            let result = match self.callback(program, procedure) {
                Ok(result) => result,
                Err(error) => {
                    self.dialogue_callbacks.pop();
                    self.dialogue_waiters.clear();
                    self.world.dialogue.pending_barter = false;
                    self.end_session();
                    return Err(error);
                }
            };
            if matches!(result, RunOutcome::Blocked | RunOutcome::Waiting) {
                return Ok(());
            }
            self.dialogue_callbacks.pop();
            if self.world.dialogue.after_callback() {
                self.realize_dialogue()?;
                return Ok(());
            }
        }
        self.resume_dialogue()
    }
    fn resume_dialogue(&mut self) -> Result<(), WorldError> {
        if let Some(program) = self.dialogue_waiters.pop()
            && let Some(mut vm) = self.programs.remove(&program)
        {
            vm.status.0 &= !0x20;
            let result = self.run_vm(&mut vm);
            self.programs.insert(program, vm);
            result?;
        }
        if let Some(program) = self.dialogue_callbacks.last().copied()
            && self
                .programs
                .get(&program)
                .is_some_and(|vm| vm.status.0 & 0x20 != 0)
        {
            self.dialogue_callbacks.pop();
            if self.world.dialogue.after_callback() {
                self.realize_dialogue()?;
            } else {
                return self.resume_dialogue();
            }
        }
        self.drain_requests()
    }
    /// NPC reaction inputs: player reputation is the player's sheet value;
    /// campaign counters come from configured campaign-global indices (SPEC-006
    /// names them but not their indices), else the host-supplied values.
    pub fn reputation_inputs(&self) -> crate::dialogue::Reputation {
        let mut inputs = self.world.reputation;
        if let Some(sheet) = self.world.player_sheet() {
            inputs.value = sheet.reputation;
        }
        if let (Some(indices), Some(gvars)) =
            (self.world.rules.reputation_globals, &self.world.gvars)
        {
            let read = |index: usize| gvars.get(index).copied().unwrap_or(0);
            inputs.child_killer = read(indices.child_killer);
            inputs.bad_monsters = read(indices.bad_monsters);
            inputs.good_monsters = read(indices.good_monsters);
            inputs.champion = read(indices.champion);
            inputs.berserker = read(indices.berserker);
        }
        inputs
    }
    pub(crate) fn reaction(&mut self, object: ObjectRef) -> i32 {
        let Some(sid) = self.world.script_for(object) else {
            return -1;
        };
        let Ok(base) = self.world.ensure_locals(sid) else {
            return -1;
        };
        // The standard layout needs four locals in the script's own slice;
        // missing local storage yields -1.
        if self
            .world
            .scripts
            .get(&sid)
            .is_none_or(|script| script.local_count < 4)
        {
            return -1;
        }
        let charisma = self.world.player_stat(3);
        let presence = self.world.perk(Perk::Presence);
        let cult = self.world.perk(Perk::CultOfPersonality) > 0;
        let reputation = self.reputation_inputs();
        self.world
            .lvars
            .get_mut(base..base + 4)
            .map_or(-1, |locals| {
                npc_reaction(locals, charisma, presence, cult, reputation)
            })
    }
    pub(crate) fn start_dialogue(
        &mut self,
        vm: &Vm,
        object: ObjectRef,
        reaction: i32,
        head: i32,
        background: i32,
    ) -> Result<(), WorldError> {
        if self.world.combat || object == 0 {
            return Ok(());
        }
        if self.world.get(object).is_none() {
            return Err(WorldError::Invalid("invalid dialogue object".into()));
        }
        let owner = self.self_object(vm);
        let head = if head == -1 {
            None
        } else {
            // SPEC-005 §3.3 safe rule: with no dialogue target yet, the mood
            // comes from the new self target.
            let source = match self.world.dialogue.target {
                0 => owner,
                target => target,
            };
            let mood = Mood::from_reaction(self.reaction(source));
            // SPEC-006 §3.11 safe rule: a head outside the list selects no
            // fidget art and is diagnosed; the session still opens.
            let definition = self.assets.head(head).unwrap_or_else(|error| {
                self.world
                    .diagnostic(format!("head {head} is unavailable: {error}"));
                HeadDefinition {
                    index: (head & 0xfff) as u16,
                    stem: String::new(),
                    counts: [0; 3],
                }
            });
            Some(TalkingHead::new(
                definition,
                mood,
                self.world.elapsed_ms,
                &mut self.world.rng,
            )?)
        };
        self.world.dialogue.reaction = head
            .as_ref()
            .map_or(reaction, |head| i32::from(head.mood.fidget()));
        if !self.world.dialogue.open {
            self.presentation = Some(PresentationState {
                font: self.library.font,
                mouse_hidden: self.library.mouse_hidden,
                scrolling: self.world.scrolling_enabled,
                palette_cycling: self.library.palette_cycling,
                music_paused: self.library.music_paused,
            });
        }
        self.world.dialogue.start(owner, vm.id, background, head)?;
        self.world.background_enabled = false;
        self.world.scrolling_enabled = false;
        self.library.mouse_hidden = false;
        self.library.palette_cycling = false;
        Ok(())
    }
    pub(crate) fn end_session(&mut self) -> bool {
        if !self.world.dialogue.end() {
            return false;
        }
        self.assets.release_dialogue_art();
        if let Some(saved) = self.presentation.take() {
            self.library.font = saved.font;
            self.library.mouse_hidden = saved.mouse_hidden;
            self.library.palette_cycling = saved.palette_cycling;
            self.library.music_paused = saved.music_paused;
            self.world.scrolling_enabled = saved.scrolling;
        }
        self.world.emit(WorldEffect::StopSpeech);
        self.world.background_enabled = true;
        self.world.broadcast(ScriptEvent::MapUpdate, 0);
        true
    }
    pub fn action(
        &mut self,
        actor: ObjectRef,
        target: ObjectRef,
        action: ObjectAction,
    ) -> Result<(), WorldError> {
        let object = self
            .world
            .get(target)
            .ok_or_else(|| WorldError::Invalid("interaction target missing".into()))?;
        let target_category = object.category();
        if self.world.get(actor).is_none_or(|o| !o.alive()) {
            return Err(WorldError::Invalid("actor is inactive/dead".into()));
        }
        // SPEC-013 §3.1-3.4: the skill-on request, approach and stage.
        if let ObjectAction::UseSkill(skill) = action {
            return if self.use_skill_on(actor, target, skill)? {
                Ok(())
            } else {
                Err(WorldError::Invalid("skill request rejected".into()))
            };
        }
        if action == ObjectAction::Pickup && target_category != Category::Item {
            return Err(WorldError::Invalid("only items can be picked up".into()));
        }
        if matches!(action, ObjectAction::LookAt | ObjectAction::Description)
            && ((object.raw.art_id >> 24) & 15) == 4
        {
            return Err(WorldError::Invalid("tile art cannot be examined".into()));
        }
        // SPEC-003 §2.6: talk is granted to critters by category and to other
        // objects only by extended flag 0x4000.
        // SPEC-006 §3.8: the ordinary talk action is restricted to a player
        // actor and critter target (SPEC-003's 0x4000 permission predicate is
        // `Prototype::permits_talk`, not a widening of this path).
        if action == ObjectAction::Talk {
            if actor != self.world.player
                || object.category() != Category::Critter
                || self.world.combat
            {
                return Err(WorldError::Invalid(
                    "talk requires the player and a critter outside combat".into(),
                ));
            }
            // SPEC-007 §3.11.1: at object distance 9 or more, or with a
            // shot-blocked line, the player runs to the target first; the
            // stage after that approach does not chase again.
            let far =
                self.world.object_distance(actor, target) >= crate::host_motion::TALK_RUN_DISTANCE;
            let blocked = self.world.shot(actor, target).hard_obstruction;
            if (far || blocked) && !self.arriving {
                return self.approach(actor, target, action, true);
            }
            if far {
                return Err(WorldError::Invalid("talk target is out of reach".into()));
            }
            // Post-movement: an unobstructed shot line (SPEC-007 shot query).
            if blocked {
                return Err(WorldError::Invalid(
                    "talk requires an unobstructed shot line".into(),
                ));
            }
            // Dialogue entry: tile-distance limit 12 and a reachability path.
            let reachable = match (
                self.world.get(actor).and_then(|o| o.raw.hex),
                self.world.get(target).and_then(|o| o.raw.hex),
            ) {
                (Some(from), Some(to)) => !self.world.route(actor, from, to, false).is_empty(),
                _ => false,
            };
            if self.world.distance(actor, target) > 12 || !reachable {
                return Err(WorldError::Invalid(
                    "talk target is beyond 12 tiles or unreachable".into(),
                ));
            }
            let sid = self
                .world
                .script_for(target)
                .ok_or_else(|| WorldError::Invalid("target has no dialogue script".into()))?;
            self.world.background_enabled = false;
            self.world.dialogue.target = target;
            self.dispatch(ScriptRequest {
                script: sid,
                event: ScriptEvent::Talk,
                source: 0,
                target: 0,
                fixed: 0,
                action: -1,
                only_present: false,
            })?;
            if !self.world.dialogue.open {
                self.world.background_enabled = true;
                self.map_update()?;
            }
            return Ok(());
        }
        if let ObjectAction::UseObject(item) = action {
            if self
                .world
                .get(item)
                .and_then(|item| item.prototype.as_ref())
                .is_some_and(|prototype| !prototype.permits_use_on())
            {
                return Err(WorldError::Invalid(
                    "item does not permit use on another object".into(),
                ));
            }
            return self.use_item_on(actor, item, target);
        }
        // SPEC-013 §3.16: containers (the get-object action) and critters
        // (looting) are used without the prototype's use permission.
        let container = object.prototype.as_ref().is_some_and(|prototype| {
            matches!(&prototype.data, PrototypeData::Item(item) if matches!(item.data, ItemData::Container { .. }))
        });
        if action == ObjectAction::Use
            && !container
            && target_category != Category::Critter
            && object
                .prototype
                .as_ref()
                .is_some_and(|prototype| !prototype.permits_use())
        {
            return Err(WorldError::Invalid("object does not permit use".into()));
        }
        if action == ObjectAction::Use && object.owner == Some(actor) {
            self.world
                .emit(WorldEffect::DefaultAction { action, target });
            self.map_update()?;
            return Ok(());
        }
        // SPEC-013 §3.16: a container that cannot be picked up is used
        // (and looted) instead.
        if action == ObjectAction::Pickup
            && object.prototype.as_ref().is_some_and(|p| {
                matches!(&p.data, PrototypeData::Item(item) if matches!(item.data, ItemData::Container { .. }))
                    && !p.extended.contains(vault_formats::pro::ExtendedFlags::PICKUP_CONTAINER)
            })
        {
            return self.action(actor, target, ObjectAction::Use);
        }
        // SPEC-007 §3.11.3: a pickup or a use runs as the actor's
        // interaction sequence, whose callbacks perform it. Using a critter
        // loots it (SPEC-013 §3.16); a ladder up is climbed (SPEC-013
        // §3.23).
        let ladder_up = object.prototype.as_ref().is_some_and(|prototype| {
            matches!(
                prototype.data,
                PrototypeData::Scenery(vault_formats::pro::Scenery {
                    data: vault_formats::pro::SceneryData::LadderUp { .. },
                    ..
                })
            )
        });
        match action {
            ObjectAction::Pickup => return self.pickup_sequence(actor, target),
            ObjectAction::Use if target_category == Category::Critter => {
                return self.loot_sequence(actor, target);
            }
            ObjectAction::Use if container => return self.container_sequence(actor, target),
            ObjectAction::Use if ladder_up => return self.ladder_sequence(actor, target),
            ObjectAction::Use => {
                return self.use_sequence(actor, target, crate::motion::Call::Use { target });
            }
            _ => {}
        }
        let event = match action {
            ObjectAction::LookAt => ScriptEvent::LookAt,
            ObjectAction::Description => ScriptEvent::Description,
            _ => unreachable!(),
        };
        let result = if let Some(script) = self.world.script_for(target) {
            self.dispatch(ScriptRequest {
                script,
                event,
                source: actor,
                target,
                fixed: 0,
                action: -1,
                only_present: false,
            })?
        } else {
            DispatchResult::default()
        };
        if result.pending {
            return Ok(());
        }
        if !result.overridden {
            match action {
                // SPEC-014 §3.27: `proto.msg` 490, or 491/492 at random
                // for a dead critter, naming the object; nothing while the
                // player is dead. Without the catalog the short text is the
                // name (with "Dead " for a dead critter). The pick draws
                // from the presentation generator, not the world's.
                ObjectAction::LookAt if self.world.is_dead(self.world.player) => {}
                ObjectAction::LookAt => {
                    let object = self.world.get(target).expect("target");
                    let name = object.name.clone();
                    let dead = object.category() == Category::Critter
                        && (self.world.is_dead(target)
                            || object
                                .character
                                .as_ref()
                                .is_some_and(|sheet| sheet.current_hp <= 0));
                    let id = if dead {
                        crate::RandomSource::range(&mut self.presentation_rng, 491, 492)?
                    } else {
                        490
                    };
                    match self.game_text("proto.msg", id) {
                        Some(format) => self.world.emit(WorldEffect::Message(
                            crate::host_drugs::format_message(
                                &format,
                                &[crate::host_drugs::Arg::Text(&name)],
                            ),
                        )),
                        None if dead => self
                            .world
                            .emit(WorldEffect::Message([b"Dead ".as_slice(), &name].concat())),
                        None => self.world.emit(WorldEffect::Message(name)),
                    }
                }
                ObjectAction::Description => self.world.emit(WorldEffect::Message(
                    self.world.get(target).expect("target").description.clone(),
                )),
                _ => {}
            }
            self.world
                .emit(WorldEffect::DefaultAction { action, target });
        }
        if action == ObjectAction::Description
            && let Some(sheet) = self.world.get(target).and_then(|o| o.character.as_ref())
        {
            let hp = sheet.current_hp;
            let max = sheet.stat(7, self.world.context(target));
            let condition = if hp <= 0 {
                0
            } else if hp >= max {
                4
            } else {
                3 * hp / max.max(1) + 1
            };
            let text = if target == self.world.player || self.world.perk(Perk::Awareness) > 0 {
                format!("Condition {condition}, HP {hp}/{max}")
            } else {
                format!("Condition {condition}")
            };
            self.world.emit(WorldEffect::Message(text.into_bytes()));
        }
        self.drain_requests()
    }
    /// SPEC-007 §3.11.3 using an item on a target runs as the scenery use
    /// sequence, whose use callback is [`Self::use_item_stage`].
    fn use_item_on(
        &mut self,
        actor: ObjectRef,
        item: ObjectRef,
        target: ObjectRef,
    ) -> Result<(), WorldError> {
        if self.world.get(item).is_none() || self.world.get(target).is_none() {
            return Err(WorldError::Invalid("missing item/target".into()));
        }
        self.use_sequence(actor, target, crate::motion::Call::UseOn { item, target })
    }
    /// SPEC-006 §3.8 the use-on stage once the actor stands by the target.
    pub(crate) fn use_item_stage(
        &mut self,
        actor: ObjectRef,
        item: ObjectRef,
        target: ObjectRef,
    ) -> Result<(), WorldError> {
        if self.world.get(item).is_none() || self.world.get(target).is_none() {
            return Ok(());
        }
        // SPEC-013 §3.13: kits take their own route, with no use-on scripts.
        let pid = self.world.get(item).expect("item").raw.pid;
        if self.kit_skill(pid).is_some() {
            return self.use_kit(actor, item, target);
        }
        let item_script = self.world.script_for(item);
        let target_script = self.world.script_for(target);
        let mut retained = 0;
        if let Some(script) = item_script {
            let result = self.dispatch(ScriptRequest {
                script,
                event: ScriptEvent::UseObject,
                source: actor,
                target,
                fixed: 0,
                action: -1,
                only_present: false,
            })?;
            if result.pending {
                return Ok(());
            }
            retained = result.result;
            if retained != 0 {
                if retained == 1 {
                    self.world.erase(item);
                }
                self.map_update()?;
                return Ok(());
            }
        }
        let result = if let Some(script) = target_script {
            self.dispatch(ScriptRequest {
                script,
                event: ScriptEvent::UseObject,
                source: actor,
                target: item,
                fixed: 0,
                action: -1,
                only_present: false,
            })?
        } else {
            DispatchResult::default()
        };
        if result.pending {
            return Ok(());
        }
        let drug = self
            .world
            .item_data(item)
            .is_some_and(|data| matches!(data.data, ItemData::Drug(_)));
        if !result.overridden && drug {
            // SPEC-011 §3.6: the built-in drug use on a target.
            self.use_drug_on(actor, item, target)?;
            return self.map_update();
        }
        if !result.overridden {
            self.world.emit(WorldEffect::DefaultAction {
                action: ObjectAction::UseObject(item),
                target,
            });
        } else if item_script.is_none() {
            retained = result.result;
        }
        if retained == 1 {
            self.world.erase(item);
        }
        self.map_update()
    }
    /// SPEC-007 §3.11.1 an approach to `target` (a run when `run`), after
    /// which the request runs its own stage, re-checking its distance rule.
    /// An unreachable target refuses the approach.
    pub(crate) fn approach(
        &mut self,
        actor: ObjectRef,
        target: ObjectRef,
        action: ObjectAction,
        run: bool,
    ) -> Result<(), WorldError> {
        let (Some(a), Some(b)) = (self.world.get(actor), self.world.get(target)) else {
            return Err(WorldError::Invalid(
                "missing interaction actor/target".into(),
            ));
        };
        if a.raw.elevation != b.raw.elevation {
            return Err(WorldError::Invalid(
                "interaction is on another elevation".into(),
            ));
        }
        let movement = self
            .plan_to_object(actor, target)
            .ok_or_else(|| WorldError::Invalid("unplaced interaction actor/target".into()))?;
        if movement.directions.is_empty() {
            return Err(WorldError::Invalid(
                "interaction target is unreachable".into(),
            ));
        }
        self.approach_sequence(actor, target, action, run)
    }
    /// SPEC-014 §3.27 a movement-mode click: walk toward `tile`, limited in
    /// combat to the actor's AP plus free movement. Returns whether a route
    /// exists.
    pub fn walk_to(&mut self, actor: ObjectRef, tile: i32) -> Result<bool, WorldError> {
        self.walk_toward(actor, tile, false, 0)
    }
    /// `walk_to` keeping `reserve` action points in combat (the player's
    /// Ctrl walk keeps the current attack's cost, SPEC-014 §3.27); nothing
    /// moves when no points remain for the route.
    pub fn walk_to_reserving(
        &mut self,
        actor: ObjectRef,
        tile: i32,
        reserve: i32,
    ) -> Result<bool, WorldError> {
        self.walk_toward(actor, tile, false, reserve)
    }
    pub fn close_dialogue(&mut self) -> Result<bool, WorldError> {
        if !self.world.dialogue.open {
            return Ok(false);
        }
        self.world.dialogue.pending_barter = false;
        self.world.dialogue.options.clear();
        self.world.dialogue.waiting = false;
        self.world.dialogue.depth = 0;
        self.end_session();
        self.resume_dialogue()?;
        self.map_update()?;
        Ok(true)
    }
}
impl<A: WorldAssets> VmHost for WorldHost<A> {
    fn now_ms(&self) -> u32 {
        self.world.elapsed_ms as u32
    }
    fn extension(&mut self, opcode: u16, vm: &mut Vm) -> Result<(), VmError> {
        let call = extensions::decode(vm, opcode)?;
        if self.calls.len() < 100_000 {
            self.calls.push(CallTrace {
                opcode,
                operands: call.args.iter().map(|arg| arg.value).collect(),
            });
        }
        let result = if opcode <= 0x80a0 {
            self.library_call(vm, &call)?
        } else {
            self.game_call(vm, &call)?
        };
        if call.spec.push == PushKind::None && result.is_some() {
            return Err(VmError::Host(
                "statement unexpectedly returned a value".into(),
            ));
        }
        if let Some(result) = result {
            let value = match result {
                ExternalValue::Value(value) => value,
                ExternalValue::String(text) => vm.intern(&text)?,
            };
            vm.value_stack.push(value)?;
        }
        Ok(())
    }
    fn program_mut(&mut self, id: u32) -> Result<&mut Vm, VmError> {
        self.programs
            .get_mut(&id)
            .ok_or_else(|| VmError::Host("missing program".into()))
    }
    fn purge(&mut self, id: u32) -> Result<(), VmError> {
        self.registry.purge(id)?;
        self.library.cleanup(id);
        Ok(())
    }
    /// SPEC-004 0x8008-0x800B: start a fresh program image (no stack cloning).
    /// The interpreter pass runs its bootstrap; the VM blocks an attached or
    /// spawning parent. A replacement inherits the old program's parent link.
    fn launch(
        &mut self,
        name: &[u8],
        mode: LaunchMode,
        parent: Option<u32>,
        critical: bool,
    ) -> Result<u32, VmError> {
        let (path, program) = self.assets.program_by_name(name)?;
        let mut vm = Vm::new(program);
        vm.id = self.next_program;
        self.next_program = self
            .next_program
            .checked_add(1)
            .ok_or_else(|| VmError::Host("program identifiers exhausted".into()))?;
        self.registry.register_program(vm.id, path.as_bytes());
        vm.parent = parent;
        if critical {
            vm.status.0 |= VmStatus::CRITICAL;
        }
        if mode == LaunchMode::Replace
            && let Some(caller) = parent.and_then(|parent| self.programs.get_mut(&parent))
        {
            caller.child = Some(vm.id);
        }
        let id = vm.id;
        self.programs.insert(id, vm);
        Ok(id)
    }
    fn wait_ready(&mut self, reference: u32, _start: u32, _deadline: u32) -> Result<bool, VmError> {
        if reference == 2 {
            // SPEC-005 §3.2 safe rule: wait until playback ends (the
            // reference's predicate has the opposite polarity).
            Ok(!self.library.movie_playing)
        } else {
            Err(VmError::Host("unknown wait predicate".into()))
        }
    }
    fn export_variable(&mut self, owner: u32, name: &[u8]) -> Result<(), VmError> {
        self.registry.export_variable(owner, name)
    }
    fn fetch_variable(&mut self, name: &[u8]) -> Result<ExternalValue, VmError> {
        self.registry.fetch_variable(name)
    }
    fn store_variable(&mut self, name: &[u8], value: ExternalValue) -> Result<(), VmError> {
        self.registry.store_variable(name, value)
    }
    fn export_procedure(
        &mut self,
        name: &[u8],
        procedure: ExportedProcedure,
    ) -> Result<(), VmError> {
        self.registry.export_procedure(name, procedure)
    }
    fn import_procedure(&mut self, name: &[u8]) -> Result<ExportedProcedure, VmError> {
        self.registry.import_procedure(name)
    }
}
