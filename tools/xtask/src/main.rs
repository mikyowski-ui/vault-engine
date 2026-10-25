use std::{
    error::Error,
    path::{Path, PathBuf},
    process::{Command, ExitCode},
};

use clap::{Parser, Subcommand};
use vault_fixtures::{state, vm};

#[derive(Parser)]
#[command(about = "Vault Engine workspace verification")]
struct Args {
    #[command(subcommand)]
    command: Task,
}

#[derive(Subcommand)]
enum Task {
    /// Run all engineering gates from AGENTS.md (clippy and tests with `--locked`).
    Check,
    /// Materialize a complete synthetic map/prototype/art/INT corpus in an empty
    /// directory, and the same corpus with the enhanced camera demo map
    /// `maps/cam_demo.map` in `<PATH>-cam-demo`, also empty.
    Fixtures {
        #[arg(long, value_name = "PATH")]
        output: PathBuf,
    },
    /// Export the current setup as a ready-to-share mod folder: a
    /// `vault-mod.toml` with the enhanced viewport, zoom, smoothing and
    /// roof-peek settings of `vault-engine.toml` (found as the game finds
    /// it, from the working directory; the enabled mods' manifests apply
    /// over it), the table overrides and enhanced scripts of its game data
    /// and mods, and a README.txt on loading it in the launcher's Mods
    /// tab. Runs the desktop shell's `--export-mod`.
    ExportMod {
        /// The mod's name and folder name (letters, digits, '-', '_', '.').
        #[arg(long, value_name = "SLUG")]
        name: String,
        /// The folder to write, such as mods/<SLUG>; it must not exist or
        /// be empty.
        #[arg(long, value_name = "PATH")]
        output: PathBuf,
        /// Also take table overrides and enhanced scripts from this loose
        /// folder (repeatable), such as the `-cam-demo` corpus `fixtures`
        /// writes.
        #[arg(long, value_name = "PATH")]
        from: Vec<PathBuf>,
        /// The viewport (`fixed_640x480` or `unlocked_widescreen`) instead
        /// of the settings'.
        #[arg(long, value_name = "MODE")]
        viewport: Option<String>,
        /// The zoom a map starts at (0.5-3.0) instead of the settings'.
        #[arg(long, value_name = "ZOOM")]
        zoom: Option<String>,
    },
}

/// The workspace root.
fn root() -> Result<&'static Path, Box<dyn Error>> {
    Ok(Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .ok_or("xtask is not inside a workspace")?)
}

fn cargo() -> std::ffi::OsString {
    std::env::var_os("CARGO").unwrap_or_else(|| "cargo".into())
}

fn check() -> Result<(), Box<dyn Error>> {
    let root = root()?;
    let cargo = cargo();
    for args in [
        &["fmt", "--all", "--", "--check"][..],
        &[
            "clippy",
            "--workspace",
            "--all-targets",
            "--locked",
            "--",
            "-D",
            "warnings",
        ],
        &["test", "--workspace", "--locked"],
    ] {
        let status = Command::new(&cargo).args(args).current_dir(root).status()?;
        if !status.success() {
            return Err(format!("cargo {} failed: {status}", args.join(" ")).into());
        }
    }
    Ok(())
}

fn main() -> ExitCode {
    let result = match Args::parse().command {
        Task::Check => check(),
        Task::Fixtures { output } => materialize(&output),
        Task::ExportMod {
            name,
            output,
            from,
            viewport,
            zoom,
        } => export_mod(&name, &output, &from, viewport, zoom),
    };
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("xtask: {error}");
            ExitCode::FAILURE
        }
    }
}

/// The shell's `--export-mod` in the working directory (where the game
/// would look for `vault-engine.toml`), so relative paths are the user's.
fn export_mod(
    name: &str,
    output: &Path,
    from: &[PathBuf],
    viewport: Option<String>,
    zoom: Option<String>,
) -> Result<(), Box<dyn Error>> {
    let manifest = root()?.join("Cargo.toml");
    let mut command = Command::new(cargo());
    command
        .args([
            "run",
            "--quiet",
            "--locked",
            "--package",
            "vault-platform",
            "--manifest-path",
        ])
        .arg(manifest)
        .args(["--", "--export-mod", name, "--export-output"])
        .arg(output);
    for folder in from {
        command.arg("--export-from").arg(folder);
    }
    if let Some(viewport) = viewport {
        command.args(["--viewport", &viewport]);
    }
    if let Some(zoom) = zoom {
        command.args(["--zoom", &zoom]);
    }
    let status = command.status()?;
    if !status.success() {
        return Err(format!("the mod export failed: {status}").into());
    }
    Ok(())
}

/// The synthetic corpus in `output`, and beside it in `<output>-cam-demo`
/// the same corpus with the enhanced camera demo added. The demo's script
/// parses only under the Enhanced preset, so it is kept out of `output`,
/// whose files a Vanilla audit validates.
fn materialize(output: &Path) -> Result<(), Box<dyn Error>> {
    let mut name = output
        .file_name()
        .ok_or("fixture output directory needs a name")?
        .to_os_string();
    name.push("-cam-demo");
    let demo = output.with_file_name(name);
    for directory in [output, &demo] {
        if directory.exists() && directory.read_dir()?.next().is_some() {
            return Err(format!(
                "fixture output directory {} must be empty",
                directory.display()
            )
            .into());
        }
    }
    for directory in [output, &demo] {
        std::fs::create_dir_all(directory)?;
        state::write_corpus(directory);
        std::fs::write(directory.join("scripts/core-fixture.int"), vm::program())?;
    }
    // `maps/cam_demo.map` and its map script, appended to `scripts.lst`.
    state::write_cam_demo(&demo);
    println!("Synthetic corpus written to {}", output.display());
    println!("Enhanced camera demo corpus written to {}", demo.display());
    Ok(())
}
