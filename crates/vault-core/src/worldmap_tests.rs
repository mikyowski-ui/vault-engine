//! SPEC-008 §3.4-§3.7 world travel and the §4.4 world vectors.
use crate::test_support::{self as support, Draws, TestAssets as NoAssets};
use crate::{
    CharacterSheet, Perk, TravelOutcome, WorldHost, WorldState, queue::EventKind, worldmap::*,
};
use std::sync::Arc;
use vault_formats::map::ObjectState;

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
    support::set_hp(&mut world, player, 10);
    WorldHost::new(world, NoAssets::default())
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
    assert_eq!(speed(250, 0), Some((120, 7200)));
    assert_eq!(speed(33, 0), Some((79, 10936)));
    assert_eq!(speed(100, 4), Some((120, 0)));
    // Safe rule: Outdoorsman clamps below at 0 and Pathfinder to 0-4.
    assert_eq!(speed(-50, 0), Some((60, 14400)));
    assert_eq!(speed(-100, 0), Some((60, 14400)));
    assert_eq!(speed(50, 5), Some((90, 0)));
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
}

#[test]
fn spec008_script_directed_town_travel_runs_to_completion_without_reveal() {
    // SPEC-008 §3.4: cells (21,13) and Necropolis's (22,13) are city, and
    // its jittered target stays inside its cell: no walk-mask checks, no
    // city time.
    let mut host = traveller(0);
    host.world.world_map.set_position([1075, 675]);
    assert_eq!(terrain([21, 13]), Terrain::City);
    assert_eq!(terrain([22, 13]), Terrain::City);
    host.world.world_map.walk_mask = Some(Arc::new(
        WalkMask::new(vec![0xff; WalkMask::ROWS * WalkMask::ROW_BYTES]).unwrap(),
    ));
    let start = host.world.clock.ticks;
    host.scripted_town_travel(5).unwrap();
    let [x, y] = host.world.world_map.position();
    assert!((1109..=1141).contains(&x) && (659..=691).contains(&y));
    assert_eq!(host.world.world_map.cell(), Some([22, 13]));
    assert_eq!(host.world.clock.ticks, start);
    assert_eq!(host.world.world_map.travel, None, "no travel state is kept");
    // Already in the town's cell, and town -1 or out of range: nothing.
    let position = host.world.world_map.position();
    for town in [5, -1, 12] {
        host.scripted_town_travel(town).unwrap();
        assert_eq!(host.world.world_map.position(), position);
    }

    // §4.4: from the initial position to Shady Sands (town 2, cell 21,1)
    // the target lies in X 1059-1091 and Y 59-91, so the trip stays in row
    // 1, crosses columns 17-20, and changes no discovery byte. Time is added
    // directly: the due event is left for normal queue processing.
    let mut host = traveller(1);
    host.world.world_map.reset_discovery();
    let before = host.world.world_map.saved.discovery.clone();
    let start = host.world.clock.ticks;
    host.world
        .queue
        .insert(start, 100, 0, EventKind::Other(9))
        .unwrap();
    host.world.world_map.interval = 50;
    host.world.world_map.schedule.counter = 3;
    host.scripted_town_travel(2).unwrap();
    let [x, y] = host.world.world_map.position();
    assert!((1059..=1091).contains(&x) && (59..=91).contains(&y));
    assert_eq!(host.world.world_map.cell(), Some([21, 1]));
    assert_eq!(host.world.world_map.saved.discovery, before);
    assert!(host.world.clock.ticks > start + 100);
    assert!(host.world.queue.next_due().is_some(), "no queue processing");
    // W and Q are recomputed; the shared terrain counter is untouched.
    assert_eq!(
        host.world.world_map.speed.map(|s| (s.interval, s.ticks)),
        Some((90, 7200))
    );
    assert_eq!(host.world.world_map.schedule.counter, 3);
    assert!(host.world.world_map.interval < 90);
}

