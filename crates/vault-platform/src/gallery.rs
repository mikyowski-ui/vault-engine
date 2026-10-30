//! The documentation gallery (`cargo xtask capture-gallery`, which runs the
//! shell's hidden `--capture-gallery`): headless sessions over the synthetic
//! fixture corpus, composed on the CPU as the desktop composes them, and
//! written as indexed PNGs and looping GIFs into `docs/assets/`. No retail
//! data is read, so every picture is clean-room safe. Engine
//! infrastructure, not part of any specification.
//!
//! `xtask` stages the inputs in one folder: the synthetic corpus in
//! `corpus/`, restyled with `vault_fixtures::gallery`'s procedural art and
//! its showcase map, and two placeholder installs the launcher accepts,
//! `Fallout/` (MASTER.DAT and CRITTER.DAT) and `Fallout 2/` (master.dat
//! and critter.dat). Each session runs on a fixed clock, so the same
//! sources give the same files byte for byte.
//!
//! Every showcase session draws its world canvas through one SPEC-022
//! renderer: the reference path by default, or the path `--render-path`
//! names. The instanced path draws the same canvases, so the files are
//! the same on both paths.
mod demos;
mod encode;

use crate::{
    AppResult, EnhancedShown, bezel_layer,
    config::{Config, GamePaths},
    enhanced::EnhancedSettings,
    enhanced_layers,
    launcher::{self, Launcher, Setup, color, font},
    map_preview::{MapPreview, SharedRenderer},
    preview::{AssetPreview, PreviewArgs},
    replay::{enhanced_layout, interactive_world, pointer_at},
    session::{SessionConfig, SessionOrigin, SessionRuntime},
    side_tiles,
};
use encode::Indexed;
use std::{
    cell::RefCell,
    path::{Path, PathBuf},
};
use vault_core::{EnginePreset, GameProfile, WorldEffect, camera::CameraCommand};
use vault_formats::{HexGrid, Palette, pro::Category};
use vault_render::{IndexedImage, RenderPath};

/// The GIFs' frame rate: delays alternate 3, 3 and 4 hundredths.
const FPS: u64 = 30;
/// The animations' surface, a 16:9 window the size of a README column.
const MOVIE: [u32; 2] = [640, 360];
/// The comparison's frames: Vanilla's 640 by 480, and a 16:9 window as
/// tall.
const CLASSIC: [u32; 2] = [640, 480];
const WIDE: [u32; 2] = [854, 480];
/// The label band above each side-by-side picture, and the divider.
const BAND: u32 = 28;
const DIVIDER: u32 = 4;

thread_local! {
    /// SPEC-022 the renderer every showcase session started on this thread
    /// draws its world canvas with ([`with_renderer`]): the reference path
    /// unless a capture names another.
    static RENDERER: RefCell<SharedRenderer> = RefCell::new(SharedRenderer::default());
}

/// Run `capture` with every showcase session it starts drawing through
/// `renderer`, one renderer (and GPU device) for them all; the renderer
/// before it is back afterwards, even after a panic.
fn with_renderer<T>(renderer: SharedRenderer, capture: impl FnOnce() -> T) -> T {
    struct Restore(Option<SharedRenderer>);
    impl Drop for Restore {
        fn drop(&mut self) {
            if let Some(previous) = self.0.take() {
                RENDERER.with(|slot| slot.replace(previous));
            }
        }
    }
    let _restore = Restore(Some(RENDERER.with(|slot| slot.replace(renderer))));
    capture()
}

/// Write the gallery into `output` from the inputs staged in `stage`, the
/// world canvases drawn on `path` (SPEC-022: the files are the same on
/// either). The launcher's picture names the staged installs by paths
/// relative to `stage`, as a user's settings would, so the process must
/// run there (`cargo xtask capture-gallery` starts it there).
pub fn capture(output: &Path, stage: &Path, path: RenderPath) -> AppResult<()> {
    with_renderer(SharedRenderer::new(path), || capture_files(output, stage))
}

