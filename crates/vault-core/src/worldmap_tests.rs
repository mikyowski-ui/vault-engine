//! SPEC-008 §3.4-§3.7 world travel and the §4.4 world vectors.
use crate::{
    CharacterSheet, Perk, RandomSource, TravelOutcome, WorldError, WorldHost, WorldState,
    queue::EventKind, worldmap::*,
};
use std::{collections::VecDeque, sync::Arc};
use vault_formats::map::ObjectState;

/// Replays scripted draws, checking each requested range.
struct Draws(VecDeque<i32>);
impl Draws {
    fn new(values: &[i32]) -> Self {
        Self(values.iter().copied().collect())
    }
}
impl RandomSource for Draws {
    fn range(&mut self, first: i32, last: i32) -> Result<i32, WorldError> {
        let value = self.0.pop_front().expect("scripted draw available");
        assert!(
            (first..=last).contains(&value),
            "{value} outside {first}..={last}"
        );
        Ok(value)
    }
}

struct NoAssets;
impl crate::assets::WorldAssets for NoAssets {
    fn prototype(&self, _pid: u32) -> Result<Arc<vault_formats::Prototype>, WorldError> {
        Err(WorldError::Invalid("no prototypes".into()))
    }
    fn program(&self, _index: i32) -> Result<(String, vault_vm::IntProgram), WorldError> {
        Err(WorldError::Invalid("no programs".into()))
    }
    fn message(&self, _list: i32, _id: i32) -> Option<vault_formats::msg::Message> {
        None
    }
}

/// A player with displayed Outdoorsman 50 (base 10 plus 40 invested) and
/// the given Pathfinder ranks, damaged to 10 HP.
fn traveller(pathfinder: u32) -> WorldHost<NoAssets> {
    let mut sheet = CharacterSheet::default();
    sheet.invested[17] = 40;
    if pathfinder > 0 {
        sheet.perks.insert(Perk::Pathfinder, pathfinder);
    }
    sheet.current_hp = 10;
    let mut world = WorldState::new(sheet, 1).unwrap();
    let player = world.player;
    if let Some(ObjectState::Critter(state)) = world.get_mut(player).map(|o| &mut o.raw.state) {
        state.hp = 10;
    }
    WorldHost::new(world, NoAssets)
}

fn hp(host: &WorldHost<NoAssets>) -> i32 {
    host.world.player_sheet().unwrap().current_hp
}

#[test]
fn spec008_fixed_world_grids_towns_and_hotspots() {
    // Row 00 of each group, and region digits A-F as 10-15.
    let digits = |row: &[u8; 28]| row.iter().map(|d| format!("{d:X}")).collect::<String>();
    assert_eq!(digits(&TERRAIN[0]), "1110000001111111111100000000");
    assert_eq!(digits(&FREQUENCY[0]), "3333333332211100001112223333");
    assert_eq!(digits(&REGION[0]), "BBBBBBBBB2222224442255500000");
    assert_eq!(digits(&REGION[13]), "FFFFFF020000002E888099999000");
    assert_eq!(digits(&TERRAIN[29]), "3333333333333333333330000000");
    assert_eq!(terrain([16, 1]), Terrain::Mountain);
    assert_eq!(terrain([0, 6]), Terrain::Coast);
    assert_eq!(terrain([22, 13]), Terrain::City);
    assert_eq!(terrain([20, 2]), Terrain::Desert);
    assert_eq!((frequency([9, 0]), region([0, 0])), (2, 11));
    assert_eq!(cell_of(START), Some([16, 1]));
    assert_eq!(cell_of([1399, 1499]), Some([27, 29]));
    assert_eq!((cell_of([1400, 0]), cell_of([-1, 0])), (None, None));
    assert_eq!(clamp_scroll([-5, 2000]), [0, 1058]);
    assert_eq!(clamp_scroll([960, 10]), [950, 10]);
    assert_eq!(town_at([16, 1]), Some(0));
    assert_eq!(town_at([15, 20]), Some(11));
    assert_eq!(town_at([0, 0]), None);
    assert_eq!(TOWNS[6].entrance_globals, &[576, 577, 578, 579, 580, 581]);
    assert_eq!((TOWNS[8].known_global, TOWNS[8].cell), (78, [3, 1]));
    assert_eq!(
        HOTSPOTS[0][0],
        Hotspot {
            x: 202,
            y: 303,
            entry: 0,
            map: "V13ENT.MAP"
        }
    );
    assert_eq!(hotspots(6).count(), 6);
    assert_eq!(hotspots(4).count(), 1);
    assert_eq!(hotspots(14).next().unwrap().1.map, "BRODEAD.MAP");
    assert_eq!(HOTSPOTS[4][1], Hotspot::EMPTY);
}

