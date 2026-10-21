use crate::{geometry::GeometryError, map::*, pro::*, *};
#[allow(dead_code)]
#[path = "../../../tests/fixtures/world/support.rs"]
mod fixture;

fn lookup(pid: u32) -> Result<PrototypeInfo, ParseError> {
    assert_eq!(pid, 1);
    Ok(Prototype::parse(&fixture::container())?.info())
}
#[test]
fn spec003_container_unaligned_payload_and_flag_namespaces() {
    let bytes = fixture::container();
    assert_eq!(bytes.len(), 65);
    let pro = Prototype::parse(&bytes).unwrap();
    assert_eq!(
        (pro.pid, pro.message_base, pro.art_id, pro.script),
        (1, 100, 0, Some(-1))
    );
    assert_eq!(
        pro.light,
        Some(Light {
            radius: 0,
            intensity: 0
        })
    );
    assert!(pro.flags.contains(BasicFlags::NONBLOCKING));
    assert!(pro.extended.contains(ExtendedFlags::USE));
    assert!(pro.extended.contains(ExtendedFlags::PICKUP_CONTAINER));
    let PrototypeData::Item(item) = pro.data else {
        panic!()
    };
    assert_eq!(
        (
            item.material,
            item.size,
            item.weight,
            item.cost,
            item.inventory_art,
            item.stored_byte
        ),
        (1, 5, 2, 10, 0x07000000, 0xa5)
    );
    assert_eq!(
        item.data,
        ItemData::Container {
            capacity: 100,
            flags: ContainerFlags(1)
        }
    );
    assert_eq!(
        BasicFlags(0xffff_ffff).translucency(),
        Some(Translucency::Opaque)
    );
    assert_eq!(ExtendedFlags(0x98).attack_modes(), [8, 0]);
    assert!(ExtendedFlags(0x98).invalid_attack_mode());
    assert_eq!(ExtendedFlags(0x87).attack_modes(), [7, 8]);
    assert!(!ExtendedFlags(0x87).invalid_attack_mode());
    let packed = PackedLocation(20100 | (5 << 26) | (2 << 29));
    assert_eq!(
        (packed.hex(), packed.orientation(), packed.elevation()),
        (20100, 5, 2)
    );
}
#[test]
fn all_prototype_categories_subtypes_and_every_required_prefix() {
    for category in 0..6 {
        for subtype in 0..match category {
            0 => 7,
            2 => 6,
            _ => 1,
        } {
            let bytes = fixture::prototype(category, subtype);
            let pro = Prototype::parse(&bytes).unwrap();
            assert_eq!(pro.consumed_len, bytes.len());
            for end in 0..bytes.len() {
                assert!(Prototype::parse(&bytes[..end]).is_err());
            }
            let mut extended = bytes.clone();
            extended.extend([0; 24]);
            assert_eq!(Prototype::parse(&extended).unwrap(), pro);
            if category == 4 {
                assert!(pro.light.is_none());
            }
            if category == 5 {
                assert!(pro.script.is_none());
            }
        }
    }
    let mut weapon = fixture::prototype(0, 3);
    fixture::word(&mut weapon, 0x3d, 7);
    fixture::word(&mut weapon, 0x41, 19);
    weapon[0x79] = 0xef;
    let PrototypeData::Item(Item {
        data: ItemData::Weapon(weapon),
        ..
    }) = Prototype::parse(&weapon).unwrap().data
    else {
        panic!()
    };
    assert_eq!((weapon.damage, weapon.sound), ([7, 19], 0xef));
    let mut bad = fixture::container();
    fixture::word(&mut bad, 0, 0x10000001);
    assert!(Prototype::parse(&bad).is_err());
    fixture::word(&mut bad, 0, 1);
    fixture::word(&mut bad, 0x20, 7);
    assert!(Prototype::parse(&bad).is_err());
}
#[test]
fn spec003_empty_map_variable_order_and_omissions() {
    let bytes = fixture::empty_map();
    assert_eq!(bytes.len(), 284);
    let map = MapFile::parse(&bytes, &lookup).unwrap();
    assert_eq!(&map.header.name[..9], b"SYNTH.MAP");
    assert_eq!(
        (
            map.header.entry_hex,
            map.header.entry_elevation,
            map.header.entry_orientation
        ),
        (20100, 0, 0)
    );
    assert!(map.diagnostics.is_empty());
    // Safe rule: an entry orientation outside 0-5 reads as 0, diagnosed.
    for orientation in [6, -1] {
        let mut bad = bytes.clone();
        fixture::word(&mut bad, 0x1c, orientation);
        let map = MapFile::parse(&bad, &lookup).unwrap();
        assert_eq!(map.header.entry_orientation, 0);
        assert_eq!(map.diagnostics.len(), 1);
    }
    assert_eq!(
        (map.header.map_number, map.header.map_script_index()),
        (7, None)
    );
    assert_eq!(map.mvars, [7, -2]);
    assert_eq!(map.lvars, [42]);
    assert!(map.tiles.iter().all(Option::is_none));
    assert_eq!(map.tile(2, 5050), Some(TilePair::BLANK));
    assert_eq!(map.objects.total, 0);
    assert!(
        map.scripts
            .iter()
            .all(|category| category.active().count() == 0)
    );
    for end in 0..bytes.len() {
        assert!(
            MapFile::parse(&bytes[..end], &lookup).is_err(),
            "prefix {end}"
        );
    }
    let mut bad = bytes.clone();
    fixture::word(&mut bad, 0, 20);
    assert!(MapFile::parse(&bad, &lookup).is_err());
    let mut negative = bytes;
    negative.drain(236..248);
    fixture::word(&mut negative, 32, -1);
    fixture::word(&mut negative, 48, -2);
    let map = MapFile::parse(&negative, &lookup).unwrap();
    assert!(map.mvars.is_empty() && map.lvars.is_empty());
}
#[test]
fn spec003_depth_first_inventory_quantities_and_owners() {
    let bytes = fixture::inventory();
    assert_eq!(bytes.len(), 196);
    let objects = MapObjects::parse(&bytes, &lookup, true).unwrap();
    assert_eq!(objects.consumed_len, 196);
    assert_eq!(objects.total, 1);
    let parent = &objects.elevations[0][0];
    assert_eq!(
        (parent.id, parent.hex, parent.file_offset, parent.owner),
        (42, Some(20100), 8, None)
    );
    assert_eq!(parent.inventory.len(), 1);
    let entry = &parent.inventory[0];
    assert_eq!(entry.quantity, 3);
    assert_eq!(
        (
            entry.object.id,
            entry.object.hex,
            entry.object.owner,
            entry.object.file_offset
        ),
        (43, None, Some(42), 100)
    );
    assert_eq!(
        (entry.object.stored_elevation, entry.object.elevation),
        (2, 0)
    );
    for end in 0..bytes.len() {
        assert!(MapObjects::parse(&bytes[..end], &lookup, true).is_err());
    }
    let mut invalid = bytes.clone();
    fixture::word(&mut invalid, 0x54, 0);
    assert!(MapObjects::parse(&invalid, &lookup, true).is_err());
    let mut full = fixture::empty_map();
    full.truncate(268);
    full.extend(bytes);
    assert_eq!(MapFile::parse(&full, &lookup).unwrap().objects.total, 1);
}
#[test]
fn map_tiles_and_per_slot_script_dispatch() {
    let mut bytes = fixture::empty_map();
    bytes.truncate(248);
    fixture::word(&mut bytes, 40, 0x0d);
    bytes.extend(fixture::words(&vec![0x31235045; 10000]));
    for category in 0..5 {
        bytes.extend(1_i32.to_be_bytes());
        bytes.extend(fixture::script_slot(category, category as u32 + 1));
        for slot in 1..16 {
            bytes.extend(fixture::script_slot((slot % 3) as u8, 0));
        }
        bytes.extend(fixture::words(&[1, -559038737]));
    }
    bytes.extend(fixture::words(&[1, 1]));
    let mut object = fixture::object(0, 20100, 50, &[]);
    fixture::word(&mut object, 0x40, 1);
    fixture::word(&mut object, 0x44, 77);
    fixture::word(&mut object, 0x54, 0xcccccccc_u32 as i32);
    bytes.extend(object);
    bytes.extend(fixture::words(&[0, 0]));
    let map = MapFile::parse(&bytes, &lookup).unwrap();
    let tile = map.tile(0, 0).unwrap();
    assert_eq!(
        (tile.floor(), tile.roof(), tile.stored, tile.word),
        (0x45, 0x123, 0x31235045, 0x21235045)
    );
    assert!(tile.floor_hidden() && !tile.roof_hidden() && tile.roof_barrier());
    for (category, stream) in map.scripts.iter().enumerate() {
        assert_eq!(stream.active().count(), 1);
        assert_eq!(stream.blocks[0].slots.len(), 16);
        assert_eq!(stream.active().next().unwrap().id >> 24, category as u32);
        assert_eq!(stream.active().next().unwrap().flags, ScriptFlags(0));
        assert!(matches!(
            stream.blocks[0].slots[1].prefix,
            ScriptPrefix::Spatial { .. }
        ));
        assert!(matches!(
            stream.blocks[0].slots[2].prefix,
            ScriptPrefix::Timed(_)
        ));
    }
    assert_eq!(map.consumed_len, bytes.len());
    assert_eq!(map.objects.elevations[0][0].script_id, 1);
    assert_eq!(map.objects.elevations[0][0].cached_script_file, 0);
    assert_eq!(
        map.objects.elevations[0][0].state,
        ObjectState::Other {
            word: 0,
            detail: ObjectDetail::Plain
        }
    );
    let mut unsaved = bytes.clone();
    fixture::word(&mut unsaved, 40, 0x0c);
    fixture::word(&mut unsaved, 248 + 40000 + 4 + 24, 0);
    fixture::word(&mut unsaved, 248 + 40000 + 4 + 28, 3);
    let base = MapFile::parse(&unsaved, &lookup).unwrap();
    let active = base.scripts[0].active().next().unwrap();
    assert_eq!((active.stored_local_count, active.local_count), (3, 0));
    assert_eq!(base.lvar(active, 0), None);
    // SPEC-003 §3.2: a saved slice past the local pool is a runtime
    // out-of-range access (reading zero), not a load failure: diagnosed.
    fixture::word(&mut unsaved, 40, 0x0d);
    let saved = MapFile::parse(&unsaved, &lookup).unwrap();
    assert_eq!(
        saved.lvar(saved.scripts[0].active().next().unwrap(), 0),
        None
    );
    assert!(saved.diagnostics.iter().any(|line| line.contains("beyond")));
    // §2.9 [May-improve]: a negative category count is rejected.
    let mut bad = bytes;
    fixture::word(&mut bad, 248 + 40000, -1);
    assert!(MapFile::parse(&bad, &lookup).is_err());
}

