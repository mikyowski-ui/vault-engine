use super::test_support::{interactive, synthetic_corpus};
use super::*;
use vault_formats::LocalDirProvider;
use winit::keyboard::KeyCode;

#[test]
fn spec010_effect_rectangles_grow_while_outlined() {
    let window = [640, 380];
    // A 20 x 30 frame whose right column is x = -1: off screen, until
    // an outline grows it by a pixel on every side.
    let sprite = Some(([-20, 100], [20, 30]));
    assert_eq!(effect_rect(sprite, 0), [-20, 100, -1, 129]);
    assert!(!overlaps_window(effect_rect(sprite, 0), window));
    assert_eq!(effect_rect(sprite, 1), [-21, 99, 0, 130]);
    assert!(overlaps_window(effect_rect(sprite, 1), window));
    // Any outline type counts (16 item, an unlisted 3); bit 31
    // disables it and type 0 is no outline.
    assert!(overlaps_window(effect_rect(sprite, 16), window));
    assert!(overlaps_window(effect_rect(sprite, 3), window));
    assert!(!overlaps_window(effect_rect(sprite, 0x8000_0001), window));
    assert!(!overlaps_window(effect_rect(sprite, 0x0100_0000), window));
    // Inclusive bounds at the far edges.
    assert!(overlaps_window(
        effect_rect(Some(([639, 379], [5, 5])), 0),
        window
    ));
    assert!(!overlaps_window(
        effect_rect(Some(([640, 0], [5, 5])), 0),
        window
    ));
    assert!(!overlaps_window(
        effect_rect(Some(([0, 380], [5, 5])), 0),
        window
    ));
    // Unreadable art: (0, 0)-(0, 0), which overlaps the window.
    assert_eq!(effect_rect(None, 1), [0; 4]);
    assert!(overlaps_window(effect_rect(None, 0), window));
}

/// Every file under `root`, as VFS-relative paths and bytes.
fn files(root: &std::path::Path) -> Vec<(String, Vec<u8>)> {
    let mut found = Vec::new();
    let mut pending = vec![root.to_path_buf()];
    while let Some(directory) = pending.pop() {
        for entry in std::fs::read_dir(directory).unwrap() {
            let path = entry.unwrap().path();
            if path.is_dir() {
                pending.push(path);
            } else {
                let relative = path
                    .strip_prefix(root)
                    .unwrap()
                    .to_string_lossy()
                    .replace('\\', "/");
                found.push((relative, std::fs::read(&path).unwrap()));
            }
        }
    }
    found.sort();
    found
}
/// A SPEC-008 archive of raw (`0x20`) members: the root directory
/// table (`.` first), each directory's file table, then the payloads.
fn dat(files: &[(String, Vec<u8>)]) -> Vec<u8> {
    let mut directories: BTreeMap<String, Vec<(String, &[u8])>> = BTreeMap::new();
    directories.entry(".".into()).or_default();
    for (path, bytes) in files {
        let path = path.to_ascii_uppercase().replace('/', "\\");
        let (directory, file) = path
            .rsplit_once('\\')
            .map_or((".".to_owned(), path.clone()), |(d, f)| {
                (d.to_owned(), f.to_owned())
            });
        directories
            .entry(directory)
            .or_default()
            .push((file, bytes));
    }
    let word = |out: &mut Vec<u8>, value: usize| out.extend((value as u32).to_be_bytes());
    let mut root = Vec::new();
    for value in [directories.len(), 0, 0, 0] {
        word(&mut root, value);
    }
    for name in directories.keys() {
        root.push(name.len() as u8);
        root.extend(name.as_bytes());
    }
    let tables: usize = directories
        .values()
        .map(|files| {
            16 + files
                .iter()
                .map(|(name, _)| 1 + name.len() + 16)
                .sum::<usize>()
        })
        .sum();
    let (mut table, mut payload) = (Vec::new(), Vec::new());
    let mut offset = root.len() + tables;
    for files in directories.values() {
        for value in [files.len(), 0, 16, 0] {
            word(&mut table, value);
        }
        for (name, bytes) in files {
            table.push(name.len() as u8);
            table.extend(name.as_bytes());
            for value in [0x20, offset, bytes.len(), bytes.len()] {
                word(&mut table, value);
            }
            payload.extend_from_slice(bytes);
            offset += bytes.len();
        }
    }
    [root, table, payload].concat()
}
/// SPEC-014 §3.44: the entrance placement reinserts the player last
/// on its hex, over the synthetic item there; the item is moved onto
/// that hex again, so it is drawn (and picked) on top.
fn raise_item(preview: &mut MapPreview) {
    let item = preview
        .host
        .world
        .iter()
        .find(|(_, object)| object.raw.pid == 1 && object.raw.hex == Some(20100))
        .map(|(reference, _)| reference)
        .unwrap();
    preview
        .host
        .world
        .move_object(item, 20100, 0, false)
        .unwrap();
}

