use std::{
    error::Error,
    path::Path,
    process::{Command, ExitCode},
};

use clap::{Parser, Subcommand};

#[derive(Parser)]
#[command(about = "Vault Engine workspace verification")]
struct Args {
    #[command(subcommand)]
    command: Task,
}

#[derive(Subcommand)]
enum Task {
    /// Run all engineering gates from AGENTS.md.
    Check,
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
            "--",
            "-D",
            "warnings",
        ],
        &["test", "--workspace"],
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
    };
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("xtask: {error}");
            ExitCode::FAILURE
        }
    }
}
