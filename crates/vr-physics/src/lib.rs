//! Physics for VR (issue VR-03): avian3d rigid bodies, Boneworks-style physics hands that
//! follow the tracked controllers with strength-limited PD controllers, grip points,
//! two-handed grabs and weighted procedural test props.
//!
//! Engine choice: **avian3d 0.8** (Bevy 0.20). It is ECS-native (bodies, colliders and joints
//! are plain components, so other plugins can query/extend them without a handle mapping
//! layer), ships fixed/prismatic/spherical joints and swept CCD needed for hands, grips, stabs
//! and arrows, and runs in Bevy's fixed schedule. bevy_rapier3d 0.37 would also work with
//! Bevy 0.20; see `docs/vr/PHYSICS.md` for the comparison.
//!
//! * [`pd`]: Bevy-free PD controllers and the strength model.
//! * [`grab`]: Bevy-free grip selection.
//! * [`props`]: procedural weapons/boxes.
//! * [`hands`]: the Bevy/avian plugin.
//! * [`testing`]: a headless, deterministic test rig.
pub mod grab;
pub mod hands;
pub mod pd;
pub mod props;
pub mod testing;

pub use avian3d;
pub use hands::{
    GrabMessage, Grabbable, HandSet, Held, PhysicsHand, Prop, VrLayer, VrPhysicsPlugin, VrStrength,
    prop_layers, spawn_prop, spawn_static_greybox, static_box,
};
