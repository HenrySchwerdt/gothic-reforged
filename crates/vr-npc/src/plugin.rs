//! Bevy/avian integration of the active ragdoll and the combat AI.
//!
//! Each fixed step, before the melee contacts run:
//! 1. **perceive & think**: the AI sees the player (feet from the VR rig, else the avatar's
//!    hitzones) and the player's swung weapons as threats;
//! 2. **animate**: the target pose comes from the AI intent via full-body IK, the ragdoll
//!    steps toward it under gravity;
//! 3. **interact**: physics hands push ragdoll joints, and a closed empty hand next to a
//!    joint holds it;
//! 4. **sync**: kinematic bone colliders (so hands and the world collide with the body), the
//!    [`HitzoneSet`] for the melee model and the NPC's kinematic weapon (grip on the
//!    ragdoll's right hand, orientation from the AI).
//!
//! After the melee step, hits become ragdoll impulses and blocked/parried attacks make the AI
//! recoil.
use crate::ai::{AttackSlots, CombatAi, NpcIntent, Perception, Phase, Threat, local_weapon_pose};
use crate::animator::NpcAnimator;
use crate::ragdoll::{ActiveRagdoll, GrabHold, Stance};
use avian3d::prelude::*;
use bevy::prelude::*;
use gothic_vr::body::{BONES, Joint};
use gothic_vr::{Pose, VrButton, VrInputRes, VrPlayer, VrWorld, transform_to_pose};
use gothic_vr_melee::profiles::profile_for;
use gothic_vr_melee::{
    ClashKind, ClashMessage, HitMessage, HitzoneSet, MeleeSet, MeleeWeapon, PlayerAvatar,
    hitzones_from_body,
};
use gothic_vr_physics::props::PropSpec;
use gothic_vr_physics::{PhysicsHand, spawn_prop};

/// A VR opponent.
#[derive(Component, Debug, Clone)]
pub struct VrNpc {
    pub ai: CombatAi,
    pub ragdoll: ActiveRagdoll,
    pub animator: NpcAnimator,
    pub weapon: Option<Entity>,
    pub intent: Option<NpcIntent>,
}

/// A kinematic collider following one ragdoll bone.
#[derive(Component, Debug, Clone, Copy)]
pub struct NpcBone {
    pub npc: Entity,
    pub bone: (Joint, Joint),
}

/// Global attack tokens.
#[derive(Resource, Debug, Default)]
pub struct NpcSlots(pub AttackSlots);

/// Impulse (N*s) per (m/s of impact * kg of weapon) given to the ragdoll by a hit.
const HIT_IMPULSE: f32 = 0.9;
/// Fastest an NPC moves its weapon grip (m/s) and turns it (rad/s).
const MAX_WEAPON_SPEED: f32 = 12.0;
const MAX_WEAPON_SPIN: f32 = 25.0;
/// Radius of the NPC's body against level geometry, m.
const NPC_RADIUS: f32 = 0.3;
/// How close (m) a closing hand must be to a joint to hold it.
const GRAB_REACH: f32 = 0.12;

/// Ragdoll NPCs with combat AI. Needs the VR, physics and melee plugins.
pub struct VrNpcPlugin;

impl Plugin for VrNpcPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<NpcSlots>().add_systems(
            FixedUpdate,
            (
                (think, animate, interact, sync).chain().before(MeleeSet),
                react.after(MeleeSet),
            ),
        );
    }
}

