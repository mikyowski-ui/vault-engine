//! SPEC-006 §3.9 speech requests (the §4.4 `message_str` vector and the
//! stale-head safe rule), review history and the Ask About lookup, and the
//! SPEC-014 §3.45 Tell Me About tests and Review texts (Fixture R).
use crate::{
    CharacterSheet, ObjectRef, RandomSource, WorldEffect, WorldHost, WorldState,
    dialogue::{
        DialogueOption, DialogueText, HeadDefinition, HeadIdle, Mood, REVIEW_CAPACITY, ReviewEntry,
        TalkingHead,
    },
    fixture_world as fixture,
    host_talk::{ReviewText, TellMeAbout, ask_tokens},
    test_support::TestAssets,
};
use std::sync::Arc;
use vault_formats::{MsgCatalog, Prototype};

/// The player script's list 1, the target `abel`'s list 5 (script index
/// 4), and `proto.msg`.
fn host() -> (WorldHost<TestAssets>, ObjectRef) {
    let mut assets = TestAssets::default();
    assets.catalogs.insert(
        1,
        MsgCatalog::parse(
            b"{7000}{}{Killian}\n{7005}{}{Water Chip}\n\
              {7100}{}{Killian is the mayor. He runs a store.}\n\
              {7105}{}{The chip is in Vault 12.}\n{975}{}{Player script name answer}",
        )
        .unwrap(),
    );
    assets.catalogs.insert(
        5,
        MsgCatalog::parse(
            b"{170}{hry174}{Spoken}\n{171}{}{Silent}\n{175}{hry175}{Replaced}\n\
              {970}{}{Me Abel.}\n{972}{}{error}\n\
              {980}{}{Hng.}\n{981}{abel981}{Why do you say that?}",
        )
        .unwrap(),
    );
    assets.insert_messages(
        "proto.msg",
        b"{650}{}{[Done]}\n{904}{}{This person has nothing to tell you about.}",
    );
    let mut host = WorldHost::new(
        WorldState::new(CharacterSheet::default(), 1).unwrap(),
        assets,
    );
    let abel = host
        .world
        .spawn(
            Arc::new(Prototype::parse(&fixture::prototype(1, 0)).unwrap()),
            20100,
            0,
        )
        .unwrap();
    host.world.get_mut(abel).unwrap().name = b"Abel".to_vec();
    host.world.add_script(abel, 4, 4, 4).unwrap();
    host.world.map.header.map_number = 10;
    host.world.dialogue.target = abel;
    (host, abel)
}

/// A talk session whose `start_gdialog` set up talking head `harry`.
fn talking(host: &mut WorldHost<TestAssets>) {
    let head = TalkingHead::without_fidget(
        HeadDefinition {
            index: 3,
            stem: "harry".into(),
            counts: [0; 3],
        },
        Mood::Neutral,
        HeadIdle::default(),
    );
    let target = host.world.dialogue.target;
    host.world.dialogue.talking = true;
    host.world.dialogue.start(target, 1, 0, Some(head)).unwrap();
}

fn speech(host: &mut WorldHost<TestAssets>) -> Vec<WorldEffect> {
    host.world
        .effects
        .drain(..)
        .filter(|effect| matches!(effect, WorldEffect::Speech(_) | WorldEffect::StopSpeech))
        .collect()
}

fn literal(text: &[u8]) -> DialogueText {
    DialogueText::Literal(text.to_vec())
}

fn message(list: i32, message: i32) -> DialogueText {
    DialogueText::Message { list, message }
}

