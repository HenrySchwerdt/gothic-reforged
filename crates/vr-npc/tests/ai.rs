//! Combat AI: telegraphs, slots, distance, blocking, recoil, edge-first strikes.
use glam::Vec3;
use gothic_vr_npc::ai::*;

const DT: f32 = 1.0 / 90.0;

fn per(player: Vec3) -> Perception {
    Perception {
        player,
        threat: None,
        can_act: true,
    }
}

#[test]
fn every_strike_is_telegraphed_on_its_side() {
    let mut ai = CombatAi::new(Vec3::new(0.0, 0.0, -1.5), 0.0, 7);
    let mut slots = AttackSlots::new(1);
    let mut telegraph = 0.0;
    let mut strikes = 0;
    let mut last = Phase::Ready;
    for _ in 0..(20.0 / DT) as usize {
        let i = ai.update(1, &per(Vec3::ZERO), &mut slots, DT);
        match (last, i.phase) {
            (Phase::Telegraph(..), Phase::Telegraph(..)) => telegraph += DT,
            (_, Phase::Telegraph(..)) => telegraph = DT,
            (Phase::Telegraph(d0, _), Phase::Strike(d1, _)) => {
                assert_eq!(d0, d1);
                assert!(telegraph >= 0.5, "telegraph only {telegraph}s");
                strikes += 1;
            }
            (p, Phase::Strike(_, 0.0)) => panic!("strike without telegraph after {p:?}"),
            _ => {}
        }
        if let Phase::Telegraph(d, t) = i.phase
            && t > 0.4
        {
            let local = ai.frame().inverse().transform_point(i.weapon.position);
            match d {
                AttackDir::FromRight => assert!(local.x > 0.3, "{local}"),
                AttackDir::FromLeft => assert!(local.x < -0.1, "{local}"),
                AttackDir::Overhead => assert!(local.y > 1.6, "{local}"),
                AttackDir::Thrust => assert!(local.z > -0.1, "{local}"),
            }
        }
        last = i.phase;
    }
    assert!(strikes >= 5, "{strikes}");
}

#[test]
fn strikes_lead_with_the_edge() {
    let s = AiSettings::default();
    for dir in [
        AttackDir::FromRight,
        AttackDir::FromLeft,
        AttackDir::Overhead,
    ] {
        let a = local_weapon_pose(Phase::Strike(dir, s.strike_time * 0.45), &s);
        let b = local_weapon_pose(Phase::Strike(dir, s.strike_time * 0.55), &s);
        // A point 0.6 m down the blade.
        let p = |w: &gothic_vr::Pose| w.transform_point(Vec3::new(0.0, 0.0, -0.6));
        let v = (p(&b) - p(&a)).normalize();
        let edge = a.rotation * Vec3::Y;
        assert!(v.dot(edge) > 0.7, "{dir:?}: {}", v.dot(edge));
        let speed = p(&b).distance(p(&a)) / (s.strike_time * 0.1);
        assert!(speed > 6.0, "{dir:?} strike speed {speed}");
    }
}

#[test]
fn attack_slots_limit_simultaneous_attackers() {
    let mut slots = AttackSlots::new(2);
    let mut ais: Vec<CombatAi> = (0..3)
        .map(|i| {
            let a = i as f32 * 2.1;
            CombatAi::new(
                Vec3::new(1.5 * a.sin(), 0.0, 1.5 * a.cos()),
                0.0,
                i as u64 + 1,
            )
        })
        .collect();
    let mut attacked = [false; 3];
    for _ in 0..(30.0 / DT) as usize {
        let mut busy = 0;
        for (k, ai) in ais.iter_mut().enumerate() {
            let i = ai.update(k as u64, &per(Vec3::ZERO), &mut slots, DT);
            if matches!(
                i.phase,
                Phase::Telegraph(..) | Phase::Strike(..) | Phase::Recover(..)
            ) {
                busy += 1;
                attacked[k] = true;
            }
        }
        assert!(busy <= 2, "{busy} attackers at once");
        assert!(slots.holders().len() <= 2);
    }
    assert_eq!(attacked, [true; 3], "everyone gets a turn");
}

