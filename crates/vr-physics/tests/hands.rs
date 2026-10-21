//! Physics hands on avian3d: PD follow, collision, grabbing, weight, two hands, throwing,
//! snapping and determinism. Fully headless and deterministic (fixed 90 Hz steps).
use bevy::prelude::*;
use gothic_vr::{Hand, Pose};
use gothic_vr_physics::testing::{self as common, Rig};
use gothic_vr_physics::{Held, PhysicsHand, VrStrength, props, spawn_prop, static_box};

fn floor(c: &mut Commands) {
    c.spawn(static_box(
        Vec3::new(0.0, -0.5, 0.0),
        Vec3::new(10.0, 0.5, 10.0),
        Quat::IDENTITY,
    ));
}

fn table(c: &mut Commands) {
    floor(c);
    c.spawn(static_box(
        Vec3::new(0.0, 0.8, -0.6),
        Vec3::new(0.8, 0.1, 0.6),
        Quat::IDENTITY,
    ));
}

/// Grip frame of a weapon lying flat on the table (edge horizontal), grip at (0.2, 1.0, -0.3).
const LYING: Pose = Pose {
    position: Vec3::new(0.2, 1.05, -0.3),
    rotation: Quat::from_xyzw(
        0.0,
        0.0,
        std::f32::consts::FRAC_1_SQRT_2,
        std::f32::consts::FRAC_1_SQRT_2,
    ),
};

/// Hand hovering just above the handle of `prop` (8 cm up, 3 cm + `along` toward the pommel).
fn above_handle(rig: &Rig, prop: Entity, along: f32) -> Pose {
    let t = rig.transform(prop);
    Pose::new(
        t.transform_point(Vec3::new(0.0, 0.0, 0.03 + along)) + Vec3::Y * 0.08,
        t.rotation,
    )
}

/// Rig with `spec` resting on the table and the right hand hovering above its handle.
fn hand_over(spec: props::PropSpec) -> (Rig, Entity) {
    let mut rig = Rig::new(table);
    rig.set_hand(
        Hand::Right,
        Pose::new(Vec3::new(0.2, 1.4, 0.1), LYING.rotation),
    );
    rig.set_hand(
        Hand::Left,
        Pose::new(Vec3::new(-0.4, 1.3, 0.2), Quat::IDENTITY),
    );
    rig.step(30);
    let e = rig.spawn(|c| spawn_prop(c, &spec, LYING));
    rig.step(60);
    let over = above_handle(&rig, e, 0.0);
    rig.set_hand(Hand::Right, over);
    rig.step(40);
    (rig, e)
}

#[test]
fn hand_follows_the_tracked_pose() {
    let mut rig = Rig::new(floor);
    let target = Vec3::new(0.3, 1.4, -0.5);
    rig.set_hand(Hand::Right, Pose::new(target, Quat::from_rotation_y(0.5)));
    rig.step(90);
    assert!(
        rig.hand_pos(Hand::Right).distance(target) < 0.01,
        "{}",
        rig.hand_pos(Hand::Right)
    );
}

#[test]
fn walls_stop_the_physical_hand() {
    let mut rig = Rig::new(|c| {
        floor(c);
        c.spawn(static_box(
            Vec3::new(0.0, 1.5, -0.6),
            Vec3::new(2.0, 1.5, 0.05),
            Quat::IDENTITY,
        ));
    });
    // The tracked hand goes 40 cm into the wall (front face at z = -0.55).
    rig.set_hand(
        Hand::Right,
        Pose::new(Vec3::new(0.2, 1.2, -0.95), Quat::IDENTITY),
    );
    rig.step(20);
    let p = rig.hand_pos(Hand::Right);
    assert!(p.z > -0.55 + 0.04 - 0.01, "hand passed the wall: {p}");
}

#[test]
fn grabbing_snaps_the_grip_into_the_hand_and_carries_it() {
    let (mut rig, sword) = hand_over(props::sword());
    rig.grip(Hand::Right, true);
    rig.step(2);
    assert_eq!(rig.app.world().get::<Held>(sword).unwrap().count(), 1);
    // Lift and turn the blade upright.
    rig.set_hand(
        Hand::Right,
        Pose::new(Vec3::new(0.2, 1.5, -0.4), Quat::from_rotation_x(0.6)),
    );
    rig.step(90);
    let t = rig.transform(sword);
    let hand_p = rig.hand_pos(Hand::Right);
    let anchor = rig.app.world().get::<Held>(sword).unwrap().hands[1].unwrap();
    let grip = t.transform_point(anchor);
    assert!(
        grip.distance(hand_p) < 0.01,
        "grip stays in the hand {grip} {hand_p}"
    );
    assert!(grip.distance(Vec3::new(0.2, 1.5, -0.4)) < 0.03, "{grip}");
    // Blade along the hand's -Z.
    let blade = t.rotation * Vec3::NEG_Z;
    assert!(
        blade.dot(Quat::from_rotation_x(0.6) * Vec3::NEG_Z) > 0.97,
        "{blade}"
    );
    // Release: it falls.
    rig.grip(Hand::Right, false);
    rig.step(60);
    assert_eq!(rig.app.world().get::<Held>(sword).unwrap().count(), 0);
    assert!(rig.transform(sword).translation.y < 1.2);
}