/// SPEC-006 §4.4: `gsay_reply` with the literal text of `message_str`.
#[test]
fn spec006_message_str_speech_vector() {
    let (mut host, _) = host();
    talking(&mut host);
    // Sound field `X`: speech starts as `message_str` runs, with the same
    // lip-sync state as a reply's.
    let text = host.spoken_message(5, 170).unwrap().text;
    assert_eq!(speech(&mut host), [WorldEffect::Speech(b"hry174".to_vec())]);
    assert!(host.world.dialogue.lip_sync.is_some());
    host.reply(1, literal(&text)).unwrap();
    host.world.dialogue.begin(0).unwrap();
    host.realize_dialogue().unwrap();
    // The literal reply makes no request: `X` keeps playing.
    assert!(speech(&mut host).is_empty());
    assert_eq!(host.world.dialogue.speech.as_deref(), Some(&b"hry174"[..]));
    assert_eq!(host.world.dialogue.resolved_reply, b"Spoken");
    // No sound field: no speech, the earlier one continues.
    assert_eq!(host.spoken_message(5, 171).unwrap().text, b"Silent");
    assert!(speech(&mut host).is_empty());
    assert_eq!(host.world.dialogue.speech.as_deref(), Some(&b"hry174"[..]));
    // A message-backed reply with sound field `Y` replaces it at display.
    host.reply(1, message(5, 175)).unwrap();
    host.realize_dialogue().unwrap();
    assert_eq!(
        speech(&mut host),
        [
            WorldEffect::StopSpeech,
            WorldEffect::Speech(b"hry175".to_vec())
        ]
    );
    assert_eq!(host.world.dialogue.speech.as_deref(), Some(&b"hry175"[..]));
    // A missing message reads `Error` and requests nothing.
    host.reply(1, message(5, 999)).unwrap();
    host.realize_dialogue().unwrap();
    assert_eq!(host.world.dialogue.resolved_reply, b"Error");
    assert!(speech(&mut host).is_empty());
    // Option texts never request speech.
    host.reply(1, literal(b"Pick")).unwrap();
    host.world
        .dialogue
        .add_option(DialogueOption {
            text: message(5, 170),
            procedure: 0,
            reaction: 50,
        })
        .unwrap();
    host.realize_dialogue().unwrap();
    assert!(speech(&mut host).is_empty());
}

/// SPEC-006 §3.9 "Stale head": no request outside a talk session, with no
/// head, or before this session's `start_gdialog` (safe rule).
#[test]
fn spec006_speech_needs_this_sessions_talking_head() {
    let (mut host, abel) = host();
    talking(&mut host);
    assert!(host.speech_enabled());
    // The session ends; the next talk's `talk_p_proc` runs `message_str`
    // before its `start_gdialog`: the stale head is not used.
    assert!(host.world.dialogue.end());
    assert!(!host.world.dialogue.talking);
    host.world.dialogue.talking = true;
    host.world.dialogue.target = abel;
    assert!(!host.speech_enabled());
    assert_eq!(host.spoken_message(5, 170).unwrap().text, b"Spoken");
    assert!(speech(&mut host).is_empty());
    // A dialogue with head -1 never speaks.
    host.world.dialogue.start(abel, 1, 0, None).unwrap();
    host.spoken_message(5, 170);
    assert!(speech(&mut host).is_empty());
    // A talking head outside a talk session does not either.
    talking(&mut host);
    host.world.dialogue.talking = false;
    host.spoken_message(5, 170);
    assert!(speech(&mut host).is_empty());
}

