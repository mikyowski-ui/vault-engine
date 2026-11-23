//! `cargo xtask install-desktop`: put the release build in the Linux desktop
//! menu for the current user. Three files, and `uninstall` removes exactly
//! those three: the Desktop Entry, a 256 pixel hicolor icon drawn by
//! `vault-platform/assets/icon.rs`, and the binary, named `vault-engine`.
//!
//! The task never builds. It takes the release build as it finds it, and asks
//! for a new one when that is missing or older than the sources cargo says
//! went into it.
use std::{
    env,
    ffi::{OsStr, OsString},
    fmt, fs, io,
    path::{Path, PathBuf},
    process::{Command, Stdio},
};

use vault_formats::inflate::adler32;

#[path = "../../../crates/vault-platform/assets/icon.rs"]
mod icon;

/// The name of the installed binary, icon and desktop entry, which is also
/// the window's application ID and class.
const NAME: &str = "vault-engine";
/// The release build, from the workspace root.
const BUILT: &str = "target/release/vault-platform";
const BUILD_COMMAND: &str = "cargo build --release --package vault-platform";
const ICON_SIDE: u32 = 256;
const PNG_SIGNATURE: &[u8] = b"\x89PNG\r\n\x1a\n";
/// The menu's actions: the ID, the name shown, and the arguments.
const ACTIONS: [(&str, &str, &str); 3] = [
    (
        "fallout1",
        "Launch Fallout 1",
        "--game-profile fallout1 --game",
    ),
    (
        "fallout2",
        "Launch Fallout 2",
        "--game-profile fallout2 --game",
    ),
    ("ettu", "Launch Et Tu", "--profile ettu --game"),
];

/// Where the files go.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InstallRoot {
    /// `$XDG_DATA_HOME`, else `~/.local/share`.
    pub data: PathBuf,
    /// `~/.local/bin`.
    pub bin: PathBuf,
}

impl InstallRoot {
    /// The current user's folders, from the environment.
    pub fn for_user() -> Result<Self, String> {
        Self::resolve(env::var_os("HOME"), env::var_os("XDG_DATA_HOME"))
    }
    /// The XDG Base Directory rule: an unset, empty or relative
    /// `XDG_DATA_HOME` falls back to the default.
    fn resolve(home: Option<OsString>, xdg_data: Option<OsString>) -> Result<Self, String> {
        let home = home
            .filter(|home| !home.is_empty())
            .map(PathBuf::from)
            .ok_or("HOME is not set, so there is no folder to install into")?;
        let data = xdg_data
            .map(PathBuf::from)
            .filter(|data| data.is_absolute())
            .unwrap_or_else(|| home.join(".local/share"));
        Ok(Self {
            data,
            bin: home.join(".local/bin"),
        })
    }
    pub fn desktop_file(&self) -> PathBuf {
        self.data.join(format!("applications/{NAME}.desktop"))
    }
    pub fn icon_file(&self) -> PathBuf {
        self.data
            .join(format!("icons/hicolor/256x256/apps/{NAME}.png"))
    }
    pub fn binary_file(&self) -> PathBuf {
        self.bin.join(NAME)
    }
}

/// What an install wrote.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Report {
    pub desktop: PathBuf,
    pub icon: PathBuf,
    pub binary: PathBuf,
}

impl fmt::Display for Report {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        writeln!(formatter, "Wrote:")?;
        for path in [&self.binary, &self.icon, &self.desktop] {
            writeln!(formatter, "  {}", path.display())?;
        }
        Ok(())
    }
}

/// Install `binary` and `icon_png` under `root`. A failure before the first
/// write leaves nothing behind. The desktop entry goes last, so a menu never
/// offers a program that is not there yet.
pub fn install(root: &InstallRoot, binary: &Path, icon_png: &[u8]) -> Result<Report, String> {
    if !binary.is_file() {
        return Err(format!("{} is not a file", binary.display()));
    }
    if !icon_png.starts_with(PNG_SIGNATURE) {
        return Err("the icon is not a PNG".to_owned());
    }
    let installed = std::path::absolute(root.binary_file())
        .map_err(|error| format!("cannot place {}: {error}", root.bin.display()))?;
    let entry = desktop_entry(&installed)?;
    let (desktop, icon) = (root.desktop_file(), root.icon_file());
    replace_file(&installed, 0o755, |temp| fs::copy(binary, temp).map(|_| ()))?;
    replace_file(&icon, 0o644, |temp| fs::write(temp, icon_png))?;
    replace_file(&desktop, 0o644, |temp| fs::write(temp, &entry))?;
    Ok(Report {
        desktop,
        icon,
        binary: installed,
    })
}

