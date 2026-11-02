mod audio;
mod autoplay_runner;
mod boot;
mod config;
mod editor_screen;
mod enhanced;
mod gallery;
mod game;
#[path = "../assets/icon.rs"]
mod icon;
mod input;
mod interface;
mod launcher;
mod map_preview;
mod mods;
mod movie;
mod preview;
mod replay;
mod session;
mod shell;
#[cfg(test)]
mod spec024c_tests;
// The whole of SPEC-023's interface: the desktop itself uses the file system
// backend, and the rest is for hosts without OS paths and for the tests.
#[cfg_attr(not(test), allow(dead_code))]
mod storage;
mod toast;
mod touch;

#[cfg(test)]
pub(crate) use vault_fixtures::formats as fixture_assets;

pub(crate) use shell::{EnhancedShown, bezel_layer, enhanced_layers, side_tiles};
#[cfg(test)]
pub(crate) use shell::{InputLog, Shell};

use std::{error::Error, process::ExitCode};

use clap::Parser;
use shell::{Args, run};
use tracing_subscriber::EnvFilter;

type AppError = Box<dyn Error + Send + Sync>;
type AppResult<T> = Result<T, AppError>;

fn main() -> ExitCode {
    let args = match Args::try_parse() {
        Ok(args) => args,
        Err(error) => {
            // SPEC-021 §3.10: with a checkpoint flag a usage error exits 1,
            // because 2 means the comparison failed.
            let checkpoints = std::env::args().any(|argument| {
                ["--checkpoints-out", "--checkpoints-expect"]
                    .iter()
                    .any(|flag| argument.starts_with(flag))
            });
            if error.use_stderr() && checkpoints {
                let _ = error.print();
                return ExitCode::FAILURE;
            }
            error.exit();
        }
    };
    tracing_subscriber::fmt()
        .json()
        .with_env_filter(
            EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info")),
        )
        .with_writer(std::io::stderr)
        .init();
    match run(args) {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            if let Some(diverged) = error.downcast_ref::<replay::CheckpointsDiverged>() {
                eprintln!("{diverged}");
                return ExitCode::from(2);
            }
            tracing::error!(%error, "exiting");
            ExitCode::FAILURE
        }
    }
}
