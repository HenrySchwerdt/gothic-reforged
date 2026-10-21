//! `eCEntityAdmin`: the processing range (region of interest) and the processing list.
//!
//! Every frame the admin takes the camera position (or the player's). Two regions follow it, the
//! dynamic ROI sphere and the static ROI sphere, each widened by a hysteresis constant; a region
//! is only recomputed when the camera moved farther than that constant from the last centre.
//! On a recompute the admin visits the static entities of the valid PVS cells and all dynamic
//! entities, decides who is inside, and reports who left (exit hook first) and who entered.
use crate::entity::{Aabb, EntityTree, TreeEvent, flags};
use crate::pvs::{EntityId, PvsPrefetcher};
use bevy::math::Vec3;
use std::collections::HashSet;

/// `Entity.ROI` of `ge3.ini`, applied at start-up.
pub const DEFAULT_ROI: f32 = 4_000.0;
/// Engine default of the static ROI sphere (`Create`).
pub const DEFAULT_STATIC_ROI: f32 = 600.0;
/// Hysteresis added to both radii and used as the recompute threshold (`eCEntityAdmin+0x40`).
pub const DEFAULT_HYSTERESIS: f32 = 250.0;

/// Entities that crossed the border of the processing range in one [`EntityAdmin::process`].
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ProcessingDelta {
    /// `OnExitProcessingRange`, delivered before any enter.
    pub exited: Vec<EntityId>,
    /// `OnEnterProcessingRange`.
    pub entered: Vec<EntityId>,
}

impl ProcessingDelta {
    pub fn is_empty(&self) -> bool {
        self.exited.is_empty() && self.entered.is_empty()
    }
}

/// Step of the per-entity tick (`PreProcess`, `Process`, `PostProcess`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Phase {
    Pre,
    Process,
    Post,
}

#[derive(Debug, Clone, Copy)]
struct Region {
    centre: Vec3,
    radius: f32,
    bounds: Aabb,
}

impl Region {
    fn new(centre: Vec3, radius: f32) -> Self {
        Self {
            centre,
            radius,
            bounds: Aabb::around(centre, Vec3::splat(radius)),
        }
    }
}

/// Radii of the regions (`SetROISphere`, `SetROIStaticSphere`).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct RoiConfig {
    pub dynamic_radius: f32,
    pub static_radius: f32,
    pub hysteresis: f32,
}

impl Default for RoiConfig {
    fn default() -> Self {
        Self {
            dynamic_radius: DEFAULT_ROI,
            static_radius: DEFAULT_STATIC_ROI,
            hysteresis: DEFAULT_HYSTERESIS,
        }
    }
}

/// The processing-range engine.
#[derive(Debug)]
pub struct EntityAdmin {
    cfg: RoiConfig,
    dynamic: Option<Region>,
    previous: Option<Region>,
    static_region: Option<Region>,
    force_full: bool,
    processing_enabled: bool,
    inside: Vec<EntityId>,
    process_list: Vec<EntityId>,
    dirty: Vec<EntityId>,
    killed: Vec<EntityId>,
    pending_exits: Vec<EntityId>,
    /// Which PVS database supplies the static cells.
    pub database: u8,
}

impl Default for EntityAdmin {
    fn default() -> Self {
        Self::new()
    }
}

impl EntityAdmin {
    pub fn new() -> Self {
        Self {
            cfg: RoiConfig::default(),
            dynamic: None,
            previous: None,
            static_region: None,
            // The very first frame must evaluate regardless of where the camera is.
            force_full: true,
            processing_enabled: true,
            inside: Vec::new(),
            process_list: Vec::new(),
            dirty: Vec::new(),
            killed: Vec::new(),
            pending_exits: Vec::new(),
            database: 0,
        }
    }

    /// An admin that reads its static cells from PVS database `database`.
    pub fn for_database(database: u8) -> Self {
        Self {
            database,
            ..Self::new()
        }
    }

