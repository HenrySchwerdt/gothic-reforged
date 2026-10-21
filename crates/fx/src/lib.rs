//! Effects: particle emitters (`eCParticle_PS`), the effect map and command sequences
//! (`gCEffect*`). The simulation is plain Rust; the Bevy plugin (feature `render`) batches the
//! billboards.
pub mod boids;
pub mod draw;
pub mod effect;
pub mod efm;
pub mod matadmin;
pub mod params;
#[cfg(feature = "render")]
pub mod plugin;
pub mod rng;
pub mod scale;
pub mod sim;
pub mod space;
pub mod spell;
pub mod templates;
pub mod world;