fn capture_files(output: &Path, stage: &Path) -> AppResult<()> {
    let output = std::path::absolute(output)?;
    let stage = std::path::absolute(stage)?;
    if std::env::current_dir()? != stage {
        return Err(format!(
            "--capture-gallery runs in its stage folder, {}",
            stage.display()
        )
        .into());
    }
    std::fs::create_dir_all(&output)?;
    let corpus = stage.join("corpus");
    // The export lands in the stage, named relative to it.
    let mut files = world_files(&corpus, Path::new("mods"))?;
    files.insert(1, ("launcher-overview.png", launcher_overview()?));
    for (name, bytes) in files {
        let path = output.join(name);
        std::fs::write(&path, &bytes).map_err(|error| format!("{}: {error}", path.display()))?;
        println!("{name}: {} bytes", bytes.len());
    }
    Ok(())
}

/// The gallery's files that show the world, in capture order (every file
/// but the launcher's): the debugger's export is written to `export`.
fn world_files(corpus: &Path, export: &Path) -> AppResult<Vec<(&'static str, Vec<u8>)>> {
    Ok(vec![
        ("compare-viewport.png", compare_viewport(corpus)?),
        ("roof-xray-peek.gif", roof_peek(corpus)?),
        ("tactical-zoom.gif", tactical_zoom(corpus)?),
        ("combat-overlay.gif", movie(corpus, demos::combat_overlay)?),
        (
            "widescreen-modals-qol.gif",
            movie(corpus, demos::widescreen_modals)?,
        ),
        (
            "f9-live-debugger.gif",
            movie(corpus, |corpus| demos::debugger(corpus, export))?,
        ),
        ("touch-ui-menu.gif", movie(corpus, demos::touch_menu)?),
    ])
}

/// A demonstration's frames as a looping GIF under the showcase's
/// palette.
fn movie(corpus: &Path, scene: impl Fn(&Path) -> AppResult<demos::Frames>) -> AppResult<Vec<u8>> {
    let frames = scene(corpus)?;
    let preview = world(corpus, EnginePreset::Enhanced)?;
    Ok(encode::gif(
        &frames,
        &display(preview.palette(), preview.brightness()),
    ))
}

/// The session that films the showcase map (`vault_fixtures::gallery`)
/// over the loose corpus, with the synthetic player, under `preset`: the
/// same [`SessionRuntime`] start as the desktop's map preview (SPEC-020
/// §2.3), from a [`SessionConfig`] of its own.
fn showcase(corpus: &Path, preset: EnginePreset) -> SessionConfig {
    SessionConfig::new(
        SessionOrigin::Gallery,
        PreviewArgs {
            preset,
            game_profile: GameProfile::Fallout1,
            corpus_dir: Some(corpus.to_path_buf()),
            preview_map: Some("maps/showcase.map".into()),
            player_gcd: Some("premade/synthetic.gcd".into()),
            language: Some("english".into()),
            seed: Some(1),
            ..Default::default()
        },
    )
}

/// The showcase session (see [`showcase`]), its player placed and its
/// first frame run.
fn world(corpus: &Path, preset: EnginePreset) -> AppResult<SessionRuntime> {
    let mut preview = SessionRuntime::start(showcase(corpus, preset))?
        .ok_or("the showcase configuration names no map")?;
    map(&mut preview).set_scene_renderer(RENDERER.with(|renderer| renderer.borrow().clone()));
    // The player at the building's door, the view over the building.
    let world = &mut map(&mut preview).host.world;
    let player = world.player;
    world.move_object(player, STAND, 0, false)?;
    world.set_view_center(VIEW);
    preview.tick(16)?;
    Ok(preview)
}

/// Where the gallery's player stands: just outside the showcase
/// building's door (hex column 100, row 109).
const STAND: i32 = 100 + 200 * 109;
/// The view's center over the building (column 101, row 100). The
/// building is 736 pixels across: a 640-pixel frame cuts its sides, a
/// widescreen view holds it.
const VIEW: i32 = 101 + 200 * 100;

