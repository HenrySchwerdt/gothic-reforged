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

/// Largest angle (rad) between the held sword and where the tracked hand says it should be,
/// while the wrist twists back and forth at `rate` rad/s about the blade and across it.
fn twist_error(rate: f32) -> f32 {
    let (mut rig, sword) = hand_over(props::sword());
    rig.grip(Hand::Right, true);
    let lift = Vec3::new(0.2, 1.4, -0.4);
    rig.set_hand(Hand::Right, Pose::new(lift, LYING.rotation));
    rig.step(90);
    // The grip as held now (sword relative to the tracked hand).
    let rel = LYING.rotation.inverse() * rig.transform(sword).rotation;
    let mut worst: f32 = 0.0;
    for i in 0..180 {
        let t = i as f32 / common::HZ as f32;
        // 1 Hz back and forth, peak angular speed `rate`.
        let w = std::f32::consts::TAU;
        let a = rate / w * (w * t).sin();
        let turn = Quat::from_rotation_z(a) * Quat::from_rotation_x(0.5 * a);
        let target = turn * LYING.rotation;
        rig.set_hand(Hand::Right, Pose::new(lift, target));
        rig.step(1);
        if i > 20 {
            let want = target * rel;
            worst = worst.max(rig.transform(sword).rotation.angle_between(want));
        }
    }
    worst
}

#[test]
fn held_sword_follows_fast_wrist_turns() {
    let slow = twist_error(2.0);
    let fast = twist_error(8.0);
    println!(
        "wrist twist error: 2 rad/s {:.1} deg, 8 rad/s {:.1} deg",
        slow.to_degrees(),
        fast.to_degrees()
    );
    assert!(slow.to_degrees() < 6.0, "slow twist {}", slow.to_degrees());
    assert!(fast.to_degrees() < 20.0, "fast twist {}", fast.to_degrees());
}

/// Both hands on the two-hander's handle, right hand leading.
fn two_hands_on(rig: &mut Rig, sword: Entity) -> f32 {
    let over = above_handle(rig, sword, 0.15);
    rig.set_hand(Hand::Left, over);
    rig.step(60);
    rig.grip(Hand::Right, true);
    rig.step(2);
    rig.grip(Hand::Left, true);
    rig.step(2);
    let held = rig.app.world().get::<Held>(sword).unwrap().clone();
    held.hands[1].unwrap().distance(held.hands[0].unwrap())
}

fn hand_of(rig: &mut Rig, hand: Hand) -> PhysicsHand {
    let mut q = rig.app.world_mut().query::<&PhysicsHand>();
    q.iter(rig.app.world())
        .find(|h| h.hand == hand)
        .unwrap()
        .clone()
}

#[test]
fn second_hand_aims_the_blade_without_fighting_the_first() {
    let (mut rig, sword) = hand_over(props::two_handed_sword());
    let gap = two_hands_on(&mut rig, sword);
    assert!(!hand_of(&mut rig, Hand::Right).held.unwrap().secondary);
    assert!(hand_of(&mut rig, Hand::Left).held.unwrap().secondary);
    // Tilt the hands' line 35 degrees up and to the side while both controllers keep their
    // rotation unchanged: only the line between the hands may decide where the blade points.
    let lead = Vec3::new(0.15, 1.3, -0.35);
    let line = (Quat::from_rotation_y(0.4) * Quat::from_rotation_x(0.6) * Vec3::Z).normalize();
    rig.set_hand(Hand::Right, Pose::new(lead, LYING.rotation));
    rig.set_hand(Hand::Left, Pose::new(lead + line * gap, Quat::IDENTITY));
    rig.step(150);
    let held = rig.app.world().get::<Held>(sword).unwrap().clone();
    let t = rig.transform(sword);
    let along = (t.rotation * (held.hands[0].unwrap() - held.hands[1].unwrap())).normalize();
    assert!(along.dot(line) > 0.97, "grip line {along} vs hands {line}");
    let spin = rig
        .app
        .world()
        .get::<gothic_vr_physics::avian3d::prelude::AngularVelocity>(sword)
        .unwrap()
        .0;
    assert!(spin.length() < 0.3, "settled, not spinning: {spin}");
    // Letting go with the leading hand: the left hand takes over and keeps holding.
    rig.grip(Hand::Right, false);
    rig.step(2);
    let left = hand_of(&mut rig, Hand::Left).held.unwrap();
    assert!(!left.secondary);
    rig.step(90);
    let grip = rig.transform(sword).transform_point(left.anchor);
    assert!(
        grip.distance(rig.hand_pos(Hand::Left)) < 0.02,
        "still in the left hand"
    );
}
