//! Live validation of the launcher's game data and mod layers. Each check
//! opens what it names (directory tables only, never the members) and says
//! why it fails.
use std::{
    path::{Path, PathBuf},
    sync::Arc,
};
use vault_formats::{DatArchive, LayerEntry, LayeredVfs, LoadOrderPolicy, Vfs, VfsError, VfsPath};

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

fn open(path: &Path) -> Result<DatArchive, String> {
    if !path.is_file() {
        return Err(if path.exists() {
            "not a file".into()
        } else {
            "file not found".into()
        });
    }
    DatArchive::open(path).map_err(|error| format!("not a readable DAT archive: {error}"))
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
/// the missing ones in the loose data folder).
pub fn master(path: &Path, data_root: Option<&Path>) -> Check {
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

/// CRITTER.DAT: an archive with critter art.
pub fn critter(path: &Path) -> Check {
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
/// is the whole game data, so it must then hold [`MASTER_MEMBERS`] itself.
pub fn data_root(path: &Path, standalone: bool) -> Check {
    if !path.is_dir() {
        return Check::bad(if path.exists() {
            "not a directory"
        } else {
            "directory not found"
        });
    }
    let entries = std::fs::read_dir(path).map_or(0, |entries| entries.count());
    if standalone {
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
struct Empty;
impl Vfs for Empty {
    fn read(&self, path: &str) -> Result<Vec<u8>, VfsError> {
        Err(VfsError::NotFound(VfsPath::new(path)?))
    }
    fn files(&self) -> Box<dyn Iterator<Item = &VfsPath> + '_> {
        Box::new(std::iter::empty())
    }
}

/// The mod layers' checks, one per entry, and the whole stack's.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Mods {
    pub layers: Vec<Check>,
    pub stack: Check,
}

/// Mount each enabled layer on its own (for its error), then the whole
/// stack, as the game would: paths resolved against `directory`.
pub fn mods(directory: &Path, entries: &[LayerEntry], policy: LoadOrderPolicy) -> Mods {
    let mount = |entries: &[LayerEntry]| {
        LayeredVfs::from_entries(Arc::new(Empty), directory, entries, policy)
            .map_err(|error| error.to_string())
    };
    let layers = entries
        .iter()
        .map(|entry| {
            if !entry.enabled {
                return Check::unset("disabled");
            }
            match mount(std::slice::from_ref(entry)) {
                Ok(vfs) => Check::ok(plural(vfs.files().count(), "file")),
                Err(error) => Check::bad(error),
            }
        })
        .collect();
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
    Mods { layers, stack }
}

/// `count` and `noun`, with an `s` unless the count is one.
fn plural(count: usize, noun: &str) -> String {
    if count == 1 {
        format!("{count} {noun}")
    } else {
        format!("{count} {noun}s")
    }
}