#[test]
fn spec008_fixture_asymmetric_stepper() {
    // X-major: the strict comparison stops one Y pixel short of 102.
    let mut stepper = Stepper::new([100, 100], [104, 102]);
    let mut position = [100, 100];
    let mut path = Vec::new();
    while let Some(next) = stepper.step(position) {
        position = next;
        path.push(next);
    }
    assert_eq!(path, [[101, 100], [102, 100], [103, 101], [104, 101]]);
    // Y-major paths reach their target exactly.
    let mut stepper = Stepper::new([0, 0], [-2, 4]);
    let mut position = [0, 0];
    while let Some(next) = stepper.step(position) {
        position = next;
    }
    assert_eq!(position, [-2, 4]);
    assert_eq!(Stepper::new([5, 5], [5, 5]).step([5, 5]), None);
}

#[test]
fn spec008_fixture_travel_speed_binary32() {
    let speed = |s, p| travel_speed(s, p).map(|t| (t.interval, t.ticks)).ok();
    assert_eq!(speed(0, 0), Some((60, 14400)));
    assert_eq!(speed(50, 0), Some((90, 9600)));
    assert_eq!(speed(100, 0), Some((120, 7200)));
    assert_eq!(speed(50, 1), Some((90, 7200)));
    assert_eq!(speed(100, 2), Some((120, 3600)));
    // Capped only above.
    assert_eq!(speed(250, 0), Some((120, 7200)));
    assert_eq!(speed(33, 0), Some((79, 10936)));
    assert_eq!(speed(-50, 0), Some((30, 28800)));
    // Unsafe inputs are rejected rather than normalized.
    assert_eq!(speed(-100, 0), None);
    assert_eq!(speed(50, 5), None);
}

#[test]
fn spec008_terrain_schedules_share_one_counter() {
    let run = |schedule: &mut Schedule, terrain, iterations| {
        let (mut steps, mut charges) = (0, 0);
        for _ in 0..iterations {
            if schedule.iterate(terrain, || {
                steps += 1;
                true
            }) {
                charges += 1;
            }
        }
        (steps, charges)
    };
    let mut schedule = Schedule::default();
    // Entering a city from a cleared counter takes the extra step at once.
    assert_eq!(run(&mut schedule, Terrain::City, 1), (2, 0));
    // Steady city: four iterations, five steps, three time charges.
    assert_eq!(run(&mut schedule, Terrain::City, 4), (5, 3));
    assert_eq!(schedule.counter, 4);
    // The counter carries into mountains: steps when exhausted, every other
    // iteration in steady state, and Q charges on every iteration.
    let mut mountain = Schedule { counter: 4 };
    assert_eq!(run(&mut mountain, Terrain::Mountain, 3), (0, 3));
    assert_eq!(run(&mut mountain, Terrain::Mountain, 1), (1, 1));
    assert_eq!(run(&mut mountain, Terrain::Mountain, 4), (2, 4));
    assert_eq!(run(&mut mountain, Terrain::Desert, 2), (2, 2));
    assert_eq!(mountain.counter, 0);
    assert_eq!(run(&mut mountain, Terrain::Coast, 1), (1, 1));
    // No additional city step once movement stops.
    let mut city = Schedule::default();
    assert!(city.iterate(Terrain::City, || false));
    assert_eq!(city.counter, -1);
}

