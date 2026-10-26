mod archives;
mod items;
mod movies;
mod readiness;
mod sound;
mod stack;
mod ui;

use std::{collections::BTreeSet, error::Error, path::PathBuf, process::ExitCode, sync::Arc};

use clap::Parser;
use vault_core::{
    CharacterSheet, WorldHost, WorldState,
    ai::{Taunt, taunt_range},
    assets::{VfsWorldAssets, WorldAssets},
    host_save::SaveInfo,
    worldmap,
};
use vault_formats::{
    ArtCatalog, Fid, LocalDirProvider, LocalPrototypeCatalog, MAX_ASSET_BYTES, MapFile, MsgCatalog,
    Prototype, Vfs, VfsPath,
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
    /// Resolve every PRO/MAP art identifier through the SPEC-007 filename
    /// codec. Identifiers whose file is absent are reported as `MISSING`
    /// (not fatal); identifiers the codec rejects and PRO/MAP files that
    /// cannot be read or parsed fail.
    #[arg(long)]
    validate_art_ids: bool,
    /// Re-serialize every PRO and MAP through the SPEC-003 writers.
    #[arg(long)]
    validate_writers: bool,
    /// SPEC-008: check fixed campaign indices against the initializer, enter
    /// every map so it joins the working saved-map set (no unnumbered map
    /// may be written, §3.10), round-trip the central save grammar, and
    /// write/read a temporary save slot.
    #[arg(long)]
    validate_saves: bool,
    /// SPEC-008 §3.10: the 66 numbered maps and the six unlisted ones are
    /// present and nothing else is, every map's header word 0x34 equals the
    /// number its header name takes, `map.msg` 0-65 equals the fixed table,
    /// and the §4.5 vectors hold against the corpus `map.msg`.
    #[arg(long)]
    validate_map_numbers: bool,
    /// SPEC-014 §3.36 / SPEC-003 §2.8: every map's roof words: the roofed
    /// squares add up to the corpus total, none has roof index 0 or the
    /// barrier bit, only `LAGUNRUN.MAP` stores hide bits (257), and no
    /// hide bit survives parsing.
    #[arg(long)]
    validate_roofs: bool,
    /// SPEC-009: load `combatai.msg` and `data/ai.txt` (every packet needs
    /// all 25 keys), resolve each critter prototype's default AI packet, and
    /// count taunt ranges without catalog lines. Only counts are printed.
    #[arg(long)]
    validate_ai: bool,
    /// SPEC-010: decode every ACM and require each `acm-corpus.tsv` row
    /// (source and PCM SHA-256, header values, counters, edge samples) and
    /// every `acm-wrap.tsv` sample to match.
    #[arg(long)]
    validate_acm: bool,
    /// SPEC-010: rebuild and resolve every `sfx-resolution.tsv` name and
    /// check the `sfx-proto-bytes.tsv` prototype sound bytes.
    #[arg(long)]
    validate_sfx: bool,
    /// SPEC-010 §2.6: parse every `.LIP` (version 2, exact size, P = M - 1,
    /// no loading-check findings, increasing markers from 0) and compare
    /// its stored stem, length word and final marker with the companion
    /// ACM header.
    #[arg(long)]
    validate_lip: bool,
    /// Directory holding the SPEC-010 oracle tables.
    #[arg(long, value_name = "PATH", default_value = "oracle/SPEC-010")]
    oracle_dir: PathBuf,
    /// SPEC-011 §4.12: check the item-prototype invariants (drug selectors
    /// and stages, addiction table, armor perks and DR/DT ranges, weapon
    /// calibers and strength, pack sizes, money) and the `perk.msg` ranges,
    /// then consume every drug through the engine as the player.
    #[arg(long)]
    validate_items: bool,
    /// SPEC-012: decode every `.MVE` (summary, every frame's palette and
    /// pixel hashes with fades applied, every audio opcode) and parse every
    /// fade file and `.SVE`, comparing each with the oracle tables.
    #[arg(long)]
    validate_movies: bool,
    /// Directory holding the SPEC-012 oracle tables.
    #[arg(long, value_name = "PATH", default_value = "oracle/SPEC-012")]
    movie_oracle_dir: PathBuf,
    /// SPEC-013: the skill, Skilldex, lock, explosive, kit, poison,
    /// radiation, loot and barter message texts, the Skilldex, Set Timer,
    /// counter, loot, barter and elevator art sizes, and every elevator
    /// destination map.
    #[arg(long)]
    validate_interactions: bool,
    /// SPEC-014: interface art sizes and frame counts, `intrface.lst`
    /// positions, cursor sizes and hotspots, color keys, fonts, menu, save,
    /// options and editor messages, splash images, credits, every endgame
    /// slide's art, palette, narration and subtitles, the premades, the
    /// location table and the new-game map.
    #[arg(long)]
    validate_ui: bool,
    /// SPEC-008: stream every member of the master and critter archives
    /// (directory tables plus one member in memory at a time), decode it,
    /// and require its SHA-256 to match the loose corpus file at its path.
    #[arg(long)]
    validate_dat: bool,
    /// Master archive for --validate-dat (default: MASTER.DAT beside the
    /// corpus directory).
    #[arg(long, value_name = "PATH", requires = "validate_dat")]
    master_dat: Option<PathBuf>,
    /// Critter archive for --validate-dat (default: CRITTER.DAT beside the
    /// corpus directory).
    #[arg(long, value_name = "PATH", requires = "validate_dat")]
    critter_dat: Option<PathBuf>,
    /// SPEC-004/SPEC-005: decode every `.INT` to the end of its code and
    /// require every instruction word to be one the dispatcher executes
    /// (unknown words are listed); resolve every map's script index, script
    /// record file index and object script id through `scripts.lst` to a
    /// loadable `.INT`; and require the procedure index 1 the request path
    /// falls back to in every referenced script.
    #[arg(long)]
    validate_scripts: bool,
    /// SPEC-013 §3.21-§3.23: every exit grid and elevator destination names
    /// a map in `map.msg`, a hex 0-39999 on elevation 0-2 with a tile block,
    /// and no wall or scenery on the hex. Ladder and stairs words and
    /// `load_map` names `map.msg` lacks are reported as warnings.
    #[arg(long)]
    validate_exits: bool,
    /// SPEC-006 §3.5 / SPEC-013 §3.22 / SPEC-014 §3.39: from a fresh start
    /// on `V13ENT.MAP`, leave map after map through exit grids, the world
    /// map's town entry, ladders and scripted `load_map`, ticking the
    /// world; every entered map must have no script faulted on a missing
    /// symbol, a passable landing with room to move, and its scripted start
    /// (Vault 15's ladder lands near 20719, Shady Sands near 12107).
    #[arg(long)]
    validate_transitions: bool,
    /// SPEC-017 §8: run every `.INT` that `scripts/scripts.lst` names
    /// through the abstract value-stack model with the Fallout 1 host-word
    /// table (§8.4's model check), and report the findings by kind, the
    /// recorded exceptions, signature inference and operand-type evidence,
    /// ending with the model check's pass/fail line. Counts and signatures
    /// only; no script byte is printed. A report, not an audit category
    /// (Decision 35): it never fails the run.
    #[arg(long)]
    report_script_stack: bool,
    /// SPEC-017 §8.1: a Fallout 2 install folder (its `MASTER.DAT`,
    /// `CRITTER.DAT` and `patch000.dat`, mounted as SPEC-018 §1 says) whose
    /// scripts the stack model checks with the Fallout 2 table, adding the
    /// per-word summary of SPEC-017's words and stubs that §8.5 records.
    /// With `--report-script-stack` or `--report-fallout2-art`.
    #[arg(long, value_name = "PATH")]
    fallout2_dir: Option<PathBuf>,
    /// SPEC-019 §2.2 / D-2: which of the Fallout 2 interface art names the
    /// `--fallout2-dir` install's `intrface.lst` lists, role by role, so a
    /// spec revision can correct the candidates. A report: it never fails
    /// the run.
    #[arg(long, requires = "fallout2_dir")]
    report_fallout2_art: bool,
}

