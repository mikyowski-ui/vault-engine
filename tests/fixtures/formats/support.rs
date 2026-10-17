// Shared test-only fixture expansion. Address ranges repeat exactly one byte.
pub fn hex(text: &str) -> Vec<u8> {
    text.split_whitespace()
        .map(|byte| u8::from_str_radix(byte, 16).unwrap())
        .collect()
}

pub fn sparse(text: &str) -> Vec<u8> {
    let mut bytes = Vec::new();
    for line in text.lines().filter(|line| !line.is_empty()) {
        let (address, payload) = line.split_once(':').unwrap();
        let (start, end) = address.split_once('-').unwrap_or((address, address));
        let start = usize::from_str_radix(start, 16).unwrap();
        let end = usize::from_str_radix(end, 16).unwrap();
        assert_eq!(
            start,
            bytes.len(),
            "fixture must explicitly cover every byte"
        );
        let payload = hex(payload);
        if address.contains('-') {
            assert_eq!(payload.len(), 1);
            bytes.resize(end + 1, payload[0]);
        } else {
            bytes.extend(payload);
        }
    }
    bytes
}

pub fn palette() -> Vec<u8> {
    sparse(include_str!("palette.hex"))
}

pub fn grayscale_palette() -> Vec<u8> {
    let mut bytes = palette();
    for index in 0..32 {
        bytes[index * 3..index * 3 + 3].fill(index as u8 * 2);
        bytes[768 + index * 1024 + index * 32 + index] = index as u8;
    }
    bytes
}

pub fn shared_frm() -> Vec<u8> {
    sparse(include_str!("shared.frm.hex"))
}
pub fn animated_frm() -> Vec<u8> {
    sparse(include_str!("animated.frm.hex"))
}
pub fn fon() -> Vec<u8> {
    sparse(include_str!("font.fon.hex"))
}
pub fn aaf() -> Vec<u8> {
    sparse(include_str!("font.aaf.hex"))
}

/// A complete synthetic corpus for CLI and desktop preview tests.
pub fn write_corpus(root: &std::path::Path) {
    std::fs::create_dir_all(root.join("art/items")).unwrap();
    std::fs::create_dir_all(root.join("proto/items")).unwrap();
    std::fs::create_dir_all(root.join("scripts")).unwrap();
    std::fs::create_dir_all(root.join("text/english/game")).unwrap();
    for (path, bytes) in [
        ("color.pal", grayscale_palette()),
        ("font0.fon", fon()),
        ("font0.aaf", aaf()),
        ("art/items/animated.frm", animated_frm()),
        ("art/items/shared.frm", shared_frm()),
        ("art/items/items.lst", include_bytes!("art.lst").to_vec()),
        (
            "proto/items/items.lst",
            include_bytes!("proto.lst").to_vec(),
        ),
        (
            "scripts/scripts.lst",
            include_bytes!("scripts.lst").to_vec(),
        ),
        (
            "text/english/game/sample.msg",
            include_bytes!("messages.msg").to_vec(),
        ),
        ("text/english/game/preview.msg", b"{1}{}{AB A B}".to_vec()),
    ] {
        std::fs::write(root.join(path), bytes).unwrap();
    }
    for direction in 0..6 {
        std::fs::write(
            root.join(format!("art/items/shared.fr{direction}")),
            shared_frm(),
        )
        .unwrap();
    }
}
