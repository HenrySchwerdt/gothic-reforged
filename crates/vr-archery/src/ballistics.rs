//! Bevy-free arrow flight: gravity, quadratic drag, swept collision against world surfaces
//! and body hitzones, sticking (with speed-dependent depth), glancing ricochets.
use glam::{Quat, Vec3};
use gothic_vr::Pose;
use gothic_vr_melee::Hitzone;
use gothic_vr_melee::geom::segment_segment;
use serde::{Deserialize, Serialize};

/// Physical arrow parameters.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct ArrowSpec {
    /// kg.
    pub mass: f32,
    /// Shaft radius, m.
    pub radius: f32,
    /// Distance from the arrow's centre to its tip, m.
    pub half_length: f32,
    /// Drag constant `k` in `a = -k |v| v` (1/m): `0.5 * rho * Cd * A / m`.
    pub drag: f32,
    /// The ammunition's own damage value (Gothic item data, supplied locally).
    pub damage: f32,
}

impl Default for ArrowSpec {
    fn default() -> Self {
        let area = std::f32::consts::PI * 0.0045 * 0.0045;
        Self {
            mass: 0.03,
            radius: 0.005,
            half_length: 0.46875,
            drag: 0.5 * 1.225 * 1.6 * area / 0.03,
            damage: 10.0,
        }
    }
}

/// Anything an arrow can fly into besides bodies.
pub trait RayWorld {
    /// First hit along a normalised direction within `max` m: (distance, surface normal).
    fn raycast(&self, origin: Vec3, dir: Vec3, max: f32) -> Option<(f32, Vec3)>;
}

/// No world (open air).
pub struct NoWorld;

impl RayWorld for NoWorld {
    fn raycast(&self, _: Vec3, _: Vec3, _: f32) -> Option<(f32, Vec3)> {
        None
    }
}

/// An infinite horizontal plane at `y` plus axis-aligned boxes (tests, greybox ranges).
#[derive(Debug, Clone, Default)]
pub struct BoxWorld {
    pub floor: Option<f32>,
    /// (min, max) corners.
    pub boxes: Vec<(Vec3, Vec3)>,
}

impl RayWorld for BoxWorld {
    fn raycast(&self, o: Vec3, d: Vec3, max: f32) -> Option<(f32, Vec3)> {
        let mut best: Option<(f32, Vec3)> = None;
        let mut take = |t: f32, n: Vec3| {
            if (0.0..=max).contains(&t) && best.is_none_or(|b| t < b.0) {
                best = Some((t, n));
            }
        };
        if let Some(y) = self.floor
            && d.y < -1e-6
        {
            take((y - o.y) / d.y, Vec3::Y);
        }
        for (lo, hi) in &self.boxes {
            // Slab test with the entering face's normal.
            let mut t0 = f32::NEG_INFINITY;
            let mut t1 = f32::INFINITY;
            let mut n = Vec3::ZERO;
            let mut ok = true;
            for axis in 0..3 {
                let (oa, da, l, h) = (o[axis], d[axis], lo[axis], hi[axis]);
                if da.abs() < 1e-9 {
                    if oa < l || oa > h {
                        ok = false;
                    }
                    continue;
                }
                let (mut a, mut b) = ((l - oa) / da, (h - oa) / da);
                let mut face = Vec3::ZERO;
                face[axis] = -da.signum();
                if a > b {
                    std::mem::swap(&mut a, &mut b);
                }
                if a > t0 {
                    t0 = a;
                    n = face;
                }
                t1 = t1.min(b);
            }
            if ok && t0 <= t1 && t0 >= 0.0 {
                take(t0, n);
            }
        }
        best
    }
}

/// Where an arrow ended up.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub enum StuckIn {
    /// Stuck in a world surface at this tip pose.
    World { pose: Pose },
    /// Stuck in body `victim`'s hitzone `zone`, pose relative to that hitzone.
    Body {
        victim: u64,
        zone: usize,
        local: Pose,
    },
}

/// Result of one flight step.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum FlightEvent {
    HitWorld {
        point: Vec3,
        normal: Vec3,
        speed: f32,
    },
    HitBody {
        victim: u64,
        zone: usize,
        point: Vec3,
        speed: f32,
        direction: Vec3,
    },
    Ricochet {
        point: Vec3,
    },
}

