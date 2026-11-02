//! Live validation of the launcher's game data and mod layers. Each check
//! opens what it names (directory tables only, never the members) and says
//! why it fails.
use crate::config::{KnownPaths, discover_game, entry_any_case};
use std::{
    path::{Path, PathBuf},
    sync::Arc,
};
use vault_core::GameProfile;
use vault_formats::{
    Dat2Archive, DatArchive, LayerEntry, LayeredVfs, LoadOrderPolicy, ModManifest, Vfs, VfsError,
    VfsPath,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum State {
    /// Nothing named (or a disabled layer).
    Unset,
    Ok,
    Bad,
}

/// A check's outcome and its reason or summary.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Check {
    pub state: State,
    pub text: String,
}

impl Check {
    pub fn unset(text: impl Into<String>) -> Self {
        Self {
            state: State::Unset,
            text: text.into(),
        }
    }
    fn ok(text: impl Into<String>) -> Self {
        Self {
            state: State::Ok,
            text: text.into(),
        }
    }
    fn bad(text: impl Into<String>) -> Self {
        Self {
            state: State::Bad,
            text: text.into(),
        }
    }
    pub fn is_ok(&self) -> bool {
        self.state == State::Ok
    }
}

/// What a new game reads first, which the master database (or the loose
/// data folder over it) must hold: the palette, the campaign's initial
/// global state, and the new-game start map (SPEC-014 game flow).
pub const MASTER_MEMBERS: [&str; 3] = [
    "color.pal",
    "data/vault13.gam",
    vault_core::ui::flow::NEW_GAME_MAP,
];

/// The critter database serves the `art/critters/` requests.
const CRITTER_DIR: &[u8] = b"ART\\CRITTERS\\";

fn is_file(path: &Path) -> Result<(), String> {
    if path.is_file() {
        Ok(())
    } else if path.exists() {
        Err("not a file".into())
    } else {
        Err("file not found".into())
    }
}

fn open(path: &Path) -> Result<DatArchive, String> {
    is_file(path)?;
    DatArchive::open(path).map_err(|error| {
        // A Fallout 2 archive under the Fallout 1 profile says so.
        let hint = if Dat2Archive::open(path).is_ok() {
            " (a Fallout 2 archive: set Game to fallout2)"
        } else {
            ""
        };
        format!("not a readable DAT archive: {error}{hint}")
    })
}

/// A Fallout 2 archive (SPEC-015) and its member count. Which members a
/// Fallout 2 install needs is not yet specified (SPEC-018), so none is
/// required.
fn open_v2(path: &Path) -> Check {
    if let Err(error) = is_file(path) {
        return Check::bad(error);
    }
    match Dat2Archive::open(path) {
        Ok(archive) => Check::ok(format!(
            "{} (Fallout 2 archive)",
            plural(archive.entries().len(), "member")
        )),
        Err(error) => {
            let hint = if DatArchive::open(path).is_ok() {
                " (a Fallout 1 archive: set Game to fallout1)"
            } else {
                ""
            };
            Check::bad(format!("not a readable Fallout 2 archive: {error}{hint}"))
        }
    }
}

