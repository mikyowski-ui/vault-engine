use crate::{
    ObjectAction, Perk, WorldEffect, WorldError,
    assets::WorldAssets,
    dialogue::{DialogueOption, DialogueText},
    host::{AnimationSequence, HostResult, WorldHost},
    queue::EventKind,
    rng::{self, RandomSource},
    world::{ObjectRef, WorldObject},
};
use vault_formats::{
    HexGrid,
    map::ObjectState,
    pro::{BasicFlags, Category, LockFlags, PrototypeData},
};
use vault_vm::{
    ExternalValue, Value, Vm, VmError,
    extensions::{HostCall, base_tag},
};

fn integer(value: i32) -> Option<ExternalValue> {
    Some(ExternalValue::Value(Value::integer(value)))
}
fn string(value: Vec<u8>) -> Option<ExternalValue> {
    Some(ExternalValue::String(value))
}
fn dynamic_null() -> Option<ExternalValue> {
    Some(ExternalValue::Value(Value {
        tag: Value::DYNAMIC_STRING,
        payload: 0,
    }))
}
fn text_argument(call: &HostCall, text: usize, list: usize) -> DialogueText {
    if base_tag(call.args[text].value) == Value::STATIC_STRING {
        DialogueText::Literal(call.text(text).unwrap_or(b"").to_vec())
    } else {
        DialogueText::Message {
            list: call.int(list),
            message: call.int(text),
        }
    }
}
/// The error prefix of a nonzero reference that names no object. SPEC-005
/// §2.1 `[May-improve]`: such a reference is diagnosed and the row pushes
/// its null result instead of faulting the program (`game_call` intercepts
/// the error; references are never reused, so a stale one is unambiguous).
const STALE_REFERENCE: &str = "invalid opaque object reference";
fn stale_reference(reference: ObjectRef) -> VmError {
    VmError::Host(format!("{STALE_REFERENCE} {reference:#x}"))
}
/// The error prefix of an ordinary host failure inside a row. SPEC-005
/// §2.1: a host diagnostic followed by an ordinary return does not raise a
/// VM error, so `game_call` diagnoses it and pushes the row's null result.
const HOST_FAILURE: &str = "host failure:";
fn host_failure(error: impl std::fmt::Display) -> VmError {
    VmError::Host(format!("{HOST_FAILURE} {error}"))
}
/// SPEC-005 §2.1: the null result of each row that resolves an object
/// reference, as its null-operand path pushes it (or the row's safe rule).
fn null_result(op: u16) -> Option<ExternalValue> {
    match op {
        0x80a4 => string(b"<Critter>".to_vec()),
        0x80a5 | 0x813d | 0x8141 => dynamic_null(),
        0x80c8 | 0x80ca | 0x80d4 | 0x80d5 | 0x80ff | 0x8100 | 0x8102 | 0x8103 | 0x8139 => {
            integer(-1)
        }
        0x80d2 | 0x80d3 => integer(9999),
        0x80e8 | 0x80ec | 0x80f3 | 0x80f5 | 0x8106 | 0x8117 | 0x8144 | 0x8149 => integer(0),
        _ => None,
    }
}
impl<A: WorldAssets> WorldHost<A> {
    fn object(&self, reference: ObjectRef) -> Result<&WorldObject, VmError> {
        self.world
            .get(reference)
            .ok_or_else(|| stale_reference(reference))
    }
    fn change_hp(&mut self, reference: ObjectRef, amount: i32) -> Result<(), VmError> {
        let context = self.world.context(reference);
        let object = self
            .world
            .get_mut(reference)
            .ok_or_else(|| VmError::Host("unsafe null critter HP request".into()))?;
        let sheet = object
            .character
            .as_mut()
            .ok_or_else(|| VmError::Host("HP request requires a critter".into()))?;
        sheet.heal(amount, context);
        if let ObjectState::Critter(state) = &mut object.raw.state {
            state.hp = sheet.current_hp;
            if state.hp <= 0 {
                state.results.0 |= 0x80;
            }
        }
        Ok(())
    }
    fn queue_animation(&mut self, word: u16, values: Vec<i32>, sound: Option<Vec<u8>>) {
        self.world.emit(WorldEffect::Animation {
            opcode: word,
            values: values.clone(),
            sound: sound.clone(),
        });
        if let Some(sequence) = &mut self.building_animation {
            if sequence.commands.len() < crate::choreography::CAPACITY {
                if let Some(sound) = sound {
                    sequence.sounds.insert(sequence.commands.len(), sound);
                }
                sequence.commands.push((word, values));
            } else {
                self.world
                    .diagnostic("animation sequence is full; registration dropped");
            }
        } else {
            // An entry outside any registration is its own unreserved
            // sequence.
            let mut sequence = AnimationSequence {
                flags: 1,
                commands: vec![(word, values)],
                ..Default::default()
            };
            if let Some(sound) = sound {
                sequence.sounds.insert(0, sound);
            }
            self.register_script_sequence(&sequence);
        }
    }
    pub(crate) fn clear_animation(&mut self, object: ObjectRef) {
        self.cancel_motions(object);
    }
    fn kill(&mut self, vm: &mut Vm, object: ObjectRef, animation: i32) -> Result<(), VmError> {
        if object == 0 {
            return Ok(());
        }
        self.kill_critter(object, animation)?;
        if object == self.self_object(vm) {
            vm.status.0 |= 0x100;
        }
        Ok(())
    }
    /// The kill path: animations cleared, dead status and zero HP, the
    /// death art, queued events removed, and the corpse presented.
    pub(crate) fn kill_critter(
        &mut self,
        object: ObjectRef,
        animation: i32,
    ) -> Result<(), WorldError> {
        self.clear_animation(object);
        let art = vault_formats::Fid(
            self.world
                .get(object)
                .ok_or_else(|| WorldError::Invalid("invalid killed object".into()))?
                .raw
                .art_id,
        );
        let orientation = self.world.get(object).map_or(0, |o| o.raw.orientation);
        let fid = |host: &Self, animation: u8| {
            if art.category() == 1 {
                host.assets.construct_fid(
                    1,
                    art.base(),
                    animation,
                    art.weapon(),
                    vault_formats::Fid::selector_for_orientation(orientation),
                )
            } else {
                (art.0 & !0x00ff0000) | (u32::from(animation) << 16)
            }
        };
        // SPEC-007 §3.8: in direct/static-death callers, static codes 51-63
        // revert to forced fall-back 20 without maximum violence or art.
        let mut animation = (animation & 255) as u8;
        if (51..=63).contains(&animation)
            && (self.world.violence.unwrap_or(2) < 3
                || self.assets.object_art(fid(self, animation)).is_err())
        {
            animation = 20;
        }
        let art_id = fid(self, animation);
        let target = self.world.get_mut(object).expect("checked object");
        if let Some(sheet) = &mut target.character {
            sheet.current_hp = 0;
        }
        if let ObjectState::Critter(state) = &mut target.raw.state {
            state.hp = 0;
            state.results.0 |= 0x80;
            state.maneuvers = 0;
        }
        target.raw.art_id = art_id;
        target.opponent = 0;
        self.world.queue.remove_object(object);
        self.present_corpse(object, animation)
    }
    fn destroy(&mut self, vm: &mut Vm, object: ObjectRef) -> Result<(), VmError> {
        let owned = self.object(object)?.owner.is_some();
        self.clear_animation(object);
        if object == self.self_object(vm) {
            vm.status.0 |= 0x100;
            if owned {
                let target = self.world.get_mut(object).expect("object");
                target.raw.flags.0 |= BasicFlags::HIDDEN.0 | BasicFlags::TEMPORARY.0;
                target.raw.script_id = -1;
                if let Some(sid) = self.sid(vm) {
                    self.world.remove_script(sid);
                }
                return Ok(());
            }
        }
        self.world.erase(object);
        self.world.rebuild_lighting();
        Ok(())
    }
    fn worn(&self, item: ObjectRef) -> bool {
        self.world
            .get(item)
            .is_some_and(|object| object.raw.flags.contains(BasicFlags::WORN))
    }
    /// SPEC-013 §3.19-3.20: `poison` on another critter emits its
    /// diagnostic; `radiation_inc`/`radiation_dec` there do nothing.
    fn poison_or_radiation(
        &mut self,
        object: ObjectRef,
        amount: i32,
        radiation: bool,
    ) -> Result<(), WorldError> {
        if radiation {
            self.adjust_radiation(object, amount);
        } else if !self.adjust_poison(object, amount)? {
            self.world.diagnostic("only player poison is adjustable");
        }
        Ok(())
    }
    /// The `attack`/`attack_setup` request body (SPEC-005 §3.5). A null
    /// `attack_setup` defender is a no-op with a diagnostic (safe rule).
    fn attack_request(&mut self, vm: &Vm, op: u16, call: &HostCall) -> Result<(), VmError> {
        let (attacker, defender, accuracy, min, max, mode) = if op == 0x8143 {
            if call.uint(0) == 0 {
                self.world.diagnostic("attack_setup: null defender");
                return Ok(());
            }
            (call.uint(1), call.uint(0), 0, 0, i32::MAX, None)
        } else {
            (
                self.self_object(vm),
                call.uint(7),
                call.int(4),
                call.int(3),
                call.int(2),
                (call.int(0) == call.int(1)).then_some([call.int(1), call.int(0)]),
            )
        };
        // The eight-operand words also require a nonfleeing target.
        let fleeing = |object: &crate::world::WorldObject| matches!(&object.raw.state, ObjectState::Critter(state) if state.maneuvers & 4 != 0);
        if attacker != 0
            && defender != 0
            && self.object(attacker)?.alive()
            && self.object(defender)?.alive()
            && (op == 0x8143 || !fleeing(self.object(defender)?))
        {
            if self.world.combat {
                // SPEC-009: with alert clear, set it and reference the
                // target directly (no reaction change).
                let attacker = self.world.get_mut(attacker).expect("attacker");
                if let ObjectState::Critter(state) = &mut attacker.raw.state
                    && state.maneuvers & 1 == 0
                {
                    state.maneuvers |= 1;
                    attacker.opponent = defender;
                }
            } else {
                self.world.emit(WorldEffect::Combat {
                    attacker,
                    defender,
                    accuracy,
                    min,
                    max,
                    mode,
                });
                self.world.pending_combat = Some(crate::combat_state::CombatRequest {
                    attacker,
                    defender,
                    accuracy,
                    damage: [min, max],
                    flags: mode,
                });
            }
        }
        Ok(())
    }
    /// The `critter_damage` body: the fixed amount as both damage bounds.
    fn critter_damage(&mut self, op: u16, call: &HostCall, values: &[i32]) -> Result<(), VmError> {
        if call.uint(2) == 0 || self.object(call.uint(2))?.category() != Category::Critter {
            return Ok(());
        }
        // Fixed amount as both damage bounds: through the victim's DT
        // then DR for the low-byte damage type unless 0x100 bypasses
        // armor; 0x200 suppresses the animation request.
        let (flags, victim) = (call.int(0) as u32, call.uint(2));
        let damage = if flags & 0x100 != 0 {
            call.int(1).max(0)
        } else {
            let context = self.world.context(victim);
            let kind = (flags & 0xff) as usize;
            let sheet = self.object(victim)?.character.as_ref();
            let armor = |base: usize| {
                if kind < 7 {
                    sheet.map_or(0, |s| i64::from(s.stat(base + kind, context)))
                } else {
                    0
                }
            };
            let mut value = i64::from(call.int(1)) - armor(17);
            if value > 0 {
                value -= value * armor(24) / 100;
            }
            value.clamp(0, i64::from(i32::MAX)) as i32
        };
        let was_dead = self.world.is_dead(victim);
        self.damage_critter(victim, None, damage, 0)?;
        let died = !was_dead && self.world.is_dead(victim);
        // SPEC-007 §3.11.5: shown as a temporary attacker's hit, or without
        // animation a lethal amount kills at once (the static fall-back).
        if flags & 0x200 == 0 {
            self.world.emit(WorldEffect::Animation {
                opcode: op,
                values: values.to_vec(),
                sound: None,
            });
            self.present_script_damage(
                victim,
                damage,
                (flags & 0xff) as i32,
                flags & 0x100 != 0,
                died,
            )?;
        } else if died {
            self.kill_critter(victim, 48)?;
        }
        Ok(())
    }
    /// `turn_off_objs_in_area` / `turn_on_objs_in_area` under the SPEC-005
    /// §3.4 safe rules: reversed hex bounds are swapped and each pass
    /// advances one elevation, so the call terminates. A placed object
    /// matches with a hex between the bounds and
    /// `trunc((hex - first) / 200) <= trunc(last / 200) - trunc(first / 200)`.
    fn area_visibility(&mut self, hide: bool, hexes: [i32; 2], elevations: [i32; 2]) {
        let [first, last] = if hexes[0] > hexes[1] {
            [hexes[1], hexes[0]]
        } else {
            hexes
        };
        let low = elevations[0].min(elevations[1]).max(0);
        let high = elevations[0].max(elevations[1]).min(2);
        let rows = last / 200 - first / 200;
        let mut changed = false;
        for elevation in low..=high {
            let matching: Vec<ObjectRef> = self
                .world
                .iter()
                .filter(|(_, object)| {
                    object.owner.is_none()
                        && i32::from(object.raw.elevation) == elevation
                        && object.raw.hex.is_some_and(|hex| {
                            (first..=last).contains(&hex) && (hex - first) / 200 <= rows
                        })
                        && object.raw.flags.contains(BasicFlags::HIDDEN) != hide
                })
                .map(|(reference, _)| reference)
                .collect();
            for reference in matching {
                if self.world.get(reference).is_some() {
                    self.world.set_hidden(reference, hide);
                    changed = true;
                }
            }
        }
        if changed {
            self.world.rebuild_lighting();
        }
    }
    fn proto_data(&mut self, pid: u32, member: i32) -> HostResult {
        let Ok(prototype) = self.assets.prototype(pid) else {
            return Ok(integer(0));
        };
        if prototype.category == Category::Tile {
            return Ok(integer(0));
        }
        if matches!(member, 1 | 2) {
            let text = self
                .assets
                .prototype_text(&prototype, member == 2)
                .unwrap_or_default();
            // SPEC-005 §3.6 safe rule: the miscellaneous description is D
            // like the other categories (the reference returns I bits).
            return Ok(string(text));
        }
        let common = match member {
            0 => Some(prototype.pid as i32),
            3 => Some(prototype.art_id as i32),
            4 => Some(prototype.light.map_or(0, |l| l.radius)),
            5 => Some(prototype.light.map_or(0, |l| l.intensity)),
            6 => Some(prototype.flags.0 as i32),
            7 => Some(prototype.extended.0 as i32),
            8 if prototype.category != Category::Misc => prototype.script,
            _ => None,
        };
        let value = common.unwrap_or_else(|| match (&prototype.data, member) {
            (PrototypeData::Item(item), 9) => item.data.subtype() as i32,
            (PrototypeData::Item(item), 11) => item.material,
            (PrototypeData::Item(item), 12) => item.size,
            (PrototypeData::Item(item), 13) => item.weight,
            (PrototypeData::Item(item), 14) => item.cost,
            (PrototypeData::Item(item), 15) => item.inventory_art as i32,
            (PrototypeData::Critter(critter), 10) => critter.head_art as i32,
            (PrototypeData::Scenery(scenery), 9) => scenery.data.subtype() as i32,
            (PrototypeData::Scenery(scenery), 11) => scenery.material,
            (PrototypeData::Wall { material }, 9) => *material,
            _ => 0,
        });
        Ok(integer(value))
    }
    pub(crate) fn game_call(&mut self, vm: &mut Vm, call: &HostCall) -> HostResult {
        match self.game_call_row(vm, call) {
            // SPEC-005 §2.1 safe rule: a stale nonzero reference gives a
            // diagnostic and the row's null result, not a VM error.
            Err(VmError::Host(message))
                if message.starts_with(STALE_REFERENCE) || message.starts_with(HOST_FAILURE) =>
            {
                let op = call.spec.word;
                self.world.diagnostic(format!("{op:#06x}: {message}"));
                Ok(null_result(op))
            }
            result => result,
        }
    }
    fn game_call_row(&mut self, vm: &mut Vm, call: &HostCall) -> HostResult {
        let op = call.spec.word;
        let values: Vec<_> = call.args.iter().map(|arg| arg.int()).collect();
        let result = match op {
            0x80a1 => {
                self.give_experience(call.int(0));
                None
            }
            0x80a2 => {
                if let Some(sid) = self.sid(vm)
                    && let Some(script) = self.world.scripts.get_mut(&sid)
                {
                    script.result = call.int(0);
                }
                None
            }
            0x80a3 => {
                // Direct playback: no owning object, so no gender alias.
                self.world.emit(WorldEffect::SoundEffect {
                    name: call.text(0).unwrap_or(b"").to_vec(),
                    owner: None,
                });
                None
            }
            // Safe rule: a null operand always names `<Critter>` (the
            // reference reuses the last cached name).
            0x80a4 => {
                if call.uint(0) != 0 {
                    string(self.object(call.uint(0))?.name.clone())
                } else {
                    self.world.diagnostic("obj_name: null object");
                    string(b"<Critter>".to_vec())
                }
            }
            0x80a5 | 0x813d => {
                let object = call.uint(if op == 0x80a5 { 1 } else { 2 });
                if object == 0 {
                    dynamic_null()
                } else {
                    self.object(object)?;
                    // SPEC-010 builders; "no name" (an unchecked copy in the
                    // reference) pushes D offset zero with a diagnostic.
                    let byte = |index| u8::try_from(call.int(index)).ok();
                    let name = if op == 0x80a5 {
                        byte(0).and_then(|action| self.open_sound_name(object, action))
                    } else {
                        byte(1).zip(byte(0)).and_then(|(animation, extra)| {
                            self.character_sound_name(object, animation, extra)
                        })
                    };
                    // Safe rule for `sfx_build_char_name`: unavailable art or
                    // name pushes empty D text.
                    match name {
                        Some(name) => string(name),
                        None => {
                            self.world
                                .diagnostic(format!("opcode {op:#06x}: no sound name"));
                            if op == 0x813d {
                                string(Vec::new())
                            } else {
                                dynamic_null()
                            }
                        }
                    }
                }
            }
            0x80a6 => integer(self.world.player_sheet().map_or(0, |s| match call.int(0) {
                0 => s.unspent_skills,
                1 => s.level,
                2 => s.experience,
                3 => s.reputation,
                4 => s.karma,
                _ => 0,
            })),
            0x80a7 | 0x80bb => {
                let found = if call.int(2) == -1 {
                    0
                } else {
                    self.world
                        .iter()
                        .find(|(_, o)| {
                            o.owner.is_none()
                                && o.raw.pid == call.uint(0)
                                && o.raw.hex == Some(call.int(2))
                                && i32::from(o.raw.elevation) == call.int(1)
                        })
                        .map_or(0, |(r, _)| r)
                };
                integer(if op == 0x80bb {
                    i32::from(found != 0)
                } else {
                    found as i32
                })
            }
            0x80a8 | 0x80a9 => {
                let hex = i64::from(call.int(2)) * 200 + i64::from(call.int(3));
                let elevation = call.int(1);
                let orientation = call.int(0);
                let valid = (0..40000).contains(&hex)
                    && (0..3).contains(&elevation)
                    && (0..6).contains(&orientation);
                if op == 0x80a8 {
                    if valid {
                        self.world.set_current_elevation(elevation as u8);
                        self.world.set_view_center(hex as i32);
                        self.world.map.header.entry_hex = hex as i32;
                        self.world.map.header.entry_elevation = elevation as u8;
                        self.world.map.header.entry_orientation = orientation as u8;
                    } else {
                        self.world.diagnostic("map start/camera selection failed");
                    }
                } else {
                    let destination = if valid {
                        hex as i32
                    } else {
                        self.world.camera_hex
                    };
                    if !(0..3).contains(&elevation) {
                        return Err(VmError::Host("unrecoverable entry override".into()));
                    }
                    // An orientation outside 0-5 is diagnosed before anything
                    // moves (safe rule: the reference moves, then fails).
                    let facing = u8::try_from(orientation)
                        .ok()
                        .filter(|n| *n < 6)
                        .ok_or_else(|| host_failure("override_map_start orientation"))?;
                    self.world
                        .move_object(self.world.player, destination, elevation as u8, false)
                        .map_err(host_failure)?;
                    if let Some(player) = self.world.get_mut(self.world.player) {
                        player.raw.orientation = facing;
                    }
                }
                None
            }
            0x80aa => {
                let value = self
                    .world
                    .get(call.uint(1))
                    .filter(|o| o.category() == Category::Critter)
                    .and_then(|o| o.character.as_ref())
                    .map_or(0, |sheet| {
                        sheet.skill(call.uint(0) as usize, self.world.context(call.uint(1)))
                    });
                integer(value)
            }
            0x80ab => integer(i32::from(
                call.int(0) == 8
                    && call.uint(1) == self.world.player
                    && self.world.player_sheet().is_some_and(|s| s.flags & 1 != 0),
            )),
            0x80ac | 0x80ae => {
                let object = call.uint(2);
                let index = call.int(1);
                let sid = self.sid(vm);
                let sheet = self
                    .world
                    .get(object)
                    .filter(|o| o.category() == Category::Critter)
                    .and_then(|o| o.character.as_ref());
                if let (Some(sheet), Some(sid)) = (sheet, sid) {
                    let context = self.world.context(object);
                    if op == 0x80ac && !(0..18).contains(&index) {
                        integer(1)
                    } else if op == 0x80ae && !(0..7).contains(&index) {
                        integer(0)
                    } else {
                        let value = if op == 0x80ac {
                            sheet.skill(index as usize, context)
                        } else {
                            sheet.stat(index as usize, context)
                        };
                        let critical = sheet.stat(15, context);
                        let check = if op == 0x80ac {
                            rng::skill_check(
                                value,
                                call.int(0),
                                critical,
                                self.world.clock.ticks,
                                &mut self.world.rng,
                            )?
                        } else {
                            rng::stat_check(value, call.int(0), &mut self.world.rng)?
                        };
                        if let Some(script) = self.world.scripts.get_mut(&sid) {
                            script.margin = check.margin;
                        }
                        integer(check.code)
                    }
                } else {
                    integer(0)
                }
            }
            0x80ad | 0x80b2 | 0x80b3 | 0x80b5 => {
                self.world.diagnostic(format!(
                    "{} is a defined zero-returning stub",
                    call.spec.name
                ));
                integer(0)
            }
            0x80af => integer(match call.int(0) {
                0 | 1 => 0,
                2 | 3 => 1,
                _ => -1,
            }),
            0x80b0 => integer(match call.int(0) {
                0 | 3 => 1,
                1 | 2 => 0,
                _ => -1,
            }),
            0x80b1 => integer(
                self.sid(vm)
                    .and_then(|sid| self.world.scripts.get(&sid))
                    .map_or(0, |script| script.margin),
            ),
            0x80b4 => integer(if self.world.recording {
                call.int(0)
                    .checked_sub(call.int(1))
                    .ok_or(VmError::Arithmetic)?
                    / 2
            } else {
                self.world.rng.range(call.int(1), call.int(0))?
            }),
            0x80b6 => {
                let result = if call.uint(2) == 0 {
                    false
                } else if (0..3).contains(&call.int(0)) {
                    self.world
                        .move_object(call.uint(2), call.int(1), call.int(0) as u8, false)
                        .is_ok()
                } else {
                    false
                };
                integer(if result { 0 } else { -1 })
            }
            0x80b7 => {
                if self.world.restoring {
                    integer(0)
                } else {
                    // SPEC-005 §2.5 safe rule: when no object can be created,
                    // push I zero and skip the script replacement.
                    let hex = if call.int(2) == -1 { 0 } else { call.int(2) };
                    let created = self.assets.prototype(call.uint(3)).and_then(|prototype| {
                        let elevation = u8::try_from(call.int(1))
                            .map_err(|_| WorldError::Invalid("elevation outside 0-255".into()))?;
                        let object = self.world.spawn(prototype.clone(), hex, elevation)?;
                        Ok((prototype, object))
                    });
                    let (prototype, object) = match created {
                        Ok(created) => created,
                        Err(error) => {
                            self.world.diagnostic(format!("create_object_sid: {error}"));
                            return Ok(integer(0));
                        }
                    };
                    let default_script = prototype.script.unwrap_or(-1);
                    if let Some(target) = self.world.get_mut(object) {
                        target.name = self
                            .assets
                            .prototype_text(&prototype, false)
                            .unwrap_or_else(|| target.name.clone());
                        target.description = self
                            .assets
                            .prototype_text(&prototype, true)
                            .unwrap_or_default();
                    }
                    // Script number 0 is treated like -1 (§3.4 safe rule).
                    let file = if matches!(call.int(0), -1 | 0) {
                        if default_script == -1 {
                            None
                        } else {
                            Some(default_script & 0xffffff)
                        }
                    } else {
                        Some(call.int(0).saturating_sub(1))
                    };
                    if let Some(file) = file {
                        let category = match prototype.category {
                            Category::Critter => 4,
                            Category::Item | Category::Scenery => 3,
                            _ => 0,
                        };
                        if let Err(error) = self.world.add_script(
                            object,
                            file,
                            category,
                            self.assets.local_count(file),
                        ) {
                            self.world.diagnostic(error.to_string());
                        }
                    }
                    integer(object as i32)
                }
            }
            0x80b8 | 0x8154 => {
                // The debug text is emitted literally (SPEC-005 §3.1 safe
                // rule), never as a native format.
                let text = call.text(0).unwrap_or(b"").to_vec();
                if op == 0x80b8 || self.world.debug_messages {
                    self.world.emit(WorldEffect::Message(text));
                }
                None
            }
            0x80b9 => {
                if let Some(sid) = self.sid(vm)
                    && let Some(script) = self.world.scripts.get_mut(&sid)
                {
                    script.overridden = true;
                }
                None
            }
            0x80ba => integer(self.world.inventory_count(call.uint(1), call.uint(0))),
            0x80bc => integer(self.self_object(vm) as i32),
            0x80bd => integer(
                self.sid(vm)
                    .and_then(|sid| self.world.scripts.get(&sid))
                    .map_or(0, |s| s.source as i32),
            ),
            0x80be | 0x80c0 => integer(self.target(vm) as i32),
            0x80bf => integer(self.world.player as i32),
            0x80c1 => integer(self.world.local(self.sid(vm), call.int(0))),
            0x80c2 => {
                if !self.world.set_local(self.sid(vm), call.int(1), call.int(0)) {
                    self.world
                        .diagnostic("LVAR write outside the script's slice");
                }
                None
            }
            0x80c3 => {
                let value = usize::try_from(call.int(0))
                    .ok()
                    .and_then(|i| self.world.mvars.get(i))
                    .copied();
                if value.is_none() {
                    self.world
                        .diagnostic(format!("MVAR read {} out of bounds", call.int(0)));
                }
                integer(value.unwrap_or(0))
            }
            0x80c4 => {
                if let Some(slot) = usize::try_from(call.int(1))
                    .ok()
                    .and_then(|i| self.world.mvars.get_mut(i))
                {
                    *slot = call.int(0);
                } else {
                    self.world.diagnostic("MVAR write out of bounds");
                }
                None
            }
            0x80c5 => integer(self.world.gvars.as_ref().map_or(-1, |array| {
                usize::try_from(call.int(0))
                    .ok()
                    .and_then(|i| array.get(i))
                    .copied()
                    .unwrap_or(0)
            })),
            0x80c6 => {
                if let Some(slot) = self.world.gvars.as_mut().and_then(|array| {
                    usize::try_from(call.int(1))
                        .ok()
                        .and_then(|i| array.get_mut(i))
                }) {
                    *slot = call.int(0);
                } else {
                    self.world.diagnostic("GVAR write out of bounds/absent");
                }
                None
            }
            0x80c7 => integer(
                self.sid(vm)
                    .and_then(|sid| self.world.scripts.get(&sid))
                    .map_or(0, |s| s.event),
            ),
            0x80c8 => integer(if call.uint(0) == 0 {
                -1
            } else {
                ((self.object(call.uint(0))?.raw.art_id >> 24) & 15) as i32
            }),
            0x80c9 => integer(
                self.world
                    .get(call.uint(0))
                    .filter(|o| o.category() == Category::Item)
                    .and_then(|o| o.prototype.as_ref())
                    .map_or(-1, |p| match &p.data {
                        PrototypeData::Item(item) => item.data.subtype() as i32,
                        _ => -1,
                    }),
            ),
            0x80ca => integer(if call.uint(1) == 0 {
                -1
            } else {
                self.object(call.uint(1))?
                    .character
                    .as_ref()
                    .ok_or_else(|| host_failure("stat host requires critter"))?
                    .stat(call.uint(0) as usize, self.world.context(call.uint(1)))
            }),
            0x80cb => {
                if call.uint(2) == 0 || call.uint(2) != self.world.player {
                    integer(-1)
                } else {
                    let context = self.world.context(self.world.player);
                    if let Some(sheet) = self.world.player_sheet_mut()
                        && let Some(base) = sheet.base.get(call.uint(1) as usize).copied()
                        && let Some(value) = base.checked_add(call.int(0))
                    {
                        sheet.set_base(call.uint(1) as usize, value, context);
                    }
                    integer(0)
                }
            }
            0x80cc | 0x80cd => {
                let object = if call.uint(0) == 0 {
                    self.self_object(vm)
                } else {
                    call.uint(0)
                };
                if object != 0 && !self.world.combat {
                    self.object(object)?;
                    self.queue_animation(op, vec![object as i32], None);
                }
                None
            }
            0x80ce => {
                if !self.world.combat
                    && self.sid(vm).is_some()
                    && call.uint(2) != 0
                    && call.int(1) > -1
                    && self.object(call.uint(2))?.alive()
                {
                    if call.int(0) & 0x10 != 0 {
                        self.clear_animation(call.uint(2));
                    }
                    self.queue_animation(
                        op,
                        vec![call.int(0) & !0x10, call.int(1), call.int(2)],
                        None,
                    );
                }
                None
            }
            0x80cf => {
                self.world
                    .diagnostic("animate_jump is unsupported and consumes no operands");
                None
            }
            // SPEC-005 §3.5 safe rule: the host-busy bit is cleared on every
            // return path, including the active-dialogue early return.
            0x80d0 | 0x80dd | 0x8143 => {
                vm.status.0 |= 0x20;
                let result = if self.world.dialogue.open {
                    Ok(())
                } else {
                    self.attack_request(vm, op, call)
                };
                vm.status.0 &= !0x20;
                result?;
                None
            }
            0x80d1 => None,
            0x80d2 => {
                integer(HexGrid::angular_distance(call.int(1), call.int(0)).map_err(host_failure)?)
            }
            0x80d3 => {
                let (a, b) = (call.uint(1), call.uint(0));
                if a < 40000 || b < 40000 {
                    integer(9999)
                } else {
                    let (a, b) = (self.object(a)?, self.object(b)?);
                    integer(if a.raw.elevation != b.raw.elevation {
                        9999
                    } else {
                        HexGrid::angular_distance(a.raw.hex.unwrap_or(-1), b.raw.hex.unwrap_or(-1))
                            .map_err(|e| VmError::Host(e.to_string()))?
                    })
                }
            }
            0x80d4 => integer(if call.uint(0) == 0 {
                -1
            } else {
                self.object(call.uint(0))?.raw.hex.unwrap_or(-1)
            }),
            // A negative orientation also gives -1 (SPEC-005 §3.4 safe rule).
            0x80d5 => {
                if call.int(2) == -1 || !(0..6).contains(&call.int(1)) || call.int(0) == 0 {
                    integer(-1)
                } else {
                    integer(
                        HexGrid::step(call.int(2), call.int(1) as u8, call.int(0))
                            .map_err(host_failure)?,
                    )
                }
            }
            0x80d6 | 0x80d7 => {
                let item = call.uint(0);
                let actor = self.target(vm);
                // Safe rule: with no script for the program, diagnose and
                // return without action.
                if op == 0x80d6 && item != 0 && self.sid(vm).is_none() {
                    self.world.diagnostic("pickup_obj without a script context");
                    return Ok(None);
                }
                if item != 0 && actor != 0 {
                    let result = if op == 0x80d6 {
                        self.world.add_item(actor, item, 1, false)
                    } else {
                        self.world.remove_item(actor, item, 1)
                    };
                    if let Err(error) = result {
                        self.world.diagnostic(error.to_string());
                    }
                }
                None
            }
            0x80d8 | 0x80d9 | 0x8116 => {
                let (item, owner, quantity) = if op == 0x8116 {
                    (call.uint(1), call.uint(2), call.int(0))
                } else {
                    (call.uint(0), call.uint(1), 1)
                };
                if item != 0 && owner != 0 {
                    let worn = self.worn(item);
                    let result = if op == 0x80d9 {
                        self.world.remove_item(owner, item, quantity)
                    } else {
                        self.world.add_item(owner, item, quantity, op == 0x8116)
                    };
                    match result {
                        Err(error) => self.world.diagnostic(error.to_string()),
                        // SPEC-005 §3.8: worn armor taken off the player
                        // leaves the default player art.
                        Ok(()) if worn && op == 0x80d9 => self.unworn_art(owner),
                        Ok(()) => {}
                    }
                }
                None
            }
            0x80da => {
                let (item, critter) = (call.uint(0), call.uint(1));
                if item != 0
                    && critter != 0
                    && self.object(critter)?.category() == Category::Critter
                {
                    self.object(item)?;
                    if let Err(error) = self.wield_object(critter, item) {
                        self.world.diagnostic(error.to_string());
                    }
                }
                None
            }
            0x80db => {
                let actor = self.target(vm);
                if call.uint(0) != 0
                    && actor != 0
                    && self.sid(vm).is_some()
                    && let Err(error) = self.action(actor, call.uint(0), ObjectAction::Use)
                {
                    self.world.diagnostic(error.to_string());
                }
                None
            }
            0x80dc => {
                // SPEC-005/SPEC-007: both references and a placed target, the
                // perception test, then a stride-16 ray whose first reported
                // movement blocker is the target itself.
                let (target, observer) = (call.uint(0), call.uint(1));
                if target == 0 || observer == 0 {
                    self.world
                        .diagnostic("obj_can_see_obj requires two objects");
                    integer(0)
                } else {
                    integer(i32::from(self.world.can_see(observer, target)))
                }
            }
            0x80de => {
                self.start_dialogue(vm, call.uint(3), call.int(2), call.int(1), call.int(0))?;
                None
            }
            0x80df => {
                self.end_session();
                None
            }
            0x80e0 => {
                self.world.dialogue.reaction = call.int(0);
                if let Some(head) = &mut self.world.dialogue.head {
                    head.react(match call.int(0) {
                        -1 => 49,
                        1 => 51,
                        _ => 50,
                    });
                }
                None
            }
            0x80e1 | 0x80e2 => {
                self.area_visibility(
                    op == 0x80e1,
                    [call.int(1), call.int(0)],
                    [call.int(3), call.int(2)],
                );
                None
            }
            0x80e3 => {
                if call.uint(1) != 0 && !self.world.restoring {
                    let object = self
                        .world
                        .get_mut(call.uint(1))
                        .ok_or_else(|| VmError::Host("invalid visibility object".into()))?;
                    let mask = if object.category() == Category::Critter {
                        BasicFlags::NONBLOCKING.0
                    } else {
                        0
                    };
                    let hide = call.int(0) != 0;
                    if hide {
                        object.raw.flags.0 |= mask;
                    } else {
                        object.raw.flags.0 &= !mask;
                    }
                    // SPEC-014 §3.44: hiding disables the outline, showing
                    // enables it.
                    self.world.set_hidden(call.uint(1), hide);
                    self.world.rebuild_lighting();
                }
                None
            }
            0x80e4 => {
                // SPEC-005 0x80E4 / SPEC-008 §3.10: a name is matched
                // against `map.msg` 0-65 (its campaign parameter is stored
                // even when the name fails to match); a negative numeric
                // index stores nothing. An unmatched name or a negative
                // index makes no transition.
                let index = if let Some(name) = call.text(1) {
                    if let Some(slot) = self.world.gvars.as_mut().and_then(|a| a.get_mut(32)) {
                        *slot = call.int(0);
                    }
                    self.assets.map_index(name)
                } else if call.int(1) >= 0 {
                    if let Some(slot) = self.world.gvars.as_mut().and_then(|a| a.get_mut(32)) {
                        *slot = call.int(0);
                    }
                    Some(call.int(1))
                } else {
                    None
                };
                // SPEC-005: a transition to the map's default entrance
                // (hex, elevation and orientation all -1).
                if let Some(index) = index.filter(|index| *index >= 0) {
                    self.world.emit(WorldEffect::MapTransition {
                        index,
                        parameter: call.int(0),
                    });
                    self.request_transition(crate::world::Transition {
                        map: index,
                        elevation: -1,
                        hex: -1,
                        facing: -1,
                    });
                }
                None
            }
            0x80e5 | 0x80e6 => None,
            0x80e7 => integer(if self.animation_busy(call.uint(0)) {
                -1
            } else {
                0
            }),
            // A null object pushes I zero with no effect (safe rule).
            0x80e8 => {
                if call.uint(1) != 0 {
                    self.change_hp(call.uint(1), call.int(0))
                        .map_err(host_failure)?;
                }
                integer(0)
            }
            0x80e9 => {
                self.world.ambient_percent(call.int(0));
                None
            }
            // Time arithmetic wraps as 32-bit two's complement (safe rule).
            0x80ea => integer(self.world.clock.ticks as u32 as i32),
            0x80eb => integer((self.world.clock.ticks as u32 / 10) as i32),
            0x80ec => integer(if call.uint(0) == 0 {
                0
            } else {
                i32::from(self.object(call.uint(0))?.raw.elevation)
            }),
            0x80ed => {
                if self.world.restoring {
                    self.world
                        .diagnostic("single kill during restoration proceeds");
                }
                self.kill(vm, call.uint(1), call.int(0))
                    .map_err(host_failure)?;
                None
            }
            0x80ee => {
                if !self.world.restoring {
                    let objects: Vec<_> = self
                        .world
                        .iter()
                        .filter(|(_, o)| {
                            o.raw.pid == call.uint(1)
                                && o.alive()
                                && !o.raw.flags.contains(BasicFlags::HIDDEN)
                                && ((o.raw.art_id >> 16) & 255) < 48
                        })
                        .map(|(r, _)| r)
                        .collect();
                    for (index, object) in objects.into_iter().take(100_000).enumerate() {
                        if call.int(0) == 0 {
                            self.world.erase(object);
                        } else {
                            self.kill(
                                vm,
                                object,
                                if call.int(0) == 1 {
                                    [62, 51, 52, 53, 63, 62, 54, 56, 59, 62, 63][index % 11]
                                } else {
                                    48
                                },
                            )?;
                        }
                    }
                    self.world.map.header.last_visit = self.world.clock.ticks as u32;
                }
                None
            }
            // SPEC-005 §3.5 safe rule: the host-busy bit is cleared on every
            // return path, including the null/non-critter early return.
            0x80ef => {
                vm.status.0 |= 0x20;
                let result = self.critter_damage(op, call, &values);
                vm.status.0 &= !0x20;
                result?;
                None
            }
            0x80f0 => {
                if call.uint(2) != 0
                    && let Some(script) = self.world.script_for(call.uint(2))
                {
                    self.world
                        .queue
                        .insert(
                            self.world.clock.ticks,
                            i64::from(call.int(1)),
                            call.uint(2),
                            EventKind::Script {
                                script,
                                parameter: call.int(0),
                            },
                        )
                        .map_err(host_failure)?;
                }
                None
            }
            0x80f1 => {
                if call.uint(0) != 0 {
                    self.world.queue.remove_object(call.uint(0));
                }
                None
            }
            0x80f2 => integer(call.int(0).max(0).wrapping_mul(10)),
            0x80f3 => {
                let (selector, object, kind) = (call.int(0), call.uint(1), call.int(2));
                if object == 0 {
                    integer(0)
                } else {
                    let target = self.object(object)?;
                    let result = match kind {
                        // A negative perk or trait index gives 0 (safe rule).
                        0 if selector < 0 => 0,
                        0 => Perk::from_index(selector).map_or(0, |perk| self.world.perk(perk))
                            as i32,
                        2 => i32::from(
                            (0..16).contains(&selector)
                                && self
                                    .world
                                    .player_sheet()
                                    .is_some_and(|s| s.traits.contains(&selector)),
                        ),
                        1 => match selector {
                            5 => {
                                if let ObjectState::Critter(s) = &target.raw.state {
                                    s.ai
                                } else {
                                    0
                                }
                            }
                            6 => {
                                if let ObjectState::Critter(s) = &target.raw.state {
                                    s.team
                                } else {
                                    0
                                }
                            }
                            10 => i32::from(target.raw.orientation),
                            666 => i32::from(!target.raw.flags.contains(BasicFlags::HIDDEN)),
                            669 => i32::try_from(self.world.inventory_weight(object))
                                .map_err(|_| VmError::Arithmetic)?,
                            _ => 0,
                        },
                        _ => 0,
                    };
                    integer(result)
                }
            }
            0x80f4 => {
                if call.uint(0) != 0
                    && !(self.world.restoring
                        && self.object(call.uint(0))?.category() == Category::Critter)
                {
                    self.destroy(vm, call.uint(0))?;
                }
                None
            }
            // SPEC-005 §2.6 safe rule: 1 only for two non-null objects on
            // the same elevation, both placed, when the perception test
            // passes (the reference's guard is inverted).
            0x80f5 => {
                let (first, second) = (call.uint(1), call.uint(0));
                if first == 0 || second == 0 {
                    self.world
                        .diagnostic("obj_can_hear_obj with a null operand returns 0");
                    integer(0)
                } else {
                    let (hearer, heard) = (self.object(first)?, self.object(second)?);
                    let placed = hearer.raw.hex.is_some() && heard.raw.hex.is_some();
                    let level = hearer.raw.elevation == heard.raw.elevation;
                    integer(i32::from(
                        placed && level && self.world.can_perceive(first, second),
                    ))
                }
            }
            0x80f6 => integer(self.world.clock.hhmm()),
            0x80f7 => integer(
                self.sid(vm)
                    .and_then(|sid| self.world.scripts.get(&sid))
                    .map_or(0, |s| s.fixed),
            ),
            0x80f8 => {
                let difference = (i64::from(self.world.camera_hex) - i64::from(call.int(0))).abs();
                integer(i32::from(difference % 200 < 5 || difference / 200 < 5))
            }
            // SPEC-005 §3.3: the dialogue game state is requested with self
            // as the target; the next tick enters it outside this program.
            0x80f9 => {
                let owner = self.self_object(vm);
                if owner != 0 && !self.world.combat && self.object(owner)?.alive() {
                    self.world.dialogue.target = owner;
                    self.pending_dialogue = Some(owner);
                }
                None
            }
            0x80fa => integer(
                self.sid(vm)
                    .and_then(|sid| self.world.scripts.get(&sid))
                    .map_or(-1, |s| s.action),
            ),
            0x80fb => {
                let object = self.world.get(call.uint(0));
                integer(
                    if let Some(object) =
                        object.filter(|o| o.category() == Category::Critter && o.alive())
                    {
                        let prone = if matches!((object.raw.art_id >> 16) & 255, 48 | 49) {
                            2
                        } else {
                            0
                        };
                        prone
                            | if let ObjectState::Critter(state) = &object.raw.state {
                                state.results.0 as i32 & 0x7c
                            } else {
                                0
                            }
                    } else {
                        1
                    },
                )
            }
            0x80fc => {
                self.advance_time(call.int(0))?;
                None
            }
            0x80fd | 0x80fe => {
                if call.uint(1) != 0 {
                    self.poison_or_radiation(
                        call.uint(1),
                        if op == 0x80fe {
                            call.int(0).checked_neg().ok_or(VmError::Arithmetic)?
                        } else {
                            call.int(0)
                        },
                        true,
                    )?;
                }
                None
            }
            // SPEC-005 §2.8 safe rules: null operands push the row's null
            // result instead of leaving the caller's stack short.
            0x80ff => {
                if call.uint(2) == 0 {
                    self.world
                        .diagnostic("critter_attempt_placement: null object");
                    integer(-1)
                } else {
                    self.object(call.uint(2))?;
                    if !(0..3).contains(&call.int(0)) {
                        integer(-1)
                    } else {
                        // An elevation change removes the critter from combat
                        // (`move_object`); the object first goes to hex 0.
                        self.world
                            .move_object(call.uint(2), 0, call.int(0) as u8, false)
                            .map_err(host_failure)?;
                        let mut candidates = vec![call.int(1)];
                        for direction in 0..6 {
                            if let Ok(Some(hex)) = HexGrid::neighbor(call.int(1), direction) {
                                candidates.push(hex);
                            }
                        }
                        let chosen = candidates.into_iter().find(|hex| {
                            (0..40000).contains(hex)
                                && self
                                    .world
                                    .movement_blocker(*hex, call.int(0) as u8, call.uint(2))
                                    .is_none()
                        });
                        let success = chosen.is_some_and(|hex| {
                            self.world
                                .move_object(call.uint(2), hex, call.int(0) as u8, false)
                                .is_ok()
                        });
                        integer(if success { 0 } else { -1 })
                    }
                }
            }
            0x8100 => integer(if call.uint(0) == 0 {
                -1
            } else {
                self.object(call.uint(0))?.raw.pid as i32
            }),
            0x8101 => integer(self.world.map.header.map_number),
            0x8102 => {
                if call.uint(3) != 0 && self.object(call.uint(3))?.category() == Category::Critter {
                    match (call.int(2), call.int(1)) {
                        // SPEC-011 §3.21: the chooser's eligibility test; a
                        // failing perk is silently not added.
                        (0, selector) => {
                            let context = self.world.context(self.world.player);
                            if let Some(perk) = Perk::from_index(selector)
                                && let Some(sheet) = self.world.player_sheet_mut()
                            {
                                sheet.add_perk_rank(perk, context);
                            }
                        }
                        (1, 5) => {
                            if let Some(object) = self.world.get_mut(call.uint(3))
                                && let ObjectState::Critter(state) = &mut object.raw.state
                            {
                                state.ai = call.int(0);
                            }
                        }
                        // SPEC-009 §3.15 team switching, skipped when unchanged.
                        (1, 6) if !self.world.restoring => {
                            let critter = call.uint(3);
                            if self
                                .world
                                .critter_state(critter)
                                .is_some_and(|state| state.team != call.int(0))
                            {
                                self.set_team(critter, call.int(0));
                            }
                        }
                        _ => {}
                    }
                }
                integer(-1)
            }
            0x8103 => {
                if call.uint(3) == 0 {
                    integer(-1)
                } else {
                    self.object(call.uint(3))?;
                    integer(-1)
                }
            }
            0x8104 => return self.proto_data(call.uint(1), call.int(0)),
            // SPEC-005 §3.3 `message_str`: a positive message number goes
            // through the speech-enabled lookup, so during a talking-head
            // dialogue its speech and lip-sync start at once, as a reply's
            // do (SPEC-006 §3.9 "Speech requests").
            0x8105 => {
                let message = if call.int(0) > 0 {
                    self.spoken_message(call.int(1), call.int(0))
                } else {
                    None
                };
                string(message.map_or_else(|| b"Error".to_vec(), |message| message.text))
            }
            0x8106 => {
                let owner = call.uint(1);
                if owner == 0 {
                    self.world.diagnostic("critter_inven_obj: null critter");
                    return Ok(integer(0));
                }
                let object = self.object(owner)?;
                if object.category() != Category::Critter {
                    integer(0)
                } else if call.int(0) == -2 {
                    integer(object.inventory.len() as i32)
                } else {
                    let selector = call.int(0);
                    let flag = match selector {
                        0 => 0x04000000,
                        1 => 0x02000000,
                        2 => 0x01000000,
                        _ => 0,
                    };
                    let allowed = owner != self.world.player
                        || selector == 0
                        || selector == if self.world.active_hand == 0 { 2 } else { 1 };
                    integer(if allowed && flag != 0 {
                        object
                            .inventory
                            .iter()
                            .find(|(item, _)| {
                                self.world
                                    .get(*item)
                                    .is_some_and(|o| o.raw.flags.0 & flag != 0)
                            })
                            .map_or(0, |(item, _)| *item as i32)
                    } else {
                        0
                    })
                }
            }
            0x8107 => {
                if call.uint(2) != 0
                    && let Err(error) =
                        self.world
                            .object_light(call.uint(2), call.int(1), call.int(0))
                {
                    self.world.diagnostic(error.to_string());
                }
                None
            }
            // SPEC-005 queued world-map and town-map requests (SPEC-014
            // §3.39): `world_map` opens the current town's town map first,
            // like an exit grid holding map -1; `town_map` opens the world
            // map directly, like map -2 (SPEC-013 §3.22).
            0x8108 | 0x8109 => {
                self.request_transition(crate::world::Transition {
                    map: if op == 0x8108 { -1 } else { -2 },
                    elevation: -1,
                    hex: -1,
                    facing: -1,
                });
                None
            }
            0x810a => {
                if call.uint(2) != 0
                    && self.object(call.uint(2))?.raw.elevation == self.world.elevation
                    && base_tag(call.args[1].value) == Value::STATIC_STRING
                {
                    let text = vm.host_string(call.args[1].value)?.unwrap_or(b"").to_vec();
                    if !text.is_empty() {
                        let mut style = call.int(0);
                        if style == -1 {
                            self.floating_style = self.floating_style % 12 + 1;
                            style = self.floating_style;
                        }
                        let color = match style {
                            -2 | 2 => 31744,
                            1 | 5 | 10 => 10570,
                            3 => 992,
                            4 => 31,
                            6 => 21140,
                            7 => 32074,
                            9 => 32767,
                            11 => 8456,
                            12 => 15855,
                            _ => 32747,
                        };
                        if style == -2
                            && let Some(hex) =
                                self.world.get(self.world.player).and_then(|p| p.raw.hex)
                        {
                            self.world.set_view_center(hex);
                        }
                        let font = if style == -2 { 103 } else { 101 };
                        // A live floating text suppresses SPEC-009 taunts.
                        self.register_floating(call.uint(2), font, &text);
                        self.world.emit(WorldEffect::Floating {
                            object: call.uint(2),
                            text,
                            font,
                            color,
                        });
                    }
                }
                None
            }
            0x810b => integer(match call.int(1) {
                13 => {
                    self.world.emit(WorldEffect::EndGame);
                    0
                }
                14 => i32::from(self.world.map.header.flags.0 & 1 == 0),
                // SPEC-013 §3.21: the deferred elevator request; a type
                // outside 0-11 fails and nothing happens.
                15 => {
                    if crate::skills::elevator(call.int(0)).is_some() {
                        self.world.emit(WorldEffect::Elevator(call.int(0)));
                    }
                    0
                }
                16 => self.world.party.len() as i32,
                22 => i32::from(self.world.restoring),
                _ => 0,
            }),
            0x810c => {
                if call.uint(2) != 0 {
                    let mut negative_frame = false;
                    let object = self
                        .world
                        .get_mut(call.uint(2))
                        .ok_or_else(|| stale_reference(call.uint(2)))?;
                    match call.int(1) {
                        // Selector 1000 sets orientation only for control below 6;
                        // larger controls are a no-op. Negative controls are not
                        // safely bounded by the reference, so they are ignored.
                        1000 => {
                            if (0..6).contains(&call.int(0)) {
                                object.raw.orientation = call.int(0) as u8;
                            }
                        }
                        // SPEC-005: 1010's lower bound is unchecked in the
                        // reference; a negative frame is diagnosed and
                        // ignored (safe bound), not a VM error.
                        1010 => match usize::try_from(call.int(0)) {
                            Ok(frame) => object.raw.frame = frame,
                            Err(_) => negative_frame = true,
                        },
                        // A registered forward (control 0) or reverse animation;
                        // a forward death animation ends on its static art.
                        animation if (0..65).contains(&animation) => {
                            if let ObjectState::Critter(state) = &mut object.raw.state {
                                if call.int(0) == 0 {
                                    state.results.0 &= 2;
                                } else {
                                    state.results.0 |= 2;
                                }
                            }
                            self.queue_animation(op, values.clone(), None);
                        }
                        _ => {}
                    }
                    if negative_frame {
                        self.world.diagnostic("anim: negative frame ignored");
                    }
                }
                None
            }
            0x810d => integer(self.world.carried(call.uint(1), call.uint(0)).unwrap_or(0) as i32),
            0x810e => {
                if !self.world.combat {
                    match call.int(1) {
                        1 => {
                            // A registration left open (its builder faulted,
                            // or never sent command 3) must not absorb this
                            // program's entries.
                            if self.building_animation.is_some() {
                                self.world
                                    .diagnostic("reg_anim_func 1 discards an open registration");
                            }
                            self.building_animation = Some(AnimationSequence {
                                flags: call.int(0),
                                ..Default::default()
                            });
                            self.building_program = Some(vm.id);
                        }
                        2 => self.clear_animation(call.uint(0)),
                        3 => {
                            self.building_program = None;
                            if let Some(sequence) = self.building_animation.take() {
                                self.register_script_sequence(&sequence);
                            }
                        }
                        _ => {}
                    }
                }
                if !self.world.combat && (1..=3).contains(&call.int(1)) {
                    self.world.emit(WorldEffect::Animation {
                        opcode: op,
                        values: values.clone(),
                        sound: None,
                    });
                }
                None
            }
            0x810f..=0x8114 | 0x8126 | 0x813b => {
                let object = call.uint(call.args.len() - 1);
                if object != 0 && (!self.world.combat || op == 0x813b) {
                    let object_data = self.object(object)?;
                    let suppressed = op == 0x810f
                        && object_data.raw.pid == 0x0100002f
                        && call.int(1) == 20
                        && self.world.violence.is_none_or(|level| level < 2);
                    if !suppressed {
                        self.queue_animation(
                            op,
                            values.clone(),
                            if op == 0x813b {
                                call.args[1].text.clone()
                            } else {
                                None
                            },
                        );
                    }
                }
                None
            }
            // SPEC-012 §3.13: the movie plays modally in the presentation
            // layer, so dialogue background processing waits for it.
            0x8115 => {
                self.play_gmovie(call.int(0));
                None
            }
            0x8117 => {
                let (item, owner) = (call.uint(1), call.uint(2));
                if item == 0 || owner == 0 {
                    integer(0)
                } else {
                    let amount = self
                        .object(owner)?
                        .inventory
                        .iter()
                        .find(|(r, _)| *r == item)
                        .map_or(0, |(_, n)| *n)
                        .min(call.int(0));
                    if let Err(error) = self.world.remove_item(owner, item, amount) {
                        self.world.diagnostic(error.to_string());
                    }
                    integer(amount)
                }
            }
            0x8118 | 0x8119 => {
                let [month, day, _] = crate::host_save::game_date(self.world.clock.ticks);
                integer(i32::from(if op == 0x8118 { month } else { day }))
            }
            // SPEC-013 §3.10: one pending request, serviced by the request
            // pass; a second request before that pass overwrites the first.
            0x811a => {
                if call.int(2) == -1 {
                    self.world.diagnostic("explosion: hex -1");
                } else {
                    self.world.pending_explosion = Some(crate::world::ExplosionRequest {
                        hex: call.int(2),
                        elevation: call.int(1),
                        min: if call.int(0) == 0 { 0 } else { 1 },
                        max: call.int(0),
                    });
                }
                None
            }
            0x811b => integer(if self.world.map.header.last_visit == 0 {
                -1
            } else {
                ((self.world.clock.ticks as i64 - i64::from(self.world.map.header.last_visit))
                    / 864000) as i32
            }),
            0x811c => {
                vm.status.0 |= 0x20;
                self.world.dialogue.reset_options();
                vm.status.0 &= !0x20;
                None
            }
            0x811d => {
                self.begin_dialogue_loop(vm)?;
                None
            }
            0x811e => {
                self.reply(vm.id, text_argument(call, 0, 1))?;
                None
            }
            0x811f | 0x8121 => {
                let option = DialogueOption {
                    text: text_argument(call, 2, 3),
                    procedure: call.uint(1),
                    reaction: call.int(0),
                };
                let added = if op == 0x8121 {
                    let iq = self.world.player_stat(4);
                    let smooth = self.world.perk(Perk::SmoothTalker);
                    self.world
                        .dialogue
                        .iq_option(option, call.int(4), iq, smooth)
                        .map(|_| ())
                } else {
                    self.world.dialogue.add_option(option)
                };
                // A rejected thirty-first option is a diagnostic, so the
                // script still reaches its `gsay_end`.
                if let Err(error) = added {
                    self.world.diagnostic(error.to_string());
                }
                None
            }
            0x8120 => {
                self.reply(vm.id, text_argument(call, 1, 2))?;
                self.world.dialogue.add_option(DialogueOption {
                    text: DialogueText::Message {
                        list: -2,
                        message: -2,
                    },
                    procedure: 0,
                    reaction: 50,
                })?;
                self.begin_dialogue_loop(vm)?;
                None
            }
            0x8122 => {
                if call.uint(1) != 0 {
                    self.poison_or_radiation(call.uint(1), call.int(0), false)?;
                }
                None
            }
            0x8123 => integer(
                self.world
                    .get(call.uint(0))
                    .and_then(|o| o.character.as_ref())
                    .map_or(0, |c| c.poison),
            ),
            0x8124 => {
                if call.uint(0) != 0 {
                    self.object(call.uint(0))?;
                    self.world.party.insert(call.uint(0));
                    // SPEC-009 §3.15: the new member switches to team 0.
                    self.set_team(call.uint(0), 0);
                }
                None
            }
            0x8125 => {
                if call.uint(0) != 0 {
                    self.world.party.remove(&call.uint(0));
                }
                None
            }
            0x8127 => {
                if call.uint(1) != 0
                    && let Some(object) = self.world.get_mut(call.uint(1))
                    && let ObjectState::Critter(state) = &mut object.raw.state
                {
                    state.results.0 |= call.uint(0) & 0x7c;
                }
                None
            }
            0x8128 => integer(i32::from(self.world.combat)),
            // SPEC-011 §3.17: the parameter replaces the script barter
            // modifier and barter is requested.
            0x8129 => {
                if self.world.dialogue.open {
                    // SPEC-013 §3.18: only a bartering merchant switches.
                    self.request_barter(call.int(0));
                } else {
                    self.world.diagnostic("gdialog_barter: no dialogue is open");
                }
                None
            }
            0x812a => integer(self.world.difficulty),
            0x812b => integer(self.world.running_burning),
            0x812c => {
                let owner = self.self_object(vm);
                let inventory = self.object(owner)?.inventory.clone();
                let flag = if owner == self.world.player && self.world.active_hand == 0 {
                    0x01000000
                } else {
                    0x02000000
                };
                for (item, _) in inventory {
                    if let Some(object) = self.world.get_mut(item) {
                        object.raw.flags.0 &= !flag;
                    }
                }
                None
            }
            0x812d => integer(i32::from(
                self.world
                    .get(call.uint(0))
                    .and_then(WorldObject::lock)
                    .is_some_and(|lock| lock.contains(LockFlags::LOCKED)),
            )),
            0x812e | 0x812f | 0x814d => {
                if let Some(object) = self.world.get_mut(call.uint(0))
                    && let Some(mut lock) = object.lock()
                {
                    match op {
                        0x812e => lock.0 |= LockFlags::LOCKED.0,
                        0x812f => lock.0 &= !LockFlags::LOCKED.0,
                        _ => lock.0 |= LockFlags::JAMMED.0,
                    }
                    object.set_lock(lock);
                }
                None
            }
            0x8130 => integer(i32::from(
                self.world
                    .get(call.uint(0))
                    .is_some_and(|o| o.raw.frame != 0),
            )),
            // SPEC-007 §3.11.4 the script open and close sequences.
            0x8131 | 0x8132 => {
                self.script_open(call.uint(0), op == 0x8131);
                None
            }
            0x8133 => {
                self.world.ui_disabled = true;
                self.library.mouse_hidden = true;
                None
            }
            0x8134 => {
                self.world.ui_disabled = false;
                self.library.mouse_hidden = false;
                None
            }
            0x8135 => integer(i32::from(self.world.ui_disabled)),
            0x8136 | 0x8137 => {
                if call.int(0) != 0 {
                    self.library.faded_out = op == 0x8136;
                    self.world.emit(WorldEffect::Fade {
                        out: op == 0x8136,
                        duration: 0,
                        from_black: false,
                    });
                }
                None
            }
            0x8138 => integer(self.world.caps(call.uint(0))),
            0x8139 => {
                if call.uint(1) == 0 {
                    integer(-1)
                } else {
                    let owner = call.uint(1);
                    let amount = call.int(0);
                    let total = self.world.caps(owner);
                    if amount < 0 && i64::from(total) + i64::from(amount) < 0 {
                        integer(-1)
                    } else if amount == 0 {
                        integer(0)
                    } else {
                        let pid = self.world.rules.money_pid();
                        if amount > 0 {
                            // New money stacks into a carried money entry.
                            let reference = self
                                .assets
                                .prototype(pid)
                                .and_then(|prototype| self.world.instantiate(prototype))
                                .map_err(host_failure)?;
                            if let Some(item) = self.world.get_mut(reference) {
                                item.money = Some(0);
                            }
                            integer(
                                if self.world.add_item(owner, reference, amount, true).is_ok() {
                                    0
                                } else {
                                    self.world.erase(reference);
                                    -1
                                },
                            )
                        } else {
                            let mut remaining = -i64::from(amount);
                            while remaining > 0 {
                                let Some(item) = self.world.carried(owner, pid) else {
                                    break;
                                };
                                let parent =
                                    self.world.get(item).and_then(|o| o.owner).unwrap_or(owner);
                                let count = self
                                    .object(parent)?
                                    .inventory
                                    .iter()
                                    .find(|(r, _)| *r == item)
                                    .map_or(0, |(_, n)| *n);
                                let take = i64::from(count).min(remaining) as i32;
                                if take <= 0 || self.world.remove_item(parent, item, take).is_err()
                                {
                                    break;
                                }
                                if self.world.get(item).is_some_and(|o| o.owner.is_none()) {
                                    self.world.erase(item);
                                }
                                remaining -= i64::from(take);
                            }
                            integer(if remaining == 0 { 0 } else { -1 })
                        }
                    }
                }
            }
            0x813a => {
                if let Some(object) = self.world.get(call.uint(1)) {
                    // Weapon animation zero; the raw orientation (not orientation
                    // plus one) is the constructor's requested selector.
                    let art = vault_formats::Fid(object.raw.art_id);
                    let fid = self.assets.construct_fid(
                        art.category(),
                        art.base(),
                        call.uint(0) as u8,
                        0,
                        object.raw.orientation,
                    );
                    integer(
                        self.assets
                            .object_art(fid)
                            .ok()
                            .map_or(0, |art| i32::from(art.action_frame)),
                    )
                } else {
                    integer(0)
                }
            }
            0x813c => {
                if call.uint(2) == self.world.player
                    && self
                        .world
                        .get(call.uint(2))
                        .is_some_and(|o| o.category() == Category::Critter)
                    && let Some(sheet) = self.world.player_sheet_mut()
                    && let Some(pool) = sheet
                        .unspent_skills
                        .checked_add(call.int(0))
                        .filter(|pool| (0..=99).contains(pool))
                {
                    sheet.unspent_skills = pool;
                    for _ in 0..call.int(0).max(0) {
                        sheet.buy_skill(call.uint(1) as usize);
                    }
                }
                integer(0)
            }
            0x813e..=0x8140 | 0x8142 => {
                // The broken wrappers resolve their I operand as text first.
                let index = call.args.len() - 1;
                let base = crate::library::text(vm, call, index)?;
                let name = match op {
                    0x813e => Some(vault_formats::sfx::ambient_name(&base)),
                    // The item builder calls interface naming.
                    0x813f | 0x8140 => Some(vault_formats::sfx::interface_name(&base)),
                    _ => u8::try_from(call.int(1)).ok().and_then(|action| {
                        vault_formats::sfx::scenery_name(call.int(0), action, &base)
                    }),
                };
                match name {
                    Some(name) => string(name),
                    None => {
                        self.world
                            .diagnostic(format!("opcode {op:#06x}: no sound name"));
                        dynamic_null()
                    }
                }
            }
            0x8141 => {
                // Safe rule: an out-of-range effect type (0-4) or hit mode
                // (0-7) is a VM error.
                if !(0..=4).contains(&call.int(3)) || !(0..=7).contains(&call.int(1)) {
                    return Err(VmError::Host(
                        "sfx_build_weapon_name: effect or hit mode out of range".into(),
                    ));
                }
                let (target, weapon) = (call.uint(0), call.uint(2));
                for object in [target, weapon] {
                    if object != 0 {
                        self.object(object)?;
                    }
                }
                let name = u8::try_from(call.int(3))
                    .ok()
                    .zip(u8::try_from(call.int(1)).ok())
                    .and_then(|(effect, mode)| {
                        self.weapon_sound_name(effect, weapon, mode, target)
                    });
                match name {
                    Some(name) => string(name),
                    None => {
                        self.world
                            .diagnostic(format!("opcode {op:#06x}: no sound name"));
                        dynamic_null()
                    }
                }
            }
            0x8144 => {
                let object = call.uint(1);
                if object == 0 {
                    return Ok(integer(0));
                }
                let owner = self.object(object)?.owner;
                let count = if let Some(owner) = owner {
                    let count = self
                        .object(owner)?
                        .inventory
                        .iter()
                        .find(|(r, _)| *r == object)
                        .map_or(0, |(_, n)| *n)
                        .min(call.int(0));
                    if count > 0 {
                        self.world
                            .remove_item(owner, object, count)
                            .map_err(host_failure)?;
                        if self.world.get(object).is_some_and(|o| o.owner.is_none()) {
                            self.destroy(vm, object)?;
                        }
                    }
                    count
                } else {
                    self.destroy(vm, object)?;
                    0
                };
                integer(count)
            }
            0x8145 => {
                let actor = self.self_object(vm);
                if actor != 0
                    && call.uint(0) != 0
                    && call.uint(1) != 0
                    && let Err(error) =
                        self.action(actor, call.uint(0), ObjectAction::UseObject(call.uint(1)))
                {
                    self.world.diagnostic(error.to_string());
                }
                None
            }
            0x8146 | 0x8148 => {
                vm.status.0 |= 0x20;
                if op == 0x8146 {
                    self.world.emit(WorldEffect::Slideshow);
                } else {
                    // SPEC-012: walkm or walkw by the player's gender.
                    self.play_endgame_movie();
                }
                vm.status.0 &= !0x20;
                None
            }
            0x8147 => {
                let (source, destination) = (call.uint(1), call.uint(0));
                if source != 0 && destination != 0 && source != destination {
                    let inventory = self.object(source)?.inventory.clone();
                    self.object(destination)?;
                    for (item, count) in inventory {
                        self.world
                            .remove_item(source, item, count)
                            .map_err(host_failure)?;
                        if let Err(error) = self.world.add_item(destination, item, count, true) {
                            self.world.diagnostic(error.to_string());
                        }
                    }
                    // SPEC-005 §3.8: the player, now with empty hands and no
                    // armor, shows the jumpsuit with weapon code 0.
                    if source == self.world.player {
                        self.update_player_gender();
                    }
                }
                None
            }
            0x8149 => integer(if call.uint(0) == 0 {
                0
            } else {
                self.object(call.uint(0))?.raw.art_id as i32
            }),
            0x814a => integer(((call.uint(0) >> 16) & 255) as i32),
            0x814b => integer(
                self.world
                    .party
                    .iter()
                    .find(|reference| {
                        self.world
                            .get(**reference)
                            .is_some_and(|o| o.raw.pid == call.uint(0))
                    })
                    .copied()
                    .unwrap_or(0) as i32,
            ),
            // An invalid hex pushes 0 with a diagnostic (safe rule).
            0x814c => {
                if [call.int(1), call.int(0)]
                    .iter()
                    .all(|hex| (0..40000).contains(hex))
                {
                    integer(rotation(call.int(1), call.int(0))?)
                } else {
                    self.world
                        .diagnostic("rotation_to_tile: hex outside 0-39999");
                    integer(0)
                }
            }
            0x814e => {
                self.world.dialogue.barter_modifier = call.int(0);
                None
            }
            0x814f => integer(self.world.combat_difficulty.unwrap_or(0)),
            0x8150 => {
                let visible = if let Some(object) = self
                    .world
                    .get(call.uint(0))
                    .filter(|o| o.raw.elevation == self.world.elevation)
                {
                    if let Ok(art) = self.assets.object_art(object.raw.art_id)
                        && let Some(frame) =
                            art.frame(object.raw.orientation as usize, object.raw.frame)
                    {
                        let origin = if let Some(hex) = object.raw.hex {
                            let center = HexGrid::centered(self.world.camera_hex, 640, 380)
                                .and_then(|grid| grid.hex_center(hex))
                                .map_err(|e| VmError::Host(e.to_string()))?;
                            let rect = frame.world_rect(
                                center.map(|v| v as i32),
                                art.shifts[object.raw.orientation as usize],
                                object.raw.displacement,
                            );
                            [rect.left, rect.top]
                        } else {
                            object.raw.screen.map(i64::from)
                        };
                        origin[0] <= 640
                            && origin[1] <= 480
                            && origin[0] + i64::from(frame.width) > 0
                            && origin[1] + i64::from(frame.height) > 0
                    } else {
                        false
                    }
                } else {
                    false
                };
                integer(i32::from(visible))
            }
            0x8151 => integer(i32::from(self.world.get(call.uint(0)).is_some_and(
                |o| matches!(&o.raw.state,ObjectState::Critter(state) if state.maneuvers&4!=0),
            ))),
            0x8152 | 0x8155 => {
                let reference = call.uint(if op == 0x8152 { 1 } else { 0 });
                if let Some(object) = self.world.get_mut(reference)
                    && let ObjectState::Critter(state) = &mut object.raw.state
                {
                    if op == 0x8155 {
                        state.maneuvers |= 2;
                        object.opponent = 0;
                    } else if call.int(0) != 0 {
                        state.maneuvers |= 4;
                    } else {
                        state.maneuvers &= !4;
                    }
                }
                None
            }
            0x8153 => {
                if self.world.combat {
                    self.world.combat_quit = true;
                }
                None
            }
            _ => return Err(VmError::Opcode(op)),
        };
        Ok(result)
    }
}
fn rotation(source: i32, target: i32) -> Result<i32, VmError> {
    let a = HexGrid::default()
        .hex_reference(source)
        .map_err(|e| VmError::Host(e.to_string()))?;
    let b = HexGrid::default()
        .hex_reference(target)
        .map_err(|e| VmError::Host(e.to_string()))?;
    let (dx, dy) = (b[0] - a[0], b[1] - a[1]);
    if dx == 0 {
        return Ok(if dy < 0 { 0 } else { 2 });
    }
    let degrees = ((-dy as f64).atan2(dx as f64) * 180.0 * 0.318_309_886_285_112_2) as i32;
    let mut angle = 90 - degrees;
    if angle < 0 {
        angle += 360;
    }
    Ok((angle / 60).min(5))
}
