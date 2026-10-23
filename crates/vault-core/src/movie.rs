//! SPEC-012 game movies: the 14-entry movie table and its `play_gmovie`
//! flags, movie/subtitle/fade paths, the Pip-Boy archive selection, movie
//! volume, the subtitle band, and `MoviePlayback`, which runs a decoded
//! movie against the frame timer (sync waits, late frames and dropping),
//! palette pushes through the optional fade hook, and subtitle timing.
//! Playback is driven by host microseconds, so it never blocks.
use vault_formats::{
    ParseError,
    moviefx::{FadeEffect, FadeHook},
    mve::{self, AudioFormat, Event, Movie, ShownFrame},
    sve::{Subtitle, SubtitleTrack},
};

/// `play_gmovie` and engine flag bits (§2.11).
pub mod flags {
    /// Fade the game palette to black before the movie.
    pub const FADE_BEFORE: u32 = 0x01;
    /// Fade back to the game palette after the movie.
    pub const FADE_AFTER: u32 = 0x02;
    /// Stop background music.
    pub const STOP_MUSIC: u32 = 0x04;
    /// Pause background music for the duration.
    pub const PAUSE_MUSIC: u32 = 0x08;
}

/// How a library `playmovie` (SPEC-005 §3.2) waits when no presentation
/// layer may be attached to finish it.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum MoviePolicy {
    /// Headless (tests, tools, replays): the request is still emitted, but
    /// playback counts as ended at once, so the program's wait predicate is
    /// satisfied on its next check and a script can never hang on a movie
    /// nobody plays.
    #[default]
    CompleteImmediately,
    /// A presentation layer plays every request and reports the end through
    /// `WorldHost::movie_finished`; the program waits until then.
    WaitUntilFinished,
}

/// One row of the game movie table.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct GameMovie {
    /// File under `art\cuts\`.
    pub file: &'static str,
    /// The `play_gmovie` flags.
    pub flags: u32,
}
/// §2.11 game movie table, by index.
pub const GAME_MOVIES: [GameMovie; 14] = [
    GameMovie {
        file: "iplogo.mve",
        flags: 0x0b,
    },
    GameMovie {
        file: "mplogo.mve",
        flags: 0x0b,
    },
    GameMovie {
        file: "intro.mve",
        flags: 0x0b,
    },
    GameMovie {
        file: "vexpld.mve",
        flags: 0x09,
    },
    GameMovie {
        file: "cathexp.mve",
        flags: 0x09,
    },
    GameMovie {
        file: "ovrintro.mve",
        flags: 0x0b,
    },
    GameMovie {
        file: "boil3.mve",
        flags: 0x09,
    },
    GameMovie {
        file: "ovrrun.mve",
        flags: 0x09,
    },
    GameMovie {
        file: "walkm.mve",
        flags: 0x0b,
    },
    GameMovie {
        file: "walkw.mve",
        flags: 0x0b,
    },
    GameMovie {
        file: "dipedv.mve",
        flags: 0x09,
    },
    GameMovie {
        file: "boil1.mve",
        flags: 0x0b,
    },
    GameMovie {
        file: "boil2.mve",
        flags: 0x0b,
    },
    GameMovie {
        file: "raekills.mve",
        flags: 0x09,
    },
];
pub const IPLOGO: usize = 0;
pub const INTRO: usize = 2;
pub const VEXPLD: usize = 3;
pub const CATHEXP: usize = 4;
pub const OVRINTRO: usize = 5;
pub const BOIL3: usize = 6;
pub const WALKM: usize = 8;
pub const WALKW: usize = 9;
pub const BOIL1: usize = 11;
pub const BOIL2: usize = 12;
/// The SPEC-008 water-chip quest global (unfinished at most 1).
pub const WATER_CHIP_GLOBAL: usize = 101;
/// The Pip-Boy flags (§3.12 engine callers).
pub const ARCHIVE_FLAGS: u32 = 0x0b;