#[test]
fn spec003_unmandated_record_checks_are_diagnostics() {
    // Object frame, orientation, and inventory quantity have no validation
    // rule in SPEC-003 §2.10/§2.12: out-of-range values are substituted
    // or kept with a diagnostic rather than failing the map.
    let mut full = fixture::empty_map();
    full.truncate(268);
    full.extend(fixture::inventory());
    let mut mutated = full.clone();
    fixture::word(&mut mutated, 268 + 8 + 0x18, -3);
    fixture::word(&mut mutated, 268 + 8 + 0x1c, 6);
    fixture::word(&mut mutated, 268 + 96, 0);
    let map = MapFile::parse(&mutated, &lookup).unwrap();
    let parent = &map.objects.elevations[0][0];
    assert_eq!((parent.frame, parent.orientation), (0, 0));
    assert_eq!(parent.inventory[0].quantity, 0);
    assert_eq!(map.diagnostics.len(), 3);
    assert!(map.diagnostics[0].contains("frame -3"));
    assert!(map.diagnostics[1].contains("orientation 6"));
    assert!(map.diagnostics[2].contains("quantity 0"));
    // A negative quantity cannot be represented and is still rejected.
    fixture::word(&mut mutated, 268 + 96, -1);
    assert!(MapFile::parse(&mutated, &lookup).is_err());
    // Script records: an active slot whose identifier names another
    // category, or repeats an earlier identifier, is reported (§2.9, §3.3)
    // and the first record binds.
    let mut bytes = fixture::empty_map();
    bytes.truncate(248);
    bytes.extend(3_i32.to_be_bytes());
    bytes.extend(fixture::script_slot(0, 5));
    bytes.extend(fixture::script_slot(3, 6));
    bytes.extend(fixture::script_slot(0, 5));
    for slot in 3..16 {
        bytes.extend(fixture::script_slot(0, slot));
    }
    bytes.extend(fixture::words(&[3, 0]));
    bytes.extend(fixture::words(&[0; 8]));
    let map = MapFile::parse(&bytes, &lookup).unwrap();
    assert_eq!(map.scripts[0].active().count(), 3);
    assert_eq!(map.diagnostics.len(), 2);
    assert!(map.diagnostics[0].contains("0x03000006"));
    assert!(map.diagnostics[1].contains("duplicate active script 0x00000005"));
}
#[test]
fn subtype_trailers_saved_state_and_depth_limits() {
    for subtype in 0..7 {
        let proto = Prototype::parse(&fixture::prototype(0, subtype)).unwrap();
        let count = match subtype {
            3 => 2,
            4..=6 => 1,
            _ => 0,
        };
        let mut bytes = fixture::words(&[1, 1]);
        bytes.extend(fixture::object(0, 20100, 42, &vec![-1; count]));
        bytes.extend(fixture::words(&[0, 0]));
        let provider = |_| Ok(proto.info());
        let map = MapObjects::parse(&bytes, &provider, false).unwrap();
        if subtype == 3 {
            assert_eq!(
                map.elevations[0][0].state,
                ObjectState::Other {
                    word: 0,
                    detail: ObjectDetail::Weapon {
                        rounds: 0,
                        ammo_pid: 0
                    }
                }
            );
        }
        assert_eq!(
            MapObjects::parse(&bytes, &provider, true)
                .unwrap()
                .consumed_len,
            bytes.len()
        );
    }
    for subtype in 0..6 {
        let proto = Prototype::parse(&fixture::prototype(2, subtype)).unwrap();
        let count = match subtype {
            0 | 3 | 4 => 1,
            1 | 2 => 2,
            _ => 0,
        };
        let mut bytes = fixture::words(&[1, 1]);
        bytes.extend(fixture::object(2, 20100, 42, &vec![0; count]));
        bytes.extend(fixture::words(&[0, 0]));
        assert_eq!(
            MapObjects::parse(&bytes, &|_| Ok(proto.info()), true)
                .unwrap()
                .consumed_len,
            bytes.len()
        );
    }
    let mut deep = fixture::words(&[1, 1]);
    for index in 0..66 {
        let mut object = fixture::object(0, 20100, index, &[]);
        fixture::word(&mut object, 72, 1);
        fixture::word(&mut object, 76, 1);
        deep.extend(object);
        deep.extend(1_i32.to_be_bytes());
    }
    assert!(MapObjects::parse(&deep, &lookup, true).is_err());
}
#[test]
fn geometry_fixture_and_inverse_corner_boundaries() {
    let grid = HexGrid::centered(20100, 640, 380).unwrap();
    assert_eq!(grid.origin, [-3680, -430]);
    for line in include_str!("../../../tests/fixtures/world/geometry.txt").lines() {
        let values: Vec<i32> = line
            .split_whitespace()
            .map(|word| word.parse().unwrap())
            .collect();
        let [hex, x, y, distance]: [i32; 4] = values.try_into().unwrap();
        assert_eq!(grid.hex_center(hex).unwrap(), [i64::from(x), i64::from(y)]);
        assert_eq!(grid.pick_hex([x, y]), Some(hex));
        assert_eq!(HexGrid::lattice_distance(20100, hex).unwrap(), distance);
        assert_eq!(HexGrid::angular_distance(20100, hex).unwrap(), distance);
        assert_eq!(HexGrid::angular_walk(20100, hex).unwrap(), distance);
    }
    for (direction, hex) in [20099, 20299, 20300, 20301, 20101, 19900]
        .into_iter()
        .enumerate()
    {
        assert_eq!(
            HexGrid::neighbor(20100, direction as u8).unwrap(),
            Some(hex)
        );
    }
    assert_eq!(grid.square_corner(5050, false), Some([256, 180]));
    assert_eq!(grid.square_corner(5050, true), Some([256, 84]));
    assert_eq!(grid.pick_square([296, 198], false), Some(5050));
    assert_eq!(grid.pick_square([296, 102], true), Some(5050));
    assert_eq!(HexGrid::neighbor(20101, 0).unwrap(), Some(19900));
    assert_eq!(HexGrid::step(0, 2, 10).unwrap(), 0);
    assert_eq!(HexGrid::neighbor(0, 2).unwrap(), Some(200));
    assert_eq!(HexGrid::neighbor(199, 4).unwrap(), None);
    assert_eq!(HexGrid::lattice_distance(-1, -1).unwrap(), 9999);
    // The reference walk leaves the grid for these valid pairs; the safe
    // rule returns the lattice distance instead.
    for (a, b, distance) in [(0, 199, 199), (5225, 4558, 133)] {
        assert_eq!(HexGrid::angular_walk(a, b), Err(GeometryError::AngularWalk));
        assert_eq!(HexGrid::angular_distance(a, b).unwrap(), distance);
        assert_eq!(HexGrid::lattice_distance(a, b).unwrap(), distance);
    }
    assert_eq!(HexGrid::angular_distance(-1, 5), Ok(9999));
    assert_eq!(HexGrid::angular_distance(40000, 5), Err(GeometryError::Hex));
    // An orientation outside 0-5 returns the starting hex.
    assert_eq!(HexGrid::step(20100, 6, 3).unwrap(), 20100);
    assert_eq!(grid.pick_hex([319, 182]), Some(19900));
    assert_eq!(grid.pick_hex([320, 182]), Some(20100));
    assert_eq!(grid.pick_hex([321, 182]), Some(20099));
    for hex in 0..40000 {
        let center = grid.hex_center(hex).unwrap();
        assert_eq!(grid.pick_hex(center.map(|v| v as i32)), Some(hex));
    }
}
#[test]
fn vfs_prototype_lists_resolve_names_and_synthetic_map() {
    let dir = tempfile::tempdir().unwrap();
    fixture::write_corpus(dir.path());
    std::fs::write(
        dir.path().join("proto/items/items.lst"),
        b"synthetic.pro\nsynthetic.pro\n",
    )
    .unwrap();
    let vfs = LocalDirProvider::mount(dir.path()).unwrap();
    let catalog = LocalPrototypeCatalog::new(&vfs);
    assert_eq!(
        catalog.lookup(1).unwrap().kind,
        PrototypeKind::Item(ItemSubtype::Container)
    );
    assert!(catalog.lookup(0).is_err());
    // Entry 2 names a file whose stored PID is 1: rejected (safe rule).
    assert!(catalog.lookup(2).is_err());
    let map = MapFile::parse(&vfs.read("maps/synthetic.map").unwrap(), &catalog).unwrap();
    assert_eq!(map.objects.total, 5);
    assert!(map.tiles.iter().all(Option::is_some));
}