    pub fn config(&self) -> RoiConfig {
        self.cfg
    }
    /// `SetROISphere`.
    pub fn set_roi_radius(&mut self, r: f32) {
        self.cfg.dynamic_radius = r;
    }
    /// `SetROIStaticSphere`.
    pub fn set_static_roi_radius(&mut self, r: f32) {
        self.cfg.static_radius = r;
    }
    /// `EnableProcessing`: switches the per-frame entity tick, not the range bookkeeping.
    pub fn enable_processing(&mut self, on: bool) {
        self.processing_enabled = on;
    }
    pub fn is_processing_enabled(&self) -> bool {
        self.processing_enabled
    }
    /// `FourceFullROIUpdate`: forget the lists and re-evaluate everything next frame. Entities
    /// that were inside get their exit hook through the next recompute.
    pub fn force_full_update(&mut self) {
        self.force_full = true;
    }

    /// Entities currently inside the range (`HasProcessingRangeEntered`).
    pub fn in_range(&self) -> &[EntityId] {
        &self.inside
    }
    /// Entities that tick every frame.
    pub fn process_list(&self) -> &[EntityId] {
        &self.process_list
    }
    /// Entities waiting for destruction.
    pub fn killed(&self) -> &[EntityId] {
        &self.killed
    }
    /// The box of the current dynamic region.
    pub fn dynamic_region(&self) -> Option<Aabb> {
        self.dynamic.map(|r| r.bounds)
    }

    /// `IntersectsProcessingRangeRegion`.
    pub fn intersects_region(&self, b: &Aabb) -> bool {
        self.dynamic.is_some_and(|r| r.bounds.intersects(b))
    }
    /// `IntersectsStaticProcessingRangeRegion`.
    pub fn intersects_static_region(&self, b: &Aabb) -> bool {
        self.static_region.is_some_and(|r| r.bounds.intersects(b))
    }
    /// `IntersectsOldProcessingRangeRegion`: the region before the last recompute.
    pub fn intersects_old_region(&self, b: &Aabb) -> bool {
        self.previous.is_some_and(|r| r.bounds.intersects(b))
    }

    /// Port of `eCEntityAdmin::CullProcessingRangeRegion` (Engine:0x102317d0) for one box.
    pub fn cull(&self, b: &Aabb) -> Cull {
        let Some(r) = self.dynamic else {
            return Cull::Outside;
        };
        if !r.bounds.intersects(b) {
            return Cull::Outside;
        }
        // The sphere test of the original: a box whose farthest corner is inside the sphere.
        let far = Vec3::new(
            (b.min.x - r.centre.x)
                .abs()
                .max((b.max.x - r.centre.x).abs()),
            (b.min.y - r.centre.y)
                .abs()
                .max((b.max.y - r.centre.y).abs()),
            (b.min.z - r.centre.z)
                .abs()
                .max((b.max.z - r.centre.z).abs()),
        );
        if far.length() <= r.radius {
            Cull::Inside
        } else {
            Cull::Partial
        }
    }

    fn add_dirty(&mut self, tree: &EntityTree, id: EntityId, check: bool) {
        if tree.get(id).is_none_or(|e| e.is_killed()) {
            return;
        }
        if check && self.dirty.contains(&id) {
            return;
        }
        self.dirty.push(id);
    }

    fn remove_from_lists(&mut self, id: EntityId) {
        self.inside.retain(|e| *e != id);
        self.process_list.retain(|e| *e != id);
        self.dirty.retain(|e| *e != id);
    }

    fn drain_tree(&mut self, tree: &mut EntityTree, prefetch: &mut PvsPrefetcher) {
        for ev in tree.take_events() {
            match ev {
                TreeEvent::Dirty {
                    id,
                    check_duplicate,
                } => self.add_dirty(tree, id, check_duplicate),
                TreeEvent::ProcessingChanged(id) => self.on_entity_processing_changed(tree, id),
                TreeEvent::Killed(id) => {
                    self.killed.push(id);
                }
                TreeEvent::ExitRange(id) => {
                    self.remove_from_lists(id);
                    self.pending_exits.push(id);
                }
                TreeEvent::Moved { id, old, new } => {
                    prefetch.recalibrate(self.database, id, old.to_array(), new.to_array());
                }
            }
        }
    }

    /// Port of `eCEntityAdmin::OnEntityProcessingChanged` (Engine:0x10231620): an entity in range
    /// joins or leaves the processing list when its enabled / disabled state changed.
    pub fn on_entity_processing_changed(&mut self, tree: &EntityTree, id: EntityId) {
        let Some(e) = tree.get(id) else { return };
        if !e.has_entered_processing_range() {
            return;
        }
        let ok = e.has(flags::PROCESS_REQUIRED) && e.flags & flags::PROCESS_FORBIDDEN == 0;
        let at = self.process_list.iter().position(|x| *x == id);
        match (ok, at) {
            (true, None) => self.process_list.push(id),
            (false, Some(i)) => {
                self.process_list.swap_remove(i);
            }
            _ => {}
        }
    }

