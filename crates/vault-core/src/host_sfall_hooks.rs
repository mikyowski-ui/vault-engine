//! SPEC-024b part 3: `hs_*.int` hook scripts and the global scripts hooks
//! depend on. A chain runs synchronously, at a boundary in the host specs,
//! and only in the Input or Simulation phase: presentation reads what a
//! preview stored (§6.6), so drawing never runs a script. An empty chain
//! costs nothing and changes nothing. With sfall mode off none of this runs.
use crate::{
    ObjectRef, RandomSource,
    assets::WorldAssets,
    barter::BarterSession,
    combat::{Attack, Resolution},
    critical::effect,
    host::{WorldHost, reserve_null_string},
    host_combat::Spray,
    host_sfall::game_mode,
};
use std::collections::{BTreeMap, BTreeSet};
use vault_vm::{
    RunOutcome, Vm, VmError, VmStatus,
    sfall::{self, HookRow, HookType, SfallValue},
};

pub const HOOK_TOHIT: i32 = 0;
pub const HOOK_AFTERHITROLL: i32 = 1;
pub const HOOK_CALCAPCOST: i32 = 2;
pub const HOOK_COMBATDAMAGE: i32 = 5;
pub const HOOK_BARTERPRICE: i32 = 10;
pub const HOOK_MOVECOST: i32 = 11;
pub const HOOK_KEYPRESS: i32 = 19;
pub const HOOK_INVENTORYMOVE: i32 = 24;
/// §6.5 past this many nested chains, a chain is skipped.
const MAX_CHAINS: usize = 8;

/// One registered procedure; `None` is the script's `start`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Member {
    program: u32,
    procedure: Option<usize>,
}
/// One hook's members (§6.3).
#[derive(Debug, Default)]
struct Chain {
    /// `register_hook` and `register_hook_proc`, in registration order; they
    /// run newest first.
    front: Vec<Member>,
    /// The `hs_<name>.int` script.
    script: Option<u32>,
    /// `register_hook_proc_spec`, oldest first.
    back: Vec<Member>,
}
impl Chain {
    fn members(&self) -> Vec<Member> {
        let script = self.script.map(|program| Member {
            program,
            procedure: None,
        });
        self.front
            .iter()
            .rev()
            .copied()
            .chain(script)
            .chain(self.back.iter().copied())
            .collect()
    }
    fn held(&mut self, program: u32) -> Option<&mut Member> {
        self.front
            .iter_mut()
            .chain(self.back.iter_mut())
            .find(|member| member.program == program)
    }
    fn remove(&mut self, program: u32) {
        self.front.retain(|member| member.program != program);
        self.back.retain(|member| member.program != program);
        if self.script == Some(program) {
            self.script = None;
        }
    }
}
/// The context of one running chain (§6.4).
#[derive(Debug)]
struct Context {
    args: Vec<SfallValue>,
    arg_types: &'static [HookType],
    rets: Vec<Option<SfallValue>>,
    ret_types: &'static [HookType],
    next_arg: usize,
    next_ret: usize,
}
#[derive(Debug)]
struct GlobalScript {
    program: u32,
    /// `set_global_script_repeat`: every n steps, 0 disabling it.
    repeat: i32,
    since: i32,
    /// `set_global_script_type`.
    kind: i32,
}
/// A stored barter preview (§6.6), and the inputs it was queried from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct BarterPreview {
    inputs: (i64, i64, i32, i64),
    price: i32,
    offer: i64,
}

/// Hooks and global scripts: rebuilt by the scripts on each setup, never
/// saved. Their programs live here, not in the map's run list: they outlive
/// a map, and keep their state between runs (§6.1).
#[derive(Debug, Default)]
pub struct SfallHooks {
    chains: BTreeMap<i32, Chain>,
    programs: BTreeMap<u32, Vm>,
    /// Every global and hook script, whether running or not.
    scripts: BTreeSet<u32>,
    globals: Vec<GlobalScript>,
    contexts: Vec<Context>,
    in_setup: bool,
    setup_due: bool,
    /// §6.13 what each held physical key produced.
    held_keys: BTreeMap<i32, i32>,
    barter: Option<BarterPreview>,
    /// §6.12: entering barter or a transaction runs the query twice.
    barter_fresh: bool,
    previews: BTreeMap<Preview, (Vec<i32>, i32)>,
}
/// The values only the interface shows (§6.6).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Preview {
    /// The AP cost on the interface bar.
    SlotCost,
    /// The to-hit percentage under the targeting cursor.
    HoverChance,
}

