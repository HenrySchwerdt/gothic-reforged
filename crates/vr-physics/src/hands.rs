//! Boneworks-style physics hands on avian3d.
//!
//! * Each [`PhysicsHand`] is a dynamic sphere that chases its tracked [`XrHand`] with a
//!   strength-limited PD force/torque (see [`crate::pd`]). It collides with the world, so it
//!   stops at walls while the tracked (ghost) hand goes on.
//! * Closing the grip picks the best grip point ([`crate::grab::choose_grab`]) and locks the
//!   object to the hand with a [`FixedJoint`]; the joint's frame on the object is the grip
//!   frame, so the weapon snaps into the hand. Opening the grip removes the joint; the object
//!   keeps its velocity (throwing).
//! * Two hands: the first hand keeps the fixed joint and leads (position and roll); the second
//!   hangs on a ball joint and only aims the object, by turning the leading hand's target so the
//!   grip line points at the second hand (the scheme of Godot XR Tools' grab driver and of
//!   most VR melee games; reimplemented). Two fixed joints would fight each other and make the
//!   weapon jitter or spin. Letting go of the leading hand hands the lead to the other one.
//! * The PD sees the hand plus its share of the held mass, so heavy weapons lag and sag. It
//!   chases the controller's angular velocity as well as its pose, so fast wrist turns do not
//!   lag behind and the blade does not wobble around the target.
//! * The hand body carries an artificial wrist inertia ([`HAND_INERTIA`]): a bare 4.5 cm sphere
//!   is hundreds of times lighter in rotation than a sword, which leaves the grip joint to be
//!   solved between wildly different bodies (VRExpansion and others give the hand a virtual
//!   inertia for the same reason).
//! * Props collide with bodies: a blade stops at a body instead of passing through it (the
//!   melee layer only turns a qualifying stab into a joint and lets that blade through, see
//!   [`prop_layers`]). Weapons get low friction and a little restitution so blade on blade
//!   slides and rings off instead of sticking.
//! * [`PhysicsHand::stagger`] weakens the arm for a moment (a blocked or parried swing): the
//!   force and torque limits drop to [`STAGGER_STRENGTH`] and ramp back, so the deflection the
//!   contact gave the weapon is not cancelled by the drive in the next step.
//! * A hand that is stuck far from its target (behind geometry) or got teleported snaps back.
use crate::grab::{Candidate, GripPoint, choose_grab, object_pose_for_hand};
use crate::pd::{PdGains, angular_torque, arm_limits, inertia_about, linear_force};
use crate::props::{PartShape, PropKind, PropSpec};
use avian3d::prelude::*;
use bevy::prelude::*;
use gothic_vr::{
    Hand, HapticMessage, HapticPulse, Pose, VrButton, VrInputRes, XrHand, transform_to_pose,
};

/// Player strength (Gothic `STR`), the input of the arm force limits.
#[derive(Resource, Debug, Clone, Copy)]
pub struct VrStrength(pub f32);

impl Default for VrStrength {
    fn default() -> Self {
        Self(10.0)
    }
}

/// Collision layers used by the VR physics.
#[derive(PhysicsLayer, Default, Clone, Copy, Debug)]
pub enum VrLayer {
    #[default]
    World,
    Hand,
    Prop,
    Body,
}

/// A physical hand.
#[derive(Component, Debug, Clone)]
pub struct PhysicsHand {
    pub hand: Hand,
    pub linear: PdGains,
    pub angular: PdGains,
    /// The held object and the joint entity.
    pub held: Option<HeldBy>,
    /// Seconds the hand has been far from its target.
    pub stuck_for: f32,
    /// Seconds of weakened arm left (see [`PhysicsHand::stagger`]).
    pub staggered: f32,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct HeldBy {
    pub object: Entity,
    pub joint: Entity,
    /// Object-frame anchor of this hand.
    pub anchor: Vec3,
    /// Object-frame rotation of the grip.
    pub rotation: Quat,
    /// Second hand of a two-handed grip: ball joint, aims instead of leading.
    pub secondary: bool,
}

impl PhysicsHand {
    pub fn new(hand: Hand) -> Self {
        Self {
            hand,
            linear: PdGains::HAND_LINEAR,
            angular: PdGains::HAND_ANGULAR,
            held: None,
            stuck_for: 0.0,
            staggered: 0.0,
        }
    }

