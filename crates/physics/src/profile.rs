//! Movement parameters of a character, taken from the original template data.
//!
//! Every number in [`MovementProfile::hero`] is the value stored in the shipped
//! `Humans_Master_PC_Hero.tple` (`gCCharacterMovement_PS`, plus the first capsule of its
//! `eCCollisionShape_PS`) or the engine's scene constants; the tag after each field gives
//! its source (`V` = read from game data / instructions, `I` = inferred, `T` = tuned, no
//! original value known). See `docs/research/PHYSICS_MOVEMENT.md`.

/// Everything the controller needs to know about one kind of character.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct MovementProfile {
    /// Capsule radius, cm. V: PC_Hero capsule shape radius 33.16 (humans 33..36).
    pub radius: f32,
    /// Total capsule height from feet, cm. V: PC_Hero cylinder 133.68 + 2 * radius = 200.0.
    pub height: f32,
    /// Obstacles up to this height are climbed / dropped without jumping, cm.
    /// V: `StepHeight` 65 (humans), also the distance after which a drop becomes a fall.
    pub step_height: f32,
    /// V: `ForwardSpeedMax` 400 cm/s; the gait modifiers scale it.
    pub forward_max: f32,
    /// V: `StrafeSpeedMax` 280.
    pub strafe_max: f32,
    /// V: `BackwardSpeedMax` 280.
    pub backward_max: f32,
    /// V: `TurnSpeedMax` 200 deg/s.
    pub turn_speed_max_deg: f32,
    /// V: `MoveAcceleration` 1000 cm/s^2 (hero and common humans; 250 on legacy templates).
    pub accel: f32,
    /// V: `MoveDecceleration` -500 cm/s^2 (magnitude stored here).
    pub decel: f32,
    /// V: `SlowModifier` 0.375 (walk).
    pub slow_mod: f32,
    /// V: `FastModifier` 1.5 (sprint).
    pub fast_mod: f32,
    /// V: `SneakModifier` 0.7.
    pub sneak_mod: f32,
    /// Ground angle (deg) where uphill walking starts to slow down. V: `SteepGroundAngleMin` 20.
    pub steep_min_deg: f32,
    /// Largest walkable ground angle (deg); steeper ground makes the character slide.
    /// V: `SteepGroundAngleMax` 49.
    pub steep_max_deg: f32,
    /// V: `WalkDownSpeedScale` 0.7.
    pub walk_down_scale: f32,
    /// Smoothing time of the slope speed scale, s. V: `GroundSlopeTransInertia` 0.15.
    pub slope_trans_inertia: f32,
    /// Smoothing time of the slide speed, s. V: `SlideInertia` 0.4.
    pub slide_inertia: f32,
    /// V: `SlideSpeedMax` 800 cm/s.
    pub slide_speed_max: f32,
    /// Take-off velocity, cm/s. V: `JumpStartUpVelocity` 600 (hero), 500 on legacy templates.
    pub jump_velocity: f32,
    /// Air steering, jump. V: `JumpSteerScaleFactor` 1.0.
    pub jump_steer: f32,
    /// Air steering, fall. V: `FallSteerScaleFactor` 1.0.
    pub fall_steer: f32,
    /// Gravity, cm/s^2. V: `eCPhysicsScene_PS.GravityVector` (0, -981, 0) in the world.
    pub gravity: f32,
    /// Terminal fall speed, cm/s. V: Game.dll `ProcessPhysicsMovements` clamps velocity.y at -1500.
    pub terminal_velocity: f32,
    /// Lowest slope speed scale. V: 0.3 for non-player characters (clamp in
    /// `CalcGroundSlopeTransScale`), 0 for the player (no clamp).
    pub min_slope_scale: f32,
    /// V: `CanBePushedWhileIdle` false.
    pub can_be_pushed_while_idle: bool,
    /// Water depth where a character starts to swim, cm. V: `SwimDepth` 155 (hero).
    pub swim_depth: f32,
    /// Rate at which the feet follow the ground (stairs, ramps), cm/s.
    /// T: the original snaps through `PutToGround`; this only hides the pop.
    pub ground_follow_speed: f32,
}

/// Kept for source compatibility with the former controller configuration.
pub type ControllerConfig = MovementProfile;

impl Default for MovementProfile {
    fn default() -> Self {
        Self::hero()
    }
}

impl MovementProfile {
    /// `Humans_Master_PC_Hero.tple` (the player character).
    pub fn hero() -> Self {
        Self {
            radius: 33.16,
            height: 200.0,
            step_height: 65.0,
            forward_max: 400.0,
            strafe_max: 280.0,
            backward_max: 280.0,
            turn_speed_max_deg: 200.0,
            accel: 1000.0,
            decel: 500.0,
            slow_mod: 0.375,
            fast_mod: 1.5,
            sneak_mod: 0.7,
            steep_min_deg: 20.0,
            steep_max_deg: 49.0,
            walk_down_scale: 0.7,
            slope_trans_inertia: 0.15,
            slide_inertia: 0.4,
            slide_speed_max: 800.0,
            jump_velocity: 600.0,
            jump_steer: 1.0,
            fall_steer: 1.0,
            gravity: 981.0,
            terminal_velocity: 1500.0,
            min_slope_scale: 0.0,
            can_be_pushed_while_idle: false,
            swim_depth: 155.0,
            ground_follow_speed: 1000.0,
        }
    }

