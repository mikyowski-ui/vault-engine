//! Desktop audio: a small mixer for SPEC-010 effects, talking-head speech,
//! SPEC-005 library sounds, background music and movie audio, played on the
//! default output device through cpal (feature `audio`). Effects and speech
//! are mono 16-bit at the 22050 Hz mixer rate whatever their ACM headers
//! declare; background music is interleaved stereo at the mixer rate
//! (SPEC-010 §3.6). At most four effects play at once; a fifth request fails
//! without evicting one. A movie streams its interleaved PCM through one
//! appendable stream at the movie's own rate, on the stream's own clock
//! (SPEC-012 §3.8). Every source is resampled to the device rate by linear
//! interpolation, so the output is a deterministic function of the requests.
//!
//! Every voice carries its SPEC-010 §3.13 level L (0..32767) and plays at
//! the attenuation of `floor(master x L / 32767)`, so a master change
//! re-applies at once to everything loaded; a movie plays at its own level,
//! unscaled by master.
use std::{
    collections::{BTreeMap, BTreeSet, VecDeque},
    sync::{Arc, Mutex, MutexGuard},
};
use vault_core::worldmap::MusicSwitch;
use vault_formats::{MAX_ASSET_BYTES, Vfs, acm, acm::MIXER_RATE, sfx};

/// SPEC-010 §3.9 active effect limit.
pub const EFFECT_VOICES: usize = 4;

/// What a voice belongs to: effects share the four-voice limit, speech has
/// one voice, each library slot has its own, a movie has one stream, and
/// the background music has one voice.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Channel {
    Effect,
    Speech,
    Library(usize),
    Movie,
    Music,
}

/// The linear gain of a volume v already scaled for the device (0..32767):
/// SPEC-010 §3.13's `-1000 x log2(32767 / v)` hundredths of a decibel,
/// clamped to -10000 (so 0 is silent), as the amplitude `10^(A / 2000)`.
/// A movie plays at the gain of its level directly (SPEC-012 §3.8).
pub fn volume_gain(volume: i32) -> f32 {
    sfx::attenuation_gain(sfx::attenuation(volume))
}
/// SPEC-010 §3.13 the gain of a sound at `level` under `master` volume.
pub fn level_gain(master: i32, level: i32) -> f32 {
    sfx::attenuation_gain(sfx::device_attenuation(master, level))
}

/// SPEC-014 §3.18 the mixer's volume settings, 0..32767 each. The master
/// volume scales every voice except a movie, whose level SPEC-012 §3.8 fixes
/// from the music volume alone; the music volume sets the background
/// music's level and the movie's; the effect and speech volumes set each
/// sound's level as it starts, except that playing narration follows the
/// speech volume (SPEC-010 §3.13).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Volumes {
    pub master: i32,
    pub music: i32,
    pub effects: i32,
    pub speech: i32,
}
impl Volumes {
    pub fn from_preferences(preferences: &vault_core::ui::options::Preferences) -> Self {
        Self {
            master: preferences.master_volume,
            music: preferences.music_volume,
            effects: preferences.sndfx_volume,
            speech: preferences.speech_volume,
        }
    }
}
impl Default for Volumes {
    /// The SPEC-014 §2.3 built-in defaults.
    fn default() -> Self {
        let volume = vault_core::ui::options::DEFAULT_VOLUME;
        Self {
            master: volume,
            music: volume,
            effects: volume,
            speech: volume,
        }
    }
}

// Without a device (feature `audio` off) only tests drain the mixer.
#[cfg_attr(not(feature = "audio"), allow(dead_code))]
struct Voice {
    /// Interleaved frames of `channels` samples.
    samples: Arc<[i16]>,
    channels: usize,
    /// Source frame position in 32.32 fixed point.
    position: u64,
    /// SPEC-010 §3.13 level L, scaled by master volume as it plays.
    level: i32,
    channel: Channel,
    looping: bool,
    paused: bool,
    /// Narration: its level follows the speech volume while it plays.
    narration: bool,
}
impl Voice {
    fn frames(&self) -> u64 {
        (self.samples.len() / self.channels) as u64
    }
}

/// A stream fed while it plays: interleaved samples at its own rate.
#[cfg_attr(not(feature = "audio"), allow(dead_code))]
struct Stream {
    channel: Channel,
    samples: VecDeque<i16>,
    channels: usize,
    rate: u32,
    /// Position from the front frame in 32.32 fixed point. During an
    /// underrun it runs on past the queued frames (see `fill`).
    position: u64,
    paused: bool,
    /// Samples have arrived: from then on the stream's clock runs.
    started: bool,
}

/// The left and right values of the frame at a 32.32 `position` between
/// frame `index` and frame `next`, linearly interpolated (G-14: no
/// nearest-neighbour aliasing). A mono frame gives the same value twice.
fn interpolate(
    sample: impl Fn(usize) -> i16,
    channels: usize,
    index: u64,
    next: u64,
    position: u64,
) -> [f32; 2] {
    let fraction = (position & 0xffff_ffff) as f32 / 4_294_967_296.0;
    let at = |frame: u64, channel: usize| f32::from(sample(frame as usize * channels + channel));
    [0, channels - 1].map(|channel| {
        let first = at(index, channel);
        (first + (at(next, channel) - first) * fraction) / 32768.0
    })
}
/// Add a left/right pair to one output frame: stereo output keeps the
/// channels, mono output (and any channel past the second) gets their mean.
fn mix(frame: &mut [f32], [left, right]: [f32; 2]) {
    let stereo = frame.len() > 1;
    for (output, sample) in frame.iter_mut().enumerate() {
        *sample += match output {
            0 if stereo => left,
            1 => right,
            _ => (left + right) / 2.0,
        };
    }
}