    /// Weaken this arm for `seconds` (the last [`STAGGER_RAMP`] of them ramping back).
    pub fn stagger(&mut self, seconds: f32) {
        if seconds.is_finite() {
            self.staggered = self.staggered.max(seconds.min(2.0));
        }
    }

    /// Current fraction of the arm limits (1 when not staggered).
    pub fn strength_scale(&self) -> f32 {
        if self.staggered <= 0.0 {
            1.0
        } else if self.staggered >= STAGGER_RAMP {
            STAGGER_STRENGTH
        } else {
            STAGGER_STRENGTH + (1.0 - STAGGER_STRENGTH) * (1.0 - self.staggered / STAGGER_RAMP)
        }
    }
}

/// Arm limits while staggered, as a fraction.
pub const STAGGER_STRENGTH: f32 = 0.3;
/// Seconds over which a stagger ramps back to full strength.
pub const STAGGER_RAMP: f32 = 0.2;

/// Something hands can pick up.
#[derive(Component, Debug, Clone)]
pub struct Grabbable {
    pub grips: Vec<GripPoint>,
    pub free_grab_radius: Option<f32>,
}

/// Which hands hold this object (object-frame anchors).
#[derive(Component, Debug, Clone, Default)]
pub struct Held {
    pub hands: [Option<Vec3>; 2],
}

impl Held {
    pub fn count(&self) -> usize {
        self.hands.iter().flatten().count()
    }
}

/// The prop kind (for other mechanics) and its spec.
#[derive(Component, Debug, Clone)]
pub struct Prop(pub PropSpec);

/// A grab started or ended.
#[derive(Message, Debug, Clone, Copy, PartialEq)]
pub enum GrabMessage {
    Grabbed {
        hand: Hand,
        object: Entity,
        grip: Option<usize>,
    },
    Released {
        hand: Hand,
        object: Entity,
        velocity: Vec3,
    },
}

/// Hand body radius, m.
pub const HAND_RADIUS: f32 = 0.045;
/// Hand body mass, kg (the arm's own effective mass at the hand).
pub const HAND_MASS: f32 = 0.5;
/// Angular inertia of the hand body, kg*m^2: hand and wrist, not the bare collision sphere
/// (which would be about 0.0004).
pub const HAND_INERTIA: f32 = 0.01;
/// Beyond this distance from the target a hand counts as stuck.
const STUCK_DISTANCE: f32 = 0.45;
/// Stuck this long (s) -> snap.
const STUCK_TIME: f32 = 0.35;
/// Beyond this distance the hand snaps immediately (teleport).
const SNAP_DISTANCE: f32 = 1.5;

#[derive(SystemSet, Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum HandSet {
    Grab,
    Drive,
}

/// Physics hands, grabbing and weighted props. Add `PhysicsPlugins` yourself (or set
/// `add_physics`).
pub struct VrPhysicsPlugin {
    pub add_physics: bool,
}

impl Default for VrPhysicsPlugin {
    fn default() -> Self {
        Self { add_physics: true }
    }
}

impl Plugin for VrPhysicsPlugin {
    fn build(&self, app: &mut App) {
        if self.add_physics {
            app.add_plugins(PhysicsPlugins::default());
        }
        app.init_resource::<VrStrength>()
            .add_message::<GrabMessage>()
            .add_message::<HapticMessage>()
            .configure_sets(FixedUpdate, (HandSet::Grab, HandSet::Drive).chain())
            .add_systems(Startup, spawn_hands)
            .add_systems(FixedUpdate, grab_release.in_set(HandSet::Grab))
            .add_systems(
                FixedUpdate,
                (tick_stagger, snap_stuck_hands, drive_hands)
                    .chain()
                    .in_set(HandSet::Drive),
            );
    }
}

fn spawn_hands(mut commands: Commands) {
    for hand in Hand::BOTH {
        commands.spawn(hand_bundle(hand, Vec3::new(0.2 * hand.sign(), 1.2, -0.3)));
    }
}

