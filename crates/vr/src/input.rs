//! Bevy-free model of one XR input frame: the tracked head, two tracked controllers and their
//! buttons, plus edge detection with analog hysteresis.
//!
//! Every backend (OpenXR, the desktop fallback, a scripted CI driver, a recorded fixture) only
//! has to produce an [`XrFrame`]; everything downstream (locomotion, body IK, physics hands,
//! runtime commands) reads [`VrInput`]. Positions are metres in *tracking space* (the play
//! area: origin on the floor at the calibrated centre, Y up, -Z forward), rotations are
//! tracking-space orientations.
use glam::{Quat, Vec2, Vec3};
use serde::{Deserialize, Serialize};

/// A rigid pose in tracking space (metres).
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Pose {
    pub position: Vec3,
    pub rotation: Quat,
}

impl Default for Pose {
    fn default() -> Self {
        Self::IDENTITY
    }
}

impl Pose {
    pub const IDENTITY: Self = Self {
        position: Vec3::ZERO,
        rotation: Quat::IDENTITY,
    };

    pub fn new(position: Vec3, rotation: Quat) -> Self {
        Self { position, rotation }
    }

    /// -Z of the pose.
    pub fn forward(&self) -> Vec3 {
        self.rotation * Vec3::NEG_Z
    }

    pub fn right(&self) -> Vec3 {
        self.rotation * Vec3::X
    }

    pub fn up(&self) -> Vec3 {
        self.rotation * Vec3::Y
    }

    /// `self` applied after `inner` (parent * child).
    pub fn mul(&self, inner: &Pose) -> Pose {
        Pose {
            position: self.position + self.rotation * inner.position,
            rotation: (self.rotation * inner.rotation).normalize(),
        }
    }

    pub fn transform_point(&self, p: Vec3) -> Vec3 {
        self.position + self.rotation * p
    }

    pub fn inverse(&self) -> Pose {
        let r = self.rotation.inverse();
        Pose {
            position: r * -self.position,
            rotation: r,
        }
    }

    /// Yaw of the forward vector about +Y (0 = looking down -Z, positive turns left).
    pub fn yaw(&self) -> f32 {
        let f = self.forward();
        (-f.x).atan2(-f.z)
    }

    pub fn is_finite(&self) -> bool {
        self.position.is_finite() && self.rotation.is_finite()
    }
}

/// Which hand.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Hand {
    Left,
    Right,
}

impl Hand {
    pub const BOTH: [Hand; 2] = [Hand::Left, Hand::Right];

    pub fn index(self) -> usize {
        match self {
            Hand::Left => 0,
            Hand::Right => 1,
        }
    }

    pub fn other(self) -> Hand {
        match self {
            Hand::Left => Hand::Right,
            Hand::Right => Hand::Left,
        }
    }

    /// +1 for the right hand, -1 for the left (mirrors offsets).
    pub fn sign(self) -> f32 {
        match self {
            Hand::Left => -1.0,
            Hand::Right => 1.0,
        }
    }
}

/// Digital view of a controller's inputs (analog ones pass the hysteresis first).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum VrButton {
    Trigger,
    Grip,
    /// A (right) / X (left).
    Primary,
    /// B (right) / Y (left).
    Secondary,
    StickClick,
    Menu,
}

impl VrButton {
    pub const ALL: [VrButton; 6] = [
        VrButton::Trigger,
        VrButton::Grip,
        VrButton::Primary,
        VrButton::Secondary,
        VrButton::StickClick,
        VrButton::Menu,
    ];

    fn index(self) -> usize {
        self as usize
    }
}