#[test]
fn spec008_fixture_desert_loop_count_healing_and_encounter_check() {
    let mut host = traveller(1);
    assert_eq!(
        host.travel_speed().map(|s| (s.interval, s.ticks)),
        Ok((90, 7200))
    );
    // Desert row 0, columns 3-8: travel east well past 90 pixels.
    host.world.world_map.set_position([175, 25]);
    host.travel_to([375, 25]);
    let start = host.world.clock.ticks;
    let rng = host.world.rng.state().clone();
    for _ in 0..89 {
        assert_eq!(host.travel_iteration(), Ok(TravelOutcome::Moving));
    }
    assert_eq!(host.world.world_map.position(), [264, 25]);
    assert_eq!(host.world.clock.ticks - start, 89 * 7200);
    assert_eq!(hp(&host), 10);
    assert_eq!(host.world.rng.state(), &rng, "no dice before the check");
    let outcome = host.travel_iteration().unwrap();
    assert!(matches!(
        outcome,
        TravelOutcome::Moving | TravelOutcome::Encounter(_)
    ));
    assert_eq!(host.world.world_map.position(), [265, 25]);
    // 18 game hours, then a 24-hour application: eight healing-rate units.
    assert_eq!(host.world.clock.ticks - start, 648000);
    assert_eq!(hp(&host), 18);
    assert_ne!(host.world.rng.state(), &rng, "encounter dice were drawn");
    assert_eq!(host.world.world_map.interval, 0);
}

#[test]
fn spec008_travel_queue_events_walk_mask_and_scripted_town_travel() {
    let mut host = traveller(0);
    host.world.world_map.set_position([175, 25]);
    host.travel_to([180, 25]);
    let start = host.world.clock.ticks;
    host.world
        .queue
        .insert(start, 100, 0, EventKind::Other(9))
        .unwrap();
    // The event inside the charge is processed; the computed time is kept.
    assert_eq!(host.travel_iteration(), Ok(TravelOutcome::Moving));
    assert_eq!(host.world.queue.next_due(), None);
    assert_eq!(host.world.clock.ticks, start + 9600);
    // A set mask bit restores the prior coordinates and stops; time was
    // still charged.
    let mut mask = vec![0; WalkMask::ROWS * WalkMask::ROW_BYTES];
    mask[25 * WalkMask::ROW_BYTES + 177 / 8] = 1 << (7 - 177 % 8);
    host.world.world_map.walk_mask = Some(Arc::new(WalkMask::new(mask).unwrap()));
    assert_eq!(host.travel_iteration(), Ok(TravelOutcome::Blocked));
    assert_eq!(host.world.world_map.position(), [176, 25]);
    assert_eq!(host.world.clock.ticks, start + 2 * 9600);
    assert_eq!(host.world.world_map.travel, None);
    assert_eq!(host.travel_iteration(), Ok(TravelOutcome::Idle));
    // The mask is consulted only below X 1064 with positive Y.
    let mask = host.world.world_map.walk_mask.clone().unwrap();
    assert!(mask.blocks([177, 25]));
    assert!(!mask.blocks([177, 0]) && !mask.blocks([1064, 25]));
    assert!(WalkMask::new(vec![0; 10]).is_err());

    // Script-directed travel ignores the mask and charges no city time.
    let mut host = traveller(0);
    // Necropolis's cell (22,13) is city; its jittered target stays inside.
    host.world.world_map.set_position([1100, 650]);
    assert_eq!(terrain(cell_of([1100, 650]).unwrap()), Terrain::City);
    host.travel_to_town(5).unwrap();
    let travel = host.world.world_map.travel.unwrap();
    assert!(travel.scripted);
    let [x, y] = travel.destination;
    assert!((1109..=1141).contains(&x) && (659..=691).contains(&y));
    let start = host.world.clock.ticks;
    host.world.world_map.walk_mask = Some(Arc::new(
        WalkMask::new(vec![0xff; WalkMask::ROWS * WalkMask::ROW_BYTES]).unwrap(),
    ));
    assert_eq!(host.travel_iteration(), Ok(TravelOutcome::Moving));
    assert_ne!(host.world.world_map.position(), [1100, 650]);
    assert_eq!(host.world.clock.ticks, start);
    // The final iteration arrives.
    let mut last = TravelOutcome::Moving;
    for _ in 0..200 {
        last = host.travel_iteration().unwrap();
        if last != TravelOutcome::Moving {
            break;
        }
    }
    assert_eq!(last, TravelOutcome::Arrived);
    assert_eq!(host.world.clock.ticks, start);
}

