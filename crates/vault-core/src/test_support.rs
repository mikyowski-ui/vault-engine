//! Helpers shared by the unit tests: a configurable `WorldAssets` fake,
//! scripted random draws, synthetic prototypes, and the `.INT` fragments and
//! script records the script tests build on.
use crate::{
    ObjectRef, RandomSource, ScriptEvent, WorldError, WorldHost, WorldState,
    assets::{VfsWorldAssets, WorldAssets},
    combat::Attack,
    critical::hit_mode,
    fixture_state as fixture,
    world::{ScriptRequest, WorldTrace},
};
use std::{
    collections::{BTreeMap, BTreeSet, VecDeque},
    path::Path,
    sync::Arc,
};
use vault_formats::{
    Fid, FrmArchive, LocalDirProvider, MapFile, MsgCatalog, Prototype,
    map::{MapScript, ObjectState, ScriptBlock, ScriptCategory, ScriptPrefix},
    msg::Message,
    pro::ScriptFlags,
};
use vault_vm::IntProgram;

/// Which art identifiers [`TestAssets`] reports as existing.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) enum ArtPolicy {
    /// Art exists when it loads from `art` (the trait default).
    #[default]
    Loads,
    /// Every art exists but the animation codes in `missing`.
    All,
    /// Critter art exists but for the animation codes in `missing`; other
    /// art exists when it loads.
    Critters,
}

