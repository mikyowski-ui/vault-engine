//! SPEC-002 `.FON` and `.AAF` fonts: parsing, then every glyph lookup and
//! text measurement over the input itself.
#![no_main]

use libfuzzer_sys::fuzz_target;
use vault_formats::Font;

fuzz_target!(|data: &[u8]| {
    for font in [Font::parse_fon(data), Font::parse_aaf(data)]
        .into_iter()
        .flatten()
    {
        for code in 0..=u8::MAX {
            let _ = font.glyph(code);
            let _ = font.character_width(code);
        }
        let _ = font.text_width(data, false);
        let _ = font.text_width(data, true);
    }
});
