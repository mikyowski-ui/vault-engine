//! Campaign readiness checks over the corpus: every script's instruction
//! words against the dispatcher and its `scripts.lst` references
//! (`--validate-scripts`), every exit grid, ladder, stairs, elevator and
//! scripted `load_map` destination (`--validate-exits`), and multi-map
//! sequences through the engine's real transition paths
//! (`--validate-transitions`).
use std::{
    collections::{BTreeMap, BTreeSet, VecDeque},
    error::Error,
    sync::Arc,
};

use vault_core::{
    CharacterSheet, ObjectAction, ObjectRef, Transition, WorldEffect, WorldHost, WorldState,
    assets::{VfsWorldAssets, WorldAssets},
    routing, skills, worldmap,
};
use vault_debug::decode::{self, DecodeError, OpcodeClass};
use vault_formats::{
    Fid, HexGrid, LocalDirProvider, LocalPrototypeCatalog, MAX_ASSET_BYTES, MapFile, Vfs,
    map::{MapObject, ObjectDetail, ObjectState},
    pro::{BasicFlags, PackedLocation},
};
use vault_vm::{IntProgram, VmError, VmStatus, extensions};

use crate::{UNLISTED_MAPS, ui::UiAudit};

/// Hexes per map elevation (SPEC-003).
const HEXES: i32 = 40_000;
/// `override_map_start` (SPEC-005).
const OVERRIDE_MAP_START: u16 = 0x80a9;
/// `load_map` (SPEC-005).
const LOAD_MAP: u16 = 0x80e4;
/// Campaign index 32 receives a scripted `load_map`'s parameter (SPEC-005
/// 0x80E4) and a town entrance's selector.
const ENTRY_GLOBAL: usize = 32;

/// Warnings: reported on stderr and counted, never failed.
#[derive(Default)]
pub struct Warnings(u64);
impl Warnings {
    fn warn(&mut self, category: &str, label: &str, message: &str) {
        self.0 += 1;
        eprintln!("WARN {category}: {label}: {message}");
    }
    pub fn count(&self) -> u64 {
        self.0
    }
}

/// Whether the dispatcher executes `word` (SPEC-004 §3.7-§3.8): pushes and
/// the core words `0x8000`-`0x804B` always; a library or game word when
/// the SPEC-005 catalog registers it. The VM rejects `0x8091` like an
/// unregistered slot.
pub fn implemented(word: u16) -> bool {
    match decode::classify(word) {
        Some(OpcodeClass::Push | OpcodeClass::Core) => true,
        Some(OpcodeClass::Library | OpcodeClass::Game) => {
            word != 0x8091 && extensions::opcode(word).is_some_and(|spec| spec.registered)
        }
        _ => false,
    }
}

/// Every instruction of a program's code, in order, from the start of its
/// code to the end of the image.
fn instructions(program: &IntProgram) -> Result<Vec<decode::Instruction>, String> {
    let code = program.image();
    let mut at = program.code_start as u32;
    let mut listing = Vec::new();
    loop {
        match decode::decode(code, at) {
            Ok(instruction) => {
                at = instruction.next_address();
                listing.push(instruction);
            }
            Err(DecodeError::End(_)) => return Ok(listing),
            Err(error) => return Err(error.to_string()),
        }
    }
}

fn read_map(vfs: &LocalDirProvider, path: &str) -> Result<MapFile, String> {
    let bytes = vfs
        .read_bounded(path, MAX_ASSET_BYTES)
        .map_err(|error| error.to_string())?;
    MapFile::parse(&bytes, &LocalPrototypeCatalog::new(vfs)).map_err(|error| error.to_string())
}

/// The corpus maps under `maps/`, sorted, with their upper-case names.
fn map_paths(vfs: &LocalDirProvider) -> Vec<(String, String)> {
    let mut maps: Vec<(String, String)> = vfs
        .files()
        .filter(|path| path.extension() == Some("map") && path.as_str().starts_with("maps/"))
        .map(|path| {
            let path = path.as_str().to_string();
            let name = path["maps/".len()..].to_ascii_uppercase();
            (path, name)
        })
        .collect();
    maps.sort();
    maps
}

/// The top-level objects of every elevation (inventories excluded).
fn placed(map: &MapFile) -> impl Iterator<Item = &MapObject> {
    map.objects.elevations.iter().flatten()
}

