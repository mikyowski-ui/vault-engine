//! Synthetic SPEC-008 (Fallout 1) DAT archives, raw (uncompressed)
//! members only, and a game folder holding both databases.

/// An archive member: its name and bytes.
pub type Member<'a> = (&'a str, &'a [u8]);

/// A raw (uncompressed) synthetic DAT archive: directories with members.
pub fn archive(directories: &[(&str, &[Member])]) -> Vec<u8> {
    let count = directories.len() as u32;
    let mut tables: Vec<u8> = [count, count, 0, 0]
        .iter()
        .flat_map(|word| word.to_be_bytes())
        .collect();
    for (name, _) in directories {
        tables.push(name.len() as u8);
        tables.extend(name.as_bytes());
    }
    let table_len = tables.len()
        + directories
            .iter()
            .map(|(_, files)| 16 + files.iter().map(|(name, _)| 17 + name.len()).sum::<usize>())
            .sum::<usize>();
    let mut offset = table_len as u32;
    let mut payload: Vec<u8> = Vec::new();
    for (_, files) in directories {
        let count = files.len() as u32;
        for word in [count, count, 16, 0] {
            tables.extend(word.to_be_bytes());
        }
        for (name, body) in *files {
            tables.push(name.len() as u8);
            tables.extend(name.as_bytes());
            for word in [0x20, offset, body.len() as u32, body.len() as u32] {
                tables.extend(word.to_be_bytes());
            }
            offset += body.len() as u32;
            payload.extend(*body);
        }
    }
    tables.extend(payload);
    tables
}

/// A MASTER.DAT holding the three members a new game reads first
/// (`color.pal`, `data/vault13.gam`, `maps/v13ent.map`), with placeholder
/// bytes.
pub fn master_archive() -> Vec<u8> {
    archive(&[
        (".", &[("COLOR.PAL", b"pal")]),
        ("DATA", &[("VAULT13.GAM", b"gam")]),
        ("MAPS", &[("V13ENT.MAP", b"map")]),
    ])
}

/// A CRITTER.DAT with one member under `art/critters/`.
pub fn critter_archive() -> Vec<u8> {
    archive(&[("ART\\CRITTERS", &[("HERO.FRM", b"frm")])])
}

/// A Fallout 1 game folder: `MASTER.DAT` and `CRITTER.DAT` in `dir`, which
/// the launcher's checks accept.
pub fn fallout1_install(dir: &std::path::Path) {
    std::fs::write(dir.join("MASTER.DAT"), master_archive()).unwrap();
    std::fs::write(dir.join("CRITTER.DAT"), critter_archive()).unwrap();
}
