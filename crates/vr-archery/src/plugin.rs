//! Bevy/avian integration of the bow.
//!
//! The bow is an ordinary physics prop held by a physics hand (`gothic_vr_physics`). Each
//! fixed step the archer reads both physics hands, the grip buttons and the held bow:
//! taking an arrow over the shoulder spawns an arrow in the hand, the nocked arrow follows the
//! string, releasing fires a [`FlyingArrow`] that is simulated by [`crate::ballistics`]
//! against the static world (avian ray casts on [`VrLayer::World`]) and every
//! [`HitzoneSet`]. Body hits are reported as melee [`HitMessage`]s (`DamageType::Point`) plus
//! an [`ArrowHitMessage`] carrying the hook damage; stuck arrows ride along with the body.
use crate::ballistics::{Arrow, FlightEvent, RayWorld, StuckIn};
use crate::bow::{
    Archer, ArcherEvent, ArcherInput, ArcherState, ArrowDamage, LinearDamage, aim_assist,
};
use avian3d::prelude::*;
use bevy::prelude::*;
use gothic_vr::haptics::DrawTicker;
use gothic_vr::{
    Hand, HapticMessage, HapticPulse, Pose, VrButton, VrInputRes, VrPlayer, transform_to_pose,
};
use gothic_vr_melee::{Bone, DamageType, HitMessage, HitzoneSet, PlayerAvatar};
use gothic_vr_physics::props::{PropKind, arrow};
use gothic_vr_physics::{HandSet, Held, PhysicsHand, Prop, VrLayer, spawn_prop};

/// The player's archery state and options.
#[derive(Resource)]
pub struct Archery {
    pub archer: Archer,
    pub damage: Box<dyn ArrowDamage + Send + Sync>,
    /// Optional aim assist: (cone in degrees, strength 0..1).
    pub aim_assist: Option<(f32, f32)>,
    /// Stuck arrows kept before the oldest are removed.
    pub max_stuck: usize,
    /// String creaks while drawing (one per hand).
    pub ticks: [DrawTicker; 2],
}

impl Default for Archery {
    fn default() -> Self {
        Self {
            archer: Archer::default(),
            damage: Box::new(LinearDamage),
            aim_assist: None,
            max_stuck: 64,
            ticks: [DrawTicker::default(); 2],
        }
    }
}

/// The arrow in the hand or on the string (visual, no physics).
#[derive(Component, Debug)]
pub struct HeldArrow;

/// An arrow in flight or stuck.
#[derive(Component, Debug, Clone)]
pub struct FlyingArrow {
    pub arrow: Arrow,
    pub draw_factor: f32,
    pub stuck_since: Option<f32>,
}

/// An arrow hit a body.
#[derive(Message, Debug, Clone, Copy, PartialEq)]
pub struct ArrowHitMessage {
    pub arrow: Entity,
    pub victim: Entity,
    pub bone: Bone,
    pub draw_factor: f32,
    /// From the [`ArrowDamage`] hook.
    pub damage: f32,
    pub impact_speed: f32,
}

/// An arrow was fired.
#[derive(Message, Debug, Clone, Copy, PartialEq)]
pub struct ArrowFiredMessage {
    pub arrow: Entity,
    pub draw_factor: f32,
    pub speed: f32,
}

/// The two-handed bow. Needs the VR, physics and melee plugins.
pub struct VrArcheryPlugin;

impl Plugin for VrArcheryPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<Archery>()
            .add_message::<ArrowHitMessage>()
            .add_message::<ArrowFiredMessage>()
            .add_message::<HitMessage>()
            .add_message::<HapticMessage>()
            .add_systems(
                FixedUpdate,
                // After the hands moved (melee contacts run first in the step, before the
                // grabs); arrow hits are read by the next step's reactions.
                (archer, fly).chain().after(HandSet::Drive),
            );
    }
}

struct AvianWorld<'a, 'w, 's>(&'a SpatialQuery<'w, 's>);

impl RayWorld for AvianWorld<'_, '_, '_> {
    fn raycast(&self, origin: Vec3, dir: Vec3, max: f32) -> Option<(f32, Vec3)> {
        let d = Dir3::new(dir).ok()?;
        let filter = SpatialQueryFilter::from_mask(VrLayer::World);
        self.0
            .cast_ray(origin, d, max, true, &filter)
            .map(|h| (h.distance, h.normal))
    }
}

