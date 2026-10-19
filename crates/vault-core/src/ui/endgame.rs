//! SPEC-014 §3.30-3.32: the endgame slideshow (slide selection from
//! campaign globals, still slides, desert pans, narration subtitles), the
//! ending movie, and the scrolling credits.
use super::ColorKey;
use vault_formats::credits::{CreditKind, CreditLine};

/// One slideshow step: art under `art/intrface/` (with its `.pal`) and the
/// narration under `sound/speech/narrator/`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Slide {
    pub art: &'static str,
    pub narration: &'static str,
    /// A desert pan, and whether it runs left to right.
    pub pan: Option<bool>,
}
const fn still(art: &'static str, narration: &'static str) -> Slide {
    Slide {
        art,
        narration,
        pan: None,
    }
}
/// The global set to 0 after the sequence (or a failed setup).
pub const DONE_GLOBAL: usize = 299;

/// §3.30 the slides shown for campaign `globals` (zero-based), in order.
pub fn slides(globals: &[i32]) -> Vec<Slide> {
    let g = |index: usize| globals.get(index).copied().unwrap_or(0);
    let mut slides = vec![Slide {
        art: "dp",
        narration: if g(51) != 0 { "nar_11" } else { "nar_10" },
        pan: Some(true),
    }];
    let step2 = if g(13) != 0 {
        Some(still("seq2ad", "nar_15"))
    } else if g(30) != 0 && g(31) == 2 {
        Some(still("seq2b", "nar_13"))
    } else if g(30) != 0 {
        Some(still("seq2ad", "nar_12"))
    } else {
        None
    };
    let step3 = if g(7) != 0 {
        Some(still("seq3c", "nar_18"))
    } else if g(132) != 0 {
        Some(still("seq3a", "nar_16"))
    } else {
        None
    };
    let open = !matches!(g(26), 0 | 2);
    let step4 = Some(if g(12) != 0 {
        still("seq4de", "nar_23")
    } else if g(604) != 0 && open {
        still("seq4de", "nar_22")
    } else if g(604) != 0 {
        still("seq4abc", "nar_21")
    } else if open {
        still("seq4abc", "nar_20")
    } else {
        still("seq4abc", "nar_19")
    });
    let spared = g(104) != 2 || g(37) != 0;
    let step5 = if g(15) != 0 {
        Some(still("seq5d", "nar_27"))
    } else if spared && g(38) == 0 {
        Some(still("seq5b", "nar_25"))
    } else if spared {
        None
    } else {
        Some(still("seq5a", "nar_24"))
    };
    let step6 = Some(if g(108) == 2 && g(250) != 0 {
        still("seq6b", "nar_29")
    } else {
        still("seq6a", "nar_28")
    });
    let step7 = if g(14) != 0 {
        Some(still("seq7c", "nar_34"))
    } else if g(606) == 1 {
        Some(still("seq7a", "nar_32"))
    } else {
        None
    };
    let step8 = Some(if g(69) < 2 {
        still("seq8abc", "nar_37")
    } else if (g(114) != 0 && g(115) < 8) || g(115) < 4 {
        still("seq8abc", "nar_35")
    } else {
        still("seq8abc", "nar_36")
    });
    slides.extend(
        [step2, step3, step4, step5, step6, step7, step8]
            .into_iter()
            .flatten(),
    );
    slides.push(Slide {
        art: "dp",
        narration: "nar_40",
        pan: Some(false),
    });
    slides
}
impl Slide {
    pub fn art_path(&self) -> String {
        format!("art/intrface/{}.frm", self.art)
    }
    pub fn palette_path(&self) -> String {
        format!("art/intrface/{}.pal", self.art)
    }
    pub fn narration_path(&self) -> String {
        format!("sound/speech/narrator/{}.acm", self.narration)
    }
    pub fn subtitle_path(&self, language: &str) -> String {
        format!("text/{language}/cuts/{}.txt", self.narration)
    }
}

/// Still slides wait 500 ms after the fade in, and (unless input ended the
/// slide) 500 ms before the fade out; 3000 ms is the timeout when neither
/// narration nor subtitles loaded.
pub const SLIDE_PAUSE_MS: u64 = 500;
pub const SLIDE_SILENT_MS: u64 = 3000;

