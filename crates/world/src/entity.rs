//! `eCEntity` core: flags, hierarchy, transforms, node/tree boundaries and the enable / disable /
//! kill / cache propagation rules that the entity admin builds on.
//!
//! This is the engine-side model of an entity, not a rendering object. Presentation entities in
//! Bevy mirror it; the simulation asks it which entities are processed.
use crate::pvs::EntityId;
use bevy::math::{Affine3A, Vec3, Vec3A};
use std::collections::BTreeMap;

/// Bits of `eCEntity+0x3c`.
pub mod flags {
    /// `DisableProcessing(true)`; the entity and its children skip Pre/Process/PostProcess.
    pub const PROCESSING_DISABLED: u32 = 0x2;
    /// `Enable(true)`.
    pub const ENABLED: u32 = 0x8;
    /// `EnableDeactivation`: the entity is only processed while its box touches the ROI.
    pub const DEACTIVATABLE: u32 = 0x20;
    /// `Kill`.
    pub const KILLED: u32 = 0x40;
    /// Inside the processing range (`HasProcessingRangeEntered`).
    pub const IN_RANGE: u32 = 0x80;
    /// Cached in (`IsCachedIn`): geometry and property sets are resident.
    pub const CACHED_IN: u32 = 0x100;
    /// The cache-out left stale internals; refreshed on the next cache-in.
    pub const UPDATE_PENDING: u32 = 0x4_0000;
    /// The world node boundary (`+0xe8`) holds a valid box.
    pub const NODE_BOX_VALID: u32 = 0x10_0000;
    /// The world tree boundary (`+0xc0`) holds a valid box.
    pub const TREE_BOX_VALID: u32 = 0x20_0000;
    /// A dynamic entity (`eCDynamicEntity`); only these tick.
    pub const DYNAMIC: u32 = 0x80_0000;
    /// Processable (bit tested by `IsProcessable`).
    pub const PROCESSABLE: u32 = 0x400_0000;
    /// Temporary (not saved) entity.
    pub const TEMPORARY: u32 = 0x2000_0000;
    /// State after construction (`eCEntity::Invalidate`: `0x1828` is enabled, deactivatable and
    /// two bits whose meaning is unknown).
    pub const DEFAULT: u32 = 0x1828;
    /// Flags a dynamic entity needs to be a processing candidate (`0x100108`).
    pub const DYNAMIC_CANDIDATE: u32 = CACHED_IN | NODE_BOX_VALID | ENABLED;
    /// Flags a static entity needs to be a processing candidate (`0x100100`).
    pub const STATIC_CANDIDATE: u32 = CACHED_IN | NODE_BOX_VALID;
    /// Required flags of the processing list (`0x4800008`, `eCEntityAdmin+0xd0`).
    pub const PROCESS_REQUIRED: u32 = PROCESSABLE | DYNAMIC | ENABLED;
    /// Forbidden flags of the processing list (`0x42`, `eCEntityAdmin+0xd4`).
    pub const PROCESS_FORBIDDEN: u32 = KILLED | PROCESSING_DISABLED;
}

/// Axis-aligned box (`bCBox`).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Aabb {
    pub min: Vec3,
    pub max: Vec3,
}

