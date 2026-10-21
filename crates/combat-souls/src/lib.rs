//! Opt-in "Souls-style" combat for Gothic Reforged.
//!
//! This crate changes only the *flow* of a fight: stamina-gated rolls with i-frames and
//! backsteps, light / heavy chains with an input buffer and cancel windows, guard with parry and
//! riposte, poise and stagger, hit-stop, lock-on, and enemies that telegraph their attacks.
//! How hard a connected swing hurts is always answered by the original ported damage pipeline
//! through [`sink::HitSink`] (see [`sink::SimSink`]), so stats, weapons and armour stay Gothic.
//! The classic combat remains the default; this mode is a replaceable mechanic behind
//! `--combat souls`.
//!
//! The core (everything except the `plugin` feature) is Bevy-free, deterministic and stepped at
//! a fixed 60 Hz, so it is tested headlessly. Units: centimetres, Y up, Gothic yaw convention
//! (see [`math`]).
pub mod actions;
pub mod animmap;
#[cfg(feature = "pack")]
pub mod pack;
pub mod events;
pub mod foe;
pub mod hero;
pub mod lockcam;
pub mod lockon;
pub mod math;
pub mod sink;
pub mod tuning;
pub mod world;

#[cfg(feature = "plugin")]
pub mod plugin;

#[cfg(test)]
mod tests;

pub use actions::{ActionId, Dir, Hurt, StrikeKind};
pub use events::{Event, FeedbackCause, HitResult};
pub use foe::{Foe, FoeProfile, FoeState};
pub use hero::{Hero, Input};
pub use sink::{HitSink, SimSink, SimpleSink};
pub use tuning::{FeedbackTuning, Impact, Shake, Tuning};
pub use world::{Env, OpenField, SoulsWorld};
