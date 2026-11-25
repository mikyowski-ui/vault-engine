//! The Fallout 2 install report (`--report-fallout2-install`): over the
//! SPEC-018 §1 mount of an install folder, the §2.2 required members, the
//! `Fallout2Tables::load` result with its diagnostics grouped by kind, the
//! per-extension parse counts under the `fallout2` profile (SPEC-016 maps
//! and prototypes, SPEC-017's word set for `.int`), the map and prototype
//! writers' round trips, and a headless new-game smoke run: SPEC-018 §6's
//! hand-off onto `artemple` with its scripts running for a bounded number
//! of world ticks, with the VM errors by script, procedure and word and the
//! diagnostics by kind. `--mod-layer` mounts a mod's folders above the
//! install, so a total conversion's tables are the ones read, and `--sfall`
//! turns sfall mode on for the word table and the smoke run (SPEC-018 §2.4).
//!
//! A report, not an audit category: it never fails the run. Only counts,
//! file and procedure names, PIDs, word numbers, line numbers and lengths
//! are printed. Diagnostic text is grouped by kind: anything quoted becomes
//! `"…"`, and numbers become `N` (a four-digit `0x` word is kept).
//!
//! `--sfall-gap-report` is the second report here: every `.INT` a folder's
//! mount can reach, read linearly under sfall mode's meanings (SPEC-024b §3.2),
//! and ranked by how many scripts use each word, function and hook, with what
//! the engine does with it. Its tiers come from `vault_vm::sfall`'s catalog and
//! nowhere else.
use std::{
    collections::{BTreeMap, BTreeSet},
    error::Error,
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
};

use crate::archives::{data_root, entry_any_case, mod_layer, mount_install};
use vault_core::{
    CharacterSheet, GameProfile, StatContext, VmObserver, WorldEffect, WorldHost, WorldState,
    assets::{VfsWorldAssets, WorldAssets},
    fallout2::{Fallout2Tables, START_MAP},
    host_mods::{ModSession, ScriptedMod},
    movie::MoviePolicy,
    sfall_startup::{self, StartupOptions},
};
use vault_debug::decode::{DecodeMode, Instruction, disassemble_with};
use vault_formats::{
    LayeredVfs, LocalDirProvider, LocalPrototypeCatalog, MAX_ASSET_BYTES, MapFile, ModManifest,
    MsgCatalog, Prototype, Vfs, VfsError,
    fallout2::{Fallout2Install, REQUIRED_MEMBERS, missing_members},
    mod_manifest::MANIFEST,
    validate_asset,
};
use vault_vm::{
    IntProgram, RunOutcome, Value, Vm, VmError, WordSet,
    extensions::base_tag,
    sfall::{
        self, SFALL_EXPLOSIONS, SFALL_FUNCTIONS, SFALL_HOOKS, SFALL_METARULE3, SfallRow, Tier,
    },
    stack_check::Table,
};

/// The extensions parsed, in report order.
const PARSED: [&str; 13] = [
    "map", "pro", "frm", "fr0", "fr1", "fr2", "fr3", "fr4", "fr5", "msg", "pal", "lst", "int",
];
/// The smoke run's world time and frame step.
const SMOKE_MS: u64 = 60_000;
const FRAME_MS: u64 = 50;
/// How many line numbers a diagnostic kind lists.
const LINES_SHOWN: usize = 8;

/// A diagnostic's kind: quoted text becomes `"…"`, and numbers `N`, except
/// a four-digit `0x` word.
pub fn kind(message: &str) -> String {
    let mut out = String::new();
    let chars: Vec<char> = message.chars().collect();
    let mut at = 0;
    while at < chars.len() {
        let c = chars[at];
        if c == '"' {
            // Skip to the closing quote, honouring Debug escapes.
            at += 1;
            while at < chars.len() && chars[at] != '"' {
                at += if chars[at] == '\\' { 2 } else { 1 };
            }
            out.push_str("\"…\"");
            at += 1;
            continue;
        }
        if c == '0' && chars.get(at + 1) == Some(&'x') {
            let digits = chars[at + 2..]
                .iter()
                .take_while(|c| c.is_ascii_hexdigit())
                .count();
            if digits == 4 {
                out.extend(&chars[at..at + 6]);
            } else {
                out.push_str("0xN");
            }
            at += 2 + digits;
            continue;
        }
        if c.is_ascii_digit() {
            while at < chars.len() && chars[at].is_ascii_digit() {
                at += 1;
            }
            out.push('N');
            continue;
        }
        out.push(c);
        at += 1;
    }
    out
}

/// Diagnostics grouped by kind: each kind's count and first line numbers.
#[derive(Default)]
struct Kinds(BTreeMap<String, (u64, Vec<usize>)>);

impl Kinds {
    fn add(&mut self, kind: String, line: Option<usize>) {
        let entry = self.0.entry(kind).or_default();
        entry.0 += 1;
        if let Some(line) = line
            && entry.1.len() < LINES_SHOWN
        {
            entry.1.push(line);
        }
    }
    fn total(&self) -> u64 {
        self.0.values().map(|(count, _)| count).sum()
    }
    fn print(&self, title: &str) {
        let mut rows: Vec<_> = self.0.iter().collect();
        rows.sort_by(|a, b| b.1.0.cmp(&a.1.0).then(a.0.cmp(b.0)));
        for (kind, (count, lines)) in rows {
            if lines.is_empty() {
                println!("{title}: {count} x {kind}");
            } else {
                let lines: Vec<String> = lines.iter().map(usize::to_string).collect();
                println!("{title}: {count} x {kind} (lines {})", lines.join(", "));
            }
        }
    }
}