#[test]
fn spec008_fixture_discovery_stride_scout_and_west_reveal() {
    let mut map = WorldMap::default();
    map.set_position([525, 525]);
    map.reveal(false);
    assert_eq!(map.saved.discovery[29 * 10 + 10], 2);
    assert_eq!(map.discovery([10, 10]), 2);
    let ones: Vec<_> = (0..899).filter(|i| map.saved.discovery[*i] == 1).collect();
    assert_eq!(ones.len(), 8);
    for (x, y) in [(9, 9), (11, 11), (9, 11), (10, 9)] {
        assert_eq!(map.discovery([x, y]), 1);
    }
    // Scout reveals 5 x 5 without downgrading the visited cell.
    let mut scout = WorldMap::default();
    scout.set_position([525, 525]);
    scout.reveal(true);
    assert_eq!(
        scout.saved.discovery.iter().filter(|b| **b == 1).count(),
        24
    );
    scout.set_position([575, 525]);
    scout.reveal(false);
    assert_eq!(
        (scout.discovery([10, 10]), scout.discovery([11, 10])),
        (2, 2)
    );
    // Clipped at the playable edge; padding column 28 is untouched.
    let mut edge = WorldMap::default();
    edge.set_position([1375, 1475]);
    edge.reveal(true);
    assert_eq!(edge.discovery([27, 29]), 2);
    assert_eq!(edge.saved.discovery[29 * 29 + 28], 0);
    assert_eq!(edge.saved.discovery.iter().filter(|b| **b != 0).count(), 9);
    // West/ocean threshold for row 10 is 2: column 1 reveals columns 0-1.
    let mut west = WorldMap::default();
    west.set_position([75, 525]);
    west.reveal(false);
    assert_eq!((west.discovery([0, 10]), west.discovery([1, 10])), (2, 2));
    assert_eq!(west.discovery([2, 10]), 1);
    // Reset clears only rows 0-28, columns 0-27.
    let mut reset = WorldMap::default();
    reset.saved.discovery.fill(1);
    reset.reset_discovery();
    assert_eq!(reset.discovery([27, 28]), 0);
    assert_eq!(reset.discovery([0, 29]), 1);
    assert_eq!(reset.saved.discovery[28], 1);
    assert_eq!(reset.saved.discovery[898], 1);
}

#[test]
fn spec008_encounter_dice_specials_and_terrain_choices() {
    let mut map = WorldMap::default();
    // Cell (3,0): desert, frequency class 3 (encounter below 10), region B.
    map.set_position([175, 25]);
    // 3d6 = 10 is no encounter in class 3.
    let mut draws = Draws::new(&[4, 3, 3]);
    assert_eq!(map.check_encounter(&mut draws, 5, 0), Ok(None));
    // 9 is an encounter; special gate 3+3+3-5+5 = 9 < 18 gives ordinary;
    // terrain choice 2 is populated for desert.
    let mut draws = Draws::new(&[3, 3, 3, 3, 3, 3, 2]);
    assert_eq!(
        map.check_encounter(&mut draws, 5, 0),
        Ok(Some(Encounter::Terrain {
            map: "DESERT3.MAP",
            region: 11,
            context: 2
        }))
    );
    // Town cells draw the dice but suppress entry.
    map.set_position(START);
    let mut draws = Draws::new(&[1, 1, 1]);
    assert_eq!(map.check_encounter(&mut draws, 5, 0), Ok(None));
    assert!(draws.0.is_empty());
    // A passing gate (6+6+6-5+1+2x2 = 18) draws 1-100: 31 is TALKCOW.
    map.set_position([175, 25]);
    let mut draws = Draws::new(&[1, 1, 1, 6, 6, 6, 31]);
    assert_eq!(
        map.check_encounter(&mut draws, 1, 2),
        Ok(Some(Encounter::Special {
            map: "TALKCOW.MAP",
            bit: 0x02,
            region: 11
        }))
    );
    assert_eq!(map.saved.specials, 0x02);
    // A consumed special restarts the whole test, including the gate.
    let mut draws = Draws::new(&[1, 1, 1, 6, 6, 6, 45, 6, 6, 6, 100]);
    let encounter = map.check_encounter(&mut draws, 1, 2).unwrap().unwrap();
    assert_eq!((encounter.map(), encounter.context()), ("COLATRUK.MAP", 0));
    assert_eq!(map.saved.specials, 0x22);
    let mut draws = Draws::new(&[1, 1, 1, 6, 6, 6, 50, 1, 1, 1, 0]);
    assert_eq!(
        map.check_encounter(&mut draws, 1, 2)
            .unwrap()
            .unwrap()
            .map(),
        "DESERT1.MAP"
    );
    // All six consumed: ordinary without a 1-100 draw.
    map.saved.specials = ALL_SPECIALS;
    let mut draws = Draws::new(&[1, 1, 1, 6, 6, 6, 1]);
    assert_eq!(
        map.check_encounter(&mut draws, 1, 2)
            .unwrap()
            .unwrap()
            .map(),
        "DESERT2.MAP"
    );
    // Frequency thresholds: 3d6 strictly below 6/7/9/10.
    assert_eq!(ENCOUNTER_BELOW, [6, 7, 9, 10]);
    // City and coast redraw until a populated choice is found.
    let mut draws = Draws::new(&[2, 1, 0]);
    assert_eq!(terrain_map(Terrain::City, &mut draws), Ok("CITY1.MAP"));
    let mut draws = Draws::new(&[2, 1]);
    assert_eq!(terrain_map(Terrain::Coast, &mut draws), Ok("COAST2.MAP"));
    let mut draws = Draws::new(&[2, 0]);
    assert_eq!(
        terrain_map(Terrain::Mountain, &mut draws),
        Ok("MOUNTN1.MAP")
    );
    // Town destinations jitter around the cell center.
    let mut draws = Draws::new(&[-16, 16]);
    assert_eq!(town_target(2, &mut draws), Ok([1059, 91]));
    assert!(town_target(12, &mut Draws::new(&[])).is_err());
}

