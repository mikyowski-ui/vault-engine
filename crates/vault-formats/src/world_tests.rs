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
    assert_eq!(ExtendedFlags(0x98).attack_modes(), None);
    assert_eq!(ExtendedFlags(0x87).attack_modes(), Some([7, 8]));
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
    fixture::word(&mut unsaved, 40, 0x0d);
    assert!(MapFile::parse(&unsaved, &lookup).is_err());
    let mut bad = bytes;
    fixture::word(&mut bad, 248 + 40000, -1);
    assert!(MapFile::parse(&bad, &lookup).is_err());
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
    assert_eq!(
        HexGrid::angular_distance(0, 199),
        Err(GeometryError::AngularWalk)
    );
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
    assert_eq!(catalog.lookup(1).unwrap(), catalog.lookup(2).unwrap());
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
