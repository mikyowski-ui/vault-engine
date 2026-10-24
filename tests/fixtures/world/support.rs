pub use crate::formats as assets;
pub fn container() -> Vec<u8> {
    assets::sparse(include_str!("container.pro.hex"))
}
pub fn empty_map() -> Vec<u8> {
    assets::sparse(include_str!("empty.map.hex"))
}
pub fn inventory() -> Vec<u8> {
    assets::sparse(include_str!("inventory.hex"))
}
pub fn word(bytes: &mut [u8], offset: usize, value: i32) {
    bytes[offset..offset + 4].copy_from_slice(&value.to_be_bytes());
}
pub fn words(values: &[i32]) -> Vec<u8> {
    values
        .iter()
        .flat_map(|value| value.to_be_bytes())
        .collect()
}
pub fn prototype(category: u8, subtype: usize) -> Vec<u8> {
    let length = match category {
        0 => [129, 65, 125, 122, 81, 69, 61][subtype],
        1 => 412,
        2 => [49, 49, 49, 45, 45, 45][subtype],
        3 => 36,
        4 | 5 => 28,
        _ => panic!("fixture category"),
    };
    let mut bytes = vec![0; length];
    word(&mut bytes, 0, ((category as i32) << 24) | 1);
    word(&mut bytes, 4, 100);
    word(&mut bytes, 8, (category as i32) << 24);
    if category == 0 || category == 2 {
        word(&mut bytes, 0x20, subtype as i32);
        bytes[if category == 0 { 0x38 } else { 0x28 }] = 0xa5;
    }
    bytes
}
pub fn script_slot(category: u8, serial: u32) -> Vec<u8> {
    let length = match category {
        1 => 72,
        2 => 68,
        _ => 64,
    };
    let mut slot = vec![0; length];
    word(&mut slot, 0, (((category as u32) << 24) | serial) as i32);
    let tail = match category {
        1 => 16,
        2 => 12,
        _ => 8,
    };
    word(&mut slot, tail, 1);
    word(&mut slot, tail + 16, -1);
    slot
}
pub fn object(category: u8, hex: i32, id: u32, extra: &[i32]) -> Vec<u8> {
    let mut record = vec![0; 84];
    word(&mut record, 0, id as i32);
    word(&mut record, 4, hex);
    word(&mut record, 0x20, (category as i32) << 24);
    word(&mut record, 0x2c, ((category as i32) << 24) | 1);
    word(&mut record, 0x40, -1);
    word(&mut record, 0x44, -1);
    if category == 1 {
        record.extend(words(&[0; 11]));
    } else {
        record.extend(words(&[0]));
        record.extend(words(extra));
    }
    record
}
pub fn frm(width: u16, height: u16, color: u8) -> Vec<u8> {
    let mut bytes = assets::shared_frm();
    bytes.truncate(62);
    bytes[10..34].fill(0);
    word(&mut bytes, 58, 12 + i32::from(width) * i32::from(height));
    bytes.extend(width.to_be_bytes());
    bytes.extend(height.to_be_bytes());
    bytes.extend((u32::from(width) * u32::from(height)).to_be_bytes());
    bytes.extend([0; 4]);
    bytes.extend(std::iter::repeat_n(color, width as usize * height as usize));
    bytes
}
/// A small viewable map with each requested visible category and three elevations.
pub fn preview_map() -> Vec<u8> {
    let mut bytes = empty_map();
    bytes.truncate(248);
    word(&mut bytes, 0x28, 1);
    for elevation in 0..3 {
        for index in 0..10000 {
            let tile = if index == 5050 + elevation {
                0x00020002_i32
            } else {
                0x00010001
            };
            bytes.extend(tile.to_be_bytes());
        }
    }
    bytes.extend(words(&[0; 5]));
    bytes.extend(words(&[5, 5]));
    for (category, hex, id) in [
        (0, 20100, 1),
        (1, 20099, 2),
        (2, 20301, 3),
        (3, 20101, 4),
        (0, 20300, 5),
    ] {
        bytes.extend(object(category, hex, id, &[]));
    }
    bytes.extend(words(&[0, 0]));
    bytes
}
pub fn write_corpus(root: &std::path::Path) {
    assets::write_corpus(root);
    std::fs::create_dir_all(root.join("maps")).unwrap();
    std::fs::write(root.join("maps/synthetic.map"), preview_map()).unwrap();
    // SPEC-008 §3.10: a map's number comes from `map.msg` by its header
    // name, so the fixture map (header `SYNTH.MAP`, word 7) is map 7.
    std::fs::create_dir_all(root.join("text/english/game")).unwrap();
    std::fs::write(
        root.join("text/english/game/map.msg"),
        b"{7}{}{SYNTH.MAP}\n",
    )
    .unwrap();
    for (category, folder) in ["items", "critters", "scenery", "walls", "tiles", "misc"]
        .into_iter()
        .enumerate()
    {
        std::fs::create_dir_all(root.join(format!("proto/{folder}"))).unwrap();
        std::fs::write(
            root.join(format!("proto/{folder}/{folder}.lst")),
            b"synthetic.pro\n",
        )
        .unwrap();
        let subtype = match category {
            0 => 1,
            2 => 5,
            _ => 0,
        };
        std::fs::write(
            root.join(format!("proto/{folder}/synthetic.pro")),
            prototype(category as u8, subtype),
        )
        .unwrap();
        std::fs::create_dir_all(root.join(format!("art/{folder}"))).unwrap();
        // Critter rows are stems plus an alias; SPEC-007 appends the animation
        // suffix (stand, no weapon: `aa`). Other categories list filenames.
        let (names, file) = match category {
            1 => (b"sample,0\n".as_slice(), "sampleaa.frm"),
            4 => (b"tile.frm\nblank.frm\ntile.frm\n".as_slice(), "tile.frm"),
            _ => (b"sample.frm\n".as_slice(), "sample.frm"),
        };
        std::fs::write(root.join(format!("art/{folder}/{folder}.lst")), names).unwrap();
        let (width, height, color) = if category == 4 {
            (80, 36, 7)
        } else {
            (12, 20, 15 + category as u8)
        };
        std::fs::write(
            root.join(format!("art/{folder}/{file}")),
            frm(width, height, color),
        )
        .unwrap();
        if category == 1 {
            // The ground and middle reaches of SPEC-007 §3.11.3.
            for reach in ["sampleak.frm", "sampleal.frm"] {
                std::fs::write(root.join("art/critters").join(reach), frm(12, 20, 16)).unwrap();
            }
        }
        if category == 4 {
            // Index 1 is the ordinary blank tile; like shipped data it is drawn
            // but entirely transparent.
            std::fs::write(root.join("art/tiles/blank.frm"), frm(80, 36, 0)).unwrap();
        }
    }
}
