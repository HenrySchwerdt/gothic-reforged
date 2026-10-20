//! Movement mode machine and pipeline pieces of `gCCharacterMovement_PS` (batches E01/E02).
//!
//! Pure functions and small state structs, no collision queries: the [`crate::controller`]
//! feeds them probes (ground, water, vertical speed) and applies the modes they decide.
//!
//! * [`MovementMode`] and [`ModeMachine`]: `SetMovementMode` as the single writer of the mode,
//!   `CheckChangedMovementStates` transitions (water, slide, jump apex, landing, lost ground).
//! * [`wish_direction`]: camera-relative key axes to a wish direction (`ProcessMovements`).
//! * [`step_turn`]: the turn-rate model (`CalcNewRotationAngleAndVelocity`).
//! * [`AniVelocity`]: the animation-velocity filter (`CalcAniVelocity`).
//! * [`ground_follow_factor`]: the smoothing of `PutToGround`.
use crate::profile::MovementProfile;
use crate::relax::{relax, relax_vec3};
use glam::Vec3;

/// `gECharMovementMode` (field `+0x100`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
#[repr(u8)]
pub enum MovementMode {
    /// Not yet initialised; becomes [`MovementMode::Standing`] on the first state check.
    #[default]
    Uninitialised = 0,
    Standing = 1,
    Sneaking = 2,
    Walking = 3,
    Running = 4,
    Sprinting = 5,
    Jumping = 6,
    Swimming = 7,
    Diving = 8,
    /// Wading through water above the step height but below the swim depth.
    Wading = 10,
    Sliding = 0xc,
    Falling = 0xd,
    /// Scripted or animation-driven: no movement.
    Disabled = 0xe,
}

impl MovementMode {
    /// The numeric value scripts see (`GetLastMovementMode`, `OnMovementModeChanged`).
    pub fn raw(self) -> u8 {
        self as u8
    }

    /// Mode from the numeric value; modes 9 and 0xb are never produced by the original and map
    /// to `None`.
    pub fn from_raw(v: u8) -> Option<Self> {
        use MovementMode::*;
        Some(match v {
            0 => Uninitialised,
            1 => Standing,
            2 => Sneaking,
            3 => Walking,
            4 => Running,
            5 => Sprinting,
            6 => Jumping,
            7 => Swimming,
            8 => Diving,
            10 => Wading,
            0xc => Sliding,
            0xd => Falling,
            0xe => Disabled,
            _ => return None,
        })
    }

    /// "Movement is self controlled": mode below 0xc (GameDll:0x100e7620).
    pub fn is_self_controlled(self) -> bool {
        (self as u8) < 0xc
    }

    /// Standing, sneaking, walking, running or sprinting: the modes the wished gait may select.
    pub fn is_gait(self) -> bool {
        matches!(self as u8, 1..=5)
    }

    /// Wading, swimming or diving.
    pub fn is_water(self) -> bool {
        matches!(
            self,
            MovementMode::Swimming | MovementMode::Diving | MovementMode::Wading
        )
    }

    /// Whether the character may start a jump from this mode.
    pub fn can_jump(self) -> bool {
        self.is_gait()
    }

    /// Speed modifier of the mode (`SetSpeedByMode`, GameDll:0x100e9390). Swimming and diving
    /// are not listed in the original; they use the walk modifier here (tuned).
    pub fn speed_modifier(self, p: &MovementProfile) -> f32 {
        use MovementMode::*;
        match self {
            Sneaking => p.sneak_mod,
            Walking | Wading | Swimming | Diving => p.slow_mod,
            Sprinting => p.fast_mod,
            Jumping => p.jump_steer,
            Falling => p.fall_steer,
            Disabled => 0.0,
            Uninitialised | Standing | Running | Sliding => 1.0,
        }
    }
}

/// A completed mode change; hosts forward these to `OnMovementModeChanged`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ModeChange {
    pub previous: MovementMode,
    pub mode: MovementMode,
}

/// What `SetMovementMode` needs to know about the surroundings.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct ModeContext {
    /// Angle between the floor normal and up, degrees.
    pub floor_angle_deg: f32,
    /// Height of the floor sample under the character.
    pub floor_y: f32,
}

/// Sensors for [`ModeMachine::check_changed_states`].
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct StateProbe {
    pub dt: f32,
    /// Feet are within the step height of the floor (the original's "grounded" flag).
    pub on_ground: bool,
    /// Feet height.
    pub feet_y: f32,
    /// Floor sample height under the feet.
    pub floor_y: f32,
    pub floor_angle_deg: f32,
    /// Vertical velocity, cm/s.
    pub vertical_velocity: f32,
    /// Water above the feet (0 = dry), cm.
    pub water_depth: f32,
    /// Water column from the floor to the surface, cm.
    pub water_column: f32,
    /// The character is steered by the player (property set 0x16).
    pub player_controlled: bool,
    /// Horizontal speed, cm/s.
    pub speed: f32,
    /// Smoothed ground-slope speed scale.
    pub slope_scale: f32,
}

