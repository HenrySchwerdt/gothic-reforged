//! `eCSceneAdmin`: the entity registry (by name, by id, templates) and `Trigger`, plus [`Scene`],
//! which ties the entity tree, the PVS prefetcher and the entity admin together the way the
//! engine's per-frame order does: prefetcher first (cells cache in and out), then the entity
//! admin (processing range), then the kill list.
use crate::admin::{EntityAdmin, ProcessingDelta};
use crate::entity::{Aabb, EntityTree};
use crate::pvs::{
    EntityId, GridDelta, MAIN_CELL_SIZE, PvsCell, PvsError, PvsPrefetcher, PvsSphere,
};
use bevy::math::Vec3;
use std::collections::{BTreeMap, HashMap};

/// Which registry `GetEntityByPartName` searches (`eEGetEntityTypeHint`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TypeHint {
    /// Registered entities.
    Entity,
    /// Template entities.
    Template,
}

/// Port of the `eSTriggerConfig` modes handled by `eCSceneAdmin::Trigger` (Engine:0x10233ba0).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TriggerTarget {
    /// Entities registered under `name`, or every registered entity when `name` is empty.
    Registered,
    /// Every dynamic entity of the scene (the camera's frustum database in the original).
    Dynamic,
}

#[derive(Debug, Clone)]
pub struct TriggerConfig {
    pub target: TriggerTarget,
    /// Only entities with exactly this name (empty = all).
    pub name: String,
    /// Entity trigger-group bit that must be set (`0` = no requirement).
    pub group: u32,
}

#[derive(Debug, Clone, Default)]
struct Info {
    name: String,
    /// Bit set per trigger group (`eCEntity+0x194`).
    groups: u64,
}

/// Registry of entities and templates.
#[derive(Debug, Default)]
pub struct SceneAdmin {
    by_name: BTreeMap<String, Vec<EntityId>>,
    info: BTreeMap<EntityId, Info>,
    templates: BTreeMap<String, u64>,
    /// Entity id (GUID) index behind `eCEntityProxy::ResolveEntity`.
    guids: BTreeMap<String, EntityId>,
    spatial_contexts: usize,
    editor_mode: bool,
}

impl SceneAdmin {
    pub fn new() -> Self {
        Self::default()
    }

    /// `RegisterEntity` + `RegisterNameInfo`.
    pub fn register(&mut self, id: EntityId, name: &str) {
        self.unregister(id);
        self.by_name.entry(name.to_owned()).or_default().push(id);
        self.info.insert(
            id,
            Info {
                name: name.to_owned(),
                groups: 0,
            },
        );
    }

    /// `UnregisterEntity` + `UnregisterNameInfo`.
    pub fn unregister(&mut self, id: EntityId) -> bool {
        let Some(info) = self.info.remove(&id) else {
            return false;
        };
        self.guids.retain(|_, e| *e != id);
        if let Some(v) = self.by_name.get_mut(&info.name) {
            v.retain(|e| *e != id);
            if v.is_empty() {
                self.by_name.remove(&info.name);
            }
        }
        true
    }

    /// Index an entity under its GUID (the entity-id lookup that `eCEntityProxy` resolves
    /// through). A GUID names one entity; the newest registration wins.
    pub fn register_guid(&mut self, id: EntityId, guid: &str) {
        self.guids.insert(guid.to_ascii_lowercase(), id);
    }

    /// The entity registered under a GUID.
    pub fn entity_by_guid(&self, guid: &str) -> Option<EntityId> {
        self.guids.get(&guid.to_ascii_lowercase()).copied()
    }

    pub fn is_registered(&self, id: EntityId) -> bool {
        self.info.contains_key(&id)
    }
    pub fn entity_count(&self) -> usize {
        self.info.len()
    }

    /// Set a trigger group bit on an entity.
    pub fn set_trigger_group(&mut self, id: EntityId, group: u32, on: bool) {
        if let Some(i) = self.info.get_mut(&id)
            && group < 64
        {
            if on {
                i.groups |= 1 << group;
            } else {
                i.groups &= !(1 << group);
            }
        }
    }

