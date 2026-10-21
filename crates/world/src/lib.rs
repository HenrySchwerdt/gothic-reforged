//! World streaming and entity processing, ported from the original engine's PVS prefetcher
//! (`eCPVSPrefetcher3`, `eCPVSCellItem`), entity admin (`eCEntityAdmin`), scene admin
//! (`eCSceneAdmin`) and entity core (`eCEntity`). Grid coordinates are Gothic units (cm); the
//! planner touches no files and no game data.
//!
//! * [`pvs`]: the 10 000-unit prefetch grid, hysteresis and cell items.
//! * [`entity`]: entity flags, hierarchy, transforms, node/tree boundaries, kill/cache.
//! * [`admin`]: processing range (ROI) and the processing list.
//! * [`scene`]: name/template registry, `Trigger`, and the per-frame [`scene::Scene`].
//! * [`streaming`]: the host-side tile residency that follows the grid, with a Bevy plugin.
pub mod admin;
pub mod entity;
pub mod pvs;
pub mod scene;
pub mod streaming;

pub use streaming::{
    CellId, Residency, Streamer, StreamingObserver, StreamingSettings, TileStreamer,
    WorldStreamingPlugin,
};
