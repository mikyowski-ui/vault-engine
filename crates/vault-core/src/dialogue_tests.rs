//! SPEC-010 §3.12 lip-sync (vectors LIP-A to LIP-D), the SPEC-006 §3.11
//! session-start draw order (§4.4), speech requests through the host, and
//! the SPEC-002 §3.7 language filter on script messages.
use crate::{
    CharacterSheet, RandomSource, WorldEffect, WorldError, WorldHost, WorldState,
    dialogue::{
        HeadDefinition, HeadFrame, HeadIdle, HeadPhase, LipStep, LipSync, Mood, TalkingHead,
    },
    test_support::TestAssets,
};
use vault_formats::{LipFile, MsgCatalog, msg::BadWords};

/// SPEC-010 §4.7 LIP-A.
const LIP_A: [u8; 97] = [
    0x00, 0x00, 0x00, 0x02, 0x00, 0x00, 0x58, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
    0x00, 0x00, 0x03, 0xE9, 0x00, 0x00, 0x00, 0x05, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x06,
    0x53, 0x59, 0x4E, 0x54, 0x48, 0x30, 0x31, 0x00, 0x56, 0x4F, 0x43, 0x00, 0x00, 0x09, 0x13, 0x17,
    0x00, 0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00,
    0x64, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x01, 0x2C, 0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x01,
    0x2D, 0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x02, 0x58, 0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x03,
    0xE8,
];

/// A version 2 `.LIP` file.
fn lip_bytes(stem: &[u8], flags: i32, phonemes: &[u8], markers: &[(i32, i32)]) -> Vec<u8> {
    let mut bytes = Vec::new();
    for word in [2, 0x5800, flags, 0, 0, phonemes.len() as i32, 0] {
        bytes.extend_from_slice(&word.to_be_bytes());
    }
    bytes.extend_from_slice(&(markers.len() as i32).to_be_bytes());
    let mut name = [0; 8];
    name[..stem.len()].copy_from_slice(stem);
    bytes.extend_from_slice(&name);
    bytes.extend_from_slice(b"VOC\0");
    bytes.extend_from_slice(phonemes);
    for (kind, position) in markers {
        bytes.extend_from_slice(&kind.to_be_bytes());
        bytes.extend_from_slice(&position.to_be_bytes());
    }
    bytes
}

fn lip_a() -> LipFile {
    LipFile::parse(&LIP_A).unwrap()
}

/// Step a speech at `position`, the audio still playing.
fn at(lip: &mut LipSync, position: u64) -> LipStep {
    lip.set_clock(position, true);
    lip.tick()
}

fn draws(phoneme: u8) -> LipStep {
    LipStep {
        draw: Some(phoneme),
        ..LipStep::default()
    }
}

#[test]
fn spec010_lip_a_ticks() {
    // The last drawn phoneme before the speech is 0, with no redraw marked.
    let mut lip = LipSync::new(lip_a(), "a".into(), Some(0));
    assert_eq!(lip.tick(), LipStep::default(), "nothing before the start");
    lip.start(true);
    let none = LipStep::default();
    for (position, marker, phoneme, step) in [
        (0, 0, 0, none),
        (50, 1, 0, none),
        (100, 1, 0, none),
        (101, 2, 9, draws(9)),
        (300, 2, 9, none),
        (301, 3, 19, draws(19)),
        (302, 4, 23, draws(23)),
        (601, 5, 0, draws(0)),
        (999, 5, 0, none),
    ] {
        assert_eq!(at(&mut lip, position), step, "p = {position}");
        assert_eq!(
            (lip.marker, lip.phoneme),
            (marker, phoneme),
            "p = {position}"
        );
    }
    // The SPEC-006 table: phonemes 9, 19, 23, 0 are frames 8, 2, 4, 0.
    assert_eq!(
        [9, 19, 23, 0].map(|p| crate::dialogue::PHONEMES[p]),
        [8, 2, 4, 0]
    );
    // The audio finishes: the speech ends normally.
    lip.set_clock(0, false);
    assert_eq!(
        lip.tick(),
        LipStep {
            ended: true,
            ..LipStep::default()
        }
    );
    // A last drawn phoneme 23 left by earlier speech draws frame 0 at once;
    // under the safe rule every speech draws on its first tick.
    for carried in [Some(23), None] {
        let mut lip = LipSync::new(lip_a(), "a".into(), carried);
        lip.start(true);
        assert_eq!(at(&mut lip, 0), draws(0), "{carried:?}");
        assert_eq!(at(&mut lip, 50), LipStep::default());
    }
}

