//! Lockpicking with two hands: the off hand holds the tension wrench in the lock (grip held,
//! a slight twist that keeps the cylinder under load), the main hand turns the pick
//! ([`LockSession`](crate::lockpick::LockSession)). A turn only counts while the tension is on; letting go of
//! the wrench or twisting it too far (over-torque) snaps the pick (`success: Some(false)`),
//! and a lock without a known combination still hands the roll to the script (`None`).
//! The result becomes the same `Command::Lockpick { object, success }` the one-handed
//! version sends.
use crate::lockpick::{LockSession, LockSpec, LockStep, roll};
use glam::{Quat, Vec3};

/// Tension twist range, radians: below `MIN` there is no load, above `MAX` the pick snaps.
pub const TENSION_MIN: f32 = 0.10;
pub const TENSION_MAX: f32 = 0.60;

#[derive(Debug, Clone, PartialEq)]
pub struct TwoHandLock {
    pub pick: LockSession,
    tension_start: Quat,
    anchor: Vec3,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum TwoHandStep {
    Continue {
        /// 0..1 how much load the wrench puts on the lock (feedback: sound and a creak).
        load: f32,
    },
    /// Turn registered (load was on).
    Turned,
    /// Decision for `Command::Lockpick { success }`.
    Done(Option<bool>),
    /// A hand left the lock.
    Aborted,
}

impl TwoHandLock {
    /// `pick_hand` / `tension_hand`: rotation and position when both tools went in.
    pub fn new(object: u32, spec: LockSpec, pick_hand: Quat, pick_at: Vec3, tension_hand: Quat, tension_at: Vec3) -> Self {
        Self {
            pick: LockSession::new(object, spec, pick_hand, pick_at),
            tension_start: tension_hand,
            anchor: tension_at,
        }
    }

    #[allow(clippy::too_many_arguments)]
    pub fn step(
        &mut self,
        pick_held: bool,
        pick_hand: Quat,
        pick_at: Vec3,
        tension_held: bool,
        tension_hand: Quat,
        tension_at: Vec3,
        turn: f32,
        neutral: f32,
        leash: f32,
    ) -> TwoHandStep {
        if !pick_held || !tension_held || tension_at.distance(self.anchor) > leash {
            // Dropping the wrench with the pick in the lock springs the pins back: just abort.
            return TwoHandStep::Aborted;
        }
        let twist = roll(self.tension_start, tension_hand).abs();
        if twist > TENSION_MAX {
            return TwoHandStep::Done(Some(false));
        }
        let load = ((twist - TENSION_MIN) / (TENSION_MAX - TENSION_MIN)).clamp(0.0, 1.0);
        let loaded = twist >= TENSION_MIN;
        // Without load the pick turns freely and nothing registers: feed it the neutral pose.
        let pick_rot = if loaded { pick_hand } else { self.pick_start_rot() };
        match self.pick.step(true, pick_rot, pick_at, turn, neutral, leash) {
            LockStep::Continue => TwoHandStep::Continue { load },
            LockStep::Turned(_) => TwoHandStep::Turned,
            LockStep::Done(r) => TwoHandStep::Done(r),
            LockStep::Aborted => TwoHandStep::Aborted,
        }
    }

    fn pick_start_rot(&self) -> Quat {
        self.pick.start_rotation()
    }
}
