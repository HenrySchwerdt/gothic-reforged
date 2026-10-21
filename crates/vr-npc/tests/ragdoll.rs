//! Active ragdoll: balance, stagger, fall, get-up, hitzones, pushes and grabs.
use glam::Vec3;
use gothic_vr::body::{BodyPose, Joint};
use gothic_vr_melee::{Bone, hitzones_from_body};
use gothic_vr_npc::animator::NpcAnimator;
use gothic_vr_npc::ragdoll::{ActiveRagdoll, GrabHold, Stance};

const DT: f32 = 1.0 / 90.0;
const FLOOR: fn(f32, f32) -> f32 = |_, _| 0.0;

fn standing() -> (ActiveRagdoll, BodyPose) {
    let pose = NpcAnimator::standing(Vec3::ZERO, 0.0);
    (ActiveRagdoll::new(&pose), pose)
}

fn run(r: &mut ActiveRagdoll, target: &BodyPose, seconds: f32) -> Vec<Stance> {
    (0..(seconds / DT) as usize)
        .map(|_| r.step(target, FLOOR, DT))
        .collect()
}

#[test]
fn stands_still_with_muscles() {
    let (mut r, pose) = standing();
    let head0 = r.position(Joint::Head);
    let stances = run(&mut r, &pose, 5.0);
    assert!(stances.iter().all(|s| *s == Stance::Standing));
    assert!(
        r.position(Joint::Head).distance(head0) < 0.05,
        "{}",
        r.position(Joint::Head)
    );
    assert!(r.uprightness(0.0) > 0.95);
}

#[test]
fn collapses_without_muscles() {
    let (mut r, pose) = standing();
    r.set_stance(Stance::Fallen);
    r.settings.lie_time = 100.0;
    run(&mut r, &pose, 3.0);
    assert!(
        r.position(Joint::Pelvis).y < 0.3,
        "{}",
        r.position(Joint::Pelvis)
    );
    assert!(r.position(Joint::Head).y < 0.4);
    // Bones keep their length (within solver tolerance).
    let d = r.position(Joint::KneeL).distance(r.position(Joint::AnkleL));
    let rest = pose.get(Joint::KneeL).distance(pose.get(Joint::AnkleL));
    assert!((d - rest).abs() < 0.02, "{d} vs {rest}");
    for j in Joint::ALL {
        assert!(r.position(j).y >= -0.01, "{j:?} under the floor");
    }
}

#[test]
fn light_hit_staggers_then_recovers() {
    let (mut r, pose) = standing();
    run(&mut r, &pose, 0.5);
    let chest = r.position(Joint::Chest);
    r.apply_impulse(chest, Vec3::new(0.0, 0.0, 25.0), DT);
    let stances = run(&mut r, &pose, 3.0);
    assert!(stances.contains(&Stance::Stagger), "{stances:?}");
    assert!(!stances.contains(&Stance::Fallen));
    assert_eq!(*stances.last().unwrap(), Stance::Standing);
}

#[test]
fn heavy_hit_knocks_down_then_gets_up() {
    let (mut r, pose) = standing();
    run(&mut r, &pose, 0.3);
    let chest = r.position(Joint::Chest);
    r.apply_impulse(chest, Vec3::new(0.0, 5.0, 90.0), DT);
    let stances = run(&mut r, &pose, 1.5);
    assert!(stances.contains(&Stance::Fallen));
    assert!(
        r.uprightness(0.0) < 0.6,
        "on the ground: {}",
        r.uprightness(0.0)
    );
    let stances = run(&mut r, &pose, 6.0);
    assert!(stances.contains(&Stance::GettingUp), "{:?}", stances.last());
    assert_eq!(*stances.last().unwrap(), Stance::Standing);
    assert!(r.uprightness(0.0) > 0.85, "{}", r.uprightness(0.0));
}

#[test]
fn hand_pushes_and_grabs() {
    let (mut r, pose) = standing();
    run(&mut r, &pose, 0.3);
    // A hand moving 3 m/s into the chest from the front pushes it back.
    let chest = r.position(Joint::Chest);
    let before = chest.z;
    let mut touched = false;
    for i in 0..10 {
        let c = chest + Vec3::new(0.0, 0.0, -0.25 + 0.035 * i as f32);
        touched |= r.push(c, 0.045, Vec3::new(0.0, 0.0, 3.0), DT);
        r.step(&pose, FLOOR, DT);
    }
    assert!(touched);
    assert!(r.position(Joint::Chest).z > before + 0.02);
    // Grab the right hand and pull it up and out.
    run(&mut r, &pose, 2.0);
    let target = r.position(Joint::HandR) + Vec3::new(0.3, 0.3, 0.0);
    r.holds.push(GrabHold {
        joint: Joint::HandR,
        target,
        stiffness: 0.5,
    });
    run(&mut r, &pose, 0.5);
    assert!(r.position(Joint::HandR).distance(target) < 0.08);
    r.holds.clear();
}

#[test]
fn hitzones_follow_the_body() {
    let (r, _) = standing();
    let zones = hitzones_from_body(&r.pose());
    let head = zones.iter().find(|z| z.bone == Bone::Head).unwrap();
    assert!(head.pose.position.y > 1.5);
    let shin = zones.iter().find(|z| z.bone == Bone::ShinL).unwrap();
    assert!(shin.pose.position.y < 0.6);
    assert_eq!(zones.len(), 11);
}

#[test]
fn deterministic() {
    let go = || {
        let (mut r, pose) = standing();
        r.apply_impulse(r.position(Joint::Head), Vec3::new(20.0, 0.0, 10.0), DT);
        run(&mut r, &pose, 2.0);
        format!("{:?}", r.pose().joints)
    };
    assert_eq!(go(), go());
}