#[test]
fn script_categories_cross_block_boundaries_and_validate_occupancy() {
    let mut bytes = fixture::empty_map();
    bytes.truncate(248);
    bytes.extend(17_i32.to_be_bytes());
    let mut trailer = 0;
    for block in 0..2 {
        for slot in 0..16 {
            bytes.extend(fixture::script_slot(0, block * 16 + slot));
        }
        trailer = bytes.len();
        bytes.extend(fixture::words(&[if block == 0 { 16 } else { 1 }, 0]));
    }
    bytes.extend(fixture::words(&[0; 8])); // Four remaining categories and object counts.
    let map = MapFile::parse(&bytes, &lookup).unwrap();
    assert_eq!(map.scripts[0].active().count(), 17);
    assert_eq!(map.scripts[0].blocks.len(), 2);
    let mut invalid = bytes.clone();
    fixture::word(&mut invalid, trailer, 0);
    assert!(MapFile::parse(&invalid, &lookup).is_err());
    fixture::word(&mut invalid, trailer, 17);
    assert!(MapFile::parse(&invalid, &lookup).is_err());
    for end in [248, 252, 500, trailer, trailer + 4, bytes.len() - 1] {
        assert!(MapFile::parse(&bytes[..end], &lookup).is_err());
    }
}

#[test]
fn world_parsers_bound_mutated_counts_and_geometry_overflow() {
    let mut seed = 0x71357_u32;
    let mut next = || {
        seed ^= seed << 13;
        seed ^= seed >> 17;
        seed ^= seed << 5;
        seed
    };
    let provider = |_| Ok(Prototype::parse(&fixture::container()).unwrap().info());
    for original in [
        fixture::container(),
        fixture::empty_map(),
        fixture::inventory(),
    ] {
        for _ in 0..128 {
            let mut bytes = original.clone();
            let at = next() as usize % bytes.len();
            bytes[at] = next() as u8;
            let _ = Prototype::parse(&bytes);
            let _ = MapFile::parse(&bytes, &provider);
            let _ = MapObjects::parse(&bytes, &provider, true);
        }
    }
    let grid = HexGrid {
        origin: [i64::MAX, i64::MIN],
    };
    assert_eq!(
        grid.hex_center(20100),
        Err(GeometryError::ProjectionOverflow)
    );
    assert!(grid.square_corner(5050, false).is_none());
    assert_eq!(grid.pick_hex([i32::MIN, i32::MAX]), None);
    assert_eq!(grid.pick_square([i32::MIN, i32::MAX], true), None);
}