#[test]
fn spec008_town_knowledge_entrances_and_alternate_rows() {
    let mut gvars = vec![0; 700];
    let mut map = WorldMap::default();
    assert_eq!(map.position(), START);
    assert_eq!(
        (map.saved.town, map.saved.section, map.saved.towns),
        (0, 1, 1)
    );
    assert!(map.entrance_known(0, 0) && !map.entrance_known(0, 1));
    // Known value must equal 1; entrances narrow to a byte first.
    gvars[68] = 1;
    gvars[71] = 2;
    gvars[567] = 256;
    gvars[568] = 257;
    map.refresh_knowledge(&gvars, 0);
    assert!(map.town_enabled(2) && !map.town_enabled(3));
    assert_eq!(map.discovery([21, 1]), 1);
    assert!(map.entrance_known(2, 0));
    assert!(!map.entrance_known(2, 1) && map.entrance_known(2, 2));
    // Vault 13: slot zero only before tick 864000, all four at/after it.
    assert!(!map.entrance_known(0, 3));
    map.refresh_knowledge(&gvars, 864000);
    assert!((0..4).all(|slot| map.entrance_known(0, slot)));
    assert!(!map.entrance_known(0, 4));
    // Alternate rows.
    assert!(!map.entrance_known(12, 0) && !map.entrance_known(14, 0));
    gvars[18] = 1;
    gvars[587] = 1;
    gvars[583] = 1;
    map.refresh_knowledge(&gvars, 0);
    assert!(map.entrance_known(12, 0) && map.entrance_known(13, 0));
    assert!(map.entrance_known(14, 0));
    // A revealed cell is not downgraded from visited.
    map.saved.discovery[29 + 21] = 2;
    map.refresh_knowledge(&gvars, 0);
    assert_eq!(map.discovery([21, 1]), 2);
}

#[test]
fn spec008_town_buttons_labels_and_first_entry() {
    let mut gvars = vec![0; 700];
    let mut map = WorldMap::default();
    // Hub with slots 0 and 2 known: compact events, original slots.
    gvars[73] = 1;
    gvars[578] = 1;
    map.refresh_knowledge(&gvars, 0);
    let buttons = map.town_buttons(6, &gvars);
    assert_eq!(
        buttons,
        [
            TownButton {
                slot: 0,
                event: 514,
                rect: [238 - 57, 78 - 6, 164, 25],
                label: 260,
                map: "HUBENT.MAP",
                entry: 1
            },
            TownButton {
                slot: 2,
                event: 515,
                rect: [128 - 57, 138 - 6, 164, 25],
                label: 262,
                map: "HUBHEIGT.MAP",
                entry: 5
            }
        ]
    );
    // Destroyed Cathedral and Military Base use alternate rows and labels.
    gvars[18] = 1;
    gvars[17] = 1;
    map.refresh_knowledge(&gvars, 0);
    let cathedral = map.town_buttons(CATHEDRAL, &gvars);
    assert_eq!(cathedral.len(), 1);
    assert_eq!(
        (cathedral[0].label, cathedral[0].map, cathedral[0].entry),
        (290, "CHILDEAD.MAP", 0)
    );
    let base = map.town_buttons(MILITARY_BASE, &gvars);
    assert_eq!((base[0].label, base[0].map), (501, "MBDEAD.MAP"));
    // First entry loads slot zero directly and sets the bit.
    let mut fresh = WorldMap::default();
    let gvars = vec![0; 700];
    assert!(!fresh.town_enabled(2));
    let Some(TownEntry::Direct(button)) = fresh.enter_town(2, &gvars) else {
        panic!("first entry loads directly");
    };
    assert_eq!(
        (button.map, button.entry, button.slot),
        ("SHADYW.MAP", 1, 0)
    );
    assert!(fresh.town_enabled(2));
    assert_eq!(
        fresh.enter_town(2, &gvars),
        Some(TownEntry::TownMap {
            picture: 158,
            buttons: Vec::new()
        })
    );
    assert_eq!(fresh.enter_town(12, &gvars), None);
}

