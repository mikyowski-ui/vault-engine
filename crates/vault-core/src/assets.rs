use crate::{
    WorldError,
    dialogue::{HeadDefinition, Mood, selectable_fidgets},
};
use std::{
    cell::{OnceCell, RefCell},
    collections::{BTreeMap, BTreeSet},
    sync::Arc,
};
use vault_formats::mod_manifest::ModRegistry;
use vault_formats::{
    ArtCatalog, Fid, Font, FontKind, FontSet, FrmArchive, GamFile, GamSection, GcdFile,
    LocalPrototypeCatalog, LstKind, LstTable, MAX_ASSET_BYTES, MapFile, MsgCatalog, Prototype, Vfs,
    VfsPath, pro::Category,
};
use vault_vm::{IntProgram, WordSet, stack_check};

/// Resource policy is explicit. Numeric perk IDs/XP curves and other tables not
/// specified by SPEC-005/006 belong to WorldRules, not inferred bytecode constants.
pub trait WorldAssets {
    fn prototype(&self, pid: u32) -> Result<Arc<Prototype>, WorldError>;
    fn program(&self, index: i32) -> Result<(String, IntProgram), WorldError>;
    /// SPEC-004 launch operand: a program filename, resolved below `scripts/`.
    fn program_by_name(&self, _name: &[u8]) -> Result<(String, IntProgram), WorldError> {
        Err(WorldError::Invalid(
            "program launch resources unavailable".into(),
        ))
    }
    fn local_count(&self, _index: i32) -> usize {
        0
    }
    fn message(&self, list: i32, id: i32) -> Option<vault_formats::msg::Message>;
    fn prototype_text(&self, _prototype: &Prototype, _description: bool) -> Option<Vec<u8>> {
        None
    }
    fn head(&self, _index: i32) -> Result<HeadDefinition, WorldError> {
        Err(WorldError::Invalid("head resource unavailable".into()))
    }
    fn frame(&self, _path: &str) -> Result<Arc<FrmArchive>, WorldError> {
        Err(WorldError::Invalid("frame resource unavailable".into()))
    }
    fn background_path(&self, _index: i32) -> Result<String, WorldError> {
        Err(WorldError::Invalid(
            "background resource unavailable".into(),
        ))
    }
    fn interface_path(&self, _index: i32) -> Result<String, WorldError> {
        Err(WorldError::Invalid("interface resource unavailable".into()))
    }
    fn font(&self) -> Result<Arc<Font>, WorldError> {
        Err(WorldError::Invalid("font resource unavailable".into()))
    }
    /// A palette/color-table resource requested by `loadpalettetable`.
    fn palette(&self, _name: &[u8]) -> Result<vault_formats::Palette, WorldError> {
        Err(WorldError::Invalid("palette resource unavailable".into()))
    }
    /// A loaded font by SPEC-002 selection number (FON 0-9, AAF 100-110).
    fn font_number(&self, _number: i32) -> Option<Arc<Font>> {
        None
    }
    fn gam(&self, _path: &str, _section: GamSection) -> Result<Option<GamFile>, WorldError> {
        Ok(None)
    }
    /// Load problems noticed since the last call that did not stop a load
    /// (a message file that stopped early, SPEC-002 §3.5), for the world's
    /// diagnostics.
    fn take_diagnostics(&self) -> Vec<String> {
        Vec::new()
    }
    fn map(&self, _path: &str) -> Result<MapFile, WorldError> {
        Err(WorldError::Invalid("map resource unavailable".into()))
    }
    /// SPEC-008 §3.10 / SPEC-005 0x80E4: the map number a `load_map` name
    /// matches in `map.msg` 0-65, or none (no transition) when unlisted.
    fn map_index(&self, _name: &[u8]) -> Option<i32> {
        None
    }
    /// The map file `map.msg` names for `number` (SPEC-013 §3.22 loads
    /// transitions by number; SPEC-014 §3.35 takes the name from `map.msg`).
    fn map_path(&self, _number: i32) -> Option<String> {
        None
    }
    fn available(&self, _path: &[u8]) -> bool {
        true
    }
    /// The `fallout.cfg` `[system]` language directory below `text/`.
    fn language(&self) -> &str {
        "english"
    }
    fn files(&self, _pattern: &[u8]) -> Vec<Vec<u8>> {
        Vec::new()
    }
    fn dialogue_label(&self, done: bool) -> Vec<u8> {
        if done {
            b"Done".to_vec()
        } else {
            b"Continue".to_vec()
        }
    }
    fn release_dialogue_art(&self) {}
    /// The unaliased art-list token at an art index (SPEC-010 effect stems).
    fn art_token(&self, _category: u8, _index: u16) -> Option<Vec<u8>> {
        None
    }
    /// The last art-list row whose base name is `name`, without case
    /// (SPEC-005 §3.8 jumpsuit indexes).
    fn art_row(&self, _category: u8, _name: &[u8]) -> Option<u16> {
        None
    }
    /// The `critters.lst` alias of a critter art index (SPEC-007 §2.1),
    /// which picks the location names (SPEC-014 §3.38, SPEC-007 §3.12).
    fn critter_alias(&self, _index: u16) -> Option<i32> {
        None
    }
    /// Load the art selected by a complete SPEC-007 identifier.
    fn object_art(&self, _fid: u32) -> Result<Arc<FrmArchive>, WorldError> {
        Err(WorldError::Invalid("object art unavailable".into()))
    }
    /// SPEC-007 constructor. Without art resources every existence probe
    /// fails, which encodes selector 0.
    fn construct_fid(
        &self,
        category: u8,
        base: u16,
        animation: u8,
        weapon: u8,
        _selector: u8,
    ) -> u32 {
        Fid::new(category, base, animation, weapon, 0).0
    }
    /// The SPEC-007 existence test for the file an identifier selects.
    fn art_exists(&self, fid: u32) -> bool {
        self.object_art(fid).is_ok()
    }
    /// A raw resource by logical path (for example `data/ai.txt`).
    fn resource(&self, path: &str) -> Result<Vec<u8>, WorldError> {
        Err(WorldError::Invalid(format!("{path}: resource unavailable")))
    }
    /// How many mod layers sit above the base data (see
    /// `vault_formats::vfs::LayeredVfs`); 0 for plain game data.
    fn mod_layers(&self) -> usize {
        0
    }
    /// The mod layers' registered campaign-table patches, lowest layer
    /// first, as (table name, VFS path); none for plain game data.
    fn table_patches(&self) -> Vec<(String, String)> {
        Vec::new()
    }
    /// Parse scripts with the versioned enhanced opcode range accepted
    /// (`IntProgram::parse_enhanced`); the Enhanced preset turns it on.
    fn set_enhanced_scripts(&self, _enabled: bool) {}
    /// Whether scripts are parsed and checked under sfall mode's word set
    /// (SPEC-024b §3.2).
    fn set_sfall_scripts(&self, _enabled: bool) {}
    /// `prototype` takes the place of its PID's until that reloads
    /// (SPEC-024b §4.8 `set_proto_data`).
    fn replace_prototype(&self, _prototype: Arc<Prototype>) {}
    /// The game whose map and prototype layouts are read (SPEC-016).
    fn game_profile(&self) -> crate::GameProfile {
        crate::GameProfile::Fallout1
    }
    /// Read `game`'s layouts from now on.
    fn set_game_profile(&self, _game: crate::GameProfile) {}
    /// A localized game message catalog (`text/<language>/game/<file>`).
    fn game_messages(&self, file: &str) -> Result<Arc<MsgCatalog>, WorldError> {
        Err(WorldError::Invalid(format!("{file}: messages unavailable")))
    }
    /// The whole per-script catalog behind `message(list, _)`, freshly
    /// loaded for the SPEC-002 §3.7 language filter; `None` when it is
    /// missing or the assets offer only single messages.
    fn dialogue_catalog(&self, _list: i32) -> Option<MsgCatalog> {
        None
    }
}
/// Decoded frames kept at once; past it the frame cache starts over (the
/// frames callers hold stay valid).
const FRAME_CACHE_BYTES: usize = 256 * 1024 * 1024;
/// The frame cache's cost of one archive: its stored frame area plus the
/// header, the one measure both the budget and `release_dialogue_art` use.
fn frame_cost(frame: &FrmArchive) -> usize {
    frame.stored_frame_area_len() as usize + 62
}
/// Failures are cached so a missing file is probed once, but a script or a
/// save can name unbounded distinct missing identifiers: past this many
/// cached failures in one cache, its failures are dropped (safe bound).
const FAILURE_CACHE_LIMIT: usize = 1024;
fn bound_failures<K: Ord, V, E>(cache: &mut BTreeMap<K, Result<V, E>>) {
    if cache.values().filter(|entry| entry.is_err()).count() >= FAILURE_CACHE_LIMIT {
        cache.retain(|_, entry| entry.is_ok());
    }
}
fn bound_missing<K: Ord, V>(cache: &mut BTreeMap<K, Option<V>>) {
    if cache.values().filter(|entry| entry.is_none()).count() >= FAILURE_CACHE_LIMIT {
        cache.retain(|_, entry| entry.is_some());
    }
}

