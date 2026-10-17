use std::{error::Error, path::PathBuf, process::ExitCode};

use clap::Parser;
use vault_formats::{
    LocalDirProvider, LocalPrototypeCatalog, MAX_ASSET_BYTES, MapFile, Vfs, validate_asset,
};
use vault_vm::IntProgram;

const EXTENSIONS: [&str; 11] = [
    "frm", "pal", "fon", "aaf", "pro", "map", "int", "msg", "lst", "gam", "gcd",
];
const VALIDATED: [&str; 17] = [
    "pal", "frm", "fr0", "fr1", "fr2", "fr3", "fr4", "fr5", "lst", "fon", "aaf", "msg", "pro",
    "map", "int", "gam", "gcd",
];

#[derive(Parser)]
#[command(about = "Count and optionally validate assets in a loose game directory")]
struct Args {
    /// Unpacked directory containing art/, data/, color.pal, fonts, etc.
    #[arg(long, value_name = "PATH")]
    corpus_dir: PathBuf,
    /// Parse all supported formats, resolving MAP object subtypes through proto/ lists.
    #[arg(long)]
    validate_formats: bool,
}

fn audit(args: Args) -> Result<(), Box<dyn Error>> {
    let vfs = LocalDirProvider::mount(args.corpus_dir)?;
    let prototypes = LocalPrototypeCatalog::new(&vfs);
    let mut counts = [0_u64; EXTENSIONS.len()];
    let mut total = 0_u64;
    let mut passed = [0_u64; VALIDATED.len()];
    let mut failed = [0_u64; VALIDATED.len()];
    for path in vfs.files() {
        total += 1;
        if let Some(index) = EXTENSIONS
            .iter()
            .position(|ext| Some(*ext) == path.extension())
        {
            counts[index] += 1;
        }
        if args.validate_formats
            && let Some(index) = VALIDATED
                .iter()
                .position(|extension| Some(*extension) == path.extension())
        {
            let result = vfs
                .read_bounded(path.as_str(), MAX_ASSET_BYTES)
                .map_err(|error| error.to_string())
                .and_then(|bytes| match path.extension() {
                    Some("map") => MapFile::parse(&bytes, &prototypes)
                        .map(|_| ())
                        .map_err(|error| error.to_string()),
                    Some("int") => IntProgram::parse(&bytes)
                        .map(|_| ())
                        .map_err(|error| error.to_string()),
                    _ => validate_asset(path, &bytes)
                        .expect("supported extension")
                        .map_err(|error| error.to_string()),
                });
            match result {
                Ok(()) => passed[index] += 1,
                Err(error) => {
                    failed[index] += 1;
                    eprintln!("FAIL {path}: {error}");
                }
            }
        }
    }
    println!("Mounted files: {total}");
    println!("extension\tfiles");
    for (extension, count) in EXTENSIONS.iter().zip(counts) {
        println!(".{extension}\t{count}");
    }
    if args.validate_formats {
        println!("format\tpassed\tfailed");
        for (index, extension) in VALIDATED.iter().enumerate() {
            println!(".{extension}\t{}\t{}", passed[index], failed[index]);
        }
        let failures: u64 = failed.iter().sum();
        println!(
            "Validation: {} passed, {failures} failed",
            passed.iter().sum::<u64>()
        );
        if failures != 0 {
            return Err(format!("{failures} asset(s) failed validation").into());
        }
    }
    Ok(())
}

fn main() -> ExitCode {
    match audit(Args::parse()) {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("asset-audit: {error}");
            ExitCode::FAILURE
        }
    }
}
