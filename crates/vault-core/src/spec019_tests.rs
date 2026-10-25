//! SPEC-019 §4 vectors K-O through the host: party-member level-ups
//! (§3.17), the car's fuel, speed and place (§3.19), city containment and
//! town-map placement (§3.12-§3.13), and party counts and the Fallout 2
//! Push (§3.6, §3.14), with the dead-member and Fallout 1 checks.
use crate::{
    CharacterSheet, Fallout2Travel, ObjectAction, WorldEffect, WorldHost, WorldState,
    fallout2::{self, Car, Fallout2Tables, Fallout2World, Step},
    fallout2_party as party,
    host_fallout2_party::Push2,
    test_support::{TestAssets, patched, set_hp, special},
    world::ObjectRef,
};
use std::sync::Arc;
use vault_formats::{HexGrid, fallout2::save::MemberRecord, pro::BasicFlags};

fn load(files: Vec<(&'static str, String)>) -> Arc<Fallout2Tables> {
    let mut read = move |path: &str| -> Result<Option<Vec<u8>>, String> {
        Ok(files
            .iter()
            .find(|(name, _)| *name == path)
            .map(|(_, text)| text.as_bytes().to_vec()))
    };
    Arc::new(Fallout2Tables::load(&mut read).unwrap())
}
/// §4.13's tables: maps 20 and 21 in city 4, map 30 in city 7, map 40 in
/// city 9, map 90 in none; and a party record for PID `0x01000050` with
/// L0 = 3, E = 3 and three level PIDs.
fn tables() -> Arc<Fallout2Tables> {
    let mut maps = String::new();
    for map in [20, 21, 30, 40, 90] {
        maps.push_str(&format!(
            "[Map {map:03}]\nlookup_name=Map {map}\nmap_name=map{map}\n"
        ));
    }
    let mut city = String::new();
    for (area, entrances) in [(4, vec![20, 21]), (7, vec![30]), (9, vec![40])] {
        city.push_str(&format!(
            "[Area {area:02}]\narea_name=City {area}\nworld_pos={},{}\nsize=Medium\n",
            100 + 30 * area,
            100
        ));
        for (slot, map) in entrances.iter().enumerate() {
            city.push_str(&format!("entrance_{slot}=On,1,1,Map {map},-1,-1,0\n"));
        }
    }
    let worldmap = "[Data]\nterrain_types=Flat:1, Hills:2, Rough:3, Peak:5\n\
        [Tile Data]\nnum_horizontal_tiles=1\n[Tile 0]\nart_idx=1\n\
        0_0=Flat,No_Fill,None,None,None,\n1_0=Hills,No_Fill,None,None,None,\n\
        2_0=Rough,No_Fill,None,None,None,\n3_0=Peak,No_Fill,None,None,None,\n"
        .to_string();
    let party = "[Party Member 0]\nparty_member_pid=16777296\nlevel_minimum=3\n\
        level_up_every=3\nlevel_pids=16777297,16777298,16777299\n"
        .to_string();
    load(vec![
        ("data/maps.txt", maps),
        ("data/city.txt", city),
        ("data/worldmap.txt", worldmap),
        ("data/party.txt", party),
    ])
}
fn host() -> WorldHost<TestAssets> {
    let world = WorldState::new(CharacterSheet::default(), 1).unwrap();
    let mut host = WorldHost::new(world, TestAssets::all_art());
    host.world.fallout2 = Some(Box::new(Fallout2World::new_game(tables(), -1)));
    host
}
fn state(host: &mut WorldHost<TestAssets>) -> &mut Fallout2World {
    host.world.fallout2.as_deref_mut().unwrap()
}
/// A critter of `pid` with base max HP `hp`.
fn critter_prototype(pid: u32, hp: i32) -> Arc<vault_formats::Prototype> {
    let mut words = special(&[(7, hp), (8, 8)]);
    words.push((0, pid as i32));
    patched(1, 0, &words, &[])
}
fn join(host: &mut WorldHost<TestAssets>, member: ObjectRef) {
    let id = 18000 + host.world.party.len() as u32;
    host.world.get_mut(member).unwrap().raw.id = id;
    host.fallout2_party_add(member);
}
fn messages(host: &WorldHost<TestAssets>) -> Vec<Vec<u8>> {
    host.world
        .effects
        .iter()
        .filter_map(|effect| match effect {
            WorldEffect::Message(text) => Some(text.clone()),
            _ => None,
        })
        .collect()
}

#[test]
fn spec019_vector_k_level_ups_through_experience() {
    let mut host = host();
    for (pid, hp) in [(0x0100_0051, 48), (0x0100_0052, 52), (0x0100_0053, 60)] {
        host.assets
            .prototypes
            .insert(pid, critter_prototype(pid, hp));
    }
    let member = host
        .world
        .spawn(critter_prototype(0x0100_0050, 40), 20300, 0)
        .unwrap();
    set_hp(&mut host.world, member, 30);
    join(&mut host, member);
    let stage = |host: &WorldHost<TestAssets>| host.control_of(member).unwrap().stage;
    assert_eq!(stage(&host), 0);
    let draws = host.world.rng.state().clone();
    // Level 3: stage 1, PID P1, HP 30 + (48 - 40) = 38.
    host.give_experience(3000);
    assert_eq!(host.world.player_sheet().unwrap().level, 3);
    assert_eq!(stage(&host), 1);
    let object = host.world.get(member).unwrap();
    assert_eq!(object.raw.pid, 0x0100_0051);
    assert_eq!(host.world.hit_points(member), Some(38));
    let name = object.name.clone();
    assert!(
        messages(&host).contains(&[name.as_slice(), b" has gained a level."].concat()),
        "{:?}",
        messages(&host)
    );
    // Level 4 needs 6: nothing; level 6 advances to stage 2.
    host.give_experience(3000);
    assert_eq!(host.world.player_sheet().unwrap().level, 4);
    assert_eq!(stage(&host), 1);
    host.give_experience(9000);
    assert_eq!(host.world.player_sheet().unwrap().level, 6);
    assert_eq!(stage(&host), 2);
    assert_eq!(host.world.get(member).unwrap().raw.pid, 0x0100_0052);
    // The object keeps its identity and team; no draw was made.
    assert_eq!(host.world.get(member).unwrap().raw.id, 18000);
    assert_eq!(host.world.rng.state(), &draws);
    // A PID without a prototype stops the member, with a diagnostic.
    host.assets.prototypes.remove(&0x0100_0053);
    host.give_experience(20000);
    assert_eq!(stage(&host), 2);
}

#[test]
fn spec019_vector_l_car_fuel_and_speed() {
    assert_eq!(
        [1, 2, 3, 5].map(fallout2::car_steps),
        [4, 2, 1, 1],
        "difficulty 1, 2, 3 and 5"
    );
    let drive = |fuel: i32, refuel_after_first: bool| {
        let mut host = host();
        let world = state(&mut host);
        world.car = Car {
            has_car: true,
            area: 4,
            map: 20,
            fuel,
        };
        world.position = [10, 10];
        world.driving = true;
        host.fallout2_travel_to([45, 10]).unwrap();
        let mut log = Vec::new();
        for iteration in 1..=6 {
            let x = state_x(&host);
            let outcome = host.fallout2_travel_iteration().unwrap();
            log.push((state_x(&host) - x, state(&mut host).car.fuel, outcome));
            if iteration == 1 && refuel_after_first {
                let car = &mut state(&mut host).car;
                car.fuel = (car.fuel + 100).clamp(0, fallout2::FUEL_MAX);
            }
            if outcome != Fallout2Travel::Moving {
                break;
            }
        }
        log
    };
    use Fallout2Travel::{Moving, OutOfFuel};
    assert_eq!(
        drive(250, false),
        [(4, 150, Moving), (4, 50, Moving), (4, 0, OutOfFuel)]
    );
    assert_eq!(
        drive(250, true),
        [
            (4, 150, Moving),
            (4, 150, Moving),
            (4, 50, Moving),
            (4, 0, OutOfFuel)
        ]
    );
    // An empty tank walks the terrain schedule and uses no fuel.
    let walk = drive(0, false);
    assert_eq!(walk[0], (1, 0, Moving));
    // The gauge is empty; give gas clamps to 0-80000 (rule 32's clamp).
    assert_eq!(Car::default().gauge_rows(), 0);
    let clamp = |fuel: i32, gas: i32| (i64::from(fuel) + i64::from(gas)).clamp(0, 80000);
    assert_eq!((clamp(100, 90000), clamp(300, -500)), (80000, 0));
    // On difficulty 2 a car iteration makes 2 steps.
    let mut world = Fallout2World::new_game(tables(), -1);
    world.position = [60, 10];
    world.car.fuel = 1000;
    world.travel_to([90, 10]);
    assert_eq!(world.drive(), (Step::Moving, 2));
}
fn state_x(host: &WorldHost<TestAssets>) -> i32 {
    host.world.fallout2.as_deref().unwrap().position[0]
}

#[test]
fn spec019_vector_m_where_the_car_is() {
    let mut host = host();
    let enter = |host: &mut WorldHost<TestAssets>, map: i32| {
        host.world.map.header.map_number = map;
        state(host).map_loaded(map);
    };
    let open = |host: &mut WorldHost<TestAssets>| {
        host.fallout2_build_world_map(true).unwrap();
        state(host).driving
    };
    let from_world_map = |host: &mut WorldHost<TestAssets>, map: i32| {
        enter(host, map);
        host.world_map_closed();
    };
    // Given on map 20 (metarule rule 31's assignment).
    enter(&mut host, 20);
    let area = state(&mut host).tables.city.area_of_map(20);
    state(&mut host).car = Car {
        has_car: true,
        area,
        map: 20,
        fuel: 0,
    };
    let car = |host: &mut WorldHost<TestAssets>| {
        let car = state(host).car;
        (car.area, car.map)
    };
    assert_eq!(car(&mut host), (4, 20));
    enter(&mut host, 21);
    assert!(open(&mut host), "map 21 is in city 4");
    from_world_map(&mut host, 30);
    assert_eq!(car(&mut host), (7, 30));
    // A script loads map 40; leaving it walks.
    enter(&mut host, 40);
    assert!(!open(&mut host));
    from_world_map(&mut host, 30);
    assert_eq!(car(&mut host), (7, 30), "walking moves nothing");
    assert!(open(&mut host));
    from_world_map(&mut host, 90);
    assert_eq!(car(&mut host), (-1, 90));
    assert!(open(&mut host), "map 90 is car_map");
    // Without the car nothing drives.
    state(&mut host).car.has_car = false;
    assert!(!open(&mut host));
    assert!(!Car::default().drives(20, 4));
}

#[test]
fn spec019_vector_n_city_containment() {
    let mut text = String::from("[Area 00]\nworld_pos=500,400\nsize=Medium\n");
    text.push_str("entrance_0=On,1,1,Map 30,-1,-1,0\n");
    let tables = load(vec![
        ("data/city.txt", text),
        (
            "data/maps.txt",
            "[Map 030]\nlookup_name=Map 30\nmap_name=map30\n".into(),
        ),
        (
            "data/worldmap.txt",
            "[Tile Data]\nnum_horizontal_tiles=2\n[Tile 0]\n[Tile 3]\n".into(),
        ),
        (
            "data/party.txt",
            "[Party Member 0]\nparty_member_pid=16777296\n".into(),
        ),
    ]);
    let world = Fallout2World::new_game(tables, -1);
    assert_eq!(world.area_at([510, 408]), Some(0), "164 <= 169");
    assert_eq!(world.area_at([511, 408]), None, "185 > 169");
}

#[test]
fn spec019_vector_o_party_counts_and_push() {
    let mut host = host();
    let player = host.world.player;
    host.world.move_object(player, 20100, 0, false).unwrap();
    let s_hex = HexGrid::neighbor(20100, 1).unwrap().unwrap();
    let s = host
        .world
        .spawn(critter_prototype(0x0100_0050, 30), s_hex, 0)
        .unwrap();
    host.world.get_mut(s).unwrap().raw.flags = BasicFlags(0);
    let trunk = host.world.spawn(patched(0, 1, &[], &[]), 20500, 0).unwrap();
    host.world.get_mut(trunk).unwrap().raw.flags = BasicFlags::HIDDEN;
    join(&mut host, s);
    join(&mut host, trunk);
    assert_eq!(host.world.party_members().len(), 3, "all entries");
    assert_eq!(host.visible_party_count(), 2);
    // Push: D = 1; the direction-1 neighbor is blocked, direction 2 free.
    assert_eq!(crate::routing::rotation(20100, s_hex), Some(1));
    let blocker = |host: &mut WorldHost<TestAssets>, direction: u8| {
        let hex = HexGrid::neighbor(s_hex, direction).unwrap().unwrap();
        let rock = host
            .world
            .spawn(critter_prototype(0x0100_0099, 10), hex, 0)
            .unwrap();
        host.world.get_mut(rock).unwrap().raw.flags = BasicFlags(0);
    };
    blocker(&mut host, 1);
    let draws = host.world.rng.state().clone();
    assert!(host.fallout2_can_push(s));
    let free = HexGrid::neighbor(s_hex, 2).unwrap().unwrap();
    assert_eq!(host.fallout2_push(s).unwrap(), Some(Push2::Moved(free)));
    assert_eq!(host.world.rng.state(), &draws);
    // §3.14: the hidden trunk travels to the player's hex, still hidden.
    host.world.place_party().unwrap();
    let trunk_object = host.world.get(trunk).unwrap();
    assert_eq!(trunk_object.raw.hex, Some(20100));
    assert!(trunk_object.raw.flags.contains(BasicFlags::HIDDEN));
    // With directions 1, 2 and 0 blocked: the message, nothing moves.
    let mut host2 = self::host();
    let player = host2.world.player;
    host2.world.move_object(player, 20100, 0, false).unwrap();
    let s = host2
        .world
        .spawn(critter_prototype(0x0100_0050, 30), s_hex, 0)
        .unwrap();
    host2.world.get_mut(s).unwrap().raw.flags = BasicFlags(0);
    join(&mut host2, s);
    for direction in [1, 2, 0] {
        blocker(&mut host2, direction);
    }
    host2.world.effects.clear();
    assert_eq!(host2.fallout2_push(s).unwrap(), Some(Push2::Blocked));
    let name = host2.world.get(s).unwrap().name.clone();
    assert_eq!(
        messages(&host2),
        [[b"There is no room to push ".as_slice(), &name, b"."].concat()]
    );
    assert_eq!(host2.world.get(s).unwrap().raw.hex, Some(s_hex));
    // Not in combat, not the player, not a stranger.
    host2.world.combat = true;
    assert!(!host2.fallout2_can_push(s));
    host2.world.combat = false;
    assert!(!host2.fallout2_can_push(player));
    let stranger = host2
        .world
        .spawn(critter_prototype(0x0100_0099, 10), 20900, 0)
        .unwrap();
    assert!(!host2.fallout2_can_push(stranger));
    assert!(host2.action(player, stranger, ObjectAction::Push).is_err());
}

#[test]
fn spec019_capacity_retained_and_dead_members() {
    let mut host = host();
    let member = host
        .world
        .spawn(critter_prototype(0x0100_0050, 30), 20300, 0)
        .unwrap();
    join(&mut host, member);
    let record = host.control_of(member).unwrap();
    assert_eq!(record.fields, party::neutral());
    // A changed record survives leaving and rejoining.
    host.world.fallout2.as_deref_mut().unwrap().control.insert(
        18000,
        MemberRecord {
            fields: [0, 2, 0, 0, 0, 3, 4],
            ..record
        },
    );
    host.world.party.remove(&member);
    host.fallout2_party_left(member);
    assert!(state(&mut host).retained.contains_key(&18000));
    host.fallout2_party_add(member);
    assert_eq!(host.control_of(member).unwrap().fields[1], 2);
    // A dead member leaves the list, its record discarded.
    host.world.critter_state_mut(member).unwrap().results.0 |= crate::critical::effect::DEAD;
    host.drop_dead_members();
    assert!(!host.world.party.contains(&member));
    assert!(state(&mut host).control.is_empty() && state(&mut host).retained.is_empty());
    // The list holds at most 32 entries, the player included.
    let mut host = self::host();
    for k in 0..40 {
        let critter = host
            .world
            .spawn(critter_prototype(0x0100_0099, 10), 20300 + k, 0)
            .unwrap();
        join(&mut host, critter);
    }
    assert_eq!(host.world.party_members().len(), party::CAPACITY);
}

#[test]
fn spec019_fallout1_has_no_fallout2_party_rules() {
    let world = WorldState::new(CharacterSheet::default(), 1).unwrap();
    let mut host = WorldHost::new(world, TestAssets::all_art());
    let member = host
        .world
        .spawn(critter_prototype(0x0100_0050, 30), 20300, 0)
        .unwrap();
    host.world.party.insert(member);
    assert_eq!(host.control_of(member), None);
    assert!(!host.fallout2_can_push(member));
    host.drop_dead_members();
    host.party_level_ups(3);
    assert!(host.world.party.contains(&member));
}