/// The voices currently playing.
pub struct Mixer {
    voices: Vec<Voice>,
    streams: Vec<Stream>,
    /// SPEC-014 §3.18 / SPEC-010 §3.13 the master volume (0..32767), which
    /// scales every voice and stream except a movie's.
    master: i32,
    /// SPEC-012 §3.8 the movie level v: `trunc(0.94 x music_volume)`, or 0
    /// while music is off; kept even when no movie plays.
    movie_level: i32,
}
impl Default for Mixer {
    fn default() -> Self {
        Self {
            voices: Vec::new(),
            streams: Vec::new(),
            master: sfx::FULL_VOLUME,
            movie_level: sfx::music_level(vault_core::ui::options::DEFAULT_VOLUME),
        }
    }
}
impl Mixer {
    /// Start a mono voice at SPEC-010 §3.13 `level`. Speech, music and
    /// library slots replace their previous voice; an effect beyond the
    /// limit is refused.
    pub fn play(
        &mut self,
        samples: Arc<[i16]>,
        level: i32,
        channel: Channel,
        looping: bool,
    ) -> bool {
        self.play_frames(samples, 1, level, channel, looping)
    }
    /// Start a voice of interleaved `channels`-wide frames (1 or 2) at the
    /// mixer rate.
    pub fn play_frames(
        &mut self,
        samples: Arc<[i16]>,
        channels: usize,
        level: i32,
        channel: Channel,
        looping: bool,
    ) -> bool {
        match channel {
            Channel::Effect => {
                let effects = self
                    .voices
                    .iter()
                    .filter(|voice| voice.channel == Channel::Effect)
                    .count();
                if effects >= EFFECT_VOICES {
                    return false;
                }
            }
            _ => self.stop(channel),
        }
        if !(1..=2).contains(&channels) || samples.len() < channels {
            return false;
        }
        self.voices.push(Voice {
            samples,
            channels,
            position: 0,
            level,
            channel,
            looping,
            paused: false,
            narration: false,
        });
        true
    }
    /// Start narration on the speech voice at `level`: unlike talking-head
    /// speech, its level follows later speech-volume changes.
    pub fn play_narration(&mut self, samples: Arc<[i16]>, level: i32) -> bool {
        let started = self.play(samples, level, Channel::Speech, false);
        if let Some(voice) = self.voices.last_mut().filter(|_| started) {
            voice.narration = true;
        }
        started
    }
    pub fn stop(&mut self, channel: Channel) {
        self.voices.retain(|voice| voice.channel != channel);
        self.streams.retain(|stream| stream.channel != channel);
    }
    pub fn playing(&self, channel: Channel) -> bool {
        self.voices.iter().any(|voice| voice.channel == channel)
            || self.streams.iter().any(|stream| stream.channel == channel)
    }
    /// The source frame a voice on `channel` has reached (frames played
    /// at the mixer rate), `None` when none plays there.
    pub fn position(&self, channel: Channel) -> Option<u64> {
        self.voices
            .iter()
            .find(|voice| voice.channel == channel)
            .map(|voice| (voice.position >> 32).min(voice.frames()))
    }
    /// Change the level of the voice playing on `channel`.
    pub fn set_level(&mut self, channel: Channel, level: i32) {
        for voice in self.voices.iter_mut().filter(|v| v.channel == channel) {
            voice.level = level;
        }
    }
    /// Change the level of playing narration.
    pub fn set_narration_level(&mut self, level: i32) {
        for voice in self.voices.iter_mut().filter(|v| v.narration) {
            voice.level = level;
        }
    }
    /// Pause or resume what plays on `channel`; a paused voice keeps its
    /// place and plays nothing.
    pub fn set_paused(&mut self, channel: Channel, paused: bool) {
        for voice in self.voices.iter_mut().filter(|v| v.channel == channel) {
            voice.paused = paused;
        }
        for stream in self.streams.iter_mut().filter(|s| s.channel == channel) {
            stream.paused = paused;
        }
    }
    /// SPEC-010 §3.13: a new master volume applies at once to every voice
    /// and stream loaded, playing or paused.
    pub fn set_master(&mut self, volume: i32) {
        self.master = volume.clamp(0, sfx::FULL_VOLUME);
    }
    /// SPEC-012 §3.8 the movie level, applied at once to a playing movie.
    pub fn set_movie_level(&mut self, level: i32) {
        self.movie_level = level.clamp(0, sfx::FULL_VOLUME);
    }
    /// The gain of a voice or stream at `level` on `channel`: a movie plays
    /// at the movie level unscaled; everything else at its level scaled by
    /// master (SPEC-010 §3.13).
    fn channel_gain(master: i32, movie_level: i32, channel: Channel, level: i32) -> f32 {
        if channel == Channel::Movie {
            volume_gain(movie_level)
        } else {
            level_gain(master, level)
        }
    }
    /// The gain the voice on `channel` plays at now.
    #[cfg(test)]
    pub fn gain(&self, channel: Channel) -> Option<f32> {
        let level = self.level(channel)?;
        Some(Self::channel_gain(
            self.master,
            self.movie_level,
            channel,
            level,
        ))
    }
    /// Start an empty stream of `channels`-wide frames at `rate` Hz,
    /// replacing the channel's voice or stream. An underrun plays silence
    /// until more samples arrive. A movie stream plays at the movie level;
    /// any other at full level scaled by master.
    pub fn open_stream(&mut self, channel: Channel, rate: u32, channels: usize) -> bool {
        self.stop(channel);
        if rate == 0 || !(1..=2).contains(&channels) {
            return false;
        }
        self.streams.push(Stream {
            channel,
            samples: VecDeque::new(),
            channels,
            rate,
            position: 0,
            paused: false,
            started: false,
        });
        true
    }
    /// Append interleaved samples to an open stream.
    pub fn append(&mut self, channel: Channel, samples: &[i16]) -> bool {
        match self
            .streams
            .iter_mut()
            .find(|stream| stream.channel == channel)
        {
            Some(stream) => {
                stream.samples.extend(samples);
                stream.started |= !samples.is_empty();
                true
            }
            None => false,
        }
    }
    /// Samples queued in a stream.
    #[cfg(test)]
    pub fn queued(&self, channel: Channel) -> usize {
        self.streams
            .iter()
            .filter(|stream| stream.channel == channel)
            .map(|stream| stream.samples.len())
            .sum()
    }
    #[cfg(test)]
    pub fn voices(&self) -> usize {
        self.voices.len()
    }
    #[cfg(test)]
    pub fn master(&self) -> i32 {
        self.master
    }
    #[cfg(test)]
    pub fn movie_level(&self) -> i32 {
        self.movie_level
    }
    #[cfg(test)]
    pub fn paused(&self, channel: Channel) -> bool {
        self.voices
            .iter()
            .any(|voice| voice.channel == channel && voice.paused)
    }
    /// The level of the voice on `channel`.
    #[cfg(test)]
    pub fn level(&self, channel: Channel) -> Option<i32> {
        self.voices
            .iter()
            .find(|voice| voice.channel == channel)
            .map(|voice| voice.level)
    }
    #[cfg_attr(not(feature = "audio"), allow(dead_code))]
    /// Mix into interleaved `channels`-wide frames at `rate` Hz: every voice
    /// and stream is resampled linearly to `rate`, scaled by the gain of
    /// its level under master volume (a movie's by the movie level), and
    /// the sum is clamped to [-1, 1].
    pub fn fill(&mut self, out: &mut [f32], channels: usize, rate: u32) {
        out.fill(0.0);
        if channels == 0 || rate == 0 {
            return;
        }
        let (master, movie) = (self.master, self.movie_level);
        let gain_of = move |channel, level| Self::channel_gain(master, movie, channel, level);
        let step = (u64::from(MIXER_RATE) << 32) / u64::from(rate);
        for voice in &mut self.voices {
            if voice.paused {
                continue;
            }
            let frames = voice.frames();
            let gain = gain_of(voice.channel, voice.level);
            for frame in out.chunks_mut(channels) {
                let mut index = voice.position >> 32;
                if index >= frames {
                    if !voice.looping {
                        break;
                    }
                    voice.position %= frames << 32;
                    index = voice.position >> 32;
                }
                // The last frame interpolates toward the first of a loop,
                // and holds otherwise.
                let next = if index + 1 < frames {
                    index + 1
                } else if voice.looping {
                    0
                } else {
                    index
                };
                let samples = &voice.samples;
                let value = interpolate(
                    |at| samples[at],
                    voice.channels,
                    index,
                    next,
                    voice.position,
                );
                mix(frame, value.map(|value| value * gain));
                voice.position += step;
            }
        }
        self.voices
            .retain(|voice| voice.looping || voice.position >> 32 < voice.frames());
        for stream in &mut self.streams {
            if stream.paused {
                continue;
            }
            let step = (u64::from(stream.rate) << 32) / u64::from(rate);
            let frames = (stream.samples.len() / stream.channels) as u64;
            let gain = gain_of(stream.channel, sfx::FULL_VOLUME);
            for frame in out.chunks_mut(channels) {
                let index = stream.position >> 32;
                if index >= frames {
                    if !stream.started {
                        break;
                    }
                    // G-14 / SPEC-012 §3.8: an underrun (the feeder stalled)
                    // plays silence while the stream's clock runs on, so
                    // the frames that should have played meanwhile are
                    // skipped when they arrive and the stream rejoins the
                    // frame timer instead of lagging behind it.
                    stream.position = stream.position.saturating_add(step);
                    continue;
                }
                // Linear interpolation toward the next queued frame; the
                // newest frame holds until its successor arrives.
                let next = (index + 1).min(frames - 1);
                let samples = &stream.samples;
                let value = interpolate(
                    |at| samples[at],
                    stream.channels,
                    index,
                    next,
                    stream.position,
                );
                mix(frame, value.map(|value| value * gain));
                stream.position += step;
            }
            let consumed = ((stream.position >> 32) as usize).min(frames as usize);
            stream.samples.drain(..consumed * stream.channels);
            stream.position -= (consumed as u64) << 32;
        }
        for sample in out {
            *sample = sample.clamp(-1.0, 1.0);
        }
    }
}

