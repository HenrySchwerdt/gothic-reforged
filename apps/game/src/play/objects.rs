//! World objects of the play mode: registers the interact objects, routine points and loose
//! items around Ardea with the core, keeps the entity ids the core answers with, shows loose
//! items in the world and gives the NPCs their routine points.
use super::data::{
    Treasure, WorldObjectDef, load_item_meshes, load_objects, load_recipes, seed_of,
};
use super::{NpcActor, NpcExtra};
use crate::walk::{ORIGIN, Player};
use crate::world_loader::Origin;
use bevy::prelude::*;
use gothic_animation::ActorSpawner;
use gothic_runtime::{Command, CoreCommand, CoreEvent, CoreRes, Event};
use gothic_sim::script::enums::UseType;
use gothic_sim::script_player::ObjectData;
use gothic_sim::types::ActorId;
use gothic_sim::world::Pos;
use std::collections::HashMap;
use std::path::Path;

const CM: f32 = 0.01;

/// One world entity as the presentation knows it.
pub struct ObjEntry {
    pub def: WorldObjectDef,
    /// Mirrored, origin-relative centimetres (the space the world is built in).
    pub pos_cm: Vec3,
    /// Script entity id once the core registered it.
    pub entity: Option<u32>,
    /// A loose item that was picked up.
    pub taken: bool,
    /// The mesh shown for a loose item.
    pub visual: Option<Entity>,
    pub visual_tried: bool,
    /// Index into [`WorldObjects::docs`]: the sector layer the entity is stored in.
    pub doc: u32,
}

/// A world document (sector layer) that holds interact objects or loose items.
pub struct ObjDoc {
    pub path: String,
    /// Layer exists now (`Enabled` of its root, switched by `SetSectorStatus`).
    pub on: bool,
}

#[derive(Resource, Default)]
pub struct WorldObjects {
    pub list: Vec<ObjEntry>,
    pub docs: Vec<ObjDoc>,
    pub by_entity: HashMap<u32, usize>,
    by_guid: HashMap<String, usize>,
    pub treasure: Treasure,
    pub item_mesh: HashMap<String, String>,
    pub recipe_ids: HashMap<String, i32>,
    /// Blueprint item (lowercase) -> its category.
    pub recipe_cat: HashMap<String, gothic_sim::script::player::api::SkillCategory>,
    sent: bool,
    spells: String,
    recipes: Vec<super::data::RecipeDef>,
    /// NPC whose routine points were handed to the core.
    routed: std::collections::HashSet<ActorId>,
    /// Characters that got routine points (for the smoke report).
    pub with_points: u32,
}

impl WorldObjects {
    /// The entity definitions of the world export: the whole world when `objects_world.json`
    /// exists (`gothic-inspector object-dump --radius 9000000`), else the area around Ardea.
    pub fn load_defs() -> Vec<WorldObjectDef> {
        let world = Path::new(".local/objects_world.json");
        load_objects(if world.exists() {
            world
        } else {
            Path::new(".local/objects.json")
        })
    }

    /// Build the world objects: entities stored with their own `Enabled = false` do not exist,
    /// the others belong to the layer of their document (same rule as the characters).
    pub fn build(defs: Vec<WorldObjectDef>, flags: &super::layers::LayerFlags) -> Self {
        let origin = Origin(ORIGIN);
        let mut w = Self {
            treasure: Treasure::load(Path::new(".local/treasure.json")),
            item_mesh: load_item_meshes(Path::new(".local/items.json")),
            spells: std::fs::read_to_string(".local/spells.json").unwrap_or_default(),
            recipes: load_recipes(Path::new(".local/recipes.json")),
            ..Default::default()
        };
        let mut doc_index: HashMap<String, u32> = HashMap::new();
        for def in defs {
            if flags.entity_disabled(&def.document, &def.name) {
                continue;
            }
            let doc = *doc_index.entry(def.document.clone()).or_insert_with(|| {
                w.docs.push(ObjDoc {
                    path: def.document.clone(),
                    on: flags.root_enabled(&def.document),
                });
                (w.docs.len() - 1) as u32
            });
            let pos_cm = origin.point(Vec3::from(def.position));
            w.by_guid.insert(def.id.clone(), w.list.len());
            w.list.push(ObjEntry {
                def,
                pos_cm,
                entity: None,
                taken: false,
                visual: None,
                visual_tried: false,
                doc,
            });
        }
        w
    }

