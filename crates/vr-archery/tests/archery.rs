//! Bow handling, draw power, trajectories, sticking, damage hook, aim assist.
use glam::{Quat, Vec3};
use gothic_vr::{Hand, Pose};
use gothic_vr_archery::ballistics::*;
use gothic_vr_archery::bow::*;
use gothic_vr_melee::{Bone, Hitzone};

const DT: f32 = 1.0 / 90.0;

/// Head at 1.7 m looking down -Z; bow in the left hand in front, aiming along -Z.
fn input(right: Vec3, grip: bool) -> ArcherInput {
    let head = Pose::new(Vec3::new(0.0, 1.7, 0.0), Quat::IDENTITY);
    let bow = Pose::new(Vec3::new(-0.05, 1.55, -0.65), Quat::IDENTITY);
    ArcherInput {
        head,
        hands: [Some(bow), Some(Pose::new(right, Quat::IDENTITY))],
        grip: [true, grip],
        bow: Some((bow, Hand::Left)),
        busy: [true, false],
    }
}

fn quiver_spot() -> Vec3 {
    Quiver::default().center(
        &Pose::new(Vec3::new(0.0, 1.7, 0.0), Quat::IDENTITY),
        Hand::Right,
    )
}

/// Take, nock and draw to `draw` m; returns the archer ready to release.
fn drawn(draw: f32) -> (Archer, ArcherInput) {
    let mut a = Archer::default();
    let spot = quiver_spot();
    assert!(
        a.update(&input(Vec3::new(0.3, 1.2, -0.3), false))
            .is_empty()
    );
    let ev = a.update(&input(spot, true));
    assert_eq!(ev, vec![ArcherEvent::TookArrow(Hand::Right)]);
    let i = input(Vec3::ZERO, true);
    let rest = i.bow.unwrap().0.transform_point(a.bow.string_rest);
    let ev = a.update(&input(rest + Vec3::new(0.02, 0.01, 0.0), true));
    assert_eq!(ev, vec![ArcherEvent::Nocked]);
    let hand = rest + Vec3::Z * draw;
    let ev = a.update(&input(hand, true));
    assert!(ev.is_empty());
    (a, input(hand, true))
}

#[test]
fn quiver_is_over_the_shoulder_only() {
    let mut a = Archer::default();
    // Gripping in front of the chest gives nothing.
    a.update(&input(Vec3::new(0.2, 1.3, -0.3), false));
    assert!(a.update(&input(Vec3::new(0.2, 1.3, -0.3), true)).is_empty());
    assert_eq!(a.state, ArcherState::Idle);
    // Behind the right shoulder works.
    a.update(&input(quiver_spot(), false));
    assert_eq!(
        a.update(&input(quiver_spot(), true)),
        vec![ArcherEvent::TookArrow(Hand::Right)]
    );
    // Letting go away from the bow drops it.
    let ev = a.update(&input(Vec3::new(0.3, 1.3, -0.2), false));
    assert!(matches!(ev.as_slice(), [ArcherEvent::Dropped { .. }]));
}

#[test]
fn nocking_needs_the_string() {
    let mut a = Archer::default();
    a.update(&input(quiver_spot(), false));
    a.update(&input(quiver_spot(), true));
    // Far from the string: still just holding.
    assert!(a.update(&input(Vec3::new(0.4, 1.5, -0.2), true)).is_empty());
    assert_eq!(a.state, ArcherState::Holding(Hand::Right));
}

#[test]
fn draw_length_is_power() {
    let (mut a, i) = drawn(0.3);
    assert!(matches!(a.state, ArcherState::Nocked { draw, .. } if (draw - 0.3).abs() < 1e-4));
    let pose = a.arrow_pose(&i).unwrap();
    assert!(pose.forward().z < -0.99, "arrow points down range");
    let mut rel = i;
    rel.grip[1] = false;
    let ev = a.update(&rel);
    let [
        ArcherEvent::Fired {
            velocity,
            draw_factor,
            ..
        },
    ] = ev.as_slice()
    else {
        panic!("{ev:?}")
    };
    assert!((draw_factor - 0.5).abs() < 1e-3);
    assert!((velocity.length() - a.bow.max_speed * 0.5).abs() < 0.01);
    assert!(velocity.normalize().z < -0.99);

    // Over-drawing clamps to full power.
    let (mut a, mut i) = drawn(0.9);
    i.grip[1] = false;
    let ev = a.update(&i);
    assert!(
        matches!(ev.as_slice(), [ArcherEvent::Fired { draw_factor, .. }] if (*draw_factor - 1.0).abs() < 1e-6)
    );
}

