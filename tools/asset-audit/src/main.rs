use std::{collections::BTreeSet, error::Error, path::PathBuf, process::ExitCode};

use clap::Parser;
use vault_formats::{
    ArtCatalog, Fid, LocalDirProvider, LocalPrototypeCatalog, MAX_ASSET_BYTES, MapFile, Prototype,
    Vfs, VfsPath,
    map::MapObject,
    pro::{BasicFlags, ItemData, PrototypeData},
    validate_asset,
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
    /// Resolve every PRO/MAP art identifier through the SPEC-007 filename codec
    /// and require the selected file to exist.
    #[arg(long)]
    validate_art_ids: bool,
    /// Re-serialize every PRO and MAP through the SPEC-003 writers.
    #[arg(long)]
    validate_writers: bool,
}

/// The SPEC-003 writer's expected PRO bytes: the consumed payload with
/// weapon damage bounds exchanged, plus the scenery writer suffix.
fn expected_pro(bytes: &[u8], prototype: &Prototype) -> Vec<u8> {
    let mut expected = bytes[..prototype.consumed_len].to_vec();
    match &prototype.data {
        PrototypeData::Item(item) if matches!(item.data, ItemData::Weapon(_)) => {
            let (minimum, maximum) = expected[61..69].split_at_mut(4);
            minimum.swap_with_slice(maximum);
        }
        PrototypeData::Scenery(scenery) => {
            let light = prototype.light.expect("scenery has light words");
            for word in [
                light.radius,
                light.intensity,
                prototype.flags.0 as i32,
                prototype.extended.0 as i32,
                prototype.script.unwrap_or(-1),
                scenery.data.subtype() as i32,
            ] {
                expected.extend_from_slice(&word.to_be_bytes());
            }
        }
        _ => {}
    }
    expected
}

fn saved_objects(map: &MapFile) -> usize {
    map.objects
        .elevations
        .iter()
        .flatten()
        .filter(|object| object.hex.is_some() && !object.flags.contains(BasicFlags::TEMPORARY))
        .count()
}

fn check_writer(
    path: &VfsPath,
    bytes: &[u8],
    prototypes: &LocalPrototypeCatalog<'_>,
) -> Result<(), String> {
    let text = |error: vault_formats::ParseError| error.to_string();
    if path.extension() == Some("pro") {
        let prototype = Prototype::parse(bytes).map_err(text)?;
        if prototype.write() != expected_pro(bytes, &prototype) {
            return Err("PRO writer output differs from the documented transform".into());
        }
        return Ok(());
    }
    let map = MapFile::parse(bytes, prototypes).map_err(text)?;
    let first = map.write().map_err(text)?;
    let reread = MapFile::parse(&first, prototypes).map_err(text)?;
    if reread.write().map_err(text)? != first {
        return Err("MAP writer is not a parse/write fixpoint".into());
    }
    let scripts = |map: &MapFile| -> usize {
        map.scripts
            .iter()
            .flat_map(|category| category.active())
            .filter(|script| script.flags.0 & 8 == 0)
            .count()
    };
    if saved_objects(&map) != reread.objects.total || scripts(&map) != scripts(&reread) {
        return Err("MAP writer lost saved objects or retained scripts".into());
    }
    Ok(())
}

/// Art identifiers carried by a prototype; -1 marks an absent optional image.
fn prototype_art(prototype: &Prototype) -> Vec<(&'static str, u32)> {
    let mut art = vec![("art", prototype.art_id)];
    match &prototype.data {
        PrototypeData::Item(item) => {
            art.push(("inventory art", item.inventory_art));
            if let ItemData::Armor(armor) = &item.data {
                art.push(("male armor art", armor.male_art as u32));
                art.push(("female armor art", armor.female_art as u32));
            }
        }
        PrototypeData::Critter(critter) => art.push(("head art", critter.head_art)),
        _ => {}
    }
    art.retain(|(_, fid)| *fid != u32::MAX);
    art
}

