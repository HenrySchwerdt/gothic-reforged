//! Host test of the bow handling: drives the archery state machine with scripted hand trajectories (take an arrow at the
//! quiver, nock it at the string, draw, release) and checks the zones are within arm's reach of the player's body.
use gothic_vr::{Hand, Pose};
use gothic_vr_archery::bow::{Archer, ArcherEvent, ArcherInput, ArcherState};
use bevy::prelude::*;

fn main() {
    let mut ok = true;
    let mut check = |name: &str, cond: bool| {
        println!("ARCHERY {name}: {}", if cond { "PASS" } else { "FAIL" });
        ok &= cond;
    };
    let head = Pose::new(Vec3::new(0.0, 1.7, 0.0), Quat::IDENTITY);
    let mut a = Archer::default();
    a.quiver.radius = 0.26;
    a.quiver.offset = Vec3::new(0.12, -0.12, 0.16);
    a.nock_radius = 0.14;
    // Bow in the left hand, held forward; the right hand draws.
    let bow = Pose::new(Vec3::new(-0.25, 1.4, -0.45), Quat::IDENTITY);
    let quiver = a.quiver.center(&head, Hand::Right);
    let string = bow.transform_point(a.bow.string_rest);
    let back = bow.rotation * Vec3::Z;
    let mut events = Vec::new();
    let step = |a: &mut Archer, events: &mut Vec<ArcherEvent>, rh: Vec3, grip: bool| {
        let input = ArcherInput {
            head,
            hands: [Some(bow), Some(Pose::new(rh, Quat::IDENTITY))],
            grip: [true, grip],
            bow: Some((bow, Hand::Left)),
            busy: [true, false],
        };
        let ev = a.update(&input);
        events.extend(ev);
    };
    // Approach the quiver with the grip open, then squeeze inside the zone at the edge of the radius.
    let start = quiver + Vec3::new(0.0, 0.0, -0.6);
    for i in 0..20 {
        step(&mut a, &mut events, start.lerp(quiver + Vec3::new(0.0, 0.0, 0.2), i as f32 / 19.0), false);
    }
    step(&mut a, &mut events, quiver + Vec3::new(0.0, 0.0, 0.2), true);
    check("take an arrow (grip squeezed 20 cm from the quiver opening)", events.iter().any(|e| matches!(e, ArcherEvent::TookArrow(Hand::Right))));
    // Move to the string holding the grip: nocks within 14 cm.
    for i in 0..30 {
        step(&mut a, &mut events, (quiver).lerp(string + Vec3::new(0.0, 0.0, 0.1), i as f32 / 29.0), true);
    }
    check("nock at the string", events.iter().any(|e| matches!(e, ArcherEvent::Nocked)));
    // Draw 0.55 m back.
    for i in 0..40 {
        step(&mut a, &mut events, string + back * (0.55 * i as f32 / 39.0), true);
    }
    let drawn = matches!(a.state, ArcherState::Nocked { draw, .. } if draw > 0.45);
    check("draw length follows the hand (> 0.45 m)", drawn);
    // Release.
    step(&mut a, &mut events, string + back * 0.55, false);
    let fired = events.iter().find_map(|e| if let ArcherEvent::Fired { velocity, draw_factor, .. } = e { Some((*velocity, *draw_factor)) } else { None });
    check("release fires an arrow", fired.is_some());
    if let Some((v, k)) = fired {
        println!("ARCHERY   speed {:.1} m/s, draw factor {:.2}, direction {:?}", v.length(), k, v.normalize());
        check("arrow flies along the bow axis (forward)", v.normalize().dot(Vec3::NEG_Z) > 0.9);
        check("speed scales with draw (> 25 m/s)", v.length() > 25.0);
    }
    // Zones within reach: each shoulder zone must be within 0.65 m of the same-side shoulder.
    let eye = head.position;
    for (label, zone_c, shoulder) in [
        ("bow zone over the left shoulder (left hand)", Vec3::new(-0.15, 0.84 * 1.7, 0.14), eye + Vec3::new(-0.2, -0.22, 0.0)),
        ("bow zone over the right shoulder (right hand)", Vec3::new(0.15, 0.84 * 1.7, 0.14), eye + Vec3::new(0.2, -0.22, 0.0)),
        ("quiver over the right shoulder (right hand)", quiver, eye + Vec3::new(0.2, -0.22, 0.0)),
    ] {
        let d = zone_c.distance(shoulder);
        println!("ARCHERY   {label}: {d:.2} m from the shoulder");
        check(&format!("{label} reachable"), d < 0.5);
    }
    if !ok {
        std::process::exit(1);
    }
}