#[test]
fn dat_archives_supply_the_world_like_the_loose_corpus() {
    let dir = tempfile::tempdir().unwrap();
    let loose = dir.path().join("loose");
    state_fixture::write_corpus(&loose);
    let (critter, master): (Vec<_>, Vec<_>) = files(&loose)
        .into_iter()
        .partition(|(path, _)| path.starts_with("art/critters/"));
    assert!(!critter.is_empty());
    std::fs::write(dir.path().join("MASTER.DAT"), dat(&master)).unwrap();
    std::fs::write(dir.path().join("CRITTER.DAT"), dat(&critter)).unwrap();
    let patch = dir.path().join("patch");
    std::fs::create_dir_all(patch.join("data")).unwrap();
    std::fs::write(patch.join("data/patch.txt"), b"loose").unwrap();
    let args = crate::preview::PreviewArgs {
        corpus_dir: Some(patch),
        master_dat: Some(dir.path().join("MASTER.DAT")),
        critter_dat: Some(dir.path().join("CRITTER.DAT")),
        preview_map: Some("maps/synthetic.map".into()),
        player_gcd: Some("premade/synthetic.gcd".into()),
        ..Default::default()
    };
    // Critter art comes from CRITTER.DAT only; the loose root comes first.
    let vfs = args.source().unwrap();
    assert!(vfs.read("art/critters/critters.lst").is_ok());
    assert_eq!(vfs.read("data/patch.txt").unwrap(), b"loose");
    assert!(vfs.read("color.pal").is_ok());
    let crate::preview::AssetPreview::Map(mut archived) = args.load().unwrap().unwrap() else {
        panic!("map preview");
    };
    let mut unpacked = interactive(&loose);
    assert_eq!(
        archived.image().unwrap().pixels(),
        unpacked.image().unwrap().pixels(),
        "archives and loose files give the same world"
    );
    // Without both archives the loose directory alone is the source.
    let lone = crate::preview::PreviewArgs {
        critter_dat: None,
        ..args
    };
    assert!(lone.source().unwrap().read("color.pal").is_err());
}

#[test]
fn effects_speech_ui_and_library_sounds_reach_the_mixer() {
    use crate::audio::EFFECT_VOICES;
    use vault_core::dialogue::{HeadDefinition, Mood, TalkingHead};
    let dir = synthetic_corpus();
    std::fs::create_dir_all(dir.path().join("sound/sfx")).unwrap();
    std::fs::create_dir_all(dir.path().join("sound/speech/synth")).unwrap();
    for (path, vector) in [
        ("sound/sfx/IB1P1XX1.ACM", 'A'),
        ("sound/sfx/ib1lu1x1.acm", 'D'),
        ("sound/sfx/MAMTNTAN.ACM", 'B'),
        ("sound/speech/synth/hello.acm", 'H'),
    ] {
        std::fs::write(dir.path().join(path), sound_fixture::acm(vector)).unwrap();
    }
    let mut preview = interactive(dir.path());
    let (audio, mixer) = AudioHandle::detached();
    preview.audio = audio;
    preview.image().unwrap();
    let effect = |name: &[u8]| WorldEffect::SoundEffect {
        name: name.to_vec(),
        owner: None,
    };
    // Direct playback resolves through the MALIEU -> MAMTNT alias.
    preview.host.world.effects.push_back(effect(b"MALIEUAN"));
    preview.image().unwrap();
    assert!(mixer.lock().unwrap().playing(Channel::Effect));
    // At most four effects; later requests are refused, never evicting.
    for _ in 0..5 {
        preview.host.world.effects.push_back(effect(b"IB1P1XX1"));
    }
    preview.image().unwrap();
    assert_eq!(mixer.lock().unwrap().voices(), EFFECT_VOICES);
    mixer.lock().unwrap().stop(Channel::Effect);
    // A red button (the Skilldex's Cancel) plays the small-button
    // press, then release, cue.
    assert!(preview.step(KeyCode::KeyS));
    mixer.lock().unwrap().stop(Channel::Effect);
    assert!(preview.click([451 + 48, 5 + 338], false));
    assert!(preview.release(None, false));
    assert!(!preview.release(None, false), "only one release per press");
    assert_eq!(mixer.lock().unwrap().voices(), 2);
    mixer.lock().unwrap().stop(Channel::Effect);
    // Speech without a talking head (or file) finishes at once.
    preview
        .host
        .world
        .effects
        .push_back(WorldEffect::Speech(b"hello".to_vec()));
    preview.image().unwrap();
    assert!(!preview.speaking);
    // With a head, `sound/speech/<stem>/<field>.ACM` plays until done.
    let head = HeadDefinition {
        index: 0,
        stem: "synth".into(),
        counts: [1; 3],
    };
    preview.host.world.dialogue.head = Some(
        TalkingHead::new(
            head,
            Mood::Neutral,
            Default::default(),
            &mut preview.host.world.rng,
        )
        .unwrap(),
    );
    preview
        .host
        .world
        .effects
        .push_back(WorldEffect::Speech(b"hello.lip".to_vec()));
    preview.image().unwrap();
    assert!(preview.speaking && mixer.lock().unwrap().playing(Channel::Speech));
    mixer
        .lock()
        .unwrap()
        .fill(&mut [0.0; 16], 1, acm::MIXER_RATE);
    preview.tick(0).unwrap();
    assert!(!preview.speaking, "completed speech ends the speech phase");
    preview.host.world.dialogue.head = None;
    // A library slot loops until its slot stops.
    preview
        .host
        .world
        .effects
        .push_back(WorldEffect::LibrarySound {
            slot: 2,
            filename: b"sound\\sfx\\ib1lu1x1.acm".to_vec(),
            mode: 1,
        });
    preview.image().unwrap();
    assert!(mixer.lock().unwrap().playing(Channel::Library(2)));
    preview.tick(10).unwrap();
    assert!(
        !mixer.lock().unwrap().playing(Channel::Library(2)),
        "the slot is free"
    );
}

