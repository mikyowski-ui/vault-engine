//! The `enhanced` preset's party conveniences: pushing a critter out of
//! the way, each companion's combat distance, saved beside the slot in
//! [`SIDECAR`], the WIELD button of the Gear and party Trade windows, and
//! the HUD's party status pills. Engine extension, not part of any
//! specification; under Vanilla nothing here is reachable, and `SAVE.DAT`
//! never changes.
use crate::{
    EnginePreset, ObjectRef, WorldError, WorldHost,
    assets::WorldAssets,
    critical::{effect, hit_mode},
    fallout2_party::distance as distance2,
};
use std::collections::BTreeMap;
use vault_formats::{
    Fid,
    fallout2::party::ControlField,
    pro::{BasicFlags, Category, ItemData},
};

/// The enhanced state file in a save slot, beside `SAVE.DAT`.
pub const SIDECAR: &str = "VAULTENG.TOML";

/// A companion's combat distance: how its AI packet's `max_dist` is read
/// (SPEC-009 §3.6 step 4, the pursuit stop, and §3.8 step 1, the flee
/// stop).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Distance {
    /// The packet's own value.
    #[default]
    Default,
    /// 3 hexes for both.
    Close,
    /// 1 hex for both: the companion holds its ground.
    Hold,
    /// No pursuit stop; fleeing keeps the packet's value.
    Charge,
}

impl Distance {
    pub const ALL: [Self; 4] = [Self::Default, Self::Close, Self::Hold, Self::Charge];
    pub fn name(self) -> &'static str {
        match self {
            Self::Default => "default",
            Self::Close => "close",
            Self::Hold => "hold",
            Self::Charge => "charge",
        }
    }
    pub fn parse(text: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|mode| mode.name() == text)
    }
    /// The HUD pill's three-letter label.
    pub fn label(self) -> &'static str {
        match self {
            Self::Default => "DEF",
            Self::Close => "CLS",
            Self::Hold => "HLD",
            Self::Charge => "CHG",
        }
    }
    /// The next mode in the window's cycle.
    pub fn next(self) -> Self {
        let index = Self::ALL.iter().position(|mode| *mode == self).unwrap_or(0);
        Self::ALL[(index + 1) % Self::ALL.len()]
    }
    /// The §3.6 step 4 limit for a packet's `max_dist`.
    pub fn pursuit_limit(self, max_dist: i32) -> i32 {
        match self {
            Self::Default => max_dist,
            Self::Close => 3,
            Self::Hold => 1,
            Self::Charge => i32::MAX,
        }
    }
    /// The §3.8 step 1 limit for a packet's `max_dist`.
    pub fn flee_limit(self, max_dist: i32) -> i32 {
        match self {
            Self::Default | Self::Charge => max_dist,
            Self::Close => 3,
            Self::Hold => 1,
        }
    }
}

/// The sidecar's text for the companions' distances (by object ID; the
/// default is not written).
pub fn write_sidecar(distances: &BTreeMap<u32, Distance>) -> String {
    let mut text = String::from(
        "# Vault Engine enhanced state (SAVE.DAT is unchanged).\n[companion_distance]\n",
    );
    for (id, mode) in distances {
        if *mode != Distance::Default {
            text += &format!("{id} = \"{}\"\n", mode.name());
        }
    }
    text
}

/// The distances a sidecar's text records; a malformed line is an error
/// naming it. The doors' table is [`crate::doors::parse_table`]'s.
pub fn parse_sidecar(text: &str) -> Result<BTreeMap<u32, Distance>, String> {
    let mut distances = BTreeMap::new();
    let mut section = "";
    for (index, line) in text.lines().enumerate() {
        let at = |message: String| format!("{SIDECAR} line {}: {message}", index + 1);
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        if let Some(name) = line
            .strip_prefix('[')
            .and_then(|line| line.strip_suffix(']'))
        {
            section = match name.trim() {
                "companion_distance" => "companion_distance",
                crate::doors::TABLE => crate::doors::TABLE,
                _ => return Err(at(format!("unknown table `[{name}]`"))),
            };
            continue;
        }
        if section == crate::doors::TABLE {
            continue;
        }
        if section.is_empty() {
            return Err(at("a key before `[companion_distance]`".into()));
        }
        let (id, mode) = line
            .split_once('=')
            .ok_or_else(|| at("expected `<object id> = \"<mode>\"`".into()))?;
        let id: u32 = id
            .trim()
            .parse()
            .map_err(|_| at(format!("`{}` is not an object id", id.trim())))?;
        let mode = mode.trim().trim_matches('"');
        let mode = Distance::parse(mode).ok_or_else(|| at(format!("unknown distance `{mode}`")))?;
        distances.insert(id, mode);
    }
    Ok(distances)
}