/// Components of a physics hand.
pub fn hand_bundle(hand: Hand, at: Vec3) -> impl Bundle {
    (
        PhysicsHand::new(hand),
        RigidBody::Dynamic,
        Collider::sphere(HAND_RADIUS),
        ColliderDensity(HAND_MASS / (4.0 / 3.0 * std::f32::consts::PI * HAND_RADIUS.powi(3))),
        AngularInertia::new(Vec3::splat(HAND_INERTIA)),
        NoAutoAngularInertia,
        GravityScale(0.0),
        SweptCcd::default(),
        CollisionLayers::new(
            VrLayer::Hand,
            [VrLayer::World, VrLayer::Prop, VrLayer::Body],
        ),
        Transform::from_translation(at),
        Name::new(format!("PhysicsHand{hand:?}")),
    )
}

/// Collision layers of a prop. `through_bodies` lets it pass bodies (a blade stuck in a wound,
/// for a moment after it came out).
pub fn prop_layers(through_bodies: bool) -> CollisionLayers {
    if through_bodies {
        CollisionLayers::new(
            VrLayer::Prop,
            [VrLayer::World, VrLayer::Hand, VrLayer::Prop],
        )
    } else {
        CollisionLayers::new(
            VrLayer::Prop,
            [VrLayer::World, VrLayer::Hand, VrLayer::Prop, VrLayer::Body],
        )
    }
}

/// Spawn a prop as a dynamic rigid body with a compound collider.
pub fn spawn_prop(commands: &mut Commands, spec: &PropSpec, pose: Pose) -> Entity {
    let shapes: Vec<(Vec3, Quat, Collider)> = spec
        .parts
        .iter()
        .map(|p| {
            let c = match p.shape {
                PartShape::Box(h) => Collider::cuboid(h.x * 2.0, h.y * 2.0, h.z * 2.0),
                PartShape::Capsule {
                    radius,
                    half_length,
                } => Collider::capsule(radius, half_length * 2.0),
            };
            (p.offset, p.rotation, c)
        })
        .collect();
    let mut e = commands.spawn((
        Prop(spec.clone()),
        Grabbable {
            grips: spec.grips.clone(),
            free_grab_radius: spec.free_grab_radius,
        },
        Held::default(),
        RigidBody::Dynamic,
        Collider::compound(shapes),
        ColliderDensity(spec.density()),
        prop_layers(false),
        Transform::from_translation(pose.position).with_rotation(pose.rotation),
        Name::new(format!("{:?}", spec.kind)),
    ));
    if matches!(
        spec.kind,
        PropKind::Arrow | PropKind::Sword | PropKind::TwoHandedSword
    ) {
        e.insert(SweptCcd::default());
    }
    if matches!(
        spec.kind,
        PropKind::Sword | PropKind::TwoHandedSword | PropKind::Axe | PropKind::Mace
    ) {
        // Steel on steel: slides and rings off a little.
        e.insert((Friction::new(0.3), Restitution::new(0.15)));
    }
    e.id()
}

/// Static world box (metres).
pub fn static_box(center: Vec3, half: Vec3, rotation: Quat) -> impl Bundle {
    (
        RigidBody::Static,
        Collider::cuboid(half.x * 2.0, half.y * 2.0, half.z * 2.0),
        CollisionLayers::new(
            VrLayer::World,
            [VrLayer::Hand, VrLayer::Prop, VrLayer::Body],
        ),
        Transform::from_translation(center).with_rotation(rotation),
    )
}

/// Static colliders for a gothic-vr greybox layout.
pub fn spawn_static_greybox(commands: &mut Commands, boxes: &[gothic_vr::greybox::GreyboxBox]) {
    for b in boxes {
        commands.spawn((
            static_box(b.center, b.half_extents, b.rotation),
            Name::new(b.name),
        ));
    }
}