/// SPEC-010 §3.12: the speech voice's position drives the talking
/// head's mouth, at 69% of the speech volume (0 still lip-syncs).
#[test]
fn talking_heads_lip_sync_with_their_speech() {
    use vault_core::dialogue::{HeadDefinition, HeadIdle, Mood, TalkingHead};
    let dir = synthetic_corpus();
    let speech = dir.path().join("sound/speech/synth");
    std::fs::create_dir_all(&speech).unwrap();
    std::fs::write(speech.join("hello.acm"), sound_fixture::acm('H')).unwrap();
    // Vector H holds 4 samples (8 bytes). Phoneme 9 (frame 8) shows from
    // byte 2; the final marker lies past the
    // audio end.
    let mut lip = Vec::new();
    for word in [2, 0x5800, 0, 0, 0, 2, 0, 3] {
        lip.extend_from_slice(&i32::to_be_bytes(word));
    }
    lip.extend_from_slice(b"HELLO\0\0\0VOC\0");
    lip.extend_from_slice(&[0, 9]);
    for (kind, position) in [(1, 0), (1, 2), (1, i32::MAX)] {
        lip.extend_from_slice(&i32::to_be_bytes(kind));
        lip.extend_from_slice(&i32::to_be_bytes(position));
    }
    std::fs::write(speech.join("hello.lip"), lip).unwrap();
    let mut preview = interactive(dir.path());
    let (audio, mixer) = AudioHandle::detached();
    preview.audio = audio;
    preview.image().unwrap();
    for volume in [22281, 0] {
        preview.preferences.speech_volume = volume;
        preview.host.world.dialogue.head = Some(TalkingHead::without_fidget(
            HeadDefinition {
                index: 0,
                stem: "synth".into(),
                counts: [0; 3],
            },
            Mood::Neutral,
            HeadIdle::default(),
        ));
        preview.host.world.dialogue.updater = true;
        preview.host.request_speech(b"hello");
        preview.image().unwrap();
        assert!(preview.speaking);
        // SPEC-010 §3.13: trunc(0.69 x V), 15373 at the default.
        assert_eq!(
            mixer.lock().unwrap().level(Channel::Speech),
            Some(sfx::speech_level(volume))
        );
        let shown = |preview: &MapPreview| {
            let head = preview.host.world.dialogue.head.as_ref().unwrap();
            head.shown.map(|shown| (shown.animation, shown.frame))
        };
        mixer
            .lock()
            .unwrap()
            .fill(&mut [0.0; 2], 1, acm::MIXER_RATE);
        preview.tick(100).unwrap();
        assert_eq!(shown(&preview), Some((10, 8)), "volume {volume}");
        // The voice runs out: the next tick ends the speech normally.
        mixer
            .lock()
            .unwrap()
            .fill(&mut vec![0.0; 1 << 20], 1, acm::MIXER_RATE);
        preview.tick(200).unwrap();
        assert_eq!(shown(&preview), Some((10, 0)));
        assert!(preview.host.world.dialogue.speech.is_none());
        let head = preview.host.world.dialogue.head.as_ref().unwrap();
        assert_eq!(head.idle_seconds, 3);
    }
    // Without the audio file the speech ends on the next tick.
    preview.host.request_speech(b"missing");
    preview.image().unwrap();
    assert!(!preview.speaking);
    preview.tick(300).unwrap();
    assert!(preview.host.world.dialogue.speech.is_none());
}

#[test]
fn game_movies_play_modally_with_subtitles_and_return_to_their_screen() {
    let dir = synthetic_corpus();
    let root = dir.path();
    crate::movie::tests::movie_corpus(root);
    let (synthetic, _, _) = crate::movie::fixture::synthetic('A');
    std::fs::write(root.join("art/cuts/boil1.mve"), synthetic).unwrap();
    std::fs::create_dir_all(root.join("text/english/cuts")).unwrap();
    std::fs::write(root.join("text/english/cuts/boil1.sve"), b"1:AB\r\n").unwrap();
    std::fs::write(root.join("font1.aaf"), state_fixture::preview_aaf()).unwrap();
    std::fs::write(
        root.join("art/cuts/subtitle.pal"),
        state_fixture::preview_palette(),
    )
    .unwrap();
    let mut preview = interactive(root);
    let (audio, mixer) = AudioHandle::detached();
    preview.audio = audio;
    preview.image().unwrap();
    // A boil movie: subtitles are forced; the world waits while it plays.
    assert!(preview.host.play_game_movie(vault_core::movie::BOIL1, 0x07));
    preview.image().unwrap();
    assert!(preview.movie_active() && preview.display_override().is_some());
    // The movie's path is a debug-mode title hint.
    assert_eq!(preview.title(), "Fallout — Vault Engine");
    preview.set_debug(true);
    assert!(preview.title().contains(r"art\cuts\boil1.mve"));
    preview.set_debug(false);
    let ticks = preview.host.world.clock.ticks;
    let mut now = 0;
    while !preview.tick(now).unwrap() {
        now += 16;
    }
    let image = preview.image().unwrap();
    assert_eq!((image.width(), image.height()), (640, 480));
    // The subtitle band sits below the centered 64 x 64 frame.
    let font = preview.host.assets.font_number(101).unwrap();
    let band = vault_core::movie::subtitle_band(font.height() as i32, 64, 208);
    let band_pixels = &image.pixels()[band[1] as usize * 640..(band[1] + band[3]) as usize * 640];
    assert!(
        band_pixels.iter().any(|pixel| *pixel != 0),
        "subtitle drawn"
    );
    assert!(mixer.lock().unwrap().playing(Channel::Movie));
    for _ in 0..5 {
        now += 16;
        preview.tick(now).unwrap();
    }
    assert_eq!(
        preview.host.world.clock.ticks, ticks,
        "modal: the clock waits"
    );
    // A key skips the movie and returns to the map: playback ends on
    // black, and flag 0x02 fades back to the game palette (SPEC-012
    // §3.12, SPEC-014 §3.2).
    assert!(preview.step(KeyCode::Space));
    assert!(!preview.movie_active());
    assert_eq!(preview.display_override(), Some([[0, 0, 0, 255]; 256]));
    for _ in 0..50 {
        now += 16;
        preview.tick(now).unwrap();
    }
    assert!(preview.display_override().is_none());
    assert!(!mixer.lock().unwrap().playing(Channel::Movie));
    assert_eq!(preview.image().unwrap().height(), 480);
    // The startup sequence: iplogo is absent, so only intro plays.
    preview.queue_startup_movies();
    assert!(preview.movie_active());
    preview.set_debug(true);
    assert!(preview.title().contains("intro.mve"));
    preview.set_debug(false);
    while preview.movie_active() {
        now += 16;
        preview.tick(now).unwrap();
        assert!(now < 100_000, "the movie ends by itself");
    }
    assert_eq!(preview.host.world.persistence.movies[..3], [0, 0, 1]);
    // The map fades in from black (a game fade), which holds input
    // until it ends (SPEC-014 §3.2).
    assert!(preview.fade_blocks());
    assert!(preview.step(KeyCode::KeyP), "swallowed");
    assert!(matches!(preview.screen, Screen::Map));
    preview.finish_fades();
    // The Pip-Boy archive lists boil1 (index 11) and replays it from
    // the row's release (SPEC-014 §3.41.5); a click then release skips
    // it, back to the Pip-Boy.
    assert!(preview.step(KeyCode::KeyP));
    assert!(preview.tap([60, 430]), "the Archives button");
    preview.image().unwrap();
    assert!(preview.click([300, 70], false));
    assert!(!preview.movie_active(), "rows act on their release");
    assert!(preview.release(Some([300, 70]), false));
    assert!(preview.movie_active());
    assert!(preview.click([10, 10], false));
    assert!(preview.release(None, false));
    assert!(matches!(preview.screen, Screen::PipBoy(_)));
}