/// Post-landing grace during which sliding cannot start (`+0x3c8`), seconds.
pub const LANDING_GRACE: f32 = 0.3;
/// Seconds without vertical progress after which a fall counts as landed (`+0x398`).
pub const NO_PROGRESS_LANDING: f32 = 0.3;
/// Vertical velocity below which a jump turns into a fall, cm/s.
pub const JUMP_APEX_VELOCITY: f32 = 0.5;
/// Water depth margin before a falling character switches to swimming, cm.
pub const FALL_SWIM_MARGIN: f32 = 2.0;
/// Slide entry: fraction of the steep band from which a slow character slides.
pub const SLIDE_BAND_FRACTION: f32 = 0.7;
/// Slide entry: speed fraction (of forward max) below which the band rule applies.
pub const SLIDE_SPEED_FRACTION: f32 = 0.3;
/// Slide entry: slope scale at or below which the band rule applies.
pub const SLIDE_MIN_SCALE: f32 = 0.05;
/// Degrees of hysteresis before a slide ends.
pub const SLIDE_EXIT_MARGIN_DEG: f32 = 1.0;

/// Mode machine: `SetMovementMode` is the only writer of [`ModeMachine::mode`].
#[derive(Debug, Clone, Default)]
pub struct ModeMachine {
    mode: MovementMode,
    previous: MovementMode,
    /// Gait the controls ask for (`WishedMovementMode`, `+0xac`).
    wished: MovementMode,
    /// Remaining post-landing grace.
    landing_grace: f32,
    /// Time without vertical progress while falling.
    air_timer: f32,
    last_feet_y: f32,
    /// Seconds spent in the current mode.
    pub time_in_mode: f32,
    /// Height of the floor when the character left the ground (fall damage reference).
    pub fall_start_height: f32,
    /// A sprint-jump keeps sprint steering in the air (`+0x3d1`).
    pub sprint_jump: bool,
    /// Vertical velocity at the end of the last fall (`LastFallVelocity`).
    pub last_fall_velocity: f32,
    mode_before_disable: MovementMode,
    queue: Vec<ModeChange>,
}

impl ModeMachine {
    /// Machine that starts in `mode` without reporting a change (spawn, teleport).
    pub fn with_mode(mode: MovementMode) -> Self {
        Self {
            mode,
            previous: mode,
            ..Self::default()
        }
    }

    /// New machine in [`MovementMode::Uninitialised`].
    pub fn new() -> Self {
        Self::default()
    }

    /// Current mode.
    pub fn mode(&self) -> MovementMode {
        self.mode
    }

    /// Mode before the last change (`GetLastMovementMode`).
    pub fn previous(&self) -> MovementMode {
        self.previous
    }

    /// Gait last requested by the controls.
    pub fn wished(&self) -> MovementMode {
        self.wished
    }

    /// Take the mode changes since the last call, oldest first.
    pub fn drain_changes(&mut self) -> Vec<ModeChange> {
        std::mem::take(&mut self.queue)
    }

    /// Whether sliding is currently blocked by the post-landing grace.
    pub fn in_landing_grace(&self) -> bool {
        self.landing_grace > 0.0
    }

    fn gait_to_return_to(&self) -> MovementMode {
        if self.previous.is_gait() && self.previous != MovementMode::Standing {
            self.previous
        } else {
            MovementMode::Running
        }
    }

    /// `SetMovementMode` (GameDll:0x100e7630). Returns whether the mode changed.
    ///
    /// Standing is refused on floor steeper than `SteepGroundAngleMax` and while swimming or
    /// wading. A change resets the landing/air timers; the sprint-jump flag is set when jumping
    /// out of a sprint and cleared unless the new mode is jump or fall.
    pub fn set_mode(&mut self, to: MovementMode, p: &MovementProfile, ctx: &ModeContext) -> bool {
        if to == self.mode {
            return false;
        }
        if to == MovementMode::Standing
            && (ctx.floor_angle_deg > p.steep_max_deg
                || matches!(self.mode, MovementMode::Swimming | MovementMode::Wading))
        {
            return false;
        }
        if to == MovementMode::Disabled {
            self.mode_before_disable = self.mode;
        }
        if to == MovementMode::Jumping {
            self.sprint_jump = self.mode == MovementMode::Sprinting;
        }
        let from = self.mode;
        if from.is_self_controlled() && matches!(to, MovementMode::Falling | MovementMode::Jumping)
        {
            self.fall_start_height = ctx.floor_y;
        }
        self.previous = from;
        self.mode = to;
        self.landing_grace = 0.0;
        self.air_timer = 0.0;
        self.time_in_mode = 0.0;
        self.last_fall_velocity = 0.0;
        if !matches!(to, MovementMode::Falling | MovementMode::Jumping) {
            self.sprint_jump = false;
        }
        self.queue.push(ModeChange {
            previous: from,
            mode: to,
        });
        true
    }

