//! SPEC-008 §4.3 header and the 27-component payload grammar.
use crate::{
    ParseError,
    map::{CritterState, InventoryEntry, MapObject, ObjectDetail, ObjectState},
    pro::{BasicFlags, CombatFlags, Light, Prototype, PrototypeInfo},
    savegame::*,
};

fn fixture_header() -> Vec<u8> {
    let mut bytes = vec![0; HEADER_LEN];
    for (offset, hex) in [
        (
            0x0000,
            "46 41 4C 4C 4F 55 54 20 53 41 56 45 20 46 49 4C 45 00 00 00 00 00 00 00",
        ),
        (0x0018, "00 01 00 01 52"),
        (0x001D, "53 79 6E 74 68 65 74 69 63 20 45 69 67 68 74 00"),
        (0x003D, "41 72 63 68 69 76 65 20 74 65 73 74 00"),
        (0x005B, "00 10 00 0A 07 EA 00 00 00 2E"),
        (0x0065, "00 0C 00 06 08 71 00 0D 2F 00 00 01 00 06"),
        (0x0073, "53 59 4E 54 48 2E 53 41 56 00"),
        (0x0083, "00 01 02 03 04 05 06 07"),
        (0x74E2, "7F"),
    ] {
        let replacement = crate::fixture_assets::hex(hex);
        bytes[offset..offset + replacement.len()].copy_from_slice(&replacement);
    }
    bytes
}

#[test]
fn spec008_fixture_complete_save_header() {
    let bytes = fixture_header();
    let header = SaveHeader::parse(&bytes).unwrap();
    assert_eq!((header.version, header.release), ([1, 1], b'R'));
    assert_eq!(header.name_text(), b"Synthetic Eight");
    assert_eq!(header.description_text(), b"Archive test");
    assert_eq!(header.host_date, [16, 10, 2026]);
    // Hour 12 plus minute 34, not HHMM 1234.
    assert_eq!(header.host_time, 46);
    assert_eq!(header.game_date, [12, 6, 2161]);
    assert_eq!(header.game_ticks, 864000);
    assert_eq!((header.elevation, header.map), (1, 6));
    assert_eq!(header.map_file_text(), b"SYNTH.SAV");
    assert_eq!(&header.thumbnail[..8], &[0, 1, 2, 3, 4, 5, 6, 7]);
    // Row 132, column 223 is the thumbnail's last byte, at 0x74E2.
    assert_eq!(header.thumbnail[132 * 224 + 223], 127);
    assert!(header.reserved.iter().all(|byte| *byte == 0));
    assert_eq!(header.write().unwrap(), bytes);
    // Only the first eighteen signature bytes are compared.
    let mut padded = bytes.clone();
    padded[0x12] = b'!';
    assert!(SaveHeader::parse(&padded).is_ok());
    let mut terminated = bytes.clone();
    terminated[0x11] = b'!';
    assert!(SaveHeader::parse(&terminated).is_err());
    // Another version still passes this header reader.
    let mut versioned = bytes.clone();
    versioned[0x19] = 9;
    assert_eq!(SaveHeader::parse(&versioned).unwrap().version, [9, 1]);
    // A strict validator rejects a missing reserved tail.
    assert!(SaveHeader::parse(&bytes[..0x74e3]).is_err());
}

fn lookup(pid: u32) -> Result<PrototypeInfo, ParseError> {
    assert_eq!(pid, 1);
    Ok(
        Prototype::parse(&crate::fixture_assets::sparse(include_str!(
            "../../../tests/fixtures/world/container.pro.hex"
        )))?
        .info(),
    )
}

fn player_tree() -> MapObject {
    let item = MapObject {
        id: 7,
        hex: None,
        displacement: [0; 2],
        screen: [0; 2],
        frame: 0,
        orientation: 0,
        art_id: 0,
        flags: BasicFlags(BasicFlags::TEMPORARY.0),
        stored_elevation: 1,
        elevation: 1,
        pid: 1,
        combat_id: -1,
        light: Light {
            radius: 0,
            intensity: 0,
        },
        outline: 0,
        script_id: 0x0300_0002,
        cached_script_file: -1,
        inventory_capacity: 0,
        state: ObjectState::Other {
            word: 0,
            detail: ObjectDetail::Plain,
        },
        inventory: Vec::new(),
        owner: Some(1),
        file_offset: 0,
    };
    MapObject {
        id: 1,
        hex: Some(20100),
        pid: 0x0100_0000,
        art_id: 0x0100_0000,
        flags: BasicFlags(BasicFlags::TEMPORARY.0),
        script_id: 0x0400_0001,
        inventory_capacity: 1,
        state: ObjectState::Critter(CritterState {
            reaction: 0,
            damage: 0,
            maneuvers: 0,
            ap: 8,
            results: CombatFlags(0),
            ai: 0,
            team: 0,
            attacker: -1,
            hp: 30,
            radiation: 0,
            poison: 0,
        }),
        inventory: vec![InventoryEntry {
            quantity: 2,
            object: item.clone(),
        }],
        owner: None,
        ..item
    }
}