/// Remove the three files `install` writes and nothing else, the desktop
/// entry first. Returns those that were there, so a second run is harmless.
pub fn uninstall(root: &InstallRoot) -> Result<Vec<PathBuf>, String> {
    let mut removed = Vec::new();
    for path in [root.desktop_file(), root.icon_file(), root.binary_file()] {
        match fs::remove_file(&path) {
            Ok(()) => removed.push(path),
            Err(error) if error.kind() == io::ErrorKind::NotFound => {}
            Err(error) => return Err(format!("cannot remove {}: {error}", path.display())),
        }
    }
    Ok(removed)
}

/// `cargo xtask install-desktop`.
pub fn run() -> Result<(), String> {
    if !cfg!(target_os = "linux") {
        println!("install-desktop is Linux-only; nothing was installed.");
        return Ok(());
    }
    let workspace = workspace()?;
    let binary = workspace.join(BUILT);
    check_built(&workspace, &binary)?;
    let root = InstallRoot::for_user()?;
    let report = install(&root, &binary, &png(ICON_SIDE, &icon::rgba(ICON_SIDE))?)?;
    print!("{report}");
    for note in refresh(&root) {
        println!("{note}");
    }
    println!("{}", path_note(&root.bin, env::var_os("PATH").as_deref()));
    Ok(())
}

/// `cargo xtask install-desktop --uninstall`.
pub fn run_uninstall() -> Result<(), String> {
    if !cfg!(target_os = "linux") {
        println!("install-desktop is Linux-only; nothing was removed.");
        return Ok(());
    }
    let root = InstallRoot::for_user()?;
    let removed = uninstall(&root)?;
    if removed.is_empty() {
        println!("Nothing to remove: Vault Engine is not installed.");
        return Ok(());
    }
    for path in removed {
        println!("Removed {}", path.display());
    }
    for note in refresh(&root) {
        println!("{note}");
    }
    Ok(())
}

fn workspace() -> Result<PathBuf, String> {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .map(Path::to_path_buf)
        .ok_or_else(|| "xtask is not inside a workspace".to_owned())
}

/// The release build is there and no older than its sources, by the list
/// cargo wrote beside it. That list, unlike a walk of `crates/`, leaves out
/// what the binary does not contain (test files, dev-only crates), so a
/// fresh build always passes.
fn check_built(workspace: &Path, binary: &Path) -> Result<(), String> {
    let build = |problem: String| format!("{problem}; run `{BUILD_COMMAND}` first");
    let modified = |path: &Path| fs::metadata(path).and_then(|data| data.modified());
    let Ok(built) = modified(binary) else {
        return Err(build(format!("{} is missing", binary.display())));
    };
    let list = binary.with_extension("d");
    let text = fs::read_to_string(&list)
        .map_err(|error| build(format!("cannot read {}: {error}", list.display())))?;
    let sources = dep_info_sources(&text);
    if sources.is_empty() {
        return Err(build(format!("{} lists no sources", list.display())));
    }
    for source in sources {
        let source = workspace.join(source);
        match modified(&source) {
            Ok(time) if time <= built => {}
            Ok(_) => {
                return Err(build(format!(
                    "{} is older than {}",
                    binary.display(),
                    source.display()
                )));
            }
            Err(_) => {
                return Err(build(format!(
                    "{} was built from {}, which is gone",
                    binary.display(),
                    source.display()
                )));
            }
        }
    }
    Ok(())
}

/// The sources on a Makefile-style dependency line, `target: a b\ c`.
fn dep_info_sources(text: &str) -> Vec<PathBuf> {
    let line = text.lines().next().unwrap_or_default();
    let Some((_, sources)) = line.split_once(": ") else {
        return Vec::new();
    };
    let mut paths = vec![String::new()];
    let mut chars = sources.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '\\' if chars.peek() == Some(&' ') => {
                chars.next();
                paths.last_mut().expect("never empty").push(' ');
            }
            ' ' => paths.push(String::new()),
            c => paths.last_mut().expect("never empty").push(c),
        }
    }
    paths
        .into_iter()
        .filter(|path| !path.is_empty())
        .map(PathBuf::from)
        .collect()
}