    /// The character touched ground after a fall or jump: record the impact speed, start the
    /// landing grace and leave the airborne mode (sliding on steep ground, else the last gait).
    pub fn landed(&mut self, impact_vy: f32, steep: bool, p: &MovementProfile, ctx: &ModeContext) {
        let target = if steep {
            MovementMode::Sliding
        } else {
            self.gait_to_return_to()
        };
        self.set_mode(target, p, ctx);
        self.last_fall_velocity = impact_vy;
        if !steep {
            self.landing_grace = LANDING_GRACE;
        }
    }

    /// Set the gait the controls want (`SetWishedMovementMode`); applied only while a gait mode
    /// is active (not in the air, water or while sliding).
    pub fn request_gait(&mut self, wished: MovementMode, p: &MovementProfile, ctx: &ModeContext) {
        if !wished.is_gait() {
            return;
        }
        self.wished = wished;
        if self.mode.is_gait() {
            self.set_mode(wished, p, ctx);
        }
    }

    /// Script/animation control: enter or leave [`MovementMode::Disabled`].
    pub fn set_disabled(&mut self, disabled: bool, p: &MovementProfile, ctx: &ModeContext) {
        if disabled {
            self.set_mode(MovementMode::Disabled, p, ctx);
        } else if self.mode == MovementMode::Disabled {
            let back = if self.mode_before_disable == MovementMode::Disabled {
                MovementMode::Running
            } else {
                self.mode_before_disable
            };
            self.set_mode(back, p, ctx);
        }
    }

    /// Dive request (scripted): swimming and diving toggle.
    pub fn set_diving(&mut self, dive: bool, p: &MovementProfile, ctx: &ModeContext) {
        match (dive, self.mode) {
            (true, MovementMode::Swimming) => {
                self.set_mode(MovementMode::Diving, p, ctx);
            }
            (false, MovementMode::Diving) => {
                self.set_mode(MovementMode::Swimming, p, ctx);
            }
            _ => {}
        }
    }

    /// `CanSlide` (GameDll:0x100ea700): sliding starts on ground steeper than
    /// `SteepGroundAngleMax`, or, once past 70 % of the steep band, for a slow character whose
    /// slope speed scale has run out.
    pub fn can_slide(&self, p: &MovementProfile, probe: &StateProbe) -> bool {
        if !probe.player_controlled
            || self.landing_grace > 0.0
            || self.mode == MovementMode::Swimming
            || self.mode == MovementMode::Diving
        {
            return false;
        }
        let a = probe.floor_angle_deg;
        if a > p.steep_max_deg {
            return true;
        }
        let band = (p.steep_max_deg - p.steep_min_deg).max(1e-3);
        let speed_fraction = if self.mode == MovementMode::Falling {
            1.0
        } else {
            probe.speed / p.forward_max.max(1e-3)
        };
        (a - p.steep_min_deg) / band >= SLIDE_BAND_FRACTION
            && speed_fraction <= SLIDE_SPEED_FRACTION
            && probe.slope_scale <= SLIDE_MIN_SCALE
    }

    /// Water mode for a probe, or `None` for dry land (`CheckChangedMovementStates`).
    fn water_mode(&self, p: &MovementProfile, probe: &StateProbe) -> Option<MovementMode> {
        let depth = probe.water_depth;
        let swim = p.swim_depth;
        let wade = if probe.player_controlled {
            p.step_height
        } else {
            p.wade_depth
        };
        if self.mode == MovementMode::Falling && depth > 0.0 {
            return (swim <= probe.water_column && depth > FALL_SWIM_MARGIN)
                .then_some(MovementMode::Swimming);
        }
        let hysteresis = if self.mode == MovementMode::Swimming {
            p.swim_hysteresis
        } else {
            0.0
        };
        // A deep water body keeps a swimmer swimming while it sinks to the swim line after
        // entering; leaving needs the floor to come up (column below the swim depth).
        let deep = swim <= probe.water_column;
        let swimming = matches!(self.mode, MovementMode::Swimming | MovementMode::Diving);
        let swim_now = (deep && (depth >= wade || swimming)) || depth >= swim - hysteresis;
        if swim_now {
            Some(if self.mode == MovementMode::Diving {
                MovementMode::Diving
            } else {
                MovementMode::Swimming
            })
        } else if depth >= wade {
            Some(MovementMode::Wading)
        } else {
            None
        }
    }

