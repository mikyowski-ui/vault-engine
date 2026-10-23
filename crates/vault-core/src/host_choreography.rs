//! SPEC-007 §3.9-3.10 attack choreography in the world: the sequence an
//! attack registers (the attacker's animations and sounds, the projectile,
//! and the reactions of the defender, the extra victims and the attacker),
//! the thrown weapon's consumption, and the shows' playback.
//!
//! The attack's effects apply at once, in the order the sequence would
//! commit them: the forced callbacks and moves, then the §3.6 cleanup.
//! The show then plays them, after the shows before it; in real time,
//! combat waits until every show has played.
use crate::{
    ObjectRef, RandomSource, WorldEffect, WorldError,
    ai::{self, Taunt},
    assets::WorldAssets,
    choreography::{Action, Display, Frames, Owner, Refused, Sequence, Show, Started},
    combat::{Attack, DeathChoice, Resolution, attack_mode, corpse_flattens, damage_type},
    critical::{effect, hit_mode},
    host::WorldHost,
    host_combat::Spray,
    routing::{RayOptions, cast_ray, rotation, step},
    world::ExplosionArt,
};
use std::collections::BTreeMap;
use vault_formats::{
    Fid, HexGrid,
    pro::{BasicFlags, ItemData},
    sfx,
};

/// How attack shows and the world's own sequences (SPEC-007 §3.11) play.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Playback {
    /// Each plays to its end as soon as it is registered (headless runs).
    #[default]
    Instant,
    /// They play over time, shows one after another, and combat waits for
    /// them.
    RealTime,
}

/// The requested light radius of a projectile (§3.9), at its own
/// intensity; setting it gives 8, or 0 without intensity.
const PROJECTILE_LIGHT_RADIUS: i32 = 9;
/// The lit flare, whose throw is the only corpus throw that carries light.
const LIT_FLARE: u32 = 205;
/// The player's ordinary light (§3.9 safe rule): radius 4 at full intensity.
const PLAYER_LIGHT: (i32, i32) = (4, 65536);
/// The weapon explosion art of the explode effect (`roktxpd.frm`).
const EXPLODED_WEAPON_ART: u32 = 0x0500_000A;
/// §3.11.5 script damage's temporary attacker art, its sound, and its
/// knockback direction (the direction from a hex to itself).
const SCRIPT_ATTACKER_ART: u32 = 0x0200_01F5;
const SCRIPT_DAMAGE_SOUND: &[u8] = b"whc1xxx1";
const SOUTHEAST: u8 = 2;

/// Animation codes (SPEC-007 §2.2).
mod animation {
    pub const STAND: u8 = 0;
    pub const WALK: u8 = 1;
    pub const DODGE: u8 = 13;
    pub const HIT_FRONT: u8 = 14;
    pub const HIT_BACK: u8 = 15;
    pub const THROW: u8 = 18;
    pub const FALL_BACK: u8 = 20;
    pub const FALL_FRONT: u8 = 21;
    pub const BURNED: u8 = 29;
    pub const FIRE_DANCE: u8 = 33;
    pub const BLOODY_BACK: u8 = 34;
    pub const BLOODY_FRONT: u8 = 35;
    pub const PRONE_UP: u8 = 36;
    pub const BACK_UP: u8 = 37;
    pub const TAKE_OUT: u8 = 38;
    pub const POINT: u8 = 43;
    pub const UNPOINT: u8 = 44;
    pub const CONTINUOUS: u8 = 47;
}
/// SPEC-010 character sound extras.
mod extra {
    pub const NONE: u8 = 0;
    pub const KNOCKDOWN: u8 = 1;
    pub const DIE: u8 = 3;
    pub const CONTACT: u8 = 4;
}

/// A resolved attack, as its sequence shows it.
pub(crate) struct Scene<'a> {
    pub attacker: ObjectRef,
    /// The final recorded defender (§3.6).
    pub defender: ObjectRef,
    pub attack: &'a Attack,
    /// The recorded defender's attack profile.
    pub victim: &'a Attack,
    pub spray: &'a Spray,
    /// The attack hit the recorded defender.
    pub hit: bool,
    /// The §3.7.1 impact hex of a miss.
    pub impact: Option<i32>,
    /// The weapon explosion an explosive projectile ends in.
    pub explosion: Option<ExplosionArt>,
    /// The attack-animation selector a death takes from attacker art
    /// `0x020001F5` (§3.8), for script damage (§3.11.5).
    pub selector: Option<u8>,
}

/// An effect the sequence commits: applied to the world at once, before
/// the §3.6 cleanup.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Effect {
    /// A knockback slide or a fire-dance run ends on this hex.
    Move {
        object: ObjectRef,
        hex: i32,
        elevation: u8,
    },
    /// The corpse presentation for a final death animation (§3.8).
    Corpse {
        victim: ObjectRef,
        animation: u8,
    },
    /// The attack's weapon drops at a hex, or is destroyed.
    Drop {
        weapon: ObjectRef,
        hex: i32,
        elevation: u8,
    },
    Destroy {
        weapon: ObjectRef,
    },
}

/// A built show, its effects, and a throw's consumption.
pub(crate) struct Built {
    pub show: Show,
    pub effects: Vec<Effect>,
    pub thrown: Option<Thrown>,
}

/// A throw's consumption (§3.9): the thrown unit, its landing hex and
/// elevation (none when explosive), and the weapon code the attacker's
/// hand shows afterwards.
pub(crate) struct Thrown {
    pub item: ObjectRef,
    pub landing: Option<(i32, u8)>,
    pub hand: u8,
}

/// The builder's working state: the show, effects, presentation-only
/// owners, and each critter's facing as the sequence will have it.
#[derive(Default)]
struct Draft {
    show: Show,
    effects: Vec<Effect>,
    next_shown: u32,
    facing: BTreeMap<ObjectRef, u8>,
}
impl Draft {
    fn shown(&mut self) -> Owner {
        self.next_shown += 1;
        Owner::Shown(self.next_shown)
    }
}

/// Which sequence of the show an entry joins.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Into {
    Main,
    Extra(usize),
}

impl<A: WorldAssets> WorldHost<A> {
    /// Attack shows are still playing, or in combat the world's own
    /// sequences (a walk, a door): combat waits for them.
    pub fn presenting(&self) -> bool {
        !self.world.shows.is_empty() || (self.world.combat && !self.motions.is_empty())
    }
    /// One playback pass at `now_ms`: the first show registers or takes a
    /// pass, starting its sounds and taunts; a finished show leaves and the
    /// next one starts. Returns whether anything changed.
    pub(crate) fn step_shows(&mut self, now_ms: u64) -> bool {
        let mut changed = false;
        for _ in 0..64 {
            let Some(show) = self.world.shows.front_mut() else {
                break;
            };
            let started = show.pass(now_ms);
            let taunts = show.taunts.clone();
            changed |= !started.is_empty() || show.due_ms().is_some();
            self.start_all(&started, &taunts);
            if !self.world.shows.front().is_some_and(Show::finished) {
                break;
            }
            self.world.shows.pop_front();
            changed = true;
        }
        self.sync_combat_lock();
        changed
    }
    /// Sounds and taunts a show started.
    fn start_all(&mut self, started: &[Started], taunts: &[ai::TauntCallback]) {
        for item in started {
            match item {
                Started::Sound { name, object } if !name.is_empty() => {
                    self.world.emit(WorldEffect::SoundEffect {
                        name: name.clone(),
                        owner: *object,
                    })
                }
                Started::Taunt(index) => {
                    if let Some(callback) = taunts.get(*index) {
                        self.show_taunt(callback);
                    }
                }
                _ => {}
            }
        }
    }
    /// A built show plays: at once, or queued behind the others. The world
    /// objects it shows first take the state it leaves them in: the last
    /// art, frame, facing and displacement, with critters that are neither
    /// prone nor dead back in their standing pose.
    pub(crate) fn present(&mut self, mut show: Show) {
        let now = self.world.elapsed_ms;
        let mut finished = show.clone();
        let started = finished.run_to_end(now);
        for (owner, display) in finished.displays() {
            let Owner::World(object) = owner else {
                continue;
            };
            self.settle(*object, display);
        }
        match self.playback {
            Playback::Instant => {
                let taunts = std::mem::take(&mut show.taunts);
                self.start_all(&started, &taunts);
            }
            Playback::RealTime => {
                show.combat = self.world.combat;
                self.world.shows.push_back(show);
                self.sync_combat_lock();
            }
        }
    }
    /// A world object takes a show's final display.
    fn settle(&mut self, object: ObjectRef, display: &Display) {
        if self.world.get(object).is_none() {
            return;
        }
        let art = Fid(display.art);
        let resting = art.category() == 1 && !matches!(art.animation(), 20..=35 | 48..=63);
        let fid = if resting {
            self.assets.construct_fid(
                1,
                art.base(),
                animation::STAND,
                art.weapon(),
                Fid::selector_for_orientation(display.orientation),
            )
        } else {
            display.art
        };
        if let Some(found) = self.world.get_mut(object) {
            found.raw.art_id = fid;
            found.raw.frame = if resting { 0 } else { display.frame };
            found.raw.orientation = display.orientation;
            found.raw.displacement = if resting {
                [0; 2]
            } else {
                display.displacement
            };
        }
    }

