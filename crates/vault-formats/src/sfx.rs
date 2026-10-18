//! SPEC-010 §3.8-3.11: sound-effect name builders, the effect lookup and
//! alias chain, speech file paths, and distance-based relative volume.
//! Builders return names without a directory or extension.
use crate::fid::critter_suffix;

/// Weapon effect types 0-4 and their letters.
pub mod effect {
    pub const READY: u8 = 0;
    pub const ATTACK: u8 = 1;
    pub const OUT_OF_AMMO: u8 = 2;
    pub const FLIGHT: u8 = 3;
    pub const HIT: u8 = 4;
}
const EFFECT_LETTERS: [u8; 5] = *b"RAOFH";
/// Open/close actions 0-4: open, close, lock, unlock, use.
const ACTION_LETTERS: [u8; 5] = *b"OCLNU";

/// Uppercase ASCII `a`-`z`; a zero byte (from a zero sound byte) ends the
/// name at that point.
fn finish(mut name: Vec<u8>) -> Vec<u8> {
    if let Some(end) = name.iter().position(|byte| *byte == 0) {
        name.truncate(end);
    }
    name.make_ascii_uppercase();
    name
}

/// Character effect: the unaliased art-list stem, then the SPEC-007 suffix
/// for the animation (animation 38 takes its weapon code from `extra`),
/// with the first suffix letter overridden for falls with extra 2 (`y`) or
/// 3 (`z`) and for punch/kick contact (extra 4, `z`). `None` when the
/// suffix rule fails.
pub fn character_name(stem: &[u8], animation: u8, art_weapon: u8, extra: u8) -> Option<Vec<u8>> {
    let weapon = if animation == 38 { extra } else { art_weapon };
    let mut suffix = critter_suffix(animation, weapon).ok()?;
    match (animation, extra) {
        (20 | 21, 2) => suffix[0] = b'y',
        (20 | 21, 3) | (16 | 17, 4) => suffix[0] = b'z',
        _ => {}
    }
    Some(finish([stem, &suffix].concat()))
}

/// What a hit sound knows about its target.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Target {
    None,
    /// An item, scenery, or wall with its prototype material.
    Material(i32),
    /// Any other target category (critters included): no material.
    Other,
}
/// The material letter: `X` unless the effect is a hit on a target with a
/// weapon damage type other than plasma, EMP, or explosion; then glass,
/// metal, plastic `M`, wood `W`, dirt, stone, cement `S`, else `F`.
pub fn material_letter(effect: u8, target: Target, damage_type: i32) -> u8 {
    if effect != effect::HIT || target == Target::None || matches!(damage_type, 3 | 5 | 6) {
        return b'X';
    }
    match target {
        Target::Material(0..=2) => b'M',
        Target::Material(3) => b'W',
        Target::Material(4..=6) => b'S',
        _ => b'F',
    }
}
/// Weapon effect: `W`, the effect letter, the weapon sound byte (zero ends
/// the name), the variant digit (`1` for R and O; otherwise `1` for hit
/// modes 0, 2, 4 and `2` for the rest, kick included), the material letter,
/// `XX`, `1`. Effect types outside 0-4 have no name.
pub fn weapon_name(effect: u8, sound: u8, hit_mode: u8, material: u8) -> Option<Vec<u8>> {
    let letter = *EFFECT_LETTERS.get(usize::from(effect))?;
    let variant =
        if matches!(effect, effect::READY | effect::OUT_OF_AMMO) || matches!(hit_mode, 0 | 2 | 4) {
            b'1'
        } else {
            b'2'
        };
    Some(finish(vec![
        b'W', letter, sound, variant, material, b'X', b'X', b'1',
    ]))
}

/// The object an open/close name is built for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Opened {
    /// A scenery object's sound byte, or `None` when its prototype cannot
    /// be loaded (which gives `A`).
    Scenery(Option<u8>),
    /// Any other object, treated as an item, with its item sound byte.
    Item(u8),
}
/// `S` action `DOORS` sound byte for scenery; `I` action `CNTNR` sound byte
/// otherwise (a zero byte gives the 7-character name). Actions outside 0-4
/// have no name.
pub fn open_name(object: Opened, action: u8) -> Option<Vec<u8>> {
    let action = *ACTION_LETTERS.get(usize::from(action))?;
    Some(match object {
        Opened::Scenery(sound) => finish(
            [
                &[b'S', action],
                b"DOORS".as_slice(),
                &[sound.unwrap_or(b'A')],
            ]
            .concat(),
        ),
        Opened::Item(sound) => finish([&[b'I', action], b"CNTNR".as_slice(), &[sound]].concat()),
    })
}