fn synthetic_save(campaign: usize) -> SaveGame {
    SaveGame {
        header: SaveHeader {
            elevation: 1,
            ..SaveHeader::default()
        },
        player_combat_id: 3,
        campaign: (0..campaign as i32).collect(),
        campaign_status: 1,
        maps: vec![b"V13ENT.SAV".to_vec(), b"VAULT13.SAV".to_vec()],
        automap_length: 797,
        campaign_b: (100..100 + campaign as i32).collect(),
        campaign_b_status: 0,
        player: player_tree(),
        viewport: 20100,
        sneak_working: 1,
        stats: CritterStats {
            flags: 8,
            base: [5; 35],
            bonus: [1; 35],
            skills: [2; 18],
            body: 0,
            experience: 0,
            kill_category: 0,
        },
        kills: [1; 15],
        tags: [0, 3, -1, -1],
        perks: [0; 64],
        combat: CombatSave {
            state: 1,
            turn_running: 1,
            free_movement: 2,
            experience: 50,
            combatants: 2,
            noncombatants: 1,
            player_id: 3,
            list: vec![3, 0, 1],
        },
        progression: [4, 2, 1500, 3, -5],
        queue: vec![
            QueueRecord {
                due: 10,
                kind: 0,
                owner: 1,
                payload: QueuePayload::Drug {
                    stats: [0, 1, 2],
                    modifiers: [1, -1, 2],
                },
            },
            QueueRecord {
                due: 11,
                kind: 1,
                owner: 1,
                payload: QueuePayload::None,
            },
            QueueRecord {
                due: 12,
                kind: 2,
                owner: 1,
                payload: QueuePayload::Withdrawal([1, 0x00000020, 3]),
            },
            QueueRecord {
                due: 13,
                kind: 3,
                owner: 1,
                payload: QueuePayload::Script {
                    script: 0x0400_0001,
                    parameter: 7,
                },
            },
            QueueRecord {
                due: 14,
                kind: 6,
                owner: -2,
                payload: QueuePayload::Radiation {
                    level: 50,
                    healing: 0,
                },
            },
            QueueRecord {
                due: 15,
                kind: 12,
                owner: -2,
                payload: QueuePayload::None,
            },
        ],
        traits: [3, -1],
        automap_flags: 0x3,
        preferences: std::array::from_fn(|index| match index {
            13 => 1.5_f32.to_bits(),
            18 => 1.0_f32.to_bits(),
            19 => 0.25_f32.to_bits(),
            other => other as u32,
        }),
        reconciled_level: 2,
        pending_perk: 1,
        world_map: WorldMapSave {
            discovery: (0..899).map(|i| (i % 3) as u8).collect(),
            entrances: vec![1; 105],
            towns: 0b101,
            specials: 0x21,
            town: 0,
            section: 1,
            position: [825, 75],
        },
        movies: [1; 14],
        skill_uses: [[1, 2, 3]; 18],
        party_count: 3,
        next_item_id: 4000,
        party: vec![11, 12],
        interface: [1, 0, 1, 0],
    }
}

#[test]
fn spec008_save_payload_grammar_round_trips_all_components() {
    let save = synthetic_save(5);
    let bytes = save.write().unwrap();
    let parsed = SaveGame::parse(&bytes, 5, &lookup).unwrap();
    assert_eq!(parsed.write().unwrap(), bytes);
    assert_eq!(parsed.campaign, [0, 1, 2, 3, 4]);
    assert_eq!(
        parsed.maps,
        [b"V13ENT.SAV".to_vec(), b"VAULT13.SAV".to_vec()]
    );
    assert_eq!(parsed.combat.list, [3, 0, 1]);
    assert_eq!(parsed.queue, save.queue);
    assert_eq!(parsed.party, [11, 12]);
    assert_eq!(parsed.preference_float(13), Some(1.5));
    assert_eq!(parsed.preference_float(12), None);
    assert_eq!(parsed.world_map, save.world_map);
    // The player tree is written with temporary clear and script -1.
    assert_eq!(parsed.player.flags.0 & BasicFlags::TEMPORARY.0, 0);
    assert_eq!(parsed.player.script_id, -1);
    assert_eq!(parsed.player.inventory[0].object.script_id, -1);
    assert_eq!(parsed.player.inventory[0].quantity, 2);
    // G is not stored: another installed campaign count misparses.
    assert!(SaveGame::parse(&bytes, 6, &lookup).is_err());
    assert!(SaveGame::parse(&bytes, 4, &lookup).is_err());
    // Truncation anywhere in the payload is rejected.
    for end in [HEADER_LEN + 3, bytes.len() - 1] {
        assert!(SaveGame::parse(&bytes[..end], 5, &lookup).is_err());
    }
    // An inactive combat component is the four-byte state word alone.
    let mut quiet = synthetic_save(5);
    quiet.combat.state = 2;
    let shorter = quiet.write().unwrap();
    assert_eq!(bytes.len() - shorter.len(), 7 * 4 + 3 * 4);
    assert_eq!(
        SaveGame::parse(&shorter, 5, &lookup).unwrap().combat.list,
        Vec::<i32>::new()
    );
}

#[test]
fn spec008_save_manifest_names_and_queue_types_are_bounded() {
    let mut long = synthetic_save(1);
    long.maps = vec![b"THIRTEENBYTES".to_vec()];
    assert!(long.write().is_err());
    // A 13-byte name before its NUL fails the safe parser.
    let mut save = synthetic_save(1);
    save.maps = vec![b"TWELVE__.SAV".to_vec()];
    let mut bytes = save.write().unwrap();
    let name = HEADER_LEN + 4 + 4 + 1 + 4;
    assert_eq!(&bytes[name..name + 12], b"TWELVE__.SAV");
    bytes.insert(name, b'X');
    assert!(SaveGame::parse(&bytes, 1, &lookup).is_err());
    // An empty manifest is not a normal save.
    let mut empty = synthetic_save(1);
    empty.maps.clear();
    assert!(empty.write().is_err());
    // Unknown timed-event types have no defined payload.
    let mut unknown = synthetic_save(1);
    unknown.queue = vec![QueueRecord {
        due: 1,
        kind: 13,
        owner: -2,
        payload: QueuePayload::None,
    }];
    assert!(unknown.write().is_err());
}