/// SPEC-004/SPEC-005/SPEC-006 script readiness: every `.INT` decodes to the
/// end of its code with only words the dispatcher executes; every map's
/// script index, every script record's file index and every object's
/// script id resolves (through `scripts.lst` to a loadable `.INT`); and
/// every script a map references has the procedure index 1 the generic
/// request path falls back to (SPEC-004 §3.7). Problems on the six
/// unlisted maps (not campaign maps, SPEC-008 §3.10) are warnings.
pub fn audit_scripts(
    vfs: &Arc<LocalDirProvider>,
    warnings: &mut Warnings,
) -> Result<UiAudit, Box<dyn Error>> {
    let mut audit = UiAudit::default();
    let assets = VfsWorldAssets::new(vfs.clone(), "english")?;
    let mut unknown: BTreeMap<u16, BTreeSet<String>> = BTreeMap::new();
    let mut words = 0_u64;
    let mut scripts: Vec<String> = vfs
        .files()
        .filter(|path| path.extension() == Some("int"))
        .map(|path| path.as_str().to_string())
        .collect();
    scripts.sort();
    for path in &scripts {
        let program = match vfs
            .read_bounded(path, MAX_ASSET_BYTES)
            .map_err(|error| error.to_string())
            .and_then(|bytes| IntProgram::parse(&bytes).map_err(|error| error.to_string()))
        {
            Ok(program) => program,
            Err(error) => {
                audit.record("instruction words", path, Err(error));
                continue;
            }
        };
        let result = instructions(&program).and_then(|listing| {
            words += listing.len() as u64;
            let mut missing: BTreeMap<u16, usize> = BTreeMap::new();
            for instruction in listing {
                if !implemented(instruction.word) {
                    *missing.entry(instruction.word).or_default() += 1;
                    unknown
                        .entry(instruction.word)
                        .or_default()
                        .insert(path.clone());
                }
            }
            if missing.is_empty() {
                Ok(())
            } else {
                Err(missing
                    .iter()
                    .map(|(word, count)| format!("unknown word {word:#06x} x{count}"))
                    .collect::<Vec<_>>()
                    .join(", "))
            }
        });
        audit.record("instruction words", path, result);
    }
    println!(
        "Scripts: {} files, {words} instructions, {} unknown word(s)",
        scripts.len(),
        unknown.len()
    );
    for (word, files) in &unknown {
        println!(
            "Unknown word {word:#06x} ({}) in {} file(s)",
            decode::mnemonic(*word),
            files.len()
        );
    }
    // The references: map script, script records, objects.
    let mut referenced: BTreeSet<i32> = BTreeSet::new();
    for (path, name) in map_paths(vfs) {
        let campaign = !UNLISTED_MAPS.contains(&name.as_str());
        let mut report =
            |audit: &mut UiAudit, category: &'static str, label: &str, result| match result {
                Err(error) if !campaign => {
                    warnings.warn(category, label, &format!("{error} (unlisted map)"))
                }
                result => audit.record(category, label, result),
            };
        let map = match read_map(vfs, &path) {
            Ok(map) => map,
            Err(error) => {
                report(&mut audit, "map script index", &name, Err(error));
                continue;
            }
        };
        let resolve = |index: i32| -> Result<(), String> {
            assets
                .program(index)
                .map(|_| ())
                .map_err(|error| error.to_string())
        };
        if let Some(index) = map.header.map_script_index() {
            let index = index as i32;
            referenced.insert(index);
            report(&mut audit, "map script index", &name, resolve(index));
        }
        let mut ids = BTreeSet::new();
        for category in &map.scripts {
            for record in category.active() {
                ids.insert(record.id);
                referenced.insert(record.file_index);
                report(
                    &mut audit,
                    "script records",
                    &format!(
                        "{name} script {:#010x} file {}",
                        record.id, record.file_index
                    ),
                    resolve(record.file_index),
                );
            }
        }
        for object in placed(&map) {
            if object.script_id == -1 {
                continue;
            }
            let result = if ids.contains(&(object.script_id as u32)) {
                Ok(())
            } else {
                Err("no script record has this id".to_string())
            };
            report(
                &mut audit,
                "object scripts",
                &format!(
                    "{name} object {} pid {:#010x} script {:#010x}",
                    object.id, object.pid, object.script_id
                ),
                result,
            );
        }
    }
    // SPEC-004 §3.7: the generic request path substitutes procedure index
    // 1 for a missing name, so every referenced script needs one.
    for index in referenced {
        let Ok((path, program)) = assets.program(index) else {
            continue;
        };
        let result = if program.procedures().len() > 1 {
            Ok(())
        } else {
            Err("no procedure index 1 for the lifecycle fallback".to_string())
        };
        audit.record("lifecycle fallback", &path, result);
    }
    Ok(audit)
}

/// Wall and scenery blockers of one map elevation: by hex, whether the
/// object is multihex (SPEC-007 movement blockers, items, critters and
/// miscellaneous art excluded).
struct Blockers([BTreeMap<i32, bool>; 3]);
impl Blockers {
    fn of(map: &MapFile) -> Self {
        let mut blockers: [BTreeMap<i32, bool>; 3] = Default::default();
        for object in placed(map) {
            let Some(hex) = object.hex else {
                continue;
            };
            if object.flags.0 & (BasicFlags::HIDDEN.0 | BasicFlags::NONBLOCKING.0) != 0
                || !matches!(Fid(object.art_id).category(), 2..=3)
            {
                continue;
            }
            if let Some(level) = blockers.get_mut(usize::from(object.elevation)) {
                let multihex = object.flags.contains(BasicFlags::MULTIHEX);
                *level.entry(hex).or_default() |= multihex;
            }
        }
        Self(blockers)
    }
    /// The hex holds a blocker, or a multihex blocker stands next to it.
    fn blocked(&self, hex: i32, elevation: usize) -> bool {
        let Some(level) = self.0.get(elevation) else {
            return false;
        };
        level.contains_key(&hex)
            || (0..6).any(|direction| {
                level
                    .get(&routing::step(hex, direction))
                    .is_some_and(|multihex| *multihex)
            })
    }
}

