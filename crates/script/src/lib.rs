//! Port of the Gothic 3 `Script_Game.dll` behaviour over engine-interface traits.
//!
//! The crate is Bevy-free and deterministic: every function takes `&mut dyn ScriptHost` (see
//! [`host`]) and draws randomness only through [`host::ScriptCtx::random`]. The simulation crate
//! implements the host traits; [`testhost::TestWorld`] is an in-memory implementation for tests.
//!
//! Ported so far (see `docs/research/SCRIPT_PORT_PLAN.md` for per-batch status):
//!
//! * Phase 0: [`helpers`] (B00), [`stats`] (B01), [`predicates`] (B02).
//! * Phase 1: [`routine`] (B03 routine engine and party, B04 work stations, B05 idle, social,
//!   guard, sleep and ambient tasks).
//!
//! Every ported function carries a `Port of <RegistryName> (S:<addr>)` doc line; `S:` addresses
//! are relative virtual addresses in the original DLL. No decompiled code is included: the
//! behaviour was re-derived and re-expressed as data tables and explicit state machines.

pub mod assess;
pub mod attitude;
pub mod combat;
pub mod config;
pub mod crime;
pub mod damage;
pub mod down;
pub mod enums;
pub mod helpers;
pub mod host;
pub mod perception;
pub mod predicates;
pub mod routine;
pub mod stats;
pub mod testhost;
