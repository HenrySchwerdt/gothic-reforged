//! Locomotion state machine for humanoids.
//!
//! The physics / character controller writes a [`LocomotionInput`]; [`Locomotion::step`]
//! turns it into a [`LocoKey`] (what should play) and a ground speed (for playback-rate
//! sync). [`resolve_motion`] maps a key to a concrete motion name of the installed
//! `_compiledAnimation.pak` through the name grammar in [`crate::names`].
//!
//! Mapping used for the hero set (`Hero_<Pose>_<Left>_<Right>_P0_<Action>_<Phase>_N_<Dir>_...`):
//!
//! | state | pose | action / phase | direction |
//! | --- | --- | --- | --- |
//! | Idle | `Stand` / `Sneak` | `Ambient` / `Loop` (fallback `Move` / `Stand`) | `Fwd` |
//! | Walk | `Stand` / `Sneak` | `Move` / `Walk` | `Fwd` `Back` `Left` `Right` |
//! | Run | `Stand` | `Move` / `Run` (fallback Walk) | `Fwd` `Back` `Left` `Right` |
//! | Sprint | `Stand` | `Move` / `Sprint` (fallback Run) | `Fwd` |
//! | TurnLeft/Right | `Stand` / `Sneak` | `Move` / `Turn90` | `Left` `Right` |
//! | Jump | `Stand` | `Jump` / `Stand` (one shot) | `Fwd` |
//! | Fall | `Stand` | `Fall` / `Loop` | `Fwd` |
//! | Land | `Stand` | `Fall` / `End` (fallback `Recover`) | `Fwd` |
//!
//! `Left`/`Right` hands per [`Stance`]: Unarmed `None/None`, Fist `None/Fist`, OneHand
//! `None/1H`, OneHandShield `Shield/1H`, TwoHand `None/2H`, Bow `Bow/Arrow`, CrossBow
//! `CrossBow/Bolt`, Staff `None/Staff`, Magic `None/Cast`, Torch `Torch/None`. Missing
//! combinations fall back to `None/None`.
use crate::names::{MotionCatalog, MotionQuery};
use bevy::{ecs::component::Component, math::Vec3};

#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug, Default)]
pub enum Stance {
    #[default]
    Unarmed,
    Fist,
    OneHand,
    OneHandShield,
    TwoHand,
    Bow,
    CrossBow,
    Staff,
    Magic,
    Torch,
}

impl Stance {
    /// `(left hand, right hand)` tokens of the motion names.
    pub fn hands(self) -> (&'static str, &'static str) {
        match self {
            Stance::Unarmed => ("None", "None"),
            Stance::Fist => ("None", "Fist"),
            Stance::OneHand => ("None", "1H"),
            Stance::OneHandShield => ("Shield", "1H"),
            Stance::TwoHand => ("None", "2H"),
            Stance::Bow => ("Bow", "Arrow"),
            Stance::CrossBow => ("CrossBow", "Bolt"),
            Stance::Staff => ("None", "Staff"),
            Stance::Magic => ("None", "Cast"),
            Stance::Torch => ("Torch", "None"),
        }
    }

    /// Whether a weapon is held in the right hand (drives equipment attachment).
    pub fn weapon_drawn(self) -> bool {
        !matches!(self, Stance::Unarmed | Stance::Torch)
    }
}

/// Written by the controller (or AI), read by the locomotion system.
#[derive(Component, Clone, Debug)]
pub struct LocomotionInput {
    /// World-space velocity in centimetres per second.
    pub velocity: Vec3,
    pub grounded: bool,
    pub stance: Stance,
    pub sneak: bool,
    /// Signed yaw from the current facing to the desired facing, radians, positive is
    /// counter-clockwise seen from above (left). Triggers turn-in-place when standing.
    pub turn_error: f32,
}