/// The unlocked widescreen view, its camera centered on [`VIEW`] through a
/// scripted pan (`op_cam_pan_to`'s path).
fn widen(preview: &mut AssetPreview, smooth: bool) -> AppResult<()> {
    preview.set_enhanced(Some(EnhancedSettings {
        smooth_camera: smooth,
        ..EnhancedSettings::default()
    }));
    map(preview)
        .host
        .world
        .effects
        .push_back(WorldEffect::Camera(CameraCommand::PanTo {
            tile: VIEW,
            duration_ms: 0,
        }));
    preview.tick(32)?;
    Ok(())
}

fn map(preview: &mut AssetPreview) -> &mut MapPreview {
    interactive_world(preview).expect("a map preview")
}

/// The display colours of `palette` at its brightness, 8 bits a channel.
fn display(palette: &Palette, brightness: f64) -> [[u8; 3]; 256] {
    palette
        .display_rgba8(brightness)
        .map(|[r, g, b, _]| [r, g, b])
}

/// The index whose colour is nearest `rgb`, below the cycling range (229
/// on) and other than index 0; the lowest index wins a tie.
fn nearest(colors: &[[u8; 3]; 256], rgb: [u8; 3]) -> u8 {
    (1..229u8)
        .min_by_key(|index| {
            let color = colors[usize::from(*index)];
            (0..3)
                .map(|axis| (i32::from(color[axis]) - i32::from(rgb[axis])).pow(2))
                .sum::<i32>()
        })
        .unwrap_or(1)
}

/// The enhanced frame for `surface`, composed with the pointer's art as
/// the desktop's layers draw it.
fn enhanced_picture(
    preview: &mut AssetPreview,
    surface: [u32; 2],
    pointer: Option<[f64; 2]>,
) -> AppResult<IndexedImage> {
    let layout = enhanced_layout(preview, surface).ok_or("no unlocked viewport")?;
    if let Some(at) = pointer {
        preview.enhanced_pointer(pointer_at(&layout, Some(at)));
    }
    let frame = preview
        .enhanced_frame(&layout)
        .ok_or("no enhanced frame")??;
    let sides = frame
        .side_panel
        .as_ref()
        .map(|panel| side_tiles(&layout, panel))
        .unwrap_or_default();
    let palette = preview.palette().display_rgba8(preview.brightness());
    let bezel = if frame.bezel {
        bezel_layer(layout.surface, layout.ui_rect, &palette)
    } else {
        None
    };
    let shown = EnhancedShown {
        layout,
        frame,
        sides,
        bezel,
    };
    Ok(vault_render::compose_layers(
        &enhanced_layers(&shown, pointer),
        surface,
        0,
    )?)
}

/// Pictures side by side under a label band, an amber divider between
/// them: `ink` labels and the divider over `ground`.
fn side_by_side(pictures: &[(&str, &IndexedImage)], ground: u8, ink: u8) -> AppResult<Indexed> {
    let width = pictures.iter().map(|(_, image)| image.width()).sum::<u32>()
        + DIVIDER * (pictures.len() as u32 - 1);
    let height = BAND
        + pictures
            .iter()
            .map(|(_, image)| image.height())
            .max()
            .unwrap_or(0);
    let mut canvas = IndexedImage::new(width, height)?;
    canvas.pixels_mut().fill(ground);
    let mut x = 0;
    for (index, (label, image)) in pictures.iter().enumerate() {
        if index > 0 {
            canvas.fill_rect([x as i32, 0, DIVIDER as i32, height as i32], ink);
            x += DIVIDER;
        }
        let text = label.to_ascii_uppercase();
        let left = x as i32 + (image.width() as i32 - font::width(&text) * 2) / 2;
        font::draw_scaled(&mut canvas, [left, 6], &text, ink, 2);
        canvas.copy_image(image, [x as i32, BAND as i32]);
        x += image.width();
    }
    Ok(Indexed {
        width,
        height,
        pixels: canvas.pixels().to_vec(),
    })
}