    /// `GetEntityByName`: the first entity registered under the name.
    pub fn entity_by_name(&self, name: &str) -> Option<EntityId> {
        self.by_name.get(name)?.first().copied()
    }
    /// `GetEntitiesByName`.
    pub fn entities_by_name(&self, name: &str) -> &[EntityId] {
        self.by_name.get(name).map_or(&[], Vec::as_slice)
    }
    /// `GetNumEntitiesByName`.
    pub fn count_by_name(&self, name: &str) -> usize {
        self.entities_by_name(name).len()
    }
    /// `CheckIfEntityNameExists`.
    pub fn name_exists(&self, name: &str) -> bool {
        self.by_name.contains_key(name)
    }
    /// `GetUniqueEntityByName`: succeeds only when exactly one entity carries the name.
    pub fn unique_entity_by_name(&self, name: &str) -> Option<EntityId> {
        match self.entities_by_name(name) {
            [one] => Some(*one),
            _ => None,
        }
    }

    /// `RegisterTemplateEntity`: a duplicate name is refused (the original warns and gives the
    /// newcomer a random id; here the first registration stays).
    pub fn register_template(&mut self, name: &str, payload: u64) -> bool {
        if self.templates.contains_key(name) {
            return false;
        }
        self.templates.insert(name.to_owned(), payload);
        true
    }
    pub fn unregister_template(&mut self, name: &str) -> bool {
        self.templates.remove(name).is_some()
    }
    /// `GetTemplateEntityByName`.
    pub fn template(&self, name: &str) -> Option<u64> {
        self.templates.get(name).copied()
    }
    pub fn template_names(&self) -> impl Iterator<Item = &str> {
        self.templates.keys().map(String::as_str)
    }

    /// `RegisterSpatialContext` / `UnregisterSpatialContext` counter.
    pub fn register_spatial_context(&mut self) {
        self.spatial_contexts += 1;
    }
    pub fn unregister_spatial_context(&mut self) {
        self.spatial_contexts = self.spatial_contexts.saturating_sub(1);
    }
    pub fn spatial_context_count(&self) -> usize {
        self.spatial_contexts
    }
    pub fn enable_editor_mode(&mut self, on: bool) {
        self.editor_mode = on;
    }
    pub fn is_editor_mode(&self) -> bool {
        self.editor_mode
    }

    /// Entities whose name starts with `part` (ignoring case), killed ones excluded, ordered by
    /// name (`S_CompareEntityNames`) then id.
    fn matches_part(&self, tree: &EntityTree, part: &str) -> Vec<EntityId> {
        let part = part.to_lowercase();
        let mut v: Vec<(String, EntityId)> = self
            .info
            .iter()
            .filter(|(id, i)| {
                i.name.to_lowercase().starts_with(&part)
                    && tree.get(**id).is_none_or(|e| !e.is_killed())
            })
            .map(|(id, i)| (i.name.to_lowercase(), *id))
            .collect();
        v.sort();
        v.into_iter().map(|(_, id)| id).collect()
    }

    /// Port of `eCSceneAdmin::GetEntityByPartName` (Engine:0x10232c20): prefix search, case
    /// insensitive; `index` picks from the name-ordered matches. `None` index (the original's
    /// -1) returns the first match found in registry order.
    pub fn entity_by_part_name(
        &self,
        tree: &EntityTree,
        part: &str,
        hint: TypeHint,
        index: Option<usize>,
    ) -> Option<EntityId> {
        gothic_portcov::hit!("entity_by_part_name");
        if hint != TypeHint::Entity {
            return None;
        }
        let m = self.matches_part(tree, part);
        match index {
            None => self
                .info
                .iter()
                .find(|(id, i)| {
                    i.name.to_lowercase().starts_with(&part.to_lowercase())
                        && tree.get(**id).is_none_or(|e| !e.is_killed())
                })
                .map(|(id, _)| *id),
            Some(i) => m.get(i).copied(),
        }
    }