#[test]
fn world_map_music_and_the_music_switch_reach_the_mixer() {
    let dir = synthetic_corpus();
    let root = dir.path();
    std::fs::create_dir_all(root.join("sound/music")).unwrap();
    for track in ["03wrldmp", "wind2", "13carvrn"] {
        std::fs::write(
            root.join(format!("sound/music/{track}.acm")),
            sound_fixture::acm('B'),
        )
        .unwrap();
    }
    let mut preview = interactive(root);
    let (audio, mixer) = AudioHandle::detached();
    preview.audio = audio.clone();
    preview.tick(0).unwrap();
    // SPEC-008 §3.10: the load's `wind2` and the city track (map 7,
    // `13carvrn`) reached the world; the last one is the city's.
    assert_eq!(
        preview.host.last_music.as_deref(),
        Some("13carvrn"),
        "the world's record"
    );
    // The preview's W opens the world map outside a transition: its
    // music is requested here.
    assert!(preview.step(KeyCode::KeyW));
    preview.tick(16).unwrap();
    assert_eq!(
        audio.last_music(),
        Some(MusicRequest::track("03wrldmp", true))
    );
    audio.wait_music();
    assert!(mixer.lock().unwrap().playing(Channel::Music));
    // SPEC-008 §3.10: a music volume of 0 turns music off by volume in
    // the mixer and the world; a request is only recorded, and setting
    // the volume again plays the last recorded track once.
    preview.preferences.music_volume = 0;
    preview.tick(24).unwrap();
    assert!(!audio.music_enabled() && !preview.host.music_enabled);
    assert!(!mixer.lock().unwrap().playing(Channel::Music));
    preview.host.request_music("wind2");
    preview.tick(32).unwrap();
    assert!(!mixer.lock().unwrap().playing(Channel::Music));
    assert_eq!(
        audio.last_music(),
        Some(MusicRequest::track("03wrldmp", true))
    );
    preview.preferences.music_volume = 9000;
    preview.tick(40).unwrap();
    assert!(audio.music_enabled() && preview.host.music_enabled);
    preview.tick(48).unwrap();
    assert_eq!(audio.last_music(), Some(MusicRequest::track("wind2", true)));
    audio.wait_music();
    assert!(mixer.lock().unwrap().playing(Channel::Music));
    // S-11: a negative town has no town map; the failure path opens
    // the world map, with its music.
    preview.return_to_map();
    preview.host.world.world_map.saved.town = -1;
    preview.open_town_map();
    assert!(matches!(preview.screen, Screen::World(_)));
    preview.tick(64).unwrap();
    assert_eq!(
        audio.last_music(),
        Some(MusicRequest::track("03wrldmp", true))
    );
}