/// The destination maps by number, parsed once.
struct Destinations<'a> {
    vfs: &'a LocalDirProvider,
    assets: &'a VfsWorldAssets,
    maps: BTreeMap<i32, Result<(String, MapFile, Blockers), String>>,
}
impl<'a> Destinations<'a> {
    fn get(&mut self, number: i32) -> &Result<(String, MapFile, Blockers), String> {
        let (vfs, assets) = (self.vfs, self.assets);
        self.maps.entry(number).or_insert_with(|| {
            let path = assets
                .map_path(number)
                .ok_or_else(|| format!("map {number} is not in map.msg"))?;
            let map = read_map(vfs, &path).map_err(|error| format!("{path}: {error}"))?;
            let blockers = Blockers::of(&map);
            Ok((path, map, blockers))
        })
    }
    /// SPEC-013 §3.22 the destination tile: map `number` exists, the hex is
    /// on the map and its elevation 0-2, a tile block exists there, and no
    /// wall or scenery blocks it. Hex -1 or 0 (or an elevation outside
    /// 0-2) keeps the map's default entrance, checked instead.
    fn check(&mut self, number: i32, hex: i32, elevation: i32) -> Result<(), String> {
        let (_, map, blockers) = self.get(number).as_ref().map_err(String::clone)?;
        let (hex, elevation, entrance) = if hex == -1 || hex == 0 || !(0..=2).contains(&elevation) {
            (
                map.header.entry_hex,
                i32::from(map.header.entry_elevation),
                true,
            )
        } else {
            (hex, elevation, false)
        };
        let place = if entrance {
            format!("default entrance {hex}/{elevation}")
        } else {
            format!("hex {hex}/{elevation}")
        };
        if !(0..HEXES).contains(&hex) {
            return Err(format!("{place} is outside 0-39999"));
        }
        if !(0..=2).contains(&elevation) {
            return Err(format!("{place}: elevation outside 0-2"));
        }
        let level = elevation as usize;
        if map.tiles[level].is_none() {
            return Err(format!("{place}: no tile block on that elevation"));
        }
        if blockers.blocked(hex, level) {
            return Err(format!("{place} is blocked by a wall or scenery"));
        }
        Ok(())
    }
}

/// One scripted `load_map` call found in a program: its map operand (a
/// number or a name) and its entry parameter.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub enum MapOperand {
    Number(i32),
    Name(String),
}
impl std::fmt::Display for MapOperand {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Number(number) => write!(f, "{number}"),
            Self::Name(name) => write!(f, "{name:?}"),
        }
    }
}
/// Every `load_map` whose map and parameter are pushed as constants just
/// before it, by script file: (map, parameter). Calls with computed
/// operands are counted separately.
fn load_map_calls(vfs: &LocalDirProvider) -> (BTreeMap<(MapOperand, i32), BTreeSet<String>>, u64) {
    let mut found: BTreeMap<(MapOperand, i32), BTreeSet<String>> = BTreeMap::new();
    let mut computed = 0;
    for path in vfs.files() {
        if path.extension() != Some("int") {
            continue;
        }
        let Some(program) = vfs
            .read_bounded(path.as_str(), MAX_ASSET_BYTES)
            .ok()
            .and_then(|bytes| IntProgram::parse(&bytes).ok())
        else {
            continue;
        };
        let Ok(listing) = instructions(&program) else {
            continue;
        };
        for window in listing.windows(3) {
            let [map, parameter, call] = window else {
                continue;
            };
            if call.word != LOAD_MAP {
                continue;
            }
            // SPEC-004 §2.5: 0xC001 pushes an integer, 0x9001 and 0x9801 a
            // string literal.
            let map = match (map.word, map.immediate) {
                (0xc001, Some(value)) => Some(MapOperand::Number(value as i32)),
                (0x9001 | 0x9801, Some(offset)) => program.literal(offset).map(|text| {
                    MapOperand::Name(
                        String::from_utf8_lossy(text)
                            .trim_end_matches('\0')
                            .to_string(),
                    )
                }),
                _ => None,
            };
            let parameter = match (parameter.word, parameter.immediate) {
                (0xc001, Some(value)) => Some(value as i32),
                _ => None,
            };
            match (map, parameter) {
                (Some(map), Some(parameter)) => {
                    found
                        .entry((map, parameter))
                        .or_default()
                        .insert(path.as_str().to_string());
                }
                _ => computed += 1,
            }
        }
    }
    (found, computed)
}

