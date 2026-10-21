//! P-02: `move_object` rebuilds the light map only when the move can change
//! it, with results identical to an unconditional rebuild.
use crate::{
    CharacterSheet, ObjectRef, WorldState,
    world::{WorldObject, hex_line, object_record},
};
use std::collections::{BTreeSet, VecDeque};
use vault_formats::{HexGrid, pro::BasicFlags};

fn object(world: &mut WorldState, pid: u32, hex: i32, flags: u32) -> ObjectRef {
    let mut raw = object_record(pid, 0);
    raw.hex = Some(hex);
    raw.flags = BasicFlags(flags);
    world
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
        .unwrap()
}
/// The light map after a move equals an unconditional rebuild's.
fn assert_identical(world: &mut WorldState) {
    let kept = world.tile_light.clone();
    world.rebuild_lighting();
    assert_eq!(kept, world.tile_light);
}

#[test]
fn p02_moves_that_cannot_change_the_light_map_skip_the_rebuild_with_identical_results() {
    let mut world = WorldState::new(CharacterSheet::default(), 1).unwrap();
    let player = world.player;
    // The player lights radius 4 at full intensity from 20100 (SPEC-007
    // §3.9), so reach ends at lattice distance 5.
    world.set_light(player, 4, 65536);
    let blocker = object(&mut world, 0x0100_0002, 20102, 0);
    let hidden = object(&mut world, 0x0100_0003, 20103, BasicFlags::HIDDEN.0);
    let translucent = object(&mut world, 0x0200_0001, 20104, BasicFlags::LIGHT_THROUGH.0);
    let twin = object(&mut world, 0x0100_0004, 20105, 0);
    object(&mut world, 0x0100_0005, 20105, 0);
    world.rebuild_lighting();
    let cases = [
        // A blocker moving within reach.
        (blocker, 20101, 0, true),
        // The same hex and elevation: no change.
        (blocker, 20101, 0, false),
        // Leaving the reach still vacates a lit hex.
        (blocker, 30000, 0, true),
        // Far from every source.
        (blocker, 30001, 0, false),
        // Back within reach, onto a hex a hidden object holds.
        (blocker, 20103, 0, true),
        // An elevation change vacates the lit hex.
        (blocker, 20103, 1, true),
        // A hidden object neither blocks nor lights.
        (hidden, 20101, 0, false),
        // A light-through object does not block.
        (translucent, 20101, 0, false),
        // A hex another blocker keeps occupied, to a hex out of reach.
        (twin, 20106, 0, false),
        // A light source always rebuilds.
        (player, 20101, 0, true),
    ];
    for (reference, hex, elevation, expected) in cases {
        let (from, from_elevation) = {
            let object = world.get(reference).unwrap();
            (object.raw.hex, object.raw.elevation)
        };
        assert_eq!(
            world.move_changes_lighting(reference, from, from_elevation, hex, elevation),
            expected,
            "{reference} to {hex} on {elevation}"
        );
        world.move_object(reference, hex, elevation, false).unwrap();
        assert_identical(&mut world);
    }
}

#[test]
fn p02_light_lines_stay_within_the_slack_of_the_radius() {
    // For sample centers near every border and in the middle, every tile on
    // the interior of a line from the center to a hex `rebuild_lighting`
    // lights lies within the radius plus the slack the move guard uses.
    let columns = [0, 1, 2, 3, 50, 51, 100, 101, 196, 197, 198, 199];
    let rows = [0, 1, 2, 50, 51, 197, 198, 199];
    for column in columns {
        for row in rows {
            let center = column + 200 * row;
            for radius in 0..=8 {
                let mut visited = BTreeSet::new();
                let mut todo = VecDeque::from([(center, 0)]);
                while let Some((hex, distance)) = todo.pop_front() {
                    if !visited.insert(hex) {
                        continue;
                    }
                    let line = hex_line(center, hex);
                    for tile in line.iter().skip(1).take(line.len().saturating_sub(2)) {
                        let reach = HexGrid::lattice_distance(center, *tile).unwrap();
                        assert!(
                            reach <= radius + WorldState::LIGHT_LINE_SLACK,
                            "line {center} to {hex} passes {tile} at distance {reach}"
                        );
                    }
                    if distance < radius {
                        for direction in 0..6 {
                            if let Ok(Some(next)) = HexGrid::neighbor(hex, direction) {
                                todo.push_back((next, distance + 1));
                            }
                        }
                    }
                }
            }
        }
    }
}