/// SPEC-018 §2: a Fallout 2 install, from the configured `MASTER.DAT`
/// path: its folder's archives are found (case folded, regular files,
/// ambiguity refused), opened as SPEC-015 specifies (a refused
/// `patch000.dat` refuses the install), and the master database as mounted
/// (`data_root`, then `patch000.dat`, then `MASTER.DAT`) must hold the
/// required members. Returns the MASTER.DAT and CRITTER.DAT checks.
pub fn fallout2(master: &Path, critter: Option<&Path>, data_root: Option<&Path>) -> [Check; 2] {
    use vault_formats::fallout2::{Fallout2Install, missing_members};
    let bad = |text: String| [Check::bad(text), Check::unset("")];
    let folder = Fallout2Install::folder_of(master);
    let install = match Fallout2Install::discover(&folder) {
        Ok(install) => install,
        Err(error) => return bad(error),
    };
    let named = |path: &Path, found: &Path| {
        std::fs::canonicalize(path).ok() == std::fs::canonicalize(found).ok()
    };
    if !named(master, &install.master) {
        return bad(format!(
            "Fallout 2 reads {} in its folder",
            install.master.display()
        ));
    }
    let critter_check = match critter {
        Some(critter) if !named(critter, &install.critter) => Check::bad(format!(
            "Fallout 2 reads {} beside MASTER.DAT",
            install.critter.display()
        )),
        _ => Check::ok(format!("{} (Fallout 2 archive)", install.critter.display())),
    };
    let opened = match install.open() {
        Ok(opened) => opened,
        Err(error) => return bad(error),
    };
    let loose = match data_root
        .map(vault_formats::LocalDirProvider::mount)
        .transpose()
    {
        Ok(loose) => loose.map(Arc::new),
        Err(error) => return bad(format!("data folder: {error}")),
    };
    let provider = opened.provider(loose);
    let count = opened.master.entries().len();
    let missing = missing_members(&provider);
    if !missing.is_empty() {
        return [
            Check::bad(format!(
                "{} (Fallout 2 archive); missing {}",
                plural(count, "member"),
                missing.join(", ")
            )),
            critter_check,
        ];
    }
    let patch = if opened.patch.is_some() {
        ", patch000.dat mounted"
    } else {
        ""
    };
    [
        Check::ok(format!(
            "{} (Fallout 2 archive){patch}",
            plural(count, "member")
        )),
        critter_check,
    ]
}

/// `members` the archive (or the loose `data_root`) is missing.
fn missing(archive: Option<&DatArchive>, data_root: Option<&Path>) -> Vec<&'static str> {
    MASTER_MEMBERS
        .into_iter()
        .filter(|member| {
            let key = member.to_ascii_uppercase().replace('/', "\\");
            !archive.is_some_and(|archive| archive.entry(key.as_bytes()).is_some())
                && !data_root.is_some_and(|root| find_loose(root, member).is_some())
        })
        .collect()
}

/// MASTER.DAT: an archive holding every [`MASTER_MEMBERS`] entry (or with
/// the missing ones in the loose data folder); for Fallout 2, a readable
/// SPEC-015 archive.
pub fn master(path: &Path, data_root: Option<&Path>, game: GameProfile) -> Check {
    if game == GameProfile::Fallout2 {
        return open_v2(path);
    }
    let archive = match open(path) {
        Ok(archive) => archive,
        Err(error) => return Check::bad(error),
    };
    let count = archive.paths().count();
    match missing(Some(&archive), data_root).as_slice() {
        [] => Check::ok(plural(count, "member")),
        missing => Check::bad(format!(
            "{}; missing {}",
            plural(count, "member"),
            missing.join(", ")
        )),
    }
}

/// CRITTER.DAT: an archive with critter art; for Fallout 2, a readable
/// SPEC-015 archive.
pub fn critter(path: &Path, game: GameProfile) -> Check {
    if game == GameProfile::Fallout2 {
        return open_v2(path);
    }
    let archive = match open(path) {
        Ok(archive) => archive,
        Err(error) => return Check::bad(error),
    };
    let count = archive.paths().count();
    let art = archive
        .paths()
        .filter(|path| {
            path.len() > CRITTER_DIR.len()
                && path[..CRITTER_DIR.len()].eq_ignore_ascii_case(CRITTER_DIR)
        })
        .count();
    if art == 0 {
        Check::bad(format!(
            "{}; none under art/critters/",
            plural(count, "member")
        ))
    } else {
        Check::ok(format!("{}, {art} critter art", plural(count, "member")))
    }
}