    /// `CheckChangedMovementStates` (GameDll:0x100e5cd0), run once per frame before the movement
    /// is processed.
    pub fn check_changed_states(&mut self, p: &MovementProfile, probe: &StateProbe) {
        let ctx = ModeContext {
            floor_angle_deg: probe.floor_angle_deg,
            floor_y: probe.floor_y,
        };
        let dt = probe.dt;
        self.landing_grace = (self.landing_grace - dt).max(0.0);
        self.time_in_mode += dt;
        if self.mode == MovementMode::Uninitialised {
            self.set_mode(MovementMode::Standing, p, &ctx);
        }
        if self.mode == MovementMode::Disabled {
            self.last_feet_y = probe.feet_y;
            return;
        }

        // Water.
        match self.water_mode(p, probe) {
            Some(w) => {
                if self.mode != MovementMode::Jumping || w == MovementMode::Swimming {
                    self.set_mode(w, p, &ctx);
                }
            }
            None if self.mode.is_water() && self.mode != MovementMode::Wading
                || self.mode == MovementMode::Wading =>
            {
                let back = self.gait_to_return_to();
                self.set_mode(back, p, &ctx);
            }
            None => {}
        }

        // Slide.
        if !matches!(
            self.mode,
            MovementMode::Jumping | MovementMode::Sliding | MovementMode::Falling
        ) && self.can_slide(p, probe)
        {
            self.set_mode(MovementMode::Sliding, p, &ctx);
        }

        match self.mode {
            MovementMode::Jumping if probe.vertical_velocity < JUMP_APEX_VELOCITY => {
                self.set_mode(MovementMode::Falling, p, &ctx);
            }
            MovementMode::Falling => {
                if (probe.feet_y - self.last_feet_y).abs() < 0.5 || probe.vertical_velocity > 0.0 {
                    self.air_timer += dt;
                } else {
                    self.air_timer = 0.0;
                }
                if probe.on_ground || self.air_timer > NO_PROGRESS_LANDING {
                    if self.can_slide(p, probe) {
                        self.set_mode(MovementMode::Sliding, p, &ctx);
                    } else {
                        let back = self.gait_to_return_to();
                        self.set_mode(back, p, &ctx);
                    }
                    self.last_fall_velocity = probe.vertical_velocity;
                    self.landing_grace = LANDING_GRACE;
                }
            }
            MovementMode::Sliding => {
                let leave = probe.floor_angle_deg <= p.steep_max_deg - SLIDE_EXIT_MARGIN_DEG
                    && !self.can_slide(p, probe);
                if !probe.on_ground {
                    self.set_mode(MovementMode::Falling, p, &ctx);
                } else if leave {
                    let back = self.gait_to_return_to();
                    self.set_mode(back, p, &ctx);
                }
            }
            m if (m.is_gait() || m == MovementMode::Wading) && !probe.on_ground => {
                self.set_mode(MovementMode::Falling, p, &ctx);
            }
            _ => {}
        }
        self.last_feet_y = probe.feet_y;
    }

    /// Return to the wished gait after landing or leaving water, or stand when there is no input.
    pub fn settle(&mut self, wish_active: bool, p: &MovementProfile, ctx: &ModeContext) {
        if matches!(self.mode, MovementMode::Running | MovementMode::Standing) && !wish_active {
            self.set_mode(MovementMode::Standing, p, ctx);
        } else if self.mode.is_gait() {
            let want =
                if wish_active && self.wished.is_gait() && self.wished != MovementMode::Standing {
                    self.wished
                } else if wish_active {
                    MovementMode::Running
                } else {
                    MovementMode::Standing
                };
            self.set_mode(want, p, ctx);
        }
    }
}

/// Key axes (`-1..1` or `0..1`) of the movement keys.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct MoveKeys {
    pub forward: f32,
    pub backward: f32,
    pub left: f32,
    pub right: f32,
}

/// Wish direction from the reference frame's forward and right axes
/// (`gCCharacterControl_PS::ProcessMovements`, GameDll:0x100ddd40): the axes are flattened to the
/// horizontal plane when their vertical component is below 0.9999, every pressed key adds its
/// axis, the sum is normalised. Zero without input.
pub fn wish_direction(ref_forward: Vec3, ref_right: Vec3, keys: &MoveKeys) -> Vec3 {
    let flat = |v: Vec3| {
        if v.y.abs() < 0.9999 {
            Vec3::new(v.x, 0.0, v.z).normalize_or_zero()
        } else {
            v
        }
    };
    let (f, r) = (flat(ref_forward), flat(ref_right));
    let sum = f * (keys.forward - keys.backward) + r * (keys.right - keys.left);
    sum.normalize_or_zero()
}

/// State of the turn-rate model.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct TurnState {
    /// Angular velocity, degrees per second (positive turns toward positive angle differences).
    pub velocity_deg: f32,
}

/// Wrap an angle in degrees to `(-180, 180]`.
pub fn wrap_deg(a: f32) -> f32 {
    let mut a = a % 360.0;
    if a > 180.0 {
        a -= 360.0;
    } else if a <= -180.0 {
        a += 360.0;
    }
    a
}