/// The mixer shared between the main thread and the device callback.
type SharedMixer = Arc<Mutex<Mixer>>;

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

/// A background-music request (SPEC-008 §3.10 city music, SPEC-014 §3.4
/// menu music): the track file and whether it loops.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MusicRequest {
    pub path: String,
    pub looping: bool,
}
impl MusicRequest {
    /// SPEC-008 §3.10: track `t` means `sound/music/<t>.acm`.
    pub fn track(name: &str, looping: bool) -> Self {
        Self {
            path: format!("sound/music/{name}.acm"),
            looping,
        }
    }
}

/// A track being decoded for the latest request.
struct PendingTrack {
    request: MusicRequest,
    token: u64,
}

/// The background-music player state shared by every handle to one mixer.
struct MusicState {
    /// SPEC-008 §3.10 the music on/off state: while music is off a request
    /// is only recorded.
    switch: MusicSwitch,
    /// The last request, recorded even while music is disabled.
    last: Option<MusicRequest>,
    pending: Option<PendingTrack>,
    next_token: u64,
    /// SPEC-012 §3.12 flag `0x08`: a movie paused the music.
    paused: bool,
    /// SPEC-010 §3.13 the music level, `trunc(0.94 x music_volume)`.
    level: i32,
    /// The music volume setting, which also gives the movie level.
    volume: i32,
    /// Decoded tracks by path, so a track heard before restarts at once
    /// (cleared when it would pass `MAX_DECODED_SAMPLES`).
    decoded: BTreeMap<String, Arc<[i16]>>,
    decoded_samples: usize,
    /// Tracks that could not be read or decoded (reported once).
    failed: BTreeSet<String>,
    worker: Option<std::thread::JoinHandle<()>>,
}
impl Default for MusicState {
    fn default() -> Self {
        let volume = vault_core::ui::options::DEFAULT_VOLUME;
        Self {
            switch: MusicSwitch::default(),
            last: None,
            pending: None,
            next_token: 0,
            paused: false,
            level: sfx::music_level(volume),
            volume,
            decoded: BTreeMap::new(),
            decoded_samples: 0,
            failed: BTreeSet::new(),
            worker: None,
        }
    }
}
/// Decoded music samples kept before the cache is cleared.
const MAX_DECODED_SAMPLES: usize = 32 * 1024 * 1024;
impl MusicState {
    fn remember(&mut self, path: String, samples: Arc<[i16]>) {
        if self.decoded_samples + samples.len() > MAX_DECODED_SAMPLES {
            self.decoded.clear();
            self.decoded_samples = 0;
        }
        self.decoded_samples += samples.len();
        self.decoded.insert(path, samples);
    }
    /// Replace the music voice with `samples` (stereo frames), from its
    /// beginning, paused if a movie holds the music paused.
    fn start(&self, mixer: &Mutex<Mixer>, samples: Arc<[i16]>, looping: bool) {
        let mut mixer = lock(mixer);
        // SPEC-010 §3.6: music is interleaved stereo at the mixer rate,
        // whatever the ACM header says.
        mixer.play_frames(samples, 2, self.level, Channel::Music, looping);
        if self.paused {
            mixer.set_paused(Channel::Music, true);
        }
    }
}

