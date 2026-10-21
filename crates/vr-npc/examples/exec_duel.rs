//! Headless greybox duel for the execution layer (VR-12): a stubbed original-AI intent stream
//! drives an orc (humanoid, sword, contacts from the melee model) and a wolf (quadruped)
//! against a scripted player who raises a guard every other window.
//!
//! `cargo run -p gothic-vr-npc --example exec_duel` prints the event log and fails if the
//! demo misses a telegraph, a slot wait, a block with recoil or a hit event.
use glam::{Quat, Vec3};
use gothic_vr::Pose;
use gothic_vr_melee::contact::{BodyMotion, MeleeSettings, MeleeWorld, WeaponMotion};
use gothic_vr_melee::profiles::profile_for;
use gothic_vr_melee::{MeleeEvent, dummy_zones};
use gothic_vr_npc::ai::{AttackSlots, Perception};
use gothic_vr_npc::exec::*;
use gothic_vr_physics::props::PropKind;

const DT: f32 = 1.0 / 90.0;
const PLAYER: u64 = 1;
const PLAYER_SWORD: u64 = 101;
const ORC: u64 = 10;
const ORC_SWORD: u64 = 110;
const WOLF: u64 = 20;
const SECONDS: f32 = 24.0;

/// Stand-in for the original AI (`gothic-sim` decides these from the combat scripts).
fn original_ai(npc: u64, t: f32) -> Option<AiIntent> {
    let (period, actions): (f32, &[u8]) = match npc {
        // PowerAttack, QuickAttackL, PierceAttack, Parade, QuickAttackR.
        ORC => (2.4, &[2, 5, 11, 16, 4]),
        // Attack, JumpAttack.
        _ => (2.0, &[1, 12]),
    };
    let k = (t / period) as usize;
    (t - k as f32 * period < DT).then(|| AiIntent {
        target: Some(PLAYER),
        action: AiAction::from_sim(actions[k % actions.len()]),
    })
}

/// During odd four-second windows the player holds the blade across the line to whoever is
/// attacking: low against the wolf, at chest height against the orc.
fn player_guard(t: f32, attacker: Option<(Vec3, bool)>) -> Option<Pose> {
    let (from, low) = attacker?;
    ((t / 4.0) as usize % 2 == 1).then(|| {
        let to = Vec3::new(from.x, 0.0, from.z).normalize_or(Vec3::NEG_Z);
        let across = Vec3::Y.cross(to).normalize();
        let (reach, height, rise) = if low {
            (0.75, 0.85, 1.0)
        } else {
            (0.5, 1.35, 0.35)
        };
        let centre = to * reach + Vec3::Y * height;
        let dir = (across + Vec3::Y * rise).normalize();
        // The weapon's blade is its -Z axis; the hilt sits 0.4 m to one side.
        Pose::new(
            centre - dir * 0.4,
            Quat::from_rotation_arc(Vec3::NEG_Z, dir),
        )
    })
}

