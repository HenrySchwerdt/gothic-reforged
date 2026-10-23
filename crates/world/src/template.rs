//! Template entities: spawning entities from templates at runtime and patching them
//! (`eCTemplateEntity`, `gCEntity::CreateFromTemplate`, `eCEntity::CreateFromTemplate`,
//! `eCDynamicEntity::SetCreator`).
//!
//! A template carries the property sets an entity of that kind has. An entity created from a
//! template remembers its creator, so a changed template can patch all its instances
//! (`PatchInstances`) and deleting a template deletes them (`DeleteInstances`). A template can
//! reference a base template (`RefTemplate`): the base is applied first, so a derived template
//! only has to state what it changes. Spawn commands in scripts and the respawn of killed
//! characters go through [`TemplateLibrary::spawn`].
use crate::entity::{Aabb, EntityTree};
use crate::proxy::{PropertyBook, PropertySet};
use crate::pvs::EntityId;
use crate::scene::Scene;
use bevy::math::Vec3;
use std::collections::BTreeMap;

/// A property set of a template.
#[derive(Debug, Clone, PartialEq)]
pub struct TemplateSet {
    pub set: PropertySet,
    /// The instance's set is replaced as a whole instead of being patched property by property
    /// (`PatchEntity` asks the template set, `+0x74`).
    pub replace: bool,
}

impl TemplateSet {
    pub fn new(set: PropertySet) -> Self {
        Self {
            set,
            replace: false,
        }
    }
}

/// An entity template (`eCTemplateEntity`).
#[derive(Debug, Clone, PartialEq)]
pub struct EntityTemplate {
    pub name: String,
    /// Base template (`+0x1c4`).
    pub ref_template: Option<String>,
    pub sets: Vec<TemplateSet>,
    /// Instances are dynamic entities (characters, items) unless stated otherwise.
    pub dynamic: bool,
    /// Half extent of the instance's node box.
    pub half_extent: Vec3,
}

impl EntityTemplate {
    pub fn new(name: &str) -> Self {
        Self {
            name: name.to_owned(),
            ref_template: None,
            sets: Vec::new(),
            dynamic: true,
            half_extent: Vec3::new(40.0, 90.0, 40.0),
        }
    }
}

/// Result of [`TemplateLibrary::patch_entity`] (`bEResult`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PatchResult {
    /// The entity now matches the template.
    Patched,
    /// Nothing done: the entity was killed, or the reference chain is broken or circular.
    Skipped,
}

/// Why a spawn failed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SpawnError {
    /// The path has no template name (`CreateFromTemplate` returns false).
    EmptyName,
    UnknownTemplate(String),
}

impl std::fmt::Display for SpawnError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::EmptyName => write!(f, "template path without a name"),
            Self::UnknownTemplate(n) => write!(f, "unknown template {n}"),
        }
    }
}
impl std::error::Error for SpawnError {}

/// Port of `gCDatabase::SplitTemplatePath` (Game:0x1018e1d0): `Layer:Name`, slashes trimmed.
/// The layer is `None` when the path names only the template.
pub fn split_template_path(path: &str) -> (Option<String>, String) {
    gothic_portcov::hit!("split_template_path");
    let p = path.trim_matches('/');
    match p.rsplit_once(':') {
        Some((layer, name)) => (
            Some(layer.trim_matches('/').to_owned()).filter(|l| !l.is_empty()),
            name.trim_matches('/').to_owned(),
        ),
        None => (None, p.to_owned()),
    }
}

/// Maximum length of a `RefTemplate` chain (guards against authored cycles).
const MAX_REF_DEPTH: usize = 16;

/// Templates, their instances and the property sets of all entities.
#[derive(Debug, Default)]
pub struct TemplateLibrary {
    templates: BTreeMap<String, EntityTemplate>,
    /// Creator -> instances (`AddInstanceEntity`).
    instances: BTreeMap<String, Vec<EntityId>>,
    /// Property sets of every entity this library created or patched.
    pub book: PropertyBook,
}

