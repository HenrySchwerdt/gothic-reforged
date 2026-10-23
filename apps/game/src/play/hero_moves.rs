//! The engine side of the hero's script instructions: combat moves and the weapon hold.
//!
//! The ported `PS_Melee*` states queue `_AI_*` instructions; the simulation hands each one to the
//! host as an event (`EventKind::HeroMove`, `EventKind::HeroHold`) and waits. This module is the
//! port of what `Game.dll`'s script processing unit does with them:
//!
//! * `gCScriptProcessingUnit::sAICombatMoveStart` (G:0x1016abb0): the clip of the move is named
//!   `<Actor>_<AniState>_<Left>_<Right>_P0_<Action>_<Phase>_N_<Dir>_00_%...` with the use types of
//!   the hand items, `Dir` from the angle to the target (`Fwd`, or `Left` / `Right` beyond 45
//!   degrees; a missing clip is retried with `Fwd`, a missing `Fwd` clip ends the instruction at
//!   once); it takes over the primary motion slot (the locomotion mixer is out until it is
//!   done) and plays the clip at the instruction's speed.
//! * `sAICombatMoveItlLoop` (G:0x1016dd00): a `Hit` clip calls the script's `OnHit` when it has
//!   played 60 % of its length ([`HIT_FRACTION`]); the damage of the move happens then.
//! * `sAICombatMoveInstr` / `sAICombatMoveStartRecover` (G:0x101696e0, G:0x1016e360): when a
//!   `Hit` clip ends the instruction is finished and the matching `Recover` clip plays on its own
//!   (the next instruction and the next key do not wait for it).
//! * `_AI_HoldInventoryItems_Base` (S:44d30): the `Begin` clip of the `Hold` action plays, the
//!   item moves between the back slot and the hand (`PSInventory::HoldStacks`), the `End` clip
//!   plays.
//!
//! Not ported: `EnableCombatMovementFromSPU`'s steering toward the target (the move's own root
//! motion is applied instead), the `_O_` overlay variants of the hold clips.
use super::anim::OverlayQueue;
use super::hero::stance_of;
use super::{HeroActor, PlayState};
use crate::walk::{Collision, Player};
use bevy::prelude::*;
use gothic_animation::names::MotionQuery;
use gothic_animation::{ActorLibrary, Animator, Locomotion};
use gothic_runtime::{Command, CoreCommand, CoreEvent, CoreRes, Event};
use gothic_sim::script::enums::{Action, AniState};
use gothic_sim::script::host::MovePhase;
use gothic_sim::sim::EventKind;
use gothic_sim::types::ActorId;

/// `OnHit` fires when the `Hit` clip reached this fraction of its length (G:0x10308308).
pub const HIT_FRACTION: f32 = gothic_sim::script::player::melee::HIT_FRACTION;
/// The angle beyond which a move against a target uses the `Left` / `Right` clip: the cosine of
/// 45 degrees (`_DAT_10308538`).
const SIDE_COS: f32 = 0.7071;

/// What the hero's animator does for the script right now.
#[derive(Debug, Clone, PartialEq)]
enum Run {
    Idle,
    /// A combat-move clip of an instruction in flight.
    Move {
        seq: u32,
        phase: MovePhase,
        hit_sent: bool,
    },
    /// The `Recover` clip that follows a `Hit` clip; nobody waits for it.
    Recover,
    /// The hold: `Begin` clip, then the swap, then the `End` clip.
    Hold {
        seq: u32,
        draw: bool,
        ended_begin: bool,
    },
}

/// The hero's script-driven animation state.
#[derive(Resource)]
pub struct HeroMover {
    run: Run,
    /// The name of the clip that plays (for the QA assertions and the log).
    pub clip: Option<String>,
    /// The `Action` and `Phase` of the instruction in flight.
    pub current: Option<(Action, MovePhase)>,
    /// The instruction in flight is a combat move (movement keys are ignored while it plays).
    pub moving: bool,
    /// Log of the clips started, newest last: `(clip, speed)`.
    pub log: Vec<(String, f32)>,
    /// A script-owned clip is active: the locomotion mixer is out.
    owned: bool,
    /// Action, pose and direction word of the last combat move (the `Recover` clip uses them).
    last_query: Option<(Action, AniState, &'static str)>,
}

impl Default for HeroMover {
    fn default() -> Self {
        Self {
            run: Run::Idle,
            clip: None,
            current: None,
            moving: false,
            log: Vec::new(),
            owned: false,
            last_query: None,
        }
    }
}

/// The host plays the hero's clips and reports their moments back to the simulation.
fn announce(mut cmd: MessageWriter<CoreCommand>) {
    cmd.write(CoreCommand(Command::HostDrivesHero(true)));
}

pub fn register(app: &mut App) {
    app.init_resource::<HeroMover>()
        .add_systems(PostStartup, announce)
        .add_systems(
            Update,
            (hero_mover, move_root_motion)
                .chain()
                .after(super::hero::sync),
        );
}

/// The direction word of the clip name (`sAICombatMoveStart`): the move's direction vector is the
/// one toward the target, else the hero's own forward axis; beyond 45 degrees off the facing the
/// word is `Left` or `Right` (the sign of the cross product), else `Fwd`.
pub fn direction_word(forward: Vec2, to_target: Option<Vec2>) -> &'static str {
    let Some(d) = to_target.and_then(Vec2::try_normalize) else {
        return "Fwd";
    };
    let f = forward.normalize_or_zero();
    if f.dot(d) >= SIDE_COS {
        return "Fwd";
    }
    // Bevy's frame: +Y up, the hero looks along -Z; the left of a facing `f` (x, z) is the cross
    // product's positive y: f x d = f.x * d.y - f.y * d.x with the xz plane mapped to (x, y).
    let cross = f.x * d.y - f.y * d.x;
    if cross > 0.0 { "Right" } else { "Left" }
}

