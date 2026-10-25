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
/// SPEC-006 §3.5 the per-program budget of a script's first time slice.
const FIRST_REQUEST_BUDGET: u32 = 5000;

/// An observer of script execution: a debugger's live instruction trace.
/// While one is attached to a host, its scripts run one instruction per
/// `Vm::step` call (the same scheduling as larger budgets) and every step is
/// reported before and after it executes: requested procedures, the time
/// slice and the condition streams of conditional procedures. Nested script
/// runs are reported too; the observer is never held across a step.
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
/// SPEC-005 0x80A5 and the other D-returning rows push "no name" as D
/// offset zero, which is also where a program's first interned string would
/// live (SPEC-004 §3.1 says nothing of offset zero). Reserving the empty
/// string there keeps the null value from aliasing a real string.
fn reserve_null_string(vm: &mut Vm) -> Result<(), VmError> {
    vm.intern(b"").map(|_| ())
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
    /// The program that opened `building_animation`; its fault drops it.
    pub(crate) building_program: Option<u32>,
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
    /// SPEC-014 §3.27 the combat-wide count of running sequences and the
    /// input lock it drives.
    pub combat_lock: crate::clicks::CombatLock,
    /// The scheduler time of the world's sequences.
    pub(crate) motion_time: u64,
    /// SPEC-014 §3.28 when the world's sequences stopped for a pause.
    pub(crate) paused: crate::host_motion::Paused,
    /// SPEC-012 §3.12 a water-countdown movie was requested and has not
    /// finished playing.
    pub(crate) countdown_movie: bool,
    /// SPEC-007 §3.11.2 the idle-fidget background task, while it runs.
    pub(crate) fidget: Option<crate::host_fidget::Fidget>,
    /// SPEC-006 §3.7 the critter-script cycle counter: 0 at program start
    /// and never reset by a map change, a game load or a new game (a shell
    /// that builds a new host for a new game carries it over).
    pub critter_cycle: usize,
    /// SPEC-013 §3.22: the open town or world map was started by a
    /// transition to map -1 or -2, so the request pending when it closes
    /// is discarded.
    pub(crate) transition_world_map: bool,
    presentation: Option<PresentationState>,
    /// An interaction's approach has just ended: its action runs as the
    /// sequence's proximity step (SPEC-013 §3.2), not a new approach.
    pub(crate) arriving: bool,
    /// SPEC-005 0x80F9: a dialogue-entry request for this object, serviced
    /// by the next tick outside the running script.
    pub(crate) pending_dialogue: Option<ObjectRef>,
    /// A debugger's live trace of script execution, when attached.
    pub vm_observer: Option<std::sync::Arc<std::sync::Mutex<dyn VmObserver>>>,
    /// Presentation-only draws (a dead critter's look-at line), apart from
    /// the world's generator so looking never moves its sequence.
    pub presentation_rng: crate::ParkMiller,
    /// SPEC-008 §3.10: while music is disabled, requests are recorded but
    /// nothing plays; re-enabling plays the last recorded track.
    pub music_enabled: bool,
    /// The last requested music track (`sound/music/<track>.acm`).
    pub last_music: Option<String>,
    /// SPEC-002 §3.7 language filter: the word list and the per-script
    /// message catalogs as loaded.
    pub script_messages: crate::dialogue::ScriptMessages,
    /// SPEC-005 §3.2 the programs inside one of their own self-stopping
    /// game opcodes (host-busy `0x0020`): the dispatcher skips requests to
    /// them, so the opcode's own `destroy_p_proc` or `damage_p_proc` does
    /// not run.
    pub(crate) host_busy: Vec<u32>,
    /// SPEC-005 §3.2 the override flag and result a self-stopping opcode
    /// found on its script instance before the opcode removed it (a
    /// destroyed or killed owner), by instance: the request that ran the
    /// procedure still returns them.
    pub(crate) departed: BTreeMap<u32, (bool, i32)>,
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
    fn critter_alias(&self, index: u16) -> Option<i32> {
        (**self).critter_alias(index)
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
    fn mod_layers(&self) -> usize {
        (**self).mod_layers()
    }
    fn table_patches(&self) -> Vec<(String, String)> {
        (**self).table_patches()
    }
    fn set_enhanced_scripts(&self, enabled: bool) {
        (**self).set_enhanced_scripts(enabled)
    }
    fn game_profile(&self) -> crate::GameProfile {
        (**self).game_profile()
    }
    fn set_game_profile(&self, game: crate::GameProfile) {
        (**self).set_game_profile(game)
    }
    fn game_messages(
        &self,
        file: &str,
    ) -> Result<std::sync::Arc<vault_formats::MsgCatalog>, WorldError> {
        (**self).game_messages(file)
    }
    fn dialogue_catalog(&self, list: i32) -> Option<vault_formats::MsgCatalog> {
        (**self).dialogue_catalog(list)
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
            building_program: None,
            playback: Default::default(),
            floating_style: 1,
            dialogue_waiters: Vec::new(),
            dialogue_callbacks: Vec::new(),
            motions: Vec::new(),
            running_motions: Vec::new(),
            next_motion: 0,
            combat_lock: Default::default(),
            motion_time: 0,
            paused: Default::default(),
            countdown_movie: false,
            fidget: None,
            critter_cycle: 0,
            transition_world_map: false,
            presentation: None,
            arriving: false,
            pending_dialogue: None,
            vm_observer: None,
            presentation_rng: crate::ParkMiller::seeded(1).expect("a positive seed"),
            music_enabled: true,
            last_music: None,
            script_messages: Default::default(),
            host_busy: Vec::new(),
            departed: BTreeMap::new(),
        }
    }
    /// SPEC-008 §3.10 a music request: the track is recorded as the last
    /// one and, while music is enabled, requested from the presentation
    /// layer (`sound/music/<track>.acm`, looping, restarted even when the
    /// same track is already playing). While music is disabled only the
    /// record is kept.
    pub fn request_music(&mut self, track: &str) {
        self.last_music = Some(track.to_string());
        if self.music_enabled {
            self.world.emit(WorldEffect::Music(track.to_string()));
        }
    }
    /// SPEC-008 §3.10 enabling or disabling music: re-enabling it plays the
    /// last recorded track.
    pub fn set_music_enabled(&mut self, enabled: bool) {
        let resumed = enabled && !self.music_enabled;
        self.music_enabled = enabled;
        if resumed && let Some(track) = self.last_music.clone() {
            self.world.emit(WorldEffect::Music(track));
        }
    }
    /// SPEC-008 §3.10: the world map plays `03wrldmp`. A transition to the
    /// world map requests it here; a presentation layer that opens the
    /// world map on its own (a key, the town map's World Map button) calls
    /// this when it does.
    pub fn play_world_map_music(&mut self) {
        self.request_music(crate::worldmap::WORLD_MAP_MUSIC);
    }
    pub fn rules(&mut self, rules: WorldRules) {
        self.world.rules = rules;
    }
    /// The engine behaviour preset (Vanilla by default).
    pub fn preset(&self) -> crate::EnginePreset {
        self.world.rules.preset
    }
    /// Select the engine behaviour preset. Going back to Vanilla drops any
    /// loaded table overrides unless a mod layer is mounted.
    pub fn set_preset(&mut self, preset: crate::EnginePreset) {
        self.world.rules.preset = preset;
        self.assets
            .set_enhanced_scripts(preset == crate::EnginePreset::Enhanced);
        if !self.overrides_allowed() {
            self.world.rules.tables = None;
        }
    }
    /// Whether campaign-table overrides may be read: only with the
    /// Enhanced preset or a mounted mod layer, never in plain Vanilla.
    pub fn overrides_allowed(&self) -> bool {
        self.preset() == crate::EnginePreset::Enhanced || self.assets.mod_layers() > 0
    }
    /// Read the campaign-table overrides (`data/vault-engine/<table>.txt`,
    /// see [`crate::tables`]) from the VFS, when [`Self::overrides_allowed`].
    /// Each present file replaces its whole table after full validation; a
    /// bad file is an error and leaves the tables in force unchanged.
    /// Returns the tables overridden. In plain Vanilla nothing is read and
    /// the compiled tables stay in force.
    pub fn load_table_overrides(&mut self) -> Result<Vec<crate::tables::Table>, WorldError> {
        if !self.overrides_allowed() {
            self.world.rules.tables = None;
            return Ok(Vec::new());
        }
        let mut tables = crate::tables::CampaignTables::compiled().clone();
        let mut loaded = Vec::new();
        let read = |path: &str| -> Result<String, WorldError> {
            let bytes = self.assets.resource(path)?;
            String::from_utf8(bytes)
                .map_err(|_| WorldError::Invalid(format!("{path}: not UTF-8 text")))
        };
        // The mods' record patches (`vault-mod.toml` `[[patches]]`).
        let patches = self.assets.table_patches();
        if let Some((name, _)) = patches.iter().find(|(name, _)| {
            !crate::tables::Table::ALL
                .iter()
                .any(|table| table.name() == name)
        }) {
            return Err(WorldError::Invalid(format!(
                "mod registration: no campaign table `{name}`"
            )));
        }
        for table in crate::tables::Table::ALL {
            let path = table.override_path();
            let mine: Vec<&String> = patches
                .iter()
                .filter(|(name, _)| name == table.name())
                .map(|(_, file)| file)
                .collect();
            let overridden = self.assets.available(path.as_bytes());
            if !overridden && mine.is_empty() {
                continue;
            }
            // The whole-file override (else the compiled table), then each
            // patch in load order; the result is validated as an override.
            let mut text = if overridden {
                read(&path)?
            } else {
                table.canonical().to_owned()
            };
            for file in mine {
                text = vault_formats::mod_manifest::apply_patch(&text, &read(file)?)
                    .map_err(|error| WorldError::Invalid(format!("{file}: {error}")))?;
            }
            tables = tables.with_override(table, &text)?;
            loaded.push(table);
        }
        self.world.rules.tables = (!loaded.is_empty()).then(|| std::sync::Arc::new(tables));
        Ok(loaded)
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
    /// The failure is also a world diagnostic: without the array every
    /// `global_var` read gives -1 (SPEC-005 0x80C5), so play would go on
    /// with campaign flags reading as set.
    pub fn load_campaign(&mut self) -> Result<(), WorldError> {
        self.world.trace.push(WorldTrace::InventoryRefresh);
        let gam = self
            .assets
            .gam("data/vault13.gam", GamSection::Campaign)
            .and_then(|gam| {
                gam.ok_or_else(|| {
                    WorldError::Invalid("campaign initializer data/vault13.gam did not open".into())
                })
            });
        // Table overrides belong to the campaign data; plain Vanilla reads
        // nothing here.
        if let Err(error) = self.load_table_overrides() {
            self.world
                .diagnostic(format!("campaign table override rejected: {error}"));
        }
        match gam {
            Ok(gam) => {
                self.world.gvars = Some(gam.values);
                Ok(())
            }
            Err(error) => {
                self.world.diagnostic(format!(
                    "campaign globals unavailable, every global_var reads -1: {error}"
                ));
                Err(error)
            }
        }
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
        // SPEC-006 §3.5 step 1: a dialogue still open ends with the map, so
        // nothing calls back into the left map's programs.
        if self.world.dialogue.open {
            self.world.dialogue.pending_barter = false;
            self.world.dialogue.options.clear();
            self.world.dialogue.waiting = false;
            self.world.dialogue.depth = 0;
            self.end_session();
        }
        self.world.dialogue.talking = false;
        self.world.dialogue.target = 0;
        self.pending_dialogue = None;
        for program in self.programs.keys().copied().collect::<Vec<_>>() {
            self.registry.purge(program)?;
            self.library.cleanup(program);
        }
        // SPEC-005 §2.2 / SPEC-006 §3.5 step 2: every program is freed and
        // the whole exported-variable registry is cleared, all owners.
        self.registry.clear();
        self.programs.clear();
        self.program_scripts.clear();
        self.departed.clear();
        // Nothing queued for the left map's programs may run against the
        // next map's script table, where ids can alias: callbacks (named
        // handlers, `key_callback`), dialogue waiters and script requests
        // deferred while a program was active or broadcast by the exit
        // procedures end with the map.
        self.callbacks.clear();
        self.dialogue_waiters.clear();
        self.dialogue_callbacks.clear();
        self.world.requests.clear();
        // SPEC-014 §3.35 step 1 stopped every animation; the presentations
        // and deferred requests that reference the left map's objects
        // (attack shows, an explosion and its art, a loot or combat request,
        // floating text) do not survive it either.
        self.world.shows.clear();
        self.world.explosion = None;
        self.world.pending_explosion = None;
        self.world.pending_loot = None;
        self.world.pending_combat = None;
        self.world.floating.clear();
        self.arriving = false;
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
        self.world.import_map(self.empty_map(), &self.assets)?;
        self.world.persistence.current_map = None;
        self.world.loading = false;
        self.world.unloaded = true;
        Ok(saved)
    }
    /// The SPEC-003 new-map default, at the game's map version (SPEC-016).
    fn empty_map(&self) -> vault_formats::MapFile {
        let mut map = empty_map();
        map.header.version = self.assets.game_profile().map_version();
        map
    }
    /// Read maps and prototypes in `game`'s layouts (SPEC-016) from now on.
    pub fn set_game_profile(&mut self, game: crate::GameProfile) {
        self.assets.set_game_profile(game);
    }
    pub fn load_map(&mut self, path: &str) -> Result<(), WorldError> {
        self.world.trace.push(WorldTrace::LoadStep(1));
        self.leave_map()?;
        self.teardown_map()?;
        // SPEC-008 §3.10 / SPEC-013 §3.22: loading first starts the
        // background track `wind2`, looping, until the city music starts.
        self.request_music(crate::worldmap::LOADING_MUSIC);
        self.world.trace.push(WorldTrace::LoadStep(2));
        let (map, mut saved, mut load_error, file) = match self.read_map(path) {
            Ok((map, saved, file)) => (map, saved, None, file),
            Err(error) => (self.empty_map(), false, Some(error), String::new()),
        };
        self.world.persistence.current_map = None;
        self.world.restoring = map.header.flags.0 & 1 != 0;
        for message in &map.diagnostics {
            self.world.diagnostic(message.clone());
        }
        self.world.trace.push(WorldTrace::LoadStep(3));
        self.world.import_map(map, &self.assets)?;
        self.world.unloaded = false;
        // SPEC-006 §3.5 step 3: a fresh map's critters, whose stored -1
        // last attacker is nothing, start with no last attacker; under the
        // saved-map safe rule a stored combat identifier resolves.
        if saved {
            self.world.resolve_last_attackers();
        }
        self.world.trace.push(WorldTrace::LoadStep(4));
        self.world.elevation = self.world.map.header.entry_elevation;
        // SPEC-014 §3.29: an entry hex outside the map border fails the load.
        let entry = self.world.map.header.entry_hex;
        if !self.world.set_view_center(entry) {
            // SPEC-006 §3.5 [May-improve] safe rule: a map that cannot be
            // entered is reported, and the engine is left on the reset empty
            // map, which still goes through the shared post-load stages.
            load_error = Some(WorldError::Invalid(format!(
                "map entry hex {entry} is outside the map border"
            )));
            self.world.import_map(self.empty_map(), &self.assets)?;
            self.world.restoring = false;
            saved = false;
            self.world.elevation = self.world.map.header.entry_elevation;
            self.world.set_view_center(self.world.map.header.entry_hex);
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
            player.raw.displacement = [0; 2];
        }
        // SPEC-014 §3.44: the entrance placement is a move to a hex.
        let player = self.world.player;
        self.world.reinsert(player);
        // SPEC-013 §3.22: the entrance placement is an ordinary player
        // placement, so exit grids on its hex request a transition (the
        // player is not moved off them). SPEC-014 §3.36: then, from "none"
        // after the import, it hides the roof group over the player, if any.
        self.world.check_exit_grids(hex, elevation);
        self.world.place_player_roof(hex, elevation);
        // SPEC-008 §3.10: the current map is identified from the header's
        // name field, not the requested file, and the header's map-number
        // word is overwritten with that number. The working `.SAV` name
        // (SPEC-014 §3.35) comes from the file name the map was opened
        // under, never from the number; the current town is not changed by
        // a map load (SPEC-014 §3.39).
        let catalog = self.assets.game_messages("map.msg").ok();
        let number = crate::worldmap::header_map_number(
            &self.world.map.header.name,
            self.world.map.header.map_number,
            catalog.as_deref(),
        );
        // A name §3.10 does not match may be a mod's registered map
        // (`vault-mod.toml` `[[maps]]`, numbers 66 and up).
        let number = if number < 0 {
            let name = &self.world.map.header.name;
            let name = &name[..name
                .iter()
                .position(|byte| *byte == 0)
                .unwrap_or(name.len())];
            self.assets
                .map_index(name)
                .filter(|number| *number >= crate::worldmap::MAP_COUNT as i32)
                .unwrap_or(number)
        } else {
            number
        };
        self.world.map.header.map_number = number;
        self.world.persistence.current_map = load_error
            .is_none()
            .then(|| crate::persistence::saved_map_name(file.as_bytes()))
            .flatten();
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
        // SPEC-006 §3.5 step 7 / SPEC-013 §3.22: the party is recovered
        // around the player's final entry placement, its script records
        // after the map's own critter records.
        self.world.recover_party_scripts()?;
        self.world.place_party()?;
        if let Some(player) = self.world.get_mut(self.world.player) {
            player.raw.light = Light {
                radius: 4,
                intensity: 65536,
            };
        }
        self.world.rebuild_lighting();
        self.world.background_enabled = true;
        self.world.trace.push(WorldTrace::LoadStep(8));
        for script in self.world.script_ids() {
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
        for script in self.world.script_ids() {
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
        // SPEC-006 §3.5: after step 10 every load, fresh or saved, leaves
        // the player with no last attacker.
        if let Some(player) = self.world.get_mut(self.world.player) {
            player.opponent = 0;
        }
        // SPEC-014 §3.35: once a saved copy has loaded, with its map
        // scripts, the revisit rules age it; then the city music starts.
        if saved && load_error.is_none() {
            self.revisit()?;
        }
        // SPEC-008 §3.10: once the map number is known its city track
        // replaces `wind2`; an unnumbered map leaves `wind2` looping. One
        // map change makes the reference request the track two or three
        // times; under the [May-improve] rule it is requested once per map
        // change here (the presentation layer still restarts a track that
        // already plays, the `[Must-match]` rule).
        if let Some(track) = self
            .world
            .rules
            .tables()
            .city_music(self.world.map.header.map_number)
            .map(str::to_string)
        {
            self.request_music(&track);
        }
        self.drain_requests()?;
        if let Some(error) = load_error {
            self.world.diagnostic(error.to_string());
            return Err(error);
        }
        Ok(())
    }
    pub(crate) fn clear_program_bindings(&mut self) {
        self.departed.clear();
        self.host_busy.clear();
        self.program_scripts.clear();
        self.active.clear();
        self.motions.clear();
    }
    /// SPEC-008 §3.10 opening a map by name: the file name is uppercased; a
    /// name containing `.MAP` first probes its saved copy (the first `.MAP`
    /// read as `.SAV`) in the working map set, which then becomes the file
    /// name; a name without `.MAP` is opened as given, from the working set
    /// (the master patch root) before the assets. Returns the map, whether
    /// it came from the working set, and the file name it was opened under.
    fn read_map(&self, path: &str) -> Result<(vault_formats::MapFile, bool, String), WorldError> {
        let file = crate::persistence::requested_file_name(path);
        let saved = crate::persistence::saved_probe(&file).unwrap_or_else(|| file.clone());
        if let Some(bytes) = self.world.persistence.maps.get(&saved) {
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
            let game = self.assets.game_profile();
            return Ok((
                vault_formats::MapFile::parse_for(bytes, &lookup, game)?,
                true,
                saved,
            ));
        }
        Ok((self.assets.map(path)?, false, file))
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
    /// SPEC-006 §3.5 the generic dispatcher. Every request stores its
    /// SPEC-004 §3.7 event number as the instance's action code before the
    /// program is loaded or entered; the code is not reset afterwards. A
    /// script instance without a loaded program takes the first-request
    /// path: its `.INT` is loaded (a failure fails the request with nothing
    /// run, and the next request tries again), added to the run list and
    /// given one VM time slice at the raised budget, which runs its initial
    /// stream (in corpus files the `start` body); the requested procedure is
    /// not called separately. Later requests call the cached procedure
    /// synchronously, a missing one falling back to procedure index 1.
    pub fn dispatch(&mut self, request: ScriptRequest) -> Result<DispatchResult, WorldError> {
        if !self.world.scripts_enabled {
            return Ok(DispatchResult::default());
        }
        let Some(instance) = self.world.scripts.get(&request.script).cloned() else {
            return Ok(DispatchResult::default());
        };
        // SPEC-005 §3.2: a program inside one of its own self-stopping game
        // opcodes is host-busy, and a request to it is skipped.
        if instance.initialized && self.host_busy.contains(&instance.program) {
            return Ok(DispatchResult::default());
        }
        if self.active.contains(&instance.program) && instance.program != 0 {
            self.world.request(request);
            return Ok(DispatchResult {
                pending: true,
                ..Default::default()
            });
        }
        // SPEC-005 §3.2: a program that stopped itself runs nothing, but
        // the request still stores its action number and clears the
        // override flag, leaving the result: default actions proceed.
        if instance.initialized
            && self
                .programs
                .get(&instance.program)
                .is_some_and(Vm::self_stopped)
        {
            let script = self.world.scripts.get_mut(&instance.id).expect("script");
            script.overridden = false;
            script.event = request.event.code();
            script.fixed = request.fixed;
            script.action = request.action;
            return Ok(DispatchResult {
                overridden: false,
                result: script.result,
                pending: false,
            });
        }
        // SPEC-006 §3.7: a program a VM error stopped, or one suspended on
        // another program or a blocking library call, is skipped with no
        // effect; one whose root run simply ended is still requested.
        if instance.initialized
            && self.programs.get(&instance.program).is_some_and(|vm| {
                vm.status.0
                    & (VmStatus::ERROR
                        | VmStatus::WAITING
                        | VmStatus::ATTACHED
                        | VmStatus::SPAWN_BLOCKED)
                    != 0
            })
        {
            return Ok(DispatchResult::default());
        }
        // Map broadcasts test for a present procedure before requesting it.
        let procedure = if instance.initialized {
            let vm = self
                .programs
                .get(&instance.program)
                .ok_or_else(|| WorldError::Invalid("script program unavailable".into()))?;
            let procedure = vm.program.find_procedure(request.event.name(), true);
            if request.only_present && procedure.is_none() {
                return Ok(DispatchResult::default());
            }
            procedure
        } else {
            None
        };
        let target = if request.target == 0 {
            instance.owner
        } else {
            request.target
        };
        let script = self.world.scripts.get_mut(&instance.id).expect("script");
        script.overridden = false;
        script.event = request.event.code();
        script.source = request.source;
        script.target = target;
        script.fixed = request.fixed;
        script.action = request.action;
        let first = if instance.initialized {
            None
        } else {
            match self.assets.program(instance.file) {
                Ok(loaded) => Some(loaded),
                Err(error) => {
                    if let Some(script) = self.world.scripts.get_mut(&instance.id) {
                        script.source = 0;
                    }
                    return Err(error);
                }
            }
        };
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
        let pending = if let Some((filename, program)) = first {
            let mut vm = Vm::new(program);
            reserve_null_string(&mut vm)?;
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
            let id = vm.id;
            self.programs.insert(id, vm);
            // SPEC-006 §3.5 first request, item 4: one time slice for the
            // whole run list at the raised budget; the rest of an unfinished
            // first run continues in later slices at the normal budget.
            self.time_slice(FIRST_REQUEST_BUDGET);
            self.programs.get(&id).is_some_and(|vm| {
                vm.status.0 & (VmStatus::WAITING | VmStatus::ATTACHED | VmStatus::SPAWN_BLOCKED)
                    != 0
                    && !vm.self_stopped()
            })
        } else {
            let mut vm = self
                .programs
                .remove(&instance.program)
                .ok_or_else(|| WorldError::Invalid("script program unavailable".into()))?;
            let run = vm
                .prepare_host_call(procedure.filter(|index| *index != 0).unwrap_or(1), true)
                .and_then(|_| self.run_vm(&mut vm));
            // SPEC-005 §3.2: a run that stopped itself returns as if the
            // procedure had ended.
            let pending =
                matches!(run, Ok(RunOutcome::Blocked | RunOutcome::Waiting)) && !vm.self_stopped();
            if let Err(error) = run {
                self.world
                    .diagnostic(format!("script {}: {error}", instance.file));
            }
            self.programs.insert(vm.id, vm);
            pending
        };
        // SPEC-005 §3.2: an instance its own opcode removed returns what it
        // held before that opcode.
        let departed = self.departed.remove(&instance.id);
        let result = match self.world.scripts.get_mut(&instance.id) {
            Some(script) => {
                script.source = 0;
                DispatchResult {
                    overridden: script.overridden,
                    result: script.result,
                    pending,
                }
            }
            None => departed.map_or(DispatchResult::default(), |(overridden, result)| {
                DispatchResult {
                    overridden,
                    result,
                    pending,
                }
            }),
        };
        Ok(result)
    }
    fn run_vm(&mut self, vm: &mut Vm) -> Result<RunOutcome, VmError> {
        self.active.insert(vm.id);
        let mut outcome = Err(VmError::InstructionLimit);
        if self.vm_observer.is_some() {
            // The same total budget, one observed instruction at a time.
            outcome = self.observed_run(vm, u32::MAX, 1_000_000);
        } else {
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
        self.active.remove(&vm.id);
        if let Err(error) = &outcome {
            self.program_faulted(vm, error);
        }
        outcome
    }
    /// Run `vm` for `budget` counted instructions (critical ones do not
    /// count), one instruction at a time while an observer is attached,
    /// reporting each step to it; at most `limit` observed steps.
    fn observed_run(
        &mut self,
        vm: &mut Vm,
        budget: u32,
        limit: u32,
    ) -> Result<RunOutcome, VmError> {
        let Some(observer) = self.vm_observer.clone() else {
            return vm.run(self, budget);
        };
        let lock = || {
            observer
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
        };
        let mut counted = 0;
        for _ in 0..limit {
            let critical = vm.status.contains(VmStatus::CRITICAL);
            if counted >= budget && !critical {
                return Ok(RunOutcome::Budget);
            }
            lock().before_step(vm);
            let result = vm.step(self);
            lock().after_step(vm, &result);
            match result {
                Ok(RunOutcome::Budget) => {
                    if !critical {
                        counted += 1;
                    }
                }
                result => return result,
            }
        }
        Err(VmError::InstructionLimit)
    }
    /// A program's run ended in an error. The program is faulted (ended with
    /// the error bit), and the time slice skips a faulted program, so a run
    /// that exhausted the host's instruction budget is never resumed
    /// mid-procedure or revived by a timed or conditional procedure: the
    /// budget is the `[May-improve]` safe bound on a runaway script (SPEC-004
    /// §3.3 makes a budget boundary alone no termination, so the choice is
    /// the fault, not a pending request). A registration the program left
    /// open is dropped (SPEC-005 0x810E).
    fn program_faulted(&mut self, vm: &mut Vm, error: &VmError) {
        vm.status.0 |= VmStatus::ENDED | VmStatus::ERROR;
        if self.building_program == Some(vm.id) {
            self.building_program = None;
            self.building_animation = None;
            self.world.diagnostic(format!(
                "program {}: open animation registration dropped ({error})",
                vm.id
            ));
        }
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
    /// End the talking-head speech normally at once (SPEC-010 §3.12): release
    /// it, show speech frame zero, enter the waiting state with the idle
    /// counter at 3. The head updater does this itself when the audio
    /// finishes or the final marker passes.
    pub fn speech_finished(&mut self) {
        if self.world.dialogue.stop_speech()
            && let Some(head) = &mut self.world.dialogue.head
        {
            head.speech_end();
        }
    }
    /// SPEC-006 §3.11: draw the speech frame for phoneme index 0-41 from
    /// the mood's speech art, as lip-sync (SPEC-010 §3.12) does on each
    /// phoneme change. Returns whether a frame was drawn; outside speech
    /// nothing changes, and an index past the table is diagnosed and
    /// ignored (safe bound).
    pub fn speech_phoneme(&mut self, index: usize) -> bool {
        if self.world.dialogue.speech.is_none() {
            return false;
        }
        let result = match &mut self.world.dialogue.head {
            Some(head) => head.phoneme(index),
            None => return false,
        };
        match result {
            Ok(()) => true,
            Err(error) => {
                self.world.diagnostic(format!("speech phoneme: {error}"));
                false
            }
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
    /// SPEC-013 §3.22: the town or world map closed (a location was
    /// entered). When a transition to map -1 or -2 opened it, the pending
    /// request, such as one from an exit grid at the entered map's
    /// entrance, is cleared; after a world map a script opened it stays
    /// pending for the next main-loop pass.
    pub fn world_map_closed(&mut self) {
        if std::mem::take(&mut self.transition_world_map) {
            self.world.pending_transition = None;
        }
    }
    /// SPEC-006 §3.7 the critter-script cycle step, one per background
    /// pass. Nothing happens in combat or during dialogue. Otherwise N is
    /// the number of records in the critter stream (every elevation, dead
    /// or hidden owners included); the program-wide counter advances and
    /// wraps to 0 at N or more; and with N above 0 the record at the
    /// counter's position in list order gets a `critter_p_proc` request
    /// through the generic dispatcher, by identifier (an inert duplicate
    /// gives its turn to the record it names, SPEC-003 §3.3). A script
    /// without the procedure runs procedure-table index 1; one whose file
    /// cannot be loaded does nothing (the map load already reported it).
    pub fn critter_cycle_step(&mut self) -> Result<(), WorldError> {
        if self.world.combat || self.world.dialogue.open {
            return Ok(());
        }
        let critters = self
            .world
            .script_order
            .iter()
            .filter(|slot| slot.stream == 4);
        let count = critters.clone().count();
        self.critter_cycle = self.critter_cycle.saturating_add(1);
        if self.critter_cycle >= count {
            self.critter_cycle = 0;
        }
        let Some(script) = critters.map(|slot| slot.id).nth(self.critter_cycle) else {
            return Ok(());
        };
        let request = ScriptRequest {
            script,
            event: ScriptEvent::Critter,
            source: 0,
            target: 0,
            fixed: 0,
            action: -1,
            only_present: false,
        };
        // An active program defers the request to the drain below.
        let _ = self.dispatch(request);
        self.drain_requests()
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
                // SPEC-013 §3.20 / SPEC-012 §3.12 step 1: a movie already
                // playing skips the whole handler, including the next
                // midnight.
                if self.library.movie_playing || self.countdown_movie {
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
    /// SPEC-006 §3.7 script-task step 1: the VM's time slice at the normal
    /// budget, then the requests it deferred.
    pub fn update_programs(&mut self) -> Result<(), WorldError> {
        self.time_slice(10);
        self.drain_requests()
    }
    /// One SPEC-004 VM time slice (SPEC-006 §3.5 first request, item 4):
    /// every program in the run list, newest first, runs `budget`
    /// instructions (instructions executed while critical do not count);
    /// then every program's timed and conditional procedures are checked
    /// (firing at most one each, as a deferred call that runs in a later
    /// slice); then the library's named-event callbacks are collected.
    /// Programs a VM error ended (SPEC-004 §3.8), programs blocked on a
    /// child, and programs already running are skipped; a waiting program
    /// only has its wait checked.
    fn time_slice(&mut self, budget: u32) {
        let ids: Vec<_> = self.programs.keys().rev().copied().collect();
        let skipped = |vm: &Vm| {
            vm.status
                .contains(VmStatus::ERROR | VmStatus::ATTACHED | VmStatus::SPAWN_BLOCKED)
        };
        for id in &ids {
            if self.active.contains(id) {
                continue;
            }
            let Some(mut vm) = self.programs.remove(id) else {
                continue;
            };
            if !skipped(&vm) {
                self.active.insert(*id);
                let limit = vm.hard_instruction_limit;
                let result = self.observed_run(&mut vm, budget, limit);
                self.active.remove(id);
                if let Err(error) = result {
                    self.program_faulted(&mut vm, &error);
                    self.world.diagnostic(format!("program {id}: {error}"));
                }
            }
            self.programs.insert(*id, vm);
        }
        for id in &ids {
            if self.active.contains(id) {
                continue;
            }
            let Some(mut vm) = self.programs.remove(id) else {
                continue;
            };
            if !skipped(&vm) && !vm.status.contains(VmStatus::WAITING) {
                self.active.insert(*id);
                let result = vm.poll_scheduled(self, u32::MAX);
                self.active.remove(id);
                if let Err(error) = result {
                    self.program_faulted(&mut vm, &error);
                    self.world.diagnostic(format!("program {id}: {error}"));
                }
            }
            self.programs.insert(*id, vm);
        }
        self.callbacks.extend(self.library.callbacks());
    }
    pub fn tick(&mut self, now_ms: u64) -> Result<bool, WorldError> {
        self.world.elapsed_ms = now_ms;
        // SPEC-014 §3.28: animation resumes where a pause left it.
        self.resume_background(now_ms);
        // Asset load problems that did not stop a load (SPEC-002 §3.5).
        for message in self.assets.take_diagnostics() {
            self.world.diagnostic(message);
        }
        // SPEC-006 §3.7: one background pass per call. The tasks run
        // newest-registered-first: the idle-fidget step, object animation,
        // then the script task.
        let mut changed_by_animation = false;
        // SPEC-007 §3.11.2 the idle-fidget background task.
        if !self.world.dialogue.open && self.idle_fidget(now_ms)? {
            changed_by_animation = true;
        }
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
        if self.step_world_motions(now_ms) {
            changed_by_animation = true;
            self.drain_requests()?;
        }
        // The script task: the VM's time slice, the script-created windows,
        // and, with scripts running, critter scripts enabled (map background
        // processing on) and no dialogue, one critter-script cycle step and
        // then the background clock checker.
        self.update_programs()?;
        if self.world.scripts_enabled && self.world.background_enabled && !self.world.dialogue.open
        {
            self.critter_cycle_step()?;
            let background = self.world.clock.background(
                now_ms,
                self.world.combat,
                self.world.dialogue.open,
                self.world.mode,
            )?;
            if background.queue {
                self.process_queue()?;
            }
            if background.update {
                self.map_update()?;
            }
        }
        self.drain_requests()?;
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
        let mut changed = changed_by_animation;
        // SPEC-006 §3.11 / SPEC-010 §3.12 the talking-head updater.
        changed |= self.update_head(now_ms)?;
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
            // A callback whose program is gone or whose procedure index (a
            // script-supplied `addkey` value) is bad is reported like a
            // failed program update, not a failed frame.
            if let Err(error) = self.callback(program, procedure) {
                self.world
                    .diagnostic(format!("callback {program}/{procedure}: {error}"));
            }
            self.library.release_named(program, procedure);
            changed = true;
        }
        // SPEC-005 0x80F9: a script's dialogue-entry request.
        if self.service_dialogue_request()? {
            changed = true;
        }
        // SPEC-013 §3.22: a pending transition runs in the main loop; one
        // requested during a dialogue waits for the dialogue to close.
        if !self.world.dialogue.open && self.process_transition()? {
            changed = true;
        }
        // SPEC-013 §3.10: the request pass services a scripted explosion.
        if self.service_explosion_request()? {
            changed = true;
        }
        self.sync_combat_lock();
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
        // SPEC-005 §3.2: a program that stopped itself runs nothing more.
        if self.programs.get(&program).is_some_and(Vm::self_stopped) {
            return Ok(RunOutcome::Stopped);
        }
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
            // A missing message reads `Error` (SPEC-006 §3.9).
            DialogueText::Message { list, message } if *list > 0 => {
                Ok(self.script_message(*list, *message).map_or_else(
                    || (b"Error".to_vec(), Vec::new()),
                    |value| (value.text, value.sound),
                ))
            }
            // SPEC-005 §3.3 [May-improve]: any other nonpositive list
            // number is missing text, which reads `Error` like a missing
            // message.
            DialogueText::Message { .. } => Ok((b"Error".to_vec(), Vec::new())),
        }
    }
    pub(crate) fn realize_dialogue(&mut self) -> Result<(), WorldError> {
        let reply = self
            .world
            .dialogue
            .reply
            .clone()
            .ok_or_else(|| WorldError::Invalid("no dialogue reply".into()))?;
        // SPEC-006 §3.9: a message-backed reply goes through the
        // speech-enabled lookup at display time, so its speech starts before
        // the reply text is drawn and before the next head updater tick
        // (SPEC-010 §3.12). Literal replies make no request.
        let mut text = match reply {
            DialogueText::Message { list, message } if list > 0 => self
                .spoken_message(list, message)
                .map_or_else(|| b"Error".to_vec(), |value| value.text),
            other => self.text_resource(&other, false, false)?.0,
        };
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
        } else if self.world.dialogue.open {
            // SPEC-006 §3.11: no session-start draw while a session is open;
            // the open session keeps its head.
            self.world.dialogue.head.take()
        } else {
            // SPEC-005 §3.3 safe rule: with no dialogue target yet, the mood
            // comes from the new self target.
            let source = match self.world.dialogue.target {
                0 => owner,
                target => target,
            };
            let mood = Mood::from_reaction(self.reaction(source));
            let idle = self.world.dialogue.idle;
            // SPEC-006 §3.11 safe rule: a head outside the list selects no
            // fidget art and is diagnosed; the session still opens, without
            // the session-start draw because no count was looked up.
            match self.assets.head(head) {
                Ok(definition) => Some(TalkingHead::new(
                    definition,
                    mood,
                    idle,
                    &mut self.world.rng,
                )?),
                Err(error) => {
                    self.world
                        .diagnostic(format!("head {head} is unavailable: {error}"));
                    Some(TalkingHead::without_fidget(
                        HeadDefinition {
                            index: (head & 0xfff) as u16,
                            stem: String::new(),
                            counts: [0; 3],
                        },
                        mood,
                        idle,
                    ))
                }
            }
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
        // SPEC-014 §3.46: starting a dialogue removes every floating text.
        self.world.floating.clear();
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
            // SPEC-007 §3.11.3, the talk exception: after the approach, a
            // target still at object distance 9 or more, or behind a
            // shot-blocked line, tells the player `misc.msg` 2000 ("You
            // cannot get there.") and the talk ends.
            if far || blocked {
                self.print_game("misc.msg", 2000, &[]);
                return Ok(());
            }
            return self.enter_dialogue(actor, target);
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
    /// SPEC-006 §3.8 dialogue entry with `target`: refused in combat or
    /// for a target without a script; an ordinary target must also be
    /// within tile distance 12 of `actor` with a reachability path. Map
    /// background activity is suspended, the session target is set and
    /// the talk event is requested from the target's script; when no
    /// dialogue opened, background activity resumes and the map updates.
    fn enter_dialogue(&mut self, actor: ObjectRef, target: ObjectRef) -> Result<(), WorldError> {
        if self.world.combat {
            return Err(WorldError::Invalid(
                "dialogue entry refused in combat".into(),
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
        // SPEC-006 §3.9: the talk session runs from here, before
        // `talk_p_proc`, until the talk returns: at once when no
        // dialogue opened, else when the dialogue closes.
        self.world.dialogue.talking = true;
        let dispatched = self.dispatch(ScriptRequest {
            script: sid,
            event: ScriptEvent::Talk,
            source: 0,
            target: 0,
            fixed: 0,
            action: -1,
            only_present: false,
        });
        if !self.world.dialogue.open {
            self.world.dialogue.talking = false;
        }
        dispatched?;
        if !self.world.dialogue.open {
            self.world.background_enabled = true;
            self.map_update()?;
        }
        Ok(())
    }
    /// SPEC-005 0x80F9 and §3.3: the dialogue game state a script requested
    /// with `dialogue_system_enter` is entered here, outside the requesting
    /// program, through the talk entry of the player's talk action. A
    /// request made stale by combat, an open dialogue or the target's death
    /// is dropped; a refused entry is diagnosed. Returns whether a dialogue
    /// entry ran.
    pub(crate) fn service_dialogue_request(&mut self) -> Result<bool, WorldError> {
        let Some(target) = self.pending_dialogue.take() else {
            return Ok(false);
        };
        if self.world.combat
            || self.world.dialogue.open
            || self.world.get(target).is_none_or(|object| !object.alive())
        {
            return Ok(false);
        }
        let player = self.world.player;
        if let Err(error) = self.enter_dialogue(player, target) {
            self.world
                .diagnostic(format!("dialogue_system_enter: {error}"));
        }
        Ok(true)
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
                // SPEC-006 §3.8 step 5: result 1 destroys the item, one
                // unit of a stack.
                if retained == 1 {
                    self.world.destroy_unit(item)?;
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
            self.world.destroy_unit(item)?;
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
    /// combat to the actor's AP plus free movement. Returns whether the walk
    /// registered; its route is searched only as it starts, and without one
    /// the walker stops standing.
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
    /// The enhanced opcode range follows the preset: Vanilla rejects those
    /// words exactly as unknown words.
    fn enhanced_opcodes(&self) -> bool {
        self.preset() == crate::EnginePreset::Enhanced
    }
    /// The enhanced camera requests (`0x8210`-`0x8213`) leave the world as
    /// [`WorldEffect::Camera`] effects; a refused one gives a diagnostic
    /// instead and the script goes on. Other enhanced words run the
    /// engine's own table.
    fn enhanced_extension(&mut self, opcode: u16, vm: &mut Vm) -> Result<(), VmError> {
        if !crate::camera::CAMERA_WORDS.contains(&opcode) {
            return extensions::execute_enhanced(vm, opcode);
        }
        let call = extensions::decode_enhanced(vm, opcode)?;
        if self.calls.len() < 100_000 {
            self.calls.push(CallTrace {
                opcode,
                operands: call.args.iter().map(|arg| arg.value).collect(),
            });
        }
        match crate::camera::CameraCommand::from_call(&call) {
            Ok(command) => self.world.emit(WorldEffect::Camera(command)),
            Err(message) => self.world.diagnostic(message),
        }
        Ok(())
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
    fn remove_procedures(&mut self, owner: u32) -> Result<(), VmError> {
        self.registry.remove_procedures(owner)
    }
    fn observe_before(&mut self, vm: &Vm) {
        if let Some(observer) = &self.vm_observer {
            observer
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .before_step(vm);
        }
    }
    fn observe_after(&mut self, vm: &Vm, result: &Result<RunOutcome, VmError>) {
        if let Some(observer) = &self.vm_observer {
            observer
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .after_step(vm, result);
        }
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
        reserve_null_string(&mut vm)?;
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