#[allow(clippy::too_many_arguments, clippy::type_complexity)]
fn archer(
    mut commands: Commands,
    mut archery: ResMut<Archery>,
    input: Res<VrInputRes>,
    player: Option<Res<VrPlayer>>,
    hands: Query<(&Transform, &PhysicsHand)>,
    bows: Query<(&Transform, &Prop, &Held)>,
    avatar: Query<Entity, With<PlayerAvatar>>,
    targets: Query<(Entity, &HitzoneSet), Without<PlayerAvatar>>,
    mut held_arrow: Query<
        (Entity, &mut Transform),
        (With<HeldArrow>, Without<PhysicsHand>, Without<Prop>),
    >,
    mut fired: MessageWriter<ArrowFiredMessage>,
    mut haptics: MessageWriter<HapticMessage>,
) {
    let head = player
        .as_ref()
        .map_or(input.0.head(), |p| p.world(&input.0.head()));
    let mut hand_pose = [None; 2];
    let mut busy = [false; 2];
    for (t, h) in &hands {
        hand_pose[h.hand.index()] = Some(transform_to_pose(t));
        busy[h.hand.index()] = h.held.is_some();
    }
    let bow = bows.iter().find_map(|(t, p, held)| {
        (p.0.kind == PropKind::Bow)
            .then(|| {
                Hand::BOTH
                    .into_iter()
                    .find(|h| held.hands[h.index()].is_some())
                    .map(|h| (transform_to_pose(t), h))
            })
            .flatten()
    });
    let ai = ArcherInput {
        head,
        hands: hand_pose,
        grip: Hand::BOTH.map(|h| input.0.held(h, VrButton::Grip)),
        bow,
        busy,
    };
    let string_hand = match archery.archer.state {
        ArcherState::Holding(h) | ArcherState::Nocked { hand: h, .. } => Some(h),
        ArcherState::Idle => None,
    };
    let bow_hand = bow.map(|(_, h)| h);
    let events = archery.archer.update(&ai);
    // Haptics (Valve's Longbow / ArrowHand): a click on nocking, creaks while drawing on both
    // hands (lighter on the bow), a kick on release.
    if let ArcherState::Nocked { hand, draw } = archery.archer.state {
        let k = archery.archer.bow.draw_factor(draw);
        let pulses = [
            archery.ticks[0].update(hand, k),
            bow_hand.and_then(|b| {
                archery.ticks[1].update(b, k).map(|mut p| {
                    p.amplitude *= 0.6;
                    p
                })
            }),
        ];
        for p in pulses.into_iter().flatten() {
            haptics.write(HapticMessage(p));
        }
    } else {
        for t in &mut archery.ticks {
            t.update(Hand::Left, 0.0);
        }
    }
    for ev in events {
        match ev {
            ArcherEvent::TookArrow(h) => {
                haptics.write(HapticMessage(HapticPulse::grab(h)));
                commands.spawn((HeldArrow, Transform::default(), Name::new("HeldArrow")));
            }
            ArcherEvent::Nocked => {
                for h in [string_hand, bow_hand].into_iter().flatten() {
                    haptics.write(HapticMessage(HapticPulse::nock(h)));
                }
            }
            ArcherEvent::Fired {
                origin,
                velocity,
                draw_factor,
            } => {
                if let Some(h) = string_hand {
                    haptics.write(HapticMessage(HapticPulse::release(h, draw_factor, true)));
                }
                if let Some(h) = bow_hand {
                    haptics.write(HapticMessage(HapticPulse::release(h, draw_factor, false)));
                }
                for (e, _) in &held_arrow {
                    commands.entity(e).despawn();
                }
                let velocity = match archery.aim_assist {
                    Some((cone, strength)) => {
                        let list: Vec<(u64, Vec3)> = targets
                            .iter()
                            .filter_map(|(e, s)| {
                                let z = s
                                    .zones
                                    .iter()
                                    .find(|z| z.bone == Bone::Chest)
                                    .or(s.zones.first())?;
                                Some((e.to_bits(), z.pose.position))
                            })
                            .collect();
                        aim_assist(origin, velocity, &list, cone, strength).0
                    }
                    None => velocity,
                };
                let shooter = avatar.iter().next().map(Entity::to_bits);
                let a = Arrow::new(archery.archer.arrow, origin, velocity, shooter);
                let e = commands
                    .spawn((
                        FlyingArrow {
                            arrow: a,
                            draw_factor,
                            stuck_since: None,
                        },
                        Transform::from_translation(origin).with_rotation(a.rotation()),
                        Name::new("Arrow"),
                    ))
                    .id();
                fired.write(ArrowFiredMessage {
                    arrow: e,
                    draw_factor,
                    speed: velocity.length(),
                });
            }
            ArcherEvent::Dropped { at } => {
                for (e, _) in &held_arrow {
                    commands.entity(e).despawn();
                }
                spawn_prop(&mut commands, &arrow(), at);
            }
        }
    }
    if !matches!(archery.archer.state, ArcherState::Idle)
        && let Some(p) = archery.archer.arrow_pose(&ai)
    {
        for (_, mut t) in &mut held_arrow {
            t.translation = p.position;
            t.rotation = p.rotation;
        }
    }
}

