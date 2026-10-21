//! Execution layer: the original AI decides when/whom/which move, the VR layer executes it.
use glam::Vec3;
use gothic_vr_melee::{Bone, DamageType, dummy_zones};
use gothic_vr_npc::ai::{AttackDir, AttackSlots, Perception, Phase};
use gothic_vr_npc::exec::*;

const DT: f32 = 1.0 / 90.0;
const PLAYER: u64 = 1;

fn per(player: Vec3) -> Perception {
    Perception {
        player,
        threat: None,
        can_act: true,
    }
}

fn attack(action: AiAction) -> AiIntent {
    AiIntent {
        target: Some(PLAYER),
        action,
    }
}

fn player_body() -> TargetBody {
    TargetBody {
        hitzones: dummy_zones(Vec3::ZERO),
        weapon: None,
    }
}

#[test]
fn sim_actions_map_to_moves() {
    assert_eq!(AiAction::from_sim(2), AiAction::PowerAttack);
    assert_eq!(AiAction::from_sim(16), AiAction::Parade);
    assert_eq!(AiAction::from_sim(30), AiAction::GetUpAttack);
    assert_eq!(AiAction::from_sim(7), AiAction::Other(7));
    assert!(AiAction::from_sim(11).is_attack());
    assert!(!AiAction::Parade.is_attack());
    use BodyKind::*;
    assert_eq!(
        plan(Humanoid, AiAction::PowerAttack, false),
        Some(VrMove::Swing(AttackDir::Overhead))
    );
    assert_eq!(
        plan(Humanoid, AiAction::PierceAttack, false),
        Some(VrMove::Swing(AttackDir::Thrust))
    );
    assert_eq!(
        plan(Humanoid, AiAction::QuickAttackL, false),
        Some(VrMove::Swing(AttackDir::FromLeft))
    );
    assert_ne!(
        plan(Humanoid, AiAction::QuickAttack, false),
        plan(Humanoid, AiAction::QuickAttack, true)
    );
    assert_eq!(plan(Quadruped, AiAction::Attack, false), Some(VrMove::Bite));
    assert_eq!(
        plan(Quadruped, AiAction::JumpAttack, false),
        Some(VrMove::Lunge)
    );
    assert_eq!(plan(Quadruped, AiAction::Parade, false), None);
    assert_eq!(plan(Humanoid, AiAction::Other(20), false), None);
}

#[test]
fn commanded_humanoid_only_attacks_when_ordered() {
    let mut ex = Executor::humanoid(7, Vec3::new(0.0, 0.0, -1.5), 0.0);
    let mut slots = AttackSlots::new(2);
    let target = player_body();
    for _ in 0..(10.0 / DT) as usize {
        let (intent, events) = ex.step(&per(Vec3::ZERO), &target, &mut slots, DT);
        let phase = intent.unwrap().phase;
        assert!(
            !matches!(phase, Phase::Telegraph(..) | Phase::Strike(..)),
            "attacked without an order: {phase:?}"
        );
        assert!(events.is_empty(), "{events:?}");
    }
    // The AI orders a power attack: overhead, telegraphed, then a strike.
    ex.command(attack(AiAction::PowerAttack));
    let mut telegraph = None;
    let mut struck = false;
    for _ in 0..(3.0 / DT) as usize {
        let (intent, events) = ex.step(&per(Vec3::ZERO), &target, &mut slots, DT);
        for e in events {
            if let ExecEvent::Telegraph { vr_move, lead } = e {
                telegraph = Some((vr_move, lead));
            }
        }
        if let Phase::Strike(d, _) = intent.unwrap().phase {
            assert_eq!(d, AttackDir::Overhead);
            struck = true;
        }
    }
    let (m, lead) = telegraph.expect("no telegraph");
    assert_eq!(m, VrMove::Swing(AttackDir::Overhead));
    assert!(lead >= 0.4, "{lead}");
    assert!(struck);
    assert_eq!(
        ex.current.map(|c| c.0),
        None,
        "move cleared after recovering"
    );
}

#[test]
fn attack_slots_limit_simultaneous_attackers() {
    let mut slots = AttackSlots::new(2);
    let target = player_body();
    let mut ex: Vec<Executor> = (0..3)
        .map(|i| {
            let a = i as f32 * 2.1;
            let at = Vec3::new(a.sin(), 0.0, a.cos()) * 1.5;
            let yaw = (at.x).atan2(at.z);
            Executor::humanoid(10 + i, at, yaw)
        })
        .collect();
    let mut waited = 0;
    let mut most = 0;
    for _ in 0..(8.0 / DT) as usize {
        let mut attacking = 0;
        for e in &mut ex {
            if !e.attacking() && e.intent.is_none_or(|i| i.action == AiAction::None) {
                e.command(attack(AiAction::Attack));
            }
            let (_, events) = e.step(&per(Vec3::ZERO), &target, &mut slots, DT);
            waited += events
                .iter()
                .filter(|e| matches!(e, ExecEvent::WaitingForSlot))
                .count();
            if events
                .iter()
                .any(|e| matches!(e, ExecEvent::Telegraph { .. }))
            {
                e.intent = None;
            }
            attacking += usize::from(e.attacking());
        }
        most = most.max(attacking);
        assert!(attacking <= 2, "{attacking} attack at once");
    }
    assert_eq!(most, 2);
    assert!(waited > 0, "the third never had to wait");
}