/// SPEC-018 §2.4 rule 4: the scripted layers' session, which turns manifest
/// scripting on. It writes no setting.
fn mod_session(layers: &[PathBuf]) -> Option<Arc<ModSession>> {
    let mut mods = Vec::new();
    for layer in layers {
        let Some(bytes) = loose_file(layer, MANIFEST) else {
            continue;
        };
        let parsed = std::str::from_utf8(&bytes)
            .map_err(|error| error.to_string())
            .and_then(|text| ModManifest::parse(text).map_err(|error| error.to_string()));
        match parsed {
            Ok(manifest) if manifest.scripted() => {
                mods.push(ScriptedMod::from_manifest(&manifest, &bytes));
            }
            Ok(_) => {}
            Err(error) => println!("Fallout 2 mod manifest refused: {}", kind(&error)),
        }
    }
    (!mods.is_empty()).then(|| Arc::new(ModSession::new(mods, false)))
}

/// The file `name` directly in `folder`, the name compared without case.
fn loose_file(folder: &Path, name: &str) -> Option<Vec<u8>> {
    std::fs::read(entry_any_case(folder, name, Path::is_file)?).ok()
}

/// SPEC-024c §3.1: `ddraw.ini`, through the layers, then the install folder.
fn startup_options(vfs: &dyn Vfs, folder: &Path) -> Arc<StartupOptions> {
    let bytes = vfs
        .read_bounded(sfall_startup::FILE, MAX_ASSET_BYTES)
        .ok()
        .or_else(|| loose_file(folder, sfall_startup::FILE));
    let found = bytes.is_some();
    println!(
        "Fallout 2 {}: {}",
        sfall_startup::FILE,
        if found { "read" } else { "none" }
    );
    Arc::new(bytes.map_or_else(StartupOptions::default, |bytes| {
        StartupOptions::parse(&bytes)
    }))
}

/// What the report reads, and the install under any mod layers.
type Mounted = (Arc<dyn Vfs>, Arc<dyn Vfs>);

/// SPEC-018 §1: the install's databases, with its loose `data` folder as
/// `data_root` when it has one, and `layers` above them, lowest first.
fn mount(folder: &Path, layers: &[PathBuf]) -> Result<Mounted, Box<dyn Error>> {
    let install = Fallout2Install::discover(folder)?;
    let opened = install.open()?;
    println!(
        "Fallout 2 install: MASTER.DAT, CRITTER.DAT{}",
        if install.patch.is_some() {
            ", patch000.dat above MASTER.DAT"
        } else {
            ""
        }
    );
    let loose = data_root(folder)?;
    let loose_files = loose.as_ref().map(|loose| loose.files().count());
    let provider = opened.provider(loose);
    let missing = missing_members(&provider);
    match loose_files {
        Some(count) => println!("Fallout 2 data root: data ({count} files)"),
        None => println!("Fallout 2 data root: none"),
    }
    println!("Fallout 2 mounted files: {}", provider.files().count());
    println!(
        "Fallout 2 required members: {} of {} present{}",
        REQUIRED_MEMBERS.len() - missing.len(),
        REQUIRED_MEMBERS.len(),
        if missing.is_empty() {
            String::new()
        } else {
            format!("; missing {}", missing.join(", "))
        }
    );
    let base: Arc<dyn Vfs> = Arc::new(provider);
    if layers.is_empty() {
        return Ok((base.clone(), base));
    }
    let mounted = layers
        .iter()
        .map(|layer| mod_layer(layer))
        .collect::<Result<Vec<_>, _>>()?;
    println!("Fallout 2 mod layers: {}", mounted.len());
    Ok((Arc::new(LayeredVfs::new(base.clone(), mounted)), base))
}

/// `Fallout2Tables::load` over the master database as mounted.
fn tables(vfs: &dyn Vfs) -> Option<Arc<Fallout2Tables>> {
    let mut read = |path: &str| -> Result<Option<Vec<u8>>, String> {
        match vfs.read_bounded(path, MAX_ASSET_BYTES) {
            Ok(bytes) => Ok(Some(bytes)),
            Err(VfsError::NotFound(_)) => Ok(None),
            Err(error) => Err(error.to_string()),
        }
    };
    match Fallout2Tables::load(&mut read) {
        Ok(tables) => {
            let groups = tables.worldmap.groups.len();
            println!(
                "Fallout 2 tables: loaded: {} maps, {} areas, {} encounter tables, {groups} \
                 encounter groups, {} party records, {} quests, {} holodisks, {} reputations, \
                 {} karma variables, {} endgame slides, {} walk masks",
                tables.maps.count(),
                tables.city.count(),
                tables.worldmap.tables.len(),
                tables.party.count(),
                tables.quests.len(),
                tables.holodisks.len(),
                tables.genrep.len(),
                tables.karmavar.len(),
                tables.endgame.len(),
                tables.masks.len()
            );
            let mut kinds = Kinds::default();
            for note in &tables.diagnostics {
                kinds.add(
                    format!("{}: {}", note.file, kind(&note.message)),
                    Some(note.line),
                );
            }
            println!("Fallout 2 table diagnostics: {}", kinds.total());
            kinds.print("Fallout 2 table diagnostic");
            for record in tables.maps.records() {
                // SPEC-024c §4.4: a record sharing a lower record's file.
                let first = tables.maps.records().find(|other| {
                    other.number < record.number && other.stem.eq_ignore_ascii_case(&record.stem)
                });
                if let Some(first) = first {
                    println!(
                        "Fallout 2 map file shared: map {} with map {}: {}",
                        record.number, first.number, record.stem
                    );
                }
            }
            Some(Arc::new(tables))
        }
        Err(error) => {
            println!("Fallout 2 tables: refused: {}", kind(&error.to_string()));
            None
        }
    }
}

/// One extension's results.
#[derive(Default)]
struct Parsed {
    passed: u64,
    failed: u64,
}