#[test]
fn spec010_lip_b_c_d_and_repeat() {
    // LIP-B: the final marker at 900 is passed at 901: the audio stops and
    // the speech ends with phoneme ph(0) = 0.
    let mut bytes = LIP_A;
    bytes[0x5D..0x61].copy_from_slice(&[0x00, 0x00, 0x03, 0x84]);
    let mut lip = LipSync::new(LipFile::parse(&bytes).unwrap(), "b".into(), Some(0));
    lip.start(true);
    for (position, phoneme) in [(101, 9), (301, 19), (302, 23), (601, 0)] {
        assert_eq!(at(&mut lip, position), draws(phoneme));
    }
    assert_eq!(at(&mut lip, 900), LipStep::default());
    assert_eq!(
        at(&mut lip, 901),
        LipStep {
            draw: None,
            stop_audio: true,
            ended: true
        }
    );
    assert_eq!((lip.marker, lip.phoneme, lip.running), (0, 0, false));
    // LIP-C: one tick at 350 passes markers 0-3 and draws only phoneme 23.
    let mut lip = LipSync::new(lip_a(), "c".into(), Some(0));
    lip.start(true);
    assert_eq!(at(&mut lip, 350), draws(23));
    assert_eq!(lip.marker, 4);
    // LIP-D: positions are PCM bytes, 44100 per second (sample 22050).
    let file = LipFile::parse(&lip_bytes(
        b"d",
        0,
        &[0, 9],
        &[(1, 0), (1, 44100), (1, 88200)],
    ))
    .unwrap();
    let mut lip = LipSync::new(file, "d".into(), Some(0));
    lip.start(true);
    assert_eq!(at(&mut lip, 2 * 22050), LipStep::default());
    assert_eq!(at(&mut lip, 2 * 22050 + 1), draws(9));
    // A position that jumps back to the start in the last stretch ends the
    // speech (k = 3 when M > 5).
    let mut lip = LipSync::new(lip_a(), "e".into(), Some(0));
    lip.start(true);
    at(&mut lip, 601);
    assert!(at(&mut lip, 300).stop_audio);
    // The repeat flag keeps the audio past the final marker: each tick
    // passes every marker again and holds ph(0).
    let mut repeating = LIP_A;
    repeating[0x0B] = 1;
    let mut lip = LipSync::new(LipFile::parse(&repeating).unwrap(), "r".into(), Some(0));
    lip.start(true);
    assert_eq!(at(&mut lip, 302), draws(23));
    assert_eq!(at(&mut lip, 1001), draws(0));
    assert!(lip.running);
    assert_eq!(at(&mut lip, 1002), LipStep::default());
    // Audio that could not start: lip-sync stops, the next tick ends it.
    let mut lip = LipSync::new(lip_a(), "f".into(), None);
    lip.start(false);
    assert_eq!(
        lip.tick(),
        LipStep {
            draw: Some(0),
            stop_audio: false,
            ended: true
        }
    );
    // The missing-file rule: phoneme 0 until the audio ends.
    let mut lip = LipSync::new(LipFile::silent(), "g".into(), None);
    lip.start(true);
    assert_eq!(at(&mut lip, 0), draws(0));
    assert_eq!(at(&mut lip, 10_000_000), LipStep::default());
}

