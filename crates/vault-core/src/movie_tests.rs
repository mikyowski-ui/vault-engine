//! SPEC-012 §2.11 and §3.3-3.13: the game movie table and flags, paths,
//! the Pip-Boy selection, volume, the subtitle band, paced playback (sync
//! waits, late and dropped frames, timer changes, the final deadline),
//! fades and subtitles in playback, and the movie requests of the host
//! (`play_gmovie`, endgame, archive, water countdown).
use crate::{
    CharacterSheet, WorldEffect, WorldHost, WorldState,
    assets::VfsWorldAssets,
    fixture_state as state,
    movie::{self, MoviePlayback, MovieRequest, Step, flags},
};
use std::sync::Arc;
use vault_formats::{LocalDirProvider, moviefx, sve};
use vault_vm::{IntProgram, RunOutcome, Vm};

#[allow(dead_code)]
#[path = "../../../tests/fixtures/movie/support.rs"]
mod fixture;

/// A movie of chunks of `(type, version, payload)` opcodes, each chunk
/// ended by an end-of-chunk opcode, then an end-of-stream chunk.
fn movie(chunks: &[Vec<(u8, u8, Vec<u8>)>]) -> Vec<u8> {
    let mut bytes = fixture::header_a()[..26].to_vec();
    let mut bodies: Vec<Vec<u8>> = chunks
        .iter()
        .map(|opcodes| {
            let mut body = Vec::new();
            for (kind, version, payload) in opcodes {
                body.extend((payload.len() as u16).to_le_bytes());
                body.extend([*kind, *version]);
                body.extend(payload);
            }
            body.extend([0, 0, 1, 0]);
            body
        })
        .collect();
    bodies.push(vec![0, 0, 0, 0, 0, 0, 1, 0]);
    for body in bodies {
        bytes.extend((body.len() as u16).to_le_bytes());
        bytes.extend([0, 0]);
        bytes.extend(body);
    }
    bytes.extend([0; 4]);
    bytes
}
fn words(values: &[u16]) -> Vec<u8> {
    values
        .iter()
        .flat_map(|value| value.to_le_bytes())
        .collect()
}
fn frame(step: Step) -> movie::FrameStep {
    match step {
        Step::Frame(frame) => frame,
        other => panic!("expected a frame, got {other:?}"),
    }
}

#[test]
fn spec012_movie_table_flags_and_paths() {
    let names: Vec<_> = movie::GAME_MOVIES.iter().map(|movie| movie.file).collect();
    assert_eq!(
        names,
        [
            "iplogo.mve",
            "mplogo.mve",
            "intro.mve",
            "vexpld.mve",
            "cathexp.mve",
            "ovrintro.mve",
            "boil3.mve",
            "ovrrun.mve",
            "walkm.mve",
            "walkw.mve",
            "dipedv.mve",
            "boil1.mve",
            "boil2.mve",
            "raekills.mve"
        ]
    );
    for index in 0..14 {
        let expected = if matches!(index, 0 | 1 | 2 | 5 | 8 | 9 | 11 | 12) {
            0x0b
        } else {
            0x09
        };
        assert_eq!(movie::play_gmovie_flags(index, true), Some(expected));
        let without_map = expected | if matches!(index, 3 | 4) { 0x02 } else { 0 };
        assert_eq!(movie::play_gmovie_flags(index, false), Some(without_map));
    }
    assert_eq!(movie::play_gmovie_flags(14, true), None);
    assert_eq!(movie::play_gmovie_flags(-1, true), None);
    assert_eq!(movie::movie_path(2).unwrap(), r"art\cuts\intro.mve");
    assert_eq!(
        movie::subtitle_path("french", r"art\cuts\intro.mve"),
        r"text\french\cuts\intro.SVE"
    );
    assert_eq!(
        movie::fade_path(r"art\cuts\intro.mve"),
        r"art\cuts\intro.cfg"
    );
    assert_eq!(movie::fade_path(r"art\cuts.x\intro"), r"art\cuts.cfg");
    assert!(
        movie::subtitles_forced(6) && movie::subtitles_forced(11) && movie::subtitles_forced(12)
    );
    assert!(!movie::subtitles_forced(2));
    assert_eq!(
        flags::FADE_BEFORE | flags::FADE_AFTER | flags::PAUSE_MUSIC,
        0x0b
    );
    assert_eq!(flags::STOP_MUSIC, 0x04);
}

