//! Full-body IK against recorded pose fixtures (`tests/fixtures/poses/*.json`).
//!
//! The fixtures are synthetic recordings (head + both grip poses at 72 Hz with sensor jitter),
//! produced by `regenerate_fixtures` below (`cargo test -p gothic-vr --test body_ik --
//! --ignored regenerate`). They are committed so the solver is checked against fixed data
//! rather than against the same code that generated it.
use glam::{Quat, Vec3};
use gothic_vr::body::{BodyIk, BodyPose, Joint, Proportions};
use gothic_vr::input::Pose;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Debug, Serialize, Deserialize)]
struct Recording {
    name: String,
    dt: f32,
    floor: f32,
    frames: Vec<RecFrame>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
struct RecFrame {
    head: Pose,
    hands: [Option<Pose>; 2],
}

fn dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/poses")
}

fn load(name: &str) -> Recording {
    let path = dir().join(format!("{name}.json"));
    let text = std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("{path:?}: {e}"));
    serde_json::from_str(&text).unwrap()
}

/// Runs a recording, checking the per-frame invariants, and returns all poses.
fn play(rec: &Recording) -> Vec<BodyPose> {
    let p = Proportions::for_eye_height(1.7);
    let mut ik = BodyIk::new(p);
    let mut out = Vec::new();
    for (i, f) in rec.frames.iter().enumerate() {
        let pose = ik.solve(&f.head, f.hands, |_, _| rec.floor, rec.dt);
        assert!(pose.is_finite(), "{} frame {i}: non-finite", rec.name);
        let len = |a: Joint, b: Joint| pose.get(a).distance(pose.get(b));
        for (a, b, want) in [
            (Joint::ShoulderL, Joint::ElbowL, p.upper_arm),
            (Joint::ElbowL, Joint::HandL, p.forearm),
            (Joint::ShoulderR, Joint::ElbowR, p.upper_arm),
            (Joint::ElbowR, Joint::HandR, p.forearm),
            (Joint::HipL, Joint::KneeL, p.thigh),
            (Joint::KneeL, Joint::AnkleL, p.shin),
            (Joint::HipR, Joint::KneeR, p.thigh),
            (Joint::KneeR, Joint::AnkleR, p.shin),
            (Joint::Pelvis, Joint::Neck, p.torso),
        ] {
            assert!(
                (len(a, b) - want).abs() < 2e-3,
                "{} frame {i}: {a:?}-{b:?} {} != {want}",
                rec.name,
                len(a, b)
            );
        }
        for (k, hj) in [Joint::HandL, Joint::HandR].into_iter().enumerate() {
            if let Some(h) = f.hands[k]
                && pose.reached[k]
            {
                assert!(
                    pose.get(hj).distance(h.position) < 2e-3,
                    "{} frame {i}",
                    rec.name
                );
            }
        }
        for a in [Joint::AnkleL, Joint::AnkleR] {
            assert!(
                pose.get(a).y >= rec.floor + p.ankle_height - 0.02,
                "{} frame {i}: foot below floor",
                rec.name
            );
        }
        assert!(
            ik.stepping() != [true, true],
            "{} frame {i}: both feet in the air",
            rec.name
        );
        out.push(pose);
    }
    out
}

#[test]
fn idle_look_around_keeps_hips_and_feet() {
    let rec = load("idle_look_around");
    let poses = play(&rec);
    let yaw0 = poses[0].body_yaw;
    let feet0 = (poses[0].get(Joint::AnkleL), poses[0].get(Joint::AnkleR));
    for p in &poses {
        assert!(
            (p.body_yaw - yaw0).abs() < 0.35,
            "hips twist while only looking"
        );
    }
    let last = poses.last().unwrap();
    assert!(last.get(Joint::AnkleL).distance(feet0.0) < 0.05);
    assert!(last.get(Joint::AnkleR).distance(feet0.1) < 0.05);
}

#[test]
fn turn_in_place_rotates_hips_and_steps() {
    let rec = load("turn_in_place");
    let poses = play(&rec);
    let head_yaw = rec.frames.last().unwrap().head.yaw();
    let last = poses.last().unwrap();
    let err = (last.body_yaw - head_yaw + std::f32::consts::PI).rem_euclid(std::f32::consts::TAU)
        - std::f32::consts::PI;
    assert!(err.abs() < 0.65, "hips follow within the dead band: {err}");
    // Feet re-planted beside the turned hips.
    let side = last.get(Joint::AnkleR) - last.get(Joint::AnkleL);
    let right = Quat::from_rotation_y(last.body_yaw) * Vec3::X;
    assert!(
        side.normalize().dot(right) > 0.7,
        "feet turned with the body"
    );
}

