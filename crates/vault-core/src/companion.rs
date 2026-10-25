//! The `enhanced` preset's party conveniences: pushing a critter out of
//! the way, and each companion's combat distance, saved beside the slot
//! in [`SIDECAR`]. Engine extension, not part of any specification; under
//! Vanilla nothing here is reachable, and `SAVE.DAT` never changes.
use crate::{
    EnginePreset, ObjectRef, WorldError, WorldHost, assets::WorldAssets, critical::effect,
};
use std::collections::BTreeMap;
use vault_formats::HexGrid;

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
/// naming it.
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
            section = if name.trim() == "companion_distance" {
                "companion_distance"
            } else {
                return Err(at(format!("unknown table `[{name}]`")));
            };
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
    /// Whether `critter` may be pushed: the enhanced preset, out of
    /// combat, a placed critter other than the player on the player's
    /// elevation that is neither dead nor knocked out, and a party member
    /// unless `any` (a `--debug` session) lifts that, so shopkeepers and
    /// guards stay where their scripts put them.
    pub fn can_push(&self, critter: ObjectRef, any: bool) -> bool {
        let world = &self.world;
        self.party_tools()
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
        self.party_tools()
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
        let world = &self.world;
        let (Some(from), Some(player)) = (
            world.get(critter).and_then(|object| object.raw.hex),
            world.get(world.player).and_then(|object| object.raw.hex),
        ) else {
            return Ok(None);
        };
        let elevation = world.get(critter).map_or(0, |object| object.raw.elevation);
        let away = crate::routing::rotation(player, from).unwrap_or(0);
        for direction in [away, (away + 1) % 6, (away + 5) % 6] {
            let Ok(Some(hex)) = HexGrid::neighbor(from, direction) else {
                continue;
            };
            if self
                .world
                .movement_blocker(hex, elevation, critter)
                .is_some()
            {
                continue;
            }
            if self.walk_to(critter, hex)? {
                return Ok(Some(Push::Moved(hex)));
            }
        }
        Ok(Some(Push::Blocked))
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
        if self.party_tools() && critter != self.world.player && self.world.party_member(critter) {
            self.companion_distance(critter)
        } else {
            Distance::Default
        }
    }
}

#[cfg(test)]
#[path = "companion_tests.rs"]
mod tests;