    /// Does the layer of entry `i` exist at this story state?
    pub fn layer_on(&self, i: usize) -> bool {
        self.list
            .get(i)
            .is_some_and(|e| self.docs[e.doc as usize].on)
    }

    pub fn index_of_entity(&self, entity: u32) -> Option<usize> {
        self.by_entity.get(&entity).copied()
    }

    pub fn entry_of_guid(&self, guid: &str) -> Option<&ObjEntry> {
        self.by_guid.get(guid).and_then(|i| self.list.get(*i))
    }

    /// Display name of an object or item.
    pub fn display(&self, i: usize) -> String {
        let d = &self.list[i].def;
        d.display
            .clone()
            .unwrap_or_else(|| d.name.replace('_', " "))
    }
}

pub fn register(app: &mut App) {
    app.add_systems(Startup, send_content).add_systems(
        Update,
        (
            take_registrations,
            object_layers,
            spawn_item_visuals,
            route_npcs,
        ),
    );
}

/// Items of the item database by name (case-insensitive).
fn item_known(core: &CoreRes, name: &str) -> bool {
    core.0.sim.items.get(name).is_some()
}

/// Register spells, recipes, objects and loose items with the core (once).
fn send_content(
    mut objects: ResMut<WorldObjects>,
    core: Res<CoreRes>,
    mut commands: MessageWriter<CoreCommand>,
) {
    if objects.sent {
        return;
    }
    objects.sent = true;
    if !objects.spells.is_empty() {
        commands.write(CoreCommand(Command::LoadSpells(objects.spells.clone())));
    }
    let recipes = std::mem::take(&mut objects.recipes);
    for (id, r) in recipes.iter().enumerate() {
        objects
            .recipe_ids
            .insert(r.item.to_ascii_lowercase(), id as i32);
        objects
            .recipe_cat
            .insert(r.item.to_ascii_lowercase(), r.category);
        commands.write(CoreCommand(Command::AddRecipe {
            id: id as i32,
            recipe: r.recipe.clone(),
        }));
        commands.write(CoreCommand(Command::AddRecipeItem {
            template: r.item.clone(),
            category: r.category,
        }));
    }
    objects.recipes = recipes;
    let treasure = std::mem::take(&mut objects.treasure);
    let mut n_obj = 0;
    let mut n_item = 0;
    for (tag, e) in objects.list.iter().enumerate() {
        let d = &e.def;
        let pos = Pos::new(e.pos_cm.x, e.pos_cm.y, e.pos_cm.z);
        if d.is_item {
            if d.used || !item_known(&core, &d.name) {
                continue;
            }
            commands.write(CoreCommand(Command::AddItem {
                tag: tag as u32,
                template: d.name.clone(),
                pos,
            }));
            n_item += 1;
            continue;
        }
        let use_type = d.script_use();
        let mut loot = d.stacks.clone();
        if use_type == UseType::Chest {
            let mut roll = gothic_gameplay::treasure::Roll(seed_of(&format!("{}{tag}", d.id)));
            loot.extend(treasure.plunder(
                &core.0.sim.items,
                &d.treasure_sets,
                &mut roll,
                &gothic_gameplay::treasure::NoHero,
            ));
            loot.retain(|(item, _)| item_known(&core, item));
        }
        commands.write(CoreCommand(Command::AddObject {
            tag: tag as u32,
            use_type,
            pos,
            data: ObjectData {
                name: d.display.clone().unwrap_or_else(|| d.name.clone()),
                lock: d.lock.as_ref().map(super::data::LockDef::to_lock),
                used: d.used,
                loot,
                ..Default::default()
            },
        }));
        n_obj += 1;
    }
    objects.treasure = treasure;
    eprintln!(
        "play: registered {n_obj} interact objects / points and {n_item} loose items, {} recipes",
        objects.recipes.len()
    );
}