impl Aabb {
    pub fn new(min: Vec3, max: Vec3) -> Self {
        Self { min, max }
    }
    /// Box around a point.
    pub fn around(centre: Vec3, half: Vec3) -> Self {
        Self::new(centre - half, centre + half)
    }
    /// `bCBox::IsValid`: finite and not inverted.
    pub fn is_valid(&self) -> bool {
        self.min.is_finite() && self.max.is_finite() && self.min.cmple(self.max).all()
    }
    pub fn centre(&self) -> Vec3 {
        (self.min + self.max) * 0.5
    }
    /// Touching boxes intersect (inclusive comparisons, like the region tests).
    pub fn intersects(&self, o: &Aabb) -> bool {
        self.min.cmple(o.max).all() && o.min.cmple(self.max).all()
    }
    pub fn contains_box(&self, o: &Aabb) -> bool {
        self.min.cmple(o.min).all() && o.max.cmple(self.max).all()
    }
    /// `bCBox::Merge`.
    pub fn merge(&self, o: &Aabb) -> Aabb {
        Aabb::new(self.min.min(o.min), self.max.max(o.max))
    }
    /// The box of the eight corners transformed by `m` (`bCBox::operator*=`).
    pub fn transformed(&self, m: &Affine3A) -> Aabb {
        let mut lo = Vec3::splat(f32::INFINITY);
        let mut hi = Vec3::splat(f32::NEG_INFINITY);
        for i in 0..8 {
            let c = Vec3::new(
                if i & 1 == 0 { self.min.x } else { self.max.x },
                if i & 2 == 0 { self.min.y } else { self.max.y },
                if i & 4 == 0 { self.min.z } else { self.max.z },
            );
            let p = m.transform_point3(c);
            lo = lo.min(p);
            hi = hi.max(p);
        }
        Aabb::new(lo, hi)
    }
}

/// One entity of the scene.
#[derive(Debug, Clone)]
pub struct Entity {
    pub id: EntityId,
    pub name: String,
    pub flags: u32,
    parent: Option<EntityId>,
    children: Vec<EntityId>,
    local: Affine3A,
    world: Affine3A,
    local_box: Option<Aabb>,
    node_box: Option<Aabb>,
    tree_box: Option<Aabb>,
    /// Template this entity was created from (`GetTemplateID`).
    pub template: Option<String>,
}

impl Entity {
    pub fn has(&self, mask: u32) -> bool {
        self.flags & mask == mask
    }
    pub fn is_enabled(&self) -> bool {
        self.has(flags::ENABLED)
    }
    pub fn is_killed(&self) -> bool {
        self.has(flags::KILLED)
    }
    pub fn is_dynamic(&self) -> bool {
        self.has(flags::DYNAMIC)
    }
    pub fn is_cached_in(&self) -> bool {
        self.has(flags::CACHED_IN)
    }
    pub fn has_entered_processing_range(&self) -> bool {
        self.has(flags::IN_RANGE)
    }
    pub fn is_processing_disabled(&self) -> bool {
        self.has(flags::PROCESSING_DISABLED)
    }
    pub fn can_be_deactivated(&self) -> bool {
        self.has(flags::DEACTIVATABLE)
    }
    pub fn parent(&self) -> Option<EntityId> {
        self.parent
    }
    pub fn children(&self) -> &[EntityId] {
        &self.children
    }
    pub fn local_matrix(&self) -> Affine3A {
        self.local
    }
    pub fn world_matrix(&self) -> Affine3A {
        self.world
    }
    pub fn world_position(&self) -> Vec3 {
        Vec3::from(self.world.translation)
    }
    pub fn local_position(&self) -> Vec3 {
        Vec3::from(self.local.translation)
    }
    /// World box of the entity itself (`+0xe8`); the processing range test uses it.
    pub fn node_box(&self) -> Option<Aabb> {
        self.node_box
    }
    /// World box of the entity and all descendants (`+0xc0`).
    pub fn tree_box(&self) -> Option<Aabb> {
        self.tree_box
    }
    pub fn local_box(&self) -> Option<Aabb> {
        self.local_box
    }
}

/// Changes the entity admin has to hear about (`AddDirtyProcessingRangeEntity`,
/// `OnEntityProcessingChanged`, `AddKilledEntity`, `Recalibrate`).
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum TreeEvent {
    /// Re-test the entity against the processing range (`check_duplicate` skips queued ones).
    Dirty { id: EntityId, check_duplicate: bool },
    /// Enabled or processing-disabled state changed while in range.
    ProcessingChanged(EntityId),
    /// `Kill`: queued for destruction.
    Killed(EntityId),
    /// The entity left the processing range through a cache-out or kill.
    ExitRange(EntityId),
    /// The node box centre moved (`eCPVSPrefetcher3::Recalibrate`).
    Moved { id: EntityId, old: Vec3, new: Vec3 },
}

