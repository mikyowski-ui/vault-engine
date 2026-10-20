//! SPEC-013 rules independent of world state: per-skill constants, the
//! skill gate, experience, the daily usage slots, the steal check and steal
//! experience, the radiation tables, poison timing, explosive timers and
//! damage, the elevator table and window arithmetic, and the Skilldex,
//! Set Timer, move-items counter, loot and barter window layouts.
use crate::{RandomSource, WorldError, rng};

/// SPEC-006 §3.3 skill indices used here.
pub mod skill {
    pub const SMALL_GUNS: i32 = 0;
    pub const FIRST_AID: i32 = 6;
    pub const DOCTOR: i32 = 7;
    pub const SNEAK: i32 = 8;
    pub const LOCKPICK: i32 = 9;
    pub const STEAL: i32 = 10;
    pub const TRAPS: i32 = 11;
    pub const SCIENCE: i32 = 12;
    pub const REPAIR: i32 = 13;
    pub const OUTDOORSMAN: i32 = 17;
}

/// §2.2 base experience per success and the difficulty-bonus flag.
pub fn base_experience(index: i32) -> (i32, bool) {
    match index {
        skill::FIRST_AID | skill::LOCKPICK | skill::STEAL | skill::TRAPS => {
            (25, index != skill::FIRST_AID)
        }
        skill::DOCTOR => (50, false),
        skill::OUTDOORSMAN => (100, false),
        _ => (0, false),
    }
}
/// §3.5 steps 1-4: the experience a successful use adds (before Swift
/// Learner and the cap), or 0 for a non-player, no count, or base 0.
pub fn skill_experience(index: i32, count: i32, kit_bonus: i32, player: bool) -> i32 {
    let (mut base, bonus) = base_experience(index);
    if !player || count <= 0 || base == 0 {
        return 0;
    }
    if bonus && kit_bonus < 0 {
        base += kit_bonus.saturating_abs();
    }
    count.saturating_mul(base)
}

/// What a skill's target is, for the §3.2 gate.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SkillTarget {
    pub category: vault_formats::pro::Category,
    /// The critter kill type (SPEC-003), for Science and Repair.
    pub kill_type: Option<i32>,
    pub pid: u32,
    pub is_actor: bool,
}
/// The §3.2 gate result.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Gate {
    Accepted,
    /// Rejected; `combat` shows `proto.msg` 902 when the actor is the player.
    Rejected {
        combat: bool,
    },
}
/// Scenery a Steal may target.
pub const STEAL_SCENERY: u32 = 0x0200_0384;
/// §3.2 gating per skill, in order. Sneak is handled before the gate.
pub fn gate(index: i32, combat: bool, target: &SkillTarget) -> Gate {
    use vault_formats::pro::Category::{Critter, Item, Scenery};
    let in_combat = Gate::Rejected { combat: true };
    let rejected = Gate::Rejected { combat: false };
    let refused = match index {
        skill::FIRST_AID | skill::DOCTOR => {
            if combat {
                return in_combat;
            }
            target.category != Critter
        }
        skill::LOCKPICK => {
            if combat {
                return in_combat;
            }
            !matches!(target.category, Item | Scenery)
        }
        skill::STEAL => {
            if combat {
                return in_combat;
            }
            (!matches!(target.category, Item | Critter) && target.pid != STEAL_SCENERY)
                || target.is_actor
        }
        skill::TRAPS => {
            if combat {
                return in_combat;
            }
            target.category == Critter
        }
        skill::SCIENCE | skill::REPAIR => {
            if combat {
                return in_combat;
            }
            let allowed: &[i32] = if index == skill::SCIENCE {
                &[10, 5]
            } else {
                &[10]
            };
            target.category == Critter && !target.kill_type.is_some_and(|k| allowed.contains(&k))
        }
        _ => return rejected,
    };
    if refused { rejected } else { Gate::Accepted }
}

