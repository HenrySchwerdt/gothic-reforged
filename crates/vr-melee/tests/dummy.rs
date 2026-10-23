//! Melee plugin end to end: a physics hand swings/stabs a physics sword into a training dummy.
use bevy::prelude::*;
use gothic_vr::{Hand, HapticOut, Pose};
use gothic_vr_melee::profiles::profile_for;
use gothic_vr_melee::{
    ClashMessage, DamageType, HitMessage, MeleeWeapon, PlayerAvatar, PlayerHitzonesPlugin,
    StabMessage, VrMeleePlugin, spawn_dummy,
};
use gothic_vr_physics::testing::Rig;
use gothic_vr_physics::{PhysicsHand, VrStrength, props, spawn_prop, static_box};

#[derive(Resource, Default)]
struct Log {
    hits: Vec<HitMessage>,
    stabs: Vec<StabMessage>,
    clashes: Vec<ClashMessage>,
}

fn collect(
    mut log: ResMut<Log>,
    mut h: MessageReader<HitMessage>,
    mut s: MessageReader<StabMessage>,
    mut c: MessageReader<ClashMessage>,
) {
    log.hits.extend(h.read().copied());
    log.stabs.extend(s.read().copied());
    log.clashes.extend(c.read().copied());
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

/// World tip of the sword (blade along -Z, tip 0.81 m out).
fn tip(rig: &Rig, sword: Entity) -> Vec3 {
    rig.transform(sword)
        .transform_point(Vec3::new(0.0, 0.0, -0.81))
}

/// How deep (m) the blade reaches into the dummy's chest (axis x = 0, z = -1.1, radius 0.19,
/// y 1.1..1.45): radius minus the closest horizontal distance of any blade point.
fn chest_depth(rig: &Rig, sword: Entity) -> f32 {
    let t = rig.transform(sword);
    (0..=40)
        .map(|i| t.transform_point(Vec3::new(0.0, 0.0, -0.05 - 0.76 * i as f32 / 40.0)))
        .filter(|p| (1.1..=1.45).contains(&p.y))
        .map(|p| 0.19 - Vec2::new(p.x, p.z + 1.1).length())
        .fold(f32::NEG_INFINITY, f32::max)
}

#[test]
fn the_blade_stops_at_the_body() {
    let start = Pose::new(Vec3::new(0.7, 1.25, -0.35), edge_left());
    let (mut rig, sword, _) = rig_with_sword(start);
    // Push slowly through the dummy: the blade is held up at the chest (or slides around it),
    // it never cuts into it.
    let mut deepest = f32::NEG_INFINITY;
    for i in 0..=225 {
        let x = 0.7 - 1.4 * i as f32 / 225.0;
        rig.set_hand(
            Hand::Right,
            Pose::new(Vec3::new(x, 1.25, -0.35), edge_left()),
        );
        rig.step(1);
        deepest = deepest.max(chest_depth(&rig, sword));
    }
    assert!(deepest < 0.03, "blade went {deepest} m into the chest");
    assert!(rig.app.world().resource::<Log>().hits.is_empty());
}

#[test]
fn a_slanted_thrust_glances_off() {
    // 40 degrees off the chest's normal: no stab.
    let turn = Quat::from_rotation_y(0.7);
    let start = Pose::new(Vec3::new(-0.45, 1.25, -0.05), turn);
    let (mut rig, sword, _) = rig_with_sword(start);
    let dir = turn * Vec3::NEG_Z;
    for i in 0..=11 {
        let p = start.position + dir * 0.45 * i as f32 / 11.0;
        rig.set_hand(Hand::Right, Pose::new(p, turn));
        rig.step(1);
    }
    rig.step(20);
    let log = rig.app.world().resource::<Log>();
    assert!(log.stabs.is_empty(), "{:?}", log.stabs);
    // The tip stays outside the chest.
    let t = tip(&rig, sword);
    let r = Vec2::new(t.x, t.z + 1.1).length();
    assert!(r > 0.15, "tip inside the chest: {t}");
}

#[test]
fn a_stuck_blade_passes_the_body_until_it_is_clear() {
    use gothic_vr_physics::VrLayer;
    use gothic_vr_physics::avian3d::prelude::{CollisionLayers, LayerMask};
    let start = Pose::new(Vec3::new(0.0, 1.25, 0.1), Quat::IDENTITY);
    let (mut rig, sword, _) = rig_with_sword(start);
    let hits_bodies = |rig: &Rig| {
        rig.app
            .world()
            .get::<CollisionLayers>(sword)
            .unwrap()
            .filters
            .has_all(LayerMask::from(VrLayer::Body))
    };
    assert!(hits_bodies(&rig));
    for i in 0..=11 {
        let z = 0.1 - 0.45 * i as f32 / 11.0;
        rig.set_hand(
            Hand::Right,
            Pose::new(Vec3::new(0.0, 1.25, z), Quat::IDENTITY),
        );
        rig.step(1);
    }
    rig.step(5);
    assert!(!hits_bodies(&rig), "stuck: passes the body");
    let played = rig.app.world().resource::<HapticOut>().played.clone();
    assert!(
        played
            .iter()
            .any(|p| p.hand == Hand::Right && p.frequency == 80.0),
        "stuck thud: {played:?}"
    );
    for i in 0..=40 {
        let z = -0.35 + 0.6 * i as f32 / 40.0;
        rig.set_hand(
            Hand::Right,
            Pose::new(Vec3::new(0.0, 1.25, z), Quat::IDENTITY),
        );
        rig.step(1);
    }
    rig.step(40);
    assert!(hits_bodies(&rig), "out and clear: collides again");
}

#[test]
fn a_blocked_swing_staggers_the_arm_and_loses_its_speed() {
    let start = Pose::new(Vec3::new(0.7, 1.25, -0.35), edge_left());
    let (mut rig, sword, dummy) = rig_with_sword(start);
    // The dummy holds a sword upright in front of its chest, in the swing's path.
    let block = Pose::new(
        Vec3::new(0.15, 1.25, -0.75),
        Quat::from_rotation_x(std::f32::consts::FRAC_PI_2),
    );
    rig.spawn(|c| {
        let e = spawn_prop(c, &props::sword(), block);
        let mut w = MeleeWeapon::new(profile_for(props::PropKind::Sword).unwrap());
        w.wielder = Some(dummy);
        c.entity(e)
            .insert((gothic_vr_physics::avian3d::prelude::RigidBody::Kinematic, w));
        e
    });
    let mut staggered: f32 = 0.0;
    // Speed (m/s) of a point 40 cm up the blade, per step.
    let mut speeds = Vec::new();
    let mut last = rig
        .transform(sword)
        .transform_point(Vec3::new(0.0, 0.0, -0.4));
    let mut clash_at = None;
    for i in 0..=16 {
        let x = 0.7 - 1.4 * i as f32 / 16.0;
        rig.set_hand(
            Hand::Right,
            Pose::new(Vec3::new(x, 1.25, -0.35), edge_left()),
        );
        rig.step(1);
        let p = rig
            .transform(sword)
            .transform_point(Vec3::new(0.0, 0.0, -0.4));
        speeds.push(p.distance(last) * 90.0);
        last = p;
        if clash_at.is_none() && !rig.app.world().resource::<Log>().clashes.is_empty() {
            clash_at = Some(i);
        }
        let mut q = rig.app.world_mut().query::<&PhysicsHand>();
        staggered = staggered.max(
            q.iter(rig.app.world())
                .find(|h| h.hand == Hand::Right)
                .unwrap()
                .staggered,
        );
    }
    let log = rig.app.world().resource::<Log>();
    let at = clash_at.expect("no clash");
    assert!(log.hits.is_empty(), "the block protects: {:?}", log.hits);
    assert!(staggered > 0.2, "arm staggered {staggered}");
    // The block takes the swing's speed: the blade (now sliding along the block) is at least
    // 40 % slower than just before the clash, although the tracked hand goes on at full speed.
    let before = speeds[..at].iter().cloned().fold(0.0, f32::max);
    let after = speeds[at + 1..at + 4].iter().sum::<f32>() / 3.0;
    assert!(
        after < before * 0.6,
        "blade speed {before:.1} -> {after:.1} m/s ({speeds:.1?})"
    );
    let played = rig.app.world().resource::<HapticOut>().played.clone();
    assert!(
        played.iter().any(|p| p.frequency == 320.0),
        "clash haptic: {played:?}"
    );
}
