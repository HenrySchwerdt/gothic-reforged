//! World streaming and entity processing, ported from the original engine's PVS prefetcher
//! (`eCPVSPrefetcher3`, `eCPVSCellItem`), entity admin (`eCEntityAdmin`), scene admin
//! (`eCSceneAdmin`) and entity core (`eCEntity`). Grid coordinates are Gothic units (cm); the
//! planner touches no files and no game data.
//!
//! * [`pvs`]: the 10 000-unit prefetch grid, hysteresis and cell items.
//! * [`entity`]: entity flags, hierarchy, transforms, node/tree boundaries, kill/cache.
//! * [`admin`]: processing range (ROI) and the processing list.
//! * [`scene`]: name/template registry, `Trigger`, and the per-frame [`scene::Scene`].
//! * [`dynamic`]: decay and auto-kill of dynamic entities, the dummy visual.
//! * [`sector`]: sectors and their enable flag (story-state layers), persistence.
//! * [`trigger`]: the trigger runtime (`eCTrigger_PS` and friends) with persistence.
//! * [`template`]: templates, spawning from them, patching instances.
//! * [`proxy`]: property sets, entity and property proxies.
//! * [`zones`]: collision circles and negative zones used for NPC avoidance.
//! * [`streaming`]: the host-side tile residency that follows the grid, with a Bevy plugin.
pub mod admin;
pub mod dynamic;
pub mod entity;
pub mod proxy;
pub mod pvs;
pub mod scene;
pub mod sector;
pub mod streaming;
pub mod template;
pub mod trigger;
pub mod zones;

pub use streaming::{
    CellId, Residency, Streamer, StreamingObserver, StreamingSettings, TileStreamer,
    WorldStreamingPlugin,
};