/// A shareable handle to the mixer and the music player; without a mixer,
/// audio is silent and every request reports that nothing plays.
#[derive(Clone, Default)]
pub struct AudioHandle {
    mixer: Option<SharedMixer>,
    music: Arc<Mutex<MusicState>>,
}
impl AudioHandle {
    /// A handle whose mixer no device drains (tests drive `fill` directly).
    #[cfg(test)]
    pub fn detached() -> (Self, SharedMixer) {
        let mixer = Arc::new(Mutex::new(Mixer::default()));
        (
            Self {
                mixer: Some(mixer.clone()),
                music: Default::default(),
            },
            mixer,
        )
    }
    fn lock(&self) -> Option<MutexGuard<'_, Mixer>> {
        self.mixer.as_deref().map(lock)
    }
    /// Play `samples` at SPEC-010 §3.13 `level` (scaled by master volume).
    pub fn play(&self, samples: Arc<[i16]>, level: i32, channel: Channel, looping: bool) -> bool {
        self.lock()
            .is_some_and(|mut mixer| mixer.play(samples, level, channel, looping))
    }
    /// SPEC-010 §3.13 narration (death and ending narration, the options
    /// narrator) on the speech voice at `trunc(0.69 x speech_volume)`; its
    /// level follows later speech-volume changes while it plays.
    pub fn play_narration(&self, samples: Arc<[i16]>, speech_volume: i32) -> bool {
        self.lock().is_some_and(|mut mixer| {
            mixer.play_narration(samples, sfx::speech_level(speech_volume))
        })
    }
    pub fn stop(&self, channel: Channel) {
        if let Some(mut mixer) = self.lock() {
            mixer.stop(channel);
        }
    }
    pub fn playing(&self, channel: Channel) -> bool {
        self.lock().is_some_and(|mixer| mixer.playing(channel))
    }
    /// SPEC-010 §3.12 the talking-head speech's playback position in bytes
    /// of decoded PCM (two per mono 16-bit sample, 44100 per second),
    /// `None` when no speech plays.
    pub fn speech_position(&self) -> Option<u64> {
        self.lock()?
            .position(Channel::Speech)
            .map(|frames| 2 * frames)
    }
    pub fn open_stream(&self, channel: Channel, rate: u32, channels: usize) -> bool {
        self.lock()
            .is_some_and(|mut mixer| mixer.open_stream(channel, rate, channels))
    }
    pub fn append(&self, channel: Channel, samples: &[i16]) -> bool {
        self.lock()
            .is_some_and(|mut mixer| mixer.append(channel, samples))
    }
    /// SPEC-014 §3.18 / SPEC-010 §3.13: the mixer master volume, re-applied
    /// at once to every loaded voice but a movie. F2 and F3 change only
    /// this: no preference is set, so the music on/off state stays as it is.
    pub fn set_master(&self, volume: i32) {
        if let Some(mut mixer) = self.lock() {
            mixer.set_master(volume);
        }
    }
    /// SPEC-014 §3.18 volumes set from the preferences (the sliders, a
    /// save's preference words, the startup configuration), applied at
    /// once (SPEC-010 §3.13): the master volume to every voice, the music
    /// level to the playing music, the speech level to playing narration
    /// (talking-head speech and effects keep the level they started with).
    /// SPEC-008 §3.10: master or music volume 0 turns music off by volume,
    /// stopping the track; nonzero volumes turn it back on, but the replay
    /// of the last track is left to the caller (`replay_music`, or a world
    /// that replays its own). The movie level follows the music volume and
    /// the on/off state, whether or not a movie plays (SPEC-012 §3.8).
    /// Returns the new on/off state when it changed.
    pub fn apply_volumes(&self, volumes: Volumes) -> Option<bool> {
        let mut music = lock(&self.music);
        music.level = sfx::music_level(volumes.music);
        music.volume = volumes.music;
        let change = music.switch.set_volumes(volumes.master, volumes.music);
        if change == Some(false) {
            music.pending = None;
        }
        if let Some(mut mixer) = self.lock() {
            mixer.set_master(volumes.master);
            mixer.set_level(Channel::Music, music.level);
            mixer.set_narration_level(sfx::speech_level(volumes.speech));
            if change == Some(false) {
                mixer.stop(Channel::Music);
            }
            mixer.set_movie_level(vault_core::movie::movie_volume(
                music.volume,
                music.switch.on(),
            ));
        }
        change
    }
    /// SPEC-008 §3.10 rule 1 at startup: music is off until the
    /// `fallout.cfg` `[sound] music` key (default 1) turns it on, and the
    /// configured volumes then apply as any setting does.
    pub fn configure_music(&self, configured: bool, volumes: Volumes) {
        lock(&self.music).switch = MusicSwitch::startup(configured, volumes.master, volumes.music);
        self.apply_volumes(volumes);
    }
    /// SPEC-008 §3.10 turning music back on replays the last recorded
    /// track; nothing plays while music is off.
    pub fn replay_music(&self, vfs: Arc<dyn Vfs>) {
        let mut music = lock(&self.music);
        if music.switch.on()
            && let Some(last) = music.last.clone()
        {
            self.start_music(&mut music, vfs, last);
        }
    }

    /// Play background music: the generic entry point for every music
    /// request (the main-menu track, the world map's, `wind2` and the city
    /// tracks). SPEC-008 §3.10: the request is recorded; while music is
    /// enabled the current track stops and the requested one starts from
    /// its beginning, even when it is the same track. A track decoded
    /// before restarts at once; a new one is read and decoded on a worker
    /// thread (P-02: the event loop never waits for it) while the current
    /// track keeps playing, then replaces it, unless a later request, a
    /// stop or a disable came first. A missing or undecodable track leaves
    /// silence.
    pub fn request_music(&self, vfs: Arc<dyn Vfs>, request: MusicRequest) {
        let mut music = lock(&self.music);
        music.last = Some(request.clone());
        if music.switch.on() {
            self.start_music(&mut music, vfs, request);
        }
    }
    fn start_music(&self, music: &mut MusicState, vfs: Arc<dyn Vfs>, request: MusicRequest) {
        let Some(mixer) = self.mixer.clone() else {
            return;
        };
        if let Some(samples) = music.decoded.get(&request.path).cloned() {
            music.pending = None;
            music.start(&mixer, samples, request.looping);
            return;
        }
        if music.failed.contains(&request.path) {
            music.pending = None;
            lock(&mixer).stop(Channel::Music);
            return;
        }
        // The same track already decoding starts once it is ready; the
        // request only updates how it plays.
        if let Some(pending) = &mut music.pending
            && pending.request.path == request.path
        {
            pending.request = request;
            return;
        }
        music.next_token += 1;
        let token = music.next_token;
        music.pending = Some(PendingTrack {
            request: request.clone(),
            token,
        });
        let shared = self.music.clone();
        let path = request.path;
        let worker = std::thread::Builder::new()
            .name("music-decode".into())
            .spawn(move || {
                let decoded = vfs
                    .read_bounded(&path, MAX_ASSET_BYTES)
                    .map_err(|error| error.to_string())
                    .and_then(|bytes| acm::decode(&bytes).map_err(|error| error.to_string()));
                let mut music = lock(&shared);
                if music.pending.as_ref().is_none_or(|p| p.token != token) {
                    return;
                }
                let Some(pending) = music.pending.take() else {
                    return;
                };
                match decoded {
                    Ok(decoded) => {
                        // SPEC-010 §3.7: the blocks before a failing one
                        // still play; the failure is reported.
                        if let Some(failure) = &decoded.failure {
                            tracing::warn!(%path, %failure, blocks = decoded.blocks, "music truncated at a bad block");
                        }
                        let samples: Arc<[i16]> = decoded.samples.into();
                        music.remember(path, samples.clone());
                        music.start(&mixer, samples, pending.request.looping);
                    }
                    Err(error) => {
                        tracing::warn!(%path, %error, "music unavailable");
                        music.failed.insert(path);
                        lock(&mixer).stop(Channel::Music);
                    }
                }
            });
        match worker {
            Ok(worker) => music.worker = Some(worker),
            Err(error) => {
                tracing::warn!(%error, "music decoder thread unavailable");
                music.pending = None;
            }
        }
    }
    /// SPEC-012 §3.12 flag `0x04`, and the main menu's Exit: stop the music
    /// (a track still decoding never starts). The request stays recorded.
    pub fn stop_music(&self) {
        let mut music = lock(&self.music);
        music.pending = None;
        if let Some(mut mixer) = self.lock() {
            mixer.stop(Channel::Music);
        }
    }
    /// SPEC-012 §3.12 flag `0x08`: pause (or resume) the music in place.
    pub fn pause_music(&self, paused: bool) {
        let mut music = lock(&self.music);
        music.paused = paused;
        if let Some(mut mixer) = self.lock() {
            mixer.set_paused(Channel::Music, paused);
        }
    }
    /// SPEC-008 §3.10 whether music is on.
    pub fn music_enabled(&self) -> bool {
        lock(&self.music).switch.on()
    }
    /// The last music request (tests).
    #[cfg(test)]
    pub fn last_music(&self) -> Option<MusicRequest> {
        lock(&self.music).last.clone()
    }
    /// Wait for the music decoder (tests).
    #[cfg(test)]
    pub fn wait_music(&self) {
        let worker = lock(&self.music).worker.take();
        if let Some(worker) = worker {
            worker.join().expect("the music decoder does not panic");
        }
    }
}