#[test]
fn spec008_script_directed_travel_heals_every_w_iterations_without_encounters() {
    // A long trip, Vault 13 to the Glow (town 9, cell 24,25): 24-hour
    // healing still applies every W iterations, the W-iteration counter
    // starts at zero, and no encounter dice are drawn (only the four
    // jitter draws use the generator).
    let mut host = traveller(0);
    host.world.world_map.reset_discovery();
    host.world.world_map.interval = 59;
    let rng = host.world.rng.clone();
    host.scripted_town_travel(9).unwrap();
    assert_eq!(host.world.world_map.cell(), Some([24, 25]));
    assert!(hp(&host) > 10, "healed on the way");
    assert!(host.world.world_map.saved.discovery.iter().all(|b| *b == 0));
    let mut expected = rng;
    town_target(9, &mut expected).unwrap();
    assert_eq!(host.world.rng, expected, "only the four jitter draws");
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
    // Safe rule: reset clears all 31 x 29 bytes, including the last
    // playable row and the padding.
    let mut reset = WorldMap::default();
    reset.saved.discovery.fill(1);
    reset.reset_discovery();
    assert_eq!(reset.discovery([27, 28]), 0);
    assert_eq!(reset.discovery([0, 29]), 0);
    assert!(reset.saved.discovery.iter().all(|cell| *cell == 0));
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
    // Town destinations jitter around the cell center: magnitude then sign
    // on each axis, sign 0 subtracting and 1 adding.
    let mut draws = Draws::new(&[16, 0, 16, 1]);
    assert_eq!(town_target(2, &mut draws), Ok([1059, 91]));
    assert!(draws.0.is_empty());
    let mut draws = Draws::new(&[0, 0, 5, 0]);
    assert_eq!(town_target(2, &mut draws), Ok([1075, 70]));
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
    // Known value must equal 1; entrances test the full 32-bit value.
    gvars[68] = 1;
    gvars[71] = 2;
    gvars[567] = 256;
    gvars[568] = 257;
    map.refresh_knowledge(&gvars, 0);
    assert!(map.town_enabled(2) && !map.town_enabled(3));
    assert_eq!(map.discovery([21, 1]), 1);
    assert!(map.entrance_known(2, 0));
    assert!(map.entrance_known(2, 1) && map.entrance_known(2, 2));
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
    support::set_hp(&mut host.world, player, value);
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
    // Safe rule: a nonpositive healing rate performs no rest at all.
    let player = host.world.player;
    host.world
        .get_mut(player)
        .unwrap()
        .character
        .as_mut()
        .unwrap()
        .bonus[14] = -100;
    set_hp(&mut host, 1);
    assert_eq!(
        rest(&mut host, RestRequest::UntilHealed),
        RestOutcome::Completed
    );
    assert_eq!(host.world.clock.ticks, start);
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
        support::set_hp(&mut host.world, reference, 10);
        reference
    };
    let other = living(&mut host, 20500);
    if let Some(ObjectState::Critter(state)) = host.world.get_mut(other).map(|o| &mut o.raw.state) {
        state.team = 7;
    }
    // SPEC-008 §3.10: the town is the current map's fixed table town. Town
    // 0 (VAULT13.MAP, map 6) omits the team check; the world-map component
    // plays no part.
    host.world.map.header.map_number = 6;
    host.world.world_map.saved.town = 5;
    assert!(host.rest_allowed());
    // Other towns check teams regardless of distance or visibility
    // (HOTEL.MAP, map 4, is Necropolis).
    host.world.map.header.map_number = 4;
    host.world.world_map.saved.town = 0;
    assert!(!host.rest_allowed());
    assert_eq!(rest(&mut host, RestRequest::Hours(1)), RestOutcome::Refused);
    // §4.5: map 34 (Cathedral) checks teams, map 36 (the Hub) does not.
    host.world.map.header.map_number = 34;
    assert!(!host.rest_allowed());
    host.world.map.header.map_number = 36;
    assert!(host.rest_allowed());
    // Map 17 omits it; map 3 omits it above elevation zero only.
    host.world.map.header.map_number = 17;
    assert!(host.rest_allowed());
    host.world.map.header.map_number = 3;
    assert!(!host.rest_allowed());
    host.world.elevation = 1;
    assert!(host.rest_allowed());
    // Forced maps check even in exempt towns (V13ENT.MAP, town 0).
    host.world.elevation = 0;
    host.world.map.header.map_number = 35;
    assert!(!host.rest_allowed());
    // An unnumbered map has town -1, the team-checking default.
    host.world.map.header.map_number = -1;
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

