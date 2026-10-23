//! Physical melee for VR (issue VR-04): Blade & Sorcery-style contacts between physics weapons
//! and body hitzones.
//!
//! * [`contact`]: the Bevy-free contact model (swept sub-stepping, velocity threshold, edge
//!   alignment, stab/stick/pull-out, block/parry) producing [`contact::HitEvent`]s.
//! * [`profiles`]: strikers of the procedural test weapons.
//! * [`plugin`]: Bevy/avian integration (weapons, hitzone sets, deflection impulses, stab
//!   joints, player hitzones from the IK body).
pub mod contact;
pub mod geom;
pub mod plugin;
pub mod profiles;

pub use contact::{Bone, ClashKind, DamageType, HitEvent, Hitzone, MeleeEvent};
pub use plugin::{
    ClashMessage, HitMessage, HitzoneSet, Melee, MeleeSet, MeleeWeapon, PASS_AFTER_PULL,
    PlayerAvatar, PlayerHitzonesPlugin, StabMessage, VrMeleePlugin, WOUND_DRAG, body_layers,
    dummy_zones, hitzones_from_body, spawn_dummy, zone_collider,
};