/// §3.6 availability over three timestamp slots: the first empty slot, or
/// slot 2 once `trunc((now - slot 0) / 36000)` exceeds 24 hours.
pub fn usage_slot(slots: &[i32; 3], now: u64) -> Option<usize> {
    if let Some(empty) = slots.iter().position(|slot| *slot == 0) {
        return Some(empty);
    }
    let hours = (now as i64 - i64::from(slots[0])) / 36000;
    (hours > 24).then_some(2)
}
/// §3.6 recording on a successful heal; unavailable records nothing.
pub fn record_use(slots: &mut [i32; 3], now: u64) -> bool {
    let Some(slot) = usage_slot(slots, now) else {
        return false;
    };
    let now = now.min(i32::MAX as u64) as i32;
    if slots[slot] == 0 {
        slots[slot] = now;
    } else {
        slots.copy_within(1.., 0);
        slots[2] = now;
    }
    true
}

/// §3.9.3 "in front of": the stored facings differ by 2, 3 or 4.
pub fn in_front(thief_facing: u8, victim_facing: u8) -> bool {
    matches!(thief_facing.abs_diff(victim_facing), 2..=4)
}
/// The inputs of one §3.9.3 steal check.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct StealInputs {
    /// N, already including the current attempt.
    pub attempts: i32,
    /// One unit's prototype size.
    pub item_size: i32,
    pub thief_steal: i32,
    pub thief_critical: i32,
    /// The thief is the player and has Pickpocket.
    pub pickpocket: bool,
    /// The victim pane's owner is a critter (by art category).
    pub victim_critter: bool,
    pub in_front: bool,
    /// Knocked out (0x01) or knocked down (0x02).
    pub victim_prone: bool,
    /// The thief is the player and the victim a party member.
    pub party_member: bool,
    /// The victim's effective Steal (for a critter prototype).
    pub victim_steal: Option<i32>,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct StealResult {
    pub roll: i32,
    pub caught: bool,
}
/// M = 1 - N, less `4 × size` and 25 in front (unless Pickpocket), plus 20
/// for a prone victim.
pub fn steal_modifier(inputs: &StealInputs) -> i32 {
    let mut modifier = 1 - inputs.attempts;
    if !inputs.pickpocket {
        modifier -= 4 * inputs.item_size;
        if inputs.victim_critter && inputs.in_front {
            modifier -= 25;
        }
    }
    if inputs.victim_prone {
        modifier += 20;
    }
    modifier
}
/// §3.9.3 the steal roll, then (unless critical) the catch roll; caught
/// exactly when the catch result is 2 or 3.
pub fn steal_check(
    inputs: &StealInputs,
    ticks: u64,
    rng: &mut impl RandomSource,
) -> Result<StealResult, WorldError> {
    if inputs.party_member {
        return Ok(StealResult {
            roll: 3,
            caught: false,
        });
    }
    let modifier = steal_modifier(inputs);
    let chance = (modifier + inputs.thief_steal).min(95);
    let roll = rng::skill_check(chance, 0, inputs.thief_critical, ticks, rng)?.code;
    let caught = match roll {
        3 => false,
        0 => true,
        _ => {
            let catch = inputs
                .victim_steal
                .map_or(30 - modifier, |steal| steal - modifier);
            rng::skill_check(catch, 0, 0, ticks, rng)?.code >= 2
        }
    };
    Ok(StealResult { roll, caught })
}
/// §3.9.5 pending steal experience: each success adds B, then B grows by
/// 10, so n successes give `5 n (n + 1)`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct StealExperience {
    pub pending: i32,
    increment: i32,
}
impl Default for StealExperience {
    fn default() -> Self {
        Self {
            pending: 0,
            increment: 10,
        }
    }
}
impl StealExperience {
    pub fn success(&mut self) {
        self.pending = self.pending.saturating_add(self.increment);
        self.increment = self.increment.saturating_add(10);
    }
    /// The award `min(300 - Steal, P)`. Safe rule: none when not positive.
    pub fn award(&self, thief_steal: i32) -> Option<i32> {
        let award = (300 - thief_steal).min(self.pending);
        (award > 0).then_some(award)
    }
}