/// The desert pan: `dp.frm` is 1280 wide, so D = 640 columns.
pub const PAN_WIDTH: i32 = 1280;
pub const PAN_DISTANCE: i32 = PAN_WIDTH - 640;
pub const PAN_STEP_MS: u64 = 16;
/// The step delay with a narration of `seconds` (rounded up).
pub fn pan_step_ms(narration_seconds: Option<u32>) -> u64 {
    let d = PAN_DISTANCE as u64;
    match narration_seconds {
        Some(seconds) if 1000 * u64::from(seconds) > PAN_STEP_MS * d / 2 => {
            (1000 * u64::from(seconds) + PAN_STEP_MS * (d / 2)) / d
        }
        _ => PAN_STEP_MS,
    }
}
/// The fade zone F = trunc(D / 4) = 160.
pub const PAN_FADE: i32 = PAN_DISTANCE / 4;
/// The palette scale at pan offset `position` (0-639): fading in over the
/// first F offsets and out over the last F (safe rule: `D - F`).
pub fn pan_scale(position: i32) -> f64 {
    if position < PAN_FADE {
        f64::from(position) / f64::from(PAN_FADE)
    } else if position >= PAN_DISTANCE - PAN_FADE {
        f64::from(PAN_FADE - (position - (PAN_DISTANCE - PAN_FADE))) / f64::from(PAN_FADE)
    } else {
        1.0
    }
}
/// The offset at which narration starts: 160 going right, 480 going left.
pub fn pan_narration_offset(left_to_right: bool) -> i32 {
    if left_to_right {
        PAN_FADE
    } else {
        PAN_DISTANCE - PAN_FADE
    }
}
/// Subtitles wrap at 540 pixels; n wrapped lines start at
/// `480 - 10 (n + 1)`, in font 101 white on black boxes.
pub const SUBTITLE_WIDTH: i32 = 540;
pub fn subtitle_top(lines: usize) -> i32 {
    480 - 10 * (lines as i32 + 1)
}

/// §3.31 the ending movie: music `maybe` once, 3000 ms, walkm or walkw
/// with no flags, then the credits.
pub const ENDING_MUSIC: &str = "maybe";
pub const ENDING_FOLLOW_MUSIC: &str = "10labone";
pub const ENDING_WAIT_MS: u64 = 3000;

/// §3.32 credit styles: (title font, title color, name font, name color).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CreditStyle {
    Normal,
    /// Quotes: the fonts and colors swapped.
    Reversed,
}
pub const GREY_NAME: ColorKey = ColorKey(17969);
pub const ROW_PITCH: i32 = 26;
pub const CREDIT_TICK_MS: u64 = 38;
pub const TRAILING_TICKS: u32 = 480;
impl CreditStyle {
    /// Font and color for a row kind.
    pub fn font(self, kind: CreditKind) -> (i32, ColorKey) {
        let title = (103, ColorKey(13673));
        let name = (104, ColorKey(18917));
        let (title, name) = match self {
            Self::Normal => (title, name),
            Self::Reversed => ((104, ColorKey(18917)), (103, ColorKey(13673))),
        };
        match kind {
            CreditKind::Title => title,
            CreditKind::Name => name,
            CreditKind::GreyName => (name.0, GREY_NAME),
        }
    }
}
/// The scrolling credits: one pixel per tick; each row is 26 pixels; after
/// the last row enters, 480 blank ticks.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Credits {
    pub rows: Vec<CreditLine>,
    pub tick: u32,
    typed: usize,
    /// The `tblcrd.frm` animation's starting tick, when playing.
    pub egg: Option<u32>,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CreditInput {
    Char(char),
    /// Any other key or a mouse button.
    Other,
}
pub const EGG: &str = "boom";
pub const EGG_ART: (&str, [i32; 2], u32) = ("tblcrd.frm", [135, 201], 31);
impl Credits {
    /// Rows wider than the screen (640 or more) are skipped.
    pub fn new(rows: Vec<CreditLine>, width: impl Fn(&CreditLine) -> i32) -> Self {
        Self {
            rows: rows.into_iter().filter(|row| width(row) < 640).collect(),
            tick: 0,
            typed: 0,
            egg: None,
        }
    }
    /// Ticks until the last row has entered.
    pub fn scrolling_ticks(&self) -> u32 {
        self.rows.len() as u32 * ROW_PITCH as u32
    }
    pub fn finished(&self) -> bool {
        self.tick >= self.scrolling_ticks() + TRAILING_TICKS
    }
    pub fn advance(&mut self) {
        self.tick += 1;
    }
    /// Input: `true` ends the credits. While lines scroll, `b o o m` typed
    /// exactly (lowercase) starts the easter egg instead, when the violence
    /// level is not 0; any other input ends them.
    pub fn input(&mut self, input: CreditInput, violence: i32) -> bool {
        if self.tick >= self.scrolling_ticks() {
            return true;
        }
        let expected = EGG.as_bytes()[self.typed] as char;
        match input {
            CreditInput::Char(c) if c == expected => {
                self.typed += 1;
                if self.typed == EGG.len() {
                    self.typed = 0;
                    if violence != 0 {
                        self.egg = Some(self.tick);
                    }
                }
                false
            }
            _ => true,
        }
    }
    /// The easter egg frame at the current tick (one frame every 2 ticks).
    pub fn egg_frame(&self) -> Option<u32> {
        let start = self.egg?;
        let frame = (self.tick - start) / 2;
        (frame < EGG_ART.2).then_some(frame)
    }
    /// Each visible row and its screen y at the current tick (rows enter
    /// one pixel row at a time at the bottom).
    pub fn visible(&self) -> Vec<(usize, i32)> {
        (0..self.rows.len())
            .filter_map(|row| {
                let y = 480 + ROW_PITCH * row as i32 - self.tick as i32;
                (y > -ROW_PITCH && y < 480).then_some((row, y))
            })
            .collect()
    }
}