#[test]
fn spec003_extended_flag_permissions_by_flag_category_and_subtype() {
    let with = |category: u8, subtype: usize, extended: u32| {
        let mut bytes = fixture::prototype(category, subtype);
        bytes[0x18..0x1c].copy_from_slice(&extended.to_be_bytes());
        Prototype::parse(&bytes).unwrap()
    };
    // Containers permit use by subtype, but pickup only with 0x8000.
    let container = with(0, 1, 0);
    assert!(container.permits_use() && !container.permits_use_on());
    assert!(!container.permits_pickup() && !container.permits_talk());
    assert!(with(0, 1, 0x8000).permits_pickup());
    // Drugs permit use-on by subtype; other items need the flags.
    let drug = with(0, 2, 0);
    assert!(drug.permits_use_on() && !drug.permits_use() && drug.permits_pickup());
    let weapon = with(0, 3, 0);
    assert!(!weapon.permits_use() && !weapon.permits_use_on() && weapon.permits_pickup());
    assert!(with(0, 3, 0x800).permits_use() && with(0, 3, 0x1000).permits_use_on());
    // Scenery: use by flag only, talk by 0x4000 only, never pickup.
    let door = with(2, 0, 0);
    assert!(!door.permits_use() && !door.permits_talk() && !door.permits_pickup());
    assert!(with(2, 0, 0x800).permits_use() && with(2, 5, 0x4000).permits_talk());
    // Critters talk by category; the 0x8000 pickup bit is container-only.
    let critter = with(1, 0, 0x8000);
    assert!(critter.permits_talk() && !critter.permits_pickup());
}

