//! Bevy/avian integration of the melee contact model.
//!
//! * [`MeleeWeapon`] on a physics prop makes it a weapon; its wielder is set from grabs.
//! * [`HitzoneSet`] on any entity makes it hittable; its owner keeps `zones` up to date (the
//!   ragdoll for NPCs, [`PlayerHitzonesPlugin`] for the player's IK body, nothing for a static
//!   dummy).
//! * Every fixed step the weapons' motion since the last step runs through
//!   [`MeleeWorld::step`]; results go out as [`HitMessage`], [`ClashMessage`] and
//!   [`StabMessage`], and the side effects are applied.
//!
//! Physics is left to the rigid bodies wherever it can do the job (the scheme of physics
//! melee games like Blade & Sorcery and Boneworks, reimplemented from their public
//! descriptions): blades collide with bodies and with each other, so a swing stops at the body
//! it hits and a blade on a blade deflects by contact. The melee events add what rigid contact
//! cannot:
//!
//! * a qualifying stab (point leading, along the blade within ~25 degrees, fast enough) lets
//!   that blade through bodies ([`prop_layers`]) and creates a [`PrismaticJoint`] along the
//!   wound, limited to the blade's penetration depth; the wound drags on the blade
//!   ([`WOUND_DRAG`]); pulling out removes the joint, and the blade collides with bodies again
//!   once it is clear ([`PASS_AFTER_PULL`]);
//! * a blocked or parried swing staggers the attacker's arm for the recoil time
//!   ([`gothic_vr_physics::PhysicsHand::stagger`]) and gets a small extra kick;
//! * haptics on the hands holding the weapons: hit, clash, stuck, pulled out.
use crate::contact::{
    BodyMotion, Bone, ClashKind, DamageType, Hitzone, MeleeEvent, MeleeProfile, MeleeWorld,
    WeaponMotion,
};
use crate::profiles::profile_for;
use avian3d::prelude::*;
use bevy::prelude::*;
use gothic_vr::body::Joint;
use gothic_vr::{HapticMessage, HapticPulse, Pose, VrBody, transform_to_pose};
use gothic_vr_physics::{GrabMessage, HandSet, PhysicsHand, Prop, VrLayer, prop_layers};
use std::collections::HashMap;

/// A physics prop that deals melee damage.
#[derive(Component, Debug, Clone)]
pub struct MeleeWeapon {
    pub profile: MeleeProfile,
    /// Who swings it (set while held).
    pub wielder: Option<Entity>,
    last: Option<Pose>,
    before: Option<Pose>,
}

impl MeleeWeapon {
    pub fn new(profile: MeleeProfile) -> Self {
        Self {
            profile,
            wielder: None,
            last: None,
            before: None,
        }
    }
}

/// Hitzones of a body (world space), with the previous step's copy for sweeping.
#[derive(Component, Debug, Clone, Default)]
pub struct HitzoneSet {
    pub zones: Vec<Hitzone>,
    /// Rigid body carrying each zone (for stab joints); `None` = the set's own entity.
    pub bodies: Vec<Option<Entity>>,
    prev: Vec<Hitzone>,
}

impl HitzoneSet {
    pub fn new(zones: Vec<Hitzone>) -> Self {
        Self {
            bodies: vec![None; zones.len()],
            prev: zones.clone(),
            zones,
        }
    }
}

/// The player's avatar (owner of the hitzones from the IK body, wielder of grabbed weapons).
#[derive(Component, Debug, Default, Clone, Copy)]
pub struct PlayerAvatar;

/// Contact state.
#[derive(Resource, Debug, Default)]
pub struct Melee(pub MeleeWorld);

/// A damaging hit (inputs of the damage formula).
#[derive(Message, Debug, Clone, Copy, PartialEq)]
pub struct HitMessage {
    pub attacker: Option<Entity>,
    pub weapon: Entity,
    pub victim: Entity,
    pub bone: Bone,
    pub damage_type: DamageType,
    pub impact_speed: f32,
    pub point: Vec3,
    pub direction: Vec3,
    pub quality: f32,
}

