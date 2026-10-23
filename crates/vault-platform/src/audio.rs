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
use std::{
    collections::{BTreeMap, BTreeSet, VecDeque},
    sync::{Arc, Mutex, MutexGuard},
};
use vault_formats::{MAX_ASSET_BYTES, Vfs, acm, acm::MIXER_RATE};

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

/// The linear gain of a SPEC-014 §2.3 volume setting (0..32767). SPEC-010
/// §3.11 leaves the mapping to audible attenuation open; the engine uses the
/// one conversion the specs give, SPEC-012 §3.8's
/// `-1000 × log2(32767 / v)` hundredths of a decibel (clamped to -10000, so
/// 0 is silent), for every channel.
pub fn volume_gain(volume: i32) -> f32 {
    vault_core::movie::attenuation_gain(vault_core::movie::attenuation(volume))
}

/// SPEC-014 §3.18 the mixer's volume settings, 0..32767 each. The master
/// volume scales every voice except a movie, whose level SPEC-012 §3.8 fixes
/// from the music volume alone; the music volume scales the background
/// music; the effect and speech volumes apply when a sound starts.
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
    gain: f32,
    channel: Channel,
    looping: bool,
    paused: bool,
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
    gain: f32,
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
    /// SPEC-014 §3.18 the master level, applied to every voice and stream
    /// except a movie's.
    master: f32,
}
impl Default for Mixer {
    fn default() -> Self {
        Self {
            voices: Vec::new(),
            streams: Vec::new(),
            master: 1.0,
        }
    }
}
impl Mixer {
    /// Start a mono voice. Speech, music and library slots replace their
    /// previous voice; an effect beyond the limit is refused.
    pub fn play(
        &mut self,
        samples: Arc<[i16]>,
        gain: f32,
        channel: Channel,
        looping: bool,
    ) -> bool {
        self.play_frames(samples, 1, gain, channel, looping)
    }
    /// Start a voice of interleaved `channels`-wide frames (1 or 2) at the
    /// mixer rate.
    pub fn play_frames(
        &mut self,
        samples: Arc<[i16]>,
        channels: usize,
        gain: f32,
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
            gain,
            channel,
            looping,
            paused: false,
        });
        true
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
    /// Change the gain of what plays on `channel`.
    pub fn set_gain(&mut self, channel: Channel, gain: f32) {
        for voice in self.voices.iter_mut().filter(|v| v.channel == channel) {
            voice.gain = gain;
        }
        for stream in self.streams.iter_mut().filter(|s| s.channel == channel) {
            stream.gain = gain;
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
    pub fn set_master(&mut self, gain: f32) {
        self.master = gain;
    }
    /// Start an empty stream of `channels`-wide frames at `rate` Hz,
    /// replacing the channel's voice or stream. An underrun plays silence
    /// until more samples arrive.
    pub fn open_stream(&mut self, channel: Channel, rate: u32, channels: usize, gain: f32) -> bool {
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
            gain,
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
    pub fn master(&self) -> f32 {
        self.master
    }
    #[cfg(test)]
    pub fn paused(&self, channel: Channel) -> bool {
        self.voices
            .iter()
            .any(|voice| voice.channel == channel && voice.paused)
    }
    #[cfg(test)]
    pub fn gain(&self, channel: Channel) -> Option<f32> {
        self.voices
            .iter()
            .find(|voice| voice.channel == channel)
            .map(|voice| voice.gain)
    }
    #[cfg_attr(not(feature = "audio"), allow(dead_code))]
    /// Mix into interleaved `channels`-wide frames at `rate` Hz: every voice
    /// and stream is resampled linearly to `rate`, scaled by its gain (and
    /// the master level), and the sum is clamped to [-1, 1].
    pub fn fill(&mut self, out: &mut [f32], channels: usize, rate: u32) {
        out.fill(0.0);
        if channels == 0 || rate == 0 {
            return;
        }
        let level = |channel: Channel, master: f32| {
            if channel == Channel::Movie {
                1.0
            } else {
                master
            }
        };
        let step = (u64::from(MIXER_RATE) << 32) / u64::from(rate);
        for voice in &mut self.voices {
            if voice.paused {
                continue;
            }
            let frames = voice.frames();
            let gain = voice.gain * level(voice.channel, self.master);
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
            let gain = stream.gain * level(stream.channel, self.master);
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
    /// SPEC-008 §3.10: while music is disabled a request is only recorded.
    enabled: bool,
    /// The last request, recorded even while music is disabled.
    last: Option<MusicRequest>,
    pending: Option<PendingTrack>,
    next_token: u64,
    /// SPEC-012 §3.12 flag `0x08`: a movie paused the music.
    paused: bool,
    /// The music volume's gain.
    gain: f32,
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
        Self {
            enabled: true,
            last: None,
            pending: None,
            next_token: 0,
            paused: false,
            gain: 1.0,
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
        mixer.play_frames(samples, 2, self.gain, Channel::Music, looping);
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
    pub fn play(&self, samples: Arc<[i16]>, gain: f32, channel: Channel, looping: bool) -> bool {
        self.lock()
            .is_some_and(|mut mixer| mixer.play(samples, gain, channel, looping))
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
    pub fn open_stream(&self, channel: Channel, rate: u32, channels: usize, gain: f32) -> bool {
        self.lock()
            .is_some_and(|mut mixer| mixer.open_stream(channel, rate, channels, gain))
    }
    pub fn append(&self, channel: Channel, samples: &[i16]) -> bool {
        self.lock()
            .is_some_and(|mut mixer| mixer.append(channel, samples))
    }
    /// SPEC-014 §3.18: the master volume's level for every voice but a
    /// movie's.
    pub fn set_master(&self, volume: i32) {
        if let Some(mut mixer) = self.lock() {
            mixer.set_master(volume_gain(volume));
        }
    }
    /// SPEC-014 §3.18 volumes apply to the mixer at once: the master level
    /// and the music voice's level (effects and speech take theirs when
    /// they start).
    pub fn apply_volumes(&self, volumes: Volumes) {
        self.set_master(volumes.master);
        let gain = volume_gain(volumes.music);
        let mut music = lock(&self.music);
        music.gain = gain;
        if let Some(mut mixer) = self.lock() {
            mixer.set_gain(Channel::Music, gain);
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
        if music.enabled {
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
    /// SPEC-008 §3.10: disabling music silences it but keeps recording
    /// requests; enabling it again plays the last recorded track. (Nothing
    /// in the desktop shell disables music yet: this is where the world's
    /// music-disabled state reaches the mixer.)
    #[cfg_attr(not(test), allow(dead_code))]
    pub fn set_music_enabled(&self, vfs: Arc<dyn Vfs>, enabled: bool) {
        let mut music = lock(&self.music);
        if music.enabled == enabled {
            return;
        }
        music.enabled = enabled;
        if enabled {
            if let Some(last) = music.last.clone() {
                self.start_music(&mut music, vfs, last);
            }
        } else {
            music.pending = None;
            if let Some(mut mixer) = self.lock() {
                mixer.stop(Channel::Music);
            }
        }
    }
    pub fn music_enabled(&self) -> bool {
        lock(&self.music).enabled
    }
    /// Music on or off for a world that replays its own last request when
    /// music comes back (SPEC-008 §3.10, `WorldHost::set_music_enabled`):
    /// off stops the track as `set_music_enabled` does; on only clears the
    /// flag, so the world's replay is the one track that starts.
    pub fn follow_music_enabled(&self, enabled: bool) {
        let mut music = lock(&self.music);
        if music.enabled == enabled {
            return;
        }
        music.enabled = enabled;
        if !enabled {
            music.pending = None;
            if let Some(mut mixer) = self.lock() {
                mixer.stop(Channel::Music);
            }
        }
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
#[allow(dead_code)]
#[path = "../../../tests/fixtures/sound/support.rs"]
pub(crate) mod sound_fixture;

#[cfg(test)]
mod tests {
    use super::*;

    fn tone(length: usize) -> Arc<[i16]> {
        (0..length).map(|index| (index as i16) * 100).collect()
    }

    #[test]
    fn mixer_limits_effects_resamples_and_finishes_voices() {
        let mut mixer = Mixer::default();
        for _ in 0..EFFECT_VOICES {
            assert!(mixer.play(tone(8), 1.0, Channel::Effect, false));
        }
        // A fifth effect is refused; nothing is evicted.
        assert!(!mixer.play(tone(8), 1.0, Channel::Effect, false));
        assert_eq!(mixer.voices(), EFFECT_VOICES);
        // Speech and library slots have their own voices.
        assert!(mixer.play(tone(4), 0.5, Channel::Speech, false));
        assert!(mixer.play(tone(4), 0.5, Channel::Speech, false));
        assert!(mixer.play(tone(4), 1.0, Channel::Library(3), true));
        assert_eq!(mixer.voices(), EFFECT_VOICES + 2);
        mixer.stop(Channel::Effect);
        assert!(!mixer.playing(Channel::Effect) && mixer.playing(Channel::Speech));

        // At the mixer rate each output frame takes one source sample; the
        // stereo frame carries it on both channels.
        let mut mixer = Mixer::default();
        mixer.play(
            Arc::from([16384_i16, -16384, 8192]),
            1.0,
            Channel::Effect,
            false,
        );
        let mut out = [0.0; 8];
        mixer.fill(&mut out, 2, MIXER_RATE);
        assert_eq!(out, [0.5, 0.5, -0.5, -0.5, 0.25, 0.25, 0.0, 0.0]);
        assert_eq!(mixer.voices(), 0, "a finished voice is removed");
        // At twice the rate the voice is interpolated between samples.
        mixer.play(Arc::from([0_i16, 16384]), 0.5, Channel::Speech, false);
        let mut out = [0.0; 4];
        mixer.fill(&mut out, 1, 2 * MIXER_RATE);
        assert_eq!(out, [0.0, 0.125, 0.25, 0.25]);
        assert!(!mixer.playing(Channel::Speech));
        // Looping voices wrap; mixing clamps.
        mixer.play(Arc::from([32767_i16]), 1.0, Channel::Library(0), true);
        mixer.play(Arc::from([32767_i16]), 1.0, Channel::Effect, false);
        let mut out = [0.0; 3];
        mixer.fill(&mut out, 1, MIXER_RATE);
        assert_eq!(out[0], 1.0);
        assert!(out[1] > 0.99 && mixer.playing(Channel::Library(0)));
    }

    #[test]
    fn movie_streams_interpolate_and_resync_after_an_underrun() {
        let mut mixer = Mixer::default();
        assert!(!mixer.append(Channel::Movie, &[1]), "no stream yet");
        assert!(mixer.open_stream(Channel::Movie, MIXER_RATE, 2, 1.0));
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
        assert!(mixer.open_stream(Channel::Movie, MIXER_RATE / 2, 1, 0.5));
        mixer.append(Channel::Movie, &[16384, -16384]);
        let mut out = [0.0; 4];
        mixer.fill(&mut out, 1, MIXER_RATE);
        assert_eq!(out, [0.25, 0.0, -0.25, -0.25]);
        mixer.stop(Channel::Movie);
        assert!(!mixer.playing(Channel::Movie));
        assert!(!mixer.open_stream(Channel::Movie, 0, 2, 1.0));
        // The master level does not touch a movie (SPEC-012 §3.8).
        mixer.set_master(0.5);
        mixer.open_stream(Channel::Movie, MIXER_RATE, 1, 1.0);
        mixer.append(Channel::Movie, &[16384]);
        let mut out = [0.0; 1];
        mixer.fill(&mut out, 1, MIXER_RATE);
        assert_eq!(out, [0.5]);
    }

    #[test]
    fn stereo_voices_play_frames_at_the_mixer_rate() {
        // G-05 / SPEC-010 §3.6: music is interleaved stereo at the mixer
        // rate, one output frame per source frame (not half speed).
        let mut mixer = Mixer::default();
        let music: Arc<[i16]> = Arc::from([16384_i16, -16384, 8192, 0]);
        assert!(mixer.play_frames(music.clone(), 2, 1.0, Channel::Music, false));
        let mut out = [0.0; 6];
        mixer.fill(&mut out, 2, MIXER_RATE);
        assert_eq!(out, [0.5, -0.5, 0.25, 0.0, 0.0, 0.0]);
        assert!(!mixer.playing(Channel::Music), "two frames, two outputs");
        // At twice the rate each channel is interpolated on its own; mono
        // output takes the mean.
        mixer.play_frames(music.clone(), 2, 1.0, Channel::Music, false);
        let mut out = [0.0; 4];
        mixer.fill(&mut out, 2, 2 * MIXER_RATE);
        assert_eq!(out, [0.5, -0.5, 0.375, -0.25]);
        mixer.play_frames(music.clone(), 2, 1.0, Channel::Music, false);
        let mut mono = [0.0; 1];
        mixer.fill(&mut mono, 1, MIXER_RATE);
        assert_eq!(mono, [0.0]);
        assert!(!mixer.play_frames(Arc::from([1_i16]), 2, 1.0, Channel::Music, false));
        assert!(!mixer.play_frames(music.clone(), 3, 1.0, Channel::Music, false));
        // Gains, the master level and pauses apply to what plays.
        mixer.play_frames(music, 2, 1.0, Channel::Music, true);
        mixer.set_gain(Channel::Music, 0.5);
        mixer.set_master(0.5);
        let mut out = [0.0; 2];
        mixer.fill(&mut out, 2, MIXER_RATE);
        assert_eq!(out, [0.125, -0.125]);
        mixer.set_paused(Channel::Music, true);
        mixer.fill(&mut out, 2, MIXER_RATE);
        assert_eq!(out, [0.0, 0.0]);
        mixer.set_paused(Channel::Music, false);
        mixer.fill(&mut out, 2, MIXER_RATE);
        assert_eq!(out, [0.0625, 0.0], "a paused voice resumes in place");
    }

    #[test]
    fn volume_settings_map_through_the_movie_attenuation() {
        assert_eq!(volume_gain(32767), 1.0);
        assert!(volume_gain(0) < 1e-4);
        // 22281, the default, is -556 hundredths of a decibel.
        assert!((volume_gain(22281) - 10_f32.powf(-556.0 / 2000.0)).abs() < 1e-6);
        assert_eq!(Volumes::default().music, 22281);
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
        // Disabled music records the request and plays nothing; enabling
        // plays the last recorded track.
        audio.set_music_enabled(vfs.clone(), false);
        assert!(!audio.music_enabled());
        audio.request_music(vfs.clone(), MusicRequest::track("two", false));
        audio.wait_music();
        assert!(!mixer.lock().unwrap().playing(Channel::Music));
        audio.set_music_enabled(vfs.clone(), true);
        audio.wait_music();
        assert!(mixer.lock().unwrap().playing(Channel::Music));
        assert!(!mixer.lock().unwrap().voices[0].looping);
        // A stop while a track decodes keeps it from starting.
        audio.request_music(vfs.clone(), MusicRequest::track("three", true));
        audio.stop_music();
        audio.wait_music();
        assert!(!mixer.lock().unwrap().playing(Channel::Music));
        // A missing track leaves silence; the music volume applies.
        audio.request_music(vfs.clone(), request);
        audio.wait_music();
        audio.apply_volumes(Volumes {
            music: 16383,
            ..Volumes::default()
        });
        assert_eq!(
            mixer.lock().unwrap().gain(Channel::Music),
            Some(volume_gain(16383))
        );
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
    fn handles_without_a_mixer_are_silent() {
        let silent = AudioHandle::default();
        assert!(!silent.play(tone(4), 1.0, Channel::Speech, false));
        assert!(!silent.playing(Channel::Speech));
        let (handle, mixer) = AudioHandle::detached();
        assert!(handle.play(tone(4), 1.0, Channel::Speech, false));
        assert!(handle.playing(Channel::Speech));
        mixer.lock().unwrap().fill(&mut [0.0; 4], 1, MIXER_RATE);
        assert!(!handle.playing(Channel::Speech));
        assert!(Audio::silent().handle().mixer.is_none());
    }
}