/// Spawn an NPC standing at `feet` facing `yaw`, holding `weapon` (if any).
pub fn spawn_npc(
    commands: &mut Commands,
    feet: Vec3,
    yaw: f32,
    weapon: Option<&PropSpec>,
    seed: u64,
) -> Entity {
    let pose = NpcAnimator::standing(feet, yaw);
    let ragdoll = ActiveRagdoll::new(&pose);
    let npc = commands
        .spawn((
            Transform::from_translation(feet),
            HitzoneSet::new(hitzones_from_body(&pose)),
            Name::new("VrNpc"),
        ))
        .id();
    let weapon = weapon.map(|spec| {
        let frame = Pose::new(feet, Quat::from_rotation_y(yaw));
        let ready = local_weapon_pose(Phase::Ready, &crate::ai::AiSettings::default());
        let grip = Pose::new(pose.get(Joint::HandR), frame.mul(&ready).rotation);
        let w = spawn_prop(commands, spec, grip);
        commands.entity(w).insert(RigidBody::Kinematic);
        if let Some(profile) = profile_for(spec.kind) {
            let mut mw = MeleeWeapon::new(profile);
            mw.wielder = Some(npc);
            commands.entity(w).insert(mw);
        }
        w
    });
    for bone in BONES {
        let a = pose.get(bone.0);
        let b = pose.get(bone.1);
        let len = a.distance(b).max(0.02);
        let radius = match bone.0 {
            Joint::Pelvis | Joint::Spine | Joint::Chest => 0.12,
            Joint::Neck => 0.09,
            _ => 0.05,
        };
        commands.spawn((
            NpcBone { npc, bone },
            RigidBody::Kinematic,
            Collider::capsule(radius, (len - radius).max(0.01)),
            gothic_vr_melee::body_layers(),
            bone_transform(a, b),
            Name::new(format!("NpcBone{:?}", bone.1)),
        ));
    }
    commands.entity(npc).insert(VrNpc {
        ai: CombatAi::new(feet, yaw, seed),
        ragdoll,
        animator: NpcAnimator::default(),
        weapon,
        intent: None,
    });
    npc
}

fn bone_transform(a: Vec3, b: Vec3) -> Transform {
    let d = b - a;
    let rot = if d.length_squared() > 1e-8 {
        Quat::from_rotation_arc(Vec3::Y, d.normalize())
    } else {
        Quat::IDENTITY
    };
    Transform::from_translation((a + b) / 2.0).with_rotation(rot)
}

/// Is a standing NPC capsule at `feet` (metres) inside level geometry?
fn blocked(world: &gothic_vr::gothic_physics::CollisionWorld, feet: Vec3) -> bool {
    let cm = gothic_vr::CM_PER_M;
    world.capsule_blocked(
        (feet + Vec3::Y * 0.6) * cm,
        (feet + Vec3::Y * 1.5) * cm,
        NPC_RADIUS * cm,
    )
}

/// Ground height below `(x, z)`. The ragdoll asks once per joint and substep, and every question is a raycast
/// against the level's triangle soup, so answers are memoised on a 20 cm grid (cell centre) for the lifetime of the
/// closure (one NPC, one step). On Quest this cost ~100 ms per frame for three orcs without the cache.
fn ground_fn(world: Option<&VrWorld>, near_y: f32) -> impl Fn(f32, f32) -> f32 + '_ {
    const CELL: f32 = 0.2;
    let cache = std::cell::RefCell::new(Vec::<((i32, i32), f32)>::with_capacity(32));
    move |x, z| {
        let key = ((x / CELL).floor() as i32, (z / CELL).floor() as i32);
        if let Some(&(_, y)) = cache.borrow().iter().find(|(k, _)| *k == key) {
            return y;
        }
        let (cx, cz) = ((key.0 as f32 + 0.5) * CELL, (key.1 as f32 + 0.5) * CELL);
        let y = world
            .and_then(|w| {
                w.0.ground_below(Vec3::new(cx, near_y + 1.0, cz) * gothic_vr::CM_PER_M, 300.0)
                    .map(|y| y / gothic_vr::CM_PER_M)
            })
            .unwrap_or(0.0);
        cache.borrow_mut().push((key, y));
        y
    }
}