    /// A walk the world has already made, shown (SPEC-007 §3.11.1): the
    /// mover leaves the display `from` and steps along `route` frame by
    /// frame, with the walk art (or the run art, under the run rules) at
    /// its rate plus `combat_speed` in combat (§3.10.1). It plays after the
    /// shows before it, and combat waits for it.
    pub(crate) fn present_walk(
        &mut self,
        mover: ObjectRef,
        from: Display,
        route: Vec<u8>,
        run: bool,
    ) {
        if route.is_empty() {
            return;
        }
        let animation = self.walk_animation(mover, run);
        let speed = if animation == animation::WALK {
            self.walk_speed(mover)
        } else {
            0
        };
        let arts: [u32; 6] =
            std::array::from_fn(|facing| self.critter_fid(mover, animation, None, facing as u8));
        let frames = Box::new(std::array::from_fn(|facing| {
            self.frames(arts[facing], facing as u8, speed)
        }));
        let mut show = Show::default();
        let owner = Owner::World(mover);
        show.main.show(owner, from);
        let walk = Action::Walk {
            arts,
            frames,
            route,
            cursor: 0,
            frame: 0,
        };
        if show.main.push(owner, 0, walk).is_ok() {
            self.present(show);
        }
    }
    /// §3.11.5 script damage shown: a hidden temporary attacker at the
    /// victim's hex, art `0x020001F5` facing 0 (NE), and one sequence:
    /// `whc1xxx1`, then the victim's §3.10.6 reaction with delay 0. The
    /// victim is hit from the front when it faces SE, SW or W; knockback is
    /// `trunc(damage / 10)` hexes SE, or none for a multihex victim,
    /// electrical damage or bypassed armor; a death takes the
    /// maximum-violence column for the damage type. The reaction's effects
    /// (a slide, the corpse) apply at once, as an attack's do.
    pub(crate) fn present_script_damage(
        &mut self,
        victim: ObjectRef,
        damage: i32,
        kind: i32,
        bypassed: bool,
        died: bool,
    ) -> Result<(), WorldError> {
        let Some((hex, elevation, orientation, multihex)) =
            self.world.get(victim).and_then(|object| {
                Some((
                    object.raw.hex?,
                    object.raw.elevation,
                    object.raw.orientation,
                    object.raw.flags.contains(BasicFlags::MULTIHEX),
                ))
            })
        else {
            return Ok(());
        };
        let mut raw = crate::world::object_record(0x0500_0001, 0);
        raw.art_id = SCRIPT_ATTACKER_ART;
        raw.hex = Some(hex);
        raw.elevation = elevation;
        raw.flags = BasicFlags(BasicFlags::HIDDEN.0 | BasicFlags::TEMPORARY.0);
        let attacker = self.world.insert(crate::world::WorldObject {
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
        let knockback = if bypassed || multihex || kind == damage_type::ELECTRICAL {
            0
        } else {
            damage / 10
        };
        let flags = effect::HIT | if died { effect::DEAD } else { 0 };
        let attack = Attack::default();
        let spray = Spray {
            main: Resolution {
                code: 2,
                defender_flags: flags,
                damage,
                knockback,
                ..Resolution::default()
            },
            extras: Vec::new(),
        };
        let selector = usize::try_from(kind)
            .ok()
            .and_then(|kind| crate::combat::SEVERE_DEATH.get(kind))
            .map_or(animation::FALL_BACK, |(_, maximum)| *maximum);
        let scene = Scene {
            attacker,
            defender: victim,
            attack: &attack,
            victim: &attack,
            spray: &spray,
            hit: true,
            impact: None,
            explosion: None,
            selector: Some(selector),
        };
        let mut draft = Draft::default();
        draft.facing.insert(attacker, 0);
        draft.facing.insert(victim, orientation);
        let built = self
            .sound(
                &mut draft,
                Into::Main,
                victim,
                0,
                Some(SCRIPT_DAMAGE_SOUND.to_vec()),
            )
            .and_then(|()| {
                let react = React {
                    critter: victim,
                    flags,
                    knockback,
                    direction: Some(SOUTHEAST),
                    delay: 0,
                    from_front: matches!(orientation, 2..=4),
                    profile: &attack,
                    damage,
                };
                self.react(&mut draft, Into::Main, &scene, react)
            });
        self.world.erase(attacker);
        match built {
            Ok(()) => {
                self.apply_effects(&draft.effects)?;
                self.present(draft.show);
            }
            Err(refused) => self
                .world
                .diagnostic(format!("script damage sequence refused: {refused:?}")),
        }
        Ok(())
    }
    /// §3.10 the attack's show: the melee sequence (§3.10.3) or the ranged
    /// and thrown one (§3.10.4), with the reactions (§3.10.6) and the extra
    /// victims' sequences (§3.10.7). Building draws, in order, the attack
    /// taunt, the main sequence's fire-dance draws, the hit or miss taunt,
    /// and the extras' fire-dance draws. An entry that cannot register (a
    /// full sequence, or animation art that does not exist) refuses the
    /// whole show; draws already made stay consumed.
    pub(crate) fn build_show(&mut self, scene: &Scene<'_>) -> Result<Built, Refused> {
        let mut draft = Draft::default();
        let attacker = scene.attacker;
        for critter in [attacker, scene.defender] {
            if let Some(object) = self.world.get(critter) {
                draft.facing.insert(critter, object.raw.orientation);
            }
        }
        let mut thrown = None;
        let anim = ai::attack_animation(scene.attack.mode());
        if ai::melee_form(anim) {
            self.melee(&mut draft, scene, anim)?;
        } else {
            thrown = self.ranged(&mut draft, scene, anim)?;
        }
        for index in 0..draft.show.extras.len() {
            let (object, profile, outcome) = &scene.spray.extras[index];
            let direction = self
                .hex(attacker)
                .zip(self.hex(*object))
                .and_then(|(from, to)| rotation(from, to));
            let attacker_facing = draft.facing.get(&attacker).copied().unwrap_or(0);
            let from_front = self.world.get(*object).is_some_and(|extra| {
                matches!(attacker_facing.abs_diff(extra.raw.orientation), 2..=4)
            });
            self.react(
                &mut draft,
                Into::Extra(index),
                scene,
                React {
                    critter: *object,
                    flags: outcome.defender_flags,
                    knockback: outcome.knockback,
                    direction,
                    delay: 0,
                    from_front,
                    profile,
                    damage: outcome.damage,
                },
            )?;
        }
        Ok(Built {
            show: draft.show,
            effects: draft.effects,
            thrown,
        })
    }
    fn hex(&self, object: ObjectRef) -> Option<i32> {
        self.world.get(object).and_then(|object| object.raw.hex)
    }
    /// A world object's display as it is now (off the map while carried).
    pub(crate) fn display_now(&self, object: ObjectRef) -> Option<Display> {
        let found = self.world.get(object)?;
        Some(Display {
            pid: found.raw.pid,
            art: found.raw.art_id,
            frame: found.raw.frame,
            hex: found.owner.is_none().then_some(found.raw.hex).flatten(),
            elevation: found.raw.elevation,
            orientation: found.raw.orientation,
            displacement: found.raw.displacement,
            hidden: found.raw.flags.contains(BasicFlags::HIDDEN),
            flat: found.raw.flags.contains(BasicFlags::FLAT),
        })
    }
    /// A critter art identifier: `critter`'s base, `animation`, its art's
    /// weapon code unless `weapon` is given, facing `orientation`.
    pub(crate) fn critter_fid(
        &self,
        critter: ObjectRef,
        animation: u8,
        weapon: Option<u8>,
        orientation: u8,
    ) -> u32 {
        let art = self
            .world
            .get(critter)
            .map_or(Fid(0x0100_0000), |object| Fid(object.raw.art_id));
        self.assets.construct_fid(
            1,
            art.base(),
            animation,
            weapon.unwrap_or(art.weapon()),
            Fid::selector_for_orientation(orientation),
        )
    }
    /// The frames of `fid` for `orientation`, at `trunc(1000 / fps)` ms a
    /// step (stored 0 is 10 fps), with `speed` added to the fps.
    pub(crate) fn frames(&self, fid: u32, orientation: u8, speed: i32) -> Frames {
        let Ok(art) = self.assets.object_art(fid) else {
            return Frames {
                shifts: Vec::new(),
                interval_ms: 100,
            };
        };
        let direction = usize::from(orientation);
        let shifts = (0..art.frame_count())
            .filter_map(|index| art.frame(direction, index).or_else(|| art.frame(0, index)))
            .map(|frame| frame.displacement.map(i32::from))
            .collect();
        let fps = i32::from(art.effective_fps()).saturating_add(speed);
        let fps = if fps > 0 { fps } else { 10 };
        Frames {
            shifts,
            interval_ms: 1000 / u64::from(fps.unsigned_abs()),
        }
    }
    /// The action frame of `fid`'s FRM header (0 without the art).
    fn action_frame(&self, fid: u32) -> i32 {
        self.assets
            .object_art(fid)
            .map_or(0, |art| i32::from(art.action_frame))
    }
    /// `combat_speed` for a walk animation in combat: every owner but the
    /// player, and the player with `player_speedup`.
    pub(crate) fn walk_speed(&self, owner: ObjectRef) -> i32 {
        if self.world.combat && (owner != self.world.player || self.world.player_speedup) {
            self.world.combat_speed
        } else {
            0
        }
    }
    fn sequence(draft: &mut Draft, into: Into) -> &mut Sequence {
        match into {
            Into::Main => &mut draft.show.main,
            Into::Extra(index) => &mut draft.show.extras[index],
        }
    }
    /// Show a world object in a sequence from its current state.
    fn own(&self, draft: &mut Draft, into: Into, object: ObjectRef) -> Owner {
        let owner = Owner::World(object);
        if let Some(display) = self.display_now(object) {
            Self::sequence(draft, into).show(owner, display);
        }
        owner
    }
    /// An entry for `object`.
    fn push(
        &self,
        draft: &mut Draft,
        into: Into,
        object: ObjectRef,
        delay: i32,
        action: Action,
    ) -> Result<(), Refused> {
        let owner = self.own(draft, into, object);
        Self::sequence(draft, into).push(owner, delay, action)
    }
    /// A sound entry; a missing effect name leaves a silent entry.
    fn sound(
        &self,
        draft: &mut Draft,
        into: Into,
        owner: ObjectRef,
        delay: i32,
        name: Option<Vec<u8>>,
    ) -> Result<(), Refused> {
        let object = (owner != 0).then_some(owner);
        self.push(
            draft,
            into,
            owner,
            delay,
            Action::Sound {
                name: name.unwrap_or_default(),
                object,
            },
        )
    }
    /// An animation of `critter` (its art must exist), facing as the
    /// sequence will have it then.
    fn animate(
        &self,
        draft: &mut Draft,
        into: Into,
        critter: ObjectRef,
        animation: u8,
        weapon: Option<u8>,
        delay: i32,
    ) -> Result<(), Refused> {
        let facing = draft.facing.get(&critter).copied().unwrap_or(0);
        let fid = self.critter_fid(critter, animation, weapon, facing);
        if !self.assets.art_exists(fid) {
            return Err(Refused::Art(fid));
        }
        let speed = if animation == animation::WALK {
            self.walk_speed(critter)
        } else {
            0
        };
        let frames = self.frames(fid, facing, speed);
        self.push(
            draft,
            into,
            critter,
            delay,
            Action::Animate {
                art: fid,
                frames,
                hide: false,
            },
        )
    }
    /// A turn toward `hex`: the facing and the standing pose at once.
    fn turn(
        &self,
        draft: &mut Draft,
        into: Into,
        critter: ObjectRef,
        hex: Option<i32>,
        delay: i32,
    ) -> Result<(), Refused> {
        let facing = draft.facing.get(&critter).copied().unwrap_or(0);
        let orientation = self
            .hex(critter)
            .zip(hex)
            .and_then(|(from, to)| rotation(from, to))
            .unwrap_or(facing);
        draft.facing.insert(critter, orientation);
        let art = self.critter_fid(critter, animation::STAND, None, orientation);
        self.push(
            draft,
            into,
            critter,
            delay,
            Action::Turn { orientation, art },
        )
    }
    /// A taunt request (SPEC-009 §3.17, which draws) and, when a line was
    /// chosen, its display entry.
    fn taunt(
        &mut self,
        draft: &mut Draft,
        speaker: ObjectRef,
        taunt: Taunt,
        delay: i32,
    ) -> Result<(), Refused> {
        let requested = self.request_taunt(speaker, taunt).unwrap_or(false);
        let Some(callback) = requested.then(|| self.world.ai.callbacks.pop()).flatten() else {
            return Ok(());
        };
        let index = draft.show.taunts.len();
        draft.show.taunts.push(callback);
        self.push(draft, Into::Main, speaker, delay, Action::Taunt(index))
    }
    /// The hit taunt (survived) or miss taunt for the attack's result
    /// (SPEC-009 §3.17), delay -1: the attacker speaks for melee forms,
    /// the defender otherwise.
    fn result_taunt(&mut self, draft: &mut Draft, scene: &Scene<'_>) -> Result<(), Refused> {
        let requested = self
            .request_result_taunt(
                scene.attacker,
                scene.defender,
                scene.attack,
                &scene.spray.main,
            )
            .unwrap_or(false);
        let Some(callback) = requested.then(|| self.world.ai.callbacks.pop()).flatten() else {
            return Ok(());
        };
        let speaker = callback.speaker;
        let index = draft.show.taunts.len();
        draft.show.taunts.push(callback);
        self.push(draft, Into::Main, speaker, -1, Action::Taunt(index))
    }
    /// The attack sound at the start (§3.10.2): the character sound for
    /// punch and kick; the weapon attack sound for swing and thrust, and for
    /// the other forms when the attacker's art carries a weapon code, else
    /// the character sound for the attack animation.
    fn attack_sound(&self, scene: &Scene<'_>, anim: u8) -> Option<Vec<u8>> {
        let attacker = scene.attacker;
        let coded = self
            .world
            .get(attacker)
            .is_some_and(|object| Fid(object.raw.art_id).weapon() != 0);
        let weapon = self.world.wielded(attacker, scene.attack.hit_mode);
        match (anim, weapon) {
            (16 | 17, _) | (_, None) => self.character_sound_name(attacker, anim, extra::NONE),
            (41 | 42, Some(weapon)) => {
                self.weapon_sound_name(sfx::effect::ATTACK, weapon, scene.attack.hit_mode, 0)
            }
            (_, Some(weapon)) if coded => {
                self.weapon_sound_name(sfx::effect::ATTACK, weapon, scene.attack.hit_mode, 0)
            }
            _ => self.character_sound_name(attacker, anim, extra::NONE),
        }
    }

    /// §3.10.3 the melee and unarmed sequence.
    fn melee(&mut self, draft: &mut Draft, scene: &Scene<'_>, anim: u8) -> Result<(), Refused> {
        let (attacker, defender) = (scene.attacker, scene.defender);
        let main = Into::Main;
        let swing = self.critter_fid(attacker, anim, None, 0);
        let a = self.action_frame(swing);
        self.turn(draft, main, attacker, self.hex(defender), 0)?;
        self.taunt(draft, attacker, Taunt::Attack, 0)?;
        let result = &scene.spray.main;
        let critical = result.attacker_flags & effect::CRITICAL != 0;
        let sound = self.attack_sound(scene, anim);
        if scene.hit || critical {
            self.sound(draft, main, attacker, 0, sound)?;
            self.animate(draft, main, attacker, anim, None, 0)?;
            let contact = match (anim, self.world.wielded(attacker, scene.attack.hit_mode)) {
                (16 | 17, _) | (_, None) => {
                    self.character_sound_name(attacker, anim, extra::CONTACT)
                }
                (_, Some(weapon)) => self.weapon_sound_name(
                    sfx::effect::HIT,
                    weapon,
                    scene.attack.hit_mode,
                    defender,
                ),
            };
            self.sound(draft, main, attacker, a, contact)?;
            self.reactions(draft, scene, 0)?;
        } else if self.prone_status(defender) {
            self.sound(draft, main, attacker, -1, sound)?;
            self.animate(draft, main, attacker, anim, None, 0)?;
        } else if let Some(b) = self.dodge_frame(draft, defender) {
            let dodge = self.character_sound_name(defender, animation::DODGE, extra::NONE);
            if b <= a {
                self.sound(draft, main, attacker, -1, sound)?;
                self.animate(draft, main, attacker, anim, None, 0)?;
                self.sound(draft, main, defender, a - b, dodge)?;
                self.animate(draft, main, defender, animation::DODGE, None, 0)?;
            } else {
                self.sound(draft, main, defender, -1, dodge)?;
                self.animate(draft, main, defender, animation::DODGE, None, 0)?;
                self.sound(draft, main, attacker, b - a, sound)?;
                self.animate(draft, main, attacker, anim, None, 0)?;
            }
        } else {
            // §3.10.3 safe rule: without dodge art the attacker still
            // swings, as against a knocked-down defender.
            self.sound(draft, main, attacker, -1, sound)?;
            self.animate(draft, main, attacker, anim, None, 0)?;
        }
        self.result_taunt(draft, scene)
    }
    /// §3.9 whether the projectile phase can run: the attack hits or
    /// carries no critical bit (a critical failure with an effect keeps
    /// it), and the weapon's projectile PID names a loadable prototype
    /// whose art is not -1. Returns the weapon and the projectile art.
    pub(crate) fn projectile_launch(
        &self,
        attacker: ObjectRef,
        attack: &Attack,
        result: &Resolution,
    ) -> Option<(ObjectRef, u32)> {
        if !matches!(
            attack.subtype(),
            Some(crate::combat::Subtype::Throwing | crate::combat::Subtype::Ranged)
        ) || (result.defender_flags & effect::HIT == 0
            && result.attacker_flags & effect::CRITICAL != 0)
        {
            return None;
        }
        let weapon = self.world.wielded(attacker, attack.hit_mode)?;
        let pid = u32::try_from(self.world.weapon_data(weapon)?.projectile_pid).ok()?;
        let fid = self.assets.prototype(pid).ok()?.art_id;
        (fid != u32::MAX).then_some((weapon, fid))
    }
    /// Knocked out or knocked down (status bits 0x01 or 0x02).
    fn prone_status(&self, critter: ObjectRef) -> bool {
        self.world.critter_state(critter).is_some_and(|state| {
            state.results.0 & (effect::KNOCKED_OUT | effect::KNOCKED_DOWN) != 0
        })
    }
    /// The defender's dodge art's action frame, when that art exists.
    fn dodge_frame(&self, draft: &Draft, defender: ObjectRef) -> Option<i32> {
        let facing = draft.facing.get(&defender).copied().unwrap_or(0);
        let fid = self.critter_fid(defender, animation::DODGE, None, facing);
        self.assets.art_exists(fid).then(|| self.action_frame(fid))
    }

    /// §3.10.4 the ranged and thrown sequence. Returns the throw's
    /// consumption: the thrown unit, its landing, and the hand's code.
    fn ranged(
        &mut self,
        draft: &mut Draft,
        scene: &Scene<'_>,
        anim: u8,
    ) -> Result<Option<Thrown>, Refused> {
        let (attacker, defender) = (scene.attacker, scene.defender);
        let main = Into::Main;
        let shot = self.critter_fid(attacker, anim, None, 0);
        let a = self.action_frame(shot);
        self.turn(draft, main, attacker, self.hex(defender), 0)?;
        if anim != animation::THROW {
            self.animate(draft, main, attacker, animation::POINT, None, -1)?;
        }
        self.taunt(draft, attacker, Taunt::Attack, 0)?;
        let sound = self.attack_sound(scene, anim);
        self.sound(draft, main, attacker, -1, sound)?;
        self.animate(draft, main, attacker, anim, None, 0)?;
        let launch = (anim != animation::CONTINUOUS)
            .then(|| self.projectile_launch(attacker, scene.attack, &scene.spray.main))
            .flatten();
        let thrown = anim == animation::THROW;
        let arrival = if scene.hit {
            self.hex(defender)
        } else {
            scene.impact.or_else(|| self.hex(defender))
        };
        let mut consumption = None;
        let mut erase = Vec::new();
        let mut restore = None;
        match launch.zip(arrival) {
            Some(((weapon, fid), arrival)) => {
                let flight = self.flight(draft, scene, weapon, fid, arrival, a)?;
                erase = flight.erase;
                restore = flight.restore;
                if thrown {
                    let landing = (!flight.explosive).then(|| {
                        let elevation = self
                            .world
                            .get(defender)
                            .map_or(0, |object| object.raw.elevation);
                        (arrival, elevation)
                    });
                    consumption = Some(Thrown {
                        item: weapon,
                        landing,
                        hand: self.world.thrown_hand_code(attacker, weapon),
                    });
                }
            }
            None => {
                // §3.10.4 step 7: without a projectile the defender dodges a
                // miss on the attacker's action frame, with no sound; under
                // the safe rule only when its dodge art exists. A critical
                // failure that keeps its critical bit registers no dodge.
                let result = &scene.spray.main;
                if matches!(anim, 18 | 45 | 46)
                    && !scene.hit
                    && result.attacker_flags & effect::CRITICAL == 0
                    && !self.prone_status(defender)
                    && self.dodge_frame(draft, defender).is_some()
                    && self.world.critter_art(defender)
                {
                    self.animate(draft, main, defender, animation::DODGE, None, a)?;
                }
            }
        }
        let projectile = consumption.is_some() || !erase.is_empty() || restore.is_some();
        self.reactions(draft, scene, if projectile { 0 } else { a })?;
        self.result_taunt(draft, scene)?;
        for owner in erase {
            draft.show.main.push_forced(owner, -1, Action::Remove)?;
        }
        if let Some((owner, art)) = restore {
            draft.show.main.push(owner, -1, Action::SetArt { art })?;
        }
        let attacker_flags = scene.spray.main.attacker_flags;
        let down =
            attacker_flags & (effect::KNOCKED_OUT | effect::KNOCKED_DOWN | effect::DEAD) != 0;
        if !down {
            match &consumption {
                Some(Thrown { hand, .. }) if *hand != 0 => {
                    self.animate(draft, main, attacker, animation::TAKE_OUT, Some(*hand), -1)?;
                }
                // §3.10.4 step 11 sends every throw to §3.9 step 11, where
                // without a refill item the attacker's art changes to
                // standing with weapon code 0. A throw whose projectile
                // phase did not run (a critical failure that kept its
                // critical bit, or no valid projectile) never has a refill
                // item, so it always ends on weapon code 0 (§3.9 R1-rev8),
                // until the art is next rebuilt from the equipped item.
                Some(_) | None if thrown => {
                    let facing = draft.facing.get(&attacker).copied().unwrap_or(0);
                    let art = self.critter_fid(attacker, animation::STAND, Some(0), facing);
                    self.push(draft, main, attacker, -1, Action::SetArt { art })?;
                }
                _ => self.animate(draft, main, attacker, animation::UNPOINT, None, -1)?,
            }
        }
        Ok(consumption)
    }
}

/// What a flight leaves for the clean-up: owners to erase, and the thrown
/// item whose art comes back.
struct Flight {
    explosive: bool,
    erase: Vec<Owner>,
    restore: Option<(Owner, u32)>,
}

impl<A: WorldAssets> WorldHost<A> {
    /// §3.9 steps 3-8 and §3.10.4 step 6: the projectile, hidden on the hex
    /// beside the attacker toward the defender, shows on the attack art's
    /// action frame `a` with its flight sound, and flies one stride-32 node
    /// per step to `arrival`. Then the arrival: an explosion, a thrown
    /// item's hit sound N steps on, or a non-thrown projectile's erase and
    /// hit sound on the weapon.
    fn flight(
        &mut self,
        draft: &mut Draft,
        scene: &Scene<'_>,
        weapon: ObjectRef,
        fid: u32,
        arrival: i32,
        a: i32,
    ) -> Result<Flight, Refused> {
        let (attacker, defender) = (scene.attacker, scene.defender);
        let thrown = scene.attack.mode() == attack_mode::THROW;
        let from = self.hex(attacker).unwrap_or(arrival);
        let elevation = self
            .world
            .get(attacker)
            .map_or(0, |object| object.raw.elevation);
        let direction = rotation(from, self.hex(defender).unwrap_or(arrival)).unwrap_or(0);
        let origin = HexGrid::neighbor(from, direction)
            .ok()
            .flatten()
            .unwrap_or(from);
        let (owner, pid) = if thrown {
            (
                Owner::World(weapon),
                self.world.get(weapon).map_or(0, |object| object.raw.pid),
            )
        } else {
            let pid = self
                .world
                .weapon_data(weapon)
                .map_or(0, |gun| gun.projectile_pid as u32);
            (draft.shown(), pid)
        };
        draft.show.main.show(
            owner,
            Display {
                pid,
                art: fid,
                frame: 0,
                hex: Some(origin),
                elevation,
                orientation: direction,
                displacement: [0; 2],
                hidden: true,
                flat: false,
            },
        );
        let options = RayOptions {
            stride: 32,
            check_blockers: false,
            ignore: None,
        };
        let nodes: Vec<(i32, [i32; 2])> =
            cast_ray(origin, arrival, elevation, options, |_| None, |_| false)
                .samples
                .iter()
                .map(|sample| {
                    (
                        sample.hex,
                        sample.offset.map(|o| o.clamp(-1000, 1000) as i32),
                    )
                })
                .collect();
        let n = if scene.hit {
            nodes.len().saturating_sub(1) as i32
        } else {
            0
        };
        let hit_mode = scene.attack.hit_mode;
        let flight = self.weapon_sound_name(sfx::effect::FLIGHT, weapon, hit_mode, 0);
        let hit = self.weapon_sound_name(sfx::effect::HIT, weapon, hit_mode, scene.defender);
        let sequence = &mut draft.show.main;
        sequence.push(owner, a, Action::Unhide)?;
        sequence.push(
            owner,
            0,
            Action::Sound {
                name: flight.unwrap_or_default(),
                object: Some(if thrown { weapon } else { attacker }),
            },
        )?;
        let frames = self.frames(fid, direction, self.world.combat_speed);
        sequence.push(
            owner,
            0,
            Action::Move {
                art: fid,
                frames,
                nodes,
            },
        )?;
        if let Some(explosion) = &scene.explosion {
            draft.show.explosive = true;
            let mut erase = vec![owner];
            let blast = self.frames(explosion.fid, 0, 0);
            let sequence = &mut draft.show.main;
            if thrown {
                let item = self
                    .world
                    .get(weapon)
                    .map_or(fid, |object| object.raw.art_id);
                sequence.push(owner, -1, Action::SetArt { art: item })?;
            }
            let center = self.weapon_sound_name(sfx::effect::HIT, weapon, hit_mode, 0);
            sequence.push(
                owner,
                if thrown { 0 } else { -1 },
                Action::Sound {
                    name: center.unwrap_or_default(),
                    object: None,
                },
            )?;
            sequence.push(
                owner,
                0,
                Action::Animate {
                    art: explosion.fid,
                    frames: blast.clone(),
                    hide: true,
                },
            )?;
            for object in explosion.objects.iter().skip(1) {
                let neighbor = draft.shown();
                draft.show.main.show(
                    neighbor,
                    Display {
                        pid: crate::world::EXPLOSION_ART_PID,
                        art: explosion.fid,
                        frame: 0,
                        hex: Some(object.hex),
                        elevation,
                        orientation: 0,
                        displacement: [0; 2],
                        hidden: true,
                        flat: false,
                    },
                );
                draft.show.main.push(
                    neighbor,
                    object.delay as i32,
                    Action::Animate {
                        art: explosion.fid,
                        frames: blast.clone(),
                        hide: true,
                    },
                )?;
                erase.push(neighbor);
            }
            return Ok(Flight {
                explosive: true,
                erase,
                restore: None,
            });
        }
        if thrown {
            sequence.push(
                owner,
                n,
                Action::Sound {
                    name: hit.unwrap_or_default(),
                    object: Some(weapon),
                },
            )?;
            let item = self
                .world
                .get(weapon)
                .map_or(fid, |object| object.raw.art_id);
            return Ok(Flight {
                explosive: false,
                erase: Vec::new(),
                restore: Some((owner, item)),
            });
        }
        sequence.push_forced(owner, -1, Action::Remove)?;
        sequence.push(
            owner,
            n,
            Action::Sound {
                name: hit.unwrap_or_default(),
                object: Some(weapon),
            },
        )?;
        Ok(Flight {
            explosive: false,
            erase: Vec::new(),
            restore: None,
        })
    }

    /// §3.10.6 the reactions, registered at one point with delay `d`: a
    /// placeholder per critter extra (the first carrying `d`), then the
    /// attacker's own reaction for a miss with the critical or backwash
    /// bit, or for a hit the defender's (delay `d` without placeholders)
    /// and, with the dud bit, the attacker's.
    fn reactions(&mut self, draft: &mut Draft, scene: &Scene<'_>, d: i32) -> Result<(), Refused> {
        let attacker = scene.attacker;
        let mut placeholders = 0;
        for (object, _, _) in &scene.spray.extras {
            if !self.world.critter_art(*object) {
                continue;
            }
            let index = draft.show.extras.len();
            draft.show.extras.push(Sequence::default());
            let delay = if placeholders == 0 { d } else { 0 };
            self.push(draft, Into::Main, *object, delay, Action::Release(index))?;
            placeholders += 1;
        }
        // Extras whose victims are not critters get nothing.
        let result = &scene.spray.main;
        // The attacker's own reaction: delay -1, from the front, no
        // knockback.
        let own = || React {
            critter: attacker,
            flags: result.attacker_flags,
            knockback: 0,
            direction: None,
            delay: -1,
            from_front: true,
            profile: scene.attack,
            damage: result.self_damage,
        };
        if !scene.hit && result.attacker_flags & (effect::CRITICAL | effect::BACKWASH) != 0 {
            return self.react(draft, Into::Main, scene, own());
        }
        if !scene.hit {
            return Ok(());
        }
        let defender = scene.defender;
        if self.world.critter_art(defender) {
            let (from, to) = (self.hex(attacker), self.hex(defender));
            let direction = from.zip(to).and_then(|(from, to)| rotation(from, to));
            // §3.10.6 safe rule: the attacker faces along the attacker-to-
            // defender direction for the recorded defender's front test.
            let facing = direction.unwrap_or(0);
            let from_front = self
                .world
                .get(defender)
                .is_some_and(|object| matches!(facing.abs_diff(object.raw.orientation), 2..=4));
            self.react(
                draft,
                Into::Main,
                scene,
                React {
                    critter: defender,
                    flags: result.defender_flags,
                    knockback: result.knockback,
                    direction,
                    delay: if placeholders == 0 { d } else { 0 },
                    from_front,
                    profile: scene.victim,
                    damage: result.damage,
                },
            )?;
        }
        if result.attacker_flags & effect::DUD != 0 {
            self.react(draft, Into::Main, scene, own())?;
        }
        Ok(())
    }
}

/// One critter's reaction inputs (§3.10.6).
struct React<'a> {
    critter: ObjectRef,
    flags: u32,
    knockback: i32,
    /// The attacker-to-critter hex direction, for knockback.
    direction: Option<u8>,
    delay: i32,
    from_front: bool,
    /// The attack profile against this critter, for the death choice.
    profile: &'a Attack,
    damage: i32,
}

impl<A: WorldAssets> WorldHost<A> {
    /// §3.10.6 the reaction of one critter: the first matching case of the
    /// table, then the weapon effects and the death callback.
    fn react(
        &mut self,
        draft: &mut Draft,
        into: Into,
        scene: &Scene<'_>,
        react: React<'_>,
    ) -> Result<(), Refused> {
        let critter = react.critter;
        let flags = react.flags;
        let d = react.delay;
        let dead = flags & effect::DEAD != 0;
        let current = self
            .world
            .get(critter)
            .map_or(0, |object| Fid(object.raw.art_id).animation());
        let prone = self.prone_status(critter) || matches!(current, 20..=35 | 48..=63);
        if prone {
            if dead && !self.world.is_dead(critter) {
                self.blood(draft, into, critter, current, d)?;
                self.weapon_effects(draft, into, scene, critter, flags, current)?;
            }
            return Ok(());
        }
        let death = dead.then(|| self.death_animation(scene, &react));
        let mut final_animation = death.unwrap_or(0);
        if let Some(animation) = death
            && animation == animation::FIRE_DANCE
        {
            final_animation = self.fire_dance(draft, into, critter, d)?;
        } else if let Some(animation) = death
            && react.knockback != 0
            && matches!(animation, 20 | 21)
        {
            self.knockback_executor(draft, into, &react, animation, d, true)?;
            self.blood(draft, into, critter, animation, -1)?;
        } else if let Some(animation) = death {
            let die = self.character_sound_name(critter, animation, extra::DIE);
            self.sound(draft, into, critter, d, die)?;
            let fall = self.fall_choice(critter, animation);
            final_animation = fall;
            self.animate(draft, into, critter, fall, None, 0)?;
            if matches!(fall, 20 | 21) {
                self.blood(draft, into, critter, fall, -1)?;
            }
        } else if flags & (effect::KNOCKED_OUT | effect::KNOCKED_DOWN) != 0 {
            let fall = if react.from_front {
                animation::FALL_BACK
            } else {
                animation::FALL_FRONT
            };
            let name = self.character_sound_name(critter, fall, extra::NONE);
            self.sound(draft, into, critter, d, name)?;
            if react.knockback != 0 {
                // §3.10.6 safe rule: the knockdown sound plays once (the
                // executor's second, identical name is left out).
                self.knockback_executor(draft, into, &react, fall, 0, false)?;
            } else {
                let fall = self.fall_choice(critter, fall);
                self.animate(draft, into, critter, fall, None, 0)?;
            }
        } else if flags & effect::ON_FIRE != 0
            && self.has_art(draft, critter, animation::FIRE_DANCE)
        {
            self.animate(draft, into, critter, animation::FIRE_DANCE, None, d)?;
            let facing = draft.facing.get(&critter).copied().unwrap_or(0);
            let art = self.critter_fid(critter, animation::STAND, None, facing);
            self.push(draft, into, critter, -1, Action::SetArt { art })?;
        } else if react.knockback != 0 {
            let fall = if react.from_front {
                animation::FALL_BACK
            } else {
                animation::FALL_FRONT
            };
            self.knockback_executor(draft, into, &react, fall, d, false)?;
            let up = if fall == animation::FALL_BACK {
                animation::BACK_UP
            } else {
                animation::PRONE_UP
            };
            self.animate(draft, into, critter, up, None, -1)?;
        } else {
            let flinch = if react.from_front || !self.has_art(draft, critter, animation::HIT_BACK) {
                animation::HIT_FRONT
            } else {
                animation::HIT_BACK
            };
            let name = self.character_sound_name(critter, flinch, extra::NONE);
            self.sound(draft, into, critter, d, name)?;
            self.animate(draft, into, critter, flinch, None, 0)?;
        }
        self.weapon_effects(draft, into, scene, critter, flags, final_animation)
    }
    fn has_art(&self, draft: &Draft, critter: ObjectRef, animation: u8) -> bool {
        let facing = draft.facing.get(&critter).copied().unwrap_or(0);
        self.assets
            .art_exists(self.critter_fid(critter, animation, None, facing))
    }
    /// §3.8 the lethal animation for a reaction.
    fn death_animation(&self, scene: &Scene<'_>, react: &React<'_>) -> u8 {
        let (Some(attacker), Some(victim)) = (
            self.world.get(scene.attacker),
            self.world.get(react.critter),
        ) else {
            return animation::FALL_BACK;
        };
        // §3.10.6 safe rule: the attacker faces along the hex direction.
        let facing = react.direction.unwrap_or(attacker.raw.orientation);
        let profile = react.profile;
        let choice = DeathChoice {
            violence: self.world.violence.unwrap_or(2),
            attacker_player: profile.attacker_player,
            attacker_pid: attacker.raw.pid,
            attacker_art: attacker.raw.art_id,
            victim_pid: victim.raw.pid,
            orientations: [facing, victim.raw.orientation],
            damage_type: profile
                .weapon
                .as_ref()
                .map_or(damage_type::NORMAL, |weapon| weapon.damage_type),
            mode: profile.mode(),
            damage: react.damage,
            bloody_mess: profile.player.bloody_mess,
            attack_animation: scene
                .selector
                .unwrap_or(Fid(attacker.raw.art_id).animation()),
        };
        let orientation = victim.raw.orientation;
        choice.animation(|animation| {
            self.assets
                .art_exists(self.critter_fid(react.critter, animation, None, orientation))
        })
    }
    /// §3.8 the fall choice: a fall back or front reverses when a blocker
    /// stands one or two hexes in its falling direction (behind the critter
    /// for 20, ahead of it for 21); an unavailable fall-front becomes
    /// fall-back.
    fn fall_choice(&self, critter: ObjectRef, animation: u8) -> u8 {
        if !matches!(animation, 20 | 21) {
            return animation;
        }
        let Some((hex, elevation, facing)) = self.world.get(critter).and_then(|object| {
            Some((
                object.raw.hex?,
                object.raw.elevation,
                object.raw.orientation,
            ))
        }) else {
            return animation;
        };
        let blocked = |direction: u8| {
            let near = step(hex, direction);
            let far = step(near, direction);
            [near, far].iter().any(|place| {
                *place != hex
                    && self
                        .world
                        .movement_blocker(*place, elevation, critter)
                        .is_some()
            })
        };
        let (falling, reversed) = if animation == animation::FALL_BACK {
            ((facing + 3) % 6, animation::FALL_FRONT)
        } else {
            (facing, animation::FALL_BACK)
        };
        let choice = if blocked(falling) {
            reversed
        } else {
            animation
        };
        let fid = self.critter_fid(critter, animation::FALL_FRONT, None, facing);
        if choice == animation::FALL_FRONT && !self.assets.art_exists(fid) {
            animation::FALL_BACK
        } else {
            choice
        }
    }
    /// Blood after a fall (§3.10.6): animation 34 after 20, 35 after 21,
    /// when violence is above 0 and the art exists.
    fn blood(
        &self,
        draft: &mut Draft,
        into: Into,
        critter: ObjectRef,
        fall: u8,
        delay: i32,
    ) -> Result<(), Refused> {
        let bloody = match fall {
            animation::FALL_BACK => animation::BLOODY_BACK,
            animation::FALL_FRONT => animation::BLOODY_FRONT,
            _ => return Ok(()),
        };
        if self.world.violence.unwrap_or(2) <= 0 || !self.has_art(draft, critter, bloody) {
            return Ok(());
        }
        self.animate(draft, into, critter, bloody, None, delay)
    }
    /// §3.8 the knockback executor: the fall's sound with the knockdown
    /// extra (unless `sound` is false), then the fall in place or sliding
    /// to the final hex, which the world takes at once.
    fn knockback_executor(
        &self,
        draft: &mut Draft,
        into: Into,
        react: &React<'_>,
        fall: u8,
        delay: i32,
        sound: bool,
    ) -> Result<(), Refused> {
        let critter = react.critter;
        let mut delay = delay;
        if sound {
            let name = self.character_sound_name(critter, fall, extra::KNOCKDOWN);
            self.sound(draft, into, critter, delay, name)?;
            delay = 0;
        }
        let slide = self.knockback_slide(critter, react.direction, react.knockback);
        let facing = draft.facing.get(&critter).copied().unwrap_or(0);
        let fid = self.critter_fid(critter, fall, None, facing);
        if !self.assets.art_exists(fid) {
            return Err(Refused::Art(fid));
        }
        let frames = self.frames(fid, facing, 0);
        let action = match slide.last() {
            Some(last) => {
                if let Some(object) = self.world.get(critter) {
                    draft.effects.push(Effect::Move {
                        object: critter,
                        hex: *last,
                        elevation: object.raw.elevation,
                    });
                }
                let mut nodes: Vec<(i32, [i32; 2])> =
                    slide.iter().map(|hex| (*hex, [0, 0])).collect();
                // The fall keeps playing on the final hex.
                while nodes.len() < frames.shifts.len().max(1) {
                    nodes.push((*last, [0, 0]));
                }
                Action::Move {
                    art: fid,
                    frames,
                    nodes,
                }
            }
            None => Action::Animate {
                art: fid,
                frames,
                hide: false,
            },
        };
        self.push(draft, into, critter, delay, action)
    }
    /// The hexes a knockback slides through (§3.8, the safe rule's `j - 1`).
    fn knockback_slide(
        &self,
        critter: ObjectRef,
        direction: Option<u8>,
        distance: i32,
    ) -> Vec<i32> {
        let (Some(direction), Some((hex, elevation))) = (
            direction,
            self.world
                .get(critter)
                .and_then(|object| Some((object.raw.hex?, object.raw.elevation))),
        ) else {
            return Vec::new();
        };
        let path: Vec<i32> = (0..distance.max(0))
            .scan(hex, |at, _| {
                *at = step(*at, direction);
                Some(*at)
            })
            .collect();
        let moved = crate::combat::knockback_displacement(distance, |j| {
            self.world
                .movement_blocker(path[(j - 1) as usize], elevation, critter)
                .is_some()
        });
        path[..moved.max(0) as usize].to_vec()
    }
    /// §3.10.6 the fire-dance death: its sound and dance in place, then a
    /// run of 2-5 hexes in a random direction to the farthest hex whose
    /// stride-4 path is clear (none when every path is blocked), then burned
    /// to nothing (29). Returns the final animation.
    fn fire_dance(
        &mut self,
        draft: &mut Draft,
        into: Into,
        critter: ObjectRef,
        d: i32,
    ) -> Result<u8, Refused> {
        let dance = self.character_sound_name(critter, animation::FIRE_DANCE, extra::NONE);
        self.sound(draft, into, critter, d, dance)?;
        self.animate(draft, into, critter, animation::FIRE_DANCE, None, 0)?;
        let distance = self.world.rng.range(2, 5).unwrap_or(2);
        let direction = self.world.rng.range(0, 5).unwrap_or(0) as u8;
        if let Some((hex, elevation)) = self
            .world
            .get(critter)
            .and_then(|object| Some((object.raw.hex?, object.raw.elevation)))
        {
            let run = (1..=distance).rev().find_map(|j| {
                let goal = HexGrid::step(hex, direction, j).ok()?;
                let ray = self.world.ray(critter, hex, goal, 4, Some(critter));
                (goal != hex && ray.obstacle.is_none()).then_some((goal, ray))
            });
            if let Some((goal, ray)) = run {
                self.turn(draft, into, critter, Some(goal), -1)?;
                let facing = draft.facing.get(&critter).copied().unwrap_or(0);
                let fid = self.critter_fid(critter, animation::FIRE_DANCE, None, facing);
                let frames = self.frames(fid, facing, 0);
                let nodes = ray
                    .samples
                    .iter()
                    .map(|sample| {
                        (
                            sample.hex,
                            sample.offset.map(|o| o.clamp(-1000, 1000) as i32),
                        )
                    })
                    .collect();
                self.push(
                    draft,
                    into,
                    critter,
                    0,
                    Action::Move {
                        art: fid,
                        frames,
                        nodes,
                    },
                )?;
                draft.effects.push(Effect::Move {
                    object: critter,
                    hex: goal,
                    elevation,
                });
            }
        }
        let burned = self.character_sound_name(critter, animation::BURNED, extra::NONE);
        self.sound(draft, into, critter, -1, burned)?;
        self.animate(draft, into, critter, animation::BURNED, None, 0)?;
        Ok(animation::BURNED)
    }
    /// §3.10.6 the weapon effects on the attack's weapon (the attacker's)
    /// for the reacting critter's flags, and the death callback.
    fn weapon_effects(
        &self,
        draft: &mut Draft,
        into: Into,
        scene: &Scene<'_>,
        critter: ObjectRef,
        flags: u32,
        final_animation: u8,
    ) -> Result<(), Refused> {
        let weapon = self.world.wielded(scene.attacker, scene.attack.hit_mode);
        let place = self
            .world
            .get(critter)
            .and_then(|object| Some((object.raw.hex?, object.raw.elevation)));
        if let Some(weapon) = weapon {
            if flags & effect::EXPLODE != 0
                && let Some((hex, elevation)) = place
            {
                let owner = self.own(draft, into, weapon);
                let blast = self.frames(EXPLODED_WEAPON_ART, 0, 0);
                let sound = self.weapon_sound_name(
                    sfx::effect::HIT,
                    weapon,
                    hit_mode::RIGHT_PRIMARY,
                    critter,
                );
                let sequence = Self::sequence(draft, into);
                sequence.push_forced(owner, -1, Action::Place { hex, elevation })?;
                sequence.push(
                    owner,
                    0,
                    Action::SetArt {
                        art: EXPLODED_WEAPON_ART,
                    },
                )?;
                sequence.push(
                    owner,
                    0,
                    Action::Animate {
                        art: EXPLODED_WEAPON_ART,
                        frames: blast,
                        hide: true,
                    },
                )?;
                sequence.push(
                    owner,
                    0,
                    Action::Sound {
                        name: sound.unwrap_or_default(),
                        object: Some(weapon),
                    },
                )?;
                sequence.push_forced(owner, -1, Action::Remove)?;
                draft.effects.push(Effect::Destroy { weapon });
            }
            if flags & effect::DESTROY != 0 {
                let owner = self.own(draft, into, weapon);
                Self::sequence(draft, into).push_forced(owner, -1, Action::Remove)?;
                draft.effects.push(Effect::Destroy { weapon });
            }
            if flags & effect::DROP != 0
                && let Some((hex, elevation)) = self
                    .world
                    .get(scene.attacker)
                    .and_then(|object| Some((object.raw.hex?, object.raw.elevation)))
            {
                let owner = self.own(draft, into, weapon);
                Self::sequence(draft, into).push_forced(
                    owner,
                    -1,
                    Action::Place { hex, elevation },
                )?;
                draft.effects.push(Effect::Drop {
                    weapon,
                    hex,
                    elevation,
                });
            }
        }
        if flags & effect::DEAD != 0 {
            let Some((victim, (hex, elevation))) = self.world.get(critter).zip(place) else {
                return Ok(());
            };
            let flat = corpse_flattens(victim.raw.pid);
            let dropped: Vec<ObjectRef> = if crate::combat::final_animation_drops_inventory(
                final_animation,
                victim.raw.pid,
            ) {
                victim.inventory.iter().map(|(item, _)| *item).collect()
            } else {
                Vec::new()
            };
            for item in &dropped {
                self.own(draft, into, *item);
            }
            let owner = self.own(draft, into, critter);
            Self::sequence(draft, into).push_forced(
                owner,
                -1,
                Action::Corpse {
                    flat,
                    dropped,
                    hex,
                    elevation,
                },
            )?;
            draft.effects.push(Effect::Corpse {
                victim: critter,
                animation: final_animation,
            });
        }
        Ok(())
    }

