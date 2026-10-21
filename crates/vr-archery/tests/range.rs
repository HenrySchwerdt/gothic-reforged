//! Greybox archery range end to end with physics hands.
use bevy::prelude::*;
use gothic_vr::{Hand, Pose};
use gothic_vr_archery::ballistics::StuckIn;
use gothic_vr_archery::{ArrowFiredMessage, ArrowHitMessage, FlyingArrow, VrArcheryPlugin};
use gothic_vr_melee::{DamageType, HitMessage, PlayerHitzonesPlugin, VrMeleePlugin, spawn_dummy};
use gothic_vr_physics::testing::Rig;
use gothic_vr_physics::{props, spawn_prop, static_box};

#[derive(Resource, Default)]
struct Log {
    fired: Vec<ArrowFiredMessage>,
    hits: Vec<ArrowHitMessage>,
    melee: Vec<HitMessage>,
}

fn collect(
    mut log: ResMut<Log>,
    mut f: MessageReader<ArrowFiredMessage>,
    mut h: MessageReader<ArrowHitMessage>,
    mut m: MessageReader<HitMessage>,
) {
    log.fired.extend(f.read().copied());
    log.hits.extend(h.read().copied());
    log.melee.extend(m.read().copied());
}

const BOW_HAND: Vec3 = Vec3::new(-0.05, 1.55, -0.6);

fn range(target_z: f32) -> (Rig, Entity) {
    let mut rig = Rig::with_plugins(
        |c| {
            c.spawn(static_box(
                Vec3::new(0.0, -0.5, -20.0),
                Vec3::new(20.0, 0.5, 40.0),
                Quat::IDENTITY,
            ));
        },
        |app| {
            app.add_plugins((VrMeleePlugin, PlayerHitzonesPlugin, VrArcheryPlugin))
                .init_resource::<Log>()
                .add_systems(Update, collect);
        },
    );
    let dummy = rig.spawn(|c| spawn_dummy(c, Vec3::new(BOW_HAND.x, 0.0, target_z)));
    // Bow appears in the left hand, which closes on it.
    let bow_pose = Pose::new(BOW_HAND, Quat::IDENTITY);
    rig.set_hand(Hand::Left, bow_pose);
    rig.set_hand(
        Hand::Right,
        Pose::new(Vec3::new(0.3, 1.2, -0.3), Quat::IDENTITY),
    );
    rig.step(40);
    rig.spawn(|c| spawn_prop(c, &props::bow(), bow_pose));
    rig.grip(Hand::Left, true);
    rig.step(30);
    (rig, dummy)
}

/// Quiver -> nock -> draw `draw` m -> release.
fn shoot(rig: &mut Rig, draw: f32) {
    let quiver = Vec3::new(0.12, 1.65, 0.18);
    rig.set_hand(Hand::Right, Pose::new(quiver, Quat::IDENTITY));
    rig.step(45);
    rig.grip(Hand::Right, true);
    rig.step(3);
    let rest = BOW_HAND + props::bow_string_rest();
    rig.set_hand(Hand::Right, Pose::new(rest, Quat::IDENTITY));
    rig.step(45);
    rig.set_hand(
        Hand::Right,
        Pose::new(rest + Vec3::Z * draw, Quat::IDENTITY),
    );
    rig.step(45);
    rig.grip(Hand::Right, false);
    rig.step(1);
}

#[test]
fn full_draw_hits_the_ten_metre_target() {
    let (mut rig, dummy) = range(-10.6);
    shoot(&mut rig, 0.6);
    rig.step(60);
    let log = rig.app.world().resource::<Log>();
    assert_eq!(log.fired.len(), 1, "fired");
    assert!(log.fired[0].draw_factor > 0.9, "{:?}", log.fired);
    assert_eq!(log.hits.len(), 1, "{:?}", log.melee);
    let h = log.hits[0];
    assert_eq!(h.victim, dummy);
    assert!(
        (h.damage - 30.0 * h.draw_factor).abs() < 1e-3,
        "hook damage {h:?}"
    );
    assert!(
        log.melee
            .iter()
            .any(|m| m.victim == dummy && m.damage_type == DamageType::Point)
    );
    let mut q = rig.app.world_mut().query::<&FlyingArrow>();
    let a = q.iter(rig.app.world()).next().unwrap();
    assert!(matches!(a.arrow.stuck, Some(StuckIn::Body { .. })));
}

#[test]
fn half_draw_falls_short_into_the_ground() {
    let (mut rig, _) = range(-25.0);
    shoot(&mut rig, 0.3);
    rig.step(180);
    let log = rig.app.world().resource::<Log>();
    assert_eq!(log.fired.len(), 1);
    assert!(
        (log.fired[0].draw_factor - 0.5).abs() < 0.08,
        "{:?}",
        log.fired
    );
    assert!(log.hits.is_empty(), "{:?}", log.hits);
    let mut q = rig.app.world_mut().query::<(&FlyingArrow, &Transform)>();
    let (a, t) = q.iter(rig.app.world()).next().unwrap();
    assert!(
        matches!(a.arrow.stuck, Some(StuckIn::World { .. })),
        "{:?}",
        a.arrow
    );
    assert!(
        t.translation.z > -25.0 && t.translation.z < -5.0,
        "{}",
        t.translation
    );
}