/// The int return at `index`, if a member set it.
fn int_return(rets: &[Option<SfallValue>], index: usize) -> Option<i32> {
    rets.get(index)?.as_ref().map(SfallValue::int)
}
fn object(reference: ObjectRef) -> SfallValue {
    SfallValue::Int(reference as i32)
}
fn int(value: i32) -> SfallValue {
    SfallValue::Int(value)
}

impl<A: WorldAssets> WorldHost<A> {
    fn hooks(&mut self) -> Option<&mut SfallHooks> {
        self.sfall.as_deref_mut().map(|sfall| &mut sfall.hooks)
    }
    /// A session setup is owed: after a new game or a slot load, and before
    /// the first Simulation phase that follows (§6.1).
    pub(crate) fn sfall_setup_owed(&mut self) {
        if let Some(hooks) = self.hooks() {
            hooks.setup_due = true;
        }
    }
    /// §6.1's setup, once it is owed.
    pub fn sfall_setup_if_due(&mut self) {
        if self.hooks().is_some_and(|hooks| hooks.setup_due) {
            self.sfall_setup();
        }
    }
    fn sfall_setup(&mut self) {
        let Some(hooks) = self.hooks() else {
            return;
        };
        *hooks = SfallHooks {
            in_setup: true,
            ..Default::default()
        };
        // Step 2: each hook ascending, a shared file loading once.
        let mut loaded: BTreeMap<&str, Option<u32>> = BTreeMap::new();
        for row in &sfall::SFALL_HOOKS {
            let program = match loaded.get(row.file) {
                Some(program) => *program,
                None => {
                    let program = self.load_sfall_script(row.file.as_bytes(), false);
                    loaded.insert(row.file, program);
                    program
                }
            };
            if let (Some(program), Some(hooks)) = (program, self.hooks()) {
                hooks.chains.entry(row.id).or_default().script = Some(program);
            }
        }
        // Step 3: the global scripts, ascending by lowercased base name.
        let mut names: Vec<Vec<u8>> = self
            .assets
            .files(b"scripts/gl*.int")
            .into_iter()
            .map(|path| {
                let name = path.rsplit(|byte| *byte == b'/').next().unwrap_or(&path);
                name.to_ascii_lowercase()
            })
            .collect();
        names.sort_unstable();
        names.dedup();
        for name in names {
            self.load_sfall_script(&name, true);
        }
        if let Some(hooks) = self.hooks() {
            hooks.in_setup = false;
        }
    }
    /// Load a hook or global script and run its initial stream, which in a
    /// compiled script is its `start` (SPEC-004 §2.2). A file that is absent
    /// is no member; one the mod-script check refuses takes the cannot-load
    /// path of SPEC-006 §3.7 (§6.1).
    fn load_sfall_script(&mut self, file: &[u8], global: bool) -> Option<u32> {
        let path = [b"scripts/", file].concat();
        if !self.assets.available(&path) {
            return None;
        }
        let (path, program) = match self.assets.program_by_name(file) {
            Ok(found) => found,
            Err(crate::WorldError::ScriptRefused(_)) => return None,
            Err(error) => {
                self.world
                    .diagnostic(format!("{}: {error}", String::from_utf8_lossy(file)));
                return None;
            }
        };
        let mut vm = Vm::new(program);
        reserve_null_string(&mut vm).ok()?;
        let id = self.allocate_program()?;
        vm.id = id;
        self.registry.register_program(id, path.as_bytes());
        let hooks = self.hooks()?;
        hooks.scripts.insert(id);
        if global {
            hooks.globals.push(GlobalScript {
                program: id,
                repeat: 0,
                since: 0,
                kind: 0,
            });
        }
        let run = self.run_vm(&mut vm);
        self.finish_member(&mut vm, run, None);
        self.hooks()?.programs.insert(id, vm);
        Some(id)
    }
    /// A run has come back: a VM error unloads the member from every chain,
    /// and a word that would suspend it ends it (§6.5).
    fn finish_member(
        &mut self,
        vm: &mut Vm,
        run: Result<RunOutcome, VmError>,
        restore: Option<&Resume>,
    ) {
        match run {
            Ok(RunOutcome::Ended | RunOutcome::Stopped | RunOutcome::FragmentEnd) => {}
            Ok(outcome) => {
                self.world.diagnostic(format!(
                    "hook script {}: {outcome:?} ends this member",
                    vm.id
                ));
                if let Some(resume) = restore {
                    resume.apply(vm);
                }
            }
            Err(error) => {
                self.world
                    .diagnostic(format!("hook script {}: {error}; unloaded", vm.id));
                if let Some(hooks) = self.hooks() {
                    for chain in hooks.chains.values_mut() {
                        chain.remove(vm.id);
                    }
                }
            }
        }
    }
    /// Run one member to completion (SPEC-004 §3.7's synchronous call).
    fn run_member(&mut self, member: Member) {
        let Some(mut vm) = self
            .hooks()
            .and_then(|hooks| hooks.programs.remove(&member.program))
        else {
            // A nested chain reached a script that is already running.
            self.world.diagnostic(format!(
                "hook script {} is running; this member is skipped",
                member.program
            ));
            return;
        };
        let procedure = member
            .procedure
            .or_else(|| vm.program.find_procedure(b"start", true));
        if let Some(procedure) = procedure {
            let resume = Resume::of(&vm);
            let run = vm
                .prepare_host_call(procedure, true)
                .and_then(|()| self.run_vm(&mut vm));
            self.finish_member(&mut vm, run, Some(&resume));
        }
        if let Some(hooks) = self.hooks() {
            hooks.programs.insert(member.program, vm);
        }
    }
    /// Run `hook`'s chain on `args`: `None` when no chain ran, so the
    /// engine's values stand. Each member starts with both cursors at 0,
    /// arguments and returns carry forward, and the last writer wins per
    /// index (§6.5).
    pub(crate) fn run_hook(
        &mut self,
        hook: i32,
        args: Vec<SfallValue>,
    ) -> Option<Vec<Option<SfallValue>>> {
        let row: &'static HookRow = sfall::hook_row(hook).filter(|row| row.fires)?;
        let hooks = self.hooks()?;
        let members = hooks.chains.get(&hook)?.members();
        if members.is_empty() {
            return None;
        }
        if hooks.contexts.len() >= MAX_CHAINS {
            self.world
                .diagnostic(format!("{}: past 8 nested chains; skipped", row.name));
            return None;
        }
        hooks.contexts.push(Context {
            args,
            arg_types: row.args,
            rets: vec![None; row.rets.len()],
            ret_types: row.rets,
            next_arg: 0,
            next_ret: 0,
        });
        for member in members {
            if let Some(context) = self.hooks().and_then(|hooks| hooks.contexts.last_mut()) {
                context.next_arg = 0;
                context.next_ret = 0;
            }
            self.run_member(member);
        }
        self.hooks()
            .and_then(|hooks| hooks.contexts.pop())
            .map(|context| context.rets)
    }
    /// [`Self::run_hook`] while an attack holds the world's generator.
    fn run_hook_lent(
        &mut self,
        hook: i32,
        args: Vec<SfallValue>,
        rng: Option<&mut dyn RandomSource>,
    ) -> Option<Vec<Option<SfallValue>>> {
        match rng {
            Some(rng) => {
                rng.lend(&mut self.world.rng);
                let rets = self.run_hook(hook, args);
                rng.lend(&mut self.world.rng);
                rets
            }
            None => self.run_hook(hook, args),
        }
    }
    fn context(&mut self) -> Option<&mut Context> {
        self.hooks()?.contexts.last_mut()
    }
    /// `get_sfall_arg`: the next argument, int 0 past the end or outside a
    /// chain.
    pub(crate) fn next_sfall_arg(&mut self) -> SfallValue {
        let Some(context) = self.context() else {
            return int(0);
        };
        let value = context.args.get(context.next_arg).cloned();
        context.next_arg += 1;
        value.unwrap_or(int(0))
    }
    /// `get_sfall_arg_at`: without moving the cursor.
    pub(crate) fn sfall_arg_at(&mut self, index: i32) -> SfallValue {
        usize::try_from(index)
            .ok()
            .and_then(|index| self.context()?.args.get(index).cloned())
            .unwrap_or(int(0))
    }
    /// `get_sfall_args`: all current arguments.
    pub(crate) fn sfall_args(&mut self) -> Vec<SfallValue> {
        self.context()
            .map(|context| context.args.clone())
            .unwrap_or_default()
    }
    /// `set_sfall_arg`: by the argument's declared type.
    pub(crate) fn set_sfall_arg(&mut self, index: i32, value: &SfallValue) {
        let Some(context) = self.context() else {
            return;
        };
        let Ok(index) = usize::try_from(index) else {
            return;
        };
        let (Some(kind), true) = (context.arg_types.get(index), index < context.args.len()) else {
            return;
        };
        match kind.convert(value) {
            Some(value) => context.args[index] = value,
            None => self
                .world
                .diagnostic(format!("set_sfall_arg: argument {index} cannot take that")),
        }
    }
    /// `set_sfall_return`: the next return, nothing once all are written. A
    /// value that cannot convert leaves that return unset.
    pub(crate) fn set_sfall_return(&mut self, value: &SfallValue) {
        let Some(context) = self.context() else {
            return;
        };
        let at = context.next_ret;
        let Some(kind) = context.ret_types.get(at) else {
            return;
        };
        context.next_ret += 1;
        match kind.convert(value) {
            Some(value) => context.rets[at] = Some(value),
            None => self
                .world
                .diagnostic(format!("set_sfall_return: return {at} cannot take that")),
        }
    }
    /// `init_hook`: 1 during setup, else 0, and 0 inside a chain.
    pub(crate) fn init_hook(&mut self) -> i32 {
        self.hooks().map_or(0, |hooks| {
            i32::from(hooks.in_setup && hooks.contexts.is_empty())
        })
    }
    /// `register_hook`, `register_hook_proc` and `register_hook_proc_spec`
    /// (§6.2). Procedure 0 unregisters; `start` is `None`.
    pub(crate) fn register_hook(
        &mut self,
        vm: &Vm,
        hook: i32,
        procedure: Option<i32>,
        at_end: bool,
    ) {
        let name = if procedure.is_none() {
            "register_hook"
        } else if at_end {
            "register_hook_proc_spec"
        } else {
            "register_hook_proc"
        };
        let Some(hooks) = self.hooks() else {
            return;
        };
        if !hooks.scripts.contains(&vm.id) {
            self.world.diagnostic(format!(
                "{name}: the caller is neither a global script nor a hook script"
            ));
            return;
        }
        if sfall::hook_row(hook).is_none() {
            self.world
                .diagnostic(format!("{name}: hook {hook} has no row"));
            return;
        }
        let chain = hooks.chains.entry(hook).or_default();
        if procedure == Some(0) {
            chain.front.retain(|member| member.program != vm.id);
            chain.back.retain(|member| member.program != vm.id);
            return;
        }
        let procedure = procedure.and_then(|index| usize::try_from(index).ok());
        if let Some(member) = chain.held(vm.id) {
            member.procedure = procedure;
            return;
        }
        let member = Member {
            program: vm.id,
            procedure,
        };
        if at_end {
            chain.back.push(member);
        } else {
            chain.front.push(member);
        }
    }
    fn is_global(&mut self, vm: &Vm) -> bool {
        self.hooks()
            .is_some_and(|hooks| hooks.globals.iter().any(|global| global.program == vm.id))
    }
    /// §4.8: `self_obj` in a global script, with no `set_self` in place.
    pub(crate) fn sfall_global_without_self(&mut self, vm: &Vm) -> bool {
        self.sfall_self(vm).is_none() && self.is_global(vm)
    }
    /// `set_global_script_repeat`: no effect outside a global script.
    pub(crate) fn set_global_repeat(&mut self, vm: &Vm, repeat: i32) {
        if let Some(global) = self.global_mut(vm) {
            global.repeat = repeat.max(0);
            global.since = 0;
        }
    }
    /// `set_global_script_type`.
    pub(crate) fn set_global_kind(&mut self, vm: &Vm, kind: i32) {
        if let Some(global) = self.global_mut(vm) {
            global.kind = kind;
        }
    }
    fn global_mut(&mut self, vm: &Vm) -> Option<&mut GlobalScript> {
        self.hooks()?
            .globals
            .iter_mut()
            .find(|global| global.program == vm.id)
    }
    /// A map event for the global scripts, in their order (§4.9).
    pub(crate) fn sfall_global_event(&mut self, name: &[u8]) {
        let programs: Vec<u32> = self.hooks().map_or(Vec::new(), |hooks| {
            hooks.globals.iter().map(|global| global.program).collect()
        });
        for program in programs {
            let procedure = self
                .hooks()
                .and_then(|hooks| hooks.programs.get(&program))
                .and_then(|vm| vm.program.find_procedure(name, true));
            if let Some(procedure) = procedure {
                self.run_member(Member {
                    program,
                    procedure: Some(procedure),
                });
            }
        }
    }
    /// The repeating global scripts of this step's Simulation phase.
    pub(crate) fn sfall_repeat_globals(&mut self) {
        let mode = self.sfall_mode_bits();
        let dialogue = mode & game_mode::DIALOGUE != 0;
        let world_map = mode & game_mode::WORLD_MAP != 0;
        let local = mode & game_mode::LOCAL_MAP != 0;
        let due: Vec<u32> = self.hooks().map_or(Vec::new(), |hooks| {
            hooks
                .globals
                .iter_mut()
                .filter(|global| global.repeat > 0)
                .filter_map(|global| {
                    global.since += 1;
                    if global.since < global.repeat {
                        return None;
                    }
                    global.since = 0;
                    let runs = match global.kind {
                        0 => local && !dialogue,
                        1 => true,
                        2 => world_map,
                        3 => (local || world_map) && !dialogue,
                        _ => false,
                    };
                    runs.then_some(global.program)
                })
                .collect()
        });
        for program in due {
            self.run_member(Member {
                program,
                procedure: None,
            });
        }
    }
    /// `HOOK_TOHIT` (§6.8) on `attack`'s hit chance: the chance after the
    /// cap, or what the chain made of it, clamped to -99 through 999.
    pub fn hook_hit_chance(
        &mut self,
        attacker: ObjectRef,
        target: ObjectRef,
        attack: &Attack,
        range_aware: bool,
        rng: Option<&mut dyn RandomSource>,
    ) -> Result<i32, crate::WorldError> {
        if self.sfall.is_none() {
            return attack.hit_chance(range_aware);
        }
        let (chance, raw) = attack.chance_parts(range_aware)?;
        let source = self
            .world
            .get(attacker)
            .and_then(|object| object.raw.hex)
            .unwrap_or(-1);
        let args = vec![
            int(chance),
            object(attacker),
            object(target),
            int(i32::from(attack.location)),
            int(source),
            int(i32::from(attack.hit_mode)),
            int(i32::from(range_aware)),
            int(raw),
        ];
        let rets = self.run_hook_lent(HOOK_TOHIT, args, rng);
        Ok(rets
            .and_then(|rets| int_return(&rets, 0))
            .map_or(chance, |value| value.clamp(-99, 999)))
    }
    /// `HOOK_AFTERHITROLL` (§6.9): the result, with the attack's body part
    /// and defender changed when a return says so.
    pub(crate) fn hook_after_roll(
        &mut self,
        attacker: ObjectRef,
        target: &mut ObjectRef,
        attack: &mut Attack,
        code: i32,
        rng: &mut impl RandomSource,
    ) -> Result<i32, crate::WorldError> {
        if self.sfall.is_none() {
            return Ok(code);
        }
        let args = vec![
            int(code),
            object(attacker),
            object(*target),
            int(i32::from(attack.location)),
            int(attack.hit_chance(true)?),
        ];
        rng.lend(&mut self.world.rng);
        let rets = self.run_hook(HOOK_AFTERHITROLL, args);
        rng.lend(&mut self.world.rng);
        let Some(rets) = rets else {
            return Ok(code);
        };
        if let Some(part) = int_return(&rets, 1).filter(|part| (0..=8).contains(part)) {
            attack.location = part as u8;
        }
        let new_target = int_return(&rets, 2)
            .map(|target| target as ObjectRef)
            .filter(|critter| {
                *critter != 0 && self.world.critter_art(*critter) && !self.world.is_dead(*critter)
            });
        if let Some(new_target) = new_target.filter(|critter| critter != target) {
            let mut rebuilt = self.world.attack_profile(
                attacker,
                new_target,
                attack.hit_mode,
                attack.location,
                attack.aimed,
            )?;
            rebuilt.weapon = attack.weapon.clone();
            rebuilt.accuracy_bonus = attack.accuracy_bonus;
            rebuilt.hooked_chance = attack.hooked_chance;
            rebuilt.hooked_cost = attack.hooked_cost;
            *attack = rebuilt;
            *target = new_target;
        }
        Ok(int_return(&rets, 0)
            .filter(|roll| (0..=3).contains(roll))
            .unwrap_or(code))
    }
    /// `HOOK_CALCAPCOST` (§6.10) at the end of an attack-cost query.
    pub fn hook_ap_cost(&mut self, critter: ObjectRef, attack: &Attack) -> i32 {
        let cost = attack.ap_cost();
        if self.sfall.is_none() {
            return cost;
        }
        let weapon = self.world.wielded(critter, attack.hit_mode).unwrap_or(0);
        self.calc_ap_cost(
            critter,
            i32::from(attack.hit_mode),
            attack.aiming(),
            cost,
            weapon,
        )
    }
    /// The hook on the 2 AP reload and generic-use results.
    pub fn hook_fixed_cost(&mut self, critter: ObjectRef, cost: i32, item: ObjectRef) -> i32 {
        if self.sfall.is_none() {
            return cost;
        }
        self.calc_ap_cost(critter, 0, false, cost, item)
    }
    fn calc_ap_cost(
        &mut self,
        critter: ObjectRef,
        kind: i32,
        aimed: bool,
        cost: i32,
        item: ObjectRef,
    ) -> i32 {
        let args = vec![
            object(critter),
            int(kind),
            int(i32::from(aimed)),
            int(cost),
            object(item),
        ];
        self.run_hook(HOOK_CALCAPCOST, args)
            .and_then(|rets| int_return(&rets, 0))
            .map_or(cost, |value| value.max(0))
    }
    /// `HOOK_MOVECOST` (§6.13): `hexes` becoming `cost` AP.
    pub(crate) fn hook_move_cost(&mut self, critter: ObjectRef, hexes: i32, cost: i32) -> i32 {
        if self.sfall.is_none() {
            return cost;
        }
        let args = vec![object(critter), int(hexes), int(cost)];
        self.run_hook(HOOK_MOVECOST, args)
            .and_then(|rets| int_return(&rets, 0))
            .map_or(cost, |value| value.max(0))
    }
    /// `HOOK_COMBATDAMAGE` (§6.11), once per victim of a hit: the primary
    /// target first, then each extra in spray order. Nothing is debited yet.
    /// Returns whether the main target's chain ran.
    pub(crate) fn hook_combat_damage(
        &mut self,
        attacker: ObjectRef,
        target: ObjectRef,
        attack: &Attack,
        spray: &mut Spray,
    ) -> bool {
        if self.sfall.is_none() {
            return false;
        }
        let weapon = self.world.wielded(attacker, attack.hit_mode).unwrap_or(0);
        let kind = i32::from(attack.hit_mode);
        let part = i32::from(attack.location);
        let hit = spray.main.defender_flags & effect::HIT != 0;
        if hit {
            let main = &mut spray.main;
            self.damage_chain(attacker, target, weapon, part, kind, main, false);
        }
        for (victim, _, outcome) in &mut spray.extras {
            let victim = *victim;
            self.damage_chain(attacker, victim, weapon, part, kind, outcome, true);
        }
        hit
    }
    #[allow(clippy::too_many_arguments)]
    fn damage_chain(
        &mut self,
        attacker: ObjectRef,
        target: ObjectRef,
        weapon: ObjectRef,
        part: i32,
        kind: i32,
        outcome: &mut Resolution,
        extra: bool,
    ) {
        let args = vec![
            object(target),
            object(attacker),
            int(outcome.damage),
            int(if extra { 0 } else { outcome.self_damage }),
            int(outcome.defender_flags as i32),
            int(if extra {
                0
            } else {
                outcome.attacker_flags as i32
            }),
            object(weapon),
            int(part),
            int(outcome.multiplier),
            int(outcome.rounds),
            int(outcome.knockback),
            int(kind),
            int(0),
        ];
        let Some(rets) = self.run_hook(HOOK_COMBATDAMAGE, args) else {
            return;
        };
        if let Some(damage) = int_return(&rets, 0) {
            outcome.damage = damage.max(0);
        }
        if let Some(damage) = int_return(&rets, 1) {
            outcome.self_damage = damage.max(0);
        }
        if let Some(flags) = int_return(&rets, 2) {
            outcome.defender_flags = flags as u32;
        }
        if let Some(flags) = int_return(&rets, 3) {
            outcome.attacker_flags = flags as u32;
        }
        if let Some(knockback) = int_return(&rets, 4) {
            outcome.knockback = knockback.max(0);
        }
    }
    fn barter_inputs(&mut self, session: &BarterSession) -> (i64, i64, i32, i64) {
        (
            self.world.inventory_value(session.table),
            i64::from(self.world.caps(session.table)),
            self.barter_modifier(session),
            self.world.inventory_value(session.offer),
        )
    }
    /// `HOOK_BARTERPRICE` (§6.12): the price and the offer's value.
    fn barter_chain(&mut self, session: &BarterSession, offered: bool) -> (i32, i64) {
        let plain = self.barter_price(session);
        let (value, caps, _, offer) = self.barter_inputs(session);
        let party = self.control_of(session.merchant).is_some();
        let args = vec![
            object(self.world.player),
            object(session.merchant),
            int(plain),
            object(session.table),
            int(caps as i32),
            int(value.min(i64::from(i32::MAX)) as i32),
            object(session.offer),
            int(offer.min(i64::from(i32::MAX)) as i32),
            int(i32::from(offered && !party)),
            int(i32::from(party)),
        ];
        let Some(rets) = self.run_hook(HOOK_BARTERPRICE, args) else {
            return (plain, offer);
        };
        let price = match int_return(&rets, 0) {
            None | Some(-1) => plain,
            Some(value) => value.max(0),
        };
        let offer = int_return(&rets, 1).map_or(offer, |value| i64::from(value.max(0)));
        (price, offer)
    }
    /// The price and offer value of the offer control's comparison.
    pub(crate) fn hooked_barter(&mut self, session: &BarterSession, offered: bool) -> (i32, i64) {
        if self.sfall.is_none() {
            let offer = self.world.inventory_value(session.offer);
            return (self.barter_price(session), offer);
        }
        self.barter_chain(session, offered)
    }
    /// A transaction was made: the next preview is a fresh one.
    pub(crate) fn sfall_barter_changed(&mut self) {
        if let Some(hooks) = self.hooks() {
            hooks.barter_fresh = true;
            hooks.barter = None;
        }
    }
    /// The barter screen's preview, in the Simulation phase (§6.6): at most
    /// once per step, and only when its inputs differ from the last step's.
    /// On entering barter and after a transaction it runs twice, for the
    /// player and then for the NPC (§6.12).
    pub fn sfall_barter_preview(&mut self, session: &BarterSession) {
        if self.sfall.is_none() {
            return;
        }
        let inputs = self.barter_inputs(session);
        let held = self
            .hooks()
            .and_then(|hooks| hooks.barter)
            .filter(|preview| preview.inputs == inputs);
        if held.is_some() {
            return;
        }
        let fresh = self.hooks().is_some_and(|hooks| {
            let first = hooks.barter.is_none();
            first || std::mem::take(&mut hooks.barter_fresh)
        });
        if fresh {
            self.barter_chain(session, false);
        }
        let (price, offer) = self.barter_chain(session, false);
        if let Some(hooks) = self.hooks() {
            hooks.barter_fresh = false;
            hooks.barter = Some(BarterPreview {
                inputs,
                price,
                offer,
            });
        }
    }
    /// Barter closed: its preview goes with it.
    pub(crate) fn sfall_barter_closed(&mut self) {
        if let Some(hooks) = self.hooks() {
            hooks.barter = None;
            hooks.barter_fresh = false;
        }
    }
    /// A stored preview of `kind`, queried now only when `inputs` differ
    /// from the previous step's (§6.6).
    pub fn sfall_preview(
        &mut self,
        kind: Preview,
        inputs: Vec<i32>,
        query: impl FnOnce(&mut Self) -> i32,
    ) -> Option<i32> {
        self.hooks()?;
        let held = self
            .hooks()?
            .previews
            .get(&kind)
            .filter(|(old, _)| *old == inputs)
            .map(|(_, value)| *value);
        if held.is_some() {
            return held;
        }
        let value = query(self);
        self.hooks()?.previews.insert(kind, (inputs, value));
        Some(value)
    }
    /// The value stored for `kind` from these `inputs`, for presentation.
    /// Other inputs have no stored value, and show the unhooked one.
    pub fn sfall_previewed(&self, kind: Preview, inputs: &[i32]) -> Option<i32> {
        self.sfall
            .as_deref()?
            .hooks
            .previews
            .get(&kind)
            .filter(|(old, _)| old == inputs)
            .map(|(_, value)| *value)
    }
    /// What the barter screen stored, for presentation.
    pub fn sfall_barter_shown(&self) -> Option<(i32, i64)> {
        let preview = self.sfall.as_deref()?.hooks.barter?;
        Some((preview.price, preview.offer))
    }
    /// The called-shot window's eight chances, queried once as it opens.
    pub fn hooked_chances(
        &mut self,
        attacker: ObjectRef,
        target: ObjectRef,
        attack: &Attack,
    ) -> Result<[i32; 8], crate::WorldError> {
        let mut chances = [0; 8];
        for (location, chance) in chances.iter_mut().enumerate() {
            let mut aimed = attack.clone();
            aimed.location = location as u8;
            *chance = self.hook_hit_chance(attacker, target, &aimed, true, None)?;
        }
        Ok(chances)
    }
    /// `HOOK_INVENTORYMOVE` (§6.13): whether the engine handler runs. It can
    /// only prevent a move.
    pub fn sfall_inventory_move(
        &mut self,
        target: i32,
        item: ObjectRef,
        replaced: ObjectRef,
    ) -> bool {
        if self.sfall.is_none() {
            return true;
        }
        let args = vec![int(target), object(item), object(replaced)];
        self.run_hook(HOOK_INVENTORYMOVE, args)
            .and_then(|rets| int_return(&rets, 0))
            .is_none_or(|value| value == -1)
    }
    /// `HOOK_KEYPRESS` (§6.13) for one physical press or release: the key
    /// the game sees, as a scan code. The input log holds the physical key,
    /// so a replay runs the hook again. A release also ends whatever its
    /// press produced.
    pub fn sfall_key(&mut self, pressed: bool, scan: i32, virtual_key: i32) -> i32 {
        if self.sfall.is_none() {
            return scan;
        }
        let args = vec![int(i32::from(pressed)), int(scan), int(virtual_key)];
        let returned = self
            .run_hook(HOOK_KEYPRESS, args)
            .and_then(|rets| int_return(&rets, 0))
            .filter(|key| *key != 0);
        let Some(hooks) = self.hooks() else {
            return scan;
        };
        if pressed {
            let logical = returned.unwrap_or(scan);
            hooks.held_keys.insert(scan, logical);
            logical
        } else {
            hooks.held_keys.remove(&scan).or(returned).unwrap_or(scan)
        }
    }
}

