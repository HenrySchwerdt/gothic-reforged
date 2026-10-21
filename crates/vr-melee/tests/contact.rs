//! Contact model scenarios against a capsule dummy (no engine, fully deterministic).
use glam::{Quat, Vec3};
use gothic_vr::Pose;
use gothic_vr_melee::contact::*;
use gothic_vr_melee::profiles::profile_for;
use gothic_vr_physics::props::PropKind;

const DT: f32 = 1.0 / 90.0;
const PLAYER: u64 = 1;
const DUMMY: u64 = 100;
const SWORD: u64 = 10;
const SHIELD_SWORD: u64 = 11;

fn dummy() -> Vec<Hitzone> {
    vec![
        Hitzone::between(
            Bone::Chest,
            Vec3::new(0.0, 0.95, -1.0),
            Vec3::new(0.0, 1.45, -1.0),
            0.18,
        ),
        Hitzone::between(
            Bone::Head,
            Vec3::new(0.0, 1.58, -1.0),
            Vec3::new(0.0, 1.72, -1.0),
            0.11,
        ),
    ]
}

/// Sword held at `hand`, blade yawed by `deg` (0 = straight -Z, positive turns toward -X),
/// rolled so the edge faces the swing direction when `edge_first`.
fn swing_pose(hand: Vec3, deg: f32, edge_first: bool) -> Pose {
    let roll = if edge_first {
        Quat::from_rotation_z(std::f32::consts::FRAC_PI_2)
    } else {
        Quat::IDENTITY
    };
    Pose::new(hand, Quat::from_rotation_y(deg.to_radians()) * roll)
}

/// Run a sequence of weapon poses against the dummy (+ optional defender weapon poses).
fn run(
    world: &mut MeleeWorld,
    attacker: &[Pose],
    profile_kind: PropKind,
    defender: Option<&[Pose]>,
    wielder: Option<u64>,
) -> Vec<MeleeEvent> {
    let profile = profile_for(profile_kind).unwrap();
    let sword = profile_for(PropKind::Sword).unwrap();
    let zones = dummy();
    let mut out = Vec::new();
    for i in 1..attacker.len() {
        let mut weapons = vec![WeaponMotion {
            id: SWORD,
            wielder,
            prev: attacker[i - 1],
            cur: attacker[i],
            profile: &profile,
        }];
        if let Some(d) = defender {
            weapons.push(WeaponMotion {
                id: SHIELD_SWORD,
                wielder: Some(DUMMY),
                prev: d[(i - 1).min(d.len() - 1)],
                cur: d[i.min(d.len() - 1)],
                profile: &sword,
            });
        }
        let bodies = [BodyMotion {
            id: DUMMY,
            prev: &zones,
            cur: &zones,
        }];
        out.extend(world.step(&weapons, &bodies, DT));
    }
    out
}

fn sweep(from: f32, to: f32, seconds: f32, edge_first: bool) -> Vec<Pose> {
    sweep_from(Vec3::new(0.25, 1.2, -0.2), from, to, seconds, edge_first)
}

fn sweep_from(hand: Vec3, from: f32, to: f32, seconds: f32, edge_first: bool) -> Vec<Pose> {
    let n = (seconds / DT).round().max(1.0) as usize;
    (0..=n)
        .map(|i| {
            let d = from + (to - from) * i as f32 / n as f32;
            swing_pose(hand, d, edge_first)
        })
        .collect()
}

fn hits(events: &[MeleeEvent]) -> Vec<HitEvent> {
    events
        .iter()
        .filter_map(|e| match e {
            MeleeEvent::Hit(h) => Some(*h),
            _ => None,
        })
        .collect()
}

#[test]
fn fast_swing_cuts() {
    let mut w = MeleeWorld::default();
    let ev = run(
        &mut w,
        &sweep(-40.0, 60.0, 0.1, true),
        PropKind::Sword,
        None,
        Some(PLAYER),
    );
    let h = hits(&ev);
    assert_eq!(h.len(), 1, "{ev:?}");
    let h = h[0];
    assert_eq!(h.damage_type, DamageType::Edge);
    assert_eq!(h.bone, Bone::Chest);
    assert_eq!(
        (h.attacker, h.victim, h.weapon),
        (Some(PLAYER), DUMMY, SWORD)
    );
    assert!(
        h.impact_speed > 9.0 && h.impact_speed < 16.0,
        "{}",
        h.impact_speed
    );
    assert!(h.quality > 0.8);
    // On the dummy's surface.
    let r = Vec3::new(h.point.x, 0.0, h.point.z + 1.0).length();
    assert!((r - 0.18).abs() < 0.03, "{}", h.point);
    assert!(
        h.direction.x < -0.8,
        "swing goes toward -X: {}",
        h.direction
    );
}

