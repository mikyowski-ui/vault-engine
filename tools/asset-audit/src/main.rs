use std::{
    collections::BTreeSet,
    error::Error,
    path::{Path, PathBuf},
    process::ExitCode,
    sync::Arc,
};

use clap::Parser;
use vault_core::{
    CharacterSheet, WorldHost, WorldState,
    ai::{Taunt, taunt_range},
    assets::VfsWorldAssets,
    host_save::SaveInfo,
    worldmap,
};
use vault_formats::{
    ArtCatalog, Fid, LocalDirProvider, LocalPrototypeCatalog, MAX_ASSET_BYTES, MapFile, Prototype,
    Vfs, VfsPath,
    map::MapObject,
    pro::{BasicFlags, ItemData, PrototypeData},
    savegame::SaveGame,
    validate_asset,
};
use vault_vm::IntProgram;

const EXTENSIONS: [&str; 11] = [
    "frm", "pal", "fon", "aaf", "pro", "map", "int", "msg", "lst", "gam", "gcd",
];
const VALIDATED: [&str; 17] = [
    "pal", "frm", "fr0", "fr1", "fr2", "fr3", "fr4", "fr5", "lst", "fon", "aaf", "msg", "pro",
    "map", "int", "gam", "gcd",
];

#[derive(Parser)]
#[command(about = "Count and optionally validate assets in a loose game directory")]
struct Args {
    /// Unpacked directory containing art/, data/, color.pal, fonts, etc.
    #[arg(long, value_name = "PATH")]
    corpus_dir: PathBuf,
    /// Parse all supported formats, resolving MAP object subtypes through proto/ lists.
    #[arg(long)]
    validate_formats: bool,
    /// Resolve every PRO/MAP art identifier through the SPEC-007 filename codec
    /// and require the selected file to exist.
    #[arg(long)]
    validate_art_ids: bool,
    /// Re-serialize every PRO and MAP through the SPEC-003 writers.
    #[arg(long)]
    validate_writers: bool,
    /// SPEC-008: check fixed campaign indices against the initializer, enter
    /// every map so it joins the working saved-map set, round-trip the
    /// central save grammar, and write/read a temporary save slot.
    #[arg(long)]
    validate_saves: bool,
    /// SPEC-009: load `combatai.msg` and `data/ai.txt` (every packet needs
    /// all 25 keys), resolve each critter prototype's default AI packet, and
    /// count taunt ranges without catalog lines. Only counts are printed.
    #[arg(long)]
    validate_ai: bool,
}

/// The SPEC-003 writer's expected PRO bytes: the consumed payload with
/// weapon damage bounds exchanged, plus the scenery writer suffix.
fn expected_pro(bytes: &[u8], prototype: &Prototype) -> Vec<u8> {
    let mut expected = bytes[..prototype.consumed_len].to_vec();
    match &prototype.data {
        PrototypeData::Item(item) if matches!(item.data, ItemData::Weapon(_)) => {
            let (minimum, maximum) = expected[61..69].split_at_mut(4);
            minimum.swap_with_slice(maximum);
        }
        PrototypeData::Scenery(scenery) => {
            let light = prototype.light.expect("scenery has light words");
            for word in [
                light.radius,
                light.intensity,
                prototype.flags.0 as i32,
                prototype.extended.0 as i32,
                prototype.script.unwrap_or(-1),
                scenery.data.subtype() as i32,
            ] {
                expected.extend_from_slice(&word.to_be_bytes());
            }
        }
        _ => {}
    }
    expected
}

fn saved_objects(map: &MapFile) -> usize {
    map.objects
        .elevations
        .iter()
        .flatten()
        .filter(|object| object.hex.is_some() && !object.flags.contains(BasicFlags::TEMPORARY))
        .count()
}

fn check_writer(
    path: &VfsPath,
    bytes: &[u8],
    prototypes: &LocalPrototypeCatalog<'_>,
) -> Result<(), String> {
    let text = |error: vault_formats::ParseError| error.to_string();
    if path.extension() == Some("pro") {
        let prototype = Prototype::parse(bytes).map_err(text)?;
        if prototype.write() != expected_pro(bytes, &prototype) {
            return Err("PRO writer output differs from the documented transform".into());
        }
        return Ok(());
    }
    let map = MapFile::parse(bytes, prototypes).map_err(text)?;
    let first = map.write().map_err(text)?;
    let reread = MapFile::parse(&first, prototypes).map_err(text)?;
    if reread.write().map_err(text)? != first {
        return Err("MAP writer is not a parse/write fixpoint".into());
    }
    let scripts = |map: &MapFile| -> usize {
        map.scripts
            .iter()
            .flat_map(|category| category.active())
            .filter(|script| script.flags.0 & 8 == 0)
            .count()
    };
    if saved_objects(&map) != reread.objects.total || scripts(&map) != scripts(&reread) {
        return Err("MAP writer lost saved objects or retained scripts".into());
    }
    Ok(())
}

