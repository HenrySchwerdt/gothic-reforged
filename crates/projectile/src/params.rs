//! The property set of a projectile (`gCProjectile_PS`) and the physics constants around it.
use glam::Vec3;

/// `ShootVelocity` is in kilometres per hour: `GetDirection` (GameDll:0x10151e30) scales the unit
/// direction by `ShootVelocity * (1 / 3.6) * 100`, i.e. to centimetres per second.
pub const VELOCITY_UNIT_CM_S: f32 = 100.0 / 3.6;
/// `PostInitializeProperties` (GameDll:0x10151d10): default `ShootVelocity`.
pub const DEFAULT_SHOOT_VELOCITY: f32 = 100.0;
/// Default `TouchAngleTreshold` (degrees). The shipped code only stores it.
pub const DEFAULT_TOUCH_ANGLE_DEG: f32 = 80.0;
/// Default `DecayDuration`, seconds.
pub const DEFAULT_DECAY_SECS: f32 = 10.0;
/// Scene gravity, cm/s^2 (`eCPhysicsScene_PS.GravityVector`, see `PHYSICS_MOVEMENT.md`).
pub const GRAVITY_CM_S2: f32 = 981.0;
/// `OnTouch` (GameDll:0x10152650): a stopping projectile is put this far (cm) along its flight
/// direction past the contact point.
pub const EMBED_DEPTH_CM: f32 = 35.0;
/// `OnTouch`: a projectile that links to the world node it hit decays `DecayDuration` plus this
/// many seconds later.
pub const LINKED_DECAY_EXTRA_SECS: f32 = 60.0;
/// `OnTouch`: how long (ms) an arrow stays attached to a bone of its victim.
pub const ATTACH_LIFETIME_MS: u32 = 60_000;
/// `OnTouch`: attachment lifetime when the victim is the player, the hit is a head shot, or the
/// victim already carries [`MAX_ATTACHMENTS`] attachments: the projectile vanishes at once.
pub const ATTACH_SHORT_MS: u32 = 1;
/// `OnTouch`: attachments (arrows) a single actor visual carries at most.
pub const MAX_ATTACHMENTS: usize = 8;
/// Shortest horizontal speed (cm/s) at which a flight continues; below it a falling projectile
/// counts as at rest.
pub const REST_SPEED: f32 = 20.0;
/// Speed kept along the surface normal when a projectile bounces off a hard surface [I]: the
/// shipped materials are not decoded, a low restitution reproduces the dull clatter of arrows.
pub const BOUNCE_RESTITUTION: f32 = 0.3;
/// Safety bound on a flight (seconds) [I]: the engine ends a flight through
/// `OnExitProcessingRange`; this is the same rule expressed in time.
pub const MAX_FLIGHT_SECS: f32 = 20.0;
/// `Shoot` (S:86450): the player's arrow speed per unit of draw multiplier, km/h.
pub const PLAYER_ARROW_KMH_PER_DRAW: f32 = 200.0;
/// `Shoot`: linear and angular damping the player's arrow is given (the template has 0.1).
pub const PLAYER_ARROW_DAMPING: f32 = 0.01;
/// `Shoot`: height above the target origin NPC archers aim at (cm), see `OnGetAimAngle`.
pub const NPC_AIM_HEIGHT_CM: f32 = 120.0;

/// `gEProjectilePath`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum PathStyle {
    /// Constant velocity along the launch direction (value 0).
    #[default]
    Line,
    /// Constant velocity, re-aimed on request (value 1, unused by the shipped templates).
    Curve,
    /// Ballistic flight under gravity (value 2): the hero's arrows and bolts.
    Physics,
    /// Homing: the direction is recomputed towards the target every `TargetUpdateMSec`
    /// (value 3).
    Missile,
}

impl PathStyle {
    /// The enumeration value stored in templates.
    pub fn from_value(v: i64) -> Option<Self> {
        Some(match v {
            0 => Self::Line,
            1 => Self::Curve,
            2 => Self::Physics,
            3 => Self::Missile,
            _ => return None,
        })
    }
    /// The value as stored in templates.
    pub fn value(self) -> i64 {
        match self {
            Self::Line => 0,
            Self::Curve => 1,
            Self::Physics => 2,
            Self::Missile => 3,
        }
    }
}

