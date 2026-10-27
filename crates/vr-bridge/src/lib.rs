//! Adapters between the VR physics layer and the game's own physics and runtime (issue VR-09).
//!
//! Two physics stacks coexist on purpose (see `docs/vr/PHYSICS.md`):
//!
//! * `gothic-physics` is the game's: a static triangle world (`CollisionWorld`, centimetres),
//!   the re-implemented Gothic 3 character controller, verlet ragdolls and crowd separation.
//!   Locomotion and the IK body of `gothic-vr` already run on it.
//! * avian3d drives what VR adds and Gothic 3 never simulated: physics hands with PD joints,
//!   grabbed props with mass, weapon-versus-body contacts, stab joints, arrows. It needs
//!   rigid-body dynamics, joints and a contact solver, which `gothic-physics` does not have.
//!
//! This crate keeps the two consistent instead of merging them:
//!
//! * [`units`]: centimetres / sim yaw <-> metres / Bevy yaw.
//! * [`level`]: the same level triangles go into the `CollisionWorld` and into avian static
//!   trimesh colliders, chunk by chunk.
//! * [`actors`]: every actor of the `gothic-runtime` core gets a kinematic avian proxy with
//!   melee hitzones that follows the core's position; VR melee hits on a proxy become the
//!   core's `Command::Attack`, so the original combat rules deal the damage.
//! * [`feed`]: the core's state as the view models of the VR windows and HUD (dialog options,
//!   quest log, character, trade, loot, recipes) and the window actions applied to the core.
//! * [`tick`]: ticks the core at its 30 Hz in `Update`, because `CorePlugin` sets the fixed
//!   timestep to 30 Hz while avian and the VR layer run their fixed steps at 90 Hz.
pub mod actors;
pub mod feed;
pub mod level;
pub mod tick;
pub mod units;

pub use actors::{ActorProxies, BridgeSettings, RuntimeActor};
pub use level::{LevelChunk, LevelChunkMessage};
pub use tick::CoreTickPlugin;

use bevy::prelude::*;

/// All adapters. `tick_core: true` adds [`CoreTickPlugin`] (use it instead of
/// `gothic_runtime::CorePlugin` in an app that also runs the VR physics).
pub struct VrBridgePlugin {
    pub tick_core: bool,
}

impl Default for VrBridgePlugin {
    fn default() -> Self {
        Self { tick_core: true }
    }
}

impl Plugin for VrBridgePlugin {
    fn build(&self, app: &mut App) {
        if self.tick_core {
            app.add_plugins(CoreTickPlugin);
        }
        app.add_plugins((level::LevelPlugin, actors::ActorsPlugin));
    }
}
