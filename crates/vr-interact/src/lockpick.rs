//! Physical lockpicking: hold the pick in the lock (grip) and turn the wrist left or right.
//!
//! A lock either has a known combination of turns (the host's lock data; a wrong turn breaks
//! the pick and reports `success: Some(false)`, the full combination opens it with
//! `Some(true)`) or none, in which case the first turn hands the roll to the script
//! (`success: None`, `_AI_Unlock`). With the key the lock opens at once.
use glam::{Quat, Vec3};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Turn {
    Left,
    Right,
}

/// What the host knows about a lock.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct LockSpec {
    pub locked: bool,
    /// The hero carries the key.
    pub has_key: bool,
    /// The turns that open it, if the host has them.
    pub combination: Option<Vec<Turn>>,
}

impl LockSpec {
    pub fn combination(turns: &[Turn]) -> Self {
        Self {
            locked: true,
            has_key: false,
            combination: Some(turns.to_vec()),
        }
    }
}

/// Result of feeding one frame to a [`LockSession`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LockStep {
    Continue,
    /// A correct turn, more to go.
    Turned(Turn),
    /// The lock decided: the value goes into `Command::Lockpick { success }`.
    Done(Option<bool>),
    /// The hand let go or left the lock.
    Aborted,
}

/// One attempt at a lock.
#[derive(Debug, Clone, PartialEq)]
pub struct LockSession {
    pub object: u32,
    pub spec: LockSpec,
    /// Hand orientation and position when the pick went in.
    start: Quat,
    anchor: Vec3,
    progress: usize,
    /// Back at neutral since the last turn.
    neutral: bool,
}

/// Wrist roll about the hand's forward axis relative to `start`, radians (positive = left,
/// counter-clockwise as the player sees it).
pub fn roll(start: Quat, cur: Quat) -> f32 {
    let rel = (start.inverse() * cur).normalize();
    // Twist about local Z (the hand's -Z points forward).
    let a = 2.0 * rel.z.atan2(rel.w);
    if a > std::f32::consts::PI {
        a - std::f32::consts::TAU
    } else if a < -std::f32::consts::PI {
        a + std::f32::consts::TAU
    } else {
        a
    }
}

impl LockSession {
    pub fn new(object: u32, spec: LockSpec, hand: Quat, at: Vec3) -> Self {
        Self {
            object,
            spec,
            start: hand,
            anchor: at,
            progress: 0,
            neutral: true,
        }
    }

    pub fn progress(&self) -> usize {
        self.progress
    }

    /// One frame: `held` = the grip is still closed, `hand` the hand's rotation and position.
    pub fn step(
        &mut self,
        held: bool,
        hand: Quat,
        at: Vec3,
        turn: f32,
        neutral: f32,
        leash: f32,
    ) -> LockStep {
        if !held || at.distance(self.anchor) > leash {
            return LockStep::Aborted;
        }
        let r = roll(self.start, hand);
        if r.abs() < neutral {
            self.neutral = true;
            return LockStep::Continue;
        }
        if !self.neutral || r.abs() < turn {
            return LockStep::Continue;
        }
        self.neutral = false;
        let t = if r > 0.0 { Turn::Left } else { Turn::Right };
        match &self.spec.combination {
            None => LockStep::Done(None),
            Some(c) if c.get(self.progress) == Some(&t) => {
                self.progress += 1;
                if self.progress == c.len() {
                    LockStep::Done(Some(true))
                } else {
                    LockStep::Turned(t)
                }
            }
            Some(_) => LockStep::Done(Some(false)),
        }
    }
}