/// §2.6 base radiation level and its Endurance check modifier.
pub fn radiation_level(total: i32) -> (i32, i32) {
    match total {
        ..=99 => (0, 2),
        100..=199 => (1, 0),
        200..=399 => (2, -2),
        400..=599 => (3, -4),
        600..=999 => (4, -6),
        _ => (5, -8),
    }
}
/// §2.6 per-level penalties: ST, PE, EN, CH, IN, AG, current HP, healing
/// rate. Level L's stage message is `misc.msg` 1000 + L - 1.
pub const RADIATION_PENALTIES: [[i32; 8]; 6] = [
    [0, 0, 0, 0, 0, 0, 0, 0],
    [-1, 0, 0, 0, 0, 0, 0, 0],
    [-1, 0, 0, 0, 0, -1, 0, -3],
    [-2, 0, -1, 0, 0, -2, -5, -5],
    [-4, -3, -3, -3, -1, -5, -15, -10],
    [-6, -5, -5, -5, -3, -6, -20, -10],
];
/// The stats the penalty columns change (current HP has no stat bonus).
pub const RADIATION_STATS: [Option<usize>; 8] = [
    Some(0),
    Some(1),
    Some(2),
    Some(3),
    Some(4),
    Some(5),
    None,
    Some(14),
];
/// Recovery follows onset after 7 days.
pub const RADIATION_RECOVERY_TICKS: u64 = 6_048_000;
/// §3.19 the next poison tick: `10 × (505 - 5 × level)` ticks (zero or
/// negative is due at once).
pub fn poison_delay(level: i32) -> i64 {
    10 * (505 - 5 * i64::from(level))
}

/// §3.10 explosives: Dynamite (51, armed 206) and Plastic Explosives (85,
/// armed 209).
pub const DYNAMITE: u32 = 51;
pub const PLASTIC: u32 = 85;
pub const DYNAMITE_ARMED: u32 = 206;
pub const PLASTIC_ARMED: u32 = 209;
pub fn armed_pid(pid: u32) -> Option<u32> {
    match pid {
        DYNAMITE => Some(DYNAMITE_ARMED),
        PLASTIC => Some(PLASTIC_ARMED),
        _ => None,
    }
}
pub fn is_explosive(pid: u32) -> bool {
    matches!(pid, DYNAMITE | PLASTIC | DYNAMITE_ARMED | PLASTIC_ARMED)
}

/// §3.10 explosion art: miscellaneous art identifiers (`0x05000000` plus
/// the `misc.lst` index).
pub mod explosion_art {
    /// `roktxpd.frm` (10): timed and scripted explosions, and weapons with
    /// explosion damage that are not thrown (the Rocket Launcher).
    pub const ROCKET: u32 = 0x0500_000a;
    /// `expa.frm` (29): thrown explosion damage (Frag Grenade, Molotov).
    pub const GRENADE: u32 = 0x0500_001d;
    /// `expp.frm` (31): thrown plasma damage (Plasma Grenade).
    pub const PLASMA: u32 = 0x0500_001f;
    /// `empxpld.frm` (2): thrown EMP damage (Pulse Grenade).
    pub const EMP: u32 = 0x0500_0002;
}
/// The Plasma Grenade, whose direction-0 art waits 4 frame steps.
pub const PLASMA_GRENADE: u32 = 26;
/// §3.10 whether a weapon explodes, by its prototype's damage type (not the
/// ammunition's): thrown with explosion, plasma or EMP damage, or used
/// without throwing with explosion damage. Returns the explosion art and
/// direction 0's adjacent-art start delay, 2 frame steps (4 for the Plasma
/// Grenade).
pub fn weapon_explosion(pid: u32, damage_type: i32, thrown: bool) -> Option<(u32, u32)> {
    use crate::combat::damage_type::{EMP, EXPLOSION, PLASMA};
    let art = match (thrown, damage_type) {
        (true, EXPLOSION) => explosion_art::GRENADE,
        (true, PLASMA) => explosion_art::PLASMA,
        (true, EMP) => explosion_art::EMP,
        (false, EXPLOSION) => explosion_art::ROCKET,
        _ => return None,
    };
    Some((art, if pid == PLASMA_GRENADE { 4 } else { 2 }))
}

