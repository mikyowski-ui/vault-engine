//! SPEC-001 §2.1/3.1/3.2/3.7, SPEC-002 §3.2 and the SPEC-014 §3.44 gray
//! levels.
use crate::{ParseError, binary::Reader};
use std::sync::OnceLock;

pub const TRANSPARENT_INDEX: u8 = 0;

#[derive(Debug, Clone)]
pub struct Palette {
    rgb6: [[u8; 3]; 256],
    mapped: [bool; 256],
    rgb555: Vec<u8>,
    /// Supplied intensity, additive and multiplicative tables, in that order.
    /// A nonmatching discriminator has no supplied tables.
    tables: Option<Vec<u8>>,
    /// Every generated intensity, evaluated on the first bulk use.
    generated: OnceLock<Box<[u8]>>,
    /// The generated additive mix, evaluated on first use.
    generated_mix: OnceLock<Box<[u8]>>,
    /// The generated multiplicative mix, evaluated on first use.
    generated_product: OnceLock<Box<[u8]>>,
}

impl Palette {
    pub fn parse(bytes: &[u8]) -> Result<Self, ParseError> {
        let mut reader = Reader::new(bytes, "PAL")?;
        // The RGB triples and RGB555 lookup are a complete base palette.
        // Additional bytes may supply a four-byte tag (or padding).
        let base = reader.take(0x8300)?;
        let tag = if bytes.len() > reader.pos {
            Some(reader.take(4)?)
        } else {
            None
        };
        let tables = if tag == Some(b"CWEN".as_slice()) {
            Some(reader.take(3 * 65536)?.to_vec())
        } else {
            None
        };
        let mut rgb6 = [[0; 3]; 256];
        let mut mapped = [false; 256];
        for (index, triple) in base[..768].as_chunks::<3>().0.iter().enumerate() {
            if triple.iter().all(|channel| *channel <= 63) {
                rgb6[index].copy_from_slice(triple);
                mapped[index] = true;
            }
        }
        Ok(Self {
            rgb6,
            mapped,
            rgb555: base[768..0x8300].to_vec(),
            tables,
            generated: OnceLock::new(),
            generated_mix: OnceLock::new(),
            generated_product: OnceLock::new(),
        })
    }

    pub fn rgb6(&self, index: u8) -> [u8; 3] {
        self.rgb6[usize::from(index)]
    }

    pub fn is_mapped(&self, index: u8) -> bool {
        self.mapped[usize::from(index)]
    }

    /// Neutral-gamma display palette. Every entry is opaque: index 0 is a
    /// transparency key only for transparent blits, never a display alpha.
    pub fn rgba8(&self) -> [[u8; 4]; 256] {
        self.display_rgba8(1.0)
    }

    /// Display palette at the configured gamma; unavailable entries stay black.
    pub fn display_rgba8(&self, gamma: f64) -> [[u8; 4]; 256] {
        std::array::from_fn(|index| display_rgb6(self.rgb6[index], gamma))
    }

    pub fn supplied_tables(&self) -> Option<&[u8]> {
        self.tables.as_deref()
    }

    pub fn lookup_rgb5(&self, rgb: [u8; 3]) -> Option<u8> {
        if rgb.iter().any(|channel| *channel > 31) {
            return None;
        }
        Some(
            self.rgb555
                [usize::from(rgb[0]) * 1024 + usize::from(rgb[1]) * 32 + usize::from(rgb[2])],
        )
    }

    /// P(i), the five-bit components of a mapped entry (SPEC-001 §3.7).
    fn components(&self, index: u8) -> Option<[u8; 3]> {
        self.is_mapped(index)
            .then(|| self.rgb6(index).map(|channel| channel / 2))
    }

    /// SPEC-001 §3.7 the intensity table, row `foreground` and column
    /// `level`; supplied tables take precedence, and a generated entry is
    /// evaluated on demand.
    pub fn intensity(&self, foreground: u8, level: u8) -> u8 {
        if let Some(tables) = &self.tables {
            return tables[usize::from(foreground) * 256 + usize::from(level)];
        }
        let Some(rgb) = self.components(foreground) else {
            return 0;
        };
        let rgb = rgb.map(|c| {
            let (c, k) = (u16::from(c), u16::from(level));
            (if k < 128 {
                c * k / 128
            } else {
                c + (31 - c) * (k - 128) / 128
            }) as u8
        });
        self.lookup_rgb5(rgb).expect("generated five-bit channels")
    }

    /// The intensity of every color at every level, indexed `color * 256 +
    /// level`: the supplied table, or the generated rule evaluated once.
    pub fn intensity_table(&self) -> &[u8] {
        if let Some(tables) = &self.tables {
            return &tables[..65536];
        }
        self.generated.get_or_init(|| {
            (0..=u16::MAX)
                .map(|index| self.intensity((index >> 8) as u8, index as u8))
                .collect()
        })
    }

