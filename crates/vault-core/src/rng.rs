use crate::WorldError;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct RngState {
    pub recurrence: i32,
    pub shuffle: [i32; 32],
    pub selector: i32,
}
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ParkMiller {
    state: RngState,
}
pub trait RandomSource {
    fn range(&mut self, first: i32, last: i32) -> Result<i32, WorldError>;
}
impl ParkMiller {
    pub fn seeded(seed: i32) -> Result<Self, WorldError> {
        let mut rng = Self::default();
        rng.seed(seed, None)?;
        Ok(rng)
    }
    pub fn from_state(state: RngState) -> Result<Self, WorldError> {
        if state.recurrence < 0 || state.selector < 0 || state.shuffle.iter().any(|v| *v < 0) {
            return Err(WorldError::Invalid("negative RNG state".into()));
        }
        Ok(Self { state })
    }
    pub fn state(&self) -> &RngState {
        &self.state
    }
    fn recurrence(n: i32) -> i64 {
        16807 * i64::from(n % 127773) - 2836 * i64::from(n / 127773)
    }
    /// Seeding retains shuffle[0]. Seed -1 needs two explicit host random draws.
    pub fn seed(&mut self, seed: i32, ambient: Option<[u32; 2]>) -> Result<(), WorldError> {
        let seed = if seed == -1 {
            let [a, b] = ambient
                .ok_or_else(|| WorldError::Invalid("ambient seed requires host entropy".into()))?;
            (a.wrapping_mul(65536).wrapping_add(b) & 0x7fffffff) as i32
        } else {
            seed
        };
        let mut n = seed.max(1);
        for index in (1..=40).rev() {
            let q = Self::recurrence(n);
            n = if q < 0 {
                (q + 2147483648) as i32
            } else {
                q as i32
            };
            if index < 32 {
                self.state.shuffle[index] = n;
            }
        }
        self.state.recurrence = n;
        self.state.selector = self.state.shuffle[0];
        Ok(())
    }
    pub fn draw(&mut self) -> u32 {
        let mut q = Self::recurrence(self.state.recurrence);
        while q < 0 {
            q += 2147483647;
        }
        let slot = (self.state.selector as u32 % 32) as usize;
        let output = self.state.shuffle[slot];
        self.state.shuffle[slot] = q as i32;
        self.state.recurrence = q as i32;
        self.state.selector = output;
        output as u32
    }
    pub fn startup_diagnostic(&mut self) {
        for _ in 0..100_000 {
            self.draw();
        }
    }
}
impl RandomSource for ParkMiller {
    /// Every call consumes one draw. Reversed bounds return the first bound
    /// (`[Must-match]`); under the SPEC-006 §3.4 safe rule, so does any range
    /// whose width does not fit in 1 through 2147483647.
    fn range(&mut self, first: i32, last: i32) -> Result<i32, WorldError> {
        let width = i64::from(last) - i64::from(first) + 1;
        let draw = self.draw();
        if !(1..=i64::from(i32::MAX)).contains(&width) {
            return Ok(first);
        }
        Ok((i64::from(first) + i64::from(draw) % width) as i32)
    }
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Check {
    pub code: i32,
    pub margin: i32,
}
pub fn classify(
    margin: i32,
    critical: i32,
    ticks: u64,
    rng: &mut impl RandomSource,
) -> Result<Check, WorldError> {
    let success = margin >= 0;
    let mut code = if success { 2 } else { 1 };
    if ticks >= 864000 {
        let threshold = if success {
            i64::from(margin) / 10 + i64::from(critical)
        } else {
            -i64::from(margin) / 10
        };
        if i64::from(rng.range(1, 100)?) <= threshold {
            code = if success { 3 } else { 0 };
        }
    }
    Ok(Check { code, margin })
}
pub fn skill_check(
    chance: i32,
    modifier: i32,
    critical: i32,
    ticks: u64,
    rng: &mut impl RandomSource,
) -> Result<Check, WorldError> {
    let chance = chance.checked_add(modifier).ok_or(WorldError::Arithmetic)?;
    let margin = chance
        .checked_sub(rng.range(1, 100)?)
        .ok_or(WorldError::Arithmetic)?;
    classify(margin, critical, ticks, rng)
}
pub fn stat_check(
    stat: i32,
    modifier: i32,
    rng: &mut impl RandomSource,
) -> Result<Check, WorldError> {
    let margin = stat
        .checked_add(modifier)
        .and_then(|v| v.checked_sub(0))
        .ok_or(WorldError::Arithmetic)?
        .checked_sub(rng.range(1, 10)?)
        .ok_or(WorldError::Arithmetic)?;
    Ok(Check {
        code: if margin >= 0 { 2 } else { 1 },
        margin,
    })
}
pub fn contest(
    attacker: i32,
    defender: i32,
    critical: i32,
    ticks: u64,
    rng: &mut impl RandomSource,
) -> Result<Check, WorldError> {
    let attack = skill_check(attacker, 0, critical, ticks, rng)?;
    if attack.code < 2 {
        return Ok(attack);
    }
    let defend = skill_check(defender, 0, critical, ticks, rng)?;
    let margin = if defend.code >= 2 {
        attack
            .margin
            .checked_sub(defend.margin)
            .ok_or(WorldError::Arithmetic)?
    } else {
        attack.margin
    };
    classify(margin, 0, ticks, rng)
}