#[test]
fn crouch_bends_knees_without_stepping() {
    let rec = load("crouch");
    let poses = play(&rec);
    let low = poses
        .iter()
        .min_by(|a, b| a.get(Joint::Pelvis).y.total_cmp(&b.get(Joint::Pelvis).y))
        .unwrap();
    let stand = &poses[0];
    assert!(low.get(Joint::Pelvis).y < stand.get(Joint::Pelvis).y - 0.25);
    // Knee pushed forward of the hip-ankle line.
    let fwd = Quat::from_rotation_y(low.body_yaw) * Vec3::NEG_Z;
    let knee = low.get(Joint::KneeL);
    let mid = (low.get(Joint::HipL) + low.get(Joint::AnkleL)) / 2.0;
    assert!((knee - mid).dot(fwd) > 0.1, "knee bends forward");
    assert!(
        low.get(Joint::AnkleL).distance(stand.get(Joint::AnkleL)) < 0.05,
        "feet stay planted"
    );
}

#[test]
fn walking_alternates_feet() {
    let rec = load("walk_forward");
    let p = Proportions::for_eye_height(1.7);
    let mut ik = BodyIk::new(p);
    let mut steps = [0u32; 2];
    let mut was = [false; 2];
    for f in &rec.frames {
        ik.solve(&f.head, f.hands, |_, _| rec.floor, rec.dt);
        let now = ik.stepping();
        for k in 0..2 {
            if now[k] && !was[k] {
                steps[k] += 1;
            }
        }
        was = now;
    }
    assert!(steps[0] >= 3 && steps[1] >= 3, "steps {steps:?}");
    assert!(steps[0].abs_diff(steps[1]) <= 1, "alternating {steps:?}");
    play(&rec);
}

#[test]
fn reach_and_swing_track_the_hands() {
    for name in ["reach", "sword_swing"] {
        let rec = load(name);
        let poses = play(&rec);
        // Every target of the first three seconds is within arm's reach.
        let in_reach = (3.0 / rec.dt) as usize;
        let reached = poses.iter().take(in_reach).filter(|p| p.reached[1]).count();
        let n = poses.len().min(in_reach);
        assert!(
            reached * 100 >= n * 95,
            "{name}: right hand reached {reached}/{n}"
        );
        // Elbows never point up above the shoulder line while the hand is below it.
        for (p, f) in poses.iter().zip(&rec.frames) {
            if let Some(h) = f.hands[1]
                && h.position.y < p.get(Joint::ShoulderR).y - 0.2
            {
                assert!(p.get(Joint::ElbowR).y < p.get(Joint::ShoulderR).y + 0.02);
            }
        }
    }
    // The out-of-reach part of "reach" is reported, not faked.
    let rec = load("reach");
    let poses = play(&rec);
    assert!(poses.iter().any(|p| !p.reached[1]));
}

#[test]
fn untracked_hands_hang() {
    let rec = load("idle_look_around");
    let mut ik = BodyIk::new(Proportions::for_eye_height(1.7));
    let f = rec.frames[0];
    let pose = ik.solve(&f.head, [None, None], |_, _| rec.floor, rec.dt);
    assert!(pose.get(Joint::HandL).y < pose.get(Joint::ShoulderL).y - 0.4);
    assert!(pose.reached[0] && pose.reached[1]);
}

// ---- fixture generation --------------------------------------------------------------------

struct Jitter(u64);

impl Jitter {
    fn next(&mut self) -> f32 {
        self.0 = self
            .0
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        ((self.0 >> 33) as f32 / (1u64 << 31) as f32) - 0.5
    }
    fn v(&mut self, s: f32) -> Vec3 {
        Vec3::new(self.next(), self.next(), self.next()) * s
    }
}

