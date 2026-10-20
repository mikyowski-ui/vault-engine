//! SPEC-007 §3.9 projectiles: a thrown weapon's consumption and the flight
//! of a thrown weapon or of another ranged weapon's projectile. The attack
//! is final when this runs, so the world changes at once: the thrown unit
//! leaves the attacker, lands where the projectile ends (or, explosive, is
//! erased), and the attacker's art follows its hand. The flight itself is
//! a presentation timeline, `ProjectileFlight`.
use crate::{
    ObjectRef, WorldEffect, WorldError,
    assets::WorldAssets,
    combat::{Attack, Resolution, Subtype, attack_mode},
    critical::effect,
    host::WorldHost,
    routing::{RayOptions, cast_ray, rotation},
    world::{ExplosionArt, FlightEvent, ProjectileFlight},
};
use vault_formats::{Fid, HexGrid, pro::ItemData, sfx};

/// The light radius a projectile flies with, at its own intensity.
const PROJECTILE_LIGHT_RADIUS: i32 = 9;
/// The throw animation.
const THROW_ANIMATION: u8 = 18;

/// A projectile about to fly: the weapon it leaves and its art.
pub(crate) struct Launch {
    pub weapon: ObjectRef,
    pub fid: u32,
}
/// Where a projectile ends: whether the attack hits, the arrival hex (the
/// target's on a hit, the §3.7.1 impact hex on a miss), and the explosion
/// that replaces an explosive projectile there.
pub(crate) struct Landing {
    pub hit: bool,
    pub arrival: i32,
    pub explosion: Option<ExplosionArt>,
}

