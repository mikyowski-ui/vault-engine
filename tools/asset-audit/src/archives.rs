//! SPEC-008 archive audit: stream every member of the master and critter
//! databases (only the directory tables and one member are held at a time)
//! and compare its SHA-256 with the loose file at the same path in the
//! unpacked corpus.
use std::{
    collections::BTreeSet,
    fs::File,
    io::BufReader,
    path::{Path, PathBuf},
};

use sha2::{Digest, Sha256};
use vault_formats::{MAX_ASSET_BYTES, Vfs, VfsPath, dat::DatTables};

/// One archive's results.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct ArchiveCounts {
    pub archive: PathBuf,
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
                "DAT {}: {} members, {} matched, {} differ, {} missing loose, {} failed",
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

/// Audit `archives` against the mounted loose corpus. An archive that
/// cannot be opened or whose directory tables do not parse counts as one
/// failed member, and the remaining archives are still audited.
pub fn audit_dat(vfs: &dyn Vfs, archives: &[&Path]) -> DatAudit {
    let loose: BTreeSet<&str> = vfs.files().map(VfsPath::as_str).collect();
    let mut unseen = loose.clone();
    let mut audit = DatAudit {
        loose_files: loose.len() as u64,
        ..DatAudit::default()
    };
    for &archive in archives {
        let name = archive.display();
        let mut counts = ArchiveCounts {
            archive: archive.to_path_buf(),
            ..ArchiveCounts::default()
        };
        let opened = File::open(archive)
            .map_err(|error| error.to_string())
            .map(BufReader::new)
            .and_then(|mut reader| {
                let tables = DatTables::read(&mut reader).map_err(|error| error.to_string())?;
                Ok((reader, tables))
            });
        let (mut reader, tables) = match opened {
            Ok(opened) => opened,
            Err(error) => {
                counts.failed += 1;
                eprintln!("FAIL {name}: {error}");
                audit.archives.push(counts);
                continue;
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
            let bytes = match entry.read_from(&mut reader) {
                Ok(bytes) => bytes,
                Err(error) => {
                    counts.failed += 1;
                    eprintln!("FAIL {name}: {path}: {error}");
                    continue;
                }
            };
            if !loose.contains(path.as_str()) {
                counts.missing += 1;
                eprintln!("MISSING {name}: {path}: no loose file");
                continue;
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
        audit.archives.push(counts);
    }
    audit.loose_only = unseen.len() as u64;
    audit
}