impl Default for LocomotionInput {
    fn default() -> Self {
        Self {
            velocity: Vec3::ZERO,
            grounded: true,
            stance: Stance::Unarmed,
            sneak: false,
            turn_error: 0.0,
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum MoveKind {
    Idle,
    Walk,
    Run,
    Sprint,
    TurnLeft,
    TurnRight,
    Jump,
    Fall,
    Land,
}

#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum MoveDir {
    Fwd,
    Back,
    Left,
    Right,
}

impl MoveDir {
    fn token(self) -> &'static str {
        match self {
            MoveDir::Fwd => "Fwd",
            MoveDir::Back => "Back",
            MoveDir::Left => "Left",
            MoveDir::Right => "Right",
        }
    }
}

/// What the animation should currently show.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub struct LocoKey {
    pub kind: MoveKind,
    pub dir: MoveDir,
    pub stance: Stance,
    pub sneak: bool,
}

#[derive(Clone, Debug)]
pub struct LocomotionConfig {
    /// Below this ground speed (cm/s) the character stands.
    pub walk_enter: f32,
    pub walk_exit: f32,
    pub run_enter: f32,
    pub run_exit: f32,
    pub sprint_enter: f32,
    pub sprint_exit: f32,
    /// Upward speed that turns an airborne start into a jump instead of a fall.
    pub jump_speed: f32,
    /// Heading error that starts a turn in place, radians.
    pub turn_enter: f32,
    pub fade: f32,
    pub min_rate: f32,
    pub max_rate: f32,
}

impl Default for LocomotionConfig {
    fn default() -> Self {
        Self {
            walk_enter: 20.0,
            walk_exit: 10.0,
            run_enter: 280.0,
            run_exit: 240.0,
            sprint_enter: 460.0,
            sprint_exit: 420.0,
            jump_speed: 120.0,
            turn_enter: 0.9,
            fade: crate::animator::DEFAULT_FADE,
            min_rate: 0.5,
            max_rate: 1.7,
        }
    }
}

/// Velocity components in the character's frame (cm/s).
#[derive(Clone, Copy, Debug, Default)]
pub struct LocalMotion {
    pub forward: f32,
    pub right: f32,
    pub up: f32,
}

#[derive(Component, Clone, Debug)]
pub struct Locomotion {
    pub config: LocomotionConfig,
    pub key: LocoKey,
    /// Ground speed of the latest step (cm/s).
    pub ground_speed: f32,
    /// Key whose clip was last started on the animator (managed by the runtime system).
    pub playing: Option<LocoKey>,
}

impl Default for Locomotion {
    fn default() -> Self {
        Self::new(LocomotionConfig::default())
    }
}

impl Locomotion {
    pub fn new(config: LocomotionConfig) -> Self {
        Self {
            config,
            key: LocoKey {
                kind: MoveKind::Idle,
                dir: MoveDir::Fwd,
                stance: Stance::Unarmed,
                sneak: false,
            },
            ground_speed: 0.0,
            playing: None,
        }
    }

