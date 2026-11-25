//! SPEC-001 §3.7 and SPEC-022 §3.11 rule 2 in the shell: a script's
//! `loadpalettetable` replaces the tables a lit scene is drawn with, on both
//! render paths, and a fade, which changes only the display palette, does
//! not.
use super::*;
use crate::map_preview::test_support::{
    cpu_renderer, gpu_lock, gpu_renderer, interactive, synthetic_corpus,
};
use std::path::Path;
use vault_formats::Palette;
use vault_render::IndexedImage;

/// A palette file's bytes with every RGB555 result turned around: the
/// tables it generates relight every index differently from `color.pal`'s.
fn script_palette(root: &Path) -> Arc<Palette> {
    let mut bytes = std::fs::read(root.join("color.pal")).unwrap();
    for result in &mut bytes[768..0x8300] {
        *result = 255 - *result;
    }
    Arc::new(Palette::parse(&bytes).unwrap())
}

/// What `loadpalettetable` (0x806E) leaves in the library.
fn load_script_palette(preview: &mut MapPreview, palette: Option<Arc<Palette>>) {
    preview.host.library.palette = palette;
    preview.host.library.palette_generation += 1;
}

/// The frames of one session on `renderer`, with the tables built after
/// each: the map's palette, a repeat, a fade to black over it, the script's
/// palette, a repeat, and the map's palette again.
fn session(preview: &mut MapPreview, script: &Arc<Palette>) -> Vec<(IndexedImage, usize)> {
    let renderer = preview.scene_renderer().clone();
    let mut frames = Vec::new();
    let mut frame = |preview: &mut MapPreview| {
        let image = preview.map_view_image().unwrap();
        frames.push((image, renderer.lock().renderer.table_builds()));
    };
    preview.tick(16).unwrap();
    // The in-world cursor adds a translucent outline, drawn with a blend
    // table of the palette.
    preview.pointer_moved([300, 200]);
    frame(preview);
    frame(preview);

    preview.fades.push_game(true);
    let game = preview.palette.clone();
    preview.fades.tick(100, &game);
    preview.fades.tick(450, &game);
    assert!(preview.display_override().is_some(), "the fade runs");
    frame(preview);
    preview.fades.tick(800, &game);
    preview.fades.tick(801, &game);
    assert!(preview.display_override().is_some(), "black stays");
    frame(preview);
    preview.fades = Default::default();

    load_script_palette(preview, Some(script.clone()));
    frame(preview);
    frame(preview);
    load_script_palette(preview, None);
    frame(preview);
    frames
}

/// What a session proves on one path: a fade reaches neither the canvas nor
/// the tables, the script's palette changes both, a repeat builds nothing,
/// and removing the palette draws the first canvas again. The reference
/// path keeps no tables between refreshes.
fn check_session(frames: &[(IndexedImage, usize)], label: &str, builds: [usize; 7]) {
    let pixels: Vec<&[u8]> = frames.iter().map(|(image, _)| image.pixels()).collect();
    assert_eq!(pixels.len(), 7, "{label}");
    assert!(pixels[1] == pixels[0], "{label}: a repeat");
    assert!(pixels[2] == pixels[0], "{label}: a running fade");
    assert!(pixels[3] == pixels[0], "{label}: a finished fade");
    assert!(pixels[4] != pixels[0], "{label}: the script's palette");
    assert!(pixels[5] == pixels[4], "{label}: its repeat");
    assert!(pixels[6] == pixels[0], "{label}: the map's palette again");
    let built: Vec<usize> = frames.iter().map(|(_, built)| *built).collect();
    assert_eq!(built, builds, "{label}: table builds");
}

#[test]
fn a_script_palette_relights_the_map_on_both_paths() {
    let dir = synthetic_corpus();
    let script = script_palette(dir.path());
    let mut reference = interactive(dir.path());
    let expected = session(&mut reference, &script);
    check_session(&expected, "reference", [0; 7]);

    let mut instanced = interactive(dir.path());
    let renderer = cpu_renderer();
    instanced.set_scene_renderer(renderer.clone());
    let frames = session(&mut instanced, &script);
    check_session(&frames, "instanced", [1, 1, 1, 1, 2, 2, 3]);
    assert_eq!(renderer.session_totals().refused, 0);
    assert!(renderer.logged_causes().is_empty());
    assert_eq!(renderer.session_totals().instanced, frames.len());

    for (index, ((want, _), (got, _))) in expected.iter().zip(&frames).enumerate() {
        assert!(want.pixels() == got.pixels(), "frame {index}");
    }
}

/// The same frames on the GPU executor (a GPU test: it skips without an
/// adapter unless `VAULT_REQUIRE_GPU=1`).
#[test]
fn a_script_palette_relights_the_map_on_the_gpu() {
    let _gpu = gpu_lock();
    let Some(gpu) = gpu_renderer() else {
        return;
    };
    let dir = synthetic_corpus();
    let script = script_palette(dir.path());
    let expected = session(&mut interactive(dir.path()), &script);
    let mut instanced = interactive(dir.path());
    let renderer = SharedRenderer::from_renderer(gpu);
    instanced.set_scene_renderer(renderer.clone());
    let frames = session(&mut instanced, &script);
    for (index, ((want, _), (got, _))) in expected.iter().zip(&frames).enumerate() {
        assert!(want.pixels() == got.pixels(), "frame {index}");
    }
    drop(instanced);
    drop(renderer);
}

/// One frame of several refreshes (the `enhanced` canvas's tiles) compares
/// the palette's contents once, and the next frame again.
#[test]
fn a_frame_of_many_refreshes_compares_the_palette_once() {
    let dir = synthetic_corpus();
    let script = script_palette(dir.path());
    let mut preview = interactive(dir.path());
    let renderer = cpu_renderer();
    preview.set_scene_renderer(renderer.clone());
    preview.tick(16).unwrap();
    let view = preview.view;
    let tiles = [(view, [0, 0]); 6];
    let counts = |renderer: &SharedRenderer| {
        let shared = renderer.lock();
        (
            shared.renderer.table_builds(),
            shared.renderer.table_comparisons(),
        )
    };
    preview
        .render_scenes(&tiles, None, None, None, false)
        .unwrap();
    assert_eq!(counts(&renderer), (1, 0));
    preview
        .render_scenes(&tiles, None, None, None, false)
        .unwrap();
    assert_eq!(counts(&renderer), (1, 1));
    load_script_palette(&mut preview, Some(script));
    preview
        .render_scenes(&tiles, None, None, None, false)
        .unwrap();
    assert_eq!(counts(&renderer), (2, 2));
}