    /// Does `id` qualify for the processing range? The shared rule of the three collectors.
    /// `static_rule` selects `CollectStaticProcessingRangeEntity` (box test always, flags
    /// `0x100100`), otherwise the dynamic and dirty rule (flags `0x100108`, box test only for
    /// deactivatable entities).
    fn qualifies(&self, tree: &EntityTree, id: EntityId, static_rule: bool) -> bool {
        let Some(e) = tree.get(id) else { return false };
        if e.is_killed() {
            return false;
        }
        let need = if static_rule {
            flags::STATIC_CANDIDATE
        } else {
            flags::DYNAMIC_CANDIDATE
        };
        if !e.has(need) {
            return false;
        }
        if !static_rule && !e.can_be_deactivated() {
            return true;
        }
        e.node_box().is_some_and(|b| match self.cull(&b) {
            Cull::Outside => false,
            Cull::Inside | Cull::Partial => true,
        })
    }

    fn list_ok(tree: &EntityTree, id: EntityId) -> bool {
        tree.get(id).is_some_and(|e| {
            e.has(flags::PROCESS_REQUIRED) && e.flags & flags::PROCESS_FORBIDDEN == 0
        })
    }

    /// Port of `eCEntityAdmin::Process` (Engine:0x10230a70), the range part.
    ///
    /// `camera` is the active camera position (or the player's). Static candidates come from the
    /// valid cells of the admin's PVS database, dynamic ones from the whole tree.
    pub fn process(
        &mut self,
        tree: &mut EntityTree,
        prefetch: &mut PvsPrefetcher,
        camera: Vec3,
    ) -> ProcessingDelta {
        self.drain_tree(tree, prefetch);
        let hyst = self.cfg.hysteresis;
        let forced = self.force_full;

        if self
            .static_region
            .is_none_or(|r| r.centre.distance(camera) > hyst)
            || forced
        {
            self.static_region = Some(Region::new(camera, self.cfg.static_radius + hyst));
        }

        let mut enter: Vec<EntityId> = Vec::new();
        let mut exit: Vec<EntityId> = std::mem::take(&mut self.pending_exits);

        let recompute = self
            .dynamic
            .is_none_or(|r| r.centre.distance(camera) > hyst)
            || forced;
        if recompute {
            self.previous = self.dynamic;
            self.dynamic = Some(Region::new(camera, self.cfg.dynamic_radius + hyst));
            self.inside.clear();
            self.process_list.clear();
            self.dirty.clear();
            // Static entities of the valid cells.
            for id in prefetch.entities_in_valid_cells(self.database) {
                let was = tree
                    .get(id)
                    .is_some_and(|e| e.has_entered_processing_range());
                if self.qualifies(tree, id, true) {
                    self.inside.push(id);
                    if !was {
                        enter.push(id);
                    }
                } else if was {
                    exit.push(id);
                }
            }
            // Dynamic entities.
            let dyn_ids: Vec<EntityId> = tree.dynamic_entities().map(|e| e.id).collect();
            for id in dyn_ids {
                let was = tree
                    .get(id)
                    .is_some_and(|e| e.has_entered_processing_range());
                if self.qualifies(tree, id, false) {
                    self.inside.push(id);
                    if !was {
                        enter.push(id);
                    }
                    if Self::list_ok(tree, id) {
                        self.process_list.push(id);
                    }
                } else if was {
                    exit.push(id);
                }
            }
            // Anything still flagged as inside but no longer visited (its cell left the grid and
            // it was not cached out explicitly) leaves as well.
            let inside: HashSet<EntityId> = self.inside.iter().copied().collect();
            for e in tree.iter() {
                if e.has_entered_processing_range() && !inside.contains(&e.id) {
                    exit.push(e.id);
                }
            }
            self.force_full = false;
        } else if !self.dirty.is_empty() {
            // UpdateProcessingRangeEntity, newest first.
            let dirty = std::mem::take(&mut self.dirty);
            for id in dirty.into_iter().rev() {
                let was = tree
                    .get(id)
                    .is_some_and(|e| e.has_entered_processing_range());
                if self.qualifies(tree, id, false) {
                    if was {
                        continue;
                    }
                    self.inside.push(id);
                    if Self::list_ok(tree, id) {
                        self.process_list.push(id);
                    }
                    enter.push(id);
                } else if was {
                    exit.push(id);
                }
            }
        }

        // Exit hooks first.
        let mut delta = ProcessingDelta::default();
        let mut seen = HashSet::new();
        for id in exit {
            if !seen.insert(id) {
                continue;
            }
            if enter.contains(&id) {
                // The original treats an entity on both lists as a fatal error.
                debug_assert!(false, "entity {} both entered and exited", id.0);
                enter.retain(|e| *e != id);
            }
            tree.set_flags(id, flags::IN_RANGE, false);
            self.remove_from_lists(id);
            delta.exited.push(id);
        }
        // Enter hooks: only cached-in, not yet entered entities.
        for id in enter {
            let ok = tree
                .get(id)
                .is_some_and(|e| e.is_cached_in() && !e.has_entered_processing_range());
            if ok {
                tree.set_flags(id, flags::IN_RANGE, true);
                delta.entered.push(id);
            } else {
                self.remove_from_lists(id);
            }
        }
        delta
    }

