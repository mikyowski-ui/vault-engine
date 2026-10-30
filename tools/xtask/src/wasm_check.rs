//! SPEC-023 §3.17: the `wasm32-unknown-unknown` compile gate. It keeps the
//! portable crates compiling for that target, with no download and no new
//! dependency. It links and runs nothing, and it never installs anything:
//! where the target is missing it says so and checks what it can on the host.
//! [`Probe`] and [`Gate`] decide; [`run`] only carries the decision out.
use std::{
    error::Error,
    ffi::{OsStr, OsString},
    fs, io,
    path::Path,
    process::Command,
};

pub const TARGET: &str = "wasm32-unknown-unknown";
/// In order: each is checked alone, so that the features of one crate cannot
/// unify with another's.
pub const CRATES: [&str; 4] = ["vault-vm", "vault-formats", "vault-guidance", "vault-core"];

const SKIPPED: &str = "skipped: wasm32-unknown-unknown target not installed";
const MISSING: &str = "error: wasm32-unknown-unknown target not installed";
const HOW_TO_INSTALL: &str = "install it with `rustup target add wasm32-unknown-unknown`, or \
                              with your distribution's wasm32 standard library package";

/// What the probe of the target found.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Probe {
    /// `rustc --print target-libdir` exited 0.
    pub printed: bool,
    /// The path it printed is a directory.
    pub directory: bool,
    /// That directory holds a `libcore-*.rlib`.
    pub core_library: bool,
}

impl Probe {
    /// Installed exactly when all three hold. The exit status alone is not
    /// enough: a distribution's `rustc` prints the path, and exits 0, whether
    /// or not the directory exists.
    pub fn installed(self) -> bool {
        self.printed && self.directory && self.core_library
    }
    /// Look at the directory `rustc` printed.
    pub fn of_directory(directory: &Path) -> Self {
        let entries = fs::read_dir(directory);
        let directory = entries.is_ok();
        let core_library = entries.into_iter().flatten().flatten().any(|entry| {
            entry
                .file_name()
                .to_str()
                .is_some_and(|name| name.starts_with("libcore-") && name.ends_with(".rlib"))
        });
        Self {
            printed: true,
            directory,
            core_library,
        }
    }
}

/// What the gate runs, from the probe and `VAULT_REQUIRE_WASM`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Gate {
    /// The four checks for the target.
    Target,
    /// The target is missing: the host check, in its place.
    Host,
    /// The target is required and missing: nothing.
    Refuse,
}

impl Gate {
    /// Only the value `1` requires the target, as for `VAULT_REQUIRE_GPU`.
    pub fn decide(probe: Probe, require: Option<&OsStr>) -> Self {
        if probe.installed() {
            Self::Target
        } else if require == Some(OsStr::new("1")) {
            Self::Refuse
        } else {
            Self::Host
        }
    }
    /// The lines printed before anything runs.
    pub fn opening(self) -> &'static [&'static str] {
        match self {
            Self::Target => &[],
            Self::Host => &[SKIPPED, HOW_TO_INSTALL],
            Self::Refuse => &[MISSING],
        }
    }
    /// The closing line and exit status, from the first crate that failed.
    /// A refused gate ran nothing, and its opening line is all it says.
    pub fn outcome(self, failed: Option<&str>) -> (Option<String>, u8) {
        match (self, failed) {
            (Self::Refuse, _) => (None, 1),
            (_, Some(name)) => (Some(format!("wasm-check failed: {name}")), 1),
            (Self::Target, None) => (Some("wasm-check ok: 4 crates".to_owned()), 0),
            (Self::Host, None) => (Some("host check ok: 4 crates".to_owned()), 0),
        }
    }
}

/// The arguments of one crate's check, for the target or for the host.
pub fn check_args(name: &str, gate: Gate) -> Vec<&str> {
    let mut args = vec!["check"];
    if gate == Gate::Target {
        args.extend(["--target", TARGET]);
    }
    args.extend(["--locked", "--lib", "--no-default-features", "-p", name]);
    args
}

/// Ask `rustc` where the target's library is, and look there. A `rustc` that
/// cannot run or fails has no target to offer.
fn probe(rustc: &OsStr) -> Probe {
    let output = Command::new(rustc)
        .args(["--print", "target-libdir", "--target", TARGET])
        .output();
    match output {
        Ok(output) if output.status.success() => {
            let text = String::from_utf8_lossy(&output.stdout);
            Probe::of_directory(Path::new(text.trim()))
        }
        _ => Probe {
            printed: false,
            directory: false,
            core_library: false,
        },
    }
}