#[test]
fn slow_swing_does_no_damage() {
    let mut w = MeleeWorld::default();
    let ev = run(
        &mut w,
        &sweep(-40.0, 60.0, 2.0, true),
        PropKind::Sword,
        None,
        Some(PLAYER),
    );
    assert!(hits(&ev).is_empty(), "{ev:?}");
}

#[test]
fn waggling_inside_the_body_does_not_rehit() {
    let mut w = MeleeWorld::default();
    // Blade resting through the chest, then shaken 3 cm at 30 Hz (5+ m/s) while staying inside.
    let base = swing_pose(Vec3::new(0.25, 1.2, -0.2), 15.0, true);
    let poses: Vec<Pose> = (0..180)
        .map(|i| {
            let t = i as f32 * DT;
            let off = Vec3::X * 0.03 * (t * std::f32::consts::TAU * 30.0).sin();
            Pose::new(base.position + off, base.rotation)
        })
        .collect();
    let ev = run(&mut w, &poses, PropKind::Sword, None, Some(PLAYER));
    assert!(hits(&ev).is_empty(), "{ev:?}");
    // Small in-and-out jitter at the surface is too slow to count.
    let mut w = MeleeWorld::default();
    let edge = swing_pose(Vec3::new(0.25, 1.2, -0.2), 4.0, true);
    let poses: Vec<Pose> = (0..180)
        .map(|i| {
            let t = i as f32 * DT;
            let off = Vec3::X * 0.015 * (t * std::f32::consts::TAU * 8.0).sin();
            Pose::new(edge.position + off, edge.rotation)
        })
        .collect();
    let ev = run(&mut w, &poses, PropKind::Sword, None, Some(PLAYER));
    assert!(hits(&ev).is_empty(), "{ev:?}");
}

#[test]
fn flat_of_the_blade_is_blunt_and_weak() {
    let mut w = MeleeWorld::default();
    let ev = run(
        &mut w,
        &sweep(-40.0, 60.0, 0.1, false),
        PropKind::Sword,
        None,
        Some(PLAYER),
    );
    let h = hits(&ev);
    assert_eq!(h.len(), 1);
    assert_eq!(h[0].damage_type, DamageType::Blunt);
    assert!(h[0].quality < 0.4);
}

#[test]
fn mace_is_blunt() {
    let mut w = MeleeWorld::default();
    // The mace is shorter: step in.
    let poses = sweep_from(Vec3::new(0.25, 1.2, -0.45), -40.0, 60.0, 0.12, true);
    let ev = run(&mut w, &poses, PropKind::Mace, None, Some(PLAYER));
    let h = hits(&ev);
    assert_eq!(h.len(), 1, "{ev:?}");
    assert_eq!(h[0].damage_type, DamageType::Blunt);
}

#[test]
fn a_swing_cannot_tunnel_in_one_step() {
    let mut w = MeleeWorld::default();
    // 100 degrees in a single 1/90 s step (~140 m/s at the tip).
    let ev = run(
        &mut w,
        &sweep(-40.0, 60.0, DT, true),
        PropKind::Sword,
        None,
        Some(PLAYER),
    );
    assert_eq!(hits(&ev).len(), 1, "{ev:?}");
}

#[test]
fn wielder_cannot_hit_himself() {
    let mut w = MeleeWorld::default();
    let ev = run(
        &mut w,
        &sweep(-40.0, 60.0, 0.1, true),
        PropKind::Sword,
        None,
        Some(DUMMY),
    );
    assert!(hits(&ev).is_empty());
}

#[test]
fn rehit_after_cooldown_on_a_new_swing() {
    let mut w = MeleeWorld::default();
    let mut poses = sweep(-40.0, 60.0, 0.1, true);
    poses.extend(sweep(60.0, -40.0, 0.5, true)); // slow return
    poses.extend(sweep(-40.0, 60.0, 0.1, true));
    let ev = run(&mut w, &poses, PropKind::Sword, None, Some(PLAYER));
    assert_eq!(hits(&ev).len(), 2, "{ev:?}");
}

fn thrust(from_z: f32, to_z: f32, seconds: f32) -> Vec<Pose> {
    let n = (seconds / DT).round() as usize;
    (0..=n)
        .map(|i| {
            let z = from_z + (to_z - from_z) * i as f32 / n as f32;
            Pose::new(Vec3::new(0.0, 1.2, z), Quat::IDENTITY)
        })
        .collect()
}