/// Art identifiers carried by a prototype; -1 marks an absent optional image.
fn prototype_art(prototype: &Prototype) -> Vec<(&'static str, u32)> {
    let mut art = vec![("art", prototype.art_id)];
    match &prototype.data {
        PrototypeData::Item(item) => {
            art.push(("inventory art", item.inventory_art));
            if let ItemData::Armor(armor) = &item.data {
                art.push(("male armor art", armor.male_art as u32));
                art.push(("female armor art", armor.female_art as u32));
            }
        }
        PrototypeData::Critter(critter) => art.push(("head art", critter.head_art)),
        _ => {}
    }
    art.retain(|(_, fid)| *fid != u32::MAX);
    art
}

fn map_art(object: &MapObject, art: &mut Vec<(&'static str, u32)>) {
    art.push(("object art", object.art_id));
    for child in &object.inventory {
        map_art(&child.object, art);
    }
}

/// Per source (`.pro`, `.map`): identifiers whose decoded file exists, whose
/// decoded file is absent from this corpus, and that the codec rejects.
fn audit_art_ids(vfs: &LocalDirProvider) -> Result<[[u64; 3]; 2], Box<dyn Error>> {
    let catalog = ArtCatalog::load(vfs)?;
    let prototypes = LocalPrototypeCatalog::new(vfs);
    let files: BTreeSet<&str> = vfs.files().map(VfsPath::as_str).collect();
    let mut counts = [[0; 3]; 2];
    for path in vfs.files() {
        let (source, art) = match path.extension() {
            Some("pro") => {
                let bytes = vfs.read_bounded(path.as_str(), MAX_ASSET_BYTES)?;
                (0, prototype_art(&Prototype::parse(&bytes)?))
            }
            Some("map") => {
                let bytes = vfs.read_bounded(path.as_str(), MAX_ASSET_BYTES)?;
                let map = MapFile::parse(&bytes, &prototypes)?;
                let mut art = Vec::new();
                for object in map.objects.elevations.iter().flatten() {
                    map_art(object, &mut art);
                }
                (1, art)
            }
            _ => continue,
        };
        for (field, fid) in art {
            match catalog
                .path(Fid(fid))
                .map_err(|error| error.to_string())
                .and_then(|art| VfsPath::new(&art).map_err(|error| error.to_string()))
            {
                Ok(art) if files.contains(art.as_str()) => counts[source][0] += 1,
                Ok(art) => {
                    counts[source][1] += 1;
                    eprintln!("MISSING {path}: {field} {fid:#010x}: {art}");
                }
                Err(error) => {
                    counts[source][2] += 1;
                    eprintln!("FAIL {path}: {field} {fid:#010x}: {error}");
                }
            }
        }
    }
    Ok(counts)
}

/// SPEC-008 corpus checks. Returns maps entered, working `.SAV` files that
/// reparse with the saved bit, and failures.
fn audit_saves(dir: &Path) -> Result<[u64; 3], Box<dyn Error>> {
    let vfs = Arc::new(LocalDirProvider::mount(dir)?);
    let prototypes = LocalPrototypeCatalog::new(&*vfs);
    let assets = VfsWorldAssets::new(vfs.clone(), "english")?;
    let mut host = WorldHost::new(WorldState::new(CharacterSheet::default(), 1)?, assets);
    host.load_campaign()?;
    let count = host.world.gvars.as_ref().map_or(0, Vec::len);
    let highest = worldmap::TOWNS
        .iter()
        .flat_map(|town| {
            town.entrance_globals
                .iter()
                .copied()
                .chain([town.known_global])
        })
        .chain(vault_guidance::QUESTS.iter().flatten().copied())
        .chain(vault_guidance::DISK_GLOBALS)
        .chain([
            worldmap::REGION_GLOBAL,
            worldmap::ENTRY_GLOBAL,
            worldmap::BASE_DESTROYED_GLOBAL,
            worldmap::CATHEDRAL_DESTROYED_GLOBAL,
            vault_guidance::WATER_CHIP_GLOBAL,
        ])
        .max()
        .unwrap_or(0);
    println!("Campaign globals: {count}; highest fixed SPEC-008 index: {highest}");
    if highest >= count {
        return Err(format!("campaign initializer lacks global {highest}").into());
    }
    let mut maps: Vec<_> = vfs
        .files()
        .filter(|path| path.extension() == Some("map"))
        .map(|path| path.as_str().to_string())
        .collect();
    maps.sort();
    let [mut entered, mut saved, mut failed] = [0_u64; 3];
    for path in &maps {
        match host.load_map(path) {
            Ok(()) => entered += 1,
            Err(error) => {
                failed += 1;
                eprintln!("FAIL {path}: {error}");
            }
        }
    }
    host.world.persist_current_map(true);
    for (name, bytes) in &host.world.persistence.maps {
        match MapFile::parse(bytes, &prototypes) {
            Ok(map) if map.header.flags.0 & 1 != 0 => saved += 1,
            Ok(_) => {
                failed += 1;
                eprintln!("FAIL {name}: saved-state bit clear");
            }
            Err(error) => {
                failed += 1;
                eprintln!("FAIL {name}: {error}");
            }
        }
    }
    let save = host.build_save(&SaveInfo::default())?;
    let central = save.write()?;
    if SaveGame::parse(&central, count, &prototypes)?.write()? != central {
        return Err("central SAVE.DAT grammar round trip changed bytes".into());
    }
    println!(
        "SAVE.DAT: {} bytes, {} manifest maps, automap {} bytes",
        central.len(),
        save.maps.len(),
        save.automap_length
    );
    let root = tempfile::tempdir()?;
    host.save_game(root.path(), 1, &SaveInfo::default())?;
    let (gvars, ticks) = (host.world.gvars.clone(), host.world.clock.ticks);
    let working: Vec<_> = host.world.persistence.maps.keys().cloned().collect();
    host.load_game(root.path(), 1)?;
    if host.world.gvars != gvars
        || host.world.clock.ticks != ticks
        || host
            .world
            .persistence
            .maps
            .keys()
            .cloned()
            .collect::<Vec<_>>()
            != working
    {
        return Err("save slot round trip did not restore campaign/time/map state".into());
    }
    println!(
        "Save slot round trip: restored {}",
        host.world.current_map_name().unwrap_or_default()
    );
    Ok([entered, saved, failed])
}

/// SPEC-009 corpus checks. Returns packets, parser diagnostics, critter
/// prototypes resolved and falling back to entry 0, and nonempty taunt
/// ranges with and without catalog lines.
fn audit_ai(dir: &Path) -> Result<[u64; 6], Box<dyn Error>> {
    let vfs = Arc::new(LocalDirProvider::mount(dir)?);
    let assets = VfsWorldAssets::new(vfs.clone(), "english")?;
    let mut host = WorldHost::new(WorldState::new(CharacterSheet::default(), 1)?, assets);
    host.load_combat_ai()?;
    let (Some(packets), Some(messages)) = (
        host.world.ai.packets.clone(),
        host.world.ai.messages.clone(),
    ) else {
        return Err("combat AI did not initialize".into());
    };
    let mut counts = [0_u64; 6];
    counts[0] = packets.len() as u64;
    counts[1] = packets.diagnostics.len() as u64;
    for path in vfs.files() {
        if path.extension() != Some("pro") || !path.as_str().starts_with("proto/critters/") {
            continue;
        }
        let prototype = Prototype::parse(&vfs.read_bounded(path.as_str(), MAX_ASSET_BYTES)?)?;
        if let PrototypeData::Critter(critter) = &prototype.data {
            counts[if packets.resolve(critter.ai).1 { 2 } else { 3 }] += 1;
        }
    }
    let taunts = [Taunt::Run, Taunt::Move, Taunt::Attack, Taunt::Miss]
        .into_iter()
        .chain((0..8).map(Taunt::Hit));
    for taunt in taunts.collect::<Vec<_>>() {
        for packet in &packets.packets {
            let (start, end) = taunt_range(packet, taunt);
            if end < start {
                continue;
            }
            let lines = (start..=end).any(|id| messages.get(id).is_some());
            counts[if lines { 4 } else { 5 }] += 1;
        }
    }
    Ok(counts)
}

fn audit(args: Args) -> Result<(), Box<dyn Error>> {
    if args.validate_ai {
        let [packets, diagnostics, resolved, fallback, lines, empty] = audit_ai(&args.corpus_dir)?;
        println!("AI packets: {packets} parsed, {diagnostics} token diagnostics");
        println!("Critter AI packets: {resolved} resolved, {fallback} fall back to entry 0");
        println!("Taunt ranges: {lines} with catalog lines, {empty} without");
    }
    if args.validate_saves {
        let [entered, saved, failed] = audit_saves(&args.corpus_dir)?;
        println!("Saves: {entered} maps entered, {saved} working saves, {failed} failed");
        if failed != 0 {
            return Err(format!("{failed} save check(s) failed").into());
        }
    }
    let vfs = LocalDirProvider::mount(args.corpus_dir)?;
    let prototypes = LocalPrototypeCatalog::new(&vfs);
    let mut counts = [0_u64; EXTENSIONS.len()];
    let mut total = 0_u64;
    let mut passed = [0_u64; VALIDATED.len()];
    let mut failed = [0_u64; VALIDATED.len()];
    for path in vfs.files() {
        total += 1;
        if let Some(index) = EXTENSIONS
            .iter()
            .position(|ext| Some(*ext) == path.extension())
        {
            counts[index] += 1;
        }
        if args.validate_formats
            && let Some(index) = VALIDATED
                .iter()
                .position(|extension| Some(*extension) == path.extension())
        {
            let result = vfs
                .read_bounded(path.as_str(), MAX_ASSET_BYTES)
                .map_err(|error| error.to_string())
                .and_then(|bytes| match path.extension() {
                    Some("map") => MapFile::parse(&bytes, &prototypes)
                        .map(|_| ())
                        .map_err(|error| error.to_string()),
                    Some("int") => IntProgram::parse(&bytes)
                        .map(|_| ())
                        .map_err(|error| error.to_string()),
                    _ => validate_asset(path, &bytes)
                        .expect("supported extension")
                        .map_err(|error| error.to_string()),
                });
            match result {
                Ok(()) => passed[index] += 1,
                Err(error) => {
                    failed[index] += 1;
                    eprintln!("FAIL {path}: {error}");
                }
            }
        }
    }
    println!("Mounted files: {total}");
    println!("extension\tfiles");
    for (extension, count) in EXTENSIONS.iter().zip(counts) {
        println!(".{extension}\t{count}");
    }
    if args.validate_formats {
        println!("format\tpassed\tfailed");
        for (index, extension) in VALIDATED.iter().enumerate() {
            println!(".{extension}\t{}\t{}", passed[index], failed[index]);
        }
        let failures: u64 = failed.iter().sum();
        println!(
            "Validation: {} passed, {failures} failed",
            passed.iter().sum::<u64>()
        );
        if failures != 0 {
            return Err(format!("{failures} asset(s) failed validation").into());
        }
    }
    if args.validate_writers {
        let mut counts = [[0_u64; 2]; 2];
        for path in vfs.files() {
            let source = match path.extension() {
                Some("pro") => 0,
                Some("map") => 1,
                _ => continue,
            };
            let result = vfs
                .read_bounded(path.as_str(), MAX_ASSET_BYTES)
                .map_err(|error| error.to_string())
                .and_then(|bytes| check_writer(path, &bytes, &prototypes));
            match result {
                Ok(()) => counts[source][0] += 1,
                Err(error) => {
                    counts[source][1] += 1;
                    eprintln!("FAIL {path}: {error}");
                }
            }
        }
        println!("writer\tpassed\tfailed");
        for (source, [passed, failed]) in ["pro", "map"].iter().zip(counts) {
            println!(".{source}\t{passed}\t{failed}");
        }
        let failures = counts[0][1] + counts[1][1];
        println!(
            "Writers: {} passed, {failures} failed",
            counts[0][0] + counts[1][0]
        );
        if failures != 0 {
            return Err(format!("{failures} writer round trip(s) failed").into());
        }
    }
    if args.validate_art_ids {
        let counts = audit_art_ids(&vfs)?;
        println!("art-id source\tresolved\tmissing\tinvalid");
        for (source, [resolved, missing, invalid]) in ["pro", "map"].iter().zip(counts) {
            println!(".{source}\t{resolved}\t{missing}\t{invalid}");
        }
        let [resolved, missing, invalid] =
            std::array::from_fn(|column| counts[0][column] + counts[1][column]);
        println!("Art identifiers: {resolved} resolved, {missing} missing, {invalid} invalid");
        if invalid != 0 {
            return Err(format!("{invalid} art identifier(s) cannot be decoded").into());
        }
    }
    Ok(())
}

fn main() -> ExitCode {
    match audit(Args::parse()) {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("asset-audit: {error}");
            ExitCode::FAILURE
        }
    }
}
