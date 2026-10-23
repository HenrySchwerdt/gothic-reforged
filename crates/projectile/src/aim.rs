//! The aim coupling of the bow, the crossbow and spells: the draw multiplier, the pitch blend of
//! the aim clips (`eCVisualAnimation_PS::UpdateAim` / `gCScriptProcessingUnit::sAIPlayAimItlLoop`)
//! and the crosshair (`OnGetAimAngle`, S:83300).
//!
//! The aim clips exist in three pitch variants, `Fwd`, `Up` and `Down`
//! (`Hero_Stand_Bow_Arrow_P0_Aim_Raise_O_Up_00_%_00_P0_0`). Every frame the script function
//! `OnGetAimAngle` returns the sine of the angle between the horizontal and the line from the
//! bow to the crosshair target. The engine turns it into an angle (`asin`), plays the `Fwd`
//! variant in motion slot 4 with weight `1 - |angle| / 90 deg` and, when that is below 1, the
//! `Up` (angle >= 0) or `Down` variant in slot 5 at full weight, started at the same play time.
use glam::Vec3;

/// A bow is fully drawn after this many seconds (`OnPowerAim_Loop`, S:84b90).
pub const FULL_DRAW_SECS: f32 = 1.95;
/// The multiplier of a bow that was just raised.
pub const MIN_DRAW: f32 = 0.2;
/// Fire-quality ammunition ignites only at this draw or more (`COMBAT_AI_RULES` 9.6).
pub const FIRE_MIN_DRAW: f32 = 0.5;
/// `OnGetAimAngle` returns this when the hero holds no ammunition: keep the last angle.
pub const NO_AIM_VALUE: f32 = 2.0;
/// Closest pick distance of the crosshair scale, cm.
pub const CROSSHAIR_NEAR_CM: f32 = 100.0;
/// Farthest pick distance of the crosshair scale, cm.
pub const CROSSHAIR_FAR_CM: f32 = 4000.0;
/// Runtime effect that glows on the bow while it is drawn (`PS_Ranged_PowerAim`), stopped by
/// `Shoot`.
pub const RAISE_EFFECT: &str = "eff_ani_fight_bow_raise_01";
/// Effect started at the moment of the shot (`Shoot`).
pub const SHOOT_EFFECT: &str = "eff_ani_fight_bow_shoot_01";

/// Port of the rule of `OnPowerAim_Loop`: `0.2 + 0.8 * min(1, t / 1.95)` with `t` the time held.
pub fn draw_multiplier(held_secs: f32) -> f32 {
    gothic_portcov::hit!("draw_multiplier");
    MIN_DRAW + (1.0 - MIN_DRAW) * (held_secs / FULL_DRAW_SECS).clamp(0.0, 1.0)
}

/// Port of `GUI2::SetCrossHairSize` as `OnGetAimAngle` calls it: the crosshair shrinks from 1 at
/// 100 cm to 0 at 4000 cm of pick distance.
pub fn crosshair_size(pick_distance: f32) -> f32 {
    gothic_portcov::hit!("crosshair_size");
    1.0 - pick_distance.clamp(CROSSHAIR_NEAR_CM, CROSSHAIR_FAR_CM) / CROSSHAIR_FAR_CM
}

/// The player branch of `OnGetAimAngle`: the sine of the pitch of the line from the weapon
/// (`origin`, the pose of the item in the weapon slot) to the point under the crosshair.
/// `None` when there is no such item (the script returns [`NO_AIM_VALUE`]).
pub fn hero_aim_sine(origin: Vec3, pick_point: Vec3) -> f32 {
    let d = pick_point - origin;
    d.try_normalize().map_or(0.0, |n| n.y)
}

/// The NPC branch of `OnGetAimAngle` [I]: the sine of the pitch from the bow to the aim point on
/// the target (centre of its box, or 120 cm above its origin).
pub fn npc_aim_sine(origin: Vec3, target_aim_point: Vec3) -> f32 {
    hero_aim_sine(origin, target_aim_point)
}