#[test]
fn spec003_prototype_writer_round_trips_in_reader_order() {
    // Fixture A and every category without a write-side rule round-trip exactly.
    let container = fixture::container();
    assert_eq!(Prototype::parse(&container).unwrap().write(), container);
    for (category, subtype) in [
        (0, 0),
        (0, 2),
        (0, 4),
        (0, 5),
        (0, 6),
        (1, 0),
        (3, 0),
        (4, 0),
        (5, 0),
    ] {
        let bytes = fixture::prototype(category, subtype);
        assert_eq!(
            Prototype::parse(&bytes).unwrap().write(),
            bytes,
            "{category}/{subtype}"
        );
    }
    // Weapons read minimum at payload 0x04 / maximum at 0x08; the safe-rule
    // writer keeps that order, so a save/reload keeps the bounds.
    let mut weapon = fixture::prototype(0, 3);
    fixture::word(&mut weapon, 57 + 4, 2);
    fixture::word(&mut weapon, 57 + 8, 7);
    let parsed = Prototype::parse(&weapon).unwrap();
    let PrototypeData::Item(Item {
        data: ItemData::Weapon(read),
        ..
    }) = &parsed.data
    else {
        panic!("weapon")
    };
    assert_eq!(read.damage, [2, 7]);
    assert_eq!(parsed.write(), weapon);
    // Scenery omits the reference writer's 24-byte suffix (radius,
    // intensity, flags, extended flags, script, subtype); readers still
    // tolerate it after the consumed payload.
    for subtype in 0..6 {
        let mut scenery = fixture::prototype(2, subtype);
        fixture::word(&mut scenery, 12, 3);
        fixture::word(&mut scenery, 16, 0x8000);
        fixture::word(&mut scenery, 20, 0x10);
        fixture::word(&mut scenery, 24, 0x800);
        fixture::word(&mut scenery, 28, 9);
        let parsed = Prototype::parse(&scenery).unwrap();
        assert_eq!(parsed.write(), scenery);
        let mut suffixed = scenery.clone();
        suffixed.extend(fixture::words(&[3, 0x8000, 0x10, 0x800, 9, subtype as i32]));
        let reread = Prototype::parse(&suffixed).unwrap();
        assert_eq!(
            (reread.consumed_len, reread.data),
            (scenery.len(), parsed.data)
        );
    }
}

