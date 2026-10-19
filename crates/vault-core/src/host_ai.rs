//! SPEC-009 world integration: AI initialization and the language-filter
//! state, packet resolution, the last-attacker setter, retaliation and
//! onlooker alerting, danger selection over live state, the NPC turn (flee,
//! healing, weapon search and switching, hit mode, called shots, the bounded
//! attack loop, reloading), team switching, random retargeting, and combat
//! taunts.
use crate::{
    Movement, ObjectRef, RandomSource, StepOutcome, WorldEffect, WorldError, WorldState,
    ai::{
        self, AttackEnv, CalledShot, HealingInventory, InventoryWeapon, ModeWeapon, Switched,
        Taunt, TauntCallback, ThreatView, WeaponChoice, intent,
    },
    assets::WorldAssets,
    combat::{Attack, Resolution, weapon_skill},
    combat_state::CombatRequest,
    critical::{effect, hit_mode, location},
    host::WorldHost,
    host_combat::{Refusal, Spray},
    rng::stat_check,
    routing::rotation,
};
use std::sync::Arc;
use vault_formats::{
    Fid, HexGrid,
    ai::{Packet, Packets},
    map::{ObjectDetail, ObjectState},
    pro::{
        BasicFlags, Category, ExtendedFlags, Item, ItemData, ItemSubtype, PrototypeData, Weapon,
    },
};

impl ThreatView for WorldState {
    /// A reference that no longer resolves counts as none.
    fn last_attacker(&self, critter: ObjectRef) -> Option<ObjectRef> {
        self.get(critter)
            .map(|object| object.opponent)
            .filter(|reference| *reference != 0 && self.get(*reference).is_some())
    }
    fn dead(&self, critter: ObjectRef) -> bool {
        self.is_dead(critter)
    }
    fn team(&self, critter: ObjectRef) -> i32 {
        self.critter_state(critter).map_or(0, |state| state.team)
    }
    fn dist(&self, a: ObjectRef, b: ObjectRef) -> i32 {
        self.object_distance(a, b)
    }
    fn perceives(&self, observer: ObjectRef, target: ObjectRef) -> bool {
        self.can_perceive(observer, target)
    }
}

impl WorldState {
    /// §3.6 party membership: object IDs below 18000 are never members;
    /// otherwise the party list, which includes the player, decides.
    pub fn party_member(&self, critter: ObjectRef) -> bool {
        self.get(critter)
            .is_some_and(|object| object.raw.id >= ai::PARTY_ID_FLOOR)
            && (critter == self.player || self.party.contains(&critter))
    }
    pub(crate) fn effective(&self, critter: ObjectRef, stat: usize) -> i32 {
        self.get(critter)
            .and_then(|object| object.character.as_ref())
            .map_or(0, |sheet| sheet.stat(stat, self.context(critter)))
    }
    pub(crate) fn body_type(&self, critter: ObjectRef) -> i32 {
        self.get(critter)
            .and_then(|object| object.character.as_ref())
            .map_or(0, |sheet| sheet.body)
    }
    fn critter_art(&self, object: ObjectRef) -> bool {
        self.get(object)
            .is_some_and(|object| Fid(object.raw.art_id).category() == 1)
    }
    fn knocked_out(&self, critter: ObjectRef) -> bool {
        self.critter_state(critter)
            .is_some_and(|state| state.results.0 & effect::KNOCKED_OUT != 0)
    }
    pub(crate) fn action_points(&self, critter: ObjectRef) -> i32 {
        self.critter_state(critter).map_or(0, |state| state.ap)
    }
    fn add_intent(&mut self, critter: ObjectRef, bits: u32) {
        if let Some(state) = self.critter_state_mut(critter) {
            state.maneuvers |= bits;
        }
    }
    pub fn item_subtype(&self, item: ObjectRef) -> Option<ItemSubtype> {
        match &self.get(item)?.prototype.as_deref()?.data {
            PrototypeData::Item(data) => Some(data.data.subtype()),
            _ => None,
        }
    }
    pub(crate) fn weapon_data(&self, item: ObjectRef) -> Option<&Weapon> {
        match &self.get(item)?.prototype.as_deref()?.data {
            PrototypeData::Item(Item {
                data: ItemData::Weapon(weapon),
                ..
            }) => Some(weapon),
            _ => None,
        }
    }
    /// The extended-flag attack-mode nibble for a hit mode.
    fn mode_nibble(&self, weapon: ObjectRef, mode: u8) -> u8 {
        let secondary = u32::from(matches!(
            mode,
            hit_mode::LEFT_SECONDARY | hit_mode::RIGHT_SECONDARY
        ));
        self.get(weapon)
            .and_then(|object| object.prototype.as_deref())
            .map_or(0, |prototype| {
                ((prototype.extended.0 >> (4 * secondary)) & 15) as u8
            })
    }
    /// Loaded rounds and ammunition PID of a weapon instance.
    pub fn loaded(&self, weapon: ObjectRef) -> (i32, i32) {
        match self.get(weapon).map(|object| &object.raw.state) {
            Some(ObjectState::Other {
                detail: ObjectDetail::Weapon { rounds, ammo_pid },
                ..
            }) => (*rounds, *ammo_pid),
            _ => (0, -1),
        }
    }
    pub(crate) fn ammo_rounds(&self, ammo: ObjectRef) -> i32 {
        match self.get(ammo).map(|object| &object.raw.state) {
            Some(ObjectState::Other {
                detail: ObjectDetail::Ammunition(rounds),
                ..
            }) => *rounds,
            _ => 0,
        }
    }
    pub(crate) fn set_ammo_rounds(&mut self, ammo: ObjectRef, rounds: i32) {
        if let Some(object) = self.get_mut(ammo) {
            let word = match object.raw.state {
                ObjectState::Other { word, .. } => word,
                ObjectState::Critter(_) => return,
            };
            object.raw.state = ObjectState::Other {
                word,
                detail: ObjectDetail::Ammunition(rounds),
            };
        }
    }
    /// SPEC-007 §3.4 reload permission: a weapon and ammunition of equal
    /// caliber; a loaded weapon also requires its loaded ammunition PID.
    pub fn reload_permitted(&self, weapon: ObjectRef, ammo: ObjectRef) -> bool {
        let Some(gun) = self.weapon_data(weapon) else {
            return false;
        };
        let Some((pid, PrototypeData::Item(item))) = self
            .get(ammo)
            .and_then(|object| Some((object.raw.pid, &object.prototype.as_deref()?.data)))
        else {
            return false;
        };
        let ItemData::Ammunition { caliber, .. } = item.data else {
            return false;
        };
        let (rounds, loaded) = self.loaded(weapon);
        gun.caliber == caliber && (rounds <= 0 || pid as i32 == loaded)
    }
    /// §3.4 retaliation rating: 0 for nothing, non-critter art, or a dead or
    /// knocked-out critter; otherwise Melee Damage raised to any greater
    /// right-hand, then left-hand weapon maximum, plus effective AC (which
    /// includes the unspent-AP bonus).
    pub fn retaliation_rating(&self, critter: ObjectRef) -> i32 {
        let rateable = critter != 0
            && self.critter_art(critter)
            && !self.is_dead(critter)
            && !self.knocked_out(critter);
        let maximum = |mode| {
            self.wielded(critter, mode)
                .and_then(|item| self.weapon_data(item))
                .map(|weapon| weapon.damage[1])
        };
        ai::retaliation_rating(rateable.then(|| ai::Rating {
            rateable,
            melee_damage: self.effective(critter, 11),
            weapons: [
                maximum(hit_mode::RIGHT_PRIMARY),
                maximum(hit_mode::LEFT_PRIMARY),
            ],
            armor_class: self.effective(critter, 9),
        }))
    }
}

