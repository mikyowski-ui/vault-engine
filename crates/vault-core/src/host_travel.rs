//! SPEC-008 §3.4/§3.5/§3.7 world travel loop: terrain schedules, walk-mask
//! stops, queue-interrupted time charges, every-third-iteration discovery,
//! W-iteration 24-hour healing and encounter checks, town entry, and the
//! script-directed town travel; SPEC-014 §3.39 building the world-map
//! screen and its campaign checks.
use crate::{
    Perk, WorldEffect, WorldError,
    assets::WorldAssets,
    critical::effect,
    host::WorldHost,
    movie,
    worldmap::{
        self, ENTRY_GLOBAL, Encounter, REGION_GLOBAL, Schedule, Stepper, Terrain, TownButton,
        TownEntry, Travel, TravelSpeed,
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

/// SPEC-014 §3.39 how a pass's campaign checks leave the world map.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WorldMapExit {
    /// The water ran out: `boil3` played and a quit is requested.
    Quit,
    /// A base explosion sent the party to Vault 13: `V13ENT.MAP` loads
    /// with entry selector 0 after this pass.
    Vault13,
}

/// SPEC-014 §3.39 campaign indices of the per-pass checks.
const WATER_GLOBAL: usize = movie::WATER_GLOBAL;
const VATS_TIMER_GLOBAL: usize = 147;
const LAIR_TIMER_GLOBAL: usize = 55;
const VATS_STATUS_GLOBAL: usize = 308;
const LAIR_STATUS_GLOBAL: usize = 309;
const KARMA_GLOBAL: usize = 155;
/// The countdown fires this many ticks after its stored tick.
const EXPLOSION_DELAY: i64 = 2410;
/// SPEC-012 §3.13 world-map water and vault movies play with flags `0x0B`.
const WORLD_MOVIE_FLAGS: u32 = 0x0b;
/// Vault 13's entrance map (map 35), loaded with selector 0.
const VAULT13_ENTRANCE: &str = "V13ENT.MAP";

impl<A: WorldAssets> WorldHost<A> {
    fn set_global(&mut self, index: usize, value: i32) {
        // SPEC-018 §6.2: the engine writes no campaign global under
        // Fallout 2.
        if self.world.fallout2.is_some() {
            return;
        }
        if let Some(slot) = self.world.gvars.as_mut().and_then(|g| g.get_mut(index)) {
            *slot = value;
        }
    }
    fn global(&self, index: usize) -> i32 {
        self.world
            .gvars
            .as_ref()
            .and_then(|gvars| gvars.get(index))
            .copied()
            .unwrap_or(0)
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
    /// SPEC-014 §3.39 building the world-map screen, on opening (`opening`,
    /// which also computes W and Q once) and on each return from a town
    /// map: town knowledge is refreshed, the SPEC-008 §3.5 reveal runs once
    /// at the party's cell, so the party's cell is always visited, and the
    /// travel-dot count starts again at 0.
    pub fn build_world_map(&mut self, opening: bool) -> Result<(), WorldError> {
        if opening {
            self.world.world_map.speed = Some(self.travel_speed()?);
        }
        self.refresh_world_knowledge();
        let scout = self.world.perk(Perk::Scout) > 0;
        let map = &mut self.world.world_map;
        map.reveal(scout);
        map.iterations = 0;
        map.mark = None;
        Ok(())
    }
    /// Begin interactive travel toward a world pixel, or restart it from
    /// the current pixel.
    pub fn travel_to(&mut self, destination: [i32; 2]) {
        let map = &mut self.world.world_map;
        map.travel = Some(Travel {
            destination,
            stepper: Stepper::new(map.position(), destination),
        });
    }
    /// SPEC-008 §3.4 script-directed town travel toward `town`, run to
    /// completion in this call with nothing drawn. Town -1 (or any town
    /// outside 0-11) does nothing, and so does a party already in the
    /// town's cell. Otherwise: the four-draw jitter of §3.7, W and Q
    /// recomputed, the W-iteration counter and a private terrain counter at
    /// zero, then the terrain schedule until the line completes. No
    /// encounter tests, walk-mask checks, queue processing, discovery
    /// reveal or path marks; time is added directly (none on the city
    /// branch), and 24-hour party healing still applies every W iterations.
    pub fn scripted_town_travel(&mut self, town: i32) -> Result<(), WorldError> {
        // SPEC-018 §5.4: Fallout 2 moves directly at the map load instead.
        if self.world.fallout2.is_some() {
            return Ok(());
        }
        let Some(cell) = usize::try_from(town)
            .ok()
            .and_then(|town| worldmap::TOWNS.get(town))
            .map(|town| town.cell)
        else {
            return Ok(());
        };
        if self.world.world_map.cell() == Some(cell) {
            return Ok(());
        }
        let destination = worldmap::town_target(town as usize, &mut self.world.rng)?;
        let speed = self.travel_speed()?;
        let map = &mut self.world.world_map;
        map.speed = Some(speed);
        map.interval = 0;
        let mut schedule = Schedule::default();
        let mut stepper = Stepper::new(map.position(), destination);
        // A line ends within max(A, B) + 1 steps; mountains step every
        // other iteration, so this bound is never reached by a legal line.
        let tables = self.world.rules.tables.clone();
        let tables = tables
            .as_deref()
            .unwrap_or_else(|| crate::tables::CampaignTables::compiled());
        for _ in 0..4 * (worldmap::WORLD_SIZE[0] + worldmap::WORLD_SIZE[1]) {
            let map = &mut self.world.world_map;
            let terrain = map
                .cell()
                .map_or(Terrain::Desert, |cell| tables.terrain(cell));
            let mut position = map.position();
            let mut done = false;
            let charge = schedule.iterate(terrain, || {
                if done {
                    return false;
                }
                match stepper.step(position) {
                    Some(next) => position = next,
                    None => done = true,
                }
                !done
            });
            map.set_position(position);
            if charge && terrain != Terrain::City {
                let ticks = self
                    .world
                    .clock
                    .ticks
                    .checked_add(u64::from(speed.ticks))
                    .ok_or(WorldError::Arithmetic)?;
                self.world.clock.set(ticks);
            }
            let map = &mut self.world.world_map;
            map.interval += 1;
            if map.interval >= speed.interval {
                map.interval = 0;
                self.heal_party(24);
            }
            if done {
                return Ok(());
            }
        }
        Err(WorldError::Invalid(
            "script-directed town travel did not finish".into(),
        ))
    }
    /// One movement-loop iteration: the terrain schedule's step attempts,
    /// then the time charge, the discovery update and travel dot on
    /// iterations 1, 4, 7, ... since the screen was built, and the
    /// W-iteration healing and encounter check.
    pub fn travel_iteration(&mut self) -> Result<TravelOutcome, WorldError> {
        let Some(mut travel) = self.world.world_map.travel else {
            return Ok(TravelOutcome::Idle);
        };
        let speed = match self.world.world_map.speed {
            Some(speed) => speed,
            None => self.travel_speed()?,
        };
        let tables = self.world.rules.tables.clone();
        let tables = tables
            .as_deref()
            .unwrap_or_else(|| crate::tables::CampaignTables::compiled());
        let map = &mut self.world.world_map;
        map.mark = None;
        let terrain = map
            .cell()
            .map_or(Terrain::Desert, |cell| tables.terrain(cell));
        let mask = map.walk_mask.clone();
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
        if charge {
            let result = self.charge_travel_time(speed.ticks)?;
            if result != 0 {
                self.world.world_map.travel = None;
                return Ok(TravelOutcome::Interrupted(result));
            }
        }
        let scout = self.world.perk(Perk::Scout) > 0;
        let map = &mut self.world.world_map;
        if map.iterations.is_multiple_of(3) {
            map.reveal(scout);
            map.mark = Some(map.position());
        }
        map.iterations = map.iterations.wrapping_add(1);
        map.interval += 1;
        if map.interval >= speed.interval {
            map.interval = 0;
            self.heal_party(24);
            let luck = self.world.player_stat(6);
            let explorer = self.world.perk(Perk::Explorer);
            let world = &mut self.world;
            if let Some(encounter) =
                world
                    .world_map
                    .check_encounter_with(tables, &mut world.rng, luck, explorer)?
            {
                world.world_map.travel = None;
                return Ok(TravelOutcome::Encounter(encounter));
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
    pub(crate) fn charge_travel_time(&mut self, ticks: u32) -> Result<i32, WorldError> {
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
        let tables = self.world.rules.tables.clone();
        let tables = tables
            .as_deref()
            .unwrap_or_else(|| crate::tables::CampaignTables::compiled());
        Ok(Encounter::Terrain {
            map: worldmap::terrain_map(tables.terrain(cell), &mut self.world.rng)?,
            region: tables.region(cell),
            context: 1,
        })
    }
    /// Load an encounter/dropout map after global 65 receives the cell's
    /// encounter region and global 32 the entry context.
    pub fn enter_encounter(&mut self, encounter: Encounter) -> Result<(), WorldError> {
        self.set_global(REGION_GLOBAL, i32::from(encounter.region()));
        self.set_global(ENTRY_GLOBAL, encounter.context());
        self.load_map(&format!("maps/{}", encounter.map().to_ascii_lowercase()))?;
        self.world_map_closed();
        Ok(())
    }
    /// Derive town and entrance knowledge from the campaign globals.
    pub fn refresh_world_knowledge(&mut self) {
        // SPEC-018 §5.8: campaign globals do not drive Fallout 2's areas.
        if self.world.fallout2.is_some() {
            return;
        }
        let ticks = self.world.clock.ticks;
        let gvars = self.world.gvars.clone().unwrap_or_default();
        self.world.world_map.refresh_knowledge(&gvars, ticks);
    }
    /// Whether the party's world-map cell is town `town`'s cell.
    pub fn party_in_town(&self, town: usize) -> bool {
        worldmap::TOWNS
            .get(town)
            .is_some_and(|row| self.world.world_map.cell() == Some(row.cell))
    }
    /// Enter a town: the first entry loads its slot-zero map directly; a
    /// later entry makes it the current town and returns the town map's
    /// buttons for selection.
    pub fn enter_town(&mut self, town: usize) -> Result<Option<TownEntry>, WorldError> {
        self.refresh_world_knowledge();
        let gvars = self.world.gvars.clone().unwrap_or_default();
        let entry = self.world.world_map.enter_town(town, &gvars);
        match entry {
            Some(TownEntry::Direct(button)) => self.select_town_entrance(town, button)?,
            Some(TownEntry::TownMap { .. }) => self.world.world_map.saved.town = town as i32,
            None => {}
        }
        Ok(entry)
    }
    /// A town-map selection passes the slot's entry selector through global
    /// 32 (not automatically an elevation), makes the town the current town
    /// with the original slot as the current section, and loads the slot's
    /// map by name.
    pub fn select_town_entrance(
        &mut self,
        town: usize,
        button: TownButton,
    ) -> Result<(), WorldError> {
        self.world.world_map.saved.town = town as i32;
        self.world.world_map.saved.section = button.slot as i32;
        self.load_world_map_destination(button.map, button.entry)
    }
    /// SPEC-014 §3.39 leaving to a map: campaign index 32 receives the entry
    /// selector, and the map loads by name. Closing the world map discards a
    /// transition request made during the load (SPEC-013 §3.22).
    pub fn load_world_map_destination(&mut self, map: &str, entry: i32) -> Result<(), WorldError> {
        self.set_global(ENTRY_GLOBAL, entry);
        self.load_map(&format!("maps/{}", map.to_ascii_lowercase()))?;
        self.world_map_closed();
        Ok(())
    }
    /// SPEC-014 §3.39 the campaign checks of every world-map pass, after
    /// any travel iterations:
    ///
    /// 1. With campaign index 10 at 0 and index 101 not 2, `boil3` plays
    ///    and the screen leaves with a quit request.
    /// 2. Otherwise the Military Base vats explode once the clock is 2410
    ///    ticks past index 147 (when nonzero): `vexpld`, index 308 = 2, the
    ///    party travels to Vault 13 when index 18 is set, 10000 experience,
    ///    index 155 + 5 (clamped to -100..100), `worldmap.msg` 500, index
    ///    147 = 0 and index 17 = 1.
    /// 3. Then the Master's lair, by index 55: `cathexp`, index 309 = 2,
    ///    the same travel when index 17 is set, 10000 experience, index 155
    ///    + 10, no message, index 55 = 0 and index 18 = 1.
    ///
    /// The reference takes the tick difference as unsigned 32-bit, so a
    /// stored tick later than the clock fires at once; under the
    /// `[May-improve]` rule a countdown fires only when the clock is at
    /// least 2410 ticks past it. Under the safe bound, check 1 runs only
    /// when the campaign holds both of its indices.
    pub fn world_map_checks(&mut self) -> Result<Option<WorldMapExit>, WorldError> {
        // SPEC-019 §2.7 / SPEC-018 §6.2: Fallout 1's campaign checks do not
        // run under Fallout 2.
        if self.world.fallout2.is_some() {
            return Ok(None);
        }
        let indices = self.world.gvars.as_ref().map_or(0, Vec::len);
        if indices > movie::WATER_CHIP_GLOBAL
            && self.global(WATER_GLOBAL) == 0
            && self.global(movie::WATER_CHIP_GLOBAL) != 2
        {
            self.play_game_movie(movie::BOIL3, WORLD_MOVIE_FLAGS);
            return Ok(Some(WorldMapExit::Quit));
        }
        let mut exit = None;
        let now = i64::try_from(self.world.clock.ticks).map_err(|_| WorldError::Arithmetic)?;
        let due = |host: &Self, index| {
            host.global(index) != 0 && now - i64::from(host.global(index)) >= EXPLOSION_DELAY
        };
        if due(self, VATS_TIMER_GLOBAL) {
            self.play_game_movie(movie::VEXPLD, WORLD_MOVIE_FLAGS);
            self.set_global(VATS_STATUS_GLOBAL, 2);
            if self.global(worldmap::CATHEDRAL_DESTROYED_GLOBAL) != 0 {
                self.scripted_town_travel(0)?;
                exit = Some(WorldMapExit::Vault13);
            }
            self.give_experience(10000);
            self.add_karma(5);
            let text = self
                .assets
                .game_messages("worldmap.msg")
                .ok()
                .and_then(|catalog| catalog.get(500).map(|message| message.text.clone()));
            if let Some(text) = text {
                self.world.emit(WorldEffect::Message(text));
            }
            self.set_global(VATS_TIMER_GLOBAL, 0);
            self.set_global(worldmap::BASE_DESTROYED_GLOBAL, 1);
        }
        if due(self, LAIR_TIMER_GLOBAL) {
            self.play_game_movie(movie::CATHEXP, WORLD_MOVIE_FLAGS);
            self.set_global(LAIR_STATUS_GLOBAL, 2);
            if self.global(worldmap::BASE_DESTROYED_GLOBAL) != 0 {
                self.scripted_town_travel(0)?;
                exit = Some(WorldMapExit::Vault13);
            }
            self.give_experience(10000);
            self.add_karma(10);
            self.set_global(LAIR_TIMER_GLOBAL, 0);
            self.set_global(worldmap::CATHEDRAL_DESTROYED_GLOBAL, 1);
        }
        Ok(exit)
    }
    /// Campaign index 155 plus `amount`, clamped to -100 through 100.
    fn add_karma(&mut self, amount: i32) {
        let value = self.global(KARMA_GLOBAL).saturating_add(amount);
        self.set_global(KARMA_GLOBAL, value.clamp(-100, 100));
    }
    /// SPEC-014 §3.39 a base explosion's exit: `V13ENT.MAP` (map 35) with
    /// entry selector 0.
    pub fn enter_vault13_after_explosion(&mut self) -> Result<(), WorldError> {
        self.load_world_map_destination(VAULT13_ENTRANCE, 0)
    }
}