/// §3.25 the skill books: PID, skill and the `proto.msg` result message.
/// No other PID is a skill book.
pub const SKILL_BOOKS: [(u32, i32, i32); 5] = [
    (73, skill::SCIENCE, 802),
    (76, skill::REPAIR, 803),
    (80, skill::FIRST_AID, 804),
    (86, skill::OUTDOORSMAN, 806),
    (102, skill::SMALL_GUNS, 805),
];
/// §3.25 the skill and result message of a book PID.
pub fn skill_book(pid: u32) -> Option<(i32, i32)> {
    SKILL_BOOKS
        .iter()
        .find(|(book, _, _)| *book == pid)
        .map(|(_, skill, message)| (*skill, *message))
}
/// §3.25 the skill points a book adds at effective skill `percent`:
/// `trunc((100 - S) / 10)`, truncating toward zero; 0 or less adds none.
pub fn book_points(percent: i32) -> i32 {
    (100 - percent) / 10
}
/// §3.25 the reading time: `36000 × (11 - IN)` ticks, 1 to 10 game hours
/// (effective Intelligence clamped to 1-10).
pub fn book_ticks(intelligence: i32) -> u64 {
    36_000 * (11 - intelligence.clamp(1, 10)) as u64
}
/// Timed-queue types for a timely and a premature explosion.
pub const EXPLOSION_EVENT: u32 = 8;
pub const PREMATURE_EVENT: u32 = 11;
pub const SNEAK_EVENT: u32 = 10;
/// The Traps check result's event type and delay for a timer of S seconds.
pub fn explosive_timer(result: i32, seconds: i32) -> (u32, u64) {
    match result {
        0 => (PREMATURE_EVENT, 0),
        1 => (PREMATURE_EVENT, (10 * seconds / 2).max(0) as u64),
        _ => (EXPLOSION_EVENT, (10 * seconds).max(0) as u64),
    }
}
pub fn explosion_range(pid: u32) -> (i32, i32) {
    if matches!(pid, DYNAMITE | DYNAMITE_ARMED) {
        (30, 50)
    } else {
        (40, 80)
    }
}
/// Center-hex damage `v = draw - DT`, less `trunc(v × DR / 100)` when
/// positive, at least 0; knockback `trunc(v / 10)` for a single-hex victim.
pub fn explosion_damage(draw: i32, threshold: i32, resistance: i32, multihex: bool) -> (i32, i32) {
    let mut value = draw - threshold;
    if value > 0 {
        value -= value * resistance / 100;
    }
    let value = value.max(0);
    (value, if multihex { 0 } else { value / 10 })
}
/// A re-queued explosive waits 50 ticks while another explosion animates.
pub const EXPLOSION_RETRY_TICKS: u64 = 50;

/// §3.15 Set Timer: 60 seconds to start, steps of 10 within 10-180.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SetTimer {
    pub seconds: i32,
}
impl Default for SetTimer {
    fn default() -> Self {
        Self { seconds: 60 }
    }
}
impl SetTimer {
    pub fn plus(&mut self) {
        if self.seconds < 180 {
            self.seconds += 10;
        }
    }
    pub fn minus(&mut self) {
        if self.seconds > 10 {
            self.seconds -= 10;
        }
    }
    /// Enter accepts a multiple of 10 from 10 to 180.
    pub fn accepts(&self) -> bool {
        (10..=180).contains(&self.seconds) && self.seconds % 10 == 0
    }
    /// The `bignum.frm` digits: minutes at x = 133, tens of seconds at 161,
    /// ones at 175 (y = 64).
    pub fn digits(&self) -> [(i32, u8); 3] {
        let s = self.seconds;
        [
            (133, (s / 60) as u8),
            (161, ((s % 60) / 10) as u8),
            (175, (s % 10) as u8),
        ]
    }
}

