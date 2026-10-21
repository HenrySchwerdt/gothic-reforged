//! Comfort locomotion: tracked head + thumbsticks drive the game's own character controller
//! (`gothic_physics::CharacterController::step_command`), so VR obeys the same step height,
//! slope limits, gravity and collision as the classic hero.
//!
//! Bevy-free and deterministic. Units: the tracking space and the rig origin are metres (XR
//! convention), the controller and [`CollisionWorld`] are Gothic centimetres; the conversion
//! happens only here.
//!
//! Per frame ([`Locomotion::update`]):
//! 1. turning (snap or smooth) rotates the rig origin about the head, so the view pivots in
//!    place;
//! 2. room-scale: the capsule is nudged under the real head; whatever a wall blocks pushes the
//!    origin back so the head cannot enter geometry;
//! 3. the move stick becomes a [`MoveCommand`] (direction relative to head or hand, gait by
//!    deflection, sprint by stick click, sneaking by real crouching), the controller steps;
//! 4. the origin follows the capsule (floor of the play area = character feet; seated players
//!    are lifted to standing height);
//! 5. optional teleport (parabolic arc, validated landing) and the comfort vignette.
use crate::CM_PER_M;
use crate::input::{Hand, Pose, VrButton, VrInput, apply_deadzone};
use glam::{Quat, Vec2, Vec3};
use gothic_physics::{CharacterController, CollisionWorld, Gait, MoveCommand, StepReport};
use serde::{Deserialize, Serialize};

/// What "stick forward" means.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum MoveReference {
    Head,
    Hand(Hand),
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub enum TurnMode {
    Snap { degrees: f32 },
    Smooth { degrees_per_second: f32 },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Stance {
    Standing,
    /// The player sits; the rig lifts the view to the calibrated standing eye height.
    Seated,
}

/// Player comfort options.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct ComfortSettings {
    pub move_reference: MoveReference,
    /// Hand whose stick moves.
    pub move_hand: Hand,
    /// Hand whose stick turns (X) and aims teleport (forward).
    pub turn_hand: Hand,
    pub turn: TurnMode,
    pub teleport: bool,
    /// Maximum vignette opacity, 0 disables it.
    pub vignette_strength: f32,
    pub stance: Stance,
    /// Calibrated standing eye height of the player, m.
    pub standing_eye_height: f32,
    /// Eye height while seated, m (only used with [`Stance::Seated`]).
    pub seated_eye_height: f32,
    pub deadzone: f32,
    /// Below this fraction of the standing eye height the character sneaks.
    pub crouch_ratio: f32,
    /// Stick deflection below this walks, above runs.
    pub walk_threshold: f32,
    /// Steepest teleport landing, degrees.
    pub teleport_max_slope_deg: f32,
    /// Launch speed of the teleport arc, m/s.
    pub teleport_speed: f32,
    /// Let the capsule follow the real head (room scale).
    pub room_scale: bool,
}

impl Default for ComfortSettings {
    fn default() -> Self {
        Self {
            move_reference: MoveReference::Head,
            move_hand: Hand::Left,
            turn_hand: Hand::Right,
            turn: TurnMode::Snap { degrees: 30.0 },
            teleport: true,
            vignette_strength: 0.6,
            stance: Stance::Standing,
            standing_eye_height: 1.7,
            seated_eye_height: 1.2,
            deadzone: 0.15,
            crouch_ratio: 0.78,
            walk_threshold: 0.6,
            teleport_max_slope_deg: 35.0,
            teleport_speed: 8.0,
            room_scale: true,
        }
    }
}

/// Snap turn fires above this stick X and re-arms below [`SNAP_REARM`].
const SNAP_FIRE: f32 = 0.7;
const SNAP_REARM: f32 = 0.35;
/// Teleport aims above this stick Y and fires when it falls below [`TELEPORT_RELEASE`].
const TELEPORT_AIM: f32 = 0.7;
const TELEPORT_RELEASE: f32 = 0.3;
/// Speed (m/s) / turn rate (rad/s) at which the vignette reaches full strength.
const VIGNETTE_SPEED: f32 = 4.0;
const VIGNETTE_TURN: f32 = 2.5;
/// Vignette smoothing, 1/s.
const VIGNETTE_RATE: f32 = 8.0;
/// Room-scale head motion below this (m) is ignored.
const ROOM_SCALE_EPS: f32 = 0.01;