    /// SPEC-001 §3.7 the additive mix table, row `first` and column
    /// `second`: the supplied table, or else the channel-wise sum through
    /// the RGB555 lookup. A sum whose largest channel exceeds 31 by e loses
    /// e from every channel (clamped at 0) and is lightened through
    /// intensity column 128 + e; it is not clamped channel by channel.
    pub fn additive(&self, first: u8, second: u8) -> u8 {
        let index = usize::from(first) * 256 + usize::from(second);
        if let Some(tables) = &self.tables {
            return tables[65536 + index];
        }
        self.generated_mix.get_or_init(|| {
            self.generate_mix(|a, b| {
                let sum: [u8; 3] = std::array::from_fn(|channel| a[channel] + b[channel]);
                let largest = sum.into_iter().max().unwrap_or(0);
                if largest <= 31 {
                    return self.lookup_rgb5(sum).expect("five-bit sum");
                }
                let excess = largest - 31;
                let rest = sum.map(|channel| channel.saturating_sub(excess));
                let base = self.lookup_rgb5(rest).expect("five-bit remainder");
                self.intensity_table()[usize::from(base) * 256 + 128 + usize::from(excess)]
            })
        })[index]
    }

    /// SPEC-001 §3.7 the multiplicative mix table: the supplied table, or
    /// else the channel-wise `trunc(a × b / 32)` through the RGB555 lookup.
    pub fn multiplicative(&self, first: u8, second: u8) -> u8 {
        let index = usize::from(first) * 256 + usize::from(second);
        if let Some(tables) = &self.tables {
            return tables[2 * 65536 + index];
        }
        self.generated_product.get_or_init(|| {
            self.generate_mix(|a, b| {
                let product = std::array::from_fn(|channel| {
                    (u16::from(a[channel]) * u16::from(b[channel]) / 32) as u8
                });
                self.lookup_rgb5(product).expect("five-bit product")
            })
        })[index]
    }

    /// A generated mix table over every pair of mapped entries; a pair with
    /// an unmapped entry passes the other entry through, or the row entry
    /// when neither is mapped.
    fn generate_mix(&self, mix: impl Fn([u8; 3], [u8; 3]) -> u8) -> Box<[u8]> {
        (0..=u16::MAX)
            .map(|index| {
                let (a, b) = ((index >> 8) as u8, index as u8);
                match (self.components(a), self.components(b)) {
                    (Some(pa), Some(pb)) => mix(pa, pb),
                    (_, Some(_)) => b,
                    _ => a,
                }
            })
            .collect()
    }

    /// SPEC-014 §3.44 the gray level 0-7 of an entry for translucent draws:
    /// `trunc(trunc((b + 3r + 6g) / 10) / 4)` for the common table, or
    /// `trunc(trunc((r + 5g + 4b) / 10) / 4)` for the glass table, over the
    /// five-bit components (an unmapped entry, like index 0, is level 0).
    pub fn gray_level(&self, index: u8, glass: bool) -> u8 {
        if index == 0 {
            return 0;
        }
        let [r, g, b] = self.components(index).unwrap_or_default().map(u16::from);
        let sum = if glass {
            r + 5 * g + 4 * b
        } else {
            b + 3 * r + 6 * g
        };
        (sum / 10 / 4) as u8
    }

    /// SPEC-014 §3.37 a pixel drawn through a mask weight `weight` (0-128)
    /// over `destination`: weight 0 draws the source, otherwise
    /// `mix[intensity[source][weight]][intensity[destination][128 - weight]]`.
    pub fn mask_blend(&self, source: u8, destination: u8, weight: u8) -> u8 {
        if weight == 0 {
            return source;
        }
        let weight = weight.min(128);
        let table = self.intensity_table();
        self.additive(
            table[usize::from(source) * 256 + usize::from(weight)],
            table[usize::from(destination) * 256 + usize::from(128 - weight)],
        )
    }

    /// Shade `pixels` in place, each by its level in `levels`, through the
    /// intensity table; index 0, the transparency key, stays 0.
    pub fn shade(&self, pixels: &mut [u8], levels: &[u8]) {
        let table = self.intensity_table();
        for (pixel, level) in pixels.iter_mut().zip(levels) {
            if *pixel != 0 {
                *pixel = table[usize::from(*pixel) * 256 + usize::from(*level)];
            }
        }
    }

