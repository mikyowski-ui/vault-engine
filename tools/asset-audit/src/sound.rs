//! SPEC-010 corpus checks against the oracle tables: ACM decoding (source
//! and PCM hashes, header values, counters, edge samples, and every wrapped
//! sample), effect-name construction and lookup, and prototype sound bytes.
use sha2::{Digest, Sha256};
use std::{
    collections::{BTreeMap, BTreeSet},
    error::Error,
    path::Path,
};
use vault_formats::{
    LocalDirProvider, LstKind, LstTable, MAX_ASSET_BYTES, Prototype, Vfs, acm,
    pro::{ItemData, PrototypeData},
    sfx::{self, Opened, Owner, Target, effect},
};

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}
/// Tab-separated rows, skipping `#` comment lines.
fn table(path: &Path) -> Result<Vec<Vec<String>>, Box<dyn Error>> {
    let text =
        std::fs::read_to_string(path).map_err(|error| format!("{}: {error}", path.display()))?;
    Ok(text
        .lines()
        .filter(|line| !line.is_empty() && !line.starts_with('#'))
        .map(|line| line.split('\t').map(str::to_owned).collect())
        .collect())
}
fn field(row: &[String], index: usize) -> Result<&str, String> {
    row.get(index)
        .map(String::as_str)
        .ok_or_else(|| format!("oracle row has no column {index}"))
}
fn joined(samples: &[i16]) -> String {
    samples
        .iter()
        .map(i16::to_string)
        .collect::<Vec<_>>()
        .join(",")
}

/// One `acm-corpus.tsv` row: every listed value must match the decoder.
/// Returns the file's wrapped-sample count.
fn check_acm(
    vfs: &LocalDirProvider,
    row: &[String],
    wraps: &[(usize, i32, i32)],
) -> Result<usize, String> {
    let path = field(row, 0)?;
    let bytes = vfs
        .read_bounded(path, MAX_ASSET_BYTES)
        .map_err(|error| error.to_string())?;
    if hex(&Sha256::digest(&bytes)) != field(row, 1)? {
        return Err("source bytes differ from the oracle's asset".into());
    }
    let decoded = acm::decode(&bytes).map_err(|error| error.to_string())?;
    let header = decoded.header;
    let observed = [
        bytes.len().to_string(),
        header.samples.to_string(),
        header.channels.to_string(),
        header.rate.to_string(),
        header.levels.to_string(),
        header.rows.to_string(),
        decoded.blocks.to_string(),
        decoded.past_end.to_string(),
        hex(&Sha256::digest(decoded.pcm())),
        joined(&decoded.samples[..decoded.samples.len().min(8)]),
        joined(&decoded.samples[decoded.samples.len().saturating_sub(4)..]),
    ];
    const NAMES: [&str; 11] = [
        "file bytes",
        "sample count",
        "header channels",
        "header rate",
        "levels",
        "rows",
        "blocks",
        "zero bytes past EOF",
        "PCM SHA-256",
        "first samples",
        "last samples",
    ];
    for (index, value) in observed.iter().enumerate() {
        let expected = field(row, index + 2)?;
        if value != expected {
            return Err(format!("{}: {value} != {expected}", NAMES[index]));
        }
    }
    let wrapped: Vec<_> = decoded
        .wrapped
        .iter()
        .map(|(index, shifted)| (*index, *shifted, i32::from(decoded.samples[*index])))
        .collect();
    if wrapped != wraps {
        return Err(format!("wrapped samples {wrapped:?} != {wraps:?}"));
    }
    Ok(wrapped.len())
}

/// `[rows, passed, failed, corpus ACM files, wrapped samples matched]`.
pub fn audit_acm(vfs: &LocalDirProvider, oracle: &Path) -> Result<[u64; 5], Box<dyn Error>> {
    let rows = table(&oracle.join("acm-corpus.tsv"))?;
    let mut wraps: BTreeMap<String, Vec<(usize, i32, i32)>> = BTreeMap::new();
    for row in table(&oracle.join("acm-wrap.tsv"))? {
        wraps.entry(field(&row, 0)?.to_owned()).or_default().push((
            field(&row, 1)?.parse()?,
            field(&row, 2)?.parse()?,
            field(&row, 3)?.parse()?,
        ));
    }
    let listed: BTreeSet<_> = rows.iter().filter_map(|row| row.first().cloned()).collect();
    let corpus: Vec<_> = vfs
        .files()
        .filter(|path| path.extension() == Some("acm"))
        .map(|path| path.as_str().to_owned())
        .collect();
    let mut counts = [rows.len() as u64, 0, 0, corpus.len() as u64, 0];
    for path in corpus.iter().filter(|path| !listed.contains(*path)) {
        counts[2] += 1;
        eprintln!("FAIL {path}: no oracle row");
    }
    for row in &rows {
        let path = field(row, 0)?;
        match check_acm(vfs, row, wraps.get(path).map_or(&[][..], Vec::as_slice)) {
            Ok(wrapped) => {
                counts[1] += 1;
                counts[4] += wrapped as u64;
            }
            Err(error) => {
                counts[2] += 1;
                eprintln!("FAIL {path}: {error}");
            }
        }
    }
    let expected_wraps: usize = wraps.values().map(Vec::len).sum();
    if counts[4] != expected_wraps as u64 && counts[2] == 0 {
        counts[2] += 1;
        eprintln!(
            "FAIL acm-wrap.tsv: {expected_wraps} rows, {} matched",
            counts[4]
        );
    }
    Ok(counts)
}