/// Two weapons met.
#[derive(Message, Debug, Clone, Copy, PartialEq)]
pub struct ClashMessage {
    pub kind: ClashKind,
    pub attacker_weapon: Entity,
    pub defender_weapon: Entity,
    pub attacker: Option<Entity>,
    pub defender: Option<Entity>,
    pub point: Vec3,
    pub recoil: f32,
}

/// A weapon got stuck in or pulled out of a body.
#[derive(Message, Debug, Clone, Copy, PartialEq)]
pub enum StabMessage {
    Stuck {
        weapon: Entity,
        victim: Entity,
        bone: Bone,
    },
    PulledOut {
        weapon: Entity,
        victim: Entity,
    },
}

/// A stab: the joint, the body it is anchored to and the wound axis in that body's frame.
#[derive(Debug, Clone, Copy)]
struct Wound {
    joint: Entity,
    body: Entity,
    axis: Vec3,
}

/// Stab joints by weapon.
#[derive(Resource, Debug, Default)]
struct StabJoints(HashMap<Entity, Wound>);

/// A weapon that came out of a wound passes bodies for this long (s) so it does not get shoved
/// out of the body it is still partly inside.
pub const PASS_AFTER_PULL: f32 = 0.25;

/// Drag of a wound on the blade along its axis, 1/s (fraction of the sliding velocity removed
/// per second, scaled by the weapon's mass): pushing a blade in or pulling it out takes force.
pub const WOUND_DRAG: f32 = 12.0;

/// The weapon passes bodies until then (elapsed seconds); see [`PASS_AFTER_PULL`].
#[derive(Component, Debug, Clone, Copy)]
struct PassBodies(f32);

#[derive(SystemSet, Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct MeleeSet;

/// Melee contacts. Needs [`gothic_vr_physics::VrPhysicsPlugin`].
pub struct VrMeleePlugin;

impl Plugin for VrMeleePlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<Melee>()
            .init_resource::<StabJoints>()
            .add_message::<HitMessage>()
            .add_message::<ClashMessage>()
            .add_message::<StabMessage>()
            .add_message::<HapticMessage>()
            .add_systems(Update, auto_weapons)
            .add_systems(
                FixedUpdate,
                (track_wielders, melee_step, wound_drag, collide_again)
                    .chain()
                    .in_set(MeleeSet)
                    .before(HandSet::Grab),
            );
    }
}

/// Give props with a melee profile their [`MeleeWeapon`].
fn auto_weapons(mut commands: Commands, props: Query<(Entity, &Prop), Without<MeleeWeapon>>) {
    for (e, p) in &props {
        if let Some(profile) = profile_for(p.0.kind) {
            commands.entity(e).insert(MeleeWeapon::new(profile));
        } else {
            commands.entity(e).insert(MeleeWeapon::new(MeleeProfile {
                strikers: vec![],
                guard: (Vec3::ZERO, Vec3::ZERO, 0.0),
                max_penetration: 0.0,
            }));
        }
    }
}

/// Grabbed weapons belong to the player avatar (if any) while held.
fn track_wielders(
    mut grabs: MessageReader<GrabMessage>,
    player: Query<Entity, With<PlayerAvatar>>,
    mut weapons: Query<&mut MeleeWeapon>,
    held: Query<&gothic_vr_physics::Held>,
) {
    for g in grabs.read() {
        match *g {
            GrabMessage::Grabbed { object, .. } => {
                if let Ok(mut w) = weapons.get_mut(object) {
                    w.wielder = player.iter().next();
                }
            }
            GrabMessage::Released { object, .. } => {
                if let Ok(mut w) = weapons.get_mut(object)
                    && held.get(object).is_ok_and(|h| h.count() == 0)
                {
                    w.wielder = None;
                }
            }
        }
    }
}

