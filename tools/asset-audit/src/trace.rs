//! SPEC-021 §3.7 the dump modes: `--dump-vm-trace` and
//! `--dump-combat-trace`. Each is exclusive; a usage or load error exits 1
//! without writing a trace.
use std::{error::Error, io::Write, path::Path, process::ExitCode, sync::Arc};

use vault_core::{
    WorldHost, WorldState,
    assets::VfsWorldAssets,
    traces::{
        DEFAULT_MAX_ROUNDS, DEFAULT_MAX_STEPS, combat_trace_host, parse_hex_dump, run_combat_trace,
        trace_defender, trace_program_name, trace_vm, write_combat_trace,
    },
};
use vault_formats::LocalDirProvider;
use vault_vm::{IntProgram, Vm};

use crate::Args;

/// Whether the command line asks for a dump mode.
pub fn requested(args: &Args) -> bool {
    args.dump_vm_trace.is_some() || args.dump_combat_trace.is_some()
}
/// Whether a raw command line names a dump flag (for usage errors that
/// stop the parse itself).
pub fn named(arguments: &[String]) -> bool {
    arguments.iter().any(|argument| {
        ["--dump-vm-trace", "--dump-combat-trace"]
            .iter()
            .any(|flag| argument == flag || argument.starts_with(&format!("{flag}=")))
    })
}
/// The flags that only a dump mode reads.
pub fn stray(args: &Args) -> Option<&'static str> {
    [
        (args.max_steps.is_some(), "--max-steps"),
        (args.max_rounds.is_some(), "--max-rounds"),
        (args.seed.is_some(), "--seed"),
        (args.trace_out.is_some(), "--trace-out"),
    ]
    .into_iter()
    .find_map(|(given, flag)| given.then_some(flag))
}

/// The `--validate-*` and `--report-*` flags given.
fn audit_flags(args: &Args) -> Vec<&'static str> {
    [
        (args.validate_formats, "--validate-formats"),
        (args.validate_art_ids, "--validate-art-ids"),
        (args.validate_writers, "--validate-writers"),
        (args.validate_saves, "--validate-saves"),
        (args.validate_map_numbers, "--validate-map-numbers"),
        (args.validate_roofs, "--validate-roofs"),
        (args.validate_ai, "--validate-ai"),
        (args.validate_acm, "--validate-acm"),
        (args.validate_sfx, "--validate-sfx"),
        (args.validate_lip, "--validate-lip"),
        (args.validate_items, "--validate-items"),
        (args.validate_movies, "--validate-movies"),
        (args.validate_interactions, "--validate-interactions"),
        (args.validate_ui, "--validate-ui"),
        (args.validate_dat, "--validate-dat"),
        (args.validate_scripts, "--validate-scripts"),
        (args.validate_exits, "--validate-exits"),
        (args.validate_transitions, "--validate-transitions"),
        (args.report_script_stack, "--report-script-stack"),
        (args.report_fallout2_art, "--report-fallout2-art"),
        (args.report_fallout2_install, "--report-fallout2-install"),
    ]
    .into_iter()
    .filter_map(|(given, flag)| given.then_some(flag))
    .collect()
}

fn fail(message: impl std::fmt::Display) -> ExitCode {
    eprintln!("asset-audit: {message}");
    ExitCode::FAILURE
}
/// Write the trace to `--trace-out` (created or replaced) or stdout.
fn emit(out: Option<&Path>, text: &str) -> Result<(), Box<dyn Error>> {
    match out {
        Some(path) => std::fs::write(path, text)
            .map_err(|error| format!("{}: {error}", path.display()).into()),
        None => {
            let mut stdout = std::io::stdout().lock();
            stdout.write_all(text.as_bytes())?;
            stdout.flush()?;
            Ok(())
        }
    }
}