    /// The tick part of `Process`: for every entity of the processing list that is dynamic,
    /// inside, alive, enabled and not processing-disabled, run `PreProcess`, `Process` and
    /// `PostProcess` (`ProcessDynamicEntity`).
    pub fn tick(&self, tree: &EntityTree, mut f: impl FnMut(EntityId, Phase)) -> usize {
        if !self.processing_enabled {
            return 0;
        }
        let mut n = 0;
        for &id in &self.process_list {
            let Some(e) = tree.get(id) else { continue };
            if e.is_dynamic()
                && e.has_entered_processing_range()
                && !e.is_killed()
                && e.is_enabled()
                && !e.is_processing_disabled()
            {
                f(id, Phase::Pre);
                f(id, Phase::Process);
                f(id, Phase::Post);
                n += 1;
            }
        }
        n
    }

    /// Port of `eCEntityAdmin::KillEntities`: destroy the queued entities (newest first) and
    /// hand back every id that disappeared so registries can forget them.
    pub fn flush_kills(&mut self, tree: &mut EntityTree) -> Vec<EntityId> {
        let mut gone = Vec::new();
        for id in std::mem::take(&mut self.killed).into_iter().rev() {
            for d in tree.destroy(id) {
                self.remove_from_lists(d);
                gone.push(d);
            }
        }
        gone
    }

    /// `RemoveEarlyKilledEntity`: take an entity back off the kill list.
    pub fn remove_early_killed(&mut self, id: EntityId) -> bool {
        let n = self.killed.len();
        self.killed.retain(|e| *e != id);
        n != self.killed.len()
    }
}