/// The six character effects the oracle samples per critter list: stand,
/// dodge, fall back (plain, pass out, die), and punch contact.
const CHARACTER_EFFECTS: [(u8, u8); 6] = [(0, 0), (13, 0), (20, 0), (20, 2), (20, 3), (16, 4)];
/// The eight weapon effects the oracle samples per sound byte.
fn weapon_names(sound: u8) -> Vec<Vec<u8>> {
    let hit = |target| sfx::material_letter(effect::HIT, target, 0);
    [
        (effect::READY, 2, b'X'),
        (effect::ATTACK, 2, b'X'),
        (effect::ATTACK, 3, b'X'),
        (effect::OUT_OF_AMMO, 2, b'X'),
        (effect::FLIGHT, 2, b'X'),
        (effect::HIT, 2, hit(Target::None)),
        (effect::HIT, 2, hit(Target::Material(1))),
        (effect::HIT, 2, hit(Target::Other)),
    ]
    .into_iter()
    .filter_map(|(kind, mode, material)| sfx::weapon_name(kind, sound, mode, material))
    .collect()
}

/// One `sfx-resolution.tsv` row: the requested name must come from the
/// builder for its context, and the lookup must try and resolve the same
/// stems (case-insensitively).
fn check_resolution(
    row: &[String],
    critters: &LstTable,
    exists: &impl Fn(&[u8]) -> bool,
) -> Result<(), String> {
    let (kind, context, requested) = (field(row, 0)?, field(row, 1)?, field(row, 2)?);
    let female = context.ends_with(" female");
    let owner = match kind {
        "character" => {
            let index: u32 = context
                .trim_start_matches("list ")
                .trim_end_matches(" female")
                .parse()
                .map_err(|_| format!("bad context {context}"))?;
            let stem = critters
                .art_id(index)
                .and_then(|entry| entry.name.clone())
                .ok_or_else(|| format!("critter list has no row {index}"))?;
            let built: Vec<_> = CHARACTER_EFFECTS
                .iter()
                .filter_map(|(animation, extra)| sfx::character_name(&stem, *animation, 0, *extra))
                .collect();
            if !built.iter().any(|name| name == requested.as_bytes()) {
                return Err(format!("{requested} is not built for {context}"));
            }
            Some(Owner {
                critter: true,
                female,
            })
        }
        "weapon" => {
            let sound = context
                .strip_prefix("code ")
                .and_then(|code| code.bytes().next())
                .ok_or_else(|| format!("bad context {context}"))?;
            if !weapon_names(sound)
                .iter()
                .any(|name| name == requested.as_bytes())
            {
                return Err(format!("{requested} is not built for {context}"));
            }
            None
        }
        "literal" | "alias" => None,
        _ => return Err(format!("unknown kind {kind}")),
    };
    let found = sfx::lookup(requested.as_bytes(), owner, exists);
    let attempts: Vec<&str> = field(row, 4)?.split(" > ").collect();
    let same = found.attempts.len() == attempts.len()
        && found
            .attempts
            .iter()
            .zip(&attempts)
            .all(|(tried, listed)| tried.eq_ignore_ascii_case(listed.as_bytes()));
    if !same {
        return Err(format!(
            "attempts {:?} != {attempts:?}",
            found
                .attempts
                .iter()
                .map(|stem| String::from_utf8_lossy(stem).into_owned())
                .collect::<Vec<_>>()
        ));
    }
    let resolved = field(row, 3)?;
    let matches = match &found.resolved {
        Some(stem) => stem.eq_ignore_ascii_case(resolved.as_bytes()),
        None => resolved == "-",
    };
    if !matches {
        return Err(format!("resolved {:?} != {resolved}", found.resolved));
    }
    Ok(())
}

fn byte_label(byte: u8) -> String {
    if byte == 0 {
        "0".into()
    } else {
        char::from(byte).to_string()
    }
}