/// Why an entity operation failed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EntityError {
    Unknown(EntityId),
    /// Linking would make an entity its own ancestor.
    Cycle,
}

impl std::fmt::Display for EntityError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Unknown(id) => write!(f, "unknown entity {}", id.0),
            Self::Cycle => write!(f, "link would create a cycle"),
        }
    }
}
impl std::error::Error for EntityError {}

/// Arena of all entities with their hierarchy.
#[derive(Debug, Default)]
pub struct EntityTree {
    entities: BTreeMap<EntityId, Entity>,
    next: u64,
    events: Vec<TreeEvent>,
}

impl EntityTree {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn len(&self) -> usize {
        self.entities.len()
    }
    pub fn is_empty(&self) -> bool {
        self.entities.is_empty()
    }
    pub fn get(&self, id: EntityId) -> Option<&Entity> {
        self.entities.get(&id)
    }
    pub fn iter(&self) -> impl Iterator<Item = &Entity> {
        self.entities.values()
    }
    /// Events queued for the entity admin; the admin drains them.
    pub fn take_events(&mut self) -> Vec<TreeEvent> {
        std::mem::take(&mut self.events)
    }

    /// Create an entity. Dynamic entities carry the dynamic and processable bits that put them
    /// into the processing list.
    pub fn create(&mut self, name: &str, dynamic: bool) -> EntityId {
        let id = EntityId(self.next);
        self.next += 1;
        let mut f = flags::DEFAULT;
        if dynamic {
            f |= flags::DYNAMIC | flags::PROCESSABLE;
        }
        self.entities.insert(
            id,
            Entity {
                id,
                name: name.to_owned(),
                flags: f,
                parent: None,
                children: Vec::new(),
                local: Affine3A::IDENTITY,
                world: Affine3A::IDENTITY,
                local_box: None,
                node_box: None,
                tree_box: None,
                template: None,
            },
        );
        id
    }

    fn entity_mut(&mut self, id: EntityId) -> Result<&mut Entity, EntityError> {
        self.entities.get_mut(&id).ok_or(EntityError::Unknown(id))
    }

    /// Ids of `id` and all its descendants, parents before children.
    pub fn subtree(&self, id: EntityId) -> Vec<EntityId> {
        let mut out = Vec::new();
        let mut stack = vec![id];
        while let Some(e) = stack.pop() {
            if let Some(ent) = self.entities.get(&e) {
                out.push(e);
                stack.extend(ent.children.iter().rev().copied());
            }
        }
        out
    }

    /// Link `child` under `parent` (`eCNode` child list); `None` makes it a root. The child keeps
    /// its local matrix, so its world matrix follows the new parent.
    pub fn link(&mut self, child: EntityId, parent: Option<EntityId>) -> Result<(), EntityError> {
        if !self.entities.contains_key(&child) {
            return Err(EntityError::Unknown(child));
        }
        if let Some(p) = parent {
            if !self.entities.contains_key(&p) {
                return Err(EntityError::Unknown(p));
            }
            if self.subtree(child).contains(&p) {
                return Err(EntityError::Cycle);
            }
        }
        self.detach(child, false);
        if let Some(p) = parent {
            self.entity_mut(p)?.children.push(child);
        }
        self.entity_mut(child)?.parent = parent;
        self.update_child_dependencies(child);
        if let Some(p) = parent {
            self.update_parent_dependencies(p);
        }
        Ok(())
    }

    /// `eCEntity::RemoveFromParent`: detach from the parent; with `refresh` the parent's tree box
    /// is recomputed.
    pub fn remove_from_parent(&mut self, id: EntityId, refresh: bool) {
        self.detach(id, refresh);
        self.update_child_dependencies(id);
    }

    fn detach(&mut self, id: EntityId, refresh: bool) {
        let Some(parent) = self.entities.get(&id).and_then(Entity::parent) else {
            return;
        };
        if let Some(p) = self.entities.get_mut(&parent) {
            p.children.retain(|c| *c != id);
        }
        if let Some(e) = self.entities.get_mut(&id) {
            e.parent = None;
        }
        if refresh {
            self.update_parent_dependencies(parent);
        }
    }