/// Raw state of one tracked controller in one frame.
#[derive(Debug, Clone, Copy, PartialEq, Default, Serialize, Deserialize)]
pub struct ControllerState {
    /// Grip pose; `None` while tracking is lost.
    pub pose: Option<Pose>,
    /// Linear velocity reported by the runtime (m/s, tracking space), if any.
    pub linear_velocity: Option<Vec3>,
    /// Angular velocity reported by the runtime (rad/s as a scaled axis, tracking space), if
    /// any.
    #[serde(default)]
    pub angular_velocity: Option<Vec3>,
    /// 0..1.
    pub trigger: f32,
    /// 0..1.
    pub grip: f32,
    /// -1..1 each axis, +Y is "stick forward".
    pub thumbstick: Vec2,
    pub primary: bool,
    pub secondary: bool,
    pub stick_click: bool,
    pub menu: bool,
}

/// Everything a backend reports for one displayed frame.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct XrFrame {
    pub head: Pose,
    /// `[left, right]`.
    pub hands: [ControllerState; 2],
}

impl Default for XrFrame {
    fn default() -> Self {
        Self {
            head: Pose::new(Vec3::new(0.0, 1.7, 0.0), Quat::IDENTITY),
            hands: [ControllerState::default(); 2],
        }
    }
}

impl XrFrame {
    pub fn hand(&self, hand: Hand) -> &ControllerState {
        &self.hands[hand.index()]
    }

    pub fn hand_mut(&mut self, hand: Hand) -> &mut ControllerState {
        &mut self.hands[hand.index()]
    }

    /// Replace non-finite values (a broken runtime or fixture) so nothing downstream can be
    /// poisoned by NaN: a non-finite head keeps `fallback.head`, a non-finite hand pose is
    /// treated as lost tracking and analog values are clamped.
    pub fn sanitized(mut self, fallback: &XrFrame) -> XrFrame {
        if !self.head.is_finite() {
            self.head = fallback.head;
        }
        self.head.rotation = self.head.rotation.normalize();
        for h in &mut self.hands {
            if h.pose.is_some_and(|p| !p.is_finite()) {
                h.pose = None;
            }
            if let Some(p) = &mut h.pose {
                p.rotation = p.rotation.normalize();
            }
            if h.linear_velocity.is_some_and(|v| !v.is_finite()) {
                h.linear_velocity = None;
            }
            if h.angular_velocity.is_some_and(|v| !v.is_finite()) {
                h.angular_velocity = None;
            }
            let fin = |v: f32| if v.is_finite() { v } else { 0.0 };
            h.trigger = fin(h.trigger).clamp(0.0, 1.0);
            h.grip = fin(h.grip).clamp(0.0, 1.0);
            h.thumbstick = Vec2::new(fin(h.thumbstick.x), fin(h.thumbstick.y))
                .clamp(Vec2::splat(-1.0), Vec2::splat(1.0));
        }
        self
    }
}

/// Press / release thresholds of the analog trigger and grip.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct AnalogThresholds {
    pub press: f32,
    pub release: f32,
}

impl Default for AnalogThresholds {
    fn default() -> Self {
        Self {
            press: 0.75,
            release: 0.55,
        }
    }
}

/// Current and previous frame with digital button states (hysteresis applied).
#[derive(Debug, Clone, Default)]
pub struct VrInput {
    pub current: XrFrame,
    pub previous: XrFrame,
    pub thresholds: AnalogThresholds,
    down: [[bool; 6]; 2],
    was_down: [[bool; 6]; 2],
    /// Frames received so far.
    pub frames: u64,
}

impl VrInput {
    pub fn new() -> Self {
        Self::default()
    }

    /// Feed the next frame.
    pub fn push(&mut self, frame: XrFrame) {
        let frame = frame.sanitized(&self.current);
        self.previous = if self.frames == 0 {
            frame
        } else {
            self.current
        };
        self.current = frame;
        self.was_down = self.down;
        for hand in Hand::BOTH {
            let c = frame.hand(hand);
            let d = &mut self.down[hand.index()];
            let analog = |held: bool, v: f32, t: &AnalogThresholds| {
                if held { v > t.release } else { v >= t.press }
            };
            d[VrButton::Trigger.index()] =
                analog(d[VrButton::Trigger.index()], c.trigger, &self.thresholds);
            d[VrButton::Grip.index()] = analog(d[VrButton::Grip.index()], c.grip, &self.thresholds);
            d[VrButton::Primary.index()] = c.primary;
            d[VrButton::Secondary.index()] = c.secondary;
            d[VrButton::StickClick.index()] = c.stick_click;
            d[VrButton::Menu.index()] = c.menu;
        }
        self.frames += 1;
    }