fn map_art(object: &MapObject, art: &mut Vec<(&'static str, u32)>) {
    art.push(("object art", object.art_id));
    for child in &object.inventory {
        map_art(&child.object, art);
    }
}

/// Per source (`.pro`, `.map`): identifiers whose decoded file exists, whose
/// decoded file is absent from this corpus, and that the codec rejects.
fn audit_art_ids(vfs: &LocalDirProvider) -> Result<[[u64; 3]; 2], Box<dyn Error>> {
    let catalog = ArtCatalog::load(vfs)?;
    let prototypes = LocalPrototypeCatalog::new(vfs);
    let files: BTreeSet<&str> = vfs.files().map(VfsPath::as_str).collect();
    let mut counts = [[0; 3]; 2];
    for path in vfs.files() {
        let (source, art) = match path.extension() {
            Some("pro") => {
                let bytes = vfs.read_bounded(path.as_str(), MAX_ASSET_BYTES)?;
                (0, prototype_art(&Prototype::parse(&bytes)?))
            }
            Some("map") => {
                let bytes = vfs.read_bounded(path.as_str(), MAX_ASSET_BYTES)?;
                let map = MapFile::parse(&bytes, &prototypes)?;
                let mut art = Vec::new();
                for object in map.objects.elevations.iter().flatten() {
                    map_art(object, &mut art);
                }
                (1, art)
            }
            _ => continue,
        };
        for (field, fid) in art {
            match catalog
                .path(Fid(fid))
                .map_err(|error| error.to_string())
                .and_then(|art| VfsPath::new(&art).map_err(|error| error.to_string()))
            {
                Ok(art) if files.contains(art.as_str()) => counts[source][0] += 1,
                Ok(art) => {
                    counts[source][1] += 1;
                    eprintln!("MISSING {path}: {field} {fid:#010x}: {art}");
                }
                Err(error) => {
                    counts[source][2] += 1;
                    eprintln!("FAIL {path}: {field} {fid:#010x}: {error}");
                }
            }
        }
    }
    Ok(counts)
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
    if args.validate_writers {
        let mut counts = [[0_u64; 2]; 2];
        for path in vfs.files() {
            let source = match path.extension() {
                Some("pro") => 0,
                Some("map") => 1,
                _ => continue,
            };
            let result = vfs
                .read_bounded(path.as_str(), MAX_ASSET_BYTES)
                .map_err(|error| error.to_string())
                .and_then(|bytes| check_writer(path, &bytes, &prototypes));
            match result {
                Ok(()) => counts[source][0] += 1,
                Err(error) => {
                    counts[source][1] += 1;
                    eprintln!("FAIL {path}: {error}");
                }
            }
        }
        println!("writer\tpassed\tfailed");
        for (source, [passed, failed]) in ["pro", "map"].iter().zip(counts) {
            println!(".{source}\t{passed}\t{failed}");
        }
        let failures = counts[0][1] + counts[1][1];
        println!(
            "Writers: {} passed, {failures} failed",
            counts[0][0] + counts[1][0]
        );
        if failures != 0 {
            return Err(format!("{failures} writer round trip(s) failed").into());
        }
    }
    if args.validate_art_ids {
        let counts = audit_art_ids(&vfs)?;
        println!("art-id source\tresolved\tmissing\tinvalid");
        for (source, [resolved, missing, invalid]) in ["pro", "map"].iter().zip(counts) {
            println!(".{source}\t{resolved}\t{missing}\t{invalid}");
        }
        let [resolved, missing, invalid] =
            std::array::from_fn(|column| counts[0][column] + counts[1][column]);
        println!("Art identifiers: {resolved} resolved, {missing} missing, {invalid} invalid");
        if invalid != 0 {
            return Err(format!("{invalid} art identifier(s) cannot be decoded").into());
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