/// SPEC-003/SPEC-013 §3.22-§3.23 exit readiness: every exit grid's
/// destination (map in `map.msg`, hex 0-39999, elevation 0-2, a tile block
/// on it, no wall or scenery on the hex), every elevator destination
/// (SPEC-013 §3.21), every ladder and stairs instance's stored destination,
/// and every constant scripted `load_map` destination. Ladder and stairs
/// words are never read by the engine (the object's script moves the
/// player, SPEC-013 §3.23), so their problems are warnings, as are
/// `load_map` names `map.msg` does not list (no transition happens,
/// SPEC-008 §3.10).
pub fn audit_exits(
    vfs: &Arc<LocalDirProvider>,
    warnings: &mut Warnings,
) -> Result<UiAudit, Box<dyn Error>> {
    let mut audit = UiAudit::default();
    let assets = VfsWorldAssets::new(vfs.clone(), "english")?;
    let mut destinations = Destinations {
        vfs,
        assets: &assets,
        maps: BTreeMap::new(),
    };
    let [mut grids, mut ladders] = [0_u64; 2];
    for (path, name) in map_paths(vfs) {
        let map = match read_map(vfs, &path) {
            Ok(map) => map,
            Err(error) => {
                audit.record("exit grids", &name, Err(error));
                continue;
            }
        };
        let number = map.header.map_number;
        let mut seen = BTreeSet::new();
        for object in placed(&map) {
            let ObjectState::Other { detail, .. } = &object.state else {
                continue;
            };
            let here = object.hex.unwrap_or(-1);
            match *detail {
                ObjectDetail::ExitGrid {
                    map: to,
                    hex,
                    elevation,
                    ..
                } => {
                    grids += 1;
                    if !seen.insert((to, hex, elevation)) {
                        continue;
                    }
                    let label = format!(
                        "{name} grid {here}/{} -> map {to} hex {hex}/{elevation}",
                        object.elevation
                    );
                    // Map 0 is the world map, -1 the town map (SPEC-013
                    // §3.22): no tile to check.
                    let result = if to <= 0 {
                        Ok(())
                    } else {
                        destinations.check(to, hex, elevation)
                    };
                    audit.record("exit grids", &label, result);
                }
                ObjectDetail::Ladder {
                    destination: PackedLocation(word),
                    ..
                }
                | ObjectDetail::Stairs {
                    destination: PackedLocation(word),
                    ..
                } => {
                    ladders += 1;
                    let to = match *detail {
                        ObjectDetail::Stairs { map, .. }
                        | ObjectDetail::Ladder { map: Some(map), .. }
                            if map > 0 =>
                        {
                            map
                        }
                        _ => number,
                    };
                    let location = PackedLocation(word);
                    let label = format!(
                        "{name} object {} at {here}/{} -> map {to} hex {}/{}",
                        object.id,
                        object.elevation,
                        location.hex(),
                        location.elevation()
                    );
                    if word == 0 || word == u32::MAX {
                        continue;
                    }
                    if let Err(error) = destinations.check(
                        to,
                        location.hex() as i32,
                        i32::from(location.elevation()),
                    ) {
                        warnings.warn("ladders and stairs", &label, &error);
                    }
                }
                _ => {}
            }
        }
    }
    let mut elevators = 0;
    for (kind, elevator) in skills::ELEVATORS.iter().enumerate() {
        for (level, (map, elevation, hex)) in elevator.destinations.iter().enumerate() {
            elevators += 1;
            let label = format!("elevator {kind} level {level} -> map {map} hex {hex}/{elevation}");
            let result = destinations.check(*map, *hex, i32::from(*elevation));
            audit.record("elevator destinations", &label, result);
        }
    }
    let (calls, computed) = load_map_calls(vfs);
    for ((map, parameter), files) in &calls {
        let files: Vec<_> = files.iter().cloned().collect();
        let label = format!("load_map({map}, {parameter}) in {}", files.join(" "));
        let number = match map {
            MapOperand::Number(number) => Some(*number),
            MapOperand::Name(name) => assets.map_index(name.as_bytes()),
        };
        match number {
            // A name `map.msg` does not list loads nothing (SPEC-008 §3.10):
            // a data quirk of the shipped scripts.
            None => warnings.warn(
                "scripted load_map",
                &label,
                "the map is not in map.msg; no transition happens",
            ),
            // The destination's own script places the player (SPEC-006
            // §3.5), so only the map itself is checked here.
            Some(number) => {
                let result = destinations
                    .get(number)
                    .as_ref()
                    .map(|_| ())
                    .map_err(String::clone);
                audit.record("scripted load_map", &label, result);
            }
        }
    }
    println!(
        "Exits: {grids} exit grids, {ladders} ladders and stairs, {elevators} elevator \
         destinations, {} constant load_map calls ({computed} computed)",
        calls.len()
    );
    Ok(audit)
}

