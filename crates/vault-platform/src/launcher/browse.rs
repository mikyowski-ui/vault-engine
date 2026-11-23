//! The launcher's folder browser: one directory listed with `std::fs`,
//! folders first, navigated with the keys or the pointer, with shortcuts to
//! the folders the launcher remembers and the usual install locations.
//!
//! Finding those locations never walks a tree: each is a fixed name tested
//! for existence, or a path Steam or Heroic wrote in its own settings file.
use super::widgets::List;
use crate::config::{KnownPaths, entry_any_case};
use std::{
    collections::HashSet,
    io::Read,
    path::{MAIN_SEPARATOR, Path, PathBuf},
};

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

/// A folder the browser can jump to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Shortcut {
    pub label: String,
    pub path: PathBuf,
    pub pinned: bool,
}

/// Which folders have been seen. A link and its target are one folder, as
/// `~/.steam/steam` and `~/.local/share/Steam` usually are.
#[derive(Default)]
struct Seen(HashSet<PathBuf>);

impl Seen {
    fn first(&mut self, path: &Path) -> bool {
        self.0
            .insert(std::fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf()))
    }
}

/// A path's last component, or the whole path for a root.
fn name_of(path: &Path) -> String {
    path.file_name().map_or_else(
        || path.display().to_string(),
        |name| name.to_string_lossy().into_owned(),
    )
}

/// A path's last two components: `common` alone would say nothing.
fn tail_of(path: &Path) -> String {
    match path.parent().and_then(Path::file_name) {
        Some(parent) => format!("{}/{}", parent.to_string_lossy(), name_of(path)),
        None => name_of(path),
    }
}

/// The browser's shortcuts: `known`'s pinned folders, then its Fallout 1,
/// Fallout 2 and mod folders, then `common` and `roots`, each folder once
/// under the first of these that names it. A remembered folder that is gone
/// is left out, but a pinned one stays, so that it can be unpinned.
pub fn shortcuts(known: &KnownPaths, common: &[PathBuf], roots: &[PathBuf]) -> Vec<Shortcut> {
    let mut seen = Seen::default();
    let mut list = Vec::new();
    let mut add = |label: String, path: &Path, pinned: bool| {
        if seen.first(path) {
            list.push(Shortcut {
                label,
                path: path.to_path_buf(),
                pinned,
            });
        }
    };
    for path in &known.pinned {
        add(name_of(path), path, true);
    }
    for (kind, paths) in [
        ("Fallout 1", &known.fallout1),
        ("Fallout 2", &known.fallout2),
        ("Mods", &known.mods),
    ] {
        for path in paths.iter().filter(|path| path.is_dir()) {
            add(format!("{kind}: {}", name_of(path)), path, false);
        }
    }
    for path in common {
        add(tail_of(path), path, false);
    }
    for path in roots {
        add(path.display().to_string(), path, false);
    }
    list
}

/// The file-system roots: the existing drive letters on Windows, else `/`.
#[cfg(windows)]
pub fn roots() -> Vec<PathBuf> {
    drive_roots(Path::is_dir)
}

/// The file-system roots: the existing drive letters on Windows, else `/`.
#[cfg(not(windows))]
pub fn roots() -> Vec<PathBuf> {
    vec![PathBuf::from("/")]
}

/// The drive letters whose root `exists` accepts.
#[cfg(any(windows, test))]
fn drive_roots(exists: impl Fn(&Path) -> bool) -> Vec<PathBuf> {
    (b'A'..=b'Z')
        .map(|letter| PathBuf::from(format!("{}:\\", char::from(letter))))
        .filter(|root| exists(root))
        .collect()
}

/// The folder names a game is looked for under, in any case.
const GAME_FOLDERS: [&str; 6] = [
    "Fallout",
    "Fallout 1",
    "Fallout 2",
    "fallout1",
    "fallout2",
    "fallout1in2",
];

/// The most of a launcher's settings file that is read.
const MAX_SETTINGS_TEXT: u64 = 4 << 20;

/// The existing folders found so far, each once.
#[derive(Default)]
struct Found {
    seen: Seen,
    dirs: Vec<PathBuf>,
}