/// `compare-viewport.png`: the synthetic map in Vanilla's 640x480 frame
/// beside the enhanced unlocked widescreen view of a 16:9 window as tall,
/// at the same zoom.
fn compare_viewport(corpus: &Path) -> AppResult<Vec<u8>> {
    let mut vanilla = world(corpus, EnginePreset::Vanilla)?;
    let classic = vanilla.image()?;
    debug_assert_eq!([classic.width(), classic.height()], CLASSIC);
    let mut enhanced = world(corpus, EnginePreset::Enhanced)?;
    widen(&mut enhanced, false)?;
    let wide = enhanced_picture(&mut enhanced, WIDE, None)?;
    let colors = display(enhanced.palette(), enhanced.brightness());
    let picture = side_by_side(
        &[("Vanilla 4:3", &classic), ("Enhanced Widescreen", &wide)],
        nearest(&colors, [12, 14, 12]),
        nearest(&colors, [255, 192, 64]),
    )?;
    Ok(encode::png(&picture, &colors))
}

/// `launcher-overview.png`: the launcher's Game data tab with each game's
/// profile active, both archives checked, from the staged installs.
fn launcher_overview() -> AppResult<Vec<u8>> {
    let paths = |folder: &str, master: &str, critter: &str| GamePaths {
        master_dat: Some(PathBuf::from(folder).join(master)),
        critter_dat: Some(PathBuf::from(folder).join(critter)),
        data_root: None,
    };
    let mut config = Config::with_profiles(
        GameProfile::Fallout1,
        crate::config::PerGame {
            fallout1: paths("Fallout", "MASTER.DAT", "CRITTER.DAT"),
            fallout2: paths("Fallout 2", "master.dat", "critter.dat"),
        },
    );
    config.preset = Some(crate::config::Preset::Enhanced);
    let mut launcher = Launcher::new(Setup {
        config,
        settings_path: None,
        save_dirs: Vec::new(),
        start_dir: PathBuf::from("."),
        problem: None,
        data_dirs: Default::default(),
    });
    let mut frames = Vec::new();
    for game in [GameProfile::Fallout1, GameProfile::Fallout2] {
        launcher.select_game(game);
        launcher
            .ready()
            .map_err(|reason| format!("the staged {game:?} install: {reason}"))?;
        frames.push(launcher.image());
    }
    let colors = launcher::palette().map(|[r, g, b, _]| [r, g, b]);
    let picture = side_by_side(
        &[
            ("Fallout 1 profile", &frames[0]),
            ("Fallout 2 profile", &frames[1]),
        ],
        color::BACKGROUND,
        color::FOCUS,
    )?;
    Ok(encode::png(&picture, &colors))
}

/// A frame's delay in hundredths: 3, 3, 4, ... for 30 frames a second.
fn delay(frame: u64) -> u16 {
    if frame % 3 == 2 { 4 } else { 3 }
}
/// The shell clock at a frame, from `start`.
fn clock(start: u64, frame: u64) -> u64 {
    start + frame * 1000 / FPS
}

/// The surface point over canvas pixel `pixel` (its center).
fn surface_point(preview: &mut AssetPreview, pixel: [f64; 2]) -> AppResult<[f64; 2]> {
    let layout = enhanced_layout(preview, MOVIE).ok_or("no unlocked viewport")?;
    let per_world = f64::from(layout.ui_scale * layout.zoom);
    Ok([0, 1].map(|axis| f64::from(layout.world_rect[axis]) + (pixel[axis] + 0.5) * per_world))
}

