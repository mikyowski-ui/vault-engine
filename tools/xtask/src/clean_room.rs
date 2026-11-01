//! `cargo xtask clean-room-check`: the ingestion rule of `CLAUDE.md` as a scan
//! of `specs/`, `oracle/` and `docs/`. A hit fails the run. Nothing here edits
//! a file or knows an exception for one: the rule forbids sanitizing on this
//! host, so a hit is for a person to decide.
//!
//! It looks for three things, line by line:
//!
//! - **A 40-character hash:** a run of exactly 40 hexadecimal digits, in either
//!   case. A longer run is no git hash: the oracle's SHA-256 digests are 64.
//! - **A foreign source path:** a path with a directory named `src`, in any
//!   case, after either slash. In `specs/` and `oracle/` every such path is
//!   foreign, since what crosses the wall may name none (`PROVENANCE.md`, "The
//!   quarantine barrier"). In `docs/`, which this repository writes about
//!   itself, `crates/<member>/src/...` and `tools/<member>/src/...` are not,
//!   for a folder that exists, with no other `src` or `..` after it.
//! - **A C code fence:** a Markdown fence whose language is in [`C_LANGUAGES`].
//!   Only the tag is read, so a listing under `text` or no tag is not found:
//!   those fences hold pseudocode and sample files that look much the same.
//!
//! A hit prints where it is and what kind it is, never the text, so that the
//! report cannot carry what it reports. A file with a NUL in its first 8000
//! bytes (git's test) is binary and skipped; any other is read, with bytes
//! that are not UTF-8 replaced. A file that cannot be read, or a missing
//! folder, is an error and not a pass.
use std::{
    collections::BTreeSet,
    error::Error,
    fmt, fs, io,
    path::{Path, PathBuf},
};

const HASH_DIGITS: usize = 40;
const BINARY_PROBE: usize = 8000;
/// Compared without case.
const C_LANGUAGES: [&str; 9] = ["c", "h", "cc", "cpp", "cxx", "c++", "hh", "hpp", "hxx"];

/// Which rule a folder's `src` paths come under.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Scope {
    /// Material that crossed the wall: no exemption.
    Ingested,
    /// This repository's own writing: its own paths are allowed.
    Docs,
}

/// The folders scanned, from the workspace root.
pub const ROOTS: [(&str, Scope); 3] = [
    ("specs", Scope::Ingested),
    ("oracle", Scope::Ingested),
    ("docs", Scope::Docs),
];

/// What a line was found to hold.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    Hash,
    ForeignPath,
    CFence,
}

impl fmt::Display for Kind {
    fn fmt(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
        formatter.write_str(match self {
            Self::Hash => "40-character hexadecimal string",
            Self::ForeignPath => "foreign source path",
            Self::CFence => "C code fence",
        })
    }
}

/// One hit, as `file:line: kind`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Hit {
    pub file: String,
    pub line: usize,
    pub kind: Kind,
}

impl fmt::Display for Hit {
    fn fmt(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
        write!(formatter, "{}:{}: {}", self.file, self.line, self.kind)
    }
}

/// The workspace folders whose source paths `docs/` may name, as `crates/name`
/// and `tools/name`.
#[derive(Debug, Default)]
pub struct Members(BTreeSet<String>);

impl Members {
    pub fn of_workspace(root: &Path) -> io::Result<Self> {
        let mut found = BTreeSet::new();
        for area in ["crates", "tools"] {
            for entry in fs::read_dir(root.join(area))? {
                let entry = entry?;
                if entry.file_type()?.is_dir() {
                    let name = entry.file_name();
                    found.insert(format!("{area}/{}", name.to_string_lossy()));
                }
            }
        }
        Ok(Self(found))
    }
}

fn has_hash(line: &str) -> bool {
    line.split(|c: char| !c.is_ascii_hexdigit())
        .any(|run| run.len() == HASH_DIGITS)
}

fn is_src(segment: &str) -> bool {
    segment.eq_ignore_ascii_case("src")
}

/// Whether `parts` is a path into one of `members`' sources: one `src`, where
/// a member's is, and nothing that climbs back out of it.
fn is_own(parts: &[&str], members: &Members) -> bool {
    let start = parts
        .iter()
        .take_while(|part| matches!(**part, "." | ".."))
        .count();
    let [area, name, "src", rest @ ..] = &parts[start..] else {
        return false;
    };
    members.0.contains(&format!("{area}/{name}"))
        && !rest.iter().any(|part| is_src(part) || *part == "..")
}