fn main() {
    let sword = profile_for(PropKind::Sword).expect("sword profile");
    let mut melee = MeleeWorld::new(MeleeSettings::default());
    let mut slots = AttackSlots::new(1);
    let mut orc = Executor::humanoid(ORC, Vec3::new(0.0, 0.0, -1.6), 0.0);
    let mut wolf = Executor::quadruped(WOLF, Vec3::new(1.8, 0.0, -2.2), 0.0);
    let zones = dummy_zones(Vec3::ZERO);
    let rest = Pose::new(Vec3::new(0.3, 0.9, -0.2), Quat::from_rotation_x(-1.2));
    let mut orc_prev: Option<Pose> = None;
    let mut player_prev = rest;
    let (mut telegraphs, mut waits, mut blocks, mut hits) = (0, 0, 0, 0);
    let per = Perception {
        player: Vec3::ZERO,
        threat: None,
        can_act: true,
    };
    for tick in 0..(SECONDS / DT) as usize {
        let t = tick as f32 * DT;
        for ex in [&mut orc, &mut wolf] {
            if let Some(i) = original_ai(ex.id, t) {
                ex.command(i);
            }
        }
        let attacker = if orc.attacking() {
            Some((orc_prev.map_or(Vec3::NEG_Z, |p| p.position), false))
        } else if let (true, Body::Quadruped(q)) = (wolf.attacking(), &wolf.body) {
            Some((q.root, true))
        } else {
            None
        };
        let guard = player_guard(t, attacker);
        let player_pose = guard.unwrap_or(rest);
        let blade = |p: &Pose| {
            (
                p.transform_point(Vec3::new(0.0, 0.0, 0.1)),
                p.transform_point(Vec3::new(0.0, 0.0, -0.8)),
            )
        };
        let target = TargetBody {
            hitzones: zones.clone(),
            weapon: guard.as_ref().map(blade),
        };
        let mut events = Vec::new();
        let (intent, ev) = orc.step(&per, &target, &mut slots, DT);
        events.extend(ev.into_iter().map(|e| (ORC, e)));
        let (_, ev) = wolf.step(&per, &target, &mut slots, DT);
        events.extend(ev.into_iter().map(|e| (WOLF, e)));

        // The orc's sword goes through the melee contact model like any physical weapon.
        let grip = intent.expect("humanoid intent").weapon;
        let weapons = [
            WeaponMotion {
                id: ORC_SWORD,
                wielder: Some(ORC),
                prev: orc_prev.unwrap_or(grip),
                cur: grip,
                profile: &sword,
            },
            WeaponMotion {
                id: PLAYER_SWORD,
                wielder: Some(PLAYER),
                prev: player_prev,
                cur: player_pose,
                profile: &sword,
            },
        ];
        let bodies = [BodyMotion {
            id: PLAYER,
            prev: &zones,
            cur: &zones,
        }];
        for e in melee.step(&weapons, &bodies, DT) {
            match e {
                MeleeEvent::Hit(h) if h.attacker == Some(ORC) => {
                    if let Some(hit) = orc.on_melee_hit(
                        h.victim,
                        h.bone,
                        h.damage_type,
                        h.point,
                        h.direction,
                        h.impact_speed,
                    ) {
                        events.push((ORC, ExecEvent::Hit(hit)));
                    }
                }
                MeleeEvent::Clash {
                    attacker: Some(ORC),
                    recoil,
                    ..
                } => {
                    if let Some(e) = orc.on_clash(recoil) {
                        events.push((ORC, e));
                    }
                }
                _ => {}
            }
        }
        orc_prev = Some(grip);
        player_prev = player_pose;

        for (who, e) in events {
            let name = if who == ORC { "orc " } else { "wolf" };
            match e {
                ExecEvent::Telegraph { vr_move, lead } => {
                    telegraphs += 1;
                    println!("{t:6.2}s {name} telegraphs {vr_move:?} ({lead:.2}s lead)");
                }
                ExecEvent::WaitingForSlot => {
                    waits += 1;
                    println!("{t:6.2}s {name} waits for an attack slot");
                }
                ExecEvent::Blocked => {
                    blocks += 1;
                    println!("{t:6.2}s {name} blocked, recoils");
                }
                ExecEvent::Hit(h) => {
                    hits += 1;
                    println!(
                        "{t:6.2}s {name} HIT {:?} via {:?} on {:?} ({:?}, {:.1} m/s)",
                        h.action, h.vr_move, h.bone, h.damage_type, h.impact_speed
                    );
                }
                ExecEvent::GettingUp => println!("{t:6.2}s {name} gets up"),
            }
        }
    }
    println!("telegraphs {telegraphs}, slot waits {waits}, blocks {blocks}, hits {hits}");
    assert!(telegraphs > 0 && waits > 0 && blocks > 0 && hits > 0);
}