/// A flying (or stuck) arrow. `pos` is the arrow's centre.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Arrow {
    pub spec: ArrowSpec,
    pub pos: Vec3,
    pub vel: Vec3,
    pub stuck: Option<StuckIn>,
    /// Seconds in flight.
    pub age: f32,
    /// Who shot it.
    pub shooter: Option<u64>,
}

/// Below this speed (m/s) an arrow that hits something just drops.
pub const MIN_STICK_SPEED: f32 = 8.0;
/// Hits flatter than this (`-dir . normal`) glance off.
pub const GLANCE: f32 = 0.2;
const GRAVITY: Vec3 = Vec3::new(0.0, -9.81, 0.0);

impl Arrow {
    pub fn new(spec: ArrowSpec, pos: Vec3, vel: Vec3, shooter: Option<u64>) -> Self {
        Self {
            spec,
            pos,
            vel,
            stuck: None,
            age: 0.0,
            shooter,
        }
    }

    /// Orientation: -Z along the flight direction.
    pub fn rotation(&self) -> Quat {
        let d = self.vel.normalize_or_zero();
        if d == Vec3::ZERO {
            return Quat::IDENTITY;
        }
        Quat::from_rotation_arc(Vec3::NEG_Z, d)
    }

    pub fn tip(&self) -> Vec3 {
        self.pos + self.vel.normalize_or_zero() * self.spec.half_length
    }

    /// World pose: in flight from the velocity, when stuck from what it is stuck in.
    pub fn pose(&self, zones: impl Fn(u64, usize) -> Option<Hitzone>) -> Pose {
        match self.stuck {
            None => Pose::new(self.pos, self.rotation()),
            Some(StuckIn::World { pose }) => pose,
            Some(StuckIn::Body {
                victim,
                zone,
                local,
            }) => match zones(victim, zone) {
                Some(z) => z.pose.mul(&local),
                None => Pose::new(self.pos, self.rotation()),
            },
        }
    }

    /// Advance the flight by `dt`; bodies are `(id, hitzones)`, the shooter is ignored.
    pub fn step(
        &mut self,
        world: &dyn RayWorld,
        bodies: &[(u64, &[Hitzone])],
        dt: f32,
    ) -> Option<FlightEvent> {
        if self.stuck.is_some() || !(dt > 0.0 && dt.is_finite()) {
            return None;
        }
        self.age += dt;
        // Sub-steps keep the drag integration accurate at 90+ m/s.
        let n = ((self.vel.length() * dt / 0.25).ceil() as usize).clamp(1, 16);
        let h = dt / n as f32;
        for _ in 0..n {
            let speed = self.vel.length();
            let acc = GRAVITY - self.vel * (self.spec.drag * speed);
            let v1 = self.vel + acc * h;
            let tip0 = self.tip();
            let p1 = self.pos + v1 * h;
            let dir1 = v1.normalize_or_zero();
            let tip1 = p1 + dir1 * self.spec.half_length;
            if let Some(ev) = self.collide(world, bodies, tip0, tip1, v1) {
                return Some(ev);
            }
            self.vel = v1;
            self.pos = p1;
        }
        None
    }