/// Run the gate from `root`, and return its exit status.
pub fn run(root: &Path, cargo: &OsStr) -> Result<u8, Box<dyn Error>> {
    let rustc = std::env::var_os("RUSTC").unwrap_or_else(|| OsString::from("rustc"));
    let require = std::env::var_os("VAULT_REQUIRE_WASM");
    let gate = Gate::decide(probe(&rustc), require.as_deref());
    gate.opening().iter().for_each(|line| println!("{line}"));
    let mut failed = None;
    if gate != Gate::Refuse {
        for name in CRATES {
            let status = Command::new(cargo)
                .args(check_args(name, gate))
                .current_dir(root)
                .status()
                .map_err(|error: io::Error| format!("cargo check -p {name}: {error}"))?;
            if !status.success() {
                failed = Some(name);
                break;
            }
        }
    }
    let (closing, code) = gate.outcome(failed);
    if let Some(line) = closing {
        println!("{line}");
    }
    Ok(code)
}

#[cfg(test)]
mod tests {
    use super::*;

    const VECTORS: &str = include_str!("../../../tests/fixtures/spec023/wasm-gate.tsv");

    /// A row's check result: the crate that failed, if one did.
    fn failed(result: &str) -> Option<&str> {
        result.strip_prefix("fail:")
    }

    #[test]
    fn t16_every_row_of_the_gate_table() {
        let rows: Vec<Vec<&str>> = VECTORS
            .lines()
            .skip(1)
            .map(|line| line.split('\t').collect())
            .collect();
        assert_eq!(rows.len(), 11);
        for row in rows {
            let [
                case,
                rustc_exit,
                libdir,
                libcore,
                require,
                target_check,
                host_check,
                outcome,
                exit,
                first_line,
                last_line,
            ] = row[..]
            else {
                panic!("{row:?}");
            };
            let probe = Probe {
                printed: rustc_exit == "0",
                directory: matches!(libdir, "empty" | "present"),
                core_library: libcore == "yes",
            };
            let require = (require != "unset").then(|| OsStr::new(require));
            let gate = Gate::decide(probe, require);
            let (ran, result) = match gate {
                Gate::Target => ("wasm", failed(target_check)),
                Gate::Host => ("host", failed(host_check)),
                Gate::Refuse => ("none", None),
            };
            let expected_run = match (target_check, host_check) {
                ("-", "-") => "none",
                ("-", _) => "host",
                _ => "wasm",
            };
            assert_eq!(ran, expected_run, "{case}");
            let (closing, code) = gate.outcome(result);
            let mut lines: Vec<&str> = gate.opening().to_vec();
            lines.extend(closing.as_deref());
            assert_eq!(code.to_string(), exit, "{case}");
            assert_eq!(lines.first(), Some(&first_line), "{case}");
            assert_eq!(lines.last(), Some(&last_line), "{case}");
            let found = match (gate, code) {
                (Gate::Refuse, _) => "error",
                (_, 1) => "fail",
                (Gate::Host, _) => "skip",
                (Gate::Target, _) => "ok",
            };
            assert_eq!(found, outcome, "{case}");
        }
    }

    #[test]
    fn t16_the_probe_tests_the_directory_and_the_library_in_it() {
        let root = std::env::temp_dir().join(format!("xtask-probe-{}", std::process::id()));
        fs::create_dir_all(&root).unwrap();
        let missing = Probe::of_directory(&root.join("missing"));
        assert!(!missing.directory && !missing.installed());
        let empty = Probe::of_directory(&root);
        assert!(empty.directory && !empty.core_library && !empty.installed());
        fs::write(root.join("libstd-0.rlib"), b"").unwrap();
        assert!(!Probe::of_directory(&root).installed());
        fs::write(root.join("libcore-0.rlib"), b"").unwrap();
        assert!(Probe::of_directory(&root).installed());
        fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn the_check_is_the_spec_command_with_and_without_the_target() {
        assert_eq!(
            check_args("vault-core", Gate::Target).join(" "),
            "check --target wasm32-unknown-unknown --locked --lib --no-default-features \
             -p vault-core"
        );
        assert_eq!(
            check_args("vault-vm", Gate::Host).join(" "),
            "check --locked --lib --no-default-features -p vault-vm"
        );
    }
}