/// The output device stream (kept alive on the main thread) and its mixer.
pub struct Audio {
    handle: AudioHandle,
    #[cfg(feature = "audio")]
    _stream: Option<cpal::Stream>,
}
impl Audio {
    /// No output: requests resolve and decode but play silently.
    pub fn silent() -> Self {
        Self {
            handle: AudioHandle::default(),
            #[cfg(feature = "audio")]
            _stream: None,
        }
    }
    /// Open the default output device, or fall back to silence.
    #[cfg(feature = "audio")]
    pub fn open() -> Self {
        match open_stream() {
            Ok((stream, mixer)) => Self {
                handle: AudioHandle {
                    mixer: Some(mixer),
                    music: Default::default(),
                },
                _stream: Some(stream),
            },
            Err(error) => {
                tracing::warn!(%error, "audio output unavailable; sounds are silent");
                Self::silent()
            }
        }
    }
    #[cfg(not(feature = "audio"))]
    pub fn open() -> Self {
        tracing::info!("built without the audio feature; sounds are silent");
        Self::silent()
    }
    pub fn handle(&self) -> AudioHandle {
        self.handle.clone()
    }
}

#[cfg(feature = "audio")]
fn open_stream() -> Result<(cpal::Stream, SharedMixer), Box<dyn std::error::Error>> {
    use cpal::{
        SampleFormat,
        traits::{DeviceTrait, HostTrait, StreamTrait},
    };
    let device = cpal::default_host()
        .default_output_device()
        .ok_or("no default output device")?;
    let supported = device.default_output_config()?;
    let format = supported.sample_format();
    let config: cpal::StreamConfig = supported.into();
    let (channels, rate) = (usize::from(config.channels), config.sample_rate.0);
    let mixer = Arc::new(Mutex::new(Mixer::default()));
    let errors = |error: cpal::StreamError| tracing::warn!(%error, "audio stream error");
    let shared = mixer.clone();
    // A converting format mixes into a reused f32 scratch buffer.
    let mut scratch: Vec<f32> = Vec::new();
    let stream = match format {
        SampleFormat::F32 => device.build_output_stream(
            &config,
            move |out: &mut [f32], _: &cpal::OutputCallbackInfo| {
                render(&shared, out, channels, rate);
            },
            errors,
            None,
        )?,
        SampleFormat::I16 => device.build_output_stream(
            &config,
            move |out: &mut [i16], _: &cpal::OutputCallbackInfo| {
                scratch.resize(out.len(), 0.0);
                render(&shared, &mut scratch, channels, rate);
                for (target, value) in out.iter_mut().zip(&scratch) {
                    *target = (value * 32767.0) as i16;
                }
            },
            errors,
            None,
        )?,
        SampleFormat::U16 => device.build_output_stream(
            &config,
            move |out: &mut [u16], _: &cpal::OutputCallbackInfo| {
                scratch.resize(out.len(), 0.0);
                render(&shared, &mut scratch, channels, rate);
                for (target, value) in out.iter_mut().zip(&scratch) {
                    *target = ((value * 32767.0) as i32 + 32768) as u16;
                }
            },
            errors,
            None,
        )?,
        other => return Err(format!("unsupported output sample format {other:?}").into()),
    };
    stream.play()?;
    tracing::info!(channels, rate, "audio output started");
    Ok((stream, mixer))
}