    /// `SetLocalMatrix`: place the entity relative to its parent.
    pub fn set_local_matrix(&mut self, id: EntityId, m: Affine3A) -> Result<(), EntityError> {
        self.entity_mut(id)?.local = m;
        self.update_child_dependencies(id);
        if let Some(p) = self.entities.get(&id).and_then(Entity::parent) {
            self.update_parent_dependencies(p);
        }
        Ok(())
    }

    /// `SetLocalPosition`: keep the rotation and scale, replace the translation.
    pub fn set_local_position(&mut self, id: EntityId, p: Vec3) -> Result<(), EntityError> {
        let mut m = self
            .entities
            .get(&id)
            .ok_or(EntityError::Unknown(id))?
            .local;
        m.translation = Vec3A::from(p);
        self.set_local_matrix(id, m)
    }

    /// `SetWorldMatrix`: express a world placement in the parent's space.
    pub fn set_world_matrix(&mut self, id: EntityId, world: Affine3A) -> Result<(), EntityError> {
        let parent_world = self
            .entities
            .get(&id)
            .ok_or(EntityError::Unknown(id))?
            .parent
            .and_then(|p| self.entities.get(&p))
            .map_or(Affine3A::IDENTITY, |p| p.world);
        self.set_local_matrix(id, parent_world.inverse() * world)
    }

    /// `SetWorldPosition`.
    pub fn set_world_position(&mut self, id: EntityId, p: Vec3) -> Result<(), EntityError> {
        let mut w = self
            .entities
            .get(&id)
            .ok_or(EntityError::Unknown(id))?
            .world;
        w.translation = Vec3A::from(p);
        self.set_world_matrix(id, w)
    }

    /// `SetLocalNodeBoundary`: the entity's own box in its local space; invalid boxes clear it.
    /// The world node box and the flag follow (`SetWorldNodeBoundary`).
    pub fn set_local_node_box(
        &mut self,
        id: EntityId,
        local: Option<Aabb>,
    ) -> Result<(), EntityError> {
        self.entity_mut(id)?.local_box = local.filter(Aabb::is_valid);
        self.update_child_dependencies(id);
        if let Some(p) = self.entities.get(&id).and_then(Entity::parent) {
            self.update_parent_dependencies(p);
        }
        Ok(())
    }

    /// Port of `eCEntity::UpdateChildDependencies` (Engine:0x10227aa0): world matrix = parent
    /// world * local, the world node box follows, the moved centre is reported and the change
    /// recurses into the children.
    fn update_child_dependencies(&mut self, id: EntityId) {
        let Some(e) = self.entities.get(&id) else {
            return;
        };
        let parent_world = e
            .parent
            .and_then(|p| self.entities.get(&p))
            .map(|p| p.world);
        let world = parent_world.map_or(e.local, |pw| pw * e.local);
        let old_centre = e.node_box.map(|b| b.centre());
        let was_cached = e.is_cached_in();
        let node_box = e
            .local_box
            .map(|b| b.transformed(&world))
            .filter(Aabb::is_valid);
        let children = e.children.clone();
        let Some(e) = self.entities.get_mut(&id) else {
            return;
        };
        e.world = world;
        e.node_box = node_box;
        if node_box.is_some() {
            e.flags |= flags::NODE_BOX_VALID;
        } else {
            e.flags &= !flags::NODE_BOX_VALID;
        }
        if let (Some(old), Some(new)) = (old_centre, node_box.map(|b| b.centre()))
            && old != new
        {
            self.events.push(TreeEvent::Moved { id, old, new });
        }
        if was_cached {
            self.events.push(TreeEvent::Dirty {
                id,
                check_duplicate: true,
            });
        }
        for c in children {
            self.update_child_dependencies(c);
        }
        self.recompute_tree_box(id);
    }

    /// Port of `eCEntity::UpdateParentDependencies` (Engine:0x10227860): the tree box of an
    /// entity is its node box merged with the tree boxes of its children; the change bubbles up.
    fn update_parent_dependencies(&mut self, id: EntityId) {
        let mut cur = Some(id);
        while let Some(c) = cur {
            self.recompute_tree_box(c);
            cur = self.entities.get(&c).and_then(Entity::parent);
        }
    }