/// SPEC-019 §2.2 the interface-art report (`--report-fallout2-art`): per
/// role, the first candidate `art/intrface/intrface.lst` lists, or
/// `missing` (the engine then draws the role's stand-in).
fn report_fallout2_art(folder: &std::path::Path) -> Result<(), Box<dyn Error>> {
    let install = vault_formats::fallout2::Fallout2Install::discover(folder)?;
    let provider = install.open()?.provider(None);
    let catalog = vault_formats::ArtCatalog::load(&provider)?;
    let roles = vault_core::ui::fallout2::NAMED_ART;
    let mut present = 0;
    for role in roles {
        let found = role
            .candidates
            .iter()
            .find(|name| catalog.last_row(6, name.as_bytes()).is_some());
        if found.is_some() {
            present += 1;
        }
        println!(
            "Fallout 2 interface art {}: {}",
            role.candidates.join(" / "),
            found.copied().unwrap_or("missing")
        );
    }
    println!(
        "Fallout 2 interface art: {present} of {} roles listed",
        roles.len()
    );
    Ok(())
}

/// SPEC-017 §8 the script stack report (`--report-script-stack`): the
/// Fallout 1 corpus with the Fallout 1 table, then a Fallout 2 install with
/// the Fallout 2 table when one is given. The final line is §8.4's model
/// check: the Fallout 1 run passes with zero findings.
fn report_script_stack(vfs: &dyn Vfs, fallout2: Option<&PathBuf>) -> Result<(), Box<dyn Error>> {
    let (audit, table) = stack::audit(vfs, vault_vm::WordSet::FALLOUT1)?;
    audit.report("fallout1", &table, &[]);
    let passed = audit.total() == 0;
    if let Some(folder) = fallout2 {
        report_fallout2_script_stack(folder)?;
    }
    println!(
        "Script stack model check: {}",
        if passed { "passed" } else { "failed" }
    );
    Ok(())
}