fn record(name: &str, seconds: f32, f: impl Fn(f32) -> RecFrame) -> Recording {
    let dt = 1.0 / 72.0;
    let mut j = Jitter(name.len() as u64 * 7919);
    let n = (seconds / dt) as usize;
    let frames = (0..n)
        .map(|i| {
            let mut fr = f(i as f32 * dt);
            fr.head.position += j.v(0.002);
            for h in fr.hands.iter_mut().flatten() {
                h.position += j.v(0.003);
            }
            fr
        })
        .collect();
    Recording {
        name: name.into(),
        dt,
        floor: 0.0,
        frames,
    }
}

fn rest_hand(head: &Pose, side: f32) -> Option<Pose> {
    let yaw = Quat::from_rotation_y(head.yaw());
    Some(Pose::new(
        Vec3::new(head.position.x, 0.0, head.position.z)
            + yaw * Vec3::new(0.25 * side, 0.95, -0.15),
        yaw,
    ))
}

#[test]
#[ignore = "rewrites the committed fixtures"]
fn regenerate_fixtures() {
    let stand = |pos: Vec3, yaw: f32, pitch: f32| {
        Pose::new(
            pos,
            Quat::from_rotation_y(yaw) * Quat::from_rotation_x(pitch),
        )
    };
    let recs = vec![
        record("idle_look_around", 4.0, |t| {
            let head = stand(
                Vec3::new(0.0, 1.7, 0.0),
                (t * 1.6).sin() * 0.55,
                (t * 0.9).sin() * 0.3,
            );
            RecFrame {
                head,
                hands: [
                    rest_hand(&Pose::new(head.position, Quat::IDENTITY), -1.0),
                    rest_hand(&Pose::new(head.position, Quat::IDENTITY), 1.0),
                ],
            }
        }),
        record("turn_in_place", 3.0, |t| {
            let yaw = (t / 2.5).min(1.0) * std::f32::consts::PI;
            let head = stand(Vec3::new(0.0, 1.7, 0.0), yaw, 0.0);
            RecFrame {
                head,
                hands: [rest_hand(&head, -1.0), rest_hand(&head, 1.0)],
            }
        }),
        record("crouch", 3.0, |t| {
            let k = (t * std::f32::consts::PI / 3.0).sin();
            let head = stand(Vec3::new(0.0, 1.7 - 0.75 * k, 0.05 * k), 0.0, -0.2 * k);
            RecFrame {
                head,
                hands: [rest_hand(&head, -1.0), rest_hand(&head, 1.0)],
            }
        }),
        record("walk_forward", 3.0, |t| {
            let z = -1.1 * t;
            let bob = (t * 2.0 * std::f32::consts::TAU).sin() * 0.02;
            let head = stand(Vec3::new(0.0, 1.7 + bob, z), 0.0, 0.0);
            RecFrame {
                head,
                hands: [rest_hand(&head, -1.0), rest_hand(&head, 1.0)],
            }
        }),
        record("reach", 4.0, |t| {
            let head = stand(Vec3::new(0.0, 1.7, 0.0), 0.0, 0.0);
            // Forward, up, to the side, then far out of reach.
            let target = if t < 1.0 {
                Vec3::new(0.25, 1.4, -0.4)
            } else if t < 2.0 {
                Vec3::new(0.2, 2.0, -0.2)
            } else if t < 3.0 {
                Vec3::new(0.7, 1.3, 0.0)
            } else {
                Vec3::new(0.3, 1.4, -1.5)
            };
            RecFrame {
                head,
                hands: [
                    rest_hand(&head, -1.0),
                    Some(Pose::new(target, Quat::IDENTITY)),
                ],
            }
        }),
        record("sword_swing", 2.0, |t| {
            let head = stand(Vec3::new(0.0, 1.7, 0.0), 0.0, 0.0);
            // Overhead-right to low-left diagonal and back, twice.
            let a = (t * std::f32::consts::TAU).sin();
            let p = Vec3::new(
                if a > 0.0 { 0.3 * a } else { 0.15 * a },
                1.4 + 0.35 * a,
                -0.3,
            );
            RecFrame {
                head,
                hands: [
                    rest_hand(&head, -1.0),
                    Some(Pose::new(p, Quat::from_rotation_z(-0.8 * a))),
                ],
            }
        }),
    ];
    std::fs::create_dir_all(dir()).unwrap();
    for r in recs {
        let text = serde_json::to_string(&r).unwrap();
        std::fs::write(dir().join(format!("{}.json", r.name)), text).unwrap();
    }
}
