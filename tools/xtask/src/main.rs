use std::{
    error::Error,
    path::{Path, PathBuf},
    process::{Command, ExitCode},
};

use clap::{Parser, Subcommand};

#[allow(dead_code)]
#[path = "../../../tests/fixtures/formats/support.rs"]
pub(crate) mod fixture_assets;

#[allow(dead_code)]
#[path = "../../../tests/fixtures/world/support.rs"]
pub(crate) mod fixture_world;
#[allow(dead_code)]
#[path = "../../../tests/fixtures/state/support.rs"]
mod state;
#[allow(dead_code)]
#[path = "../../../tests/fixtures/vm/support.rs"]
mod vm;

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
    /// Materialize a complete synthetic map/prototype/art/INT corpus in an empty directory.
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

fn materialize(output: &Path) -> Result<(), Box<dyn Error>> {
    if output.exists() && output.read_dir()?.next().is_some() {
        return Err("fixture output directory must be empty".into());
    }
    std::fs::create_dir_all(output)?;
    state::write_corpus(output);
    std::fs::write(output.join("scripts/core-fixture.int"), vm::program())?;
    println!("Synthetic corpus written to {}", output.display());
    Ok(())
}
