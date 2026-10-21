//! Headless quest bot (TOOL-01).
//!
//! Drives the Bevy-free quest/dialog core (`gothic-gameplay`: [`GameDatabase`],
//! [`GameState`], the dialog [`gothic_gameplay::Engine`] and the quest-manager world hooks)
//! through quests like a player would: it plans a route through the info/quest graph
//! ([`plan`]), talks to NPCs, satisfies item / kill / defeat / area objectives with recorded
//! cheats, records a trace and, when nothing makes progress any more, reports every stuck
//! quest with the reason ([`report`]). Format: `docs/QUESTBOT.md`.
//!
//! Developed against the synthetic ten-quest database in [`fixture`]; on a machine with the
//! game, `cargo run -p gothic-questbot --release -- <Data dir>` plays the real quests.
//!
//! [`GameDatabase`]: gothic_gameplay::GameDatabase
//! [`GameState`]: gothic_gameplay::GameState
pub mod bot;
pub mod fixture;
pub mod plan;
pub mod report;

pub use bot::{BotOptions, QuestBot, play};
pub use report::{BotReport, StuckQuest, StuckReason};
