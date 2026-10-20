//! Gothic Reforged gameplay simulation: deterministic, engine-independent, headless-testable.
//!
//! Rules cite `docs/research/COMBAT_AI_RULES.md` as `[CA x.y]` (and `GAME_RULES.md` as
//! `[GAME_RULES x.y]`); `[I]` marks inferred behaviour, `[U]`/stubs are listed in the module
//! docs. The default [`config::SimConfig`] is the shipped `ge3.INI`.
pub mod actor;
pub mod attitude;
pub mod brain;
pub mod combat;
pub mod config;
pub mod crime;
pub mod data;
pub mod economy;
pub mod item;
pub mod perception;
pub mod rng;
pub mod routine;
pub mod sim;
pub mod tactics;
pub mod types;
pub mod vitals;
pub mod world;

pub use actor::Actor;
pub use combat::{Attack, HitOutcome, resolve_hit};
pub use config::SimConfig;
pub use sim::Sim;
pub use types::ActorId;
