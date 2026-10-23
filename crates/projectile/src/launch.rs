//! How the player states loose a projectile.
//!
//! * Arrows and bolts: port of the script function `Shoot` (S:86450): the hero's projectile is
//!   ballistic, flies `draw * 200` km/h along the line from the arrow in the right hand to the
//!   point under the crosshair and has 0.01 damping; an NPC's flies straight at the focus with
//!   the template's `ShootVelocity`. Both touch `MaterialDependent` whatever the template says.
//! * Spell missiles: port of `MagicProjectile` (S:52db0): spawned at the rune/staff slot item of
//!   the caster, launched along the caster's forward axis (lifted by 0.01) with the target
//!   entity set; a power cast with a target homes.
use glam::Vec3;

use crate::flight::Projectile;
use crate::params::{
    NPC_AIM_HEIGHT_CM, PLAYER_ARROW_DAMPING, PLAYER_ARROW_KMH_PER_DRAW, PathStyle,
    ProjectileParams, TouchBehavior, speed_cm_s, unit_or,
};

/// `MagicProjectile`: the vertical lift of the caster's forward axis before it is normalised.
pub const SPELL_AIM_LIFT: f32 = 0.01;

/// A projectile ready to be shot from `origin`.
#[derive(Debug, Clone, PartialEq)]
pub struct Launch {
    /// The projectile, owner and aim set.
    pub projectile: Projectile,
    /// Where it appears: the pose of the ammunition in the hand, or of the spell item.
    pub origin: Vec3,
}

/// Port of the player branch of `Shoot` (S:86450). `origin` is the world position of the
/// arrow in the right-hand slot, `pick_point` the point under the screen centre
/// (`PSCamera::GetScreenCenterPickPoint`), `draw` the damage hit multiplier
/// ([`crate::aim::draw_multiplier`]; a crossbow passes 1).
pub fn hero_arrow(
    template: &ProjectileParams,
    owner: u64,
    origin: Vec3,
    pick_point: Vec3,
    draw: f32,
) -> Launch {
    gothic_portcov::hit!("hero_arrow");
    let mut params = template.clone();
    params.touch = TouchBehavior::MaterialDependent;
    params.path = PathStyle::Physics;
    params.shoot_velocity = draw * PLAYER_ARROW_KMH_PER_DRAW;
    params.linear_damping = PLAYER_ARROW_DAMPING;
    let mut projectile = Projectile::new(params, owner);
    projectile.set_target_direction(unit_or(pick_point - origin, Vec3::NEG_Z));
    Launch { projectile, origin }
}

/// Port of the NPC branch of `Shoot`: a straight flight along the archer's aim with the focus as
/// target, at the template's `ShootVelocity`.
pub fn npc_arrow(
    template: &ProjectileParams,
    owner: u64,
    origin: Vec3,
    forward: Vec3,
    target: Option<u64>,
) -> Launch {
    gothic_portcov::hit!("npc_arrow");
    let mut params = template.clone();
    params.touch = TouchBehavior::MaterialDependent;
    params.path = PathStyle::Line;
    let mut projectile = Projectile::new(params, owner);
    projectile.set_target_direction(unit_or(forward, Vec3::NEG_Z));
    projectile.set_target(target);
    Launch { projectile, origin }
}

/// Port of `MagicProjectile` (S:52db0). `caster_forward` is the forward axis of the caster's
/// pose; `power` is a power cast (action 45), which makes the missile home when it has a target.
pub fn spell_missile(
    template: &ProjectileParams,
    owner: u64,
    origin: Vec3,
    caster_forward: Vec3,
    target: Option<u64>,
    power: bool,
) -> Launch {
    gothic_portcov::hit!("spell_missile");
    let mut params = template.clone();
    params.path = if target.is_some() && power {
        PathStyle::Missile
    } else {
        PathStyle::Line
    };
    let mut projectile = Projectile::new(params, owner);
    let lifted = Vec3::new(
        caster_forward.x,
        caster_forward.y + SPELL_AIM_LIFT,
        caster_forward.z,
    );
    projectile.set_target_direction(unit_or(lifted, Vec3::NEG_Z));
    projectile.set_target(target);
    Launch { projectile, origin }
}

/// The direction an NPC archer's `OnGetAimAngle` (S:83300) aims at: `NPC_AIM_HEIGHT_CM` above
/// the origin of its target when the target has no usable bounding box.
pub fn npc_aim_point(target_origin: Vec3) -> Vec3 {
    target_origin + Vec3::Y * NPC_AIM_HEIGHT_CM
}

