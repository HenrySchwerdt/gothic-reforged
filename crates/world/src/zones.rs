//! Avoidance volumes of the navigation: collision circles around characters and objects, and
//! negative zones (`gCCollisionCircle_PSObj`, `gCDynamicCollisionCircle_PS`, `gCNegZone_PS`,
//! `gCNegZone_PSObj`, `gCDynamicCollisionCircle_PS::OnEnterProcessingRange`).
//!
//! All tests work in the ground plane (`x`, `z`): the original volumes are infinite vertical
//! cylinders and prisms. A *collision circle* keeps NPCs out of an entity (single cylinder) or
//! out of the lane an entity sweeps (double cylinder: two centres and two radii). A *negative
//! zone* is a polygon of stakes NPCs must not enter; it carries a bounding circle for cheap
//! culling and is checked for self-intersection when authored.
use crate::pvs::EntityId;
use bevy::math::Vec2;
use std::collections::BTreeMap;

/// Shape of a collision circle (`gCCollisionCircle_PSObj+0x30`).
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum CircleShape {
    /// One cylinder.
    Single { centre: Vec2, radius: f32 },
    /// Two cylinders joined by a lane; the radius changes linearly along it.
    Double { a: Vec2, b: Vec2, ra: f32, rb: f32 },
}

impl CircleShape {
    /// Port of `gCCollisionCircle_PSObj::IntersectsPoint` (Game:0x1013b190): is the point
    /// inside the volume? `inclusive` includes the border (`bCInfCylinder::Cull`'s flag).
    pub fn contains(&self, p: Vec2, inclusive: bool) -> bool {
        gothic_portcov::hit!("contains");
        let (d2, r) = match *self {
            Self::Single { centre, radius } => (p.distance_squared(centre), radius),
            Self::Double { a, b, ra, rb } => {
                let (t, closest) = closest_on_segment(p, a, b);
                (p.distance_squared(closest), ra + (rb - ra) * t)
            }
        };
        if inclusive { d2 <= r * r } else { d2 < r * r }
    }

    /// Port of `gCCollisionCircle_PSObj::IntersectsStretch` (Game:0x1013ad30): does the segment
    /// `from -> to` touch the volume? Returns the entry parameter (0..1) of the first touch.
    pub fn first_hit(&self, from: Vec2, to: Vec2) -> Option<f32> {
        gothic_portcov::hit!("first_hit");
        if self.contains(from, true) {
            return Some(0.0);
        }
        // Sampled sweep against the (possibly varying) radius: the segment is cut so that no
        // step is longer than a quarter of the smallest radius.
        let r_min = match *self {
            Self::Single { radius, .. } => radius,
            Self::Double { ra, rb, .. } => ra.min(rb),
        }
        .max(1.0);
        let len = from.distance(to);
        let steps = ((len / (r_min * 0.25)).ceil() as usize).clamp(1, 4096);
        (1..=steps)
            .map(|i| i as f32 / steps as f32)
            .find(|t| self.contains(from.lerp(to, *t), true))
    }
}

fn closest_on_segment(p: Vec2, a: Vec2, b: Vec2) -> (f32, Vec2) {
    let ab = b - a;
    let l2 = ab.length_squared();
    if l2 <= f32::EPSILON {
        return (0.0, a);
    }
    let t = ((p - a).dot(ab) / l2).clamp(0.0, 1.0);
    (t, a + ab * t)
}

/// A collision circle attached to an entity.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CollisionCircle {
    pub shape: CircleShape,
    pub enabled: bool,
}

/// Negative zone polygon (`gCNegZone_PS` stakes) with the data `OnStakesChanged`
/// (Game:0x10140900) derives: the bounding circle and the winding direction.
#[derive(Debug, Clone, PartialEq)]
pub struct NegZone {
    stakes: Vec<Vec2>,
    centre: Vec2,
    radius: f32,
    /// The stakes run clockwise in the ground plane.
    pub clockwise: bool,
}

