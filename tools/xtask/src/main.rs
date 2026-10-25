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
}

fn check() -> Result<(), Box<dyn Error>> {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .ok_or("xtask is not inside a workspace")?;
    let cargo = std::env::var_os("CARGO").unwrap_or_else(|| "cargo".into());
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
    };
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("xtask: {error}");
            ExitCode::FAILURE
        }
    }
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
