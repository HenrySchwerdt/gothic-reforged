//! Scripted base-layer poses: work-station animations, sitting, sleeping, kneeling at a chest.
//!
//! The ported script states (`PS_Interact_*`, `ZS_Smith`, `ZS_Sleep`, ...) name an *action*
//! (`Anvil`, `Cauldron`, `SitStool`, `OpenChest`, ...); the installed motion catalog holds the
//! clips under the same names (`Hero_Stand_..._Anvil_Begin`, `Hero_SitStool_..._Ambient_Loop`).
//! A [`Posed`] character leaves the locomotion state machine, plays Begin -> Loop -> End of the
//! action and returns to locomotion when it is done (or released).
use super::anim::find_motion;
use bevy::prelude::*;
use gothic_animation::{ActorLibrary, Animator, Locomotion, LocomotionInput, Stance};
use std::collections::VecDeque;

/// Actions whose clips are poses of their own (the character stays in the pose until it is
/// released): `<Stand>_<Action>_Begin` enters, `<Action>_Ambient_Loop` holds, `<Action>_Stand_Begin`
/// leaves.
/// Actions played kneeling (`Kneel` pose).
pub const KNEEL_ACTIONS: &[&str] = &["OpenChest", "Lockpick"];

pub const POSE_ACTIONS: &[&str] = &[
    "SitStool",
    "SitBench",
    "SitGround",
    "SitThrone",
    "SleepBed",
    "SleepGround",
    "Kneel",
];

#[derive(Clone, Debug)]
struct Step {
    motion: String,
    /// The clip loops (ends by time or release, not by itself).
    looping: bool,
}

/// A character in a scripted pose (replaces its locomotion while present).
#[derive(Component, Debug)]
pub struct Posed {
    steps: VecDeque<Step>,
    exit: Vec<Step>,
    /// After a timed action inside a seat or bed pose the character settles back into the pose.
    hold_after: Option<Step>,
    /// The loop step ends after this many seconds (`None`: held until released).
    loop_secs: Option<f32>,
    current: Option<(bool, f32)>,
    /// Release requested: run the exit steps and go back to locomotion.
    releasing: bool,
    started: bool,
    pub action: String,
}

impl Posed {
    /// Ask the pose to end: the leaving clip (if any) plays, then locomotion resumes.
    pub fn release(&mut self) {
        if !self.releasing {
            self.releasing = true;
            self.steps.clear();
            self.steps.extend(self.exit.drain(..));
            self.current = None;
        }
    }

    pub fn is_releasing(&self) -> bool {
        self.releasing
    }
}

fn catalog_name(
    lib: &ActorLibrary,
    family: &str,
    stance: Stance,
    pose: &str,
    action: &str,
    phase: &str,
) -> Option<String> {
    find_motion(lib, family, stance, pose, action, phase)
}