#[test]
fn spec008_party_resting_healing_uses_whole_three_hour_units() {
    let mut host = traveller(0);
    host.heal_party(2);
    assert_eq!(hp(&host), 10);
    host.heal_party(5);
    assert_eq!(hp(&host), 11);
    host.heal_party(24);
    assert_eq!(hp(&host), 19);
    host.heal_party(24 * 10);
    let max = host
        .world
        .player_sheet()
        .unwrap()
        .stat(7, host.world.context(host.world.player));
    assert_eq!(hp(&host), max);
    let player = host.world.player;
    let Some(ObjectState::Critter(state)) = host.world.get(player).map(|o| &o.raw.state) else {
        panic!("player critter state");
    };
    assert_eq!(state.hp, max);
}

// SPEC-008 §3.9 Pip-Boy rest and the §4.4 rest vectors.
use crate::host_rest::{RestOutcome, RestRequest, minutes_until_hour, rest_allocation};

fn rest(host: &mut WorldHost<NoAssets>, request: RestRequest) -> RestOutcome {
    host.rest(request, &mut || false).unwrap()
}
fn set_hp(host: &mut WorldHost<NoAssets>, value: i32) {
    let player = host.world.player;
    let object = host.world.get_mut(player).unwrap();
    object.character.as_mut().unwrap().current_hp = value;
    if let ObjectState::Critter(state) = &mut object.raw.state {
        state.hp = value;
    }
}
fn max_hp(host: &WorldHost<NoAssets>) -> i32 {
    host.world
        .player_sheet()
        .unwrap()
        .stat(7, host.world.context(host.world.player))
}

#[test]
fn spec008_fixture_rest_allocation_and_credited_minutes() {
    assert_eq!(rest_allocation(3, 0), Some((0.6875, 0.0, 13.75)));
    assert_eq!(rest_allocation(0, 0), None);
    let (a, n, k) = rest_allocation(0, 10).unwrap();
    assert!((a - (0.25 + 35.0 / 1440.0)).abs() < 1e-12);
    assert!((n - 20.0 * a).abs() < 1e-12 && k.abs() < 1e-12);

    // One three-hour request: 13 executed hour steps credit 13 x 13 = 169.
    let mut host = traveller(0);
    host.open_pipboy();
    let start = host.world.clock.ticks;
    let mut steps = 0;
    let outcome = host
        .rest(RestRequest::Hours(3), &mut || {
            steps += 1;
            false
        })
        .unwrap();
    assert_eq!((outcome, steps), (RestOutcome::Completed, 13));
    assert_eq!(host.world.clock.ticks - start, 108000);
    assert_eq!((host.world.rest_credit, hp(&host)), (169, 10));

    // Eighteen ten-minute requests without reopening: one healing unit.
    let mut host = traveller(0);
    host.open_pipboy();
    let start = host.world.clock.ticks;
    for request in 0..18 {
        assert_eq!(
            rest(&mut host, RestRequest::Minutes(10)),
            RestOutcome::Completed
        );
        assert_eq!(hp(&host), if request < 17 { 10 } else { 11 });
    }
    assert_eq!(host.world.clock.ticks - start, 108000);
    assert_eq!(host.world.rest_credit, 0);
    // Reopening resets the counter.
    host.world.rest_credit = 170;
    host.open_pipboy();
    assert_eq!(host.world.rest_credit, 0);
}