/// Which pitch variant of a clip is meant.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Pitch {
    /// Looking along the horizon (motion slot 4).
    Fwd,
    /// Aiming up (slot 5).
    Up,
    /// Aiming down (slot 5).
    Down,
}

impl Pitch {
    fn word(self) -> &'static str {
        match self {
            Self::Fwd => "Fwd",
            Self::Up => "Up",
            Self::Down => "Down",
        }
    }
}

/// The phases of the aim clips of the installed game.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Phase {
    /// `Aim_Raise`: the weapon comes up (`PS_Ranged_Aim`).
    Raise,
    /// `Aim_PowerRaise`: the bow is drawn (`PS_Ranged_PowerAim`).
    PowerRaise,
    /// `Aim_Aim`: the hold pose of the aim loop and the first part of the shot.
    Aim,
    /// `Aim_Recover`: the weapon goes down after the shot (right hand empty).
    Recover,
    /// `Reload_Begin`.
    ReloadBegin,
    /// `Reload_Raise`.
    ReloadRaise,
    /// `Reload_End`.
    ReloadEnd,
    /// `Cock_Begin`: the crossbow is cocked (no pitch variants).
    CockBegin,
}

impl Phase {
    fn action_and_phase(self) -> (&'static str, &'static str) {
        match self {
            Self::Raise => ("Aim", "Raise"),
            Self::PowerRaise => ("Aim", "PowerRaise"),
            Self::Aim => ("Aim", "Aim"),
            Self::Recover => ("Aim", "Recover"),
            Self::ReloadBegin => ("Reload", "Begin"),
            Self::ReloadRaise => ("Reload", "Raise"),
            Self::ReloadEnd => ("Reload", "End"),
            Self::CockBegin => ("Cock", "Begin"),
        }
    }

    /// Does the right hand hold ammunition during this phase? `Recover` and the start of a reload
    /// have the arrow gone.
    pub fn right_hand(self, ammunition: &'static str) -> &'static str {
        match self {
            Self::Recover => "None",
            _ => ammunition,
        }
    }
}

/// Name of an aim clip as stored in `_compiledAnimation.pak` (without the extension):
/// `<actor>_Stand_<left>_<right>_P0_<action>_<phase>_O_<pitch>_00_%_00_P0_0` with `left` the
/// weapon (`Bow`, `CrossBow`) and `right` its ammunition (`Arrow`, `Bolt`, `None`).
pub fn clip_name(actor: &str, left: &str, right: &str, phase: Phase, pitch: Pitch) -> String {
    let (action, ph) = phase.action_and_phase();
    format!(
        "{actor}_Stand_{left}_{right}_P0_{action}_{ph}_O_{}_00_%_00_P0_0",
        pitch.word()
    )
}

/// The result of an aim update: what to play in the motion slots 4 and 5.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct AimBlend {
    /// The aim angle, radians above the horizon (negative below).
    pub angle: f32,
    /// Weight of the `Fwd` clip (slot 4).
    pub forward_weight: f32,
    /// The overlay variant (slot 5, weight 1) when the forward weight is below 1.
    pub overlay: Option<Pitch>,
}

/// The cached aim angle of `eCVisualAnimation_PS` (`GetLastAutoAimAngle`) and the conversion of
/// `OnGetAimAngle` values into a blend.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct AimPitch {
    last_angle: f32,
}

impl AimPitch {
    /// Port of the angle step of `UpdateAim` (Engine:0x10030230): a value of 1 or more keeps the
    /// last angle; otherwise `angle = asin(clamp(value, -1, 1))`.
    pub fn update(&mut self, aim_sine: f32) -> AimBlend {
        gothic_portcov::hit!("update");
        if aim_sine < 1.0 {
            self.last_angle = aim_sine.clamp(-1.0, 1.0).asin();
        }
        let half_pi = std::f32::consts::FRAC_PI_2;
        let forward_weight = 1.0 - (self.last_angle / half_pi).abs();
        AimBlend {
            angle: self.last_angle,
            forward_weight,
            overlay: (forward_weight < 1.0).then_some(if self.last_angle < 0.0 {
                Pitch::Down
            } else {
                Pitch::Up
            }),
        }
    }

