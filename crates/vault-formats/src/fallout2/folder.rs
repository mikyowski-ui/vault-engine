//! SPEC-018 §2.1: finding a Fallout 2 install's archives in a folder,
//! opening them as SPEC-015 specifies, and mounting SPEC-018 §1's databases.
use crate::{Dat2Archive, Dat2Provider, DatArchive, LocalDirProvider};
use std::{
    path::{Path, PathBuf},
    sync::Arc,
};

/// The archives found in an install folder.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Fallout2Install {
    pub folder: PathBuf,
    pub master: PathBuf,
    pub critter: PathBuf,
    /// `patch000.dat`, when present.
    pub patch: Option<PathBuf>,
}

/// The opened archives.
#[derive(Debug, Clone)]
pub struct OpenedInstall {
    pub master: Arc<Dat2Archive>,
    pub patch: Option<Arc<Dat2Archive>>,
    pub critter: Arc<Dat2Archive>,
}

impl Fallout2Install {
    /// §2.1: list the folder's own entries and match `MASTER.DAT`,
    /// `CRITTER.DAT` and `patch000.dat` with ASCII case folded. Only a
    /// regular file (links followed) matches; two entries matching one
    /// name refuse the install; the first two are required.
    pub fn discover(folder: &Path) -> Result<Self, String> {
        let entries =
            std::fs::read_dir(folder).map_err(|error| format!("{}: {error}", folder.display()))?;
        let mut found: [Vec<PathBuf>; 3] = Default::default();
        const NAMES: [&str; 3] = ["MASTER.DAT", "CRITTER.DAT", "patch000.dat"];
        for entry in entries.flatten() {
            let path = entry.path();
            let Some(name) = entry.file_name().to_str().map(str::to_owned) else {
                continue;
            };
            let Some(index) = NAMES.iter().position(|n| n.eq_ignore_ascii_case(&name)) else {
                continue;
            };
            // A folder or other entry with a matching name is skipped.
            if path.is_file() {
                found[index].push(path);
            }
        }
        for (index, paths) in found.iter_mut().enumerate() {
            paths.sort();
            if let [first, second, ..] = paths.as_slice() {
                return Err(format!(
                    "two entries match {}: {} and {}",
                    NAMES[index],
                    file_name(first),
                    file_name(second)
                ));
            }
        }
        let [master, critter, patch] = found.map(|mut paths| paths.pop());
        let master = master.ok_or_else(|| format!("{}: MASTER.DAT not found", folder.display()))?;
        let critter =
            critter.ok_or_else(|| format!("{}: CRITTER.DAT not found", folder.display()))?;
        Ok(Self {
            folder: folder.to_path_buf(),
            master,
            critter,
            patch,
        })
    }
    /// The install folder of a configured `MASTER.DAT` path: its parent.
    pub fn folder_of(master: &Path) -> PathBuf {
        match master.parent() {
            Some(parent) if !parent.as_os_str().is_empty() => parent.to_path_buf(),
            _ => PathBuf::from("."),
        }
    }
    /// §2.1: open each archive as SPEC-015 specifies. Any refusal refuses
    /// the install, naming the archive and the rule; a `MASTER.DAT` that
    /// opens as a SPEC-008 archive is named as Fallout 1 data.
    pub fn open(&self) -> Result<OpenedInstall, String> {
        let open = |path: &Path| -> Result<Arc<Dat2Archive>, String> {
            Dat2Archive::open(path)
                .map(Arc::new)
                .map_err(|error| format!("{}: {error} (SPEC-015 §2)", file_name(path)))
        };
        let master = open(&self.master).map_err(|error| {
            if DatArchive::open(&self.master).is_ok() {
                format!(
                    "{}: this folder holds Fallout 1 data (a SPEC-008 archive; set Game to fallout1)",
                    file_name(&self.master)
                )
            } else {
                error
            }
        })?;
        let patch = self.patch.as_deref().map(open).transpose()?;
        let critter = open(&self.critter)?;
        Ok(OpenedInstall {
            master,
            patch,
            critter,
        })
    }
}

impl OpenedInstall {
    /// SPEC-018 §1's databases: `data_root` first, `patch000.dat` above
    /// `MASTER.DAT`, critter art from `CRITTER.DAT` alone.
    pub fn provider(&self, loose: Option<Arc<LocalDirProvider>>) -> Dat2Provider {
        let mut layers = vec![self.master.clone()];
        layers.extend(self.patch.clone());
        let loose = loose.map(|root| root as Arc<dyn crate::Vfs>);
        Dat2Provider::new(loose, layers, Some(self.critter.clone()))
    }
}

fn file_name(path: &Path) -> String {
    path.file_name().map_or_else(
        || path.display().to_string(),
        |name| name.to_string_lossy().into_owned(),
    )
}