/// How a transition step leaves the current map.
#[derive(Debug, Clone, Copy)]
enum Via {
    /// The player is placed on an exit grid of the current map whose
    /// destination is this map (SPEC-013 §3.22).
    Grid(&'static str),
    /// The world map's town entry (SPEC-014 §3.39): a first entry loads the
    /// town's slot-zero map, a later one its first known entrance.
    Town(usize),
    /// The player uses the ladder or stairs whose script has this name
    /// (SPEC-013 §3.23).
    Use(&'static str),
    /// `load_map` as 0x80E4 does it: campaign index 32 = the parameter
    /// found in the corpus scripts, then a transition to the map with hex,
    /// elevation and facing -1.
    LoadMap(&'static str),
}
/// One step and the map it must reach; `hex` is the landing a scripted
/// start must give (within two hexes).
struct Step {
    via: Via,
    map: Option<&'static str>,
    hex: Option<i32>,
}
const fn step(via: Via, map: Option<&'static str>, hex: Option<i32>) -> Step {
    Step { via, map, hex }
}
/// The sequences, each from a fresh new-game start on `V13ENT.MAP`.
const SEQUENCES: [(&str, &[Step]); 3] = [
    (
        "Vault 15 ladder",
        &[
            step(Via::Town(1), Some("VAULTENT.MAP"), None),
            step(Via::Use("bv2vault"), Some("VAULTBUR.MAP"), Some(20719)),
        ],
    ),
    (
        "Shady Sands",
        &[step(Via::Town(2), Some("SHADYW.MAP"), Some(12107))],
    ),
    (
        "Campaign",
        &[
            step(Via::Grid("VAULT13.MAP"), Some("VAULT13.MAP"), None),
            step(Via::Grid("V13ENT.MAP"), Some("V13ENT.MAP"), None),
            step(Via::Town(1), Some("VAULTENT.MAP"), None),
            step(Via::Use("bv2vault"), Some("VAULTBUR.MAP"), Some(20719)),
            step(Via::Town(2), Some("SHADYW.MAP"), Some(12107)),
            step(Via::Town(3), None, None),
            step(Via::LoadMap("JUNKCSNO.MAP"), Some("JUNKCSNO.MAP"), None),
            step(Via::Town(6), None, None),
            step(Via::Town(5), None, None),
            step(Via::Town(7), None, None),
            step(Via::Town(10), None, None),
            step(Via::Town(4), None, None),
            step(Via::Town(9), None, None),
            step(Via::Town(8), None, None),
            step(Via::Town(11), None, None),
        ],
    ),
];
/// The new-game map (SPEC-014 §3.12).
const START_MAP: &str = "maps/v13ent.map";
/// Game time each map runs after its entry, and the most a ladder's walk,
/// animation and script may take.
const SETTLE_MS: u64 = 3000;
const USE_MS: u64 = 8000;
const FRAME_MS: u64 = 50;
/// A landing with fewer reachable hexes than this is a pocket.
const MIN_REACH: usize = 20;
const REACH_CAP: usize = 400;
/// A scripted start may be followed by a small script move.
const LANDING_SLACK: i32 = 2;

/// A headless game: the desktop's world set-up (campaign, language filter,
/// combat AI) over the corpus, ticked in 50 ms frames.
struct Game {
    host: WorldHost<VfsWorldAssets>,
    now: u64,
    diagnostics: Vec<String>,
    /// The landing the last exit grid used chose (SPEC-013 §3.22 step 2).
    grid_landing: Option<(i32, u8)>,
}
impl Game {
    fn start(vfs: &Arc<LocalDirProvider>) -> Result<Self, String> {
        let text = |error: &dyn std::fmt::Display| error.to_string();
        let assets = VfsWorldAssets::new(vfs.clone(), "english").map_err(|e| text(&e))?;
        let state = WorldState::new(CharacterSheet::default(), 1).map_err(|e| text(&e))?;
        let mut host = WorldHost::new(state, assets);
        host.update_player_gender();
        host.load_campaign().map_err(|e| format!("campaign: {e}"))?;
        host.load_language_filter();
        // As on the desktop, a world without combat AI still runs (NPC
        // combat turns are skipped).
        let mut diagnostics = Vec::new();
        if let Err(error) = host.load_combat_ai() {
            diagnostics.push(format!("combat AI unavailable: {error}"));
        }
        host.load_map(START_MAP)
            .map_err(|e| format!("{START_MAP}: {e}"))?;
        let mut game = Self {
            host,
            now: 1000,
            diagnostics,
            grid_landing: None,
        };
        game.drain();
        Ok(game)
    }
    fn drain(&mut self) {
        for effect in self.host.world.effects.drain(..) {
            if let WorldEffect::Diagnostic(message) = effect {
                self.diagnostics.push(message);
            }
        }
    }
    /// Frames for up to `ms` of game time, stopping early when `stop`
    /// holds.
    fn run(&mut self, ms: u64, stop: impl Fn(&Self) -> bool) -> Result<(), String> {
        let end = self.now + ms;
        while self.now < end {
            self.host
                .tick(self.now)
                .map_err(|error| error.to_string())?;
            self.drain();
            self.now += FRAME_MS;
            if stop(self) {
                break;
            }
        }
        Ok(())
    }
    /// The current map as `NAME.MAP` (a revisit loads the working `.SAV`).
    fn map_name(&self) -> String {
        self.host.world.current_map_name().map_or_else(
            || "<none>".into(),
            |name| match name.rsplit_once('.') {
                Some((stem, _)) => format!("{stem}.MAP").to_ascii_uppercase(),
                None => name.to_ascii_uppercase(),
            },
        )
    }
    fn player_at(&self) -> (i32, u8) {
        let world = &self.host.world;
        world.get(world.player).map_or((-1, 0), |object| {
            (object.raw.hex.unwrap_or(-1), object.raw.elevation)
        })
    }
    fn script_name(&self, file: i32) -> String {
        self.host
            .assets
            .program(file)
            .map(|(path, _)| path)
            .unwrap_or_else(|_| format!("script {file}"))
    }
    /// Programs of the current map that ended with the error bit, with
    /// their errors.
    fn faults(&self) -> Vec<(String, VmError)> {
        let world = &self.host.world;
        let mut faults: Vec<(String, VmError)> = world
            .scripts
            .values()
            .filter_map(|instance| {
                let vm = self.host.programs.get(&instance.program)?;
                (vm.status.0 & VmStatus::ERROR != 0).then(|| {
                    (
                        self.script_name(instance.file),
                        vm.last_error
                            .clone()
                            .unwrap_or(VmError::Host("no error recorded".into())),
                    )
                })
            })
            .collect();
        faults.sort_by(|a, b| a.0.cmp(&b.0));
        faults.dedup_by(|a, b| a.0 == b.0);
        faults
    }
    /// The hexes the player reaches by unblocked steps, up to the cap.
    fn reach(&self) -> usize {
        let world = &self.host.world;
        let player = world.player;
        let (hex, elevation) = self.player_at();
        if !(0..HEXES).contains(&hex) {
            return 0;
        }
        let mut seen = BTreeSet::from([hex]);
        let mut queue = VecDeque::from([hex]);
        while let Some(at) = queue.pop_front() {
            if seen.len() >= REACH_CAP {
                break;
            }
            for direction in 0..6 {
                let next = routing::step(at, direction);
                if (0..HEXES).contains(&next)
                    && !seen.contains(&next)
                    && world.movement_blocker(next, elevation, player).is_none()
                {
                    seen.insert(next);
                    queue.push_back(next);
                }
            }
        }
        seen.len()
    }
    /// Whether the current map script's program holds `override_map_start`.
    fn map_script_overrides(&self) -> bool {
        let world = &self.host.world;
        world
            .map_script
            .and_then(|sid| world.scripts.get(&sid))
            .and_then(|instance| self.host.assets.program(instance.file).ok())
            .and_then(|(_, program)| instructions(&program).ok())
            .is_some_and(|listing| listing.iter().any(|i| i.word == OVERRIDE_MAP_START))
    }
    /// The hex of the last `override_map_start` the trace holds (y x 200 +
    /// x, SPEC-005). The trace keeps the operands in pop order: rotation,
    /// elevation, y, x.
    fn override_target(&self) -> Option<i32> {
        self.host
            .calls
            .iter()
            .rev()
            .find(|call| call.opcode == OVERRIDE_MAP_START)
            .and_then(|call| {
                let y = call.operands.get(2)?.payload as i32;
                let x = call.operands.get(3)?.payload as i32;
                Some(y * 200 + x)
            })
    }
    fn map_number(&self, name: &str) -> Result<i32, String> {
        self.host
            .assets
            .map_index(name.as_bytes())
            .ok_or_else(|| format!("{name} is not in map.msg"))
    }
    /// Leave the current map by `via`; returns how it was left.
    fn leave(
        &mut self,
        via: Via,
        load_maps: &BTreeMap<(MapOperand, i32), BTreeSet<String>>,
    ) -> Result<String, String> {
        let player = self.host.world.player;
        self.grid_landing = None;
        match via {
            Via::Grid(name) => {
                let number = self.map_number(name)?;
                let grid = self.host.world.iter().find_map(|(_, object)| {
                    match (&object.raw.state, object.raw.hex, object.owner) {
                        (
                            ObjectState::Other {
                                detail:
                                    ObjectDetail::ExitGrid {
                                        map,
                                        hex: to,
                                        elevation: level,
                                        ..
                                    },
                                ..
                            },
                            Some(hex),
                            None,
                        ) if *map == number => Some((hex, object.raw.elevation, *to, *level)),
                        _ => None,
                    }
                });
                let (hex, elevation, to, level) =
                    grid.ok_or_else(|| format!("no exit grid on this map leads to {name}"))?;
                // SPEC-013 §3.22 step 2: the grid's hex replaces the map's
                // entrance (and any scripted start) when it is neither -1
                // nor 0 and its elevation is 0-2.
                self.grid_landing =
                    (to != -1 && to != 0 && (0..=2).contains(&level)).then_some((to, level as u8));
                self.host
                    .world
                    .move_object(player, hex, elevation, false)
                    .map_err(|error| error.to_string())?;
                self.run(FRAME_MS, |_| false)?;
                Ok(format!("exit grid {hex}/{elevation}"))
            }
            Via::Town(town) => {
                let entry = self
                    .host
                    .enter_town(town)
                    .map_err(|error| error.to_string())?;
                match entry {
                    Some(worldmap::TownEntry::Direct(button)) => {
                        Ok(format!("town {town} first entry {}", button.map))
                    }
                    Some(worldmap::TownEntry::TownMap { .. }) => {
                        let gvars = self.host.world.gvars.clone().unwrap_or_default();
                        let button = self
                            .host
                            .world
                            .world_map
                            .town_buttons(town, &gvars)
                            .first()
                            .copied()
                            .ok_or_else(|| format!("town {town} has no known entrance"))?;
                        self.host
                            .select_town_entrance(town, button)
                            .map_err(|error| error.to_string())?;
                        Ok(format!("town {town} entrance {}", button.map))
                    }
                    None => Err(format!("town {town} cannot be entered")),
                }
            }
            Via::Use(script) => {
                let target = self.host.world.iter().find_map(|(reference, object)| {
                    let file = self
                        .host
                        .world
                        .script_for(reference)
                        .and_then(|sid| self.host.world.scripts.get(&sid))?
                        .file;
                    let name = self.script_name(file);
                    let stem = name.rsplit('/').next()?.split('.').next()?;
                    (object.owner.is_none() && stem.eq_ignore_ascii_case(script)).then_some((
                        reference,
                        object.raw.hex?,
                        object.raw.elevation,
                    ))
                });
                let (target, hex, elevation): (ObjectRef, i32, u8) =
                    target.ok_or_else(|| format!("no object on this map runs {script}"))?;
                // Stand one or two hexes away on its elevation.
                let spot = (0..6)
                    .flat_map(|direction| {
                        let near = routing::step(hex, direction);
                        [near, routing::step(near, direction)]
                    })
                    .find(|at| {
                        self.host
                            .world
                            .movement_blocker(*at, elevation, player)
                            .is_none()
                    })
                    .ok_or("no free hex beside it")?;
                self.host
                    .world
                    .move_object(player, spot, elevation, false)
                    .map_err(|error| error.to_string())?;
                let start = self.map_name();
                self.host
                    .action(player, target, ObjectAction::Use)
                    .map_err(|error| error.to_string())?;
                self.run(USE_MS, |game| game.map_name() != start)?;
                Ok(format!("use {script} at {hex}/{elevation}"))
            }
            Via::LoadMap(name) => {
                let number = self.map_number(name)?;
                let parameter = load_maps
                    .keys()
                    .find(|(map, _)| match map {
                        MapOperand::Name(map) => map.eq_ignore_ascii_case(name),
                        MapOperand::Number(map) => *map == number,
                    })
                    .map_or(0, |(_, parameter)| *parameter);
                if let Some(slot) = self
                    .host
                    .world
                    .gvars
                    .as_mut()
                    .and_then(|gvars| gvars.get_mut(ENTRY_GLOBAL))
                {
                    *slot = parameter;
                }
                self.host.request_transition(Transition {
                    map: number,
                    elevation: -1,
                    hex: -1,
                    facing: -1,
                });
                self.run(FRAME_MS, |_| false)?;
                Ok(format!("load_map({name}, {parameter})"))
            }
        }
    }
}

/// The checks after a map was entered (or loaded fresh): the expected map,
/// no symbol faults, a passable landing with room to move, and the scripted
/// start honoured.
fn check_landing(
    audit: &mut UiAudit,
    warnings: &mut Warnings,
    game: &Game,
    label: &str,
    step: Option<&Step>,
    landing: (i32, u8),
) {
    let faults = game.faults();
    let symbols: Vec<String> = faults
        .iter()
        .filter_map(|(name, error)| match error {
            VmError::Symbol(symbol) => Some(format!("{name} ({symbol})")),
            _ => None,
        })
        .collect();
    for (name, error) in &faults {
        if !matches!(error, VmError::Symbol(_)) {
            warnings.warn("script faults", label, &format!("{name}: {error}"));
        }
    }
    audit.record(
        "symbol faults",
        label,
        if symbols.is_empty() {
            Ok(())
        } else {
            Err(format!(
                "{} script(s) faulted on a missing symbol: {}",
                symbols.len(),
                symbols.join(", ")
            ))
        },
    );
    let world = &game.host.world;
    let header = (world.map.header.entry_hex, world.map.header.entry_elevation);
    let (hex, elevation) = landing;
    let reach = game.reach();
    let blocker = (0..HEXES)
        .contains(&hex)
        .then(|| world.movement_blocker(hex, elevation, world.player))
        .flatten();
    let result = if !(0..HEXES).contains(&hex) {
        Err(format!("the player is off the map ({hex})"))
    } else if blocker.is_some() {
        Err(format!("hex {hex}/{elevation} is blocked"))
    } else if reach < MIN_REACH {
        Err(format!(
            "hex {hex}/{elevation} reaches only {reach} hexes (header default {}/{})",
            header.0, header.1
        ))
    } else {
        Ok(())
    };
    audit.record("landing", label, result);
    let overrides = game.map_script_overrides();
    let target = game.override_target();
    let expected = step.and_then(|step| step.hex);
    let result = match (expected, target) {
        // An exit grid's own destination wins over the scripted start
        // (SPEC-013 §3.22 step 2).
        _ if game.grid_landing.is_some() => match game.grid_landing {
            Some(grid) if grid == landing => Ok(()),
            Some((to, level)) => Err(format!(
                "the exit grid leads to {to}/{level} but the player stands on {hex}/{elevation}"
            )),
            None => Ok(()),
        },
        (Some(expected), _) => {
            let distance = HexGrid::angular_distance(hex, expected).unwrap_or(i32::MAX);
            if distance <= LANDING_SLACK {
                Ok(())
            } else {
                Err(format!(
                    "landed on {hex}/{elevation}, expected near {expected} \
                     (header default {}/{}, override_map_start {})",
                    header.0,
                    header.1,
                    if target.is_some() { "ran" } else { "never ran" }
                ))
            }
        }
        (None, Some(target))
            if HexGrid::angular_distance(hex, target).unwrap_or(i32::MAX) > LANDING_SLACK =>
        {
            Err(format!(
                "override_map_start chose {target} but the player stands on {hex}/{elevation}"
            ))
        }
        (None, None) if overrides && hex == header.0 => {
            if symbols.is_empty() {
                warnings.warn(
                    "scripted start",
                    label,
                    "the map script holds override_map_start but it did not run on this entry",
                );
                Ok(())
            } else {
                Err(format!(
                    "the map script's override_map_start never ran; the player stands on \
                     the header default {}/{}",
                    header.0, header.1
                ))
            }
        }
        _ => Ok(()),
    };
    audit.record("scripted start", label, result);
    println!(
        "Transition {label}: {} at {hex}/{elevation} (header {}/{}), reach {reach}{}, \
         {} faulted script(s){}",
        game.map_name(),
        header.0,
        header.1,
        if reach >= REACH_CAP { "+" } else { "" },
        faults.len(),
        match target {
            Some(target) => format!(", override_map_start {target}"),
            None if overrides => ", override_map_start not run".to_string(),
            None => String::new(),
        }
    );
}

/// SPEC-006 §3.5 / SPEC-013 §3.22-§3.23 / SPEC-014 §3.39 transition
/// readiness: from a fresh new-game start on `V13ENT.MAP`, each sequence
/// leaves map after map through the engine's own paths (exit grids, the
/// world map's town entry, a ladder's use, a scripted `load_map`), running
/// each map for three seconds of game time. Every entered map must be the
/// expected one, with no script faulted on a missing symbol (SPEC-005 §2.2:
/// exports do not outlive their map), the player on a passable hex with
/// room to move, and the map script's `override_map_start` honoured.
pub fn audit_transitions(
    vfs: &Arc<LocalDirProvider>,
    warnings: &mut Warnings,
) -> Result<UiAudit, Box<dyn Error>> {
    let mut audit = UiAudit::default();
    let (load_maps, _) = load_map_calls(vfs);
    let mut first = true;
    for (sequence, steps) in SEQUENCES {
        let mut game = match Game::start(vfs) {
            Ok(game) => game,
            Err(error) => {
                audit.record("reached", &format!("{sequence}: start"), Err(error));
                continue;
            }
        };
        if first {
            for note in game
                .diagnostics
                .iter()
                .filter(|note| note.starts_with("combat AI"))
            {
                warnings.warn("set-up", sequence, note);
            }
        }
        if let Err(error) = game.run(SETTLE_MS, |_| false) {
            audit.record("reached", &format!("{sequence}: start"), Err(error));
            continue;
        }
        if first {
            // The new-game map itself, entered fresh.
            let landing = game.player_at();
            check_landing(
                &mut audit,
                warnings,
                &game,
                "new game V13ENT.MAP",
                None,
                landing,
            );
            first = false;
        }
        for (index, step) in steps.iter().enumerate() {
            let from = game.map_name();
            let label = format!(
                "{sequence} {}: {from} -> {}",
                index + 1,
                step.map.unwrap_or("?")
            );
            game.host.calls.clear();
            let start_faults = game.diagnostics.len();
            let left = game.leave(step.via, &load_maps);
            let landing = game.player_at();
            let reached = game.map_name();
            let result = match &left {
                Err(error) => Err(error.clone()),
                Ok(_)
                    if step
                        .map
                        .is_some_and(|map| !map.eq_ignore_ascii_case(&reached)) =>
                {
                    Err(format!("reached {reached}"))
                }
                Ok(_) if step.map.is_none() && reached == from => Err(format!("still on {from}")),
                Ok(_) => Ok(()),
            };
            let ok = result.is_ok();
            audit.record("reached", &label, result);
            if !ok {
                // Later steps start from the wrong map.
                break;
            }
            let label = format!("{label} ({reached}, {})", left.unwrap_or_default());
            if let Err(error) = game.run(SETTLE_MS, |_| false) {
                audit.record("reached", &label, Err(error));
                break;
            }
            let symbols = game.diagnostics[start_faults..]
                .iter()
                .filter(|message| message.to_ascii_lowercase().contains("symbol"))
                .count();
            if symbols != 0 {
                eprintln!("NOTE {label}: {symbols} symbol diagnostic(s)");
            }
            check_landing(&mut audit, warnings, &game, &label, Some(step), landing);
        }
    }
    Ok(audit)
}
