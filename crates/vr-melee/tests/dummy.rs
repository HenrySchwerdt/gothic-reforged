//! Melee plugin end to end: a physics hand swings/stabs a physics sword into a training dummy.
use bevy::prelude::*;
use gothic_vr::{Hand, Pose};
use gothic_vr_melee::{
    DamageType, HitMessage, PlayerAvatar, PlayerHitzonesPlugin, StabMessage, VrMeleePlugin,
    spawn_dummy,
};
use gothic_vr_physics::testing::Rig;
use gothic_vr_physics::{VrStrength, props, spawn_prop, static_box};

#[derive(Resource, Default)]
struct Log {
    hits: Vec<HitMessage>,
    stabs: Vec<StabMessage>,
}

fn collect(
    mut log: ResMut<Log>,
    mut h: MessageReader<HitMessage>,
    mut s: MessageReader<StabMessage>,
) {
    log.hits.extend(h.read().copied());
    log.stabs.extend(s.read().copied());
}

/// Edge facing -X (the swing direction), blade forward.
fn edge_left() -> Quat {
    Quat::from_rotation_z(std::f32::consts::FRAC_PI_2)
}

fn rig_with_sword(hand: Pose) -> (Rig, Entity, Entity) {
    let mut rig = Rig::with_plugins(
        |c| {
            c.spawn(static_box(
                Vec3::new(0.0, -0.5, 0.0),
                Vec3::new(10.0, 0.5, 10.0),
                Quat::IDENTITY,
            ));
        },
        |app| {
            app.add_plugins((VrMeleePlugin, PlayerHitzonesPlugin))
                .init_resource::<Log>()
                .add_systems(Update, collect);
        },
    );
    rig.app.insert_resource(VrStrength(100.0));
    let dummy = rig.spawn(|c| spawn_dummy(c, Vec3::new(0.0, 0.0, -1.1)));
    rig.set_hand(Hand::Right, hand);
    rig.step(30);
    let sword = rig.spawn(|c| spawn_prop(c, &props::sword(), hand));
    rig.grip(Hand::Right, true);
    rig.step(30);
    (rig, sword, dummy)
}

fn sweep(rig: &mut Rig, from_x: f32, to_x: f32, seconds: f32) {
    let n = (seconds * 90.0) as usize;
    for i in 0..=n {
        let x = from_x + (to_x - from_x) * i as f32 / n as f32;
        rig.set_hand(
            Hand::Right,
            Pose::new(Vec3::new(x, 1.25, -0.35), edge_left()),
        );
        rig.step(1);
    }
    rig.step(10);
}

#[test]
fn fast_swing_hits_the_dummy_with_the_edge() {
    let start = Pose::new(Vec3::new(0.7, 1.25, -0.35), edge_left());
    let (mut rig, sword, dummy) = rig_with_sword(start);
    sweep(&mut rig, 0.7, -0.7, 0.18);
    let log = rig.app.world().resource::<Log>();
    assert_eq!(log.hits.len(), 1, "{:?}", log.hits);
    let h = log.hits[0];
    assert_eq!((h.weapon, h.victim), (sword, dummy));
    assert_eq!(h.damage_type, DamageType::Edge);
    assert!(h.impact_speed > 2.5, "{}", h.impact_speed);
    let mut q = rig
        .app
        .world_mut()
        .query_filtered::<Entity, With<PlayerAvatar>>();
    let player = q.iter(rig.app.world()).next();
    assert_eq!(h.attacker, player, "the player wields the grabbed sword");
}

#[test]
fn slow_push_through_the_dummy_does_nothing() {
    let start = Pose::new(Vec3::new(0.7, 1.25, -0.35), edge_left());
    let (mut rig, _, _) = rig_with_sword(start);
    sweep(&mut rig, 0.7, -0.7, 2.5);
    assert!(rig.app.world().resource::<Log>().hits.is_empty());
}

#[test]
fn thrust_sticks_and_pulls_out() {
    let start = Pose::new(Vec3::new(0.0, 1.25, 0.1), Quat::IDENTITY);
    let (mut rig, sword, dummy) = rig_with_sword(start);
    // Thrust 45 cm forward in 0.12 s: the tip goes from z -0.71 into the chest (surface -0.91).
    for i in 0..=11 {
        let z = 0.1 - 0.45 * i as f32 / 11.0;
        rig.set_hand(
            Hand::Right,
            Pose::new(Vec3::new(0.0, 1.25, z), Quat::IDENTITY),
        );
        rig.step(1);
    }
    rig.step(20);
    {
        let log = rig.app.world().resource::<Log>();
        assert!(
            log.stabs.iter().any(|s| matches!(s, StabMessage::Stuck { weapon, victim, .. } if *weapon == sword && *victim == dummy)),
            "{:?} {:?}",
            log.stabs,
            log.hits
        );
        assert_eq!(log.hits.len(), 1);
        assert_eq!(log.hits[0].damage_type, DamageType::Point);
    }
    let mut q = rig
        .app
        .world_mut()
        .query::<&avian3d::prelude::PrismaticJoint>();
    assert_eq!(q.iter(rig.app.world()).count(), 1, "stab joint");
    // Pull straight back.
    for i in 0..=40 {
        let z = -0.35 + 0.6 * i as f32 / 40.0;
        rig.set_hand(
            Hand::Right,
            Pose::new(Vec3::new(0.0, 1.25, z), Quat::IDENTITY),
        );
        rig.step(1);
    }
    rig.step(10);
    let log = rig.app.world().resource::<Log>();
    assert!(
        log.stabs
            .iter()
            .any(|s| matches!(s, StabMessage::PulledOut { .. })),
        "{:?}",
        log.stabs
    );
    let mut q = rig
        .app
        .world_mut()
        .query::<&avian3d::prelude::PrismaticJoint>();
    assert_eq!(q.iter(rig.app.world()).count(), 0);
}