/// Fill a device buffer, never blocking the callback: a contended pass is
/// silent.
#[cfg(feature = "audio")]
fn render(mixer: &Mutex<Mixer>, out: &mut [f32], channels: usize, rate: u32) {
    match mixer.try_lock() {
        Ok(mut mixer) => mixer.fill(out, channels, rate),
        Err(_) => out.fill(0.0),
    }
}

#[cfg(test)]
pub(crate) use vault_fixtures::sound as sound_fixture;

#[cfg(test)]
mod tests {
    use super::*;

    /// SPEC-010 §3.13 full level: 0 dB under full master volume.
    const FULL: i32 = sfx::FULL_VOLUME;

    fn tone(length: usize) -> Arc<[i16]> {
        (0..length).map(|index| (index as i16) * 100).collect()
    }
    fn close(out: &[f32], expected: &[f32]) -> bool {
        out.len() == expected.len()
            && out
                .iter()
                .zip(expected)
                .all(|(out, expected)| (out - expected).abs() < 1e-6)
    }

    #[test]
    fn mixer_limits_effects_resamples_and_finishes_voices() {
        let mut mixer = Mixer::default();
        for _ in 0..EFFECT_VOICES {
            assert!(mixer.play(tone(8), FULL, Channel::Effect, false));
        }
        // A fifth effect is refused; nothing is evicted.
        assert!(!mixer.play(tone(8), FULL, Channel::Effect, false));
        assert_eq!(mixer.voices(), EFFECT_VOICES);
        // Speech and library slots have their own voices.
        assert!(mixer.play(tone(4), 16384, Channel::Speech, false));
        assert!(mixer.play(tone(4), 16384, Channel::Speech, false));
        assert!(mixer.play(tone(4), FULL, Channel::Library(3), true));
        assert_eq!(mixer.voices(), EFFECT_VOICES + 2);
        mixer.stop(Channel::Effect);
        assert!(!mixer.playing(Channel::Effect) && mixer.playing(Channel::Speech));

        // At the mixer rate each output frame takes one source sample; the
        // stereo frame carries it on both channels.
        let mut mixer = Mixer::default();
        mixer.play(
            Arc::from([16384_i16, -16384, 8192]),
            FULL,
            Channel::Effect,
            false,
        );
        let mut out = [0.0; 8];
        mixer.fill(&mut out, 2, MIXER_RATE);
        assert_eq!(out, [0.5, 0.5, -0.5, -0.5, 0.25, 0.25, 0.0, 0.0]);
        assert_eq!(mixer.voices(), 0, "a finished voice is removed");
        // At twice the rate the voice is interpolated between samples.
        mixer.play(Arc::from([0_i16, 16384]), FULL, Channel::Speech, false);
        let mut out = [0.0; 4];
        mixer.fill(&mut out, 1, 2 * MIXER_RATE);
        assert_eq!(out, [0.0, 0.25, 0.5, 0.5]);
        assert!(!mixer.playing(Channel::Speech));
        // Looping voices wrap; mixing clamps.
        mixer.play(Arc::from([32767_i16]), FULL, Channel::Library(0), true);
        mixer.play(Arc::from([32767_i16]), FULL, Channel::Effect, false);
        let mut out = [0.0; 3];
        mixer.fill(&mut out, 1, MIXER_RATE);
        assert_eq!(out[0], 1.0);
        assert!(out[1] > 0.99 && mixer.playing(Channel::Library(0)));
    }