fn key(name: &str) -> String {
    name.to_ascii_lowercase()
}

impl TemplateLibrary {
    pub fn new() -> Self {
        Self::default()
    }

    /// Add or replace a template. Existing instances are not touched until
    /// [`Self::patch_instances`].
    pub fn add(&mut self, t: EntityTemplate) {
        self.templates.insert(key(&t.name), t);
    }

    pub fn get(&self, name: &str) -> Option<&EntityTemplate> {
        self.templates.get(&key(name))
    }

    pub fn len(&self) -> usize {
        self.templates.len()
    }
    pub fn is_empty(&self) -> bool {
        self.templates.is_empty()
    }

    /// Port of `eCTemplateEntity::GetRefTemplate` (Engine:0x1022c7a0).
    pub fn ref_template(&self, name: &str) -> Option<&EntityTemplate> {
        gothic_portcov::hit!("ref_template");
        self.get(name)
            .and_then(|t| t.ref_template.as_deref())
            .and_then(|r| self.get(r))
    }

    /// The template and its bases, base first. `None` when a base is missing or the chain
    /// loops.
    fn chain(&self, name: &str) -> Option<Vec<&EntityTemplate>> {
        let mut out = vec![self.get(name)?];
        while let Some(r) = out.last().and_then(|t| t.ref_template.as_deref()) {
            let next = self.get(r)?;
            if out.len() >= MAX_REF_DEPTH || out.iter().any(|t| std::ptr::eq(*t, next)) {
                return None;
            }
            out.push(next);
        }
        out.reverse();
        Some(out)
    }

    /// Instances created from a template that still exist.
    pub fn instances(&self, template: &str) -> &[EntityId] {
        self.instances
            .get(&key(template))
            .map_or(&[], Vec::as_slice)
    }

    /// Port of `eCTemplateEntity::PatchEntity` (Engine:0x1022bf30). Applies the template chain
    /// (base first) to the property sets of `entity`:
    ///
    /// * a template set that is flagged `replace`, or any set when `replace_all` is given,
    ///   replaces the entity's set;
    /// * otherwise an existing set is patched (the template's properties override) and a missing
    ///   one is created;
    /// * sets the whole chain does not have are removed from the entity.
    ///
    /// Killed entities are skipped.
    pub fn patch_entity(
        &mut self,
        tree: &EntityTree,
        template: &str,
        entity: EntityId,
        replace_all: bool,
    ) -> PatchResult {
        gothic_portcov::hit!("patch_entity");
        if tree.get(entity).is_none_or(|e| e.is_killed()) {
            return PatchResult::Skipped;
        }
        let Some(chain) = self.chain(template) else {
            return PatchResult::Skipped;
        };
        let chain: Vec<EntityTemplate> = chain.into_iter().cloned().collect();
        let mut wanted: Vec<String> = Vec::new();
        for t in &chain {
            for ts in &t.sets {
                if !wanted.iter().any(|w| w.eq_ignore_ascii_case(&ts.set.class)) {
                    wanted.push(ts.set.class.clone());
                }
                if ts.replace || replace_all {
                    self.book.add(entity, ts.set.clone());
                } else if let Some(existing) = self.book.set_mut(entity, &ts.set.class) {
                    for (k, v) in &ts.set.props {
                        existing.props.insert(k.clone(), v.clone());
                    }
                } else {
                    self.book.add(entity, ts.set.clone());
                }
            }
        }
        // Sets the template does not know leave the entity (walked backwards in the original).
        let stale: Vec<String> = self
            .book
            .sets(entity)
            .iter()
            .filter(|s| !wanted.iter().any(|w| w.eq_ignore_ascii_case(&s.class)))
            .map(|s| s.class.clone())
            .collect();
        for c in stale {
            self.book.remove(entity, &c);
        }
        PatchResult::Patched
    }

