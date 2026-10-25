//! Live validation of the launcher's game data and mod layers. Each check
//! opens what it names (directory tables only, never the members) and says
//! why it fails.
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

/// A VFS path below `root`, matched one component at a time without
/// regard to ASCII case (as the loose mount reads it).
pub fn find_loose(root: &Path, path: &str) -> Option<PathBuf> {
    path.split('/').try_fold(root.to_path_buf(), |dir, part| {
        let mut found: Vec<PathBuf> = std::fs::read_dir(&dir)
            .ok()?
            .flatten()
            .filter(|entry| {
                entry
                    .file_name()
                    .to_str()
                    .is_some_and(|name| name.eq_ignore_ascii_case(part))
            })
            .map(|entry| entry.path())
            .collect();
        found.sort();
        found.into_iter().next()
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
    let (layers, manifests) = entries
        .iter()
        .map(|entry| {
            let manifest = manifest(entry);
            if !entry.enabled {
                return (Check::unset("disabled"), manifest.ok().flatten());
            }
            let check = match (mount(std::slice::from_ref(entry)), &manifest) {
                (Err(error), _) => Check::bad(error),
                (Ok(_), Err(error)) => Check::bad(error.clone()),
                (Ok(vfs), Ok(_)) => Check::ok(plural(vfs.files().count(), "file")),
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