/// `art\cuts\<file>` for a table index.
pub fn movie_path(index: usize) -> Option<String> {
    GAME_MOVIES
        .get(index)
        .map(|movie| format!("art\\cuts\\{}", movie.file))
}
/// The movie path with everything from its last `.` replaced by `suffix`.
fn replace_extension(path: &str, suffix: &str) -> String {
    let stem = path.rfind('.').map_or(path, |dot| &path[..dot]);
    format!("{stem}{suffix}")
}
/// §3.10 `text\<language>\cuts\<movie name>.SVE`: the movie path from its
/// last backslash on, with its extension replaced.
pub fn subtitle_path(language: &str, movie: &str) -> String {
    let name = movie.rfind('\\').map_or(movie, |slash| &movie[slash + 1..]);
    format!(
        "text\\{language}\\cuts\\{}",
        replace_extension(name, ".SVE")
    )
}
/// §3.11 the fade file beside the movie (`art\cuts\intro.cfg`).
pub fn fade_path(movie: &str) -> String {
    replace_extension(movie, ".cfg")
}
/// The three boil movies always show subtitles.
pub fn subtitles_forced(index: usize) -> bool {
    matches!(index, BOIL3 | BOIL1 | BOIL2)
}
/// §3.13 `play_gmovie` flags: the table's, plus `0x02` for indexes 3 and 4
/// when no map is loaded. Safe rule: an index outside the table fails.
pub fn play_gmovie_flags(index: i32, map_loaded: bool) -> Option<u32> {
    let index = usize::try_from(index).ok()?;
    let movie = GAME_MOVIES.get(index)?;
    let extra = if matches!(index, VEXPLD | CATHEXP) && !map_loaded {
        flags::FADE_AFTER
    } else {
        0
    };
    Some(movie.flags | extra)
}
/// §3.12 Pip-Boy selection `a >= 0`: scan indexes 3..13 for played movies,
/// decrementing `a` at each and stopping when it reaches 0 or below (so 0
/// and 1 both pick the first). Safe rule: no qualifying movie plays nothing.
pub fn archive_selection(played: &[u8; 14], selection: i32) -> Option<usize> {
    if selection < 0 {
        return None;
    }
    let mut remaining = selection;
    for (index, flag) in played.iter().enumerate().skip(3) {
        if *flag != 0 {
            remaining -= 1;
            if remaining <= 0 {
                return Some(index);
            }
        }
    }
    None
}
/// The movies the Pip-Boy archive lists: played indexes 3..13 in order.
pub fn archive_movies(played: &[u8; 14]) -> Vec<usize> {
    (3..14).filter(|index| played[*index] != 0).collect()
}
/// §3.8 movie volume: `0.94 ×` the background-music volume (truncated), or
/// 0 when music is disabled (SPEC-010 §3.13: not scaled by master volume).
pub fn movie_volume(music_volume: i32, music_enabled: bool) -> i32 {
    if music_enabled {
        vault_formats::sfx::music_level(music_volume)
    } else {
        0
    }
}
/// §3.8 device attenuation `-1000 × log2(32767 / v)`, clamped to
/// -10000..0 (truncated); `v = 0` gives -10000. The value is in the
/// device's hundredths of a decibel (SPEC-010 §3.13).
pub fn attenuation(volume: i32) -> i32 {
    vault_formats::sfx::attenuation(volume)
}
/// The linear gain of an attenuation in hundredths of a decibel.
pub fn attenuation_gain(attenuation: i32) -> f32 {
    vault_formats::sfx::attenuation_gain(attenuation)
}
/// The 640 x 480 movie window.
pub const WINDOW: [i32; 2] = [640, 480];
/// §3.9 the centered top-left corner of a frame.
pub fn frame_origin(size: [usize; 2]) -> [i32; 2] {
    [
        (WINDOW[0] - size[0] as i32) / 2,
        (WINDOW[1] - size[1] as i32) / 2,
    ]
}
/// §3.10 the subtitle band `[x, y, width, height]`: the full window width,
/// `text height + 4` tall (clipped to the window bottom), with its top at
/// `(480 - H - Y - th) / 2 + H + Y` for the last displayed frame's height
/// H and top Y.
pub fn subtitle_band(text_height: i32, frame_height: i32, frame_top: i32) -> [i32; 4] {
    subtitle_band_lines(text_height, frame_height, frame_top, 1)
}
/// §3.10 step 6 `[May-improve]` safe rule: the band for an entry wrapped
/// onto `lines` lines grows downward by one text height per extra line,
/// `lines × th + 4` tall, clipped to the window bottom (one line gives the
/// reference band).
pub fn subtitle_band_lines(
    text_height: i32,
    frame_height: i32,
    frame_top: i32,
    lines: usize,
) -> [i32; 4] {
    let top = (WINDOW[1] - frame_height - frame_top - text_height) / 2 + frame_height + frame_top;
    let lines = i32::try_from(lines.max(1)).unwrap_or(i32::MAX);
    let height = text_height.saturating_mul(lines).saturating_add(4);
    [0, top, WINDOW[0], height.min(WINDOW[1] - top).max(0)]
}
/// §3.10 step 6 subtitle wrapping for the 640-pixel window: `(start, end)`
/// byte ranges of the lines. Each character adds its width (SPEC-002 §3.1;
/// a space counts as the space width); while the sum is at most 640 the
/// letter spacing is added and the scan continues. At the first character
/// that takes the sum past 640 the line steps back to the last space before
/// that character and ends just before it; the following spaces are
/// skipped, and the next line starts at the first non-space character.
///
/// `[May-improve]`: text with no space before the overflow point makes the
/// reference wrapper loop forever; here such a line breaks before the
/// overflowing character (and holds at least one character, so a single
/// glyph wider than the window still advances).
pub fn subtitle_lines(font: &vault_formats::Font, text: &[u8]) -> Vec<(usize, usize)> {
    let limit = i64::from(WINDOW[0]);
    let mut lines = Vec::new();
    let mut start = 0;
    while start < text.len() {
        let mut sum = 0_i64;
        let mut end = text.len();
        for (at, code) in text.iter().enumerate().skip(start) {
            // An unsupported code neither draws nor advances (SPEC-002).
            let Some(width) = font.character_width(*code) else {
                continue;
            };
            sum += i64::from(width);
            if sum > limit {
                end = match text[start..at].iter().rposition(|byte| *byte == b' ') {
                    Some(space) => start + space,
                    None => at.max(start + 1),
                };
                break;
            }
            sum += i64::from(font.spacing());
        }
        lines.push((start, end));
        start = end;
        while text.get(start) == Some(&b' ') {
            start += 1;
        }
    }
    lines
}

