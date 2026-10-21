//! SPEC-012 desktop movie playback: one movie in the 640 x 480 window with
//! its fade file and subtitles, paced by the host clock (one movie step per
//! update), audio streamed in step with the frame timer, and the §3.12 exit
//! rules (a key, or a mouse button pressed during the movie and released).
use crate::{
    AppResult,
    audio::{AudioHandle, Channel},
};
use std::sync::Arc;
use vault_core::movie::{
    self, MoviePlayback, MovieRequest, Step,
    flags::{PAUSE_MUSIC, STOP_MUSIC},
};
use vault_formats::{
    Font, FontSet, MAX_ASSET_BYTES, Palette, Vfs, moviefx, mve::AudioFormat, palette::display_rgb6,
    sve,
};
use vault_render::{IndexedImage, movie::MovieWindow};

/// SPEC-012 §3.10: subtitles use font 101 and this color table.
pub const SUBTITLE_FONT: i32 = 101;
pub const SUBTITLE_PALETTE: &str = "art/cuts/subtitle.pal";

/// The subtitle font, color table, and text color (RGB555 31,31,31).
pub struct SubtitleStyle {
    font: Arc<Font>,
    palette: Palette,
    color: u8,
}
impl SubtitleStyle {
    /// Font 101 (from `fonts`, or the namespace's startup font search) and
    /// `art/cuts/subtitle.pal`; `None` when either is unavailable.
    pub fn load(vfs: &dyn Vfs, font: Option<Arc<Font>>) -> Option<Self> {
        let font = font.or_else(|| FontSet::load(vfs).get(SUBTITLE_FONT).cloned())?;
        let palette = vfs
            .read_bounded(SUBTITLE_PALETTE, MAX_ASSET_BYTES)
            .ok()
            .and_then(|bytes| Palette::parse(&bytes).ok())?;
        let color = palette.lookup_rgb5([31, 31, 31])?;
        Some(Self {
            font,
            palette,
            color,
        })
    }
}

/// Little-endian PCM bytes as samples; 8-bit samples are unsigned.
fn samples(format: &AudioFormat, bytes: &[u8]) -> Vec<i16> {
    if format.sixteen_bit {
        bytes
            .as_chunks::<2>()
            .0
            .iter()
            .map(|pair| i16::from_le_bytes(*pair))
            .collect()
    } else {
        bytes
            .iter()
            .map(|byte| (i16::from(*byte) - 128) << 8)
            .collect()
    }
}