    /// Port of `gCEntity::CreateFromTemplate` (Game:0x100664e0) and
    /// `eCEntity::CreateFromTemplate`: spawn an entity from the template named by `path`
    /// (`Layer:Name` or `Name`), place it, cache it in and patch its property sets. The
    /// instance is registered under `instance_name`. Returns the entity.
    pub fn spawn(
        &mut self,
        scene: &mut Scene,
        path: &str,
        instance_name: &str,
        position: Vec3,
    ) -> Result<EntityId, SpawnError> {
        gothic_portcov::hit!("spawn");
        let (_, name) = split_template_path(path);
        if name.is_empty() {
            return Err(SpawnError::EmptyName);
        }
        let Some(t) = self.get(&name) else {
            return Err(SpawnError::UnknownTemplate(name));
        };
        let (dynamic, half, canonical) = (t.dynamic, t.half_extent, t.name.clone());
        let id = if dynamic {
            scene.spawn_dynamic(instance_name, position, half)
        } else {
            let id = scene.tree.create(instance_name, false);
            let _ = scene
                .tree
                .set_local_node_box(id, Some(Aabb::around(Vec3::ZERO, half)));
            let _ = scene.tree.set_local_position(id, position);
            scene.tree.cache_in(id);
            scene.registry.register(id, instance_name);
            id
        };
        scene.tree.set_template(id, Some(&canonical));
        self.instances.entry(key(&canonical)).or_default().push(id);
        self.patch_entity(&scene.tree, &canonical, id, false);
        Ok(id)
    }

    /// Port of `eCTemplateEntity::PatchInstances` (Engine:0x1022c620): re-patch every live
    /// instance after the template changed. Returns how many were patched.
    pub fn patch_instances(&mut self, tree: &EntityTree, template: &str) -> usize {
        gothic_portcov::hit!("patch_instances");
        let ids: Vec<EntityId> = self.instances(template).to_vec();
        ids.into_iter()
            .filter(|id| self.patch_entity(tree, template, *id, false) == PatchResult::Patched)
            .count()
    }

    /// An instance left the world (`RemoveInstanceEntity`): forget it and its property sets.
    pub fn forget_instance(&mut self, entity: EntityId) {
        for v in self.instances.values_mut() {
            v.retain(|e| *e != entity);
        }
        self.book.clear(entity);
    }

