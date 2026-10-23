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
//! | HeavyLand | `Stand` | `Fall` / `Hit` (`ZS_HeavyLand`, S:ae9b0; fallback `End`) | `Fwd` |
//! | DiveIn | `Swim` | `Fall` / `Hit` (`ZS_DiveIn`, S:af3e0) | `Fwd` |
//! | Slide | `Stand` | `Slide` / `Begin` (`ZS_Slide`) | `Fwd` |
//! | Wade / Swim | `Wade` / `Swim` | the walk / run / sprint / idle / turn clips of the pose | as ground |
//!
//! `Left`/`Right` hands per [`Stance`]: Unarmed `None/None`, Fist `None/Fist`, OneHand
//! `None/1H`, OneHandShield `Shield/1H`, TwoHand `None/2H`, Bow `Bow/Arrow`, CrossBow
//! `CrossBow/Bolt`, Staff `None/Staff`, Magic `None/Cast`, Torch `Torch/None`. Missing
//! combinations fall back to `None/None`.
use crate::names::{MotionCatalog, MotionQuery};
use bevy::{ecs::component::Component, math::Vec3};

/// Entity-local forward axis of an animated character (Bevy convention, `Transform::forward`);
/// the animator rotates the skeleton so the authored facing direction ends up here.
pub const FORWARD: Vec3 = Vec3::NEG_Z;
/// Entity-local right axis (`FORWARD x Y`).
pub const RIGHT: Vec3 = Vec3::X;

/// Gait the movement mode machine selected (`gECharMovementMode` 2..5). When present the mixer
/// follows it instead of guessing the gait from the ground speed (the original never promotes a
/// gait by speed: the locomotion clip is chosen by the mode and played at `speed / nominal`).
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum GaitHint {
    Sneak,
    Walk,
    Run,
    Sprint,
}

/// The body pose family of the locomotion clips: `Hero_<Pose>_...` (`Stand` on land, `Wade`
/// in shallow water, `Swim` while swimming and diving). Wading and swimming are the movement
/// modes 10 / 7 / 8 of `gECharMovementMode`.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug, Default)]
pub enum BodyPose {
    #[default]
    Stand,
    Wade,
    Swim,
}

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
    /// Every stance, in declaration order.
    pub const ALL: [Stance; 10] = [
        Stance::Unarmed,
        Stance::Fist,
        Stance::OneHand,
        Stance::OneHandShield,
        Stance::TwoHand,
        Stance::Bow,
        Stance::CrossBow,
        Stance::Staff,
        Stance::Magic,
        Stance::Torch,
    ];

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
    /// The guard is up (`AniState::Parade`): the clips of the `Parade` pose
    /// (`Hero_Parade_<L>_<R>_P0_Move_...`).
    pub parade: bool,
    /// Signed yaw from the current facing to the desired facing, radians, positive is
    /// counter-clockwise seen from above (left). Triggers turn-in-place when standing.
    pub turn_error: f32,
    /// Gait from the movement mode machine; `None` keeps the speed-threshold guess.
    pub gait: Option<GaitHint>,
    /// Pose family: wading / swimming (movement modes 10 / 7 / 8).
    pub body: BodyPose,
    /// The slide mode (`gECharMovementMode` 0xc, `ZS_Slide`).
    pub slide: bool,
    /// The landing that just happened hurt (`ZS_HeavyLand`): play the heavy landing clip
    /// instead of the plain landing. Set by the host together with the landing.
    pub heavy_land: bool,
    /// The fall ended in deep water (`ZS_DiveIn`): play the dive-in clip of the swim pose.
    pub dive_in: bool,
}