/// SPEC-006 §3.9 review history.
#[test]
fn spec006_review_history() {
    let (mut host, _) = host();
    talking(&mut host);
    let dialogue = &mut host.world.dialogue;
    // A choice before any reply is ignored (safe rule).
    dialogue.waiting = true;
    dialogue
        .add_option(DialogueOption {
            text: literal(b"early"),
            procedure: 0,
            reaction: 50,
        })
        .unwrap();
    dialogue.choose(Some(0)).unwrap();
    assert!(dialogue.review.is_empty());
    // Replies add entries; a literal option records its bullet, a message
    // option its list and number, the sentinels themselves.
    for (reply, option) in [
        (message(5, 980), literal(b"Hi")),
        (literal(b"Plain"), message(5, 981)),
        (message(5, 171), message(-1, -1)),
        (message(5, 171), message(-2, -2)),
    ] {
        dialogue.set_reply(1, reply).unwrap();
        dialogue
            .add_option(DialogueOption {
                text: option,
                procedure: 0,
                reaction: 50,
            })
            .unwrap();
        dialogue.waiting = true;
        dialogue.choose(Some(0)).unwrap();
    }
    assert_eq!(
        dialogue.review[..2],
        [
            ReviewEntry {
                reply: message(5, 980),
                choice: Some(literal(b"\x95 Hi")),
            },
            ReviewEntry {
                reply: literal(b"Plain"),
                choice: Some(message(5, 981)),
            },
        ]
    );
    // Digit 0 records nothing.
    dialogue.set_reply(1, literal(b"Zero")).unwrap();
    dialogue.waiting = true;
    dialogue.choose(None).unwrap();
    assert_eq!(dialogue.review.last().unwrap().choice, None);
    assert_eq!(
        host.review_texts(),
        [
            ReviewText {
                reply: b"Hng.".to_vec(),
                choice: Some(b"\x95 Hi".to_vec()),
            },
            ReviewText {
                reply: b"Plain".to_vec(),
                choice: Some(b"Why do you say that?".to_vec()),
            },
            ReviewText {
                reply: b"Silent".to_vec(),
                choice: Some(Vec::new()),
            },
            ReviewText {
                reply: b"Silent".to_vec(),
                choice: Some(b"[Done]".to_vec()),
            },
            ReviewText {
                reply: b"Zero".to_vec(),
                choice: None,
            },
        ]
    );
    // Review looks texts up without speech; a missing message is `Error`.
    assert_eq!(host.review_text(&message(5, 170)), b"Spoken");
    assert_eq!(host.review_text(&message(9, 1)), b"Error");
    assert!(speech(&mut host).is_empty());
    // 80 entries: the 81st reply is only logged, and the 80th entry's
    // choice is recorded (safe rule).
    for _ in host.world.dialogue.review.len()..REVIEW_CAPACITY {
        host.reply(1, literal(b"More")).unwrap();
    }
    host.world.effects.clear();
    host.reply(1, literal(b"Lost")).unwrap();
    assert_eq!(host.world.dialogue.review.len(), REVIEW_CAPACITY);
    assert!(
        host.world
            .effects
            .iter()
            .any(|effect| matches!(effect, WorldEffect::Diagnostic(text) if text.contains("full")))
    );
    let dialogue = &mut host.world.dialogue;
    dialogue
        .add_option(DialogueOption {
            text: literal(b"Last"),
            procedure: 0,
            reaction: 50,
        })
        .unwrap();
    dialogue.waiting = true;
    dialogue.choose(Some(0)).unwrap();
    assert_eq!(
        dialogue.review[REVIEW_CAPACITY - 1].choice,
        Some(literal(b"\x95 Last"))
    );
    // `gsay_start` empties the history.
    dialogue.reset_options();
    assert!(dialogue.review.is_empty());
}