#[test]
fn spec012_pipboy_selection_volume_and_placement() {
    let mut played = [0_u8; 14];
    assert_eq!(movie::archive_selection(&played, 0), None, "safe rule");
    played[2] = 1; // Index 2 is below the archive's range.
    played[4] = 1;
    played[10] = 1;
    assert_eq!(movie::archive_movies(&played), [4, 10]);
    assert_eq!(movie::archive_selection(&played, 0), Some(4));
    assert_eq!(movie::archive_selection(&played, 1), Some(4));
    assert_eq!(movie::archive_selection(&played, 2), Some(10));
    assert_eq!(movie::archive_selection(&played, 3), None);
    assert_eq!(movie::archive_selection(&played, -1), None);
    // Volume: 0.94 x the music volume; attenuation in hundredths of a dB.
    assert_eq!(movie::movie_volume(32767, true), 30800);
    assert_eq!(movie::movie_volume(32767, false), 0);
    assert_eq!(movie::attenuation(30800), -89);
    assert_eq!(movie::attenuation(32767), 0);
    assert_eq!(movie::attenuation(0), -10000);
    assert_eq!(movie::attenuation(1), -10000, "clamped");
    assert!((movie::attenuation_gain(-2000) - 0.1).abs() < 1e-6);
    // §3.9 placement and Fixture F's band variant.
    assert_eq!(movie::frame_origin([432, 320]), [104, 80]);
    assert_eq!(movie::subtitle_band(10, 320, 80), [0, 435, 640, 14]);
    assert_eq!(movie::subtitle_band(12, 320, 80), [0, 434, 640, 16]);
    assert_eq!(
        movie::subtitle_band(10, 480, 0),
        [0, 475, 640, 5],
        "clipped"
    );
}

#[test]
fn spec012_playback_paces_frames_drops_late_ones_and_waits_at_the_end() {
    let timer = |rate: u16| (2, 0, [words(&[rate, 0]), words(&[1])].concat());
    let video = (5, 0, words(&[2, 1]));
    let sync = (4, 0, Vec::new());
    let show = |count: u16| (7, 0, words(&[0, count]));
    let bytes = movie(&[
        vec![timer(1000), video],
        vec![sync.clone(), show(0)],
        vec![sync.clone(), show(0)],
        vec![sync.clone(), show(0)],
        vec![(14, 0, Vec::new()), sync.clone(), show(0)],
        vec![sync.clone(), show(2)],
        vec![timer(2000), sync.clone(), show(0)],
    ]);
    let mut playback = MoviePlayback::new(bytes, Vec::new(), None).unwrap();
    // T = 1000 x 1 + 0: the first sync waits for deadline + T/4 = 1250.
    assert_eq!(playback.step(0).unwrap(), Step::Waiting);
    assert_eq!(playback.step(1249).unwrap(), Step::Waiting);
    let first = frame(playback.step(1250).unwrap());
    assert!(first.displayed && first.frame.number == 1);
    // At 5000 the next sync (2250) is 2750 late, beyond T/2: the frame
    // without a palette push is dropped.
    let second = frame(playback.step(5000).unwrap());
    assert!(second.dropped && !second.displayed);
    // The frame after a dropped frame is never late.
    let third = frame(playback.step(5000).unwrap());
    assert!(third.displayed && !third.dropped);
    // Suppressed frames are neither displayed nor dropped, and count.
    let fourth = frame(playback.step(9000).unwrap());
    assert!(!fourth.displayed && !fourth.dropped && fourth.frame.suppressed);
    // A late frame that pushes the palette is still displayed.
    let fifth = frame(playback.step(9000).unwrap());
    assert!(fifth.displayed && fifth.palette && fifth.frame.number == 5);
    assert_eq!(playback.dropped(), 1);
    // A new period waits for the current deadline (6000), then restarts
    // the deadline at now + 2000.
    assert_eq!(playback.step(5999).unwrap(), Step::Waiting);
    assert_eq!(playback.step(6000).unwrap(), Step::Waiting, "sync at 8500");
    assert_eq!(playback.step(8499).unwrap(), Step::Waiting);
    assert!(frame(playback.step(8500).unwrap()).displayed);
    // The end waits for the deadline once more (10000).
    assert_eq!(playback.step(9999).unwrap(), Step::Waiting);
    assert_eq!(playback.step(10000).unwrap(), Step::Finished);
    assert!(playback.finished());
    assert_eq!(playback.step(20000).unwrap(), Step::Finished);
}

