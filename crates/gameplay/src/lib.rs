//! Gothic Reforged quest and dialog engine.
//!
//! Pure Rust, no Bevy, deterministic. Loads the game's plain-text quest/info/string data
//! into an indexed [`db::GameDatabase`], keeps all mutable progress in a serializable
//! [`state::GameState`] (the save-game core), evaluates which dialog infos are available
//! ([`conditions`], [`dialog`]), executes info scripts ([`script`]) and statically checks
//! the whole quest graph ([`lint`]).
//!
//! Semantics that are not yet confirmed by decompiling `Game.dll` are isolated in
//! [`conditions`] and the quest-transition function in [`script`] and tagged `[I]`.
pub mod conditions;
pub mod db;
pub mod dialog;
pub mod focus;
pub mod inventory;
pub mod lint;
pub mod model;
pub mod mover;
pub mod questmgr;
pub mod script;
pub mod state;
pub mod stats;
pub mod trade;
pub mod transcript;
pub mod treasure;

pub use conditions::Assumptions;
pub use db::{GameDatabase, InfoId, QuestId};
pub use dialog::{Choice, DialogSession, Engine};
pub use state::GameState;
