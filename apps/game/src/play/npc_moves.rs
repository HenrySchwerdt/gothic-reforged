//! The engine side of the NPCs' script instructions: combat moves (swings, guard, jump back) and
//! the hit reactions. The ported `ZS_Attack_*` states queue `_AI_*` commands; the simulation hands
//! each `sAICombatMoveInstr` to the host as an [`EventKind::NpcMove`] and waits. This module does
//! what `hero_moves` does for the hero, for every shown character:
//!
//! * the clip of the move is named by the engine's rule (`sAICombatMoveStart`, G:0x1016abb0:
//!   `<Actor>_<AniState>_<Left>_<Right>_P0_<Action>_<Phase>_N_<Dir>`, `Dir` from the angle to the
//!   target, a missing clip is retried with `Fwd`, a missing `Fwd` clip ends the instruction),
//!   played at the instruction's speed (`sub_42a0`, `combat::commands::animation_speed`);
//! * a `Hit` clip calls the script's `OnHit` at 60 percent of its length
//!   (`sAICombatMoveItlLoop`, G:0x1016dd00, [`HIT_FRACTION`]): the damage of the move lands
//!   then (`Command::NpcMoveHit`);
//! * the instruction ends with the clip (`Command::NpcExecDone`); after a `Hit` clip the matching
//!   `Recover` clip plays on its own (`sAICombatMoveStartRecover`, G:0x1016e360).
//!
//! The length of every clip comes from the animation data, so the swing speed of an NPC is the
//! original's: clip length divided by the play speed of the move.
use super::anim::OverlayQueue;
use super::hero::stance_of;
use super::hero_moves::{HIT_FRACTION, direction_word, move_clip};
use super::{NpcActor, Roster};
use bevy::prelude::*;
use gothic_animation::{ActorLibrary, Animator, Locomotion, MountOverride, Stance, WeaponMount};
use gothic_sim::item::Use;
use gothic_runtime::{Command, CoreCommand, CoreEvent, CoreRes, Event};
use gothic_sim::script::enums::{Action, AniState};
use gothic_sim::script::host::MovePhase;
use gothic_sim::sim::EventKind;
use gothic_sim::types::{ActorId, AiMode};
use std::collections::HashSet;

/// What the animator of one NPC does for the script right now.
#[derive(Debug, Clone, PartialEq, Default)]
enum Run {
    #[default]
    Idle,
    /// A combat-move clip of an instruction in flight.
    Move {
        seq: u32,
        phase: MovePhase,
        hit_sent: bool,
    },
    /// The `Recover` clip that follows a `Hit` clip; nobody waits for it.
    Recover,
}