fn take_registrations(mut events: MessageReader<CoreEvent>, mut objects: ResMut<WorldObjects>) {
    for e in events.read() {
        match &e.0 {
            Event::ObjectRegistered { tag, entity } | Event::ItemRegistered { tag, entity } => {
                if let Some(o) = objects.list.get_mut(*tag as usize) {
                    o.entity = Some(*entity);
                }
                objects.by_entity.insert(*entity, *tag as usize);
            }
            _ => {}
        }
    }
}

/// A sector switch changes which object layers exist.
fn object_layers(
    mut events: MessageReader<CoreEvent>,
    mut objects: ResMut<WorldObjects>,
    core: Res<CoreRes>,
) {
    if !events
        .read()
        .any(|e| matches!(e.0, Event::SectorSwitched { .. }))
    {
        return;
    }
    for d in &mut objects.docs {
        d.on = core.0.document_enabled(&d.path);
    }
}

/// Loose items are shown with the mesh of their item template while the hero is within
/// `SHOW_ITEMS` of them (a few at a time) and dropped again beyond `DROP_ITEMS`; the world holds
/// tens of thousands of them, so the list is only scanned a few times per second.
const SHOW_ITEMS: f32 = 3000.0;
const DROP_ITEMS: f32 = 4500.0;

fn spawn_item_visuals(
    mut objects: ResMut<WorldObjects>,
    player: Res<Player>,
    time: Res<Time>,
    mut spawner: ActorSpawner,
    mut last: Local<f32>,
) {
    let now = time.elapsed_secs();
    if now - *last < 0.25 {
        return;
    }
    *last = now;
    let hero = player.ctrl.position;
    let mut budget = 4;
    let WorldObjects {
        list,
        docs,
        item_mesh,
        ..
    } = &mut *objects;
    for e in list.iter_mut() {
        if !e.def.is_item || e.taken || e.entity.is_none() {
            continue;
        }
        if !docs[e.doc as usize].on {
            if let Some(v) = e.visual.take() {
                spawner.commands.entity(v).despawn();
                e.visual_tried = false;
            }
            continue;
        }
        let d = (e.pos_cm.x - hero.x).abs().max((e.pos_cm.z - hero.z).abs());
        if let Some(v) = e.visual {
            if d > DROP_ITEMS {
                spawner.commands.entity(v).despawn();
                e.visual = None;
                e.visual_tried = false;
            }
            continue;
        }
        if budget == 0 || e.visual_tried || d > SHOW_ITEMS {
            continue;
        }
        e.visual_tried = true;
        budget -= 1;
        let Some(mesh) = item_mesh.get(&e.def.name.to_ascii_lowercase()) else {
            continue;
        };
        let tf = Transform {
            translation: e.pos_cm * CM,
            rotation: Quat::from_rotation_y(-e.def.heading),
            scale: Vec3::splat(CM),
        };
        match spawner.spawn_static(mesh, tf) {
            Ok(en) => e.visual = Some(en),
            Err(err) => eprintln!("play: item {} has no mesh {mesh}: {err:#}", e.def.name),
        }
    }
}

/// Once an NPC exists in the core, hand it the working / relaxing / sleeping points of its
/// routine (`gCNavigation_PS` ids resolved against the registered objects).
fn route_npcs(
    mut objects: ResMut<WorldObjects>,
    npcs: Query<(&NpcActor, &NpcExtra)>,
    mut commands: MessageWriter<CoreCommand>,
) {
    if objects.by_entity.is_empty() {
        return;
    }
    for (npc, extra) in &npcs {
        let id = npc.0;
        if objects.routed.contains(&id) {
            continue;
        }
        objects.routed.insert(id);
        let find = |g: &Option<String>| {
            g.as_ref()
                .and_then(|g| objects.entry_of_guid(g))
                .and_then(|e| e.entity)
        };
        let (work, relax, sleep) = (find(&extra.work), find(&extra.relax), find(&extra.sleep));
        if work.is_some() || relax.is_some() || sleep.is_some() {
            objects.with_points += 1;
            commands.write(CoreCommand(Command::SetRoutinePoints {
                actor: id,
                work,
                relax,
                sleep,
            }));
        }
    }
}