fn fixture_c_map() -> Vec<u8> {
    let mut bytes = fixture::empty_map();
    bytes.truncate(bytes.len() - 16);
    bytes.extend(fixture::inventory());
    bytes
}

#[test]
fn spec003_map_writer_round_trips_and_recomputes_omission_bits() {
    // Fixture B: flags 0x0F stay correct and darkness is already 1.
    let empty = fixture::empty_map();
    assert_eq!(
        MapFile::parse(&empty, &lookup).unwrap().write().unwrap(),
        empty
    );
    // Changing the stored darkness word: writing always sets it to 1.
    let mut dark = empty.clone();
    fixture::word(&mut dark, 0x2c, 9);
    assert_eq!(
        MapFile::parse(&dark, &lookup).unwrap().write().unwrap(),
        empty
    );

    // Fixture C's elevation-0 object clears that elevation's omission bit, so a
    // blank 40000-byte block is written; the child's stored elevation 2 becomes
    // the enclosing elevation 0. Object counts for all elevations remain.
    let source = fixture_c_map();
    let map = MapFile::parse(&source, &lookup).unwrap();
    let written = map.write().unwrap();
    let mut expected = source[..0xf8].to_vec();
    fixture::word(&mut expected, 0x28, 0x0d);
    for _ in 0..10000 {
        expected.extend(0x0001_0001_u32.to_be_bytes());
    }
    let mut objects = source[0xf8..].to_vec();
    fixture::word(&mut objects, 20 + 0x8c, 0);
    expected.extend(objects);
    assert_eq!(written, expected);
    let reread = MapFile::parse(&written, &lookup).unwrap();
    assert_eq!(reread.header.flags, MapFlags(0x0d));
    let parent = &reread.objects.elevations[0][0];
    assert_eq!((parent.id, parent.inventory[0].quantity), (42, 3));
    assert_eq!(parent.inventory[0].object.id, 43);
}