/// Teleport aim result.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct TeleportAim {
    /// Arc sample points (world metres), up to the hit.
    pub arc: Vec<Vec3>,
    /// Valid landing point (feet, world metres).
    pub target: Option<Vec3>,
}

/// What happened in one [`Locomotion::update`].
#[derive(Debug, Clone, PartialEq, Default)]
pub struct LocomotionReport {
    pub step: StepReport,
    pub gait: Option<Gait>,
    pub crouching: bool,
    /// Radians the origin turned this frame.
    pub turned: f32,
    pub teleported: bool,
    pub teleport_aim: Option<TeleportAim>,
    /// Current vignette opacity 0..1.
    pub vignette: f32,
    /// Head motion a wall refused (m, xz).
    pub head_blocked: Vec3,
}

/// Rig state: where tracking space sits in the world.
#[derive(Debug, Clone, PartialEq)]
pub struct Locomotion {
    pub settings: ComfortSettings,
    /// Tracking space -> world (metres, yaw only).
    pub origin: Pose,
    yaw: f32,
    snap_armed: bool,
    aiming: bool,
    last_aim: Option<TeleportAim>,
    vignette: f32,
}

impl Locomotion {
    /// A rig whose play-area floor is at the controller's feet, with the head (from `input`)
    /// above them.
    pub fn new(settings: ComfortSettings, ctrl: &CharacterController, head: &Pose) -> Self {
        let mut l = Self {
            settings,
            origin: Pose::IDENTITY,
            yaw: 0.0,
            snap_armed: true,
            aiming: false,
            last_aim: None,
            vignette: 0.0,
        };
        l.recenter(ctrl, head);
        l
    }

    /// Put the origin so the head is straight above the character's feet.
    pub fn recenter(&mut self, ctrl: &CharacterController, head: &Pose) {
        let feet = ctrl.position / CM_PER_M;
        let r = Quat::from_rotation_y(self.yaw);
        let h = r * Vec3::new(head.position.x, 0.0, head.position.z);
        self.origin = Pose::new(
            Vec3::new(feet.x - h.x, feet.y + self.seat_lift(), feet.z - h.z),
            r,
        );
    }

    pub fn yaw(&self) -> f32 {
        self.yaw
    }

    /// Height added to tracking space so a seated player stands in game.
    pub fn seat_lift(&self) -> f32 {
        match self.settings.stance {
            Stance::Standing => 0.0,
            Stance::Seated => {
                (self.settings.standing_eye_height - self.settings.seated_eye_height).max(0.0)
            }
        }
    }

    /// World pose of a tracking-space pose.
    pub fn to_world(&self, p: &Pose) -> Pose {
        self.origin.mul(p)
    }

    pub fn head_world(&self, input: &VrInput) -> Pose {
        self.to_world(&input.head())
    }

    /// Is the player physically crouching?
    pub fn crouching(&self, head: &Pose) -> bool {
        let eye = head.position.y + self.seat_lift();
        eye < self.settings.standing_eye_height * self.settings.crouch_ratio
    }

    fn rotate_about_head(&mut self, head: &Pose, angle: f32) {
        let head_w = self.origin.transform_point(head.position);
        self.yaw += angle;
        let r = Quat::from_rotation_y(self.yaw);
        let p = head_w - r * head.position;
        self.origin = Pose::new(Vec3::new(p.x, self.origin.position.y, p.z), r);
    }

