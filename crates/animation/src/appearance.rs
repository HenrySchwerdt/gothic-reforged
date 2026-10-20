//! Data-driven NPC appearance: the world entity's `eCVisualAnimation_PS` body parts and its
//! equipped item templates (see "NPC appearance chain" in `ACTOR_ANIMATION_FORMAT.md`) become
//! an [`NpcSpec`]. Nothing here guesses from names: every actor, mesh, slot and material
//! switch is read from the property data, and anything that does not resolve is reported.
use crate::{ActorLibrary, NpcSpec, runtime::AttachSpec};
use gothic_formats::props::records::NpcLook;

/// Actor name (no extension) of a `.fxa` resource reference.
fn actor_stem(file: &str) -> &str {
    let base = file.rsplit(['/', '\\']).next().unwrap_or(file);
    base.rsplit_once('.').map_or(base, |(stem, _)| stem)
}

/// Part name understood by [`ActorLibrary::part`]: actor plus `@M<switch>`.
fn part_name(file: &str, switch: i32) -> String {
    if switch >= 0 {
        format!("{}@M{switch}", actor_stem(file))
    } else {
        actor_stem(file).to_string()
    }
}

/// Builds the character description for `look`, or the reason it cannot be resolved.
pub fn resolve(library: &ActorLibrary, look: &NpcLook) -> Result<NpcSpec, String> {
    let visual = &look.visual;
    let body = visual.parts.iter().find(|p| p.slot == "Slot_Body");
    let (body_file, body_switch) = match (body, &visual.skeleton) {
        (Some(p), _) => (p.file.as_str(), p.switch),
        // Whole-body creatures and skeleton NPCs: the skeleton resource is the mesh.
        (None, Some(s)) if visual.parts.is_empty() => (s.as_str(), visual.skeleton_switch),
        _ => return Err("no body resource".into()),
    };
    if !library.has_actor(actor_stem(body_file)) {
        return Err(format!("body actor {body_file} not installed"));
    }
    let mut spec = NpcSpec::new(part_name(body_file, body_switch));
    for part in &visual.parts {
        if part.slot == "Slot_Body" {
            continue;
        }
        if !library.has_actor(actor_stem(&part.file)) {
            return Err(format!("{} actor {} not installed", part.slot, part.file));
        }
        spec = spec.with_part(part_name(&part.file, part.switch));
    }
    for item in &look.inventory.equipped {
        let Some(mesh) = item.visual.as_ref().and_then(|v| v.mesh.as_deref()) else {
            continue;
        };
        // The entity lists its attachments as (item GUID, bone); items without an entry sit on
        // the node named after their inventory slot (`Hair` -> `Slot_Hair`, the same names the
        // attachment lists use everywhere else).
        let slot = item
            .item_entity
            .as_ref()
            .and_then(|guid| {
                visual
                    .attachments
                    .iter()
                    .find(|(g, _)| g.as_ref() == Some(guid))
                    .map(|(_, bone)| bone.clone())
            })
            .unwrap_or_else(|| format!("Slot_{}", item.slot));
        if !library.has_mesh(mesh) {
            // The installation does not ship this mesh (retired templates); the engine has
            // nothing to show either. Reported by [`missing_meshes`], not fatal.
            continue;
        }
        spec = spec.with_attachment(AttachSpec {
            mesh: mesh.to_string(),
            switch: item.visual.as_ref().map_or(0, |v| v.mesh_switch),
            slot,
        });
    }
    Ok(spec)
}

/// Equipped item meshes that the installation does not ship (skipped when spawning).
pub fn missing_meshes(library: &ActorLibrary, look: &NpcLook) -> Vec<String> {
    look.inventory
        .equipped
        .iter()
        .filter_map(|i| i.visual.as_ref()?.mesh.as_deref().map(|m| (i, m)))
        .filter(|(_, m)| !library.has_mesh(m))
        .map(|(i, m)| format!("{} -> {m}", i.template))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fxa_references_map_to_actor_stems_with_switch() {
        assert_eq!(
            actor_stem("G3_Head_Slave_Myrtana_02.FXA"),
            "G3_Head_Slave_Myrtana_02"
        );
        assert_eq!(
            actor_stem("dir/G3_Orc_Body_Warrior.fxa"),
            "G3_Orc_Body_Warrior"
        );
        assert_eq!(
            part_name("G3_Orc_Body_Warrior.fxa", 4),
            "G3_Orc_Body_Warrior@M4"
        );
        assert_eq!(
            part_name("G3_Orc_Body_Warrior.fxa", -1),
            "G3_Orc_Body_Warrior"
        );
    }
}