/// The loose data folder: an existing directory. Without the databases it
/// is the whole game data, so it must then hold [`MASTER_MEMBERS`] itself
/// (Fallout 1 only: Fallout 2's are not yet specified).
pub fn data_root(path: &Path, standalone: bool, game: GameProfile) -> Check {
    if !path.is_dir() {
        return Check::bad(if path.exists() {
            "not a directory"
        } else {
            "directory not found"
        });
    }
    let entries = std::fs::read_dir(path).map_or(0, |entries| entries.count());
    if standalone && game == GameProfile::Fallout1 {
        let missing = missing(None, Some(path));
        if !missing.is_empty() {
            return Check::bad(format!("no databases, and missing {}", missing.join(", ")));
        }
    }
    Check::ok(format!("folder, {entries} entries"))
}

/// The three checks of one game's paths: the data folder, then the
/// databases in that game's format.
pub fn install(
    master: Option<&Path>,
    critter: Option<&Path>,
    root: Option<&Path>,
    game: GameProfile,
) -> [Check; 3] {
    let databases = master.is_some() || critter.is_some();
    let root_check = match root {
        None => Check::unset("optional: loose files read before the archives"),
        Some(root) => data_root(root, !databases, game),
    };
    let root = root.filter(|_| root_check.is_ok());
    let required = |other: &str| Check {
        state: State::Bad,
        text: format!("required with {other}"),
    };
    let [master_check, critter_check] = if game == GameProfile::Fallout2 {
        // SPEC-018 §2: the install folder holding MASTER.DAT is listed
        // for its archives, which are opened, and the master database
        // as mounted must hold the required members.
        match master {
            Some(path) => fallout2(path, critter, root),
            None => [
                Check {
                    state: State::Bad,
                    text: "required: Fallout 2 reads MASTER.DAT and CRITTER.DAT".into(),
                },
                Check::unset("found beside MASTER.DAT"),
            ],
        }
    } else {
        [
            match (master, critter) {
                (Some(path), _) => self::master(path, root, game),
                (None, Some(_)) => required("CRITTER.DAT"),
                (None, None) => Check::unset("not set: the data folder alone is the game data"),
            },
            match (critter, master) {
                (Some(path), _) => self::critter(path, game),
                (None, Some(_)) => required("MASTER.DAT"),
                (None, None) => Check::unset("not set"),
            },
        ]
    };
    [master_check, critter_check, root_check]
}

/// Whether [`install`]'s checks can start the game, or the first reason
/// they cannot.
pub fn install_ready(checks: &[Check; 3]) -> Result<(), String> {
    const NAMES: [&str; 3] = ["MASTER.DAT", "CRITTER.DAT", "Data folder"];
    if let Some((name, check)) = NAMES
        .iter()
        .zip(checks)
        .find(|(_, check)| check.state == State::Bad)
    {
        return Err(format!("{name}: {}.", check.text));
    }
    if checks.iter().all(|check| check.state == State::Unset) {
        return Err("Name MASTER.DAT and CRITTER.DAT, or a data folder.".into());
    }
    Ok(())
}

/// A VFS path below `root`, matched one component at a time without
/// regard to ASCII case (as the loose mount reads it).
pub fn find_loose(root: &Path, path: &str) -> Option<PathBuf> {
    path.split('/').try_fold(root.to_path_buf(), |dir, part| {
        entry_any_case(&dir, part, |_| true)
    })
}

/// What [`detect_install`] found in a folder.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Detected {
    pub master: Option<PathBuf>,
    pub critter: Option<PathBuf>,
    pub data_root: Option<PathBuf>,
    /// The folder, when it holds game data.
    pub install_dir: Option<PathBuf>,
    /// A `ddraw.ini` stands beside the databases (name only, never read).
    pub sfall_ini: bool,
    pub mods_dir: Option<PathBuf>,
    /// A file or folder called `fallout1in2` was found (name only).
    pub fallout1in2: bool,
    /// Remembered Fallout 1 folders this install links.
    pub secondary_dat_dirs: Vec<PathBuf>,
    /// Whose archive format MASTER.DAT opens in.
    pub edition: Option<GameProfile>,
    /// What was found and what was not, in plain words.
    pub notes: Vec<String>,
}

