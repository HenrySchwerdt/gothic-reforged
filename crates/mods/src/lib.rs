//! Data overlay mods (MOD-01): mod folders with a manifest, a deterministic load order and
//! typed TOML/RON patches that add, change or delete quests, infos (dialog lines), items, NPC
//! templates and strings of the game database; conflicts are detected and reported; a polling
//! hot reload re-applies everything during development. See `docs/MODDING.md`.
//!
//! ```no_run
//! # fn main() -> anyhow::Result<()> {
//! # let base_db = gothic_gameplay::GameDatabase::load_install("Data".as_ref(), "German")?;
//! use gothic_mods::{ModData, apply, discover, resolve};
//! let base = ModData::from_database(&base_db);
//! let (data, report) = apply(base, &resolve(discover("mods".as_ref())?));
//! println!("{}", report.to_text());
//! let db = data.to_database("German")?;
//! # Ok(()) }
//! ```
pub mod apply;
pub mod data;
pub mod diff;
pub mod hot;
pub mod loader;
pub mod manifest;
pub mod patch;

pub use apply::{Conflict, ModReport, OpError, apply};
pub use data::ModData;
pub use diff::diff;
pub use hot::HotReload;
pub use loader::{LoadOrder, LoadedMod, OrderProblem, discover, resolve};
pub use manifest::Manifest;
pub use patch::{PatchFile, RecordKind};