    pub fn held(&self, hand: Hand, button: VrButton) -> bool {
        self.down[hand.index()][button.index()]
    }

    pub fn just_pressed(&self, hand: Hand, button: VrButton) -> bool {
        self.down[hand.index()][button.index()] && !self.was_down[hand.index()][button.index()]
    }

    pub fn just_released(&self, hand: Hand, button: VrButton) -> bool {
        !self.down[hand.index()][button.index()] && self.was_down[hand.index()][button.index()]
    }

    pub fn head(&self) -> Pose {
        self.current.head
    }

    pub fn hand_pose(&self, hand: Hand) -> Option<Pose> {
        self.current.hand(hand).pose
    }

    pub fn stick(&self, hand: Hand) -> Vec2 {
        self.current.hand(hand).thumbstick
    }

    /// Hand velocity in tracking space: the runtime's value, else the frame difference.
    pub fn hand_velocity(&self, hand: Hand, dt: f32) -> Vec3 {
        let c = self.current.hand(hand);
        if let Some(v) = c.linear_velocity {
            return v;
        }
        match (c.pose, self.previous.hand(hand).pose) {
            (Some(a), Some(b)) if dt > 0.0 => (a.position - b.position) / dt,
            _ => Vec3::ZERO,
        }
    }

    /// Hand angular velocity in tracking space (scaled axis, rad/s): the runtime's value, else
    /// the rotation between the last two frames.
    pub fn hand_angular_velocity(&self, hand: Hand, dt: f32) -> Vec3 {
        let c = self.current.hand(hand);
        if let Some(w) = c.angular_velocity {
            return w;
        }
        match (c.pose, self.previous.hand(hand).pose) {
            (Some(a), Some(b)) if dt > 0.0 => rotation_between(b.rotation, a.rotation) / dt,
            _ => Vec3::ZERO,
        }
    }
}

/// Shortest rotation from `from` to `to` as a scaled axis (radians).
pub fn rotation_between(from: Quat, to: Quat) -> Vec3 {
    let mut d = to * from.inverse();
    if d.w < 0.0 {
        d = -d;
    }
    let (axis, angle) = d.normalize().to_axis_angle();
    if angle.is_finite() && axis.is_finite() {
        axis * angle
    } else {
        Vec3::ZERO
    }
}

/// Radial deadzone with rescaling so the output still reaches 1.
pub fn apply_deadzone(stick: Vec2, deadzone: f32) -> Vec2 {
    let len = stick.length();
    if len <= deadzone || len <= f32::EPSILON {
        return Vec2::ZERO;
    }
    let scaled = ((len - deadzone) / (1.0 - deadzone)).min(1.0);
    stick / len * scaled
}

#[cfg(test)]
mod tests {
    use super::*;

    fn frame_with(f: impl FnOnce(&mut XrFrame)) -> XrFrame {
        let mut fr = XrFrame::default();
        f(&mut fr);
        fr
    }

    #[test]
    fn trigger_hysteresis() {
        let mut i = VrInput::new();
        let t = |v: f32| frame_with(|f| f.hand_mut(Hand::Right).trigger = v);
        i.push(t(0.7));
        assert!(!i.held(Hand::Right, VrButton::Trigger));
        i.push(t(0.8));
        assert!(i.just_pressed(Hand::Right, VrButton::Trigger));
        i.push(t(0.6));
        assert!(
            i.held(Hand::Right, VrButton::Trigger),
            "above release stays held"
        );
        assert!(!i.just_pressed(Hand::Right, VrButton::Trigger));
        i.push(t(0.5));
        assert!(i.just_released(Hand::Right, VrButton::Trigger));
        assert!(!i.held(Hand::Left, VrButton::Trigger));
    }

