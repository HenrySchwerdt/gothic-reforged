//! Gameplay camera of Gothic 3 (engine port of `gCCameraAI_PS`, batch E00).
//!
//! Bevy-free and deterministic: feed [`CameraInput`] every frame, read [`CameraOutput`]. Presets
//! come from `Camera_PC_Camera.tple` ([`PresetTable::from_tple`]), tuning from `ge3.INI`
//! ([`CameraConfig::from_ini`]). Units are centimetres, Y up, the camera looks along local -Z.
pub mod aim;
pub mod config;
pub mod dialog;
pub mod math;
pub mod preset;
pub mod state;
pub mod world;

pub use config::CameraConfig;
pub use dialog::DialogActor;
pub use preset::{CameraMode, CameraPreset, PresetKey, PresetTable};
pub use state::{CameraInput, CameraOutput, CameraState, HeroView};
pub use world::{CameraWorld, NoWorld};
