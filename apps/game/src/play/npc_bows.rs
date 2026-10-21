//! The bows and crossbows of the NPCs. Like the hero's (`ranged::bow_sync`) a bow is an animated
//! actor (`Bow_Hunting_01.xact`, the item template has no mesh) linked to the slot bone of its
//! item type (`gCInventorySlot::GetBone`, Game:0x100bd8d0): `Slot_LeftHand_Weapon` while the bow
//! is in the hands (`_AI_SelectWeapon` readied it), `Slot_Bow` / `Slot_CrossBow` on the back while
//! the fighter carries it. Characters carry a bow in their inventory (the treasure sets of the
//! world data give most fighters a sword and a bow), so the bow is on the back of every one of
//! them and in the hands of the ones that shoot.
use super::NpcActor;
use super::ranged::actor_candidates;
use bevy::prelude::*;
use gothic_animation::runtime::CharacterComposition;
use gothic_animation::{ActorSpawner, Animator, NpcSpec};
use gothic_runtime::CoreRes;
use gothic_sim::item::Use;
use std::collections::{HashMap, HashSet};

const CM: f32 = 0.01;

/// The bow actor of one NPC.
#[derive(Component)]
pub struct NpcBowVisual {
    owner: Entity,
    template: String,
    actor: String,
}

/// The bow or crossbow an NPC has: its template and kind, and whether the hands hold it.
fn bow_of(core: &CoreRes, npc: gothic_sim::types::ActorId) -> Option<(String, Use, bool)> {
    let a = core.0.sim.actor(npc);
    let is_bow = |u: Use| matches!(u, Use::Bow | Use::CrossBow);
    if let Some(l) = a.equipment.left.as_ref().filter(|d| is_bow(d.use_kind())) {
        return Some((l.name.clone(), l.use_kind(), a.drawn));
    }
    if let Some((l, _)) = a.brain.melee_kit.as_ref()
        && let Some(l) = l.as_ref().filter(|d| is_bow(d.use_kind()))
    {
        return Some((l.name.clone(), l.use_kind(), false));
    }
    a.inventory
        .iter()
        .filter_map(|(t, _)| core.0.sim.items.get(t))
        .find(|d| is_bow(d.use_kind()))
        .map(|d| (d.name.clone(), d.use_kind(), false))
}

#[allow(clippy::too_many_arguments)]
pub fn npc_bows(
    core: Res<CoreRes>,
    mut spawner: ActorSpawner,
    npcs: Query<(
        Entity,
        &NpcActor,
        &Animator,
        &CharacterComposition,
        &GlobalTransform,
    )>,
    mut bows: Query<
        (
            Entity,
            &NpcBowVisual,
            &mut Transform,
            &mut Animator,
            &mut Visibility,
        ),
        Without<NpcActor>,
    >,
    mut failed: Local<HashSet<String>>,
) {
    let mut want: HashMap<Entity, (String, Use, bool)> = HashMap::new();
    for (e, npc, ..) in &npcs {
        if let Some(b) = bow_of(&core, npc.0) {
            want.insert(e, b);
        }
    }
    // Bows nobody wants any more (the body went, the bow changed) leave.
    let mut have: HashSet<Entity> = HashSet::new();
    for (bow, v, ..) in &bows {
        match want.get(&v.owner) {
            Some((t, ..)) if *t == v.template => {
                have.insert(v.owner);
            }
            _ => spawner.commands.entity(bow).despawn(),
        }
    }
    // New bows (a few per frame: an actor costs a mesh build).
    let mut made = 0;
    for (owner, (template, ..)) in &want {
        if have.contains(owner) || failed.contains(template) || made >= 2 {
            continue;
        }
        let Some(name) = actor_candidates(template).into_iter().find_map(|n| {
            let l = n.to_lowercase();
            spawner
                .library
                .actor_names(|a| a.to_lowercase().contains(&l))
                .into_iter()
                .min_by_key(String::len)
        }) else {
            failed.insert(template.clone());
            continue;
        };
        match spawner.spawn(
            &NpcSpec::new(name.clone()),
            Transform::from_scale(Vec3::splat(CM)),
        ) {
            Ok(e) => {
                spawner.commands.entity(e).insert(NpcBowVisual {
                    owner: *owner,
                    template: template.clone(),
                    actor: name,
                });
                made += 1;
            }
            Err(_) => {
                failed.insert(template.clone());
            }
        }
    }
    // Place every bow at the slot bone of its owner.
    for (_, v, mut tf, mut anim, mut vis) in &mut bows {
        let Some((_, kind, in_hands)) = want.get(&v.owner) else {
            continue;
        };
        let Ok((_, _, owner_anim, comp, root)) = npcs.get(v.owner) else {
            continue;
        };
        let slot = if *in_hands {
            "slot_lefthand_weapon"
        } else if *kind == Use::CrossBow {
            "slot_crossbow"
        } else {
            "slot_bow"
        };
        let Some(j) = comp
            .0
            .joint_by_name
            .get(slot)
            .and_then(|&j| owner_anim.joints.get(j as usize))
        else {
            vis.set_if_neq(Visibility::Hidden);
            continue;
        };
        *tf = Transform::from_matrix(Mat4::from(root.affine() * *j));
        vis.set_if_neq(Visibility::Inherited);
        let clip = format!("{}_Relax", v.actor);
        if anim.current_name() != Some(clip.as_str()) && spawner.library.has_motion(&clip) {
            let rig = anim.rig.clone();
            if let Ok(m) = spawner.library.motion(&rig, &clip) {
                anim.play(m, 0.0, 1.0, 0.0);
            }
        }
    }
}