/// Map a preflight refusal to its SPEC-009 result code.
fn refusal_code(refusal: Refusal) -> u8 {
    match refusal {
        Refusal::NoAmmunition => 1,
        Refusal::Range => 2,
        Refusal::ActionPoints => 3,
        Refusal::DeadDefender => 4,
        Refusal::Obstructed => 5,
        Refusal::CrippledArmTwoHanded => 6,
        Refusal::CrippledArms => 7,
    }
}

impl<A: WorldAssets> WorldHost<A> {
    /// §3.1 initialization: `combatai.msg` (failure is an error), then
    /// `data/ai.txt` into the sorted packet table. A missing file or a
    /// zero-packet table is an error rather than an empty table.
    pub fn load_combat_ai(&mut self) -> Result<(), WorldError> {
        let messages = self.assets.game_messages("combatai.msg")?;
        let packets = Packets::parse(&self.assets.resource("data/ai.txt")?)?;
        for diagnostic in &packets.diagnostics {
            self.world.diagnostic(format!("ai.txt: {diagnostic}"));
        }
        self.world.ai.packets = Some(Arc::new(packets));
        self.world.ai.messages = Some(messages);
        // A nonzero language-filter preference at load applies the filter.
        self.world.ai.filtered = false;
        if self.world.ai.language_filter.unwrap_or(0) != 0 {
            self.apply_language_filter();
        }
        Ok(())
    }
    /// §2.5 preference re-evaluation. The remembered state starts at -1, so
    /// the first evaluation acts. A changed value of exactly 1 filters the
    /// loaded catalog; any other changed value reloads it unfiltered.
    pub fn reevaluate_language_filter(&mut self) -> Result<(), WorldError> {
        let value = self.world.ai.language_filter.unwrap_or(0);
        if value == self.world.ai.filter_state {
            return Ok(());
        }
        self.world.ai.filter_state = value;
        if self.world.ai.messages.is_none() {
            return Ok(());
        }
        if value == 1 {
            self.apply_language_filter();
        } else {
            self.world.ai.messages = Some(self.assets.game_messages("combatai.msg")?);
            self.world.ai.filtered = false;
        }
        Ok(())
    }
    fn apply_language_filter(&mut self) {
        // SPEC-002 defines no language-filter transform: the state is
        // tracked, and the catalog text is kept unchanged.
        self.world.ai.filtered = true;
        self.world
            .diagnostic("no language-filter transform is specified; combatai.msg is unchanged");
    }
    /// §3.1 packet resolution at every query: the first sorted packet with
    /// the critter's instance number, else table entry 0 (diagnosed).
    pub fn packet(&mut self, critter: ObjectRef) -> Option<Packet> {
        let packets = self.world.ai.packets.clone()?;
        let number = self.world.critter_state(critter)?.ai;
        let (packet, matched) = packets.resolve(number);
        if !matched {
            self.world.diagnostic(format!(
                "AI packet {number} is not defined; using [{}]",
                String::from_utf8_lossy(&packet.name)
            ));
        }
        Some(packet.clone())
    }
    /// §3.4 standard setter: refuse a non-critter value, store it, and for
    /// the player write reaction 1 into the critter's script local 0.
    pub fn set_last_attacker(&mut self, critter: ObjectRef, value: ObjectRef) {
        if value != 0 && !self.world.critter_art(value) {
            return;
        }
        let Some(object) = self.world.get_mut(critter) else {
            return;
        };
        object.opponent = value;
        if value != 0
            && value == self.world.player
            && let Some(script) = self.world.script_for(critter)
        {
            self.world.set_local(Some(script), 0, 1);
        }
    }
    /// §3.4 retaliation check: switch only to a strictly higher rating.
    pub fn retaliate(&mut self, victim: ObjectRef, attacker: ObjectRef) {
        let current = self.world.get(victim).map_or(0, |object| object.opponent);
        if self.world.retaliation_rating(attacker) > self.world.retaliation_rating(current) {
            self.set_last_attacker(victim, attacker);
        }
    }
    /// §3.3 onlooker alerting: every snapshot entry that perceives the
    /// damaged critter (itself, dead entries, and any team included).
    pub fn alert_onlookers(&mut self, victim: ObjectRef) {
        let watchers: Vec<_> = self
            .world
            .ai
            .snapshot
            .iter()
            .copied()
            .filter(|watcher| self.world.can_perceive(*watcher, victim))
            .collect();
        for watcher in watchers {
            self.world.add_intent(watcher, intent::ALERT);
        }
    }
    /// §3.4 damage-application writers: a dead or knocked-out victim of a
    /// critter references it; a still-active defender runs the retaliation
    /// check if unintended or on another team, an extra only if on another
    /// team. Onlookers are alerted first.
    pub(crate) fn after_damage(&mut self, attacker: ObjectRef, victim: ObjectRef, defender: bool) {
        self.alert_onlookers(victim);
        if !self.world.critter_art(attacker) {
            return;
        }
        if self.world.is_dead(victim) || self.world.knocked_out(victim) {
            self.set_last_attacker(victim, attacker);
            return;
        }
        let other_team =
            ThreatView::team(&self.world, victim) != ThreatView::team(&self.world, attacker);
        let unintended = defender && self.world.ai.unintended == victim;
        if other_team || unintended {
            self.retaliate(victim, attacker);
        }
    }
    /// §3.4 explosion attribution with a known source: each surviving victim
    /// other than the source references it. (The explosion geometry belongs
    /// to SPEC-007 §3.7.)
    pub fn attribute_explosion(&mut self, source: ObjectRef, victims: &[ObjectRef]) {
        for victim in victims {
            if *victim != source && !self.world.is_dead(*victim) {
                self.set_last_attacker(*victim, source);
            }
        }
    }
    /// §3.4 primed explosive dropped while closing the inventory: each
    /// non-player critter off the player's team in the explosion-extras set
    /// makes a Perception check (one D10 each, in set order); a success
    /// references the player, and the first such critter starts a combat
    /// request outside combat.
    pub fn notice_explosive_drop(&mut self, extras: &[ObjectRef]) -> Result<(), WorldError> {
        let player = self.world.player;
        let team = ThreatView::team(&self.world, player);
        let mut first = true;
        for critter in extras.iter().copied() {
            if critter == player
                || !self.world.critter_art(critter)
                || ThreatView::team(&self.world, critter) == team
            {
                continue;
            }
            let perception = self.world.effective(critter, 1);
            if stat_check(perception, 0, &mut self.world.rng)?.code < 2 {
                continue;
            }
            self.set_last_attacker(critter, player);
            if std::mem::take(&mut first) && !self.world.combat {
                self.world.pending_combat = Some(CombatRequest {
                    attacker: critter,
                    defender: player,
                    accuracy: 0,
                    damage: [0, i32::MAX],
                    flags: None,
                });
            }
        }
        Ok(())
    }
    /// §3.5 `danger(C)` over the live snapshot (sorted in place).
    pub fn danger(&mut self, critter: ObjectRef) -> Option<ObjectRef> {
        let mut snapshot = std::mem::take(&mut self.world.ai.snapshot);
        let found = ai::danger(&mut snapshot, critter, &self.world);
        self.world.ai.snapshot = snapshot;
        found
    }
    /// §3.15 team switching: store the team, then clear a last attacker now
    /// on that team. (The combat outline rebuild of step 4 is presentation
    /// with no approved encoding; outlines are not modeled.)
    pub fn set_team(&mut self, critter: ObjectRef, team: i32) {
        let Some(state) = self.world.critter_state_mut(critter) else {
            return;
        };
        state.team = team;
        if let Some(attacker) = self.world.last_attacker(critter)
            && ThreatView::team(&self.world, attacker) == team
        {
            self.set_last_attacker(critter, 0);
        }
    }
    /// §3.16 replacement defender for a random-hit critical failure: no draw
    /// for an empty snapshot; otherwise scan from `random(0, n - 1)`,
    /// wrapping, for an entry other than both parties in the attacker's
    /// forward sector whose non-aimed preflight passes.
    pub fn random_target(
        &self,
        attacker: ObjectRef,
        defender: ObjectRef,
        mode: u8,
        rng: &mut impl RandomSource,
    ) -> Result<Option<ObjectRef>, WorldError> {
        let snapshot = &self.world.ai.snapshot;
        if snapshot.is_empty() {
            return Ok(None);
        }
        let count = snapshot.len();
        let start = usize::try_from(rng.range(0, count as i32 - 1)?).unwrap_or(0);
        for offset in 0..count {
            let entry = snapshot[(start + offset) % count];
            if entry == attacker
                || entry == defender
                || !self.world.in_forward_sector(attacker, entry)
            {
                continue;
            }
            let Ok(attack) =
                self.world
                    .attack_profile(attacker, entry, mode, location::UNCALLED, false)
            else {
                continue;
            };
            if self.world.preflight(attacker, entry, &attack).is_ok() {
                return Ok(Some(entry));
            }
        }
        Ok(None)
    }
    /// SPEC-007 §3.7.1 extension hex E: the weapon's effective range counted
    /// along the pixel line from the attacker's hex center through the
    /// defender's (stopping at an outer-border hex); the attacker's hex for a
    /// range of at most 0 or a shared hex.
    fn extension_hex(&self, from: i32, through: i32, range: i32) -> i32 {
        if range <= 0 || from == through {
            return from;
        }
        crate::routing::extend_line(from, through, range as usize)
            .last()
            .copied()
            .unwrap_or(from)
    }
    /// §3.7.1 Stage A, the line-of-fire candidate: repeated stride-32 casts
    /// from the attacker toward E, ignoring the last critter met. A
    /// non-critter is an obstruction (no candidate). Each critter other than
    /// the defender (which is passed with no draw) is selected when a D100 is
    /// at most `trunc(H / 3)` of its range-aware hit chance H, or 5 when it
    /// is dead; a chance of 0 or less still draws.
    pub(crate) fn line_of_fire(
        &self,
        attacker: ObjectRef,
        defender: ObjectRef,
        attack: &Attack,
        extension: i32,
        rng: &mut impl RandomSource,
    ) -> Result<Option<ObjectRef>, WorldError> {
        let Some(mut from) = self.world.get(attacker).and_then(|o| o.raw.hex) else {
            return Ok(None);
        };
        let place = if attack.location == location::UNCALLED {
            location::TORSO
        } else {
            attack.location
        };
        let mut ignore = attacker;
        // Every cast ignores the last critter met; the bound only guards
        // against stacked critters reporting each other forever.
        for _ in 0..crate::routing::MAX_RAY_SAMPLES {
            if from == extension {
                break;
            }
            let Some(blocker) = self
                .world
                .ray(attacker, from, extension, 32, Some(ignore))
                .obstacle
            else {
                break;
            };
            if !self.world.critter_art(blocker) {
                break;
            }
            if blocker != defender {
                let chance = if self.world.is_dead(blocker) {
                    5
                } else {
                    self.world
                        .attack_profile(attacker, blocker, attack.hit_mode, place, attack.aimed)
                        .and_then(|profile| profile.hit_chance(true))
                        .map_or(0, |chance| chance / 3)
                };
                if rng.range(1, 100)? <= chance {
                    return Ok(Some(blocker));
                }
            }
            match self.world.get(blocker).and_then(|o| o.raw.hex) {
                Some(hex) => from = hex,
                None => break,
            }
            ignore = blocker;
        }
        Ok(None)
    }
    /// §3.7.1 Stage B, the impact fallback. The impact hex is E, or for a
    /// grenade (throw with explosion, plasma or EMP damage) the scatter hex
    /// around the defender. A stride-32 cast from the defender toward it,
    /// ignoring the defender, selects its blocker; otherwise the first
    /// movement blocker at the impact hex (with no ignored object, so possibly
    /// the defender itself). A projectile-through candidate is never hit.
    pub(crate) fn impact_candidate(
        &self,
        defender: ObjectRef,
        attack: &Attack,
        extension: i32,
        rng: &mut impl RandomSource,
    ) -> Result<Option<ObjectRef>, WorldError> {
        let Some((hex, elevation)) = self
            .world
            .get(defender)
            .and_then(|o| Some((o.raw.hex?, o.raw.elevation)))
        else {
            return Ok(None);
        };
        let grenade = attack.mode() == crate::combat::attack_mode::THROW
            && attack.weapon.as_ref().is_some_and(|weapon| {
                use crate::combat::damage_type::{EMP, EXPLOSION, PLASMA};
                matches!(weapon.damage_type, EXPLOSION | PLASMA | EMP)
            });
        let impact = if grenade {
            crate::combat::grenade_scatter(hex, attack.distance, rng)?
        } else {
            extension
        };
        let candidate = self
            .world
            .ray(defender, hex, impact, 32, Some(defender))
            .obstacle
            .filter(|blocker| *blocker != defender)
            .or_else(|| self.world.movement_blocker(impact, elevation, 0));
        Ok(candidate.filter(|object| {
            self.world
                .get(*object)
                .is_some_and(|o| o.raw.flags.0 & BasicFlags::PROJECTILE_THROUGH.0 == 0)
        }))
    }
    /// Resolution plus the SPEC-007 follow-ups: a random-target critical
    /// failure (SPEC-009 §3.16), or the §3.7.1 bystander resolution of a
    /// ranged or throw miss (single shots and failed sprays alike). Stage A
    /// runs for an ordinary failure; under its safe rule a selected critter
    /// is committed. Stage B runs when neither the hit nor the critical bit is
    /// set afterward. A critter victim takes one round at numerator 2 with no
    /// critical; a non-critter receives the hit bit and no damage or draw.
    pub(crate) fn resolve_attack(
        &self,
        attacker: ObjectRef,
        defender: ObjectRef,
        attack: &Attack,
        rng: &mut impl RandomSource,
    ) -> Result<(Spray, Option<Victim>), WorldError> {
        let spraying = matches!(
            attack.mode(),
            crate::combat::attack_mode::BURST | crate::combat::attack_mode::CONTINUOUS
        );
        let mut spray = if spraying {
            self.resolve_spray(attacker, defender, attack, rng)?
        } else {
            Spray {
                main: attack.resolve(rng)?,
                extras: Vec::new(),
            }
        };
        let ranged = matches!(
            attack.subtype(),
            Some(crate::combat::Subtype::Ranged | crate::combat::Subtype::Throwing)
        );
        let (victim, random) = if spray.main.retarget {
            (
                self.random_target(attacker, defender, attack.hit_mode, rng)?,
                true,
            )
        } else if ranged && spray.main.code <= 1 {
            let from = self.world.get(attacker).and_then(|o| o.raw.hex);
            let through = self.world.get(defender).and_then(|o| o.raw.hex);
            let (Some(from), Some(through)) = (from, through) else {
                return Ok((spray, None));
            };
            let extension = self.extension_hex(from, through, attack.range());
            let mut candidate = None;
            if spray.main.code == 1 {
                candidate = self.line_of_fire(attacker, defender, attack, extension, rng)?;
            }
            if candidate.is_none()
                && spray.main.attacker_flags & effect::CRITICAL == 0
                && spray.main.defender_flags & effect::HIT == 0
            {
                candidate = self.impact_candidate(defender, attack, extension, rng)?;
            }
            (candidate, false)
        } else {
            (None, false)
        };
        let Some(victim) = victim else {
            return Ok((spray, None));
        };
        let main = &mut spray.main;
        main.attacker_flags &= !effect::CRITICAL;
        main.defender_flags |= effect::HIT;
        // A wall or scenery takes the hit bit only: no damage, no draw.
        let profile = self
            .world
            .critter_art(victim)
            .then(|| {
                self.world
                    .attack_profile(
                        attacker,
                        victim,
                        attack.hit_mode,
                        location::TORSO,
                        attack.aimed,
                    )
                    .ok()
            })
            .flatten();
        let Some(mut profile) = profile else {
            main.damage = 0;
            return Ok((spray, Some(Victim::Object(victim))));
        };
        profile.weapon = attack.weapon.clone();
        main.multiplier = 2;
        main.rounds = 1;
        main.damage = profile.round_damage(2, false, false, rng)? as i32;
        profile.finish(main);
        Ok((
            spray,
            Some(if random {
                Victim::Random(victim, profile)
            } else {
                Victim::Bystander(victim, profile)
            }),
        ))
    }
    /// §3.17 attack-form request sites, after the hit location is final:
    /// the attacker's attack taunt, then the hit (survived) or miss taunt
    /// from the attacker for melee forms or the defender otherwise.
    pub(crate) fn attack_taunts(
        &mut self,
        attacker: ObjectRef,
        defender: ObjectRef,
        attack: &Attack,
        result: &Resolution,
    ) -> Result<(), WorldError> {
        self.request_taunt(attacker, Taunt::Attack)?;
        let speaker = if ai::melee_form(ai::attack_animation(attack.mode())) {
            attacker
        } else {
            defender
        };
        let place = if attack.location == location::UNCALLED {
            location::TORSO
        } else {
            attack.location
        };
        if result.defender_flags & effect::HIT == 0 {
            self.request_taunt(speaker, Taunt::Miss)?;
        } else if result.defender_flags & effect::DEAD == 0 {
            self.request_taunt(speaker, Taunt::Hit(place))?;
        }
        Ok(())
    }
    /// §3.17 taunt request: critter PID, taunts enabled, a living conscious
    /// non-player speaker, the chance draw, a nonempty inclusive range, the
    /// line draw, and a catalog line; the text goes to the type's buffer
    /// and a display callback joins the current sequence.
    pub fn request_taunt(&mut self, critter: ObjectRef, taunt: Taunt) -> Result<bool, WorldError> {
        let Some(object) = self.world.get(critter) else {
            return Ok(false);
        };
        if Category::from_pid(object.raw.pid).ok() != Some(Category::Critter)
            || self.world.ai.combat_taunts.unwrap_or(1) == 0
            || critter == self.world.player
            || self.world.is_dead(critter)
            || self.world.knocked_out(critter)
        {
            return Ok(false);
        }
        let Some(packet) = self.packet(critter) else {
            return Ok(false);
        };
        if self.world.rng.range(1, 100)? > packet.chance {
            return Ok(false);
        }
        let (start, end) = ai::taunt_range(&packet, taunt);
        if end < start {
            return Ok(false);
        }
        let id = self.world.rng.range(start, end)?;
        let Some(mut text) = self
            .world
            .ai
            .messages
            .as_ref()
            .and_then(|catalog| catalog.get(id))
            .map(|message| message.text.clone())
        else {
            return Ok(false);
        };
        text.truncate(ai::TAUNT_BYTES);
        let buffer = taunt.buffer();
        self.world.ai.taunts[buffer] = text;
        self.world.ai.callbacks.push(TauntCallback {
            speaker: critter,
            buffer,
            font: packet.font,
            color: packet.color,
            outline: packet.outline_color,
        });
        Ok(true)
    }
    /// Run the sequence's taunt callbacks in registration order. Each shows
    /// its buffer's text at callback time unless any floating text exists.
    pub fn run_taunts(&mut self) {
        for callback in std::mem::take(&mut self.world.ai.callbacks) {
            let now = self.world.elapsed_ms;
            self.world.floating.retain(|(_, expiry)| *expiry > now);
            let text = self.world.ai.taunts[callback.buffer].clone();
            if !self.world.floating.is_empty()
                || text.is_empty()
                || self.world.get(callback.speaker).is_none()
            {
                continue;
            }
            self.register_floating(callback.speaker, callback.font, &text);
            self.world.emit(WorldEffect::Taunt {
                object: callback.speaker,
                text,
                font: callback.font,
                color: callback.color,
                outline: callback.outline,
            });
        }
    }
    /// Track a new floating text over `object` for the floating-text
    /// subsystem's default lifetime (3500 ms plus 1399 ms per line, words
    /// wrapped at 200 pixels in `font`).
    pub(crate) fn register_floating(&mut self, object: ObjectRef, font: i32, text: &[u8]) {
        let lines = self
            .assets
            .font_number(font)
            .map_or(1, |font| ai::wrapped_lines(&font, text, ai::TAUNT_WIDTH));
        let now = self.world.elapsed_ms;
        self.world.floating.retain(|(_, expiry)| *expiry > now);
        self.world
            .floating
            .push((object, now + ai::floating_lifetime(lines)));
    }
    /// `<critter name> <text> <item name>.` from `misc.msg` into the
    /// message monitor, truncated to 199 bytes; nothing without the line.
    fn print_misc(&mut self, id: i32, critter: ObjectRef, item: ObjectRef) {
        let Some(text) = self
            .assets
            .game_messages("misc.msg")
            .ok()
            .and_then(|catalog| catalog.get(id).map(|message| message.text.clone()))
        else {
            return;
        };
        let name = |reference| {
            self.world
                .get(reference)
                .map_or(Vec::new(), |object| object.name.clone())
        };
        let mut line = [
            name(critter),
            b" ".to_vec(),
            text,
            b" ".to_vec(),
            name(item),
        ]
        .concat();
        line.push(b'.');
        line.truncate(ai::MESSAGE_BYTES);
        self.world.emit(WorldEffect::Message(line));
    }

