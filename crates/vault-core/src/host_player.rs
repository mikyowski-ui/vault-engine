//! The player's own actions as rules, with no screen behind them: the attack
//! request, the slot's reload, a use-only item's AP and its use, and world-map
//! travel and entry. The interface and the autoplay bot share these, so a
//! click and a bot action spend the same AP and draw the same numbers. What is
//! left to a caller is named in each result: a window to open, a message to
//! print, a screen to change.
use crate::{
    Refusal, WorldEffect, WorldError, ai,
    assets::WorldAssets,
    combat_state::CombatRequest,
    critical::{hit_mode, location, state::INPUT_GATE},
    fallout2::{self, Selected},
    host::WorldHost,
    host_fallout2::Fallout2Travel,
    host_skills::BookRoute,
    host_travel::TravelOutcome,
    skills,
    ui::automap::MOTION_SENSOR,
    world::ObjectRef,
    worldmap::{self, Encounter},
};
use vault_formats::pro::{ItemData, PrototypeData};

/// A cap on the slot's reload loop: each reload fills the magazine or uses up
/// one pack, so the loop ends; this only bounds malformed empty packs.
const RELOAD_LIMIT: usize = 1000;

/// What a player attack request came to (SPEC-014 §3.38).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PlayerAttack {
    /// The input gate is shut, so nothing happened.
    Gated,
    /// The preflight or the attack refused; `cost` is the attack's AP cost,
    /// for the `combat.msg` line that prints it.
    Refused { refusal: Refusal, cost: i32 },
    /// Out of combat the request starts combat, and the caller replays it on
    /// the player's first turn.
    CombatRequested,
    /// An aimed mode in combat: the caller chooses the location.
    Aimed,
    /// The attack ran.
    Attacked,
}

/// What the slot's reload came to (SPEC-014 §3.23).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PlayerReload {
    /// In combat without the AP: nothing changed.
    ShortOfAp,
    /// The reload ran and its ready sound was emitted. `error` stopped it
    /// early, with whatever had already loaded kept.
    Ran { error: Option<WorldError> },
}

/// What using an item came to, past the hook. The caller drains effects after
/// `Done`, `Failed` and `OpenTimer`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ItemUse {
    Done,
    /// The use itself failed; its effects still stand.
    Failed(WorldError),
    /// A container in the inventory window: the window opens it.
    OpenContainer,
    /// SPEC-013 §3.10: an armable explosive opens the Set Timer window.
    OpenTimer,
    /// SPEC-013 §3.33: a Motion Sensor opens the automap.
    MotionSensor,
}

/// How one world-map iteration ended, on either edition.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Leg<E> {
    /// No travel is active.
    Idle,
    Moving,
    /// Arrived or blocked: the journey ends, and the destination is not
    /// entered automatically.
    Stopped,
    /// A queue handler stopped travel, and the party's place is entered.
    Interrupted,
    Encounter(E),
    /// SPEC-019 §3.19.3 the tank ran dry.
    OutOfFuel,
}

/// One world-map pass's two iterations (SPEC-014 §3.39).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TravelPass<E> {
    /// The travel-dot pixels the iterations marked, in order.
    pub marks: [Option<[i32; 2]>; 2],
    /// Whether either iteration travelled.
    pub travelled: bool,
    /// How the pass ended: `Moving` or `Idle` carry on to the campaign checks.
    pub last: Leg<E>,
}

/// Up to two iterations, stopping at the first that does not keep moving.
fn pass<E>(
    mut iterate: impl FnMut() -> Result<(Leg<E>, Option<[i32; 2]>), WorldError>,
) -> Result<TravelPass<E>, WorldError> {
    let mut marks = [None; 2];
    let mut travelled = false;
    let mut last = Leg::Idle;
    for mark in &mut marks {
        let (leg, pixel) = iterate()?;
        *mark = pixel;
        travelled |= !matches!(leg, Leg::Idle);
        let moving = matches!(leg, Leg::Moving);
        last = leg;
        if !moving {
            break;
        }
    }
    Ok(TravelPass {
        marks,
        travelled,
        last,
    })
}