/// A `WorldAssets` fake: every resource is missing unless a field holds it.
#[derive(Clone, Default)]
pub(crate) struct TestAssets {
    /// Prototypes by PID; then by PID type (`pid >> 24`) in `by_type`; then
    /// `any_prototype`.
    pub prototypes: BTreeMap<u32, Arc<Prototype>>,
    pub by_type: BTreeMap<u32, Arc<Prototype>>,
    pub any_prototype: Option<Arc<Prototype>>,
    /// Script files by index, with their names.
    pub programs: Vec<(String, IntProgram)>,
    /// Per-script message lists, behind `message` and `dialogue_catalog`.
    pub catalogs: BTreeMap<i32, MsgCatalog>,
    /// Game message catalogs by file name.
    pub game: BTreeMap<String, Arc<MsgCatalog>>,
    /// Raw resources by logical path.
    pub resources: BTreeMap<String, Vec<u8>>,
    /// Maps by path; `any_map` answers every other path.
    pub maps: BTreeMap<String, MapFile>,
    pub any_map: Option<MapFile>,
    /// The `map.msg` file names by map number.
    pub map_paths: BTreeMap<i32, String>,
    /// Loadable art by identifier.
    pub art: BTreeMap<u32, Arc<FrmArchive>>,
    pub art_policy: ArtPolicy,
    /// Critter animation codes without art (see [`ArtPolicy`]).
    pub missing: BTreeSet<u8>,
    /// `(category, art index)` to list token.
    pub tokens: BTreeMap<(u8, u16), Vec<u8>>,
    /// Effect files present, as upper-case stems; without a set every path
    /// is available.
    pub effects: Option<BTreeSet<Vec<u8>>>,
    /// The game profile these assets report.
    pub profile: crate::GameProfile,
    /// `set_proto_data`'s replacements, over `prototypes`.
    pub replaced: Arc<std::sync::Mutex<BTreeMap<u32, Arc<Prototype>>>>,
}
impl TestAssets {
    /// Every art exists but the `missing` animations.
    pub fn all_art() -> Self {
        Self {
            art_policy: ArtPolicy::All,
            ..Self::default()
        }
    }
    /// Adds the game message catalog `file` parsed from `text`.
    pub fn with_messages(mut self, file: &str, text: &[u8]) -> Self {
        self.insert_messages(file, text);
        self
    }
    pub fn insert_messages(&mut self, file: &str, text: &[u8]) {
        let catalog = MsgCatalog::parse(text).unwrap();
        self.game.insert(file.into(), Arc::new(catalog));
    }
    /// Adds script file `name` as the next file index.
    pub fn with_program(mut self, name: &str, program: IntProgram) -> Self {
        self.programs.push((name.into(), program));
        self
    }
}
impl WorldAssets for TestAssets {
    fn game_profile(&self) -> crate::GameProfile {
        self.profile
    }
    fn replace_prototype(&self, prototype: Arc<Prototype>) {
        self.replaced
            .lock()
            .unwrap()
            .insert(prototype.pid, prototype);
    }
    fn program_by_name(&self, name: &[u8]) -> Result<(String, IntProgram), WorldError> {
        let wanted = String::from_utf8_lossy(name).to_ascii_lowercase();
        self.programs
            .iter()
            .find(|(file, _)| file.to_ascii_lowercase() == wanted)
            .cloned()
            .ok_or_else(|| WorldError::Invalid(format!("no program {wanted}")))
    }
    fn prototype(&self, pid: u32) -> Result<Arc<Prototype>, WorldError> {
        if let Some(prototype) = self.replaced.lock().unwrap().get(&pid) {
            return Ok(prototype.clone());
        }
        self.prototypes
            .get(&pid)
            .or_else(|| self.by_type.get(&(pid >> 24)))
            .or(self.any_prototype.as_ref())
            .cloned()
            .ok_or_else(|| WorldError::Invalid(format!("prototype {pid:#x} is missing")))
    }
    fn program(&self, index: i32) -> Result<(String, IntProgram), WorldError> {
        usize::try_from(index)
            .ok()
            .and_then(|index| self.programs.get(index))
            .cloned()
            .ok_or_else(|| WorldError::Invalid(format!("script {index} does not load")))
    }
    fn message(&self, list: i32, id: i32) -> Option<Message> {
        self.catalogs.get(&list)?.get(id).cloned()
    }
    fn dialogue_catalog(&self, list: i32) -> Option<MsgCatalog> {
        self.catalogs.get(&list).cloned()
    }
    fn game_messages(&self, file: &str) -> Result<Arc<MsgCatalog>, WorldError> {
        self.game
            .get(file)
            .cloned()
            .ok_or_else(|| WorldError::Invalid(format!("{file} is missing")))
    }
    fn resource(&self, path: &str) -> Result<Vec<u8>, WorldError> {
        self.resources
            .get(path)
            .cloned()
            .ok_or_else(|| WorldError::Invalid(format!("{path} is missing")))
    }
    fn map(&self, path: &str) -> Result<MapFile, WorldError> {
        self.maps
            .get(path)
            .or(self.any_map.as_ref())
            .cloned()
            .ok_or_else(|| WorldError::Invalid(format!("{path} is missing")))
    }
    fn map_path(&self, number: i32) -> Option<String> {
        self.map_paths.get(&number).cloned()
    }
    fn art_token(&self, category: u8, index: u16) -> Option<Vec<u8>> {
        self.tokens.get(&(category, index)).cloned()
    }
    fn object_art(&self, fid: u32) -> Result<Arc<FrmArchive>, WorldError> {
        self.art
            .get(&fid)
            .cloned()
            .ok_or_else(|| WorldError::Invalid("object art unavailable".into()))
    }
    fn art_exists(&self, fid: u32) -> bool {
        let unless_missing = !self.missing.contains(&Fid(fid).animation());
        match self.art_policy {
            ArtPolicy::All => unless_missing,
            ArtPolicy::Critters if Fid(fid).category() == 1 => unless_missing,
            ArtPolicy::Loads | ArtPolicy::Critters => self.art.contains_key(&fid),
        }
    }
    fn available(&self, path: &[u8]) -> bool {
        let Some(effects) = &self.effects else {
            return true;
        };
        let stem = path
            .strip_prefix(b"sound/sfx/")
            .and_then(|name| name.strip_suffix(b".ACM"));
        stem.is_some_and(|stem| effects.contains(&stem.to_ascii_uppercase()))
    }
}

/// Resources over the directory `dir`, in English.
pub(crate) fn dir_assets(dir: &Path) -> VfsWorldAssets {
    VfsWorldAssets::new(Arc::new(LocalDirProvider::mount(dir).unwrap()), "english").unwrap()
}

/// Replays scripted draws, checking each requested range; the outcomes not
/// yet drawn stay in `.0`.
pub(crate) struct Draws(pub VecDeque<i32>);
impl Draws {
    pub fn new(values: &[i32]) -> Self {
        Self(values.iter().copied().collect())
    }
}
impl RandomSource for Draws {
    fn range(&mut self, first: i32, last: i32) -> Result<i32, WorldError> {
        let value = self.0.pop_front().expect("an outcome for every draw");
        assert!(
            (first..=last).contains(&value),
            "{value} outside {first}..={last}"
        );
        Ok(value)
    }
}