    /// §3.6 NPC turn with an optional requested target. Returns the target.
    pub fn ai_turn(
        &mut self,
        critter: ObjectRef,
        requested: Option<ObjectRef>,
    ) -> Result<Option<ObjectRef>, WorldError> {
        let Some(packet) = self.packet(critter) else {
            self.world
                .diagnostic("combat AI packets are not loaded; the NPC turn is skipped");
            return Ok(requested);
        };
        let Some(state) = self.world.critter_state(critter) else {
            return Ok(requested);
        };
        let (maneuvers, results, hp) = (state.maneuvers, state.results.0, state.hp);
        if maneuvers & intent::FLEE != 0
            || results & packet.hurt_too_much != 0
            || hp < packet.min_hp
        {
            self.ai_flee(critter)?;
            return Ok(requested);
        }
        let target = match requested {
            None => {
                self.ai_heal(critter)?;
                let target = self.danger(critter);
                if let Some(target) = target {
                    self.ai_attack(critter, target)?;
                }
                target
            }
            Some(target) => {
                self.ai_attack(critter, target)?;
                Some(target)
            }
        };
        match target {
            Some(target) => {
                if !self.world.is_dead(target)
                    && self.world.action_points(critter) != 0
                    && self.world.object_distance(critter, target) > packet.max_dist
                {
                    self.world.add_intent(critter, intent::STOP);
                }
            }
            None if !self.world.party_member(critter) => {
                let hurt = self
                    .world
                    .critter_state(critter)
                    .is_some_and(|state| state.damage > 0);
                if self
                    .world
                    .last_attacker(critter)
                    .is_some_and(|attacker| !self.world.is_dead(attacker))
                    && hurt
                {
                    self.ai_flee(critter)?;
                }
            }
            None => {
                let player = self.world.player;
                if self.world.object_distance(critter, player) > 5 {
                    self.ai_approach(critter, player, false)?;
                }
            }
        }
        Ok(target)
    }
    /// Execute a planned movement for at most `budget` hexes.
    fn run_movement(&mut self, mut movement: Movement, budget: usize) -> Result<(), WorldError> {
        movement.directions.truncate(budget);
        let mut moved = 0;
        for _ in 0..budget.saturating_mul(4).saturating_add(4) {
            if moved >= budget || movement.finished() {
                break;
            }
            match self.step_movement(&mut movement)? {
                StepOutcome::Moved => moved += 1,
                StepOutcome::Replanned => movement.directions.truncate(budget - moved),
                StepOutcome::Finished | StepOutcome::Stopped => break,
            }
        }
        Ok(())
    }
    /// §3.8 flee, relative to the player: stop at `max_dist`; otherwise set
    /// fleeing and run (with a run taunt) to the first strict-goal reachable
    /// candidate D, D+1, D+5 at N = AP..1 steps.
    pub(crate) fn ai_flee(&mut self, critter: ObjectRef) -> Result<(), WorldError> {
        let Some(packet) = self.packet(critter) else {
            return Ok(());
        };
        let player = self.world.player;
        if self.world.object_distance(critter, player) >= packet.max_dist {
            self.world.add_intent(critter, intent::STOP);
            return Ok(());
        }
        self.world.add_intent(critter, intent::FLEE);
        let (Some(from), Some(origin)) = (
            self.world.get(critter).and_then(|object| object.raw.hex),
            self.world.get(player).and_then(|object| object.raw.hex),
        ) else {
            return Ok(());
        };
        let direction = rotation(origin, from).unwrap_or(0);
        let ap = self.world.action_points(critter);
        let goal = ai::flee_candidates(direction, ap)
            .into_iter()
            .find_map(|(steps, toward)| {
                let goal = HexGrid::step(from, toward, steps).ok()?;
                (!self.world.route(critter, from, goal, true).is_empty()).then_some(goal)
            });
        let Some(goal) = goal else {
            return Ok(());
        };
        self.world.ai.callbacks.clear();
        self.request_taunt(critter, Taunt::Run)?;
        self.emit_movement_sound(critter, true);
        let budget = usize::try_from(ap).unwrap_or(0);
        if let Some(movement) = self.plan_to_tile(critter, goal, budget) {
            self.run_movement(movement, budget)?;
        }
        self.run_taunts();
        Ok(())
    }
    /// §3.12 approach: fail at distance 1 or less; otherwise an optional move
    /// taunt and a walk toward the target limited to the current AP (a zero
    /// budget fails the sequence). Success does not require getting closer.
    pub(crate) fn ai_approach(
        &mut self,
        critter: ObjectRef,
        target: ObjectRef,
        taunt: bool,
    ) -> Result<bool, WorldError> {
        if self.world.object_distance(critter, target) <= 1 {
            return Ok(false);
        }
        self.world.ai.callbacks.clear();
        if taunt {
            self.request_taunt(critter, Taunt::Move)?;
        }
        let budget = usize::try_from(self.world.action_points(critter)).unwrap_or(0);
        let movement = if budget > 0 {
            self.plan_to_object(critter, target)
        } else {
            None
        };
        let Some(movement) = movement else {
            self.world.ai.callbacks.clear();
            return Ok(false);
        };
        self.emit_movement_sound(critter, false);
        self.run_movement(movement, budget)?;
        self.run_taunts();
        Ok(true)
    }
    /// §3.7 healing for a biped below half its maximum HP, consuming each
    /// unit through the SPEC-011 §3.7 drug-use contract.
    pub(crate) fn ai_heal(&mut self, critter: ObjectRef) -> Result<(), WorldError> {
        if self.world.body_type(critter) != 0 {
            return Ok(());
        }
        let half = self.world.effective(critter, 7) / 2;
        if self.world.hit_points(critter).unwrap_or(0) >= half {
            return Ok(());
        }
        ai::heal(
            &mut Healing {
                host: self,
                critter,
            },
            half,
        )
    }