#[test]
fn spec008_map_table_gives_every_number_a_town_and_a_track() {
    use crate::worldmap::{
        MAP_COUNT, MAP_TABLE, TOWNS, city_music, map_table_name, map_town, town_center,
    };
    assert_eq!(MAP_TABLE.len(), MAP_COUNT);
    // SPEC-008 §3.10 town lookup: the interior maps no hotspot names, the
    // encounter and caravan maps (town 1, except 63-65 with town 0), and a
    // few hotspot maps.
    for (number, town) in [
        (9, 5),
        (16, 2),
        (18, 11),
        (31, 8),
        (32, 8),
        (33, 11),
        (34, 11),
        (43, 9),
        (65, 0),
        (63, 0),
        (64, 0),
        (0, 1),
        (23, 1),
        (52, 1),
        (62, 1),
        (6, 0),
        (35, 0),
        (36, 6),
        (47, 11),
        (55, 7),
    ] {
        assert_eq!(map_town(number), town, "map {number}");
    }
    assert!(MAP_TABLE.iter().all(|entry| entry.town < TOWNS.len()));
    assert_eq!(map_town(-1), -1);
    assert_eq!(map_town(66), -1);
    // City music, grouped as §3.10 lists the tracks.
    let groups: [(&str, &[i32]); 15] = [
        ("01hub", &[36, 38, 39, 40, 41, 65]),
        ("02mstrlr", &[33, 34]),
        ("04brthrh", &[13, 14, 15]),
        ("05raider", &[24]),
        ("06vault", &[6, 35]),
        (
            "07desert",
            &[
                0, 1, 2, 20, 21, 22, 23, 49, 50, 51, 52, 53, 54, 56, 57, 58, 59, 60, 61, 62, 63, 64,
            ],
        ),
        ("08vats", &[30, 31, 32, 47, 48, 55]),
        ("09glow", &[27, 42, 43]),
        ("10labone", &[28, 44, 45, 46]),
        ("11chilrn", &[17, 18, 19]),
        ("12junktn", &[10, 11, 12]),
        ("13carvrn", &[7, 8, 16, 37]),
        ("14necro", &[3, 4, 5, 9]),
        ("15shady", &[25, 26]),
        ("16follow", &[29]),
    ];
    let mut covered = 0;
    for (track, numbers) in groups {
        for number in numbers {
            assert_eq!(city_music(*number), Some(track), "map {number}");
            covered += 1;
        }
    }
    assert_eq!(covered, MAP_COUNT);
    assert_eq!(city_music(-1), None);
    assert_eq!(map_table_name(0), Some("DESERT1.MAP"));
    assert_eq!(map_table_name(36), Some("HUBENT.MAP"));
    assert_eq!(map_table_name(65), Some("HUBMIS1.MAP"));
    assert_eq!(map_table_name(66), None);
    assert_eq!(town_center(3), Some([17 * 50 + 25, 10 * 50 + 25]));
}

#[test]
fn spec008_music_on_off_state_follows_the_key_and_the_volumes() {
    use crate::worldmap::MusicSwitch;
    // Rule 1: off at startup; the `music` key (default 1) turns it on.
    assert!(MusicSwitch::startup(true, 22281, 22281).on());
    assert!(MusicSwitch::default().on());
    // Rule 2 from configuration: a startup volume of 0 keeps it off.
    assert!(!MusicSwitch::startup(true, 0, 22281).on());
    assert!(!MusicSwitch::startup(true, 22281, 0).on());
    // `music=0`: off for the whole run, whatever the volumes.
    let mut never = MusicSwitch::startup(false, 22281, 22281);
    assert!(!never.on() && !never.configured());
    assert_eq!(never.set_volumes(32767, 32767), None);
    assert!(!never.on());

    let mut switch = MusicSwitch::startup(true, 22281, 22281);
    // Rule 2: a music or master volume of 0 turns it off by volume.
    assert_eq!(switch.set_volumes(22281, 0), Some(false));
    assert_eq!(switch.set_volumes(22281, 0), None, "already off");
    // Rule 3: master set nonzero while music volume is 0 stays off; music
    // volume set nonzero turns it back on.
    assert_eq!(switch.set_volumes(30000, 0), None);
    assert_eq!(switch.set_volumes(30000, 5), Some(true));
    assert_eq!(switch.set_volumes(0, 5), Some(false));
    // Music volume set again while master is 0: on, then off at once.
    assert_eq!(switch.set_volumes(0, 9000), None);
    assert!(!switch.on());
    assert_eq!(switch.set_volumes(1, 9000), Some(true));
    // Rule 4: dialogue's halving of a music volume of 1 gives 0.
    let halved = 1 / 2;
    assert_eq!(switch.set_volumes(1, halved), Some(false));
    assert_eq!(switch.set_volumes(1, 1), Some(true));
}