impl<A: WorldAssets> WorldHost<A> {
    /// The player's attack request against `target` in `mode` (§3.38). The
    /// caller supplies the mode from the slot, or chooses one. Out of combat
    /// a passing request starts combat; in combat a mode that is not aimed
    /// attacks at once, uncalled.
    pub fn player_attack(
        &mut self,
        target: ObjectRef,
        mode: u8,
        aimed: bool,
    ) -> Result<PlayerAttack, WorldError> {
        if self.world.combat_state.flags & INPUT_GATE == 0 {
            return Ok(PlayerAttack::Gated);
        }
        let player = self.world.player;
        let mut attack =
            self.world
                .attack_profile(player, target, mode, location::UNCALLED, aimed)?;
        // SPEC-024b §6.10: the hook runs before the cost meets the AP.
        if self.hooks_on() {
            attack.hooked_cost = Some(self.hook_ap_cost(player, &attack));
        }
        let cost = attack.ap_cost();
        if let Err(refusal) = self.world.preflight(player, target, &attack) {
            return Ok(PlayerAttack::Refused { refusal, cost });
        }
        if !self.world.combat {
            self.request_combat_against(target);
            return Ok(PlayerAttack::CombatRequested);
        }
        if aimed {
            return Ok(PlayerAttack::Aimed);
        }
        // The kick draw follows the preflight (no draws).
        Ok(
            match self.perform_attack(player, target, mode, location::UNCALLED, false)? {
                Ok(_) => PlayerAttack::Attacked,
                Err(refusal) => PlayerAttack::Refused { refusal, cost },
            },
        )
    }

    /// The player asks for combat when not in it (`a`, and the item slot's
    /// attack modes).
    pub fn player_request_combat(&mut self) {
        if !self.world.combat {
            self.request_combat_against(0);
        }
    }

    fn request_combat_against(&mut self, defender: ObjectRef) {
        self.world.pending_combat = Some(CombatRequest {
            attacker: self.world.player,
            defender,
            accuracy: 0,
            damage: [0, i32::MAX],
            flags: None,
        });
    }

    /// §3.23 the slot's reload of `weapon`: from the inventory until a reload
    /// fails (the SPEC-011 §3.15 search reload, so several stacks can go). In
    /// combat 2 AP are needed, checked first, and spent only when something
    /// loaded. The weapon's ready sound follows (SPEC-010).
    pub fn player_reload(&mut self, weapon: ObjectRef) -> PlayerReload {
        let player = self.world.player;
        let combat = self.world.combat;
        let cost = if combat {
            self.hook_fixed_cost(player, 2, weapon)
        } else {
            2
        };
        if combat
            && self
                .world
                .critter_state(player)
                .is_none_or(|state| state.ap < cost)
        {
            return PlayerReload::ShortOfAp;
        }
        let mut loaded = false;
        let mut error = None;
        for _ in 0..RELOAD_LIMIT {
            match self.world.search_reload(player, weapon) {
                Ok(true) => loaded = true,
                Ok(false) => break,
                Err(failure) => {
                    error = Some(failure);
                    break;
                }
            }
        }
        if loaded
            && combat
            && let Some(state) = self.world.critter_state_mut(player)
        {
            state.ap = (state.ap - cost).max(0);
        }
        let name = self
            .weapon_sound_name(ai::READY_SOUND, weapon, hit_mode::RIGHT_PRIMARY, 0)
            .unwrap_or_default();
        self.world.emit(WorldEffect::WeaponSound {
            object: player,
            weapon,
            effect: ai::READY_SOUND,
            name,
        });
        PlayerReload::Ran { error }
    }

    /// G-15 and §3.27: in combat a use needs and spends 2 AP, and without
    /// them nothing happens. Whether the use may go on.
    pub fn player_spend_use_ap(&mut self, item: ObjectRef) -> bool {
        if !self.world.combat {
            return true;
        }
        let player = self.world.player;
        let cost = self.hook_fixed_cost(player, 2, item);
        let Some(state) = self.world.critter_state_mut(player) else {
            return false;
        };
        if state.ap < cost {
            return false;
        }
        state.ap -= cost;
        true
    }

    /// An item's prototype data.
    fn item_data(&self, item: ObjectRef) -> Option<&ItemData> {
        match &self.world.get(item)?.prototype.as_deref()?.data {
            PrototypeData::Item(data) => Some(&data.data),
            _ => None,
        }
    }