    /// §3.9 usability of `weapon` in `mode`: arms, critter art for the
    /// attack, and the weapon skill against `min_to_hit`.
    pub(crate) fn usable(
        &self,
        critter: ObjectRef,
        weapon: ObjectRef,
        mode: u8,
        min_to_hit: i32,
    ) -> bool {
        let world = &self.world;
        let (Some(object), Some(gun), Some(prototype)) = (
            world.get(critter),
            world.weapon_data(weapon),
            world.get(weapon).and_then(|item| item.prototype.as_deref()),
        ) else {
            return false;
        };
        let nibble = world.mode_nibble(weapon, mode);
        let flags = world
            .critter_state(critter)
            .map_or(0, |state| state.results.0);
        ai::weapon_usable(
            flags,
            prototype.extended.contains(ExtendedFlags::TWO_HANDED),
            min_to_hit,
            || {
                let art = Fid(object.raw.art_id);
                let fid = self.assets.construct_fid(
                    1,
                    art.base(),
                    ai::attack_animation(nibble),
                    gun.animation as u8,
                    Fid::selector_for_orientation(object.raw.orientation),
                );
                self.assets.art_exists(fid)
            },
            || {
                let big_gun = prototype.extended.contains(ExtendedFlags::BIG_GUN);
                match (
                    weapon_skill(nibble, gun.damage_type, big_gun),
                    &object.character,
                ) {
                    (Some(index), Some(sheet)) => sheet.skill(index, world.context(critter)),
                    _ => i32::MIN,
                }
            },
        )
    }
    /// An attack-mode nibble's subtype; nibbles 9-15 are diagnosed as none.
    fn subtype(&mut self, nibble: u8) -> u8 {
        let (subtype, invalid) = ai::subtype_of(nibble);
        if invalid {
            self.world
                .diagnostic(format!("attack-mode nibble {nibble} has no subtype"));
        }
        subtype
    }
    /// §3.9 item cost: prototype base cost, plus for a loaded weapon
    /// `floor(rounds x ammo cost / pack quantity)` (0 for a zero pack).
    pub(crate) fn item_cost(&mut self, item: ObjectRef) -> i32 {
        let Some(PrototypeData::Item(data)) = self
            .world
            .get(item)
            .and_then(|object| object.prototype.as_deref())
            .map(|prototype| &prototype.data)
        else {
            return 0;
        };
        let base = data.cost;
        if !matches!(data.data, ItemData::Weapon(_)) {
            return base;
        }
        let (rounds, ammo_pid) = self.world.loaded(item);
        if rounds <= 0 {
            return base;
        }
        let ammo = u32::try_from(ammo_pid)
            .ok()
            .and_then(|pid| self.assets.prototype(pid).ok());
        let Some((cost, pack)) = ammo.as_deref().and_then(|prototype| match &prototype.data {
            PrototypeData::Item(Item {
                cost,
                data: ItemData::Ammunition { quantity, .. },
                ..
            }) => Some((*cost, *quantity)),
            _ => None,
        }) else {
            self.world.diagnostic(format!(
                "loaded ammunition prototype {ammo_pid:#x} is unavailable; cost counts 0"
            ));
            return base;
        };
        let (total, zero_pack) = ai::weapon_cost(base, rounds, cost, pack);
        if zero_pack {
            self.world.diagnostic(format!(
                "ammunition prototype {ammo_pid:#x} has a zero pack quantity"
            ));
        }
        total
    }
    /// The first top-level ammunition entry the weapon may reload from.
    pub(crate) fn compatible_ammo(
        &self,
        critter: ObjectRef,
        weapon: ObjectRef,
    ) -> Option<ObjectRef> {
        self.world.first_compatible_ammo(critter, weapon)
    }
    /// §3.9 inventory search (bipeds and robots), optionally with the raw
    /// primary AP check, folding weapon preference in inventory order.
    pub(crate) fn inventory_search(
        &mut self,
        critter: ObjectRef,
        ap_check: bool,
    ) -> Option<ObjectRef> {
        if !matches!(self.world.body_type(critter), 0 | 2) {
            return None;
        }
        let min_to_hit = self.packet(critter).map_or(0, |packet| packet.min_to_hit);
        let right = self.world.wielded(critter, hit_mode::RIGHT_PRIMARY);
        let items: Vec<_> = self
            .world
            .get(critter)?
            .inventory
            .iter()
            .map(|(item, _)| *item)
            .filter(|item| self.world.item_subtype(*item) == Some(ItemSubtype::Weapon))
            .collect();
        let mut entries = Vec::with_capacity(items.len());
        for item in &items {
            let nibble = self.world.mode_nibble(*item, hit_mode::RIGHT_PRIMARY);
            let primary_subtype = self.subtype(nibble);
            entries.push(InventoryWeapon {
                right_hand: right == Some(*item),
                raw_primary_ap: self
                    .world
                    .weapon_data(*item)
                    .map_or(0, |gun| gun.ap_cost[0]),
                usable_primary: self.usable(critter, *item, hit_mode::RIGHT_PRIMARY, min_to_hit),
                loaded: self.world.loaded(*item).0,
                ammo_available: self.compatible_ammo(critter, *item).is_some(),
                choice: WeaponChoice {
                    primary_subtype,
                    cost: self.item_cost(*item),
                },
            });
        }
        let ap = self.world.action_points(critter);
        ai::inventory_best(&entries, ap_check.then_some(ap)).map(|index| items[index])
    }
    /// §3.9 environment search (bipeds only): non-hidden item-art objects on
    /// the current elevation in hex order, stably sorted by distance, up to
    /// Perception + 5 away.
    pub(crate) fn environment_search(
        &mut self,
        critter: ObjectRef,
        kind: ItemSubtype,
    ) -> Option<ObjectRef> {
        if self.world.body_type(critter) != 0 {
            return None;
        }
        let min_to_hit = self.packet(critter).map_or(0, |packet| packet.min_to_hit);
        let world = &self.world;
        let mut items: Vec<_> = world
            .occupancy(world.elevation)
            .into_values()
            .flatten()
            .filter(|item| {
                world.get(*item).is_some_and(|object| {
                    Fid(object.raw.art_id).category() == 0
                        && !object.raw.flags.contains(BasicFlags::HIDDEN)
                })
            })
            .collect();
        items.sort_by_key(|item| world.object_distance(critter, *item));
        let limit = world.effective(critter, 1).saturating_add(5);
        let right = world.wielded(critter, hit_mode::RIGHT_PRIMARY);
        for item in items {
            if world.object_distance(critter, item) > limit {
                break;
            }
            if world.item_subtype(item) != Some(kind) {
                continue;
            }
            let found = if kind == ItemSubtype::Weapon {
                self.usable(critter, item, hit_mode::RIGHT_PRIMARY, min_to_hit)
            } else {
                right.is_some_and(|weapon| world.reload_permitted(weapon, item))
            };
            if found {
                return Some(item);
            }
        }
        None
    }
    /// §3.9 retrieval through the standard pick-up action: walk with the
    /// current-AP budget, require adjacency, pay 3 AP, run the item's
    /// pick-up script, and take the item; then look it up in inventory.
    pub(crate) fn retrieve(
        &mut self,
        critter: ObjectRef,
        item: ObjectRef,
    ) -> Result<Option<ObjectRef>, WorldError> {
        let budget = usize::try_from(self.world.action_points(critter)).unwrap_or(0);
        if budget > 0
            && let Some(movement) = self.plan_to_object(critter, item)
        {
            if !movement.directions.is_empty() {
                self.emit_movement_sound(critter, false);
            }
            self.run_movement(movement, budget)?;
        }
        if self.world.object_distance(critter, item) > 1 {
            return Ok(None);
        }
        let Some(state) = self.world.critter_state_mut(critter) else {
            return Ok(None);
        };
        if state.ap < 3 {
            return Ok(None);
        }
        state.ap -= 3;
        let overridden = match self.world.script_for(item) {
            Some(script) => {
                self.dispatch(crate::world::ScriptRequest {
                    script,
                    event: crate::ScriptEvent::Pickup,
                    source: critter,
                    target: item,
                    fixed: 0,
                    action: -1,
                    only_present: false,
                })?
                .overridden
            }
            None => false,
        };
        if !overridden {
            self.world.emit(WorldEffect::DefaultAction {
                action: crate::ObjectAction::Pickup,
                target: item,
            });
            if let Err(error) = self.world.add_item(critter, item, 1, false) {
                self.world.diagnostic(format!("AI pick-up: {error}"));
            }
            self.world.rebuild_lighting();
        }
        let Some(id) = self.world.get(item).map(|object| object.raw.id) else {
            return Ok(None);
        };
        Ok(self.world.get(critter).and_then(|object| {
            object
                .inventory
                .iter()
                .map(|(entry, _)| *entry)
                .find(|entry| {
                    self.world
                        .get(*entry)
                        .is_some_and(|found| found.raw.id == id)
                })
        }))
    }
    /// Wield in the right hand (no AP for NPCs): the art shows the weapon's
    /// animation code, and the take-out animation plays.
    fn wield(&mut self, critter: ObjectRef, weapon: ObjectRef) -> Result<(), WorldError> {
        self.world
            .equip(critter, weapon, BasicFlags::RIGHT_HAND.0)?;
        let code = self
            .world
            .weapon_data(weapon)
            .map_or(0, |gun| gun.animation);
        if let Some(item) = self.world.get_mut(weapon) {
            item.raw.flags.0 &= !BasicFlags::LEFT_HAND.0;
        }
        self.show_weapon(critter, code as u8);
        self.world.emit(WorldEffect::CritterAnimation {
            object: critter,
            animation: ai::TAKE_OUT,
        });
        Ok(())
    }
    /// Unwield the right hand; the put-away animation plays on success.
    fn unwield(&mut self, critter: ObjectRef) -> bool {
        let Some(item) = self.world.wielded(critter, hit_mode::RIGHT_PRIMARY) else {
            return false;
        };
        if let Some(object) = self.world.get_mut(item) {
            object.raw.flags.0 &= !BasicFlags::RIGHT_HAND.0;
        }
        self.show_weapon(critter, 0);
        self.world.emit(WorldEffect::CritterAnimation {
            object: critter,
            animation: ai::PUT_AWAY,
        });
        true
    }
    fn show_weapon(&mut self, critter: ObjectRef, code: u8) {
        if let Some(object) = self.world.get_mut(critter) {
            let art = Fid(object.raw.art_id);
            object.raw.art_id = Fid::new(
                art.category(),
                art.base(),
                art.animation(),
                code,
                art.selector(),
            )
            .0;
        }
    }
    /// The SPEC-007 non-aimed attack cost of a mode (unaffordable on error).
    fn attack_cost(&self, critter: ObjectRef, target: ObjectRef, mode: u8) -> i32 {
        self.world
            .attack_profile(critter, target, mode, location::UNCALLED, false)
            .map_or(i32::MAX, |attack| attack.ap_cost())
    }
    /// The SPEC-007 hit chance plus the AI's accuracy bonus.
    fn ai_hit_chance(
        &self,
        critter: ObjectRef,
        target: ObjectRef,
        mode: u8,
        place: u8,
        range_aware: bool,
    ) -> Result<i32, WorldError> {
        let mut attack = self
            .world
            .attack_profile(critter, target, mode, place, false)?;
        attack.accuracy_bonus = self.world.ai.accuracy;
        attack.hit_chance(range_aware)
    }
    /// §3.9 weapon switching: the inventory best (AP-checked), else a
    /// retrieved environment weapon, wielded with its selected mode; success
    /// requires the mode's non-aimed cost within the current AP. Outputs
    /// (no weapon and punch by default) replace the caller's even on failure.
    pub(crate) fn switch_weapon(
        &mut self,
        critter: ObjectRef,
        target: ObjectRef,
    ) -> Result<(Switched, Option<ObjectRef>), WorldError> {
        let mut weapon = self.inventory_search(critter, true);
        if weapon.is_none()
            && let Some(item) = self.environment_search(critter, ItemSubtype::Weapon)
        {
            weapon = self.retrieve(critter, item)?;
        }
        let Some(found) = weapon else {
            let none = Switched {
                success: false,
                weapon: false,
                mode: ai::PUNCH,
            };
            return Ok((none, None));
        };
        let mode = self.select_mode(critter, Some(found))?;
        self.wield(critter, found)?;
        let success = self.attack_cost(critter, target, mode) <= self.world.action_points(critter);
        let switched = Switched {
            success,
            weapon: true,
            mode,
        };
        Ok((switched, Some(found)))
    }
    /// §3.10 hit-mode selection for `weapon` (draws as specified).
    pub(crate) fn select_mode(
        &mut self,
        critter: ObjectRef,
        weapon: Option<ObjectRef>,
    ) -> Result<u8, WorldError> {
        let (frequency, min_to_hit) = self
            .packet(critter)
            .map_or((0, 0), |packet| (packet.secondary_freq, packet.min_to_hit));
        let weapon =
            weapon.filter(|item| self.world.item_subtype(*item) == Some(ItemSubtype::Weapon));
        let mode_weapon = match weapon {
            None => None,
            Some(item) => {
                let nibble = self.world.mode_nibble(item, hit_mode::RIGHT_SECONDARY);
                let secondary_subtype = self.subtype(nibble);
                Some(ModeWeapon {
                    secondary_subtype,
                    usable_secondary: secondary_subtype != ai::subtype::NONE
                        && self.usable(critter, item, hit_mode::RIGHT_SECONDARY, min_to_hit),
                })
            }
        };
        let intelligence = self.world.effective(critter, 4);
        let mut rng = std::mem::take(&mut self.world.rng);
        let mode = ai::select_hit_mode(mode_weapon, frequency, intelligence, &mut rng, || {
            self.inventory_search(critter, false).is_some()
        });
        self.world.rng = rng;
        mode
    }
    /// §3.11 called-shot location for an attack in `mode`.
    pub(crate) fn select_location(
        &mut self,
        critter: ObjectRef,
        target: ObjectRef,
        mode: u8,
    ) -> Result<u8, WorldError> {
        let Some(packet) = self.packet(critter) else {
            return Ok(location::TORSO);
        };
        let Ok(aimed) = self
            .world
            .attack_profile(critter, target, mode, location::HEAD, true)
        else {
            return Ok(location::TORSO);
        };
        let inputs = CalledShot {
            aimed_affordable: aimed.ap_cost() <= self.world.action_points(critter),
            aiming_available: aimed.aimed_shot_available(),
            called_freq: packet.called_freq,
            difficulty: self.world.combat_difficulty.unwrap_or(1),
            intelligence: self.world.effective(critter, 4),
            min_to_hit: packet.min_to_hit,
        };
        let mut rng = std::mem::take(&mut self.world.rng);
        let place = ai::select_location(inputs, &mut rng, |place| {
            Ok(self
                .ai_hit_chance(critter, target, mode, place, true)
                .unwrap_or(i32::MIN))
        });
        self.world.rng = rng;
        place
    }
    /// §3.12 attack loop entry: reference the target, select the mode for
    /// the right-hand weapon, upgrade from unarmed when warranted, and run
    /// the bounded loop.
    pub(crate) fn ai_attack(
        &mut self,
        critter: ObjectRef,
        target: ObjectRef,
    ) -> Result<bool, WorldError> {
        self.set_last_attacker(critter, target);
        let mut weapon = self
            .world
            .wielded(critter, hit_mode::RIGHT_PRIMARY)
            .filter(|item| self.world.item_subtype(*item) == Some(ItemSubtype::Weapon));
        let mut mode = self.select_mode(critter, weapon)?;
        if weapon.is_none() {
            let armed = self
                .world
                .get(target)
                .is_some_and(|object| Fid(object.raw.art_id).weapon() != 0);
            let punch_art = self.world.get(critter).is_some_and(|object| {
                let art = Fid(object.raw.art_id);
                let fid = self.assets.construct_fid(
                    1,
                    art.base(),
                    16,
                    0,
                    Fid::selector_for_orientation(object.raw.orientation),
                );
                self.assets.art_exists(fid)
            });
            if self.world.body_type(target) != 0 || armed || !punch_art {
                let (switched, found) = self.switch_weapon(critter, target)?;
                weapon = found;
                mode = switched.mode;
            }
        }
        let min_to_hit = self.packet(critter).map_or(0, |packet| packet.min_to_hit);
        ai::attack_loop(
            &mut Engagement {
                host: self,
                critter,
                target,
                weapon,
            },
            mode,
            min_to_hit,
        )
    }
    /// §3.12 attack: refused while fleeing; turn toward the target, choose
    /// the location, and request the SPEC-007 attack.
    fn ai_strike(
        &mut self,
        critter: ObjectRef,
        target: ObjectRef,
        mode: u8,
    ) -> Result<bool, WorldError> {
        let fleeing = self
            .world
            .critter_state(critter)
            .is_some_and(|state| state.maneuvers & intent::FLEE != 0);
        if fleeing {
            return Ok(false);
        }
        if let (Some(from), Some(to)) = (
            self.world.get(critter).and_then(|object| object.raw.hex),
            self.world.get(target).and_then(|object| object.raw.hex),
        ) && let Some(direction) = rotation(from, to)
            && let Some(object) = self.world.get_mut(critter)
        {
            object.raw.orientation = direction;
        }
        let place = self.select_location(critter, target, mode)?;
        let aimed = !matches!(place, location::TORSO | location::UNCALLED);
        match self.perform_attack(critter, target, mode, place, aimed) {
            Ok(Ok(_)) => Ok(true),
            Ok(Err(_)) => Ok(false),
            Err(error) => {
                self.world.diagnostic(format!("AI attack: {error}"));
                Ok(false)
            }
        }
    }
    /// §3.13 reloading on preflight 1: inventory ammunition, else retrieved
    /// environment ammunition, else the out-of-ammo path (sound, hands,
    /// message 5001, unwield, then weapon switching whose outputs return).
    pub(crate) fn ai_reload(
        &mut self,
        critter: ObjectRef,
        target: ObjectRef,
        weapon: Option<ObjectRef>,
    ) -> Result<Option<(Switched, Option<ObjectRef>)>, WorldError> {
        let Some(weapon) = weapon else {
            return Ok(None);
        };
        if let Some(ammo) = self.compatible_ammo(critter, weapon) {
            self.reload_with(critter, weapon, ammo)?;
            return Ok(None);
        }
        if let Some(item) = self.environment_search(critter, ItemSubtype::Ammunition) {
            if let Some(ammo) = self.retrieve(critter, item)? {
                self.reload_with(critter, weapon, ammo)?;
            }
            return Ok(None);
        }
        self.emit_weapon_sound(critter, weapon, ai::OUT_OF_AMMO_SOUND);
        self.world.emit(WorldEffect::CritterAnimation {
            object: critter,
            animation: ai::HANDS,
        });
        self.print_misc(5001, critter, weapon);
        self.unwield(critter);
        Ok(Some(self.switch_weapon(critter, target)?))
    }
    /// The standard reload rule (SPEC-011 §3.15), as on the world state.
    pub fn reload_weapon(&mut self, weapon: ObjectRef, ammo: ObjectRef) -> i32 {
        self.world.reload_weapon(weapon, ammo)
    }
    /// §3.13 reload by the AI: an emptied ammunition unit is destroyed (the
    /// rest of a stack keeps full per-unit rounds); a permitted reload plays
    /// the ready sound and hands animation, prints 5002, and costs 2 AP with
    /// no prior AP check, saturating at 0.
    fn reload_with(
        &mut self,
        critter: ObjectRef,
        weapon: ObjectRef,
        ammo: ObjectRef,
    ) -> Result<(), WorldError> {
        let result = self.reload_weapon(weapon, ammo);
        if result == 0 {
            self.destroy_unit(ammo)?;
        }
        if result != -1 {
            self.emit_weapon_sound(critter, weapon, ai::READY_SOUND);
            self.world.emit(WorldEffect::CritterAnimation {
                object: critter,
                animation: ai::HANDS,
            });
            self.print_misc(5002, critter, weapon);
            if let Some(state) = self.world.critter_state_mut(critter) {
                state.ap = (state.ap - 2).max(0);
            }
        }
        Ok(())
    }
    /// Destroy one unit of an inventory stack; the remaining units keep a
    /// full per-unit round count (SPEC-011 §3.13 removal).
    fn destroy_unit(&mut self, item: ObjectRef) -> Result<(), WorldError> {
        self.world.destroy_unit(item)
    }
}