#[test]
fn spec012_playback_of_the_synthetic_movie_with_audio_fades_and_subtitles() {
    let (bytes, _, frames) = fixture::synthetic('A');
    let frames = fixture::frames(frames);
    let effects = moviefx::parse(fixture::fades()).unwrap();
    let subtitles = sve::parse(b"0:first\n3:second\n3:\n99:never\n");
    let mut playback = MoviePlayback::new(bytes.to_vec(), effects, Some(subtitles)).unwrap();
    let mut now = 0;
    let mut shown = Vec::new();
    let mut drawn = Vec::new();
    let mut audio = Vec::new();
    loop {
        match playback.step(now).unwrap() {
            Step::Waiting => now += 1000,
            Step::Frame(step) => {
                assert!(playback.audio_started());
                audio.extend(playback.take_audio());
                let pixels = playback.movie().video().unwrap().current().to_vec();
                assert_eq!(pixels, frames[shown.len()]);
                // Fixture G effect A covers frames 0-9: k = frame + 1.
                let k = i64::from(step.frame.number) + 1;
                let table = playback.movie().palette_table();
                let expected = (0 - k * (0 - i64::from(table[3 * 7])) / 10) as u8;
                assert_eq!(playback.device_palette()[3 * 7], expected);
                assert!(step.palette);
                drawn.push(step.subtitles.clone());
                shown.push(step.displayed);
            }
            Step::Finished => break,
        }
        assert!(now < 10_000_000);
    }
    audio.extend(playback.take_audio());
    // Frame 5 is suppressed; at 15 fps none is late.
    assert_eq!(shown, [true, true, true, true, false]);
    assert_eq!(audio.len(), 656, "the whole track-0 stream");
    assert_eq!(
        drawn,
        [
            vec![b"first".to_vec()],
            vec![],
            vec![b"second".to_vec(), Vec::new()],
            vec![],
            vec![]
        ]
    );
    assert_eq!(playback.subtitle(), Some(b"".as_slice()));
    // T = 8341 x 8 + 4; five frames end after the deadline of the fifth.
    assert!(now >= 66_732 * 5);
    // A stream error ends the movie.
    let mut broken = bytes.to_vec();
    broken.truncate(bytes.len() - 4);
    let mut playback = MoviePlayback::new(broken, Vec::new(), None).unwrap();
    let mut failed = false;
    for now in (0..2_000_000).step_by(10_000) {
        match playback.step(now) {
            Err(_) => {
                failed = true;
                break;
            }
            Ok(Step::Finished) => break,
            Ok(_) => {}
        }
    }
    assert!(failed && playback.finished());
}

/// A host over a temporary corpus with movies, a fade file, and subtitles.
fn movie_host(dir: &std::path::Path) -> WorldHost<VfsWorldAssets> {
    state::write_corpus(dir);
    let (bytes, _, _) = fixture::synthetic('A');
    std::fs::create_dir_all(dir.join("art/cuts")).unwrap();
    std::fs::create_dir_all(dir.join("text/english/cuts")).unwrap();
    for name in [
        "intro.mve",
        "VEXPLD.MVE",
        "boil1.mve",
        "boil3.mve",
        "walkw.mve",
        "walkm.mve",
    ] {
        std::fs::write(dir.join("art/cuts").join(name), bytes).unwrap();
    }
    std::fs::write(dir.join("art/cuts/intro.cfg"), fixture::fades()).unwrap();
    std::fs::write(dir.join("text/english/cuts/intro.sve"), b"1:intro\n").unwrap();
    std::fs::write(dir.join("text/english/cuts/BOIL1.SVE"), b"1:boil\n").unwrap();
    let vfs = Arc::new(LocalDirProvider::mount(dir).unwrap());
    let assets = VfsWorldAssets::new(vfs, "english").unwrap();
    WorldHost::new(
        WorldState::new(CharacterSheet::default(), 1).unwrap(),
        assets,
    )
}
fn requests(host: &mut WorldHost<VfsWorldAssets>) -> Vec<MovieRequest> {
    host.world
        .effects
        .drain(..)
        .filter_map(|effect| match effect {
            WorldEffect::Movie(request) => Some(request),
            _ => None,
        })
        .collect()
}