    /// Advance one frame.
    pub fn update(
        &mut self,
        input: &VrInput,
        ctrl: &mut CharacterController,
        world: &CollisionWorld,
        dt: f32,
    ) -> LocomotionReport {
        let dt = if dt.is_finite() {
            dt.clamp(0.0, 0.1)
        } else {
            0.0
        };
        let s = self.settings;
        let head = input.head();
        let mut report = LocomotionReport::default();

        // 1. Turning.
        let turn_x = apply_deadzone(input.stick(s.turn_hand), s.deadzone).x;
        let raw_turn_x = input.stick(s.turn_hand).x;
        let turn = match s.turn {
            TurnMode::Snap { degrees } => {
                if self.snap_armed && raw_turn_x.abs() > SNAP_FIRE {
                    self.snap_armed = false;
                    -raw_turn_x.signum() * degrees.to_radians()
                } else {
                    if raw_turn_x.abs() < SNAP_REARM {
                        self.snap_armed = true;
                    }
                    0.0
                }
            }
            TurnMode::Smooth { degrees_per_second } => {
                -turn_x * degrees_per_second.to_radians() * dt
            }
        };
        if turn != 0.0 {
            self.rotate_about_head(&head, turn);
        }
        report.turned = turn;

        // 2. Room scale: the capsule follows the real head.
        if s.room_scale {
            let head_w = self.origin.transform_point(head.position);
            let feet = ctrl.position / CM_PER_M;
            let want = Vec3::new(head_w.x - feet.x, 0.0, head_w.z - feet.z);
            if want.length() > ROOM_SCALE_EPS {
                let before = ctrl.position;
                // Sub-steps no longer than half the radius so a fast lean cannot tunnel
                // through thin walls.
                let total = want * CM_PER_M;
                let steps = (total.length() / (ctrl.config.radius * 0.5))
                    .ceil()
                    .max(1.0);
                for _ in 0..steps as usize {
                    ctrl.nudge(world, total / steps);
                }
                let moved = (ctrl.position - before) / CM_PER_M;
                let blocked = want - Vec3::new(moved.x, 0.0, moved.z);
                if blocked.length() > 1e-4 {
                    self.origin.position -= blocked;
                    report.head_blocked = blocked;
                }
            }
        }

        // 3. Stick locomotion through the game's controller.
        let stick = apply_deadzone(input.stick(s.move_hand), s.deadzone);
        let reference = match s.move_reference {
            MoveReference::Head => Some(self.to_world(&head)),
            MoveReference::Hand(h) => input.hand_pose(h).map(|p| self.to_world(&p)),
        };
        let reference_yaw = reference.map_or(self.yaw, |p| p.yaw());
        let wish = Quat::from_rotation_y(reference_yaw) * Vec3::new(stick.x, 0.0, -stick.y);
        report.crouching = self.crouching(&head);
        let deflection = stick.length();
        let gait = if report.crouching {
            Gait::Sneak
        } else if input.held(s.move_hand, VrButton::StickClick) {
            Gait::Sprint
        } else if deflection < s.walk_threshold {
            Gait::Walk
        } else {
            Gait::Run
        };
        report.gait = (deflection > 0.0).then_some(gait);
        let head_fwd = self.to_world(&head).forward();
        let cmd = MoveCommand {
            wish,
            gait,
            jump: input.just_pressed(Hand::Right, VrButton::Primary),
            face: Some(Vec3::new(head_fwd.x, 0.0, head_fwd.z)),
            ..MoveCommand::default()
        };
        report.step = ctrl.step_command(world, &cmd, dt);

        // 4. Origin follows the capsule.
        self.follow_capsule(ctrl, &head);

        // 5. Teleport.
        if s.teleport {
            self.teleport(input, ctrl, world, &mut report);
        }

        // Vignette from the head's world speed and the turn rate.
        let speed = ctrl.velocity.length() / CM_PER_M;
        let turn_rate = if matches!(s.turn, TurnMode::Smooth { .. }) && dt > 0.0 {
            turn.abs() / dt
        } else {
            0.0
        };
        let target = s.vignette_strength
            * (speed / VIGNETTE_SPEED)
                .max(turn_rate / VIGNETTE_TURN)
                .clamp(0.0, 1.0);
        let k = (VIGNETTE_RATE * dt).min(1.0);
        self.vignette += (target - self.vignette) * k;
        report.vignette = self.vignette;
        report
    }