#[test]
fn tiny_draw_drops_and_off_axis_loses_the_arrow() {
    let (mut a, mut i) = drawn(0.03);
    i.grip[1] = false;
    assert!(matches!(
        a.update(&i).as_slice(),
        [ArcherEvent::Dropped { .. }]
    ));
    let (mut a, i) = drawn(0.3);
    let mut off = i;
    let p = off.hands[1].unwrap().position + Vec3::X * 0.3;
    off.hands[1] = Some(Pose::new(p, Quat::IDENTITY));
    assert!(matches!(
        a.update(&off).as_slice(),
        [ArcherEvent::Dropped { .. }]
    ));
}

#[test]
fn damage_hook() {
    assert_eq!(LinearDamage.damage(10.0, 20.0, 0.5), 15.0);
    assert_eq!(LinearDamage.damage(10.0, 20.0, 2.0), 30.0);
}

fn fly(
    arrow: &mut Arrow,
    world: &dyn RayWorld,
    bodies: &[(u64, &[Hitzone])],
    seconds: f32,
) -> Option<FlightEvent> {
    for _ in 0..(seconds / DT) as usize {
        if let Some(e) = arrow.step(world, bodies, DT) {
            return Some(e);
        }
    }
    None
}

#[test]
fn trajectory_matches_ballistics_and_drag_shortens_it() {
    let spec = ArrowSpec {
        drag: 0.0,
        ..ArrowSpec::default()
    };
    let v = 30.0;
    let dir = Vec3::new(0.0, 1.0, -1.0).normalize();
    let floor = BoxWorld {
        floor: Some(0.0),
        boxes: vec![],
    };
    // Launch the tip from ground level.
    let mut a = Arrow::new(
        spec,
        Vec3::new(0.0, 0.0, 0.0) - dir * spec.half_length + Vec3::Y * 0.001,
        dir * v,
        None,
    );
    let ev = fly(&mut a, &floor, &[], 10.0);
    let Some(FlightEvent::HitWorld { point, .. }) = ev else {
        panic!("{ev:?}")
    };
    let range = -point.z;
    let ideal = v * v / 9.81;
    assert!((range - ideal).abs() / ideal < 0.02, "{range} vs {ideal}");

    let mut d = Arrow::new(
        ArrowSpec::default(),
        a.pos * 0.0 - dir * spec.half_length + Vec3::Y * 0.001,
        dir * v,
        None,
    );
    let Some(FlightEvent::HitWorld { point, .. }) = fly(&mut d, &floor, &[], 10.0) else {
        panic!()
    };
    assert!(
        -point.z < range * 0.97 && -point.z > range * 0.6,
        "drag range {}",
        -point.z
    );
}

#[test]
fn arrow_points_along_its_flight() {
    let mut a = Arrow::new(
        ArrowSpec::default(),
        Vec3::new(0.0, 1.5, 0.0),
        Vec3::new(0.0, 5.0, -30.0),
        None,
    );
    let up = a.rotation() * Vec3::NEG_Z;
    assert!(up.y > 0.0);
    fly(&mut a, &NoWorld, &[], 1.5);
    let down = a.rotation() * Vec3::NEG_Z;
    assert!(down.y < -0.2, "nose down on descent: {down}");
}

#[test]
fn sticks_in_walls_and_glances_off_at_flat_angles() {
    let wall = BoxWorld {
        floor: None,
        boxes: vec![(Vec3::new(-2.0, 0.0, -20.5), Vec3::new(2.0, 3.0, -20.0))],
    };
    let mut a = Arrow::new(
        ArrowSpec::default(),
        Vec3::new(0.0, 1.5, 0.0),
        Vec3::new(0.0, 0.0, -50.0),
        None,
    );
    let ev = fly(&mut a, &wall, &[], 2.0);
    assert!(matches!(ev, Some(FlightEvent::HitWorld { .. })), "{ev:?}");
    let Some(StuckIn::World { pose }) = a.stuck else {
        panic!()
    };
    let tip = pose.transform_point(Vec3::new(0.0, 0.0, -a.spec.half_length));
    assert!(tip.z < -20.0 && tip.z > -20.2, "embedded a little: {tip}");
    // It stays.
    let before = a.pos;
    fly(&mut a, &wall, &[], 1.0);
    assert_eq!(a.pos, before);

    // Skimming the wall at a flat angle ricochets.
    let side = BoxWorld {
        floor: None,
        boxes: vec![(Vec3::new(0.5, 0.0, -30.0), Vec3::new(1.0, 3.0, 0.0))],
    };
    let mut g = Arrow::new(
        ArrowSpec::default(),
        Vec3::new(0.0, 1.5, 0.0),
        Vec3::new(5.0, 0.0, -50.0),
        None,
    );
    let ev = fly(&mut g, &side, &[], 1.0);
    assert!(matches!(ev, Some(FlightEvent::Ricochet { .. })), "{ev:?}");
    assert!(g.stuck.is_none() && g.vel.x < 0.0);
}