/// The clip of a combat move, strictly by the engine's naming rule. `dir` falls back to `Fwd`
/// (the engine rewrites the eighth word and retries); anything else missing means no clip.
pub fn move_clip(
    lib: &ActorLibrary,
    family: &str,
    pose: AniState,
    hands: (&str, &str),
    action: Action,
    phase: MovePhase,
    dir: &str,
) -> Option<String> {
    let pose = match pose {
        AniState::Parade => "Parade",
        _ => "Stand",
    };
    // The pose token `Pn` of the clip name is the primary pose of the NPC
    // (`gCNPC_PS::GetPrimaryPoseExt`, state at +0x1a0): `P0` for the first clip of a move. The
    // pose the state machine reached after a `Hit` clip (the clip's `Pm`) is not tracked, so
    // the `Recover` clip, which the catalog names with the pose the `Hit` ended in, is searched
    // through all pose tokens [I].
    let poses: &[&str] = if phase == MovePhase::Recover {
        &["P0", "P1", "P2", "P3"]
    } else {
        &["P0"]
    };
    for d in [dir, "Fwd"] {
        for pn in poses {
            let q = MotionQuery {
                pose: Some(pose),
                left: Some(hands.0),
                right: Some(hands.1),
                pn: Some(pn),
                action: Some(action.name()),
                phase: Some(phase.token()),
                mode: Some("N"),
                dir: Some(d),
            };
            if let Some(n) = lib.catalog.find(family, &q) {
                return Some(n.to_string());
            }
        }
    }
    None
}

/// The `Hold` clips (`GetAni(entity, Hold, ...)`): `HoldRight` for everything but the bow, which
/// is held with the left hand (`Hold`). `Begin` and `End` of the weapon's hand combination.
fn hold_clip(lib: &ActorLibrary, family: &str, hands: (&str, &str), phase: &str) -> Option<String> {
    for action in ["HoldRight", "Hold"] {
        let q = MotionQuery {
            pose: Some("Stand"),
            left: Some(hands.0),
            right: Some(hands.1),
            pn: Some("P0"),
            action: Some(action),
            phase: Some(phase),
            mode: Some("N"),
            ..Default::default()
        };
        if let Some(n) = lib.catalog.find(family, &q) {
            return Some(n.to_string());
        }
    }
    None
}