/// SPEC-018 §2.4 rule 3: the size of a refused `.pal` that the install
/// `base` has no file at, so a mod layer serves it, and that is not
/// `color.pal`.
fn auxiliary_palette(vfs: &dyn Vfs, base: &dyn Vfs, path: &str) -> Option<usize> {
    let name = path.rsplit('/').next()?;
    if name.eq_ignore_ascii_case("color.pal") {
        return None;
    }
    let bytes = vfs.read_bounded(path, MAX_ASSET_BYTES).ok()?;
    let absent = matches!(
        base.read_bounded(path, MAX_ASSET_BYTES),
        Err(VfsError::NotFound(_))
    );
    absent.then_some(bytes.len())
}

/// A map refused over the mod layers that belongs to the install underneath:
/// the install serves it, no `maps.txt` record names it, and it parses with
/// the install's own prototypes. A mod that replaces prototypes leaves such
/// maps behind, and no game reaches them. As SPEC-018 §2.4 does for a
/// palette, this belongs to the report alone.
fn leftover_map(
    vfs: &dyn Vfs,
    catalog: &LocalPrototypeCatalog,
    tables: Option<&Fallout2Tables>,
    path: &str,
) -> bool {
    let named = |stem: &str| {
        tables.is_none_or(|tables| {
            let mut records = tables.maps.records();
            records.any(|record| record.stem.eq_ignore_ascii_case(stem))
        })
    };
    let stem = path.rsplit('/').next().and_then(|name| {
        let (stem, extension) = name.rsplit_once('.')?;
        extension.eq_ignore_ascii_case("map").then_some(stem)
    });
    if stem.is_none_or(named) {
        return false;
    }
    let Ok((bytes, None)) = vfs.read_with_layer(path, MAX_ASSET_BYTES) else {
        return false;
    };
    MapFile::parse_for(&bytes, catalog, GameProfile::Fallout2).is_ok()
}

/// Every member of a `PARSED` extension under the `fallout2` profile, with
/// the map and prototype writers' round trips. `words` checks each script,
/// and `base` is the install under any mod layers.
fn parse_all(
    vfs: &dyn Vfs,
    base: &dyn Vfs,
    words: &WordSet,
    tables: Option<&Fallout2Tables>,
) -> u64 {
    let catalog = LocalPrototypeCatalog::for_game(vfs, GameProfile::Fallout2);
    let install = (vfs.mod_layers() > 0)
        .then(|| LocalPrototypeCatalog::for_game(base, GameProfile::Fallout2));
    let mut parsed: BTreeMap<&str, Parsed> = BTreeMap::new();
    let mut failures = Kinds::default();
    let mut map_notes = Kinds::default();
    let mut stops_early = 0_u64;
    let mut short_critters = Vec::new();
    let [mut map_writes, mut pro_writes] = [[0_u64; 2]; 2];
    let (mut versions, mut auxiliary, mut leftovers) = ([0_u64; 2], 0_u64, 0_u64);
    for path in vfs.files() {
        let Some(extension) = path.extension().filter(|ext| PARSED.contains(ext)) else {
            continue;
        };
        let result: Result<(), String> = vfs
            .read_bounded(path.as_str(), MAX_ASSET_BYTES)
            .map_err(|error| error.to_string())
            .and_then(|bytes| {
                let text = |error: vault_formats::ParseError| error.to_string();
                match extension {
                    "map" => {
                        let map = MapFile::parse_for(&bytes, &catalog, GameProfile::Fallout2)
                            .map_err(text)?;
                        for note in &map.diagnostics {
                            map_notes.add(kind(note), None);
                        }
                        versions[usize::from(map.header.version == 20)] += 1;
                        // The writer is a parse/write fixpoint.
                        let round = map.write().map_err(text).and_then(|first| {
                            let again = MapFile::parse_for(&first, &catalog, GameProfile::Fallout2)
                                .map_err(text)?
                                .write()
                                .map_err(text)?;
                            Ok(again == first)
                        });
                        map_writes[usize::from(round != Ok(true))] += 1;
                        Ok(())
                    }
                    "pro" => {
                        let pro =
                            Prototype::parse_for(&bytes, GameProfile::Fallout2).map_err(text)?;
                        if !pro.diagnostics.is_empty() {
                            short_critters.push((pro.pid, bytes.len()));
                        }
                        let same = pro.write() == bytes[..pro.consumed_len]
                            && pro.consumed_len == bytes.len();
                        pro_writes[usize::from(!same)] += 1;
                        Ok(())
                    }
                    "int" => IntProgram::parse_in(&bytes, words, false)
                        .map(|_| ())
                        .map_err(|error| error.to_string()),
                    "msg" => MsgCatalog::parse(&bytes)
                        .map(|catalog| {
                            if catalog.failure().is_some() {
                                stops_early += 1;
                            }
                        })
                        .map_err(|error| error.to_string()),
                    _ => validate_asset(path, &bytes)
                        .expect("a validated extension")
                        .map_err(text),
                }
            });
        if result.is_err()
            && extension == "pal"
            && let Some(length) = auxiliary_palette(vfs, base, path.as_str())
        {
            println!("auxiliary palette: {path} {length}");
            auxiliary += 1;
            continue;
        }
        if result.is_err()
            && extension == "map"
            && let Some(install) = &install
            && leftover_map(vfs, install, tables, path.as_str())
        {
            println!("leftover map: {path}");
            leftovers += 1;
            continue;
        }
        let entry = parsed.entry(extension).or_default();
        match result {
            Ok(()) => entry.passed += 1,
            Err(error) => {
                entry.failed += 1;
                failures.add(format!(".{extension}: {}", kind(&error)), None);
                println!("Fallout 2 parse failure: {path}");
            }
        }
    }
    println!("Fallout 2 format\tpassed\tfailed");
    for extension in PARSED {
        let entry = parsed.get(extension);
        println!(
            ".{extension}\t{}\t{}",
            entry.map_or(0, |entry| entry.passed),
            entry.map_or(0, |entry| entry.failed)
        );
    }
    println!(
        "Fallout 2 maps: {} version 19, {} version 20",
        versions[0], versions[1]
    );
    println!("Fallout 2 auxiliary palettes: {auxiliary}");
    println!("Fallout 2 leftover maps: {leftovers}");
    failures.print("Fallout 2 parse failure kind");
    let failed: u64 = parsed.values().map(|entry| entry.failed).sum();
    println!(
        "Fallout 2 parsing: {} passed, {failed} failed",
        parsed.values().map(|entry| entry.passed).sum::<u64>()
    );
    println!("Fallout 2 message files stopping early (SPEC-002 §3.5): {stops_early}");
    short_critters.sort_unstable();
    let pids: Vec<String> = short_critters
        .iter()
        .map(|(pid, length)| format!("{pid:#010x} ({length} bytes)"))
        .collect();
    println!(
        "Fallout 2 critter prototypes read at 412 bytes (SPEC-016 Decision 9): {}{}",
        short_critters.len(),
        if pids.is_empty() {
            String::new()
        } else {
            format!(": {}", pids.join(", "))
        }
    );
    println!(
        "Fallout 2 writers: .map {} round trips, {} failed; .pro {} round trips, {} failed",
        map_writes[0], map_writes[1], pro_writes[0], pro_writes[1]
    );
    println!("Fallout 2 map diagnostics: {}", map_notes.total());
    map_notes.print("Fallout 2 map diagnostic");
    failed + map_writes[1] + pro_writes[1]
}