/// `gEProjectileTouchBehavior`, by the values `OnTouch` switches on. (The property table in
/// `gothic-formats` lists the first two names under one value; the code distinguishes them.)
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum TouchBehavior {
    /// Value 0: the projectile ends at the first contact (spell missiles).
    #[default]
    KillSelf,
    /// Value 1: it keeps its physics and bounces off (crossbow bolts).
    Reflect,
    /// Value 2: it stops dead and embeds.
    Stop,
    /// Value 3: soft surfaces take it, hard ones throw it back, characters take it on a bone.
    MaterialDependent,
}

impl TouchBehavior {
    /// From the stored value.
    pub fn from_value(v: i64) -> Option<Self> {
        Some(match v {
            0 => Self::KillSelf,
            1 => Self::Reflect,
            2 => Self::Stop,
            3 => Self::MaterialDependent,
            _ => return None,
        })
    }
    /// The stored value.
    pub fn value(self) -> i64 {
        match self {
            Self::KillSelf => 0,
            Self::Reflect => 1,
            Self::Stop => 2,
            Self::MaterialDependent => 3,
        }
    }
}

/// `eEShapeMaterial` of the surface a projectile touched.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ShapeMaterial {
    #[default]
    None,
    Wood,
    Metal,
    Water,
    Stone,
    Earth,
    Ice,
    Leather,
    Clay,
    Glass,
    Flesh,
    Snow,
    Debris,
    Foliage,
    Magic,
    Grass,
    Sand,
    Other(u8),
}

impl ShapeMaterial {
    /// From the enumeration value (`gothic-formats` `eEShapeMaterial`).
    pub fn from_value(v: i64) -> Self {
        match v {
            0 => Self::None,
            1 => Self::Wood,
            2 => Self::Metal,
            3 => Self::Water,
            4 => Self::Stone,
            5 => Self::Earth,
            6 => Self::Ice,
            7 => Self::Leather,
            8 => Self::Clay,
            9 => Self::Glass,
            10 => Self::Flesh,
            11 => Self::Snow,
            12 => Self::Debris,
            13 => Self::Foliage,
            14 => Self::Magic,
            15 => Self::Grass,
            20 => Self::Sand,
            other => Self::Other(other.clamp(0, 255) as u8),
        }
    }

    /// `OnTouch`, behaviour `MaterialDependent`: the six materials an arrow sticks in (wood,
    /// earth, ice, leather, flesh, snow, enumeration values 1, 5, 6, 7, 10, 11); everything else
    /// throws it back.
    pub fn takes_arrows(self) -> bool {
        matches!(
            self,
            Self::Wood | Self::Earth | Self::Ice | Self::Leather | Self::Flesh | Self::Snow
        )
    }
}

/// A decoded template property, for [`ProjectileParams::set_property`].
#[derive(Debug, Clone, PartialEq)]
pub enum Value {
    Float(f32),
    Int(i64),
    Bool(bool),
    Text(String),
}

/// The properties of a projectile entity: `gCProjectile_PS` plus the rigid body values that
/// matter for the flight.
#[derive(Debug, Clone, PartialEq)]
pub struct ProjectileParams {
    /// `PathStyle`.
    pub path: PathStyle,
    /// `TouchBehavior`.
    pub touch: TouchBehavior,
    /// `ShootVelocity`, km/h (see [`VELOCITY_UNIT_CM_S`]).
    pub shoot_velocity: f32,
    /// `TargetUpdateMSec`: homing re-aim interval (0 = every frame).
    pub target_update_ms: u32,
    /// `TouchAngleTreshold`, degrees (stored only).
    pub touch_angle_deg: f32,
    /// `LinkToBones`: stick to the bone it hit.
    pub link_to_bones: bool,
    /// `FadeOnDecay`.
    pub fade_on_decay: bool,
    /// `DecayDuration`, seconds.
    pub decay_secs: f32,
    /// `EffectPointImpact`: effect at a level hit.
    pub effect_point_impact: String,
    /// `EffectTargetHit`: effect on a character hit.
    pub effect_target_hit: String,
    /// `FuncOnTargetHit`: script function applying a spell's effect.
    pub func_on_target_hit: String,
    /// `eCRigidBody_PS.TotalMass`.
    pub mass: f32,
    /// `eCRigidBody_PS.LinearDamping`.
    pub linear_damping: f32,
    /// Radius of the collision sphere the flight sweeps (cm) [I]: `PSCollisionShape::CreateShape`
    /// takes the shape of the template, which is not decoded.
    pub radius: f32,
}