/// Run the requested dump mode.
pub fn dump(args: &Args) -> ExitCode {
    let mut given = audit_flags(args);
    if args.dump_vm_trace.is_some() && args.dump_combat_trace.is_some() {
        given.push("the other dump flag");
    }
    if !given.is_empty() {
        return fail(format!(
            "a dump mode cannot be combined with {}",
            given.join(", ")
        ));
    }
    if let Some(file) = &args.dump_vm_trace {
        for (given, flag) in [
            (args.max_rounds.is_some(), "--max-rounds"),
            (args.seed.is_some(), "--seed"),
        ] {
            if given {
                return fail(format!("{flag} applies to --dump-combat-trace only"));
            }
        }
        return dump_vm(args, file);
    }
    if args.max_steps.is_some() {
        return fail("--max-steps applies to --dump-vm-trace only");
    }
    let map = args.dump_combat_trace.as_deref().unwrap_or_default();
    match dump_combat(args, map) {
        Ok(code) => code,
        Err(error) => fail(error),
    }
}

/// `--dump-vm-trace FILE`: a `.hex` dump or raw `.INT` bytes, run from
/// offset 0 under a headless host with an empty world (no map, no objects,
/// no campaign globals, the clock at 0 ms).
fn dump_vm(args: &Args, file: &Path) -> ExitCode {
    let path = file.to_string_lossy();
    let bytes = if path.to_ascii_lowercase().ends_with(".hex") {
        std::fs::read_to_string(file)
            .map_err(|error| error.to_string())
            .and_then(|text| parse_hex_dump(&text).map_err(|error| error.to_string()))
    } else {
        std::fs::read(file).map_err(|error| error.to_string())
    };
    let program = match bytes.and_then(|bytes| IntProgram::parse(&bytes).map_err(|e| e.to_string()))
    {
        Ok(program) => program,
        Err(error) => return fail(format!("{path}: {error}")),
    };
    let mut host = match headless_host() {
        Ok(host) => host,
        Err(error) => return fail(error),
    };
    let mut vm = Vm::new(program);
    let trace = trace_vm(
        &mut vm,
        &mut host,
        &trace_program_name(&path),
        args.max_steps.unwrap_or(DEFAULT_MAX_STEPS),
    );
    if let Err(error) = emit(args.trace_out.as_deref(), &trace.text()) {
        return fail(error);
    }
    if trace.end.exit_code() != 0 {
        eprintln!(
            "asset-audit: the VM trace ended {:?} after {} rows",
            trace.end,
            trace.rows.len()
        );
    }
    ExitCode::from(trace.end.exit_code())
}
/// The headless host of the VM trace: an empty world over an empty
/// directory, so no host word finds a map, an object or a resource.
fn headless_host() -> Result<WorldHost<VfsWorldAssets>, Box<dyn Error>> {
    let empty = tempfile::tempdir()?;
    let vfs = Arc::new(LocalDirProvider::mount(empty.path())?);
    let assets = VfsWorldAssets::new(vfs, "english")?;
    Ok(WorldHost::new(WorldState::empty(1)?, assets))
}

/// SPEC-006 §3.4 as the desktop resolves `--seed`: -1 is an ambient seed
/// from two host draws (the entropy source is not part of the contract).
fn resolved_seed(seed: Option<i32>) -> i32 {
    match seed {
        Some(-1) => {
            use std::hash::{BuildHasher, Hasher};
            let draw = || {
                let mut hasher = std::collections::hash_map::RandomState::new().build_hasher();
                hasher.write_u8(0);
                hasher.finish() as u32
            };
            let [first, second] = [draw(), draw()];
            (first.wrapping_mul(65536).wrapping_add(second) & 0x7fff_ffff) as i32
        }
        seed => seed.unwrap_or(1),
    }
}
/// `--dump-combat-trace MAP`.
fn dump_combat(args: &Args, map: &str) -> Result<ExitCode, Box<dyn Error>> {
    let corpus = args
        .corpus_dir
        .as_ref()
        .ok_or("--dump-combat-trace needs --corpus-dir")?;
    let max_rounds = args.max_rounds.unwrap_or(DEFAULT_MAX_ROUNDS);
    if max_rounds == 0 {
        return Err("--max-rounds must be at least 1".into());
    }
    let vfs = Arc::new(LocalDirProvider::mount(corpus)?);
    let mut host = combat_trace_host(vfs, map, resolved_seed(args.seed))?;
    if trace_defender(&host.world).is_none() {
        return Err(format!("{map}: no critter can defend; no trace written").into());
    }
    let rows = run_combat_trace(&mut host, max_rounds)?;
    emit(args.trace_out.as_deref(), &write_combat_trace(&rows))?;
    Ok(ExitCode::SUCCESS)
}