#[test]
fn stab_penetrates_sticks_and_pulls_out() {
    let mut w = MeleeWorld::default();
    // Tip from z -0.61 to -0.96 (chest surface at -0.82) at ~3.5 m/s.
    let mut poses = thrust(0.2, -0.15, 0.1);
    let ev = run(&mut w, &poses, PropKind::Sword, None, Some(PLAYER));
    let h = hits(&ev);
    assert_eq!(h.len(), 1, "{ev:?}");
    assert_eq!(h[0].damage_type, DamageType::Point);
    assert!(ev.iter().any(|e| matches!(
        e,
        MeleeEvent::Stuck {
            victim: DUMMY,
            bone: Bone::Chest,
            ..
        }
    )));
    assert_eq!(w.stuck_in(SWORD), Some(DUMMY));

    // Wiggle sideways 3 cm and push a bit deeper: still stuck, no new hits.
    let last = *poses.last().unwrap();
    let wiggle: Vec<Pose> = (0..30)
        .map(|i| {
            Pose::new(
                last.position + Vec3::new(0.03 * (i as f32 * 0.5).sin(), 0.0, -0.002 * i as f32),
                last.rotation,
            )
        })
        .collect();
    let ev = run(&mut w, &wiggle, PropKind::Sword, None, Some(PLAYER));
    assert!(ev.is_empty(), "{ev:?}");
    assert_eq!(w.stuck_in(SWORD), Some(DUMMY));

    // Pull back out along the wound.
    poses = thrust(wiggle.last().unwrap().position.z, 0.3, 0.3);
    let ev = run(&mut w, &poses, PropKind::Sword, None, Some(PLAYER));
    assert!(
        ev.iter().any(|e| matches!(
            e,
            MeleeEvent::PulledOut {
                weapon: SWORD,
                victim: DUMMY
            }
        )),
        "{ev:?}"
    );
    assert!(hits(&ev).is_empty(), "pulling out is not a new hit");
    assert_eq!(w.stuck_in(SWORD), None);
}

#[test]
fn a_slow_poke_does_not_stick() {
    let mut w = MeleeWorld::default();
    let ev = run(
        &mut w,
        &thrust(0.2, -0.15, 1.0),
        PropKind::Sword,
        None,
        Some(PLAYER),
    );
    assert!(ev.is_empty(), "{ev:?}");
    assert_eq!(w.stuck_in(SWORD), None);
}

/// Defender sword held upright at x = 0.45 in front-right of the dummy.
fn guard_pose(x: f32) -> Pose {
    Pose::new(
        Vec3::new(x, 0.9, -0.85),
        Quat::from_rotation_x(std::f32::consts::FRAC_PI_2),
    )
}

#[test]
fn held_weapon_blocks_and_protects() {
    let mut w = MeleeWorld::default();
    let defender = vec![guard_pose(0.45)];
    let ev = run(
        &mut w,
        &sweep(-40.0, 60.0, 0.1, true),
        PropKind::Sword,
        Some(&defender),
        Some(PLAYER),
    );
    let clash = ev
        .iter()
        .find_map(|e| match e {
            MeleeEvent::Clash {
                kind,
                attacker_weapon,
                defender_weapon,
                attacker_impulse,
                recoil,
                ..
            } => Some((
                *kind,
                *attacker_weapon,
                *defender_weapon,
                *attacker_impulse,
                *recoil,
            )),
            _ => None,
        })
        .expect("clash");
    assert_eq!(clash.0, ClashKind::Block);
    assert_eq!((clash.1, clash.2), (SWORD, SHIELD_SWORD));
    assert!(clash.3.x > 1.0, "deflected back toward +X: {}", clash.3);
    assert!(clash.4 > 0.0);
    // The physical deflection is the engine's job; even when the replayed swing carries on
    // through the guard, the recoil keeps the blocked swing from damaging the body.
    assert!(hits(&ev).is_empty(), "{ev:?}");
}

#[test]
fn counter_swing_parries() {
    let mut w = MeleeWorld::default();
    // The defender's blade moves +X (into the incoming blade) at ~3.6 m/s.
    let n = (0.1 / DT).round() as usize;
    let defender: Vec<Pose> = (0..=n).map(|i| guard_pose(0.3 + 0.04 * i as f32)).collect();
    let ev = run(
        &mut w,
        &sweep(-40.0, 60.0, 0.1, true),
        PropKind::Sword,
        Some(&defender),
        Some(PLAYER),
    );
    let kind = ev
        .iter()
        .find_map(|e| match e {
            MeleeEvent::Clash {
                kind,
                attacker_weapon: SWORD,
                recoil,
                ..
            } => Some((*kind, *recoil)),
            _ => None,
        })
        .expect("clash");
    assert_eq!(kind.0, ClashKind::Parry);
    assert!(kind.1 > MeleeSettings::default().block_recoil);
    assert!(hits(&ev).is_empty());
}

#[test]
fn deterministic() {
    let go = || {
        let mut w = MeleeWorld::default();
        let mut p = sweep(-40.0, 60.0, 0.1, true);
        p.extend(thrust(0.2, -0.15, 0.1));
        format!("{:?}", run(&mut w, &p, PropKind::Sword, None, Some(PLAYER)))
    };
    assert_eq!(go(), go());
}
