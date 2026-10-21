//! Flock steering of `gCBoid` / `gCFlock_PS` (birds over Ardea, game:0x101ab660 family).
//!
//! The engine's `gCBoid::Process` blends four steering terms per member in `Roam` state:
//! cohesion (`GetPerceivedCenterVec`), separation (`GetEvadeVec`), alignment
//! (`GetNeighbourVelocity`) and the path toward the travel point (`GetPathVelocity` /
//! `FindTravelPoint`), then clamps the result to the member's maximum speed and turns the entity
//! with a slerp. This module implements those four terms with the flock's authored parameters.
//! [I] the weights and exact falloffs of the original terms are not decoded (the decompiled
//! bodies call through unresolved virtuals); the structure and parameters are.
use bevy::math::Vec3;

/// The authored parameters of a `gCFlock_PS` that steer the member (cm, cm/s).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct FlockParams {
    /// `SeekMemberDist`: members beyond this distance are not perceived as flockmates.
    pub seek_member_dist: f32,
    /// `NeighbourDist`: closer than this a flockmate pushes away.
    pub neighbour_dist: f32,
    /// `POIVeloScale`: strength of the pull toward the point of interest.
    pub poi_scale: f32,
    /// `NeighbourVeloScale`: strength of velocity matching.
    pub neighbour_scale: f32,
    pub max_speed: f32,
}

impl Default for FlockParams {
    fn default() -> Self {
        Self {
            seek_member_dist: 1000.0,
            neighbour_dist: 200.0,
            poi_scale: 1.0,
            neighbour_scale: 1.0,
            max_speed: 600.0,
        }
    }
}

/// One flock member.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Boid {
    pub pos: Vec3,
    pub vel: Vec3,
}

/// Port (structure) of gCBoid::GetPerceivedCenterVec (0x101acb60): the vector from `me` to the
/// centre of the other members within `seek_member_dist`, zero when none are perceived.
pub fn perceived_center(me: usize, flock: &[Boid], p: &FlockParams) -> Vec3 {
    let mut sum = Vec3::ZERO;
    let mut n = 0;
    for (i, b) in flock.iter().enumerate() {
        if i != me && b.pos.distance(flock[me].pos) <= p.seek_member_dist {
            sum += b.pos;
            n += 1;
        }
    }
    if n == 0 {
        Vec3::ZERO
    } else {
        sum / n as f32 - flock[me].pos
    }
}

/// Port (structure) of gCBoid::GetEvadeVec (0x101acda0): away from every member closer than
/// `neighbour_dist`, stronger the closer it is.
pub fn evade(me: usize, flock: &[Boid], p: &FlockParams) -> Vec3 {
    let mut out = Vec3::ZERO;
    for (i, b) in flock.iter().enumerate() {
        if i == me {
            continue;
        }
        let d = flock[me].pos - b.pos;
        let len = d.length();
        if len < p.neighbour_dist && len > 1.0e-3 {
            out += d / len * (1.0 - len / p.neighbour_dist) * p.max_speed;
        }
    }
    out
}

/// Port (structure) of gCBoid::GetNeighbourVelocity (0x101acf30): the mean velocity of the perceived
/// members relative to the member's own, scaled by `neighbour_scale`.
pub fn neighbour_velocity(me: usize, flock: &[Boid], p: &FlockParams) -> Vec3 {
    let mut sum = Vec3::ZERO;
    let mut n = 0;
    for (i, b) in flock.iter().enumerate() {
        if i != me && b.pos.distance(flock[me].pos) <= p.seek_member_dist {
            sum += b.vel;
            n += 1;
        }
    }
    if n == 0 {
        Vec3::ZERO
    } else {
        (sum / n as f32 - flock[me].vel) * p.neighbour_scale
    }
}

/// Port (structure) of gCBoid::GetPathVelocity (0x101ad0d0): velocity toward the travel point, at most
/// `max_speed`, scaled by `poi_scale`.
pub fn path_velocity(me: &Boid, travel_point: Vec3, p: &FlockParams) -> Vec3 {
    let to = travel_point - me.pos;
    let len = to.length();
    if len < 1.0e-3 {
        return Vec3::ZERO;
    }
    to / len * p.max_speed.min(len) * p.poi_scale
}

/// One steering step of the whole flock (`gCBoid::Process` in roam state): every member's new
/// velocity is the old one plus the weighted terms, clamped to `max_speed`, then integrated.
pub fn step(flock: &mut [Boid], travel_point: Vec3, p: &FlockParams, dt: f32) {
    let steer: Vec<Vec3> = (0..flock.len())
        .map(|i| {
            let cohesion = perceived_center(i, flock, p) * 0.01;
            cohesion
                + evade(i, flock, p)
                + neighbour_velocity(i, flock, p) * 0.125
                + path_velocity(&flock[i], travel_point, p)
        })
        .collect();
    for (b, s) in flock.iter_mut().zip(steer) {
        b.vel += s * dt;
        if b.vel.length() > p.max_speed {
            b.vel = b.vel.normalize() * p.max_speed;
        }
        b.pos += b.vel * dt;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn at(x: f32, y: f32, z: f32) -> Boid {
        Boid {
            pos: Vec3::new(x, y, z),
            vel: Vec3::ZERO,
        }
    }

    #[test]
    fn cohesion_points_at_the_flock_centre() {
        let f = [
            at(0.0, 0.0, 0.0),
            at(100.0, 0.0, 0.0),
            at(100.0, 0.0, 200.0),
        ];
        let c = perceived_center(0, &f, &FlockParams::default());
        assert_eq!(c, Vec3::new(100.0, 0.0, 100.0));
    }

    #[test]
    fn members_out_of_sight_are_ignored() {
        let f = [at(0.0, 0.0, 0.0), at(5000.0, 0.0, 0.0)];
        assert_eq!(perceived_center(0, &f, &FlockParams::default()), Vec3::ZERO);
    }

    #[test]
    fn close_members_push_apart() {
        let f = [at(0.0, 0.0, 0.0), at(50.0, 0.0, 0.0)];
        let e = evade(0, &f, &FlockParams::default());
        assert!(e.x < 0.0 && e.y == 0.0);
        assert_eq!(
            evade(
                0,
                &[at(0.0, 0.0, 0.0), at(500.0, 0.0, 0.0)],
                &FlockParams::default()
            ),
            Vec3::ZERO
        );
    }

    #[test]
    fn the_flock_heads_for_the_travel_point_and_keeps_its_speed_limit() {
        let mut f: Vec<Boid> = (0..8).map(|i| at(i as f32 * 90.0, 0.0, 0.0)).collect();
        let p = FlockParams::default();
        let target = Vec3::new(0.0, 0.0, 20_000.0);
        for _ in 0..600 {
            step(&mut f, target, &p, 1.0 / 30.0);
        }
        assert!(f.iter().all(|b| b.vel.length() <= p.max_speed + 1.0));
        assert!(f.iter().all(|b| b.pos.z > 1_000.0));
    }
}