/// `roof-xray-peek.gif`: the showcase's roofed building with the NPC
/// inside. The pointer rests on the roof over the NPC, then Alt is held:
/// the cursor cutaway, its ring dissolving through the 4x4 ordered dither,
/// reveals the NPC while the pointer circles it, and the roof returns when
/// Alt is let go.
fn roof_peek(corpus: &Path) -> AppResult<Vec<u8>> {
    let mut preview = world(corpus, EnginePreset::Enhanced)?;
    widen(&mut preview, false)?;
    let target = {
        let world = map(&mut preview);
        let player = world.host.world.player;
        let npc = world
            .host
            .world
            .iter()
            .find(|(reference, object)| {
                object.category() == Category::Critter && *reference != player
            })
            .and_then(|(_, object)| object.raw.hex)
            .ok_or("the showcase map has no NPC")?;
        let [x, y] = HexGrid::default().hex_center(npc)?;
        [x as f64, y as f64 - 20.0]
    };
    preview.tick(32)?;
    let colors = display(preview.palette(), preview.brightness());
    let mut frames = Vec::new();
    const FRAMES: u64 = 108;
    for frame in 0..FRAMES {
        let now = clock(48, frame);
        preview.tick(now)?;
        // Alt is held from frame 18 to frame 89.
        let alt = (18..90).contains(&frame);
        preview.set_modifiers([false, false, alt]);
        let angle = (frame.saturating_sub(18) as f64 / 72.0) * std::f64::consts::TAU;
        let radius = if alt { 26.0 } else { 0.0 };
        let origin = map(&mut preview)
            .enhanced_view()
            .ok_or("no unlocked viewport")?
            .grid
            .origin;
        let pixel = [
            origin[0] as f64 + target[0] + radius * angle.cos(),
            origin[1] as f64 + target[1] + radius * angle.sin() * 0.6,
        ];
        let point = surface_point(&mut preview, pixel)?;
        let image = enhanced_picture(&mut preview, MOVIE, Some(point))?;
        frames.push((indexed(&image), delay(frame)));
    }
    Ok(encode::gif(&frames, &colors))
}

/// `tactical-zoom.gif`: the scripted camera, as the camera opcodes drive
/// it: out to 0.5x, a pan to the player at the door, a smooth zoom in to
/// 3.0x, a shake, then the reset back to following the player at 1.0x.
fn tactical_zoom(corpus: &Path) -> AppResult<Vec<u8>> {
    let mut preview = world(corpus, EnginePreset::Enhanced)?;
    widen(&mut preview, true)?;
    let colors = display(preview.palette(), preview.brightness());
    // Each request at its frame (the shell clock runs at 30 frames a second).
    let script: [(u64, CameraCommand); 5] = [
        (
            0,
            CameraCommand::SetZoom {
                percent: 50,
                duration_ms: 0,
            },
        ),
        (
            12,
            CameraCommand::PanTo {
                tile: STAND,
                duration_ms: 600,
            },
        ),
        (
            33,
            CameraCommand::SetZoom {
                percent: 300,
                duration_ms: 1800,
            },
        ),
        (
            96,
            CameraCommand::Shake {
                intensity: 8,
                duration_ms: 900,
            },
        ),
        (126, CameraCommand::Reset { duration_ms: 700 }),
    ];
    let mut frames = Vec::new();
    let mut zooms = (f32::MAX, f32::MIN);
    const FRAMES: u64 = 162;
    for frame in 0..FRAMES {
        for (_, command) in script.iter().filter(|(at, _)| *at == frame) {
            map(&mut preview)
                .host
                .world
                .effects
                .push_back(WorldEffect::Camera(*command));
        }
        preview.tick(clock(48, frame))?;
        let zoom = preview.enhanced_zoom();
        zooms = (zooms.0.min(zoom), zooms.1.max(zoom));
        let image = enhanced_picture(&mut preview, MOVIE, None)?;
        frames.push((indexed(&image), delay(frame)));
    }
    println!("tactical-zoom.gif: zoom {:.2}x to {:.2}x", zooms.0, zooms.1);
    Ok(encode::gif(&frames, &colors))
}