/// SPEC-014 §4.18 Fixture R: the Tell Me About tests.
#[test]
fn spec014_tell_me_about_tests() {
    let (mut host, abel) = host();
    talking(&mut host);
    let sid = host.world.script_for(abel).unwrap();
    let base = host.world.ensure_locals(sid).unwrap();
    // Reaction 20 (initialized): safe rule, `proto.msg` 904 is shown.
    host.world.lvars[base..base + 4].copy_from_slice(&[20, 1, 1, 0]);
    assert_eq!(host.tell_me_about(), TellMeAbout::Refused);
    assert_eq!(
        host.world.dialogue.resolved_reply,
        b"This person has nothing to tell you about."
    );
    // Any critter on map 35.
    host.world.lvars[base] = 60;
    host.world.map.header.map_number = 35;
    host.world.dialogue.resolved_reply.clear();
    assert_eq!(host.tell_me_about(), TellMeAbout::Refused);
    assert!(!host.world.dialogue.resolved_reply.is_empty());
    // Otherwise the window opens, and playing speech stops (safe rule).
    host.world.map.header.map_number = 10;
    host.spoken_message(5, 170);
    host.world.effects.clear();
    assert_eq!(host.tell_me_about(), TellMeAbout::Open);
    assert_eq!(speech(&mut host), [WorldEffect::StopSpeech]);
    assert!(host.world.dialogue.speech.is_none());
    // A target that is not a critter: nothing happens.
    let item = host
        .world
        .spawn(
            Arc::new(Prototype::parse(&fixture::prototype(0, 0)).unwrap()),
            20101,
            0,
        )
        .unwrap();
    host.world.dialogue.target = item;
    assert_eq!(host.tell_me_about(), TellMeAbout::Nothing);
}

/// SPEC-014 §4.18 Fixture R and SPEC-006 §3.9: the Ask About lookup.
#[test]
fn spec006_ask_about_lookup() {
    assert_eq!(
        ask_tokens(b" water\tchip,the.  Master"),
        [&b"water"[..], b"chip", b"the", b"Master"]
    );
    let (mut host, abel) = host();
    talking(&mut host);
    // Map 10: list 1's block starts at 7000; no draw is made.
    let rng = host.world.rng.clone();
    assert!(host.ask_about(b"killian").unwrap());
    assert_eq!(
        host.world.dialogue.resolved_reply,
        b"Killian is the mayor. He runs a store."
    );
    assert_eq!(host.world.rng, rng);
    // The safe rule tries the whole trimmed text first, so "Water Chip"
    // (unreachable by tokens) answers.
    assert!(host.ask_about(b"  water chip ").unwrap());
    assert_eq!(
        host.world.dialogue.resolved_reply,
        b"The chip is in Vault 12."
    );
    // "Abel.": the token equals the target's name; one non-`Error` answer
    // in 970-979 (972 reads "error", which is excluded): `random(1, 1)`.
    let mut expected = host.world.rng.clone();
    assert_eq!(expected.range(1, 1).unwrap(), 1);
    assert!(host.ask_about(b"Abel.").unwrap());
    assert_eq!(host.world.dialogue.resolved_reply, b"Me Abel.");
    assert_eq!(host.world.rng, expected);
    // "xyz": the fallback draws once over 980 and 981, and the answer
    // speaks as a reply does.
    let mut expected = host.world.rng.clone();
    let pick = expected.range(1, 2).unwrap();
    host.world.effects.clear();
    assert!(host.ask_about(b"xyz").unwrap());
    assert_eq!(host.world.rng, expected);
    if pick == 1 {
        assert_eq!(host.world.dialogue.resolved_reply, b"Hng.");
        assert!(speech(&mut host).is_empty());
    } else {
        assert_eq!(host.world.dialogue.resolved_reply, b"Why do you say that?");
        assert_eq!(
            speech(&mut host),
            [WorldEffect::Speech(b"abel981".to_vec())]
        );
    }
    // A missing keyword message reads `Error`, so "error" matches the
    // target's 1000, and its missing answer 1100 shows `Error`.
    assert!(host.ask_about(b"ERROR").unwrap());
    assert_eq!(host.world.dialogue.resolved_reply, b"Error");
    // A target with no script: no name lookup and no fallback.
    host.world.get_mut(abel).unwrap().raw.script_id = -1;
    let rng = host.world.rng.clone();
    host.world.dialogue.resolved_reply = b"kept".to_vec();
    assert!(!host.ask_about(b"Abel").unwrap());
    assert_eq!(host.world.dialogue.resolved_reply, b"kept");
    assert_eq!(host.world.rng, rng);
    // Its word lookup still searches list 1.
    assert!(host.ask_about(b"Killian").unwrap());
}