/// Resources over a VFS. Every decoded resource (prototypes, lists, message
/// catalogs, frames and palettes) is cached by identifier or path, failures
/// included, so each file is read and decoded at most once per session
/// (frames again only after the cache budget resets).
pub struct VfsWorldAssets {
    pub vfs: Arc<dyn Vfs>,
    pub language: String,
    prototypes: RefCell<BTreeMap<u32, Result<Arc<Prototype>, WorldError>>>,
    messages: RefCell<BTreeMap<String, Option<MsgCatalog>>>,
    game_catalogs: RefCell<BTreeMap<String, Result<Arc<MsgCatalog>, WorldError>>>,
    programs: RefCell<BTreeMap<i32, (String, IntProgram)>>,
    lists: RefCell<BTreeMap<String, Result<Arc<LstTable>, WorldError>>>,
    frames: RefCell<BTreeMap<String, Result<Arc<FrmArchive>, WorldError>>>,
    frame_bytes: RefCell<usize>,
    palettes: RefCell<BTreeMap<String, Result<vault_formats::Palette, WorldError>>>,
    /// The mounted file names, collected on the first existence test.
    files: OnceCell<BTreeSet<VfsPath>>,
    art: RefCell<Option<Arc<ArtCatalog>>>,
    fonts: RefCell<Option<Arc<FontSet>>>,
    /// Load problems not yet taken (`take_diagnostics`), at most
    /// `FAILURE_CACHE_LIMIT`.
    diagnostics: RefCell<Vec<String>>,
    /// Scripts parse with the enhanced opcode range accepted (the
    /// Enhanced preset); off, scripts parse under plain SPEC-004.
    enhanced_scripts: std::cell::Cell<bool>,
    /// SPEC-024b §3.2: sfall mode's word set, once the session turned it on.
    sfall_scripts: std::cell::Cell<bool>,
    /// What the mod layers' `vault-mod.toml` manifests register (empty
    /// without mod layers).
    registry: ModRegistry,
    /// The game whose map and prototype layouts are read (SPEC-016).
    game: std::cell::Cell<crate::GameProfile>,
    /// The on-load verdicts of the scripts mod layers supplied, by
    /// normalized path: the checked program, or `None` for a refusal.
    /// Retail scripts are never entered.
    mod_scripts: RefCell<BTreeMap<String, Option<IntProgram>>>,
}
impl VfsWorldAssets {
    pub fn new(vfs: Arc<dyn Vfs>, language: &str) -> Result<Self, WorldError> {
        if language.is_empty()
            || !language
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b == b'_')
        {
            return Err(WorldError::Invalid(
                "language must be a resource directory name".into(),
            ));
        }
        Self {
            vfs,
            language: language.to_ascii_lowercase(),
            prototypes: RefCell::new(BTreeMap::new()),
            messages: RefCell::new(BTreeMap::new()),
            game_catalogs: RefCell::new(BTreeMap::new()),
            programs: RefCell::new(BTreeMap::new()),
            lists: RefCell::new(BTreeMap::new()),
            frames: RefCell::new(BTreeMap::new()),
            frame_bytes: RefCell::new(0),
            palettes: RefCell::new(BTreeMap::new()),
            files: OnceCell::new(),
            art: RefCell::new(None),
            fonts: RefCell::new(None),
            diagnostics: RefCell::new(Vec::new()),
            enhanced_scripts: std::cell::Cell::new(false),
            sfall_scripts: std::cell::Cell::new(false),
            registry: ModRegistry::default(),
            game: std::cell::Cell::new(crate::GameProfile::Fallout1),
            mod_scripts: RefCell::new(BTreeMap::new()),
        }
        .with_registry()
    }
    /// The mod layers' registrations
    /// ([`ModRegistry::from_vfs`](vault_formats::mod_manifest::ModRegistry::from_vfs)).
    /// A malformed manifest, a conflict, or a script row the base
    /// `scripts.lst` already has is an error.
    fn with_registry(mut self) -> Result<Self, WorldError> {
        let registry = ModRegistry::from_vfs(self.vfs.as_ref())
            .map_err(|error| WorldError::Invalid(format!("mod registration: {error}")))?;
        if !registry.scripts.is_empty()
            && let Ok(list) = self.list("scripts/scripts.lst", LstKind::Script)
            && let Some(index) = registry
                .scripts
                .keys()
                .find(|index| list.script_id(**index).is_some())
        {
            return Err(WorldError::Invalid(format!(
                "mod registration: script index {index} is a scripts.lst row"
            )));
        }
        self.registry = registry;
        Ok(self)
    }
    /// What the mod layers register.
    pub fn registry(&self) -> &ModRegistry {
        &self.registry
    }
    /// SPEC-002 §3.5: a message file that stops early keeps the records
    /// before the failure, which still load; the failure is reported as a
    /// diagnostic.
    fn note_message_failure(&self, path: &str, catalog: &MsgCatalog) {
        if let Some(failure) = catalog.failure() {
            let mut diagnostics = self.diagnostics.borrow_mut();
            if diagnostics.len() < FAILURE_CACHE_LIMIT {
                diagnostics.push(format!(
                    "{path}: message file stopped early ({} records kept): {failure}",
                    catalog.entries().len()
                ));
            }
        }
    }
    /// Category art lists, parsed once on first use.
    pub fn art_catalog(&self) -> Result<Arc<ArtCatalog>, WorldError> {
        if let Some(catalog) = self.art.borrow().as_ref() {
            return Ok(catalog.clone());
        }
        let catalog = Arc::new(ArtCatalog::load(&*self.vfs)?);
        *self.art.borrow_mut() = Some(catalog.clone());
        Ok(catalog)
    }
    /// SPEC-002 startup font search, performed once on first use.
    pub fn font_set(&self) -> Arc<FontSet> {
        self.fonts
            .borrow_mut()
            .get_or_insert_with(|| Arc::new(FontSet::load(&*self.vfs)))
            .clone()
    }
    fn exists(&self, path: &str) -> bool {
        let files = self
            .files
            .get_or_init(|| self.vfs.files().cloned().collect());
        VfsPath::new(path).is_ok_and(|path| files.contains(&path))
    }
    pub fn read(&self, path: &str) -> Result<Vec<u8>, WorldError> {
        self.vfs
            .read_bounded(path, MAX_ASSET_BYTES)
            .map_err(|error| WorldError::Invalid(format!("{path}: {error}")))
    }
    /// The word set scripts load under: the profile's, or sfall mode's.
    fn script_words(&self) -> WordSet {
        if self.sfall_scripts.get() {
            crate::fallout2::sfall_word_set(self.game.get())
        } else {
            crate::fallout2::word_set(self.game.get())
        }
    }
    /// A script, checked against the game profile's word set (SPEC-017
    /// §2.1), with the enhanced range as the preset says.
    fn parse_program(&self, bytes: &[u8]) -> Result<IntProgram, WorldError> {
        let words = self.script_words();
        Ok(IntProgram::parse_in(
            bytes,
            &words,
            self.enhanced_scripts.get(),
        )?)
    }
    /// The script at `path`. One a mod layer supplies is checked before it
    /// enters the VM ([`Self::check_mod_script`]) and its verdict kept for
    /// the session; a refused one is [`WorldError::ScriptRefused`]. Retail
    /// scripts, from the base archives or folder, are parsed unchecked
    /// (the offline audit covers them).
    fn load_program(&self, path: &str) -> Result<IntProgram, WorldError> {
        let key = VfsPath::new(path)
            .map(|normalized| normalized.as_str().to_owned())
            .unwrap_or_else(|_| path.to_owned());
        let refused = || WorldError::ScriptRefused(path.to_owned());
        if let Some(verdict) = self.mod_scripts.borrow().get(&key) {
            return verdict.clone().ok_or_else(refused);
        }
        let (bytes, layer) = self
            .vfs
            .read_with_layer(path, MAX_ASSET_BYTES)
            .map_err(|error| WorldError::Invalid(format!("{path}: {error}")))?;
        let Some(layer) = layer else {
            return self.parse_program(&bytes);
        };
        let verdict = match self.check_mod_script(path, &layer, &bytes) {
            Ok(program) => Some(program),
            Err(diagnostic) => {
                let mut diagnostics = self.diagnostics.borrow_mut();
                if diagnostics.len() < FAILURE_CACHE_LIMIT {
                    diagnostics.push(diagnostic);
                }
                None
            }
        };
        self.mod_scripts.borrow_mut().insert(key, verdict.clone());
        verdict.ok_or_else(refused)
    }
    /// The on-load check of a script mod layer `layer` supplies: decoded by
    /// SPEC-004's layout with every bit-15 word accepted, run through the
    /// SPEC-017 §8 stack model with the active profile's table (the
    /// enhanced range included when scripts parse enhanced, and the
    /// table's recorded exceptions), then parsed under the profile's word
    /// set as [`Self::parse_program`] does. A finding, or a refusal by
    /// either decode, refuses it with the one diagnostic returned: the
    /// path, the layer, and the findings by kind and word (never script
    /// bytes).
    fn check_mod_script(
        &self,
        path: &str,
        layer: &str,
        bytes: &[u8],
    ) -> Result<IntProgram, String> {
        let words = self.script_words();
        let enhanced = self.enhanced_scripts.get();
        let table = stack_check::Table::with_enhanced(words.clone(), enhanced);
        let profile = match self.game.get() {
            crate::GameProfile::Fallout1 => "fallout1",
            crate::GameProfile::Fallout2 => "fallout2",
        };
        let every = WordSet {
            words: 0x8000..=0xffff,
        };
        let refusal = match IntProgram::parse_in(bytes, &every, false) {
            Err(error) => format!("undecodable: {error}"),
            Ok(decoded) => {
                let name = path.rsplit('/').next().unwrap_or(path);
                let findings =
                    stack_check::check(name, &decoded, &table, stack_check::exceptions(&words));
                if findings.is_empty() {
                    match IntProgram::parse_in(bytes, &words, enhanced) {
                        Ok(program) => return Ok(program),
                        Err(error) => format!("refused by the word set: {error}"),
                    }
                } else {
                    let total = findings.values().sum::<u64>();
                    let plural = if total == 1 { "" } else { "s" };
                    format!(
                        "{total} finding{plural}: {}",
                        stack_check::summary(&findings)
                    )
                }
            }
        };
        Err(format!(
            "{path}: mod script from layer {layer:?} refused at load ({profile}{}{} table): \
             {refusal}",
            if words.sfall() { " + sfall" } else { "" },
            if enhanced { " + enhanced" } else { "" }
        ))
    }
    /// A list file, parsed once (each path has one kind).
    fn list(&self, path: &str, kind: LstKind) -> Result<Arc<LstTable>, WorldError> {
        if let Some(list) = self.lists.borrow().get(path) {
            return list.clone();
        }
        let list = self
            .read(path)
            .and_then(|bytes| Ok(Arc::new(LstTable::parse(&bytes, kind)?)));
        let mut lists = self.lists.borrow_mut();
        if list.is_err() {
            bound_failures(&mut lists);
        }
        lists.insert(path.into(), list.clone());
        list
    }
    fn art_name(&self, folder: &str, index: i32) -> Result<String, WorldError> {
        if !(0..4096).contains(&index) {
            return Err(WorldError::Invalid(
                "art index outside supported range".into(),
            ));
        }
        let list = self.list(&format!("art/{folder}/{folder}.lst"), LstKind::Art)?;
        let name = list
            .art_id(index as u32)
            .and_then(|entry| entry.name.as_deref())
            .filter(|name| !name.is_empty())
            .ok_or_else(|| WorldError::Invalid(format!("missing {folder} art index {index}")))?;
        Ok(format!(
            "art/{folder}/{}",
            std::str::from_utf8(name)
                .map_err(|_| WorldError::Invalid("invalid art filename".into()))?
        ))
    }
    fn catalog(&self, path: &str, id: i32) -> Option<vault_formats::msg::Message> {
        if !self.messages.borrow().contains_key(path) {
            let catalog = self
                .read(path)
                .ok()
                .and_then(|bytes| MsgCatalog::parse(&bytes).ok());
            if let Some(catalog) = &catalog {
                self.note_message_failure(path, catalog);
            }
            let mut messages = self.messages.borrow_mut();
            if catalog.is_none() {
                bound_missing(&mut messages);
            }
            messages.insert(path.into(), catalog);
        }
        self.messages.borrow().get(path)?.as_ref()?.get(id).cloned()
    }
    /// The per-script catalog `text/<language>/dialog/<script>.msg` of
    /// one-based message list `list` (a `scripts.lst` row).
    fn dialogue_path(&self, list: i32) -> Option<String> {
        if list <= 0 {
            return None;
        }
        let scripts = self.list("scripts/scripts.lst", LstKind::Script).ok()?;
        let name = scripts.script_id((list - 1) as u32)?.name.as_deref()?;
        let stem = std::str::from_utf8(name).ok()?.strip_suffix(".int")?;
        Some(format!("text/{}/dialog/{stem}.msg", self.language))
    }
    pub fn gcd(&self, path: &str) -> Result<GcdFile, WorldError> {
        Ok(GcdFile::parse(&self.read(path)?)?)
    }
    /// SPEC-014 §3.42 info-card pictures: `art/skilldex/` art by list index.
    pub fn skilldex_path(&self, index: i32) -> Result<String, WorldError> {
        self.art_name("skilldex", index)
    }
    fn load_prototype(&self, pid: u32) -> Result<Arc<Prototype>, WorldError> {
        let category = Category::from_pid(pid)?;
        let folder = category.directory();
        let list = self.list(&format!("proto/{folder}/{folder}.lst"), LstKind::Prototype)?;
        let name = list
            .prototype_id(pid)
            .and_then(|entry| entry.name.as_deref())
            .filter(|name| !name.is_empty())
            .ok_or_else(|| WorldError::Invalid(format!("prototype {pid:#x} has no list entry")))?;
        let path = format!(
            "proto/{folder}/{}",
            std::str::from_utf8(name)
                .map_err(|_| WorldError::Invalid("invalid prototype filename".into()))?
        );
        let prototype = Arc::new(Prototype::parse_for(&self.read(&path)?, self.game.get())?);
        for line in &prototype.diagnostics {
            let mut diagnostics = self.diagnostics.borrow_mut();
            if diagnostics.len() < FAILURE_CACHE_LIMIT {
                diagnostics.push(format!("{path}: {line}"));
            }
        }
        // SPEC-003 §3.1 safe rule: the stored PID must be the requested one.
        if prototype.pid != pid {
            return Err(WorldError::Invalid(format!(
                "{path} stores prototype {:#x}, not {pid:#x}",
                prototype.pid
            )));
        }
        Ok(prototype)
    }
}
impl WorldAssets for VfsWorldAssets {
    fn take_diagnostics(&self) -> Vec<String> {
        std::mem::take(&mut *self.diagnostics.borrow_mut())
    }
    fn replace_prototype(&self, prototype: Arc<Prototype>) {
        self.prototypes
            .borrow_mut()
            .insert(prototype.pid, Ok(prototype));
    }
    fn set_sfall_scripts(&self, enabled: bool) {
        self.sfall_scripts.set(enabled);
        self.programs.borrow_mut().clear();
    }
    fn prototype(&self, pid: u32) -> Result<Arc<Prototype>, WorldError> {
        if let Some(prototype) = self.prototypes.borrow().get(&pid) {
            return prototype.clone();
        }
        let prototype = self.load_prototype(pid);
        let mut prototypes = self.prototypes.borrow_mut();
        if prototype.is_err() {
            bound_failures(&mut prototypes);
        }
        prototypes.insert(pid, prototype.clone());
        prototype
    }
    fn program(&self, index: i32) -> Result<(String, IntProgram), WorldError> {
        if let Some(program) = self.programs.borrow().get(&index) {
            return Ok(program.clone());
        }
        if index < 0 {
            return Err(WorldError::Invalid("negative script-file index".into()));
        }
        let list = self.list("scripts/scripts.lst", LstKind::Script)?;
        // A row past the list may be a mod's registered script.
        let registered = self
            .registry
            .scripts
            .get(&(index as u32))
            .map(|script| script.file.as_bytes());
        let name = list
            .script_id(index as u32)
            .and_then(|entry| entry.name.as_deref())
            .or(registered)
            .ok_or_else(|| WorldError::Invalid(format!("script index {index} missing")))?;
        let path = format!(
            "scripts/{}",
            std::str::from_utf8(name)
                .map_err(|_| WorldError::Invalid("invalid script name".into()))?
        );
        let program = self.load_program(&path)?;
        self.programs
            .borrow_mut()
            .insert(index, (path.clone(), program.clone()));
        Ok((path, program))
    }
    fn program_by_name(&self, name: &[u8]) -> Result<(String, IntProgram), WorldError> {
        let name = std::str::from_utf8(name)
            .ok()
            .filter(|name| name.is_ascii())
            .ok_or_else(|| WorldError::Invalid("program filename is not ASCII".into()))?;
        let path = VfsPath::new(name)
            .map_err(|error| WorldError::Invalid(format!("program filename: {error}")))?;
        let path = if path.as_str().starts_with("scripts/") {
            path.to_string()
        } else {
            format!("scripts/{path}")
        };
        let program = self.load_program(&path)?;
        Ok((path, program))
    }
    fn local_count(&self, index: i32) -> usize {
        if index < 0 {
            return 0;
        }
        self.list("scripts/scripts.lst", LstKind::Script)
            .ok()
            .and_then(|list| {
                list.script_id(index as u32)
                    .and_then(|entry| entry.local_vars)
            })
            .filter(|count| *count >= 0 && *count <= 1_000_000)
            .map(|count| count as usize)
            .or_else(|| {
                self.registry
                    .scripts
                    .get(&(index as u32))
                    .map(|script| script.local_vars as usize)
            })
            .unwrap_or(0)
    }
    fn message(&self, list: i32, id: i32) -> Option<vault_formats::msg::Message> {
        self.catalog(&self.dialogue_path(list)?, id)
    }
    fn dialogue_catalog(&self, list: i32) -> Option<MsgCatalog> {
        let path = self.dialogue_path(list)?;
        self.catalog(&path, 0);
        self.messages.borrow().get(&path)?.clone()
    }
    fn prototype_text(&self, prototype: &Prototype, description: bool) -> Option<Vec<u8>> {
        if prototype.message_base == -1 {
            return None;
        }
        let stem = match prototype.category {
            Category::Item => "item",
            Category::Critter => "crit",
            Category::Scenery => "scen",
            Category::Wall => "wall",
            Category::Tile => "tile",
            Category::Misc => "misc",
        };
        self.catalog(
            &format!("text/{}/game/pro_{stem}.msg", self.language),
            prototype.message_base.checked_add(i32::from(description))?,
        )
        .map(|message| message.text)
    }
    fn head(&self, index: i32) -> Result<HeadDefinition, WorldError> {
        if !(0..4096).contains(&index) {
            return Err(WorldError::Invalid("invalid head index".into()));
        }
        let list = self.list("art/heads/heads.lst", LstKind::Art)?;
        let entry = list
            .art_id(index as u32)
            .ok_or_else(|| WorldError::Invalid("missing head".into()))?;
        let stem = std::str::from_utf8(entry.name.as_deref().unwrap_or(b""))
            .map_err(|_| WorldError::Invalid("invalid head stem".into()))?;
        let first = entry
            .head_counts
            .ok_or_else(|| WorldError::Invalid("head has no fidget counts".into()))?;
        let mut head = HeadDefinition {
            index: index as u16,
            stem: stem.into(),
            counts: first,
        };
        // SPEC-006 §3.11 safe rule: each mood reads its own column when
        // every fidget file it can select exists; otherwise it keeps the
        // reference's first-column count.
        let columns = entry.head_columns.unwrap_or(first);
        for (slot, mood) in [Mood::Good, Mood::Neutral, Mood::Bad]
            .into_iter()
            .enumerate()
        {
            let variants = selectable_fidgets(columns[slot]);
            if !variants.is_empty()
                && variants.iter().all(|variant| {
                    head.path(mood.fidget(), *variant)
                        .is_ok_and(|path| self.exists(&path))
                })
            {
                head.counts[slot] = columns[slot];
            }
        }
        Ok(head)
    }
    fn frame(&self, path: &str) -> Result<Arc<FrmArchive>, WorldError> {
        if let Some(frame) = self.frames.borrow().get(path) {
            return frame.clone();
        }
        let loaded = self
            .read(path)
            .and_then(|bytes| Ok(Arc::new(FrmArchive::parse(&bytes)?)));
        let frame = loaded.inspect(|frame| {
            let size = frame_cost(frame);
            let mut total = self.frame_bytes.borrow_mut();
            if total.saturating_add(size) > FRAME_CACHE_BYTES {
                self.frames.borrow_mut().retain(|_, cached| cached.is_err());
                *total = 0;
            }
            *total += size;
        });
        let mut frames = self.frames.borrow_mut();
        if frame.is_err() {
            bound_failures(&mut frames);
        }
        frames.insert(path.into(), frame.clone());
        frame
    }
    fn object_art(&self, fid: u32) -> Result<Arc<FrmArchive>, WorldError> {
        let path = self
            .art_catalog()?
            .path(Fid(fid))
            .map_err(|error| WorldError::Invalid(format!("art {fid:#010x}: {error}")))?;
        self.frame(&path.to_ascii_lowercase())
    }
    fn construct_fid(
        &self,
        category: u8,
        base: u16,
        animation: u8,
        weapon: u8,
        selector: u8,
    ) -> u32 {
        match self.art_catalog() {
            Ok(catalog) => {
                catalog
                    .construct(category, base, animation, weapon, selector, |path| {
                        self.exists(path)
                    })
                    .0
            }
            Err(_) => Fid::new(category, base, animation, weapon, 0).0,
        }
    }
    fn art_token(&self, category: u8, index: u16) -> Option<Vec<u8>> {
        Some(
            self.art_catalog()
                .ok()?
                .base_token(category, index)?
                .to_vec(),
        )
    }
    fn art_row(&self, category: u8, name: &[u8]) -> Option<u16> {
        self.art_catalog().ok()?.last_row(category, name)
    }
    fn critter_alias(&self, index: u16) -> Option<i32> {
        self.art_catalog().ok()?.critter_alias(index)
    }
    fn art_exists(&self, fid: u32) -> bool {
        self.art_catalog()
            .ok()
            .and_then(|catalog| catalog.path(Fid(fid)).ok())
            .is_some_and(|path| self.exists(&path))
    }
    fn resource(&self, path: &str) -> Result<Vec<u8>, WorldError> {
        self.read(path)
    }
    fn game_messages(&self, file: &str) -> Result<Arc<MsgCatalog>, WorldError> {
        let path = format!("text/{}/game/{file}", self.language);
        if let Some(catalog) = self.game_catalogs.borrow().get(&path) {
            return catalog.clone();
        }
        let catalog = self
            .read(&path)
            .and_then(|bytes| Ok(Arc::new(MsgCatalog::parse(&bytes)?)));
        if let Ok(catalog) = &catalog {
            self.note_message_failure(&path, catalog);
        }
        let mut catalogs = self.game_catalogs.borrow_mut();
        if catalog.is_err() {
            bound_failures(&mut catalogs);
        }
        catalogs.insert(path, catalog.clone());
        catalog
    }
    fn background_path(&self, index: i32) -> Result<String, WorldError> {
        self.art_name("backgrnd", index)
    }
    fn interface_path(&self, index: i32) -> Result<String, WorldError> {
        self.art_name("intrface", index)
    }
    /// Presentation text uses the AAF subsystem's initial font when it
    /// initialized, otherwise the FON subsystem's.
    fn font(&self) -> Result<Arc<Font>, WorldError> {
        let fonts = self.font_set();
        [FontKind::Antialiased, FontKind::Bitmap]
            .into_iter()
            .find_map(|kind| fonts.default_font(kind).map(|(_, font)| font.clone()))
            .ok_or_else(|| WorldError::Invalid("no dialogue font".into()))
    }
    fn font_number(&self, number: i32) -> Option<Arc<Font>> {
        self.font_set().get(number).cloned()
    }
    fn palette(&self, name: &[u8]) -> Result<vault_formats::Palette, WorldError> {
        let name = std::str::from_utf8(name)
            .map_err(|_| WorldError::Invalid("palette filename is not ASCII".into()))?;
        if let Some(palette) = self.palettes.borrow().get(name) {
            return palette.clone();
        }
        let palette = self
            .read(name)
            .and_then(|bytes| Ok(vault_formats::Palette::parse(&bytes)?));
        let mut palettes = self.palettes.borrow_mut();
        if palette.is_err() {
            bound_failures(&mut palettes);
        }
        palettes.insert(name.into(), palette.clone());
        palette
    }
    fn gam(&self, path: &str, section: GamSection) -> Result<Option<GamFile>, WorldError> {
        match self.vfs.read_bounded(path, MAX_ASSET_BYTES) {
            Ok(bytes) => Ok(Some(GamFile::parse(&bytes, section)?)),
            Err(vault_formats::VfsError::NotFound(_)) => Ok(None),
            Err(error) => Err(WorldError::Invalid(error.to_string())),
        }
    }
    /// The requested stream only: SPEC-014 §3.35 saved copies come from the
    /// working map set the host keeps in memory, never from a stray
    /// `<stem>.sav` beside the data files.
    fn map(&self, path: &str) -> Result<MapFile, WorldError> {
        let game = self.game.get();
        Ok(MapFile::parse_for(
            &self.read(path)?,
            &LocalPrototypeCatalog::for_game(&*self.vfs, game),
            game,
        )?)
    }
    /// SPEC-008 §3.10: a name is matched against `map.msg` 0-65 (uppercased,
    /// `.SAV` read as `.MAP`); an unlisted name, or one of 16 or more bytes
    /// (safe rule), has no number.
    fn map_index(&self, name: &[u8]) -> Option<i32> {
        let catalog = self.game_messages("map.msg").ok();
        let number = crate::worldmap::load_map_number(name, catalog.as_deref());
        // SPEC-008 §3.10 first; an unmatched name may be a mod's map.
        (number >= 0)
            .then_some(number)
            .or_else(|| self.registry.map_number(name).map(|number| number as i32))
    }
    /// SPEC-014 §3.35: the file name comes from `map.msg` by map number.
    fn map_path(&self, number: i32) -> Option<String> {
        if !(0..crate::worldmap::MAP_COUNT as i32).contains(&number) {
            // A mod's registered map (numbers 66 and up).
            let map = self.registry.maps.get(&u32::try_from(number).ok()?)?;
            return Some(format!("maps/{}", map.file));
        }
        let catalog = self.game_messages("map.msg").ok()?;
        let name = std::str::from_utf8(&catalog.get(number)?.text).ok()?;
        let name = name.trim();
        (!name.is_empty() && !name.contains(['/', '\\']))
            .then(|| format!("maps/{}", name.to_ascii_lowercase()))
    }
    /// Either slash style; names compare case-insensitively.
    fn available(&self, path: &[u8]) -> bool {
        std::str::from_utf8(path).is_ok_and(|path| self.exists(path))
    }
    fn language(&self) -> &str {
        &self.language
    }
    fn mod_layers(&self) -> usize {
        self.vfs.mod_layers()
    }
    fn table_patches(&self) -> Vec<(String, String)> {
        self.registry
            .patches
            .iter()
            .map(|patch| (patch.table.clone(), patch.file.clone()))
            .collect()
    }
    fn game_profile(&self) -> crate::GameProfile {
        self.game.get()
    }
    fn set_game_profile(&self, game: crate::GameProfile) {
        if self.game.replace(game) != game {
            // Cached prototypes were read in the other game's layout, and
            // cached programs and verdicts checked against its word set.
            self.prototypes.borrow_mut().clear();
            self.programs.borrow_mut().clear();
            self.mod_scripts.borrow_mut().clear();
        }
    }
    fn set_enhanced_scripts(&self, enabled: bool) {
        if self.enhanced_scripts.replace(enabled) != enabled {
            // Cached programs and verdicts were parsed under the other
            // profile.
            self.programs.borrow_mut().clear();
            self.mod_scripts.borrow_mut().clear();
        }
    }
    fn files(&self, pattern: &[u8]) -> Vec<Vec<u8>> {
        let Ok(pattern) = std::str::from_utf8(pattern) else {
            return Vec::new();
        };
        let (prefix, suffix) = pattern.split_once('*').unwrap_or((pattern, ""));
        self.vfs
            .files()
            .filter(|path| {
                path.as_str().starts_with(&prefix.to_ascii_lowercase())
                    && path.as_str().ends_with(&suffix.to_ascii_lowercase())
            })
            .take(10000)
            .map(|path| path.as_str().as_bytes().to_vec())
            .collect()
    }
    /// Talking heads and backgrounds go at a conversation's end. Interface
    /// art stays: the interface bar and windows draw it every frame.
    fn release_dialogue_art(&self) {
        self.frames.borrow_mut().retain(|path, _| {
            !["art/heads/", "art/backgrnd/"]
                .iter()
                .any(|prefix| path.starts_with(prefix))
        });
        *self.frame_bytes.borrow_mut() = self
            .frames
            .borrow()
            .values()
            .filter_map(|frame| frame.as_ref().ok())
            .map(|frame| frame_cost(frame))
            .sum();
    }
}
