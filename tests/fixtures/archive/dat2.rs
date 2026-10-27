//! SPEC-015 fixtures: the zlib test vectors (`tests/fixtures/zlib/`) with
//! their regenerated plain data, and a builder of synthetic version 2
//! archives (little-endian; member data, then the directory tree, then the
//! 8-byte footer).

/// `tests/fixtures/zlib/vectors.md`'s `lcg(seed, n)`.
pub fn lcg(seed: u32, count: usize) -> Vec<u8> {
    let mut state = seed;
    (0..count)
        .map(|_| {
            state = state.wrapping_mul(1_103_515_245).wrapping_add(12345) & 0x7fff_ffff;
            (state >> 16) as u8
        })
        .collect()
}

/// `tests/fixtures/zlib/vectors.md`'s `words(seed, n)`.
pub fn words(seed: u32, count: usize) -> Vec<u8> {
    const WORDS: [&str; 10] = [
        "vault",
        "engine",
        "water",
        "chip",
        "overseer",
        "radscorpion",
        "shady",
        "sands",
        "junktown",
        "hub",
    ];
    lcg(seed, count)
        .into_iter()
        .flat_map(|byte| format!("{} ", WORDS[usize::from(byte) % WORDS.len()]).into_bytes())
        .collect()
}

/// One zlib vector: its name, its plain data and its stream.
pub struct Vector {
    pub name: String,
    pub plain: Vec<u8>,
    pub stream: Vec<u8>,
}

/// Every zlib vector, in file order.
pub fn zlib_vectors() -> Vec<Vector> {
    let text = include_str!("../zlib/vectors.hex");
    let mut vectors: Vec<(String, Vec<u8>)> = Vec::new();
    for line in text.lines() {
        if line.starts_with('#') || line.trim().is_empty() {
            continue;
        }
        if let Some(header) = line.strip_prefix('@') {
            let name = header.split_whitespace().next().unwrap().to_owned();
            vectors.push((name, Vec::new()));
            continue;
        }
        let stream = &mut vectors.last_mut().unwrap().1;
        stream.extend(
            line.split_whitespace()
                .map(|byte| u8::from_str_radix(byte, 16).unwrap()),
        );
    }
    vectors
        .into_iter()
        .map(|(name, stream)| {
            let plain = match name.as_str() {
                "empty" => Vec::new(),
                "text" => b"Vault Engine zlib vector: fixed Huffman codes.\n".to_vec(),
                "words" => words(7, 4000),
                "noise" => lcg(11, 4096),
                "stored" => lcg(13, 70000),
                "repeat" => b"abcdefgh".repeat(5000),
                other => panic!("unknown vector {other}"),
            };
            Vector {
                name,
                plain,
                stream,
            }
        })
        .collect()
}

/// The vector named `name`.
pub fn zlib_vector(name: &str) -> Vector {
    zlib_vectors()
        .into_iter()
        .find(|vector| vector.name == name)
        .unwrap()
}

/// A version 2 archive member: its stored path, its stored bytes, whether
/// they are a zlib stream, and its logical size.
pub struct Member {
    pub path: String,
    pub stored: Vec<u8>,
    pub compressed: bool,
    pub real_size: u32,
}

impl Member {
    /// A stored (uncompressed) member.
    pub fn stored(path: &str, bytes: &[u8]) -> Self {
        Self {
            path: path.to_owned(),
            stored: bytes.to_vec(),
            compressed: false,
            real_size: bytes.len() as u32,
        }
    }
    /// A compressed member holding zlib vector `vector`.
    pub fn zlib(path: &str, vector: &Vector) -> Self {
        Self {
            path: path.to_owned(),
            stored: vector.stream.clone(),
            compressed: true,
            real_size: vector.plain.len() as u32,
        }
    }
}

