//! The archive audit: stream every member of the master and critter
//! databases (only the directory tables and one member are held at a time)
//! and compare its SHA-256 with the loose file at the same path in the
//! unpacked corpus. An archive is read as SPEC-015's or SPEC-008's by what the
//! file is, never by a flag. The mounts the Fallout 2 reports share are here
//! too: a folder's databases, its `data` root, and a mod as a layer.
use std::{
    collections::BTreeSet,
    error::Error,
    fs::File,
    io::BufReader,
    path::{Path, PathBuf},
    sync::Arc,
};

use sha2::{Digest, Sha256};
use vault_formats::{
    Dat2Archive, DatArchive, DatProvider, Database, LocalDirProvider, MAX_ASSET_BYTES, ModLayer,
    Vfs, VfsError, VfsPath, dat::DatTables, fallout2::Fallout2Install,
};

/// The entry of `dir` named `name` without regard to ASCII case, on every
/// platform, for a game's data arrives in any case. Of several, the first by
/// name, so the file system's order never decides. `vault-platform`'s
/// `config::entry_any_case` has these semantics, and a tool cannot reach it.
pub fn entry_any_case(dir: &Path, name: &str, keep: impl Fn(&Path) -> bool) -> Option<PathBuf> {
    let mut found: Vec<PathBuf> = std::fs::read_dir(dir)
        .ok()?
        .flatten()
        .filter(|entry| {
            entry
                .file_name()
                .to_str()
                .is_some_and(|entry| entry.eq_ignore_ascii_case(name))
        })
        .map(|entry| entry.path())
        .filter(|path| keep(path))
        .collect();
    found.sort();
    found.into_iter().next()
}

/// An install's loose `data` folder, in any case.
pub fn data_root(folder: &Path) -> Result<Option<Arc<LocalDirProvider>>, VfsError> {
    entry_any_case(folder, "data", Path::is_dir)
        .map(|data| LocalDirProvider::mount(data).map(Arc::new))
        .transpose()
}

/// Whether `path` opens as a SPEC-008 archive and not as a SPEC-015 one.
/// SPEC-015's is tried first: its footer must state the file's own length.
fn is_dat1(path: &Path) -> bool {
    Dat2Archive::open(path).is_err() && DatArchive::open(path).is_ok()
}

/// A folder's mounted databases, and the format they turned out to be.
pub type Install = (Arc<dyn Vfs>, &'static str);

/// A folder's databases over its `loose` root: a Fallout 1 `MASTER.DAT`
/// through [`DatProvider`], and anything else through [`Fallout2Install`],
/// whose refusals name the archive and the rule.
pub fn mount_install(
    folder: &Path,
    loose: Option<Arc<LocalDirProvider>>,
) -> Result<Install, Box<dyn Error>> {
    let master = entry_any_case(folder, "MASTER.DAT", Path::is_file);
    let Some(master) = master.filter(|master| is_dat1(master)) else {
        let opened = Fallout2Install::discover(folder)?.open()?;
        return Ok((Arc::new(opened.provider(loose)), "DAT2"));
    };
    let critter = entry_any_case(folder, "CRITTER.DAT", Path::is_file)
        .map(DatArchive::open)
        .transpose()?;
    let loose = loose.map(|root| root as Arc<dyn Vfs>);
    let database = |archive: Option<DatArchive>| Database {
        loose: loose.clone(),
        archive: archive.map(Arc::new),
    };
    let provider = DatProvider::new(database(Some(DatArchive::open(master)?)), database(critter));
    Ok((Arc::new(provider), "DAT1"))
}

/// A mod as a layer: a loose folder, or a `.dat` of either format. A SPEC-015
/// archive is a [`Vfs`], so it takes the place of a layer's loose root.
pub fn mod_layer(path: &Path) -> Result<ModLayer, Box<dyn Error>> {
    let name = path.file_name().map_or_else(
        || path.display().to_string(),
        |name| name.to_string_lossy().into_owned(),
    );
    if path.is_dir() {
        let loose: Arc<dyn Vfs> = Arc::new(LocalDirProvider::mount(path)?);
        return Ok(ModLayer::from_parts(name, Some(loose), None, None));
    }
    Ok(match Dat2Archive::open(path) {
        Ok(archive) => ModLayer::from_parts(name, Some(Arc::new(archive)), None, None),
        Err(_) => ModLayer::from_parts(name, None, Some(DatArchive::open(path)?), None),
    })
}

/// One archive's results.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct ArchiveCounts {
    pub archive: PathBuf,
    /// The file opened as a SPEC-015 archive.
    pub dat2: bool,
    pub members: u64,
    pub matched: u64,
    /// Decoded bytes whose digest differs from the loose file's.
    pub differ: u64,
    /// No loose file at the member's path.
    pub missing: u64,
    /// Members that cannot be read, decoded, or named as a VFS path.
    pub failed: u64,
}
impl ArchiveCounts {
    fn failures(&self) -> u64 {
        self.differ + self.missing + self.failed
    }
}

#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct DatAudit {
    pub archives: Vec<ArchiveCounts>,
    pub loose_files: u64,
    /// Loose files that no archive member supplies (reported, not failed).
    pub loose_only: u64,
}
impl DatAudit {
    pub fn report(&self) {
        for counts in &self.archives {
            println!(
                "{} {}: {} members, {} matched, {} differ, {} missing loose, {} failed",
                if counts.dat2 { "DAT2" } else { "DAT" },
                counts.archive.display(),
                counts.members,
                counts.matched,
                counts.differ,
                counts.missing,
                counts.failed
            );
        }
        println!(
            "Loose files: {}, {} supplied by no archive",
            self.loose_files, self.loose_only
        );
    }
    pub fn failed(&self) -> u64 {
        self.archives.iter().map(ArchiveCounts::failures).sum()
    }
}

