//! SPEC-001 §2.1/3.1/3.2 and SPEC-002 §3.2.
use crate::{ParseError, binary::Reader};

pub const TRANSPARENT_INDEX: u8 = 0;

#[derive(Debug, Clone)]
pub struct Palette {
    rgb6: [[u8; 3]; 256],
    mapped: [bool; 256],
    rgb555: Vec<u8>,
    /// Supplied intensity, additive and multiplicative tables, in that order.
    /// A nonmatching discriminator has no supplied tables.
    tables: Option<Vec<u8>>,
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
        })
    }

    pub fn rgb6(&self, index: u8) -> [u8; 3] {
        self.rgb6[usize::from(index)]
    }

    pub fn is_mapped(&self, index: u8) -> bool {
        self.mapped[usize::from(index)]
    }

    pub fn rgba8(&self) -> [[u8; 4]; 256] {
        std::array::from_fn(|index| {
            let [r, g, b] = self.rgb6[index];
            [r * 4, g * 4, b * 4, if index == 0 { 0 } else { 255 }]
        })
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

    /// Generated intensity is evaluated lazily; supplied tables take precedence.
    pub fn intensity(&self, foreground: u8, level: u8) -> u8 {
        if let Some(tables) = &self.tables {
            return tables[usize::from(foreground) * 256 + usize::from(level)];
        }
        if !self.is_mapped(foreground) {
            return 0;
        }
        let rgb = self.rgb6(foreground).map(|channel| {
            let c = u16::from(channel / 2);
            let k = u16::from(level);
            (if k < 128 {
                c * k / 128
            } else {
                c + (31 - c) * (k - 128) / 128
            }) as u8
        });
        self.lookup_rgb5(rgb).expect("generated five-bit channels")
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
                    let [r, g, b] = range.colors6[(pixel + count - self.phase[index]) % count];
                    display[usize::from(range.first) + pixel] = [r * 4, g * 4, b * 4, 255];
                }
                self.phase[index] = (self.phase[index] + 1) % count;
            }
        }
        if ready[3] {
            self.red += self.red_step;
            if self.red == 60 || self.red == 0 {
                self.red_step = -self.red_step;
            }
            display[RED_PULSE_INDEX] = [self.red as u8 * 4, 0, 0, 255];
        }
        ready.into_iter().any(|ready| ready)
    }
}
