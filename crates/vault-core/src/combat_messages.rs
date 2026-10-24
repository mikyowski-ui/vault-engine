//! SPEC-007 §3.12 combat message lines: the message-monitor lines one
//! routine prints for a combat attack (after its attack animation, before
//! its damage applies), an explosion and scripted damage, from
//! `combat.msg`.
use crate::critical::{effect, location};
use crate::host_drugs::{Arg, format_message};

/// The message file of every line.
pub const FILE: &str = "combat.msg";
/// 107 "You are not strong enough to use this weapon properly." (no `.`).
pub const STRENGTH: i32 = 107;
/// 108 " and ".
pub const AND: i32 = 108;
/// 506 "You", the player's name in every line.
pub const PLAYER_NAME: i32 = 506;
/// Called-shot location names: 1000 + 10 × art alias + location
/// (SPEC-014 §3.38).
pub const LOCATION_BASE: i32 = 1000;

/// A critter as the lines name it.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Named {
    /// `combat.msg` 506 "You" for the player, otherwise its object name.
    pub name: Vec<u8>,
    pub player: bool,
    pub female: bool,
    /// The critter's damage flags had the dead bit `0x80` before this
    /// attack's damage was applied.
    pub dead: bool,
    /// The critter-list alias of its art (the location names), if known.
    pub alias: Option<i32>,
}
impl Named {
    /// B: 500 for the player whatever the gender, 600 male, 700 female.
    pub fn base(&self) -> i32 {
        match (self.player, self.female) {
            (true, _) => 500,
            (false, false) => 600,
            (false, true) => 700,
        }
    }
    /// S, the status-list base: 200 for the player whatever the gender,
    /// 300 male, 400 female.
    pub fn status_base(&self) -> i32 {
        self.base() - 300
    }
    /// The Verbose "killed" line's wording: 207 / 257 "were killed" for a
    /// male / female player, 307 / 407 "was killed" for an NPC.
    pub fn killed(&self) -> i32 {
        match (self.player, self.female) {
            (true, false) => 207,
            (true, true) => 257,
            (false, false) => 307,
            (false, true) => 407,
        }
    }
}

/// What the attack struck when it hit.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Struck {
    Critter(Named),
    /// A wall, scenery or other non-critter.
    Object,
}

/// Everything the lines of one attack read.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct AttackLines {
    /// The player attacks with a weapon whose minimum strength is above
    /// the player's strength.
    pub strength: bool,
    pub attacker: Option<Named>,
    /// The critter aimed at.
    pub aimed: Option<Named>,
    /// The struck object, when the attack hit.
    pub struck: Option<Struck>,
    /// The struck object is not the critter aimed at.
    pub wrong_target: bool,
    /// The hit location; uncalled counts as torso.
    pub location: u8,
    pub damage: i32,
    /// The struck target's damage flags (dead included when lethal).
    pub flags: u32,
    /// A critical hit, and its section 3.6 critical-effect message (the
    /// failed-save message after a failed save).
    pub critical: bool,
    pub critical_message: Option<i32>,
    /// A miss that is a critical failure.
    pub critical_miss: bool,
    pub attacker_damage: i32,
    pub attacker_flags: u32,
    /// Spray, explosion or knock-on victims: name, damage and flags.
    pub extras: Vec<(Named, i32, u32)>,
}