#[test]
fn spec008_map_numbers_come_from_the_header_name_matched_against_map_msg() {
    use crate::worldmap::{
        MAP_TABLE, canonical_map_name, city_music, header_map_number, map_number_of_name, map_town,
    };
    use vault_formats::{MsgCatalog, automap};
    let text: String = MAP_TABLE
        .iter()
        .enumerate()
        .map(|(number, entry)| format!("{{{number}}}{{}}{{{}}}\n", entry.name))
        .collect();
    let catalog = MsgCatalog::parse(text.as_bytes()).unwrap();
    let field = |name: &str| {
        let mut field = [0_u8; 16];
        field[..name.len()].copy_from_slice(name.as_bytes());
        field
    };
    // SPEC-008 §4.5: header name, word 0x34, number, town, music, and
    // whether leaving writes a working `.SAV`.
    for (name, word, number, town, music, saves) in [
        ("hubent.map", 7, 36, 6, Some("01hub"), true),
        ("HUBENT.SAV", -1, 36, 6, Some("01hub"), true),
        ("hubent.sav", 7, 7, 1, Some("13carvrn"), true),
        ("MSTRLR34.MAP", 34, 34, 11, Some("02mstrlr"), true),
        ("DESCRVN4.MAP", 63, 63, 0, Some("07desert"), false),
        ("CARAVAN.MAP", -1, -1, -1, None, false),
    ] {
        let assigned = header_map_number(&field(name), word, Some(&catalog));
        assert_eq!(assigned, number, "{name}");
        assert_eq!(map_town(assigned), town, "{name}");
        assert_eq!(city_music(assigned), music, "{name}");
        let written =
            usize::try_from(assigned).is_ok_and(|number| !automap::EXCLUDED.contains(&number));
        assert_eq!(written, saves, "{name}");
    }
    // `TMP$MAP#.MAP` keeps the word; without `map.msg` nothing matches.
    assert_eq!(
        header_map_number(&field("TMP$MAP#.MAP"), 12, Some(&catalog)),
        12
    );
    assert_eq!(header_map_number(&field("hubent.map"), 7, None), -1);
    assert_eq!(canonical_map_name(b"hubent.sav\0\0"), b"HUBENT.MAP");
    assert_eq!(canonical_map_name(b"a.sav.sav"), b"A.MAP.SAV");
    // SPEC-005 0x80E4 names match the same way: the corpus scripts name
    // `Watrshd.map` (map 5) and `master34.map` (unlisted, no transition).
    assert_eq!(map_number_of_name(b"Watrshd.map", Some(&catalog)), 5);
    assert_eq!(map_number_of_name(b"master34.map", Some(&catalog)), -1);
    assert_eq!(map_number_of_name(b"HUBENT", Some(&catalog)), -1);
    // SPEC-008 §3.10 the `load_map` string rule: nothing else is
    // normalized, and (safe rule) 16 or more bytes match nothing.
    for (name, number) in [
        (b"hubent.map".as_slice(), 36),
        (b"HUBENT.MAP", 36),
        (b"hubent.sav", 36),
        (b"HUBENT", -1),
        (b"hubent", -1),
        (b"HUBENT.MAP ", -1),
        (b"MAPS\\HUBENT.MAP", -1),
        (b"HUBENT.MAP\0\0\0\0\0\0\0", 36),
        (b"HUBENT.MAP.....", -1),
        (b"HUBENT.MAP......", -1),
    ] {
        assert_eq!(
            load_map_number(name, Some(&catalog)),
            number,
            "{}",
            String::from_utf8_lossy(name)
        );
    }
}