fn sha256(bytes: &[u8]) -> [u8; 32] {
    Sha256::digest(bytes).into()
}

/// One member's decoded `bytes` against the loose file at `path`.
fn compare<'a>(
    vfs: &dyn Vfs,
    loose: &BTreeSet<&'a str>,
    unseen: &mut BTreeSet<&'a str>,
    counts: &mut ArchiveCounts,
    path: &VfsPath,
    bytes: Result<Vec<u8>, String>,
) {
    let archive = counts.archive.clone();
    let name = archive.display();
    let bytes = match bytes {
        Ok(bytes) => bytes,
        Err(error) => {
            counts.failed += 1;
            eprintln!("FAIL {name}: {path}: {error}");
            return;
        }
    };
    if !loose.contains(path.as_str()) {
        counts.missing += 1;
        eprintln!("MISSING {name}: {path}: no loose file");
        return;
    }
    unseen.remove(path.as_str());
    match vfs.read_bounded(path.as_str(), MAX_ASSET_BYTES) {
        Ok(loose) if sha256(&loose) == sha256(&bytes) => counts.matched += 1,
        Ok(loose) => {
            counts.differ += 1;
            eprintln!(
                "DIFF {name}: {path}: archive {} bytes, loose {} bytes",
                bytes.len(),
                loose.len()
            );
        }
        Err(error) => {
            counts.failed += 1;
            eprintln!("FAIL {name}: {path}: loose file: {error}");
        }
    }
}

/// A SPEC-015 archive's members, in payload order.
fn audit_dat2<'a>(
    archive: &Dat2Archive,
    vfs: &dyn Vfs,
    loose: &BTreeSet<&'a str>,
    unseen: &mut BTreeSet<&'a str>,
    counts: &mut ArchiveCounts,
) {
    counts.dat2 = true;
    let mut members: Vec<_> = archive.entries().iter().collect();
    members.sort_by_key(|entry| entry.offset);
    for entry in members {
        counts.members += 1;
        let bytes = archive.extract(entry).map_err(|error| error.to_string());
        compare(vfs, loose, unseen, counts, &entry.path, bytes);
    }
}

/// A SPEC-008 archive's members, in payload order. `dat2` says why it did not
/// open as SPEC-015's. A file that cannot be opened or whose directory tables
/// do not parse counts as one failed member.
fn audit_dat1<'a>(
    archive: &Path,
    dat2: &str,
    vfs: &dyn Vfs,
    loose: &BTreeSet<&'a str>,
    unseen: &mut BTreeSet<&'a str>,
    counts: &mut ArchiveCounts,
) {
    let name = archive.display();
    let opened = File::open(archive)
        .map_err(|error| error.to_string())
        .map(BufReader::new)
        .and_then(|mut reader| {
            let tables = DatTables::read(&mut reader)
                .map_err(|error| format!("{error}; as a SPEC-015 archive: {dat2}"))?;
            Ok((reader, tables))
        });
    let (mut reader, tables) = match opened {
        Ok(opened) => opened,
        Err(error) => {
            counts.failed += 1;
            eprintln!("FAIL {name}: {error}");
            return;
        }
    };
    // Members in payload order, so the stream mostly reads forward.
    let mut members: Vec<_> = tables
        .directories
        .iter()
        .flat_map(|directory| {
            directory
                .files
                .iter()
                .map(move |file| (directory.path(file), file))
        })
        .collect();
    members.sort_by_key(|(_, file)| file.offset);
    for (stored, entry) in members {
        counts.members += 1;
        let stored = String::from_utf8_lossy(&stored).replace('\\', "/");
        let path = match VfsPath::new(&stored) {
            Ok(path) => path,
            Err(error) => {
                counts.failed += 1;
                eprintln!("FAIL {name}: {stored}: {error}");
                continue;
            }
        };
        let bytes = entry
            .read_from(&mut reader)
            .map_err(|error| error.to_string());
        compare(vfs, loose, unseen, counts, &path, bytes);
    }
}

/// Audit `archives` against the mounted loose corpus, each as the format it
/// turns out to be. An archive that cannot be opened or whose directory tables
/// do not parse counts as one failed member, and the remaining archives are
/// still audited.
pub fn audit_dat(vfs: &dyn Vfs, archives: &[&Path]) -> DatAudit {
    let loose: BTreeSet<&str> = vfs.files().map(VfsPath::as_str).collect();
    let mut unseen = loose.clone();
    let mut audit = DatAudit {
        loose_files: loose.len() as u64,
        ..DatAudit::default()
    };
    for &archive in archives {
        let mut counts = ArchiveCounts {
            archive: archive.to_path_buf(),
            ..ArchiveCounts::default()
        };
        match Dat2Archive::open(archive) {
            Ok(dat2) => audit_dat2(&dat2, vfs, &loose, &mut unseen, &mut counts),
            Err(error) => audit_dat1(
                archive,
                &error.to_string(),
                vfs,
                &loose,
                &mut unseen,
                &mut counts,
            ),
        }
        audit.archives.push(counts);
    }
    audit.loose_only = unseen.len() as u64;
    audit
}
