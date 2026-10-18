//! SPEC-007 §2.4-2.5 combat encodings and built-in critical lookup data.
//! The matrices are compiled from the approved specification by `build.rs`.

/// One critical-success entry. Only `flags` and `failed_flags` were written
/// in hexadecimal in the specification; all other values are decimal.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CriticalEffect {
    /// Damage scale numerator; the damage formula divides it by 2.
    pub multiplier: i32,
    pub flags: u32,
    /// Saving stat number (0 Strength, 2 Endurance, 5 Agility, 6 Luck), or -1.
    pub save_stat: i32,
    pub save_adjustment: i32,
    /// Extra effect mask applied when the D10 save fails.
    pub failed_flags: u32,
    pub message: i32,
    pub failed_message: i32,
}

include!(concat!(env!("OUT_DIR"), "/critical_tables.rs"));

/// Index of the player matrix, selected irrespective of gender.
pub const PLAYER_MATRIX: usize = 15;

/// Hit modes: which hand/attack, or reload.
pub mod hit_mode {
    pub const LEFT_PRIMARY: u8 = 0;
    pub const LEFT_SECONDARY: u8 = 1;
    pub const RIGHT_PRIMARY: u8 = 2;
    pub const RIGHT_SECONDARY: u8 = 3;
    pub const PUNCH: u8 = 4;
    pub const KICK: u8 = 5;
    pub const LEFT_RELOAD: u8 = 6;
    pub const RIGHT_RELOAD: u8 = 7;
}

/// Hit locations 0-8: head, left/right arm, torso, right/left leg, eyes,
/// groin, uncalled.
pub mod location {
    pub const HEAD: u8 = 0;
    pub const LEFT_ARM: u8 = 1;
    pub const RIGHT_ARM: u8 = 2;
    pub const TORSO: u8 = 3;
    pub const RIGHT_LEG: u8 = 4;
    pub const LEFT_LEG: u8 = 5;
    pub const EYES: u8 = 6;
    pub const GROIN: u8 = 7;
    pub const UNCALLED: u8 = 8;
}

/// Ranged/throw accuracy adjustment by location.
pub const LOCATION_ADJUSTMENT: [i32; 9] = [-40, -30, -30, 0, -20, -20, -60, -30, 0];

/// Accuracy adjustment: the full value for ranged/throw, half (truncating
/// toward zero) for unarmed/melee. `None` outside locations 0-8.
pub fn location_adjustment(location: u8, melee: bool) -> Option<i32> {
    let full = *LOCATION_ADJUSTMENT.get(usize::from(location))?;
    Some(if melee { full / 2 } else { full })
}

/// Attack/critter effect flags. These are not basic object flags.
pub mod effect {
    pub const KNOCKED_OUT: u32 = 0x1;
    pub const KNOCKED_DOWN: u32 = 0x2;
    pub const CRIPPLED_LEFT_LEG: u32 = 0x4;
    pub const CRIPPLED_RIGHT_LEG: u32 = 0x8;
    pub const CRIPPLED_LEFT_ARM: u32 = 0x10;
    pub const CRIPPLED_RIGHT_ARM: u32 = 0x20;
    pub const BLIND: u32 = 0x40;
    pub const DEAD: u32 = 0x80;
    pub const HIT: u32 = 0x100;
    pub const CRITICAL: u32 = 0x200;
    pub const ON_FIRE: u32 = 0x400;
    pub const BYPASS: u32 = 0x800;
    pub const EXPLODE: u32 = 0x1000;
    pub const DESTROY: u32 = 0x2000;
    pub const DROP: u32 = 0x4000;
    pub const LOSE_TURN: u32 = 0x8000;
    pub const HIT_SELF: u32 = 0x10000;
    pub const LOSE_AMMO: u32 = 0x20000;
    pub const DUD: u32 = 0x40000;
    pub const HURT_SELF: u32 = 0x80000;
    pub const RANDOM_HIT: u32 = 0x100000;
    pub const CRIPPLED_RANDOM: u32 = 0x200000;
    pub const BACKWASH: u32 = 0x400000;
    pub const REVERSE: u32 = 0x800000;
    /// Bits retained as persistent critter status.
    pub const PERSISTENT: u32 = 0x80ff;
    pub const CRIPPLED_LEGS: u32 = CRIPPLED_LEFT_LEG | CRIPPLED_RIGHT_LEG;
    pub const CRIPPLED_ARMS: u32 = CRIPPLED_LEFT_ARM | CRIPPLED_RIGHT_ARM;
    /// The four limbs a random-cripple substitution chooses among, uniformly.
    pub const RANDOM_LIMBS: [u32; 4] = [
        CRIPPLED_LEFT_LEG,
        CRIPPLED_RIGHT_LEG,
        CRIPPLED_LEFT_ARM,
        CRIPPLED_RIGHT_ARM,
    ];
}

/// Combat maneuver flags, distinct from effect flags.
pub mod maneuver {
    pub const ALERTED: u32 = 0x1;
    pub const STOP_ATTACKING: u32 = 0x2;
    pub const FLEEING: u32 = 0x4;
}

/// Combat state bits.
pub mod state {
    pub const ACTIVE: u32 = 0x1;
    pub const INPUT_GATE: u32 = 0x2;
    pub const END_REQUESTED: u32 = 0x8;
}

/// Critical-success tier from `D100 + effective Better Criticals`.
pub fn success_tier(score: i32) -> usize {
    match score {
        ..=20 => 0,
        21..=45 => 1,
        46..=70 => 2,
        71..=90 => 3,
        91..=100 => 4,
        _ => 5,
    }
}

/// Critical-failure column from `D100 - 5 * (effective Luck - 5)`.
pub fn failure_column(score: i32) -> usize {
    match score {
        ..=20 => 0,
        21..=50 => 1,
        51..=75 => 2,
        76..=95 => 3,
        _ => 4,
    }
}

/// Critical-failure mask for a column 0-4. Under the SPEC-007 §2.4 safe
/// rule a table number outside 0-6 uses table 0.
pub fn failure_mask(table: i32, column: usize) -> Option<u32> {
    let row = usize::try_from(table)
        .ok()
        .and_then(|table| CRITICAL_FAILURE.get(table))
        .unwrap_or(&CRITICAL_FAILURE[0]);
    row.get(column).copied()
}

/// Critical-success effect for a victim matrix (0-14, or [`PLAYER_MATRIX`]),
/// location 0-8, and tier 0-5.
pub fn success_effect(matrix: usize, location: u8, tier: usize) -> Option<CriticalEffect> {
    CRITICAL_SUCCESS
        .get(matrix)?
        .get(usize::from(location))?
        .get(tier)
        .copied()
}