/// The names that mark Et Tu, beside the databases or in `mods`.
const ET_TU_NAMES: [&str; 2] = ["fallout1in2", "fallout1in2.dat"];

/// The game whose archive format `master` opens in (directory tables only).
/// SPEC-015's is tried first: its footer must state the file's own length.
fn edition_of(master: &Path) -> Option<GameProfile> {
    if Dat2Archive::open(master).is_ok() {
        Some(GameProfile::Fallout2)
    } else if DatArchive::open(master).is_ok() {
        Some(GameProfile::Fallout1)
    } else {
        None
    }
}

/// What the folder `dir` holds, found by name in any case. The databases'
/// directory tables tell the edition. `fallout1in2`, `mods` and `ddraw.ini`
/// are only names here: no mod and no `ddraw.ini` is opened.
///
/// A folder with `fallout1in2` looks like Et Tu, which plays Fallout 1's data
/// in Fallout 2's engine, so the newest folder of `known.fallout1` with a
/// Fallout 1 `MASTER.DAT` and a `CRITTER.DAT` is linked in
/// `secondary_dat_dirs`. The notes say when there is none.
pub fn detect_install(dir: &Path, known: &KnownPaths) -> Detected {
    let dir = std::path::absolute(dir).unwrap_or_else(|_| dir.to_path_buf());
    let mut found = Detected::default();
    if !dir.is_dir() {
        found
            .notes
            .push(format!("{} is not a folder.", dir.display()));
        return found;
    }
    found.master = entry_any_case(&dir, "master.dat", Path::is_file);
    found.critter = entry_any_case(&dir, "critter.dat", Path::is_file);
    found.data_root = entry_any_case(&dir, "data", Path::is_dir);
    found.mods_dir = entry_any_case(&dir, "mods", Path::is_dir);
    found.sfall_ini = entry_any_case(&dir, "ddraw.ini", Path::is_file).is_some();
    let marked = |folder: &Path| {
        ET_TU_NAMES
            .iter()
            .any(|name| entry_any_case(folder, name, |_| true).is_some())
    };
    found.fallout1in2 = marked(&dir) || found.mods_dir.as_deref().is_some_and(marked);
    found.edition = found.master.as_deref().and_then(edition_of);
    let databases = found.master.is_some() && found.critter.is_some();
    if databases || found.data_root.is_some() {
        found.install_dir = Some(dir.clone());
    }

    let mut notes = Vec::new();
    if found.master.is_none() {
        notes.push("No MASTER.DAT here.".to_owned());
    }
    if found.critter.is_none() {
        notes.push("No CRITTER.DAT here.".to_owned());
    }
    match found.edition {
        Some(GameProfile::Fallout1) => notes.push("Fallout 1 archives.".to_owned()),
        Some(GameProfile::Fallout2) => notes.push("Fallout 2 archives.".to_owned()),
        None if found.master.is_some() => {
            notes.push("MASTER.DAT opens as neither game's archive.".to_owned());
        }
        None => {}
    }
    if found.mods_dir.is_some() {
        notes.push("A mods folder is here.".to_owned());
    }
    if found.sfall_ini {
        notes.push("A ddraw.ini is here.".to_owned());
    }
    if found.fallout1in2 {
        notes.push("fallout1in2 is here: this looks like Et Tu.".to_owned());
        match linkable_fallout1(&dir, known) {
            Some(folder) => {
                notes.push(format!(
                    "Linked the Fallout 1 data in {}.",
                    folder.display()
                ));
                found.secondary_dat_dirs.push(folder.clone());
            }
            None if known.fallout1.is_empty() => {
                notes.push("No Fallout 1 data is remembered: Et Tu needs it.".to_owned());
            }
            None => notes.push(
                "No remembered Fallout 1 folder holds Fallout 1's MASTER.DAT and CRITTER.DAT: \
                 Et Tu needs them."
                    .to_owned(),
            ),
        }
    }
    found.notes = notes;
    found
}

