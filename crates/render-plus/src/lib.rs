//! Opt-in "remaster look" render profile for Gothic Reforged (render-plus): SSAO, TAA with
//! sharpening, volumetric fog and light shafts, soft and contact shadows, and screen-space
//! reflections, each switchable, with a frame-time budget that sheds effects when the frame
//! gets too expensive. The default profile switches nothing on, so the faithful look is unchanged
//! unless a host opts in.
//!
//! [`profile`], [`plan`] and [`budget`] are plain logic; the `render` feature adds
//! [`plugin::RenderPlusPlugin`], which applies the plan to cameras marked
//! [`plugin::RenderPlusCamera`] and the light marked [`plugin::RenderPlusSun`]. See
//! `docs/RENDER_PLUS.md`.
pub mod budget;
pub mod plan;
#[cfg(feature = "render")]
pub mod plugin;
pub mod profile;

pub use budget::{Change, Governor};
pub use plan::Plan;
pub use profile::{Budget, Effect, Preset, RenderProfile, SsaoQuality};
