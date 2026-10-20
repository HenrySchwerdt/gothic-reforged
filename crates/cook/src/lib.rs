//! gothic-cook: compile a Gothic 3 installation once into a fast, versioned,
//! platform-specific runtime cache, memory-mapped at runtime.
//!
//! * [`cook::cook`] writes the cache (parallel, incremental, content-addressed).
//! * [`cache::Cache`] reads it (`load_mesh`, `load_cell`, `load_texture`, `load_db`, ...).
//!
//! Original files stay the source of truth; the cache is rebuildable and never redistributed.
pub mod actor;
pub mod cache;
pub mod cli;
pub mod container;
pub mod cook;
pub mod db;
pub mod ids;
pub mod material;
pub mod mesh;
pub mod mmap;
pub mod remaster;
mod shim;
pub mod texture;
pub mod world;

pub use cache::{Cache, default_cache_root};
pub use cook::{CookKind, Options, Region, Report, cook};
pub use ids::{AssetId, Profile, TextureMode};