#[allow(clippy::too_many_arguments, clippy::type_complexity)]
fn fly(
    mut commands: Commands,
    time: Res<Time>,
    archery: Res<Archery>,
    spatial: SpatialQuery,
    sets: Query<(Entity, &HitzoneSet)>,
    mut arrows: Query<(Entity, &mut FlyingArrow, &mut Transform)>,
    mut hits: MessageWriter<HitMessage>,
    mut arrow_hits: MessageWriter<ArrowHitMessage>,
) {
    let dt = time.delta_secs();
    let now = time.elapsed_secs();
    let world = AvianWorld(&spatial);
    let owned: Vec<(u64, Vec<gothic_vr_melee::Hitzone>)> = sets
        .iter()
        .map(|(e, s)| (e.to_bits(), s.zones.clone()))
        .collect();
    let bodies: Vec<(u64, &[gothic_vr_melee::Hitzone])> =
        owned.iter().map(|(e, z)| (*e, z.as_slice())).collect();
    let zone_of = |victim: u64, zone: usize| {
        owned
            .iter()
            .find(|(e, _)| *e == victim)
            .and_then(|(_, z)| z.get(zone).copied())
    };
    let mut stuck: Vec<(f32, Entity)> = Vec::new();
    for (e, mut fa, mut t) in &mut arrows {
        if let Some(ev) = fa.arrow.step(&world, &bodies, dt) {
            if let FlightEvent::HitBody {
                victim,
                zone,
                point,
                speed,
                direction,
            } = ev
                && let Some(z) = zone_of(victim, zone)
            {
                let victim = Entity::from_bits(victim);
                let k = fa.draw_factor;
                let spec = &archery.archer;
                hits.write(HitMessage {
                    attacker: fa.arrow.shooter.map(Entity::from_bits),
                    weapon: e,
                    victim,
                    bone: z.bone,
                    damage_type: DamageType::Point,
                    impact_speed: speed,
                    point,
                    direction,
                    quality: k,
                });
                arrow_hits.write(ArrowHitMessage {
                    arrow: e,
                    victim,
                    bone: z.bone,
                    draw_factor: k,
                    damage: archery.damage.damage(spec.arrow.damage, spec.bow.damage, k),
                    impact_speed: speed,
                });
            }
            if fa.arrow.stuck.is_some() && fa.stuck_since.is_none() {
                fa.stuck_since = Some(now);
            }
        }
        let pose: Pose = fa.arrow.pose(zone_of);
        t.translation = pose.position;
        t.rotation = pose.rotation;
        if let Some(s) = fa.stuck_since {
            stuck.push((s, e));
        }
        // Arrows that flew off forever.
        if fa.arrow.age > 20.0 && fa.arrow.stuck.is_none() {
            commands.entity(e).despawn();
        }
        // An arrow stuck in a body whose hitzones are gone drops out.
        if let Some(StuckIn::Body { victim, .. }) = fa.arrow.stuck
            && !owned.iter().any(|(b, _)| *b == victim)
        {
            commands.entity(e).despawn();
        }
    }
    if stuck.len() > archery.max_stuck {
        stuck.sort_by(|a, b| a.0.total_cmp(&b.0));
        for (_, e) in stuck.iter().take(stuck.len() - archery.max_stuck) {
            commands.entity(*e).despawn();
        }
    }
}