#[test]
fn spec008_fixture_until_healed_repeats_three_hour_phases() {
    // A one-HP deficit: the 3-hour estimate credits only 169, then the
    // sampled-deficit loop heals and performs one more chunk at full HP.
    let mut host = traveller(0);
    let max = max_hp(&host);
    set_hp(&mut host, max - 1);
    host.open_pipboy();
    let start = host.world.clock.ticks;
    assert_eq!(
        rest(&mut host, RestRequest::UntilHealed),
        RestOutcome::Completed
    );
    assert_eq!(hp(&host), max);
    assert_eq!(host.world.clock.ticks - start, 3 * 108000);
    assert_eq!(host.world.rest_credit, 143);
    // Full HP returns immediately.
    let start = host.world.clock.ticks;
    assert_eq!(
        rest(&mut host, RestRequest::UntilHealed),
        RestOutcome::Completed
    );
    assert_eq!(host.world.clock.ticks, start);
    // A zero healing rate has no safe completion guarantee.
    let player = host.world.player;
    host.world
        .get_mut(player)
        .unwrap()
        .character
        .as_mut()
        .unwrap()
        .bonus[14] = -100;
    set_hp(&mut host, 1);
    assert!(host.rest(RestRequest::UntilHealed, &mut || false).is_err());
}

#[test]
fn spec008_rest_sessions_step_like_the_blocking_rest() {
    // Stepping a session one UI step at a time reaches the same state as the
    // blocking rest: 13 hour-phase UI steps for three hours.
    let mut stepped = traveller(0);
    stepped.open_pipboy();
    let start = stepped.world.clock.ticks;
    let mut session = stepped.begin_rest(RestRequest::Hours(3)).unwrap();
    let mut steps = 0;
    let outcome = loop {
        match stepped.advance_rest(&mut session).unwrap() {
            Some(outcome) => break outcome,
            None => steps += 1,
        }
    };
    assert_eq!((outcome, steps), (RestOutcome::Completed, 13));
    assert_eq!(session.outcome(), Some(RestOutcome::Completed));
    assert_eq!(stepped.world.clock.ticks - start, 108000);
    assert_eq!(stepped.world.rest_credit, 169);
    // Until healed matches the blocking rest exactly.
    let mut blocking = traveller(0);
    let mut sessioned = traveller(0);
    for host in [&mut blocking, &mut sessioned] {
        let max = max_hp(host);
        set_hp(host, max - 5);
        host.open_pipboy();
    }
    assert_eq!(
        rest(&mut blocking, RestRequest::UntilHealed),
        RestOutcome::Completed
    );
    let mut session = sessioned.begin_rest(RestRequest::UntilHealed).unwrap();
    while sessioned.advance_rest(&mut session).unwrap().is_none() {}
    assert_eq!(sessioned.world.clock.ticks, blocking.world.clock.ticks);
    assert_eq!(sessioned.world.rest_credit, blocking.world.rest_credit);
    assert_eq!(hp(&sessioned), hp(&blocking));
    // Cancelling (no further advance) after the first hour-phase step leaves
    // its credit unapplied, as a cancelling UI step does.
    let mut host = traveller(0);
    host.open_pipboy();
    let mut session = host.begin_rest(RestRequest::Hours(3)).unwrap();
    assert_eq!(host.advance_rest(&mut session).unwrap(), None);
    assert_eq!(host.world.rest_credit, 0);
    let mut blocking = traveller(0);
    blocking.open_pipboy();
    assert_eq!(
        blocking.rest(RestRequest::Hours(3), &mut || true).unwrap(),
        RestOutcome::Cancelled
    );
    assert_eq!(
        (blocking.world.clock.ticks, blocking.world.rest_credit),
        (host.world.clock.ticks, 0)
    );
    // Zero-length and full-HP requests finish without a UI step.
    let mut session = host.begin_rest(RestRequest::Minutes(0)).unwrap();
    assert_eq!(
        host.advance_rest(&mut session).unwrap(),
        Some(RestOutcome::Completed)
    );
}