#[allow(clippy::type_complexity)]
fn grab_release(
    mut commands: Commands,
    input: Res<VrInputRes>,
    mut hands: Query<(Entity, &mut PhysicsHand, &Transform, &LinearVelocity)>,
    mut objects: Query<
        (
            Entity,
            &Transform,
            &Grabbable,
            &mut Held,
            Option<&LinearVelocity>,
        ),
        Without<PhysicsHand>,
    >,
    mut out: MessageWriter<GrabMessage>,
    mut haptics: MessageWriter<HapticMessage>,
) {
    // Objects whose leading hand let go while another hand still holds them.
    let mut lead_lost = Vec::new();
    for (hand_e, mut hand, ht, _) in &mut hands {
        let pressed = input.0.held(hand.hand, VrButton::Grip);
        match (&hand.held, pressed) {
            (Some(h), false) => {
                let h = *h;
                commands.entity(h.joint).despawn();
                let mut velocity = Vec3::ZERO;
                if let Ok((_, _, _, mut held, v)) = objects.get_mut(h.object) {
                    held.hands[hand.hand.index()] = None;
                    velocity = v.map_or(Vec3::ZERO, |v| v.0);
                    if !h.secondary && held.count() > 0 {
                        lead_lost.push(h.object);
                    }
                }
                hand.held = None;
                out.write(GrabMessage::Released {
                    hand: hand.hand,
                    object: h.object,
                    velocity,
                });
            }
            (None, true) if input.0.just_pressed(hand.hand, VrButton::Grip) => {
                let hand_pose = transform_to_pose(ht);
                let taken: Vec<Vec<Vec3>> = objects
                    .iter()
                    .map(|(_, _, _, held, _)| held.hands.iter().flatten().copied().collect())
                    .collect();
                let cands: Vec<Candidate<'_, Entity>> = objects
                    .iter()
                    .zip(&taken)
                    .map(|((e, t, g, _, _), taken)| Candidate {
                        key: e,
                        pose: transform_to_pose(t),
                        grips: &g.grips,
                        free_grab_radius: g.free_grab_radius,
                        taken,
                    })
                    .collect();
                let Some(choice) = choose_grab(&hand_pose, &cands) else {
                    continue;
                };
                // Only the first hand pulls the object into place; a second hand attaches where
                // the object already is, on a ball joint.
                let secondary = objects
                    .get(choice.key)
                    .is_ok_and(|(_, _, _, held, _)| held.count() > 0);
                if let Ok((_, t, _, _, _)) = objects.get(choice.key)
                    && !secondary
                {
                    let want = object_pose_for_hand(&hand_pose, choice.anchor, choice.rotation);
                    let snap = transform_to_pose(t).position.distance(want.position) < 0.25;
                    if snap {
                        commands
                            .entity(choice.key)
                            .insert((Position(want.position), Rotation(want.rotation)));
                    }
                }
                let joint = grip_joint(
                    &mut commands,
                    hand.hand,
                    hand_e,
                    choice.key,
                    Isometry3d::new(choice.anchor, choice.rotation),
                    secondary,
                );
                if let Ok((_, _, _, mut held, _)) = objects.get_mut(choice.key) {
                    held.hands[hand.hand.index()] = Some(choice.anchor);
                }
                hand.held = Some(HeldBy {
                    object: choice.key,
                    joint,
                    anchor: choice.anchor,
                    rotation: choice.rotation,
                    secondary,
                });
                out.write(GrabMessage::Grabbed {
                    hand: hand.hand,
                    object: choice.key,
                    grip: choice.grip,
                });
                haptics.write(HapticMessage(HapticPulse::grab(hand.hand)));
            }
            _ => {}
        }
    }
    // The remaining hand takes the lead: its ball joint becomes a fixed one, keeping the grip
    // the hand has now.
    for object in lead_lost {
        let Ok((_, ot, _, _, _)) = objects.get(object) else {
            continue;
        };
        let object_pose = transform_to_pose(ot);
        for (hand_e, mut hand, ht, _) in &mut hands {
            let Some(h) = hand.held.filter(|h| h.object == object && h.secondary) else {
                continue;
            };
            commands.entity(h.joint).despawn();
            let rotation = object_pose.rotation.inverse() * transform_to_pose(ht).rotation;
            let joint = grip_joint(
                &mut commands,
                hand.hand,
                hand_e,
                object,
                Isometry3d::new(h.anchor, rotation),
                false,
            );
            hand.held = Some(HeldBy {
                joint,
                rotation,
                secondary: false,
                ..h
            });
        }
    }
}