    fn collide(
        &mut self,
        world: &dyn RayWorld,
        bodies: &[(u64, &[Hitzone])],
        tip0: Vec3,
        tip1: Vec3,
        v: Vec3,
    ) -> Option<FlightEvent> {
        let seg = tip1 - tip0;
        let len = seg.length();
        if len < 1e-7 {
            return None;
        }
        let dir = seg / len;
        let speed = v.length();
        // Earliest body hit along the tip's path.
        let mut body_hit: Option<(f32, u64, usize)> = None;
        for (id, zones) in bodies {
            if Some(*id) == self.shooter {
                continue;
            }
            for (zi, z) in zones.iter().enumerate() {
                let (a, b) = z.segment();
                let (p, q, s, _) = segment_segment(tip0, tip1, a, b);
                if p.distance(q) <= z.radius + self.spec.radius {
                    // Refine to the surface entry along the path.
                    let t =
                        entry_param(tip0, dir, len, a, b, z.radius + self.spec.radius).unwrap_or(s);
                    if body_hit.is_none_or(|h| t < h.0) {
                        body_hit = Some((t, *id, zi));
                    }
                }
            }
        }
        let world_hit = world.raycast(tip0, dir, len).map(|(d, n)| (d / len, n));
        let vdir = v.normalize_or_zero();
        match (body_hit, world_hit) {
            (Some((tb, id, zi)), w) if w.is_none_or(|(tw, _)| tb <= tw) => {
                let point = tip0 + seg * tb;
                if speed < MIN_STICK_SPEED {
                    self.vel *= 0.1;
                    return None;
                }
                let zone = bodies
                    .iter()
                    .find(|(b, _)| *b == id)
                    .map(|(_, z)| z[zi])
                    .expect("zone");
                let depth = (speed * 0.003).min(0.12);
                let tip = point + vdir * depth;
                let pose = Pose::new(
                    tip - vdir * self.spec.half_length,
                    Quat::from_rotation_arc(Vec3::NEG_Z, vdir),
                );
                self.stuck = Some(StuckIn::Body {
                    victim: id,
                    zone: zi,
                    local: zone.pose.inverse().mul(&pose),
                });
                self.pos = pose.position;
                self.vel = Vec3::ZERO;
                Some(FlightEvent::HitBody {
                    victim: id,
                    zone: zi,
                    point,
                    speed,
                    direction: vdir,
                })
            }
            (_, Some((tw, normal))) => {
                let point = tip0 + seg * tw;
                let into = -vdir.dot(normal);
                if into < GLANCE && speed >= MIN_STICK_SPEED {
                    // Ricochet: reflect and lose most of the energy.
                    let r = v - normal * 2.0 * v.dot(normal);
                    self.vel = r * 0.35;
                    self.pos = point + normal * 0.02
                        - self.vel.normalize_or_zero() * self.spec.half_length;
                    return Some(FlightEvent::Ricochet { point });
                }
                if speed < MIN_STICK_SPEED {
                    // Too slow to stick: falls (stays where it hit, lying).
                    let pose = Pose::new(
                        point + normal * self.spec.radius,
                        Quat::from_rotation_arc(Vec3::NEG_Z, vdir),
                    );
                    self.stuck = Some(StuckIn::World { pose });
                    self.vel = Vec3::ZERO;
                    return Some(FlightEvent::HitWorld {
                        point,
                        normal,
                        speed,
                    });
                }
                let depth = (speed * 0.002 * into).min(0.15);
                let tip = point + vdir * depth;
                let pose = Pose::new(
                    tip - vdir * self.spec.half_length,
                    Quat::from_rotation_arc(Vec3::NEG_Z, vdir),
                );
                self.stuck = Some(StuckIn::World { pose });
                self.pos = pose.position;
                self.vel = Vec3::ZERO;
                Some(FlightEvent::HitWorld {
                    point,
                    normal,
                    speed,
                })
            }
            _ => None,
        }
    }
}

/// Parameter (0..1 of `len`) where a ray enters a capsule, if it does.
fn entry_param(o: Vec3, d: Vec3, len: f32, a: Vec3, b: Vec3, r: f32) -> Option<f32> {
    // March in small steps (the segment is at most a few dozen centimetres per sub-step).
    let steps = ((len / (r * 0.25).max(0.002)).ceil() as usize).clamp(1, 256);
    for i in 0..=steps {
        let t = i as f32 / steps as f32;
        let p = o + d * (len * t);
        let (_, c, _, _) = segment_segment(p, p, a, b);
        if p.distance(c) <= r {
            return Some(t);
        }
    }
    None
}

/// Launch direction that reaches `target` from `from` at `speed` under gravity (no drag),
/// low arc. `None` when out of range.
pub fn ballistic_direction(from: Vec3, target: Vec3, speed: f32) -> Option<Vec3> {
    let d = target - from;
    let horiz = Vec3::new(d.x, 0.0, d.z);
    let x = horiz.length();
    let y = d.y;
    let g = 9.81;
    let v2 = speed * speed;
    let disc = v2 * v2 - g * (g * x * x + 2.0 * y * v2);
    if disc < 0.0 || x < 1e-4 {
        return None;
    }
    let angle = ((v2 - disc.sqrt()) / (g * x)).atan();
    let h = horiz / x;
    Some((h * angle.cos() + Vec3::Y * angle.sin()).normalize())
}
