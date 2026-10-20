//! Gothic 3 audio: format support (Ogg/WAV decode, music table, sound templates), voice-over
//! lookup by dialog string key, and a Bevy plugin on top of `bevy_audio`.
//!
//! Layout of the original data is documented in `docs/research/AUDIO_FORMATS.md`.
pub mod library;
pub mod mix;
pub mod mul;
pub mod music;
pub mod pcm;
pub mod plugin;
pub mod sfx;
pub mod speech;

pub use library::AudioLibrary;
pub use mix::Category;
pub use music::{Fragment, MusicLibrary};
pub use plugin::{
    AudioAssets, AudioSettings, GothicAudioPlugin, MusicCommand, MusicState, MusicTrack, PlaySfx,
    PlayVoice, VoiceFinished, VoiceLine, VoiceMissing, VoiceStarted, headless_app,
};
pub use speech::{SpeechIndex, SpeechLanguage, VoiceFile};