impl NegZone {
    /// Build from stakes in world space. `None` for fewer than three stakes.
    pub fn new(stakes: Vec<Vec2>) -> Option<Self> {
        if stakes.len() < 3 {
            return None;
        }
        let centre = stakes.iter().copied().sum::<Vec2>() / stakes.len() as f32;
        let radius = stakes
            .iter()
            .map(|s| s.distance(centre))
            .fold(0.0, f32::max);
        let area2: f32 = stakes
            .iter()
            .zip(stakes.iter().cycle().skip(1))
            .map(|(a, b)| a.x * b.y - b.x * a.y)
            .sum();
        Some(Self {
            stakes,
            centre,
            radius,
            clockwise: area2 < 0.0,
        })
    }

    pub fn stakes(&self) -> &[Vec2] {
        &self.stakes
    }
    pub fn bounding_circle(&self) -> (Vec2, f32) {
        (self.centre, self.radius)
    }

    /// Port of `gCNegZone_PS::Test3DPointInZone` (Game:0x10119710): the sum of the signed angles
    /// under which the stakes' edges are seen from the point; a full turn means inside. A point
    /// on a stake or on an edge is undecidable and gets `on_border`.
    pub fn contains(&self, p: Vec2, on_border: bool) -> bool {
        gothic_portcov::hit!("contains@2");
        let mut sum = 0.0_f32;
        let n = self.stakes.len();
        for i in 0..n {
            let a = self.stakes[i] - p;
            let b = self.stakes[(i + 1) % n] - p;
            if a.length() < 1.0e-4 || b.length() < 1.0e-4 {
                return on_border;
            }
            let ang = a.perp_dot(b).atan2(a.dot(b));
            if ang.abs() > std::f32::consts::PI - 1.0e-3 {
                return on_border;
            }
            sum += ang;
        }
        sum.abs() > std::f32::consts::PI
    }

    /// Port of `gCNegZone_PSObj::IntersectsPoint` (Game:0x1013c780): the bounding circle culls
    /// first, then the polygon decides.
    pub fn intersects_point(&self, p: Vec2, on_border: bool) -> bool {
        gothic_portcov::hit!("intersects_point");
        p.distance(self.centre) <= self.radius && self.contains(p, on_border)
    }

    /// Port of `gCNegZone_PSObj::IntersectsStretch`: does the segment enter the zone (an end
    /// inside, or a crossing of an edge)?
    pub fn intersects_stretch(&self, from: Vec2, to: Vec2) -> bool {
        gothic_portcov::hit!("intersects_stretch");
        if self.intersects_point(from, true) || self.intersects_point(to, true) {
            return true;
        }
        let n = self.stakes.len();
        (0..n).any(|i| segments_cross(from, to, self.stakes[i], self.stakes[(i + 1) % n]))
    }

    /// Port of `gCNegZone_PS::IsZoneIllegal` (Game:0x10119470): an authored polygon whose
    /// non-adjacent edges cross cannot be walked around.
    pub fn is_illegal(&self) -> bool {
        gothic_portcov::hit!("is_illegal");
        let n = self.stakes.len();
        for i in 0..n {
            for j in i + 2..n {
                if i == 0 && j == n - 1 {
                    continue; // neighbours through the closing edge
                }
                if segments_cross(
                    self.stakes[i],
                    self.stakes[i + 1],
                    self.stakes[j],
                    self.stakes[(j + 1) % n],
                ) {
                    return true;
                }
            }
        }
        false
    }
}

/// Do the segments `a-b` and `c-d` cross (proper crossing)?
fn segments_cross(a: Vec2, b: Vec2, c: Vec2, d: Vec2) -> bool {
    let o = |p: Vec2, q: Vec2, r: Vec2| (q - p).perp_dot(r - p);
    let (d1, d2, d3, d4) = (o(c, d, a), o(c, d, b), o(a, b, c), o(a, b, d));
    d1 * d2 < 0.0 && d3 * d4 < 0.0
}