#[allow(clippy::too_many_arguments, clippy::type_complexity)]
fn melee_step(
    mut commands: Commands,
    time: Res<Time>,
    mut melee: ResMut<Melee>,
    mut joints: ResMut<StabJoints>,
    mut weapons: Query<(Entity, &Transform, &mut MeleeWeapon)>,
    mut sets: Query<(Entity, &mut HitzoneSet)>,
    transforms: Query<&Transform>,
    mut bodies: Query<Forces>,
    masses: Query<&ComputedMass>,
    mut hands: Query<&mut PhysicsHand>,
    mut hits: MessageWriter<HitMessage>,
    mut clashes: MessageWriter<ClashMessage>,
    mut stabs: MessageWriter<StabMessage>,
    mut haptics: MessageWriter<HapticMessage>,
) {
    let dt = time.delta_secs();
    #[allow(clippy::type_complexity)]
    let motions: Vec<(
        Entity,
        Option<Entity>,
        Pose,
        Pose,
        MeleeProfile,
        Option<Pose>,
    )> = weapons
        .iter()
        .map(|(e, t, w)| {
            let cur = transform_to_pose(t);
            (
                e,
                w.wielder,
                w.last.unwrap_or(cur),
                cur,
                w.profile.clone(),
                w.before,
            )
        })
        .collect();
    let wm: Vec<WeaponMotion> = motions
        .iter()
        .map(|(e, wielder, prev, cur, profile, before)| WeaponMotion {
            id: e.to_bits(),
            wielder: wielder.map(Entity::to_bits),
            prev: *prev,
            cur: *cur,
            before: *before,
            profile,
        })
        .collect();
    let set_list: Vec<(Entity, Vec<Hitzone>, Vec<Hitzone>)> = sets
        .iter()
        .map(|(e, s)| {
            let prev = if s.prev.len() == s.zones.len() {
                s.prev.clone()
            } else {
                s.zones.clone()
            };
            (e, prev, s.zones.clone())
        })
        .collect();
    let bm: Vec<BodyMotion> = set_list
        .iter()
        .map(|(e, prev, cur)| BodyMotion {
            id: e.to_bits(),
            prev,
            cur,
        })
        .collect();
    let events = melee.0.step(&wm, &bm, dt);
    for (_, mut s) in &mut sets {
        s.prev = s.zones.clone();
    }
    for (_, t, mut w) in &mut weapons {
        w.before = w.last;
        w.last = Some(transform_to_pose(t));
    }

    let ent = Entity::from_bits;
    let impulse = |bodies: &mut Query<Forces>, e: Entity, dv: Vec3, at: Vec3| {
        let m = masses.get(e).map_or(1.0, |m| m.value());
        if let Ok(mut f) = bodies.get_mut(e) {
            f.apply_linear_impulse_at_point(dv * m, at);
        }
    };
    // Hands holding a weapon.
    let holders = |hands: &Query<&mut PhysicsHand>, weapon: Entity| -> Vec<gothic_vr::Hand> {
        hands
            .iter()
            .filter(|h| h.held.is_some_and(|b| b.object == weapon))
            .map(|h| h.hand)
            .collect()
    };
    for ev in events {
        match ev {
            MeleeEvent::Hit(h) => {
                // No bite impulse: the body's collider stops the blade.
                for hand in holders(&hands, ent(h.weapon)) {
                    haptics.write(HapticMessage(HapticPulse::hit(
                        hand,
                        h.impact_speed,
                        h.quality,
                    )));
                }
                hits.write(HitMessage {
                    attacker: h.attacker.map(ent),
                    weapon: ent(h.weapon),
                    victim: ent(h.victim),
                    bone: h.bone,
                    damage_type: h.damage_type,
                    impact_speed: h.impact_speed,
                    point: h.point,
                    direction: h.direction,
                    quality: h.quality,
                });
            }
            MeleeEvent::Clash {
                kind,
                attacker_weapon,
                defender_weapon,
                attacker,
                defender,
                point,
                attacker_impulse,
                recoil,
            } => {
                impulse(&mut bodies, ent(attacker_weapon), attacker_impulse, point);
                let closing = attacker_impulse.length();
                for mut h in &mut hands {
                    let Some(b) = h.held else { continue };
                    if b.object == ent(attacker_weapon) {
                        h.stagger(recoil.min(0.5));
                    }
                    if b.object == ent(attacker_weapon) || b.object == ent(defender_weapon) {
                        let hand = h.hand;
                        haptics.write(HapticMessage(HapticPulse::clash(hand, 2.0 + closing)));
                    }
                }
                clashes.write(ClashMessage {
                    kind,
                    attacker_weapon: ent(attacker_weapon),
                    defender_weapon: ent(defender_weapon),
                    attacker: attacker.map(ent),
                    defender: defender.map(ent),
                    point,
                    recoil,
                });
            }
            MeleeEvent::Stuck {
                weapon,
                victim,
                bone,
                entry,
                axis,
            } => {
                let (weapon, victim) = (ent(weapon), ent(victim));
                let zone_body = sets
                    .get(victim)
                    .ok()
                    .and_then(|(_, s)| {
                        s.zones
                            .iter()
                            .position(|z| z.bone == bone)
                            .and_then(|i| s.bodies[i])
                    })
                    .unwrap_or(victim);
                if let (Ok(bt), Ok(wt)) = (transforms.get(zone_body), transforms.get(weapon)) {
                    let max = motions
                        .iter()
                        .find(|m| m.0 == weapon)
                        .map_or(0.3, |m| m.4.max_penetration);
                    let body = transform_to_pose(bt);
                    let wpn = transform_to_pose(wt);
                    let inv_b = body.inverse();
                    let inv_w = wpn.inverse();
                    let joint = PrismaticJoint::new(zone_body, weapon)
                        .with_local_frame1(Isometry3d::new(
                            inv_b.transform_point(entry),
                            Quat::IDENTITY,
                        ))
                        .with_local_frame2(Isometry3d::new(
                            inv_w.transform_point(entry),
                            inv_w.rotation * body.rotation,
                        ))
                        .with_slider_axis(inv_b.rotation * axis)
                        .with_limits(-0.02, max);
                    let j = commands
                        .spawn((joint, JointCollisionDisabled, Name::new("StabJoint")))
                        .id();
                    joints.0.insert(
                        weapon,
                        Wound {
                            joint: j,
                            body: zone_body,
                            axis: inv_b.rotation * axis,
                        },
                    );
                    // The blade is in: let it through the body's colliders.
                    commands
                        .entity(weapon)
                        .insert(prop_layers(true))
                        .remove::<PassBodies>();
                }
                for hand in holders(&hands, weapon) {
                    haptics.write(HapticMessage(HapticPulse::stuck(hand)));
                }
                stabs.write(StabMessage::Stuck {
                    weapon,
                    victim,
                    bone,
                });
            }
            MeleeEvent::PulledOut { weapon, victim } => {
                if let Some(w) = joints.0.remove(&ent(weapon)) {
                    commands.entity(w.joint).despawn();
                    commands
                        .entity(ent(weapon))
                        .insert(PassBodies(time.elapsed_secs() + PASS_AFTER_PULL));
                }
                for hand in holders(&hands, ent(weapon)) {
                    haptics.write(HapticMessage(HapticPulse::pulled_out(hand)));
                }
                stabs.write(StabMessage::PulledOut {
                    weapon: ent(weapon),
                    victim: ent(victim),
                });
            }
        }
    }
}

