//! The Fallout 2 install report (`--report-fallout2-install`): over the
//! SPEC-018 §1 mount of an install folder, the §2.2 required members, the
//! `Fallout2Tables::load` result with its diagnostics grouped by kind, the
//! per-extension parse counts under the `fallout2` profile (SPEC-016 maps
//! and prototypes, SPEC-017's word set for `.int`), the map and prototype
//! writers' round trips, and a headless new-game smoke run: SPEC-018 §6's
//! hand-off onto `artemple` with its scripts running for a bounded number
//! of world ticks, with the VM errors by script, procedure and word and the
//! diagnostics by kind.
//!
//! A report, not an audit category: it never fails the run. Only counts,
//! file and procedure names, PIDs, word numbers, line numbers and lengths
//! are printed. Diagnostic text is grouped by kind: anything quoted becomes
//! `"…"`, and numbers become `N` (a four-digit `0x` word is kept).
use std::{
    collections::BTreeMap,
    error::Error,
    path::Path,
    sync::{Arc, Mutex},
};

use vault_core::{
    CharacterSheet, GameProfile, StatContext, VmObserver, WorldEffect, WorldHost, WorldState,
    assets::{VfsWorldAssets, WorldAssets},
    fallout2::{Fallout2Tables, START_MAP},
    movie::MoviePolicy,
};
use vault_formats::{
    LocalDirProvider, LocalPrototypeCatalog, MAX_ASSET_BYTES, MapFile, MsgCatalog, Prototype, Vfs,
    VfsError,
    fallout2::{Fallout2Install, REQUIRED_MEMBERS, missing_members},
    validate_asset,
};
use vault_vm::{IntProgram, RunOutcome, Vm, VmError, WordSet};

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

/// SPEC-018 §1: the install's databases, with its loose `data` folder as
/// `data_root` when it has one.
fn mount(folder: &Path) -> Result<Arc<dyn Vfs>, Box<dyn Error>> {
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
    let data = folder.join("data");
    let loose = if data.is_dir() {
        Some(Arc::new(LocalDirProvider::mount(&data)?))
    } else {
        None
    };
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
    Ok(Arc::new(provider))
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

/// Every member of a `PARSED` extension under the `fallout2` profile, with
/// the map and prototype writers' round trips.
fn parse_all(vfs: &dyn Vfs) -> u64 {
    let catalog = LocalPrototypeCatalog::for_game(vfs, GameProfile::Fallout2);
    let mut parsed: BTreeMap<&str, Parsed> = BTreeMap::new();
    let mut failures = Kinds::default();
    let mut map_notes = Kinds::default();
    let mut stops_early = 0_u64;
    let mut short_critters = Vec::new();
    let [mut map_writes, mut pro_writes] = [[0_u64; 2]; 2];
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
                    "int" => IntProgram::parse_in(&bytes, &WordSet::FALLOUT2, false)
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
fn smoke(vfs: Arc<dyn Vfs>, tables: Arc<Fallout2Tables>) -> Result<(), Box<dyn Error>> {
    let assets = VfsWorldAssets::new(vfs, "english")?;
    assets.set_game_profile(GameProfile::Fallout2);
    // SPEC-014 §3.3's first premade, as Play's `t` takes it.
    let sheet = match assets.gcd("premade/combat.gcd") {
        Ok(gcd) => CharacterSheet::from_gcd(&gcd, StatContext::default()),
        Err(_) => {
            println!("Fallout 2 smoke run: premade/combat.gcd did not load; a default sheet");
            CharacterSheet::default()
        }
    };
    let mut host = WorldHost::new(WorldState::new(sheet, 1)?, assets);
    host.begin_fallout2_game(tables);
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
    let loaded = host.load_map(START_MAP);
    drain(&mut host, &mut handoff);
    if let Err(error) = loaded {
        println!(
            "Fallout 2 smoke run: {START_MAP} did not load: {}",
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
pub fn report(folder: &Path) -> Result<(), Box<dyn Error>> {
    let vfs = mount(folder)?;
    let tables = tables(&*vfs);
    let failed = parse_all(&*vfs);
    let loaded = tables.is_some();
    match tables {
        Some(tables) => smoke(vfs, tables)?,
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