/// `sfx-proto-bytes.tsv`: the item (0x38), scenery (0x28), and weapon
/// (0x79) sound-byte distributions over the corpus prototypes, and which
/// open/close names built from them exist. Returns failures.
fn check_proto_bytes(
    vfs: &LocalDirProvider,
    oracle: &Path,
    exists: &impl Fn(&[u8]) -> bool,
) -> Result<u64, Box<dyn Error>> {
    let mut counts: BTreeMap<(String, i32, String), u64> = BTreeMap::new();
    let mut weapons: BTreeMap<String, u64> = BTreeMap::new();
    for path in vfs.files() {
        let category = match path.as_str() {
            name if name.starts_with("proto/items/") => "items",
            name if name.starts_with("proto/scenery/") => "scenery",
            _ => continue,
        };
        if path.extension() != Some("pro") {
            continue;
        }
        let prototype = Prototype::parse(&vfs.read_bounded(path.as_str(), MAX_ASSET_BYTES)?)?;
        let (subtype, sound) = match &prototype.data {
            PrototypeData::Item(item) => {
                if let ItemData::Weapon(weapon) = &item.data {
                    *weapons.entry(byte_label(weapon.sound)).or_default() += 1;
                }
                (item.data.subtype() as i32, item.stored_byte)
            }
            PrototypeData::Scenery(scenery) => (scenery.data.subtype() as i32, scenery.stored_byte),
            _ => continue,
        };
        *counts
            .entry((category.to_owned(), subtype, byte_label(sound)))
            .or_default() += 1;
    }
    let mut failures = 0;
    let rows = table(&oracle.join("sfx-proto-bytes.tsv"))?;
    let mut listed = BTreeSet::new();
    for row in &rows {
        let (category, subtype, label) = (
            field(row, 0)?,
            field(row, 1)?.parse::<i32>()?,
            field(row, 2)?,
        );
        listed.insert((category.to_owned(), subtype, label.to_owned()));
        let count = counts
            .get(&(category.to_owned(), subtype, label.to_owned()))
            .copied()
            .unwrap_or(0);
        let expected: u64 = field(row, 3)?.parse()?;
        let sound = if label == "0" { 0 } else { label.as_bytes()[0] };
        let names: Vec<String> = (0..4)
            .map(|action| {
                let object = if category == "scenery" {
                    Opened::Scenery(Some(sound))
                } else {
                    Opened::Item(sound)
                };
                sfx::open_name(object, action).unwrap_or_default()
            })
            .filter(|name| sound != 0 && exists(name))
            .map(|name| String::from_utf8_lossy(&name).into_owned())
            .collect();
        let names = if names.is_empty() {
            "-".to_owned()
        } else {
            names.join(",")
        };
        if count != expected || names != field(row, 4)? {
            failures += 1;
            eprintln!("FAIL sfx-proto-bytes {category} {subtype} {label}: {count} {names}");
        }
    }
    for key in counts.keys().filter(|key| !listed.contains(*key)) {
        failures += 1;
        eprintln!("FAIL sfx-proto-bytes: corpus has unlisted {key:?}");
    }
    let text = std::fs::read_to_string(oracle.join("sfx-proto-bytes.tsv"))?;
    let distribution = text
        .lines()
        .find_map(|line| line.split_once("distribution: ").map(|(_, rest)| rest))
        .ok_or("no weapon distribution line")?;
    let observed = weapons
        .iter()
        .map(|(label, count)| format!("{label}:{count}"))
        .collect::<Vec<_>>()
        .join(" ");
    if observed != distribution.trim() {
        failures += 1;
        eprintln!("FAIL weapon sound bytes: {observed}");
    }
    Ok(failures)
}

/// `[rows, passed, failed, prototype-byte failures]`.
pub fn audit_sfx(vfs: &LocalDirProvider, oracle: &Path) -> Result<[u64; 4], Box<dyn Error>> {
    let effects: BTreeSet<Vec<u8>> = vfs
        .files()
        .filter(|path| path.as_str().starts_with("sound/sfx/") && path.extension() == Some("acm"))
        .filter_map(|path| {
            let name = path.as_str().strip_prefix("sound/sfx/")?;
            Some(name[..name.len() - 4].to_ascii_uppercase().into_bytes())
        })
        .collect();
    let exists = |stem: &[u8]| effects.contains(&stem.to_ascii_uppercase());
    let critters = LstTable::parse(
        &vfs.read_bounded("art/critters/critters.lst", MAX_ASSET_BYTES)?,
        LstKind::Art,
    )?;
    let rows = table(&oracle.join("sfx-resolution.tsv"))?;
    let mut counts = [rows.len() as u64, 0, 0, 0];
    for row in &rows {
        match check_resolution(row, &critters, &exists) {
            Ok(()) => counts[1] += 1,
            Err(error) => {
                counts[2] += 1;
                eprintln!("FAIL {}: {error}", row.join(" "));
            }
        }
    }
    counts[3] = check_proto_bytes(vfs, oracle, &exists)?;
    Ok(counts)
}
