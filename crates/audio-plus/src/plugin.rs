//! Bevy integration: an [`EnhancedSound`] asset plays through the enhanced chain as a
//! `bevy_audio` source; [`EnhancedEmitter`] entities get their direction, faithful distance
//! gain (`gothic_audio::mix::inverse_gain`), occlusion (through the host's
//! [`OcclusionProbe`]) and the reverb of the listener's zone every frame.
//!
//! Nothing changes unless the host spawns enhanced sounds and turns
//! [`EnhancedAudio::settings`]`.enabled` on; the faithful mix of `gothic-audio` stays the
//! default.
use crate::occlusion::{Blocking, probe};
use crate::reverb::{ReverbParams, ReverbZone, zone_params};
use crate::voice::{EnhancedSettings, EnhancedVoice, VoiceParams};
use bevy::audio::{AddAudioSource, ChannelCount, Decodable, SampleRate, Source, SpatialListener};
use bevy::prelude::*;
use bevy::reflect::TypePath;
use std::num::NonZero;
use std::sync::{Arc, RwLock};
use std::time::Duration;

/// Global switches and the zone list.
#[derive(Resource, Clone)]
pub struct EnhancedAudio {
    pub settings: EnhancedSettings,
    pub zones: Vec<ReverbZone>,
    /// Reverb outside every zone.
    pub outdoor: ReverbParams,
    /// Ray spread of the occlusion probe (m).
    pub probe_spread: f32,
}

impl Default for EnhancedAudio {
    fn default() -> Self {
        Self {
            settings: EnhancedSettings::default(),
            zones: Vec::new(),
            outdoor: crate::reverb::Preset::Outdoor.params(),
            probe_spread: 0.6,
        }
    }
}

/// Answers "is the segment from `a` to `b` blocked by level geometry" (e.g. a physics
/// raycast). Without one, nothing is ever occluded.
#[derive(Resource)]
pub struct OcclusionProbe(pub Box<dyn Fn(Vec3, Vec3) -> bool + Send + Sync>);

/// Decoded mono samples of a sound plus the parameters its voice reads (one instance per
/// playing emitter, see [`EnhancedSound::instance`]).
#[derive(Asset, TypePath, Clone)]
pub struct EnhancedSound {
    pub samples: Arc<[f32]>,
    pub sample_rate: u32,
    pub looped: bool,
    params: Arc<RwLock<VoiceParams>>,
    settings: Arc<RwLock<EnhancedSettings>>,
}

impl EnhancedSound {
    /// From interleaved PCM (mixed down to mono).
    pub fn from_pcm(pcm: &gothic_audio::pcm::Pcm, looped: bool) -> Self {
        let ch = usize::from(pcm.channels.max(1));
        let samples: Vec<f32> = pcm
            .samples
            .chunks(ch)
            .map(|f| f.iter().map(|&s| f32::from(s) / 32768.0).sum::<f32>() / ch as f32)
            .collect();
        Self {
            samples: samples.into(),
            sample_rate: pcm.sample_rate.max(1),
            looped,
            params: Arc::default(),
            settings: Arc::default(),
        }
    }

    /// Decode Ogg Vorbis or WAV bytes.
    pub fn decode(bytes: &[u8], looped: bool) -> anyhow::Result<Self> {
        Ok(Self::from_pcm(&gothic_audio::pcm::decode(bytes)?, looped))
    }

    /// The parameters the plugin writes for this sound's voice.
    /// A copy sharing the samples but with its own voice parameters. The parameters live on
    /// the asset, so every emitter that plays at the same time needs its own instance.
    pub fn instance(&self) -> Self {
        Self {
            params: Arc::default(),
            settings: Arc::default(),
            ..self.clone()
        }
    }

    pub fn params(&self) -> Arc<RwLock<VoiceParams>> {
        self.params.clone()
    }
}

/// Faithful distance model of the emitter (FMOD inverse rolloff, metres).
#[derive(Component, Clone, Copy, Debug)]
pub struct EnhancedEmitter {
    pub min_distance: f32,
    pub max_distance: f32,
    pub rolloff: f32,
}

impl Default for EnhancedEmitter {
    fn default() -> Self {
        Self {
            min_distance: 1.0,
            max_distance: 50.0,
            rolloff: 1.0,
        }
    }
}

/// The stereo source the decoder hands to `bevy_audio`.
pub struct EnhancedSource {
    sound: EnhancedSound,
    voice: EnhancedVoice,
    pos: usize,
    block_in: Vec<f32>,
    block_out: Vec<f32>,
    out_pos: usize,
    finished: bool,
}