fn target_at(z: f32, x: f32) -> Vec<Hitzone> {
    vec![
        Hitzone::between(
            Bone::Chest,
            Vec3::new(x, 1.1, z),
            Vec3::new(x, 1.45, z),
            0.19,
        ),
        Hitzone::between(
            Bone::Head,
            Vec3::new(x, 1.55, z),
            Vec3::new(x, 1.75, z),
            0.11,
        ),
    ]
}

#[test]
fn hits_bodies_and_sticks_with_them() {
    let zones = target_at(-15.0, 0.0);
    let mut a = Arrow::new(
        ArrowSpec::default(),
        Vec3::new(0.0, 1.35, 0.0),
        Vec3::new(0.0, 0.3, -50.0),
        Some(1),
    );
    let ev = fly(&mut a, &NoWorld, &[(7, &zones)], 2.0);
    let Some(FlightEvent::HitBody {
        victim,
        zone,
        speed,
        point,
        ..
    }) = ev
    else {
        panic!("{ev:?}")
    };
    assert_eq!((victim, zone), (7, 0));
    assert!(speed > 40.0);
    // On the capsule's surface (the arrow dropped ~0.45 m over 15 m and hit low).
    let (a0, a1) = zones[0].segment();
    let (_, c, _, _) = gothic_vr_melee::geom::segment_segment(point, point, a0, a1);
    assert!((point.distance(c) - 0.19).abs() < 0.03, "{point}");
    assert!(point.y < 1.2, "gravity drop: {point}");
    // The target walks away: the arrow goes along.
    let mut moved = zones.clone();
    for z in &mut moved {
        z.pose.position += Vec3::new(1.0, 0.0, 0.0);
    }
    let p = a.pose(|_, i| moved.get(i).copied());
    assert!((p.position.x - 1.0).abs() < 0.05);
    // The shooter's own body is ignored.
    let mut b = Arrow::new(
        ArrowSpec::default(),
        Vec3::new(0.0, 1.35, 0.0),
        Vec3::new(0.0, 0.3, -50.0),
        Some(7),
    );
    assert!(fly(&mut b, &NoWorld, &[(7, &zones)], 0.5).is_none());
}

#[test]
fn fast_arrows_cannot_pass_through_thin_targets() {
    let zones = vec![Hitzone::between(
        Bone::HandL,
        Vec3::new(0.0, 1.0, -10.0),
        Vec3::new(0.0, 1.7, -10.0),
        0.03,
    )];
    let mut a = Arrow::new(
        ArrowSpec::default(),
        Vec3::new(0.0, 1.3, 0.0),
        Vec3::new(0.0, 0.25, -90.0),
        None,
    );
    assert!(matches!(
        fly(&mut a, &NoWorld, &[(1, &zones)], 1.0),
        Some(FlightEvent::HitBody { .. })
    ));
}

#[test]
fn aim_assist_bends_toward_a_target_in_the_cone() {
    let origin = Vec3::new(0.0, 1.5, 0.0);
    // Target 20 m out, 0.8 m to the side: a straight shot misses.
    let zones = target_at(-20.0, 0.8);
    let center = Vec3::new(0.8, 1.3, -20.0);
    let v = Vec3::new(0.0, 0.08, -1.0).normalize() * 50.0;
    let mut miss = Arrow::new(ArrowSpec::default(), origin, v, None);
    assert!(!matches!(
        fly(&mut miss, &NoWorld, &[(3, &zones)], 1.5),
        Some(FlightEvent::HitBody { .. })
    ));
    let (assisted, id) = aim_assist(origin, v, &[(3, center)], 5.0, 1.0);
    assert_eq!(id, Some(3));
    let mut hit = Arrow::new(ArrowSpec::default(), origin, assisted, None);
    assert!(matches!(
        fly(&mut hit, &NoWorld, &[(3, &zones)], 1.5),
        Some(FlightEvent::HitBody { victim: 3, .. })
    ));
    // Outside the cone or with zero strength nothing changes.
    assert_eq!(
        aim_assist(origin, v, &[(3, Vec3::new(10.0, 1.3, -20.0))], 5.0, 1.0),
        (v, None)
    );
    assert_eq!(aim_assist(origin, v, &[(3, center)], 5.0, 0.0), (v, None));
}

#[test]
fn deterministic() {
    let go = || {
        let zones = target_at(-25.0, 0.3);
        let mut a = Arrow::new(
            ArrowSpec::default(),
            Vec3::new(0.0, 1.5, 0.0),
            Vec3::new(0.4, 2.0, -45.0),
            None,
        );
        format!(
            "{:?} {:?}",
            fly(&mut a, &NoWorld, &[(1, &zones)], 3.0),
            a.pos
        )
    };
    assert_eq!(go(), go());
}