/// The Desktop Entry for an installed `binary`.
fn desktop_entry(binary: &Path) -> Result<String, String> {
    let program = binary
        .to_str()
        .ok_or_else(|| format!("{} is not UTF-8", binary.display()))?;
    let exec = escape_value(&quote_argument(program));
    let actions: String = ACTIONS.iter().map(|(id, ..)| format!("{id};")).collect();
    let mut text = format!(
        "[Desktop Entry]\n\
         Type=Application\n\
         Name=Vault Engine\n\
         Exec={exec}\n\
         Icon={NAME}\n\
         Terminal=false\n\
         StartupWMClass={NAME}\n\
         Categories=Game;RolePlaying;\n\
         Actions={actions}\n"
    );
    for (id, name, arguments) in ACTIONS {
        text.push_str(&format!(
            "\n[Desktop Action {id}]\nName={name}\nExec={exec} {arguments}\n"
        ));
    }
    Ok(text)
}

/// An `Exec` argument: `%` doubled, because it starts a field code, and in
/// double quotes when it holds a reserved character, with `"`, `` ` ``, `$`
/// and `\` escaped inside them (Desktop Entry Specification, "The Exec key").
fn quote_argument(argument: &str) -> String {
    const RESERVED: &str = " \t\n\"'\\><~|&;$*?#()`";
    let literal = argument.replace('%', "%%");
    if !argument.contains(|c| RESERVED.contains(c)) {
        return literal;
    }
    let mut quoted = String::from("\"");
    for c in literal.chars() {
        if matches!(c, '"' | '`' | '$' | '\\') {
            quoted.push('\\');
        }
        quoted.push(c);
    }
    quoted.push('"');
    quoted
}

/// A string value's own escapes, which a reader undoes before it looks at
/// any `Exec` quoting (Desktop Entry Specification, "Possible value types").
fn escape_value(value: &str) -> String {
    value
        .replace('\\', "\\\\")
        .replace('\n', "\\n")
        .replace('\t', "\\t")
        .replace('\r', "\\r")
}

/// Put a file at `target` by writing a temporary name beside it and renaming
/// that over it. A running copy keeps its old file, and a failed write leaves
/// the old one in place. The temporary name is hidden and does not end in
/// `.desktop`, so no menu reads it.
fn replace_file(
    target: &Path,
    mode: u32,
    write: impl FnOnce(&Path) -> io::Result<()>,
) -> Result<(), String> {
    let cannot = |error: io::Error| format!("cannot write {}: {error}", target.display());
    let folder = target
        .parent()
        .ok_or_else(|| format!("{} has no folder", target.display()))?;
    fs::create_dir_all(folder).map_err(cannot)?;
    let name = target.file_name().unwrap_or(OsStr::new(NAME));
    let temp = folder.join(format!(
        ".{}.{}.tmp",
        name.to_string_lossy(),
        std::process::id()
    ));
    let written = write(&temp)
        .and_then(|()| set_mode(&temp, mode))
        .and_then(|()| fs::rename(&temp, target));
    written.map_err(|error| {
        // The temporary file may never have been made.
        let _ = fs::remove_file(&temp);
        cannot(error)
    })
}

#[cfg(unix)]
fn set_mode(path: &Path, mode: u32) -> io::Result<()> {
    use std::os::unix::fs::PermissionsExt;
    fs::set_permissions(path, fs::Permissions::from_mode(mode))
}

#[cfg(not(unix))]
fn set_mode(_: &Path, _: u32) -> io::Result<()> {
    Ok(())
}

/// A best-effort refresh of the desktop's caches, as lines for the user.
fn refresh(root: &InstallRoot) -> Vec<String> {
    let applications = root.data.join("applications");
    let icons = root.data.join("icons/hicolor");
    let force = OsStr::new("-f");
    let no_theme_file = OsStr::new("-t");
    vec![
        best_effort("update-desktop-database", &[applications.as_os_str()]),
        best_effort(
            "gtk-update-icon-cache",
            &[force, no_theme_file, icons.as_os_str()],
        ),
    ]
}

/// Run a tool the install can do without, and say how it went.
fn best_effort(program: &str, arguments: &[&OsStr]) -> String {
    let status = Command::new(program)
        .args(arguments)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status();
    match status {
        Ok(status) if status.success() => format!("Refreshed with {program}."),
        Ok(status) => format!("{program} did not finish ({status}); not needed, skipped."),
        Err(error) if error.kind() == io::ErrorKind::NotFound => {
            format!("{program} is not installed; not needed, skipped.")
        }
        Err(error) => format!("{program} did not start ({error}); not needed, skipped."),
    }
}