    #[test]
    fn movie_streams_interpolate_and_resync_after_an_underrun() {
        let mut mixer = Mixer::default();
        mixer.set_movie_level(FULL);
        assert!(!mixer.append(Channel::Movie, &[1]), "no stream yet");
        assert!(mixer.open_stream(Channel::Movie, MIXER_RATE, 2));
        // Before any data arrives the stream waits without losing time.
        let mut out = [0.0; 4];
        mixer.fill(&mut out, 2, MIXER_RATE);
        assert_eq!(out, [0.0; 4]);
        assert!(mixer.append(Channel::Movie, &[16384, -16384, 8192, 0]));
        // Stereo frames keep their channels; an underrun is silent and the
        // stream keeps playing.
        let mut out = [0.0; 6];
        mixer.fill(&mut out, 2, MIXER_RATE);
        assert_eq!(out, [0.5, -0.5, 0.25, 0.0, 0.0, 0.0]);
        assert!(mixer.playing(Channel::Movie));
        assert_eq!(mixer.queued(Channel::Movie), 0);
        // G-14: the frame due during the underrun is skipped when it
        // arrives late, so the next one plays in step.
        mixer.append(Channel::Movie, &[32767, 32767, -32768, 32767]);
        let mut mono = [0.0; 1];
        mixer.fill(&mut mono, 1, MIXER_RATE);
        assert!(mono[0].abs() < 1e-4, "mono output mixes both channels");
        assert_eq!(mixer.queued(Channel::Movie), 0);
        // A mono stream at half the output rate is interpolated linearly
        // (G-14: no nearest-neighbour repeats); its newest frame holds.
        assert!(mixer.open_stream(Channel::Movie, MIXER_RATE / 2, 1));
        mixer.append(Channel::Movie, &[16384, -16384]);
        let mut out = [0.0; 4];
        mixer.fill(&mut out, 1, MIXER_RATE);
        assert_eq!(out, [0.5, 0.0, -0.5, -0.5]);
        mixer.stop(Channel::Movie);
        assert!(!mixer.playing(Channel::Movie));
        assert!(!mixer.open_stream(Channel::Movie, 0, 2));
        // Master volume does not touch a movie (SPEC-012 §3.8, SPEC-010
        // §3.13), even at 0.
        mixer.set_master(0);
        mixer.open_stream(Channel::Movie, MIXER_RATE, 1);
        mixer.append(Channel::Movie, &[16384, 16384, 16384]);
        let mut out = [0.0; 1];
        mixer.fill(&mut out, 1, MIXER_RATE);
        assert_eq!(out, [0.5]);
        // The movie level applies at once: 0.94 x 22281 is -645.
        mixer.set_movie_level(sfx::music_level(22281));
        mixer.fill(&mut out, 1, MIXER_RATE);
        assert!(close(&out, &[0.5 * 10_f32.powf(-645.0 / 2000.0)]));
        mixer.set_movie_level(0);
        mixer.fill(&mut out, 1, MIXER_RATE);
        assert!(out[0].abs() < 1e-5, "music off: level 0 is silent");
    }

    #[test]
    fn stereo_voices_play_frames_at_the_mixer_rate() {
        // G-05 / SPEC-010 §3.6: music is interleaved stereo at the mixer
        // rate, one output frame per source frame (not half speed).
        let mut mixer = Mixer::default();
        let music: Arc<[i16]> = Arc::from([16384_i16, -16384, 8192, 0]);
        assert!(mixer.play_frames(music.clone(), 2, FULL, Channel::Music, false));
        let mut out = [0.0; 6];
        mixer.fill(&mut out, 2, MIXER_RATE);
        assert_eq!(out, [0.5, -0.5, 0.25, 0.0, 0.0, 0.0]);
        assert!(!mixer.playing(Channel::Music), "two frames, two outputs");
        // At twice the rate each channel is interpolated on its own; mono
        // output takes the mean.
        mixer.play_frames(music.clone(), 2, FULL, Channel::Music, false);
        let mut out = [0.0; 4];
        mixer.fill(&mut out, 2, 2 * MIXER_RATE);
        assert_eq!(out, [0.5, -0.5, 0.375, -0.25]);
        mixer.play_frames(music.clone(), 2, FULL, Channel::Music, false);
        let mut mono = [0.0; 1];
        mixer.fill(&mut mono, 1, MIXER_RATE);
        assert_eq!(mono, [0.0]);
        assert!(!mixer.play_frames(Arc::from([1_i16]), 2, FULL, Channel::Music, false));
        assert!(!mixer.play_frames(music.clone(), 3, FULL, Channel::Music, false));
        // Levels, the master volume and pauses apply to what plays: level
        // 16384 under master 16384 is v = 8192, -1999 hundredths of a dB.
        mixer.play_frames(music, 2, FULL, Channel::Music, true);
        mixer.set_level(Channel::Music, 16384);
        mixer.set_master(16384);
        let gain = 10_f32.powf(-1999.0 / 2000.0);
        assert_eq!(mixer.gain(Channel::Music), Some(gain));
        let mut out = [0.0; 2];
        mixer.fill(&mut out, 2, MIXER_RATE);
        assert!(close(&out, &[0.5 * gain, -0.5 * gain]));
        mixer.set_paused(Channel::Music, true);
        mixer.fill(&mut out, 2, MIXER_RATE);
        assert_eq!(out, [0.0, 0.0]);
        mixer.set_paused(Channel::Music, false);
        mixer.fill(&mut out, 2, MIXER_RATE);
        assert!(
            close(&out, &[0.25 * gain, 0.0]),
            "a paused voice resumes in place"
        );
    }

    #[test]
    fn levels_scale_by_master_volume_through_the_device_attenuation() {
        assert_eq!(volume_gain(32767), 1.0);
        assert!(volume_gain(0) < 1e-4);
        // 22281, the default, is -556 hundredths of a decibel.
        assert!((volume_gain(22281) - 10_f32.powf(-556.0 / 2000.0)).abs() < 1e-6);
        assert_eq!(Volumes::default().music, 22281);
        // SPEC-010 §4.8 at the defaults: an on-screen effect, music,
        // speech and a library sound under master 22281.
        for (level, decibels) in [
            (22281, -1112),
            (sfx::music_level(22281), -1202),
            (sfx::speech_level(22281), -1648),
            (sfx::LIBRARY_LEVEL, -556),
        ] {
            let expected = 10_f32.powf(decibels as f32 / 2000.0);
            assert!((level_gain(22281, level) - expected).abs() < 1e-6);
        }
        // A master change applies at once to a loaded voice, playing or
        // paused; master 0 silences it.
        let mut mixer = Mixer::default();
        mixer.play(tone(8), 22281, Channel::Effect, false);
        mixer.set_paused(Channel::Effect, true);
        mixer.set_master(22281);
        assert_eq!(mixer.gain(Channel::Effect), Some(level_gain(22281, 22281)));
        mixer.set_master(0);
        assert!(mixer.gain(Channel::Effect).unwrap() < 1e-4);
        assert_eq!(mixer.master(), 0);
        // Narration follows the speech level; talking-head speech keeps
        // the level it started with.
        assert!(mixer.play_narration(tone(8), 15373));
        mixer.set_narration_level(100);
        assert_eq!(mixer.level(Channel::Speech), Some(100));
        mixer.play(tone(8), 15373, Channel::Speech, false);
        mixer.set_narration_level(200);
        assert_eq!(mixer.level(Channel::Speech), Some(15373));
    }

