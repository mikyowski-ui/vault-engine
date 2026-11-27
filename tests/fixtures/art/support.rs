/// A synthetic art list: `<prefix>0000.frm`, `<prefix>0001.frm` and so on,
/// one per line, `rows` of them (the brief's vector A is `art_list("s", 4100)`,
/// `s0000.frm` to `s4099.frm`). Names have at least four digits.
pub fn art_list(prefix: &str, rows: u32) -> String {
    (0..rows)
        .map(|row| format!("{prefix}{row:04}.frm\n"))
        .collect()
}
/// `vault-mod.toml` text with the extended-art tables: `art` rows are
/// (category, file, index) for `[[art]]`, `protos` rows the same for
/// `[[protos]]`, and `remaps` rows are (map, sidecar file) for `[[tile_remap]]`.
/// Only `name` is a required top-level key for these tables.
pub fn manifest(
    name: &str,
    art: &[(&str, &str, u32)],
    protos: &[(&str, &str, u32)],
    remaps: &[(&str, &str)],
) -> String {
    let mut text = format!("name = \"{name}\"\n");
    for (category, file, index) in art {
        text.push_str(&format!(
            "\n[[art]]\ncategory = \"{category}\"\nfile = \"{file}\"\nindex = {index}\n"
        ));
    }
    for (category, file, index) in protos {
        text.push_str(&format!(
            "\n[[protos]]\ncategory = \"{category}\"\nfile = \"{file}\"\nindex = {index}\n"
        ));
    }
    for (map, file) in remaps {
        text.push_str(&format!(
            "\n[[tile_remap]]\nmap = \"{map}\"\nfile = \"{file}\"\n"
        ));
    }
    text
}
/// A `VTLX` tile remap sidecar, big-endian: magic, version 1 (2 bytes), entry
/// count (2 bytes), then a 2-byte local index and a 4-byte row per pair. The
/// pairs are written as given, so a caller can build an invalid sidecar.
pub fn tile_remap(pairs: &[(u16, u32)]) -> Vec<u8> {
    let mut bytes = b"VTLX".to_vec();
    bytes.extend(1u16.to_be_bytes());
    bytes.extend((pairs.len() as u16).to_be_bytes());
    for (local, row) in pairs {
        bytes.extend(local.to_be_bytes());
        bytes.extend(row.to_be_bytes());
    }
    bytes
}
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builds_the_brief_vectors() {
        let list = art_list("s", 4100);
        assert_eq!(list.lines().count(), 4100);
        assert!(list.starts_with("s0000.frm\ns0001.frm\n"));
        assert!(list.ends_with("s4098.frm\ns4099.frm\n"));
        assert_eq!(
            tile_remap(&[(4000, 5000)]),
            [
                0x56, 0x54, 0x4C, 0x58, 0, 1, 0, 1, 0x0F, 0xA0, 0, 0, 0x13, 0x88
            ]
        );
        assert_eq!(tile_remap(&[]), [0x56, 0x54, 0x4C, 0x58, 0, 1, 0, 0]);
    }
}