/// One step of the turn model (`CalcNewRotationAngleAndVelocity`, GameDll:0x100e49f0).
///
/// `angle_deg` is the signed remaining angle to the goal. Returns the angle to turn this frame
/// (degrees, same sign convention) and updates the angular velocity. The velocity accelerates
/// toward the sign of the remaining angle with `TurnAcceleration` (`TurnDecceleration` when
/// reversing), is clamped to `TurnSpeedMax` (times `TurnSpeedModifier` while sprinting), the step
/// is the mean of old and new velocity times `dt`, clipped so it never overshoots; below one
/// degree the character snaps to the goal. With `brake` the velocity decreases once the
/// remaining angle is below `v^2 / (2 * decel)`.
pub fn step_turn(
    state: &mut TurnState,
    angle_deg: f32,
    dt: f32,
    p: &MovementProfile,
    sprinting: bool,
    brake: bool,
) -> f32 {
    let angle = wrap_deg(angle_deg);
    if angle.abs() < 1.0 {
        state.velocity_deg = if dt > 0.0 { angle / dt } else { 0.0 };
        return angle;
    }
    let old = state.velocity_deg;
    let sign = angle.signum();
    let mut v = if brake && angle.abs() < old * old / (2.0 * p.turn_decel_deg.max(1e-3)) {
        old - old.signum() * dt * p.turn_decel_deg
    } else {
        let reversing = old * sign < 0.0;
        let rate = if reversing {
            p.turn_decel_deg
        } else {
            p.turn_accel_deg
        };
        old + sign * dt * rate
    };
    let mut max = p.turn_speed_max_deg;
    if sprinting {
        max *= p.turn_speed_sprint_mod;
    }
    v = v.clamp(-max, max);
    let mut step = wrap_deg((v + old) * 0.5 * dt);
    if angle > 0.0 {
        if step > angle {
            step = angle;
            v = 0.0;
        } else if step < 0.0 {
            step = 0.0;
            v = 0.0;
        }
    } else if step < angle {
        step = angle;
        v = 0.0;
    } else if step > 0.0 {
        step = 0.0;
        v = 0.0;
    }
    state.velocity_deg = v;
    step
}

/// Smoothed world velocity feeding the locomotion mixer (`CalcAniVelocity`, GameDll:0x100e1cd0):
/// displacement per second, relaxed with `AniVelocityInertia` (5 x 0.025 s), minus the velocity
/// of the floor entity when standing on a mover.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct AniVelocity {
    pub velocity: Vec3,
}

impl AniVelocity {
    /// Feed the displacement of the last frame.
    pub fn update(&mut self, displacement: Vec3, floor_velocity: Vec3, dt: f32, inertia: f32) {
        if dt <= 0.0 {
            return;
        }
        let raw = displacement / dt - floor_velocity;
        self.velocity = if inertia > 0.0 {
            relax_vec3(self.velocity, raw, dt, inertia)
        } else {
            raw
        };
    }

    /// Velocity in a heading frame: (forward, right) components for yaw `heading` (radians,
    /// forward is -Z rotated by the yaw about Y).
    pub fn local(&self, heading: f32) -> (f32, f32) {
        let fwd = Vec3::new(-heading.sin(), 0.0, -heading.cos());
        let right = Vec3::new(heading.cos(), 0.0, -heading.sin());
        (self.velocity.dot(fwd), self.velocity.dot(right))
    }
}

/// Relax factor of the foot-height filter in `PutToGround` (GameDll:0x100e94f0): 10 when
/// standing still, 2 at full speed and while jumping (x0.025 s).
pub fn ground_follow_factor(speed: f32, forward_max: f32, jumping: bool) -> f32 {
    if jumping {
        return 2.0;
    }
    let s = if forward_max > 0.0 {
        (speed / forward_max).clamp(0.0, 1.0)
    } else {
        1.0
    };
    2.0 + 8.0 * (1.0 - s)
}