/// §3.17 the move-items counter.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Counter {
    pub value: i32,
    pub max: i32,
    fresh: bool,
}
pub const COUNTER_MIN: i32 = 1;
impl Counter {
    /// Starting at 1, up to the quantity capped at 999.
    pub fn new(quantity: i32) -> Self {
        Self {
            value: 1,
            max: quantity.clamp(COUNTER_MIN, 999),
            fresh: true,
        }
    }
    pub fn plus(&mut self) {
        if self.value < self.max {
            self.value += 1;
        }
        self.fresh = true;
    }
    /// Minus works above the minimum even past a typed maximum.
    pub fn minus(&mut self) {
        if self.value > COUNTER_MIN {
            self.value -= 1;
        }
        self.fresh = true;
    }
    pub fn all(&mut self) {
        self.value = self.max;
        self.fresh = true;
    }
    /// The first digit after another action replaces the value; later ones
    /// give `(10 × value) mod 1000 + digit`.
    pub fn digit(&mut self, digit: u8) {
        let digit = i32::from(digit.min(9));
        self.value = if self.fresh {
            digit
        } else {
            (10 * self.value) % 1000 + digit
        };
        self.fresh = false;
    }
    /// Backspace starts fresh (from 0) after another action, then
    /// `trunc(value / 10)`.
    pub fn backspace(&mut self) {
        if std::mem::take(&mut self.fresh) {
            self.value = 0;
        }
        self.value /= 10;
    }
    /// Enter accepts the minimum through the maximum (`ib1p1xx1`); any
    /// other value plays `iisxxxx1` and stays open.
    pub fn enter(&self) -> Option<i32> {
        (COUNTER_MIN..=self.max)
            .contains(&self.value)
            .then_some(self.value)
    }
    /// The three leading-zero digits.
    pub fn digits(&self) -> [u8; 3] {
        let v = self.value.clamp(0, 999);
        [(v / 100) as u8, (v / 10 % 10) as u8, (v % 10) as u8]
    }
}
/// Counter layout (window-relative; the window is at screen (140, 80)).
pub const COUNTER_ORIGIN: [i32; 2] = [140, 80];
pub const COUNTER_DIGITS: [[i32; 2]; 3] = [[153, 45], [167, 45], [181, 45]];

/// §3.15 Skilldex: window, rows and layout.
pub const SKILLDEX_ORIGIN: [i32; 2] = [451, 5];
pub const SKILLDEX_SKILLS: [i32; 8] = [
    skill::SNEAK,
    skill::LOCKPICK,
    skill::STEAL,
    skill::TRAPS,
    skill::FIRST_AID,
    skill::DOCTOR,
    skill::SCIENCE,
    skill::REPAIR,
];
pub fn skilldex_button(row: usize) -> [i32; 2] {
    [15, 45 + 36 * row as i32]
}
/// Label placement inside the 86-by-33 button image.
pub fn skilldex_label(width: i32, font_height: i32) -> [i32; 2] {
    [((86 - width) / 2 + 1).max(0), (33 - font_height) / 2 + 1]
}
/// Counter digits (hundreds, tens, ones) at x = 110, 124, 138; safe rule:
/// the value is clamped to 0-999.
pub fn skilldex_digits(value: i32) -> [u8; 3] {
    let v = value.clamp(0, 999);
    [(v / 100) as u8, (v / 10 % 10) as u8, (v % 10) as u8]
}
pub fn skilldex_counter_y(row: usize) -> i32 {
    48 + 36 * row as i32
}
/// A chosen row closes the window after `trunc(1000 / 9)` ms.
pub const SKILLDEX_CLOSE_MS: u64 = 1000 / 9;
/// Hotkeys `1`-`8` (or the shifted characters) in Skilldex row order.
pub fn skill_hotkey(row: u8) -> Option<i32> {
    SKILLDEX_SKILLS.get(usize::from(row)).copied()
}

/// §3.16 loot window layout (window at screen (80, 0)).
pub const LOOT_ORIGIN: [i32; 2] = [80, 0];
pub fn loot_player_slot(row: usize) -> [i32; 2] {
    [44, 35 + 48 * row as i32]
}
pub fn loot_target_slot(row: usize) -> [i32; 2] {
    [422, 35 + 48 * row as i32]
}
pub fn loot_player_icon(row: usize) -> [i32; 2] {
    [48, 39 + 48 * row as i32]
}
pub fn loot_target_icon(row: usize) -> [i32; 2] {
    [426, 41 + 48 * row as i32]
}
pub const LOOT_ROWS: usize = 6;
/// Screen drop zones: the target pane and the player pane.
pub fn loot_drop_target(point: [i32; 2]) -> Option<bool> {
    let [x, y] = point;
    if !(35..=323).contains(&y) {
        return None;
    }
    if (502..=566).contains(&x) {
        Some(true)
    } else if (124..=188).contains(&x) {
        Some(false)
    } else {
        None
    }
}
/// The quantity label: `x` and the quantity above 1 (capped at 99999);
/// ammunition counts rounds `pack × (quantity - 1) + front`.
pub fn quantity_label(quantity: i32, ammo: Option<(i32, i32)>) -> Option<Vec<u8>> {
    let count = match ammo {
        Some((pack, front)) => pack * (quantity - 1) + front,
        None if quantity > 1 => quantity,
        None => return None,
    };
    Some(format!("x{}", count.min(99_999)).into_bytes())
}
/// The arrow-cursor action menu for an item in the loot or barter window.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ItemEntry {
    Look,
    Unload,
    Use,
    Drop,
    Cancel,
}
pub fn item_menu(owned_by_player: bool, unloadable: bool, container: bool) -> Vec<ItemEntry> {
    let mut entries = vec![ItemEntry::Look];
    if unloadable {
        entries.push(ItemEntry::Unload);
    } else if container {
        entries.push(ItemEntry::Use);
    }
    if owned_by_player {
        entries.push(ItemEntry::Drop);
    }
    entries.push(ItemEntry::Cancel);
    entries
}
/// Opened containers nest at most nine levels deep.
pub const MAX_NESTING: usize = 9;