    /// Same-coordinate destination sampling, as specified by the synthetic fixture.
    /// Undefined coverage levels (14+) have no color result.
    pub fn blend_coverage(&self, coverage: u8, foreground: u8, destination: u8) -> Option<u8> {
        match coverage {
            0 => Some(destination),
            1..=7 => {
                let f = self.rgb6(foreground);
                let d = self.rgb6(destination);
                let rgb = std::array::from_fn(|channel| {
                    (u16::from(coverage) * u16::from(f[channel] / 2)
                        + u16::from(7 - coverage) * u16::from(d[channel] / 2))
                        as u8
                        / 7
                });
                self.lookup_rgb5(rgb)
            }
            8..=13 => Some(self.intensity(
                foreground,
                [127, 146, 164, 182, 201, 219][usize::from(coverage - 8)],
            )),
            _ => None,
        }
    }
}

/// SPEC-001 display conversion of one valid six-bit channel: raise it to the
/// gamma exponent, clamp to 0..=63, discard the fraction, then multiply by 4.
/// Gamma 1 is the neutral `x4` mapping (63 becomes 252, never 255).
pub fn display_channel(channel: u8, gamma: f64) -> u8 {
    // `as` truncates toward zero and maps NaN to zero.
    (f64::from(channel).powf(gamma).clamp(0.0, 63.0) as u8) * 4
}

pub fn display_rgb6(rgb6: [u8; 3], gamma: f64) -> [u8; 4] {
    let [r, g, b] = rgb6.map(|channel| display_channel(channel, gamma));
    [r, g, b, 255]
}

#[derive(Debug, Clone, Copy)]
pub struct CycleRange {
    pub first: u8,
    pub last: u8,
    pub interval_ms: u64,
    pub colors6: &'static [[u8; 3]],
}

pub const CYCLE_RANGES: [CycleRange; 5] = [
    CycleRange {
        first: 229,
        last: 232,
        interval_ms: 200,
        colors6: &[[0, 27, 0], [2, 28, 1], [6, 30, 3], [10, 32, 6]],
    },
    CycleRange {
        first: 233,
        last: 237,
        interval_ms: 100,
        colors6: &[
            [26, 26, 27],
            [24, 25, 31],
            [21, 26, 35],
            [0, 36, 40],
            [26, 46, 63],
        ],
    },
    CycleRange {
        first: 238,
        last: 242,
        interval_ms: 200,
        colors6: &[
            [63, 0, 0],
            [53, 0, 0],
            [36, 10, 2],
            [63, 29, 0],
            [63, 14, 0],
        ],
    },
    CycleRange {
        first: 243,
        last: 247,
        interval_ms: 142,
        colors6: &[[17, 0, 0], [30, 0, 0], [44, 0, 0], [30, 0, 0], [17, 0, 0]],
    },
    CycleRange {
        first: 248,
        last: 253,
        interval_ms: 200,
        colors6: &[
            [20, 15, 10],
            [18, 14, 10],
            [16, 13, 9],
            [15, 12, 9],
            [13, 11, 8],
            [12, 10, 8],
        ],
    },
];
pub const RED_PULSE_INDEX: usize = 254;
pub const RED_PULSE_INTERVAL_MS: u64 = 33;

#[derive(Debug, Clone)]
pub struct PaletteCycler {
    pub enabled: bool,
    pub interval_multiplier: u32,
    /// Display conversion applied to the cycled six-bit triples.
    pub gamma: f64,
    last: [u64; 4],
    phase: [usize; 5],
    red: i16,
    red_step: i16,
}

impl Default for PaletteCycler {
    fn default() -> Self {
        Self {
            enabled: true,
            interval_multiplier: 1,
            gamma: 1.0,
            last: [0; 4],
            phase: [0; 5],
            red: 0,
            red_step: 4,
        }
    }
}

impl PaletteCycler {
    /// Reset timers/enabled state without rewinding color phases.
    pub fn reset(&mut self) {
        self.last = [0; 4];
        self.enabled = true;
    }

    /// Palette fading disables cycling; returns the state to restore afterward.
    pub fn suspend_for_fade(&mut self) -> bool {
        std::mem::replace(&mut self.enabled, false)
    }

    pub fn restore_after_fade(&mut self, previous: bool) {
        self.enabled = previous;
    }