/// A version 2 archive of `members`: their stored bytes in order from
/// offset 0, then the tree, then the footer.
pub fn archive(members: &[Member]) -> Vec<u8> {
    let mut bytes = Vec::new();
    let mut tree = (members.len() as u32).to_le_bytes().to_vec();
    for member in members {
        let offset = bytes.len() as u32;
        bytes.extend(&member.stored);
        tree.extend((member.path.len() as u32).to_le_bytes());
        tree.extend(member.path.as_bytes());
        tree.push(u8::from(member.compressed));
        tree.extend(member.real_size.to_le_bytes());
        tree.extend((member.stored.len() as u32).to_le_bytes());
        tree.extend(offset.to_le_bytes());
    }
    let tree_size = tree.len() as u32;
    bytes.extend(tree);
    let file_size = bytes.len() as u32 + 8;
    bytes.extend(tree_size.to_le_bytes());
    bytes.extend(file_size.to_le_bytes());
    bytes
}

/// The two archives of a synthetic Fallout 2 install made from a synthetic
/// Fallout 1 corpus in `corpus`, written to `out`:
/// - every `.map` gets version 20 (SPEC-016 §2);
/// - every critter prototype gets SPEC-016 §3.1's trailing `damage_type`
///   word (`damage_type`), making 416 bytes;
/// - `art/critters/` goes to `CRITTER.DAT`, everything else to `MASTER.DAT`,
///   as stored members with backslash paths (SPEC-015).
///
/// Returns the master and critter archive paths.
pub fn fallout2_install(
    corpus: &std::path::Path,
    out: &std::path::Path,
    damage_type: u32,
) -> (std::path::PathBuf, std::path::PathBuf) {
    fn walk(dir: &std::path::Path, found: &mut Vec<std::path::PathBuf>) {
        let mut entries: Vec<_> = std::fs::read_dir(dir)
            .unwrap()
            .map(|entry| entry.unwrap().path())
            .collect();
        entries.sort();
        for path in entries {
            if path.is_dir() {
                walk(&path, found);
            } else {
                found.push(path);
            }
        }
    }
    let mut files = Vec::new();
    walk(corpus, &mut files);
    let (mut master, mut critter) = (Vec::new(), Vec::new());
    for path in files {
        let relative = path.strip_prefix(corpus).unwrap();
        let name = relative
            .components()
            .map(|part| part.as_os_str().to_str().unwrap().to_owned())
            .collect::<Vec<_>>()
            .join("\\");
        let lower = name.to_ascii_lowercase();
        let mut bytes = std::fs::read(&path).unwrap();
        if lower.ends_with(".map") && bytes.len() >= 4 {
            bytes[..4].copy_from_slice(&20i32.to_be_bytes());
        }
        if lower.starts_with("proto\\critters\\") && lower.ends_with(".pro") && bytes.len() == 412 {
            bytes.extend(damage_type.to_be_bytes());
        }
        let member = Member::stored(&name, &bytes);
        if lower.starts_with("art\\critters\\") {
            critter.push(member);
        } else {
            master.push(member);
        }
    }
    std::fs::create_dir_all(out).unwrap();
    let (master_path, critter_path) = (out.join("MASTER.DAT"), out.join("CRITTER.DAT"));
    std::fs::write(&master_path, archive(&master)).unwrap();
    std::fs::write(&critter_path, archive(&critter)).unwrap();
    (master_path, critter_path)
}

/// A Fallout 2 game folder the launcher's checks accept: `master.dat` with
/// each of `required` (SPEC-018 §2.2's members, passed in by the caller) as
/// a one-byte stored member, and `critter.dat` with one zlib member under
/// `art/critters/`.
pub fn launcher_install(dir: &std::path::Path, required: &[&str]) {
    let names: Vec<String> = required
        .iter()
        .map(|path| path.to_ascii_uppercase().replace('/', "\\"))
        .collect();
    let members: Vec<Member> = names
        .iter()
        .map(|name| Member::stored(name, b"x"))
        .collect();
    std::fs::write(dir.join("master.dat"), archive(&members)).unwrap();
    std::fs::write(
        dir.join("critter.dat"),
        archive(&[Member::zlib(
            "ART\\CRITTERS\\HERO.FRM",
            &zlib_vector("repeat"),
        )]),
    )
    .unwrap();
}