fn has_foreign_path(line: &str, scope: Scope, members: &Members) -> bool {
    // A word of path characters. `:` keeps a drive or a URL with its path, so
    // that neither passes for a relative path into this repository.
    line.split(|c: char| !(c.is_alphanumeric() || "_-.+~@%#$:/\\".contains(c)))
        .any(|word| {
            let parts: Vec<&str> = word.split(['/', '\\']).collect();
            // The last part is what follows the last slash: not a directory.
            let directories = &parts[..parts.len() - 1];
            directories.iter().any(|part| is_src(part))
                && !(scope == Scope::Docs && is_own(&parts, members))
        })
}

/// Whether `line` opens a fence tagged with a C language. It reads no fence
/// state: a closing fence has no tag, and a tag quoted inside a longer fence
/// is still worth a look.
fn opens_c_fence(line: &str) -> bool {
    // A fence may sit in a quote or a list item.
    let rest = line.trim_start_matches([' ', '\t', '>', '-', '*', '+']);
    let rest = rest.trim_start_matches(|c: char| c.is_ascii_digit());
    let rest = rest.trim_start_matches(['.', ')', ' ', '\t']);
    let Some(marker @ ('`' | '~')) = rest.chars().next() else {
        return false;
    };
    let tag = rest.trim_start_matches(marker);
    if rest.len() - tag.len() < 3 || (marker == '`' && tag.contains('`')) {
        return false;
    }
    tag.split(|c: char| c.is_whitespace() || ",{}".contains(c))
        .find(|word| !word.is_empty())
        .map(|word| word.trim_start_matches('.'))
        .is_some_and(|word| {
            C_LANGUAGES
                .iter()
                .any(|language| language.eq_ignore_ascii_case(word))
        })
}

/// Each hit in `text`, by 1-based line; a line gives each kind at most once.
pub fn scan_text(text: &str, scope: Scope, members: &Members) -> Vec<(usize, Kind)> {
    let mut hits = Vec::new();
    for (index, line) in text.lines().enumerate() {
        let found = [
            (has_hash(line), Kind::Hash),
            (has_foreign_path(line, scope, members), Kind::ForeignPath),
            (opens_c_fence(line), Kind::CFence),
        ];
        hits.extend(
            found
                .into_iter()
                .filter(|(present, _)| *present)
                .map(|(_, kind)| (index + 1, kind)),
        );
    }
    hits
}

fn is_binary(bytes: &[u8]) -> bool {
    bytes[..bytes.len().min(BINARY_PROBE)].contains(&0)
}

/// Every file under `directory`. A symbolic link stays in the list, so that
/// one the scan cannot read is an error and not a gap.
fn walk(directory: &Path, files: &mut Vec<PathBuf>) -> io::Result<()> {
    for entry in fs::read_dir(directory)? {
        let entry = entry?;
        if entry.file_type()?.is_dir() {
            walk(&entry.path(), files)?;
        } else {
            files.push(entry.path());
        }
    }
    Ok(())
}

/// What a whole scan found.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct Report {
    pub hits: Vec<Hit>,
    pub text_files: usize,
    pub binary_files: usize,
}

impl Report {
    /// The closing line and the exit status.
    pub fn outcome(&self) -> (String, u8) {
        let counts = format!(
            "{} text files, {} binary skipped",
            self.text_files, self.binary_files
        );
        match self.hits.len() {
            0 => (format!("clean-room-check ok: {counts}"), 0),
            hits => (format!("clean-room-check failed: {hits} hits; {counts}"), 1),
        }
    }
}

/// The path a person reads, the same on every host.
fn display(root: &Path, path: &Path) -> String {
    path.strip_prefix(root)
        .unwrap_or(path)
        .to_string_lossy()
        .replace('\\', "/")
}

/// Scan [`ROOTS`] under `root`, in a fixed order.
pub fn scan_tree(root: &Path, members: &Members) -> Result<Report, String> {
    let mut report = Report::default();
    for (folder, scope) in ROOTS {
        let mut files = Vec::new();
        let directory = root.join(folder);
        walk(&directory, &mut files).map_err(|error| format!("{folder}: {error}"))?;
        files.sort();
        for path in files {
            let name = display(root, &path);
            let bytes = fs::read(&path).map_err(|error| format!("{name}: {error}"))?;
            if is_binary(&bytes) {
                report.binary_files += 1;
                continue;
            }
            report.text_files += 1;
            let text = String::from_utf8_lossy(&bytes);
            report.hits.extend(
                scan_text(&text, scope, members)
                    .into_iter()
                    .map(|(line, kind)| Hit {
                        file: name.clone(),
                        line,
                        kind,
                    }),
            );
        }
    }
    Ok(report)
}