#[test]
fn spec008_fixture_working_names_and_save_dat_field() {
    use crate::persistence::{requested_file_name, save_dat_map_file, saved_map_name, saved_probe};
    // SPEC-008 §3.10: the file name is the uppercased final component; a
    // `.MAP` name probes its `.SAV` copy; the stem ends at the first `.`.
    assert_eq!(requested_file_name("maps/hubent.map"), "HUBENT.MAP");
    assert_eq!(requested_file_name(r"MAPS\vaultent.sav"), "VAULTENT.SAV");
    assert_eq!(saved_probe("HUBENT.MAP").as_deref(), Some("HUBENT.SAV"));
    assert_eq!(saved_probe("A.MAP.MAP").as_deref(), Some("A.SAV.MAP"));
    assert_eq!(saved_probe("HUBENT.SAV"), None);
    assert_eq!(saved_map_name(b"HUBENT.MAP").as_deref(), Some("HUBENT.SAV"));
    assert_eq!(saved_map_name(b"A.B.MAP").as_deref(), Some("A.SAV"));
    assert_eq!(saved_map_name(b"NOEXT").as_deref(), Some("NOEXT.SAV"));
    assert_eq!(save_dat_map_file("VAULTENT.SAV"), "VAULTENT.sav");
    assert_eq!(save_dat_map_file("HUBENT.SAV"), "HUBENT.sav");
}

#[test]
fn spec008_fixture_hub_jitter_takes_four_draws() {
    // §4.4: the Hub, town 6 at cell 17,14, centers on (875, 725).
    assert_eq!(town_center(6), Some([875, 725]));
    let mut draws = Draws::new(&[5, 0, 16, 1]);
    assert_eq!(town_target(6, &mut draws), Ok([870, 741]));
    assert!(draws.0.is_empty());
    let mut draws = Draws::new(&[0, 1, 0, 0]);
    assert_eq!(town_target(6, &mut draws), Ok([875, 725]));
}

#[test]
fn spec014_fixture_i_world_view_geometry() {
    use crate::worldmap::screen::{self, Marker};
    // The party at the start, Vault 13's cell, follow mode on.
    let view = screen::centered(START);
    assert_eq!(view, [578, 0]);
    let party = screen::window(START, view);
    assert_eq!(party, [269, 96]);
    assert_eq!(Marker::Triangle.origin(party), [257, 90]);
    // Visible cells: columns 11-20, rows 0-8, clipped to the view.
    let visible: Vec<_> = (0..COLUMNS)
        .flat_map(|c| (0..ROWS).map(move |r| [c, r]))
        .filter(|cell| screen::cell_rect(*cell, view).is_some())
        .collect();
    assert!(
        visible
            .iter()
            .all(|[c, r]| (11..=20).contains(c) && *r <= 8)
    );
    assert_eq!(visible.len(), 10 * 9);
    assert_eq!(screen::cell_rect([11, 0], view), Some([22, 21, 22, 50]));
    assert_eq!(screen::cell_rect([12, 1], view), Some([44, 71, 50, 50]));
    assert_eq!(screen::cell_rect([20, 8], view), Some([444, 421, 28, 42]));
    assert_eq!(screen::cell_rect([16, 1], view), Some([244, 71, 50, 50]));
    // The opening reveal without Scout: (16,1) visited, its eight
    // neighbours seen, nothing else (row 1's ocean threshold is 0).
    let mut map = WorldMap::default();
    map.reset_discovery();
    map.reveal(false);
    for cell in &visible {
        let near = (15..=17).contains(&cell[0]) && cell[1] <= 2;
        let expected = match (*cell == [16, 1], near) {
            (true, _) => 2,
            (false, true) => 1,
            _ => 0,
        };
        assert_eq!(map.discovery(*cell), expected, "{cell:?}");
    }
    // Hover.
    assert!(screen::hovers([279, 95], party));
    assert!(!screen::hovers([279, 100], party));
    assert!(screen::hovers([273, 103], party));
    // The "Vault 13" label (width 50) fills (245, 79) to (294, 88).
    assert_eq!(screen::label_origin(party, 50), [245, 79]);
    assert!(screen::label_shown([245, 79], 50));
    assert!(!screen::label_shown([245, 11], 50));
    assert!(!screen::label_shown([-28, 79], 50));
    // A destination click at (400, 300): world (956, 279), cell (19, 5).
    assert!(screen::view_click([400, 300]));
    assert!(!screen::view_click([22, 300]) && !screen::view_click([472, 300]));
    let target = screen::world_at([400, 300], view);
    assert_eq!(target, [956, 279]);
    assert_eq!(cell_of(target), Some([19, 5]));
    assert_eq!(town_at([19, 5]), None);
    // Edge scrolling: east blocked at vx 950; only north blocked at (900,
    // 0), moving to (916, 0); both blocked at (950, 0).
    assert_eq!(screen::edge([639, 240]), [1, 0]);
    assert_eq!(screen::scroll([950, 0], [1, 0]), ([950, 0], true));
    assert_eq!(screen::edge([639, 0]), [1, -1]);
    assert_eq!(screen::scroll([900, 0], [1, -1]), ([916, 0], false));
    assert_eq!(screen::scroll([950, 0], [1, -1]), ([950, 0], true));
    assert_eq!(screen::cursor_index([1, 0]), Some(3));
    assert_eq!(screen::cursor_index([1, -1]), Some(2));
    assert_eq!(screen::edge([2, 2]), [0, 0]);
    assert_eq!(screen::edge([1, 478]), [-1, 1]);
    // Home centers, then vy 0 (safe rule; the reference ends at 16); End.
    assert_eq!(screen::home(START), [578, 0]);
    assert_eq!(screen::end([578, 0]), [578, 1058]);
    // Marker windows.
    assert!(Marker::Triangle.shown([-2, 9]) && !Marker::Triangle.shown([-3, 9]));
    assert!(Marker::Triangle.shown([483, 462]) && !Marker::Triangle.shown([484, 462]));
    assert!(Marker::Plus.shown([18, 17]) && !Marker::Plus.shown([17, 17]));
    assert!(Marker::Crosshair.shown([473, 11]) && !Marker::Crosshair.shown([473, 10]));
    assert!(Marker::Bolt.shown([471, 462]) && !Marker::Bolt.shown([472, 462]));
    // The ordinary encounter bolt at (269, 96) is drawn at (266, 91).
    assert_eq!(Marker::Bolt.origin(party), [266, 91]);
    assert_eq!(screen::FLASH_IMAGES * screen::FLASH_MS, 2786);
    assert_eq!(screen::PASS_MS, 41);
    // Side panel.
    assert_eq!(screen::town_button(0), Some([508, 61, 15, 15]));
    assert_eq!(screen::town_button(11), Some([508, 367, 15, 15]));
    assert_eq!(screen::town_button(12), None);
    assert_eq!(screen::town_world_button(), [520, 439, 15, 15]);
}