#[allow(clippy::type_complexity, clippy::too_many_arguments)]
fn think(
    time: Res<Time>,
    mut slots: ResMut<NpcSlots>,
    player: Option<Res<VrPlayer>>,
    avatar: Query<&HitzoneSet, With<PlayerAvatar>>,
    weapons: Query<(&Transform, &MeleeWeapon, Option<&LinearVelocity>)>,
    avatars: Query<Entity, With<PlayerAvatar>>,
    world: Option<Res<VrWorld>>,
    mut npcs: Query<(Entity, &mut VrNpc)>,
) {
    let dt = time.delta_secs();
    let player_feet = player
        .as_ref()
        .map(|p| p.ctrl.position / gothic_vr::CM_PER_M)
        .or_else(|| {
            avatar.iter().next().and_then(|s| {
                let n = s.zones.len().max(1) as f32;
                (!s.zones.is_empty()).then(|| {
                    let c = s.zones.iter().map(|z| z.pose.position).sum::<Vec3>() / n;
                    Vec3::new(c.x, 0.0, c.z)
                })
            })
        })
        .unwrap_or(Vec3::ZERO);
    let avatar = avatars.iter().next();
    for (e, mut npc) in &mut npcs {
        // The most dangerous player weapon point near this NPC.
        let chest = npc.ragdoll.position(Joint::Chest);
        let threat = weapons
            .iter()
            .filter(|(_, w, _)| w.wielder.is_some() && w.wielder == avatar)
            .map(|(t, w, v)| {
                let (a, b, _) = w.profile.guard;
                let p = t.transform_point((a + b) / 2.0 * 0.4 + b * 0.6);
                let v = v.map_or(Vec3::ZERO, |v| v.0);
                Threat {
                    point: p,
                    velocity: v,
                }
            })
            .min_by(|a, b| a.point.distance(chest).total_cmp(&b.point.distance(chest)));
        let can_act = npc.ragdoll.stance() == Stance::Standing;
        let per = Perception {
            player: player_feet,
            threat,
            can_act,
        };
        let before = npc.ai.root;
        let mut intent = npc.ai.update(e.to_bits(), &per, &mut slots.0, dt);
        // Footwork respects the level: a step into a wall or furniture is not taken.
        if let Some(w) = world.as_deref()
            && blocked(&w.0, intent.root)
            && !blocked(&w.0, before)
        {
            intent.weapon.position += before - intent.root;
            npc.ai.root = before;
            intent.root = before;
        }
        npc.intent = Some(intent);
    }
}

fn animate(time: Res<Time>, world: Option<Res<VrWorld>>, mut npcs: Query<&mut VrNpc>) {
    let dt = time.delta_secs();
    for mut npc in &mut npcs {
        let Some(intent) = npc.intent else {
            continue;
        };
        let ground = ground_fn(world.as_deref(), intent.root.y);
        let npc = &mut *npc;
        let target = npc.animator.target(&intent, &ground, dt);
        npc.ragdoll.step(&target, &ground, dt);
        // The AI's root follows the body when it was knocked around.
        if npc.ragdoll.stance() != Stance::Standing {
            let p = npc.ragdoll.position(Joint::Pelvis);
            npc.ai.root = Vec3::new(p.x, npc.ai.root.y, p.z);
        }
    }
}

#[allow(clippy::type_complexity)]
fn interact(
    time: Res<Time>,
    input: Res<VrInputRes>,
    hands: Query<(&Transform, &PhysicsHand, Option<&LinearVelocity>)>,
    mut npcs: Query<&mut VrNpc>,
) {
    let dt = time.delta_secs();
    for mut npc in &mut npcs {
        npc.ragdoll.holds.clear();
        for (t, hand, v) in &hands {
            let v = v.map_or(Vec3::ZERO, |v| v.0);
            npc.ragdoll
                .push(t.translation, gothic_vr_physics::hands::HAND_RADIUS, v, dt);
            if hand.held.is_none() && input.0.held(hand.hand, VrButton::Grip) {
                let nearest = Joint::ALL
                    .into_iter()
                    .map(|j| (j, npc.ragdoll.position(j).distance(t.translation)))
                    .min_by(|a, b| a.1.total_cmp(&b.1));
                if let Some((joint, d)) = nearest
                    && d < GRAB_REACH
                {
                    npc.ragdoll.holds.push(GrabHold {
                        joint,
                        target: t.translation,
                        stiffness: 0.4,
                    });
                }
            }
        }
    }
}