/// The joint between a hand and what it holds: fixed for the leading hand, a ball joint for the
/// second hand of a two-handed grip.
fn grip_joint(
    commands: &mut Commands,
    hand: Hand,
    hand_e: Entity,
    object: Entity,
    frame: Isometry3d,
    secondary: bool,
) -> Entity {
    let name = Name::new(format!("Grip{hand:?}"));
    if secondary {
        commands
            .spawn((
                SphericalJoint::new(hand_e, object).with_local_frame2(frame),
                JointCollisionDisabled,
                name,
            ))
            .id()
    } else {
        commands
            .spawn((
                FixedJoint::new(hand_e, object).with_local_frame2(frame),
                JointCollisionDisabled,
                name,
            ))
            .id()
    }
}

/// Tracked pose, velocity and angular velocity of a hand.
fn target_of(targets: &Query<(&Transform, &XrHand)>, hand: Hand) -> Option<(Pose, Vec3, Vec3)> {
    targets
        .iter()
        .find(|(_, h)| h.hand == hand && h.tracked)
        .map(|(t, h)| (transform_to_pose(t), h.velocity, h.angular_velocity))
}

/// Target rotation of the leading hand of a two-handed grip: its own tracked rotation, swung so
/// the object's line from the leading grip to the second grip points at the second hand's
/// tracked position. Roll about that line stays the leading hand's.
pub fn two_hand_aim(
    lead: &Pose,
    grip_rotation: Quat,
    lead_anchor: Vec3,
    other_anchor: Vec3,
    other_target: Vec3,
) -> Quat {
    let along = other_anchor - lead_anchor;
    let want = other_target - lead.position;
    if along.length_squared() < 1e-6 || want.length_squared() < 1e-6 {
        return lead.rotation;
    }
    // Object rotation implied by the leading hand alone, and the grip line under it.
    let object = lead.rotation * grip_rotation.inverse();
    let now = (object * along).normalize();
    Quat::from_rotation_arc(now, want.normalize()) * lead.rotation
}

#[allow(clippy::type_complexity)]
fn snap_stuck_hands(
    time: Res<Time>,
    targets: Query<(&Transform, &XrHand)>,
    mut hands: Query<
        (
            &mut PhysicsHand,
            &mut Position,
            &mut Rotation,
            &mut LinearVelocity,
            &mut AngularVelocity,
        ),
        Without<Held>,
    >,
    mut objects: Query<
        (
            &Transform,
            &mut Position,
            &mut Rotation,
            &mut LinearVelocity,
            &mut AngularVelocity,
        ),
        (With<Held>, Without<PhysicsHand>),
    >,
) {
    let dt = time.delta_secs();
    for (mut hand, mut pos, mut rot, mut lv, mut av) in &mut hands {
        let Some((target, _, _)) = target_of(&targets, hand.hand) else {
            continue;
        };
        let d = pos.0.distance(target.position);
        hand.stuck_for = if d > STUCK_DISTANCE {
            hand.stuck_for + dt
        } else {
            0.0
        };
        if d < SNAP_DISTANCE && hand.stuck_for < STUCK_TIME {
            continue;
        }
        hand.stuck_for = 0.0;
        // Carry a held object along, keeping its grip alignment.
        if let Some(h) = hand.held
            && let Ok((t, mut opos, mut orot, mut olv, mut oav)) = objects.get_mut(h.object)
        {
            let hand_pose = Pose::new(pos.0, rot.0);
            let rel = hand_pose.inverse().mul(&transform_to_pose(t));
            let moved = target.mul(&rel);
            opos.0 = moved.position;
            orot.0 = moved.rotation;
            olv.0 = Vec3::ZERO;
            oav.0 = Vec3::ZERO;
        }
        pos.0 = target.position;
        rot.0 = target.rotation;
        lv.0 = Vec3::ZERO;
        av.0 = Vec3::ZERO;
    }
}

fn tick_stagger(time: Res<Time>, mut hands: Query<&mut PhysicsHand>) {
    let dt = time.delta_secs();
    for mut h in &mut hands {
        if h.staggered > 0.0 {
            h.staggered = (h.staggered - dt).max(0.0);
        }
    }
}