    pub fn poll(&mut self, now_ms: u64, display: &mut [[u8; 4]; 256]) -> bool {
        if !self.enabled {
            return false;
        }
        let due = [200, 100, 142, RED_PULSE_INTERVAL_MS]
            .map(|interval| interval.saturating_mul(u64::from(self.interval_multiplier)));
        let ready: [bool; 4] =
            std::array::from_fn(|group| now_ms.saturating_sub(self.last[group]) >= due[group]);
        for (group, ready) in ready.iter().enumerate() {
            if *ready {
                self.last[group] = now_ms;
            }
        }
        for (index, range) in CYCLE_RANGES.iter().enumerate() {
            let group = match index {
                1 => 1,
                3 => 2,
                _ => 0,
            };
            if ready[group] {
                let count = range.colors6.len();
                for pixel in 0..count {
                    let rgb6 = range.colors6[(pixel + count - self.phase[index]) % count];
                    display[usize::from(range.first) + pixel] = display_rgb6(rgb6, self.gamma);
                }
                self.phase[index] = (self.phase[index] + 1) % count;
            }
        }
        if ready[3] {
            self.red += self.red_step;
            if self.red == 60 || self.red == 0 {
                self.red_step = -self.red_step;
            }
            display[RED_PULSE_INDEX] = display_rgb6([self.red as u8, 0, 0], self.gamma);
        }
        ready.into_iter().any(|ready| ready)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A palette whose entries 1, 2, ... hold `triples` as five-bit
    /// components, with the RGB555 lookup returning exactly those entries
    /// (every other lookup is entry 0), and entries 240 and 241 unmapped.
    fn fixture(triples: &[[u8; 3]]) -> Palette {
        let mut bytes = vec![0; 0x8300];
        for (index, rgb) in triples.iter().enumerate() {
            let entry = index + 1;
            for channel in 0..3 {
                bytes[entry * 3 + channel] = rgb[channel] * 2;
            }
            let key = usize::from(rgb[0]) * 1024 + usize::from(rgb[1]) * 32 + usize::from(rgb[2]);
            bytes[0x300 + key] = entry as u8;
        }
        bytes[240 * 3..240 * 3 + 6].copy_from_slice(&[64, 0, 0, 0, 64, 0]);
        Palette::parse(&bytes).unwrap()
    }

    #[test]
    fn spec001_fixture_d_generated_table_arithmetic() {
        let triples = [
            [20, 10, 0],
            [10, 5, 0],
            [25, 20, 15],
            [10, 12, 3],
            [15, 10, 3],
            [25, 22, 6],
            [31, 20, 10],
            [31, 5, 2],
            [31, 0, 0],
            [31, 7, 7],
            [30, 3, 0],
        ];
        let palette = fixture(&triples);
        let at = |rgb: [u8; 3]| triples.iter().position(|t| *t == rgb).unwrap() as u8 + 1;
        assert!(!palette.is_mapped(240));
        // Intensity, columns 64 and 192; an unmapped row is index 0.
        assert_eq!(palette.intensity(at([20, 10, 0]), 64), at([10, 5, 0]));
        assert_eq!(palette.intensity(at([20, 10, 0]), 192), at([25, 20, 15]));
        for column in [0, 64, 128, 255] {
            assert_eq!(palette.intensity(240, column), 0);
        }
        // Additive mix: in range, then an excess of 31 lightened through
        // column 159 instead of a channel-wise clamp.
        assert_eq!(
            palette.additive(at([10, 12, 3]), at([15, 10, 3])),
            at([25, 22, 6])
        );
        assert_eq!(
            palette.additive(at([31, 20, 10]), at([31, 5, 2])),
            at([31, 7, 7])
        );
        assert_eq!(palette.intensity(at([31, 0, 0]), 159), at([31, 7, 7]));
        // Multiplicative mix of the same two entries.
        assert_eq!(
            palette.multiplicative(at([31, 20, 10]), at([31, 5, 2])),
            at([30, 3, 0])
        );
        // An unmapped entry passes the other through in either mix.
        let a = at([10, 12, 3]);
        for mix in [Palette::additive, Palette::multiplicative] {
            assert_eq!(mix(&palette, a, 240), a);
            assert_eq!(mix(&palette, 240, a), a);
            assert_eq!(mix(&palette, 240, 241), 240);
        }
    }

    #[test]
    fn supplied_mix_tables_are_read_as_stored() {
        let mut bytes = vec![0; 0x8300];
        bytes.extend(b"CWEN");
        bytes.extend((0..3 * 65536).map(|index| (index / 65536 + 1) as u8));
        let palette = Palette::parse(&bytes).unwrap();
        assert_eq!(palette.intensity(9, 9), 1);
        assert_eq!(palette.additive(9, 9), 2);
        assert_eq!(palette.multiplicative(9, 9), 3);
    }

    #[test]
    fn spec014_fixture_q_gray_levels() {
        let palette = fixture(&[[7, 31, 0], [0, 1, 0]]);
        assert_eq!(palette.gray_level(1, false), 5);
        assert_eq!(palette.gray_level(1, true), 4);
        assert_eq!(palette.gray_level(2, false), 0);
        assert_eq!(palette.gray_level(0, false), 0);
        assert_eq!(palette.gray_level(240, true), 0);
    }
}