    fn recompute_tree_box(&mut self, id: EntityId) {
        let Some(e) = self.entities.get(&id) else {
            return;
        };
        let mut b = e.node_box;
        for c in &e.children {
            if let Some(cb) = self.entities.get(c).and_then(|c| c.tree_box) {
                b = Some(b.map_or(cb, |x| x.merge(&cb)));
            }
        }
        if let Some(e) = self.entities.get_mut(&id) {
            e.tree_box = b;
            if b.is_some() {
                e.flags |= flags::TREE_BOX_VALID;
            } else {
                e.flags &= !flags::TREE_BOX_VALID;
            }
        }
    }

    /// `eCEntity::Enable`: set the enabled bit on the entity and all descendants.
    pub fn enable(&mut self, id: EntityId, on: bool) {
        for e in self.subtree(id) {
            let Some(ent) = self.entities.get_mut(&e) else {
                continue;
            };
            let before = ent.flags;
            set_flag(&mut ent.flags, flags::ENABLED, on);
            if before != ent.flags && ent.has_entered_processing_range() {
                self.events.push(TreeEvent::ProcessingChanged(e));
            }
        }
    }

    /// `eCEntity::DisableProcessing`: toggles the bit on all descendants; entities already in
    /// range tell the admin so the processing list follows.
    pub fn disable_processing(&mut self, id: EntityId, off: bool) {
        for e in self.subtree(id) {
            let Some(ent) = self.entities.get_mut(&e) else {
                continue;
            };
            if ent.is_processing_disabled() == off {
                continue;
            }
            set_flag(&mut ent.flags, flags::PROCESSING_DISABLED, off);
            if ent.has_entered_processing_range() {
                self.events.push(TreeEvent::ProcessingChanged(e));
            }
        }
    }

    /// `eCEntity::EnableDeactivation`: the entity is cached out first, the bit is set on the
    /// subtree, and an entity that was cached in (or is being made always-active) is announced
    /// to the prefetcher again.
    pub fn enable_deactivation(&mut self, id: EntityId, on: bool) {
        let was_cached = self.get(id).is_some_and(Entity::is_cached_in);
        self.cache_out(id, false, false);
        for e in self.subtree(id) {
            if let Some(ent) = self.entities.get_mut(&e) {
                set_flag(&mut ent.flags, flags::DEACTIVATABLE, on);
            }
        }
        if was_cached || !on {
            self.events.push(TreeEvent::Dirty {
                id,
                check_duplicate: false,
            });
        }
    }

    /// Port of `eCEntity::CacheInInternal` (Engine:0x10224030): make resident and queue for the
    /// processing-range test.
    pub fn cache_in(&mut self, id: EntityId) {
        let Some(e) = self.entities.get_mut(&id) else {
            return;
        };
        if e.is_cached_in() {
            return;
        }
        e.flags &= !flags::UPDATE_PENDING;
        e.flags |= flags::CACHED_IN;
        self.events.push(TreeEvent::Dirty {
            id,
            check_duplicate: false,
        });
    }

    /// Port of `eCEntity::CacheOutInternal` (Engine:0x10224150): leave the processing range,
    /// then drop residency. `mark_stale` sets the update-pending bit. `recursive` also caches out
    /// the children.
    pub fn cache_out(&mut self, id: EntityId, mark_stale: bool, recursive: bool) {
        let ids = if recursive {
            self.subtree(id)
        } else {
            vec![id]
        };
        for e in ids {
            let Some(ent) = self.entities.get_mut(&e) else {
                continue;
            };
            if ent.has_entered_processing_range() {
                ent.flags &= !flags::IN_RANGE;
                self.events.push(TreeEvent::ExitRange(e));
            }
            let Some(ent) = self.entities.get_mut(&e) else {
                continue;
            };
            set_flag(&mut ent.flags, flags::UPDATE_PENDING, mark_stale);
            ent.flags &= !flags::CACHED_IN;
        }
    }