/// A stuck blade sliding along its wound is slowed down.
fn wound_drag(
    joints: Res<StabJoints>,
    transforms: Query<&Transform>,
    masses: Query<&ComputedMass>,
    mut bodies: Query<Forces>,
) {
    for (&weapon, w) in &joints.0 {
        let Ok(t) = transforms.get(w.body) else {
            continue;
        };
        let axis = (t.rotation * w.axis).normalize_or_zero();
        let body_v = bodies
            .get_mut(w.body)
            .map_or(Vec3::ZERO, |f| f.linear_velocity());
        let m = masses.get(weapon).map_or(1.0, |m| m.value());
        if let Ok(mut f) = bodies.get_mut(weapon) {
            let slide = (f.linear_velocity() - body_v).dot(axis);
            f.apply_force(-axis * slide * m * WOUND_DRAG);
        }
    }
}

/// A pulled-out blade collides with bodies again once its grace time is over.
fn collide_again(mut commands: Commands, time: Res<Time>, passing: Query<(Entity, &PassBodies)>) {
    for (e, p) in &passing {
        if time.elapsed_secs() >= p.0 {
            commands
                .entity(e)
                .insert(prop_layers(false))
                .remove::<PassBodies>();
        }
    }
}

/// Collider matching a set of hitzones, in the frame of `origin` (so blades stop where the
/// hitzones are; a coarser collider would stop them before they register a hit).
pub fn zone_collider(zones: &[Hitzone], origin: &Pose) -> Collider {
    let inv = origin.inverse();
    Collider::compound(
        zones
            .iter()
            .map(|z| {
                let local = inv.mul(&z.pose);
                (
                    local.position,
                    local.rotation,
                    Collider::capsule(z.radius, z.half_length * 2.0),
                )
            })
            .collect(),
    )
}