impl Default for ProjectileParams {
    /// The values `PostInitializeProperties` (GameDll:0x10151d10) leaves.
    fn default() -> Self {
        Self {
            path: PathStyle::Line,
            touch: TouchBehavior::KillSelf,
            shoot_velocity: DEFAULT_SHOOT_VELOCITY,
            target_update_ms: 0,
            touch_angle_deg: DEFAULT_TOUCH_ANGLE_DEG,
            link_to_bones: false,
            fade_on_decay: false,
            decay_secs: DEFAULT_DECAY_SECS,
            effect_point_impact: String::new(),
            effect_target_hit: String::new(),
            func_on_target_hit: String::new(),
            mass: 1.0,
            linear_damping: 0.0,
            radius: 5.0,
        }
    }
}

impl ProjectileParams {
    /// An arrow template (`G3_Weapons_Ranged_Bow_Arrow`, all arrow types): ballistic, material
    /// dependent touch, sticks to bones, fades after 10 s; 0.2 mass, 0.1 damping.
    pub fn arrow() -> Self {
        Self {
            path: PathStyle::Physics,
            touch: TouchBehavior::MaterialDependent,
            link_to_bones: true,
            fade_on_decay: true,
            mass: 0.2,
            linear_damping: 0.1,
            radius: 3.0,
            ..Self::default()
        }
    }

    /// A crossbow bolt template: like an arrow but with the `Reflect` touch behaviour.
    pub fn bolt() -> Self {
        Self {
            touch: TouchBehavior::Reflect,
            ..Self::arrow()
        }
    }

    /// A spell missile (`Mis_Fireball`, `Mis_IceLance`, `Mis_LightningBolt`): straight, ends on
    /// the first contact, 10 mass, 0.2 damping; the template names the impact effect.
    pub fn spell_missile() -> Self {
        Self {
            mass: 10.0,
            linear_damping: 0.2,
            radius: 12.0,
            ..Self::default()
        }
    }

    /// Set one property by its name in the template.
    pub fn set_property(&mut self, name: &str, value: &Value) {
        let int = |v: &Value| match v {
            Value::Int(i) => Some(*i),
            Value::Float(f) => Some(*f as i64),
            _ => None,
        };
        let float = |v: &Value| match v {
            Value::Float(f) => Some(*f),
            Value::Int(i) => Some(*i as f32),
            _ => None,
        };
        let text = |v: &Value| match v {
            Value::Text(s) => Some(s.clone()),
            _ => None,
        };
        let flag = |v: &Value| match v {
            Value::Bool(b) => Some(*b),
            Value::Int(i) => Some(*i != 0),
            _ => None,
        };
        match name {
            "PathStyle" => {
                if let Some(p) = int(value).and_then(PathStyle::from_value) {
                    self.path = p;
                }
            }
            "TouchBehavior" => {
                if let Some(t) = int(value).and_then(TouchBehavior::from_value) {
                    self.touch = t;
                }
            }
            "ShootVelocity" => self.shoot_velocity = float(value).unwrap_or(self.shoot_velocity),
            "TargetUpdateMSec" => {
                self.target_update_ms =
                    int(value).map_or(self.target_update_ms, |v| v.max(0) as u32);
            }
            "TouchAngleTreshold" => {
                self.touch_angle_deg = float(value).unwrap_or(self.touch_angle_deg);
            }
            "LinkToBones" => self.link_to_bones = flag(value).unwrap_or(self.link_to_bones),
            "FadeOnDecay" => self.fade_on_decay = flag(value).unwrap_or(self.fade_on_decay),
            "DecayDuration" => self.decay_secs = float(value).unwrap_or(self.decay_secs),
            "EffectPointImpact" => {
                self.effect_point_impact = text(value).unwrap_or_default();
            }
            "EffectTargetHit" => self.effect_target_hit = text(value).unwrap_or_default(),
            "FuncOnTargetHit" => self.func_on_target_hit = text(value).unwrap_or_default(),
            "TotalMass" => self.mass = float(value).unwrap_or(self.mass),
            "LinearDamping" => self.linear_damping = float(value).unwrap_or(self.linear_damping),
            _ => {}
        }
    }