#[test]
fn spec008_dialogue_stops_or_halves_the_music_and_undoes_it_on_close() {
    use vault_core::dialogue::{HeadDefinition, HeadIdle, Mood, TalkingHead};
    let dir = synthetic_corpus();
    let root = dir.path();
    std::fs::create_dir_all(root.join("sound/music")).unwrap();
    std::fs::write(root.join("sound/music/01hub.acm"), sound_fixture::acm('B')).unwrap();
    let mut preview = interactive(root);
    let (audio, mixer) = AudioHandle::detached();
    preview.audio = audio.clone();
    preview.preferences.music_volume = 9001;
    preview.tick(0).unwrap();
    preview.request_music(MusicRequest::track("01hub", true));
    audio.wait_music();
    let playing = || mixer.lock().unwrap().playing(Channel::Music);
    assert!(playing());
    let head = || {
        TalkingHead::without_fidget(
            HeadDefinition {
                index: 3,
                stem: String::new(),
                counts: [0; 3],
            },
            Mood::Neutral,
            HeadIdle::default(),
        )
    };
    // SPEC-008 §3.10 rule 4: a talking-head session stops the track
    // when it opens; a session already open changes nothing.
    preview.host.world.dialogue.open = true;
    preview.host.world.dialogue.head = Some(head());
    preview.dialogue_music_tick();
    assert!(!playing());
    assert!(audio.music_enabled(), "the on/off state is unchanged");
    preview.dialogue_music_tick();
    assert!(!playing());
    // Closing replays the last recorded track from its beginning.
    preview.host.world.dialogue.open = false;
    preview.host.world.dialogue.head = None;
    preview.dialogue_music_tick();
    audio.wait_music();
    assert!(playing());
    assert_eq!(audio.last_music(), Some(MusicRequest::track("01hub", true)));
    // No head: the music volume current at the opening is halved
    // (truncated) and restored at the close (the safe rule), however
    // long the session stays open.
    let level = vault_formats::sfx::music_level;
    preview.host.world.dialogue.open = true;
    preview.dialogue_music_tick();
    assert_eq!(preview.volumes.music, 4500);
    assert_eq!(
        mixer.lock().unwrap().level(Channel::Music),
        Some(level(4500))
    );
    preview.tick(16).unwrap();
    preview.dialogue_music_tick();
    assert_eq!(preview.volumes.music, 4500, "halved once");
    assert!(playing());
    preview.host.world.dialogue.open = false;
    preview.dialogue_music_tick();
    assert_eq!(preview.volumes.music, 9001);
    assert_eq!(
        mixer.lock().unwrap().level(Channel::Music),
        Some(level(9001))
    );
    // A second no-head session halves the current volume again, not
    // one stored by the session before (the reference's defect).
    preview.preferences.music_volume = 7001;
    preview.tick(32).unwrap();
    preview.host.world.dialogue.open = true;
    preview.dialogue_music_tick();
    assert_eq!(preview.volumes.music, 3500);
    preview.host.world.dialogue.open = false;
    preview.dialogue_music_tick();
    assert_eq!(preview.volumes.music, 7001);
    // A volume of 1 halves to 0, which turns music off by volume; the
    // close turns it back on and the world replays its track.
    preview.preferences.music_volume = 1;
    preview.tick(48).unwrap();
    assert!(audio.music_enabled());
    preview.host.world.dialogue.open = true;
    preview.dialogue_music_tick();
    assert_eq!(preview.volumes.music, 0);
    assert!(!audio.music_enabled() && !preview.host.music_enabled);
    assert!(!playing());
    preview.host.world.dialogue.open = false;
    preview.dialogue_music_tick();
    assert_eq!(preview.volumes.music, 1);
    assert!(audio.music_enabled() && preview.host.music_enabled);
}

#[test]
fn world_audio_takes_volumes_music_requests_and_movie_music_flags() {
    let dir = synthetic_corpus();
    let root = dir.path();
    crate::movie::tests::movie_corpus(root);
    std::fs::create_dir_all(root.join("sound/sfx")).unwrap();
    std::fs::create_dir_all(root.join("sound/music")).unwrap();
    std::fs::write(root.join("sound/sfx/IB1P1XX1.ACM"), sound_fixture::acm('A')).unwrap();
    std::fs::write(root.join("sound/music/01hub.acm"), sound_fixture::acm('B')).unwrap();
    let mut preview = interactive(root);
    let (audio, mixer) = AudioHandle::detached();
    preview.audio = audio.clone();
    preview.tick(0).unwrap();
    let default = 22281;
    assert_eq!(mixer.lock().unwrap().master(), default);
    // SPEC-010 §3.13: an effect without an object plays at the effects
    // volume (V = 32767, so floor(V x S / 32767) = S).
    preview
        .host
        .world
        .effects
        .push_back(WorldEffect::SoundEffect {
            name: b"IB1P1XX1".to_vec(),
            owner: None,
        });
    preview.image().unwrap();
    assert_eq!(mixer.lock().unwrap().level(Channel::Effect), Some(default));
    // At master 22281 that is -1112 hundredths of a decibel (§4.8).
    let gain = mixer.lock().unwrap().gain(Channel::Effect).unwrap();
    assert!((gain - 10_f32.powf(-1112.0 / 2000.0)).abs() < 1e-6);
    // F2 lowers the mixer master by 2047, not the preference; the next
    // preference application resets it (SPEC-014 §3.18). It re-applies
    // to the playing effect at once (SPEC-010 §3.13).
    assert!(preview.step(KeyCode::F2));
    assert_eq!(mixer.lock().unwrap().master(), 22281 - 2047);
    assert!(mixer.lock().unwrap().gain(Channel::Effect).unwrap() < gain);
    assert_eq!(preview.preferences.master_volume, 22281);
    assert!(preview.step(KeyCode::F3));
    assert!(preview.step(KeyCode::F3));
    assert_eq!(mixer.lock().unwrap().master(), 22281 + 2047);
    // Cancelling the preferences reapplies them, the unchanged volumes
    // included, which drops the F2/F3 offset.
    assert!(preview.step(KeyCode::Escape));
    assert!(preview.step(KeyCode::KeyP));
    assert!(matches!(
        preview.screen,
        Screen::Options(options::OptionsScreen::Preferences(_))
    ));
    assert!(preview.step(KeyCode::Escape));
    assert_eq!(mixer.lock().unwrap().master(), default);
    assert!(preview.step(KeyCode::Escape));
    assert!(matches!(preview.screen, Screen::Map));
    assert!(preview.step(KeyCode::F3));
    assert_eq!(mixer.lock().unwrap().master(), 22281 + 2047);
    preview.preferences.master_volume = 30000;
    preview.tick(16).unwrap();
    assert_eq!(mixer.lock().unwrap().master(), 30000);
    // A city-music request plays its track, looping, as stereo frames
    // (SPEC-008 §3.10, SPEC-010 §3.6), at the music volume.
    preview
        .host
        .world
        .effects
        .push_back(WorldEffect::Music("01hub".into()));
    preview.image().unwrap();
    assert_eq!(audio.last_music(), Some(MusicRequest::track("01hub", true)));
    audio.wait_music();
    assert!(mixer.lock().unwrap().playing(Channel::Music));
    assert_eq!(mixer.lock().unwrap().level(Channel::Music), Some(20944));
    // SPEC-012 §3.12: flag 0x08 pauses the music for the movie and its
    // cleanup resumes it; 0x04 stops it.
    assert!(preview.host.play_game_movie(vault_core::movie::INTRO, 0x08));
    preview.image().unwrap();
    preview.tick(32).unwrap();
    assert!(preview.movie_active() && mixer.lock().unwrap().paused(Channel::Music));
    // A middle button (or a press in the letterbox) skips it too.
    assert!(preview.movie_button(true));
    assert!(preview.movie_button(false));
    assert!(!preview.movie_active());
    assert!(!mixer.lock().unwrap().paused(Channel::Music));
    assert!(mixer.lock().unwrap().playing(Channel::Music));
    assert!(preview.host.play_game_movie(vault_core::movie::INTRO, 0x04));
    preview.image().unwrap();
    preview.tick(48).unwrap();
    assert!(!mixer.lock().unwrap().playing(Channel::Music));
    assert!(preview.step(KeyCode::Space));
    // SPEC-010 §3.11: the player's sprite overlaps the map window, on
    // any elevation (no elevation condition).
    let player = preview.host.world.player;
    assert!(preview.on_screen(player));
    preview.host.world.get_mut(player).unwrap().raw.elevation = 2;
    assert!(preview.on_screen(player));
    // Far off at hex 0 it is off screen, unless its art cannot be
    // read: then its rectangle is (0, 0)-(0, 0), on screen.
    preview.host.world.get_mut(player).unwrap().raw.hex = Some(0);
    assert!(!preview.on_screen(player));
    preview.host.world.get_mut(player).unwrap().raw.art_id = 0x0fff_ffff;
    assert!(preview.on_screen(player));
}