/// The healing loop's view of one critter's inventory.
struct Healing<'a, A: WorldAssets> {
    host: &'a mut WorldHost<A>,
    critter: ObjectRef,
}
impl<A: WorldAssets> HealingInventory for Healing<'_, A> {
    type Unit = ObjectRef;
    fn hp(&self) -> i32 {
        self.host.world.hit_points(self.critter).unwrap_or(0)
    }
    fn ap(&self) -> i32 {
        self.host.world.action_points(self.critter)
    }
    fn set_ap(&mut self, ap: i32) {
        if let Some(state) = self.host.world.critter_state_mut(self.critter) {
            state.ap = ap;
        }
    }
    fn entry(&self, position: usize) -> Option<Option<u32>> {
        let world = &self.host.world;
        let (item, _) = *world.get(self.critter)?.inventory.get(position)?;
        Some(
            (world.item_subtype(item) == Some(ItemSubtype::Drug))
                .then(|| world.get(item).map_or(0, |object| object.raw.pid)),
        )
    }
    fn take_one(&mut self, position: usize) -> Option<ObjectRef> {
        let world = &mut self.host.world;
        let (item, _) = *world.get(self.critter)?.inventory.get(position)?;
        match world.remove_item(self.critter, item, 1) {
            Ok(()) => Some(item),
            Err(error) => {
                world.diagnostic(format!("AI healing: {error}"));
                None
            }
        }
    }
    fn consume(&mut self, unit: &ObjectRef) -> Result<bool, WorldError> {
        Ok(self.host.consume_drug(self.critter, *unit)? == 1)
    }
    fn restore(&mut self, unit: ObjectRef) -> Result<(), WorldError> {
        self.host.world.add_item(self.critter, unit, 1, true)
    }
    fn used(&mut self, unit: ObjectRef) -> Result<(), WorldError> {
        self.host.world.emit(WorldEffect::CritterAnimation {
            object: self.critter,
            animation: ai::HANDS,
        });
        self.host.print_misc(5000, self.critter, unit);
        self.host.world.erase(unit);
        Ok(())
    }
}