/// Result of [`EntityAdmin::cull`] (`eEProcessingRangeCull`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Cull {
    Outside,
    /// The whole box lies inside the ROI sphere: members need no individual test.
    Inside,
    Partial,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pvs::{MAIN_CELL_SIZE, PvsSphere};

    fn actor(t: &mut EntityTree, name: &str, at: Vec3) -> EntityId {
        let id = t.create(name, true);
        t.set_local_node_box(
            id,
            Some(Aabb::around(Vec3::ZERO, Vec3::new(40.0, 90.0, 40.0))),
        )
        .unwrap();
        t.set_local_position(id, at).unwrap();
        t.cache_in(id);
        id
    }

    fn setup() -> (EntityTree, PvsPrefetcher, EntityAdmin) {
        let mut p = PvsPrefetcher::new();
        let db = p.register_database(MAIN_CELL_SIZE).unwrap();
        (EntityTree::new(), p, EntityAdmin::for_database(db))
    }

    #[test]
    fn dynamic_entities_enter_and_leave_with_the_camera() {
        let (mut t, mut p, mut a) = setup();
        let near = actor(&mut t, "near", Vec3::new(1_000.0, 0.0, 0.0));
        let far = actor(&mut t, "far", Vec3::new(9_000.0, 0.0, 0.0));
        let d = a.process(&mut t, &mut p, Vec3::ZERO);
        assert_eq!(d.entered, vec![near]);
        assert!(d.exited.is_empty());
        assert!(t.get(near).unwrap().has_entered_processing_range());
        assert!(!t.get(far).unwrap().has_entered_processing_range());
        // Walk toward the far entity: the near one leaves first in the same recompute.
        let d = a.process(&mut t, &mut p, Vec3::new(8_000.0, 0.0, 0.0));
        assert_eq!(d.exited, vec![near]);
        assert_eq!(d.entered, vec![far]);
        assert_eq!(a.in_range(), &[far]);
        assert_eq!(a.process_list(), &[far]);
    }

    #[test]
    fn region_is_recomputed_only_after_the_hysteresis_distance() {
        let (mut t, mut p, mut a) = setup();
        // Edge of the region: ROI 4000 + hysteresis 250 reach 4250 from the camera; the box of
        // the entity adds 40.
        let e = actor(&mut t, "e", Vec3::new(4_400.0, 0.0, 0.0));
        a.process(&mut t, &mut p, Vec3::ZERO);
        assert!(!t.get(e).unwrap().has_entered_processing_range());
        // 200 units toward it: below the 250 hysteresis, region unchanged.
        let d = a.process(&mut t, &mut p, Vec3::new(200.0, 0.0, 0.0));
        assert!(d.is_empty());
        // 300 units: recomputed around the new centre, 4400 - 300 = 4100 < 4250 + 40.
        let d = a.process(&mut t, &mut p, Vec3::new(300.0, 0.0, 0.0));
        assert_eq!(d.entered, vec![e]);
    }

    #[test]
    fn deactivatable_flag_controls_the_box_test() {
        let (mut t, mut p, mut a) = setup();
        let always = actor(&mut t, "always", Vec3::new(50_000.0, 0.0, 0.0));
        t.enable_deactivation(always, false);
        t.cache_in(always);
        let boxed = actor(&mut t, "boxed", Vec3::new(50_000.0, 0.0, 0.0));
        let d = a.process(&mut t, &mut p, Vec3::ZERO);
        assert_eq!(d.entered, vec![always]);
        assert!(!t.get(boxed).unwrap().has_entered_processing_range());
    }

    #[test]
    fn candidates_need_cache_in_enable_and_a_valid_box() {
        let (mut t, mut p, mut a) = setup();
        let uncached = t.create("u", true);
        t.set_local_node_box(uncached, Some(Aabb::around(Vec3::ZERO, Vec3::splat(10.0))))
            .unwrap();
        let disabled = actor(&mut t, "d", Vec3::new(100.0, 0.0, 0.0));
        t.enable(disabled, false);
        let nobox = t.create("n", true);
        t.cache_in(nobox);
        let d = a.process(&mut t, &mut p, Vec3::ZERO);
        assert!(d.is_empty());
        // Enabling queues nothing: the entity is picked up by the next recompute.
        t.enable(disabled, true);
        assert!(a.process(&mut t, &mut p, Vec3::ZERO).is_empty());
        a.force_full_update();
        let d = a.process(&mut t, &mut p, Vec3::ZERO);
        assert_eq!(d.entered, vec![disabled]);
        assert!(!a.in_range().contains(&uncached) && !a.in_range().contains(&nobox));
    }

    #[test]
    fn dirty_entities_are_picked_up_between_recomputes() {
        let (mut t, mut p, mut a) = setup();
        let e = actor(&mut t, "e", Vec3::new(20_000.0, 0.0, 0.0));
        a.process(&mut t, &mut p, Vec3::ZERO);
        assert!(!t.get(e).unwrap().has_entered_processing_range());
        // Teleport next to the camera; no camera movement, so only the dirty path can see it.
        t.set_local_position(e, Vec3::new(500.0, 0.0, 0.0)).unwrap();
        let d = a.process(&mut t, &mut p, Vec3::ZERO);
        assert_eq!(d.entered, vec![e]);
        // And away again.
        t.set_local_position(e, Vec3::new(30_000.0, 0.0, 0.0))
            .unwrap();
        let d = a.process(&mut t, &mut p, Vec3::ZERO);
        assert_eq!(d.exited, vec![e]);
    }

    #[test]
    fn kill_and_cache_out_leave_the_range() {
        let (mut t, mut p, mut a) = setup();
        let k = actor(&mut t, "k", Vec3::new(100.0, 0.0, 0.0));
        let c = actor(&mut t, "c", Vec3::new(200.0, 0.0, 0.0));
        a.process(&mut t, &mut p, Vec3::ZERO);
        t.kill(k);
        t.cache_out(c, false, false);
        let d = a.process(&mut t, &mut p, Vec3::ZERO);
        let mut exited = d.exited.clone();
        exited.sort();
        assert_eq!(exited, vec![k, c]);
        assert!(a.in_range().is_empty());
        assert_eq!(a.killed(), &[k]);
        assert_eq!(a.flush_kills(&mut t), vec![k]);
        assert!(t.get(k).is_none());
    }

    #[test]
    fn processing_list_follows_enable_and_disable() {
        let (mut t, mut p, mut a) = setup();
        let e = actor(&mut t, "e", Vec3::new(100.0, 0.0, 0.0));
        a.process(&mut t, &mut p, Vec3::ZERO);
        assert_eq!(a.process_list(), &[e]);
        t.disable_processing(e, true);
        a.process(&mut t, &mut p, Vec3::ZERO);
        assert!(a.process_list().is_empty());
        assert_eq!(a.in_range(), &[e]);
        t.disable_processing(e, false);
        a.process(&mut t, &mut p, Vec3::ZERO);
        assert_eq!(a.process_list(), &[e]);
    }

    #[test]
    fn tick_runs_three_phases_per_entity_and_honours_the_switch() {
        let (mut t, mut p, mut a) = setup();
        let e1 = actor(&mut t, "e1", Vec3::new(100.0, 0.0, 0.0));
        let e2 = actor(&mut t, "e2", Vec3::new(200.0, 0.0, 0.0));
        a.process(&mut t, &mut p, Vec3::ZERO);
        let mut log = Vec::new();
        assert_eq!(a.tick(&t, |id, ph| log.push((id, ph))), 2);
        assert_eq!(log.len(), 6);
        assert_eq!(
            &log[..3],
            &[(e1, Phase::Pre), (e1, Phase::Process), (e1, Phase::Post)]
        );
        assert_eq!(log[3].0, e2);
        a.enable_processing(false);
        assert_eq!(a.tick(&t, |_, _| {}), 0);
    }

    #[test]
    fn static_entities_come_from_valid_cells_and_leave_with_them() {
        let (mut t, mut p, mut a) = setup();
        let db = a.database;
        let s = t.create("rock", false);
        t.set_local_node_box(s, Some(Aabb::around(Vec3::ZERO, Vec3::splat(100.0))))
            .unwrap();
        t.set_local_position(s, Vec3::new(2_000.0, 0.0, 1_000.0))
            .unwrap();
        t.cache_in(s);
        let pos = t.get(s).unwrap().world_position();
        p.add_entity(db, s, pos.to_array(), 1);
        // The grid has to know the cell before the admin sees the entity.
        let sph = PvsSphere::from_camera([0.0, 0.0, 0.0], 10_000.0, false);
        p.process(db, sph, false).unwrap();
        let d = a.process(&mut t, &mut p, Vec3::ZERO);
        assert_eq!(d.entered, vec![s]);
        assert!(a.process_list().is_empty(), "static entities never tick");
        // Camera far away: the cell leaves the grid and the entity with it.
        let far = Vec3::new(200_000.0, 0.0, 0.0);
        p.process(
            db,
            PvsSphere::from_camera(far.to_array(), 10_000.0, false),
            false,
        )
        .unwrap();
        t.cache_out(s, false, false);
        let d = a.process(&mut t, &mut p, far);
        assert_eq!(d.exited, vec![s]);
    }

    #[test]
    fn cull_distinguishes_inside_partial_outside() {
        let (mut t, mut p, mut a) = setup();
        a.process(&mut t, &mut p, Vec3::ZERO);
        let small = Aabb::around(Vec3::new(100.0, 0.0, 0.0), Vec3::splat(10.0));
        assert_eq!(a.cull(&small), Cull::Inside);
        let edge = Aabb::around(Vec3::new(4_200.0, 0.0, 0.0), Vec3::splat(100.0));
        assert_eq!(a.cull(&edge), Cull::Partial);
        let outside = Aabb::around(Vec3::new(9_000.0, 0.0, 0.0), Vec3::splat(10.0));
        assert_eq!(a.cull(&outside), Cull::Outside);
        assert!(a.intersects_static_region(&small));
        assert!(!a.intersects_static_region(&edge));
    }
}
