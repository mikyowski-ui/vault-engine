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