/// What a terminal launch needs. The menu uses the absolute path.
fn path_note(bin: &Path, path: Option<&OsStr>) -> String {
    let found = path.is_some_and(|path| env::split_paths(path).any(|folder| folder == bin));
    if found {
        format!(
            "{} is on PATH, so `{NAME}` also runs from a terminal.",
            bin.display()
        )
    } else {
        format!(
            "{} must be on PATH for a terminal launch; the menu entry works without it.",
            bin.display()
        )
    }
}

/// The CRC-32 of PNG chunks (ISO 3309), bit by bit: an icon is small.
fn crc32(bytes: &[u8]) -> u32 {
    let mut crc = !0u32;
    for byte in bytes {
        crc ^= u32::from(*byte);
        for _ in 0..8 {
            crc = if crc & 1 != 0 {
                (crc >> 1) ^ 0xEDB8_8320
            } else {
                crc >> 1
            };
        }
    }
    !crc
}

/// `data` in a zlib stream of stored DEFLATE blocks (RFC 1950, RFC 1951).
fn zlib_stored(data: &[u8]) -> Vec<u8> {
    const BLOCK: usize = u16::MAX as usize;
    let mut out = vec![0x78, 0x01];
    let blocks = data.len().div_ceil(BLOCK).max(1);
    let mut rest = data;
    for block in 1..=blocks {
        let (head, tail) = rest.split_at(rest.len().min(BLOCK));
        let length = head.len() as u16;
        out.push(u8::from(block == blocks));
        out.extend(length.to_le_bytes());
        out.extend((!length).to_le_bytes());
        out.extend(head);
        rest = tail;
    }
    out.extend(adler32(data).to_be_bytes());
    out
}