/// Unit direction that makes a ballistic shot of `speed_kmh` from `from` land on `to`: the flat
/// (low) arc of the two solutions; `None` when out of range. Gravity 981 cm/s^2, damping ignored.
/// A host without a camera (headless sim, the NPC-less hero release on a focus) uses it in place
/// of the crosshair.
pub fn lob_direction(from: Vec3, to: Vec3, speed_kmh: f32) -> Option<Vec3> {
    let g = crate::params::GRAVITY_CM_S2;
    let v = speed_cm_s(speed_kmh);
    let flat = Vec3::new(to.x - from.x, 0.0, to.z - from.z);
    let x = flat.length();
    let y = to.y - from.y;
    if x < 1.0 {
        return Some(if y >= 0.0 { Vec3::Y } else { Vec3::NEG_Y });
    }
    let disc = v.powi(4) - g * (g * x * x + 2.0 * y * v * v);
    if disc < 0.0 {
        return None;
    }
    let tan = (v * v - disc.sqrt()) / (g * x);
    let dir = flat / x + Vec3::Y * tan;
    Some(dir.normalize())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::flight::{ProjectileWorld, SurfaceHit};

    struct Open;
    impl ProjectileWorld for Open {
        fn surface(&self, from: Vec3, to: Vec3, _r: f32) -> Option<SurfaceHit> {
            (from.y > 0.0 && to.y <= 0.0).then(|| {
                let t = from.y / (from.y - to.y);
                SurfaceHit {
                    t,
                    point: from + (to - from) * t,
                    normal: Vec3::Y,
                    material: crate::ShapeMaterial::Earth,
                }
            })
        }
        fn actor(&self, _: Vec3, _: Vec3, _: f32, _: u64) -> Option<crate::ActorHit> {
            None
        }
        fn aim_point(&self, _: u64) -> Option<Vec3> {
            Some(Vec3::new(2000.0, 120.0, 0.0))
        }
    }

    #[test]
    fn the_hero_arrow_is_ballistic_at_200_kmh_times_the_draw() {
        let l = hero_arrow(
            &ProjectileParams::arrow(),
            1,
            Vec3::new(0.0, 160.0, 0.0),
            Vec3::new(5000.0, 160.0, 0.0),
            0.6,
        );
        let p = &l.projectile.params;
        assert_eq!(p.path, PathStyle::Physics);
        assert!((p.shoot_velocity - 120.0).abs() < 1e-4);
        assert_eq!(p.linear_damping, 0.01);
        assert_eq!(p.touch, TouchBehavior::MaterialDependent);
        assert!((l.projectile.target_direction - Vec3::X).length() < 1e-5);
    }

    #[test]
    fn the_bolt_template_is_overridden_to_material_dependent_by_shoot() {
        let l = hero_arrow(&ProjectileParams::bolt(), 1, Vec3::ZERO, Vec3::X, 1.0);
        assert_eq!(l.projectile.params.touch, TouchBehavior::MaterialDependent);
    }

    #[test]
    fn the_npc_arrow_flies_straight_at_the_template_speed() {
        let w = Open;
        let mut l = npc_arrow(
            &ProjectileParams::arrow(),
            5,
            Vec3::new(0.0, 150.0, 0.0),
            Vec3::X,
            Some(2),
        );
        assert_eq!(l.projectile.params.path, PathStyle::Line);
        l.projectile.shoot(l.origin, &w);
        assert!((l.projectile.velocity.length() - 2777.78).abs() < 0.1);
        let v = l.projectile.velocity;
        l.projectile.step(0.5, &w);
        assert_eq!(l.projectile.velocity, v, "no gravity on a line flight");
        assert!(
            l.projectile.position.y > 100.0 && l.projectile.position.y < 160.0,
            "flies at the chest"
        );
    }

    #[test]
    fn a_power_cast_with_a_target_homes_others_fly_straight() {
        let t = ProjectileParams::spell_missile();
        let power = spell_missile(&t, 1, Vec3::ZERO, Vec3::X, Some(2), true);
        assert_eq!(power.projectile.params.path, PathStyle::Missile);
        let plain = spell_missile(&t, 1, Vec3::ZERO, Vec3::X, Some(2), false);
        assert_eq!(plain.projectile.params.path, PathStyle::Line);
        let free = spell_missile(&t, 1, Vec3::ZERO, Vec3::X, None, true);
        assert_eq!(free.projectile.params.path, PathStyle::Line);
        // The forward axis is lifted by 0.01 before it is normalised.
        assert!(
            free.projectile.target_direction.y > 0.0099
                && free.projectile.target_direction.y < 0.0101
        );
    }

    #[test]
    fn a_line_missile_with_a_target_leaves_towards_its_chest() {
        let w = Open;
        let mut l = spell_missile(
            &ProjectileParams::spell_missile(),
            1,
            Vec3::new(0.0, 150.0, 0.0),
            Vec3::Z,
            Some(2),
            false,
        );
        l.projectile.shoot(l.origin, &w);
        // The target entity wins over the forward axis.
        let d = l.projectile.velocity.normalize();
        assert!(d.x > 0.99, "{d:?}");
    }

    #[test]
    fn the_lob_direction_lands_the_arrow() {
        let from = Vec3::new(0.0, 160.0, 0.0);
        let to = Vec3::new(3000.0, 100.0, 0.0);
        let dir = lob_direction(from, to, 200.0).expect("in range");
        let mut p =
            hero_arrow(&ProjectileParams::arrow(), 1, from, from + dir * 100.0, 1.0).projectile;
        p.params.linear_damping = 0.0;
        p.shoot(from, &Open);
        let mut y = None;
        for _ in 0..4000 {
            p.step(0.005, &Open);
            if p.position.x >= 3000.0 {
                y = Some(p.position.y);
                break;
            }
        }
        assert!((y.expect("passed") - 100.0).abs() < 4.0, "{y:?}");
        assert!(lob_direction(from, Vec3::new(90_000.0, 0.0, 0.0), 200.0).is_none());
    }
}