#[test]
fn a_modal_window_reuses_the_frozen_world_view() {
    let dir = synthetic_corpus();
    let mut preview = interactive(dir.path());
    preview.image().unwrap();
    assert!(preview.frozen_view.is_none(), "the map screen renders live");
    assert!(preview.step(KeyCode::KeyS));
    assert!(matches!(preview.screen, Screen::Skilldex(_)));
    let first = preview.image().unwrap();
    assert!(preview.frozen_view.is_some());
    assert_eq!(preview.image().unwrap().pixels(), first.pixels());
    // Input may change the world: the view is rendered again.
    preview.release(None, false);
    assert!(preview.frozen_view.is_none());
}

#[test]
fn inventory_character_and_barter_screens_drive_the_spec011_rules() {
    let dir = synthetic_corpus();
    std::fs::write(
        dir.path().join("text/english/game/editor.msg"),
        b"{136}{}{No free skill points available.}\n",
    )
    .unwrap();
    let mut preview = interactive(dir.path());
    raise_item(&mut preview);
    preview.image().unwrap();
    // Pick up the synthetic item with an arrow-mode click.
    preview.hud.mode = vault_core::ui::cursor::Mode::Arrow;
    preview.tap([320, 185]);
    let item = preview.selected.unwrap();
    let player = preview.host.world.player;
    assert_eq!(preview.host.world.get(item).unwrap().owner, Some(player));
    // Inventory (SPEC-014 §3.34): the item lifts from the first list
    // row (124, 35) and drops into the right hand (325, 286); dropped
    // back on the list it is unequipped; the right hand takes it again.
    assert!(preview.step(KeyCode::KeyI));
    assert!(matches!(preview.screen, Screen::Inventory(_)));
    assert_eq!(preview.image().unwrap().height(), 480);
    assert!(preview.click([134, 45], false), "first row");
    assert!(preview.release(Some([340, 300]), false), "right hand");
    let flags = |preview: &MapPreview| preview.host.world.get(item).unwrap().raw.flags;
    assert!(flags(&preview).contains(BasicFlags::RIGHT_HAND));
    preview.image().unwrap();
    assert!(preview.click([340, 300], false));
    assert!(preview.release(Some([150, 100]), false));
    assert!(!flags(&preview).contains(BasicFlags::RIGHT_HAND));
    assert!(preview.click([134, 45], false));
    assert!(preview.release(Some([340, 300]), false));
    assert!(flags(&preview).contains(BasicFlags::RIGHT_HAND));
    assert!(preview.step(KeyCode::Escape));
    assert!(matches!(preview.screen, Screen::Map));
    // Character (SPEC-014 §3.42): a press on a skill row moves the
    // cursor; the slider's plus with no points is refusal A; Cancel
    // restores the sheet. Small Guns is the row at y = 27 in the hot
    // region (370, 27) 223 by 195; the plus button is at (614, 20).
    assert!(preview.step(KeyCode::KeyC));
    preview.image().unwrap();
    assert!(preview.click([400, 30], false), "Small Guns");
    assert!(preview.click([616, 22], false), "plus");
    let refused = |preview: &MapPreview| {
        matches!(
            &preview.screen,
            Screen::Character(screen)
                if matches!(&screen.modal, Some(character::Modal::Refusal(notice))
                    if notice.lines == [b"No free skill points available.".to_vec()])
        )
    };
    assert!(refused(&preview));
    preview.image().unwrap();
    assert!(preview.step(KeyCode::Escape), "the refusal closes");
    assert!(matches!(
        &preview.screen,
        Screen::Character(screen)
            if screen.selected == vault_core::ui::editor::InfoLine::Skill(0)
                && screen.modal.is_none()
    ));
    assert!(preview.click([200, 180], false), "Armor Class");
    assert!(matches!(
        &preview.screen,
        Screen::Character(screen)
            if screen.selected == vault_core::ui::editor::InfoLine::Derived(0)
    ));
    assert!(preview.step(KeyCode::Escape));
    preview
        .host
        .world
        .player_sheet_mut()
        .unwrap()
        .unspent_skills = 2;
    let invested = preview.host.world.player_sheet().unwrap().invested[0];
    assert!(preview.step(KeyCode::KeyC));
    // The skill cursor is remembered: `+` steps Small Guns.
    assert!(preview.step(KeyCode::NumpadAdd));
    let sheet = preview.host.world.player_sheet().unwrap();
    assert_eq!((sheet.invested[0], sheet.unspent_skills), (invested + 1, 1));
    assert!(preview.click([558, 460], false), "Cancel");
    assert!(preview.release(Some([558, 460]), false));
    let sheet = preview.host.world.player_sheet().unwrap();
    assert_eq!((sheet.invested[0], sheet.unspent_skills), (invested, 2));
    // Reaching level 3 sets the pending perk on the next visit; the
    // chooser grants a rank and Done keeps it.
    let sheet = preview.host.world.player_sheet_mut().unwrap();
    sheet.level = 3;
    assert!(preview.step(KeyCode::KeyC));
    let Screen::Character(screen) = &preview.screen else {
        panic!("character screen");
    };
    let Some(character::Modal::Chooser(chooser)) = &screen.modal else {
        panic!("perk chooser at level 3");
    };
    assert!(!chooser.entries.is_empty());
    let perk = vault_core::Perk::from_index(chooser.entries[0].0).unwrap();
    preview.image().unwrap();
    assert!(preview.step(KeyCode::Enter), "Return chooses");
    let sheet = preview.host.world.player_sheet().unwrap();
    assert!(sheet.rank(perk) == 1 && !sheet.pending_perk);
    preview.image().unwrap();
    if let Screen::Character(screen) = &preview.screen
        && screen.modal.is_none()
    {
        assert!(preview.step(KeyCode::Enter), "Done");
    } else {
        assert!(preview.step(KeyCode::Escape));
    }
    assert!(matches!(preview.screen, Screen::Map));
    assert_eq!(
        preview.host.world.player_sheet().unwrap().reconciled_level,
        3
    );
    // Barter: a dialogue's pending request opens it with the target
    // (an arrow click talks to the NPC).
    preview.hud.mode = vault_core::ui::cursor::Mode::Arrow;
    assert!(preview.tap([336, 170]));
    assert!(preview.host.world.dialogue.open);
    preview.host.world.dialogue.pending_barter = true;
    preview.image().unwrap();
    let Screen::Barter(screen) = &preview.screen else {
        panic!("barter");
    };
    let offer = screen.session.offer;
    // SPEC-013 §3.18: the player's first inventory slot (subwindow
    // (29, 35), screen (109, 325)) drags onto the player table's drop
    // rectangle (screen x 245-309, y 310-454); a release elsewhere
    // moves nothing.
    assert!(preview.click([120, 340], false));
    assert!(preview.release(Some([200, 340]), false));
    assert!(preview.host.world.get(offer).unwrap().inventory.is_empty());
    assert!(preview.click([120, 340], false));
    preview.pointer_moved([260, 340]);
    assert!(preview.pointer_frame().is_some(), "the item is the cursor");
    assert!(preview.release(Some([260, 340]), false));
    assert!(!preview.host.world.get(offer).unwrap().inventory.is_empty());
    preview.image().unwrap();
    // Talk (barter window (584, 162)) returns the tables.
    assert!(preview.tap([590, 289 + 168]), "Talk");
    assert!(!preview.host.world.dialogue.pending_barter);
    assert_eq!(preview.host.world.get(item).unwrap().owner, Some(player));
    assert!(matches!(preview.screen, Screen::Map) && preview.host.world.dialogue.open);
    // Again: an offer for an empty merchant table costs nothing, so Offer
    // (`m`) trades; the result is the conversation's reply meanwhile.
    preview.host.world.dialogue.pending_barter = true;
    preview.image().unwrap();
    let Screen::Barter(screen) = &preview.screen else {
        panic!("barter");
    };
    let merchant = screen.session.merchant;
    assert!(preview.click([120, 340], false));
    assert!(preview.release(Some([260, 340]), false));
    assert!(preview.step(KeyCode::KeyM));
    assert!(matches!(&preview.screen, Screen::Barter(screen) if screen.reply.is_some()));
    assert_eq!(preview.host.world.get(item).unwrap().owner, Some(merchant));
    preview.image().unwrap();
    assert!(preview.step(KeyCode::Escape));
    assert!(matches!(preview.screen, Screen::Map));
}

