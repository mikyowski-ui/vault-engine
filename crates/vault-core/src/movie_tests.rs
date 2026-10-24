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
    test_support::dir_assets,
};
use vault_formats::{moviefx, sve};
use vault_vm::{IntProgram, RunOutcome, Vm};

use vault_fixtures::movie as fixture;

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
    // §3.10 safe rule: a wrapped entry's band grows by one text height per
    // extra line, to the window bottom.
    assert_eq!(
        movie::subtitle_band_lines(10, 320, 80, 2),
        [0, 435, 640, 24]
    );
    assert_eq!(
        movie::subtitle_band_lines(10, 320, 80, 9),
        [0, 435, 640, 45]
    );
}

/// A `.FON` of 128 glyphs, 10 high with letter spacing 1: every glyph 9
/// wide except the space, 4 wide; all bitmaps blank.
fn subtitle_font() -> vault_formats::Font {
    let mut bytes = Vec::new();
    for word in [128, 10, 1, 0, 0] {
        bytes.extend_from_slice(&i32::to_le_bytes(word));
    }
    for code in 0..128 {
        let width = if code == 32 { 4 } else { 9 };
        bytes.extend_from_slice(&i32::to_le_bytes(width));
        bytes.extend_from_slice(&i32::to_le_bytes(0));
    }
    bytes.extend_from_slice(&[0; 20]);
    vault_formats::Font::parse_fon(&bytes).unwrap()
}

