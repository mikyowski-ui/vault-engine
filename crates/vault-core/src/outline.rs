//! SPEC-014 §3.44 the outline word's operations, and the target outlines
//! combat and the crosshair cursor set. The word's bits are in
//! [`vault_formats::map::outline`]; drawing is in `vault_render::outline`.
use crate::{ObjectRef, WorldState, stats::Perk};
use vault_formats::{map::outline, pro::BasicFlags};

impl WorldState {
    /// The object's outline word (0 for none or no object).
    pub fn outline_word(&self, object: ObjectRef) -> u32 {
        self.get(object).map_or(0, |found| found.raw.outline)
    }
    /// Whether the object is outlined now: a type is set and it is not
    /// disabled. (SPEC-010 §3.11 widens such an object's on-screen
    /// rectangle by 1 pixel on every side.)
    pub fn outlined(&self, object: ObjectRef) -> bool {
        outline::outlined(self.outline_word(object))
    }
    /// Set a type (with the translucent bit, if wanted). Refused when the
    /// word already has a type or the object has the no-highlight flag
    /// `0x1000`; otherwise the word becomes `word`, with the disabled bit
    /// added when the object is hidden. Returns whether it was set.
    pub fn set_outline(&mut self, object: ObjectRef, word: u32) -> bool {
        let Some(found) = self.get_mut(object) else {
            return false;
        };
        if outline::kind(found.raw.outline) != 0
            || found.raw.flags.contains(BasicFlags::NO_HIGHLIGHT)
        {
            return false;
        }
        found.raw.outline = word;
        if found.raw.flags.contains(BasicFlags::HIDDEN) {
            found.raw.outline |= outline::DISABLED;
        }
        true
    }
    /// Enable: clear the disabled bit.
    pub fn enable_outline(&mut self, object: ObjectRef) {
        if let Some(found) = self.get_mut(object) {
            found.raw.outline &= !outline::DISABLED;
        }
    }
    /// Disable: set the disabled bit, only when a type is set.
    pub fn disable_outline(&mut self, object: ObjectRef) {
        if let Some(found) = self.get_mut(object)
            && outline::kind(found.raw.outline) != 0
        {
            found.raw.outline |= outline::DISABLED;
        }
    }
    /// Remove: the word becomes 0.
    pub fn remove_outline(&mut self, object: ObjectRef) {
        if let Some(found) = self.get_mut(object) {
            found.raw.outline = 0;
        }
    }
    /// Hide or show an object (flag `0x00000001`). Hiding disables its
    /// outline; showing a hidden object enables it. Neither reinserts the
    /// object in its hex list.
    pub fn set_hidden(&mut self, object: ObjectRef, hidden: bool) {
        let Some(found) = self.get_mut(object) else {
            return;
        };
        let was_hidden = found.raw.flags.contains(BasicFlags::HIDDEN);
        if hidden {
            found.raw.flags.0 |= BasicFlags::HIDDEN.0;
            self.disable_outline(object);
        } else {
            found.raw.flags.0 &= !BasicFlags::HIDDEN.0;
            if was_hidden {
                self.enable_outline(object);
            }
        }
    }
    /// The combat outline type of a critter: friendly (8) when the player
    /// has Friendly Foe and the critter is on the player's team, else
    /// hostile (1).
    pub fn combat_outline(&self, critter: ObjectRef) -> u32 {
        let team = |critter: ObjectRef| self.critter_state(critter).map(|state| state.team);
        if self.perk(Perk::FriendlyFoe) > 0 && team(critter) == team(self.player) {
            outline::FRIENDLY
        } else {
            outline::HOSTILE
        }
    }
    /// Combat start: every combatant gets its combat type, set disabled.
    pub(crate) fn start_combat_outlines(&mut self) {
        for critter in self.combat_state.list.clone() {
            let kind = self.combat_outline(critter);
            self.set_outline(critter, kind);
            self.disable_outline(critter);
        }
    }
    /// SPEC-009 §3.15 step 4: in combat a critter that changed teams gets
    /// its type rebuilt, and is enabled again when it was enabled.
    pub fn rebuild_combat_outline(&mut self, critter: ObjectRef) {
        if !self.combat {
            return;
        }
        let enabled = self.outlined(critter);
        self.remove_outline(critter);
        let kind = self.combat_outline(critter);
        self.set_outline(critter, kind);
        self.disable_outline(critter);
        if enabled {
            self.enable_outline(critter);
        }
    }
    /// The enable step of the target outlines, which needs the
    /// `target_highlight` setting to be nonzero now and the cursor to be
    /// in crosshair mode. In combat it enables every living combatant
    /// other than the player. Outside combat every living critter on the
    /// current elevation other than the player gets type 8 (the player's
    /// team) or 1 (anyone else), with no Friendly Foe test, enabled.
    pub fn enable_target_outlines(&mut self) {
        if self.target_highlight == 0 || !self.crosshair {
            return;
        }
        let player = self.player;
        if self.combat {
            for critter in self.combat_state.list.clone() {
                if critter != player && !self.is_dead(critter) {
                    self.enable_outline(critter);
                }
            }
            return;
        }
        let team =
            |world: &Self, critter: ObjectRef| world.critter_state(critter).map(|state| state.team);
        let player_team = team(self, player);
        for critter in self.outside_targets() {
            let kind = if team(self, critter) == player_team {
                outline::FRIENDLY
            } else {
                outline::HOSTILE
            };
            self.set_outline(critter, kind);
            self.enable_outline(critter);
        }
    }
    /// The disable step: in combat every combatant other than the player
    /// is disabled; outside combat the outlines the enable step set are
    /// removed entirely.
    pub fn disable_target_outlines(&mut self) {
        let player = self.player;
        if self.combat {
            for critter in self.combat_state.list.clone() {
                if critter != player {
                    self.disable_outline(critter);
                }
            }
            return;
        }
        for critter in self.outside_targets() {
            self.remove_outline(critter);
        }
    }
    /// The critters the outside-combat steps apply to: living, placed on
    /// the current elevation, not the player.
    fn outside_targets(&self) -> Vec<ObjectRef> {
        self.iter()
            .filter(|(reference, object)| {
                *reference != self.player
                    && object.owner.is_none()
                    && object.raw.hex.is_some()
                    && object.raw.elevation == self.elevation
                    && object.character.is_some()
                    && object.alive()
            })
            .map(|(reference, _)| reference)
            .collect()
    }
}