/// Collision layers of a body (collides with the world, hands and props).
pub fn body_layers() -> CollisionLayers {
    CollisionLayers::new(
        VrLayer::Body,
        [VrLayer::World, VrLayer::Hand, VrLayer::Prop],
    )
}

/// Keeps a [`PlayerAvatar`]'s [`HitzoneSet`] on the IK body.
pub struct PlayerHitzonesPlugin;

impl Plugin for PlayerHitzonesPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Startup, spawn_player_avatar)
            .add_systems(FixedUpdate, player_hitzones.before(MeleeSet));
    }
}

fn spawn_player_avatar(mut commands: Commands) {
    commands.spawn((
        PlayerAvatar,
        HitzoneSet::default(),
        Name::new("PlayerAvatar"),
    ));
}

/// Capsules of the generic humanoid from an IK pose.
pub fn hitzones_from_body(pose: &gothic_vr::body::BodyPose) -> Vec<Hitzone> {
    let z = |bone, a: Joint, b: Joint, r| Hitzone::between(bone, pose.get(a), pose.get(b), r);
    let head_top = pose.get(Joint::Head) + (pose.get(Joint::Head) - pose.get(Joint::Neck)) * 0.6;
    vec![
        Hitzone::between(Bone::Head, pose.get(Joint::Neck), head_top, 0.1),
        z(Bone::Chest, Joint::Spine, Joint::Neck, 0.16),
        z(Bone::Abdomen, Joint::Pelvis, Joint::Spine, 0.15),
        z(Bone::UpperArmL, Joint::ShoulderL, Joint::ElbowL, 0.05),
        z(Bone::UpperArmR, Joint::ShoulderR, Joint::ElbowR, 0.05),
        z(Bone::ForearmL, Joint::ElbowL, Joint::HandL, 0.04),
        z(Bone::ForearmR, Joint::ElbowR, Joint::HandR, 0.04),
        z(Bone::ThighL, Joint::HipL, Joint::KneeL, 0.07),
        z(Bone::ThighR, Joint::HipR, Joint::KneeR, 0.07),
        z(Bone::ShinL, Joint::KneeL, Joint::AnkleL, 0.05),
        z(Bone::ShinR, Joint::KneeR, Joint::AnkleR, 0.05),
    ]
}

fn player_hitzones(body: Res<VrBody>, mut sets: Query<&mut HitzoneSet, With<PlayerAvatar>>) {
    let Some(pose) = &body.pose else {
        return;
    };
    for mut s in &mut sets {
        let zones = hitzones_from_body(pose);
        if s.bodies.len() != zones.len() {
            s.bodies = vec![None; zones.len()];
        }
        s.zones = zones;
    }
}

/// A static training dummy (kinematic capsule body + chest/abdomen/head hitzones) standing at
/// `feet`.
pub fn spawn_dummy(commands: &mut Commands, feet: Vec3) -> Entity {
    let zones = dummy_zones(feet);
    let origin = Pose::new(feet + Vec3::Y * 0.85, Quat::IDENTITY);
    commands
        .spawn((
            RigidBody::Kinematic,
            zone_collider(&zones, &origin),
            HitzoneSet::new(zones),
            body_layers(),
            Transform::from_translation(origin.position),
            Name::new("Dummy"),
        ))
        .id()
}

/// Hitzones of the training dummy.
pub fn dummy_zones(feet: Vec3) -> Vec<Hitzone> {
    let p = |y: f32| feet + Vec3::Y * y;
    vec![
        Hitzone::between(Bone::Abdomen, p(0.85), p(1.1), 0.17),
        Hitzone::between(Bone::Chest, p(1.1), p(1.45), 0.19),
        Hitzone::between(Bone::Head, p(1.55), p(1.75), 0.11),
    ]
}
