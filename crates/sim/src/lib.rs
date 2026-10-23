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
//! * Inferred (tagged `[I]` in place): move durations of the unshown NPCs
//!   ([`sim::move_duration_ms`]), stagger lock times, FOV, `CurrentLevel`'s second term,
//!   the Orc/Assassin chapter switch, melee parry needing stamina.
//! * Magic and ranged weapons run on the ported script pipeline ([`script_magic`]): spell
//!   templates, projectiles in flight, area spells, summons, the hero's cast and bow states.
//! * Stubbed: `NotifyEnclave` modes, revolution
//!   attitude, bribe price, sneaking reactions, zone reactions, burn / poison damage rates,
//!   routine-name resolution (the [`routine::RoutineResolver`] hook), arena.
pub use gothic_script as script;

pub mod actor;
pub mod attitude;
pub mod bark;
pub mod brain;
pub mod combat;
pub mod config;
pub mod crime;
pub mod data;
pub mod economy;
pub mod item;
pub mod moves;
pub mod perception;
pub mod persist;
pub mod rng;
pub mod routine;
pub mod script_host;
pub mod script_magic;
pub mod script_player;
pub mod shots;
pub mod sim;
pub mod trade;
pub mod types;
pub mod vitals;
pub mod world;

pub use actor::Actor;
pub use combat::{Attack, HitOutcome};
pub use config::SimConfig;
pub use sim::Sim;
pub use types::ActorId;