#[test]
fn spec012_subtitles_wrap_at_the_last_space_before_640_pixels() {
    let font = subtitle_font();
    // "AAAA" is 40 pixels with its spacing, a space 5. Fourteen words and
    // their spaces reach 630; the fifteenth word's second character takes
    // the sum to 649, past 640, so the line ends before the last space.
    let words = vec!["AAAA"; 16].join(" ").into_bytes();
    let lines = movie::subtitle_lines(&font, &words);
    assert_eq!(lines, [(0, 14 * 5 - 1), (14 * 5, words.len())]);
    let first = &words[lines[0].0..lines[0].1];
    let width = font.text_width(first, false) as i32;
    assert_eq!(width, 14 * 40 + 13 * 5);
    assert_eq!((640 - width) / 2, 7, "line 0 centered");
    // The line ends just before the last space before the overflow, so a
    // double space leaves one on the line; the spaces after the break are
    // skipped.
    let doubled = [vec!["AAAA"; 14].join(" ").as_str(), "  AAAA"]
        .concat()
        .into_bytes();
    let lines = movie::subtitle_lines(&font, &doubled);
    assert_eq!(lines, [(0, 14 * 5), (14 * 5 + 1, doubled.len())]);
    // Safe rule: no space before the overflow breaks before the
    // overflowing character (64 characters fill 640 pixels exactly).
    let solid = vec![b'A'; 80];
    assert_eq!(movie::subtitle_lines(&font, &solid), [(0, 64), (64, 80)]);
    // An entry that fits is one line; an empty one has none.
    assert_eq!(movie::subtitle_lines(&font, b"AB C"), [(0, 4)]);
    assert!(movie::subtitle_lines(&font, b"").is_empty());
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
    let world = WorldState::new(CharacterSheet::default(), 1).unwrap();
    WorldHost::new(world, dir_assets(dir))
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
    use crate::queue::EventKind;
    let dir = tempfile::tempdir().unwrap();
    let mut host = movie_host(dir.path());
    let water = |host: &WorldHost<VfsWorldAssets>| host.world.gvars.as_ref().unwrap()[10];
    let set = |host: &mut WorldHost<VfsWorldAssets>, index: usize, value: i32| {
        host.world.gvars.as_mut().unwrap()[index] = value;
    };
    // The remaining water is campaign global 10, with no host setup.
    host.world.gvars = Some(vec![0; 200]);
    set(&mut host, movie::WATER_GLOBAL, 102);
    host.water_countdown();
    assert_eq!(water(&host), 101);
    assert!(requests(&mut host).is_empty(), "101 left");
    host.water_countdown();
    let boil1 = requests(&mut host).remove(0);
    assert_eq!((boil1.index, boil1.flags), (Some(movie::BOIL1), 0x07));
    assert_eq!(host.world.persistence.campaign_status, movie::BOIL1_CHOSEN);
    // Its water-movie bit is set: boil1 is not chosen again.
    host.water_countdown();
    assert!(requests(&mut host).is_empty());
    // Inside dialogue the water still drains, but nothing is chosen.
    set(&mut host, movie::WATER_GLOBAL, 51);
    host.world.dialogue.open = true;
    host.water_countdown();
    assert_eq!(water(&host), 50);
    assert_eq!(host.world.persistence.campaign_status, movie::BOIL1_CHOSEN);
    host.world.dialogue.open = false;
    // boil2 is chosen (its bit set) but its file is absent, so it does not
    // play and is not chosen again.
    host.water_countdown();
    assert!(requests(&mut host).is_empty(), "boil2.mve is absent");
    assert_eq!(host.world.persistence.campaign_status, 0x03);
    host.water_countdown();
    assert!(requests(&mut host).is_empty());
    // At 0, boil3 plays, then a quit request.
    set(&mut host, movie::WATER_GLOBAL, 1);
    host.water_countdown();
    assert_eq!(water(&host), 0);
    let effects: Vec<_> = host.world.effects.drain(..).collect();
    assert!(
        matches!(&effects[0], WorldEffect::Movie(request) if request.index == Some(movie::BOIL3) && request.flags == 0x05)
    );
    assert!(
        matches!(effects[1], WorldEffect::EndGame),
        "then a quit request"
    );
    // A played boil3 does nothing more, and 0 never decrements.
    host.water_countdown();
    assert!(host.world.effects.is_empty());
    assert_eq!(water(&host), 0);
    // [May-improve] safe rule: 0 reached during a dialogue still chooses
    // boil3 at a later midnight outside dialogue.
    host.world.persistence.movies[movie::BOIL3] = 0;
    set(&mut host, movie::WATER_GLOBAL, 1);
    host.world.dialogue.open = true;
    host.water_countdown();
    assert!(host.world.effects.is_empty());
    host.world.dialogue.open = false;
    host.water_countdown();
    assert_eq!(requests(&mut host)[0].index, Some(movie::BOIL3));
    // A quest value other than 2 still counts down; 2 stops it.
    set(&mut host, movie::WATER_GLOBAL, 150);
    set(&mut host, movie::WATER_CHIP_GLOBAL, 3);
    host.water_countdown();
    assert_eq!(water(&host), 149);
    set(&mut host, movie::WATER_CHIP_GLOBAL, 2);
    host.water_countdown();
    assert_eq!(water(&host), 149);
    // Step 1: while a countdown movie plays, the midnight handler returns
    // at once: no unjam, no countdown and no next midnight.
    set(&mut host, movie::WATER_CHIP_GLOBAL, 0);
    set(&mut host, movie::WATER_GLOBAL, 100);
    host.world.persistence.campaign_status = 0;
    host.world.persistence.movies[movie::BOIL1] = 0;
    host.movie_policy = movie::MoviePolicy::WaitUntilFinished;
    host.water_countdown();
    assert_eq!(requests(&mut host)[0].index, Some(movie::BOIL1));
    host.world.queue = Default::default();
    host.world
        .queue
        .insert(0, 0, 0, EventKind::Midnight)
        .unwrap();
    host.process_queue().unwrap();
    assert_eq!(water(&host), 99, "the handler returned at once");
    assert!(
        host.world.queue.events().next().is_none(),
        "no next midnight"
    );
    host.movie_finished();
    host.world
        .queue
        .insert(0, 0, 0, EventKind::Midnight)
        .unwrap();
    host.process_queue().unwrap();
    assert_eq!(water(&host), 98);
}