/// The step under way in one VM: its id, address and instruction word.
struct Step {
    id: u32,
    pc: u32,
    word: Option<u16>,
}

/// Records each failed step: its program, procedure and word.
#[derive(Default)]
struct Faults {
    pending: Vec<Step>,
    /// (program id, procedure, word, error kind) to count.
    faults: BTreeMap<(u32, String, Option<u16>, String), u64>,
}

impl VmObserver for Faults {
    fn before_step(&mut self, vm: &Vm) {
        let pc = vm.pc as usize;
        let word = vm
            .program
            .image()
            .get(pc..pc + 2)
            .map(|bytes| u16::from_be_bytes([bytes[0], bytes[1]]));
        self.pending.push(Step {
            id: vm.id,
            pc: vm.pc,
            word,
        });
    }
    fn after_step(&mut self, vm: &Vm, result: &Result<RunOutcome, VmError>) {
        let Some(step) = self.pending.pop() else {
            return;
        };
        let Err(error) = result else {
            return;
        };
        // The procedure whose body starts last at or before the address.
        let procedure = vm
            .program
            .procedures()
            .iter()
            .filter(|procedure| procedure.body <= step.pc)
            .max_by_key(|procedure| procedure.body)
            .map_or_else(
                || "?".to_owned(),
                |procedure| String::from_utf8_lossy(&procedure.name).into_owned(),
            );
        *self
            .faults
            .entry((step.id, procedure, step.word, kind(&error.to_string())))
            .or_default() += 1;
    }
}

