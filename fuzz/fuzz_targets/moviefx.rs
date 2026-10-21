//! SPEC-012 movie fade files, then the fade hook over a bounded run of
//! frames.
#![no_main]

use libfuzzer_sys::fuzz_target;
use vault_formats::moviefx::{self, FadeHook};

fuzz_target!(|data: &[u8]| {
    let Ok(effects) = moviefx::parse(data) else {
        return;
    };
    let Some(mut hook) = FadeHook::new(effects) else {
        return;
    };
    let table = [21_u8; 768];
    let mut device = [0_u8; 768];
    hook.push(&table, &mut device, 1, 254);
    for frame in 0..256 {
        let _ = hook.update(frame, &mut device);
    }
});
