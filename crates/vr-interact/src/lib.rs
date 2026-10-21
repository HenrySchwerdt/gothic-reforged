//! VR interactions mapped onto the game core's [`gothic_runtime::Command`]s (issue VR-11).
//!
//! The classic game logic stays the single authority: VR only changes *how* the player
//! expresses an intent. Pointing with the hand and pulling the trigger talks or uses
//! (`Confirm`, `Craft`), grabbing picks up, loots or opens (`PickUp`, `QuickLoot`,
//! `UseInteractObject`), turning a lockpick in the lock plays the lock (`Lockpick`), bringing
//! a potion to the mouth drinks it (`UseItem`), the off hand selects a spell and the casting
//! hand aims and casts it (`SelectSpell`, `CastSpell`, `AbortCast`), drawing the string
//! readies, aims and shoots the bow (`ReadyRanged`, `AimRanged`, `ReleaseRanged`), and the
//! real body crouches, dodges and jumps (`ToggleSneak`, `Dodge`, `Jump`).
//!
//! * [`targets`]: what the host registered in reach (NPCs, items, bodies, objects, recipes)
//!   and the pointing / grabbing queries.
//! * [`mapper`]: the Bevy-free [`InteractMapper`] that turns tracked poses and buttons into
//!   commands.
//! * [`lockpick`]: the physical lock-turning mini-game.
//! * [`core_api`]: the [`CoreApi`] trait (implemented for the real `Core`) and a stub core
//!   for demos and tests.
//! * [`plugin`]: Bevy integration (`VrInputRes` in, `CoreCommand` messages out).
pub mod core_api;
pub mod lockpick;
pub mod mapper;
pub mod plugin;
pub mod targets;

pub use core_api::{CoreApi, StubCore};
pub use lockpick::{LockSpec, Turn};
pub use mapper::{BowState, HeroState, InteractMapper, InteractSettings, Tracked};
pub use plugin::{InteractHero, InteractMapperRes, InteractTargets, VrInteractPlugin};
pub use targets::{Target, TargetKind};
