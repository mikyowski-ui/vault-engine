//! SPEC-011 host integration outside drugs and barter: experience awards
//! with their level-up message and sound (§3.20), the player-stat setter,
//! the perk chooser's display-name order (§3.21), opening the inventory in
//! combat (§3.11), and the drag reload's ready sound (§3.15); and the
//! SPEC-005 §3.8 player base art: the jumpsuit by gender, armor art, and
//! the inventory figure; and new item instances that know their loaded
//! ammunition (§3.14-3.16).
use crate::{
    ObjectRef, Perk, WorldEffect, WorldError, ai, assets::WorldAssets, host::WorldHost,
    progression::PerkChoice,
};
use std::sync::Arc;
use vault_formats::{
    Fid,
    pro::{BasicFlags, Item, ItemData, Prototype, PrototypeData},
};

/// SPEC-005 §3.8 the `critters.lst` base names of the male and female
/// jumpsuits.
const JUMPSUITS: [&[u8]; 2] = [b"hmjmps", b"hfjmps"];

impl<A: WorldAssets> WorldHost<A> {
    /// A new instance of `prototype` placed on the map
    /// (`WorldState::spawn`). A weapon is created loaded with its
    /// prototype's ammunition, whose prototype is recorded too, so the
    /// weapon has that ammunition's weight and value and can be unloaded
    /// (§3.14-3.16), as an imported weapon does.
    pub(crate) fn spawn(
        &mut self,
        prototype: Arc<Prototype>,
        hex: i32,
        elevation: u8,
    ) -> Result<ObjectRef, WorldError> {
        self.remember_ammo(&prototype);
        self.world.spawn(prototype, hex, elevation)
    }
    fn remember_ammo(&mut self, prototype: &Prototype) {
        if let PrototypeData::Item(Item {
            data: ItemData::Weapon(weapon),
            ..
        }) = &prototype.data
            && let Ok(pid) = u32::try_from(weapon.ammo_pid)
            && self.world.known_prototype(pid).is_none()
            && let Ok(ammo) = self.assets.prototype(pid)
        {
            self.world.remember_prototype(ammo);
        }
    }
    /// Each gained level prints `stat.msg` 600 and plays `levelup`.
    fn announce_levels(&mut self, levels: u32) {
        for _ in 0..levels {
            self.print_game("stat.msg", 600, &[]);
            self.world.emit(WorldEffect::SoundEffect {
                name: b"levelup".to_vec(),
                owner: None,
            });
        }
        // SPEC-019 §3.17: under Fallout 2 each gain may advance members.
        self.party_level_ups(levels);
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
    /// The armor `critter` wears.
    pub(crate) fn worn_armor(&self, critter: ObjectRef) -> Option<ObjectRef> {
        self.world
            .get(critter)?
            .inventory
            .iter()
            .map(|(item, _)| *item)
            .find(|item| {
                self.world
                    .get(*item)
                    .is_some_and(|object| object.raw.flags.contains(BasicFlags::WORN))
                    && matches!(
                        self.world.item_data(*item).map(|data| &data.data),
                        Some(ItemData::Armor(_))
                    )
            })
    }
    /// SPEC-005 §3.8 an armor's art field for `wearer`'s gender: the female
    /// field for gender stat 1, else the male one, as a base index (its low
    /// 12 bits); `None` for a field of -1.
    fn armor_base(&self, wearer: ObjectRef, armor: ObjectRef) -> Option<u16> {
        let female = self
            .world
            .get(wearer)?
            .character
            .as_ref()?
            .stat(34, self.world.context(wearer))
            == 1;
        let ItemData::Armor(armour) = &self.world.item_data(armor)?.data else {
            return None;
        };
        let field = if female {
            armour.female_art
        } else {
            armour.male_art
        };
        (field != -1).then_some((field & 0xfff) as u16)
    }
    /// SPEC-005 §3.8 the gender update, run at every player-sheet load
    /// (new game, premade or edited character), at the end of a load and
    /// after `move_obj_inven_to_obj` from the player. The default player
    /// art becomes the jumpsuit row for the player's gender (row 0 when
    /// `critters.lst` has none). Without worn armor the player's art is
    /// replaced at once: that base, standing, selector 0, and the current
    /// weapon code only while a hand holds an item. With armor it stays.
    pub fn update_player_gender(&mut self) {
        let player = self.world.player;
        let female = self.world.get(player).is_some_and(|object| {
            object
                .character
                .as_ref()
                .is_some_and(|sheet| sheet.stat(34, self.world.context(player)) != 0)
        });
        let default = self
            .assets
            .art_row(1, JUMPSUITS[usize::from(female)])
            .unwrap_or(0);
        self.world.default_player_art = default;
        if self.worn_armor(player).is_some() {
            return;
        }
        let holding = self.world.get(player).is_some_and(|object| {
            object.inventory.iter().any(|(item, _)| {
                self.world.get(*item).is_some_and(|item| {
                    item.raw.flags.0 & (BasicFlags::LEFT_HAND.0 | BasicFlags::RIGHT_HAND.0) != 0
                })
            })
        });
        if let Some(object) = self.world.get_mut(player) {
            let weapon = if holding {
                Fid(object.raw.art_id).weapon()
            } else {
                0
            };
            object.raw.art_id = Fid::new(1, default, 0, weapon, 0).0;
        }
    }
    /// SPEC-014 §3.34 the inventory windows' rotating figure: `owner`'s art,
    /// standing, facing `facing`, frame 0. A critter shows its worn armor's
    /// art for its gender, or the default player art without armor or for a
    /// field of -1 (SPEC-005 §3.8), with the weapon code of `active`, the
    /// interface bar's active-hand item, when it is a weapon (else 0). A
    /// non-critter shows its own art.
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
        let base = self
            .worn_armor(owner)
            .and_then(|armor| self.armor_base(owner, armor))
            .unwrap_or(self.world.default_player_art);
        let weapon = active
            .and_then(|item| match &self.world.item_data(item)?.data {
                ItemData::Weapon(weapon) => u8::try_from(weapon.animation).ok(),
                _ => None,
            })
            .unwrap_or(0);
        Some(
            self.assets
                .construct_fid(1, base, 0, weapon, Fid::selector_for_orientation(facing)),
        )
    }
    /// SPEC-005 §3.8 the player inventory window closes: the player's art
    /// becomes the figure art at once (standing, selector 0), so armor put
    /// on or taken off in the window shows from now on. `active` is the
    /// interface bar's active-hand item.
    pub fn apply_figure_art(&mut self, active: Option<ObjectRef>) {
        let player = self.world.player;
        if let Some(fid) = self.figure_art(player, active, 0)
            && let Some(object) = self.world.get_mut(player)
        {
            object.raw.art_id = fid;
            object.raw.frame = 0;
        }
    }
    /// SPEC-005 §3.8 `rm_obj_from_inven` took worn armor off the player:
    /// the default player art at once, keeping the animation, weapon code
    /// and selector. Any other owner's art is unchanged.
    pub(crate) fn unworn_art(&mut self, owner: ObjectRef) {
        if owner != self.world.player {
            return;
        }
        let base = u32::from(self.world.default_player_art);
        if let Some(object) = self.world.get_mut(owner) {
            object.raw.art_id = (object.raw.art_id & !0xfff) | base;
        }
    }
    /// `wield_obj_critter`: armor is worn, anything else goes to the right
    /// hand, or the player's active hand. For the player, SPEC-005 §3.8
    /// registers the armor's art in a new reserved animation sequence: its
    /// field for the player's gender (under the safe rule the default
    /// player art for -1, where the reference takes base index 1),
    /// standing, with the current weapon code. The worn flag moves for any
    /// critter, the protection only for the player (SPEC-011 §3.10).
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
        // From a stack the slot takes one unit (SPEC-011 §3.13).
        let item = self.world.split_unit(item)?;
        self.world.equip(critter, item, flag.0)?;
        if armour && critter == self.world.player {
            let base = self
                .armor_base(critter, item)
                .unwrap_or(self.world.default_player_art);
            let Some(object) = self.world.get(critter) else {
                return Ok(());
            };
            let fid = self.assets.construct_fid(
                1,
                base,
                0,
                Fid(object.raw.art_id).weapon(),
                Fid::selector_for_orientation(object.raw.orientation),
            );
            self.register_art_change(critter, fid);
        }
        Ok(())
    }
    /// A new reserved animation sequence whose one entry, with start delay
    /// 0, changes `object`'s art to `fid`: an instant entry, so it takes
    /// effect as the sequence registers (SPEC-007 §3.10.1).
    fn register_art_change(&mut self, object: ObjectRef, fid: u32) {
        let mut sequence = crate::motion::MotionSequence::new();
        match sequence.push(object, 0, crate::motion::Motion::SetArt { art: fid }) {
            Ok(()) => {
                self.register_motion(sequence);
            }
            Err(refused) => self
                .world
                .diagnostic(format!("art change refused: {refused:?}")),
        }
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