/// The HUD pill's label for a SPEC-019 §2.5 `distance` value: `on_your_own`
/// reads as the Fallout 1 Default (§3.18), and `snipe`, which has no
/// Fallout 1 equivalent, as SNP. A value outside the vocabulary has none.
pub fn distance_label2(value: i32) -> Option<&'static str> {
    Some(match value {
        distance2::ON_YOUR_OWN => "DEF",
        distance2::STAY_CLOSE => "CLS",
        distance2::STAY => "HLD",
        distance2::CHARGE => "CHG",
        distance2::SNIPE => "SNP",
        _ => return None,
    })
}

/// The WIELD cycle's next weapon: the one after `current` in `weapons`
/// (inventory order), "none" after the last, and the first from "none" or
/// from a held item the list lacks.
pub fn next_weapon(weapons: &[ObjectRef], current: Option<ObjectRef>) -> Option<ObjectRef> {
    match current.and_then(|held| weapons.iter().position(|weapon| *weapon == held)) {
        Some(index) => weapons.get(index + 1).copied(),
        None => weapons.first().copied(),
    }
}

/// One HUD status pill: an active companion's name, hit points and
/// distance label (`None` for a Fallout 2 member without a control
/// record, which has no distance to show or cycle).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PartyStatus {
    pub critter: ObjectRef,
    pub name: Vec<u8>,
    pub hp: i32,
    pub max_hp: i32,
    pub mode: Option<&'static str>,
}

/// What a push did.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Push {
    /// The critter walks to this hex.
    Moved(i32),
    /// No free hex away from the player.
    Blocked,
}

impl<A: WorldAssets> WorldHost<A> {
    /// Whether the enhanced preset's party conveniences apply.
    fn party_tools(&self) -> bool {
        self.preset() == EnginePreset::Enhanced
    }
    /// SPEC-019 §3.18: under the Fallout 2 profile the enhanced Push, Gear
    /// and Distance are not offered or read; Fallout 2's own party control
    /// takes their place.
    fn companion_tools(&self) -> bool {
        self.party_tools() && self.world.fallout2.is_none()
    }
    /// Whether `critter` may be pushed: the enhanced preset, out of
    /// combat, a placed critter other than the player on the player's
    /// elevation that is neither dead nor knocked out, and a party member
    /// unless `any` (a `--debug` session) lifts that, so shopkeepers and
    /// guards stay where their scripts put them.
    pub fn can_push(&self, critter: ObjectRef, any: bool) -> bool {
        let world = &self.world;
        self.companion_tools()
            && !world.combat
            && critter != world.player
            && (any || world.party_member(critter))
            && world.get(critter).is_some_and(|object| {
                object.category() == vault_formats::pro::Category::Critter
                    && object.raw.hex.is_some()
                    && world
                        .get(world.player)
                        .is_some_and(|player| player.raw.elevation == object.raw.elevation)
            })
            && world
                .critter_state(critter)
                .is_some_and(|state| state.results.0 & (effect::DEAD | effect::KNOCKED_OUT) == 0)
    }
    /// Whether `critter` is a companion whose gear and distance the
    /// enhanced window offers: a party member other than the player, out of
    /// combat.
    pub fn is_companion(&self, critter: ObjectRef) -> bool {
        self.companion_tools()
            && !self.world.combat
            && critter != self.world.player
            && self.world.party_member(critter)
    }
    /// Push `critter` one hex directly away from the player: the hex in the
    /// direction from the player's hex to its own (the SPEC-003 §3.5
    /// direction), else the two neighboring directions, as SPEC-009 §3.8
    /// tries a flight's candidates. The first free hex is walked to through
    /// the ordinary walk sequence. `None` when it may not be pushed
    /// ([`Self::can_push`], with `any` as there).
    pub fn push_critter(
        &mut self,
        critter: ObjectRef,
        any: bool,
    ) -> Result<Option<Push>, WorldError> {
        if !self.can_push(critter, any) {
            return Ok(None);
        }
        if self.world.get(critter).and_then(|o| o.raw.hex).is_none()
            || self
                .world
                .get(self.world.player)
                .and_then(|o| o.raw.hex)
                .is_none()
        {
            return Ok(None);
        }
        Ok(Some(match self.push_step(critter)? {
            Some(hex) => Push::Moved(hex),
            None => Push::Blocked,
        }))
    }
    /// A companion's combat distance.
    pub fn companion_distance(&self, critter: ObjectRef) -> Distance {
        self.world
            .get(critter)
            .and_then(|object| self.world.companion_distance.get(&object.raw.id))
            .copied()
            .unwrap_or_default()
    }
    /// Cycle a companion's combat distance; returns the new one. Nothing
    /// changes for anyone else.
    pub fn cycle_companion_distance(&mut self, critter: ObjectRef) -> Option<Distance> {
        if !self.is_companion(critter) {
            return None;
        }
        let id = self.world.get(critter)?.raw.id;
        let next = self.companion_distance(critter).next();
        if next == Distance::Default {
            self.world.companion_distance.remove(&id);
        } else {
            self.world.companion_distance.insert(id, next);
        }
        Some(next)
    }
    /// The distance mode the AI reads for `critter`: its own under the
    /// enhanced preset when it is a party member, else the default.
    pub(crate) fn ai_distance(&self, critter: ObjectRef) -> Distance {
        if self.companion_tools()
            && critter != self.world.player
            && self.world.party_member(critter)
        {
            self.companion_distance(critter)
        } else {
            Distance::Default
        }
    }

