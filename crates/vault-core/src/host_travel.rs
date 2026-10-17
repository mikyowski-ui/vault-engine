//! SPEC-008 §3.4/§3.5/§3.7 world travel loop: terrain schedules, walk-mask
//! stops, queue-interrupted time charges, every-third-iteration discovery,
//! W-iteration 24-hour healing and encounter checks, and town entry.
use crate::{
    Perk, WorldError,
    assets::WorldAssets,
    critical::effect,
    host::WorldHost,
    worldmap::{
        self, ENTRY_GLOBAL, Encounter, REGION_GLOBAL, Stepper, Terrain, TownButton, TownEntry,
        Travel, TravelSpeed,
    },
};
use std::collections::BTreeSet;
use vault_formats::map::ObjectState;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TravelOutcome {
    /// No travel is active.
    Idle,
    Moving,
    Arrived,
    /// The walking mask restored the prior coordinates and stopped movement.
    Blocked,
    /// A queue handler returned nonzero: travel stops for a town/terrain map.
    Interrupted(i32),
    Encounter(Encounter),
}

impl<A: WorldAssets> WorldHost<A> {
    fn set_global(&mut self, index: usize, value: i32) {
        if let Some(slot) = self.world.gvars.as_mut().and_then(|g| g.get_mut(index)) {
            *slot = value;
        }
    }
    /// The player's W/Q from displayed Outdoorsman and Pathfinder ranks.
    pub fn travel_speed(&self) -> Result<TravelSpeed, WorldError> {
        let context = self.world.context(self.world.player);
        let outdoorsman = self
            .world
            .player_sheet()
            .map_or(0, |sheet| sheet.skill(17, context));
        worldmap::travel_speed(outdoorsman, self.world.perk(Perk::Pathfinder))
    }
    /// Begin interactive travel toward a world pixel.
    pub fn travel_to(&mut self, destination: [i32; 2]) {
        let map = &mut self.world.world_map;
        map.travel = Some(Travel {
            destination,
            stepper: Stepper::new(map.position(), destination),
            scripted: false,
        });
    }
    /// Script-directed town travel toward a jittered town-center target.
    pub fn travel_to_town(&mut self, town: usize) -> Result<(), WorldError> {
        let destination = worldmap::town_target(town, &mut self.world.rng)?;
        let map = &mut self.world.world_map;
        map.travel = Some(Travel {
            destination,
            stepper: Stepper::new(map.position(), destination),
            scripted: true,
        });
        Ok(())
    }
    /// One movement-loop iteration: the terrain schedule's step attempts,
    /// then the time charge, the every-third-iteration discovery update, and
    /// the W-iteration healing and encounter check.
    pub fn travel_iteration(&mut self) -> Result<TravelOutcome, WorldError> {
        let Some(mut travel) = self.world.world_map.travel else {
            return Ok(TravelOutcome::Idle);
        };
        let speed = self.travel_speed()?;
        let map = &mut self.world.world_map;
        let terrain = map.cell().map_or(Terrain::Desert, worldmap::terrain);
        let mask = map.walk_mask.clone().filter(|_| !travel.scripted);
        let mut position = map.position();
        let mut outcome = TravelOutcome::Moving;
        let charge = map.schedule.iterate(terrain, || {
            if outcome != TravelOutcome::Moving {
                return false;
            }
            match travel.stepper.step(position) {
                None => outcome = TravelOutcome::Arrived,
                Some(next) if mask.as_ref().is_some_and(|mask| mask.blocks(next)) => {
                    outcome = TravelOutcome::Blocked;
                }
                Some(next) => position = next,
            }
            outcome == TravelOutcome::Moving
        });
        map.set_position(position);
        map.travel = Some(travel);
        // Script-directed travel's city branch adds no game time at all.
        if charge && !(travel.scripted && terrain == Terrain::City) {
            let result = self.charge_travel_time(speed.ticks)?;
            if result != 0 {
                self.world.world_map.travel = None;
                return Ok(TravelOutcome::Interrupted(result));
            }
        }
        let scout = self.world.perk(Perk::Scout) > 0;
        let map = &mut self.world.world_map;
        map.iterations = map.iterations.wrapping_add(1);
        if map.iterations.is_multiple_of(3) {
            map.reveal(scout);
        }
        map.interval += 1;
        if map.interval >= speed.interval {
            map.interval = 0;
            self.heal_party(24);
            if !travel.scripted {
                let luck = self.world.player_stat(6);
                let explorer = self.world.perk(Perk::Explorer);
                let world = &mut self.world;
                if let Some(encounter) =
                    world
                        .world_map
                        .check_encounter(&mut world.rng, luck, explorer)?
                {
                    world.world_map.travel = None;
                    return Ok(TravelOutcome::Encounter(encounter));
                }
            }
        }
        if outcome != TravelOutcome::Moving {
            self.world.world_map.travel = None;
        }
        Ok(outcome)
    }
    /// Charge Q ticks. A charge reaching/passing the next queued event first
    /// moves to that event tick plus one (never backwards) and processes the
    /// queue; a nonzero handler result stops at that time.
    fn charge_travel_time(&mut self, ticks: u32) -> Result<i32, WorldError> {
        let target = self
            .world
            .clock
            .ticks
            .checked_add(u64::from(ticks))
            .ok_or(WorldError::Arithmetic)?;
        if let Some(due) = self.world.queue.next_due()
            && target >= due
        {
            let interim = due.saturating_add(1).max(self.world.clock.ticks);
            self.world.clock.set(interim);
            let result = self.process_queue()?;
            if result != 0 {
                return Ok(result);
            }
        }
        self.world.clock.set(target);
        Ok(0)
    }
    /// Party resting healing for J hours: the player and each living critter
    /// party member gain `trunc(J / 3)` effective healing-rate units, bounded
    /// by maximum HP.
    pub fn heal_party(&mut self, hours: i32) {
        let members: BTreeSet<_> = std::iter::once(self.world.player)
            .chain(self.world.party.iter().copied())
            .collect();
        for member in members {
            let context = self.world.context(member);
            let Some(object) = self.world.get_mut(member) else {
                continue;
            };
            let ObjectState::Critter(state) = &mut object.raw.state else {
                continue;
            };
            let Some(sheet) = object.character.as_mut() else {
                continue;
            };
            if state.results.0 & effect::DEAD != 0 {
                continue;
            }
            let rate = sheet.stat(14, context);
            sheet.heal((hours / 3).saturating_mul(rate), context);
            state.hp = sheet.current_hp;
        }
    }
    /// Voluntary terrain dropout at the current cell (entry context 1).
    pub fn terrain_dropout(&mut self) -> Result<Encounter, WorldError> {
        let cell =
            self.world.world_map.cell().ok_or_else(|| {
                WorldError::Invalid("world position outside the cell grid".into())
            })?;
        Ok(Encounter::Terrain {
            map: worldmap::terrain_map(worldmap::terrain(cell), &mut self.world.rng)?,
            region: worldmap::region(cell),
            context: 1,
        })
    }
    /// Load an encounter/dropout map after global 65 receives the cell's
    /// encounter region and global 32 the entry context.
    pub fn enter_encounter(&mut self, encounter: Encounter) -> Result<(), WorldError> {
        self.set_global(REGION_GLOBAL, i32::from(encounter.region()));
        self.set_global(ENTRY_GLOBAL, encounter.context());
        self.load_map(&format!("maps/{}", encounter.map().to_ascii_lowercase()))
    }
    /// Derive town and entrance knowledge from the campaign globals.
    pub fn refresh_world_knowledge(&mut self) {
        let ticks = self.world.clock.ticks;
        let gvars = self.world.gvars.clone().unwrap_or_default();
        self.world.world_map.refresh_knowledge(&gvars, ticks);
    }
    /// Enter a town: the first entry loads its slot-zero map directly; a
    /// later entry returns the town map's buttons for selection.
    pub fn enter_town(&mut self, town: usize) -> Result<Option<TownEntry>, WorldError> {
        self.refresh_world_knowledge();
        let gvars = self.world.gvars.clone().unwrap_or_default();
        let entry = self.world.world_map.enter_town(town, &gvars);
        if let Some(TownEntry::Direct(button)) = entry {
            self.select_town_entrance(town, button)?;
        }
        Ok(entry)
    }
    /// A town-map selection passes the slot's entry selector through global
    /// 32 (not automatically an elevation), records the town and original
    /// slot as the current section, and loads the slot's map.
    pub fn select_town_entrance(
        &mut self,
        town: usize,
        button: TownButton,
    ) -> Result<(), WorldError> {
        self.set_global(ENTRY_GLOBAL, button.entry);
        self.world.world_map.saved.town = town as i32;
        self.world.world_map.saved.section = button.slot as i32;
        self.load_map(&format!("maps/{}", button.map.to_ascii_lowercase()))
    }
}