pub struct MoviePlayer {
    pub request: MovieRequest,
    playback: MoviePlayback,
    window: MovieWindow,
    subtitles: Option<SubtitleStyle>,
    /// Audio output (silent until the shell attaches a device).
    pub audio: AudioHandle,
    /// The background-music volume setting (SPEC-014 §3.18), which sets the
    /// movie's level (SPEC-012 §3.8).
    pub music_volume: i32,
    /// The §3.12 music flags have been applied.
    begun: bool,
    streaming: bool,
    /// The host clock (ms) when playback began.
    origin: Option<u64>,
    /// A mouse button went down during the movie; releasing every button
    /// then ends it.
    clicked: bool,
    held: u32,
    done: bool,
}
impl MoviePlayer {
    /// Open the request's movie, fade file, and subtitles through the
    /// namespace. A missing fade or subtitle file plays without it.
    pub fn load(
        vfs: &dyn Vfs,
        request: MovieRequest,
        subtitles: Option<SubtitleStyle>,
    ) -> AppResult<Self> {
        let read = |path: &str| vfs.read_bounded(&path.replace('\\', "/"), MAX_ASSET_BYTES);
        let data = read(&request.path).map_err(|error| format!("{}: {error}", request.path))?;
        let effects = match request.fades.as_deref().map(read) {
            Some(Ok(bytes)) => moviefx::parse(&bytes).unwrap_or_else(|error| {
                tracing::warn!(%error, "movie fade file ignored");
                Vec::new()
            }),
            _ => Vec::new(),
        };
        let entries = match request.subtitles.as_deref().map(read) {
            Some(Ok(bytes)) => Some(sve::parse(&bytes)),
            _ => None,
        };
        let subtitles = subtitles.filter(|_| entries.is_some());
        if entries.is_some() && subtitles.is_none() {
            tracing::warn!("subtitle font 101 or art/cuts/subtitle.pal unavailable");
        }
        let playback = MoviePlayback::new(data, effects, entries)
            .map_err(|error| format!("{}: {error}", request.path))?;
        tracing::info!(movie = %request.path, flags = request.flags, "movie started");
        Ok(Self {
            request,
            playback,
            window: MovieWindow::new(),
            subtitles,
            audio: AudioHandle::default(),
            music_volume: vault_core::ui::options::DEFAULT_VOLUME,
            begun: false,
            streaming: false,
            origin: None,
            clicked: false,
            held: 0,
            done: false,
        })
    }
    pub fn finished(&self) -> bool {
        self.done
    }
    /// SPEC-012 §3.12 step 3, once, when playback begins (or a skip comes
    /// first): flag `0x04` stops the background music, otherwise `0x08`
    /// pauses it.
    fn begin(&mut self) {
        if std::mem::replace(&mut self.begun, true) {
            return;
        }
        if self.request.flags & STOP_MUSIC != 0 {
            self.audio.stop_music();
        } else if self.request.flags & PAUSE_MUSIC != 0 {
            self.audio.pause_music(true);
        }
    }
    /// Stop playback and its audio; the cleanup unpauses the music when the
    /// flags have `0x08` (§3.12 step 7).
    pub fn stop(&mut self) {
        if !self.done {
            self.begin();
            self.done = true;
            self.audio.stop(Channel::Movie);
            if self.request.flags & PAUSE_MUSIC != 0 {
                self.audio.pause_music(false);
            }
            tracing::info!(movie = %self.request.path, dropped = self.playback.dropped(), "movie ended");
        }
    }
    /// A key or other input event skips the movie.
    pub fn key(&mut self) {
        self.stop();
    }
    pub fn press(&mut self) {
        self.clicked = true;
        self.held += 1;
    }
    /// Releasing the last held button after a press during the movie skips
    /// it; holding a button does not.
    pub fn release(&mut self) {
        self.held = self.held.saturating_sub(1);
        if self.clicked && self.held == 0 {
            self.stop();
        }
    }
    /// One movie step at host time `now_ms`; returns whether the window or
    /// palette changed.
    pub fn tick(&mut self, now_ms: u64) -> bool {
        if self.done {
            return false;
        }
        self.begin();
        let origin = *self.origin.get_or_insert(now_ms);
        let now = now_ms.saturating_sub(origin).saturating_mul(1000);
        let step = self.playback.step(now);
        self.feed_audio();
        match step {
            Ok(Step::Waiting) => false,
            Ok(Step::Frame(step)) => {
                if step.displayed
                    && let Some(video) = self.playback.movie().video()
                {
                    self.window.show_frame(
                        video.current(),
                        video.size(),
                        self.playback.movie().region(),
                    );
                }
                if let Some(style) = &self.subtitles {
                    for text in &step.subtitles {
                        self.window
                            .draw_subtitle(&style.font, &style.palette, style.color, text);
                    }
                }
                step.displayed || step.palette || !step.subtitles.is_empty()
            }
            Ok(Step::Finished) => {
                self.stop();
                true
            }
            Err(error) => {
                // Safe rule: any library error ends the movie.
                tracing::warn!(movie = %self.request.path, %error, "movie stream error");
                self.stop();
                true
            }
        }
    }
    /// Stream decoded audio once the frame timer has started it.
    fn feed_audio(&mut self) {
        let Some(format) = self.playback.audio_format() else {
            return;
        };
        if !self.playback.audio_started() {
            return;
        }
        if !self.streaming {
            // §3.8: 0.94 x the music volume (0 while music is disabled),
            // converted to device attenuation.
            let gain = movie::attenuation_gain(movie::attenuation(movie::movie_volume(
                self.music_volume,
                self.audio.music_enabled(),
            )));
            self.streaming = self.audio.open_stream(
                Channel::Movie,
                u32::from(format.rate),
                format.channels(),
                gain,
            );
        }
        let pcm = self.playback.take_audio();
        if self.streaming && !pcm.is_empty() {
            self.audio.append(Channel::Movie, &samples(&format, &pcm));
        }
    }
    pub fn image(&self) -> IndexedImage {
        self.window.image.clone()
    }
    /// The device palette through the brightness `gamma` (SPEC-012 §3.9:
    /// pixel indexes map through the 6-bit levels and gamma); black once
    /// the movie ends.
    pub fn palette(&self, gamma: f64) -> [[u8; 4]; 256] {
        if self.done {
            return [[0, 0, 0, 255]; 256];
        }
        let levels = self.playback.device_palette();
        std::array::from_fn(|index| {
            display_rgb6(
                [
                    levels[3 * index],
                    levels[3 * index + 1],
                    levels[3 * index + 2],
                ],
                gamma,
            )
        })
    }
}