    /// Port of `eCTemplateEntity::DeleteInstances` (Engine:0x1022bea0): kill all instances of
    /// the template, newest first, and drop the template. Returns the killed entities.
    pub fn delete_template(&mut self, scene: &mut Scene, template: &str) -> Vec<EntityId> {
        gothic_portcov::hit!("delete_template");
        let mut killed = Vec::new();
        for id in self
            .instances
            .remove(&key(template))
            .unwrap_or_default()
            .into_iter()
            .rev()
        {
            scene.kill(id);
            self.book.clear(id);
            killed.push(id);
        }
        self.templates.remove(&key(template));
        killed
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::proxy::PropValue;

    fn npc_template() -> EntityTemplate {
        let mut t = EntityTemplate::new("Wolf");
        t.sets.push(TemplateSet::new(
            PropertySet::new("gCNPC_PS")
                .with("Level", PropValue::Int(3))
                .with("Hostile", PropValue::Bool(true)),
        ));
        t.sets
            .push(TemplateSet::new(PropertySet::new("eCVisualAnimation_PS")));
        t
    }

    #[test]
    fn template_spawn_creates_a_registered_cached_instance_with_its_sets() {
        let mut scene = Scene::new(10_000.0);
        let mut lib = TemplateLibrary::new();
        lib.add(npc_template());
        let id = lib
            .spawn(
                &mut scene,
                "Templates:wolf",
                "Wolf_01",
                Vec3::new(500.0, 0.0, -200.0),
            )
            .unwrap();
        let e = scene.tree.get(id).unwrap();
        assert!(e.is_dynamic() && e.is_cached_in());
        assert_eq!(e.template.as_deref(), Some("Wolf"));
        assert_eq!(e.world_position(), Vec3::new(500.0, 0.0, -200.0));
        assert_eq!(scene.registry.entity_by_name("Wolf_01"), Some(id));
        assert_eq!(
            lib.book.set(id, "gCNPC_PS").unwrap().props["Level"],
            PropValue::Int(3)
        );
        assert_eq!(lib.instances("WOLF"), &[id]);
        assert_eq!(
            lib.spawn(&mut scene, "Layer:", "x", Vec3::ZERO),
            Err(SpawnError::EmptyName)
        );
        assert_eq!(
            lib.spawn(&mut scene, "Nope", "x", Vec3::ZERO),
            Err(SpawnError::UnknownTemplate("Nope".into()))
        );
    }

    #[test]
    fn derived_templates_override_the_base_and_patching_removes_unknown_sets() {
        let mut scene = Scene::new(10_000.0);
        let mut lib = TemplateLibrary::new();
        lib.add(npc_template());
        let mut alpha = EntityTemplate::new("AlphaWolf");
        alpha.ref_template = Some("Wolf".into());
        alpha.sets.push(TemplateSet::new(
            PropertySet::new("gCNPC_PS").with("Level", PropValue::Int(9)),
        ));
        lib.add(alpha);
        let id = lib
            .spawn(&mut scene, "AlphaWolf", "Alpha", Vec3::ZERO)
            .unwrap();
        let ps = lib.book.set(id, "gCNPC_PS").unwrap();
        assert_eq!(ps.props["Level"], PropValue::Int(9));
        assert_eq!(ps.props["Hostile"], PropValue::Bool(true));
        // A set the chain does not carry is removed by a patch; local edits survive patching.
        lib.book.add(id, PropertySet::new("Leftover"));
        lib.book
            .set_mut(id, "gCNPC_PS")
            .unwrap()
            .props
            .insert("Hostile".into(), PropValue::Bool(false));
        lib.patch_entity(&scene.tree, "AlphaWolf", id, false);
        assert!(lib.book.set(id, "Leftover").is_none());
        // Hostile was overridden by the base template again (the template wins on patch).
        assert_eq!(
            lib.book.set(id, "gCNPC_PS").unwrap().props["Hostile"],
            PropValue::Bool(true)
        );
    }

    #[test]
    fn broken_and_circular_chains_and_killed_entities_are_skipped() {
        let mut scene = Scene::new(10_000.0);
        let mut lib = TemplateLibrary::new();
        let mut a = EntityTemplate::new("A");
        a.ref_template = Some("B".into());
        let mut b = EntityTemplate::new("B");
        b.ref_template = Some("A".into());
        lib.add(a);
        lib.add(b);
        let id = scene.spawn_dynamic("x", Vec3::ZERO, Vec3::ONE);
        assert_eq!(
            lib.patch_entity(&scene.tree, "A", id, false),
            PatchResult::Skipped
        );
        lib.add(npc_template());
        scene.kill(id);
        assert_eq!(
            lib.patch_entity(&scene.tree, "Wolf", id, false),
            PatchResult::Skipped
        );
    }

    #[test]
    fn changed_templates_patch_instances_and_deletion_kills_them() {
        let mut scene = Scene::new(10_000.0);
        let mut lib = TemplateLibrary::new();
        lib.add(npc_template());
        let a = lib.spawn(&mut scene, "Wolf", "W1", Vec3::ZERO).unwrap();
        let b = lib
            .spawn(&mut scene, "Wolf", "W2", Vec3::X * 100.0)
            .unwrap();
        let mut t = npc_template();
        t.sets[0]
            .set
            .props
            .insert("Level".into(), PropValue::Int(7));
        lib.add(t);
        assert_eq!(lib.patch_instances(&scene.tree, "Wolf"), 2);
        assert_eq!(
            lib.book.set(b, "gCNPC_PS").unwrap().props["Level"],
            PropValue::Int(7)
        );
        let killed = lib.delete_template(&mut scene, "Wolf");
        assert_eq!(killed, vec![b, a]);
        assert!(scene.tree.get(a).unwrap().is_killed());
        assert!(lib.get("Wolf").is_none());
        assert_eq!(
            split_template_path("/L:/Name/"),
            (Some("L".into()), "Name".into())
        );
    }
}