/// The avoidance volumes of one navigation scene.
#[derive(Debug, Default)]
pub struct AvoidanceSet {
    circles: BTreeMap<EntityId, CollisionCircle>,
    zones: BTreeMap<EntityId, NegZone>,
}

/// Which volume blocked a query.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Blocker {
    Circle(EntityId),
    Zone(EntityId),
}

impl AvoidanceSet {
    pub fn new() -> Self {
        Self::default()
    }

    /// Port of `gCDynamicCollisionCircle_PS::CreateCollisionCirclePSObject`
    /// (Game:0x10106a30): a circle exists while its radius is positive. Returns whether one was
    /// created.
    pub fn add_circle(&mut self, owner: EntityId, centre: Vec2, radius: f32) -> bool {
        gothic_portcov::hit!("add_circle");
        if radius <= 0.0 {
            return false;
        }
        self.circles.insert(
            owner,
            CollisionCircle {
                shape: CircleShape::Single { centre, radius },
                enabled: true,
            },
        );
        true
    }

    /// Port of `DestroyCollisionCirclePSObject` (Game:0x10106c90).
    pub fn remove_circle(&mut self, owner: EntityId) -> bool {
        gothic_portcov::hit!("remove_circle");
        self.circles.remove(&owner).is_some()
    }

    /// Port of `gDynamicCollisionCircle_PS::OnEnterProcessingRange` (Game:0x10108c20): the
    /// circle follows its entity; it is recreated at the entity's new place.
    pub fn follow(&mut self, owner: EntityId, centre: Vec2) {
        gothic_portcov::hit!("follow");
        if let Some(c) = self.circles.get_mut(&owner)
            && let CircleShape::Single { centre: ce, .. } = &mut c.shape
        {
            *ce = centre;
        }
    }

    /// `SetEnabled` of the circle object.
    pub fn set_circle_enabled(&mut self, owner: EntityId, on: bool) {
        if let Some(c) = self.circles.get_mut(&owner) {
            c.enabled = on;
        }
    }

    /// Install a negative zone for an entity (`gCNegZone_PS::CreatePSObject`); a zone that
    /// [`NegZone::is_illegal`] is refused, like the original refuses to use it.
    pub fn add_zone(&mut self, owner: EntityId, zone: NegZone) -> bool {
        if zone.is_illegal() {
            return false;
        }
        self.zones.insert(owner, zone);
        true
    }

    /// `OnPropertySetRemoved`.
    pub fn remove_zone(&mut self, owner: EntityId) -> bool {
        self.zones.remove(&owner).is_some()
    }

    pub fn circle_count(&self) -> usize {
        self.circles.len()
    }
    pub fn zone_count(&self) -> usize {
        self.zones.len()
    }

    /// The volumes that contain the point. `ignore` is the entity asking (its own circle never
    /// blocks it).
    pub fn blocked_at(&self, p: Vec2, ignore: Option<EntityId>) -> Vec<Blocker> {
        let mut out: Vec<Blocker> = self
            .circles
            .iter()
            .filter(|(e, c)| c.enabled && Some(**e) != ignore && c.shape.contains(p, false))
            .map(|(e, _)| Blocker::Circle(*e))
            .collect();
        out.extend(
            self.zones
                .iter()
                .filter(|(_, z)| z.intersects_point(p, false))
                .map(|(e, _)| Blocker::Zone(*e)),
        );
        out
    }