    /// `eCEntity::Kill` / `KillIntern`: the whole subtree is flagged and leaves the processing
    /// range; the entity is queued once for the admin's kill list.
    pub fn kill(&mut self, id: EntityId) {
        let Some(e) = self.entities.get(&id) else {
            return;
        };
        if e.is_killed() {
            return;
        }
        self.events.push(TreeEvent::Killed(id));
        for s in self.subtree(id) {
            if let Some(ent) = self.entities.get_mut(&s) {
                ent.flags |= flags::KILLED;
                if ent.has_entered_processing_range() {
                    ent.flags &= !flags::IN_RANGE;
                    self.events.push(TreeEvent::ExitRange(s));
                }
            }
        }
    }

    /// Remove an entity and its subtree from the arena (after the kill list was flushed).
    pub fn destroy(&mut self, id: EntityId) -> Vec<EntityId> {
        self.detach(id, true);
        let ids = self.subtree(id);
        for e in &ids {
            self.entities.remove(e);
        }
        ids
    }

    /// Set or clear flags directly (used by the admin for the in-range bit).
    pub(crate) fn set_flags(&mut self, id: EntityId, mask: u32, on: bool) {
        if let Some(e) = self.entities.get_mut(&id) {
            set_flag(&mut e.flags, mask, on);
        }
    }

    /// Every dynamic entity (`eCEntityDynamicContext` members).
    pub fn dynamic_entities(&self) -> impl Iterator<Item = &Entity> {
        self.entities.values().filter(|e| e.is_dynamic())
    }

    /// `eCEntity::GetDistanceToEntity` (world origins).
    pub fn distance(&self, a: EntityId, b: EntityId) -> Option<f32> {
        Some(
            self.entities
                .get(&a)?
                .world_position()
                .distance(self.entities.get(&b)?.world_position()),
        )
    }

    /// `eCEntity::GetRoot`: the topmost ancestor.
    pub fn root(&self, id: EntityId) -> Option<EntityId> {
        let mut cur = id;
        self.entities.get(&cur)?;
        while let Some(p) = self.entities.get(&cur).and_then(Entity::parent) {
            cur = p;
        }
        Some(cur)
    }
}