    /// The cached angle.
    pub fn last_angle(&self) -> f32 {
        self.last_angle
    }

    /// `GameReset`: forget the angle.
    pub fn reset(&mut self) {
        self.last_angle = 0.0;
    }
}

/// How long the string has been held, for [`draw_multiplier`].
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct BowDraw {
    held: f32,
}

impl BowDraw {
    /// A fresh draw.
    pub fn new() -> Self {
        Self::default()
    }
    /// Advance by a frame.
    pub fn tick(&mut self, dt: f32) {
        self.held += dt.max(0.0);
    }
    /// Seconds held.
    pub fn held(&self) -> f32 {
        self.held
    }
    /// The current damage hit multiplier.
    pub fn multiplier(&self) -> f32 {
        draw_multiplier(self.held)
    }
    /// Fully drawn?
    pub fn is_full(&self) -> bool {
        self.held >= FULL_DRAW_SECS
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_draw_multiplier_grows_from_one_fifth_to_one_in_1_95_seconds() {
        assert!((draw_multiplier(0.0) - 0.2).abs() < 1e-6);
        assert!((draw_multiplier(0.975) - 0.6).abs() < 1e-5);
        assert!((draw_multiplier(1.95) - 1.0).abs() < 1e-6);
        assert_eq!(draw_multiplier(9.0), 1.0);
    }

    #[test]
    fn the_crosshair_shrinks_with_distance() {
        assert_eq!(crosshair_size(50.0), 1.0 - 100.0 / 4000.0);
        assert!((crosshair_size(2000.0) - 0.5).abs() < 1e-6);
        assert_eq!(crosshair_size(9000.0), 0.0);
    }

    #[test]
    fn a_level_aim_plays_only_the_forward_clip() {
        let mut aim = AimPitch::default();
        let b = aim.update(0.0);
        assert_eq!(b.forward_weight, 1.0);
        assert_eq!(b.overlay, None);
    }

    #[test]
    fn aiming_up_blends_in_the_up_clip_and_down_the_down_clip() {
        let mut aim = AimPitch::default();
        // 30 degrees up.
        let b = aim.update(0.5);
        assert!((b.angle - 30f32.to_radians()).abs() < 1e-5);
        assert!((b.forward_weight - (1.0 - 30.0 / 90.0)).abs() < 1e-5);
        assert_eq!(b.overlay, Some(Pitch::Up));
        let b = aim.update(-0.5);
        assert_eq!(b.overlay, Some(Pitch::Down));
        assert!((b.forward_weight - (1.0 - 30.0 / 90.0)).abs() < 1e-5);
        // Straight up is the pure Up clip.
        let b = aim.update(0.9999);
        assert!(b.forward_weight < 0.03);
    }

    #[test]
    fn a_value_of_one_or_more_keeps_the_last_angle() {
        let mut aim = AimPitch::default();
        aim.update(0.5);
        let b = aim.update(NO_AIM_VALUE);
        assert!((b.angle - 30f32.to_radians()).abs() < 1e-5);
        aim.reset();
        assert_eq!(aim.update(NO_AIM_VALUE).angle, 0.0);
    }

    #[test]
    fn the_hero_aim_sine_follows_the_pick_point() {
        let origin = Vec3::new(0.0, 160.0, 0.0);
        assert!((hero_aim_sine(origin, Vec3::new(1000.0, 160.0, 0.0))).abs() < 1e-6);
        let up = hero_aim_sine(origin, Vec3::new(1000.0, 160.0 + 577.35, 0.0));
        assert!((up - 0.5).abs() < 1e-3, "{up}");
        assert!(hero_aim_sine(origin, Vec3::new(1000.0, 0.0, 0.0)) < 0.0);
    }

    #[test]
    fn clip_names_match_the_installed_animation_files() {
        assert_eq!(
            clip_name("Hero", "Bow", "Arrow", Phase::PowerRaise, Pitch::Up),
            "Hero_Stand_Bow_Arrow_P0_Aim_PowerRaise_O_Up_00_%_00_P0_0"
        );
        assert_eq!(
            clip_name(
                "Hero",
                "Bow",
                Phase::Recover.right_hand("Arrow"),
                Phase::Recover,
                Pitch::Fwd
            ),
            "Hero_Stand_Bow_None_P0_Aim_Recover_O_Fwd_00_%_00_P0_0"
        );
        assert_eq!(
            clip_name("Hero", "CrossBow", "Bolt", Phase::CockBegin, Pitch::Fwd),
            "Hero_Stand_CrossBow_Bolt_P0_Cock_Begin_O_Fwd_00_%_00_P0_0"
        );
        assert_eq!(
            clip_name("Hero", "Bow", "Arrow", Phase::ReloadRaise, Pitch::Down),
            "Hero_Stand_Bow_Arrow_P0_Reload_Raise_O_Down_00_%_00_P0_0"
        );
    }

    #[test]
    fn a_held_bow_reaches_full_strength() {
        let mut d = BowDraw::new();
        for _ in 0..40 {
            d.tick(0.05);
        }
        assert!(d.is_full());
        assert_eq!(d.multiplier(), 1.0);
    }
}

/// Lengths in seconds of the hero's aim and cast clips, measured from `_compiledAnimation.pak`
/// (the `Up` and `Down` variants have the same length). The script plays them at an animation
/// speed factor of 1 for the hero; the draw ramp of [`draw_multiplier`] (1.95 s) runs in step
/// with the 2.0 s `PowerRaise` clip.
pub mod clips {
    /// Bow clips (`Hero_Stand_Bow_*`).
    pub mod bow {
        /// `Aim_Raise`.
        pub const RAISE: f32 = 0.4;
        /// `Aim_PowerRaise`.
        pub const POWER_RAISE: f32 = 2.0;
        /// `Aim_Aim`.
        pub const AIM: f32 = 0.4;
        /// `Aim_Recover` (the arrow is gone).
        pub const RECOVER: f32 = 0.4;
        /// `Reload_Begin`.
        pub const RELOAD_BEGIN: f32 = 0.28;
        /// `Reload_Raise`.
        pub const RELOAD_RAISE: f32 = 0.4;
        /// `Reload_End`.
        pub const RELOAD_END: f32 = 0.28;
    }
    /// Crossbow clips (`Hero_Stand_CrossBow_*`).
    pub mod crossbow {
        /// `Aim_Raise`.
        pub const RAISE: f32 = 0.6;
        /// `Aim_Aim`.
        pub const AIM: f32 = 0.2;
        /// `Aim_Recover`.
        pub const RECOVER: f32 = 0.6;
        /// `Cock_Begin`: cocking takes this long.
        pub const COCK: f32 = 2.8;
        /// `Reload_Begin`.
        pub const RELOAD_BEGIN: f32 = 0.6;
        /// `Reload_End`.
        pub const RELOAD_END: f32 = 0.56;
    }
    /// Spell casting with a staff or rune in hand (`Hero_Stand_Staff_Cast_P0_<Kind>_<Phase>`):
    /// `(raise, hit, recover)`. The projectile leaves when `_AI_Cast_Execute` begins, i.e. when
    /// the raise clip has ended; the hit clip follows.
    pub mod cast {
        /// `Cast`.
        pub const CAST: (f32, f32, f32) = (0.8, 0.28, 0.84);
        /// `QuickCast`.
        pub const QUICK: (f32, f32, f32) = (0.16, 0.2, 0.36);
        /// `PowerCast`.
        pub const POWER: (f32, f32, f32) = (0.92, 0.24, 0.76);
    }
}