/// Build the pose of an action for a character of `family`; `None` when the catalog has no clip
/// for it. `secs` is the loop time of a work animation, `None` holds until released. `in_pose`
/// is the seat / bed pose the character is in (a campfire sitter frying meat plays the
/// `SitGround` `Fry` clips and settles back into the sitting loop).
pub fn build(
    lib: &ActorLibrary,
    family: &str,
    stance: Stance,
    action: &str,
    secs: Option<f32>,
    in_pose: Option<&str>,
) -> Option<Posed> {
    let mut steps = VecDeque::new();
    let mut exit = Vec::new();
    let mut hold_after = None;
    let is_pose = POSE_ACTIONS.contains(&action);
    let step = |m: String, looping: bool| Step { motion: m, looping };
    let mut label = action.to_string();
    if is_pose {
        if let Some(m) = catalog_name(lib, family, stance, "Stand", action, "Begin") {
            steps.push_back(step(m, false));
        }
        let hold = catalog_name(lib, family, stance, action, "Ambient", "Loop")?;
        steps.push_back(step(hold, true));
        if let Some(m) = catalog_name(lib, family, stance, action, "Stand", "Begin") {
            exit.push(step(m, false));
        }
    } else if let Some(pose) = in_pose {
        let begin = catalog_name(lib, family, stance, pose, action, "Begin");
        let looped = catalog_name(lib, family, stance, pose, action, "Loop")
            .or_else(|| catalog_name(lib, family, stance, pose, action, "Ambient"));
        if begin.is_none() && looped.is_none() {
            return None;
        }
        if let Some(m) = begin {
            steps.push_back(step(m, false));
        }
        if let Some(m) = looped {
            steps.push_back(step(m, true));
        }
        if let Some(m) = catalog_name(lib, family, stance, pose, action, "End") {
            steps.push_back(step(m, false));
        }
        hold_after =
            catalog_name(lib, family, stance, pose, "Ambient", "Loop").map(|m| step(m, true));
        if let Some(m) = catalog_name(lib, family, stance, pose, "Stand", "Begin") {
            exit.push(step(m, false));
        }
        label = pose.to_string();
    } else {
        // Kneeling actions (`OpenChest`, `Lockpick`) have their own pose.
        let pose = if KNEEL_ACTIONS.contains(&action) {
            "Kneel"
        } else {
            "Stand"
        };
        let begin = catalog_name(lib, family, stance, pose, action, "Begin");
        let looped = catalog_name(lib, family, stance, pose, action, "Loop")
            .or_else(|| catalog_name(lib, family, stance, pose, action, "Ambient"));
        if begin.is_none() && looped.is_none() {
            return None;
        }
        if let Some(m) = begin {
            steps.push_back(step(m, false));
        }
        if let Some(m) = looped {
            steps.push_back(step(m, true));
        }
        if let Some(m) = catalog_name(lib, family, stance, pose, action, "End") {
            exit.push(step(m, false));
        }
    }
    Some(Posed {
        steps,
        exit,
        hold_after,
        loop_secs: secs,
        current: None,
        releasing: false,
        started: false,
        action: label,
    })
}

/// Drives every [`Posed`] character: starts the next clip, ends timed loops, restores locomotion.
pub fn pose_system(
    time: Res<Time>,
    blendings: Res<super::Blendings>,
    mut lib: ResMut<ActorLibrary>,
    mut commands: Commands,
    mut q: Query<(Entity, &mut Animator, &mut Posed)>,
) {
    let now = time.elapsed_secs();
    for (entity, mut animator, mut p) in &mut q {
        if !p.started {
            p.started = true;
            commands
                .entity(entity)
                .remove::<(Locomotion, LocomotionInput)>();
        }
        animator.overlay = None;
        let done = match p.current {
            None => true,
            Some((true, deadline)) => p.loop_secs.is_some() && now >= deadline,
            Some((false, _)) => animator.finished(),
        };
        if !done {
            continue;
        }
        let timed_loop_ended = p.current.is_some_and(|(looping, _)| looping)
            && p.loop_secs.is_some()
            && p.hold_after.is_none()
            && p.steps.is_empty();
        if timed_loop_ended {
            // A timed loop ended: leave through the exit clip.
            p.release();
        }
        let mut settled = false;
        let next = p.steps.pop_front().or_else(|| {
            // A timed action inside a seat pose ends in the pose's own hold loop.
            if p.releasing {
                None
            } else {
                settled = true;
                p.hold_after.take()
            }
        });
        let Some(step) = next else {
            commands
                .entity(entity)
                .remove::<Posed>()
                .insert((Locomotion::default(), LocomotionInput::default()));
            continue;
        };
        if settled {
            // The settled hold loop of a pose has no end of its own.
            p.loop_secs = None;
        }
        let rig = animator.rig.clone();
        match lib.motion(&rig, &step.motion) {
            Ok(motion) => {
                let fade = animator.current_name().map_or(0.15, |old| {
                    gothic_animation::blend::script_fade(&blendings.0, old, &step.motion)
                });
                animator.play(motion, fade, 1.0, 0.0);
                let deadline = now + p.loop_secs.unwrap_or(f32::INFINITY);
                p.current = Some((step.looping, deadline));
            }
            Err(e) => {
                eprintln!("pose {}: {e:#}", step.motion);
                p.current = Some((false, now));
            }
        }
    }
}