    /// The volumes a straight move from `a` to `b` runs into, with the first circle hit
    /// parameter where there is one.
    pub fn blocked_along(
        &self,
        a: Vec2,
        b: Vec2,
        ignore: Option<EntityId>,
    ) -> Vec<(Blocker, Option<f32>)> {
        let mut out: Vec<(Blocker, Option<f32>)> = self
            .circles
            .iter()
            .filter(|(e, c)| c.enabled && Some(**e) != ignore)
            .filter_map(|(e, c)| {
                c.shape
                    .first_hit(a, b)
                    .map(|t| (Blocker::Circle(*e), Some(t)))
            })
            .collect();
        out.extend(
            self.zones
                .iter()
                .filter(|(_, z)| z.intersects_stretch(a, b))
                .map(|(e, _)| (Blocker::Zone(*e), None)),
        );
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn square(c: Vec2, h: f32) -> NegZone {
        NegZone::new(vec![
            c + Vec2::new(-h, -h),
            c + Vec2::new(h, -h),
            c + Vec2::new(h, h),
            c + Vec2::new(-h, h),
        ])
        .unwrap()
    }

    #[test]
    fn zone_point_test_uses_the_angle_sum() {
        let z = square(Vec2::ZERO, 100.0);
        assert!(z.contains(Vec2::new(10.0, 20.0), false));
        assert!(!z.contains(Vec2::new(300.0, 0.0), false));
        // On a stake or an edge the caller's default decides.
        assert!(z.contains(Vec2::new(100.0, 100.0), true));
        assert!(!z.contains(Vec2::new(100.0, 0.0), false));
        assert!(z.intersects_point(Vec2::ZERO, false));
        // A segment through the zone from outside to outside crosses an edge.
        assert!(z.intersects_stretch(Vec2::new(-300.0, 0.0), Vec2::new(300.0, 0.0)));
        assert!(!z.intersects_stretch(Vec2::new(-300.0, 200.0), Vec2::new(300.0, 200.0)));
    }

    #[test]
    fn bowtie_zones_are_illegal() {
        let bow = NegZone::new(vec![
            Vec2::new(0.0, 0.0),
            Vec2::new(100.0, 100.0),
            Vec2::new(100.0, 0.0),
            Vec2::new(0.0, 100.0),
        ])
        .unwrap();
        assert!(bow.is_illegal());
        assert!(!square(Vec2::ZERO, 50.0).is_illegal());
        assert!(NegZone::new(vec![Vec2::ZERO, Vec2::X]).is_none());
        let mut set = AvoidanceSet::new();
        assert!(!set.add_zone(EntityId(1), bow));
        assert!(set.add_zone(EntityId(2), square(Vec2::ZERO, 50.0)));
    }

    #[test]
    fn collision_circles_block_points_and_sweeps_but_not_their_owner() {
        let mut set = AvoidanceSet::new();
        assert!(!set.add_circle(EntityId(1), Vec2::ZERO, 0.0));
        assert!(set.add_circle(EntityId(1), Vec2::new(100.0, 0.0), 50.0));
        assert_eq!(
            set.blocked_at(Vec2::new(120.0, 0.0), None),
            vec![Blocker::Circle(EntityId(1))]
        );
        assert!(
            set.blocked_at(Vec2::new(120.0, 0.0), Some(EntityId(1)))
                .is_empty()
        );
        let hit = set.blocked_along(Vec2::ZERO, Vec2::new(300.0, 0.0), None);
        assert_eq!(hit.len(), 1);
        let t = hit[0].1.unwrap();
        assert!((0.15..0.35).contains(&t), "entry at {t}");
        // The circle follows its entity out of the way.
        set.follow(EntityId(1), Vec2::new(100.0, 500.0));
        assert!(
            set.blocked_along(Vec2::ZERO, Vec2::new(300.0, 0.0), None)
                .is_empty()
        );
        set.set_circle_enabled(EntityId(1), false);
        assert!(set.blocked_at(Vec2::new(100.0, 500.0), None).is_empty());
        assert!(set.remove_circle(EntityId(1)));
    }

    #[test]
    fn double_cylinder_covers_the_lane_between_its_ends() {
        let lane = CircleShape::Double {
            a: Vec2::ZERO,
            b: Vec2::new(400.0, 0.0),
            ra: 40.0,
            rb: 80.0,
        };
        assert!(lane.contains(Vec2::new(200.0, 50.0), false));
        assert!(!lane.contains(Vec2::new(200.0, 70.0), false));
        assert!(lane.contains(Vec2::new(400.0, 70.0), false));
    }
}
