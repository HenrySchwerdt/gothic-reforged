//! Humanoid opponents for VR (issue VR-05).
//!
//! * [`ragdoll`]: Bevy-free active ragdoll (bones, joint limits, pose-matching muscles,
//!   balance, stagger / fall / get-up, hits, pushes, grabs).
//! * [`ai`]: Bevy-free melee combat AI (telegraphed directional attacks, attack slots,
//!   distance keeping, blocking with the physical weapon, recoil).
//! * [`animator`]: target poses from the AI's intent via the generic full-body IK.
//! * [`plugin`]: Bevy/avian integration (bone colliders, hitzones, physical weapon, hit and
//!   block reactions, pushes and grabs by the player's physics hands).
pub mod ai;
pub mod animator;
pub mod plugin;
pub mod ragdoll;

pub use plugin::{NpcBone, NpcSlots, VrNpc, VrNpcPlugin, spawn_npc};
