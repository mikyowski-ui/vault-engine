//! SPEC-011 host integration outside drugs and barter: experience awards
//! with their level-up message and sound (§3.20), the player-stat setter,
//! the perk chooser's display-name order (§3.21), opening the inventory in
//! combat (§3.11), the player's armor art (§3.10), and the drag reload's
//! ready sound (§3.15).
use crate::{
    ObjectRef, Perk, WorldEffect, WorldError, ai, assets::WorldAssets, host::WorldHost,
    progression::PerkChoice,
};
use vault_formats::pro::{BasicFlags, ItemData};

/// The player prototype, whose art is the unarmored body art.
const PLAYER_PID: u32 = 0x0100_0000;

impl<A: WorldAssets> WorldHost<A> {
    /// Each gained level prints `stat.msg` 600 and plays `levelup`.
    fn announce_levels(&mut self, levels: u32) {
        for _ in 0..levels {
            self.print_game("stat.msg", 600, &[]);
            self.world.emit(WorldEffect::SoundEffect {
                name: b"levelup".to_vec(),
                owner: None,
            });
        }
    }
    /// SPEC-011 §3.20 experience award (`give_exp_points`, combat kills,
    /// skill and quest awards).
    pub fn give_experience(&mut self, amount: i32) -> u32 {
        let levels = self.world.give_experience(amount);
        self.announce_levels(levels);
        levels
    }
    /// The player-stat setter (selectors 0-4); setting the experience
    /// re-runs the level loop. Other selectors are rejected (safe rule).
    pub fn set_pc_stat(&mut self, index: i32, value: i32) -> bool {
        let context = self.world.context(self.world.player);
        let Some(levels) = self
            .world
            .player_sheet_mut()
            .and_then(|sheet| sheet.set_pc_stat(index, value, context))
        else {
            return false;
        };
        self.world.sync_hit_points(self.world.player);
        self.announce_levels(levels);
        true
    }
    /// §3.21 chooser list: every eligible perk with its `perk.msg` name
    /// (101 + index), in display-name order.
    pub fn perk_choices(&self) -> Vec<(Perk, Vec<u8>)> {
        let context = self.world.context(self.world.player);
        let mut choices: Vec<_> = self
            .world
            .player_sheet()
            .map(|sheet| sheet.perk_choices(context))
            .unwrap_or_default()
            .into_iter()
            .map(|perk| {
                let name = self
                    .game_text("perk.msg", 101 + perk.index() as i32)
                    .unwrap_or_default();
                (perk, name)
            })
            .collect();
        choices.sort_by(|a, b| a.1.cmp(&b.1).then(a.0.cmp(&b.0)));
        choices
    }
    /// §3.21 choosing a perk in the chooser.
    pub fn choose_perk(&mut self, perk: Perk) -> PerkChoice {
        let context = self.world.context(self.world.player);
        let choice = self
            .world
            .player_sheet_mut()
            .map_or(PerkChoice::Refused, |sheet| {
                sheet.choose_perk(perk, context)
            });
        self.world.sync_hit_points(self.world.player);
        choice
    }
    /// §3.11: opening the inventory in combat costs `4 - Quick Pockets` AP
    /// for the player; a cost above the current AP prints `inventry.msg` 19
    /// and refuses. Returns whether the inventory opens.
    pub fn open_inventory(&mut self) -> bool {
        if !self.world.combat {
            return true;
        }
        let cost = crate::combat::inventory_entry_cost(self.world.perk(Perk::QuickPockets));
        let player = self.world.player;
        let ap = self.world.action_points(player);
        if cost > ap {
            self.print_game("inventry.msg", 19, &[]);
            return false;
        }
        if let Some(state) = self.world.critter_state_mut(player) {
            state.ap -= cost;
        }
        true
    }
    /// §3.10 player armor art: worn armor selects its female or male art
    /// (by the player's gender; -1 falls back to art index 1); no armor
    /// restores the body art of the player prototype.
    pub fn refresh_armor_art(&mut self, critter: ObjectRef) {
        if critter != self.world.player {
            return;
        }
        let Some(object) = self.world.get(critter) else {
            return;
        };
        let female = object
            .character
            .as_ref()
            .is_some_and(|sheet| sheet.stat(34, self.world.context(critter)) == 1);
        let worn = object.inventory.iter().find_map(|(item, _)| {
            let worn = self.world.get(*item)?.raw.flags.contains(BasicFlags::WORN);
            match &self.world.item_data(*item)?.data {
                ItemData::Armor(armour) if worn => Some(if female {
                    armour.female_art
                } else {
                    armour.male_art
                }),
                _ => None,
            }
        });
        let base = match worn {
            Some(-1) => 1,
            Some(art) => art & 0xfff,
            None => match self.assets.prototype(PLAYER_PID) {
                Ok(prototype) => (prototype.art_id & 0xfff) as i32,
                Err(_) => return,
            },
        };
        if let Some(object) = self.world.get_mut(critter) {
            object.raw.art_id = (object.raw.art_id & !0xfff) | base as u32;
        }
    }
    /// SPEC-014 §3.34 the inventory windows' rotating figure: `owner`'s art,
    /// standing, facing `facing`, frame 0. A critter shows its worn armor's
    /// male or female art index for its gender (the default player art when
    /// that index is -1), else the default player art, with the weapon code
    /// of `active`, the interface bar's active-hand item, when it is a
    /// weapon (else 0). A non-critter shows its own art. The player's
    /// prototype is a runtime one (SPEC-003) with no file behind it, so
    /// when it cannot be loaded the default art is the player's current
    /// art.
    pub fn figure_art(
        &self,
        owner: ObjectRef,
        active: Option<ObjectRef>,
        facing: u8,
    ) -> Option<u32> {
        let object = self.world.get(owner)?;
        if object.category() != vault_formats::pro::Category::Critter {
            return Some(object.raw.art_id);
        }
        let female = object
            .character
            .as_ref()
            .is_some_and(|sheet| sheet.stat(34, self.world.context(owner)) == 1);
        let armor = object.inventory.iter().find_map(|(item, _)| {
            let worn = self.world.get(*item)?.raw.flags.contains(BasicFlags::WORN);
            match &self.world.item_data(*item)?.data {
                ItemData::Armor(armour) if worn => Some(if female {
                    armour.female_art
                } else {
                    armour.male_art
                }),
                _ => None,
            }
        });
        let default = || {
            self.assets
                .prototype(PLAYER_PID)
                .map_or(object.raw.art_id, |prototype| prototype.art_id)
                & 0xfff
        };
        let base = match armor {
            Some(index) if index != -1 => index as u32 & 0xfff,
            _ => default(),
        };
        let weapon = active
            .and_then(|item| match &self.world.item_data(item)?.data {
                ItemData::Weapon(weapon) => u8::try_from(weapon.animation).ok(),
                _ => None,
            })
            .unwrap_or(0);
        Some(self.assets.construct_fid(
            1,
            base as u16,
            0,
            weapon,
            vault_formats::Fid::selector_for_orientation(facing),
        ))
    }
    /// `wield_obj_critter`: armor is worn (§3.10 for the player, the worn
    /// flag only for an NPC), anything else goes to the right hand, or the
    /// player's active hand.
    pub fn wield_object(&mut self, critter: ObjectRef, item: ObjectRef) -> Result<(), WorldError> {
        let armour = self
            .world
            .item_data(item)
            .is_some_and(|data| matches!(data.data, ItemData::Armor(_)));
        let flag = if armour {
            BasicFlags::WORN
        } else if critter == self.world.player && self.world.active_hand == 0 {
            BasicFlags::LEFT_HAND
        } else {
            BasicFlags::RIGHT_HAND
        };
        self.world.equip(critter, item, flag.0)?;
        if armour {
            self.refresh_armor_art(critter);
        }
        Ok(())
    }
    /// §3.15 inventory drag reload; anything loaded plays the weapon's
    /// ready sound.
    pub fn drag_reload(
        &mut self,
        owner: ObjectRef,
        weapon: ObjectRef,
        ammo: ObjectRef,
        count: i32,
    ) -> Result<bool, WorldError> {
        let loaded = self.world.drag_reload(owner, weapon, ammo, count)?;
        if loaded {
            self.emit_weapon_sound(owner, weapon, ai::READY_SOUND);
        }
        Ok(loaded)
    }
}
