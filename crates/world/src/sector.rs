//! Sectors: the story-state switches of whole world layers (`gCSector`,
//! `gCSectorPersistence_PS`, `eCContextBase::Enable`).
//!
//! A sector owns geometry layers (static `_cstat` documents) and dynamic layers (the `.node` and
//! `.lrentdat` documents with NPCs, objects and nav zones). Each layer is a context whose root
//! entity carries every entity of the layer. Sectors are not paged by distance (that is the PVS
//! prefetcher); a sector has an *enabled* flag that the game flips with `SetSectorStatus` (and
//! the console's `ToggleSector`). Enabling or disabling a sector enables or disables the root
//! entity of every layer, which propagates down the subtree. That is why the post-liberation
//! layers of a city (`Ardea_NPC_02`) are stored with `Enabled = false` on all entities: they are
//! the saved image of a disabled context.
//!
//! The flag persists in the savegame (`gCSectorPersistence_PS`): `collect` records
//! `(name, enabled)` for all sectors, `update_all` re-applies them after a load.
use crate::entity::EntityTree;
use crate::pvs::EntityId;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// Kind of a sector layer.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum LayerKind {
    /// Static geometry (`gCGeometryLayer`, `.lrgeodat`).
    Geometry,
    /// Entities placed by the designers (`gCDynamicLayer`, `.node` / `.lrentdat`).
    Dynamic,
}

/// One layer of a sector.
#[derive(Debug, Clone)]
pub struct Layer {
    /// Document the layer was loaded from.
    pub document: String,
    pub kind: LayerKind,
    /// Root entity of the layer's context (`eCContextBase+0x40`); `None` until it is loaded.
    pub root: Option<EntityId>,
}

/// Port of the state of `gCSector`.
#[derive(Debug, Clone)]
pub struct Sector {
    pub name: String,
    /// `gCSector+0x4c`.
    pub enabled: bool,
    pub layers: Vec<Layer>,
}

/// Result of switching a sector.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SectorSwitch {
    pub name: String,
    pub enabled: bool,
    /// The flag differs from before.
    pub changed: bool,
}

/// All sectors of the loaded world (`gCWorld+0x48`).
#[derive(Debug, Clone)]
pub struct SectorAdmin {
    sectors: BTreeMap<String, Sector>,
    /// `gCSector::ms_bEnableLayerActivation`.
    layer_activation: bool,
    /// `gCNavigationAdmin+0x50`: the nav scene must be rebuilt.
    nav_dirty: bool,
    /// `eCPVSPrefetcher3::ForcePrefetch` requested by `eCContextBase::Enable`.
    force_prefetch: bool,
}

impl Default for SectorAdmin {
    fn default() -> Self {
        Self {
            sectors: BTreeMap::new(),
            layer_activation: true,
            nav_dirty: false,
            force_prefetch: false,
        }
    }
}

fn key(name: &str) -> String {
    name.to_ascii_lowercase()
}

impl SectorAdmin {
    pub fn new() -> Self {
        Self::default()
    }

    /// Port of `gCSector::EnableSectorActivation` (Game:0x10192ca0): with the switch off,
    /// `OnEnable` only stores the flag and leaves the layers alone.
    pub fn enable_layer_activation(&mut self, on: bool) {
        self.layer_activation = on;
    }

    /// Add a sector with its initial flag. An existing sector keeps its layers.
    pub fn register(&mut self, name: &str, enabled: bool) {
        self.sectors
            .entry(key(name))
            .and_modify(|s| s.enabled = enabled)
            .or_insert_with(|| Sector {
                name: name.to_owned(),
                enabled,
                layers: Vec::new(),
            });
    }

    /// Port of `gCSector::InsertGeometryLayer` / `InsertDynamicLayer`: attach a layer. Returns
    /// false for an unknown sector. A layer added to a disabled sector starts disabled, so a
    /// late-loaded document cannot revive a switched-off sector.
    pub fn add_layer(
        &mut self,
        tree: &mut EntityTree,
        sector: &str,
        document: &str,
        kind: LayerKind,
        root: Option<EntityId>,
    ) -> bool {
        let Some(s) = self.sectors.get_mut(&key(sector)) else {
            return false;
        };
        if let (Some(r), true) = (root, s.enabled) {
            // A layer saved with Enabled = false in an enabled sector is activated.
            tree.enable(r, true);
        } else if let Some(r) = root {
            enable_context(tree, r, false);
        }
        s.layers.push(Layer {
            document: document.to_owned(),
            kind,
            root,
        });
        true
    }

