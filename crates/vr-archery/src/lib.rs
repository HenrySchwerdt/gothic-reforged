//! Physical archery for VR (issue VR-06).
//!
//! * [`bow`]: Bevy-free archer state machine (quiver over the shoulder, nocking, draw power,
//!   release), the damage hook `(ammo + bow) * draw_factor` and optional aim assist.
//! * [`ballistics`]: Bevy-free arrow flight with gravity and drag, swept hits on world
//!   surfaces and body hitzones, sticking and ricochets.
//! * [`plugin`]: Bevy/avian integration (bow in a physics hand, arrows against the avian
//!   world and every hitzone set, hit messages with the hook damage).
pub mod ballistics;
pub mod bow;
pub mod plugin;

pub use plugin::{
    Archery, ArrowFiredMessage, ArrowHitMessage, FlyingArrow, HeldArrow, VrArcheryPlugin,
};
