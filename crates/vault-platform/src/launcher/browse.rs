//! The launcher's folder browser: one directory listed with `std::fs`,
//! folders first, navigated with the keys or the pointer.
use super::widgets::List;
use std::path::{Path, PathBuf};

/// One listed entry.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Entry {
    /// `..` for the parent directory.
    pub name: String,
    pub dir: bool,
}

/// `dir`'s entries: `..` first when it has a parent, then the folders and
/// then the files, each sorted by name without regard to case (ties by
/// exact name, so the order never depends on the file system's).
pub fn list(dir: &Path) -> std::io::Result<Vec<Entry>> {
    let mut entries: Vec<Entry> = std::fs::read_dir(dir)?
        .flatten()
        .map(|entry| Entry {
            name: entry.file_name().to_string_lossy().into_owned(),
            // A link to a folder is followed, as a file chooser would.
            dir: entry.path().is_dir(),
        })
        .collect();
    entries.sort_by(|a, b| {
        (!a.dir, a.name.to_lowercase(), &a.name).cmp(&(!b.dir, b.name.to_lowercase(), &b.name))
    });
    if dir.parent().is_some() {
        entries.insert(
            0,
            Entry {
                name: "..".into(),
                dir: true,
            },
        );
    }
    Ok(entries)
}

/// What the browser picks for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Target {
    Field(super::Field),
    Layer,
}

/// An open browser.
#[derive(Debug, Clone)]
pub struct Browser {
    pub dir: PathBuf,
    pub entries: Vec<Entry>,
    pub list: List,
    pub target: Target,
    /// Why the directory could not be listed.
    pub error: Option<String>,
}

impl Browser {
    pub fn open(dir: PathBuf, target: Target) -> Self {
        let mut browser = Self {
            dir: PathBuf::new(),
            entries: Vec::new(),
            list: List::default(),
            target,
            error: None,
        };
        browser.go(dir);
        browser
    }
    /// List `dir`; an unlistable directory shows its error and only `..`.
    pub fn go(&mut self, dir: PathBuf) {
        let dir = std::path::absolute(&dir).unwrap_or(dir);
        match list(&dir) {
            Ok(entries) => {
                self.entries = entries;
                self.error = None;
            }
            Err(error) => {
                self.entries = vec![Entry {
                    name: "..".into(),
                    dir: true,
                }];
                self.error = Some(format!("{}: {error}", dir.display()));
            }
        }
        self.dir = dir;
        self.list = List::default();
    }
    pub fn up(&mut self) {
        if let Some(parent) = self.dir.parent() {
            let child = self
                .dir
                .file_name()
                .map(|name| name.to_string_lossy().into_owned());
            self.go(parent.to_path_buf());
            // The folder just left stays selected.
            if let Some(at) =
                child.and_then(|child| self.entries.iter().position(|entry| entry.name == child))
            {
                self.list.selected = at;
            }
        }
    }
    /// Open the selected folder, or return the selected file.
    pub fn enter(&mut self) -> Option<PathBuf> {
        let entry = self.entries.get(self.list.selected)?.clone();
        if entry.name == ".." {
            self.up();
            None
        } else if entry.dir {
            self.go(self.dir.join(&entry.name));
            None
        } else {
            Some(self.dir.join(&entry.name))
        }
    }
}