    pub fn sector(&self, name: &str) -> Option<&Sector> {
        self.sectors.get(&key(name))
    }

    pub fn names(&self) -> impl Iterator<Item = &str> {
        self.sectors.values().map(|s| s.name.as_str())
    }

    /// Port of `gCSector::IsEnabled` (Game:0x10192cc0). Unknown sectors count as enabled: the
    /// world simply has no switch for them.
    pub fn is_enabled(&self, name: &str) -> bool {
        self.sector(name).is_none_or(|s| s.enabled)
    }

    /// The sector a document belongs to: the one whose name is a directory of the path (the
    /// authored layout is `<Region>/<Sector>/<Sector>[_suffix].<ext>`) or the file stem.
    pub fn sector_of_document(&self, path: &str) -> Option<&Sector> {
        let norm = path.replace('\\', "/").to_ascii_lowercase();
        let mut parts: Vec<&str> = norm.split('/').collect();
        let file = parts.pop().unwrap_or("");
        for dir in parts.iter().rev() {
            if let Some(s) = self.sectors.get(*dir) {
                return Some(s);
            }
        }
        let stem = file.split('.').next().unwrap_or(file);
        self.sectors.get(stem).or_else(|| {
            // `<Sector>_SHyb` and similar suffixes.
            self.sectors
                .iter()
                .filter(|(k, _)| stem.starts_with(k.as_str()))
                .max_by_key(|(k, _)| k.len())
                .map(|(_, s)| s)
        })
    }

    /// Does the document of a world entity currently exist? Documents outside any known
    /// sector are always live.
    pub fn document_enabled(&self, path: &str) -> bool {
        self.sector_of_document(path).is_none_or(|s| s.enabled)
    }

    /// Port of `gCSector::Enable` (Game:0x10192cd0). The original asks the `World_MCP` entity's
    /// `gCSectorPersistence_PS` to switch (`UpdateSector`, which calls `OnEnable` and logs) and
    /// falls back to `OnEnable` itself; both end in [`Self::on_enable`]. `None` for an unknown
    /// sector.
    pub fn enable(&mut self, tree: &mut EntityTree, name: &str, on: bool) -> Option<SectorSwitch> {
        self.on_enable(tree, name, on)
    }

    /// Port of `gCSector::OnEnable` (Game:0x10192d50): store the flag; with layer activation on,
    /// enable (or disable) the context of every geometry layer and every dynamic layer and flag
    /// the navigation admin for a rebuild.
    pub fn on_enable(
        &mut self,
        tree: &mut EntityTree,
        name: &str,
        on: bool,
    ) -> Option<SectorSwitch> {
        let layer_activation = self.layer_activation;
        let s = self.sectors.get_mut(&key(name))?;
        let changed = s.enabled != on;
        s.enabled = on;
        if layer_activation {
            for kind in [LayerKind::Geometry, LayerKind::Dynamic] {
                for l in s.layers.iter().filter(|l| l.kind == kind) {
                    if let Some(r) = l.root {
                        enable_context(tree, r, on);
                    }
                }
            }
            self.force_prefetch = true;
            self.nav_dirty = true;
        }
        Some(SectorSwitch {
            name: s.name.clone(),
            enabled: on,
            changed,
        })
    }

    /// Port of `gCSector::ContainsEntity` (Game:0x10192b60): is `entity` below a layer root of
    /// the sector?
    pub fn contains_entity(&self, tree: &EntityTree, sector: &str, entity: EntityId) -> bool {
        let Some(s) = self.sector(sector) else {
            return false;
        };
        s.layers
            .iter()
            .filter_map(|l| l.root)
            .any(|r| r == entity || tree.contains_parent(entity, r))
    }

