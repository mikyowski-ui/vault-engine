//! SPEC-010 §4.3-4.5 and §4.8 name construction, lookup, volume,
//! speech-path, and device-attenuation vectors.
use crate::sfx::*;

#[test]
fn spec010_name_construction_vectors() {
    let character = |animation, weapon, extra| character_name(b"hmjmps", animation, weapon, extra);
    assert_eq!(character(16, 0, 4).unwrap(), b"HMJMPSZQ");
    assert_eq!(character(21, 0, 2).unwrap(), b"HMJMPSYB");
    assert_eq!(character(20, 0, 1).unwrap(), b"HMJMPSBA");
    assert_eq!(character(20, 0, 3).unwrap(), b"HMJMPSZA");
    assert_eq!(
        character(38, 0, 5).unwrap(),
        b"HMJMPSHC",
        "extra supplies weapon 5"
    );
    assert_eq!(character(39, 0, 0), None, "weapon 0 is rejected");
    assert_eq!(character(0, 11, 0), None, "weapon codes 11-15 fail");

    let item = Target::Material(1);
    assert_eq!(
        weapon_name(effect::HIT, b'E', 2, material_letter(effect::HIT, item, 0)).unwrap(),
        b"WHE1MXX1"
    );
    assert_eq!(
        weapon_name(effect::HIT, b'E', 5, material_letter(effect::HIT, item, 0)).unwrap(),
        b"WHE2MXX1"
    );
    assert_eq!(
        weapon_name(
            effect::HIT,
            b'H',
            0,
            material_letter(effect::HIT, Target::Other, 0)
        )
        .unwrap(),
        b"WHH1FXX1"
    );
    assert_eq!(
        weapon_name(effect::HIT, b'H', 0, material_letter(effect::HIT, item, 6)).unwrap(),
        b"WHH1XXX1"
    );
    assert_eq!(
        weapon_name(effect::ATTACK, b'H', 3, b'X').unwrap(),
        b"WAH2XXX1"
    );
    assert_eq!(
        weapon_name(effect::READY, b'A', 3, b'X').unwrap(),
        b"WRA1XXX1"
    );
    assert_eq!(
        weapon_name(effect::OUT_OF_AMMO, b'A', 5, b'X').unwrap(),
        b"WOA1XXX1"
    );
    assert_eq!(
        weapon_name(effect::ATTACK, 0, 2, b'X').unwrap(),
        b"WA",
        "zero ends the name"
    );
    // Materials: glass/metal/plastic M, wood W, dirt/stone/cement S,
    // leather and no material F; only hits with a target take a letter.
    let letters: Vec<u8> = (0..8)
        .map(|material| material_letter(effect::HIT, Target::Material(material), 0))
        .collect();
    assert_eq!(letters, b"MMMWSSSF");
    assert_eq!(material_letter(effect::HIT, Target::None, 0), b'X');
    assert_eq!(material_letter(effect::FLIGHT, item, 0), b'X');
    assert_eq!(material_letter(effect::HIT, item, 3), b'X');
    assert_eq!(material_letter(effect::HIT, item, 5), b'X');

    assert_eq!(
        open_name(Opened::Scenery(Some(b'C')), 1).unwrap(),
        b"SCDOORSC"
    );
    assert_eq!(open_name(Opened::Scenery(None), 0).unwrap(), b"SODOORSA");
    assert_eq!(open_name(Opened::Item(b'B'), 0).unwrap(), b"IOCNTNRB");
    assert_eq!(open_name(Opened::Item(0), 0).unwrap(), b"IOCNTNR");
    assert_eq!(open_name(Opened::Item(b'b'), 2).unwrap(), b"ILCNTNRB");
    assert_eq!(ambient_name(b"wind"), b"A  WIND1");
    assert_eq!(interface_name(b"button"), b"NBUTTON1");
    assert_eq!(
        ambient_name(b"thunder12"),
        b"ATHUNDER121",
        "never truncated"
    );
    assert_eq!(scenery_name(1, 4, b"ab").unwrap(), b"SPU  AB1");
    assert_eq!(scenery_name(0, 0, b"doors").unwrap(), b"SAODOORS1");
    // Action and effect codes beyond the letter tables have no name.
    assert_eq!(open_name(Opened::Item(b'A'), 5), None);
    assert_eq!(scenery_name(0, 9, b"x"), None);
    assert_eq!(weapon_name(5, b'A', 2, b'X'), None);
}

/// Request, owner, attempt order, and resolved stem.
type LookupCase<'a> = (&'a [u8], Option<Owner>, &'a [&'a [u8]], Option<&'a [u8]>);