    /// Advances the state machine. `clip_finished` reports that the one-shot clip of the
    /// current state (jump, land, turn) has ended.
    pub fn step(&mut self, input: &LocomotionInput, v: LocalMotion, clip_finished: bool) -> LocoKey {
        let c = &self.config;
        let prev = self.key;
        let speed = v.forward.hypot(v.right);
        self.ground_speed = speed;
        let mut next = LocoKey {
            stance: input.stance,
            sneak: input.sneak,
            ..prev
        };
        if !input.grounded {
            next.dir = MoveDir::Fwd;
            next.kind = match prev.kind {
                MoveKind::Jump if !clip_finished => MoveKind::Jump,
                MoveKind::Jump | MoveKind::Fall => MoveKind::Fall,
                _ if v.up > c.jump_speed => MoveKind::Jump,
                _ => MoveKind::Fall,
            };
            self.key = next;
            return next;
        }
        if matches!(prev.kind, MoveKind::Jump | MoveKind::Fall) {
            next.kind = MoveKind::Land;
            next.dir = MoveDir::Fwd;
            self.key = next;
            return next;
        }
        if prev.kind == MoveKind::Land && !clip_finished && speed < c.run_enter {
            self.key = next;
            return next;
        }
        if matches!(prev.kind, MoveKind::TurnLeft | MoveKind::TurnRight)
            && !clip_finished
            && speed < c.walk_enter
        {
            self.key = next;
            return next;
        }
        let was_walk = matches!(prev.kind, MoveKind::Walk | MoveKind::Run | MoveKind::Sprint);
        let was_run = matches!(prev.kind, MoveKind::Run | MoveKind::Sprint);
        let was_sprint = prev.kind == MoveKind::Sprint;
        let moving = speed > if was_walk { c.walk_exit } else { c.walk_enter };
        if moving {
            let angle = v.right.atan2(v.forward);
            let sticky = |dir: MoveDir| if prev.dir == dir { 0.17 } else { 0.0 };
            let a = angle.abs();
            next.dir = if a <= 1.05 + sticky(MoveDir::Fwd) {
                MoveDir::Fwd
            } else if a >= 2.09 - sticky(MoveDir::Back) {
                MoveDir::Back
            } else if angle > 0.0 {
                MoveDir::Right
            } else {
                MoveDir::Left
            };
            let run = speed > if was_run { c.run_exit } else { c.run_enter };
            let sprint = speed > if was_sprint { c.sprint_exit } else { c.sprint_enter };
            next.kind = if input.sneak {
                MoveKind::Walk
            } else if sprint && next.dir == MoveDir::Fwd {
                MoveKind::Sprint
            } else if run {
                MoveKind::Run
            } else {
                MoveKind::Walk
            };
        } else if input.turn_error.abs() > c.turn_enter {
            next.dir = MoveDir::Fwd;
            next.kind = if input.turn_error > 0.0 {
                MoveKind::TurnLeft
            } else {
                MoveKind::TurnRight
            };
        } else {
            next.dir = MoveDir::Fwd;
            next.kind = MoveKind::Idle;
        }
        self.key = next;
        next
    }

    /// Playback rate that matches the clip's nominal ground speed to the real one.
    pub fn rate_for(&self, key: LocoKey, nominal_speed: f32) -> f32 {
        match key.kind {
            MoveKind::Walk | MoveKind::Run | MoveKind::Sprint if nominal_speed > 1.0 => {
                (self.ground_speed / nominal_speed).clamp(self.config.min_rate, self.config.max_rate)
            }
            _ => 1.0,
        }
    }

    /// Whether the state ends by itself (the system watches `Animator::finished`).
    pub fn is_one_shot(kind: MoveKind) -> bool {
        matches!(
            kind,
            MoveKind::Jump | MoveKind::Land | MoveKind::TurnLeft | MoveKind::TurnRight
        )
    }
}

/// `(action, phase, dir)` alternatives for a key, best first.
fn alternatives(key: LocoKey) -> Vec<(&'static str, &'static str, &'static str)> {
    let dir = key.dir.token();
    match key.kind {
        MoveKind::Idle => vec![("Ambient", "Loop", "Fwd"), ("Move", "Stand", "Fwd")],
        MoveKind::Walk => vec![("Move", "Walk", dir), ("Move", "Walk", "Fwd")],
        MoveKind::Run => vec![
            ("Move", "Run", dir),
            ("Move", "Run", "Fwd"),
            ("Move", "Walk", dir),
            ("Move", "Walk", "Fwd"),
        ],
        MoveKind::Sprint => vec![
            ("Move", "Sprint", "Fwd"),
            ("Move", "Run", "Fwd"),
            ("Move", "Walk", "Fwd"),
        ],
        MoveKind::TurnLeft => vec![("Move", "Turn90", "Left")],
        MoveKind::TurnRight => vec![("Move", "Turn90", "Right")],
        MoveKind::Jump => vec![("Jump", "Stand", "Fwd")],
        MoveKind::Fall => vec![("Fall", "Loop", "Fwd")],
        MoveKind::Land => vec![("Fall", "End", "Fwd"), ("Fall", "Recover", "Fwd")],
    }
}