    /// Entities of all layer roots of a sector (`CollectContexts` + their subtrees).
    pub fn entities(&self, tree: &EntityTree, sector: &str) -> Vec<EntityId> {
        self.sector(sector)
            .map(|s| {
                s.layers
                    .iter()
                    .filter_map(|l| l.root)
                    .flat_map(|r| tree.subtree(r))
                    .collect()
            })
            .unwrap_or_default()
    }

    /// The nav scene must be rebuilt (read-and-clear of `gCNavigationAdmin+0x50`).
    pub fn take_nav_dirty(&mut self) -> bool {
        std::mem::take(&mut self.nav_dirty)
    }

    /// `ForcePrefetch` was requested since the last call.
    pub fn take_force_prefetch(&mut self) -> bool {
        std::mem::take(&mut self.force_prefetch)
    }

    /// Port of `gCSectorPersistence_PS::CollectSectors` (Game:0x101a18f0): the save image.
    pub fn collect(&self) -> Vec<SectorSave> {
        self.sectors
            .values()
            .map(|s| SectorSave {
                name: s.name.clone(),
                enabled: s.enabled,
            })
            .collect()
    }

    /// Port of `gCSectorPersistence_PS::UpdateSectors` (Game:0x101a1800): re-apply a save
    /// image. Returns the names that are not part of the world (the original logs a warning).
    pub fn update_all(&mut self, tree: &mut EntityTree, saved: &[SectorSave]) -> Vec<String> {
        let mut unknown = Vec::new();
        for s in saved {
            if self.on_enable(tree, &s.name, s.enabled).is_none() {
                unknown.push(s.name.clone());
            }
        }
        unknown
    }
}

/// `(name, enabled)` entry of the persisted sector list.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SectorSave {
    pub name: String,
    pub enabled: bool,
}

