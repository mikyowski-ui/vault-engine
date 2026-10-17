use std::{error::Error, path::PathBuf, process::ExitCode};

use clap::Parser;
use vault_formats::{LocalDirProvider, Vfs};

const EXTENSIONS: [&str; 9] = [
    "frm", "pal", "fon", "aaf", "pro", "map", "int", "msg", "lst",
];

#[derive(Parser)]
#[command(about = "Count assets in a loose game directory without parsing their contents")]
struct Args {
    /// Unpacked directory containing art/, data/, color.pal, fonts, etc.
    #[arg(long, value_name = "PATH")]
    corpus_dir: PathBuf,
}

fn audit(args: Args) -> Result<(), Box<dyn Error>> {
    let vfs = LocalDirProvider::mount(args.corpus_dir)?;
    let mut counts = [0_u64; EXTENSIONS.len()];
    let mut total = 0_u64;
    for path in vfs.files() {
        total += 1;
        if let Some(index) = EXTENSIONS
            .iter()
            .position(|ext| Some(*ext) == path.extension())
        {
            counts[index] += 1;
        }
    }
    println!("Mounted files: {total}");
    println!("extension\tfiles");
    for (extension, count) in EXTENSIONS.iter().zip(counts) {
        println!(".{extension}\t{count}");
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