/// A square 8-bit RGBA PNG. It is stored, not compressed: the file is written
/// once, and a few hundred kilobytes do not matter on the desktop.
fn png(side: u32, rgba: &[u8]) -> Result<Vec<u8>, String> {
    let row = side as usize * 4;
    if side == 0 || rgba.len() != row * side as usize {
        return Err(format!(
            "{} bytes are not a {side}x{side} RGBA image",
            rgba.len()
        ));
    }
    let mut out = PNG_SIGNATURE.to_vec();
    let mut chunk = |kind: &[u8; 4], data: &[u8]| {
        out.extend((data.len() as u32).to_be_bytes());
        let start = out.len();
        out.extend(kind);
        out.extend(data);
        let crc = crc32(&out[start..]);
        out.extend(crc.to_be_bytes());
    };
    let mut header = Vec::with_capacity(13);
    header.extend(side.to_be_bytes());
    header.extend(side.to_be_bytes());
    // Bit depth 8, color type 6 (RGBA), deflate, filter 0, no interlace.
    header.extend([8, 6, 0, 0, 0]);
    chunk(b"IHDR", &header);
    let mut rows = Vec::with_capacity((row + 1) * side as usize);
    for pixels in rgba.chunks_exact(row) {
        rows.push(0);
        rows.extend(pixels);
    }
    chunk(b"IDAT", &zlib_stored(&rows));
    chunk(b"IEND", &[]);
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{Duration, SystemTime};
    use vault_formats::inflate::zlib_decompress;

    /// A scratch folder, with a space in its name, removed at the end.
    struct Scratch(PathBuf);

    impl Scratch {
        fn new(name: &str) -> Self {
            let path = env::temp_dir().join(format!("xtask desktop {}-{name}", std::process::id()));
            let _ = fs::remove_dir_all(&path);
            fs::create_dir_all(&path).unwrap();
            Self(path)
        }
        fn root(&self) -> InstallRoot {
            InstallRoot {
                data: self.0.join("data"),
                bin: self.0.join("bin"),
            }
        }
        /// A stand-in for the release build.
        fn binary(&self, text: &str) -> PathBuf {
            let path = self.0.join("built");
            fs::write(&path, text).unwrap();
            path
        }
    }

    impl Drop for Scratch {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    /// Every file under `folder`, relative to it, sorted.
    fn files(folder: &Path) -> Vec<String> {
        fn walk(folder: &Path, base: &Path, out: &mut Vec<String>) {
            for entry in fs::read_dir(folder).into_iter().flatten().flatten() {
                let path = entry.path();
                if path.is_dir() {
                    walk(&path, base, out);
                } else {
                    let relative = path.strip_prefix(base).unwrap();
                    out.push(relative.display().to_string());
                }
            }
        }
        let mut out = Vec::new();
        walk(folder, folder, &mut out);
        out.sort();
        out
    }

    fn fake_png() -> Vec<u8> {
        png(2, &[1; 16]).unwrap()
    }

    /// The value of `key` in `section`, as a reader sees the raw line.
    fn field<'a>(entry: &'a str, section: &str, key: &str) -> Option<&'a str> {
        let header = format!("[{section}]");
        entry
            .lines()
            .skip_while(|line| *line != header)
            .skip(1)
            .take_while(|line| !line.starts_with('['))
            .find_map(|line| line.strip_prefix(key)?.strip_prefix('='))
    }

    /// Check the PNG's signature, every chunk's CRC and the IHDR, and return
    /// the decompressed rows.
    fn decode(file: &[u8], side: u32) -> Vec<u8> {
        assert_eq!(&file[..8], PNG_SIGNATURE);
        let mut at = 8;
        let mut kinds = Vec::new();
        let mut idat = Vec::new();
        while at < file.len() {
            let length = u32::from_be_bytes(file[at..at + 4].try_into().unwrap()) as usize;
            let body = &file[at + 4..at + 8 + length];
            let stored = &file[at + 8 + length..at + 12 + length];
            assert_eq!(crc32(body), u32::from_be_bytes(stored.try_into().unwrap()));
            let (kind, data) = body.split_at(4);
            kinds.push(String::from_utf8_lossy(kind).into_owned());
            match kind {
                b"IHDR" => {
                    assert_eq!(&data[..4], side.to_be_bytes());
                    assert_eq!(&data[4..8], side.to_be_bytes());
                    assert_eq!(&data[8..], [8, 6, 0, 0, 0]);
                }
                b"IDAT" => idat.extend(data),
                _ => {}
            }
            at += 12 + length;
        }
        assert_eq!(kinds, ["IHDR", "IDAT", "IEND"]);
        let size = (side as usize * 4 + 1) * side as usize;
        zlib_decompress(&idat, size).unwrap()
    }

    #[test]
    fn the_entry_has_each_field() {
        let entry = desktop_entry(Path::new("/home/a/.local/bin/vault-engine")).unwrap();
        let main = "Desktop Entry";
        let exec = "/home/a/.local/bin/vault-engine";
        assert!(entry.starts_with("[Desktop Entry]\n"));
        for (key, value) in [
            ("Type", "Application"),
            ("Name", "Vault Engine"),
            ("Exec", exec),
            ("Icon", "vault-engine"),
            ("Terminal", "false"),
            ("StartupWMClass", "vault-engine"),
            ("Categories", "Game;RolePlaying;"),
            ("Actions", "fallout1;fallout2;ettu;"),
        ] {
            assert_eq!(field(&entry, main, key), Some(value), "{key}");
        }
        for (id, name, arguments) in [
            (
                "fallout1",
                "Launch Fallout 1",
                "--game-profile fallout1 --game",
            ),
            (
                "fallout2",
                "Launch Fallout 2",
                "--game-profile fallout2 --game",
            ),
            ("ettu", "Launch Et Tu", "--profile ettu --game"),
        ] {
            let section = format!("Desktop Action {id}");
            assert_eq!(field(&entry, &section, "Name"), Some(name));
            assert_eq!(
                field(&entry, &section, "Exec"),
                Some(format!("{exec} {arguments}").as_str())
            );
        }
    }

    #[test]
    fn a_path_with_a_space_is_quoted_in_each_exec() {
        let entry = desktop_entry(Path::new("/home/a b/bin/vault-engine")).unwrap();
        let quoted = "\"/home/a b/bin/vault-engine\"";
        assert_eq!(field(&entry, "Desktop Entry", "Exec"), Some(quoted));
        assert_eq!(
            field(&entry, "Desktop Action fallout2", "Exec"),
            Some("\"/home/a b/bin/vault-engine\" --game-profile fallout2 --game")
        );
        assert_eq!(
            field(&entry, "Desktop Action ettu", "Exec"),
            Some("\"/home/a b/bin/vault-engine\" --profile ettu --game")
        );
    }

    #[test]
    fn exec_quoting_follows_the_specification() {
        let exec = |path: &str| escape_value(&quote_argument(path));
        assert_eq!(exec("/plain/path-1_2.x"), "/plain/path-1_2.x");
        // A dollar sign is `\$` to the Exec rule, then `\\$` in the file, and
        // a backslash is `\\`, then `\\\\`, as the specification spells out.
        assert_eq!(exec("/a$b"), "\"/a\\\\$b\"");
        assert_eq!(exec("/a\\b"), "\"/a\\\\\\\\b\"");
        assert_eq!(exec("/a\"b"), "\"/a\\\\\"b\"");
        assert_eq!(exec("/a`b"), "\"/a\\\\`b\"");
        // Reserved, and nothing to escape within the quotes.
        for reserved in ["/a'b", "/a(b)", "/a~b", "/a&b", "/a;b", "/a#b", "/a*b"] {
            assert_eq!(exec(reserved), format!("\"{reserved}\""));
        }
        // A field code would otherwise start at a percent sign.
        assert_eq!(exec("/a%f"), "/a%%f");
        assert_eq!(exec("/a b%f"), "\"/a b%%f\"");
        // Control characters take the string value's escapes.
        assert_eq!(exec("/a\nb"), "\"/a\\nb\"");
        assert_eq!(exec("/a\tb"), "\"/a\\tb\"");
    }

    #[cfg(unix)]
    #[test]
    fn an_entry_needs_a_utf8_path() {
        use std::os::unix::ffi::OsStrExt;
        let path = Path::new(OsStr::from_bytes(b"/home/\xff/vault-engine"));
        assert!(desktop_entry(path).unwrap_err().contains("not UTF-8"));
    }

    // The desktop entry is a Linux convention: POSIX paths and PATH.
    #[cfg(unix)]
    #[test]
    fn the_folders_follow_the_base_directory_rule() {
        let resolve = |home: Option<&str>, xdg: Option<&str>| {
            InstallRoot::resolve(home.map(Into::into), xdg.map(Into::into))
        };
        let default = InstallRoot {
            data: "/home/a/.local/share".into(),
            bin: "/home/a/.local/bin".into(),
        };
        assert_eq!(resolve(Some("/home/a"), None), Ok(default.clone()));
        assert_eq!(resolve(Some("/home/a"), Some("")), Ok(default.clone()));
        assert_eq!(resolve(Some("/home/a"), Some("rel")), Ok(default));
        let moved = resolve(Some("/home/a"), Some("/xdg")).unwrap();
        assert_eq!(moved.data, Path::new("/xdg"));
        assert_eq!(moved.bin, Path::new("/home/a/.local/bin"));
        assert_eq!(
            moved.icon_file(),
            Path::new("/xdg/icons/hicolor/256x256/apps/vault-engine.png")
        );
        assert!(resolve(None, Some("/xdg")).is_err());
        assert!(resolve(Some(""), None).is_err());
    }

    #[test]
    fn the_crc_is_the_standard_one() {
        assert_eq!(crc32(b"123456789"), 0xCBF4_3926);
    }

    #[test]
    fn the_icon_png_decodes_to_the_icon() {
        let pixels = icon::rgba(ICON_SIDE);
        // Past one stored block, so the block boundary is covered.
        assert!(pixels.len() > usize::from(u16::MAX));
        let rows = decode(&png(ICON_SIDE, &pixels).unwrap(), ICON_SIDE);
        let row = ICON_SIDE as usize * 4;
        for (decoded, source) in rows.chunks_exact(row + 1).zip(pixels.chunks_exact(row)) {
            assert_eq!(decoded[0], 0);
            assert_eq!(&decoded[1..], source);
        }
    }

    #[test]
    fn a_stored_block_can_be_exactly_full_or_empty() {
        for length in [0, 1, 65535, 65536, 131070] {
            let data = vec![7; length];
            assert_eq!(zlib_decompress(&zlib_stored(&data), length).unwrap(), data);
        }
    }

    #[test]
    fn a_png_needs_matching_pixels() {
        assert!(png(0, &[]).is_err());
        assert!(png(2, &[0; 15]).is_err());
        assert!(png(2, &[0; 17]).is_err());
    }

    // The desktop entry is a Linux convention: POSIX paths and PATH.
    #[cfg(unix)]
    #[test]
    fn install_writes_the_three_files() {
        let scratch = Scratch::new("install");
        let root = scratch.root();
        let binary = scratch.binary("program");
        let report = install(&root, &binary, &fake_png()).unwrap();
        assert_eq!(report.desktop, root.desktop_file());
        assert_eq!(report.icon, root.icon_file());
        assert_eq!(report.binary, root.binary_file());
        assert_eq!(
            files(&scratch.0),
            [
                "bin/vault-engine",
                "built",
                "data/applications/vault-engine.desktop",
                "data/icons/hicolor/256x256/apps/vault-engine.png",
            ]
        );
        assert_eq!(fs::read(root.binary_file()).unwrap(), b"program");
        assert_eq!(fs::read(root.icon_file()).unwrap(), fake_png());
        let entry = fs::read_to_string(root.desktop_file()).unwrap();
        let exec = format!("\"{}\"", root.binary_file().display());
        assert_eq!(field(&entry, "Desktop Entry", "Exec"), Some(exec.as_str()));
        let report = report.to_string();
        assert!(report.contains("vault-engine.desktop"));
        assert!(report.contains("vault-engine.png"));
    }

    #[cfg(unix)]
    #[test]
    fn the_binary_is_executable_and_the_rest_are_not() {
        use std::os::unix::fs::PermissionsExt;
        let scratch = Scratch::new("modes");
        let root = scratch.root();
        let binary = scratch.binary("program");
        fs::set_permissions(&binary, fs::Permissions::from_mode(0o600)).unwrap();
        install(&root, &binary, &fake_png()).unwrap();
        let mode = |path: PathBuf| fs::metadata(path).unwrap().permissions().mode() & 0o7777;
        assert_eq!(mode(root.binary_file()), 0o755);
        assert_eq!(mode(root.icon_file()), 0o644);
        assert_eq!(mode(root.desktop_file()), 0o644);
    }

    #[test]
    fn reinstalling_leaves_a_running_copy_its_old_file() {
        use std::io::Read;
        let scratch = Scratch::new("running");
        let root = scratch.root();
        install(&root, &scratch.binary("old"), &fake_png()).unwrap();
        // An open file stands for a running program's mapped image.
        let mut running = fs::File::open(root.binary_file()).unwrap();
        install(&root, &scratch.binary("new"), &fake_png()).unwrap();
        let mut seen = String::new();
        running.read_to_string(&mut seen).unwrap();
        assert_eq!(seen, "old");
        assert_eq!(fs::read(root.binary_file()).unwrap(), b"new");
        // No temporary file stays behind.
        assert_eq!(files(&root.bin), ["vault-engine"]);
    }

    #[test]
    fn a_bad_input_installs_nothing() {
        let scratch = Scratch::new("refused");
        let root = scratch.root();
        let binary = scratch.binary("program");
        assert!(install(&root, &binary, b"not a png").is_err());
        assert!(install(&root, &scratch.0.join("gone"), &fake_png()).is_err());
        assert_eq!(files(&scratch.0), ["built"]);
    }

    #[test]
    fn a_failed_write_leaves_no_temporary_file() {
        let scratch = Scratch::new("failed");
        let root = scratch.root();
        // A folder where the binary belongs: the rename cannot replace it.
        fs::create_dir_all(root.binary_file()).unwrap();
        let binary = scratch.binary("program");
        let error = install(&root, &binary, &fake_png()).unwrap_err();
        assert!(error.contains("cannot write"), "{error}");
        assert_eq!(fs::read_dir(&root.bin).unwrap().count(), 1);
    }

    // The desktop entry is a Linux convention: POSIX paths and PATH.
    #[cfg(unix)]
    #[test]
    fn uninstall_removes_the_three_files_alone() {
        let scratch = Scratch::new("uninstall");
        let root = scratch.root();
        install(&root, &scratch.binary("program"), &fake_png()).unwrap();
        let keep = [
            root.bin.join("other-tool"),
            root.data.join("applications/other.desktop"),
            root.data.join("icons/hicolor/256x256/apps/other.png"),
            root.data.join("applications/mimeinfo.cache"),
        ];
        for path in &keep {
            fs::write(path, "theirs").unwrap();
        }
        let removed = uninstall(&root).unwrap();
        assert_eq!(
            removed,
            [root.desktop_file(), root.icon_file(), root.binary_file()]
        );
        assert_eq!(
            files(&scratch.0),
            [
                "bin/other-tool",
                "built",
                "data/applications/mimeinfo.cache",
                "data/applications/other.desktop",
                "data/icons/hicolor/256x256/apps/other.png",
            ]
        );
        // There is nothing left to remove the second time.
        assert_eq!(uninstall(&root).unwrap(), Vec::<PathBuf>::new());
    }

    #[test]
    fn uninstalling_nothing_is_fine() {
        let scratch = Scratch::new("empty");
        assert_eq!(uninstall(&scratch.root()).unwrap(), Vec::<PathBuf>::new());
    }

    /// A "build" with one source, the source `newer` seconds newer than it.
    fn built(scratch: &Scratch, newer: u64) -> PathBuf {
        let binary = scratch.0.join("vault-platform");
        let source = scratch.0.join("a file.rs");
        fs::write(&binary, "program").unwrap();
        fs::write(&source, "fn main() {}").unwrap();
        let list = format!(
            "{}: {}\n",
            binary.display(),
            source.display().to_string().replace(' ', "\\ ")
        );
        fs::write(binary.with_extension("d"), list).unwrap();
        let base = SystemTime::now() - Duration::from_secs(1000);
        let set = |path: &Path, time: SystemTime| {
            let file = fs::File::options().write(true).open(path).unwrap();
            file.set_modified(time).unwrap();
        };
        set(&binary, base);
        set(&source, base + Duration::from_secs(newer));
        binary
    }

    #[test]
    fn a_build_as_new_as_its_sources_passes() {
        let scratch = Scratch::new("fresh");
        let binary = built(&scratch, 0);
        assert_eq!(check_built(&scratch.0, &binary), Ok(()));
    }

    #[test]
    fn an_old_build_asks_for_a_new_one() {
        let scratch = Scratch::new("stale");
        let binary = built(&scratch, 60);
        let error = check_built(&scratch.0, &binary).unwrap_err();
        assert!(error.contains("is older than"), "{error}");
        assert!(error.contains("a file.rs"), "{error}");
        assert!(error.contains(BUILD_COMMAND), "{error}");
    }

    #[test]
    fn a_missing_build_asks_for_one() {
        let scratch = Scratch::new("missing");
        let error = check_built(&scratch.0, &scratch.0.join("gone")).unwrap_err();
        assert!(error.contains("is missing"), "{error}");
        assert!(error.contains(BUILD_COMMAND), "{error}");
    }

    #[test]
    fn a_build_without_its_list_or_with_a_gone_source_asks_for_one() {
        let scratch = Scratch::new("list");
        let binary = built(&scratch, 0);
        fs::remove_file(scratch.0.join("a file.rs")).unwrap();
        let error = check_built(&scratch.0, &binary).unwrap_err();
        assert!(error.contains("which is gone"), "{error}");
        fs::write(binary.with_extension("d"), "nothing").unwrap();
        let error = check_built(&scratch.0, &binary).unwrap_err();
        assert!(error.contains("lists no sources"), "{error}");
        fs::remove_file(binary.with_extension("d")).unwrap();
        let error = check_built(&scratch.0, &binary).unwrap_err();
        assert!(error.contains("cannot read"), "{error}");
    }

    #[test]
    fn dep_info_keeps_escaped_spaces() {
        let sources = dep_info_sources("/t/bin: /a/one.rs /a\\ b/two.rs crates/x.rs\n\n");
        assert_eq!(
            sources,
            [
                PathBuf::from("/a/one.rs"),
                PathBuf::from("/a b/two.rs"),
                PathBuf::from("crates/x.rs"),
            ]
        );
        assert!(dep_info_sources("").is_empty());
    }

    #[test]
    fn a_missing_tool_is_fine() {
        let note = best_effort("vault-engine-no-such-tool", &[]);
        assert!(note.contains("not installed"), "{note}");
    }

    // The desktop entry is a Linux convention: POSIX paths and PATH.
    #[cfg(unix)]
    #[test]
    fn the_path_note_says_whether_a_terminal_will_find_it() {
        let bin = Path::new("/home/a/.local/bin");
        let on = OsStr::new("/usr/bin:/home/a/.local/bin");
        assert!(path_note(bin, Some(on)).contains("is on PATH"));
        let off = OsStr::new("/usr/bin");
        assert!(path_note(bin, Some(off)).contains("must be on PATH"));
        assert!(path_note(bin, None).contains("must be on PATH"));
    }
}
