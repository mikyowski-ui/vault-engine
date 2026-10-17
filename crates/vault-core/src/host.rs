use crate::{
    ObjectAction, ObjectRef, Perk, ScriptEvent, WorldEffect, WorldError, WorldRules, WorldState,
    assets::WorldAssets,
    dialogue::{DialogueText, Mood, TalkingHead, npc_reaction},
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
    BasicHost, ExportedProcedure, ExternalValue, RunOutcome, Value, Vm, VmError, VmHost,
    extensions::{self, PushKind},
};

pub(crate) type HostResult = Result<Option<ExternalValue>, VmError>;
#[derive(Debug, Clone)]
pub struct CallTrace {
    pub opcode: u16,
    pub operands: Vec<Value>,
}
#[derive(Debug, Clone, Default)]
pub struct AnimationSequence {
    pub flags: i32,
    pub commands: Vec<(u16, Vec<i32>)>,
}
#[derive(Debug, Clone, Copy, Default)]
pub struct DispatchResult {
    pub overridden: bool,
    pub result: i32,
    pub pending: bool,
}
struct Interaction {
    actor: ObjectRef,
    target: ObjectRef,
    action: ObjectAction,
    path: VecDeque<i32>,
    next_ms: u64,
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
    pub(crate) callbacks: VecDeque<(u32, u32)>,
    program_scripts: BTreeMap<u32, u32>,
    active: BTreeSet<u32>,
    next_program: u32,
    pub building_animation: Option<AnimationSequence>,
    pub animations: Vec<AnimationSequence>,
    pub last_object_name: Vec<u8>,
    pub floating_style: i32,
    dialogue_waiters: Vec<u32>,
    dialogue_callbacks: Vec<u32>,
    host_text: BTreeMap<u32, Vec<u8>>,
    interactions: VecDeque<Interaction>,
    presentation: Option<PresentationState>,
}
impl<T: WorldAssets + ?Sized> WorldAssets for Box<T> {
    fn prototype(&self, pid: u32) -> Result<std::sync::Arc<vault_formats::Prototype>, WorldError> {
        (**self).prototype(pid)
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
    fn available(&self, path: &[u8]) -> bool {
        (**self).available(path)
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
    fn sound_name(&self, opcode: u16, args: &[i32]) -> Result<Vec<u8>, WorldError> {
        (**self).sound_name(opcode, args)
    }
    fn object_art(
        &self,
        category: Category,
        fid: u32,
        orientation: u8,
    ) -> Result<std::sync::Arc<vault_formats::FrmArchive>, WorldError> {
        (**self).object_art(category, fid, orientation)
    }
    fn font_number(&self, number: i32) -> Option<std::sync::Arc<vault_formats::Font>> {
        (**self).font_number(number)
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
            callbacks: VecDeque::new(),
            program_scripts: BTreeMap::new(),
            active: BTreeSet::new(),
            next_program: 1,
            building_animation: None,
            animations: Vec::new(),
            last_object_name: b"<Critter>".to_vec(),
            floating_style: 1,
            dialogue_waiters: Vec::new(),
            dialogue_callbacks: Vec::new(),
            host_text: BTreeMap::new(),
            interactions: VecDeque::new(),
            presentation: None,
        }
    }
    pub fn rules(&mut self, rules: WorldRules) {
        self.world.rules = rules;
    }
    pub(crate) fn opaque_text(&mut self, text: Vec<u8>) -> Result<u32, VmError> {
        if self.host_text.len() >= 65536 || text.len() > 1023 {
            return Err(VmError::Host("host text registry limit".into()));
        }
        let reference = 0x40000000 + self.host_text.len() as u32;
        self.host_text.insert(reference, text);
        Ok(reference)
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
    pub fn load_campaign(&mut self) -> Result<(), WorldError> {
        self.world.trace.push(WorldTrace::InventoryRefresh);
        if let Some(gam) = self.assets.gam("data/vault13.gam", GamSection::Campaign)? {
            self.world.gvars = Some(gam.values);
        }
        Ok(())
    }
    pub fn load_map(&mut self, path: &str) -> Result<(), WorldError> {
        self.world.trace.push(WorldTrace::LoadStep(1));
        self.world.map_exit_cleanup(|_| true);
        if let Some(script) = self.world.map_script {
            self.dispatch(ScriptRequest {
                script,
                event: ScriptEvent::MapExit,
                source: 0,
                target: 0,
                fixed: 0,
                action: -1,
                only_present: false,
            })?;
        }
        self.world.map.header.flags.0 |= 1;
        self.world.map.header.last_visit = self.world.clock.ticks as u32;
        self.animations.clear();
        self.interactions.clear();
        self.building_animation = None;
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
        self.world.trace.push(WorldTrace::LoadStep(2));
        let (map, load_error) = match self.assets.map(path) {
            Ok(map) => (map, None),
            Err(error) => (empty_map(), Some(error)),
        };
        self.world.restoring = map.header.flags.0 & 1 != 0;
        self.world.trace.push(WorldTrace::LoadStep(3));
        self.world.import_map(map, &self.assets)?;
        self.world.trace.push(WorldTrace::LoadStep(4));
        self.world.elevation = self.world.map.header.entry_elevation;
        self.world.camera_hex = self.world.map.header.entry_hex;
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
            self.dispatch(ScriptRequest {
                script: sid,
                event: ScriptEvent::MapEnter,
                source: 0,
                target: 0,
                fixed,
                action: -1,
                only_present: false,
            })?;
            self.world.spatial_enabled = true;
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
            self.dispatch(ScriptRequest {
                script,
                event: ScriptEvent::Start,
                source: 0,
                target: 0,
                fixed: 0,
                action: -1,
                only_present: false,
            })?;
        }
        self.world.spatial_enabled = false;
        for script in self.world.script_order.clone() {
            if Some(script) != self.world.map_script {
                self.dispatch(ScriptRequest {
                    script,
                    event: ScriptEvent::MapEnter,
                    source: 0,
                    target: 0,
                    fixed,
                    action: -1,
                    only_present: true,
                })?;
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
        self.drain_requests()?;
        if let Some(error) = load_error {
            self.world.diagnostic(error.to_string());
            return Err(error);
        }
        Ok(())
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
        for _ in 0..1000 {
            match vm.run(self, 1000) {
                Ok(RunOutcome::Budget) => {}
                result => {
                    outcome = result;
                    break;
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
    pub fn map_update(&mut self) -> Result<(), WorldError> {
        let previous = self.world.spatial_enabled;
        self.world.spatial_enabled = false;
        self.world.broadcast(ScriptEvent::MapUpdate, 0);
        let result = self.drain_requests();
        self.world.spatial_enabled = previous;
        result
    }
    pub fn process_queue(&mut self) -> Result<(), WorldError> {
        let snapshot = self.world.clock.ticks;
        let mut count = 0;
        while let Some(event) = self.world.queue.pop_due(snapshot) {
            count += 1;
            if count > 100_000 {
                return Err(WorldError::Invalid("queue dispatch limit exceeded".into()));
            }
            self.world.trace.push(WorldTrace::QueueFired(event.id));
            match event.kind {
                EventKind::Script { script, parameter } => {
                    if self.world.scripts.contains_key(&script) {
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
                    }
                }
                EventKind::MapUpdate => {
                    self.map_update()?;
                    self.world
                        .queue
                        .retain(|event| event.kind != EventKind::MapUpdate);
                    self.world.queue.insert(
                        self.world.clock.ticks,
                        600,
                        0,
                        EventKind::MapUpdate,
                    )?;
                }
                EventKind::Midnight => {
                    let refs: Vec<_> = self.world.iter().map(|(r, _)| r).collect();
                    for reference in refs {
                        if let Some(object) = self.world.get_mut(reference)
                            && let Some(mut lock) = object.lock()
                        {
                            lock.0 &= !LockFlags::JAMMED.0;
                            object.set_lock(lock);
                        }
                    }
                    self.world.queue.retain(|event| {
                        !matches!(event.kind, EventKind::MapUpdate | EventKind::Midnight)
                    });
                    self.world.enqueue_map_timers()?;
                }
                EventKind::Poison | EventKind::Radiation | EventKind::Other(_) => self
                    .world
                    .diagnostic(format!("timed maintenance event {:?}", event.kind)),
            }
        }
        Ok(())
    }
    pub fn advance_time(&mut self, ticks: i32) -> Result<(), WorldError> {
        let mut ticks = i64::from(ticks);
        if ticks < 0 {
            return Err(WorldError::Invalid(
                "negative explicit time advance is undefined".into(),
            ));
        }
        while ticks >= 864000 {
            self.world.clock.advance(864000)?;
            self.process_queue()?;
            ticks -= 864000;
        }
        self.world.clock.advance(ticks)?;
        self.process_queue()
    }
    pub fn tick(&mut self, now_ms: u64) -> Result<bool, WorldError> {
        self.world.elapsed_ms = now_ms;
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
        let mut changed = false;
        if !self.world.dialogue.open
            && let Some(mut interaction) = self.interactions.pop_front()
        {
            if now_ms >= interaction.next_ms {
                let next = interaction.path.pop_front();
                if let Some(hex) = next {
                    let elevation = self
                        .world
                        .get(interaction.actor)
                        .map_or(self.world.elevation, |object| object.raw.elevation);
                    if self.world.blocked(hex, elevation, interaction.actor) {
                        self.world.diagnostic("interaction path became blocked");
                    } else {
                        self.world
                            .move_object(interaction.actor, hex, elevation, true)?;
                        self.drain_requests()?;
                        changed = true;
                        if interaction.path.is_empty() {
                            if let Err(error) = self.action(
                                interaction.actor,
                                interaction.target,
                                interaction.action,
                            ) {
                                self.world.diagnostic(error.to_string());
                            }
                        } else {
                            interaction.next_ms = now_ms + 100;
                            self.interactions.push_front(interaction);
                        }
                    }
                }
            } else {
                self.interactions.push_front(interaction);
            }
        }
        if let Some(head) = &mut self.world.dialogue.head {
            let art = self.assets.frame(&head.path()?)?;
            changed = head.tick(now_ms, art.frame_count(), art.fps, &mut self.world.rng)?;
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
            let (text, _) = self.text_resource(&option.text, true, index == 0)?;
            if text.len() > 897 {
                return Err(WorldError::Invalid(
                    "resolved option exceeds 897 bytes".into(),
                ));
            }
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
    pub(crate) fn reaction(&mut self, object: ObjectRef) -> i32 {
        let Some(sid) = self.world.script_for(object) else {
            return -1;
        };
        let Ok(base) = self.world.ensure_locals(sid) else {
            return -1;
        };
        let charisma = self.world.player_stat(3);
        let presence = self.world.perk(Perk::Presence);
        let cult = self.world.perk(Perk::CultOfPersonality) > 0;
        let reputation = self.world.reputation;
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
            if self.world.dialogue.target == 0 {
                return Err(WorldError::Invalid(
                    "head reaction requires the previous dialogue target".into(),
                ));
            }
            let mood = Mood::from_reaction(self.reaction(self.world.dialogue.target));
            Some(TalkingHead::new(
                self.assets.head(head)?,
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
        let container_locked = target_category == Category::Item
            && object
                .lock()
                .is_some_and(|lock| lock.contains(LockFlags::LOCKED));
        if self.world.get(actor).is_none_or(|o| !o.alive()) {
            return Err(WorldError::Invalid("actor is inactive/dead".into()));
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
        let talkable = object.category() == Category::Critter
            || object.prototype.as_ref().is_some_and(|p| p.permits_talk());
        if action == ObjectAction::Talk {
            if actor != self.world.player || !talkable || self.world.combat {
                return Err(WorldError::Invalid(
                    "talk requires the player, a talkable target, and no combat".into(),
                ));
            }
            if self.world.distance(actor, target) >= 9 {
                return self.approach(actor, target, action, 8);
            }
            if !self.world.line_clear(actor, target, false) {
                return Err(WorldError::Invalid(
                    "talk requires an unobstructed line".into(),
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
        if action == ObjectAction::Use
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
        if let ObjectAction::UseSkill(skill) = action {
            if skill == 8 {
                if let Some(player) = self.world.player_sheet_mut() {
                    player.flags ^= 1;
                }
                return Ok(());
            }
            let category = object.category();
            if !(0..18).contains(&skill)
                || self.world.combat && matches!(skill, 6 | 7 | 9 | 10 | 11 | 12 | 13)
                || matches!(skill, 6 | 7) && category != Category::Critter
                || skill == 9 && !matches!(category, Category::Item | Category::Scenery)
                || skill == 10 && actor == target
                || skill == 11 && category == Category::Critter
            {
                return Err(WorldError::Invalid(
                    "skill target/combat restriction".into(),
                ));
            }
            if object
                .lock()
                .is_some_and(|lock| lock.contains(LockFlags::JAMMED))
            {
                return Err(WorldError::Invalid("lock is jammed".into()));
            }
            if matches!(skill, 12 | 13)
                && object.character.as_ref().is_some_and(|sheet| {
                    sheet.body != 2 && !(skill == 12 && sheet.kill_category == 5)
                })
            {
                return Err(WorldError::Invalid(
                    "science/repair requires a robotic target (except Science on brahmin)".into(),
                ));
            }
        }
        if action==ObjectAction::Pickup
            && object.prototype.as_ref().is_some_and(|p|matches!(&p.data,PrototypeData::Item(item) if matches!(item.data,ItemData::Container{..}))&&!p.extended.contains(vault_formats::pro::ExtendedFlags::PICKUP_CONTAINER)) {
            self.world.emit(WorldEffect::DefaultAction {action:ObjectAction::Use,target});return Ok(());
        }
        if matches!(
            action,
            ObjectAction::Pickup | ObjectAction::Use | ObjectAction::UseSkill(_)
        ) && self.world.distance(actor, target) > 1
        {
            return self.approach(actor, target, action, 1);
        }
        if matches!(action, ObjectAction::Pickup | ObjectAction::Use) && self.world.combat {
            let actor = self.world.get_mut(actor).expect("actor");
            if let ObjectState::Critter(state) = &mut actor.raw.state {
                if state.ap < 3 {
                    return Err(WorldError::Invalid("interaction requires 3 AP".into()));
                }
                state.ap -= 3;
            }
        }
        if action == ObjectAction::Use && container_locked {
            return Err(WorldError::Invalid("container is locked".into()));
        }
        let event = match action {
            ObjectAction::LookAt => ScriptEvent::LookAt,
            ObjectAction::Description => ScriptEvent::Description,
            ObjectAction::Use => ScriptEvent::Use,
            ObjectAction::Pickup => ScriptEvent::Pickup,
            ObjectAction::UseSkill(_) => ScriptEvent::UseSkill,
            _ => unreachable!(),
        };
        let result = if let Some(script) = self.world.script_for(target) {
            self.dispatch(ScriptRequest {
                script,
                event,
                source: actor,
                target,
                fixed: 0,
                action: if let ObjectAction::UseSkill(skill) = action {
                    skill
                } else {
                    -1
                },
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
                ObjectAction::LookAt => {
                    let object = self.world.get(target).expect("target");
                    let text = if object
                        .character
                        .as_ref()
                        .is_some_and(|sheet| sheet.current_hp <= 0)
                    {
                        [b"Dead ".as_slice(), object.name.as_slice()].concat()
                    } else {
                        object.name.clone()
                    };
                    self.world.emit(WorldEffect::Message(text));
                }
                ObjectAction::Description => self.world.emit(WorldEffect::Message(
                    self.world.get(target).expect("target").description.clone(),
                )),
                ObjectAction::Pickup => {
                    let amount = self
                        .world
                        .get(target)
                        .and_then(|o| o.money)
                        .map_or(1, |n| n.max(1));
                    self.world.add_item(actor, target, amount, false)?;
                    if let Some(object) = self.world.get_mut(target)
                        && object.money.is_some()
                    {
                        object.money = Some(0);
                    }
                    self.world.rebuild_lighting();
                }
                ObjectAction::Use => {
                    let object = self.world.get_mut(target).expect("target");
                    if object.category() == Category::Scenery
                        && let Some(mut lock) = object.lock()
                    {
                        if lock.contains(LockFlags::LOCKED) {
                            return Err(WorldError::Invalid("door/container is locked".into()));
                        }
                        lock.0 ^= 1;
                        object.raw.frame = usize::from(lock.0 & 1 != 0);
                        object.set_lock(lock);
                    }
                }
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
        if action == ObjectAction::Use {
            self.map_update()?;
        }
        self.drain_requests()
    }
    fn use_item_on(
        &mut self,
        actor: ObjectRef,
        item: ObjectRef,
        target: ObjectRef,
    ) -> Result<(), WorldError> {
        if self.world.get(item).is_none() || self.world.get(target).is_none() {
            return Err(WorldError::Invalid("missing item/target".into()));
        }
        if self.world.distance(actor, target) > 1 {
            return self.approach(actor, target, ObjectAction::UseObject(item), 1);
        }
        let pid = self.world.get(item).expect("item").raw.pid;
        let medical = if self.world.rules.doctor_bag_pid == Some(pid) {
            Some(7)
        } else if self.world.rules.first_aid_kit_pid == Some(pid) {
            Some(6)
        } else {
            None
        };
        if let Some(skill) = medical {
            if self.world.combat
                || self
                    .world
                    .get(target)
                    .is_none_or(|object| object.character.is_none())
            {
                return Err(WorldError::Invalid(
                    "medical kit requires a critter outside combat".into(),
                ));
            }
            let context = self.world.context(actor);
            let sheet = self
                .world
                .get(actor)
                .and_then(|object| object.character.as_ref())
                .ok_or_else(|| WorldError::Invalid("medical actor has no skills".into()))?;
            let chance = sheet.skill(skill, context);
            let critical = sheet.stat(15, context);
            let check = crate::rng::skill_check(
                chance,
                critical + 20,
                critical,
                self.world.clock.ticks,
                &mut self.world.rng,
            )?;
            self.world.emit(WorldEffect::Medical {
                actor,
                target,
                skill: skill as i32,
                code: check.code,
                margin: check.margin,
                kit_bonus: 20,
            });
            if crate::RandomSource::range(&mut self.world.rng, 1, 10)? == 1 {
                if let Some(owner) = self.world.get(item).and_then(|object| object.owner) {
                    self.world.remove_item(owner, item, 1)?;
                }
                self.world.erase(item);
            }
            return self.map_update();
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
    fn approach(
        &mut self,
        actor: ObjectRef,
        target: ObjectRef,
        action: ObjectAction,
        range: i32,
    ) -> Result<(), WorldError> {
        let a = self
            .world
            .get(actor)
            .ok_or_else(|| WorldError::Invalid("missing actor".into()))?;
        let b = self
            .world
            .get(target)
            .ok_or_else(|| WorldError::Invalid("missing target".into()))?;
        if a.raw.elevation != b.raw.elevation {
            return Err(WorldError::Invalid(
                "interaction is on another elevation".into(),
            ));
        }
        let (start, end) = (
            a.raw
                .hex
                .ok_or_else(|| WorldError::Invalid("unplaced actor".into()))?,
            b.raw
                .hex
                .ok_or_else(|| WorldError::Invalid("unplaced target".into()))?,
        );
        let blocked: BTreeSet<_> = self
            .world
            .iter()
            .filter(|(r, o)| {
                *r != actor
                    && o.raw.elevation == a.raw.elevation
                    && o.alive()
                    && o.raw.flags.0 & (BasicFlags::HIDDEN.0 | BasicFlags::NONBLOCKING.0) == 0
            })
            .filter_map(|(_, o)| o.raw.hex)
            .collect();
        let mut previous = vec![-1; 40000];
        previous[start as usize] = start;
        let mut queue = VecDeque::from([start]);
        let mut found = None;
        while let Some(hex) = queue.pop_front() {
            if vault_formats::HexGrid::lattice_distance(hex, end)
                .is_ok_and(|distance| distance <= range)
            {
                found = Some(hex);
                break;
            }
            for direction in 0..6 {
                if let Ok(Some(next)) = vault_formats::HexGrid::neighbor(hex, direction)
                    && previous[next as usize] == -1
                    && !blocked.contains(&next)
                {
                    previous[next as usize] = hex;
                    queue.push_back(next);
                }
            }
        }
        let mut at =
            found.ok_or_else(|| WorldError::Invalid("interaction target is unreachable".into()))?;
        let mut path = VecDeque::new();
        while at != start {
            path.push_front(at);
            at = previous[at as usize];
        }
        if path.is_empty() {
            return Err(WorldError::Invalid(
                "interaction cannot satisfy its distance rule".into(),
            ));
        }
        self.interactions.retain(|pending| pending.actor != actor);
        self.interactions.push_back(Interaction {
            actor,
            target,
            action,
            path,
            next_ms: self.world.elapsed_ms + 100,
        });
        Ok(())
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
    fn wait_ready(&mut self, reference: u32, _start: u32, _deadline: u32) -> Result<bool, VmError> {
        if reference == 2 {
            Ok(self.library.dialogue_depth > 0 || self.library.movie_playing)
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