    /// Launch speed in cm/s.
    pub fn speed_cm_s(&self) -> f32 {
        speed_cm_s(self.shoot_velocity)
    }
}

/// Port of the scale in `gCProjectile_PS::GetDirection`: `ShootVelocity` (km/h) to cm/s.
pub fn speed_cm_s(shoot_velocity_kmh: f32) -> f32 {
    gothic_portcov::hit!("speed_cm_s");
    shoot_velocity_kmh * VELOCITY_UNIT_CM_S
}

/// Unit vector of `v`, or `fallback` for a zero vector.
pub(crate) fn unit_or(v: Vec3, fallback: Vec3) -> Vec3 {
    v.try_normalize().unwrap_or(fallback)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_velocity_unit_is_kilometres_per_hour() {
        // Mis_Fireball: ShootVelocity 100 -> 27.78 m/s.
        assert!((speed_cm_s(100.0) - 2777.78).abs() < 0.01);
        // A fully drawn bow: 1.0 * 200 km/h -> 55.6 m/s.
        assert!((speed_cm_s(PLAYER_ARROW_KMH_PER_DRAW) - 5555.56).abs() < 0.01);
    }

    #[test]
    fn engine_defaults_match_post_initialize_properties() {
        let d = ProjectileParams::default();
        assert_eq!(d.shoot_velocity, 100.0);
        assert_eq!(d.touch_angle_deg, 80.0);
        assert_eq!(d.decay_secs, 10.0);
        assert_eq!(d.path, PathStyle::Line);
        assert_eq!(d.touch, TouchBehavior::KillSelf);
        assert_eq!(d.target_update_ms, 0);
    }

    #[test]
    fn templates_set_properties_by_name() {
        let mut p = ProjectileParams::default();
        p.set_property("PathStyle", &Value::Int(2));
        p.set_property("TouchBehavior", &Value::Int(3));
        p.set_property("ShootVelocity", &Value::Float(65.0));
        p.set_property("LinkToBones", &Value::Bool(true));
        p.set_property(
            "EffectPointImpact",
            &Value::Text("eff_magic_destroyevil_impact_01".into()),
        );
        assert_eq!(p.path, PathStyle::Physics);
        assert_eq!(p.touch, TouchBehavior::MaterialDependent);
        assert_eq!(p.shoot_velocity, 65.0);
        assert!(p.link_to_bones);
        assert_eq!(p.effect_point_impact, "eff_magic_destroyevil_impact_01");
    }

    #[test]
    fn arrows_stick_in_six_materials() {
        let sticks: Vec<i64> = (0..21)
            .filter(|v| ShapeMaterial::from_value(*v).takes_arrows())
            .collect();
        assert_eq!(sticks, vec![1, 5, 6, 7, 10, 11]);
    }

    #[test]
    fn installed_templates() {
        let arrow = ProjectileParams::arrow();
        assert_eq!(arrow.path.value(), 2);
        assert_eq!(arrow.touch.value(), 3);
        assert!(arrow.link_to_bones && arrow.fade_on_decay);
        assert_eq!(ProjectileParams::bolt().touch.value(), 1);
        let missile = ProjectileParams::spell_missile();
        assert_eq!((missile.path.value(), missile.touch.value()), (0, 0));
        assert_eq!(missile.linear_damping, 0.2);
    }
}
