//! Gothic Reforged gameplay simulation: deterministic, engine-independent, headless-testable.
//!
//! Rules cite `docs/research/COMBAT_AI_RULES.md` as `[CA x.y]` (and `GAME_RULES.md` as
//! `[GAME_RULES x.y]`); `[I]` marks inferred behaviour, `[U]`/stubs are listed in the module
//! docs. The default [`config::SimConfig`] is the shipped `ge3.INI`.
//!
//! # Coverage
//!
//! * Implemented from read instructions: damage pipeline ([`combat`]), NPC attributes from
//!   level ([`actor`]), protection, kill XP, parry / stagger classes, weakness table, fall damage,
//!   regeneration, trade prices, pickpocketing, armor fame thresholds ([`vitals`], [`economy`]),
//!   political / species attitudes ([`attitude`]), perception ranges ([`perception`]), crime
//!   bookkeeping ([`crime`]), time-of-day routines ([`routine`]).
//! * Inferred (tagged `[I]` in place): per-move scores and move durations ([`tactics`],
//!   [`sim::move_duration_ms`]), stagger lock times, run speed, FOV, `CurrentLevel`'s second term,
//!   the Orc/Assassin chapter switch, melee parry needing stamina.
//! * Stubbed: NPC spell casting AI (the `cast` API exists), `NotifyEnclave` modes, revolution
//!   attitude, bribe price, sneaking reactions, zone reactions, burn / poison damage rates,
//!   routine-name resolution (the [`routine::RoutineResolver`] hook), arena.
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
pub mod script_host;
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