#[test]
fn spec003_map_writer_grid_order_exclusions_and_script_compaction() {
    let mut map = MapFile::parse(&fixture_c_map(), &lookup).unwrap();
    let template = map.objects.elevations[0][0].clone();
    let placed = |id: u32, hex: Option<i32>, flags: u32| {
        let mut object = template.clone();
        object.id = id;
        object.hex = hex;
        object.flags = BasicFlags(flags);
        object.inventory.clear();
        object
    };
    map.objects.elevations[0] = vec![
        placed(3, Some(300), 0x10),
        placed(1, Some(100), 0x10),
        placed(9, Some(50), 0x14), // temporary: not saved
        placed(8, None, 0x10),     // unplaced: not found by the grid traversal
        placed(2, Some(200), 0x10),
        placed(4, Some(100), 0x10), // same hex keeps its relative order
    ];
    map.objects.elevations[2] = vec![placed(5, Some(7), 0x14)];
    let script = |serial: u32, flags: u32| MapScript {
        id: (3 << 24) | serial,
        link: 0,
        prefix: ScriptPrefix::None,
        stored_flags: ScriptFlags(flags),
        flags: ScriptFlags(flags),
        file_index: 0,
        object_id: -1,
        local_base: -1,
        stored_local_count: 0,
        local_count: 0,
        result: 0,
        event: 0,
        parameter: 0,
        action: 0,
        override_default: 0,
        execution_state: 0,
        quantity: 0,
        annotations: 0,
        file_offset: 0,
    };
    let slots: Vec<_> = (1..=18)
        .map(|serial| script(serial, if serial % 3 == 0 { 8 } else { 0 }))
        .collect();
    map.scripts[3] = ScriptCategory {
        declared_count: 18,
        blocks: vec![
            ScriptBlock {
                slots: slots[..16].to_vec(),
                active: 16,
            },
            ScriptBlock {
                slots: slots[16..].to_vec(),
                active: 2,
            },
        ],
    };
    let written = map.write().unwrap();
    let reread = MapFile::parse(&written, &lookup).unwrap();
    let ids: Vec<_> = reread.objects.elevations[0].iter().map(|o| o.id).collect();
    assert_eq!(ids, [1, 4, 2, 3]);
    // Elevation 2 had only a temporary object, so its block stays omitted.
    assert!(reread.objects.elevations[2].is_empty());
    assert_eq!(reread.header.flags, MapFlags(0x0d));
    assert_eq!(reread.objects.total, 4);
    // Twelve of eighteen item scripts are retained and compacted: 12 records,
    // one block whose four padding slots are inactive.
    let retained: Vec<_> = reread.scripts[3]
        .active()
        .map(|s| s.id & 0xffffff)
        .collect();
    assert_eq!(retained, [1, 2, 4, 5, 7, 8, 10, 11, 13, 14, 16, 17]);
    assert_eq!(reread.scripts[3].blocks.len(), 1);
    // A temporary inventory child is skipped entirely (safe rule): no
    // quantity, no record, and only the written entries are counted.
    let mut child = map.clone();
    child.objects.elevations[0] = vec![template.clone()];
    let entries = template.inventory.len();
    child.objects.elevations[0][0].inventory[0].object.flags = BasicFlags(0x4);
    let reread = MapFile::parse(&child.write().unwrap(), &lookup).unwrap();
    let parent = &reread.objects.elevations[0][0];
    assert_eq!(parent.inventory.len(), entries - 1);
    assert!(
        parent
            .inventory
            .iter()
            .all(|entry| !entry.object.flags.contains(BasicFlags::TEMPORARY))
    );
}