/// Swing a held prop left-right and return the worst distance between hand and target.
fn swing_lag(spec: props::PropSpec, strength: f32) -> f32 {
    let (mut rig, _) = hand_over(spec);
    rig.app.insert_resource(VrStrength(strength));
    rig.grip(Hand::Right, true);
    rig.step(30);
    let mut worst: f32 = 0.0;
    let w = std::f32::consts::TAU * 1.5;
    for i in 0..180 {
        let t = i as f32 / common::HZ as f32;
        let target = Vec3::new(0.6 * (w * t).sin(), 1.3, -0.45);
        rig.set_hand(Hand::Right, Pose::new(target, Quat::IDENTITY));
        rig.step(1);
        if i > 60 {
            worst = worst.max(rig.hand_pos(Hand::Right).distance(target));
        }
    }
    worst
}

#[test]
fn heavy_weapons_lag_strength_helps() {
    let sword = swing_lag(props::sword(), 10.0);
    let heavy = swing_lag(props::two_handed_sword(), 10.0);
    let heavy_strong = swing_lag(props::two_handed_sword(), 150.0);
    println!(
        "swing lag: sword {sword:.3} m, two-hander {heavy:.3} m, two-hander STR 150 {heavy_strong:.3} m"
    );
    assert!(sword < heavy, "sword {sword} vs two-hander {heavy}");
    assert!(heavy_strong < heavy, "{heavy_strong} vs {heavy}");
    assert!(sword < 0.2, "light weapon tracks: {sword}");
}

#[test]
fn two_handed_grip() {
    let (mut rig, sword) = hand_over(props::two_handed_sword());
    // Left hand above the handle further toward the pommel (+Z).
    let over = above_handle(&rig, sword, 0.15);
    rig.set_hand(Hand::Left, over);
    rig.step(60);
    rig.grip(Hand::Right, true);
    rig.step(2);
    rig.grip(Hand::Left, true);
    rig.step(2);
    let held = rig.app.world().get::<Held>(sword).unwrap().clone();
    assert_eq!(held.count(), 2);
    let (r, l) = (held.hands[1].unwrap(), held.hands[0].unwrap());
    assert!(r.distance(l) >= 0.079, "hands apart on the handle: {r} {l}");
    // Lift with both hands.
    rig.set_hand(
        Hand::Right,
        Pose::new(Vec3::new(0.2, 1.4, -0.4), Quat::IDENTITY),
    );
    rig.set_hand(
        Hand::Left,
        Pose::new(Vec3::new(0.2, 1.4, -0.4 + r.distance(l)), Quat::IDENTITY),
    );
    rig.step(120);
    assert!(
        rig.transform(sword).translation.y > 1.3,
        "{}",
        rig.transform(sword).translation
    );
}

#[test]
fn releasing_mid_swing_throws() {
    let mut rig = Rig::new(table);
    rig.set_hand(
        Hand::Right,
        Pose::new(Vec3::new(0.2, 1.2, -0.3), Quat::IDENTITY),
    );
    rig.step(30);
    let crate_e = rig.spawn(|c| {
        spawn_prop(
            c,
            &props::crate_box(Vec3::splat(0.08), 1.0),
            Pose::new(Vec3::new(0.2, 0.99, -0.3), Quat::IDENTITY),
        )
    });
    rig.step(30);
    let start = rig.transform(crate_e).translation;
    rig.set_hand(
        Hand::Right,
        Pose::new(start + Vec3::Y * 0.13, Quat::IDENTITY),
    );
    rig.step(30);
    rig.grip(Hand::Right, true);
    rig.step(2);
    for i in 0..20 {
        rig.set_hand(
            Hand::Right,
            Pose::new(
                start + Vec3::new(0.0, 0.3, -0.06 * i as f32),
                Quat::IDENTITY,
            ),
        );
        rig.step(1);
    }
    rig.grip(Hand::Right, false);
    rig.step(1);
    let v = rig
        .app
        .world()
        .get::<avian3d::prelude::LinearVelocity>(crate_e)
        .unwrap()
        .0;
    assert!(v.z < -2.0, "thrown forward: {v}");
    rig.step(40);
    assert!(rig.transform(crate_e).translation.z < start.z - 1.5);
}

#[test]
fn stuck_hand_snaps_back() {
    let mut rig = Rig::new(|c| {
        floor(c);
        c.spawn(static_box(
            Vec3::new(0.0, 1.5, -0.6),
            Vec3::new(2.0, 1.5, 0.05),
            Quat::IDENTITY,
        ));
    });
    // Target 0.6 m behind the wall: the hand is stuck, then snaps through.
    rig.set_hand(
        Hand::Right,
        Pose::new(Vec3::new(0.2, 1.2, -1.2), Quat::IDENTITY),
    );
    rig.step(10);
    assert!(rig.hand_pos(Hand::Right).z > -0.55);
    rig.step(40);
    assert!(
        rig.hand_pos(Hand::Right)
            .distance(Vec3::new(0.2, 1.2, -1.2))
            < 0.05
    );
}

#[test]
fn simulation_is_deterministic() {
    let run = || {
        let (mut rig, axe) = hand_over(props::axe());
        rig.grip(Hand::Right, true);
        for i in 0..120 {
            let t = i as f32 / 90.0;
            rig.set_hand(
                Hand::Right,
                Pose::new(
                    Vec3::new(0.5 * (t * 7.0).sin(), 1.3, -0.4),
                    Quat::from_rotation_y(t),
                ),
            );
            rig.step(1);
        }
        let t = rig.transform(axe);
        (t.translation, t.rotation, rig.hand_pos(Hand::Right))
    };
    assert_eq!(run(), run());
}

#[test]
fn hands_exist_per_side() {
    let mut rig = Rig::new(floor);
    let mut q = rig.app.world_mut().query::<&PhysicsHand>();
    let sides: Vec<Hand> = q.iter(rig.app.world()).map(|h| h.hand).collect();
    assert!(sides.contains(&Hand::Left) && sides.contains(&Hand::Right));
}