    /// The world takes a built attack's effects: weapon drops and
    /// destruction, knockback and fire-dance moves, and corpse
    /// presentation.
    pub(crate) fn apply_effects(&mut self, effects: &[Effect]) -> Result<(), WorldError> {
        for effect in effects {
            match effect {
                Effect::Move {
                    object,
                    hex,
                    elevation,
                } => self.world.move_object(*object, *hex, *elevation, false)?,
                Effect::Corpse { victim, animation } => self.present_corpse(*victim, *animation)?,
                Effect::Drop {
                    weapon,
                    hex,
                    elevation,
                } => {
                    if let Some(owner) = self.world.get(*weapon).and_then(|object| object.owner) {
                        let quantity = self.world.entry_count(owner, *weapon).unwrap_or(1);
                        self.world.remove_item(owner, *weapon, quantity)?;
                    }
                    self.world.move_object(*weapon, *hex, *elevation, false)?;
                }
                Effect::Destroy { weapon } => {
                    if let Some(owner) = self.world.get(*weapon).and_then(|object| object.owner) {
                        let quantity = self.world.entry_count(owner, *weapon).unwrap_or(1);
                        self.world.remove_item(owner, *weapon, quantity)?;
                    }
                    self.world.erase(*weapon);
                }
            }
        }
        Ok(())
    }
    /// §3.9 the throw's consumption once its sequence registered: one unit
    /// leaves the attacker; lit at the requested radius 9 (8, or none
    /// without intensity), it lands where the projectile ends, or is
    /// erased when explosive. Under the §3.9 safe rule a thrower left
    /// without a lit flare in either hand gets its ordinary light back.
    pub(crate) fn consume_throw(
        &mut self,
        attacker: ObjectRef,
        item: ObjectRef,
        landing: Option<(i32, u8)>,
    ) -> Result<(), WorldError> {
        let (projectile, _) = self.world.consume_thrown(attacker, item)?;
        match landing {
            Some((hex, elevation)) => {
                let intensity = self
                    .world
                    .get(projectile)
                    .map_or(0, |object| object.raw.light.intensity);
                self.world
                    .set_light(projectile, PROJECTILE_LIGHT_RADIUS, intensity);
                self.world.move_object(projectile, hex, elevation, false)?;
            }
            None => self.world.erase(projectile),
        }
        let lit = self.world.get(attacker).is_some_and(|object| {
            object.inventory.iter().any(|(held, _)| {
                self.world.get(*held).is_some_and(|held| {
                    held.raw.flags.0 & (BasicFlags::LEFT_HAND.0 | BasicFlags::RIGHT_HAND.0) != 0
                        && held.raw.pid == LIT_FLARE
                })
            })
        });
        if !lit {
            let light = if attacker == self.world.player {
                Some(PLAYER_LIGHT)
            } else {
                self.world
                    .get(attacker)
                    .and_then(|object| object.prototype.as_ref()?.light)
                    .map(|light| (light.radius, light.intensity))
            };
            if let Some((radius, intensity)) = light
                && let Some(object) = self.world.get_mut(attacker)
            {
                object.raw.light = vault_formats::pro::Light { radius, intensity };
                self.world.rebuild_lighting();
            }
        }
        Ok(())
    }
}

impl crate::WorldState {
    /// §3.9 the weapon code the attacker's hand shows after throwing `item`:
    /// under the safe rule the units left in the hand keep its code;
    /// otherwise that of the first identical unequipped entry, which
    /// refills the hand; 0 when nothing does.
    pub(crate) fn thrown_hand_code(&self, attacker: ObjectRef, item: ObjectRef) -> u8 {
        let code = |item: ObjectRef| match self.item_data(item).map(|data| &data.data) {
            Some(ItemData::Weapon(weapon)) => u8::try_from(weapon.animation).unwrap_or(0),
            _ => 0,
        };
        if self.entry_count_any(item).unwrap_or(1) > 1 {
            return code(item);
        }
        self.refill_for(attacker, item).map_or(0, code)
    }
    /// The quantity of `item`'s entry in its owner's inventory.
    fn entry_count_any(&self, item: ObjectRef) -> Option<i32> {
        let owner = self.get(item)?.owner?;
        self.entry_count(owner, item)
    }
    /// The quantity of `item`'s entry in `owner`'s own inventory.
    pub(crate) fn entry_count(&self, owner: ObjectRef, item: ObjectRef) -> Option<i32> {
        self.get(owner)?
            .inventory
            .iter()
            .find(|(entry, _)| *entry == item)
            .map(|(_, count)| *count)
    }
}
