//! SPEC-014 §3.36 roof hiding and restoring: Fixture G, corner-touching and
//! barrier groups, elevation changes, and the player-only rule.
use crate::{
    CharacterSheet, WorldState,
    roof::{self, ROOF_BARRIER, ROOF_HIDDEN, RoofTracker},
    world::{WorldObject, object_record},
};
use vault_formats::map::TilePair;

/// A 100 × 100 grid of blank pairs with the given squares' roof art index.
fn grid(roofed: &[((i32, i32), u32)]) -> Vec<TilePair> {
    let mut tiles = vec![TilePair::BLANK; 10000];
    for ((column, row), index) in roofed {
        let position = roof::square_position(*column, *row).unwrap();
        tiles[position].word = (index << 16) | 0x0001;
        tiles[position].stored = tiles[position].word;
    }
    tiles
}
/// SPEC-014 §4.7: squares (10-12, 20-21) with roof index 5, (13, 22) with
/// index 5 touching them only at a corner; with `bridge`, (13, 21) too.
fn fixture_g(bridge: bool) -> Vec<TilePair> {
    let mut squares: Vec<_> = (10..=12)
        .flat_map(|column| [((column, 20), 5), ((column, 21), 5)])
        .collect();
    squares.push(((13, 22), 5));
    if bridge {
        squares.push(((13, 21), 5));
    }
    grid(&squares)
}
fn world_with(tiles: [Option<Vec<TilePair>>; 3]) -> WorldState {
    let mut world = WorldState::new(CharacterSheet::default(), 1).unwrap();
    world.map.tiles = tiles;
    world
}
fn hidden(world: &WorldState, elevation: u8) -> Vec<(i32, i32)> {
    world
        .hidden_roof_squares(elevation)
        .iter()
        .map(|position| ((position % 100) as i32, (position / 100) as i32))
        .collect()
}
fn place(world: &mut WorldState, hex: i32, elevation: u8) {
    let player = world.player;
    world.move_object(player, hex, elevation, false).unwrap();
}

#[test]
fn spec014_fixture_g_hides_and_restores_edge_connected_groups() {
    let mut world = world_with([Some(fixture_g(false)), None, None]);
    assert_eq!(world.roof, RoofTracker::NONE);
    let group: Vec<_> = (20..=21)
        .flat_map(|row| (10..=12).map(move |column| (column, row)))
        .collect();
    // Step 1: (9, 20) is empty and so was "none": nothing to restore or hide.
    assert_eq!(roof::roof_square(8018), (9, 20));
    place(&mut world, 8018, 0);
    assert!(hidden(&world, 0).is_empty());
    assert_eq!(
        world.roof,
        RoofTracker {
            column: 9,
            row: 20,
            elevation: 0,
            last_empty: true
        }
    );
    // Step 2: (10, 20) is a roof: E changed, hide fill of the six squares.
    place(&mut world, 8020, 0);
    assert_eq!(hidden(&world, 0), group);
    for (column, row) in &group {
        assert_eq!(world.roof_tile(0, *column, *row).word >> 16, 0x1005);
    }
    // Steps 3 and 4: within the group every nibble is 1, nothing changes.
    place(&mut world, 8422, 0);
    assert_eq!(hidden(&world, 0), group);
    place(&mut world, 8424, 0);
    assert_eq!(hidden(&world, 0), group);
    // Step 5: (13, 22) touches the group only at a corner, nibble 0 against
    // 1: restore from (12, 21), hide from (13, 22).
    place(&mut world, 8826, 0);
    assert_eq!(hidden(&world, 0), [(13, 22)]);
    // Step 6: (14, 22) is empty: restore from (13, 22).
    place(&mut world, 8828, 0);
    assert!(hidden(&world, 0).is_empty());
    assert_eq!(
        world.roof,
        RoofTracker {
            column: 14,
            row: 22,
            elevation: 0,
            last_empty: true
        }
    );
    // The fills left every other bit of the words alone.
    assert!(
        world.map.tiles[0]
            .as_ref()
            .unwrap()
            .iter()
            .all(|tile| tile.word == tile.stored)
    );
}

#[test]
fn spec014_fixture_g_with_the_bridge_square_is_one_group_of_eight() {
    let mut world = world_with([Some(fixture_g(true)), None, None]);
    place(&mut world, 8018, 0);
    place(&mut world, 8020, 0);
    assert_eq!(hidden(&world, 0).len(), 8);
    place(&mut world, 8422, 0);
    place(&mut world, 8424, 0);
    // Step 5 changes nothing: (13, 22) joins the group along an edge.
    place(&mut world, 8826, 0);
    assert_eq!(hidden(&world, 0).len(), 8);
    place(&mut world, 8828, 0);
    assert!(hidden(&world, 0).is_empty());
}

