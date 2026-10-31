//! SPEC-024b §6.19: the boundaries of the hooks Revision 4 promoted. Each
//! function here is one engine query: with sfall mode off, or an empty
//! chain, the engine's value stands and nothing runs.
use crate::{
    ObjectRef, RandomSource, WorldError,
    assets::WorldAssets,
    combat::{Attack, Resolution},
    host::WorldHost,
    host_sfall_hooks::{Preview, int, int_return, object},
    skills::skill,
};
use std::cell::RefCell;
use vault_vm::sfall::SfallValue;

pub const HOOK_DEATHANIM2: i32 = 4;
pub const HOOK_AMMOCOST: i32 = 17;
pub const HOOK_MOUSECLICK: i32 = 20;
pub const HOOK_STEAL: i32 = 22;
pub const HOOK_WITHINPERCEPTION: i32 = 23;
pub const HOOK_INVENWIELD: i32 = 25;
pub const HOOK_ADJUSTFID: i32 = 26;
pub const HOOK_COMBATTURN: i32 = 27;
pub const HOOK_CARTRAVEL: i32 = 28;
pub const HOOK_RESTTIMER: i32 = 30;
pub const HOOK_USEANIMOBJ: i32 = 32;
pub const HOOK_DESCRIPTIONOBJ: i32 = 34;
pub const HOOK_USESKILLON: i32 = 35;
pub const HOOK_SETLIGHTING: i32 = 38;
pub const HOOK_ADJUSTRADS: i32 = 45;
pub const HOOK_CANUSEWEAPON: i32 = 48;

/// `HOOK_WITHINPERCEPTION`'s arg3.
pub(crate) mod perception {
    pub const OTHER: i32 = 0;
    pub const CAN_SEE: i32 = 1;
    pub const AI_TARGET: i32 = 3;
}

/// What `HOOK_STEAL` decided about one moved item.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum StealHook {
    /// SPEC-013 §3.9.3 runs.
    Engine,
    /// 1: moved, not caught.
    Moved,
    /// 0: caught, as SPEC-013 §3.9.4 has it.
    Caught,
    /// 2: not moved, and the window stays open.
    Kept,
}

/// `ThreatView` over a host, so that `ai::danger`'s perception queries run
/// their chains: the AI reads through `&self`, and only `perceives` writes.
struct HookedThreats<'a, A: WorldAssets>(RefCell<&'a mut WorldHost<A>>);
impl<A: WorldAssets> crate::ai::ThreatView for HookedThreats<'_, A> {
    fn last_attacker(&self, critter: ObjectRef) -> Option<ObjectRef> {
        self.0.borrow().world.last_attacker(critter)
    }
    fn dead(&self, critter: ObjectRef) -> bool {
        self.0.borrow().world.dead(critter)
    }
    fn team(&self, critter: ObjectRef) -> i32 {
        self.0.borrow().world.team(critter)
    }
    fn dist(&self, a: ObjectRef, b: ObjectRef) -> i32 {
        self.0.borrow().world.dist(a, b)
    }
    fn perceives(&self, observer: ObjectRef, target: ObjectRef) -> bool {
        self.0
            .borrow_mut()
            .perceives(observer, target, perception::AI_TARGET)
    }
}