    /// Port of `eCSceneAdmin::GetEntityCountByPartName` (Engine:0x10232f90).
    pub fn count_by_part_name(&self, tree: &EntityTree, part: &str, hint: TypeHint) -> usize {
        gothic_portcov::hit!("count_by_part_name");
        if hint != TypeHint::Entity {
            return 0;
        }
        self.matches_part(tree, part).len()
    }

    fn can_be_triggered(&self, id: EntityId, cfg: &TriggerConfig) -> bool {
        let Some(i) = self.info.get(&id) else {
            return false;
        };
        if !cfg.name.is_empty() && i.name != cfg.name {
            return false;
        }
        cfg.group == 0 || (cfg.group < 64 && i.groups & (1 << cfg.group) != 0)
    }

    /// Port of `eCSceneAdmin::Trigger` (Engine:0x10233ba0): the entities that receive the
    /// trigger from `source`. Delivery (the entity's trigger virtual) is the caller's job.
    pub fn trigger_targets(&self, tree: &EntityTree, cfg: &TriggerConfig) -> Vec<EntityId> {
        gothic_portcov::hit!("trigger_targets");
        match cfg.target {
            TriggerTarget::Registered => {
                if cfg.name.is_empty() {
                    self.info
                        .keys()
                        .copied()
                        .filter(|id| self.can_be_triggered(*id, cfg))
                        .collect()
                } else {
                    self.entities_by_name(&cfg.name)
                        .iter()
                        .copied()
                        .filter(|id| self.can_be_triggered(*id, cfg))
                        .collect()
                }
            }
            TriggerTarget::Dynamic => tree
                .dynamic_entities()
                .map(|e| e.id)
                .filter(|id| self.can_be_triggered(*id, cfg))
                .collect(),
        }
    }
}

/// A static entity the world data places in a cell.
#[derive(Debug, Clone)]
pub struct StaticDesc {
    pub name: String,
    pub position: Vec3,
    pub half_extent: Vec3,
    /// Payload size credited to the cell budget.
    pub weight: u64,
}

/// Supplies the static entities of a PVS cell (the world documents in the real game).
pub trait CellSource {
    fn cell_entities(&self, cell: PvsCell, cell_size: u32) -> Vec<StaticDesc>;
}

/// Everything that changed in one [`Scene::update`].
#[derive(Debug, Default)]
pub struct SceneUpdate {
    pub grid: GridDelta,
    pub processing: ProcessingDelta,
    /// Entities destroyed this frame (cells that vanished, kills).
    pub destroyed: Vec<EntityId>,
}

/// The scene model: tree, prefetcher, admin and registry with the engine's frame order.
pub struct Scene {
    pub tree: EntityTree,
    pub prefetch: PvsPrefetcher,
    pub admin: EntityAdmin,
    pub registry: SceneAdmin,
    /// Sectors and their enable flags; a switch forces the next prefetch evaluation.
    pub sectors: crate::sector::SectorAdmin,
    db: u8,
    /// Far clip plane of the observing camera; it is the PVS sphere radius.
    pub far_clip: f32,
    pub special_camera: bool,
    cell_entities: HashMap<PvsCell, Vec<EntityId>>,
}

impl Scene {
    pub fn new(far_clip: f32) -> Self {
        let mut prefetch = PvsPrefetcher::new();
        let db = prefetch
            .register_database(MAIN_CELL_SIZE)
            .expect("a fresh prefetcher has room for a database");
        let admin = EntityAdmin::for_database(db);
        Self {
            tree: EntityTree::new(),
            prefetch,
            admin,
            registry: SceneAdmin::new(),
            sectors: crate::sector::SectorAdmin::new(),
            db,
            far_clip,
            special_camera: false,
            cell_entities: HashMap::new(),
        }
    }

    pub fn database(&self) -> u8 {
        self.db
    }