#[cfg(test)]
#[allow(dead_code)]
#[path = "../../../tests/fixtures/movie/support.rs"]
pub(crate) mod fixture;

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use vault_formats::LocalDirProvider;

    pub(crate) fn movie_corpus(dir: &std::path::Path) {
        let (bytes, _, _) = fixture::synthetic('B');
        std::fs::create_dir_all(dir.join("art/cuts")).unwrap();
        std::fs::write(dir.join("art/cuts/intro.mve"), bytes).unwrap();
        std::fs::write(dir.join("art/cuts/intro.cfg"), fixture::fades()).unwrap();
    }

    #[test]
    fn plays_paced_frames_streams_audio_and_skips_on_release() {
        let dir = tempfile::tempdir().unwrap();
        movie_corpus(dir.path());
        let vfs = LocalDirProvider::mount(dir.path()).unwrap();
        let request = MovieRequest {
            index: Some(2),
            flags: 0,
            path: r"art\cuts\intro.mve".into(),
            subtitles: None,
            fades: Some(r"art\cuts\intro.cfg".into()),
        };
        let (audio, mixer) = AudioHandle::detached();
        let mut player = MoviePlayer::load(&vfs, request.clone(), None).unwrap();
        player.audio = audio;
        // The first sync waits for deadline + T/4 (about 83 ms).
        assert!(!player.tick(0));
        assert!(!player.tick(50));
        assert!(player.tick(90));
        let image = player.image();
        assert_eq!((image.width(), image.height()), (640, 480));
        // The 64 x 64 frame sits at (288, 208); the border stays index 0.
        assert!(
            image.pixels()[208 * 640 + 288..208 * 640 + 352]
                .iter()
                .any(|p| *p != 0)
        );
        assert!(image.pixels()[..640].iter().all(|p| *p == 0));
        assert!(mixer.lock().unwrap().playing(Channel::Movie));
        assert!(mixer.lock().unwrap().queued(Channel::Movie) > 0);
        // Fixture G fades in from black: frame 1 is k = 2 of 10.
        assert!(
            player
                .palette(1.0)
                .iter()
                .any(|entry| entry[..3] != [0, 0, 0])
        );
        // Holding a button does not skip; releasing it does.
        player.press();
        assert!(!player.finished());
        player.release();
        assert!(player.finished());
        assert!(!mixer.lock().unwrap().playing(Channel::Movie));
        assert_eq!(player.palette(1.0), [[0, 0, 0, 255]; 256]);
        // A release without a press during the movie does not skip, and
        // the movie finishes on its own.
        let mut player = MoviePlayer::load(&vfs, request, None).unwrap();
        player.release();
        let mut now = 0;
        while !player.finished() {
            player.tick(now);
            now += 16;
            assert!(now < 10_000);
        }
        assert!(now > 4 * 66);
    }
}