/// The lines, in order, each ending with the appended `.` (but the
/// strength line). `verbose` is the `combat_messages` preference (1).
pub fn lines(
    attack: &AttackLines,
    verbose: bool,
    text: &dyn Fn(i32) -> Option<Vec<u8>>,
) -> Vec<Vec<u8>> {
    let text = |id: i32| text(id).unwrap_or_default();
    let mut out = Vec::new();
    let finish = |mut line: Vec<u8>| {
        line.push(b'.');
        line
    };
    // 1. Strength.
    if attack.strength {
        out.push(text(STRENGTH));
    }
    let attacker = attack.attacker.as_ref();
    // 2. Wrong target.
    if attack.wrong_target {
        match &attack.struck {
            Some(Struck::Critter(struck)) => {
                let aimed = attack.aimed.as_ref();
                let line = if aimed.is_some_and(|aimed| aimed.player) {
                    format_message(&text(struck.base() + 8), &[Arg::Text(&struck.name)])
                } else {
                    let name = aimed.map(|aimed| aimed.name.as_slice()).unwrap_or_default();
                    format_message(
                        &text(struck.base() + 9),
                        &[Arg::Text(&struck.name), Arg::Text(name)],
                    )
                };
                out.push(finish(line));
            }
            Some(Struck::Object) => {
                if let Some(attacker) = attacker {
                    let line =
                        format_message(&text(attacker.base() + 15), &[Arg::Text(&attacker.name)]);
                    out.push(finish(line));
                }
            }
            None => {}
        }
    }
    // 3. Struck target.
    if let Some(Struck::Critter(struck)) = &attack.struck
        && !struck.dead
    {
        let mut line = struck_line(struck, attack, &text);
        let effect_text = attack
            .critical_message
            .filter(|_| verbose && attack.critical)
            .map(text);
        match effect_text {
            Some(effect_text) => {
                line.extend(effect_text);
                out.push(finish(line));
                if attack.flags & effect::DEAD != 0 {
                    let mut killed = struck.name.clone();
                    killed.push(b' ');
                    killed.extend(text(struck.killed()));
                    out.push(finish(killed));
                }
            }
            None => {
                line.extend(status_list(struck, attack.flags, &text));
                out.push(finish(line));
            }
        }
    }
    // 4. Attacker.
    if let Some(attacker) = attacker
        && !attacker.dead
    {
        let base = attacker.base();
        let hit = attack.struck.is_some();
        if !hit {
            let mut line = if attack.critical_miss {
                wording(base, [14, 33, 34], attacker, attack.attacker_damage, &text)
            } else {
                format_message(&text(base + 15), &[Arg::Text(&attacker.name)])
            };
            line.extend(status_list(attacker, attack.attacker_flags, &text));
            out.push(finish(line));
        }
        if (hit || !attack.critical_miss) && attack.attacker_damage > 0 {
            let mut line = wording(base, [27, 23, 13], attacker, attack.attacker_damage, &text);
            line.extend(status_list(attacker, attack.attacker_flags, &text));
            out.push(finish(line));
        }
    }
    // 5. Extra victims.
    for (victim, damage, flags) in &attack.extras {
        if victim.dead {
            continue;
        }
        let mut line = wording(victim.base(), [27, 23, 13], victim, *damage, &text);
        line.extend(status_list(victim, *flags, &text));
        out.push(finish(line));
    }
    out
}

/// Line 3's text before its tail: torso (location 3, uncalled included)
/// or a named location, critical or normal, by damage 0, 1 or other.
fn struck_line(struck: &Named, attack: &AttackLines, text: &dyn Fn(i32) -> Vec<u8>) -> Vec<u8> {
    let base = struck.base();
    let place = if attack.location == location::UNCALLED {
        location::TORSO
    } else {
        attack.location
    };
    if place == location::TORSO {
        let offsets = if attack.critical {
            [28, 24, 20]
        } else {
            [27, 23, 13]
        };
        return wording(base, offsets, struck, attack.damage, text);
    }
    // A missing location name leaves the line empty before its tail.
    let Some(name) = struck
        .alias
        .and_then(|alias| alias.checked_mul(10))
        .and_then(|row| row.checked_add(LOCATION_BASE + i32::from(place)))
        .map(text)
        .filter(|name| !name.is_empty())
    else {
        return Vec::new();
    };
    let offset = match (attack.critical, attack.damage) {
        (true, 0) => 25,
        (true, 1) => 21,
        (true, _) => 11,
        (false, 0) => 26,
        (false, 1) => 22,
        (false, _) => 12,
    };
    let damage = i64::from(attack.damage);
    format_message(
        &text(base + offset),
        &[Arg::Text(&struck.name), Arg::Text(&name), Arg::Int(damage)],
    )
}

/// A name-only wording by damage: `offsets` for 0, 1 and any other value
/// (negative included), which is formatted with `%d`.
fn wording(
    base: i32,
    offsets: [i32; 3],
    critter: &Named,
    damage: i32,
    text: &dyn Fn(i32) -> Vec<u8>,
) -> Vec<u8> {
    let offset = match damage {
        0 => offsets[0],
        1 => offsets[1],
        _ => offsets[2],
    };
    format_message(
        &text(base + offset),
        &[Arg::Text(&critter.name), Arg::Int(i64::from(damage))],
    )
}

/// The status list of a critter's damage flags: nothing for none; dead
/// alone as " and " + S + 7; otherwise every other set bit but hit and
/// critical, from bit 0, each but the last as ", " + S + bit and the last
/// as " and " + S + bit. The reference looks the last one up at the bare
/// bit number, which `combat.msg` lacks; the `[May-improve]` safe rule
/// used here gives it S + bit like the others.
pub fn status_list(critter: &Named, flags: u32, text: &dyn Fn(i32) -> Vec<u8>) -> Vec<u8> {
    let base = critter.status_base();
    let flags = flags & !(effect::HIT | effect::CRITICAL);
    let mut out = Vec::new();
    if flags & effect::DEAD != 0 {
        out.extend(text(AND));
        out.extend(text(base + 7));
        return out;
    }
    let bits: Vec<i32> = (0..32).filter(|bit| flags & (1 << bit) != 0).collect();
    for (index, bit) in bits.iter().enumerate() {
        if index + 1 == bits.len() {
            out.extend(text(AND));
        } else {
            out.extend_from_slice(b", ");
        }
        out.extend(text(base + bit));
    }
    out
}