#[test]
fn spec012_game_movie_requests_mark_played_and_choose_subtitles_and_fades() {
    let dir = tempfile::tempdir().unwrap();
    let mut host = movie_host(dir.path());
    // Intro: fade file found; subtitles follow the preference (off).
    assert!(host.play_game_movie(movie::INTRO, 0x0b));
    assert_eq!(
        requests(&mut host),
        [MovieRequest {
            index: Some(2),
            flags: 0x0b,
            path: r"art\cuts\intro.mve".into(),
            subtitles: None,
            fades: Some(r"art\cuts\intro.cfg".into()),
        }]
    );
    assert_eq!(host.world.persistence.movies[2], 1);
    host.set_subtitles(true);
    assert_eq!(host.world.persistence.preferences[9], 1);
    assert!(host.play_game_movie(movie::INTRO, 0));
    assert_eq!(
        requests(&mut host)[0].subtitles.as_deref(),
        Some(r"text\english\cuts\intro.SVE")
    );
    host.set_subtitles(false);
    // The boil movies always use their subtitles.
    assert!(host.play_game_movie(movie::BOIL1, 0x07));
    assert_eq!(
        requests(&mut host)[0].subtitles.as_deref(),
        Some(r"text\english\cuts\boil1.SVE")
    );
    // Missing movies (always raekills) fail and stay unplayed.
    assert!(!host.play_game_movie(13, 0x09));
    assert!(!host.play_game_movie(7, 0x09));
    assert!(!host.play_game_movie(14, 0));
    assert_eq!(host.world.persistence.movies[13], 0);
    assert_eq!(host.world.persistence.movies[7], 0);
    assert!(requests(&mut host).is_empty());
    // The endgame plays walkm or walkw by gender (stat 34).
    assert!(host.play_endgame_movie());
    assert_eq!(requests(&mut host)[0].index, Some(movie::WALKM));
    if let Some(sheet) = host.world.player_sheet_mut() {
        sheet.base[34] = 1;
    }
    assert!(host.play_endgame_movie());
    let walk = requests(&mut host).remove(0);
    assert_eq!((walk.index, walk.flags), (Some(movie::WALKW), 0));
    // The archive: played movies from index 3 on.
    host.world.persistence.movies = [0; 14];
    host.world.persistence.movies[2] = 1;
    assert!(!host.play_archive_movie(0), "nothing played from index 3");
    host.world.persistence.movies[3] = 1;
    host.world.persistence.movies[11] = 1;
    assert!(host.play_archive_movie(2));
    let archive = requests(&mut host).remove(0);
    assert_eq!((archive.index, archive.flags), (Some(movie::BOIL1), 0x0b));
}

#[test]
fn spec012_play_gmovie_opcode_flags_failures_and_no_result() {
    let dir = tempfile::tempdir().unwrap();
    let mut host = movie_host(dir.path());
    let program = IntProgram::parse(&state::program(
        &[b"x"],
        &[(b"start", state::returning(Vec::new()))],
    ))
    .unwrap();
    let run = |host: &mut WorldHost<VfsWorldAssets>, index: i32| {
        let code = [state::push(index), state::instruction(0x8115)].concat();
        let mut vm = Vm::fragment(program.clone(), &code).unwrap();
        assert!(matches!(vm.run(host, 100), Ok(RunOutcome::FragmentEnd)));
        assert_eq!(vm.value_stack.byte_len(), 0, "no result");
    };
    // No map is loaded: index 3 adds 0x02 to its 0x09.
    run(&mut host, 3);
    let request = requests(&mut host).remove(0);
    assert_eq!((request.index, request.flags), (Some(3), 0x0b));
    assert_eq!(request.path, r"art\cuts\vexpld.mve");
    // A failed play and an index outside 0..13 are diagnostics only.
    for index in [13, 14, -1] {
        run(&mut host, index);
        assert!(
            host.world
                .effects
                .iter()
                .any(|effect| matches!(effect, WorldEffect::Diagnostic(_)))
        );
        assert!(requests(&mut host).is_empty());
    }
    // A string operand raises the script error.
    let code = [state::literal(6), state::instruction(0x8115)].concat();
    let mut vm = Vm::fragment(program.clone(), &code).unwrap();
    assert!(vm.run(&mut host, 100).is_err());
}