/// One bounded attack loop's live operations against one target.
struct Engagement<'a, A: WorldAssets> {
    host: &'a mut WorldHost<A>,
    critter: ObjectRef,
    target: ObjectRef,
    /// The loop's current weapon (replaced by weapon switching).
    weapon: Option<ObjectRef>,
}
impl<A: WorldAssets> AttackEnv for Engagement<'_, A> {
    fn disabled(&self) -> bool {
        self.host
            .world
            .critter_state(self.critter)
            .is_some_and(|state| {
                state.results.0 & (effect::KNOCKED_OUT | effect::DEAD | effect::LOSE_TURN) != 0
            })
    }
    fn preflight(&mut self, mode: u8) -> Result<u8, WorldError> {
        let world = &self.host.world;
        let Ok(attack) =
            world.attack_profile(self.critter, self.target, mode, location::UNCALLED, false)
        else {
            return Ok(4);
        };
        Ok(world
            .preflight(self.critter, self.target, &attack)
            .map_or_else(refusal_code, |()| 0))
    }
    fn hit_chance(&mut self, mode: u8, range_aware: bool) -> Result<i32, WorldError> {
        let chance = self.host.ai_hit_chance(
            self.critter,
            self.target,
            mode,
            location::UNCALLED,
            range_aware,
        );
        Ok(chance.unwrap_or_else(|error| {
            self.host
                .world
                .diagnostic(format!("AI hit chance: {error}"));
            i32::MIN
        }))
    }
    fn attack(&mut self, mode: u8) -> Result<bool, WorldError> {
        self.host.ai_strike(self.critter, self.target, mode)
    }
    fn attack_cost(&mut self, mode: u8) -> Result<i32, WorldError> {
        Ok(self.host.attack_cost(self.critter, self.target, mode))
    }
    fn ap(&self) -> i32 {
        self.host.world.action_points(self.critter)
    }
    fn approach(&mut self, taunt: bool) -> Result<bool, WorldError> {
        self.host.ai_approach(self.critter, self.target, taunt)
    }
    fn flee(&mut self) -> Result<(), WorldError> {
        self.host.ai_flee(self.critter)
    }
    fn has_weapon(&self) -> bool {
        self.weapon.is_some()
    }
    fn switch_weapon(&mut self) -> Result<Switched, WorldError> {
        let (switched, weapon) = self.host.switch_weapon(self.critter, self.target)?;
        self.weapon = weapon;
        Ok(switched)
    }
    fn reload(&mut self) -> Result<Option<Switched>, WorldError> {
        let switched = self
            .host
            .ai_reload(self.critter, self.target, self.weapon)?;
        Ok(switched.map(|(switched, weapon)| {
            self.weapon = weapon;
            switched
        }))
    }
}

/// The object an attack lands on in place of, or as, its resolved target.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Victim {
    /// A random-hit critical failure's replacement defender (SPEC-009
    /// §3.16), treated as the unintended victim.
    Random(ObjectRef, Attack),
    /// A missed shot's critter victim (SPEC-007 §3.7.1), possibly the
    /// intended defender itself.
    Bystander(ObjectRef, Attack),
    /// A missed shot's non-critter impact: the hit bit, no damage.
    Object(ObjectRef),
}