impl<A: WorldAssets> WorldHost<A> {
    /// §3.9 whether the projectile phase runs for a throw or another ranged
    /// attack: the attack hits or carries no critical bit (a critical
    /// failure with an effect keeps it), and the weapon's projectile PID
    /// names a loadable prototype whose art is not -1.
    pub(crate) fn projectile_launch(
        &self,
        attacker: ObjectRef,
        attack: &Attack,
        result: &Resolution,
    ) -> Option<Launch> {
        if !matches!(attack.subtype(), Some(Subtype::Throwing | Subtype::Ranged))
            || (result.defender_flags & effect::HIT == 0
                && result.attacker_flags & effect::CRITICAL != 0)
        {
            return None;
        }
        let weapon = self.world.wielded(attacker, attack.hit_mode)?;
        let pid = u32::try_from(self.world.weapon_data(weapon)?.projectile_pid).ok()?;
        let fid = self.assets.prototype(pid).ok()?.art_id;
        (fid != u32::MAX).then_some(Launch { weapon, fid })
    }
    /// §3.9 the projectile phase. The attacker turns toward `target`, the
    /// final defender, and its attack sound plays. A throw consumes one
    /// unit, which is the projectile: lit with radius 9, it lands at the
    /// arrival hex and stays there as the item it is, or, explosive, is
    /// erased. The flight shows once the attacker's attack art reaches its
    /// action frame, from the hex beside the attacker toward the target,
    /// one stride-32 ray sample per step; a non-explosive arrival's hit
    /// sound waits N steps (the ray's sample count less one on a hit, 0 on
    /// a miss), and the explosion replaces an explosive one on arrival.
    /// Returns, for a throw, the weapon code the attacker's hand shows.
    pub(crate) fn launch_projectile(
        &mut self,
        attacker: ObjectRef,
        target: ObjectRef,
        attack: &Attack,
        launch: Launch,
        landing: Landing,
    ) -> Result<Option<u8>, WorldError> {
        let Launch { weapon, fid } = launch;
        let Landing {
            hit,
            arrival,
            explosion,
        } = landing;
        let thrown = attack.mode() == attack_mode::THROW;
        let Some((from, elevation, facing, art)) = self.world.get(attacker).and_then(|object| {
            Some((
                object.raw.hex?,
                object.raw.elevation,
                object.raw.orientation,
                Fid(object.raw.art_id),
            ))
        }) else {
            return Ok(None);
        };
        let direction = self
            .world
            .get(target)
            .and_then(|object| object.raw.hex)
            .and_then(|to| rotation(from, to))
            .unwrap_or(facing);
        if let Some(object) = self.world.get_mut(attacker) {
            object.raw.orientation = direction;
        }
        let hit_mode = attack.hit_mode;
        let attack_sound = if thrown && art.weapon() == 0 {
            self.character_sound_name(attacker, THROW_ANIMATION, 0)
        } else {
            self.weapon_sound_name(sfx::effect::ATTACK, weapon, hit_mode, 0)
        };
        self.emit_effect(attack_sound, attacker);
        // The wait is the action frame of the attack art for the base,
        // weapon code and facing before the turn (0 without that art).
        let swing = self
            .assets
            .object_art(self.assets.construct_fid(
                1,
                art.base(),
                crate::ai::attack_animation(attack.mode()),
                art.weapon(),
                Fid::selector_for_orientation(facing),
            ))
            .ok();
        let wait = swing
            .as_ref()
            .map_or(0, |swing| u32::try_from(swing.action_frame).unwrap_or(0));
        let wait_ms = swing
            .as_ref()
            .and_then(|swing| swing.frame_interval_ms())
            .unwrap_or(100);
        let interval_ms = self
            .assets
            .object_art(fid)
            .ok()
            .and_then(|art| art.frame_interval_ms())
            .unwrap_or(100);
        let origin = HexGrid::neighbor(from, direction)
            .ok()
            .flatten()
            .unwrap_or(from);
        let options = RayOptions {
            stride: 32,
            check_blockers: false,
            ignore: None,
        };
        let samples = cast_ray(origin, arrival, elevation, options, |_| None, |_| false).samples;
        let mut path = vec![(origin, [0, 0])];
        path.extend(samples.iter().map(|sample| {
            (
                sample.hex,
                sample.offset.map(|offset| offset.clamp(-1000, 1000) as i32),
            )
        }));
        let flight_sound = self.weapon_sound_name(sfx::effect::FLIGHT, weapon, hit_mode, 0);
        let explosive = explosion.is_some();
        let hit_sound = if explosive {
            None
        } else {
            let steps = if hit {
                samples.len().saturating_sub(1)
            } else {
                0
            };
            let owner = if hit { target } else { 0 };
            self.weapon_sound_name(sfx::effect::HIT, weapon, hit_mode, owner)
                .map(|name| (steps, name))
        };
        let explosion = explosion.map(|art| {
            let sound = self.weapon_sound_name(sfx::effect::HIT, weapon, hit_mode, 0);
            (art, sound)
        });
        let mut item = None;
        let mut hand = None;
        if thrown {
            let (projectile, held) = self.world.consume_thrown(attacker, weapon)?;
            hand = Some(held.map_or(0, |held| self.weapon_code(held)));
            if explosive {
                self.world.erase(projectile);
            } else {
                if let Some(object) = self.world.get_mut(projectile) {
                    object.raw.light.radius = PROJECTILE_LIGHT_RADIUS;
                }
                let landing = self
                    .world
                    .get(target)
                    .map_or(elevation, |object| object.raw.elevation);
                self.world
                    .move_object(projectile, arrival, landing, false)?;
                item = Some(projectile);
            }
        }
        self.world.projectiles.push(ProjectileFlight {
            item,
            fid,
            orientation: direction,
            elevation,
            path,
            wait,
            wait_ms,
            interval_ms,
            position: None,
            flight_sound: flight_sound.filter(|name| !name.is_empty()),
            hit_sound: hit_sound.filter(|(_, name)| !name.is_empty()),
            explosion,
            next_ms: None,
            done: false,
        });
        Ok(hand)
    }
    /// An item's weapon code (0 for a non-weapon).
    fn weapon_code(&self, item: ObjectRef) -> u8 {
        match self.world.item_data(item).map(|data| &data.data) {
            Some(ItemData::Weapon(weapon)) => u8::try_from(weapon.animation).unwrap_or(0),
            _ => 0,
        }
    }
    /// §3.9 step 11: unless the attack left the attacker knocked out,
    /// knocked down or dead, it stands with the weapon code its hand shows:
    /// the refill's, or under the §3.9 safe rule that of the units left in
    /// the hand (the reference shows 0 for those).
    pub(crate) fn show_hand(&mut self, attacker: ObjectRef, weapon: u8) {
        let status = self
            .world
            .critter_state(attacker)
            .map_or(0, |state| state.results.0);
        if status & (effect::KNOCKED_OUT | effect::KNOCKED_DOWN) != 0
            || self.world.is_dead(attacker)
        {
            return;
        }
        if let Some(object) = self.world.get_mut(attacker) {
            let art = Fid(object.raw.art_id);
            object.raw.art_id = Fid::new(1, art.base(), 0, weapon, 0).0;
            object.raw.frame = 0;
        }
    }
    /// One poll of the projectiles in flight: their sounds as they show and
    /// arrive, an explosive one's explosion art, and the removal of
    /// finished flights. Returns whether anything changed.
    pub(crate) fn step_projectiles(&mut self, now_ms: u64) -> bool {
        let mut changed = false;
        let mut flights = std::mem::take(&mut self.world.projectiles);
        for flight in &mut flights {
            let before = (flight.position, flight.done);
            for event in flight.poll(now_ms) {
                match event {
                    FlightEvent::Sound(name) => self.world.emit(WorldEffect::SoundEffect {
                        name,
                        owner: flight.item,
                    }),
                    FlightEvent::Explosion(art, sound) => {
                        if let Some(name) = sound.filter(|name| !name.is_empty()) {
                            self.world
                                .emit(WorldEffect::SoundEffect { name, owner: None });
                        }
                        self.world.explosion_effects.push(art);
                    }
                }
            }
            changed |= before != (flight.position, flight.done);
        }
        flights.retain(|flight| !flight.done);
        // Flights launched while polling (none today) are kept after these.
        flights.append(&mut self.world.projectiles);
        self.world.projectiles = flights;
        changed
    }
}
