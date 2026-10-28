//! Head look-at of characters (`eCLookAtConstraint`, ENGINE_MAP B10): `Entity::SetLookAtTarget`
//! (called by the ported combat scripts, `ScriptHost::set_look_at`) and the dialog partner give
//! a target; the ported [`LookAtSet`] turns the head node towards it (default limits of
//! `ImportLookAtConstraints`: -360..360, speed 1, no `.xlac` side file ships in the animation
//! archive), the animator applies the rotation after the clips.
use super::{HeroActor, NpcActor, PlayState};
use bevy::prelude::*;
use gothic_animation::Animator;
use gothic_animation::lookat::LookAtSet;
use gothic_runtime::CoreRes;
use gothic_sim::types::ActorId;
use std::collections::HashMap;

/// Height of the point looked at above a character's feet (m) [I].
const HEAD_HEIGHT: f32 = 1.6;

/// A character's look-at constraints and the rig node of the head.
#[derive(Component)]
pub struct LookAt {
    set: LookAtSet,
    head: u16,
    /// The target currently followed (QA).
    pub following: Option<ActorId>,
}

/// The rig node the head constraint acts on: the first joint of the head chain
/// (`<Skeleton>_Head_Head_1`, below the three neck joints).
fn head_node(a: &Animator) -> Option<u16> {
    a.rig
        .names
        .iter()
        .enumerate()
        .find(|(i, n)| {
            n.to_ascii_lowercase().ends_with("head_head_1")
                && a.rig
                    .slot_of
                    .get(*i)
                    .is_some_and(|s| *s != gothic_animation::rig::NONE)
        })
        .map(|(i, _)| i as u16)
}

pub fn register(app: &mut App) {
    app.add_systems(Update, drive_look_at.after(super::npcs::sync_npcs));
}

#[allow(clippy::too_many_arguments)]
fn drive_look_at(
    time: Res<Time>,
    core: Res<CoreRes>,
    state: Res<PlayState>,
    mut commands: Commands,
    mut npcs: Query<(
        Entity,
        &NpcActor,
        &GlobalTransform,
        &mut Animator,
        Option<&mut LookAt>,
    )>,
    hero: Query<&GlobalTransform, (With<HeroActor>, Without<NpcActor>)>,
    others: Query<(&NpcActor, &GlobalTransform), Without<Animator>>,
) {
    let dt = time.delta_secs();
    let sim = &core.0.sim;
    let hero_id = core.0.hero;
    let hero_pos = hero.single().ok().map(GlobalTransform::translation);
    // Where every character stands (the targets are characters).
    let mut at: HashMap<ActorId, Vec3> = HashMap::new();
    for (_, n, g, _, _) in npcs.iter() {
        at.insert(n.0, g.translation());
    }
    for (n, g) in &others {
        at.insert(n.0, g.translation());
    }
    if let Some(p) = hero_pos {
        at.insert(hero_id, p);
    }
    for (entity, npc, root, mut animator, look) in &mut npcs {
        let actor = sim.actor(npc.0);
        let want = if !actor.is_alive() {
            None
        } else if state.dialog_npc == Some(npc.0) {
            Some(hero_id)
        } else {
            actor.look_at
        };
        let target = want.and_then(|t| at.get(&t).copied());
        let Some(mut look) = look else {
            // nothing to follow and no constraint yet: nothing to do
            if target.is_none() {
                continue;
            }
            let Some(head) = head_node(&animator) else {
                if std::env::var_os("GR_LOOK_LOG").is_some() {
                    let c: Vec<_> = animator
                        .rig
                        .names
                        .iter()
                        .filter(|n| {
                            let l = n.to_ascii_lowercase();
                            l.contains("head") || l.contains("neck")
                        })
                        .collect();
                    eprintln!("look-at: no head node, candidates {c:?}");
                }
                continue;
            };
            let mut set = LookAtSet::default();
            if let Some(name) = animator.rig.names.get(head as usize) {
                set.set_constraint(
                    name,
                    Vec3::new(-360.0, -360.0, 0.0),
                    Vec3::new(360.0, 360.0, 0.0),
                    1.0,
                );
            }
            commands.entity(entity).insert(LookAt {
                set,
                head,
                following: want,
            });
            continue;
        };
        look.following = want.filter(|_| target.is_some());
        // The target in the character's own frame (centimetres, the entity frame of the rig).
        look.set.target = target.map(|t| {
            root.affine()
                .inverse()
                .transform_point3(t + Vec3::Y * HEAD_HEIGHT)
        });
        let head = look.head;
        let out = {
            let a = &*animator;
            look.set.step(dt, |name| {
                a.rig.node(name).and_then(|n| a.node_entity_pos(n))
            })
        };
        let rotating = out
            .iter()
            .any(|(_, q)| q.angle_between(Quat::IDENTITY) > 1e-3);
        animator.look = if rotating {
            vec![(head, out.first().map_or(Quat::IDENTITY, |(_, q)| *q))]
        } else {
            Vec::new()
        };
    }
}