/// Draws are recorded as `(first, last)` and answered in turn.
#[derive(Default)]
struct Draws {
    values: Vec<i32>,
    seen: Vec<(i32, i32)>,
}
impl RandomSource for Draws {
    fn range(&mut self, first: i32, last: i32) -> Result<i32, WorldError> {
        self.seen.push((first, last));
        Ok(self.values.remove(0))
    }
}

/// SPEC-006 §4.4 session-start draw order.
#[test]
fn spec006_session_start_draw_order() {
    let definition = HeadDefinition {
        index: 7,
        stem: "synth".into(),
        counts: [3; 3],
    };
    let mut rng = Draws {
        values: vec![60],
        ..Draws::default()
    };
    let head = TalkingHead::new(
        definition.clone(),
        Mood::Neutral,
        HeadIdle::default(),
        &mut rng,
    )
    .unwrap();
    // One draw opens the session: neutral variant 2, idle counter reset.
    assert_eq!(rng.seen, [(1, 100)]);
    assert_eq!(head.art_id().unwrap(), 0x08042007);
    assert_eq!(head.idle_seconds, 0);
    assert_eq!(head.shown, None, "no head frame while the session opens");
    // The first reply's speech runs to its end, with no draws.
    let mut speaking = head.clone();
    let mut lip = LipSync::new(lip_a(), "s".into(), None);
    lip.start(true);
    for position in [0, 101, 302, 601] {
        if let Some(phoneme) = at(&mut lip, position).draw {
            speaking.phoneme(usize::from(phoneme)).unwrap();
        }
    }
    lip.set_clock(0, false);
    assert!(lip.tick().ended);
    speaking.speech_end();
    assert_eq!(
        speaking.shown,
        Some(HeadFrame {
            animation: 10,
            variant: 0,
            frame: 0
        })
    );
    assert_eq!(definition.art_id(10, 0).unwrap(), 0x080A0007);
    assert_eq!(speaking.idle_seconds, 3);
    // The next tick: the wait has elapsed (retained timestamp zero); 10 joins
    // the idle counter, the threshold is drawn, then the fidget with
    // floor(13 / 2) = 6 added. Variant 2 is never shown.
    let mut rng = Draws {
        values: vec![2, 70],
        ..Draws::default()
    };
    assert!(speaking.tick(10000, 4, 10, &mut rng).unwrap());
    assert_eq!(rng.seen, [(0, 3), (1, 100)]);
    // 70 + 6 = 76 selects variant 2 again, and the idle counter resets.
    assert_eq!(
        (speaking.phase, speaking.variant, speaking.idle_seconds),
        (HeadPhase::Fidget, 2, 0)
    );
    let mut rng = Draws {
        values: vec![2, 71],
        ..Draws::default()
    };
    let mut other = head.clone();
    other.speech_end();
    other.tick(10000, 4, 10, &mut rng).unwrap();
    assert_eq!(other.variant, 3, "71 + 6 = 77 selects variant 3");
    // Without speech the first tick shows frame 0 of `synthnf2.frm`.
    let mut quiet = head.clone();
    assert!(quiet.tick(1000, 4, 10, &mut Draws::default()).unwrap());
    assert_eq!(
        quiet.shown_path().as_deref(),
        Some("art/heads/synthnf2.frm")
    );
    assert_eq!(quiet.shown.unwrap().frame, 0);
    // An option that stops the speech first: the chosen fidget then plays
    // from frame 0.
    let mut stopped = head.clone();
    stopped.phoneme(9).unwrap();
    assert!(stopped.tick(1000, 4, 10, &mut Draws::default()).unwrap());
    assert_eq!(
        stopped.shown,
        Some(HeadFrame {
            animation: 4,
            variant: 2,
            frame: 0
        })
    );
}