impl Found {
    fn add(&mut self, path: PathBuf) {
        if path.is_dir() && self.seen.first(&path) {
            self.dirs.push(path);
        }
    }
    /// The games directly in `folder`, by name.
    fn games_in(&mut self, folder: &Path) {
        for name in GAME_FOLDERS {
            if let Some(path) = entry_any_case(folder, name, Path::is_dir) {
                self.add(path);
            }
        }
    }
    /// A folder games are installed in: the games first, then the folder.
    fn library(&mut self, folder: PathBuf) {
        self.games_in(&folder);
        self.add(folder);
    }
}

/// The start of the file at `path`, as text.
fn read_settings(path: &Path) -> Option<String> {
    let mut bytes = Vec::new();
    std::fs::File::open(path)
        .ok()?
        .take(MAX_SETTINGS_TEXT)
        .read_to_end(&mut bytes)
        .ok()?;
    Some(String::from_utf8_lossy(&bytes).into_owned())
}

/// The double-quoted strings in `text` with their escapes undone, which is
/// all that is needed of a Steam key-value file or of JSON.
fn quoted(text: &str) -> Vec<String> {
    let mut strings = Vec::new();
    let mut chars = text.chars();
    while let Some(open) = chars.next() {
        if open != '"' {
            continue;
        }
        let mut string = String::new();
        while let Some(next) = chars.next() {
            match next {
                '"' => break,
                '\\' => string.extend(chars.next()),
                other => string.push(other),
            }
        }
        strings.push(string);
    }
    strings
}

/// The library folders a `libraryfolders.vdf` names: `"path" "<folder>"`
/// inside a numbered block, or the older `"1" "<folder>"`.
fn steam_libraries(text: &str) -> Vec<PathBuf> {
    text.lines()
        .filter_map(|line| match quoted(line).as_slice() {
            [key, value] if key.eq_ignore_ascii_case("path") => Some(PathBuf::from(value)),
            [key, value]
                if key.bytes().all(|byte| byte.is_ascii_digit())
                    && Path::new(value).is_absolute() =>
            {
                Some(PathBuf::from(value))
            }
            _ => None,
        })
        .collect()
}

/// Every string following the string `key` in JSON `text`, with a leading
/// `~/` taken from `home`.
fn json_paths(text: &str, key: &str, home: Option<&Path>) -> Vec<PathBuf> {
    quoted(text)
        .windows(2)
        .filter(|pair| pair[0] == key)
        .map(|pair| match (pair[1].strip_prefix("~/"), home) {
            (Some(rest), Some(home)) => home.join(rest),
            _ => PathBuf::from(&pair[1]),
        })
        .collect()
}