/// SPEC-018 §6 the new-game hand-off onto the start map, with scripts
/// running, then `SMOKE_MS` of world ticks.
fn smoke(
    vfs: Arc<dyn Vfs>,
    tables: Arc<Fallout2Tables>,
    session: Option<Arc<ModSession>>,
    startup: Option<Arc<StartupOptions>>,
) -> Result<(), Box<dyn Error>> {
    let assets = VfsWorldAssets::new(vfs, "english")?;
    assets.set_game_profile(GameProfile::Fallout2);
    // SPEC-024c §3: the file chooses the map, when sfall mode reads one.
    let start = startup.as_ref().map(|startup| {
        startup.new_game_start(
            START_MAP,
            |path| assets.available(path.as_bytes()),
            tables.worldmap.world_size(),
        )
    });
    let map = start
        .as_ref()
        .map_or_else(|| START_MAP.to_owned(), |start| start.map.clone());
    // SPEC-014 §3.3's first premade, as Play's `t` takes it.
    let sheet = match assets.gcd("premade/combat.gcd") {
        Ok(gcd) => CharacterSheet::from_gcd(&gcd, StatContext::default()),
        Err(_) => {
            println!("Fallout 2 smoke run: premade/combat.gcd did not load; a default sheet");
            CharacterSheet::default()
        }
    };
    let appearance = sheet.read_words.map(|(_, appearance)| appearance);
    let mut host = WorldHost::new(WorldState::new(sheet, 1)?, assets);
    host.begin_fallout2_game_on(tables, &map);
    if let Some(startup) = startup {
        host.enable_sfall()?;
        let start = start.unwrap_or_else(|| startup.new_game_start(&map, |_| true, [i32::MAX; 2]));
        for note in start.notes {
            host.world.diagnostic(note);
        }
        host.apply_startup(startup, start.world_pos);
    }
    if let Some(session) = session {
        host.enable_mod_scripts(session);
    }
    host.sfall_take_sheet(appearance);
    host.update_player_gender();
    host.movie_policy = MoviePolicy::CompleteImmediately;
    host.load_campaign()?;
    host.load_language_filter();
    if let Err(error) = host.load_combat_ai() {
        println!(
            "Fallout 2 smoke run: combat AI unavailable: {}",
            kind(&error.to_string())
        );
    }
    // The table diagnostics are reported above.
    host.world.effects.clear();
    let faults = Arc::new(Mutex::new(Faults::default()));
    host.vm_observer = Some(faults.clone());
    let drain = |host: &mut WorldHost<VfsWorldAssets>, kinds: &mut Kinds| {
        for effect in host.world.effects.drain(..) {
            if let WorldEffect::Diagnostic(message) = effect {
                kinds.add(kind(&message), None);
            }
        }
    };
    let mut handoff = Kinds::default();
    let loaded = host.load_map(&map);
    drain(&mut host, &mut handoff);
    if let Err(error) = loaded {
        println!(
            "Fallout 2 smoke run: {map} did not load: {}",
            kind(&error.to_string())
        );
        return Ok(());
    }
    let mut ticking = Kinds::default();
    let (mut now, mut frames) = (1000, 0_u64);
    let mut stopped = None;
    while now < 1000 + SMOKE_MS {
        if let Err(error) = host.tick(now) {
            stopped = Some(kind(&error.to_string()));
            break;
        }
        drain(&mut host, &mut ticking);
        now += FRAME_MS;
        frames += 1;
    }
    drain(&mut host, &mut ticking);
    host.vm_observer = None;
    let map = host.world.map.header.map_number;
    let name = host.world.current_map_name().unwrap_or_default();
    let scripts: Vec<_> = host.world.scripts.values().collect();
    let loaded_programs = host.programs.len();
    println!(
        "Fallout 2 smoke run: {name} (map {map}), {frames} ticks of {FRAME_MS} ms, {} script \
         records, {loaded_programs} programs loaded, Fallout 2 word set: {}",
        scripts.len(),
        host.programs
            .values()
            .all(|vm| vm.program.words().fallout2())
    );
    if let Some(error) = &stopped {
        println!("Fallout 2 smoke run: a tick failed: {error}");
    }
    // Each script record: its file, the locals `scripts.lst` declares, and
    // the slice the record has.
    let script_name = |file: i32| {
        host.assets
            .program(file)
            .map(|(path, _)| path)
            .unwrap_or_else(|_| format!("script {file}"))
    };
    for instance in &scripts {
        println!(
            "Fallout 2 smoke script: {:#010x} {}: {} locals declared, a slice of {}",
            instance.id,
            script_name(instance.file),
            host.assets.local_count(instance.file),
            instance.local_count
        );
    }
    // Each failing program by its script file.
    let program_file: BTreeMap<u32, i32> = scripts
        .iter()
        .map(|instance| (instance.program, instance.file))
        .collect();
    let faults = std::mem::take(&mut faults.lock().unwrap_or_else(|e| e.into_inner()).faults);
    let mut grouped: BTreeMap<(String, String, Option<u16>, String), u64> = BTreeMap::new();
    for ((id, procedure, word, error), count) in faults {
        let script = program_file
            .get(&id)
            .map_or_else(|| format!("program {id}"), |file| script_name(*file));
        *grouped.entry((script, procedure, word, error)).or_default() += count;
    }
    println!(
        "Fallout 2 smoke VM errors: {} ({} distinct)",
        grouped.values().sum::<u64>(),
        grouped.len()
    );
    for ((script, procedure, word, error), count) in &grouped {
        let word = word.map_or_else(|| "?".to_owned(), |word| format!("{word:#06x}"));
        println!("Fallout 2 smoke VM error: {count} x {script} {procedure} word {word}: {error}");
    }
    let mut by_error: BTreeMap<&str, u64> = BTreeMap::new();
    for ((_, _, _, error), count) in &grouped {
        *by_error.entry(error).or_default() += count;
    }
    for (error, count) in by_error {
        println!("Fallout 2 smoke VM error kind: {count} x {error}");
    }
    println!("Fallout 2 smoke hand-off diagnostics: {}", handoff.total());
    handoff.print("Fallout 2 smoke hand-off diagnostic");
    println!("Fallout 2 smoke tick diagnostics: {}", ticking.total());
    ticking.print("Fallout 2 smoke tick diagnostic");
    Ok(())
}

/// The whole report; the last line says whether every member parsed and
/// round-tripped and the tables loaded.
pub fn report(folder: &Path, layers: &[PathBuf], sfall: bool) -> Result<(), Box<dyn Error>> {
    let session = mod_session(layers);
    let (words, name) = if sfall {
        (WordSet::FALLOUT2_SFALL, "fallout2 sfall")
    } else {
        (WordSet::FALLOUT2, "fallout2")
    };
    let words = words.with_mods(session.is_some());
    let mods = if session.is_some() {
        ", with the mod words"
    } else {
        ""
    };
    println!("Fallout 2 word table: {name}{mods}");
    let (vfs, base) = mount(folder, layers)?;
    let tables = tables(&*vfs);
    let failed = parse_all(&*vfs, &*base, &words, tables.as_deref());
    let startup = sfall.then(|| startup_options(&*vfs, folder));
    let loaded = tables.is_some();
    match tables {
        Some(tables) => smoke(vfs, tables, session, startup)?,
        None => println!("Fallout 2 smoke run: not run (the tables were refused)"),
    }
    println!(
        "Fallout 2 install check: {}",
        if failed == 0 && loaded {
            "passed"
        } else {
            "failed"
        }
    );
    Ok(())
}

/// How many script names a ranked row shows.
const SCRIPTS_SHOWN: usize = 3;
/// A row's status. [`UNIMPLEMENTED`] covers a reserved word, a number or name
/// outside the catalog, and a hook ID without a row.
const IMPLEMENTED: &str = "P1";
const STUBBED: &str = "P2 stubbed";
const TIER_X: &str = "tier X";
const UNIMPLEMENTED: &str = "unimplemented";
/// The selector is not a constant this scan can read, so the row is unknown.
const UNRESOLVED: &str = "unresolved";
const REGISTER_HOOK: u16 = 0x8207;
const REGISTER_HOOK_PROC: u16 = 0x8262;
const REGISTER_HOOK_PROC_SPEC: u16 = 0x827d;
/// SPEC-004's local call, which pops its index and argument count, then the
/// arguments, and leaves one result.
const CALL: u16 = 0x8005;
/// SPEC-017 §5.2's own `metarule3` rules, which are not sfall's.
const ENGINE_METARULES: std::ops::RangeInclusive<i32> = 100..=111;