#[test]
fn spec005_playmovie_completes_headless_and_waits_for_a_presentation_layer() {
    let dir = tempfile::tempdir().unwrap();
    let mut host = movie_host(dir.path());
    let program = IntProgram::parse(&state::program(
        &[br"art\cuts\intro"],
        &[(b"start", state::returning(Vec::new()))],
    ))
    .unwrap();
    let code = [state::literal(6), state::instruction(0x807a)].concat();
    let path = r"art\cuts\intro.mve";
    // Headless by default: the request is emitted (with `.mve` appended)
    // and the program runs on instead of waiting forever.
    assert_eq!(host.movie_policy, movie::MoviePolicy::CompleteImmediately);
    let mut vm = Vm::fragment(program.clone(), &code).unwrap();
    assert!(matches!(
        vm.run(&mut host, 100),
        Ok(RunOutcome::FragmentEnd)
    ));
    assert!(!host.library.movie_playing);
    assert_eq!(requests(&mut host)[0].path, path);
    // A presentation layer: the program waits until playback ends.
    host.movie_policy = movie::MoviePolicy::WaitUntilFinished;
    let mut vm = Vm::fragment(program.clone(), &code).unwrap();
    assert!(matches!(vm.run(&mut host, 100), Ok(RunOutcome::Waiting)));
    assert!(matches!(vm.run(&mut host, 100), Ok(RunOutcome::Waiting)));
    assert_eq!(requests(&mut host)[0].path, path);
    host.movie_finished();
    assert!(matches!(
        vm.run(&mut host, 100),
        Ok(RunOutcome::FragmentEnd)
    ));
    // A missing movie is a VM error under either policy.
    let missing = [state::literal(6), state::instruction(0x807a)].concat();
    let program = IntProgram::parse(&state::program(
        &[b"missing"],
        &[(b"start", state::returning(Vec::new()))],
    ))
    .unwrap();
    for policy in [
        movie::MoviePolicy::CompleteImmediately,
        movie::MoviePolicy::WaitUntilFinished,
    ] {
        host.movie_policy = policy;
        let mut vm = Vm::fragment(program.clone(), &missing).unwrap();
        assert!(vm.run(&mut host, 100).is_err());
    }
}

#[test]
fn spec012_midnight_water_countdown() {
    let dir = tempfile::tempdir().unwrap();
    let mut host = movie_host(dir.path());
    let mut gvars = vec![0; 200];
    gvars[150] = 102;
    host.world.gvars = Some(gvars);
    // Without a host-supplied water global nothing happens.
    host.water_countdown();
    assert_eq!(host.world.gvars.as_ref().unwrap()[150], 102);
    host.world.rules.water_global = Some(150);
    host.water_countdown();
    assert!(requests(&mut host).is_empty(), "101 left");
    host.water_countdown();
    let boil1 = requests(&mut host).remove(0);
    assert_eq!((boil1.index, boil1.flags), (Some(movie::BOIL1), 0x07));
    host.water_countdown();
    assert!(requests(&mut host).is_empty(), "boil1 already played");
    // Inside dialogue the water still drains, but nothing plays.
    host.world.gvars.as_mut().unwrap()[150] = 51;
    host.world.dialogue.open = true;
    host.water_countdown();
    assert!(requests(&mut host).is_empty());
    host.world.dialogue.open = false;
    host.water_countdown();
    assert!(requests(&mut host).is_empty(), "boil2.mve is absent");
    host.world.gvars.as_mut().unwrap()[150] = 1;
    host.water_countdown();
    let effects: Vec<_> = host.world.effects.drain(..).collect();
    assert!(
        matches!(&effects[0], WorldEffect::Movie(request) if request.index == Some(movie::BOIL3) && request.flags == 0x05)
    );
    assert!(
        matches!(effects[1], WorldEffect::EndGame),
        "then a quit request"
    );
    // A finished water-chip quest stops the countdown.
    host.world.gvars.as_mut().unwrap()[movie::WATER_CHIP_GLOBAL] = 2;
    host.water_countdown();
    assert_eq!(host.world.gvars.as_ref().unwrap()[150], 0);
}