#[test]
fn spec014_fixture_i_town_map_geometry() {
    use crate::worldmap::screen;
    let mut gvars = vec![0; 700];
    let mut map = WorldMap::default();
    // The Hub with slots 0, 1 and 3 known: events 514-516.
    gvars[73] = 1;
    gvars[577] = 1;
    gvars[579] = 1;
    map.refresh_knowledge(&gvars, 0);
    let buttons = map.town_buttons(6, &gvars);
    let summary: Vec<_> = buttons
        .iter()
        .map(|b| (b.event, b.slot, b.map, b.entry))
        .collect();
    assert_eq!(
        summary,
        [
            (514, 0, "HUBENT.MAP", 1),
            (515, 1, "HUBDWNTN.MAP", 3),
            (516, 3, "HUBOLDTN.MAP", 2)
        ]
    );
    // Slot 3: button (249, 131), triangle (318, 143), label "Old Town"
    // (width 52) at x 249 + 56.
    assert_eq!(buttons[2].rect, [249, 131, 164, 25]);
    assert_eq!(screen::marker_triangle(buttons[2].rect), [318, 143]);
    assert_eq!(screen::marker_label_x(52), 56);
    assert_eq!(screen::marker_label_x(200), 0);
    // Vault 13: one marker before tick 864000, four from it on.
    map.refresh_knowledge(&gvars, 863_999);
    assert_eq!(map.town_buttons(0, &gvars).len(), 1);
    assert_eq!(map.town_buttons(0, &gvars)[0].map, "V13ENT.MAP");
    map.refresh_knowledge(&gvars, 864_000);
    assert_eq!(map.town_buttons(0, &gvars).len(), 4);
    // A destroyed Cathedral: one marker, triangle (247, 307), "Crater"
    // (width 40) at x 178 + 62, loading CHILDEAD.MAP with selector 0.
    gvars[18] = 1;
    map.refresh_knowledge(&gvars, 0);
    let cathedral = map.town_buttons(CATHEDRAL, &gvars);
    assert_eq!(cathedral.len(), 1);
    assert_eq!(screen::marker_triangle(cathedral[0].rect), [247, 307]);
    assert_eq!(cathedral[0].rect[0] + screen::marker_label_x(40), 240);
    assert_eq!((cathedral[0].map, cathedral[0].entry), ("CHILDEAD.MAP", 0));
    // A destroyed Military Base: triangle (118, 70), "Ruins", MBDEAD.MAP.
    gvars[17] = 1;
    map.refresh_knowledge(&gvars, 0);
    let base = map.town_buttons(MILITARY_BASE, &gvars);
    assert_eq!(screen::marker_triangle(base[0].rect), [118, 70]);
    assert_eq!(
        (base[0].label, base[0].map, base[0].entry),
        (501, "MBDEAD.MAP", 0)
    );
    // Vault 15: slot 0 is VAULTENT.MAP (selector 0), slots 1-3 VAULTBUR.
    let vault15: Vec<_> = hotspots(1).map(|(_, h)| (h.map, h.entry)).collect();
    assert_eq!(
        vault15,
        [
            ("VAULTENT.MAP", 0),
            ("VAULTBUR.MAP", 1),
            ("VAULTBUR.MAP", 2),
            ("VAULTBUR.MAP", 3)
        ]
    );
    assert_eq!(screen::town_picture(1).as_deref(), Some("twnmap01.frm"));
    assert_eq!(screen::town_picture(12), None);
}