/// Run the check from `root`, and return its exit status.
pub fn run(root: &Path) -> Result<u8, Box<dyn Error>> {
    let members = Members::of_workspace(root)?;
    let report = scan_tree(root, &members)?;
    report.hits.iter().for_each(|hit| println!("{hit}"));
    let (closing, code) = report.outcome();
    println!("{closing}");
    Ok(code)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn members() -> Members {
        Members(
            ["crates/vault-core", "tools/xtask"]
                .map(str::to_owned)
                .into(),
        )
    }

    /// Built here, so that this file holds no such string itself.
    fn hash() -> String {
        "a1".repeat(HASH_DIGITS / 2)
    }

    fn foreign(line: &str, scope: Scope) -> bool {
        has_foreign_path(line, scope, &members())
    }

    #[test]
    fn a_hash_is_exactly_forty_digits_in_either_case() {
        for line in [
            hash(),
            hash().to_uppercase(),
            format!("commit {}.", hash()),
            format!("`{}`", hash()),
            format!("https://example.test/commit/{}#top", hash()),
            format!("v1-2-g{}", hash()),
        ] {
            assert!(has_hash(&line), "{line}");
        }
        for digits in [39, 41, 64, 128] {
            let line = format!("sha {} end", "b".repeat(digits));
            assert!(!has_hash(&line), "{digits}");
        }
        assert!(!has_hash(&"z".repeat(40)));
        assert!(!has_hash(&format!("{} {}", "c".repeat(20), "d".repeat(20))));
    }

    #[test]
    fn nothing_under_a_source_directory_passes_in_ingested_material() {
        for line in [
            "see example-engine/src/widget.c for it",
            "`src/`",
            "src/widget.c",
            "./src/widget.c",
            "[it](../src/widget.c)",
            "C:\\work\\example\\SRC\\widget.c",
            "https://example.test/x/blob/main/src/widget.c",
            "(Src/widget.c)",
            // This repository's own paths are no exception here.
            "crates/vault-core/src/lib.rs",
            "tools/xtask/src/main.rs",
        ] {
            assert!(foreign(line, Scope::Ingested), "{line}");
        }
    }

    #[test]
    fn the_word_and_its_neighbours_are_not_paths() {
        for line in [
            "the src of an image",
            "src",
            "resrc/widget.c",
            "src-tree/widget.c",
            "src.rs/widget.c",
            "source/widget.c",
            "example/src",
            "<img src=\"assets/one.png\">",
        ] {
            assert!(!foreign(line, Scope::Ingested), "{line}");
            assert!(!foreign(line, Scope::Docs), "{line}");
        }
    }

    #[test]
    fn docs_may_name_only_a_real_members_own_sources() {
        for line in [
            "crates/vault-core/src/lib.rs",
            "crates/vault-core/src/ui/bar.rs:12",
            "(`tools/xtask/src/main.rs`, `other.rs`;",
            "[link](../crates/vault-core/src/lib.rs)",
            "./tools/xtask/src/",
            "crates\\vault-core\\src\\lib.rs",
        ] {
            assert!(!foreign(line, Scope::Docs), "{line}");
        }
        for line in [
            // No such member.
            "crates/other/src/lib.rs",
            "tools/other/src/main.rs",
            // The short form, which names no folder here.
            "vault-core/src/lib.rs",
            // Under something else.
            "example/crates/vault-core/src/lib.rs",
            "https://example.test/crates/vault-core/src/lib.rs",
            // Not where a member keeps its sources.
            "crates/vault-core/tests/src/lib.rs",
            "crates/vault-core/SRC/lib.rs",
            // Out of the member again, or into a second tree.
            "crates/vault-core/src/../../../other/widget.c",
            "crates/vault-core/src/vendor/src/widget.c",
            "crates/vault-core/../src/widget.c",
            "src/widget.c",
        ] {
            assert!(foreign(line, Scope::Docs), "{line}");
        }
    }

    #[test]
    fn a_fence_is_c_by_its_tag() {
        for line in [
            "```c",
            "```C",
            "``` c",
            "```cpp",
            "```c++",
            "```h",
            "````c",
            "~~~c",
            "   ```c",
            "```c title=\"x\"",
            "```{.c}",
            "```{c}",
            "```c,ignore",
            "> ```c",
            "- ```c",
            "1. ```c",
        ] {
            assert!(opens_c_fence(line), "{line}");
        }
        for line in [
            "```",
            "~~~",
            "```text",
            "```rust",
            "```sh",
            "```toml",
            "```json",
            "```cs",
            "```css",
            "```csharp",
            "```clojure",
            "```c```",
            "``c",
            "use ```c here",
            "c",
        ] {
            assert!(!opens_c_fence(line), "{line}");
        }
    }

    #[test]
    fn hits_carry_their_line_with_each_kind_once() {
        let text = format!(
            "clean\r\nx {hash} {hash}\r\n\nsrc/one.c src/two.c {hash}\n```c\nint;\n```\n",
            hash = hash()
        );
        assert_eq!(
            scan_text(&text, Scope::Ingested, &members()),
            [
                (2, Kind::Hash),
                (4, Kind::Hash),
                (4, Kind::ForeignPath),
                (5, Kind::CFence),
            ]
        );
    }

    /// A tree that is removed on drop, so a failed assert leaves nothing.
    struct Tree(PathBuf);

    impl Tree {
        fn new(name: &str) -> Self {
            let root = std::env::temp_dir()
                .join(format!("xtask-clean-room-{name}-{}", std::process::id()));
            for folder in [
                "specs/approved",
                "oracle/SPEC-1",
                "docs",
                "crates/one",
                "tools",
            ] {
                fs::create_dir_all(root.join(folder)).unwrap();
            }
            Self(root)
        }
    }

    impl Drop for Tree {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn a_tree_reports_every_hit_in_order_and_keeps_its_text_out() {
        let tree = Tree::new("hits");
        let root = &tree.0;
        let write = |path: &str, bytes: &[u8]| fs::write(root.join(path), bytes).unwrap();
        write("specs/approved/b.md", b"crates/one/src/lib.rs\n");
        write("specs/approved/a.md", b"fine\n```cpp\n");
        // Latin-1, not UTF-8, with the hit after it.
        let mut latin = b"caf\xE9 \xFF\n".to_vec();
        latin.extend(hash().as_bytes());
        write("oracle/SPEC-1/vectors.tsv", &latin);
        // The hit is past the bytes that make it binary.
        let mut binary = b"\x00\x01".to_vec();
        binary.extend(hash().as_bytes());
        write("oracle/SPEC-1/movie.mve", &binary);
        write(
            "docs/guide.md",
            b"crates/one/src/lib.rs\nother/src/lib.rs\n",
        );
        let members = Members::of_workspace(root).unwrap();
        let report = scan_tree(root, &members).unwrap();
        let lines: Vec<String> = report.hits.iter().map(Hit::to_string).collect();
        assert_eq!(
            lines,
            [
                "specs/approved/a.md:2: C code fence",
                "specs/approved/b.md:1: foreign source path",
                "oracle/SPEC-1/vectors.tsv:2: 40-character hexadecimal string",
                "docs/guide.md:2: foreign source path",
            ]
        );
        assert_eq!((report.text_files, report.binary_files), (4, 1));
        assert_eq!(
            report.outcome(),
            (
                "clean-room-check failed: 4 hits; 4 text files, 1 binary skipped".to_owned(),
                1
            )
        );
        assert_eq!(run(root).unwrap(), 1);
    }

    #[test]
    fn a_clean_tree_passes_and_says_what_it_read() {
        let tree = Tree::new("clean");
        fs::write(tree.0.join("docs/guide.md"), "```text\nfine\n```\n").unwrap();
        let report = scan_tree(&tree.0, &Members::default()).unwrap();
        assert_eq!(
            report.outcome(),
            (
                "clean-room-check ok: 1 text files, 0 binary skipped".to_owned(),
                0
            )
        );
        assert_eq!(run(&tree.0).unwrap(), 0);
    }

    #[test]
    fn a_missing_folder_is_an_error_and_not_a_pass() {
        let tree = Tree::new("missing");
        fs::remove_dir_all(tree.0.join("oracle")).unwrap();
        let error = scan_tree(&tree.0, &Members::default()).unwrap_err();
        assert!(error.starts_with("oracle: "), "{error}");
    }

    #[cfg(unix)]
    #[test]
    fn a_link_that_cannot_be_read_is_an_error_and_not_a_gap() {
        let tree = Tree::new("link");
        std::os::unix::fs::symlink(tree.0.join("nowhere"), tree.0.join("docs/link.md")).unwrap();
        let error = scan_tree(&tree.0, &Members::default()).unwrap_err();
        assert!(error.starts_with("docs/link.md: "), "{error}");
    }

    #[test]
    fn the_scan_reads_exactly_the_three_folders() {
        assert_eq!(ROOTS.map(|(folder, _)| folder), ["specs", "oracle", "docs"]);
        assert_eq!(
            ROOTS.map(|(_, scope)| scope),
            [Scope::Ingested, Scope::Ingested, Scope::Docs]
        );
    }
}