/// `base` right-justified with spaces to at least `width` (never truncated).
fn justified(base: &[u8], width: usize) -> Vec<u8> {
    let mut text = vec![b' '; width.saturating_sub(base.len())];
    text.extend_from_slice(base);
    text
}
/// Ambient: `A`, base padded to 6, `1`.
pub fn ambient_name(base: &[u8]) -> Vec<u8> {
    finish([b"A".as_slice(), &justified(base, 6), b"1"].concat())
}
/// Interface and item: `N`, base padded to 6, `1`.
pub fn interface_name(base: &[u8]) -> Vec<u8> {
    finish([b"N".as_slice(), &justified(base, 6), b"1"].concat())
}
/// Scenery: `S`, `P` for selector 1 (passive) or `A`, the action letter,
/// base padded to 4, `1`. Actions outside 0-4 have no name.
pub fn scenery_name(selector: i32, action: u8, base: &[u8]) -> Option<Vec<u8>> {
    let mode = if selector == 1 { b'P' } else { b'A' };
    let action = *ACTION_LETTERS.get(usize::from(action))?;
    Some(finish(
        [&[b'S', mode, action], justified(base, 4).as_slice(), b"1"].concat(),
    ))
}

/// An owning object supplied by an animation-registered effect.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Owner {
    pub critter: bool,
    /// Gender stat 1.
    pub female: bool,
}
/// The stems tried, in order, and the first that exists.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Lookup {
    pub attempts: Vec<Vec<u8>>,
    pub resolved: Option<Vec<u8>>,
}
/// The VFS path of an effect stem.
pub fn effect_path(stem: &[u8]) -> String {
    format!("sound/sfx/{}.ACM", String::from_utf8_lossy(stem))
}
/// "From character 7 on" (empty for a shorter name).
fn tail(name: &[u8]) -> &[u8] {
    name.get(6..).unwrap_or(&[])
}
/// §3.9 lookup: the name itself; for a critter owner and a name starting
/// with `H` or `N`, `H` + gender letter + `XXXX` + tail (an `A` becomes `F`
/// or `M` by gender), then `HMXXXX` + tail after a female attempt; then,
/// for names starting exactly with `MALIEU` or `MAMTN2`, `MAMTNT` + tail.
/// `exists` receives each stem (matched case-insensitively by the caller).
pub fn lookup(name: &[u8], owner: Option<Owner>, mut exists: impl FnMut(&[u8]) -> bool) -> Lookup {
    let mut attempts = vec![name.to_vec()];
    if owner.is_some_and(|owner| owner.critter) && matches!(name.first(), Some(b'H' | b'N')) {
        let female = owner.is_some_and(|owner| owner.female);
        let gender = match name.get(1).copied() {
            Some(b'A') => {
                if female {
                    b'F'
                } else {
                    b'M'
                }
            }
            Some(other) => other,
            None => 0,
        };
        attempts.push([&[b'H', gender], b"XXXX".as_slice(), tail(name)].concat());
        if gender == b'F' {
            attempts.push([b"HMXXXX".as_slice(), tail(name)].concat());
        }
    }
    if name.starts_with(b"MALIEU") || name.starts_with(b"MAMTN2") {
        attempts.push([b"MAMTNT".as_slice(), tail(name)].concat());
    }
    let mut tried = Vec::new();
    for attempt in attempts {
        let found = exists(&attempt);
        tried.push(attempt);
        if found {
            let resolved = tried.last().cloned();
            return Lookup {
                attempts: tried,
                resolved,
            };
        }
    }
    Lookup {
        attempts: tried,
        resolved: None,
    }
}

/// §3.10 speech stem: the sound field up to its first `.`, then at most 8
/// characters.
pub fn speech_stem(sound: &[u8]) -> &[u8] {
    let stem = &sound[..sound.iter().position(|b| *b == b'.').unwrap_or(sound.len())];
    &stem[..stem.len().min(8)]
}
/// Dialogue speech: `sound/speech/<head stem>/<stem>.ACM`. A head stem with
/// a dot is rejected (the reference would cut only the lip-file path).
pub fn speech_path(head: &[u8], sound: &[u8]) -> Option<String> {
    if head.contains(&b'.') {
        return None;
    }
    Some(format!(
        "sound/speech/{}/{}.ACM",
        String::from_utf8_lossy(head),
        String::from_utf8_lossy(speech_stem(sound))
    ))
}
/// Narration plays `sound/speech/<relative name>.ACM`.
pub fn narration_path(name: &[u8]) -> String {
    format!(
        "sound/speech/{}.ACM",
        String::from_utf8_lossy(name).replace('\\', "/")
    )
}

pub const FULL_VOLUME: i32 = 32767;
pub const FAR_VOLUME: i32 = 10922;
/// §3.11 relative volume: full on screen or within Perception, then
/// linearly falling to `2P`, else 10922 (no division for `P = 0`).
pub fn relative_volume(on_screen: bool, distance: i32, perception: i32) -> i32 {
    if on_screen || distance <= perception {
        FULL_VOLUME
    } else if distance < perception.saturating_mul(2) {
        let fall = 21844 * i64::from(distance - perception) / i64::from(perception);
        FULL_VOLUME - fall as i32
    } else {
        FAR_VOLUME
    }
}
/// The applied effect volume `floor(V x S / 32767)`.
pub fn applied_volume(relative: i32, setting: i32) -> i32 {
    (i64::from(relative) * i64::from(setting) / i64::from(FULL_VOLUME)) as i32
}