#[test]
fn keeps_fighting_distance() {
    let mut slots = AttackSlots::default();
    let mut far = CombatAi::new(Vec3::new(0.0, 0.0, -6.0), 0.0, 3);
    let mut near = CombatAi::new(Vec3::new(0.0, 0.0, -0.4), 0.0, 4);
    for _ in 0..(5.0 / DT) as usize {
        far.update(1, &per(Vec3::ZERO), &mut slots, DT);
        near.update(2, &per(Vec3::ZERO), &mut slots, DT);
    }
    let s = AiSettings::default();
    let d = |a: &CombatAi| Vec3::new(a.root.x, 0.0, a.root.z).length();
    assert!(
        d(&far) <= s.max_range + 0.3 && d(&far) >= s.min_range - 0.3,
        "{}",
        d(&far)
    );
    assert!(d(&near) >= s.min_range - 0.3, "{}", d(&near));
}

#[test]
fn blocks_a_swing_from_the_right_with_its_blade() {
    let mut slots = AttackSlots::default();
    let mut ai = CombatAi::new(Vec3::new(0.0, 0.0, -1.5), 0.0, 5);
    ai.cooldown = 10.0;
    // NPC faces +Z (toward the player at the origin); its right is -X in world.
    for _ in 0..30 {
        ai.update(1, &per(Vec3::ZERO), &mut slots, DT);
    }
    let right = ai.frame().rotation * Vec3::X;
    let chest = ai.root + Vec3::Y * 1.2;
    let threat = Threat {
        point: chest + right * 0.6 - ai.frame().forward() * -0.3,
        velocity: -right * 6.0,
    };
    let i = ai.update(
        1,
        &Perception {
            player: Vec3::ZERO,
            threat: Some(threat),
            can_act: true,
        },
        &mut slots,
        DT,
    );
    assert_eq!(i.phase, Phase::Block(GuardSide::Right));
    let local = ai.frame().inverse().transform_point(i.weapon.position);
    assert!(
        local.x > 0.2 && local.z < -0.2,
        "blade in front-right: {local}"
    );
    let blade = i.weapon.rotation * Vec3::NEG_Z;
    assert!(blade.y > 0.9, "blade upright: {blade}");
    // A slow-moving weapon is no threat.
    let calm = Threat {
        velocity: -right * 0.5,
        ..threat
    };
    let i = ai.update(
        1,
        &Perception {
            player: Vec3::ZERO,
            threat: Some(calm),
            can_act: true,
        },
        &mut slots,
        DT,
    );
    assert_eq!(i.phase, Phase::Ready);
}

#[test]
fn recoils_when_blocked_and_frees_the_slot() {
    let mut slots = AttackSlots::new(1);
    let mut ai = CombatAi::new(Vec3::new(0.0, 0.0, -1.5), 0.0, 9);
    ai.cooldown = 0.0;
    let mut phase = Phase::Ready;
    for _ in 0..200 {
        phase = ai.update(1, &per(Vec3::ZERO), &mut slots, DT).phase;
        if matches!(phase, Phase::Strike(..)) {
            break;
        }
    }
    assert!(matches!(phase, Phase::Strike(..)), "{phase:?}");
    ai.recoil(0.8);
    let p = ai.update(1, &per(Vec3::ZERO), &mut slots, DT).phase;
    assert!(matches!(p, Phase::Recoil(_)));
    for _ in 0..(0.9 / DT) as usize {
        ai.update(1, &per(Vec3::ZERO), &mut slots, DT);
    }
    assert!(slots.holders().is_empty());
    assert!(!matches!(ai.phase, Phase::Recoil(_)));
}

#[test]
fn disabled_while_off_balance() {
    let mut slots = AttackSlots::new(1);
    let mut ai = CombatAi::new(Vec3::new(0.0, 0.0, -1.5), 0.0, 2);
    ai.cooldown = 0.0;
    ai.update(1, &per(Vec3::ZERO), &mut slots, DT);
    assert_eq!(slots.holders(), &[1]);
    let down = Perception {
        can_act: false,
        ..per(Vec3::ZERO)
    };
    let i = ai.update(1, &down, &mut slots, DT);
    assert_eq!(i.phase, Phase::Disabled);
    assert!(slots.holders().is_empty());
}