/// SPEC-017 §8.1 a Fallout 2 install's scripts with the Fallout 2 table,
/// with the per-word summary §8.5 records.
fn report_fallout2_script_stack(folder: &std::path::Path) -> Result<(), Box<dyn Error>> {
    let install = vault_formats::fallout2::Fallout2Install::discover(folder)?;
    let provider = install.open()?.provider(None);
    let (audit, table) = stack::audit(&provider, vault_vm::WordSet::FALLOUT2)?;
    audit.report("fallout2", &table, &stack::SPEC017_WORDS);
    for word in stack::STUB_WORDS {
        println!(
            "Script stack stub {word:#06x}: {} occurrences",
            audit.occurrences.get(&word).copied().unwrap_or(0)
        );
    }
    let reserved: u64 = audit
        .occurrences
        .range(vault_vm::extensions::FALLOUT2_RESERVED)
        .map(|(_, count)| count)
        .sum();
    println!("Script stack reserved-range occurrences: {reserved}");
    Ok(())
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
        // The safe-rule writer reproduces exactly the bytes the reader consumed.
        if prototype.write() != bytes[..prototype.consumed_len] {
            return Err("PRO writer output differs from the consumed bytes".into());
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
/// decoded file is absent from this corpus, that the codec rejects, and
/// source files that cannot be read or parsed (each reported, and the
/// audit continues with the next file).
fn audit_art_ids(vfs: &LocalDirProvider) -> Result<[[u64; 4]; 2], Box<dyn Error>> {
    let catalog = ArtCatalog::load(vfs)?;
    let prototypes = LocalPrototypeCatalog::new(vfs);
    let files: BTreeSet<&str> = vfs.files().map(VfsPath::as_str).collect();
    let mut counts = [[0; 4]; 2];
    for path in vfs.files() {
        let source = match path.extension() {
            Some("pro") => 0,
            Some("map") => 1,
            _ => continue,
        };
        let art = vfs
            .read_bounded(path.as_str(), MAX_ASSET_BYTES)
            .map_err(|error| error.to_string())
            .and_then(|bytes| {
                if source == 0 {
                    Prototype::parse(&bytes)
                        .map(|prototype| prototype_art(&prototype))
                        .map_err(|error| error.to_string())
                } else {
                    let map = MapFile::parse(&bytes, &prototypes).map_err(|e| e.to_string())?;
                    let mut art = Vec::new();
                    for object in map.objects.elevations.iter().flatten() {
                        map_art(object, &mut art);
                    }
                    Ok(art)
                }
            });
        let art = match art {
            Ok(art) => art,
            Err(error) => {
                counts[source][3] += 1;
                eprintln!("FAIL {path}: {error}");
                continue;
            }
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
fn audit_saves(vfs: &Arc<LocalDirProvider>) -> Result<[u64; 3], Box<dyn Error>> {
    let prototypes = LocalPrototypeCatalog::new(&**vfs);
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
    let mut last_numbered = None;
    for path in &maps {
        match host.load_map(path) {
            Ok(()) => {
                entered += 1;
                if host.world.map.header.map_number >= 0 {
                    last_numbered = Some(path.clone());
                }
            }
            Err(error) => {
                failed += 1;
                eprintln!("FAIL {path}: {error}");
            }
        }
    }
    // A save needs a numbered current map (SPEC-008 §3.10): leave the last
    // map for the last numbered one when it has no number.
    if host.world.map.header.map_number < 0 {
        let path = last_numbered.ok_or("no map in the corpus has a map number")?;
        host.load_map(&path)?;
    }
    if !host.world.persist_current_map(true) {
        return Err("the current map could not be written to the working map set".into());
    }
    // SPEC-008 §3.10 / SPEC-014 §3.35 step 5: every working `.SAV` is a
    // numbered map's, under the name `map.msg` gives its number, and never
    // one of the unlisted maps.
    let catalog = host.assets.game_messages("map.msg").ok();
    for (name, bytes) in &host.world.persistence.maps {
        let map = match MapFile::parse(bytes, &prototypes) {
            Ok(map) => map,
            Err(error) => {
                failed += 1;
                eprintln!("FAIL {name}: {error}");
                continue;
            }
        };
        let number = map.header.map_number;
        let expected = vault_core::persistence::working_map_name(number, catalog.as_deref());
        let header_name = String::from_utf8_lossy(&map.header.name)
            .trim_end_matches('\0')
            .to_string();
        let problem = if map.header.flags.0 & 1 == 0 {
            Some("saved-state bit clear".to_string())
        } else if number < 0 {
            Some("an unnumbered map was written to the working set".to_string())
        } else if expected.as_deref() != Some(name.as_str()) {
            Some(format!("map {number} should be written as {expected:?}"))
        } else if header_name != *name {
            Some(format!("header name {header_name:?} is not {name:?}"))
        } else if UNLISTED_MAPS.contains(&name.replace(".SAV", ".MAP").as_str()) {
            Some("an unlisted map was written to the working set".to_string())
        } else {
            None
        };
        match problem {
            None => saved += 1,
            Some(problem) => {
                failed += 1;
                eprintln!("FAIL {name}: {problem}");
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

/// SPEC-008 §3.10: the six corpus maps `map.msg` does not list; they store
/// -1 and load with map number -1.
const UNLISTED_MAPS: [&str; 6] = [
    "CARAVAN.MAP",
    "JUNKDEMO.MAP",
    "TEMPLAT1.MAP",
    "ZDESERT1.MAP",
    "ZDESERT2.MAP",
    "ZDESERT3.MAP",
];
/// SPEC-008 §4.5: header name, word 0x34, assigned number, town, music
/// after load, and whether leaving writes a working `.SAV`.
struct MapNumberVector {
    name: &'static str,
    word: i32,
    number: i32,
    town: i32,
    music: Option<&'static str>,
    saves: bool,
}
const MAP_NUMBER_VECTORS: [MapNumberVector; 6] = [
    MapNumberVector {
        name: "hubent.map",
        word: 7,
        number: 36,
        town: 6,
        music: Some("01hub"),
        saves: true,
    },
    MapNumberVector {
        name: "HUBENT.SAV",
        word: -1,
        number: 36,
        town: 6,
        music: Some("01hub"),
        saves: true,
    },
    MapNumberVector {
        name: "hubent.sav",
        word: 7,
        number: 7,
        town: 1,
        music: Some("13carvrn"),
        saves: true,
    },
    MapNumberVector {
        name: "MSTRLR34.MAP",
        word: 34,
        number: 34,
        town: 11,
        music: Some("02mstrlr"),
        saves: true,
    },
    MapNumberVector {
        name: "DESCRVN4.MAP",
        word: 63,
        number: 63,
        town: 0,
        music: Some("07desert"),
        saves: false,
    },
    MapNumberVector {
        name: "CARAVAN.MAP",
        word: -1,
        number: -1,
        town: -1,
        music: None,
        saves: false,
    },
];

/// SPEC-008 §3.10 corpus checks: `map.msg` 0-65 equals the fixed table;
/// the 66 numbered maps and the six unlisted ones are present and no other
/// `.map` is; every map's header word 0x34 equals the number its header
/// name takes (its own number, or -1 for the unlisted six); and the §4.5
/// vectors hold against the corpus `map.msg`.
fn audit_map_numbers(vfs: &LocalDirProvider) -> Result<ui::UiAudit, Box<dyn Error>> {
    let mut audit = ui::UiAudit::default();
    let catalog =
        MsgCatalog::parse(&vfs.read_bounded("text/english/game/map.msg", MAX_ASSET_BYTES)?)?;
    for (number, entry) in worldmap::MAP_TABLE.iter().enumerate() {
        let result = match catalog.get(number as i32) {
            None => Err("missing".into()),
            Some(message) if message.text != entry.name.as_bytes() => Err(format!(
                "{:?} != {:?}",
                String::from_utf8_lossy(&message.text),
                entry.name
            )),
            Some(_) => Ok(()),
        };
        audit.record("map.msg", &format!("message {number}"), result);
    }
    let present: BTreeSet<String> = vfs
        .files()
        .filter(|path| path.extension() == Some("map") && path.as_str().starts_with("maps/"))
        .map(|path| path.as_str()["maps/".len()..].to_ascii_uppercase())
        .collect();
    let expected: Vec<(String, i32)> = worldmap::MAP_TABLE
        .iter()
        .enumerate()
        .map(|(number, entry)| (entry.name.to_string(), number as i32))
        .chain(UNLISTED_MAPS.iter().map(|name| (name.to_string(), -1)))
        .collect();
    for (name, number) in &expected {
        let result = if present.contains(name) {
            Ok(())
        } else {
            Err("missing".into())
        };
        audit.record("maps present", name, result);
        if !present.contains(name) {
            continue;
        }
        let bytes = match vfs.read_bounded(
            &format!("maps/{}", name.to_ascii_lowercase()),
            MAX_ASSET_BYTES,
        ) {
            Ok(bytes) => bytes,
            Err(error) => {
                audit.record("header word 0x34", name, Err(error.to_string()));
                continue;
            }
        };
        let result = match (bytes.get(0x04..0x14), bytes.get(0x34..0x38)) {
            (Some(field), Some(word)) => {
                let word = i32::from_be_bytes(word.try_into().expect("4 bytes"));
                let assigned = worldmap::header_map_number(field, word, Some(&catalog));
                if word != *number {
                    Err(format!("header word {word} != {number}"))
                } else if assigned != word {
                    Err(format!(
                        "header name {:?} takes number {assigned}, word {word}",
                        String::from_utf8_lossy(field).trim_end_matches('\0')
                    ))
                } else {
                    Ok(())
                }
            }
            _ => Err("header shorter than 0x38 bytes".into()),
        };
        audit.record("header word 0x34", name, result);
    }
    for name in &present {
        if !expected.iter().any(|(known, _)| known == name) {
            audit.record("maps present", name, Err("not a corpus map".into()));
        }
    }
    audit.record(
        "maps present",
        "count",
        if present.len() == expected.len() {
            Ok(())
        } else {
            Err(format!(
                "{} maps, expected {}",
                present.len(),
                expected.len()
            ))
        },
    );
    for vector in MAP_NUMBER_VECTORS {
        let mut field = [0_u8; 16];
        field[..vector.name.len()].copy_from_slice(vector.name.as_bytes());
        let assigned = worldmap::header_map_number(&field, vector.word, Some(&catalog));
        let written = usize::try_from(assigned)
            .is_ok_and(|number| !vault_formats::automap::EXCLUDED.contains(&number));
        let actual = (
            assigned,
            worldmap::map_town(assigned),
            worldmap::city_music(assigned),
            written,
        );
        let expected = (vector.number, vector.town, vector.music, vector.saves);
        let result = if actual == expected {
            Ok(())
        } else {
            Err(format!("{actual:?} != {expected:?}"))
        };
        audit.record("SPEC-008 §4.5 vectors", vector.name, result);
    }
    Ok(audit)
}

/// SPEC-014 §3.36 corpus facts: the roofed squares of all 72 maps, and the
/// maps that store hide bits, which loading clears (SPEC-003 §2.8).
const ROOFED_SQUARES: u64 = 15_056;
const STORED_HIDE_BITS: [(&str, u64); 1] = [("LAGUNRUN.MAP", 257)];

/// SPEC-014 §3.36 / SPEC-003 §2.8 corpus checks on every map's roof words:
/// no roofed square (roof index other than 1) has index 0 or the barrier
/// bit `0x20000000`; only the maps listed store hide bits `0x10000000`, and
/// exactly as many; parsing leaves no hide bit set; and the roofed squares
/// add up to the corpus total.
fn audit_roofs(vfs: &LocalDirProvider) -> Result<ui::UiAudit, Box<dyn Error>> {
    let prototypes = LocalPrototypeCatalog::new(vfs);
    let mut audit = ui::UiAudit::default();
    let mut maps: Vec<String> = vfs
        .files()
        .filter(|path| path.extension() == Some("map") && path.as_str().starts_with("maps/"))
        .map(|path| path.as_str().to_string())
        .collect();
    maps.sort();
    let [mut roofed, mut stored_total] = [0_u64; 2];
    for path in &maps {
        let name = path["maps/".len()..].to_ascii_uppercase();
        let map = match vfs
            .read_bounded(path, MAX_ASSET_BYTES)
            .map_err(|error| error.to_string())
            .and_then(|bytes| MapFile::parse(&bytes, &prototypes).map_err(|e| e.to_string()))
        {
            Ok(map) => map,
            Err(error) => {
                audit.record("roof words", &name, Err(error));
                continue;
            }
        };
        let [mut squares, mut zero, mut barrier, mut stored, mut hidden] = [0_u64; 5];
        for tile in map.tiles.iter().flatten().flatten() {
            if tile.roof() != 1 {
                squares += 1;
            }
            if tile.roof() == 0 {
                zero += 1;
            }
            if tile.roof_barrier() {
                barrier += 1;
            }
            if tile.stored & 0x1000_0000 != 0 {
                stored += 1;
            }
            if tile.roof_hidden() {
                hidden += 1;
            }
        }
        roofed += squares;
        stored_total += stored;
        audit.record(
            "roof index 0",
            &name,
            if zero == 0 {
                Ok(())
            } else {
                Err(format!("{zero} roofed square(s) with index 0"))
            },
        );
        audit.record(
            "barrier bit",
            &name,
            if barrier == 0 {
                Ok(())
            } else {
                Err(format!("{barrier} square(s) with the barrier bit"))
            },
        );
        let expected = STORED_HIDE_BITS
            .iter()
            .find(|(map, _)| *map == name)
            .map_or(0, |(_, count)| *count);
        audit.record(
            "stored hide bits",
            &name,
            if stored == expected {
                Ok(())
            } else {
                Err(format!("{stored} stored hide bit(s), expected {expected}"))
            },
        );
        audit.record(
            "hidden after parse",
            &name,
            if hidden == 0 {
                Ok(())
            } else {
                Err(format!("{hidden} hide bit(s) survived parsing"))
            },
        );
    }
    audit.record(
        "roofed squares",
        "total",
        if roofed == ROOFED_SQUARES {
            Ok(())
        } else {
            Err(format!("{roofed} != {ROOFED_SQUARES}"))
        },
    );
    println!(
        "Roofs: {} maps, {roofed} roofed squares, {stored_total} stored hide bits",
        maps.len()
    );
    Ok(audit)
}

/// SPEC-009 corpus checks. Returns packets, parser diagnostics, critter
/// prototypes resolved and falling back to entry 0, nonempty taunt ranges
/// with and without catalog lines, and critter prototypes that cannot be
/// read or parsed (reported; the scan continues).
fn audit_ai(vfs: &Arc<LocalDirProvider>) -> Result<[u64; 7], Box<dyn Error>> {
    let assets = VfsWorldAssets::new(vfs.clone(), "english")?;
    let mut host = WorldHost::new(WorldState::new(CharacterSheet::default(), 1)?, assets);
    host.load_combat_ai()?;
    let (Some(packets), Some(messages)) = (
        host.world.ai.packets.clone(),
        host.world.ai.messages.clone(),
    ) else {
        return Err("combat AI did not initialize".into());
    };
    let mut counts = [0_u64; 7];
    counts[0] = packets.len() as u64;
    counts[1] = packets.diagnostics.len() as u64;
    for path in vfs.files() {
        if path.extension() != Some("pro") || !path.as_str().starts_with("proto/critters/") {
            continue;
        }
        let parsed = vfs
            .read_bounded(path.as_str(), MAX_ASSET_BYTES)
            .map_err(|error| error.to_string())
            .and_then(|bytes| Prototype::parse(&bytes).map_err(|error| error.to_string()));
        let prototype = match parsed {
            Ok(prototype) => prototype,
            Err(error) => {
                counts[6] += 1;
                eprintln!("FAIL {path}: {error}");
                continue;
            }
        };
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
            // The packet's message bounds are untrusted, so look the
            // range up in the catalog instead of scanning every id in it.
            let lines = messages.entries().range(start..=end).next().is_some();
            counts[if lines { 4 } else { 5 }] += 1;
        }
    }
    Ok(counts)
}

/// Failed categories of one run. Every requested category runs to the end
/// and reports its own tallies; the process exits nonzero only after all of
/// them have run (README "Loose-directory VFS and corpus audit").
#[derive(Default)]
struct Failures {
    ran: usize,
    failed: Vec<String>,
}
impl Failures {
    /// Run one category. The check returns its failed-check count; an error
    /// that stops the category early is recorded as a failure too.
    fn run(
        &mut self,
        category: &str,
        unit: &str,
        check: impl FnOnce() -> Result<u64, Box<dyn Error>>,
    ) {
        self.ran += 1;
        match check() {
            Ok(0) => {}
            Ok(failed) => self.failed.push(format!("{failed} {unit} failed")),
            Err(error) => self.failed.push(format!("{category}: {error}")),
        }
    }
}

fn audit(args: Args) -> Result<(), Box<dyn Error>> {
    // One mount serves every category.
    let vfs = Arc::new(LocalDirProvider::mount(&args.corpus_dir)?);
    let mut failures = Failures::default();
    if args.validate_acm {
        failures.run("ACM", "ACM check(s)", || {
            let [rows, passed, failed, files, wrapped] = sound::audit_acm(&vfs, &args.oracle_dir)?;
            println!("ACM files: {files} in corpus, {rows} oracle rows");
            println!(
                "ACM decoding: {passed} passed, {failed} failed; {wrapped} wrapped samples matched"
            );
            Ok(failed)
        });
    }
    if args.validate_sfx {
        failures.run("SFX", "SFX check(s)", || {
            let [rows, passed, failed, bytes] = sound::audit_sfx(&vfs, &args.oracle_dir)?;
            println!("SFX resolution: {rows} rows, {passed} passed, {failed} failed");
            println!("SFX prototype sound bytes: {bytes} failures");
            Ok(failed + bytes)
        });
    }
    if args.validate_lip {
        failures.run("LIP", "LIP check(s)", || {
            let [files, passed, failed, before_end] = sound::audit_lip(&vfs);
            println!(
                "LIP files: {files}, {passed} passed, {failed} failed; {before_end} end before the audio"
            );
            Ok(failed)
        });
    }
    for (enabled, title, check) in [
        (
            args.validate_interactions,
            "Interactions",
            ui::audit_interactions as fn(&LocalDirProvider) -> _,
        ),
        (args.validate_ui, "UI", ui::audit_ui),
    ] {
        if enabled {
            failures.run(title, &format!("{title} check(s)"), || {
                let audit = check(&vfs)?;
                audit.report(title);
                Ok(audit.failed())
            });
        }
    }
    if args.validate_dat {
        failures.run("DAT", "archive member check(s)", || {
            let beside = |name: &str| args.corpus_dir.join("..").join(name);
            let master = args
                .master_dat
                .clone()
                .unwrap_or_else(|| beside("MASTER.DAT"));
            let critter = args
                .critter_dat
                .clone()
                .unwrap_or_else(|| beside("CRITTER.DAT"));
            let audit = archives::audit_dat(&*vfs, &[&master, &critter]);
            audit.report();
            Ok(audit.failed())
        });
    }
    if args.validate_movies {
        failures.run("Movies", "movie check(s)", || {
            let audit = movies::audit_movies(&vfs, &args.movie_oracle_dir)?;
            audit.report();
            Ok(audit.failed())
        });
    }
    if args.validate_items {
        failures.run("Items", "item invariant(s)", || {
            let audit = items::audit_items(&*vfs)?;
            audit.report();
            Ok(audit.failed() as u64)
        });
        failures.run("Drug consumption", "drug consumption check(s)", || {
            let [consumed, queued, healed, failed] = items::consume_drugs(&vfs)?;
            println!(
                "Drug consumption: {consumed} consumed, {queued} delayed stages queued, \
                 {healed} healed, {failed} failed"
            );
            Ok(failed as u64)
        });
    }
    if args.validate_ai {
        failures.run("AI", "AI critter prototype(s)", || {
            let [
                packets,
                diagnostics,
                resolved,
                fallback,
                lines,
                empty,
                unreadable,
            ] = audit_ai(&vfs)?;
            println!("AI packets: {packets} parsed, {diagnostics} token diagnostics");
            println!(
                "Critter AI packets: {resolved} resolved, {fallback} fall back to entry 0, \
                 {unreadable} unreadable"
            );
            println!("Taunt ranges: {lines} with catalog lines, {empty} without");
            Ok(unreadable)
        });
    }
    if args.validate_map_numbers {
        failures.run("Map numbers", "map number check(s)", || {
            let audit = audit_map_numbers(&vfs)?;
            audit.report("Map numbers");
            Ok(audit.failed())
        });
    }
    if args.validate_roofs {
        failures.run("Roofs", "roof check(s)", || {
            let audit = audit_roofs(&vfs)?;
            audit.report("Roofs");
            Ok(audit.failed())
        });
    }
    for (enabled, title, check) in [
        (
            args.validate_scripts,
            "Scripts",
            readiness::audit_scripts as fn(&Arc<LocalDirProvider>, &mut readiness::Warnings) -> _,
        ),
        (args.validate_exits, "Exits", readiness::audit_exits),
        (
            args.validate_transitions,
            "Transitions",
            readiness::audit_transitions,
        ),
    ] {
        if enabled {
            failures.run(title, &format!("{title} check(s)"), || {
                let mut warnings = readiness::Warnings::default();
                let audit = check(&vfs, &mut warnings)?;
                audit.report(title);
                println!("{title} warnings: {}", warnings.count());
                Ok(audit.failed())
            });
        }
    }
    // A report, outside the categories: it prints and never fails the run.
    if args.report_script_stack {
        report_script_stack(&*vfs, args.fallout2_dir.as_ref())?;
    }
    if args.report_fallout2_art
        && let Some(folder) = &args.fallout2_dir
    {
        report_fallout2_art(folder)?;
    }
    if args.validate_saves {
        failures.run("Saves", "save check(s)", || {
            let [entered, saved, failed] = audit_saves(&vfs)?;
            println!("Saves: {entered} maps entered, {saved} working saves, {failed} failed");
            Ok(failed)
        });
    }
    let prototypes = LocalPrototypeCatalog::new(&*vfs);
    let mut counts = [0_u64; EXTENSIONS.len()];
    let mut total = 0_u64;
    let mut passed = [0_u64; VALIDATED.len()];
    let mut failed = [0_u64; VALIDATED.len()];
    // SPEC-002 §3.5/§4.5: a message file that breaks the grammar stops
    // loading at the bad record and keeps the earlier ones. The corpus ships
    // such files; the engine reproduces that outcome, so they are reported,
    // not failed.
    let mut stopped_early = 0_u64;
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
                    Some("msg") => MsgCatalog::parse(&bytes)
                        .map(|catalog| {
                            if let Some(error) = catalog.failure() {
                                stopped_early += 1;
                                println!(
                                    "MSG STOPS EARLY {path}: {} record(s) kept: {error}",
                                    catalog.entries().len()
                                );
                            }
                        })
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
        failures.run("Formats", "asset(s) in format validation", || {
            println!("format\tpassed\tfailed");
            for (index, extension) in VALIDATED.iter().enumerate() {
                println!(".{extension}\t{}\t{}", passed[index], failed[index]);
            }
            let failures: u64 = failed.iter().sum();
            println!(
                "Validation: {} passed, {failures} failed",
                passed.iter().sum::<u64>()
            );
            println!("Message files stopping early (SPEC-002 §3.5): {stopped_early}");
            Ok(failures)
        });
    }
    if args.validate_writers {
        failures.run("Writers", "writer round trip(s)", || {
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
            Ok(failures)
        });
    }
    if args.validate_art_ids {
        failures.run("Art identifiers", "art identifier check(s)", || {
            let counts = audit_art_ids(&vfs)?;
            println!("art-id source\tresolved\tmissing\tinvalid\tunreadable");
            for (source, [resolved, missing, invalid, unreadable]) in
                ["pro", "map"].iter().zip(counts)
            {
                println!(".{source}\t{resolved}\t{missing}\t{invalid}\t{unreadable}");
            }
            let [resolved, missing, invalid, unreadable] =
                std::array::from_fn(|column| counts[0][column] + counts[1][column]);
            println!("Art identifiers: {resolved} resolved, {missing} missing, {invalid} invalid");
            println!("Art-id source files: {unreadable} unreadable");
            // Missing files are reported, not failed: the reference
            // guarantees no fallback resource.
            Ok(invalid + unreadable)
        });
    }
    if failures.ran != 0 {
        println!(
            "Audit categories: {} run, {} failed",
            failures.ran,
            failures.failed.len()
        );
    }
    if failures.failed.is_empty() {
        return Ok(());
    }
    for failure in &failures.failed {
        eprintln!("asset-audit: {failure}");
    }
    Err(format!(
        "{} of {} categories failed",
        failures.failed.len(),
        failures.ran
    )
    .into())
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