fn tier_status(tier: Tier) -> &'static str {
    match tier {
        Tier::P1 => IMPLEMENTED,
        Tier::P2 => STUBBED,
        Tier::X => TIER_X,
        Tier::R => UNIMPLEMENTED,
    }
}

/// A row of a ranked table: its number, name and status.
type RowKey = (i32, String, &'static str);

/// What uses one row.
#[derive(Default)]
struct Usage {
    scripts: BTreeSet<String>,
    uses: u64,
}

/// The scan's tallies.
#[derive(Default)]
struct Gap {
    scanned: u64,
    /// Scripts that could not be read or decoded, with why.
    failed: Vec<(String, String)>,
    words: BTreeMap<RowKey, Usage>,
    hooks: BTreeMap<RowKey, Usage>,
}

fn count(rows: &mut BTreeMap<RowKey, Usage>, row: RowKey, script: &str) {
    let usage = rows.entry(row).or_default();
    usage.uses += 1;
    if !usage.scripts.contains(script) {
        usage.scripts.insert(script.to_owned());
    }
}

/// The values `instruction` pops and pushes, when it can sit inside an operand
/// expression and they are fixed. `None` stops the walk of [`operand`]: control
/// flow and a variable or reserved word have no such effect.
fn effect(instruction: &Instruction, table: &Table) -> Option<(usize, usize)> {
    if instruction.immediate.is_some() {
        return Some((0, 1));
    }
    let (pops, pushes) = match instruction.word {
        0x8000 | 0x8002 | 0x8003 | 0x804a | 0x804b => (0, 0),
        0x800c => (0, 1),
        0x8012 | 0x8014 | 0x8028 | 0x802d | 0x8032 | 0x8043..=0x8046 => (1, 1),
        0x8033..=0x8042 => (2, 1),
        word @ 0x804c..=0xffff => table.signature(word)?,
        _ => return None,
    };
    Some((usize::from(pops), usize::from(pushes)))
}

/// The constant push that left the operand `depth` values below the top of the
/// stack when `listing[at]` runs, walking back through straight-line code. `None`
/// when a walk meets anything [`effect`] stops at, or the value is computed.
/// Every jump into an expression needs a jump over it first, so stopping at
/// control flow also keeps the walk from crossing a merge.
fn operand<'a>(
    listing: &'a [Instruction],
    at: usize,
    mut depth: usize,
    table: &Table,
) -> Option<&'a Instruction> {
    for index in (0..at).rev() {
        let instruction = &listing[index];
        let (pops, pushes) = if instruction.word == CALL {
            let arguments = operand(listing, index, 1, table).and_then(constant_int)?;
            (2 + usize::try_from(arguments).ok()?, 1)
        } else {
            effect(instruction, table)?
        };
        if depth < pushes {
            return instruction.immediate.is_some().then_some(instruction);
        }
        depth = depth - pushes + pops;
    }
    None
}

/// The value of an integer push.
fn constant_int(instruction: &Instruction) -> Option<i32> {
    let value = Value {
        payload: instruction.immediate?,
        tag: instruction.word,
    };
    (base_tag(value) == Value::INTEGER).then_some(value.payload as i32)
}

/// The text of a static string push.
fn constant_text<'a>(program: &'a IntProgram, instruction: &Instruction) -> Option<&'a [u8]> {
    let value = Value {
        payload: instruction.immediate?,
        tag: instruction.word,
    };
    (base_tag(value) == Value::STATIC_STRING)
        .then(|| program.literal(value.payload))
        .flatten()
}

/// Whether `text` has a function name's shape. A script's other strings are
/// its contents, and those are never printed.
fn name_shaped(text: &[u8]) -> bool {
    (1..=64).contains(&text.len())
        && text
            .iter()
            .all(|byte| byte.is_ascii_alphanumeric() || *byte == b'_')
}

/// A sub-function's row of `rows`: `family`'s unknown-number rule otherwise.
fn sub_function(rows: &[SfallRow], family: &str, sub: Option<i32>) -> (String, &'static str) {
    let Some(sub) = sub else {
        return (format!("{family}(?)"), UNRESOLVED);
    };
    rows.iter()
        .find(|row| i32::from(row.id) == sub)
        .map_or_else(
            || (format!("{family}({sub})"), UNIMPLEMENTED),
            |row| (row.name.to_owned(), tier_status(row.tier)),
        )
}

/// A `sfall_funcN` call: its function by name, as §4.2 matches it.
fn named_function(family: &str, name: Option<&[u8]>) -> (String, &'static str) {
    let Some(name) = name else {
        return (format!("{family}(?)"), UNRESOLVED);
    };
    let shown = String::from_utf8_lossy(name).into_owned();
    match sfall::function_tier(name) {
        Some(tier) => (shown, tier_status(tier)),
        None if name_shaped(name) => (shown, UNIMPLEMENTED),
        None => (format!("{family}(not a name)"), UNIMPLEMENTED),
    }
}

/// What the instruction `listing[at]` counts as in the word table, under
/// SPEC-024b's meanings, if it is an sfall word at all. A dispatcher call counts
/// as the row it selects: the deepest operand, in every dispatcher's public call
/// order the first argument.
fn word_use(
    program: &IntProgram,
    listing: &[Instruction],
    at: usize,
    table: &Table,
) -> Option<(String, &'static str)> {
    let word = listing[at].word;
    let selector = |row: &SfallRow| {
        let depth = usize::from(row.pops.unwrap_or(1)).saturating_sub(1);
        operand(listing, at, depth, table)
    };
    let metarule3 = &SFALL_METARULE3[0];
    if word == metarule3.dispatch {
        let sub = selector(metarule3).and_then(constant_int);
        if sub.is_some_and(|sub| ENGINE_METARULES.contains(&sub)) {
            return None;
        }
        return Some(sub_function(&SFALL_METARULE3, "metarule3", sub));
    }
    if word > sfall::LAST_WORD {
        return Some(sfall::mod_row(word).map_or_else(
            || ("unknown".to_owned(), UNIMPLEMENTED),
            |row| (row.name.to_owned(), tier_status(row.tier)),
        ));
    }
    let row = sfall::word_row(word)?;
    if word == SFALL_EXPLOSIONS[0].dispatch {
        let sub = selector(row).and_then(constant_int);
        return Some(sub_function(&SFALL_EXPLOSIONS, row.name, sub));
    }
    if SFALL_FUNCTIONS.iter().any(|row| row.dispatch == word) {
        let name = selector(row).and_then(|name| constant_text(program, name));
        return Some(named_function(row.name, name));
    }
    let name = if row.tier == Tier::R {
        "reserved"
    } else {
        row.name
    };
    Some((name.to_owned(), tier_status(row.tier)))
}