/// The usual install folders that exist on this machine, each once, a game's
/// own folder before the library holding it: the XDG data folders, Steam
/// libraries (`steamapps/common`, the extra ones `libraryfolders.vdf` names
/// included), GOG, Heroic, the home's `Games`, macOS's Application Support
/// and Windows's Program Files. `home` and `env` stand for the user's home
/// and environment. Nothing is scanned: a folder absent here is not looked
/// for, and of the games Heroic lists only those named Fallout are taken.
pub fn common_install_dirs(
    home: Option<&Path>,
    env: &dyn Fn(&str) -> Option<String>,
) -> Vec<PathBuf> {
    let var = |name: &str| env(name).filter(|value| !value.is_empty());
    let dir_var = |name: &str| var(name).map(PathBuf::from);
    let in_home = |path: &str| home.map(|home| home.join(path));
    let support = in_home("Library/Application Support");
    let program_files: Vec<PathBuf> = ["ProgramFiles(x86)", "ProgramFiles"]
        .into_iter()
        .filter_map(dir_var)
        .collect();
    let mut found = Found::default();

    let shared = var("XDG_DATA_DIRS").unwrap_or_else(|| "/usr/local/share:/usr/share".into());
    let data_dirs: Vec<PathBuf> = dir_var("XDG_DATA_HOME")
        .or_else(|| in_home(".local/share"))
        .into_iter()
        .chain(std::env::split_paths(&shared))
        .filter(|dir| dir.is_absolute())
        .collect();
    for dir in &data_dirs {
        found.games_in(dir);
        found.library(dir.join("games"));
    }

    let steam = data_dirs
        .iter()
        .chain(&support)
        .chain(&program_files)
        .map(|dir| dir.join("Steam"))
        .chain(
            [
                ".steam/steam",
                ".steam/root",
                ".var/app/com.valvesoftware.Steam/.local/share/Steam",
                "snap/steam/common/.local/share/Steam",
            ]
            .into_iter()
            .filter_map(in_home),
        );
    for root in steam {
        let steamapps = root.join("steamapps");
        found.library(steamapps.join("common"));
        let text = read_settings(&steamapps.join("libraryfolders.vdf"));
        for library in steam_libraries(text.as_deref().unwrap_or_default()) {
            found.library(library.join("steamapps").join("common"));
        }
    }

    let gog = in_home("GOG Games")
        .into_iter()
        .chain(support.iter().map(|dir| dir.join("GOG.com")))
        .chain(
            program_files
                .iter()
                .flat_map(|dir| [dir.join("GOG Galaxy").join("Games"), dir.join("GOG.com")]),
        )
        // `C:` joined to a name would be relative to that drive's folder.
        .chain(
            var("SystemDrive")
                .map(|drive| Path::new(&format!("{drive}{MAIN_SEPARATOR}")).join("GOG Games")),
        );
    for folder in gog {
        found.library(folder);
    }

    let heroic = dir_var("XDG_CONFIG_HOME")
        .or_else(|| in_home(".config"))
        .into_iter()
        .chain(in_home(".var/app/com.heroicgameslauncher.hgl/config"))
        .chain(support.clone())
        .chain(dir_var("APPDATA"))
        .filter(|dir| dir.is_absolute())
        .map(|dir| dir.join("heroic"));
    for dir in heroic {
        let text = read_settings(&dir.join("config.json")).unwrap_or_default();
        for path in json_paths(&text, "defaultInstallPath", home) {
            found.library(path);
        }
        let text = read_settings(&dir.join("gog_store/installed.json")).unwrap_or_default();
        for path in json_paths(&text, "install_path", home) {
            if name_of(&path).to_ascii_lowercase().contains("fallout") {
                found.add(path);
            }
        }
    }

    for path in ["Games/Heroic", "Games", "games"]
        .into_iter()
        .filter_map(in_home)
    {
        found.library(path);
    }
    for dir in support.into_iter().chain(program_files) {
        found.library(dir);
    }
    found.dirs
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
    shortcuts: Vec<Shortcut>,
}

