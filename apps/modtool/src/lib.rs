//! Mod manager for Gothic Reforged (`modtool`): scaffold, validate, pack (optionally signed),
//! install / uninstall, enable / disable / reorder, and a static conflict report, for data
//! overlay mods (`gothic-mods`) and Rhai script mods (`gothic-mods-script`). The
//! [`modlist::ModList`] model is what an in-game mod menu reads and writes.
//! See `docs/MODDING.md` ("Mod manager").
pub mod conflicts;
pub mod install;
pub mod modlist;
pub mod package;
pub mod scaffold;
pub mod validate;

pub use conflicts::{WriteConflict, static_conflicts};
pub use install::{InstallOptions, InstallReport, install, uninstall};
pub use modlist::{ModEntry, ModList, ModStatus};
pub use package::{Package, PackageManifest, SignatureStatus, keygen, pack};
pub use validate::{ModKind, ValidationReport, validate_dir};

/// File extensions a mod may contain (text data, scripts, docs and common media). Original
/// game resource formats are not in the list on purpose.
pub const ALLOWED_EXTENSIONS: &[&str] = &[
    "toml", "ron", "rhai", "md", "txt", "json", "srt", "png", "jpg", "dds", "ogg", "wav",
];