#[allow(clippy::type_complexity)]
fn drive_hands(
    time: Res<Time>,
    strength: Res<VrStrength>,
    gravity: Res<Gravity>,
    targets: Query<(&Transform, &XrHand)>,
    hands: Query<(Entity, &PhysicsHand)>,
    mut bodies: Query<Forces>,
    objects: Query<
        (
            &ComputedMass,
            &ComputedAngularInertia,
            &ComputedCenterOfMass,
            &Transform,
            &Held,
        ),
        Without<PhysicsHand>,
    >,
) {
    let dt = time.delta_secs();
    let full = arm_limits(strength.0);
    for (hand_e, hand) in &hands {
        let k = hand.strength_scale();
        let limits = crate::pd::ArmLimits {
            max_force: full.max_force * k,
            max_torque: full.max_torque * k,
        };
        let Some((mut target, target_vel, target_ang)) = target_of(&targets, hand.hand) else {
            continue;
        };
        // Leading hand of a two-handed grip: aim at the other hand.
        if let Some(h) = hand.held.filter(|h| !h.secondary)
            && let Some((other, other_h)) = hands.iter().find_map(|(_, o)| {
                o.held
                    .filter(|oh| oh.object == h.object && oh.secondary)
                    .map(|oh| (o.hand, oh))
            })
            && let Some((other_target, _, _)) = target_of(&targets, other)
        {
            target.rotation = two_hand_aim(
                &target,
                h.rotation,
                h.anchor,
                other_h.anchor,
                other_target.position,
            );
        }
        let Ok(f) = bodies.get_mut(hand_e) else {
            continue;
        };
        let (pos, rot) = (f.position(), f.rotation());
        let (vel, ang) = (f.linear_velocity(), f.angular_velocity());
        // Mass and inertia this arm moves: the hand plus its share of the held object. The
        // drive is split between hand and object (force by mass, torque by each body's
        // inertia tensor about the hand), so the grip joint only carries the residual and
        // stays stiff even for a 0.5 kg hand on a 4 kg sword with a tiny roll inertia.
        let mut mass = HAND_MASS;
        let mut held = None;
        let mut gravity_torque = Vec3::ZERO;
        let mut held_weight = 0.0;
        if let Some(h) = hand.held
            && let Ok((m, i, com, t, by)) = objects.get(h.object)
        {
            let share = 1.0 / by.count().max(1) as f32;
            let m = m.value() * share;
            // Only the leading hand turns the object; the second hand's ball joint passes no
            // torque.
            let turn = if h.secondary { 0.0 } else { 1.0 };
            let local = Mat3::from(i.tensor()) * turn;
            let com_world = t.transform_point(com.0);
            let i_about_hand = inertia_about(local, t.rotation, m, com_world, pos);
            mass += m;
            held_weight = m;
            // Hold the object's weight moment up as well.
            gravity_torque = -(com_world - pos).cross(gravity.0 * m) * turn;
            held = Some((h.object, m, i_about_hand));
        }
        let gravity_ff = gravity.0 * (held_weight / mass);
        let force = linear_force(
            pos,
            vel,
            target.position,
            target_vel,
            mass,
            gravity_ff,
            hand.linear,
            limits.max_force,
            dt,
        );
        let drive = angular_torque(
            rot,
            ang,
            target.rotation,
            target_ang,
            Mat3::IDENTITY,
            hand.angular,
            f32::INFINITY,
            dt,
        );
        // `drive` is the wished angular acceleration (unit inertia); scale per body.
        let hand_torque = drive * HAND_INERTIA;
        let obj_torque = held.map_or(Vec3::ZERO, |(_, _, i)| i * drive) + gravity_torque;
        let total = (hand_torque + obj_torque).length();
        let scale = if total > limits.max_torque {
            limits.max_torque / total
        } else {
            1.0
        };
        let lin_obj = held.map_or(0.0, |(_, m, _)| m / mass);
        if let Ok(mut f) = bodies.get_mut(hand_e) {
            f.apply_force(force * (1.0 - lin_obj));
            f.apply_torque(hand_torque * scale);
        }
        if let Some((obj, _, _)) = held
            && let Ok(mut f) = bodies.get_mut(obj)
        {
            // At the hand, so the object's share also produces the lever torque about it.
            f.apply_force_at_point(force * lin_obj, pos);
            f.apply_torque(obj_torque * scale);
        }
    }
}