/// Where a member's call began, for a word that would suspend it (§6.5).
struct Resume {
    pc: u32,
    status: VmStatus,
    f: i32,
    g: i32,
    wait: u32,
    values: usize,
    returns: usize,
}
impl Resume {
    fn of(vm: &Vm) -> Self {
        Self {
            pc: vm.pc,
            status: vm.status,
            f: vm.f,
            g: vm.g,
            wait: vm.wait_predicate,
            values: vm.value_stack.byte_len(),
            returns: vm.return_stack.byte_len(),
        }
    }
    fn apply(&self, vm: &mut Vm) {
        let _ = vm.value_stack.truncate(self.values);
        let _ = vm.return_stack.truncate(self.returns);
        vm.pc = self.pc;
        vm.status = self.status;
        vm.f = self.f;
        vm.g = self.g;
        vm.wait_predicate = self.wait;
    }
}

#[cfg(test)]
impl<A: WorldAssets> WorldHost<A> {
    /// The program of global script `index`, in load order.
    pub(crate) fn global_for_test(&mut self, index: usize) -> u32 {
        self.hooks().expect("sfall mode").globals[index].program
    }
    /// `register_hook_proc(hook, 0)` from `program`.
    pub(crate) fn unregister_for_test(&mut self, hook: i32, program: u32) {
        let mut vm = Vm::new(self.test_program(program));
        vm.id = program;
        self.register_hook(&vm, hook, Some(0), false);
    }
    fn test_program(&mut self, program: u32) -> vault_vm::IntProgram {
        self.hooks().expect("sfall mode").programs[&program]
            .program
            .clone()
    }
    /// Fill the context stack as `count` nested chains would.
    pub(crate) fn fill_contexts_for_test(&mut self, count: usize) {
        let hooks = self.hooks().expect("sfall mode");
        hooks.contexts.clear();
        for _ in 0..count {
            hooks.contexts.push(Context {
                args: Vec::new(),
                arg_types: &[],
                rets: Vec::new(),
                ret_types: &[],
                next_arg: 0,
                next_ret: 0,
            });
        }
    }
}