/// The script-driven animation state of one NPC body.
#[derive(Component, Debug, Default)]
pub struct NpcMover {
    run: Run,
    /// A script-owned clip is active: the locomotion mixer is out.
    owned: bool,
    /// The clip that plays (for the QA assertions and the log).
    pub clip: Option<String>,
    /// Action, pose and direction word of the last combat move (the `Recover` clip uses them).
    last: Option<(Action, AniState, &'static str)>,
    /// The move in flight is a cast.
    casting: bool,
}

/// Log of the clips the NPC movers started, newest last: `(time, actor, clip, speed)`.
#[derive(Resource, Default)]
pub struct NpcMoveLog {
    pub clips: Vec<(f32, ActorId, String, f32)>,
    /// Instructions that had no clip (ended at once).
    pub missing: Vec<(ActorId, String)>,
}

/// Bodies the host drives right now (the core was told with `HostDrivesNpc`).
#[derive(Resource, Default)]
struct Driven(HashSet<ActorId>);

pub fn register(app: &mut App) {
    app.init_resource::<NpcMoveLog>()
        .init_resource::<Driven>()
        .add_systems(
            Update,
            (announce_bodies, npc_mover, npc_root_motion, npc_weapon_mounts)
                .chain()
                .after(super::npcs::sync_npcs),
        );
}

/// Tell the core which bodies are on screen: their clips set the pace of their moves.
fn announce_bodies(
    mut driven: ResMut<Driven>,
    mut cmd: MessageWriter<CoreCommand>,
    q: Query<&NpcActor, With<Animator>>,
    mut commands: Commands,
    fresh: Query<(Entity, &NpcActor), (With<Animator>, Without<NpcMover>)>,
) {
    for (e, _) in &fresh {
        commands.entity(e).insert(NpcMover::default());
    }
    let now: HashSet<ActorId> = q.iter().map(|n| n.0).collect();
    for a in now.difference(&driven.0) {
        cmd.write(CoreCommand(Command::HostDrivesNpc {
            actor: *a,
            on: true,
        }));
    }
    for a in driven.0.difference(&now) {
        cmd.write(CoreCommand(Command::HostDrivesNpc {
            actor: *a,
            on: false,
        }));
    }
    if driven.0 != now {
        driven.0 = now;
    }
}

#[allow(clippy::too_many_arguments)]
fn npc_mover(
    mut events: MessageReader<CoreEvent>,
    roster: Res<Roster>,
    core: Res<CoreRes>,
    blendings: Res<super::Blendings>,
    mut lib: ResMut<ActorLibrary>,
    time: Res<Time>,
    mut log: ResMut<NpcMoveLog>,
    mut cmd: MessageWriter<CoreCommand>,
    mut commands: Commands,
    mut q: Query<(
        Entity,
        &NpcActor,
        &Transform,
        &mut Animator,
        &mut NpcMover,
        &super::anim::CombatStance,
        Option<&mut OverlayQueue>,
    )>,
) {
    let now = time.elapsed_secs();
    let start = |lib: &mut ActorLibrary,
                 blendings: &super::Blendings,
                 animator: &mut Animator,
                 mover: &mut NpcMover,
                 log: &mut NpcMoveLog,
                 actor: ActorId,
                 name: &str,
                 speed: f32|
     -> bool {
        let rig = animator.rig.clone();
        match lib.motion(&rig, name) {
            Ok(motion) => {
                let fade = animator.current_name().map_or(0.0, |old| {
                    gothic_animation::blend::script_fade(&blendings.0, old, name)
                });
                animator.overlay = None;
                animator.play(motion, fade, speed, 0.0);
                mover.clip = Some(name.to_string());
                log.clips.push((now, actor, name.to_string(), speed));
                true
            }
            Err(e) => {
                eprintln!("npc move {name}: {e:#}");
                false
            }
        }
    };
    for e in events.read() {
        let Event::Sim(kind) = &e.0 else { continue };
        match kind {
            EventKind::NpcMove {
                actor,
                seq,
                action,
                phase,
                speed,
                pose,
                target,
            } => {
                let Some(&entity) = roster.by_actor.get(actor) else {
                    cmd.write(CoreCommand(Command::NpcExecDone {
                        actor: *actor,
                        seq: *seq,
                    }));
                    continue;
                };
                let Ok((entity, _, tf, mut animator, mut mover, cs, overlay)) = q.get_mut(entity)
                else {
                    cmd.write(CoreCommand(Command::NpcExecDone {
                        actor: *actor,
                        seq: *seq,
                    }));
                    continue;
                };
                let family = animator.rig.family.clone();
                let sim_actor = core.0.sim.actor(*actor);
                let stance = match stance_of(&sim_actor.equipment) {
                    Stance::Unarmed => cs.0,
                    s => s,
                };
                let hands = stance.hands();
                let forward = {
                    let f = tf.rotation * Vec3::NEG_Z;
                    Vec2::new(f.x, f.z)
                };
                let to_target = target.and_then(|t| {
                    let p = core.0.sim.world.actors.get(&t)?.0;
                    let me = core.0.sim.world.actors.get(actor)?.0;
                    Some(Vec2::new(p.x - me.x, p.z - me.z))
                });
                let dir = direction_word(forward, to_target);
                let clip = move_clip(&lib, &family, *pose, hands, *action, *phase, dir);
                let ok = clip.as_deref().is_some_and(|n| {
                    start(
                        &mut lib,
                        &blendings,
                        &mut animator,
                        &mut mover,
                        &mut log,
                        *actor,
                        n,
                        *speed,
                    )
                });
                if ok {
                    if let Some(mut q) = overlay {
                        q.0.clear();
                    }
                    mover.owned = true;
                    mover.run = Run::Move {
                        seq: *seq,
                        phase: *phase,
                        hit_sent: false,
                    };
                    commands.entity(entity).remove::<Locomotion>();
                } else {
                    // The engine skips a move without a clip: the instruction ends at once and
                    // no blow lands (`sAICombatMoveStart` returns false).
                    log.missing.push((
                        *actor,
                        format!("{family} {action:?} {phase:?} {pose:?} {hands:?} {dir}"),
                    ));
                    cmd.write(CoreCommand(Command::NpcExecDone {
                        actor: *actor,
                        seq: *seq,
                    }));
                }
                mover.last = Some((*action, *pose, dir));
                mover.casting = matches!(action, Action::Cast | Action::PowerCast | Action::QuickCast);
            }
            EventKind::NpcStop { actor } => {
                if let Some(&entity) = roster.by_actor.get(actor)
                    && let Ok((_, _, _, _, mut mover, _, _)) = q.get_mut(entity)
                    && matches!(mover.run, Run::Move { .. })
                {
                    // `FullStop`: the instruction is dropped; the clip runs out.
                    mover.run = Run::Recover;
                }
            }
            _ => {}
        }
    }
    // ---- progress of the clips in flight ------------------------------------------------------
    for (entity, npc, _, mut animator, mut mover, cs, _) in &mut q {
        let down = {
            let a = core.0.sim.actor(npc.0);
            matches!(a.ai_mode, AiMode::Dead | AiMode::Down) || a.hp <= 0
        };
        if down {
            // The lying clips are `sync_npcs`'s business (`play_fall`); the mover steps aside.
            mover.run = Run::Idle;
            mover.owned = false;
            mover.clip = None;
            continue;
        }
        if mover.run == Run::Idle {
            if mover.owned {
                mover.owned = false;
                mover.clip = None;
                commands.entity(entity).insert(Locomotion::default());
            }
            continue;
        }
        let family = animator.rig.family.clone();
        let sim_actor = core.0.sim.actor(npc.0);
        let stance = match stance_of(&sim_actor.equipment) {
            Stance::Unarmed => cs.0,
            s => s,
        };
        let hands = stance.hands();
        let (time, length) = (
            animator.current_time(),
            animator.current_clip().map_or(0.0, |c| c.duration),
        );
        let finished = animator.finished() || (length > 0.0 && time >= length - 1e-3);
        match mover.run.clone() {
            Run::Move {
                seq,
                phase,
                hit_sent,
            } => {
                // A spell leaves when `_AI_Cast_Execute` begins, i.e. with its `Hit` clip; a blow
                // lands at 60 percent of its clip (`sAICombatMoveItlLoop`).
                let frac = if mover.casting { 0.0 } else { HIT_FRACTION };
                if phase == MovePhase::Hit
                    && !hit_sent
                    && length > 0.0
                    && time >= frac * length
                {
                    cmd.write(CoreCommand(Command::NpcMoveHit {
                        actor: npc.0,
                        seq,
                    }));
                    mover.run = Run::Move {
                        seq,
                        phase,
                        hit_sent: true,
                    };
                }
                if finished {
                    cmd.write(CoreCommand(Command::NpcExecDone {
                        actor: npc.0,
                        seq,
                    }));
                    if phase == MovePhase::Hit {
                        let recover = mover.last.and_then(|(a, p, d)| {
                            move_clip(&lib, &family, p, hands, a, MovePhase::Recover, d)
                        });
                        let played = recover.as_deref().is_some_and(|n| {
                            let rig = animator.rig.clone();
                            match lib.motion(&rig, n) {
                                Ok(m) => {
                                    animator.play(m, 0.0, 1.0, 0.0);
                                    mover.clip = Some(n.to_string());
                                    log.clips.push((now, npc.0, n.to_string(), 1.0));
                                    true
                                }
                                Err(_) => false,
                            }
                        });
                        mover.run = if played { Run::Recover } else { Run::Idle };
                    } else {
                        mover.run = Run::Idle;
                    }
                }
            }
            Run::Recover => {
                if finished {
                    mover.run = Run::Idle;
                }
            }
            Run::Idle => {}
        }
    }
}

/// Which of the weapons on an NPC's body are in its hands: the fighter that switched from the
/// sword to the bow (`_AI_SelectWeapon`, S:49600) has the bow in the left hand and the sword on
/// its back. The mounts are told apart by the slot they draw to (`Slot_RightHand_Weapon` melee
/// and staff, `Slot_LeftHand_Weapon` bow and crossbow, `Slot_LeftHand_Shield`); a character
/// without drawn weapons keeps the stance rule of the animation crate.
fn npc_weapon_mounts(
    core: Res<CoreRes>,
    mut commands: Commands,
    npcs: Query<(&NpcActor, &Animator, &Children)>,
    mounts: Query<(&WeaponMount, Option<&MountOverride>)>,
) {
    for (npc, animator, children) in &npcs {
        let actor = core.0.sim.actor(npc.0);
        let idx = |n: &str| animator.rig.index.get(n).copied();
        let (lh, rh, sh) = (
            idx("slot_lefthand_weapon"),
            idx("slot_righthand_weapon"),
            idx("slot_lefthand_shield"),
        );
        let eq = &actor.equipment;
        for child in children.iter() {
            let Ok((mount, current)) = mounts.get(child) else {
                continue;
            };
            let role = Some(mount.drawn_joint);
            let drawn = if !actor.drawn {
                None
            } else if role == lh {
                Some(matches!(eq.left_use(), Use::Bow | Use::CrossBow))
            } else if role == rh {
                Some(eq.right_use().is_melee_weapon() || eq.left_use() == Use::Staff)
            } else if role == sh {
                Some(eq.left_use() == Use::Shield)
            } else {
                None
            };
            match (drawn, current) {
                (Some(d), Some(c)) if c.0 == d => {}
                (Some(d), _) => {
                    commands.entity(child).insert(MountOverride(d));
                }
                (None, Some(_)) => {
                    commands.entity(child).remove::<MountOverride>();
                }
                (None, None) => {}
            }
        }
    }
}

/// While a combat clip plays the body travels with the clip's own root motion (the lunge of a
/// swing, the hop of a jump back); the simulation owns the position, so the delta goes to it.
fn npc_root_motion(
    mut cmd: MessageWriter<CoreCommand>,
    q: Query<(&NpcActor, &Animator, &NpcMover, &Transform)>,
) {
    for (npc, animator, mover, tf) in &q {
        if !mover.owned || mover.run == Run::Idle {
            continue;
        }
        let rm = animator.root_motion.translation;
        if rm.length_squared() < 1e-6 {
            continue;
        }
        let w = tf.rotation * rm;
        cmd.write(CoreCommand(Command::NpcNudge {
            actor: npc.0,
            dx: w.x,
            dz: w.z,
        }));
    }
}