    /// Create and register a dynamic entity that is cached in at once (an actor, an item).
    pub fn spawn_dynamic(&mut self, name: &str, position: Vec3, half_extent: Vec3) -> EntityId {
        let id = self.tree.create(name, true);
        // Both calls only fail for unknown ids.
        let _ = self
            .tree
            .set_local_node_box(id, Some(Aabb::around(Vec3::ZERO, half_extent)));
        let _ = self.tree.set_local_position(id, position);
        self.tree.cache_in(id);
        self.registry.register(id, name);
        id
    }

    /// Move an entity (world space).
    pub fn move_to(&mut self, id: EntityId, position: Vec3) {
        let _ = self.tree.set_world_position(id, position);
    }

    /// Kill an entity; it is destroyed in the next update's kill-list flush.
    pub fn kill(&mut self, id: EntityId) {
        self.tree.kill(id);
    }

    fn cache_in_cell(&mut self, cell: PvsCell, source: &dyn CellSource) {
        let mut ids = Vec::new();
        for d in source.cell_entities(cell, MAIN_CELL_SIZE) {
            let id = self.tree.create(&d.name, false);
            let _ = self
                .tree
                .set_local_node_box(id, Some(Aabb::around(Vec3::ZERO, d.half_extent)));
            let _ = self.tree.set_local_position(id, d.position);
            self.prefetch
                .add_entity(self.db, id, d.position.to_array(), d.weight);
            self.tree.cache_in(id);
            self.registry.register(id, &d.name);
            ids.push(id);
        }
        self.registry.register_spatial_context();
        self.cell_entities.insert(cell, ids);
    }

    fn cache_out_cell(&mut self, cell: PvsCell, destroyed: &mut Vec<EntityId>) {
        let Some(ids) = self.cell_entities.remove(&cell) else {
            return;
        };
        self.registry.unregister_spatial_context();
        for id in ids {
            self.tree.cache_out(id, false, false);
            for d in self.tree.destroy(id) {
                self.registry.unregister(d);
                destroyed.push(d);
            }
        }
    }

