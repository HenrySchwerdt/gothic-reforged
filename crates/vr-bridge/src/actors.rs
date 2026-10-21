//! Runtime actors as VR melee targets.
//!
//! Each actor the core spawns (`Event::ActorSpawned`, except the hero) gets a proxy entity: a
//! kinematic avian capsule on the `Body` layer (hands and props collide with it) and a
//! [`HitzoneSet`] in a standing pose. Every fixed step the proxy follows the core's position
//! and facing (`Core::sim.world.actors`, centimetres and sim yaw). A VR melee hit
//! (`HitMessage`) by the player on a proxy becomes `Command::Attack { target }`: the core
//! applies its reach, strike gap and the original damage rules.
//!
//! The proxy is kinematic: VR hands can push against it but cannot shove the core's actor.
//! Characters that need physical reactions use `gothic-vr-npc`'s active ragdoll as their
//! body and report their position back to the core instead.
use crate::units::{pos_to_m, sim_yaw_to_bevy};
use avian3d::prelude::*;
use bevy::prelude::*;
use gothic_runtime::{Command, CoreCommand, CoreEvent, CoreRes, Event};
use gothic_sim::types::ActorId;
use gothic_vr_melee::{Bone, HitMessage, Hitzone, HitzoneSet, MeleeSet, PlayerAvatar};
use gothic_vr_physics::VrLayer;
use std::collections::BTreeMap;

/// The core actor a proxy stands for.
#[derive(Component, Debug, Clone, Copy, PartialEq, Eq)]
pub struct RuntimeActor(pub ActorId);

/// Actor -> proxy entity.
#[derive(Resource, Debug, Default, Clone)]
pub struct ActorProxies(pub BTreeMap<ActorId, Entity>);

#[derive(Resource, Debug, Clone, Copy)]
pub struct BridgeSettings {
    /// Hits below this quality (scratches, slow touches) are not sent to the core.
    pub min_quality: f32,
    /// Standing height of the proxies, m.
    pub height: f32,
}

impl Default for BridgeSettings {
    fn default() -> Self {
        Self {
            min_quality: 0.3,
            height: 1.8,
        }
    }
}

/// Hitzones of a standing humanoid `height` metres tall at `feet`, facing Bevy `yaw`.
pub fn standing_zones(feet: Vec3, yaw: f32, height: f32) -> Vec<Hitzone> {
    let k = height / 1.8;
    let rot = Quat::from_rotation_y(yaw);
    let p = |x: f32, y: f32| feet + rot * Vec3::new(x * k, y * k, 0.0);
    vec![
        Hitzone::between(Bone::Head, p(0.0, 1.58), p(0.0, 1.76), 0.11 * k),
        Hitzone::between(Bone::Chest, p(0.0, 1.15), p(0.0, 1.45), 0.18 * k),
        Hitzone::between(Bone::Abdomen, p(0.0, 0.92), p(0.0, 1.15), 0.16 * k),
        Hitzone::between(Bone::UpperArmL, p(-0.24, 1.45), p(-0.26, 1.15), 0.06 * k),
        Hitzone::between(Bone::UpperArmR, p(0.24, 1.45), p(0.26, 1.15), 0.06 * k),
        Hitzone::between(Bone::ThighL, p(-0.1, 0.9), p(-0.1, 0.5), 0.08 * k),
        Hitzone::between(Bone::ThighR, p(0.1, 0.9), p(0.1, 0.5), 0.08 * k),
        Hitzone::between(Bone::ShinL, p(-0.1, 0.5), p(-0.1, 0.08), 0.06 * k),
        Hitzone::between(Bone::ShinR, p(0.1, 0.5), p(0.1, 0.08), 0.06 * k),
    ]
}

pub(crate) struct ActorsPlugin;

impl Plugin for ActorsPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<ActorProxies>()
            .init_resource::<BridgeSettings>()
            .add_message::<CoreCommand>()
            .add_message::<CoreEvent>()
            .add_systems(Update, spawn_proxies)
            .add_systems(
                FixedUpdate,
                follow_core
                    .run_if(resource_exists::<CoreRes>)
                    .before(MeleeSet),
            )
            .add_systems(FixedUpdate, hits_to_core.after(MeleeSet));
    }
}

fn spawn_proxies(
    mut commands: Commands,
    mut proxies: ResMut<ActorProxies>,
    settings: Res<BridgeSettings>,
    core: Option<Res<CoreRes>>,
    mut events: MessageReader<CoreEvent>,
) {
    for e in events.read() {
        let Event::ActorSpawned { actor, name } = &e.0 else {
            continue;
        };
        let Some(core) = core.as_ref() else {
            continue;
        };
        if core.0.hero == *actor || proxies.0.contains_key(actor) {
            continue;
        }
        let (feet, yaw) = core
            .0
            .sim
            .world
            .actors
            .get(actor)
            .map_or((Vec3::ZERO, 0.0), |(p, y)| {
                (pos_to_m(*p), sim_yaw_to_bevy(*y))
            });
        let h = settings.height;
        let e = commands
            .spawn((
                RuntimeActor(*actor),
                HitzoneSet::new(standing_zones(feet, yaw, h)),
                RigidBody::Kinematic,
                Collider::capsule(0.25, h - 0.5),
                CollisionLayers::new(
                    VrLayer::Body,
                    [VrLayer::World, VrLayer::Hand, VrLayer::Prop],
                ),
                Transform::from_translation(feet + Vec3::Y * h * 0.5)
                    .with_rotation(Quat::from_rotation_y(yaw)),
                Name::new(format!("Actor {name}")),
            ))
            .id();
        proxies.0.insert(*actor, e);
    }
}

fn follow_core(
    mut commands: Commands,
    core: Res<CoreRes>,
    settings: Res<BridgeSettings>,
    mut proxies: ResMut<ActorProxies>,
    mut q: Query<(&mut Transform, &mut HitzoneSet), With<RuntimeActor>>,
) {
    let actors = &core.0.sim.world.actors;
    let h = settings.height;
    proxies.0.retain(|id, e| {
        let Some((p, y)) = actors.get(id) else {
            commands.entity(*e).despawn();
            return false;
        };
        if let Ok((mut t, mut zones)) = q.get_mut(*e) {
            let feet = pos_to_m(*p);
            let yaw = sim_yaw_to_bevy(*y);
            t.translation = feet + Vec3::Y * h * 0.5;
            t.rotation = Quat::from_rotation_y(yaw);
            zones.zones = standing_zones(feet, yaw, h);
        }
        true
    });
}

fn hits_to_core(
    settings: Res<BridgeSettings>,
    mut hits: MessageReader<HitMessage>,
    actors: Query<&RuntimeActor>,
    player: Query<(), With<PlayerAvatar>>,
    mut out: MessageWriter<CoreCommand>,
) {
    for h in hits.read() {
        let by_player = h.attacker.is_some_and(|a| player.get(a).is_ok());
        if !by_player || h.quality < settings.min_quality {
            continue;
        }
        if let Ok(RuntimeActor(target)) = actors.get(h.victim) {
            out.write(CoreCommand(Command::Attack { target: *target }));
        }
    }
}
