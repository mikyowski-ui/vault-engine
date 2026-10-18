//! Desktop audio: a small mixer for SPEC-010 effects, talking-head speech,
//! and SPEC-005 library sounds, played on the default output device through
//! cpal (feature `audio`). Effects and speech are mono 16-bit at the 22050 Hz
//! mixer rate whatever their ACM headers declare. At most four effects play at
//! once; a fifth request fails without evicting one.
use std::sync::{Arc, Mutex, MutexGuard};
use vault_formats::acm::MIXER_RATE;

/// SPEC-010 §3.9 active effect limit.
pub const EFFECT_VOICES: usize = 4;

/// What a voice belongs to: effects share the four-voice limit, speech has
/// one voice, and each library slot has its own.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Channel {
    Effect,
    Speech,
    Library(usize),
}

// Without a device (feature `audio` off) only tests drain the mixer.
#[cfg_attr(not(feature = "audio"), allow(dead_code))]
struct Voice {
    samples: Arc<[i16]>,
    /// Source position in 32.32 fixed point.
    position: u64,
    gain: f32,
    channel: Channel,
    looping: bool,
}

/// The voices currently playing.
#[derive(Default)]
pub struct Mixer {
    voices: Vec<Voice>,
}
impl Mixer {
    /// Start a voice. Speech and library slots replace their previous
    /// voice; an effect beyond the limit is refused.
    pub fn play(
        &mut self,
        samples: Arc<[i16]>,
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
        if samples.is_empty() {
            return false;
        }
        self.voices.push(Voice {
            samples,
            position: 0,
            gain,
            channel,
            looping,
        });
        true
    }
    pub fn stop(&mut self, channel: Channel) {
        self.voices.retain(|voice| voice.channel != channel);
    }
    pub fn playing(&self, channel: Channel) -> bool {
        self.voices.iter().any(|voice| voice.channel == channel)
    }
    #[cfg(test)]
    pub fn voices(&self) -> usize {
        self.voices.len()
    }
    #[cfg_attr(not(feature = "audio"), allow(dead_code))]
    /// Mix into interleaved `channels`-wide frames at `rate` Hz: every voice
    /// is resampled linearly from the mixer rate and copied to all output
    /// channels; the sum is clamped to [-1, 1].
    pub fn fill(&mut self, out: &mut [f32], channels: usize, rate: u32) {
        out.fill(0.0);
        if channels == 0 || rate == 0 {
            return;
        }
        let step = (u64::from(MIXER_RATE) << 32) / u64::from(rate);
        for voice in &mut self.voices {
            let length = voice.samples.len() as u64;
            for frame in out.chunks_mut(channels) {
                let mut index = voice.position >> 32;
                if index >= length {
                    if !voice.looping {
                        break;
                    }
                    voice.position %= length << 32;
                    index = voice.position >> 32;
                }
                let first = f32::from(voice.samples[index as usize]);
                let next = voice
                    .samples
                    .get(index as usize + 1)
                    .map_or(first, |sample| f32::from(*sample));
                let fraction = (voice.position & 0xffff_ffff) as f32 / 4_294_967_296.0;
                let value = (first + (next - first) * fraction) / 32768.0 * voice.gain;
                for sample in frame {
                    *sample += value;
                }
                voice.position += step;
            }
        }
        self.voices
            .retain(|voice| voice.looping || voice.position >> 32 < voice.samples.len() as u64);
        for sample in out {
            *sample = sample.clamp(-1.0, 1.0);
        }
    }
}

/// The mixer shared between the main thread and the device callback.
type SharedMixer = Arc<Mutex<Mixer>>;

/// A shareable handle to the mixer; without one, audio is silent and every
/// request reports that nothing plays.
#[derive(Clone, Default)]
pub struct AudioHandle {
    mixer: Option<SharedMixer>,
}
impl AudioHandle {
    /// A handle whose mixer no device drains (tests drive `fill` directly).
    #[cfg(test)]
    pub fn detached() -> (Self, SharedMixer) {
        let mixer = Arc::new(Mutex::new(Mixer::default()));
        (
            Self {
                mixer: Some(mixer.clone()),
            },
            mixer,
        )
    }
    fn lock(&self) -> Option<MutexGuard<'_, Mixer>> {
        self.mixer.as_ref().map(|mixer| {
            mixer
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner())
        })
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
                handle: AudioHandle { mixer: Some(mixer) },
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