/// Finds the installed motion for `key` in the skeleton family `family` (`"Hero"`).
pub fn resolve_motion<'a>(
    catalog: &'a MotionCatalog,
    family: &str,
    key: LocoKey,
) -> Option<&'a str> {
    let (l, r) = key.stance.hands();
    let mut hands = vec![(l, r)];
    if (l, r) != ("None", "None") {
        hands.push(("None", "None"));
    }
    let poses: &[&str] = if key.sneak && key.kind != MoveKind::Jump {
        &["Sneak", "Stand"]
    } else {
        &["Stand"]
    };
    // Same pose and hands first (degrading the movement kind), then simpler hands/poses.
    for pose in poses {
        for &(left, right) in &hands {
            for (action, phase, dir) in alternatives(key) {
                let query = MotionQuery {
                    pose: Some(pose),
                    left: Some(left),
                    right: Some(right),
                    pn: Some("P0"),
                    action: Some(action),
                    phase: Some(phase),
                    dir: Some(dir),
                    ..Default::default()
                };
                if let Some(found) = catalog.find(family, &query) {
                    return Some(found);
                }
            }
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    fn input(stance: Stance) -> LocomotionInput {
        LocomotionInput {
            stance,
            ..Default::default()
        }
    }

    fn lm(forward: f32, right: f32, up: f32) -> LocalMotion {
        LocalMotion { forward, right, up }
    }

    #[test]
    fn walk_run_sprint_with_hysteresis() {
        let mut l = Locomotion::default();
        let i = input(Stance::Unarmed);
        assert_eq!(l.step(&i, lm(0.0, 0.0, 0.0), false).kind, MoveKind::Idle);
        assert_eq!(l.step(&i, lm(100.0, 0.0, 0.0), false).kind, MoveKind::Walk);
        assert_eq!(l.step(&i, lm(300.0, 0.0, 0.0), false).kind, MoveKind::Run);
        // still run just below the enter threshold
        assert_eq!(l.step(&i, lm(260.0, 0.0, 0.0), false).kind, MoveKind::Run);
        assert_eq!(l.step(&i, lm(500.0, 0.0, 0.0), false).kind, MoveKind::Sprint);
        assert_eq!(l.step(&i, lm(440.0, 0.0, 0.0), false).kind, MoveKind::Sprint);
        assert_eq!(l.step(&i, lm(15.0, 0.0, 0.0), false).kind, MoveKind::Walk);
        assert_eq!(l.step(&i, lm(5.0, 0.0, 0.0), false).kind, MoveKind::Idle);
    }

    #[test]
    fn strafe_and_backpedal_directions() {
        let mut l = Locomotion::default();
        let i = input(Stance::OneHand);
        assert_eq!(l.step(&i, lm(0.0, 100.0, 0.0), false).dir, MoveDir::Right);
        assert_eq!(l.step(&i, lm(0.0, -100.0, 0.0), false).dir, MoveDir::Left);
        assert_eq!(l.step(&i, lm(-100.0, 0.0, 0.0), false).dir, MoveDir::Back);
        assert_eq!(l.step(&i, lm(100.0, 10.0, 0.0), false).dir, MoveDir::Fwd);
    }

    #[test]
    fn jump_fall_land_sequence() {
        let mut l = Locomotion::default();
        let mut i = input(Stance::Unarmed);
        i.grounded = false;
        assert_eq!(l.step(&i, lm(0.0, 0.0, 300.0), false).kind, MoveKind::Jump);
        assert_eq!(l.step(&i, lm(0.0, 0.0, 100.0), false).kind, MoveKind::Jump);
        assert_eq!(l.step(&i, lm(0.0, 0.0, -50.0), true).kind, MoveKind::Fall);
        i.grounded = true;
        assert_eq!(l.step(&i, lm(0.0, 0.0, 0.0), false).kind, MoveKind::Land);
        assert_eq!(l.step(&i, lm(0.0, 0.0, 0.0), false).kind, MoveKind::Land);
        assert_eq!(l.step(&i, lm(0.0, 0.0, 0.0), true).kind, MoveKind::Idle);
        // walking off a ledge is a fall, not a jump
        i.grounded = false;
        assert_eq!(l.step(&i, lm(100.0, 0.0, -10.0), false).kind, MoveKind::Fall);
    }

    #[test]
    fn turn_in_place_runs_to_completion() {
        let mut l = Locomotion::default();
        let mut i = input(Stance::Unarmed);
        i.turn_error = 1.5;
        assert_eq!(l.step(&i, lm(0.0, 0.0, 0.0), false).kind, MoveKind::TurnLeft);
        i.turn_error = 0.1;
        assert_eq!(l.step(&i, lm(0.0, 0.0, 0.0), false).kind, MoveKind::TurnLeft);
        assert_eq!(l.step(&i, lm(0.0, 0.0, 0.0), true).kind, MoveKind::Idle);
        i.turn_error = -1.5;
        assert_eq!(l.step(&i, lm(0.0, 0.0, 0.0), false).kind, MoveKind::TurnRight);
    }

    #[test]
    fn sneak_uses_walk_state_and_rate_sync() {
        let mut l = Locomotion::default();
        let mut i = input(Stance::Unarmed);
        i.sneak = true;
        assert_eq!(l.step(&i, lm(400.0, 0.0, 0.0), false).kind, MoveKind::Walk);
        let key = l.key;
        let rate = l.rate_for(key, 150.0);
        assert!((rate - l.config.max_rate).abs() < 1e-6);
        assert_eq!(l.rate_for(LocoKey { kind: MoveKind::Idle, ..key }, 150.0), 1.0);
    }

    #[test]
    fn resolution_degrades_gracefully() {
        let names = [
            "Hero_Stand_None_None_P0_Move_Walk_N_Fwd_00_%_00_P0_160",
            "Hero_Stand_None_None_P0_Move_Run_N_Fwd_00_%_00_P0_400",
            "Hero_Stand_None_1H_P0_Move_Walk_N_Fwd_00_%_00_P0_160",
            "Hero_Sneak_None_None_P0_Move_Walk_N_Fwd_00_%_00_P0_150",
            "Hero_Stand_None_None_P0_Ambient_Loop_N_Fwd_00_%_00_P0_0",
            "Hero_Stand_None_None_P0_Fall_Loop_N_Fwd_00_%_00_P0_0",
        ];
        let catalog = MotionCatalog::new(names);
        let key = |kind, stance, sneak| LocoKey {
            kind,
            dir: MoveDir::Fwd,
            stance,
            sneak,
        };
        // Run with a sword has no run clip in this set: walk with the sword wins over
        // running unarmed.
        assert_eq!(
            resolve_motion(&catalog, "Hero", key(MoveKind::Run, Stance::OneHand, false)),
            Some(names[2])
        );
        assert_eq!(
            resolve_motion(&catalog, "Hero", key(MoveKind::Sprint, Stance::Unarmed, false)),
            Some(names[1])
        );
        assert_eq!(
            resolve_motion(&catalog, "Hero", key(MoveKind::Walk, Stance::Unarmed, true)),
            Some(names[3])
        );
        // Bow stance does not exist here: falls back to unarmed.
        assert_eq!(
            resolve_motion(&catalog, "Hero", key(MoveKind::Fall, Stance::Bow, false)),
            Some(names[5])
        );
        assert_eq!(
            resolve_motion(&catalog, "Hero", key(MoveKind::Idle, Stance::Unarmed, false)),
            Some(names[4])
        );
        assert!(resolve_motion(&catalog, "Orc", key(MoveKind::Idle, Stance::Unarmed, false)).is_none());
    }
}