#[allow(clippy::too_many_arguments)]
fn hero_mover(
    mut events: MessageReader<CoreEvent>,
    mut mover: ResMut<HeroMover>,
    mut lib: ResMut<ActorLibrary>,
    blendings: Res<super::Blendings>,
    core: Res<CoreRes>,
    player: Res<Player>,
    mut commands: Commands,
    mut cmd: MessageWriter<CoreCommand>,
    mut hero: Query<(Entity, &mut Animator, Option<&mut OverlayQueue>), With<HeroActor>>,
) {
    let Ok((entity, mut animator, mut overlay)) = hero.single_mut() else {
        return;
    };
    let family = animator.rig.family.clone();
    let hands = stance_of(&core.0.sim.actor(core.0.hero).equipment).hands();
    let mut start = |mover: &mut HeroMover,
                     lib: &mut ActorLibrary,
                     animator: &mut Animator,
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
                mover.log.push((name.to_string(), speed));
                true
            }
            Err(e) => {
                eprintln!("hero move {name}: {e:#}");
                false
            }
        }
    };
    for e in events.read() {
        let Event::Sim(kind) = &e.0 else { continue };
        match kind {
            EventKind::HeroMove {
                seq,
                action,
                phase,
                speed,
                pose,
                target,
            } => {
                let hero_pos = player.ctrl.position;
                let forward = Vec2::new(-player.ctrl.heading.sin(), -player.ctrl.heading.cos());
                let to_target = target.and_then(|t| actor_dir(&core, t, hero_pos));
                let dir = direction_word(forward, to_target);
                let clip = move_clip(&lib, &family, *pose, hands, *action, *phase, dir);
                let ok = clip
                    .as_deref()
                    .is_some_and(|n| start(&mut mover, &mut lib, &mut animator, n, *speed));
                if ok {
                    if let Some(q) = overlay.as_mut() {
                        q.0.clear();
                    }
                    mover.owned = true;
                    mover.moving = true;
                    mover.current = Some((*action, *phase));
                    mover.run = Run::Move {
                        seq: *seq,
                        phase: *phase,
                        hit_sent: false,
                    };
                    commands.entity(entity).remove::<Locomotion>();
                } else {
                    // The engine skips a move without a clip: the instruction ends at once and
                    // no blow lands (`sAICombatMoveStart` returns false).
                    eprintln!(
                        "hero move: no clip for {action:?} {phase:?} {pose:?} {hands:?} dir {dir}"
                    );
                    cmd.write(CoreCommand(Command::HeroExecDone { seq: *seq }));
                }
                mover.last_query = Some((*action, *pose, dir));
            }
            EventKind::HeroHold { seq, draw } => {
                let name = hold_clip(&lib, &family, hands, "Begin");
                let ok = name
                    .as_deref()
                    .is_some_and(|n| start(&mut mover, &mut lib, &mut animator, n, 1.0));
                if ok {
                    mover.owned = true;
                    mover.moving = false;
                    mover.current = None;
                    mover.run = Run::Hold {
                        seq: *seq,
                        draw: *draw,
                        ended_begin: false,
                    };
                    commands.entity(entity).remove::<Locomotion>();
                } else {
                    // No hold clip: the item changes slots at once.
                    cmd.write(CoreCommand(Command::HeroHoldSwap { seq: *seq }));
                    cmd.write(CoreCommand(Command::HeroExecDone { seq: *seq }));
                }
            }
            EventKind::HeroStop => {
                // `FullStop`: the instruction is dropped; the clip is left to run out and the
                // locomotion mixer takes over when the next instruction does not start.
                if matches!(mover.run, Run::Move { .. } | Run::Hold { .. }) {
                    mover.run = Run::Recover;
                    mover.moving = false;
                }
            }
            _ => {}
        }
    }
    // ---- progress of the clip in flight ------------------------------------------------------
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
            if phase == MovePhase::Hit && !hit_sent && length > 0.0 && time >= HIT_FRACTION * length
            {
                cmd.write(CoreCommand(Command::HeroMoveHit { seq }));
                mover.run = Run::Move {
                    seq,
                    phase,
                    hit_sent: true,
                };
            }
            if finished {
                cmd.write(CoreCommand(Command::HeroExecDone { seq }));
                mover.moving = false;
                if phase == MovePhase::Hit {
                    // `sAICombatMoveStartRecover`: the recovery plays by itself.
                    let recover = mover.last_query.and_then(|(a, p, d)| {
                        move_clip(&lib, &family, p, hands, a, MovePhase::Recover, d)
                    });
                    let played = recover
                        .as_deref()
                        .is_some_and(|n| start(&mut mover, &mut lib, &mut animator, n, 1.0));
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
        Run::Hold {
            seq,
            draw,
            ended_begin,
        } => {
            if finished {
                if !ended_begin {
                    cmd.write(CoreCommand(Command::HeroHoldSwap { seq }));
                    let end = hold_clip(&lib, &family, hands, "End");
                    let ok = end
                        .as_deref()
                        .is_some_and(|n| start(&mut mover, &mut lib, &mut animator, n, 1.0));
                    mover.run = if ok {
                        Run::Hold {
                            seq,
                            draw,
                            ended_begin: true,
                        }
                    } else {
                        cmd.write(CoreCommand(Command::HeroExecDone { seq }));
                        Run::Idle
                    };
                } else {
                    cmd.write(CoreCommand(Command::HeroExecDone { seq }));
                    mover.run = Run::Idle;
                }
            }
        }
        Run::Idle => {}
    }
    if mover.run == Run::Idle && mover.owned {
        mover.owned = false;
        mover.moving = false;
        mover.current = None;
        mover.clip = None;
        commands.entity(entity).insert(Locomotion::default());
    }
}

/// Direction on the ground from the hero to an actor, in the xz plane mapped to `(x, y)`.
fn actor_dir(core: &CoreRes, target: ActorId, hero: Vec3) -> Option<Vec2> {
    let p = core.0.sim.world.actors.get(&target)?.0;
    Some(Vec2::new(p.x - hero.x, p.z - hero.z))
}

/// While a combat-move clip plays the hero travels with the clip's own root motion (the nominal
/// speed in the clip name, `Fwd`, `Left`, ... as the clip's direction).
fn move_root_motion(
    mover: Res<HeroMover>,
    mut player: ResMut<Player>,
    collision: Res<Collision>,
    state: Res<PlayState>,
    hero: Query<(&Animator, &Transform), With<HeroActor>>,
) {
    if !mover.owned || mover.run == Run::Idle {
        return;
    }
    let Ok((animator, tf)) = hero.single() else {
        return;
    };
    let rm = animator.root_motion.translation;
    if rm.length_squared() < 1e-6 {
        return;
    }
    let world = Quat::from_rotation_y(state.hero_yaw) * rm;
    let _ = tf;
    player
        .ctrl
        .nudge(&collision.0, Vec3::new(world.x, 0.0, world.z));
}