/// §3.18 barter drop routes by screen x (all spanning y 310-454).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BarterRoute {
    PlayerToTable,
    MerchantToTable,
    TableToPlayer,
    TableToMerchant,
}
impl BarterRoute {
    pub fn zone(self) -> std::ops::RangeInclusive<i32> {
        match self {
            Self::PlayerToTable => 245..=309,
            Self::MerchantToTable => 330..=394,
            Self::TableToPlayer => 80..=144,
            Self::TableToMerchant => 475..=539,
        }
    }
    /// `inventry.msg` failure message.
    pub fn failure(self) -> i32 {
        match self {
            Self::PlayerToTable | Self::TableToPlayer => 26,
            Self::MerchantToTable | Self::TableToMerchant => 25,
        }
    }
    pub fn accepts(self, point: [i32; 2]) -> bool {
        (310..=454).contains(&point[1]) && self.zone().contains(&point[0])
    }
}
/// The merchant prototype flag that permits bartering.
pub const BARTER_FLAG: u32 = 0x02;

/// §2.7 an elevator: background, optional panel, key characters and
/// destinations (map, elevation, hex) in button order.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Elevator {
    pub background: &'static str,
    pub panel: Option<&'static str>,
    pub keys: &'static [u8],
    pub destinations: &'static [(i32, u8, i32)],
}
const V9: &[(i32, u8, i32)] = &[(9, 0, 13704), (9, 1, 23302), (9, 2, 17308)];
pub const ELEVATORS: [Elevator; 12] = [
    Elevator {
        background: "el_bos.frm",
        panel: None,
        keys: b"1234",
        destinations: &[
            (14, 0, 18940),
            (14, 1, 18936),
            (15, 0, 21340),
            (15, 1, 21340),
        ],
    },
    Elevator {
        background: "el_bos.frm",
        panel: Some("el_bos2.frm"),
        keys: b"G1",
        destinations: &[(13, 0, 20502), (14, 0, 14912)],
    },
    Elevator {
        background: "el_mast1.frm",
        panel: None,
        keys: b"123",
        destinations: &[(33, 0, 12498), (33, 1, 20094), (34, 0, 17312)],
    },
    Elevator {
        background: "el_mast1.frm",
        panel: Some("el_mast2.frm"),
        keys: b"34",
        destinations: &[(34, 0, 16140), (34, 1, 16140)],
    },
    Elevator {
        background: "el_mil1.frm",
        panel: None,
        keys: b"123",
        destinations: &[(31, 0, 14920), (31, 1, 14920), (32, 0, 12944)],
    },
    Elevator {
        background: "el_mil1.frm",
        panel: Some("el_mil2.frm"),
        keys: b"34",
        destinations: &[(32, 0, 24520), (32, 1, 24520)],
    },
    Elevator {
        background: "el_mil1.frm",
        panel: None,
        keys: b"123",
        destinations: &[(42, 0, 22526), (42, 1, 22526), (42, 2, 22526)],
    },
    Elevator {
        background: "el_mil1.frm",
        panel: Some("el_mil3.frm"),
        keys: b"346",
        destinations: &[(42, 2, 14086), (43, 0, 14086), (43, 2, 14086)],
    },
    Elevator {
        background: "el_vault.frm",
        panel: None,
        keys: b"123",
        destinations: &[(6, 0, 14104), (6, 1, 22504), (6, 2, 17312)],
    },
    Elevator {
        background: "el_mil1.frm",
        panel: None,
        keys: b"123",
        destinations: V9,
    },
    Elevator {
        background: "el_mil1.frm",
        panel: None,
        keys: b"123",
        destinations: V9,
    },
    Elevator {
        background: "el_mil1.frm",
        panel: Some("el_mil4.frm"),
        keys: b"456",
        destinations: &[(43, 0, 14130), (43, 1, 14130), (43, 2, 14130)],
    },
];
pub fn elevator(kind: i32) -> Option<&'static Elevator> {
    usize::try_from(kind)
        .ok()
        .and_then(|kind| ELEVATORS.get(kind))
}
/// Window placement for a `w` by `h` background.
pub fn elevator_origin(size: [i32; 2]) -> [i32; 2] {
    [(640 - size[0]) / 2, (480 - 100 - 1 - size[1]) / 2]
}
pub fn elevator_button(i: usize) -> [i32; 2] {
    [13, 40 + 60 * i as i32]
}
pub const GAUGE: (&str, [i32; 2], [i32; 2]) = ("gaj000.frm", [121, 41], [92, 55]);
impl Elevator {
    pub fn buttons(&self) -> usize {
        self.destinations.len()
    }
    /// A key selects the button with that character (case-sensitive in the
    /// reference; safe rule: letters match either case).
    pub fn button_for_key(&self, key: u8) -> Option<usize> {
        self.keys.iter().position(|k| k.eq_ignore_ascii_case(&key))
    }
    /// Safe-rule start level: the entry matching the current map and the
    /// player's elevation, else 0.
    pub fn start_level(&self, map: i32, elevation: u8) -> usize {
        self.destinations
            .iter()
            .position(|(m, e, _)| *m == map && *e == elevation)
            .unwrap_or(0)
    }
    /// Gauge cell `trunc(level × 12 / (n - 1))`.
    pub fn gauge_cell(&self, level: usize) -> i32 {
        (level as i32 * 12) / (self.buttons() as i32 - 1).max(1)
    }
    /// The travel sound for `levels` levels travelled.
    pub fn travel_sound(&self, levels: usize) -> Option<&'static str> {
        let table: &[&str] = match self.buttons() {
            2 => &["elv1_1"],
            3 => &["elv1_2", "elv1_1"],
            4 => &["elv1_3", "elv2_3", "elv1_1"],
            _ => &[],
        };
        table.get(levels.checked_sub(1)?).copied()
    }
    /// Busy-wait per gauge step: 23, 46 or 69 ms for 2, 3 or 4 buttons.
    pub fn step_ms(&self) -> u64 {
        ((self.buttons() as f32 - 1.0) / 12.0 * 276.923_07) as u64
    }
    /// The gauge cells drawn moving from `from` to `to`: steps of
    /// `(n - 1) / 12` cell units; safe rule: the exact target cell ends it.
    pub fn gauge_path(&self, from: usize, to: usize) -> Vec<i32> {
        let step = (self.buttons() as f32 - 1.0) / 12.0;
        let target = self.gauge_cell(to) as f32;
        let mut position = self.gauge_cell(from) as f32;
        let mut cells = Vec::new();
        if from == to {
            return cells;
        }
        let down = target < position;
        loop {
            position += if down { -step } else { step };
            if (down && position <= target) || (!down && position >= target) {
                break;
            }
            cells.push(position as i32);
        }
        cells.push(target as i32);
        cells
    }
}
/// The final pause after travel.
pub const ELEVATOR_PAUSE_MS: u64 = 200;
/// Scenery doors closed by an elevator move within 4 hexes.
pub const ELEVATOR_DOORS: [u32; 3] = [0x0200_0099, 0x0200_01A5, 0x0200_01D6];
/// Exit grids are miscellaneous PIDs `0x05000010`-`0x05000017`.
pub fn is_exit_grid(pid: u32) -> bool {
    (0x0500_0010..=0x0500_0017).contains(&pid)
}