    fn follow_capsule(&mut self, ctrl: &CharacterController, head: &Pose) {
        let feet = ctrl.position / CM_PER_M;
        let head_w = self.origin.transform_point(head.position);
        self.origin.position.x += feet.x - head_w.x;
        self.origin.position.z += feet.z - head_w.z;
        self.origin.position.y = feet.y + self.seat_lift();
    }

    fn teleport(
        &mut self,
        input: &VrInput,
        ctrl: &mut CharacterController,
        world: &CollisionWorld,
        report: &mut LocomotionReport,
    ) {
        let s = self.settings;
        let y = input.stick(s.turn_hand).y;
        if y > TELEPORT_AIM {
            self.aiming = true;
        }
        if !self.aiming {
            return;
        }
        if let Some(hand) = input.hand_pose(s.turn_hand) {
            let hand_w = self.to_world(&hand);
            let aim = teleport_arc(
                world,
                hand_w.position,
                hand_w.forward(),
                s.teleport_speed,
                ctrl.config.radius,
                ctrl.config.height,
                s.teleport_max_slope_deg,
            );
            self.last_aim = Some(aim.clone());
            report.teleport_aim = Some(aim);
        }
        if y < TELEPORT_RELEASE {
            self.aiming = false;
            if let Some(target) = self.last_aim.take().and_then(|a| a.target) {
                // Lands through the controller's own fall/landing rules.
                ctrl.teleport(target * CM_PER_M + Vec3::Y);
                self.follow_capsule(ctrl, &input.head());
                report.teleported = true;
            }
        }
    }
}

/// Gravity of the teleport arc, m/s^2.
const ARC_GRAVITY: f32 = 9.81;
const ARC_STEP: f32 = 0.04;
const ARC_MAX_TIME: f32 = 2.5;

/// Sample a parabolic teleport arc and validate the landing: the surface must be flatter than
/// `max_slope_deg` and a standing capsule (`radius`, `height`, cm) must fit there.
pub fn teleport_arc(
    world: &CollisionWorld,
    from: Vec3,
    dir: Vec3,
    speed: f32,
    radius_cm: f32,
    height_cm: f32,
    max_slope_deg: f32,
) -> TeleportAim {
    let mut aim = TeleportAim::default();
    let dir = dir.normalize_or_zero();
    if dir == Vec3::ZERO || !from.is_finite() {
        return aim;
    }
    let v0 = dir * speed;
    let mut prev = from;
    aim.arc.push(prev);
    let mut t = 0.0;
    while t < ARC_MAX_TIME {
        t += ARC_STEP;
        let p = from + v0 * t + Vec3::new(0.0, -0.5 * ARC_GRAVITY * t * t, 0.0);
        let seg = p - prev;
        let len = seg.length();
        if len > 1e-6
            && let Some((d, n)) = world.raycast_hit(prev * CM_PER_M, seg / len, len * CM_PER_M)
        {
            let hit = prev + seg / len * (d / CM_PER_M);
            aim.arc.push(hit);
            let flat = n.y >= max_slope_deg.to_radians().cos();
            let feet = hit * CM_PER_M + Vec3::Y;
            let r = radius_cm;
            let clear = !world.capsule_blocked(
                feet + Vec3::Y * (r + 2.0),
                feet + Vec3::Y * (height_cm - r).max(r + 2.0),
                r,
            );
            if flat && clear {
                aim.target = Some(hit);
            }
            return aim;
        }
        aim.arc.push(p);
        prev = p;
    }
    aim
}

/// Head-height helper for calibration: the current eye height becomes the standing height.
pub fn calibrate_standing(settings: &mut ComfortSettings, head: &Pose) {
    if head.position.y.is_finite() && head.position.y > 0.5 {
        settings.standing_eye_height = head.position.y;
    }
}

