//! Static props at scale.
//!
//! The core (Bevy-free) is a [`ClusterBvh`] over instance bounds: leaves are clusters of
//! nearby props, queried against the camera frustum, a draw distance and the shadow-caster
//! volume of every cascade ([`geom::cascade_caster_volume`]). [`LodRanges`] describe levels of
//! detail. With the `bevy` feature, [`plugin::StaticRenderPlugin`] registers meshes with LOD
//! ranges, spawns instances so Bevy's GPU-driven path batches them (shared mesh + material ->
//! one multi-draw-indirect batch with GPU per-instance culling), and hides whole clusters that
//! neither the view nor any shadow cascade can see.
pub mod bvh;
pub mod geom;
pub mod lod;
#[cfg(feature = "bevy")]
pub mod plugin;

pub use bvh::{Cluster, ClusterBvh, CullQuery, CullStats};
pub use geom::{Aabb, Containment, Frustum, Plane, cascade_caster_volume};
pub use lod::LodRanges;