const BLOCK: usize = 256;

impl Iterator for EnhancedSource {
    type Item = f32;

    fn next(&mut self) -> Option<f32> {
        if self.out_pos >= self.block_out.len() {
            if self.finished {
                return None;
            }
            self.block_in.clear();
            let n = self.sound.samples.len();
            while self.block_in.len() < BLOCK {
                if self.pos >= n {
                    if self.sound.looped && n > 0 {
                        self.pos = 0;
                    } else {
                        break;
                    }
                }
                self.block_in.push(self.sound.samples[self.pos]);
                self.pos += 1;
            }
            if self.block_in.is_empty() {
                self.finished = true;
                return None;
            }
            let params = *self.sound.params.read().unwrap_or_else(|p| p.into_inner());
            let settings = *self
                .sound
                .settings
                .read()
                .unwrap_or_else(|p| p.into_inner());
            self.block_out.resize(self.block_in.len() * 2, 0.0);
            self.voice
                .process(&self.block_in, &mut self.block_out, &params, &settings);
            self.out_pos = 0;
        }
        let s = self.block_out[self.out_pos];
        self.out_pos += 1;
        Some(s)
    }
}

impl Source for EnhancedSource {
    fn current_span_len(&self) -> Option<usize> {
        None
    }

    fn channels(&self) -> ChannelCount {
        NonZero::<u16>::new(2).unwrap_or(NonZero::<u16>::MIN)
    }

    fn sample_rate(&self) -> SampleRate {
        NonZero::new(self.sound.sample_rate).unwrap_or(NonZero::<u32>::MIN)
    }

    fn total_duration(&self) -> Option<Duration> {
        None
    }
}

impl Decodable for EnhancedSound {
    type Decoder = EnhancedSource;

    fn decoder(&self) -> EnhancedSource {
        EnhancedSource {
            voice: EnhancedVoice::new(self.sample_rate as f32),
            sound: self.clone(),
            pos: 0,
            block_in: Vec::with_capacity(BLOCK),
            block_out: Vec::with_capacity(BLOCK * 2),
            out_pos: 0,
            finished: false,
        }
    }
}

pub struct EnhancedAudioPlugin;

impl Plugin for EnhancedAudioPlugin {
    fn build(&self, app: &mut App) {
        app.add_audio_source::<EnhancedSound>()
            .init_resource::<EnhancedAudio>()
            .add_systems(PostUpdate, update_voices);
    }
}

/// Listener space: x right, y up, z forward.
fn to_listener(listener: &GlobalTransform, p: Vec3) -> (Vec3, f32) {
    let local = listener.affine().inverse().transform_point3(p);
    (Vec3::new(local.x, local.y, -local.z), local.length())
}

fn update_voices(
    audio: Res<EnhancedAudio>,
    probe_fn: Option<Res<OcclusionProbe>>,
    listener: Query<&GlobalTransform, With<SpatialListener>>,
    sounds: Res<Assets<EnhancedSound>>,
    emitters: Query<(
        &GlobalTransform,
        &EnhancedEmitter,
        &AudioPlayer<EnhancedSound>,
    )>,
) {
    let Some(lis) = listener.iter().next() else {
        return;
    };
    let reverb = zone_params(lis.translation().to_array(), &audio.zones, audio.outdoor);
    for (tf, em, player) in &emitters {
        let Some(sound) = sounds.get(&player.0) else {
            continue;
        };
        let (dir, dist) = to_listener(lis, tf.translation());
        let blocking = match (
            &probe_fn,
            audio.settings.enabled && audio.settings.occlusion,
        ) {
            (Some(p), true) => probe(
                lis.translation().to_array(),
                tf.translation().to_array(),
                audio.probe_spread,
                &|a, b| (p.0)(Vec3::from_array(a), Vec3::from_array(b)),
            ),
            _ => Blocking::default(),
        };
        let params = VoiceParams {
            direction: dir.to_array(),
            distance_gain: gothic_audio::mix::inverse_gain(
                dist,
                em.min_distance,
                em.max_distance,
                em.rolloff,
            ),
            blocking,
            reverb,
        };
        *sound.params.write().unwrap_or_else(|p| p.into_inner()) = params;
        // The global switches reach the audio thread through the sound.
        *sound.settings.write().unwrap_or_else(|p| p.into_inner()) = audio.settings;
    }
}