#[test]
fn map_preview_loads_all_categories_and_handles_camera_roofs_elevations() {
    let dir = tempfile::tempdir().unwrap();
    fixture::write_corpus(dir.path());
    let vfs = Arc::new(LocalDirProvider::mount(dir.path()).unwrap());
    let mut preview = MapPreview::load(vfs, r"MAPS\SYNTHETIC.MAP", "english", None, 1).unwrap();
    assert_eq!(preview.host.world.map.objects.total, 5);
    let image = preview.image().unwrap();
    for color in [7, 15, 16, 17, 18] {
        assert!(
            image.pixels().contains(&color),
            "missing category color {color}"
        );
    }
    assert_eq!(preview.hover([320, 190]).unwrap().hex, Some(20100));
    assert!(preview.hover([320, 190]).is_none());
    preview.step(KeyCode::KeyR);
    assert!(!preview.view.roofs);
    preview.step(KeyCode::ArrowRight);
    assert_eq!(preview.hover([288, 190]).unwrap().hex, Some(20100));
    // PageUp and PageDown change the viewed elevation within 0-2.
    for (key, elevation) in [
        (KeyCode::PageUp, 1),
        (KeyCode::PageUp, 2),
        (KeyCode::PageUp, 2),
        (KeyCode::PageDown, 1),
    ] {
        assert!(preview.step(key));
        assert_eq!(preview.view.elevation, elevation);
        preview.image().unwrap();
    }
    assert!(!preview.step(KeyCode::Space));
}
#[test]
fn roof_art_binds_whatever_the_hide_bit_except_the_blank_index() {
    // S-19 / SPEC-014 §3.36: a roof group hidden when the map's art is
    // bound must still draw once it is restored.
    let dir = tempfile::tempdir().unwrap();
    fixture::write_corpus(dir.path());
    let vfs = Arc::new(LocalDirProvider::mount(dir.path()).unwrap());
    let preview = MapPreview::load(vfs, r"MAPS\SYNTHETIC.MAP", "english", None, 1).unwrap();
    let mut map = preview.host.world.map.clone();
    // Roof index 0 (`tile.frm`, used by no floor), hidden, over the
    // blank floor; every other roof is the blank index 1.
    let square = &mut map.tiles[0].as_mut().unwrap()[5050];
    square.word = 0x1000_0001;
    assert!(square.roof_hidden() && square.roof() == 0);
    let mut art = MapSprites::default();
    art.bind(
        &preview.host.assets,
        &map,
        &preview.host.world.render_objects(),
        None,
    )
    .unwrap();
    assert!(art.tile_art.first().is_some_and(Option::is_some));
}
#[test]
fn the_pause_window_and_modal_windows_suspend_palette_cycling() {
    let dir = synthetic_corpus();
    let mut preview = interactive(dir.path());
    preview.image().unwrap();
    assert!(preview.palette_cycling());
    // SPEC-014 §3.17: in game, the pause window suspends cycling until
    // a key closes it.
    preview.open_pause();
    assert!(!preview.palette_cycling());
    assert!(preview.step(KeyCode::KeyA));
    assert!(matches!(preview.screen, Screen::Map));
    assert!(preview.palette_cycling());
    // §3.8: so does the character editor, here the character screen.
    assert!(preview.step(KeyCode::KeyC));
    assert!(!preview.palette_cycling());
    assert!(preview.step(KeyCode::Escape));
    assert!(preview.palette_cycling());
    // SPEC-014 §3.39: so does the world map, paused or not.
    assert!(preview.step(KeyCode::KeyW));
    assert!(!preview.palette_cycling());
    assert!(preview.step(KeyCode::KeyP));
    assert!(matches!(&preview.screen, Screen::World(world) if world.paused));
    assert!(!preview.palette_cycling());
}
#[test]
fn clicking_npc_renders_choices_executes_callbacks_and_returns_to_map() {
    let dir = synthetic_corpus();
    let mut preview = interactive(dir.path());
    preview.image().unwrap();
    // SPEC-014 §3.27: a right-click cycles movement to arrow; an arrow
    // click on a talkable critter talks to it.
    assert!(preview.host.library.palette_cycling);
    assert!(preview.click([336, 170], true));
    assert_eq!(preview.hud.mode, vault_core::ui::cursor::Mode::Arrow);
    assert!(preview.tap([336, 170]));
    let selected = preview.selected.unwrap();
    assert_eq!(
        preview.host.world.get(selected).unwrap().category(),
        Category::Critter
    );
    assert!(preview.host.world.dialogue.open && preview.host.world.dialogue.waiting);
    // The shell's cycling poll follows this flag (SPEC-001/SPEC-006).
    assert!(!preview.host.library.palette_cycling);
    let image = preview.image().unwrap();
    assert_eq!((image.width(), image.height()), (640, 480));
    assert_eq!(preview.choices.len(), 1);
    assert_eq!(preview.choices[0].rectangle[0..2], [127, 335]);
    assert!(preview.click([200, 338], false));
    assert_eq!(preview.host.world.dialogue.options[0].procedure, 0);
    preview.image().unwrap();
    assert!(preview.step(KeyCode::Digit1));
    assert!(!preview.host.world.dialogue.open);
    assert!(preview.host.library.palette_cycling);
    // The map frame now carries the interface bar below the view.
    assert_eq!(preview.image().unwrap().height(), 480);
    // An arrow click on an item picks it up into authoritative inventory.
    raise_item(&mut preview);
    preview.image().unwrap();
    preview.tap([320, 185]);
    let item = preview.selected.unwrap();
    assert_eq!(
        preview.host.world.get(item).unwrap().category(),
        Category::Item
    );
    assert_eq!(
        preview.host.world.get(item).unwrap().owner,
        Some(preview.host.world.player)
    );
}