/// A hook by ID, whatever registered it. §6.2 ignores an ID without a row.
fn hook_key(id: i32) -> RowKey {
    match sfall::hook_row(id) {
        Some(row) => (
            id,
            row.name.to_owned(),
            if row.fires { IMPLEMENTED } else { STUBBED },
        ),
        None => (id, "unknown".to_owned(), UNIMPLEMENTED),
    }
}

/// The hook `listing[at]` registers, when it is §6.2's registration word: the
/// hook ID is its first argument, and procedure 0 unregisters instead.
fn registered_hook(listing: &[Instruction], at: usize, table: &Table) -> Option<RowKey> {
    let word = listing[at].word;
    let pops = match word {
        REGISTER_HOOK => 1,
        REGISTER_HOOK_PROC | REGISTER_HOOK_PROC_SPEC => 2,
        _ => return None,
    };
    let int = |depth| operand(listing, at, depth, table).and_then(constant_int);
    if pops == 2 && int(0) == Some(0) {
        return None;
    }
    Some(int(pops - 1).map_or((-1, "(hook not read)".to_owned(), UNRESOLVED), hook_key))
}

/// §6.1: the hooks a `.INT` serves by its name alone, under a `scripts` folder.
fn hook_file_rows(path: &str) -> Vec<RowKey> {
    let mut parts = path.rsplit('/');
    let (Some(file), Some("scripts")) = (parts.next(), parts.next()) else {
        return Vec::new();
    };
    SFALL_HOOKS
        .iter()
        .filter(|row| row.file.eq_ignore_ascii_case(file))
        .map(|row| hook_key(row.id))
        .collect()
}

/// Every `.INT` in `vfs`, by path, whatever folder holds it. A script that
/// cannot be read or decoded is counted and named, and the next one is read.
fn scan(vfs: &dyn Vfs) -> Gap {
    // SPEC-024b §3.2: the loader must not refuse what this report exists to
    // show, so every bit-15 word decodes, and the tables below say what it is.
    let every = WordSet {
        words: 0x8000..=0xffff,
        mods: false,
    };
    let mode = DecodeMode::new(&every, false);
    let table = Table::new(WordSet::FALLOUT2_SFALL.with_mods(true));
    let mut gap = Gap::default();
    for path in vfs.files().filter(|path| path.extension() == Some("int")) {
        let path = path.as_str();
        let script = path.strip_prefix("scripts/").unwrap_or(path);
        let parsed = vfs
            .read_bounded(path, MAX_ASSET_BYTES)
            .map_err(|error| error.to_string())
            .and_then(|bytes| {
                IntProgram::parse_in(&bytes, &every, false).map_err(|error| error.to_string())
            });
        let program = match parsed {
            Ok(program) => program,
            Err(error) => {
                gap.failed.push((script.to_owned(), kind(&error)));
                continue;
            }
        };
        gap.scanned += 1;
        for row in hook_file_rows(path) {
            count(&mut gap.hooks, row, script);
        }
        let start = program.code_start as u32;
        let listing = disassemble_with(program.image(), start, usize::MAX, mode);
        for at in 0..listing.len() {
            if listing[at].immediate.is_some() {
                continue;
            }
            if let Some((name, status)) = word_use(&program, &listing, at, &table) {
                let row = (i32::from(listing[at].word), name, status);
                count(&mut gap.words, row, script);
            }
            if let Some(row) = registered_hook(&listing, at, &table) {
                count(&mut gap.hooks, row, script);
            }
        }
    }
    gap
}

/// Rows ranked by scripts affected, then uses, then number and name, so the
/// output is deterministic.
fn ranked(rows: &BTreeMap<RowKey, Usage>) -> Vec<(&RowKey, &Usage)> {
    let mut ranked: Vec<_> = rows.iter().collect();
    ranked.sort_by(|a, b| {
        let rank = |row: &(&RowKey, &Usage)| (row.1.scripts.len(), row.1.uses);
        rank(b).cmp(&rank(a)).then_with(|| a.0.cmp(b.0))
    });
    ranked
}

/// The table `rows`, each row's number written by `number`.
fn print_table(
    title: &str,
    column: &str,
    rows: &BTreeMap<RowKey, Usage>,
    number: impl Fn(&RowKey) -> String,
) {
    let uses: u64 = rows.values().map(|usage| usage.uses).sum();
    println!("Sfall gap {title}: {} rows, {uses} uses", rows.len());
    println!("{column}\tname\tstatus\tscripts\tuses\tfirst scripts");
    for (key, usage) in ranked(rows) {
        let shown: Vec<&str> = usage
            .scripts
            .iter()
            .take(SCRIPTS_SHOWN)
            .map(String::as_str)
            .collect();
        let more = usage.scripts.len() - shown.len();
        let more = if more == 0 {
            String::new()
        } else {
            format!(", +{more} more")
        };
        println!(
            "{}\t{}\t{}\t{}\t{}\t{}{more}",
            number(key),
            key.1,
            key.2,
            usage.scripts.len(),
            usage.uses,
            shown.join(", ")
        );
    }
}