    /// Whether the WIELD button is offered on `critter`, a critter, under
    /// the enhanced preset: the Gear window's companion
    /// ([`Self::is_companion`]) under Fallout 1, or under Fallout 2, out of
    /// combat, a party member with a control record, as SPEC-019 §3.7's
    /// Trade window takes it.
    pub fn can_wield_for(&self, critter: ObjectRef) -> bool {
        let is_critter = self
            .world
            .get(critter)
            .is_some_and(|object| object.category() == Category::Critter);
        is_critter
            && if self.world.fallout2.is_some() {
                self.party_tools() && !self.world.combat && self.control_of(critter).is_some()
            } else {
                self.is_companion(critter)
            }
    }
    /// The weapons WIELD cycles through: `critter`'s weapon entries, in
    /// inventory order.
    pub fn companion_weapons(&self, critter: ObjectRef) -> Vec<ObjectRef> {
        self.world.get(critter).map_or(Vec::new(), |object| {
            object
                .inventory
                .iter()
                .map(|(item, _)| *item)
                .filter(|item| {
                    self.world
                        .item_data(*item)
                        .is_some_and(|data| matches!(data.data, ItemData::Weapon(_)))
                })
                .collect()
        })
    }
    /// The held (primary) weapon WIELD names: the right hand's item.
    pub fn companion_weapon(&self, critter: ObjectRef) -> Option<ObjectRef> {
        self.world.wielded(critter, hit_mode::RIGHT_PRIMARY)
    }
    /// One WIELD press: the next weapon of [`next_weapon`] in the right
    /// hand ([`Self::wield_companion`]). Returns the weapon now held, or
    /// `None` when the button is not offered ([`Self::can_wield_for`]).
    pub fn cycle_companion_weapon(
        &mut self,
        critter: ObjectRef,
    ) -> Result<Option<Option<ObjectRef>>, WorldError> {
        if !self.can_wield_for(critter) {
            return Ok(None);
        }
        let next = next_weapon(
            &self.companion_weapons(critter),
            self.companion_weapon(critter),
        );
        self.wield_companion(critter, next)?;
        Ok(Some(next))
    }
    /// Hold `weapon` in `critter`'s right hand, or nothing, through the
    /// script words' paths (SPEC-005): `wield_obj_critter` (one unit of a
    /// stack takes the hand) or `inven_unwield` (the right-hand flag
    /// cleared, the entry left in its place). The critter's art then takes
    /// the weapon's animation code (SPEC-007 §2.1) as
    /// [`Self::held_weapon_code`] allows, at once and without an animation,
    /// as the inventory window's figure art is applied (SPEC-005 §3.8).
    pub(crate) fn wield_companion(
        &mut self,
        critter: ObjectRef,
        weapon: Option<ObjectRef>,
    ) -> Result<(), WorldError> {
        let code = match weapon {
            Some(weapon) => {
                self.wield_object(critter, weapon)?;
                self.world
                    .weapon_data(weapon)
                    .and_then(|gun| u8::try_from(gun.animation).ok())
                    .unwrap_or(0)
            }
            None => {
                let inventory = self
                    .world
                    .get(critter)
                    .map(|object| object.inventory.clone())
                    .unwrap_or_default();
                for (item, _) in inventory {
                    if let Some(object) = self.world.get_mut(item) {
                        object.raw.flags.0 &= !BasicFlags::RIGHT_HAND.0;
                    }
                }
                0
            }
        };
        let code = self.held_weapon_code(critter, code);
        if self.world.critter_art(critter)
            && let Some(object) = self.world.get_mut(critter)
        {
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
        Ok(())
    }
    /// The weapon code `critter`'s art takes for a held weapon of `code`:
    /// the code itself when the critter-art database has the critter's
    /// standing art with it (SPEC-007 §2.1-§2.2, at its facing), else 0, so
    /// the weapon is held but drawn unarmed.
    pub fn held_weapon_code(&self, critter: ObjectRef, code: u8) -> u8 {
        if code == 0 || code > 15 {
            return 0;
        }
        let facing = self
            .world
            .get(critter)
            .map_or(0, |object| object.raw.orientation);
        let fid = self.critter_fid(critter, 0, Some(code), facing);
        if self.assets.art_exists(fid) { code } else { 0 }
    }

    /// The HUD's status pills, in party order: every party member but the
    /// player that is a placed, visible critter and not dead (a Fallout 1
    /// companion as [`Self::is_companion`] counts it, without the combat
    /// test). A non-critter entry such as the Fallout 2 car trunk has none.
    /// Empty without the enhanced preset.
    pub fn party_status(&self) -> Vec<PartyStatus> {
        if !self.party_tools() {
            return Vec::new();
        }
        let world = &self.world;
        let fallout2 = world.fallout2.is_some();
        world
            .party_members()
            .into_iter()
            .filter(|member| *member != world.player)
            .filter_map(|critter| {
                let object = world.get(critter)?;
                let active = object.category() == Category::Critter
                    && object.raw.hex.is_some()
                    && !object.raw.flags.contains(BasicFlags::HIDDEN)
                    && world
                        .critter_state(critter)
                        .is_some_and(|state| state.results.0 & effect::DEAD == 0)
                    && (fallout2 || world.party_member(critter));
                let mode = if fallout2 {
                    self.control_of(critter).and_then(|record| {
                        distance_label2(record.fields[ControlField::Distance as usize])
                    })
                } else {
                    Some(self.companion_distance(critter).label())
                };
                active.then(|| PartyStatus {
                    critter,
                    name: object.name.clone(),
                    hp: world.hit_points(critter).unwrap_or(0),
                    max_hp: world.effective(critter, 7),
                    mode,
                })
            })
            .collect()
    }
    /// A pill's mode region, out of combat: under Fallout 1 the enhanced
    /// Distance cycles ([`Self::cycle_companion_distance`]); under Fallout 2
    /// the control record's `distance` takes the next value of the member's
    /// allowed set, as a SPEC-019 §3.15.3 custom-panel row does, which makes
    /// the disposition `custom`. Returns whether it changed.
    pub fn cycle_party_distance(&mut self, critter: ObjectRef) -> bool {
        if !self.party_tools() || self.world.combat {
            return false;
        }
        if self.world.fallout2.is_some() {
            self.cycle_control(critter, ControlField::Distance)
        } else {
            self.cycle_companion_distance(critter).is_some()
        }
    }
}

#[cfg(test)]
#[path = "companion_tests.rs"]
mod tests;
