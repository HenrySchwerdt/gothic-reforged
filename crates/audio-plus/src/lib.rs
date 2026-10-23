//! Opt-in enhanced audio for Gothic Reforged, on top of `gothic-audio` (which reproduces the
//! original FMOD mix and stays the default):
//!
//! * [`hrtf`]: spherical-head HRTF (ITD, head shadow, pinna notch, front/back);
//! * [`occlusion`]: ray-probe occlusion / obstruction -> low-pass and gain;
//! * [`reverb`]: Freeverb-style reverb with presets and blended reverb zones;
//! * [`voice::EnhancedVoice`]: the chain for one source, switchable per feature
//!   ([`voice::EnhancedSettings`], all off = the faithful equal-power pan).
//!
//! The DSP is plain deterministic f32 code without allocation while processing; the `plugin`
//! feature wraps it as a `bevy_audio` source, and [`bridge`] switches `gothic-audio`'s
//! positional channels onto it. See `docs/AUDIO_ENHANCED.md`.
#[cfg(feature = "plugin")]
pub mod bridge;
pub mod dsp;
pub mod hrtf;
pub mod occlusion;
#[cfg(feature = "plugin")]
pub mod plugin;
pub mod reverb;
pub mod voice;
pub mod wav;

pub use hrtf::Direction;
pub use occlusion::{Blocking, OcclusionTuning, probe};
pub use reverb::{Preset, Reverb, ReverbParams, ReverbZone, Shape, zone_params};
pub use voice::{EnhancedSettings, EnhancedVoice, VoiceParams};
