//! Save slots in a folder (`host-fs`): `SAVEGAME/SLOTnn` under a writable
//! root, for the tools that save outside a session and for the tests. A
//! session goes through `vault_platform::session` and its `StorageBackend`
//! instead; both commit a [`SlotImage`] with the same staged protocol.
use super::{
    AUTOMAP_FILE, CENTRAL_FILE, QUICK_SLOT, SaveInfo, SlotImage, SlotReader, checked_companion,
    slot_name,
};
use crate::{WorldError, assets::WorldAssets, host::WorldHost};
use std::{
    fs,
    path::{Path, PathBuf},
};
use vault_formats::staged::{self, StagedOp, StagingNames};

/// `SAVEGAME/SLOTnn` under the writable patch root, for slots 01-10.
pub fn slot_directory(root: &Path, slot: u8) -> Result<PathBuf, WorldError> {
    Ok(root.join("SAVEGAME").join(slot_name(slot)?))
}

/// The quick-save slot's folder ([`QUICK_SLOT`]) below the save root.
pub fn quick_slot_directory(root: &Path) -> PathBuf {
    root.join("SAVEGAME").join(QUICK_SLOT)
}

fn io(path: &Path, error: std::io::Error) -> WorldError {
    WorldError::Invalid(format!("{}: {error}", path.display()))
}

/// Case-insensitive lookup of a slot file: the fixed central and automap
/// names, or a [`valid_companion_name`](super::valid_companion_name).
fn slot_file(dir: &Path, name: &str) -> Result<PathBuf, WorldError> {
    if name != CENTRAL_FILE && name != AUTOMAP_FILE {
        checked_companion(name)?;
    }
    let exact = dir.join(name);
    if exact.is_file() {
        return Ok(exact);
    }
    let entries = fs::read_dir(dir).map_err(|error| io(dir, error))?;
    entries
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .find(|path| {
            path.file_name()
                .and_then(|file| file.to_str())
                .is_some_and(|file| file.eq_ignore_ascii_case(name))
        })
        .ok_or_else(|| WorldError::Invalid(format!("{}: missing save companion", exact.display())))
}

/// The file `name` of folder `dir`, keeping the spelling an existing file
/// has (SPEC-023 §3.8 rule 4).
fn stored_path(dir: &Path, name: &str) -> PathBuf {
    fs::read_dir(dir)
        .into_iter()
        .flatten()
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .find(|path| {
            path.file_name()
                .and_then(|file| file.to_str())
                .is_some_and(|file| file.eq_ignore_ascii_case(name))
        })
        .unwrap_or_else(|| dir.join(name))
}

/// Commit `image` to the slot folder `dir` under the SPEC-008 §3.3 safe rule
/// ([`staged`]): one batch, so a failure leaves the old slot untouched. As
/// SPEC-023 §3.11 rule 2 says, it holds a put of every file of the image, a
/// delete of the sidecar when the image has none, and a delete of every stale
/// `.SAV` ([`SlotImage::stale`]). Other files of the slot are left alone.
pub fn write_slot(dir: &Path, image: &SlotImage) -> Result<(), WorldError> {
    let targets: Vec<(PathBuf, &[u8])> = image
        .files()
        .iter()
        .map(|(name, bytes)| (stored_path(dir, name), bytes.as_slice()))
        .collect();
    let sidecar = stored_path(dir, crate::companion::SIDECAR);
    let mut ops: Vec<StagedOp> = targets
        .iter()
        .map(|(target, bytes)| StagedOp::Put { target, bytes })
        .collect();
    if !image.has_sidecar() {
        ops.push(StagedOp::Delete { target: &sidecar });
    }
    let stale = stale_files(dir, image)?;
    ops.extend(stale.iter().map(|target| StagedOp::Delete { target }));
    staged::commit(&ops, StagingNames::Reserved, |_| Ok(()))
        .map_err(|error| WorldError::Invalid(error.to_string()))
}

/// The stale `.SAV` files of slot folder `dir`. A folder is not an entry
/// (SPEC-023 §3.9), and a slot that has no folder yet has no files.
fn stale_files(dir: &Path, image: &SlotImage) -> Result<Vec<PathBuf>, WorldError> {
    let entries = match fs::read_dir(dir) {
        Ok(entries) => entries,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(error) => return Err(io(dir, error)),
    };
    let mut stale = Vec::new();
    for entry in entries {
        let entry = entry.map_err(|error| io(dir, error))?;
        let path = entry.path();
        if entry
            .file_name()
            .to_str()
            .is_some_and(|name| image.stale(name))
            && entry
                .file_type()
                .map_err(|error| io(&path, error))?
                .is_file()
        {
            stale.push(path);
        }
    }
    stale.sort();
    Ok(stale)
}

/// A slot's files in the folder that holds them.
struct FolderSlot<'a>(&'a Path);

impl SlotReader for FolderSlot<'_> {
    fn read_file(&mut self, name: &str) -> Result<Option<Vec<u8>>, WorldError> {
        if name == crate::companion::SIDECAR {
            let path = self.0.join(name);
            return match fs::read(&path) {
                Ok(bytes) => Ok(Some(bytes)),
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
                Err(error) => Err(io(&path, error)),
            };
        }
        let path = slot_file(self.0, name)?;
        fs::read(&path).map(Some).map_err(|error| io(&path, error))
    }
}

impl<A: WorldAssets> WorldHost<A> {
    /// Save into numbered slot `slot` below `root` ([`write_slot`]).
    pub fn save_game(&mut self, root: &Path, slot: u8, info: &SaveInfo) -> Result<(), WorldError> {
        self.save_game_in(&slot_directory(root, slot)?, info)
    }
    /// [`Self::save_game`] into the slot directory `dir` (a numbered slot's,
    /// or [`quick_slot_directory`]).
    pub fn save_game_in(&mut self, dir: &Path, info: &SaveInfo) -> Result<(), WorldError> {
        let image = self.slot_image(info)?;
        write_slot(dir, &image)
    }
    /// [`Self::load_slot`] from numbered slot `slot` below `root`.
    pub fn load_game(&mut self, root: &Path, slot: u8) -> Result<(), WorldError> {
        self.load_game_from(&slot_directory(root, slot)?)
    }
    /// [`Self::load_game`] from the slot directory `dir` (a numbered
    /// slot's, or [`quick_slot_directory`]).
    pub fn load_game_from(&mut self, dir: &Path) -> Result<(), WorldError> {
        self.load_slot(&mut FolderSlot(dir))
    }
}