    /// Ordinary humanoid NPC (726 + 252 + 65 templates share these values): the hero profile
    /// with the NPC capsule (bandit: radius 35.3, cylinder 129.3) and the slope clamp.
    pub fn human_npc() -> Self {
        Self {
            radius: 35.3,
            height: 129.3 + 2.0 * 35.3,
            min_slope_scale: 0.3,
            swim_depth: 200.0,
            jump_velocity: 600.0,
            ..Self::hero()
        }
    }

    /// Fill from a template property lookup (`gCCharacterMovement_PS` property names such as
    /// `StepHeight`); missing keys keep `base`. Booleans are 0/1.
    pub fn from_lookup(base: Self, get: impl Fn(&str) -> Option<f32>) -> Self {
        let mut p = base;
        let f = |name: &str, dst: &mut f32| {
            if let Some(v) = get(name) {
                *dst = v;
            }
        };
        f("StepHeight", &mut p.step_height);
        f("ForwardSpeedMax", &mut p.forward_max);
        f("StrafeSpeedMax", &mut p.strafe_max);
        f("BackwardSpeedMax", &mut p.backward_max);
        f("TurnSpeedMax", &mut p.turn_speed_max_deg);
        f("MoveAcceleration", &mut p.accel);
        f("SlowModifier", &mut p.slow_mod);
        f("FastModifier", &mut p.fast_mod);
        f("SneakModifier", &mut p.sneak_mod);
        f("SteepGroundAngleMin", &mut p.steep_min_deg);
        f("SteepGroundAngleMax", &mut p.steep_max_deg);
        f("WalkDownSpeedScale", &mut p.walk_down_scale);
        f("GroundSlopeTransInertia", &mut p.slope_trans_inertia);
        f("SlideInertia", &mut p.slide_inertia);
        f("SlideSpeedMax", &mut p.slide_speed_max);
        f("JumpStartUpVelocity", &mut p.jump_velocity);
        f("JumpSteerScaleFactor", &mut p.jump_steer);
        f("FallSteerScaleFactor", &mut p.fall_steer);
        f("SwimDepth", &mut p.swim_depth);
        if let Some(d) = get("MoveDecceleration") {
            p.decel = d.abs();
        }
        if let Some(b) = get("CanBePushedWhileIdle") {
            p.can_be_pushed_while_idle = b != 0.0;
        }
        p
    }

    /// Ground speed of a gait (cm/s): `ForwardSpeedMax` times the gait modifier.
    /// V: Game.dll `SetSpeedByMode` (Sneak -> SneakModifier, Walk -> SlowModifier, Run -> 1,
    /// Sprint -> FastModifier).
    pub fn gait_speed(&self, gait: crate::Gait) -> f32 {
        self.forward_max * gait.modifier(self)
    }

    /// Maximum speed in the local direction `(strafe, forward)` (unit vector): forward /
    /// backward speed blended with the strafe speed on an ellipse. I: the original blends
    /// the three maxima by the angle to the facing axes (`GetCurrentMaxSpeedOfDir`).
    pub fn max_speed_of_dir(&self, strafe: f32, forward: f32) -> f32 {
        let fm = if forward >= 0.0 {
            self.forward_max
        } else {
            self.backward_max
        };
        let denom = ((forward / fm).powi(2) + (strafe / self.strafe_max).powi(2)).sqrt();
        if denom < 1.0e-6 { fm } else { 1.0 / denom }
    }
}

/// Fall damage for `fall_time` seconds spent in the falling state.
/// V (`COMBAT_AI_RULES` 5.1, `OnMovementModeChanged`): `floor(160 * (t - 0.9)^2)` for `t > 0.9`.
/// The Acrobat perk halving is a script matter and not applied here.
pub fn fall_damage(fall_time: f32) -> i32 {
    if fall_time > 0.9 {
        (160.0 * (fall_time - 0.9).powi(2)).floor() as i32
    } else {
        0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fall_damage_matches_documented_examples() {
        assert_eq!(fall_damage(0.9), 0);
        assert_eq!(fall_damage(1.5), 57);
        assert_eq!(fall_damage(2.0), 193);
    }

    #[test]
    fn gait_speeds_match_clip_names() {
        let p = MovementProfile::hero();
        assert_eq!(p.gait_speed(crate::Gait::Run), 400.0);
        assert_eq!(p.gait_speed(crate::Gait::Sprint), 600.0);
        assert!((p.gait_speed(crate::Gait::Walk) - 150.0).abs() < 1e-3);
    }

    #[test]
    fn strafe_is_slower_than_forward() {
        let p = MovementProfile::hero();
        assert_eq!(p.max_speed_of_dir(0.0, 1.0), 400.0);
        assert_eq!(p.max_speed_of_dir(1.0, 0.0), 280.0);
        assert_eq!(p.max_speed_of_dir(0.0, -1.0), 280.0);
    }

    #[test]
    fn lookup_overrides_only_given_keys() {
        let p = MovementProfile::from_lookup(MovementProfile::hero(), |k| {
            (k == "StepHeight").then_some(60.0)
        });
        assert_eq!(p.step_height, 60.0);
        assert_eq!(p.forward_max, 400.0);
    }
}