/// The newest remembered folder other than `dir` with Fallout 1's databases.
fn linkable_fallout1<'a>(dir: &Path, known: &'a KnownPaths) -> Option<&'a PathBuf> {
    let own = std::fs::canonicalize(dir).ok();
    known.fallout1.iter().find(|folder| {
        std::fs::canonicalize(folder).ok() != own
            && discover_game(std::slice::from_ref(folder), GameProfile::Fallout1).is_some()
    })
}

/// No files: the cheap base the mod layers are validated over.
pub(crate) struct Empty;
impl Vfs for Empty {
    fn read(&self, path: &str) -> Result<Vec<u8>, VfsError> {
        Err(VfsError::NotFound(VfsPath::new(path)?))
    }
    fn files(&self) -> Box<dyn Iterator<Item = &VfsPath> + '_> {
        Box::new(std::iter::empty())
    }
}

/// The mod layers' checks, one per entry, and the whole stack's.
#[derive(Debug, Clone, PartialEq)]
pub struct Mods {
    pub layers: Vec<Check>,
    /// Each entry's `vault-mod.toml`, when its folder has a valid one.
    pub manifests: Vec<Option<ModManifest>>,
    pub stack: Check,
}

/// Mount each enabled layer on its own (for its error), then the whole
/// stack, as the game would: paths resolved against `directory`. Each
/// folder's `vault-mod.toml` is read too (also for a disabled layer, so the
/// list names it); a malformed one fails its layer, as it fails a launch.
pub fn mods(directory: &Path, entries: &[LayerEntry], policy: LoadOrderPolicy) -> Mods {
    let mount = |entries: &[LayerEntry]| {
        LayeredVfs::from_entries(Arc::new(Empty), directory, entries, policy)
            .map_err(|error| error.to_string())
    };
    let manifest = |entry: &LayerEntry| match crate::mods::layer_dir(directory, entry, policy) {
        Some(dir) => crate::mods::read_manifest(&dir),
        None => Ok(None),
    };
    let mut enabled_dirs = Vec::new();
    let (layers, manifests) = entries
        .iter()
        .map(|entry| {
            let manifest = manifest(entry);
            if !entry.enabled {
                return (Check::unset("disabled"), manifest.ok().flatten());
            }
            // SPEC-024a §2.2: with the layers below it, as a launch reads
            // them, so a conflict shows on the higher layer.
            enabled_dirs.extend(crate::mods::layer_dir(directory, entry, policy));
            let conflict = crate::mods::manifest_layers(&enabled_dirs).err();
            let check = match (mount(std::slice::from_ref(entry)), &manifest, conflict) {
                (Err(error), _, _) => Check::bad(error),
                (Ok(_), Err(error), _) => Check::bad(error.clone()),
                (Ok(_), Ok(_), Some(error)) => Check::bad(error),
                (Ok(vfs), Ok(_), None) => Check::ok(plural(vfs.files().count(), "file")),
            };
            (check, manifest.ok().flatten())
        })
        .unzip();
    let stack = if entries.is_empty() {
        Check::unset("no mod layers")
    } else {
        match mount(entries) {
            Ok(vfs) => Check::ok(format!(
                "{} of {} mounted, {}",
                vfs.mod_layers(),
                plural(entries.len(), "layer"),
                plural(vfs.files().count(), "file")
            )),
            Err(error) => Check::bad(error),
        }
    };
    Mods {
        layers,
        manifests,
        stack,
    }
}