/// A fixture-template prototype with `words` and `bytes` patched.
pub(crate) fn patched(
    category: u8,
    subtype: usize,
    words: &[(usize, i32)],
    bytes: &[(usize, u8)],
) -> Arc<Prototype> {
    let mut data = fixture::world::prototype(category, subtype);
    for (offset, value) in words {
        fixture::world::word(&mut data, *offset, *value);
    }
    for (offset, value) in bytes {
        data[*offset] = *value;
    }
    Arc::new(Prototype::parse(&data).unwrap())
}
/// The critter prototype words for base SPECIAL 5, then the base `stats`.
pub(crate) fn special(stats: &[(usize, i32)]) -> Vec<(usize, i32)> {
    (0..7)
        .map(|stat| (stat, 5))
        .chain(stats.iter().copied())
        .map(|(stat, value)| (48 + 4 * stat, value))
        .collect()
}
/// The `npc_prototype` fixture critter.
pub(crate) fn npc_prototype() -> Arc<Prototype> {
    Arc::new(Prototype::parse(&fixture::npc_prototype()).unwrap())
}
/// A critter's hit points, on its sheet and in its instance state.
pub(crate) fn set_hp(world: &mut WorldState, critter: ObjectRef, hp: i32) {
    let object = world.get_mut(critter).unwrap();
    object.character.as_mut().unwrap().current_hp = hp;
    if let ObjectState::Critter(state) = &mut object.raw.state {
        state.hp = hp;
    }
}
pub(crate) fn hp<A: WorldAssets>(host: &WorldHost<A>, critter: ObjectRef) -> i32 {
    host.world.hit_points(critter).unwrap()
}
/// The right-hand primary attack of `attacker` on `defender` at `place`.
pub(crate) fn right_primary(
    world: &WorldState,
    attacker: ObjectRef,
    defender: ObjectRef,
    place: u8,
    aimed: bool,
) -> Attack {
    let mode = hit_mode::RIGHT_PRIMARY;
    world
        .attack_profile(attacker, defender, mode, place, aimed)
        .unwrap()
}
pub(crate) fn gvar<A: WorldAssets>(host: &WorldHost<A>, index: usize) -> i32 {
    host.world.gvars.as_ref().unwrap()[index]
}

/// `gvar[index] += 1`.
pub(crate) fn count(index: i32) -> Vec<u8> {
    [
        fixture::push(index),
        fixture::push(index),
        fixture::instruction(0x80c5),
        fixture::push(1),
        fixture::instruction(0x8039),
        fixture::instruction(0x80c6),
    ]
    .concat()
}
/// A program of returning `procedures` and no strings.
pub(crate) fn program(procedures: &[(&[u8], Vec<u8>)]) -> IntProgram {
    let procedures: Vec<(&[u8], Vec<u8>)> = procedures
        .iter()
        .map(|(name, body)| (*name, fixture::returning(body.clone())))
        .collect();
    IntProgram::parse(&fixture::program(&[], &procedures)).unwrap()
}
/// A request of `event` to `script`, from no source and with no action.
pub(crate) fn request(script: u32, event: ScriptEvent) -> ScriptRequest {
    ScriptRequest {
        script,
        event,
        source: 0,
        target: 0,
        fixed: 0,
        action: -1,
        only_present: false,
    }
}
/// The scripts that received a request of `event`, in order.
pub(crate) fn requested<A: WorldAssets>(host: &WorldHost<A>, event: ScriptEvent) -> Vec<u32> {
    host.world
        .trace
        .iter()
        .filter_map(|entry| match entry {
            WorldTrace::Script { id, event: e, .. } if *e == event => Some(*id),
            _ => None,
        })
        .collect()
}
/// A fresh map script record of `file`, unbound to any object.
pub(crate) fn record(id: u32, file: i32, prefix: ScriptPrefix) -> MapScript {
    MapScript {
        id,
        link: 0,
        prefix,
        stored_flags: ScriptFlags(0),
        flags: ScriptFlags(0),
        file_index: file,
        object_id: -1,
        local_base: -1,
        stored_local_count: 0,
        local_count: 0,
        result: 0,
        event: 0,
        parameter: 0,
        action: 0,
        override_default: 0,
        execution_state: 0,
        quantity: 0,
        annotations: 0,
        file_offset: 0,
    }
}
/// One script stream holding `records`, all active, in one block.
pub(crate) fn stream(records: Vec<MapScript>) -> ScriptCategory {
    ScriptCategory {
        declared_count: records.len(),
        blocks: vec![ScriptBlock {
            active: records.len(),
            slots: records,
        }],
    }
}