    /// The player's use of `item`, from the inventory window (`in_inventory`)
    /// or the item slot: the skill books by their route first (SPEC-013
    /// §3.25); a container opens in the inventory window; an explosive opens
    /// the Set Timer window (§3.10, from either place); a Motion Sensor the
    /// automap; a kit on the player; a drug; else the item's use procedure.
    /// `Err` is a hook's failure, with nothing used.
    pub fn player_use_item(
        &mut self,
        item: ObjectRef,
        route: BookRoute,
        in_inventory: bool,
    ) -> Result<ItemUse, WorldError> {
        let player = self.world.player;
        let pid = self.world.get(item).map_or(0, |object| object.raw.pid);
        let container = matches!(self.item_data(item), Some(ItemData::Container { .. }));
        // SPEC-024b §6.15: before the item's own path. Opening a container
        // in the window is no use.
        let opens = container && in_inventory;
        if !opens && self.hook_item_use(player, item)? {
            return Ok(ItemUse::Done);
        }
        if skills::skill_book(pid).is_some() {
            return Ok(match self.read_book(item, route) {
                Ok(_) => ItemUse::Done,
                Err(error) => ItemUse::Failed(error),
            });
        }
        if opens {
            return Ok(ItemUse::OpenContainer);
        }
        if skills::is_explosive(pid) {
            return Ok(if self.explosive_armable(item) {
                ItemUse::OpenTimer
            } else {
                ItemUse::Done
            });
        }
        if pid == MOTION_SENSOR {
            return Ok(ItemUse::MotionSensor);
        }
        let drug = matches!(self.item_data(item), Some(ItemData::Drug(_)));
        let used = if self.kit_skill(pid).is_some() {
            self.use_kit(player, item, player).map(|_| ())
        } else if drug {
            self.use_drug(player, item).map(|_| ())
        } else {
            self.action(player, item, crate::ObjectAction::Use)
        };
        Ok(match used {
            Ok(()) => ItemUse::Done,
            Err(error) => ItemUse::Failed(error),
        })
    }

    /// One Fallout 1 world-map pass's travel.
    pub fn world_travel_pass(&mut self) -> Result<TravelPass<Encounter>, WorldError> {
        pass(|| {
            let leg = match self.travel_iteration()? {
                TravelOutcome::Idle => Leg::Idle,
                TravelOutcome::Moving => Leg::Moving,
                TravelOutcome::Arrived | TravelOutcome::Blocked => Leg::Stopped,
                TravelOutcome::Interrupted(_) => Leg::Interrupted,
                TravelOutcome::Encounter(encounter) => Leg::Encounter(encounter),
            };
            Ok((leg, self.world.world_map.mark))
        })
    }

    /// One Fallout 2 world-map pass's travel.
    pub fn fallout2_travel_pass(&mut self) -> Result<TravelPass<Selected>, WorldError> {
        pass(|| {
            let leg = match self.fallout2_travel_iteration()? {
                Fallout2Travel::Idle => Leg::Idle,
                Fallout2Travel::Moving => Leg::Moving,
                Fallout2Travel::Arrived | Fallout2Travel::Blocked => Leg::Stopped,
                Fallout2Travel::OutOfFuel => Leg::OutOfFuel,
                Fallout2Travel::Interrupted(_) => Leg::Interrupted,
                Fallout2Travel::Encounter(selected) => Leg::Encounter(selected),
            };
            let mark = self.world.fallout2.as_deref().and_then(|state| state.mark);
            Ok((leg, mark))
        })
    }

    /// Head for town `town`'s jittered center (SPEC-008 §3.7).
    pub fn player_head_for_town(&mut self, town: usize) -> Result<(), WorldError> {
        let target = worldmap::town_target(town, &mut self.world.rng)?;
        self.travel_to(target);
        Ok(())
    }

    /// SPEC-019 §3.12.3 head for city `city`'s jittered position. `false` for
    /// a city the tables do not hold.
    pub fn player_head_for_city(&mut self, city: usize) -> Result<bool, WorldError> {
        let Some(position) = self
            .world
            .fallout2
            .as_deref()
            .and_then(|state| state.areas.get(city))
            .map(|area| area.position)
        else {
            return Ok(false);
        };
        let target = fallout2::city_target(position, &mut self.world.rng)?;
        self.fallout2_travel_to(target)?;
        Ok(true)
    }

    /// Enter the terrain under the party (SPEC-008 §3.5 choice, index 65 and
    /// index 32 = 1).
    pub fn player_enter_terrain(&mut self) -> Result<(), WorldError> {
        self.terrain_dropout()
            .and_then(|encounter| self.enter_encounter(encounter))
    }

    /// The known cities, in table order.
    pub fn fallout2_known_cities(&self) -> Vec<usize> {
        let Some(state) = self.world.fallout2.as_deref() else {
            return Vec::new();
        };
        state
            .tables
            .city
            .areas()
            .map(|area| area.number as usize)
            .filter(|city| state.areas.get(*city).is_some_and(|area| area.known >= 1))
            .collect()
    }

    /// The city whose circle holds the party, known or not.
    pub fn fallout2_party_city(&self) -> Option<usize> {
        let state = self.world.fallout2.as_deref()?;
        state.area_at(state.position)
    }

    /// Whether `city` is locked.
    pub fn fallout2_city_locked(&self, city: usize) -> bool {
        self.world
            .fallout2
            .as_deref()
            .and_then(|state| state.areas.get(city))
            .is_some_and(|area| area.locked)
    }
}