impl<A: WorldAssets> WorldHost<A> {
    /// The first return of `hook`'s chain on `args`, if a member set it.
    fn hook_int(&mut self, hook: i32, args: Vec<SfallValue>) -> Option<i32> {
        self.run_hook(hook, args)
            .and_then(|rets| int_return(&rets, 0))
    }
    /// `HOOK_DEATHANIM2`: the death `animation` chosen for `target`, or the
    /// chain's, which needs art.
    pub(crate) fn hook_death(
        &mut self,
        attacker: ObjectRef,
        target: ObjectRef,
        weapon: Option<ObjectRef>,
        damage: i32,
        animation: u8,
    ) -> u8 {
        if self.sfall.is_none() {
            return animation;
        }
        let pid = weapon
            .and_then(|weapon| self.world.get(weapon))
            .map_or(-1, |found| found.raw.pid as i32);
        let args = vec![
            int(pid),
            object(attacker),
            object(target),
            int(damage),
            int(i32::from(animation)),
        ];
        let Some(chosen) = self.hook_int(HOOK_DEATHANIM2, args) else {
            return animation;
        };
        let facing = self.facing(target);
        let found = u8::try_from(chosen).ok().filter(|code| {
            let art = self.critter_fid(target, *code, None, facing);
            self.assets.art_exists(art)
        });
        found.unwrap_or_else(|| {
            self.world.diagnostic(format!(
                "HOOK_DEATHANIM2: animation {chosen} has no art; the engine's stands"
            ));
            animation
        })
    }
    /// `HOOK_AMMOCOST` type 2 as a burst's rounds are counted: the cost of
    /// each round, or `None` with the mode off.
    pub(crate) fn hook_burst_cost(
        &mut self,
        attacker: ObjectRef,
        attack: &Attack,
        rounds: i32,
        rng: &mut impl RandomSource,
    ) -> Option<i32> {
        self.sfall.as_ref()?;
        let weapon = self.world.wielded(attacker, attack.hit_mode)?;
        let args = vec![object(weapon), int(rounds), int(1), int(2)];
        rng.lend(&mut self.world.rng);
        let cost = self.hook_int(HOOK_AMMOCOST, args);
        rng.lend(&mut self.world.rng);
        Some(cost.filter(|cost| *cost >= 0).unwrap_or(1))
    }
    /// `HOOK_AMMOCOST` type 0 after a single attack, or 3 after a burst: the
    /// ammunition `weapon` spends. Damage was settled before this runs.
    pub(crate) fn hook_ammo_spent(&mut self, weapon: ObjectRef, resolution: &Resolution) -> i32 {
        let (rounds, spent, kind) = match resolution.burst_cost {
            Some(cost) => (resolution.ammo, resolution.ammo.saturating_mul(cost), 3),
            None => (1, resolution.ammo, 0),
        };
        if self.sfall.is_none() {
            return spent;
        }
        let args = vec![object(weapon), int(rounds), int(spent), int(kind)];
        self.hook_int(HOOK_AMMOCOST, args)
            .filter(|spent| *spent >= 0)
            .unwrap_or(spent)
    }
    /// `HOOK_MOUSECLICK` for one recorded press or release, before the
    /// engine handles it. The input log holds the event, so a replay runs
    /// the hook again.
    pub fn sfall_mouse(&mut self, pressed: bool, button: i32) {
        if self.sfall.is_some() {
            self.run_hook(HOOK_MOUSECLICK, vec![int(i32::from(pressed)), int(button)]);
        }
    }
    /// `HOOK_STEAL` for `quantity` of `item`: the check's result, and the
    /// experience that replaces SPEC-013 §3.9.5's increment.
    pub(crate) fn hook_steal(
        &mut self,
        thief: ObjectRef,
        victim: ObjectRef,
        item: ObjectRef,
        planting: bool,
        quantity: i32,
    ) -> (StealHook, Option<i32>) {
        if self.sfall.is_none() {
            return (StealHook::Engine, None);
        }
        let args = vec![
            object(thief),
            object(victim),
            object(item),
            int(i32::from(planting)),
            int(quantity),
        ];
        let Some(rets) = self.run_hook(HOOK_STEAL, args) else {
            return (StealHook::Engine, None);
        };
        let check = match int_return(&rets, 0) {
            Some(1) => StealHook::Moved,
            Some(0) => StealHook::Caught,
            Some(2) => StealHook::Kept,
            _ => StealHook::Engine,
        };
        (check, int_return(&rets, 1).filter(|award| *award >= 0))
    }
    /// `HOOK_WITHINPERCEPTION` on `perceives` having given `in_range`: 0, 1,
    /// or 2. A value outside them leaves the engine's result.
    fn hook_perception(
        &mut self,
        observer: ObjectRef,
        target: ObjectRef,
        in_range: bool,
        kind: i32,
    ) -> i32 {
        let engine = i32::from(in_range);
        if self.sfall.is_none() {
            return engine;
        }
        let args = vec![object(observer), object(target), int(engine), int(kind)];
        self.hook_int(HOOK_WITHINPERCEPTION, args)
            .filter(|result| (0..=2).contains(result))
            .unwrap_or(engine)
    }
    /// SPEC-009 §3.3 `perceives`, then its chain. 2 acts as 1 here.
    pub(crate) fn perceives(&mut self, observer: ObjectRef, target: ObjectRef, kind: i32) -> bool {
        let in_range = self.world.can_perceive(observer, target);
        self.hook_perception(observer, target, in_range, kind) != 0
    }
    /// `obj_can_see_obj`: 2 skips the blocker ray.
    pub(crate) fn hooked_can_see(&mut self, observer: ObjectRef, target: ObjectRef) -> bool {
        if self.sfall.is_none() {
            return self.world.can_see(observer, target);
        }
        let placed =
            |host: &Self, object| host.world.get(object).is_some_and(|o| o.raw.hex.is_some());
        if !placed(self, observer) || !placed(self, target) {
            return false;
        }
        let in_range = self.world.can_perceive(observer, target);
        match self.hook_perception(observer, target, in_range, perception::CAN_SEE) {
            0 => false,
            2 => true,
            _ => self.world.clear_sight(observer, target),
        }
    }
    /// §3.5 `danger` over `snapshot`, with its perception queries hooked.
    pub(crate) fn hooked_danger(
        &mut self,
        snapshot: &mut [ObjectRef],
        critter: ObjectRef,
    ) -> Option<ObjectRef> {
        if self.sfall.is_none() {
            return crate::ai::danger(snapshot, critter, &self.world);
        }
        crate::ai::danger(snapshot, critter, &HookedThreats(RefCell::new(self)))
    }
    /// The slot `HOOK_INVENWIELD` names `item` by: 0 armor, 1 right hand, 2
    /// left hand. `None` when it is not equipped.
    fn equipped_slot(&self, item: ObjectRef) -> Option<i32> {
        use vault_formats::pro::BasicFlags;
        let flags = self.world.get(item)?.raw.flags;
        if flags.contains(BasicFlags::WORN) {
            Some(0)
        } else if flags.contains(BasicFlags::RIGHT_HAND) {
            Some(1)
        } else if flags.contains(BasicFlags::LEFT_HAND) {
            Some(2)
        } else {
            None
        }
    }
    /// `HOOK_INVENWIELD`: whether the engine's handler runs.
    pub(crate) fn hook_wield(
        &mut self,
        critter: ObjectRef,
        item: ObjectRef,
        slot: i32,
        wielding: bool,
    ) -> bool {
        if self.sfall.is_none() {
            return true;
        }
        let args = vec![
            object(critter),
            object(item),
            int(slot),
            int(i32::from(wielding)),
            int(0),
        ];
        self.hook_int(HOOK_INVENWIELD, args)
            .is_none_or(|value| value == -1)
    }
    /// `HOOK_INVENWIELD` as a script removes `item` from `owner`'s
    /// inventory. If it is equipped the chain runs, and the removal happens
    /// whatever it returns.
    pub(crate) fn hook_removal(&mut self, owner: ObjectRef, item: ObjectRef) {
        if self.sfall.is_none() {
            return;
        }
        if let Some(slot) = self.equipped_slot(item) {
            let args = vec![object(owner), object(item), int(slot), int(0), int(1)];
            self.run_hook(HOOK_INVENWIELD, args);
        }
    }
    /// `HOOK_ADJUSTFID`, a preview: the figure the screen shows for `fid`.
    pub fn sfall_figure(&mut self, fid: u32) -> Option<i32> {
        let inputs = vec![fid as i32];
        self.sfall_preview(Preview::Figure, inputs, |host| {
            let args = vec![int(fid as i32), int(fid as i32)];
            host.hook_int(HOOK_ADJUSTFID, args)
                .filter(|chosen| host.assets.art_exists(*chosen as u32))
                .unwrap_or(fid as i32)
        })
    }
    /// What presentation shows for the figure `fid`.
    pub fn sfall_figure_shown(&self, fid: u32) -> u32 {
        self.sfall_previewed(Preview::Figure, &[fid as i32])
            .map_or(fid, |shown| shown as u32)
    }
    /// `HOOK_COMBATTURN` at the start of `critter`'s turn: whether to skip
    /// it.
    pub(crate) fn hook_turn_start(&mut self, critter: ObjectRef, resumed: bool) -> bool {
        let Some(hooks) = self.hooks() else {
            return false;
        };
        hooks.turn_resumed = resumed;
        let args = vec![int(1), object(critter), int(i32::from(resumed))];
        self.hook_int(HOOK_COMBATTURN, args) == Some(1)
    }
    /// `HOOK_COMBATTURN` at the end of `critter`'s turn. -1 ends combat at
    /// the next turn boundary.
    pub(crate) fn hook_turn_end(&mut self, critter: ObjectRef) {
        let Some(hooks) = self.hooks() else {
            return;
        };
        let resumed = std::mem::take(&mut hooks.turn_resumed);
        let args = vec![int(0), object(critter), int(i32::from(resumed))];
        if self.hook_int(HOOK_COMBATTURN, args) == Some(-1)
            && let Some(hooks) = self.hooks()
        {
            hooks.combat_ending = true;
        }
    }
    /// Whether a turn's chain ended combat.
    pub(crate) fn sfall_ends_combat(&mut self) -> bool {
        self.hooks()
            .is_some_and(|hooks| std::mem::take(&mut hooks.combat_ending))
    }
    /// `HOOK_COMBATTURN` once combat has ended, with `critter` the last
    /// turn's.
    pub(crate) fn hook_combat_over(&mut self, critter: ObjectRef, abrupt: bool) {
        if let Some(hooks) = self.hooks() {
            hooks.combat_ending = false;
            hooks.turn_resumed = false;
            let args = vec![int(if abrupt { -1 } else { -2 }), object(critter), int(0)];
            self.run_hook(HOOK_COMBATTURN, args);
        }
    }
    /// `HOOK_CARTRAVEL`: the steps and the fuel of one car iteration.
    pub(crate) fn hook_car_travel(&mut self, steps: i32, fuel: i32) -> (i32, i32) {
        if self.sfall.is_none() {
            return (steps, fuel);
        }
        let Some(rets) = self.run_hook(HOOK_CARTRAVEL, vec![int(steps), int(fuel)]) else {
            return (steps, fuel);
        };
        (
            int_return(&rets, 0)
                .filter(|speed| *speed >= 0)
                .unwrap_or(steps),
            int_return(&rets, 1).map_or(fuel, |fuel| fuel.max(0)),
        )
    }
    /// `HOOK_RESTTIMER`'s return for `event` of a rest of `span`, its hours
    /// and minutes.
    pub(crate) fn hook_rest(&mut self, event: i32, span: (i32, i32)) -> Option<i32> {
        self.sfall.as_ref()?;
        let ticks = self.world.clock.ticks as u32 as i32;
        let args = vec![int(ticks), int(event), int(span.0), int(span.1)];
        self.hook_int(HOOK_RESTTIMER, args)
    }
    /// `HOOK_USEANIMOBJ` on `animation`, the code a use will play. `None`
    /// leaves the engine's; `Some(None)` skips the animation.
    pub(crate) fn hook_use_animation(
        &mut self,
        actor: ObjectRef,
        target: ObjectRef,
        animation: u8,
    ) -> Option<Option<u8>> {
        self.sfall.as_ref()?;
        let args = vec![object(actor), object(target), int(i32::from(animation))];
        let chosen = self.hook_int(HOOK_USEANIMOBJ, args)?;
        if chosen == -1 {
            return Some(None);
        }
        let facing = self.facing(actor);
        let code = u8::try_from(chosen).ok().filter(|code| {
            let art = self.critter_fid(actor, *code, None, facing);
            self.assets.art_exists(art)
        });
        if code.is_none() {
            self.world.diagnostic(format!(
                "HOOK_USEANIMOBJ: animation {chosen} has no art; the engine's stands"
            ));
        }
        code.map(Some)
    }
    /// `HOOK_DESCRIPTIONOBJ`: the text that replaces `target`'s own.
    pub(crate) fn hook_description(&mut self, target: ObjectRef) -> Option<Vec<u8>> {
        self.sfall.as_ref()?;
        let rets = self.run_hook(HOOK_DESCRIPTIONOBJ, vec![object(target)])?;
        match rets.first()? {
            Some(SfallValue::Text(text)) => Some(text.as_slice().to_vec()),
            _ => None,
        }
    }
    /// `HOOK_USESKILLON` before gating: the request's user, and whether it
    /// passes the combat refusal. `None` cancels the request.
    pub(crate) fn hook_skill_on(
        &mut self,
        actor: ObjectRef,
        target: ObjectRef,
        index: i32,
    ) -> Option<(ObjectRef, bool)> {
        if self.sfall.is_none() {
            return Some((actor, false));
        }
        let args = vec![object(actor), object(target), int(index)];
        let Some(rets) = self.run_hook(HOOK_USESKILLON, args) else {
            return Some((actor, false));
        };
        let user = match int_return(&rets, 0) {
            Some(-1) => return None,
            None | Some(0) => actor,
            Some(_) if index == skill::STEAL => actor,
            Some(user) if self.world.critter_art(user as ObjectRef) => user as ObjectRef,
            Some(user) => {
                self.world.diagnostic(format!(
                    "HOOK_USESKILLON: {user} is no critter; the user stands"
                ));
                actor
            }
        };
        let controlled = user == self.world.player || self.control_of(user).is_some();
        Some((user, controlled && int_return(&rets, 1) == Some(1)))
    }
    /// `HOOK_SETLIGHTING` on `intensity` and `radius`; `owner` is -1 for the
    /// map, which takes no radius.
    pub(crate) fn hook_light(&mut self, owner: i32, intensity: i32, radius: i32) -> (i32, i32) {
        if self.sfall.is_none() {
            return (intensity, radius);
        }
        let args = vec![int(owner), int(intensity), int(radius)];
        let Some(rets) = self.run_hook(HOOK_SETLIGHTING, args) else {
            return (intensity, radius);
        };
        (
            int_return(&rets, 0).map_or(intensity, |value| value.clamp(0, 65536)),
            int_return(&rets, 1)
                .filter(|_| owner != -1)
                .map_or(radius, |value| value.clamp(0, 8)),
        )
    }
    /// `obj_set_light_level`.
    pub(crate) fn hooked_object_light(
        &mut self,
        reference: ObjectRef,
        percent: i32,
        radius: i32,
    ) -> Result<(), WorldError> {
        let requested = crate::WorldState::light_request(percent)?;
        let (requested, radius) = self.hook_light(reference as i32, requested, radius);
        self.world.store_object_light(reference, requested, radius)
    }
    /// SPEC-007 §3.9's light on a landed projectile.
    pub(crate) fn hooked_projectile_light(
        &mut self,
        projectile: ObjectRef,
        radius: i32,
        intensity: i32,
    ) {
        let (intensity, radius) = self.hook_light(projectile as i32, intensity, radius);
        self.world.set_light(projectile, radius, intensity);
    }
    /// `set_light_level`.
    pub(crate) fn hooked_map_light(&mut self, percent: i32) {
        let computed = self
            .world
            .ambient_of(crate::WorldState::ambient_request(percent));
        self.world.ambient = self.hook_light(-1, computed, -1).0;
    }
    /// `HOOK_ADJUSTRADS`: the amount `critter`'s radiation changes by.
    pub(crate) fn hook_radiation(&mut self, critter: ObjectRef, amount: i32) -> i32 {
        if self.sfall.is_none() {
            return amount;
        }
        self.hook_int(HOOK_ADJUSTRADS, vec![object(critter), int(amount)])
            .unwrap_or(amount)
    }
    /// SPEC-009 §3.9's usability test, then `HOOK_CANUSEWEAPON`.
    pub(crate) fn hooked_usable(
        &mut self,
        critter: ObjectRef,
        weapon: ObjectRef,
        mode: u8,
        min_to_hit: i32,
    ) -> bool {
        let usable = self.usable(critter, weapon, mode, min_to_hit);
        self.weapon_chain(critter, weapon, i32::from(mode), usable)
    }
    pub(crate) fn weapon_chain(
        &mut self,
        critter: ObjectRef,
        weapon: ObjectRef,
        kind: i32,
        usable: bool,
    ) -> bool {
        if self.sfall.is_none() {
            return usable;
        }
        let args = vec![
            object(critter),
            object(weapon),
            int(kind),
            int(i32::from(usable)),
        ];
        self.hook_int(HOOK_CANUSEWEAPON, args)
            .map_or(usable, |value| value != 0)
    }
    /// `HOOK_CANUSEWEAPON`'s preview of the player's active `weapon` in
    /// `mode`.
    pub fn sfall_slot_usable(&mut self, weapon: ObjectRef, mode: u8) -> Option<i32> {
        let player = self.world.player;
        let usable = self.usable(player, weapon, mode, 0);
        let inputs = vec![weapon as i32, i32::from(mode), i32::from(usable)];
        self.sfall_preview(Preview::SlotUsable, inputs, |host| {
            i32::from(host.weapon_chain(player, weapon, -1, usable))
        })
    }
    /// Whether the slot's preview refuses `weapon` in `mode`.
    pub fn sfall_slot_refused(&self, weapon: ObjectRef, mode: u8) -> bool {
        let player = self.world.player;
        let usable = self.usable(player, weapon, mode, 0);
        let inputs = [weapon as i32, i32::from(mode), i32::from(usable)];
        self.sfall_previewed(Preview::SlotUsable, &inputs) == Some(0)
    }
}