/// Filter the foot height toward the sampled floor.
pub fn follow_ground(current_y: f32, floor_y: f32, dt: f32, factor: f32) -> f32 {
    relax(current_y, floor_y, dt, factor)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn hero() -> MovementProfile {
        MovementProfile::hero()
    }

    fn probe() -> StateProbe {
        StateProbe {
            dt: 1.0 / 60.0,
            on_ground: true,
            ..StateProbe::default()
        }
    }

    fn machine_in(mode: MovementMode) -> ModeMachine {
        let mut m = ModeMachine::new();
        m.check_changed_states(&hero(), &probe());
        m.mode = mode;
        m.drain_changes();
        m
    }

    #[test]
    fn raw_values_match_the_original_enum() {
        assert_eq!(MovementMode::Sliding.raw(), 0xc);
        assert_eq!(MovementMode::Falling.raw(), 0xd);
        assert_eq!(MovementMode::Wading.raw(), 10);
        assert_eq!(MovementMode::from_raw(9), None);
        assert_eq!(MovementMode::from_raw(0xb), None);
        assert!(MovementMode::Jumping.is_self_controlled());
        assert!(!MovementMode::Sliding.is_self_controlled());
    }

    #[test]
    fn first_check_initialises_to_standing_and_reports_it() {
        let mut m = ModeMachine::new();
        m.check_changed_states(&hero(), &probe());
        assert_eq!(m.mode(), MovementMode::Standing);
        assert_eq!(
            m.drain_changes(),
            vec![ModeChange {
                previous: MovementMode::Uninitialised,
                mode: MovementMode::Standing
            }]
        );
        assert!(m.drain_changes().is_empty());
    }

    #[test]
    fn gait_requests_apply_only_on_the_ground() {
        let p = hero();
        let ctx = ModeContext::default();
        let mut m = machine_in(MovementMode::Standing);
        m.request_gait(MovementMode::Sprinting, &p, &ctx);
        assert_eq!(m.mode(), MovementMode::Sprinting);
        m.mode = MovementMode::Jumping;
        m.request_gait(MovementMode::Walking, &p, &ctx);
        assert_eq!(m.mode(), MovementMode::Jumping);
        assert_eq!(m.wished(), MovementMode::Walking);
    }

    #[test]
    fn standing_is_refused_on_steep_floor_and_in_water() {
        let p = hero();
        let mut m = machine_in(MovementMode::Running);
        let steep = ModeContext {
            floor_angle_deg: 50.0,
            floor_y: 0.0,
        };
        assert!(!m.set_mode(MovementMode::Standing, &p, &steep));
        let mut m = machine_in(MovementMode::Swimming);
        assert!(!m.set_mode(MovementMode::Standing, &p, &ModeContext::default()));
    }

    #[test]
    fn jump_ends_at_the_apex() {
        let p = hero();
        let mut m = machine_in(MovementMode::Running);
        m.set_mode(MovementMode::Jumping, &p, &ModeContext::default());
        let mut pr = probe();
        pr.on_ground = false;
        pr.vertical_velocity = 300.0;
        m.check_changed_states(&p, &pr);
        assert_eq!(m.mode(), MovementMode::Jumping);
        pr.vertical_velocity = 0.2;
        m.check_changed_states(&p, &pr);
        assert_eq!(m.mode(), MovementMode::Falling);
    }

    #[test]
    fn sprint_jump_sets_and_clears_the_flag() {
        let p = hero();
        let mut m = machine_in(MovementMode::Sprinting);
        m.set_mode(MovementMode::Jumping, &p, &ModeContext::default());
        assert!(m.sprint_jump);
        m.set_mode(MovementMode::Falling, &p, &ModeContext::default());
        assert!(m.sprint_jump);
        m.set_mode(MovementMode::Running, &p, &ModeContext::default());
        assert!(!m.sprint_jump);
    }

    #[test]
    fn landing_returns_to_a_gait_and_starts_the_grace() {
        let p = hero();
        let mut m = machine_in(MovementMode::Falling);
        let mut pr = probe();
        pr.on_ground = true;
        pr.vertical_velocity = -900.0;
        m.check_changed_states(&p, &pr);
        assert_eq!(m.mode(), MovementMode::Running);
        assert_eq!(m.last_fall_velocity, -900.0);
        assert!(m.in_landing_grace());
        // No slide inside the grace window even on steep ground.
        pr.floor_angle_deg = 55.0;
        pr.player_controlled = true;
        m.check_changed_states(&p, &pr);
        assert_ne!(m.mode(), MovementMode::Sliding);
        for _ in 0..30 {
            m.check_changed_states(&p, &pr);
        }
        assert_eq!(m.mode(), MovementMode::Sliding);
    }

    #[test]
    fn falling_without_vertical_progress_lands_after_0_3_seconds() {
        let p = hero();
        let mut m = machine_in(MovementMode::Falling);
        let mut pr = probe();
        pr.on_ground = false;
        pr.vertical_velocity = 0.0;
        for _ in 0..17 {
            m.check_changed_states(&p, &pr);
        }
        assert_eq!(m.mode(), MovementMode::Falling);
        for _ in 0..3 {
            m.check_changed_states(&p, &pr);
            if m.mode() != MovementMode::Falling {
                break;
            }
        }
        assert_eq!(m.mode(), MovementMode::Running);
    }

    #[test]
    fn walking_off_a_ledge_falls() {
        let p = hero();
        let mut m = machine_in(MovementMode::Running);
        let mut pr = probe();
        pr.on_ground = false;
        m.check_changed_states(&p, &pr);
        assert_eq!(m.mode(), MovementMode::Falling);
        assert_eq!(m.previous(), MovementMode::Running);
    }

    #[test]
    fn slide_thresholds_follow_the_steep_band() {
        let p = hero();
        let m = machine_in(MovementMode::Running);
        let mut pr = probe();
        pr.player_controlled = true;
        pr.floor_angle_deg = 48.0;
        pr.speed = 400.0;
        pr.slope_scale = 0.1;
        assert!(
            !m.can_slide(&p, &pr),
            "fast character keeps walking below the maximum"
        );
        pr.floor_angle_deg = 49.5;
        assert!(m.can_slide(&p, &pr));
        // 70 % of the 20..49 band is 40.3 degrees: slow character with no slope scale left slides.
        pr.floor_angle_deg = 41.0;
        pr.speed = 100.0;
        pr.slope_scale = 0.0;
        assert!(m.can_slide(&p, &pr));
        pr.player_controlled = false;
        assert!(!m.can_slide(&p, &pr));
    }

    #[test]
    fn water_thresholds_wade_swim_and_hysteresis() {
        let p = hero();
        let mut m = machine_in(MovementMode::Running);
        let mut pr = probe();
        pr.player_controlled = true;
        pr.water_column = 400.0;
        pr.water_depth = 50.0;
        m.check_changed_states(&p, &pr);
        assert_eq!(
            m.mode(),
            MovementMode::Running,
            "below the 65 cm step height: dry"
        );
        pr.water_depth = 70.0;
        m.check_changed_states(&p, &pr);
        // Deep column and depth above the step height: swimming starts.
        assert_eq!(m.mode(), MovementMode::Swimming);
        pr.water_depth = 150.0;
        m.check_changed_states(&p, &pr);
        assert_eq!(
            m.mode(),
            MovementMode::Swimming,
            "10 cm hysteresis while swimming"
        );
        pr.water_depth = 100.0;
        pr.water_column = 100.0;
        m.check_changed_states(&p, &pr);
        assert_eq!(m.mode(), MovementMode::Wading);
        pr.water_depth = 10.0;
        m.check_changed_states(&p, &pr);
        assert_eq!(m.mode(), MovementMode::Running);
    }

    #[test]
    fn falling_into_deep_water_swims_after_a_two_unit_margin() {
        let p = hero();
        let mut m = machine_in(MovementMode::Falling);
        let mut pr = probe();
        pr.on_ground = false;
        pr.vertical_velocity = -500.0;
        pr.feet_y = 100.0;
        pr.player_controlled = true;
        pr.water_column = 300.0;
        pr.water_depth = 1.0;
        m.check_changed_states(&p, &pr);
        assert_eq!(m.mode(), MovementMode::Falling);
        pr.water_depth = 3.0;
        m.check_changed_states(&p, &pr);
        assert_eq!(m.mode(), MovementMode::Swimming);
    }

    #[test]
    fn diving_toggles_from_swimming_only() {
        let p = hero();
        let ctx = ModeContext::default();
        let mut m = machine_in(MovementMode::Running);
        m.set_diving(true, &p, &ctx);
        assert_eq!(m.mode(), MovementMode::Running);
        m.mode = MovementMode::Swimming;
        m.set_diving(true, &p, &ctx);
        assert_eq!(m.mode(), MovementMode::Diving);
        m.set_diving(false, &p, &ctx);
        assert_eq!(m.mode(), MovementMode::Swimming);
    }

    #[test]
    fn disabled_mode_restores_the_previous_mode() {
        let p = hero();
        let ctx = ModeContext::default();
        let mut m = machine_in(MovementMode::Walking);
        m.set_disabled(true, &p, &ctx);
        assert_eq!(m.mode(), MovementMode::Disabled);
        m.check_changed_states(&p, &probe());
        assert_eq!(m.mode(), MovementMode::Disabled);
        m.set_disabled(false, &p, &ctx);
        assert_eq!(m.mode(), MovementMode::Walking);
    }

    #[test]
    fn mode_speed_modifiers_give_the_hero_gait_speeds() {
        let p = hero();
        let speed = |m: MovementMode| p.forward_max * m.speed_modifier(&p);
        assert_eq!(speed(MovementMode::Running), 400.0);
        assert_eq!(speed(MovementMode::Sprinting), 600.0);
        assert!((speed(MovementMode::Walking) - 150.0).abs() < 1e-3);
        assert!((speed(MovementMode::Sneaking) - 280.0).abs() < 1e-3);
        assert_eq!(speed(MovementMode::Disabled), 0.0);
    }

    #[test]
    fn wish_direction_flattens_and_normalises() {
        let fwd = Vec3::new(0.0, -0.5, -1.0).normalize();
        let right = Vec3::X;
        let w = wish_direction(
            fwd,
            right,
            &MoveKeys {
                forward: 1.0,
                ..Default::default()
            },
        );
        assert!((w - Vec3::NEG_Z).length() < 1e-6);
        let d = wish_direction(
            fwd,
            right,
            &MoveKeys {
                forward: 1.0,
                right: 1.0,
                ..Default::default()
            },
        );
        assert!((d.length() - 1.0).abs() < 1e-6);
        assert!((d.x - d.z.abs()).abs() < 1e-6);
        assert_eq!(wish_direction(fwd, right, &MoveKeys::default()), Vec3::ZERO);
        // Opposing keys cancel.
        let z = wish_direction(
            fwd,
            right,
            &MoveKeys {
                forward: 1.0,
                backward: 1.0,
                ..Default::default()
            },
        );
        assert_eq!(z, Vec3::ZERO);
    }

    #[test]
    fn turn_rate_is_200_degrees_per_second_and_100_while_sprinting() {
        let p = hero();
        for (sprint, rate) in [(false, 200.0), (true, 100.0)] {
            let mut st = TurnState::default();
            let mut remaining = 170.0_f32;
            let mut t = 0.0;
            let dt = 1.0 / 60.0;
            while remaining > 0.5 && t < 5.0 {
                let step = step_turn(&mut st, remaining, dt, &p, sprint, false);
                remaining -= step;
                t += dt;
            }
            let expected = 170.0 / rate;
            assert!(
                (t - expected).abs() < 0.08,
                "sprint={sprint}: {t} vs {expected}"
            );
        }
    }

    #[test]
    fn turn_never_overshoots_and_snaps_inside_one_degree() {
        let p = hero();
        let mut st = TurnState::default();
        let s = step_turn(&mut st, 0.4, 1.0 / 60.0, &p, false, false);
        assert!((s - 0.4).abs() < 1e-6);
        let mut st = TurnState {
            velocity_deg: 200.0,
        };
        let s = step_turn(&mut st, 2.0, 1.0 / 30.0, &p, false, false);
        assert!(s <= 2.0 + 1e-4, "{s}");
        assert_eq!(st.velocity_deg, 0.0);
        // Negative direction mirrors.
        let mut st = TurnState::default();
        assert!(step_turn(&mut st, -90.0, 1.0 / 60.0, &p, false, false) < 0.0);
    }

    #[test]
    fn turn_braking_slows_down_before_the_goal() {
        let mut p = hero();
        p.turn_decel_deg = 400.0;
        let mut st = TurnState {
            velocity_deg: 200.0,
        };
        // v^2 / (2 d) = 200^2 / 800 = 50 degrees of braking distance.
        let before = st.velocity_deg;
        step_turn(&mut st, 30.0, 1.0 / 60.0, &p, false, true);
        assert!(st.velocity_deg < before);
    }

    #[test]
    fn ani_velocity_converges_with_the_125_ms_filter() {
        let mut a = AniVelocity::default();
        let dt = 1.0 / 60.0;
        let step = Vec3::new(0.0, 0.0, -400.0 * dt);
        // After 0.125 s (one time constant) ~63 % of the final velocity (boost makes it a bit faster).
        for _ in 0..8 {
            a.update(step, Vec3::ZERO, dt, 5.0);
        }
        let v = -a.velocity.z;
        assert!(v > 400.0 * 0.6 && v < 400.0 * 0.9, "{v}");
        for _ in 0..120 {
            a.update(step, Vec3::ZERO, dt, 5.0);
        }
        assert!((-a.velocity.z - 400.0).abs() < 1.0);
        let (f, r) = a.local(0.0);
        assert!((f - 400.0).abs() < 1.0 && r.abs() < 1e-3);
        // Floor movers are subtracted.
        let mut b = AniVelocity::default();
        for _ in 0..200 {
            b.update(step, Vec3::new(0.0, 0.0, -400.0), dt, 5.0);
        }
        assert!(b.velocity.length() < 1.0);
    }

    #[test]
    fn ground_follow_is_tight_when_fast_and_soft_when_still() {
        assert_eq!(ground_follow_factor(0.0, 400.0, false), 10.0);
        assert_eq!(ground_follow_factor(400.0, 400.0, false), 2.0);
        assert_eq!(ground_follow_factor(0.0, 400.0, true), 2.0);
        let dt = 1.0 / 60.0;
        let still = follow_ground(0.0, 30.0, dt, ground_follow_factor(0.0, 400.0, false));
        let fast = follow_ground(0.0, 30.0, dt, ground_follow_factor(400.0, 400.0, false));
        assert!(fast > still * 3.0, "{fast} {still}");
    }

    #[test]
    fn directional_max_blends_linearly_by_angle() {
        let p = hero();
        assert_eq!(p.directional_max(0.0, 1.0, 1.0, false), 400.0);
        // 45 degrees: halfway between forward 400 and strafe 280.
        let (s, f) = (
            std::f32::consts::FRAC_1_SQRT_2,
            std::f32::consts::FRAC_1_SQRT_2,
        );
        assert!((p.directional_max(s, f, 1.0, false) - 340.0).abs() < 0.5);
        // 135 degrees: halfway between strafe 280 and backward 280.
        assert!((p.directional_max(s, -f, 1.0, false) - 280.0).abs() < 0.5);
        // Sprint scales only the forward part: pure strafe stays 280, forward becomes 600.
        assert_eq!(p.directional_max(1.0, 0.0, 1.5, true), 280.0);
        assert_eq!(p.directional_max(0.0, 1.0, 1.5, true), 600.0);
        assert!((p.directional_max(0.0, 1.0, 0.375, false) - 150.0).abs() < 1e-3);
    }
}