/// The files directly in `dir`, by name.
fn entries(dir: &Path) -> Vec<PathBuf> {
    let found = std::fs::read_dir(dir).into_iter().flatten().flatten();
    let mut paths: Vec<PathBuf> = found.map(|entry| entry.path()).collect();
    paths.sort();
    paths
}

fn is_dat(path: &Path) -> bool {
    let extension = path.extension().and_then(|extension| extension.to_str());
    path.is_file() && extension.is_some_and(|extension| extension.eq_ignore_ascii_case("dat"))
}

/// `folder` as the report reads it. A folder with both databases is an install,
/// over its `data` root; any other is a mod's, its files loose. Above that, by
/// name, come the `.dat` files beside the databases and those in `mods`, then
/// (for an install, whose `data` root does not reach them) the folders in `mods`,
/// then `layers`. A layer that will not mount is named, and the rest are read.
fn gap_mount(folder: &Path, layers: &[PathBuf]) -> Result<Arc<dyn Vfs>, Box<dyn Error>> {
    let databases = ["MASTER.DAT", "CRITTER.DAT"];
    let install = databases
        .iter()
        .all(|name| entry_any_case(folder, name, Path::is_file).is_some());
    let (base, mounted): (Arc<dyn Vfs>, String) = if install {
        let (vfs, format) = mount_install(folder, data_root(folder)?)?;
        (vfs, format!("an install's {format} databases"))
    } else {
        let loose = LocalDirProvider::mount(folder)?;
        (Arc::new(loose), "a mod's loose files".to_owned())
    };
    let database = |path: &Path| {
        let name = path.file_name().and_then(|name| name.to_str());
        install
            && name.is_some_and(|name| {
                databases
                    .iter()
                    .chain(&["patch000.dat"])
                    .any(|database| database.eq_ignore_ascii_case(name))
            })
    };
    let mut sources: Vec<PathBuf> = entries(folder)
        .into_iter()
        .filter(|path| is_dat(path) && !database(path))
        .collect();
    if let Some(mods) = entry_any_case(folder, "mods", Path::is_dir) {
        let held = entries(&mods).into_iter();
        sources.extend(held.filter(|path| is_dat(path) || (install && path.is_dir())));
    }
    sources.extend_from_slice(layers);
    let mut mounted_layers = Vec::new();
    let mut refused = 0_u64;
    for source in &sources {
        let name = source.file_name().map_or_else(
            || source.display().to_string(),
            |name| name.to_string_lossy().into_owned(),
        );
        match mod_layer(source) {
            Ok(layer) => {
                println!("Sfall gap layer: {name}");
                mounted_layers.push(layer);
            }
            Err(error) => {
                refused += 1;
                println!(
                    "Sfall gap layer refused: {name}: {}",
                    kind(&error.to_string())
                );
            }
        }
    }
    println!(
        "Sfall gap mount: {mounted}, {} layers, {refused} refused",
        mounted_layers.len()
    );
    if mounted_layers.is_empty() {
        return Ok(base);
    }
    Ok(Arc::new(LayeredVfs::new(base, mounted_layers)))
}

/// The gap report: never a failed run. Only counts, word numbers, script and
/// function names and hook IDs are printed. A string that is not shaped like a
/// function name is never printed.
pub fn gap_report(folder: &Path, layers: &[PathBuf]) -> Result<(), Box<dyn Error>> {
    let vfs = gap_mount(folder, layers)?;
    let gap = scan(&*vfs);
    println!(
        "Sfall gap scripts: {} decoded, {} failed",
        gap.scanned,
        gap.failed.len()
    );
    for (script, error) in &gap.failed {
        println!("Sfall gap failed script: {script}: {error}");
    }
    print_table("words", "word", &gap.words, |key| format!("{:#06x}", key.0));
    print_table("hooks", "hook", &gap.hooks, |key| {
        if key.2 == UNRESOLVED {
            "?".to_owned()
        } else {
            key.0.to_string()
        }
    });
    let statuses = |rows: &BTreeMap<RowKey, Usage>, status: &str| {
        rows.keys().filter(|key| key.2 == status).count()
    };
    let lacking: BTreeSet<&String> = gap
        .words
        .iter()
        .chain(&gap.hooks)
        .filter(|(key, _)| key.2 != IMPLEMENTED)
        .flat_map(|(_, usage)| &usage.scripts)
        .collect();
    println!(
        "Sfall gap summary: words {} P1, {} P2 stubbed, {} tier X, {} unimplemented, {} \
         unresolved; hooks {} P1, {} P2 stubbed, {} unimplemented, {} unresolved; {} scripts \
         use a row that is not P1",
        statuses(&gap.words, IMPLEMENTED),
        statuses(&gap.words, STUBBED),
        statuses(&gap.words, TIER_X),
        statuses(&gap.words, UNIMPLEMENTED),
        statuses(&gap.words, UNRESOLVED),
        statuses(&gap.hooks, IMPLEMENTED),
        statuses(&gap.hooks, STUBBED),
        statuses(&gap.hooks, UNIMPLEMENTED),
        statuses(&gap.hooks, UNRESOLVED),
        lacking.len()
    );
    println!(
        "Sfall gap scope: a linear scan of what the files hold, not what a run reaches. Selectors \
         are read only from constants in straight-line code; code a script guards or never \
         calls still counts, and a word's meaning is sfall mode's."
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::kind;

    #[test]
    fn kinds_hide_quoted_text_and_numbers() {
        assert_eq!(
            kind(r#"type_04: item "(2)41 \"x\" 7" dropped"#),
            "type_N: item \"…\" dropped"
        );
        assert_eq!(
            kind("0x8109 (unnamed): reached with operands 12, 13, 0x00ff00ff"),
            "0x8109 (unnamed): reached with operands N, N, 0xN"
        );
    }
}