    /// One frame: PVS prefetch around `camera`, then the entity admin, then the kill list.
    pub fn update(
        &mut self,
        camera: Vec3,
        source: &dyn CellSource,
    ) -> Result<SceneUpdate, PvsError> {
        let mut out = SceneUpdate::default();
        // `eCContextBase::Enable` ends in `ForcePrefetch`.
        if self.sectors.take_force_prefetch() {
            self.prefetch.force(self.db);
            self.admin.force_full_update();
        }
        let sphere = PvsSphere::from_camera(camera.to_array(), self.far_clip, self.special_camera);
        if let Some(grid) = self
            .prefetch
            .process(self.db, sphere, self.special_camera)?
        {
            for &cell in &grid.deleted {
                self.cache_out_cell(cell, &mut out.destroyed);
            }
            for &cell in &grid.added {
                self.cache_in_cell(cell, source);
            }
            out.grid = grid;
        }
        out.processing = self
            .admin
            .process(&mut self.tree, &mut self.prefetch, camera);
        for id in self.admin.flush_kills(&mut self.tree) {
            self.registry.unregister(id);
            out.destroyed.push(id);
        }
        Ok(out)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeSet;

    struct Lattice;
    impl CellSource for Lattice {
        /// One static entity every 1000 units.
        fn cell_entities(&self, cell: PvsCell, size: u32) -> Vec<StaticDesc> {
            let [x0, z0] = cell.min_corner(size);
            let mut v = Vec::new();
            for i in 0..10 {
                for j in 0..10 {
                    v.push(StaticDesc {
                        name: format!("rock_{}_{}_{i}_{j}", cell.x, cell.z),
                        position: Vec3::new(
                            x0 + 500.0 + 1000.0 * i as f32,
                            0.0,
                            z0 + 500.0 + 1000.0 * j as f32,
                        ),
                        half_extent: Vec3::splat(100.0),
                        weight: 1,
                    });
                }
            }
            v
        }
    }

    #[test]
    fn registry_name_lookups() {
        let mut tree = EntityTree::new();
        let mut r = SceneAdmin::new();
        let a = tree.create("Gate_01", true);
        let b = tree.create("Gate_02", true);
        let c = tree.create("gate_02", true);
        r.register(a, "Gate_01");
        r.register(b, "Gate_02");
        r.register(c, "gate_02");
        assert_eq!(r.entity_by_name("Gate_01"), Some(a));
        assert!(r.name_exists("Gate_02") && !r.name_exists("Gate_03"));
        assert_eq!(r.unique_entity_by_name("Gate_01"), Some(a));
        r.register(tree.create("Gate_01", true), "Gate_01");
        assert_eq!(r.unique_entity_by_name("Gate_01"), None);
        assert_eq!(r.count_by_name("Gate_01"), 2);
        // Prefix search ignores case and sorts by name.
        assert_eq!(r.count_by_part_name(&tree, "GATE_0", TypeHint::Entity), 4);
        assert_eq!(r.count_by_part_name(&tree, "gate_02", TypeHint::Entity), 2);
        assert_eq!(
            r.entity_by_part_name(&tree, "gate_0", TypeHint::Entity, Some(0)),
            Some(a)
        );
        assert_eq!(
            r.entity_by_part_name(&tree, "gate_0", TypeHint::Entity, Some(9)),
            None
        );
        assert_eq!(
            r.entity_by_part_name(&tree, "gate_0", TypeHint::Template, Some(0)),
            None
        );
        // Killed entities are skipped.
        tree.kill(a);
        assert_eq!(r.count_by_part_name(&tree, "gate_01", TypeHint::Entity), 1);
        assert!(r.unregister(b) && !r.unregister(b));
        assert!(!r.name_exists("Gate_02") || r.count_by_name("Gate_02") == 0);
    }

    #[test]
    fn templates_refuse_duplicates() {
        let mut r = SceneAdmin::new();
        assert!(r.register_template("NPC_Human", 1));
        assert!(!r.register_template("NPC_Human", 2));
        assert_eq!(r.template("NPC_Human"), Some(1));
        assert!(r.unregister_template("NPC_Human"));
        assert_eq!(r.template("NPC_Human"), None);
    }

    #[test]
    fn trigger_filters_by_name_and_group() {
        let mut tree = EntityTree::new();
        let mut r = SceneAdmin::new();
        let a = tree.create("door", true);
        let b = tree.create("door", true);
        let c = tree.create("lamp", false);
        for (id, n) in [(a, "door"), (b, "door"), (c, "lamp")] {
            r.register(id, n);
        }
        r.set_trigger_group(a, 3, true);
        let cfg = |target, name: &str, group| TriggerConfig {
            target,
            name: name.into(),
            group,
        };
        assert_eq!(
            r.trigger_targets(&tree, &cfg(TriggerTarget::Registered, "door", 0)),
            vec![a, b]
        );
        assert_eq!(
            r.trigger_targets(&tree, &cfg(TriggerTarget::Registered, "door", 3)),
            vec![a]
        );
        assert_eq!(
            r.trigger_targets(&tree, &cfg(TriggerTarget::Registered, "", 0))
                .len(),
            3
        );
        // Dynamic mode never reaches the static lamp.
        assert_eq!(
            r.trigger_targets(&tree, &cfg(TriggerTarget::Dynamic, "", 0)),
            vec![a, b]
        );
    }

    /// Headless walk: a camera crosses several cells; the set of cached-in cells, the static
    /// entities in range and the actors in range are checked at every step.
    #[test]
    fn walking_across_cells_loads_and_unloads_the_expected_sets() {
        let mut scene = Scene::new(10_000.0);
        let near_actor = scene.spawn_dynamic(
            "hero_friend",
            Vec3::new(5_500.0, 0.0, 5_000.0),
            Vec3::new(40.0, 90.0, 40.0),
        );
        let far_actor = scene.spawn_dynamic(
            "far_npc",
            Vec3::new(45_000.0, 0.0, 5_000.0),
            Vec3::new(40.0, 90.0, 40.0),
        );
        let mut resident: BTreeSet<PvsCell> = BTreeSet::new();
        let mut log: Vec<(f32, usize, usize)> = Vec::new();
        let mut x = 5_000.0;
        let mut entered_far = None;
        let mut exited_near = None;
        let mut step = 0;
        while x < 60_000.0 {
            let cam = Vec3::new(x, 0.0, 5_000.0);
            let u = scene.update(cam, &Lattice).unwrap();
            for c in &u.grid.added {
                assert!(resident.insert(*c), "cell loaded twice");
            }
            for c in &u.grid.deleted {
                assert!(resident.remove(c), "unloaded a cell that was not resident");
            }
            // The resident set is always exactly the grid's valid cells.
            let valid: BTreeSet<_> = scene
                .prefetch
                .valid_cells(scene.database())
                .into_iter()
                .collect();
            assert_eq!(valid, resident);
            // The camera's own cell is resident at all times.
            assert!(resident.contains(&PvsCell::of(cam.x, cam.z, MAIN_CELL_SIZE)));
            // Static entities exist only for resident cells: 100 per cell.
            let statics = scene.tree.iter().filter(|e| !e.is_dynamic()).count();
            assert_eq!(statics, 100 * resident.len());
            // No static entity is in range unless it is near the camera.
            for e in scene
                .tree
                .iter()
                .filter(|e| !e.is_dynamic() && e.has_entered_processing_range())
            {
                assert!(
                    e.world_position().distance(cam) < 4_250.0 * 1.5 + 300.0,
                    "{}",
                    e.name
                );
            }
            if u.processing.entered.contains(&far_actor) && entered_far.is_none() {
                entered_far = Some(x);
            }
            if u.processing.exited.contains(&near_actor) && exited_near.is_none() {
                exited_near = Some(x);
            }
            log.push((x, resident.len(), scene.admin.in_range().len()));
            x += 400.0;
            step += 1;
        }
        assert!(step > 100);
        // 3x3 block at the start; the window never exceeds 3 x 3 for far 10000.
        assert_eq!(log[0].1, 9);
        assert!(
            log.iter().all(|(_, n, _)| *n <= 12),
            "at most one extra column while crossing"
        );
        // The near actor (at 5500) leaves once the ROI (4250 + box) no longer reaches it, the far
        // actor (at 45000) enters when the camera is within about 4250 of it.
        let exited_near = exited_near.expect("near actor left");
        assert!((9_500.0..=10_500.0).contains(&exited_near), "{exited_near}");
        let entered_far = entered_far.expect("far actor entered");
        assert!(
            (40_500.0..=41_300.0).contains(&entered_far),
            "{entered_far}"
        );
        assert!(
            !scene
                .tree
                .get(near_actor)
                .unwrap()
                .has_entered_processing_range()
        );
        // Everything behind the camera has been unloaded; the first cells are gone.
        assert!(!resident.contains(&PvsCell { x: 100, z: 100 }));
    }

    #[test]
    fn killed_entities_are_destroyed_and_unregistered() {
        let mut scene = Scene::new(10_000.0);
        let a = scene.spawn_dynamic("victim", Vec3::new(100.0, 0.0, 0.0), Vec3::splat(40.0));
        let u = scene.update(Vec3::ZERO, &Lattice).unwrap();
        assert!(u.processing.entered.contains(&a));
        scene.kill(a);
        let u = scene.update(Vec3::ZERO, &Lattice).unwrap();
        assert_eq!(u.processing.exited, vec![a]);
        assert!(u.destroyed.contains(&a));
        assert!(scene.tree.get(a).is_none() && !scene.registry.is_registered(a));
    }

    #[test]
    fn illegal_camera_positions_are_refused() {
        let mut scene = Scene::new(10_000.0);
        assert!(
            scene
                .update(Vec3::new(f32::NAN, 0.0, 0.0), &Lattice)
                .is_err()
        );
        assert!(scene.update(Vec3::new(2.0e6, 0.0, 0.0), &Lattice).is_err());
    }
}
