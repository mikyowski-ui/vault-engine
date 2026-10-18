//! SPEC-012 §2.10/§3.11 movie fade files (`art/cuts/<movie>.cfg`, the
//! SPEC-009 §2.2 configuration grammar) and the fade hook that replaces the
//! movie's palette-push path while at least one effect is loaded.
use crate::{
    ParseError,
    ai::{Config, decimal_prefix},
};

/// One loaded effect covering frames `start .. start + steps - 1`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FadeEffect {
    pub start: i32,
    pub steps: i32,
    /// `fade_type=out`; otherwise `in`.
    pub out: bool,
    /// R, G, B 6-bit levels, stored modulo 256.
    pub color: [u8; 3],
}
impl FadeEffect {
    /// Whether `start <= frame <= start + steps - 1`.
    pub fn covers(&self, frame: u32) -> bool {
        let (frame, start) = (i64::from(frame), i64::from(self.start));
        (start..start + i64::from(self.steps)).contains(&frame)
    }
    /// The level at step `k` for a source level and this effect's color
    /// component, with truncating division, stored modulo 256. Fade in:
    /// `C - trunc(k (C - src) / S)`; fade out: `src - trunc(k (src - C) / S)`.
    pub fn level(&self, k: i64, source: u8, component: usize) -> u8 {
        let (color, source) = (i64::from(self.color[component]), i64::from(source));
        let steps = i64::from(self.steps);
        let level = if self.out {
            source - k * (source - color) / steps
        } else {
            color - k * (color - source) / steps
        };
        level as u8
    }
}

/// `values` split at commas: the first `count - 1` values end at a comma
/// and the last is the leading integer of the remainder. With `count >= 2`
/// and fewer than `count - 1` commas nothing loads.
fn leading_values(text: &[u8], count: i32) -> Option<Vec<i32>> {
    if count <= 0 {
        return Some(Vec::new());
    }
    let mut values = Vec::new();
    let mut rest = text;
    for _ in 1..count {
        let comma = rest.iter().position(|b| *b == b',')?;
        values.push(decimal_prefix(&rest[..comma]).0);
        rest = &rest[comma + 1..];
    }
    values.push(decimal_prefix(rest).0);
    Some(values)
}

/// Parse a fade file into its effects in load order (`effect_frames`
/// order). A missing `info` key, too few commas, or an effect section
/// without a valid `fade_type`, a `fade_color` with two commas, or
/// `fade_steps` loads nothing for that part.
pub fn parse(bytes: &[u8]) -> Result<Vec<FadeEffect>, ParseError> {
    let config = Config::parse(bytes)?;
    let Some(info) = config.section(b"info") else {
        return Ok(Vec::new());
    };
    let total = info
        .get(b"total_effects")
        .map_or(0, |v| decimal_prefix(v).0);
    let Some(starts) = info
        .get(b"effect_frames")
        .and_then(|frames| leading_values(frames, total))
    else {
        return Ok(Vec::new());
    };
    let mut effects = Vec::new();
    for start in starts {
        let Some(section) = config.section(start.to_string().as_bytes()) else {
            continue;
        };
        let out = match section.get(b"fade_type").map(<[u8]>::to_ascii_lowercase) {
            Some(kind) if kind == b"in" => false,
            Some(kind) if kind == b"out" => true,
            _ => continue,
        };
        let Some(color) = section
            .get(b"fade_color")
            .and_then(|color| leading_values(color, 3))
        else {
            continue;
        };
        let Some(steps) = section.get(b"fade_steps") else {
            continue;
        };
        effects.push(FadeEffect {
            start,
            steps: decimal_prefix(steps).0,
            out,
            color: [color[0] as u8, color[1] as u8, color[2] as u8],
        });
    }
    Ok(effects)
}

/// §3.11 steps 2-3: palette pushes go to a source palette and reach the
/// device only outside a fade; each step's update writes the whole device
/// palette while an effect covers the frame.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FadeHook {
    effects: Vec<FadeEffect>,
    source: [u8; 768],
    inside: bool,
}
impl FadeHook {
    /// The hook exists only when at least one effect loaded. The movie
    /// starts inside a fade if an effect starts at frame 1 or earlier.
    pub fn new(effects: Vec<FadeEffect>) -> Option<Self> {
        if effects.is_empty() {
            return None;
        }
        Some(Self {
            inside: effects.iter().any(|effect| effect.start <= 1),
            effects,
            source: [0; 768],
        })
    }
    pub fn effects(&self) -> &[FadeEffect] {
        &self.effects
    }
    pub fn inside(&self) -> bool {
        self.inside
    }
    /// A palette push of table entries `start .. start + count`.
    pub fn push(&mut self, table: &[u8; 768], device: &mut [u8; 768], start: u16, count: u16) {
        crate::mve::push_palette(table, &mut self.source, start, count);
        if !self.inside {
            crate::mve::push_palette(table, device, start, count);
        }
    }
    /// The update after a step with frame counter `frame`; effects are
    /// searched newest-loaded first. Returns whether the device palette was
    /// written.
    pub fn update(&mut self, frame: u32, device: &mut [u8; 768]) -> bool {
        let found = self.effects.iter().rev().find(|e| e.covers(frame)).copied();
        self.inside = found.is_some();
        let Some(effect) = found else {
            return false;
        };
        let k = i64::from(frame) - i64::from(effect.start) + 1;
        for (index, level) in device.iter_mut().enumerate() {
            *level = effect.level(k, self.source[index], index % 3);
        }
        true
    }
}