    #[test]
    fn music_requests_decode_off_the_event_loop_and_follow_the_rules() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join("sound/music")).unwrap();
        for (name, vector) in [("one", 'B'), ("two", 'D'), ("three", 'C')] {
            std::fs::write(
                dir.path().join(format!("sound/music/{name}.acm")),
                sound_fixture::acm(vector),
            )
            .unwrap();
        }
        let vfs: Arc<dyn Vfs> =
            Arc::new(vault_formats::LocalDirProvider::mount(dir.path()).unwrap());
        let (audio, mixer) = AudioHandle::detached();
        let request = MusicRequest::track("one", true);
        assert_eq!(request.path, "sound/music/one.acm");
        audio.request_music(vfs.clone(), request.clone());
        audio.wait_music();
        assert!(mixer.lock().unwrap().playing(Channel::Music));
        assert_eq!(audio.last_music(), Some(request.clone()));
        // The same track restarts from its beginning (SPEC-008 §3.10).
        mixer.lock().unwrap().fill(&mut [0.0; 8], 2, MIXER_RATE);
        audio.request_music(vfs.clone(), request.clone());
        assert_eq!(mixer.lock().unwrap().voices[0].position, 0);
        // A movie's 0x08 pauses it in place, 0x04 stops it.
        audio.pause_music(true);
        assert!(mixer.lock().unwrap().voices[0].paused);
        audio.pause_music(false);
        audio.stop_music();
        assert!(!mixer.lock().unwrap().playing(Channel::Music));
        // SPEC-008 §3.10: a music volume of 0 turns music off by volume,
        // stopping the track and the movie level; requests are recorded
        // and nothing plays. Setting the volume again turns it back on,
        // and the last recorded track replays.
        audio.request_music(vfs.clone(), request.clone());
        audio.wait_music();
        let silent = Volumes {
            music: 0,
            ..Volumes::default()
        };
        assert_eq!(audio.apply_volumes(silent), Some(false));
        assert!(!audio.music_enabled());
        assert!(!mixer.lock().unwrap().playing(Channel::Music));
        assert_eq!(mixer.lock().unwrap().movie_level(), 0);
        audio.request_music(vfs.clone(), MusicRequest::track("two", false));
        audio.wait_music();
        assert!(!mixer.lock().unwrap().playing(Channel::Music));
        audio.replay_music(vfs.clone());
        assert!(!mixer.lock().unwrap().playing(Channel::Music), "still off");
        assert_eq!(audio.apply_volumes(silent), None);
        assert_eq!(audio.apply_volumes(Volumes::default()), Some(true));
        assert_eq!(mixer.lock().unwrap().movie_level(), 20944);
        audio.replay_music(vfs.clone());
        audio.wait_music();
        assert!(mixer.lock().unwrap().playing(Channel::Music));
        assert!(!mixer.lock().unwrap().voices[0].looping);
        // Master volume 0 turns it off too; master set nonzero while the
        // music volume is 0 leaves it off.
        let muted = Volumes {
            master: 0,
            ..Volumes::default()
        };
        assert_eq!(audio.apply_volumes(muted), Some(false));
        assert!(!mixer.lock().unwrap().playing(Channel::Music));
        assert_eq!(audio.apply_volumes(silent), None);
        assert_eq!(audio.apply_volumes(Volumes::default()), Some(true));
        audio.replay_music(vfs.clone());
        audio.wait_music();
        // A stop while a track decodes keeps it from starting.
        audio.request_music(vfs.clone(), MusicRequest::track("three", true));
        audio.stop_music();
        audio.wait_music();
        assert!(!mixer.lock().unwrap().playing(Channel::Music));
        // A missing track leaves silence; the music volume applies at once
        // as `trunc(0.94 x music_volume)`, and the movie level with it.
        audio.request_music(vfs.clone(), request);
        audio.wait_music();
        audio.apply_volumes(Volumes {
            music: 16383,
            ..Volumes::default()
        });
        assert_eq!(mixer.lock().unwrap().level(Channel::Music), Some(15400));
        assert_eq!(mixer.lock().unwrap().movie_level(), 15400);
        audio.request_music(vfs, MusicRequest::track("missing", true));
        audio.wait_music();
        assert!(!mixer.lock().unwrap().playing(Channel::Music));
        // Without a mixer a request is only recorded.
        let silent = AudioHandle::default();
        silent.request_music(
            Arc::new(vault_formats::LocalDirProvider::mount(dir.path()).unwrap()),
            MusicRequest::track("one", true),
        );
        assert_eq!(silent.last_music().map(|r| r.looping), Some(true));
    }

    #[test]
    fn the_music_key_switches_music_off_for_the_whole_run() {
        // SPEC-008 §3.10 rule 1: `music=0` keeps music off whatever the
        // volumes; the movie level stays 0.
        let (audio, mixer) = AudioHandle::detached();
        audio.configure_music(false, Volumes::default());
        assert!(!audio.music_enabled());
        assert_eq!(mixer.lock().unwrap().movie_level(), 0);
        assert_eq!(audio.apply_volumes(Volumes::default()), None);
        assert!(!audio.music_enabled());
        // The default key at configured volumes of 0 starts off by volume.
        let (audio, mixer) = AudioHandle::detached();
        let muted = Volumes {
            master: 0,
            ..Volumes::default()
        };
        audio.configure_music(true, muted);
        assert!(!audio.music_enabled());
        assert_eq!(mixer.lock().unwrap().master(), 0);
        assert_eq!(audio.apply_volumes(Volumes::default()), Some(true));
        // Master volume does not scale the movie level.
        assert_eq!(mixer.lock().unwrap().movie_level(), 20944);
    }

    #[test]
    fn handles_without_a_mixer_are_silent() {
        let silent = AudioHandle::default();
        assert!(!silent.play(tone(4), FULL, Channel::Speech, false));
        assert!(!silent.playing(Channel::Speech));
        let (handle, mixer) = AudioHandle::detached();
        assert!(handle.play(tone(4), FULL, Channel::Speech, false));
        assert!(handle.playing(Channel::Speech));
        mixer.lock().unwrap().fill(&mut [0.0; 4], 1, MIXER_RATE);
        assert!(!handle.playing(Channel::Speech));
        assert!(Audio::silent().handle().mixer.is_none());
    }
}