/// Port of `eCContextBase::Enable` (Engine:0x1021a060) for a context root: a context that is
/// switched off is cached out first (stale), then the root and its subtree are enabled or
/// disabled.
pub fn enable_context(tree: &mut EntityTree, root: EntityId, on: bool) {
    if !on {
        tree.cache_out(root, true, false);
    }
    tree.enable(root, on);
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A layer root with `n` NPC entities below it.
    fn layer(tree: &mut EntityTree, name: &str, n: usize) -> EntityId {
        let root = tree.create(name, false);
        for i in 0..n {
            let e = tree.create(&format!("{name}_npc{i}"), true);
            tree.link(e, Some(root)).unwrap();
        }
        root
    }

    fn enabled_names(tree: &EntityTree, admin: &SectorAdmin, sector: &str) -> usize {
        admin
            .entities(tree, sector)
            .iter()
            .filter(|e| tree.get(**e).unwrap().is_enabled())
            .count()
    }

    #[test]
    fn toggling_a_sector_swaps_the_entity_sets() {
        let mut tree = EntityTree::new();
        let mut admin = SectorAdmin::new();
        admin.register("G3_Myrtana_01_Ardea_NPC_01", true);
        admin.register("G3_Myrtana_01_Ardea_NPC_02", false);
        let r1 = layer(&mut tree, "npc01", 3);
        let r2 = layer(&mut tree, "npc02", 5);
        admin.add_layer(
            &mut tree,
            "G3_Myrtana_01_Ardea_NPC_01",
            "G3_World_01/Myrtana/Ardea_City/G3_Myrtana_01_Ardea_NPC_01/G3_Myrtana_01_Ardea_NPC_01.lrentdat",
            LayerKind::Dynamic,
            Some(r1),
        );
        admin.add_layer(
            &mut tree,
            "G3_Myrtana_01_Ardea_NPC_02",
            "G3_World_01/Myrtana/Ardea_City/G3_Myrtana_01_Ardea_NPC_02/G3_Myrtana_01_Ardea_NPC_02.lrentdat",
            LayerKind::Dynamic,
            Some(r2),
        );
        // New game: the first population exists, the post-liberation layer is switched off.
        assert_eq!(
            enabled_names(&tree, &admin, "G3_Myrtana_01_Ardea_NPC_01"),
            4
        );
        assert_eq!(
            enabled_names(&tree, &admin, "G3_Myrtana_01_Ardea_NPC_02"),
            0
        );

        // Liberation: layer 1 off, layer 2 on.
        let off = admin
            .enable(&mut tree, "g3_myrtana_01_ardea_npc_01", false)
            .unwrap();
        assert!(off.changed);
        admin
            .enable(&mut tree, "G3_Myrtana_01_Ardea_NPC_02", true)
            .unwrap();
        assert_eq!(
            enabled_names(&tree, &admin, "G3_Myrtana_01_Ardea_NPC_01"),
            0
        );
        assert_eq!(
            enabled_names(&tree, &admin, "G3_Myrtana_01_Ardea_NPC_02"),
            6
        );
        assert!(admin.take_nav_dirty());
        assert!(!admin.take_nav_dirty());
        assert!(admin.take_force_prefetch());
    }

    #[test]
    fn disabling_caches_the_context_out_and_unknown_sectors_are_reported() {
        let mut tree = EntityTree::new();
        let mut admin = SectorAdmin::new();
        admin.register("A", true);
        let r = layer(&mut tree, "a", 1);
        admin.add_layer(&mut tree, "A", "A.lrentdat", LayerKind::Dynamic, Some(r));
        tree.cache_in(r);
        assert!(tree.get(r).unwrap().is_cached_in());
        admin.enable(&mut tree, "A", false);
        assert!(!tree.get(r).unwrap().is_cached_in());
        assert!(admin.enable(&mut tree, "Nope", true).is_none());
        let unknown = admin.update_all(
            &mut tree,
            &[
                SectorSave {
                    name: "A".into(),
                    enabled: true,
                },
                SectorSave {
                    name: "Gone".into(),
                    enabled: true,
                },
            ],
        );
        assert_eq!(unknown, vec!["Gone".to_owned()]);
        assert!(admin.is_enabled("A"));
    }

    #[test]
    fn layer_activation_switch_only_stores_the_flag() {
        let mut tree = EntityTree::new();
        let mut admin = SectorAdmin::new();
        admin.register("A", true);
        let r = layer(&mut tree, "a", 2);
        admin.add_layer(&mut tree, "A", "A.node", LayerKind::Geometry, Some(r));
        admin.enable_layer_activation(false);
        admin.enable(&mut tree, "A", false);
        assert!(!admin.is_enabled("A"));
        assert_eq!(enabled_names(&tree, &admin, "A"), 3);
    }

    #[test]
    fn documents_map_to_their_sector_and_the_flag_persists() {
        let mut tree = EntityTree::new();
        let mut admin = SectorAdmin::new();
        admin.register("G3_Myrtana_01_Ardea_NPC_02", false);
        let p = "G3_World_01/Myrtana/Ardea_City/G3_Myrtana_01_Ardea_NPC_02/G3_Myrtana_01_Ardea_NPC_02.lrentdat";
        assert!(!admin.document_enabled(p));
        assert!(admin.document_enabled("G3_World_01/Elsewhere/x.node"));
        assert!(!admin.document_enabled("g3_myrtana_01_ardea_npc_02_shyb.node"));
        let saved = admin.collect();
        admin.enable(&mut tree, "G3_Myrtana_01_Ardea_NPC_02", true);
        assert!(admin.document_enabled(p));
        admin.update_all(&mut tree, &saved);
        assert!(!admin.document_enabled(p));
    }

    #[test]
    fn contains_entity_walks_the_layer_roots() {
        let mut tree = EntityTree::new();
        let mut admin = SectorAdmin::new();
        admin.register("A", true);
        admin.register("B", true);
        let ra = layer(&mut tree, "a", 1);
        let rb = layer(&mut tree, "b", 1);
        admin.add_layer(&mut tree, "A", "A.node", LayerKind::Dynamic, Some(ra));
        admin.add_layer(&mut tree, "B", "B.node", LayerKind::Dynamic, Some(rb));
        let child_a = tree.get(ra).unwrap().children()[0];
        assert!(admin.contains_entity(&tree, "A", child_a));
        assert!(!admin.contains_entity(&tree, "B", child_a));
    }
}
