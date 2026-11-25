//! SPEC-024a §2.2 `vault-mod.toml`: the parser, then what a session does with
//! a manifest that parses: its registrations, its README, and the writer,
//! whose text reads back.
#![no_main]

use libfuzzer_sys::fuzz_target;
use vault_formats::mod_manifest::{ModManifest, ModRegistry, ModSetting};

/// Far above any manifest. The parser checks each key against the keys of its
/// section, so a longer input only spends the run on repeats.
const MAX_INPUT: usize = 1 << 16;

fuzz_target!(|data: &[u8]| {
    if data.len() > MAX_INPUT {
        return;
    }
    // A manifest that is not UTF-8 is refused before it reaches the parser.
    let Ok(text) = std::str::from_utf8(data) else {
        return;
    };
    let _ = ModSetting::parse(text);
    let Ok(manifest) = ModManifest::parse(text) else {
        return;
    };
    let _ = manifest.scripted();
    // Twice, so that two layers registering one row meet.
    let _ = ModRegistry::build([&manifest, &manifest]);
    let _ = manifest.readme(&[]);
    if let Ok(written) = manifest.to_toml() {
        let _ = ModManifest::parse(&written);
    }
});