fn set_flag(f: &mut u32, mask: u32, on: bool) {
    if on {
        *f |= mask;
    } else {
        *f &= !mask;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn boxed(t: &mut EntityTree, name: &str, at: Vec3) -> EntityId {
        let id = t.create(name, true);
        t.set_local_node_box(id, Some(Aabb::around(Vec3::ZERO, Vec3::splat(50.0))))
            .unwrap();
        t.set_local_position(id, at).unwrap();
        id
    }

    #[test]
    fn world_matrix_follows_parent_and_children_move_with_it() {
        let mut t = EntityTree::new();
        let parent = boxed(&mut t, "p", Vec3::new(1000.0, 0.0, 0.0));
        let child = boxed(&mut t, "c", Vec3::new(0.0, 0.0, 200.0));
        t.link(child, Some(parent)).unwrap();
        assert_eq!(
            t.get(child).unwrap().world_position(),
            Vec3::new(1000.0, 0.0, 200.0)
        );
        t.set_local_position(parent, Vec3::new(0.0, 500.0, 0.0))
            .unwrap();
        assert_eq!(
            t.get(child).unwrap().world_position(),
            Vec3::new(0.0, 500.0, 200.0)
        );
        // Tree box of the parent covers the child.
        let tb = t.get(parent).unwrap().tree_box().unwrap();
        assert!(tb.contains_box(&t.get(child).unwrap().node_box().unwrap()));
        assert!(t.get(parent).unwrap().has(flags::TREE_BOX_VALID));
        t.remove_from_parent(child, true);
        assert_eq!(
            t.get(child).unwrap().world_position(),
            Vec3::new(0.0, 0.0, 200.0)
        );
        let tb = t.get(parent).unwrap().tree_box().unwrap();
        assert!(tb.max.z < 100.0);
    }

    #[test]
    fn link_rejects_cycles_and_unknown_ids() {
        let mut t = EntityTree::new();
        let a = t.create("a", false);
        let b = t.create("b", false);
        t.link(b, Some(a)).unwrap();
        assert_eq!(t.link(a, Some(b)), Err(EntityError::Cycle));
        assert_eq!(t.link(a, Some(a)), Err(EntityError::Cycle));
        assert!(matches!(
            t.link(a, Some(EntityId(99))),
            Err(EntityError::Unknown(_))
        ));
        assert_eq!(t.root(b), Some(a));
    }

    #[test]
    fn set_world_position_respects_the_parent() {
        let mut t = EntityTree::new();
        let p = boxed(&mut t, "p", Vec3::new(100.0, 0.0, 0.0));
        let c = t.create("c", false);
        t.link(c, Some(p)).unwrap();
        t.set_world_position(c, Vec3::new(150.0, 0.0, 0.0)).unwrap();
        assert_eq!(
            t.get(c).unwrap().local_position(),
            Vec3::new(50.0, 0.0, 0.0)
        );
    }

    #[test]
    fn enable_and_disable_processing_propagate_to_children() {
        let mut t = EntityTree::new();
        let p = t.create("p", true);
        let c = t.create("c", true);
        t.link(c, Some(p)).unwrap();
        assert!(t.get(c).unwrap().is_enabled());
        t.enable(p, false);
        assert!(!t.get(p).unwrap().is_enabled() && !t.get(c).unwrap().is_enabled());
        t.disable_processing(p, true);
        assert!(t.get(c).unwrap().is_processing_disabled());
        t.disable_processing(p, false);
        assert!(!t.get(c).unwrap().is_processing_disabled());
    }

    #[test]
    fn kill_flags_the_subtree_and_queues_once() {
        let mut t = EntityTree::new();
        let p = t.create("p", true);
        let c = t.create("c", true);
        t.link(c, Some(p)).unwrap();
        t.take_events();
        t.kill(p);
        t.kill(p);
        assert!(t.get(c).unwrap().is_killed());
        let kills = t
            .take_events()
            .into_iter()
            .filter(|e| matches!(e, TreeEvent::Killed(_)))
            .count();
        assert_eq!(kills, 1);
        assert_eq!(t.destroy(p).len(), 2);
        assert!(t.get(c).is_none());
    }

    #[test]
    fn cache_in_out_toggles_residency_and_leaves_range() {
        let mut t = EntityTree::new();
        let e = boxed(&mut t, "e", Vec3::ZERO);
        t.take_events();
        t.cache_in(e);
        assert!(t.get(e).unwrap().is_cached_in());
        t.set_flags(e, flags::IN_RANGE, true);
        t.cache_out(e, true, false);
        let ev = t.take_events();
        assert!(ev.contains(&TreeEvent::ExitRange(e)));
        let ent = t.get(e).unwrap();
        assert!(!ent.is_cached_in() && !ent.has_entered_processing_range());
        assert!(ent.has(flags::UPDATE_PENDING));
        t.cache_in(e);
        assert!(!t.get(e).unwrap().has(flags::UPDATE_PENDING));
    }

    #[test]
    fn default_flags_are_enabled_and_deactivatable() {
        let mut t = EntityTree::new();
        let s = t.create("s", false);
        let d = t.create("d", true);
        let s = t.get(s).unwrap();
        assert!(s.is_enabled() && s.can_be_deactivated() && !s.is_dynamic());
        assert!(t.get(d).unwrap().has(flags::PROCESS_REQUIRED));
        assert_eq!(t.dynamic_entities().count(), 1);
    }

    #[test]
    fn moving_an_entity_reports_the_new_centre() {
        let mut t = EntityTree::new();
        let e = boxed(&mut t, "e", Vec3::ZERO);
        t.take_events();
        t.set_local_position(e, Vec3::new(10_000.0, 0.0, 0.0))
            .unwrap();
        assert!(t.take_events().iter().any(|ev| matches!(
            ev,
            TreeEvent::Moved { id, new, .. } if *id == e && new.x == 10_000.0
        )));
    }
}