/// SPEC-003 over the real corpus: every shipped `.map` parses against the
/// corpus prototypes, with safe-rule substitutions reported as diagnostics
/// rather than errors.
#[test]
#[ignore = "needs the local corpus"]
fn corpus_every_map_parses() {
    let Some(corpus) = crate::tests::corpus_dir() else {
        return;
    };
    let vfs = LocalDirProvider::mount(corpus.join("fallout1-unpacked")).unwrap();
    let catalog = LocalPrototypeCatalog::new(&vfs);
    let maps: Vec<&VfsPath> = vfs
        .files()
        .filter(|path| path.as_str().starts_with("maps/") && path.as_str().ends_with(".map"))
        .collect();
    let mut failures = Vec::new();
    let mut diagnostics = 0;
    for path in &maps {
        match MapFile::parse(&vfs.read(path.as_str()).unwrap(), &catalog) {
            Ok(map) => {
                diagnostics += map.diagnostics.len();
                for line in &map.diagnostics {
                    eprintln!("  {}: {line}", path.as_str());
                }
            }
            Err(error) => failures.push(format!("{}: {error}", path.as_str())),
        }
    }
    eprintln!(
        "{} corpus maps, {} failures, {diagnostics} diagnostics",
        maps.len(),
        failures.len()
    );
    for line in &failures {
        eprintln!("  {line}");
    }
    assert_eq!(maps.len(), 72);
    assert!(failures.is_empty(), "{} maps failed", failures.len());
}