fn host(resources: &[(&str, Vec<u8>)]) -> WorldHost<TestAssets> {
    let assets = TestAssets {
        resources: resources
            .iter()
            .map(|(path, bytes)| (path.to_string(), bytes.clone()))
            .collect(),
        ..TestAssets::default()
    };
    let mut host = WorldHost::new(
        WorldState::new(CharacterSheet::default(), 1).unwrap(),
        assets,
    );
    let definition = HeadDefinition {
        index: 7,
        stem: "synth".into(),
        counts: [0; 3],
    };
    host.world.dialogue.head = Some(TalkingHead::without_fidget(
        definition,
        Mood::Neutral,
        HeadIdle::default(),
    ));
    host.world.dialogue.updater = true;
    host
}

fn speech_effects(host: &mut WorldHost<TestAssets>) -> Vec<WorldEffect> {
    host.world
        .effects
        .drain(..)
        .filter(|effect| matches!(effect, WorldEffect::Speech(_) | WorldEffect::StopSpeech))
        .collect()
}

fn shown(host: &WorldHost<TestAssets>) -> Option<(u8, usize)> {
    let head = host.world.dialogue.head.as_ref()?;
    head.shown.map(|shown| (shown.animation, shown.frame))
}

#[test]
fn spec010_speech_requests_through_the_host() {
    let lip = lip_bytes(b"VOICE", 0, &[0, 9], &[(1, 0), (1, 100), (1, 50_000)]);
    let mut host = host(&[("sound/speech/synth/voice.LIP", lip)]);
    host.request_speech(b"voice.lip");
    assert_eq!(
        speech_effects(&mut host),
        [WorldEffect::Speech(b"voice.lip".to_vec())]
    );
    // The audio stem comes from the lip file.
    assert_eq!(
        host.speech_file(b"voice.lip").as_deref(),
        Some("sound/speech/synth/VOICE.ACM")
    );
    // Until the start is reported, the head updater waits.
    host.update_head(0).unwrap();
    assert_eq!(shown(&host), None);
    host.speech_started(true);
    host.speech_clock(Some(0));
    assert!(host.update_head(10).unwrap());
    assert_eq!(shown(&host), Some((10, 0)), "safe rule: first tick draws");
    host.speech_clock(Some(101));
    assert!(host.update_head(20).unwrap());
    assert_eq!(shown(&host), Some((10, 8)));
    // The audio finishes: speech frame 0, waiting, idle counter 3.
    host.speech_clock(None);
    assert!(host.update_head(30).unwrap());
    assert_eq!(shown(&host), Some((10, 0)));
    let head = host.world.dialogue.head.as_ref().unwrap();
    assert_eq!((head.phase, head.idle_seconds), (HeadPhase::Waiting, 3));
    assert!(host.world.dialogue.speech.is_none() && host.world.dialogue.lip_sync.is_none());

    // The final marker passed before the audio end stops the audio.
    host.request_speech(b"voice");
    host.speech_started(true);
    host.speech_clock(Some(50_001));
    host.world.effects.clear();
    assert!(host.update_head(40).unwrap());
    assert_eq!(speech_effects(&mut host), [WorldEffect::StopSpeech]);
    assert!(host.world.dialogue.speech.is_none());

    // No head updater while the session opens.
    host.world.dialogue.updater = false;
    host.world.dialogue.head.as_mut().unwrap().shown = None;
    host.request_speech(b"voice");
    host.speech_started(true);
    host.speech_clock(Some(200));
    assert!(!host.update_head(50).unwrap());
    assert_eq!(shown(&host), None);
    host.world.dialogue.updater = true;
    assert!(host.update_head(60).unwrap());
    assert_eq!(shown(&host), Some((10, 8)));
    // A stop without an ending draws nothing and keeps the frame.
    host.world.effects.clear();
    host.stop_speech();
    assert_eq!(speech_effects(&mut host), [WorldEffect::StopSpeech]);
    assert_eq!(shown(&host), Some((10, 8)));
    assert_eq!(host.world.dialogue.head.as_ref().unwrap().idle_seconds, 3);
}