fn indexed(image: &IndexedImage) -> Indexed {
    Indexed {
        width: image.width(),
        height: image.height(),
        pixels: image.pixels().to_vec(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The gallery's corpus in a new temporary stage.
    fn stage() -> tempfile::TempDir {
        let stage = tempfile::tempdir().unwrap();
        let corpus = stage.path().join("corpus");
        std::fs::create_dir_all(&corpus).unwrap();
        vault_fixtures::state::write_corpus(&corpus);
        vault_fixtures::gallery::write_hud(&corpus);
        vault_fixtures::gallery::write_showcase(&corpus);
        stage
    }

    /// A GIF's size, its stored frames' delays (hundredths) and whether it
    /// loops, walking its blocks.
    fn gif_frames(file: &[u8]) -> ([u16; 2], Vec<u16>, bool) {
        assert_eq!(&file[..6], b"GIF89a");
        let size = [0, 2].map(|at| u16::from_le_bytes([file[6 + at], file[7 + at]]));
        assert_eq!(file[10] & 0x80, 0x80, "a global colour table");
        let mut at = 13 + 768;
        let (mut delays, mut loops) = (Vec::new(), false);
        let skip = |mut at: usize| {
            while file[at] != 0 {
                at += 1 + usize::from(file[at]);
            }
            at + 1
        };
        loop {
            match file[at] {
                0x21 => {
                    match file[at + 1] {
                        0xF9 => delays.push(u16::from_le_bytes([file[at + 4], file[at + 5]])),
                        0xFF => loops |= &file[at + 3..at + 14] == b"NETSCAPE2.0",
                        _ => {}
                    }
                    at = skip(at + 2);
                }
                0x2C => at = skip(at + 11),
                0x3B => return (size, delays, loops),
                other => panic!("block {other:#04x} at {at}"),
            }
        }
    }

    /// The fixture bar's art draws through the engine's bar: the steel
    /// chassis fills the bar window, the message monitor has its green
    /// edge (index 56, the cube's green), and the counters show digits.
    #[test]
    fn the_fixture_interface_bar_draws_through_the_engine() {
        let stage = stage();
        let mut preview = world(&stage.path().join("corpus"), EnginePreset::Vanilla).unwrap();
        let frame = preview.image().unwrap();
        let pixel = |[x, y]: [u32; 2]| frame.pixels()[(y * frame.width() + x) as usize];
        let bar = &frame.pixels()[379 * 640..];
        let painted = bar.iter().filter(|index| **index != 0).count();
        assert!(painted > bar.len() * 3 / 4, "{painted} of {}", bar.len());
        assert_eq!(pixel([60, 379 + 20]), 56);
        // The HP counter's cells (SPEC-014 §3.21, at 473, 40): lit segments.
        let lit = (0..33)
            .flat_map(|x| (0..17).map(move |y| [473 + x, 379 + 40 + y]))
            .filter(|at| pixel(*at) != 0)
            .count();
        assert!(lit > 20, "{lit} lit counter pixels");
        // Without the art the bar window stays black, as the tests'
        // corpus keeps it.
        let plain = tempfile::tempdir().unwrap();
        vault_fixtures::state::write_corpus(plain.path());
        vault_fixtures::gallery::write_showcase(plain.path());
        let mut blank = world(plain.path(), EnginePreset::Vanilla).unwrap();
        let frame = blank.image().unwrap();
        let bar = &frame.pixels()[379 * 640..];
        assert!(bar.iter().filter(|index| **index != 0).count() < bar.len() / 10);
    }

    #[test]
    fn the_viewport_comparison_is_the_same_each_time() {
        let stage = stage();
        let corpus = stage.path().join("corpus");
        let first = compare_viewport(&corpus).unwrap();
        assert_eq!(first, compare_viewport(&corpus).unwrap());
        assert_eq!(&first[..8], b"\x89PNG\r\n\x1a\n");
        let width = CLASSIC[0] + DIVIDER + WIDE[0];
        let height = BAND + CLASSIC[1];
        assert_eq!(
            &first[16..24],
            [width.to_be_bytes(), height.to_be_bytes()].concat()
        );
    }

    #[test]
    fn the_animations_loop_at_thirty_frames_a_second() {
        let stage = stage();
        let corpus = stage.path().join("corpus");
        for (file, frames) in [
            (roof_peek(&corpus).unwrap(), 108),
            (tactical_zoom(&corpus).unwrap(), 162),
        ] {
            let (size, delays, loops) = gif_frames(&file);
            assert_eq!(size, MOVIE.map(|side| side as u16));
            assert!(loops);
            // 30 frames a second: 10 hundredths every three frames.
            let total: u32 = delays.iter().map(|delay| u32::from(*delay)).sum();
            assert_eq!(total, frames / 3 * 10);
            assert!(delays.len() > 10, "{} stored frames", delays.len());
        }
    }

    /// The demonstrations of play: each a looping animation of the movie
    /// size with many changes, the debugger's export written to the stage.
    #[test]
    fn the_demonstrations_render_as_looping_animations() {
        let stage = stage();
        let corpus = stage.path().join("corpus");
        let export = stage.path().join("mods");
        let files = [
            ("combat", movie(&corpus, demos::combat_overlay)),
            ("windows", movie(&corpus, demos::widescreen_modals)),
            (
                "debugger",
                movie(&corpus, |corpus| demos::debugger(corpus, &export)),
            ),
            ("touch", movie(&corpus, demos::touch_menu)),
        ];
        for (name, file) in files {
            let (size, delays, loops) = gif_frames(&file.unwrap());
            assert_eq!(size, MOVIE.map(|side| side as u16), "{name}");
            assert!(loops, "{name}");
            assert!(delays.len() > 15, "{name}: {} stored frames", delays.len());
        }
        assert!(export.join("enhanced-setup/vault-mod.toml").is_file());
    }

    /// The gallery's world files drawn through `renderer`. The debugger's
    /// export goes to `mods` in `stage`, emptied first: its note names the
    /// folder, so both runs export to the same one.
    fn files_on(stage: &Path, renderer: SharedRenderer) -> Vec<(&'static str, Vec<u8>)> {
        let export = stage.join("mods");
        if export.exists() {
            std::fs::remove_dir_all(&export).unwrap();
        }
        with_renderer(renderer, || {
            world_files(&stage.join("corpus"), &export).unwrap()
        })
    }

    /// The files are byte for byte the same on both paths, and the
    /// instanced session repeats every refresh of the reference one.
    /// Returns its refreshes and refusal causes.
    fn assert_gallery_matches(
        instanced: SharedRenderer,
    ) -> (vault_debug::FrameRender, Vec<&'static str>) {
        let stage = stage();
        let reference = SharedRenderer::default();
        let expected = files_on(stage.path(), reference.clone());
        let files = files_on(stage.path(), instanced.clone());
        assert_eq!(expected.len(), 7);
        for ((name, expected), (other, file)) in expected.iter().zip(&files) {
            assert_eq!(name, other);
            assert!(expected == file, "{name} differs between the render paths");
        }
        // The instanced session repeats every refresh of the reference one.
        let drawn = reference.session_totals();
        assert_eq!(drawn.refreshes(), drawn.reference, "{drawn:?}");
        let totals = instanced.session_totals();
        assert_eq!(totals.refreshes(), drawn.reference, "{totals:?}");
        let causes = instanced.logged_causes();
        let counted: usize = causes.iter().map(|cause| instanced.refusals(cause)).sum();
        assert_eq!(counted, totals.refused, "{causes:?}");
        (totals, causes)
    }

    /// The gallery comparison run on the CPU executor (any host): the same
    /// files on both paths, every scene drawn by the instanced path, with
    /// no refusal (SPEC-022 §3.16).
    #[test]
    fn the_gallery_is_the_same_on_both_render_paths() {
        let (totals, causes) =
            assert_gallery_matches(crate::map_preview::test_support::cpu_renderer());
        println!("gallery on the CPU executor: {totals:?}, fallbacks {causes:?}");
        assert!(totals.instanced > 0, "{totals:?}");
        assert_eq!(totals.refused, 0, "{totals:?} {causes:?}");
        assert_eq!(totals.reference, 0, "{totals:?}");
        assert!(causes.is_empty(), "{causes:?}");
    }

    /// The same run on the GPU (a GPU test: it skips without an adapter
    /// unless `VAULT_REQUIRE_GPU=1`).
    #[test]
    fn the_gallery_on_the_gpu_is_the_same_as_the_reference() {
        let _gpu = crate::map_preview::test_support::gpu_lock();
        let Some(gpu) = crate::map_preview::test_support::gpu_renderer() else {
            return;
        };
        let (totals, causes) = assert_gallery_matches(SharedRenderer::from_renderer(gpu));
        println!("gallery on the GPU: {totals:?}, fallbacks {causes:?}");
        assert!(totals.instanced > 0, "{totals:?}");
        assert_eq!(totals.refused, 0, "{totals:?} {causes:?}");
        assert!(causes.is_empty(), "{causes:?}");
    }
}