#[test]
fn spec014_fills_do_not_connect_diagonally_and_any_order_gives_the_same_set() {
    let mut tiles = grid(&[((5, 5), 7), ((6, 6), 7), ((6, 5), 7), ((40, 40), 7)]);
    assert_eq!(roof::hide_fill(&mut tiles, 5, 5), 3);
    assert_eq!(roof::hide_fill(&mut tiles, 5, 5), 0);
    assert_eq!(roof::restore_fill(&mut tiles, 6, 6), 3);
    // A start outside the grid, an empty start, or a start with the barrier
    // bit hides nothing.
    assert_eq!(roof::hide_fill(&mut tiles, -1, -1), 0);
    assert_eq!(roof::hide_fill(&mut tiles, 100, 0), 0);
    assert_eq!(roof::hide_fill(&mut tiles, 7, 7), 0);
    tiles[roof::square_position(40, 40).unwrap()].word |= ROOF_BARRIER;
    assert_eq!(roof::hide_fill(&mut tiles, 40, 40), 0);
}

#[test]
fn spec014_barrier_squares_are_never_hidden_and_stop_the_fills() {
    // Three roofed squares in a row along row 30; the middle one carries
    // the barrier bit.
    let mut tiles = grid(&[((20, 30), 5), ((21, 30), 5), ((22, 30), 5)]);
    tiles[roof::square_position(21, 30).unwrap()].word |= ROOF_BARRIER;
    let mut world = world_with([Some(tiles), None, None]);
    place(&mut world, 12040, 0);
    assert_eq!(hidden(&world, 0), [(20, 30)]);
    // Standing on the barrier square: nibble 2 differs from 1, so the first
    // square is restored, and the hide fill from the barrier hides nothing.
    place(&mut world, 12042, 0);
    assert!(hidden(&world, 0).is_empty());
    assert_eq!(world.roof_tile(0, 21, 30).word & ROOF_HIDDEN, 0);
    // The far side is a separate group; the barrier is never hidden.
    place(&mut world, 12044, 0);
    assert_eq!(hidden(&world, 0), [(22, 30)]);
    assert_eq!(world.roof_tile(0, 21, 30).word & ROOF_HIDDEN, 0);
}

#[test]
fn spec014_elevation_changes_restore_on_the_elevation_where_the_group_was_hidden() {
    // Elevation 0 and 1 both roof the player's start square (50, 50);
    // square (60, 60) of elevation 1 is empty.
    let mut world = world_with([
        Some(grid(&[((50, 50), 5)])),
        Some(grid(&[((50, 50), 6)])),
        None,
    ]);
    place(&mut world, 20100, 0);
    assert_eq!(hidden(&world, 0), [(50, 50)]);
    // Rising to an empty square: E changed, and the `[May-improve]` restore
    // runs on elevation 0, where the group was hidden.
    place(&mut world, 24120, 1);
    assert!(hidden(&world, 0).is_empty());
    assert!(hidden(&world, 1).is_empty());
    assert_eq!(
        world.roof,
        RoofTracker {
            column: 60,
            row: 60,
            elevation: 1,
            last_empty: true
        }
    );
    place(&mut world, 20100, 1);
    assert_eq!(hidden(&world, 1), [(50, 50)]);
    assert!(hidden(&world, 0).is_empty());
    // Down again on the same square: the elevation differs, so the rule
    // runs; p is read on elevation 1 (nibble 1) and c on elevation 0
    // (nibble 0): restore on elevation 1, hide on elevation 0.
    place(&mut world, 20100, 0);
    assert_eq!(hidden(&world, 0), [(50, 50)]);
    assert!(hidden(&world, 1).is_empty());
    // An elevation without tile data is all empty.
    place(&mut world, 20100, 2);
    assert!(hidden(&world, 0).is_empty());
    assert!(world.hidden_roof_squares(2).is_empty());
}

#[test]
fn spec014_only_the_player_hides_roofs_and_a_fresh_world_starts_from_none() {
    let mut world = world_with([Some(fixture_g(false)), None, None]);
    let mut raw = object_record(0x0100_0001, 0);
    raw.hex = Some(20100);
    let critter = world
        .insert(WorldObject {
            raw,
            prototype: None,
            character: None,
            name: Vec::new(),
            description: Vec::new(),
            owner: None,
            inventory: Vec::new(),
            money: None,
            active: true,
            opponent: 0,
        })
        .unwrap();
    world.move_object(critter, 8020, 0, false).unwrap();
    assert!(hidden(&world, 0).is_empty());
    assert_eq!(world.roof, RoofTracker::NONE);
    assert_eq!(
        WorldState::fresh(CharacterSheet::default(), 1)
            .unwrap()
            .roof,
        RoofTracker::NONE
    );
    assert_eq!(RoofTracker::NONE.last(), None);
}

#[test]
fn spec014_restore_on_leaving_runs_from_the_last_square_only_when_roofed() {
    let mut world = world_with([Some(fixture_g(false)), None, None]);
    // Nothing recorded: nothing to read outside the grid, nothing restored.
    world.restore_roofs();
    place(&mut world, 8020, 0);
    assert_eq!(hidden(&world, 0).len(), 6);
    // Another hidden group elsewhere is not connected to the last square.
    let position = roof::square_position(40, 40).unwrap();
    world.map.tiles[0].as_mut().unwrap()[position].word = (5 << 16) | 0x0001 | ROOF_HIDDEN;
    world.restore_roofs();
    assert_eq!(hidden(&world, 0), [(40, 40)]);
    // The tracking state is unchanged by the restore.
    assert_eq!(
        world.roof,
        RoofTracker {
            column: 10,
            row: 20,
            elevation: 0,
            last_empty: false
        }
    );
}