#[test]
fn spec010_lookup_vectors() {
    let files: [&[u8]; 5] = [
        b"HMXXXXAN",
        b"HFXXXXAA",
        b"HMXXXXAA",
        b"MAMTNTAN",
        b"IB1P1XX1",
    ];
    let exists = |stem: &[u8]| files.iter().any(|file| file.eq_ignore_ascii_case(stem));
    let male = Some(Owner {
        critter: true,
        female: false,
    });
    let female = Some(Owner {
        critter: true,
        female: true,
    });
    let cases: [LookupCase; 8] = [
        (
            b"HAPOWRAA",
            male,
            &[b"HAPOWRAA", b"HMXXXXAA"],
            Some(b"HMXXXXAA"),
        ),
        (
            b"HAPOWRAA",
            female,
            &[b"HAPOWRAA", b"HFXXXXAA"],
            Some(b"HFXXXXAA"),
        ),
        (
            b"HFCMBTAN",
            male,
            &[b"HFCMBTAN", b"HFXXXXAN", b"HMXXXXAN"],
            Some(b"HMXXXXAN"),
        ),
        (
            b"NACHLDAA",
            female,
            &[b"NACHLDAA", b"HFXXXXAA"],
            Some(b"HFXXXXAA"),
        ),
        (b"HAPOWRAA", None, &[b"HAPOWRAA"], None),
        (
            b"MALIEUAN",
            None,
            &[b"MALIEUAN", b"MAMTNTAN"],
            Some(b"MAMTNTAN"),
        ),
        (b"malieuan", None, &[b"malieuan"], None),
        (b"ib1p1xx1", None, &[b"ib1p1xx1"], Some(b"ib1p1xx1")),
    ];
    for (name, owner, attempts, resolved) in cases {
        let found = lookup(name, owner, exists);
        assert_eq!(
            found.attempts,
            attempts,
            "{}",
            String::from_utf8_lossy(name)
        );
        assert_eq!(found.resolved.as_deref(), resolved);
    }
    // A non-critter owner takes no gender alias; MAMTN2 aliases too.
    let item = Some(Owner {
        critter: false,
        female: false,
    });
    assert_eq!(lookup(b"HAPOWRAA", item, exists).attempts.len(), 1);
    assert_eq!(
        lookup(b"MAMTN2AN", None, exists).resolved.as_deref(),
        Some(b"MAMTNTAN".as_slice())
    );
    // A name shorter than 7 characters gives an empty tail.
    assert_eq!(
        lookup(b"HA", male, |_| false).attempts,
        [b"HA".to_vec(), b"HMXXXX".to_vec()]
    );
    assert_eq!(effect_path(b"HMXXXXAA"), "sound/sfx/HMXXXXAA.ACM");
}

#[test]
fn spec010_relative_volume_and_speech_paths() {
    for (distance, perception, relative, applied) in [
        (5, 5, 32767, 16383),
        (6, 5, 28399, 14199),
        (9, 5, 15292, 7645),
        (10, 5, 10922, 5460),
        (7, 4, 16384, 8191),
        (3, 0, 10922, 5460),
        (0, 0, 32767, 16383),
    ] {
        let volume = relative_volume(false, distance, perception);
        assert_eq!(volume, relative, "d {distance} P {perception}");
        assert_eq!(applied_volume(volume, 16383), applied);
    }
    assert_eq!(relative_volume(true, 40, 1), FULL_VOLUME);

    // Speech: the sound field before its first dot, at most 8 characters.
    assert_eq!(speech_stem(b"vic_12.lip"), b"vic_12");
    assert_eq!(speech_stem(b"abcdefghij"), b"abcdefgh");
    assert_eq!(speech_stem(b"abcdefgh"), b"abcdefgh", "8 characters work");
    assert_eq!(
        speech_path(b"aradesh", b"ara_101.acm").unwrap(),
        "sound/speech/aradesh/ara_101.ACM"
    );
    assert_eq!(speech_path(b"bad.stem", b"x"), None);
    assert_eq!(
        narration_path(b"narrator\\options"),
        "sound/speech/narrator/options.ACM"
    );
}

#[test]
fn spec010_multihex_effect_distance() {
    // §3.11 step 3: minus 1 for each multi-hex object, not below 0.
    assert_eq!(effect_distance(6, [false, false]), 6);
    assert_eq!(effect_distance(6, [true, false]), 5);
    assert_eq!(effect_distance(6, [false, true]), 5);
    assert_eq!(effect_distance(6, [true, true]), 4);
    assert_eq!(effect_distance(1, [true, true]), 0);
    assert_eq!(effect_distance(0, [true, false]), 0);
}

#[test]
fn spec010_volume_level_vectors() {
    // §4.8: (master M, level L, v, A).
    let effect = applied_volume(FAR_VOLUME, 22281);
    assert_eq!(effect, 7426);
    let music = music_level(22281);
    assert_eq!(music, 20944);
    let speech = speech_level(22281);
    assert_eq!(speech, 15373);
    for (master, level, volume, decibels) in [
        (22281, 22281, 15150, -1112),
        (22281, effect, 5049, -2698),
        (22281, music, 14241, -1202),
        (22281, speech, 10453, -1648),
        (22281, LIBRARY_LEVEL, 22281, -556),
        (32767, 32, 32, -9999),
        (32767, 31, 31, -10000),
        (0, 32767, 0, -10000),
        (0, 1, 0, -10000),
    ] {
        assert_eq!(device_volume(master, level), volume, "M {master} L {level}");
        assert_eq!(attenuation(volume), decibels, "v {volume}");
        assert_eq!(device_attenuation(master, level), decibels);
    }
    // Movie audio: v = trunc(0.94 x music_volume), unscaled by master.
    assert_eq!(attenuation(music_level(22281)), -645);
    // v is capped at 32767; full volume is 0 dB.
    assert_eq!(device_volume(32767, 32767), 32767);
    assert_eq!(attenuation(32767), 0);
    // The double-precision products: 0.69 x 300 truncates to 206.
    assert_eq!(speech_level(300), 206);
    assert_eq!(speech_level(32767), 22609);
    assert_eq!(speech_level(0), 0);
    assert_eq!(music_level(32767), 30800);
    assert_eq!(music_level(1), 0);
    // Amplitude 10^(A / 2000): every 2000 is a factor of 10.
    assert!((attenuation_gain(-2000) - 0.1).abs() < 1e-6);
    assert_eq!(attenuation_gain(0), 1.0);
}
