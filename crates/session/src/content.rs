//! The commands of the new-game setup.
use crate::data::{LockDef, Treasure, WorldObjectDef, RecipeDef, seed_of};
use gothic_gameplay::inventory::ItemCatalog;
use gothic_gameplay::treasure::{NoHero, Roll};
use gothic_runtime::Command;
use gothic_sim::script::enums::UseType;
use gothic_sim::script::player::api::SkillCategory;
use gothic_sim::script_player::ObjectData;
use gothic_sim::types::ActorId;
use gothic_sim::world::Pos;
use std::collections::HashMap;

/// A world entity with the position it has in the host's frame (what the core's `Pos` uses).
pub struct Placed<'a> {
    /// Host-chosen tag the core answers with (`Event::ObjectRegistered`).
    pub tag: u32,
    pub def: &'a WorldObjectDef,
    pub pos: Pos,
}

/// The setup commands and what the host needs to remember of them.
#[derive(Default)]
pub struct ContentSet {
    pub commands: Vec<Command>,
    pub objects: u32,
    pub items: u32,
    /// Recipe blueprint item (lowercase) -> recipe id / category.
    pub recipe_ids: HashMap<String, i32>,
    pub recipe_cat: HashMap<String, SkillCategory>,
}

/// Spells, recipes, objects (with their locks and loot) and loose items. `item_known` tells whether an item template exists.
pub fn content_commands<'a>(
    placed: impl IntoIterator<Item = Placed<'a>>,
    treasure: &Treasure,
    recipes: &[RecipeDef],
    spells: &str,
    items: &dyn ItemCatalog,
    item_known: &dyn Fn(&str) -> bool,
) -> ContentSet {
    let mut out = ContentSet::default();
    if !spells.is_empty() {
        out.commands.push(Command::LoadSpells(spells.to_string()));
    }
    for (id, r) in recipes.iter().enumerate() {
        out.recipe_ids.insert(r.item.to_ascii_lowercase(), id as i32);
        out.recipe_cat.insert(r.item.to_ascii_lowercase(), r.category);
        out.commands.push(Command::AddRecipe {
            id: id as i32,
            recipe: r.recipe.clone(),
        });
        out.commands.push(Command::AddRecipeItem {
            template: r.item.clone(),
            category: r.category,
        });
    }
    for p in placed {
        let (d, tag, pos) = (p.def, p.tag, p.pos);
        if d.is_item {
            if d.used || !item_known(&d.name) {
                continue;
            }
            out.commands.push(Command::AddItem {
                tag,
                template: d.name.clone(),
                pos,
            });
            out.items += 1;
            continue;
        }
        let use_type = d.script_use();
        let mut loot = d.stacks.clone();
        if use_type == UseType::Chest {
            let mut roll = Roll(seed_of(&format!("{}{tag}", d.id)));
            loot.extend(treasure.plunder(items, &d.treasure_sets, &mut roll, &NoHero));
            loot.retain(|(item, _)| item_known(item));
        }
        out.commands.push(Command::AddObject {
            tag,
            use_type,
            pos,
            data: ObjectData {
                name: d.display.clone().unwrap_or_else(|| d.name.clone()),
                lock: d.lock.as_ref().map(LockDef::to_lock),
                used: d.used,
                loot,
                use_func: d.use_func.clone(),
                ..Default::default()
            },
        });
        out.objects += 1;
    }
    out
}

/// The routine points of a character (script entity ids of registered objects); `None` when it has none.
pub fn routine_command(
    actor: ActorId,
    work: Option<u32>,
    relax: Option<u32>,
    sleep: Option<u32>,
) -> Option<Command> {
    (work.is_some() || relax.is_some() || sleep.is_some()).then_some(Command::SetRoutinePoints {
        actor,
        work,
        relax,
        sleep,
    })
}

/// One `RegisterSector` per distinct sector name (the first flag wins), in order.
pub fn sector_commands(sectors: impl IntoIterator<Item = (String, bool)>) -> Vec<Command> {
    let mut out: Vec<Command> = Vec::new();
    for (name, enabled) in sectors {
        if !out
            .iter()
            .any(|c| matches!(c, Command::RegisterSector { name: n, .. } if *n == name))
        {
            out.push(Command::RegisterSector { name, enabled });
        }
    }
    out
}

/// The sector a world document belongs to: its directory (`<Region>/<Sector>/<Sector>.lrentdat`).
pub fn sector_name(doc: &str) -> String {
    let norm = doc.replace(char::from(92), "/");
    let mut parts: Vec<&str> = norm.split('/').collect();
    parts.pop();
    parts.last().map(|s| (*s).to_owned()).unwrap_or_default()
}
