//! Projectiles of Gothic 3: arrows, bolts and spell missiles (engine port of `gCProjectile_PS`,
//! GameDll:0x10151cc0..0x10153130) and the bow aim coupling of the player states.
//!
//! The crate is Bevy-free and deterministic. Positions are centimetres, Y is up. A host owns the
//! geometry and answers [`ProjectileWorld`] queries; this crate owns the rules:
//!
//! * [`params`]: the property set of a projectile template (`PathStyle`, `TouchBehavior`,
//!   `ShootVelocity` in km/h, decay, ...) with the engine's defaults and the numbers of the
//!   installed templates.
//! * [`flight`]: [`Projectile`], the per-frame flight (`ShootIteration`), the touch rules
//!   (`OnTouch`: kill, reflect, stop, material dependent, sticking into characters) and the decay.
//! * [`launch`]: how the player states aim and release a projectile (`Shoot` S:86450 for arrows,
//!   `MagicProjectile` S:52db0 for spell missiles).
//! * [`aim`]: the bow and spell aim animation coupling: the pitch blend of `UpdateAim`, the draw
//!   timeline, crosshair size and the clip names.
pub mod aim;
pub mod flight;
pub mod launch;
pub mod params;
pub mod shape;

pub use flight::{ActorHit, Event, Projectile, ProjectileWorld, SurfaceHit};
pub use launch::Launch;
pub use params::{PathStyle, ProjectileParams, ShapeMaterial, TouchBehavior};