#[test]
fn spec008_rest_presets_queue_events_and_cancellation() {
    let at = |hour: u64, minute: u64| hour * 36000 + minute * 600;
    assert_eq!(minutes_until_hour(at(5, 0), 6), 60);
    assert_eq!(minutes_until_hour(at(6, 0), 6), 1440);
    assert_eq!(minutes_until_hour(at(6, 30), 6), 1410);
    assert_eq!(minutes_until_hour(at(23, 15), 0), 45);
    assert_eq!(minutes_until_hour(at(0, 0), 0), 1440);
    assert_eq!(minutes_until_hour(at(12, 59), 18), 301);

    let mut host = traveller(0);
    host.world.clock.set(at(17, 30));
    assert_eq!(
        rest(&mut host, RestRequest::UntilHour(18)),
        RestOutcome::Completed
    );
    assert_eq!(host.world.clock.ticks, at(18, 0));

    // An event inside a step is processed; the endpoint is still exact.
    let start = host.world.clock.ticks;
    host.world
        .queue
        .insert(start, 50000, 0, EventKind::Other(7))
        .unwrap();
    assert_eq!(
        rest(&mut host, RestRequest::Hours(3)),
        RestOutcome::Completed
    );
    assert_eq!(host.world.clock.ticks, start + 108000);
    assert_eq!(host.world.queue.next_due(), None);
    // The final catch-up pass is strict: an event due exactly at the
    // endpoint stays queued, one tick earlier is processed.
    let start = host.world.clock.ticks;
    host.world
        .queue
        .insert(start, 108000, 0, EventKind::Other(7))
        .unwrap();
    assert_eq!(
        rest(&mut host, RestRequest::Hours(3)),
        RestOutcome::Completed
    );
    assert_eq!(host.world.queue.next_due(), Some(start + 108000));
    host.world.queue.retain(|_| false);
    let start = host.world.clock.ticks;
    host.world
        .queue
        .insert(start, 107999, 0, EventKind::Other(7))
        .unwrap();
    assert_eq!(
        rest(&mut host, RestRequest::Hours(3)),
        RestOutcome::Completed
    );
    assert_eq!(host.world.queue.next_due(), None);
    // Escape stops waiting after the current step.
    let start = host.world.clock.ticks;
    assert_eq!(
        host.rest(RestRequest::Hours(6), &mut || true).unwrap(),
        RestOutcome::Cancelled
    );
    assert_eq!(host.world.clock.ticks, start);
}

#[test]
fn spec008_rest_eligibility_attackers_and_team_locations() {
    let mut host = traveller(0);
    let critter =
        Arc::new(vault_formats::Prototype::parse(&crate::fixture_world::prototype(1, 0)).unwrap());
    let living = |host: &mut WorldHost<NoAssets>, hex| {
        let reference = host.world.spawn(critter.clone(), hex, 0).unwrap();
        let object = host.world.get_mut(reference).unwrap();
        object.character.as_mut().unwrap().current_hp = 10;
        if let ObjectState::Critter(state) = &mut object.raw.state {
            state.hp = 10;
        }
        reference
    };
    let other = living(&mut host, 20500);
    if let Some(ObjectState::Critter(state)) = host.world.get_mut(other).map(|o| &mut o.raw.state) {
        state.team = 7;
    }
    // Town 0 omits the team check.
    assert_eq!(host.world.world_map.saved.town, 0);
    assert!(host.rest_allowed());
    // Other towns check teams regardless of distance or visibility.
    host.world.world_map.saved.town = 5;
    assert!(!host.rest_allowed());
    assert_eq!(rest(&mut host, RestRequest::Hours(1)), RestOutcome::Refused);
    // Map 17 omits it; map 3 omits it above elevation zero only.
    host.world.map.header.map_number = 17;
    assert!(host.rest_allowed());
    host.world.map.header.map_number = 3;
    assert!(!host.rest_allowed());
    host.world.elevation = 1;
    assert!(host.rest_allowed());
    // Forced maps check even in exempt towns.
    host.world.elevation = 0;
    host.world.world_map.saved.town = 0;
    host.world.map.header.map_number = 35;
    assert!(!host.rest_allowed());
    // Only the current elevation counts.
    host.world.get_mut(other).unwrap().raw.elevation = 1;
    assert!(host.rest_allowed());
    // A critter last attacked by the player always blocks; dead ones never.
    host.world.map.header.map_number = 0;
    let friend = living(&mut host, 20600);
    host.world.get_mut(friend).unwrap().opponent = host.world.player;
    assert!(!host.rest_allowed());
    if let Some(ObjectState::Critter(state)) = host.world.get_mut(friend).map(|o| &mut o.raw.state)
    {
        state.results.0 |= crate::critical::effect::DEAD;
    }
    assert!(host.rest_allowed());
}