impl Default for LocomotionInput {
    fn default() -> Self {
        Self {
            velocity: Vec3::ZERO,
            grounded: true,
            stance: Stance::Unarmed,
            sneak: false,
            parade: false,
            turn_error: 0.0,
            gait: None,
            body: BodyPose::Stand,
            slide: false,
            heavy_land: false,
            dive_in: false,
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
    /// `ZS_HeavyLand`: the clip of a landing that cost hit points.
    HeavyLand,
    /// `ZS_DiveIn`: falling into deep water.
    DiveIn,
    /// `ZS_Slide`: sliding down ground steeper than `SteepGroundAngleMax`.
    Slide,
    /// The slide ended on ground (`Slide_End`).
    SlideEnd,
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
    pub parade: bool,
    pub body: BodyPose,
}

impl LocoKey {
    /// Standing idle in `stance` (the key a freshly spawned character plays).
    pub fn idle(stance: Stance) -> Self {
        Self {
            kind: MoveKind::Idle,
            dir: MoveDir::Fwd,
            stance,
            sneak: false,
            parade: false,
            body: BodyPose::Stand,
        }
    }
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
                parade: false,
                body: BodyPose::Stand,
            },
            ground_speed: 0.0,
            playing: None,
        }
    }

    /// Advances the state machine. `clip_finished` reports that the one-shot clip of the
    /// current state (jump, land, turn) has ended.
    pub fn step(
        &mut self,
        input: &LocomotionInput,
        v: LocalMotion,
        clip_finished: bool,
    ) -> LocoKey {
        let c = &self.config;
        let prev = self.key;
        let speed = v.forward.hypot(v.right);
        self.ground_speed = speed;
        let mut next = LocoKey {
            stance: input.stance,
            sneak: input.sneak,
            parade: input.parade,
            body: input.body,
            ..prev
        };
        // Swimming and diving: the swim pose never falls or lands. A fall that ended in the
        // water plays the dive-in clip once (`ZS_DiveIn`).
        if input.body == BodyPose::Swim
            && (input.dive_in || prev.kind == MoveKind::DiveIn)
            && !(prev.kind == MoveKind::DiveIn && clip_finished)
            && speed < c.run_enter
        {
            next.kind = MoveKind::DiveIn;
            next.dir = MoveDir::Fwd;
            self.key = next;
            return next;
        }
        if input.slide {
            next.kind = MoveKind::Slide;
            next.dir = MoveDir::Fwd;
            self.key = next;
            return next;
        }
        if !input.grounded && input.body == BodyPose::Stand {
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
        if prev.kind == MoveKind::Slide && input.body == BodyPose::Stand {
            next.kind = MoveKind::SlideEnd;
            next.dir = MoveDir::Fwd;
            self.key = next;
            return next;
        }
        // The slide stops at once (the speed accumulators are reset); the filtered animation
        // velocity still shows the old speed for a moment, so only a steering player (a gait
        // from the movement mode machine) interrupts the clip.
        if prev.kind == MoveKind::SlideEnd && !clip_finished && input.gait.is_none() {
            self.key = next;
            return next;
        }
        if matches!(prev.kind, MoveKind::Jump | MoveKind::Fall) && input.body == BodyPose::Stand {
            next.kind = if input.heavy_land {
                MoveKind::HeavyLand
            } else {
                MoveKind::Land
            };
            next.dir = MoveDir::Fwd;
            self.key = next;
            return next;
        }
        if matches!(prev.kind, MoveKind::HeavyLand) && !clip_finished {
            // `ZS_HeavyLand` plays its clip to the end before the routine goes on.
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
            let sprint = speed
                > if was_sprint {
                    c.sprint_exit
                } else {
                    c.sprint_enter
                };
            next.kind = if let Some(g) = input.gait {
                match g {
                    GaitHint::Sneak | GaitHint::Walk => MoveKind::Walk,
                    GaitHint::Run => MoveKind::Run,
                    GaitHint::Sprint if next.dir == MoveDir::Fwd => MoveKind::Sprint,
                    GaitHint::Sprint => MoveKind::Run,
                }
            } else if input.sneak {
                MoveKind::Walk
            } else if sprint && next.dir == MoveDir::Fwd {
                MoveKind::Sprint
            } else if run {
                MoveKind::Run
            } else {
                MoveKind::Walk
            };
        } else if input.turn_error.abs() > c.turn_enter && input.body != BodyPose::Swim {
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
                (self.ground_speed / nominal_speed)
                    .clamp(self.config.min_rate, self.config.max_rate)
            }
            _ => 1.0,
        }
    }

    /// Whether the state ends by itself (the system watches `Animator::finished`).
    pub fn is_one_shot(kind: MoveKind) -> bool {
        matches!(
            kind,
            MoveKind::Jump
                | MoveKind::Land
                | MoveKind::HeavyLand
                | MoveKind::SlideEnd
                | MoveKind::DiveIn
                | MoveKind::TurnLeft
                | MoveKind::TurnRight
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
        MoveKind::HeavyLand => vec![
            ("Fall", "Hit", "Fwd"),
            ("Fall", "End", "Fwd"),
            ("Fall", "Recover", "Fwd"),
        ],
        MoveKind::DiveIn => vec![("Fall", "Hit", "Fwd"), ("Move", "Stand", "Fwd")],
        MoveKind::Slide => vec![("Slide", "Begin", "Fwd"), ("Fall", "Loop", "Fwd")],
        MoveKind::SlideEnd => vec![("Slide", "End", "Fwd"), ("Fall", "End", "Fwd")],
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
    let poses: &[&str] = if key.body != BodyPose::Stand {
        // Water poses have no weapon variants: the unarmed clips of the pose.
        hands.clear();
        hands.push(("None", "None"));
        match key.body {
            BodyPose::Wade => &["Wade", "Stand"],
            _ => &["Swim", "Wade"],
        }
    } else if key.parade && key.kind != MoveKind::Jump {
        &["Parade", "Stand"]
    } else if key.sneak && key.kind != MoveKind::Jump {
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
        assert_eq!(
            l.step(&i, lm(500.0, 0.0, 0.0), false).kind,
            MoveKind::Sprint
        );
        assert_eq!(
            l.step(&i, lm(440.0, 0.0, 0.0), false).kind,
            MoveKind::Sprint
        );
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
        assert_eq!(
            l.step(&i, lm(100.0, 0.0, -10.0), false).kind,
            MoveKind::Fall
        );
    }

    #[test]
    fn turn_in_place_runs_to_completion() {
        let mut l = Locomotion::default();
        let mut i = input(Stance::Unarmed);
        i.turn_error = 1.5;
        assert_eq!(
            l.step(&i, lm(0.0, 0.0, 0.0), false).kind,
            MoveKind::TurnLeft
        );
        i.turn_error = 0.1;
        assert_eq!(
            l.step(&i, lm(0.0, 0.0, 0.0), false).kind,
            MoveKind::TurnLeft
        );
        assert_eq!(l.step(&i, lm(0.0, 0.0, 0.0), true).kind, MoveKind::Idle);
        i.turn_error = -1.5;
        assert_eq!(
            l.step(&i, lm(0.0, 0.0, 0.0), false).kind,
            MoveKind::TurnRight
        );
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
        assert_eq!(
            l.rate_for(
                LocoKey {
                    kind: MoveKind::Idle,
                    ..key
                },
                150.0
            ),
            1.0
        );
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
            parade: false,
            body: BodyPose::Stand,
        };
        // Run with a sword has no run clip in this set: walk with the sword wins over
        // running unarmed.
        assert_eq!(
            resolve_motion(&catalog, "Hero", key(MoveKind::Run, Stance::OneHand, false)),
            Some(names[2])
        );
        assert_eq!(
            resolve_motion(
                &catalog,
                "Hero",
                key(MoveKind::Sprint, Stance::Unarmed, false)
            ),
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
            resolve_motion(
                &catalog,
                "Hero",
                key(MoveKind::Idle, Stance::Unarmed, false)
            ),
            Some(names[4])
        );
        assert!(
            resolve_motion(&catalog, "Orc", key(MoveKind::Idle, Stance::Unarmed, false)).is_none()
        );
    }

    #[test]
    fn gait_hint_selects_the_clip_regardless_of_speed() {
        let mut loco = Locomotion::default();
        let mut i = input(Stance::Unarmed);
        i.gait = Some(GaitHint::Run);
        assert_eq!(loco.step(&i, lm(60.0, 0.0, 0.0), false).kind, MoveKind::Run);
        i.gait = Some(GaitHint::Sprint);
        assert_eq!(
            loco.step(&i, lm(300.0, 0.0, 0.0), false).kind,
            MoveKind::Sprint
        );
        i.gait = Some(GaitHint::Walk);
        assert_eq!(
            loco.step(&i, lm(500.0, 0.0, 0.0), false).kind,
            MoveKind::Walk
        );
        // Without a hint the speed decides (100 cm/s is a walk).
        i.gait = None;
        let mut guess = Locomotion::default();
        assert_eq!(
            guess.step(&i, lm(100.0, 0.0, 0.0), false).kind,
            MoveKind::Walk
        );
    }
    #[test]
    fn slide_heavy_landing_and_water_poses() {
        let mut l = Locomotion::default();
        let mut i = input(Stance::Unarmed);
        // sliding keeps the slide clip, the end plays `Slide_End`, then idle again
        i.slide = true;
        assert_eq!(l.step(&i, lm(0.0, 0.0, 0.0), false).kind, MoveKind::Slide);
        i.slide = false;
        assert_eq!(
            l.step(&i, lm(0.0, 0.0, 0.0), false).kind,
            MoveKind::SlideEnd
        );
        assert_eq!(l.step(&i, lm(0.0, 0.0, 0.0), true).kind, MoveKind::Idle);
        // a landing that hurt plays the heavy clip to its end
        let mut l = Locomotion::default();
        i.grounded = false;
        l.step(&i, lm(0.0, 0.0, -900.0), false);
        i.grounded = true;
        i.heavy_land = true;
        assert_eq!(
            l.step(&i, lm(0.0, 0.0, 0.0), false).kind,
            MoveKind::HeavyLand
        );
        i.heavy_land = false;
        assert_eq!(
            l.step(&i, lm(0.0, 0.0, 0.0), false).kind,
            MoveKind::HeavyLand
        );
        assert_eq!(l.step(&i, lm(0.0, 0.0, 0.0), true).kind, MoveKind::Idle);
        // the swim pose never falls and has no turn clips
        let mut l = Locomotion::default();
        i.body = BodyPose::Swim;
        i.grounded = false;
        i.turn_error = 2.0;
        assert_eq!(l.step(&i, lm(0.0, 0.0, 0.0), false).kind, MoveKind::Idle);
        i.dive_in = true;
        assert_eq!(
            l.step(&i, lm(0.0, 0.0, -300.0), false).kind,
            MoveKind::DiveIn
        );
        i.dive_in = false;
        assert_eq!(l.step(&i, lm(0.0, 0.0, 0.0), true).kind, MoveKind::Idle);
        // clip resolution in the water poses (shipped names)
        let names = [
            "Hero_Swim_None_None_P0_Ambient_Loop_N_Fwd_00_%_00_P0_0",
            "Hero_Swim_None_None_P0_Fall_Hit_N_Fwd_00_%_00_P0_0",
            "Hero_Swim_None_None_P0_Move_Run_N_Fwd_00_%_00_P0_400",
            "Hero_Wade_None_None_P0_Move_Walk_N_Fwd_00_%_00_P0_160",
            "Hero_Stand_None_None_P0_Fall_Hit_N_Fwd_00_%_00_P0_430",
            "Hero_Stand_None_None_P0_Slide_Begin_N_Fwd_00_%_00_P0_0",
            "Hero_Stand_None_None_P0_Slide_End_N_Fwd_00_%_00_P0_0",
        ];
        let catalog = MotionCatalog::new(names);
        let key = |kind, body| LocoKey {
            kind,
            dir: MoveDir::Fwd,
            stance: Stance::OneHand,
            sneak: false,
            parade: false,
            body,
        };
        let r = |k| resolve_motion(&catalog, "Hero", k);
        assert_eq!(r(key(MoveKind::Idle, BodyPose::Swim)), Some(names[0]));
        assert_eq!(r(key(MoveKind::Run, BodyPose::Swim)), Some(names[2]));
        assert_eq!(r(key(MoveKind::DiveIn, BodyPose::Swim)), Some(names[1]));
        assert_eq!(r(key(MoveKind::Walk, BodyPose::Wade)), Some(names[3]));
        assert_eq!(r(key(MoveKind::HeavyLand, BodyPose::Stand)), Some(names[4]));
        assert_eq!(r(key(MoveKind::Slide, BodyPose::Stand)), Some(names[5]));
        assert_eq!(r(key(MoveKind::SlideEnd, BodyPose::Stand)), Some(names[6]));
    }
}
