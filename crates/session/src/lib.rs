//! What every host does at the start of a new game before the first tick: hand the core the world's interact objects,
//! loose items, recipes and spells, the sector layers, and every character its routine points. The data sits in the
//! user's own exports ([`data`]); the functions here turn it into [`Command`]s. Used by `apps/game` and `apps/quest`
//! (hosts keep only their own bookkeeping: entities, visuals, coordinate frames).
pub mod content;
pub mod data;

pub use content::{ContentSet, Placed, content_commands, routine_command, sector_commands, sector_name};