/// `count` and `noun`, with an `s` unless the count is one.
fn plural(count: usize, noun: &str) -> String {
    if count == 1 {
        format!("{count} {noun}")
    } else {
        format!("{count} {noun}s")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::{TempDir, tempdir};
    use vault_fixtures::dat::fallout1_install;

    /// A Fallout 2 folder: empty archives in that game's format.
    fn fallout2_install(dir: &Path) {
        let archive = vault_fixtures::dat2::archive(&[]);
        fs::write(dir.join("MASTER.DAT"), &archive).unwrap();
        fs::write(dir.join("CRITTER.DAT"), &archive).unwrap();
    }

    /// A synthetic Et Tu folder, its marker in `mods`.
    fn et_tu() -> TempDir {
        let temp = tempdir().unwrap();
        fallout2_install(temp.path());
        fs::create_dir_all(temp.path().join("mods/fallout1in2")).unwrap();
        fs::write(temp.path().join("ddraw.ini"), b"").unwrap();
        temp
    }

    fn fallout1() -> TempDir {
        let temp = tempdir().unwrap();
        fallout1_install(temp.path());
        temp
    }

    fn known(fallout1: &[&Path]) -> KnownPaths {
        KnownPaths {
            fallout1: fallout1.iter().map(|path| path.to_path_buf()).collect(),
            ..KnownPaths::default()
        }
    }

    fn has_note(found: &Detected, text: &str) -> bool {
        found.notes.iter().any(|note| note.contains(text))
    }

    #[test]
    fn a_fallout1_folder_is_told_by_its_archives_in_any_case() {
        let temp = tempdir().unwrap();
        let dir = temp.path();
        fallout1_install(dir);
        fs::rename(dir.join("MASTER.DAT"), dir.join("mAsTeR.dAt")).unwrap();
        fs::rename(dir.join("CRITTER.DAT"), dir.join("Critter.Dat")).unwrap();
        fs::create_dir(dir.join("DaTa")).unwrap();

        let found = detect_install(dir, &KnownPaths::default());
        assert_eq!(found.master, Some(dir.join("mAsTeR.dAt")));
        assert_eq!(found.critter, Some(dir.join("Critter.Dat")));
        assert_eq!(found.data_root, Some(dir.join("DaTa")));
        assert_eq!(found.install_dir.as_deref(), Some(dir));
        assert_eq!(found.edition, Some(GameProfile::Fallout1));
        assert!(!found.fallout1in2 && !found.sfall_ini && found.mods_dir.is_none());
        assert!(found.secondary_dat_dirs.is_empty());
        assert!(!has_note(&found, "Et Tu"), "{:?}", found.notes);
    }

    #[test]
    fn a_fallout2_folder_without_a_marker_links_nothing() {
        let temp = tempdir().unwrap();
        fallout2_install(temp.path());
        fs::create_dir(temp.path().join("MODS")).unwrap();
        let fallout1 = fallout1();

        let found = detect_install(temp.path(), &known(&[fallout1.path()]));
        assert_eq!(found.edition, Some(GameProfile::Fallout2));
        assert_eq!(found.mods_dir, Some(temp.path().join("MODS")));
        assert!(!found.fallout1in2);
        assert!(found.secondary_dat_dirs.is_empty());
    }

    #[test]
    fn et_tu_links_the_newest_remembered_fallout1_folder() {
        let et_tu = et_tu();
        let (older, newer) = (fallout1(), fallout1());
        let empty = tempdir().unwrap();
        let list = [et_tu.path(), empty.path(), newer.path(), older.path()];

        let found = detect_install(et_tu.path(), &known(&list));
        assert!(found.fallout1in2 && found.sfall_ini);
        assert_eq!(found.mods_dir, Some(et_tu.path().join("mods")));
        assert_eq!(found.edition, Some(GameProfile::Fallout2));
        assert_eq!(found.secondary_dat_dirs, [newer.path().to_path_buf()]);
        assert!(has_note(&found, "Linked the Fallout 1 data"));
    }

    #[test]
    fn et_tu_with_no_fallout1_remembered_says_so() {
        let et_tu = et_tu();
        let found = detect_install(et_tu.path(), &KnownPaths::default());
        assert!(found.fallout1in2);
        assert!(found.secondary_dat_dirs.is_empty());
        assert!(has_note(&found, "No Fallout 1 data is remembered"));
    }

    #[test]
    fn a_remembered_folder_without_fallout1s_databases_is_not_linked() {
        let et_tu = et_tu();
        // Fallout 2's archives under the Fallout 1 list, and an empty folder.
        let fallout2 = tempdir().unwrap();
        fallout2_install(fallout2.path());
        let empty = tempdir().unwrap();

        let list = [fallout2.path(), empty.path(), Path::new("/no/such/folder")];
        let found = detect_install(et_tu.path(), &known(&list));
        assert!(found.secondary_dat_dirs.is_empty());
        assert!(has_note(&found, "No remembered Fallout 1 folder"));
    }

    #[test]
    fn the_marker_is_found_by_name_as_a_file_or_a_folder_in_any_case() {
        let beside = tempdir().unwrap();
        fs::create_dir(beside.path().join("Fallout1In2")).unwrap();
        assert!(detect_install(beside.path(), &KnownPaths::default()).fallout1in2);

        let in_mods = tempdir().unwrap();
        fs::create_dir(in_mods.path().join("mods")).unwrap();
        fs::write(in_mods.path().join("mods/FALLOUT1IN2.DAT"), b"").unwrap();
        assert!(detect_install(in_mods.path(), &KnownPaths::default()).fallout1in2);

        let other = tempdir().unwrap();
        fs::create_dir(other.path().join("mods")).unwrap();
        fs::create_dir(other.path().join("mods/fallout2in1")).unwrap();
        assert!(!detect_install(other.path(), &KnownPaths::default()).fallout1in2);
    }

    #[test]
    fn a_folder_of_nothing_says_what_is_missing() {
        let temp = tempdir().unwrap();
        // Named as a file would be, but a folder: not a database or an ini.
        fs::create_dir(temp.path().join("master.dat")).unwrap();
        fs::create_dir(temp.path().join("DDRAW.INI")).unwrap();
        let found = detect_install(temp.path(), &KnownPaths::default());
        assert_eq!(found.master, None);
        assert_eq!(found.install_dir, None);
        assert_eq!(found.edition, None);
        assert!(!found.sfall_ini);
        assert!(has_note(&found, "No MASTER.DAT") && has_note(&found, "No CRITTER.DAT"));

        fs::write(temp.path().join("CRITTER.DAT"), b"junk").unwrap();
        let found = detect_install(&temp.path().join("CRITTER.DAT"), &KnownPaths::default());
        assert!(has_note(&found, "not a folder"));
    }

    #[test]
    fn a_master_of_neither_format_is_reported() {
        let temp = tempdir().unwrap();
        fs::write(temp.path().join("MASTER.DAT"), b"junk").unwrap();
        fs::write(temp.path().join("CRITTER.DAT"), b"junk").unwrap();
        let found = detect_install(temp.path(), &KnownPaths::default());
        assert_eq!(found.edition, None);
        assert_eq!(found.install_dir.as_deref(), Some(temp.path()));
        assert!(has_note(&found, "neither game's archive"));
    }

    #[test]
    fn find_loose_folds_each_component() {
        let temp = tempdir().unwrap();
        fs::create_dir_all(temp.path().join("Data/MAPS")).unwrap();
        fs::write(temp.path().join("Data/MAPS/V13Ent.map"), b"").unwrap();
        assert_eq!(
            find_loose(temp.path(), "data/maps/v13ent.map"),
            Some(temp.path().join("Data/MAPS/V13Ent.map"))
        );
        assert_eq!(find_loose(temp.path(), "data/maps/other.map"), None);
        assert_eq!(find_loose(temp.path(), "data/maps/v13ent.map/x"), None);
    }
}
