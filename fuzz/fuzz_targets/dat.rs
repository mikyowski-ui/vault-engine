//! SPEC-008 `.DAT` archives: the in-memory parser and the streamed reader
//! must both reject malformed tables and payloads without panicking.
#![no_main]

use libfuzzer_sys::fuzz_target;
use std::io::Cursor;
use vault_formats::dat::{DatArchive, DatTables};

fuzz_target!(|data: &[u8]| {
    if let Ok(archive) = DatArchive::parse(data.to_vec()) {
        for path in archive.paths().collect::<Vec<_>>() {
            if let Some(entry) = archive.entry(&path) {
                let _ = archive.extract(entry);
            }
        }
    }
    let mut stream = Cursor::new(data);
    if let Ok(tables) = DatTables::read(&mut stream) {
        for directory in &tables.directories {
            for file in &directory.files {
                let _ = file.read_from(&mut stream);
            }
        }
    }
});