impl Browser {
    pub fn open(dir: PathBuf, target: Target) -> Self {
        let mut browser = Self {
            dir: PathBuf::new(),
            entries: Vec::new(),
            list: List::default(),
            target,
            error: None,
            shortcuts: Vec::new(),
        };
        browser.go(dir);
        browser
    }
    pub fn shortcuts(&self) -> &[Shortcut] {
        &self.shortcuts
    }
    pub fn set_shortcuts(&mut self, shortcuts: Vec<Shortcut>) {
        self.shortcuts = shortcuts;
    }
    /// Go to the shortcut at `at`; `false` when there is none. Going lists
    /// the folder as [`Self::go`] does, an unlistable one with its error.
    pub fn open_shortcut(&mut self, at: usize) -> bool {
        let Some(path) = self.shortcuts.get(at).map(|it| it.path.clone()) else {
            return false;
        };
        self.go(path);
        true
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::{collections::HashMap, fs};
    use tempfile::tempdir;

    /// `path` as a VDF or JSON string holds it: each backslash doubled, so
    /// a Windows path survives the unescaping that reads it back.
    fn escaped(path: &Path) -> String {
        path.display().to_string().replace('\\', "\\\\")
    }

    fn make(path: &Path) -> PathBuf {
        fs::create_dir_all(path).unwrap();
        path.to_path_buf()
    }

    /// Common folders under a synthetic `root`, with the system's shared
    /// data folders replaced so that nothing outside `root` is looked at.
    fn common(root: &Path, vars: &[(&str, &Path)]) -> Vec<PathBuf> {
        let mut map: HashMap<String, String> = vars
            .iter()
            .map(|(key, value)| ((*key).to_owned(), value.display().to_string()))
            .collect();
        map.entry("XDG_DATA_DIRS".to_owned())
            .or_insert_with(|| root.join("no-shared-data").display().to_string());
        common_install_dirs(Some(root), &|key| map.get(key).cloned())
    }

    #[test]
    fn shortcuts_run_from_pinned_to_common_each_shown_once() {
        let temp = tempdir().unwrap();
        let dir = |name: &str| make(&temp.path().join(name));
        let (pin, one, two, mods) = (dir("pin"), dir("one"), dir("two"), dir("mods"));
        let (steam, root) = (dir("steam/common"), dir("root"));
        let known = KnownPaths {
            pinned: vec![pin.clone()],
            fallout1: vec![one.clone(), pin.clone()],
            fallout2: vec![two.clone(), one.clone()],
            mods: vec![mods.clone()],
        };
        let common = [two.clone(), steam.clone()];
        let roots = [root.clone(), pin.clone()];

        let list = shortcuts(&known, &common, &roots);
        let paths: Vec<&Path> = list.iter().map(|it| it.path.as_path()).collect();
        assert_eq!(paths, [&pin, &one, &two, &mods, &steam, &root]);
        let labels: Vec<&str> = list.iter().map(|it| it.label.as_str()).collect();
        let root_label = root.display().to_string();
        assert_eq!(
            labels,
            [
                "pin",
                "Fallout 1: one",
                "Fallout 2: two",
                "Mods: mods",
                "steam/common",
                root_label.as_str(),
            ]
        );
        assert_eq!(
            list.iter().map(|it| it.pinned).collect::<Vec<_>>(),
            [true, false, false, false, false, false]
        );
    }

    #[test]
    fn a_folder_that_is_gone_is_left_out_unless_pinned() {
        let temp = tempdir().unwrap();
        let gone = temp.path().join("gone");
        let known = KnownPaths {
            pinned: vec![temp.path().join("unplugged")],
            fallout1: vec![gone.clone()],
            mods: vec![gone],
            ..KnownPaths::default()
        };
        let list = shortcuts(&known, &[], &[]);
        assert_eq!(list.len(), 1);
        assert!(list[0].pinned && list[0].label == "unplugged");
    }

    #[cfg(unix)]
    #[test]
    fn a_link_and_its_target_are_one_shortcut() {
        let temp = tempdir().unwrap();
        let real = make(&temp.path().join("real"));
        let link = temp.path().join("link");
        std::os::unix::fs::symlink(&real, &link).unwrap();
        let known = KnownPaths {
            pinned: vec![link.clone()],
            ..KnownPaths::default()
        };
        let list = shortcuts(&known, &[real], &[]);
        assert_eq!(list.len(), 1);
        assert_eq!(list[0].path, link);
    }

    #[test]
    fn shortcuts_leave_key_and_pointer_navigation_as_it_was() {
        let temp = tempdir().unwrap();
        let start = make(&temp.path().join("start"));
        make(&start.join("child"));
        let other = make(&temp.path().join("other"));
        make(&other.join("inner"));

        let mut browser = Browser::open(start.clone(), Target::Layer);
        browser.list.selected = 1;
        browser.set_shortcuts(shortcuts(
            &KnownPaths {
                pinned: vec![other.clone()],
                ..KnownPaths::default()
            },
            &[],
            &[],
        ));
        assert_eq!(browser.shortcuts().len(), 1);
        assert_eq!(browser.list.selected, 1);
        assert_eq!(browser.dir, start);

        assert_eq!(browser.enter(), None);
        assert_eq!(browser.dir, start.join("child"));
        browser.up();
        assert_eq!(browser.dir, start);

        assert!(browser.open_shortcut(0));
        assert_eq!(browser.dir, other);
        assert_eq!(browser.list.selected, 0);
        assert!(browser.entries.iter().any(|entry| entry.name == "inner"));
        assert!(!browser.open_shortcut(1));
        assert_eq!(browser.dir, other);
        // Shortcuts outlive the folder they were set in.
        assert_eq!(browser.shortcuts().len(), 1);
    }

    #[test]
    fn a_shortcut_to_a_missing_folder_shows_the_error() {
        let temp = tempdir().unwrap();
        let mut browser = Browser::open(temp.path().to_path_buf(), Target::Layer);
        browser.set_shortcuts(vec![Shortcut {
            label: "gone".into(),
            path: temp.path().join("gone"),
            pinned: true,
        }]);
        assert!(browser.open_shortcut(0));
        assert!(browser.error.is_some());
        browser.up();
        assert_eq!(browser.dir, temp.path());
        assert_eq!(browser.error, None);
    }

    #[test]
    fn roots_are_the_existing_drive_letters() {
        let drives = drive_roots(|root| matches!(root.to_str(), Some("C:\\" | "E:\\")));
        assert_eq!(drives, [PathBuf::from("C:\\"), PathBuf::from("E:\\")]);
        #[cfg(not(windows))]
        assert_eq!(roots(), [PathBuf::from("/")]);
    }

    #[test]
    fn nothing_is_listed_when_nothing_exists() {
        let temp = tempdir().unwrap();
        assert_eq!(common(temp.path(), &[]), Vec::<PathBuf>::new());
        // Without a home, only what the environment names is looked at.
        let shared = temp.path().join("shared").display().to_string();
        let env = |key: &str| (key == "XDG_DATA_DIRS").then(|| shared.clone());
        assert_eq!(common_install_dirs(None, &env), Vec::<PathBuf>::new());
    }

    #[test]
    fn steam_libraries_are_read_from_the_vdf_file_alone() {
        let temp = tempdir().unwrap();
        let home = temp.path();
        let steamapps = make(&home.join(".local/share/Steam/steamapps"));
        let common_dir = make(&steamapps.join("common"));
        let game = make(&common_dir.join("FALLOUT 2"));
        make(&common_dir.join("Other Game"));
        let extra = make(&home.join("mnt/SteamLibrary/steamapps/common"));
        let old_form = make(&home.join("mnt/Old/steamapps/common"));
        // A library holding no `steamapps/common` is no shortcut.
        make(&home.join("mnt/Empty"));
        let vdf = format!(
            "\"libraryfolders\"\n{{\n\t\"0\"\n\t{{\n\t\t\"path\"\t\t\"{}\"\n\t\t\
             \"contentid\"\t\t\"5\"\n\t\t\"apps\"\n\t\t{{\n\t\t\t\"38400\"\t\t\"123\"\n\t\t}}\n\t}}\n\
             \t\"1\"\n\t{{\n\t\t\"path\"\t\t\"{}\"\n\t}}\n\t\"2\"\t\t\"{}\"\n\
             \t\"3\"\n\t{{\n\t\t\"path\"\t\t\"{}\"\n\t}}\n\
             \t\"4\"\n\t{{\n\t\t\"path\"\t\t\"{}\"\n\t}}\n}}\n",
            escaped(&home.join(".local/share/Steam")),
            escaped(&home.join("mnt/SteamLibrary")),
            escaped(&home.join("mnt/Old")),
            escaped(&home.join("mnt/Empty")),
            escaped(&home.join("mnt/Missing")),
        );
        fs::write(steamapps.join("libraryfolders.vdf"), vdf).unwrap();

        assert_eq!(common(home, &[]), [game, common_dir, extra, old_form]);
    }

    #[cfg(unix)]
    #[test]
    fn the_steam_link_in_home_is_not_listed_twice() {
        let temp = tempdir().unwrap();
        let home = temp.path();
        let steam = make(&home.join(".local/share/Steam"));
        let common_dir = make(&steam.join("steamapps/common"));
        make(&home.join(".steam"));
        std::os::unix::fs::symlink(&steam, home.join(".steam/steam")).unwrap();
        std::os::unix::fs::symlink(&steam, home.join(".steam/root")).unwrap();
        assert_eq!(common(home, &[]), [common_dir]);
    }

    #[test]
    fn steam_key_values_undo_their_escapes_and_skip_other_numbers() {
        // The old form (a number for a key) counts only with an absolute
        // path, and what is absolute depends on the platform.
        let old = if cfg!(windows) {
            "E:\\old\\library"
        } else {
            "/old/library"
        };
        let text = format!(
            "\t\"path\"\t\t\"D:\\\\Steam Library\"\n\
             \t\"PATH\"\t\t\"/mnt/games\"\n\
             \t\"1\"\t\t\"{}\"\n\
             \t\"38400\"\t\t\"123\"\n\
             \t\"label\"\t\t\"/not/this\"\n\
             \t\"0\"\n\t{{\n",
            escaped(Path::new(old)),
        );
        assert_eq!(
            steam_libraries(&text),
            [
                PathBuf::from("D:\\Steam Library"),
                PathBuf::from("/mnt/games"),
                PathBuf::from(old),
            ]
        );
    }

    #[test]
    fn gog_folders_are_found_with_the_games_in_them() {
        let temp = tempdir().unwrap();
        let home = temp.path();
        let gog = make(&home.join("GOG Games"));
        let fallout = make(&gog.join("Fallout"));
        let fallout2 = make(&gog.join("Fallout 2"));
        make(&gog.join("Another Game"));
        assert_eq!(common(home, &[]), [fallout, fallout2, gog]);
    }

    #[test]
    fn heroic_settings_name_the_install_folders() {
        let temp = tempdir().unwrap();
        let home = temp.path();
        let library = make(&home.join("Library Disk/Heroic"));
        let game = make(&library.join("Fallout"));
        let elsewhere = make(&home.join("Elsewhere/FALLOUT 2 GOG"));
        make(&home.join("Elsewhere/Another Game"));
        let config = make(&home.join(".config/heroic"));
        fs::write(
            config.join("config.json"),
            format!(
                "{{\"defaultSettings\":{{\"language\":\"en\",\"defaultInstallPath\":\"{}\"}}}}",
                escaped(&library)
            ),
        )
        .unwrap();
        make(&config.join("gog_store"));
        fs::write(
            config.join("gog_store/installed.json"),
            format!(
                "{{\"installed\":[{{\"install_path\":\"{}\"}},{{\"install_path\":\"{}\"}},\
                 {{\"install_path\":\"{}\"}}]}}",
                escaped(&elsewhere),
                escaped(&home.join("Elsewhere/Another Game")),
                escaped(&home.join("Elsewhere/Fallout Missing")),
            ),
        )
        .unwrap();
        assert_eq!(common(home, &[]), [game, library, elsewhere]);
    }

    #[test]
    fn heroics_conventional_folder_needs_no_settings() {
        let temp = tempdir().unwrap();
        let home = temp.path();
        let heroic = make(&home.join("Games/Heroic"));
        let fallout = make(&heroic.join("Fallout 2"));
        let games = home.join("Games");
        assert_eq!(common(home, &[]), [fallout, heroic, games]);
    }

    #[test]
    fn a_tilde_in_a_heroic_path_is_the_home() {
        let home = Path::new("/home/someone");
        assert_eq!(
            json_paths("{\"key\": \"~/Games\"}", "key", Some(home)),
            [home.join("Games")]
        );
        assert_eq!(
            json_paths("{\"key\": \"~/Games\"}", "key", None),
            [PathBuf::from("~/Games")]
        );
    }

    #[test]
    fn xdg_folders_come_from_the_environment() {
        let temp = tempdir().unwrap();
        let home = temp.path();
        let data_home = make(&home.join("xdg-home"));
        let game = make(&data_home.join("Fallout 2"));
        let shared = make(&home.join("xdg-shared/games"));
        // Not read: the variable moves it.
        make(&home.join(".local/share/games"));
        let vars = [
            ("XDG_DATA_HOME", data_home.as_path()),
            ("XDG_DATA_DIRS", &home.join("xdg-shared")),
        ];
        assert_eq!(common(home, &vars), [game, shared]);
    }

    #[test]
    fn a_relative_xdg_folder_is_ignored() {
        let temp = tempdir().unwrap();
        let vars = [("XDG_DATA_HOME", Path::new("relative"))];
        assert_eq!(common(temp.path(), &vars), Vec::<PathBuf>::new());
    }

    #[test]
    fn application_support_and_program_files_are_listed() {
        let temp = tempdir().unwrap();
        let home = temp.path();
        let support = make(&home.join("Library/Application Support"));
        let mac_steam = make(&support.join("Steam/steamapps/common"));
        let mac_gog = make(&support.join("GOG.com"));
        let x86 = make(&home.join("Program Files (x86)"));
        let win_steam = make(&x86.join("Steam/steamapps/common"));
        let galaxy = make(&x86.join("GOG Galaxy/Games"));
        let plain = make(&home.join("Program Files"));
        let drive = make(&home.join("C"));
        let root_gog = make(&drive.join("GOG Games"));
        let vars = [
            ("ProgramFiles(x86)", x86.as_path()),
            ("ProgramFiles", plain.as_path()),
            ("SystemDrive", drive.as_path()),
        ];
        assert_eq!(
            common(home, &vars),
            [
                mac_steam, win_steam, mac_gog, galaxy, root_gog, support, x86, plain
            ]
        );
    }
}