#[test]
fn quadruped_bite_lands_as_hit_event_with_the_action() {
    let mut wolf = Executor::quadruped(5, Vec3::new(0.0, 0.0, -3.0), 0.0);
    let mut slots = AttackSlots::new(1);
    let target = player_body();
    wolf.command(attack(AiAction::QuickAttack));
    let mut hits = Vec::new();
    let mut telegraphed = false;
    for _ in 0..(4.0 / DT) as usize {
        let (intent, events) = wolf.step(&per(Vec3::ZERO), &target, &mut slots, DT);
        assert!(intent.is_none());
        for e in events {
            match e {
                ExecEvent::Telegraph { vr_move, .. } => {
                    assert_eq!(vr_move, VrMove::Bite);
                    assert!(hits.is_empty(), "hit before the telegraph");
                    telegraphed = true;
                }
                ExecEvent::Hit(h) => hits.push(h),
                _ => {}
            }
        }
    }
    assert!(telegraphed);
    assert_eq!(hits.len(), 1, "one hit per attack: {hits:?}");
    let h = hits[0];
    assert_eq!((h.attacker, h.victim), (5, PLAYER));
    assert_eq!(h.action, AiAction::QuickAttack);
    assert_eq!(h.vr_move, VrMove::Bite);
    assert_eq!(h.damage_type, DamageType::Point);
    assert!(h.impact_speed > 0.5, "{}", h.impact_speed);
    assert!(slots.holders().is_empty(), "slot released after recovery");
}

#[test]
fn blocked_quadruped_recoils_without_hit() {
    let mut wolf = Executor::quadruped(5, Vec3::new(0.0, 0.0, -1.3), 0.0);
    let mut slots = AttackSlots::new(1);
    // Player holds the blade crosswise in front of the body at the wolf's head height.
    let target = TargetBody {
        hitzones: dummy_zones(Vec3::ZERO),
        weapon: Some((Vec3::new(-0.5, 0.75, -0.45), Vec3::new(0.5, 0.95, -0.45))),
    };
    wolf.command(attack(AiAction::JumpAttack));
    let mut blocked = false;
    for _ in 0..(3.0 / DT) as usize {
        let (_, events) = wolf.step(&per(Vec3::ZERO), &target, &mut slots, DT);
        for e in events {
            match e {
                ExecEvent::Hit(h) => panic!("hit through the guard: {h:?}"),
                ExecEvent::Blocked => blocked = true,
                _ => {}
            }
        }
        if blocked {
            let Body::Quadruped(q) = &wolf.body else {
                unreachable!()
            };
            assert!(matches!(q.phase, QuadPhase::Recoil(_)), "{:?}", q.phase);
            break;
        }
    }
    assert!(blocked);
}

#[test]
fn humanoid_clash_recoils_and_melee_hit_carries_the_action() {
    let mut ex = Executor::humanoid(7, Vec3::new(0.0, 0.0, -1.5), 0.0);
    let mut slots = AttackSlots::new(1);
    let target = player_body();
    assert_eq!(ex.on_clash(0.5), None, "not attacking");
    assert!(
        ex.on_melee_hit(PLAYER, Bone::Chest, DamageType::Edge, Vec3::Y, Vec3::X, 5.0)
            .is_none(),
        "no move in flight"
    );
    ex.command(attack(AiAction::PowerAttack));
    while !matches!(
        ex.step(&per(Vec3::ZERO), &target, &mut slots, DT)
            .0
            .unwrap()
            .phase,
        Phase::Strike(..)
    ) {}
    let hit = ex
        .on_melee_hit(
            PLAYER,
            Bone::Head,
            DamageType::Edge,
            Vec3::Y,
            Vec3::NEG_Y,
            6.0,
        )
        .expect("hit event");
    assert_eq!(hit.action, AiAction::PowerAttack);
    assert_eq!(hit.vr_move, VrMove::Swing(AttackDir::Overhead));
    assert_eq!(hit.bone, Bone::Head);
    assert!(
        ex.on_melee_hit(PLAYER, Bone::Chest, DamageType::Edge, Vec3::Y, Vec3::X, 5.0)
            .is_none(),
        "one hit per attack"
    );
    assert_eq!(ex.on_clash(0.5), None, "follow-through is not a block");
    // Next attack meets the player's guard before landing: block and recoil.
    while ex.attacking() {
        ex.step(&per(Vec3::ZERO), &target, &mut slots, DT);
    }
    ex.command(attack(AiAction::QuickAttackL));
    while !matches!(
        ex.step(&per(Vec3::ZERO), &target, &mut slots, DT)
            .0
            .unwrap()
            .phase,
        Phase::Strike(..)
    ) {}
    assert_eq!(ex.on_clash(0.5), Some(ExecEvent::Blocked));
    let (intent, _) = ex.step(&per(Vec3::ZERO), &target, &mut slots, DT);
    assert!(matches!(intent.unwrap().phase, Phase::Recoil(_)));
}

#[test]
fn parade_raises_the_guard() {
    let mut ex = Executor::humanoid(7, Vec3::new(0.0, 0.0, -1.5), 0.0);
    let mut slots = AttackSlots::new(1);
    ex.command(AiIntent {
        target: Some(PLAYER),
        action: AiAction::Parade,
    });
    let (intent, _) = ex.step(&per(Vec3::ZERO), &player_body(), &mut slots, DT);
    assert!(matches!(intent.unwrap().phase, Phase::Block(_)));
}

#[test]
fn knocked_down_quadruped_gets_up() {
    let mut wolf = Executor::quadruped(5, Vec3::new(0.0, 0.0, -1.3), 0.0);
    let mut slots = AttackSlots::new(1);
    wolf.on_hit_taken(true, &mut slots);
    let mut up = false;
    for _ in 0..(3.0 / DT) as usize {
        let (_, events) = wolf.step(&per(Vec3::ZERO), &player_body(), &mut slots, DT);
        up |= events.contains(&ExecEvent::GettingUp);
    }
    assert!(up);
}
