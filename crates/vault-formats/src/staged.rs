//! The staged write of SPEC-008 §3.3 and SPEC-023 §3.9: a batch of file
//! changes that lands whole or not at all. Every put is first written to a
//! `<name>.TMP` beside its target, so a failure there leaves the old files
//! untouched. Only then is the batch committed: each target that exists is
//! renamed to its `.BAK` name, the staged files are renamed into place, and
//! the backups are removed. If a step fails, the staged files are removed
//! and the backups restored.
use std::{
    collections::BTreeSet,
    fs, io,
    path::{Path, PathBuf},
};

/// One change of a batch.
#[derive(Debug, Clone, Copy)]
pub enum StagedOp<'a> {
    Put { target: &'a Path, bytes: &'a [u8] },
    Delete { target: &'a Path },
}

impl StagedOp<'_> {
    fn target(&self) -> &Path {
        match self {
            Self::Put { target, .. } | Self::Delete { target } => target,
        }
    }
}

/// Who owns the `.TMP` and `.BAK` names beside the targets.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StagingNames {
    /// The writer: one found there is a leftover, and is replaced.
    Reserved,
    /// Nobody: one found there is a real file, and another name is used.
    Ordinary,
}

/// A failed step, naming the file it failed on.
#[derive(Debug, thiserror::Error)]
#[error("{}: {source}", path.display())]
pub struct StagedError {
    pub path: PathBuf,
    #[source]
    pub source: io::Error,
}

fn failed(path: &Path) -> impl FnOnce(io::Error) -> StagedError + '_ {
    move |source| StagedError {
        path: path.to_owned(),
        source,
    }
}

/// `candidate(0)`, or the first numbered name after it that no other file of
/// the batch uses and, for [`StagingNames::Ordinary`], that does not exist.
/// Names compare with case folded, as some file systems do.
fn free_name(
    names: StagingNames,
    taken: &mut BTreeSet<String>,
    candidate: impl Fn(usize) -> PathBuf,
) -> PathBuf {
    for number in 0.. {
        let path = candidate(number);
        let existing = names == StagingNames::Ordinary && fs::symlink_metadata(&path).is_ok();
        if !existing && taken.insert(path.to_string_lossy().to_ascii_lowercase()) {
            return path;
        }
    }
    unreachable!("the name counter does not end")
}

fn staging_name(target: &Path, number: usize) -> PathBuf {
    let name = target.file_name().unwrap_or_default().to_string_lossy();
    target.with_file_name(match number {
        0 => format!("{name}.TMP"),
        number => format!("{name}.{number}.TMP"),
    })
}

fn backup_name(target: &Path, number: usize) -> PathBuf {
    target.with_extension(match number {
        0 => "BAK".to_owned(),
        number => format!("{number}.BAK"),
    })
}

/// Apply `ops` as one batch, in the protocol the module describes. Parent
/// folders of a put are created, and stay if the batch fails. `before_staging`
/// runs before the put at each index is written, and an error from it fails
/// the batch as a write would.
pub fn commit(
    ops: &[StagedOp<'_>],
    names: StagingNames,
    mut before_staging: impl FnMut(usize) -> io::Result<()>,
) -> Result<(), StagedError> {
    let mut taken: BTreeSet<String> = ops
        .iter()
        .map(|op| op.target().to_string_lossy().to_ascii_lowercase())
        .collect();
    // What cannot be moved aside is refused before anything is written.
    let mut existing = Vec::new();
    for op in ops {
        match fs::symlink_metadata(op.target()) {
            Ok(metadata) if metadata.is_file() => existing.push(op.target()),
            Ok(_) => {
                return Err(StagedError {
                    path: op.target().to_owned(),
                    source: io::Error::other("not a regular file"),
                });
            }
            Err(error) if error.kind() == io::ErrorKind::NotFound => {}
            Err(error) => return Err(failed(op.target())(error)),
        }
    }

    let mut staged: Vec<(PathBuf, &Path)> = Vec::new();
    let staging = ops.iter().try_for_each(|op| {
        let StagedOp::Put { target, bytes } = *op else {
            return Ok(());
        };
        let path = free_name(names, &mut taken, |number| staging_name(target, number));
        if let Some(parent) = target.parent() {
            fs::create_dir_all(parent).map_err(failed(parent))?;
        }
        before_staging(staged.len()).map_err(failed(&path))?;
        fs::write(&path, bytes).map_err(failed(&path))?;
        staged.push((path, target));
        Ok(())
    });
    let remove_staged = |staged: &[(PathBuf, &Path)]| {
        for (path, _) in staged {
            let _ = fs::remove_file(path);
        }
    };
    if let Err(error) = staging {
        remove_staged(&staged);
        return Err(error);
    }

    let mut backups: Vec<(PathBuf, &Path)> = Vec::new();
    let mut committed: Vec<&Path> = Vec::new();
    let result = existing
        .iter()
        .try_for_each(|target| {
            let backup = free_name(names, &mut taken, |number| backup_name(target, number));
            fs::rename(target, &backup).map_err(failed(target))?;
            backups.push((backup, target));
            Ok(())
        })
        .and_then(|()| {
            staged.iter().try_for_each(|(path, target)| {
                fs::rename(path, target).map_err(failed(target))?;
                committed.push(target);
                Ok(())
            })
        });
    match result {
        Ok(()) => {
            for (backup, _) in backups {
                let _ = fs::remove_file(backup);
            }
            Ok(())
        }
        Err(error) => {
            for target in committed {
                let _ = fs::remove_file(target);
            }
            remove_staged(&staged);
            for (backup, target) in backups {
                let _ = fs::rename(backup, target);
            }
            Err(error)
        }
    }
}