    #[test]
    fn digital_edges() {
        let mut i = VrInput::new();
        i.push(frame_with(|f| f.hand_mut(Hand::Left).primary = true));
        assert!(i.just_pressed(Hand::Left, VrButton::Primary));
        i.push(frame_with(|f| f.hand_mut(Hand::Left).primary = true));
        assert!(i.held(Hand::Left, VrButton::Primary));
        assert!(!i.just_pressed(Hand::Left, VrButton::Primary));
        i.push(XrFrame::default());
        assert!(i.just_released(Hand::Left, VrButton::Primary));
    }

    #[test]
    fn non_finite_input_is_sanitized() {
        let mut i = VrInput::new();
        i.push(XrFrame::default());
        let bad = frame_with(|f| {
            f.head.position = Vec3::NAN;
            f.hand_mut(Hand::Left).pose = Some(Pose::new(Vec3::INFINITY, Quat::IDENTITY));
            f.hand_mut(Hand::Left).trigger = f32::NAN;
            f.hand_mut(Hand::Right).thumbstick = Vec2::new(5.0, -5.0);
        });
        i.push(bad);
        assert!(i.head().is_finite());
        assert_eq!(i.head(), XrFrame::default().head);
        assert!(i.hand_pose(Hand::Left).is_none());
        assert_eq!(i.current.hand(Hand::Left).trigger, 0.0);
        assert_eq!(i.stick(Hand::Right), Vec2::new(1.0, -1.0));
    }

    #[test]
    fn velocity_falls_back_to_difference() {
        let mut i = VrInput::new();
        let at = |x: f32| {
            frame_with(|f| {
                f.hand_mut(Hand::Right).pose = Some(Pose::new(Vec3::X * x, Quat::IDENTITY))
            })
        };
        i.push(at(0.0));
        i.push(at(0.1));
        assert!((i.hand_velocity(Hand::Right, 0.1) - Vec3::X).length() < 1e-5);
        assert_eq!(i.hand_velocity(Hand::Left, 0.1), Vec3::ZERO);
    }

    #[test]
    fn angular_velocity_from_runtime_or_difference() {
        let mut i = VrInput::new();
        let at = |a: f32| {
            frame_with(|f| {
                f.hand_mut(Hand::Right).pose = Some(Pose::new(Vec3::ZERO, Quat::from_rotation_y(a)))
            })
        };
        i.push(at(0.0));
        i.push(at(0.2));
        let w = i.hand_angular_velocity(Hand::Right, 0.1);
        assert!((w - Vec3::Y * 2.0).length() < 1e-4, "{w}");
        let mut f = at(0.2);
        f.hand_mut(Hand::Right).angular_velocity = Some(Vec3::X * 5.0);
        i.push(f);
        assert_eq!(i.hand_angular_velocity(Hand::Right, 0.1), Vec3::X * 5.0);
        // Broken runtime values are dropped.
        f.hand_mut(Hand::Right).angular_velocity = Some(Vec3::NAN);
        assert_eq!(f.sanitized(&f).hand(Hand::Right).angular_velocity, None);
    }

    #[test]
    fn deadzone_rescales() {
        assert_eq!(apply_deadzone(Vec2::new(0.1, 0.0), 0.15), Vec2::ZERO);
        let full = apply_deadzone(Vec2::new(0.0, 1.0), 0.15);
        assert!((full.y - 1.0).abs() < 1e-6);
        let half = apply_deadzone(Vec2::new(0.575, 0.0), 0.15);
        assert!((half.x - 0.5).abs() < 1e-3);
    }

    #[test]
    fn pose_algebra() {
        let p = Pose::new(Vec3::new(1.0, 2.0, 3.0), Quat::from_rotation_y(0.7));
        let id = p.mul(&p.inverse());
        assert!(id.position.length() < 1e-5);
        assert!(id.rotation.angle_between(Quat::IDENTITY) < 1e-4);
        assert!((p.yaw() - 0.7).abs() < 1e-5);
    }
}