#[test]
fn spec014_world_map_build_reveals_and_marks_iterations_one_four_seven() {
    // Building the screen reveals the party's cell and restarts the count;
    // travel then marks (and reveals) on iterations 1, 4, 7 and 10.
    let mut host = traveller(0);
    host.world.world_map.reset_discovery();
    host.world.world_map.set_position([175, 25]);
    host.build_world_map(true).unwrap();
    assert_eq!(host.world.world_map.discovery([3, 0]), 2);
    assert_eq!(host.world.world_map.speed.map(|s| s.interval), Some(90));
    host.travel_to([375, 25]);
    let marked: Vec<_> = (1..=10)
        .filter(|_| {
            host.travel_iteration().unwrap();
            host.world.world_map.mark.is_some()
        })
        .collect();
    assert_eq!(marked, [1, 4, 7, 10]);
    // A rebuild restarts the count.
    host.build_world_map(false).unwrap();
    host.travel_iteration().unwrap();
    assert!(host.world.world_map.mark.is_some());
}

#[test]
fn spec014_world_map_campaign_checks() {
    use crate::WorldMapExit;
    let mut host = traveller(0);
    // No campaign: nothing.
    assert_eq!(host.world_map_checks(), Ok(None));
    // Water at 0 with the chip quest not done: quit (boil3 is not in this
    // asset set, so it only fails to play).
    let mut gvars = vec![0; 700];
    gvars[10] = 0;
    gvars[101] = 1;
    host.world.gvars = Some(gvars.clone());
    assert_eq!(host.world_map_checks(), Ok(Some(WorldMapExit::Quit)));
    gvars[101] = 2;
    // The vats at tick 1000 fire 2410 ticks later.
    gvars[147] = 1000;
    gvars[155] = 98;
    host.world.gvars = Some(gvars.clone());
    host.world.clock.set(3409);
    assert_eq!(host.world_map_checks(), Ok(None));
    assert_eq!(host.world.gvars.as_ref().unwrap()[147], 1000);
    host.world.clock.set(3410);
    let experience = host.world.player_sheet().unwrap().experience;
    assert_eq!(host.world_map_checks(), Ok(None));
    let g = host.world.gvars.clone().unwrap();
    assert_eq!((g[147], g[17], g[308], g[155]), (0, 1, 2, 100));
    assert_eq!(
        host.world.player_sheet().unwrap().experience,
        experience + 10000
    );
    // Under the safe rule a stored tick later than the clock waits.
    host.world.gvars.as_mut().unwrap()[55] = 5000;
    assert_eq!(host.world_map_checks(), Ok(None));
    // Both in one pass: step 2 sees index 18 at 0, step 3 sees the index
    // 17 just set, so only the lair moves the party to Vault 13.
    let mut host = traveller(0);
    let mut gvars = vec![0; 700];
    gvars[10] = 5;
    gvars[147] = 1;
    gvars[55] = 1;
    host.world.gvars = Some(gvars);
    host.world.world_map.set_position([1225, 1275]);
    host.world.clock.set(10_000);
    assert_eq!(host.world_map_checks(), Ok(Some(WorldMapExit::Vault13)));
    assert_eq!(host.world.world_map.cell(), Some([16, 1]));
    let g = host.world.gvars.clone().unwrap();
    assert_eq!((g[55], g[18], g[309], g[155]), (0, 1, 2, 15));
}