/// A game movie request for the presentation layer (§3.12): the movie was
/// found and is already marked played.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MovieRequest {
    /// The game movie table index; `None` for a movie played by path.
    pub index: Option<usize>,
    pub flags: u32,
    /// The movie's resource path (`art\cuts\<file>`).
    pub path: String,
    /// The subtitle file, when subtitles are on and it exists.
    pub subtitles: Option<String>,
    /// The fade file, when it exists.
    pub fades: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Timer {
    period: u64,
    deadline: u64,
}

/// One completed step: a processed show-frame and what changed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FrameStep {
    pub frame: ShownFrame,
    /// Neither suppressed nor dropped: the current buffer is presented.
    pub displayed: bool,
    pub dropped: bool,
    /// The device palette changed (a push reached it, or a fade wrote it).
    pub palette: bool,
    /// Subtitle texts drawn after this step, in order; the last stays.
    pub subtitles: Vec<Vec<u8>>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Step {
    /// The step waits for the frame timer; call again later.
    Waiting,
    Frame(FrameStep),
    /// The movie finished (after the final deadline).
    Finished,
}

/// §3.2-3.11 playback of one movie.
#[derive(Debug, Clone)]
pub struct MoviePlayback {
    movie: Movie,
    device: [u8; 768],
    fades: Option<FadeHook>,
    subtitles: Option<SubtitleTrack>,
    timer: Option<Timer>,
    late: bool,
    previous_dropped: bool,
    dropped: u32,
    /// An event whose wait is not over yet.
    pending: Option<Event>,
    /// A push reached the device during the current step.
    pushed: bool,
    audio: Vec<u8>,
    audio_started: bool,
    finished: bool,
}
impl MoviePlayback {
    /// §3.1: open the movie (header and first record). The fade hook
    /// exists when at least one effect is supplied; subtitles are timed
    /// when supplied. The device palette starts black.
    pub fn new(
        data: Vec<u8>,
        effects: Vec<FadeEffect>,
        subtitles: Option<Vec<Subtitle>>,
    ) -> Result<Self, ParseError> {
        Ok(Self {
            movie: Movie::open(data)?,
            device: [0; 768],
            fades: FadeHook::new(effects),
            subtitles: subtitles.map(SubtitleTrack::new),
            timer: None,
            late: false,
            previous_dropped: false,
            dropped: 0,
            pending: None,
            pushed: false,
            audio: Vec::new(),
            audio_started: false,
            finished: false,
        })
    }
    pub fn movie(&self) -> &Movie {
        &self.movie
    }
    /// The 6-bit device palette (256 x 3 levels).
    pub fn device_palette(&self) -> &[u8; 768] {
        &self.device
    }
    pub fn dropped(&self) -> u32 {
        self.dropped
    }
    pub fn audio_format(&self) -> Option<AudioFormat> {
        self.movie.audio_format()
    }
    /// Audio output starts at the first sync point after configuration.
    pub fn audio_started(&self) -> bool {
        self.audio_started
    }
    /// The decoded track-0 stream since the last call, in stream order.
    pub fn take_audio(&mut self) -> Vec<u8> {
        std::mem::take(&mut self.audio)
    }
    /// The subtitle currently drawn.
    pub fn subtitle(&self) -> Option<&[u8]> {
        self.subtitles.as_ref().and_then(SubtitleTrack::shown)
    }
    pub fn finished(&self) -> bool {
        self.finished
    }
    /// Run the current step as far as the clock (`now` in microseconds)
    /// allows. A sync point waits until `now >= deadline + T/4`; a timer
    /// change and the end of the movie wait for the current deadline.
    /// Errors end the movie (safe rule).
    pub fn step(&mut self, now: u64) -> Result<Step, ParseError> {
        if self.finished {
            return Ok(Step::Finished);
        }
        loop {
            let event = match self.pending.take() {
                Some(event) => event,
                None => match self.movie.next_event() {
                    Ok(event) => event,
                    Err(error) => {
                        self.finished = true;
                        return Err(error);
                    }
                },
            };
            match event {
                Event::Timer { rate, subdivision } => {
                    let period = mve::frame_period(rate, subdivision);
                    match self.timer {
                        Some(timer) if timer.period == period => {}
                        Some(timer) if now < timer.deadline => {
                            self.pending = Some(event);
                            return Ok(Step::Waiting);
                        }
                        _ => {
                            self.timer = Some(Timer {
                                period,
                                deadline: now + period,
                            })
                        }
                    }
                }
                Event::AudioFormat(_) => {}
                Event::Sync => {
                    if let Some(timer) = &mut self.timer {
                        let target = timer.deadline + timer.period / 4;
                        if now < target {
                            self.pending = Some(event);
                            return Ok(Step::Waiting);
                        }
                        let late = now - target;
                        timer.deadline += timer.period;
                        self.late = late > timer.period / 2 && !self.previous_dropped;
                    }
                    self.audio_started |= self.movie.audio_format().is_some();
                }
                Event::Audio { pcm, .. } => self.audio.extend(pcm),
                Event::Frame(frame) => return Ok(Step::Frame(self.show(frame))),
                Event::Finished => {
                    if let Some(timer) = self.timer
                        && now < timer.deadline
                    {
                        self.pending = Some(event);
                        return Ok(Step::Waiting);
                    }
                    self.finished = true;
                    return Ok(Step::Finished);
                }
            }
        }
    }
    /// §3.7 show frame, then the step's fade and subtitle updates.
    fn show(&mut self, frame: ShownFrame) -> FrameStep {
        let (start, count) = (frame.palette_start, frame.palette_count);
        if count > 0 {
            let table = self.movie.palette_table();
            match &mut self.fades {
                Some(hook) => {
                    let inside = hook.inside();
                    hook.push(table, &mut self.device, start, count);
                    self.pushed |= !inside;
                }
                None => {
                    mve::push_palette(table, &mut self.device, start, count);
                    self.pushed = true;
                }
            }
        }
        let dropped = !frame.suppressed && self.late && count == 0;
        if dropped {
            self.dropped += 1;
        }
        self.previous_dropped = dropped;
        let faded = self
            .fades
            .as_mut()
            .is_some_and(|hook| hook.update(frame.number, &mut self.device));
        let subtitles = self
            .subtitles
            .as_mut()
            .map(|track| track.update(frame.number))
            .unwrap_or_default()
            .into_iter()
            .map(|entry| entry.text)
            .collect();
        FrameStep {
            frame,
            displayed: !frame.suppressed && !dropped,
            dropped,
            palette: std::mem::take(&mut self.pushed) || faded,
            subtitles,
        }
    }
}