/// Stick vector (deadzoned) a backend would report for a world direction; used by tests and
/// scripted drivers.
pub fn stick_for(direction_yaw_relative: f32, deflection: f32) -> Vec2 {
    Vec2::new(-direction_yaw_relative.sin(), direction_yaw_relative.cos()) * deflection
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::greybox::{self, STAIR_RISE, STAIR_STEPS};
    use crate::input::XrFrame;
    use gothic_physics::{MovementMode, MovementProfile};

    const DT: f32 = 1.0 / 72.0;

    struct Rig {
        input: VrInput,
        ctrl: CharacterController,
        loco: Locomotion,
        world: CollisionWorld,
    }

    fn rig_at(feet_m: Vec3, settings: ComfortSettings) -> Rig {
        let world = greybox::collision_world(&greybox::default_layout());
        let mut ctrl = CharacterController::new(MovementProfile::hero(), feet_m * CM_PER_M);
        // Settle on the ground.
        for _ in 0..30 {
            ctrl.step_command(&world, &MoveCommand::default(), DT);
        }
        let mut input = VrInput::new();
        input.push(XrFrame::default());
        let loco = Locomotion::new(settings, &ctrl, &input.head());
        Rig {
            input,
            ctrl,
            loco,
            world,
        }
    }

    impl Rig {
        fn run(&mut self, frames: usize, f: impl Fn(&mut XrFrame)) -> LocomotionReport {
            let mut last = LocomotionReport::default();
            for _ in 0..frames {
                let mut fr = XrFrame::default();
                f(&mut fr);
                self.input.push(fr);
                last = self
                    .loco
                    .update(&self.input, &mut self.ctrl, &self.world, DT);
            }
            last
        }
        fn feet(&self) -> Vec3 {
            self.ctrl.position / CM_PER_M
        }
    }

    fn look(f: &mut XrFrame, yaw: f32) {
        f.head.rotation = Quat::from_rotation_y(yaw);
    }

    #[test]
    fn head_relative_stick_moves_where_the_head_looks() {
        let mut r = rig_at(Vec3::new(0.0, 0.0, 5.0), ComfortSettings::default());
        // Look 90 degrees left (-X) and push the stick forward.
        r.run(72, |f| {
            look(f, std::f32::consts::FRAC_PI_2);
            f.hand_mut(Hand::Left).thumbstick = Vec2::Y;
        });
        let feet = r.feet();
        assert!(feet.x < -1.5, "{feet}");
        assert!((feet.z - 5.0).abs() < 0.3, "{feet}");
        // Head stays above the capsule.
        let head = r.loco.head_world(&r.input);
        assert!(Vec2::new(head.position.x - feet.x, head.position.z - feet.z).length() < 1e-3);
        assert!((head.position.y - (feet.y + 1.7)).abs() < 1e-3);
    }

    #[test]
    fn hand_relative_ignores_head() {
        let s = ComfortSettings {
            move_reference: MoveReference::Hand(Hand::Left),
            ..Default::default()
        };
        let mut r = rig_at(Vec3::new(0.0, 0.0, 5.0), s);
        r.run(72, |f| {
            look(f, std::f32::consts::FRAC_PI_2);
            f.hand_mut(Hand::Left).pose = Some(Pose::new(
                Vec3::new(-0.2, 1.2, -0.3),
                Quat::from_rotation_y(-std::f32::consts::FRAC_PI_2),
            ));
            f.hand_mut(Hand::Left).thumbstick = Vec2::Y;
        });
        assert!(r.feet().x > 1.5, "hand points +X: {}", r.feet());
    }

    #[test]
    fn walk_run_by_deflection_and_sprint_by_click() {
        let mut r = rig_at(Vec3::new(0.0, 0.0, 5.0), ComfortSettings::default());
        let rep = r.run(30, |f| {
            f.hand_mut(Hand::Left).thumbstick = Vec2::new(0.0, 0.45)
        });
        assert_eq!(rep.gait, Some(Gait::Walk));
        assert_eq!(r.ctrl.movement_mode(), MovementMode::Walking);
        let rep = r.run(30, |f| f.hand_mut(Hand::Left).thumbstick = Vec2::Y);
        assert_eq!(rep.gait, Some(Gait::Run));
        let rep = r.run(30, |f| {
            f.hand_mut(Hand::Left).thumbstick = Vec2::Y;
            f.hand_mut(Hand::Left).stick_click = true;
        });
        assert_eq!(rep.gait, Some(Gait::Sprint));
    }

    #[test]
    fn snap_turn_pivots_about_the_head_and_needs_rearm() {
        let mut r = rig_at(Vec3::new(0.0, 0.0, 5.0), ComfortSettings::default());
        let lean = |f: &mut XrFrame| f.head.position = Vec3::new(0.3, 1.7, 0.0);
        r.run(5, lean);
        let head0 = r.loco.head_world(&r.input).position;
        let rep = r.run(1, |f| {
            lean(f);
            f.hand_mut(Hand::Right).thumbstick = Vec2::X;
        });
        assert!(
            (rep.turned + 30f32.to_radians()).abs() < 1e-5,
            "right = clockwise"
        );
        let head1 = r.loco.head_world(&r.input).position;
        assert!(head0.distance(head1) < 0.02, "{head0} {head1}");
        // Holding the stick does not turn again ...
        let rep = r.run(10, |f| {
            lean(f);
            f.hand_mut(Hand::Right).thumbstick = Vec2::X;
        });
        assert_eq!(rep.turned, 0.0);
        // ... releasing and pushing again does.
        r.run(1, lean);
        let rep = r.run(1, |f| {
            lean(f);
            f.hand_mut(Hand::Right).thumbstick = Vec2::X;
        });
        assert!(rep.turned != 0.0);
        assert!((r.loco.yaw() + 60f32.to_radians()).abs() < 1e-5);
    }

    #[test]
    fn smooth_turn_rate_and_vignette() {
        let s = ComfortSettings {
            turn: TurnMode::Smooth {
                degrees_per_second: 90.0,
            },
            deadzone: 0.0,
            ..Default::default()
        };
        let mut r = rig_at(Vec3::new(0.0, 0.0, 5.0), s);
        let rep = r.run(72, |f| {
            f.hand_mut(Hand::Right).thumbstick = Vec2::new(-1.0, 0.0)
        });
        assert!((r.loco.yaw() - 90f32.to_radians()).abs() < 0.02);
        assert!(rep.vignette > 0.3);
        let rep = r.run(72, |_| {});
        assert!(rep.vignette < 0.01, "vignette fades at rest");
    }

    #[test]
    fn real_crouch_sneaks() {
        let mut r = rig_at(Vec3::new(0.0, 0.0, 5.0), ComfortSettings::default());
        let rep = r.run(20, |f| f.head.position.y = 1.0);
        assert!(rep.crouching);
        let rep = r.run(20, |f| {
            f.head.position.y = 1.0;
            f.hand_mut(Hand::Left).thumbstick = Vec2::Y;
        });
        assert_eq!(rep.gait, Some(Gait::Sneak));
        assert_eq!(r.ctrl.movement_mode(), MovementMode::Sneaking);
    }

    #[test]
    fn seated_player_is_lifted_and_not_crouching() {
        let s = ComfortSettings {
            stance: Stance::Seated,
            ..Default::default()
        };
        let mut r = rig_at(Vec3::new(0.0, 0.0, 5.0), s);
        let rep = r.run(10, |f| f.head.position.y = 1.2);
        assert!(!rep.crouching);
        let head = r.loco.head_world(&r.input).position;
        assert!((head.y - (r.feet().y + 1.7)).abs() < 1e-3);
    }

    #[test]
    fn climbs_the_stairs() {
        let mut r = rig_at(Vec3::new(4.0, 0.0, -1.5), ComfortSettings::default());
        r.run(160, |f| f.hand_mut(Hand::Left).thumbstick = Vec2::Y);
        let top = STAIR_RISE * STAIR_STEPS as f32;
        assert!(r.feet().z < -8.5 && r.feet().z > -11.5, "{}", r.feet());
        assert!((r.feet().y - top).abs() < 0.05, "{}", r.feet());
    }

    #[test]
    fn walkable_ramp_up_steep_ramp_not() {
        let mut r = rig_at(Vec3::new(9.0, 0.0, -1.0), ComfortSettings::default());
        r.run(72 * 2, |f| f.hand_mut(Hand::Left).thumbstick = Vec2::Y);
        assert!(r.feet().y > 1.0, "25 degree ramp climbed: {}", r.feet());

        let mut r = rig_at(Vec3::new(13.0, 0.0, -1.0), ComfortSettings::default());
        r.run(72 * 3, |f| f.hand_mut(Hand::Left).thumbstick = Vec2::Y);
        assert!(r.feet().y < 0.7, "60 degree ramp refused: {}", r.feet());
    }

    #[test]
    fn leaning_into_a_wall_pushes_the_view_back() {
        // Wall plane at x = -3.85 (z -6..0); stand 0.5 m east of it.
        let mut r = rig_at(Vec3::new(-3.35, 0.0, -3.0), ComfortSettings::default());
        let mut rep = LocomotionReport::default();
        for i in 1..=30 {
            let x = -(i as f32) / 30.0;
            rep = r.run(1, |f| f.head.position = Vec3::new(x, 1.7, 0.0));
        }
        assert!(rep.head_blocked.x < -0.01, "{:?}", rep.head_blocked);
        assert!(r.feet().x > -3.85 + 0.3, "capsule stays out: {}", r.feet());
        let head = r.loco.head_world(&r.input).position;
        assert!(head.x > -3.85, "head stays out of the wall: {head}");
    }

    #[test]
    fn teleport_lands_on_the_platform_and_refuses_walls() {
        let mut r = rig_at(Vec3::new(4.0, 0.0, -2.0), ComfortSettings::default());
        // Aim with the right hand forward/up toward the platform (z -8..-12, top 1.44 m).
        let aim = |f: &mut XrFrame| {
            f.hand_mut(Hand::Right).pose = Some(Pose::new(
                Vec3::new(0.2, 1.3, -0.3),
                Quat::from_rotation_x(0.35),
            ));
        };
        let rep = r.run(3, |f| {
            aim(f);
            f.hand_mut(Hand::Right).thumbstick = Vec2::Y;
        });
        let target = rep.teleport_aim.unwrap().target.expect("valid target");
        assert!(
            (target.y - STAIR_RISE * STAIR_STEPS as f32).abs() < 0.02,
            "{target}"
        );
        let rep = r.run(1, aim);
        assert!(rep.teleported);
        assert!(r.feet().distance(target) < 0.05);

        // Straight into the wall: the arc hits a vertical face, no target.
        let world = &r.world;
        let a = teleport_arc(
            world,
            Vec3::new(-3.0, 1.3, -3.0),
            Vec3::NEG_X,
            8.0,
            33.0,
            200.0,
            35.0,
        );
        assert!(a.target.is_none());
        assert!(a.arc.len() >= 2);
    }

    #[test]
    fn locomotion_is_deterministic() {
        let drive = || {
            let mut r = rig_at(Vec3::new(0.0, 0.0, 5.0), ComfortSettings::default());
            for i in 0..200 {
                let mut f = XrFrame::default();
                look(&mut f, (i as f32 * 0.03).sin());
                f.hand_mut(Hand::Left).thumbstick = stick_for(0.3, 0.9);
                f.hand_mut(Hand::Right).thumbstick =
                    Vec2::new(if i % 50 == 0 { 1.0 } else { 0.0 }, 0.0);
                r.input.push(f);
                r.loco.update(&r.input, &mut r.ctrl, &r.world, DT);
            }
            (r.ctrl.position, r.loco.origin)
        };
        assert_eq!(drive(), drive());
    }
}