#[allow(clippy::type_complexity)]
fn sync(
    time: Res<Time>,
    mut npcs: Query<(&VrNpc, &mut HitzoneSet, &mut Transform), Without<NpcBone>>,
    mut bones: Query<(&NpcBone, &mut Position, &mut Rotation), Without<VrNpc>>,
    mut weapons: Query<
        (&Transform, &mut LinearVelocity, &mut AngularVelocity),
        (With<MeleeWeapon>, Without<VrNpc>, Without<NpcBone>),
    >,
) {
    let dt = time.delta_secs().max(1e-4);
    for (bone, mut pos, mut rot) in &mut bones {
        let Ok((npc, _, _)) = npcs.get(bone.npc) else {
            continue;
        };
        let t = bone_transform(
            npc.ragdoll.position(bone.bone.0),
            npc.ragdoll.position(bone.bone.1),
        );
        pos.0 = t.translation;
        rot.0 = t.rotation;
    }
    for (npc, mut zones, mut tf) in &mut npcs {
        let pose = npc.ragdoll.pose();
        zones.zones = hitzones_from_body(&pose);
        tf.translation = Vec3::new(
            npc.ai.root.x,
            pose.get(Joint::AnkleL).y.min(pose.get(Joint::AnkleR).y) - 0.08,
            npc.ai.root.z,
        );
        tf.rotation = Quat::from_rotation_y(npc.ai.yaw);
        let (Some(w), Some(intent)) = (npc.weapon, npc.intent) else {
            continue;
        };
        let Ok((wt, mut lv, mut av)) = weapons.get_mut(w) else {
            continue;
        };
        let want = Pose::new(npc.ragdoll.position(Joint::HandR), intent.weapon.rotation);
        let cur = transform_to_pose(wt);
        // Kinematic drive, limited to what an arm can do.
        lv.0 = ((want.position - cur.position) / dt).clamp_length_max(MAX_WEAPON_SPEED);
        av.0 = gothic_vr_physics::pd::angular_velocity_between(cur.rotation, want.rotation, dt)
            .clamp_length_max(MAX_WEAPON_SPIN);
    }
}

#[allow(clippy::type_complexity)]
fn react(
    time: Res<Time>,
    mut hits: MessageReader<HitMessage>,
    mut clashes: MessageReader<ClashMessage>,
    masses: Query<&ComputedMass>,
    mut npcs: Query<(Entity, &mut VrNpc)>,
    mut weapons: Query<&mut MeleeWeapon>,
) {
    let dt = time.delta_secs();
    // Keep the NPCs' weapons attributed to them.
    for (e, npc) in &npcs {
        if let Some(w) = npc.weapon
            && let Ok(mut mw) = weapons.get_mut(w)
        {
            mw.wielder = Some(e);
        }
    }
    for h in hits.read() {
        if let Ok((_, mut npc)) = npcs.get_mut(h.victim) {
            let m = masses.get(h.weapon).map_or(1.5, |m| m.value());
            npc.ragdoll
                .apply_impulse(h.point, h.direction * h.impact_speed * m * HIT_IMPULSE, dt);
        }
    }
    for c in clashes.read() {
        if let Some(a) = c.attacker
            && let Ok((_, mut npc)) = npcs.get_mut(a)
            && matches!(
                npc.ai.phase,
                Phase::Telegraph(..) | Phase::Strike(..) | Phase::Recover(..)
            )
        {
            npc.ai.recoil(c.recoil);
            if c.kind == ClashKind::Parry {
                let hand = npc.ragdoll.position(Joint::HandR);
                let back = -(npc.ai.frame().forward()) * 14.0;
                npc.ragdoll.apply_impulse(hand, back, dt);
            }
        }
    }
}