#[test]
fn spec010_missing_and_malformed_lip_files() {
    let mut wrong = LIP_A.to_vec();
    wrong[3] = 9;
    let empty = lip_bytes(b"EMPTY", 0, &[], &[]);
    let mut host = host(&[
        ("sound/speech/synth/wrong.LIP", wrong),
        ("sound/speech/synth/short.LIP", LIP_A[..90].to_vec()),
        ("sound/speech/synth/empty.LIP", empty),
    ]);
    // A missing file: the audio named by the field, one phoneme 0.
    host.request_speech(b"gone");
    assert_eq!(
        host.speech_file(b"gone").as_deref(),
        Some("sound/speech/synth/gone.ACM")
    );
    assert_eq!(
        host.world.dialogue.lip_sync.as_ref().unwrap().lip,
        LipFile::silent()
    );
    // A missing ACM: lip-sync stops, and the next tick ends the speech.
    host.speech_started(false);
    host.update_head(0).unwrap();
    assert!(host.world.dialogue.speech.is_none());
    assert_eq!(host.world.dialogue.head.as_ref().unwrap().idle_seconds, 3);
    // Another version is logged and handled as missing.
    host.request_speech(b"wrong");
    assert_eq!(
        host.world.dialogue.lip_sync.as_ref().unwrap().lip,
        LipFile::silent()
    );
    assert!(host.world.effects.iter().any(|effect| matches!(
        effect,
        WorldEffect::Diagnostic(text) if text.contains("version 9")
    )));
    // A short file abandons the speech: the earlier speech state is left
    // without audio, and the next tick ends it normally.
    host.speech_started(true);
    host.world.effects.clear();
    host.request_speech(b"short");
    assert_eq!(speech_effects(&mut host), [WorldEffect::StopSpeech]);
    assert_eq!(host.world.dialogue.speech.as_deref(), Some(&b"wrong"[..]));
    host.world.dialogue.head.as_mut().unwrap().idle_seconds = 0;
    host.update_head(0).unwrap();
    assert!(host.world.dialogue.speech.is_none());
    assert_eq!(host.world.dialogue.head.as_ref().unwrap().idle_seconds, 3);
    // Zero counts abandon it too; with no earlier speech nothing happens.
    host.request_speech(b"empty");
    assert!(speech_effects(&mut host).is_empty());
    assert!(host.world.dialogue.speech.is_none());
}

#[test]
fn spec002_language_filter_on_script_messages() {
    let mut host = host(&[("data/badwords.txt", b"darn\r\n".to_vec())]);
    host.assets.catalogs.insert(
        1,
        MsgCatalog::parse(b"{100}{snd}{oh darn}\n{101}{}{fine}").unwrap(),
    );
    host.load_language_filter();
    assert_eq!(*host.script_messages.words, BadWords::parse(b"darn"));
    // Preference 0 at first use: loaded unfiltered, no draw, and kept so.
    let rng = host.world.rng.clone();
    assert_eq!(host.script_message(1, 100).unwrap().text, b"oh darn");
    host.world.ai.language_filter = Some(1);
    assert_eq!(host.script_message(1, 100).unwrap().text, b"oh darn");
    assert_eq!(host.world.rng, rng);
    // Reapplying preferences discards the catalog; it reloads filtered, with
    // one draw per load, not per lookup.
    host.reevaluate_language_filter().unwrap();
    let mut expected = host.world.rng.clone();
    let k = expected.range(1, 29).unwrap() as usize - 1;
    let message = host.script_message(1, 100).unwrap();
    assert_eq!(message.sound, b"snd");
    let mut text = b"oh darn".to_vec();
    host.script_messages.words.filter_text(&mut text, k);
    assert_eq!(message.text, text);
    assert_ne!(message.text, b"oh darn");
    assert_eq!(host.script_message(1, 101).unwrap().text, b"fine");
    assert_eq!(host.world.rng, expected);
    // Value 2 never filters.
    host.world.ai.language_filter = Some(2);
    host.reevaluate_language_filter().unwrap();
    assert_eq!(host.script_message(1, 100).unwrap().text, b"oh darn");
    assert_eq!(host.script_message(2, 100), None);
}
