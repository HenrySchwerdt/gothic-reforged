//! The flight and touch rules of a projectile.
//!
//! Port of `gCProjectile_PS` (GameDll:0x10151cc0..0x10153130): `Shoot`, `ShootIteration`,
//! `GetDirection`, `OnTouch` and `OnProcess` (the decay and fade). The engine lets PhysX move
//! the body and reacts to contacts; this port steps the same rules itself and asks the host for
//! the contacts ([`ProjectileWorld`]).
//!
//! * Path `Line` / `Curve`: the body is given the same velocity every frame, gravity has no
//!   effect. `Missile` re-evaluates the direction towards the target's chest every
//!   `TargetUpdateMSec`. `Physics` gets its velocity once and then falls under gravity with the
//!   rigid body's linear damping.
//! * The first contact ends the flight (`IsFlying` false, `HasCollided` true) and resolves by
//!   `TouchBehavior`, see [`TouchBehavior`].
//! * A landed projectile decays `DecayDuration` seconds later (fading when `FadeOnDecay`).
use glam::Vec3;

use crate::params::{
    ATTACH_LIFETIME_MS, ATTACH_SHORT_MS, BOUNCE_RESTITUTION, EMBED_DEPTH_CM, GRAVITY_CM_S2,
    LINKED_DECAY_EXTRA_SECS, MAX_ATTACHMENTS, MAX_FLIGHT_SECS, PathStyle, ProjectileParams,
    REST_SPEED, ShapeMaterial, TouchBehavior, unit_or,
};

/// Longest step (seconds) the flight integrates at once, so the sweeps stay short.
pub const MAX_SUBSTEP: f32 = 1.0 / 120.0;
/// Tangential speed kept when a falling projectile slides off a surface [I].
const BOUNCE_FRICTION: f32 = 0.6;

/// A contact with static geometry.
#[derive(Debug, Clone, PartialEq)]
pub struct SurfaceHit {
    /// Position along the swept segment, `0..=1`.
    pub t: f32,
    /// Contact point.
    pub point: Vec3,
    /// Surface normal (towards the side the projectile came from).
    pub normal: Vec3,
    /// Material of the surface.
    pub material: ShapeMaterial,
}

/// A contact with a character.
#[derive(Debug, Clone, PartialEq)]
pub struct ActorHit {
    /// Position along the swept segment, `0..=1`.
    pub t: f32,
    /// Contact point.
    pub point: Vec3,
    /// Host id of the character.
    pub actor: u64,
    /// Name of the bone the contact lies on (`None` when the host has no skeleton shapes).
    pub bone: Option<String>,
    /// The bone name contains `_Head` (a head shot, damage doubles).
    pub head: bool,
    /// The character is the player (arrows vanish at once instead of sticking).
    pub is_player: bool,
    /// How many attachments (arrows) the character's visual already carries.
    pub attachments: usize,
}

/// What a projectile asks of its surroundings.
pub trait ProjectileWorld {
    /// First static contact of a sphere of `radius` swept from `from` to `to`. Trigger volumes
    /// are not contacts (`OnTouch` ignores them).
    fn surface(&self, from: Vec3, to: Vec3, radius: f32) -> Option<SurfaceHit>;
    /// First character contact along the same sweep; `ignore` is the shooter (the shot passes
    /// through its owner).
    fn actor(&self, from: Vec3, to: Vec3, radius: f32, ignore: u64) -> Option<ActorHit>;
    /// The point a homing projectile steers for: the chest bone (`<Species>_Spine_Spine_1`) of
    /// the target, falling back to its position. `None` when the target is gone.
    fn aim_point(&self, actor: u64) -> Option<Vec3>;
}

/// What happened during a step.
#[derive(Debug, Clone, PartialEq)]
pub enum Event {
    /// Contact with a character: apply the damage once.
    HitActor {
        /// The victim.
        actor: u64,
        /// Contact point.
        point: Vec3,
        /// Unit flight direction at impact.
        direction: Vec3,
        /// Speed at impact, cm/s.
        speed: f32,
        /// Bone it touched.
        bone: Option<String>,
        /// A head shot.
        head: bool,
    },
    /// Contact with the level: play `EffectPointImpact`, call `OnProjectileTouchsLevel`.
    HitLevel {
        /// Contact point.
        point: Vec3,
        /// Surface normal.
        normal: Vec3,
        /// Surface material.
        material: ShapeMaterial,
    },
    /// The projectile stopped and stands in the surface (or in the character it follows).
    Embedded {
        /// Where it stands.
        position: Vec3,
        /// The character whose node it follows.
        linked_to: Option<u64>,
    },
    /// The projectile hangs on a bone of the victim.
    Attached {
        /// The victim.
        actor: u64,
        /// The bone.
        bone: Option<String>,
        /// World position at the moment of contact.
        point: Vec3,
        /// How long it stays, milliseconds.
        lifetime_ms: u32,
    },
    /// The projectile was thrown back and falls on.
    Bounced {
        /// Where.
        point: Vec3,
    },
    /// The projectile is finished and the host removes it.
    Ended,
}

/// Where a projectile is in its life (`IsFlying`, `HasCollided`, the decay flags).
#[derive(Debug, Clone, PartialEq)]
pub enum Phase {
    /// Created, not shot yet.
    Ready,
    /// In flight.
    Flying,
    /// After a contact with physics left on: falling and bouncing, harmless.
    Falling,
    /// Lying still.
    Resting,
    /// Standing in a surface.
    Embedded,
    /// Hanging on a bone for the remaining milliseconds.
    Attached {
        /// The victim.
        actor: u64,
        /// The bone.
        bone: Option<String>,
        /// Time left.
        left_ms: u32,
    },
    /// Removed.
    Gone,
}

/// One projectile entity.
#[derive(Debug, Clone, PartialEq)]
pub struct Projectile {
    /// Template values.
    pub params: ProjectileParams,
    /// `PSInteraction.Owner`, the shooter.
    pub owner: u64,
    /// `TargetEntity`.
    pub target: Option<u64>,
    /// `TargetDirection`.
    pub target_direction: Vec3,
    /// Current position.
    pub position: Vec3,
    /// Position of the previous step (for interpolation).
    pub previous: Vec3,
    /// Current velocity, cm/s.
    pub velocity: Vec3,
    /// Unit direction the model points along.
    pub direction: Vec3,
    phase: Phase,
    update_timer: f32,
    flight_time: f32,
    decay_left: Option<f32>,
    linked: Option<u64>,
}

impl Projectile {
    /// A projectile of `owner`, not yet shot.
    pub fn new(params: ProjectileParams, owner: u64) -> Self {
        Self {
            params,
            owner,
            target: None,
            target_direction: Vec3::ZERO,
            position: Vec3::ZERO,
            previous: Vec3::ZERO,
            velocity: Vec3::ZERO,
            direction: Vec3::NEG_Z,
            phase: Phase::Ready,
            update_timer: 0.0,
            flight_time: 0.0,
            decay_left: None,
            linked: None,
        }
    }

    /// `SetTargetEntity`.
    pub fn set_target(&mut self, target: Option<u64>) {
        self.target = target;
    }

    /// `SetTargetDirection`.
    pub fn set_target_direction(&mut self, dir: Vec3) {
        self.target_direction = dir;
    }

    /// The phase.
    pub fn phase(&self) -> &Phase {
        &self.phase
    }

    /// `IsFlying`.
    pub fn is_flying(&self) -> bool {
        self.phase == Phase::Flying
    }

    /// `HasCollided`.
    pub fn has_collided(&self) -> bool {
        !matches!(self.phase, Phase::Ready | Phase::Flying)
    }

    /// The host removes the projectile.
    pub fn is_gone(&self) -> bool {
        self.phase == Phase::Gone
    }

    /// Render alpha: 1 until the last `DecayDuration` seconds before the decay, then following
    /// the ease curve down to 0 (`OnProcess`, only with `FadeOnDecay`).
    pub fn alpha(&self) -> f32 {
        match self.decay_left {
            Some(left) if self.params.fade_on_decay && left <= self.params.decay_secs => {
                ease(left / self.params.decay_secs.max(f32::EPSILON))
            }
            _ => 1.0,
        }
    }

    /// Port of `gCProjectile_PS::GetDirection`: the launch/steering velocity. A target entity
    /// wins (towards its chest), then the stored `TargetDirection`; the unit vector is scaled by
    /// `ShootVelocity` (km/h).
    fn steer(&self, world: &dyn ProjectileWorld) -> Option<Vec3> {
        gothic_portcov::hit!("steer");
        let dir = if let Some(t) = self.target {
            let aim = world.aim_point(t)?;
            unit_or(aim - self.position, self.direction)
        } else if self.target_direction.length_squared() > 0.0 {
            self.target_direction
        } else {
            return None;
        };
        Some(dir * self.params.speed_cm_s())
    }

    /// Port of `gCProjectile_PS::Shoot` (GameDll:0x10152210) with the first `ShootIteration`: put
    /// the projectile at `origin` and give it its launch velocity. A projectile with neither
    /// target nor direction stays where it is.
    pub fn shoot(&mut self, origin: Vec3, world: &dyn ProjectileWorld) {
        gothic_portcov::hit!("shoot");
        if self.phase != Phase::Ready {
            return;
        }
        self.position = origin;
        self.previous = origin;
        if let Some(v) = self.steer(world) {
            self.velocity = v;
            self.direction = unit_or(v, self.direction);
        } else {
            self.velocity = self.direction * self.params.speed_cm_s();
        }
        self.phase = Phase::Flying;
    }

    /// Advance by `dt` seconds (the scaled frame time) and report what happened.
    pub fn step(&mut self, dt: f32, world: &dyn ProjectileWorld) -> Vec<Event> {
        let mut events = Vec::new();
        let mut left = dt.max(0.0);
        self.previous = self.position;
        while left > 0.0 && !self.is_gone() {
            let h = left.min(MAX_SUBSTEP);
            left -= h;
            match self.phase {
                Phase::Flying => self.fly(h, world, &mut events),
                Phase::Falling => self.fall(h, world, &mut events),
                Phase::Embedded | Phase::Resting => self.wait(h, &mut events),
                Phase::Attached { .. } => self.hang(h, &mut events),
                Phase::Ready | Phase::Gone => break,
            }
        }
        events
    }

    /// `ShootIteration` plus the contact tests of one sub-step of the flight.
    fn fly(&mut self, h: f32, world: &dyn ProjectileWorld, events: &mut Vec<Event>) {
        self.flight_time += h;
        if self.flight_time > MAX_FLIGHT_SECS {
            self.phase = Phase::Gone;
            events.push(Event::Ended);
            return;
        }
        match self.params.path {
            PathStyle::Line | PathStyle::Curve => {}
            PathStyle::Missile => {
                self.update_timer += h;
                if self.update_timer * 1000.0 >= self.params.target_update_ms as f32 {
                    if let Some(v) = self.steer(world) {
                        self.velocity = v;
                    }
                    self.update_timer = 0.0;
                }
            }
            PathStyle::Physics => {
                self.velocity.y -= GRAVITY_CM_S2 * h;
                self.velocity *= (1.0 - self.params.linear_damping * h).max(0.0);
            }
        }
        let from = self.position;
        let to = from + self.velocity * h;
        let radius = self.params.radius;
        let surface = world.surface(from, to, radius);
        let actor = world.actor(from, to, radius, self.owner);
        // The earlier contact wins; a tie goes to the character.
        match (surface, actor) {
            (Some(s), Some(a)) if s.t < a.t => self.touch_level(s, events),
            (_, Some(a)) => self.touch_actor(a, events),
            (Some(s), None) => self.touch_level(s, events),
            (None, None) => {
                self.position = to;
                let speed = self.velocity.length();
                if speed > 1.0 {
                    self.direction = self.velocity / speed;
                }
            }
        }
    }

    fn impact_direction(&self) -> Vec3 {
        unit_or(self.velocity, self.direction)
    }

    /// `OnTouch` against static geometry.
    fn touch_level(&mut self, hit: SurfaceHit, events: &mut Vec<Event>) {
        let dir = self.impact_direction();
        self.position = hit.point;
        events.push(Event::HitLevel {
            point: hit.point,
            normal: hit.normal,
            material: hit.material,
        });
        match self.params.touch {
            TouchBehavior::KillSelf => self.end(events),
            TouchBehavior::Reflect => self.bounce(hit.point, hit.normal, events),
            TouchBehavior::Stop => self.embed(hit.point, dir, None, events),
            TouchBehavior::MaterialDependent => {
                if hit.material.takes_arrows() {
                    self.embed(hit.point, dir, None, events);
                } else {
                    self.bounce(hit.point, hit.normal, events);
                }
            }
        }
    }

    /// `OnTouch` against a character.
    fn touch_actor(&mut self, hit: ActorHit, events: &mut Vec<Event>) {
        let dir = self.impact_direction();
        let speed = self.velocity.length();
        self.position = hit.point;
        events.push(Event::HitActor {
            actor: hit.actor,
            point: hit.point,
            direction: dir,
            speed,
            bone: hit.bone.clone(),
            head: hit.head,
        });
        match self.params.touch {
            TouchBehavior::KillSelf => self.end(events),
            TouchBehavior::Reflect => {
                // The body keeps its physics and drops off the victim.
                self.velocity = -dir * (speed * BOUNCE_RESTITUTION * 0.5);
                self.become_falling(hit.point, events);
            }
            TouchBehavior::Stop => self.embed(hit.point, dir, Some(hit.actor), events),
            TouchBehavior::MaterialDependent => {
                if self.params.link_to_bones && hit.bone.is_some() {
                    self.attach(&hit, events);
                } else {
                    self.velocity = -dir * (speed * BOUNCE_RESTITUTION * 0.5);
                    self.become_falling(hit.point, events);
                }
            }
        }
    }

    /// `OnTouch`, `AddAttachment` with the lifetime rule of the shipped code: 60 s, or at once
    /// when the victim is the player, the hit is on the head, or it carries 8 attachments.
    fn attach(&mut self, hit: &ActorHit, events: &mut Vec<Event>) {
        let short = hit.is_player || hit.head || hit.attachments >= MAX_ATTACHMENTS;
        let lifetime_ms = if short {
            ATTACH_SHORT_MS
        } else {
            ATTACH_LIFETIME_MS
        };
        self.velocity = Vec3::ZERO;
        self.phase = Phase::Attached {
            actor: hit.actor,
            bone: hit.bone.clone(),
            left_ms: lifetime_ms,
        };
        self.decay_left = None;
        events.push(Event::Attached {
            actor: hit.actor,
            bone: hit.bone.clone(),
            point: hit.point,
            lifetime_ms,
        });
    }

    /// `OnTouch`, `Stop`: stand `EMBED_DEPTH_CM` along the flight past the contact. A projectile
    /// that follows a character's node decays `LINKED_DECAY_EXTRA_SECS` later.
    fn embed(&mut self, contact: Vec3, dir: Vec3, linked: Option<u64>, events: &mut Vec<Event>) {
        self.position = contact + dir * EMBED_DEPTH_CM;
        self.velocity = Vec3::ZERO;
        self.phase = Phase::Embedded;
        self.linked = linked;
        let extra = if linked.is_some() {
            LINKED_DECAY_EXTRA_SECS
        } else {
            0.0
        };
        self.decay_left = Some(self.params.decay_secs + extra);
        events.push(Event::Embedded {
            position: self.position,
            linked_to: linked,
        });
    }

    fn bounce(&mut self, point: Vec3, normal: Vec3, events: &mut Vec<Event>) {
        self.velocity = reflect(self.velocity, normal);
        self.position = point + normal * 0.5;
        self.become_falling(point, events);
    }

    fn become_falling(&mut self, point: Vec3, events: &mut Vec<Event>) {
        self.phase = Phase::Falling;
        self.decay_left = Some(self.params.decay_secs);
        events.push(Event::Bounced { point });
    }

    fn end(&mut self, events: &mut Vec<Event>) {
        self.phase = Phase::Gone;
        events.push(Event::Ended);
    }

    /// A harmless falling body: gravity, damping and a few bounces.
    fn fall(&mut self, h: f32, world: &dyn ProjectileWorld, events: &mut Vec<Event>) {
        self.velocity.y -= GRAVITY_CM_S2 * h;
        self.velocity *= (1.0 - self.params.linear_damping * h).max(0.0);
        let from = self.position;
        let to = from + self.velocity * h;
        if let Some(hit) = world.surface(from, to, self.params.radius) {
            self.position = hit.point + hit.normal * 0.5;
            let v = reflect(self.velocity, hit.normal);
            let along = v.dot(hit.normal);
            let tangent = (v - hit.normal * along) * BOUNCE_FRICTION;
            self.velocity = tangent + hit.normal * along;
            if self.velocity.length() < REST_SPEED {
                self.velocity = Vec3::ZERO;
                self.phase = Phase::Resting;
            }
        } else {
            self.position = to;
        }
        self.tick_decay(h, events);
    }

    /// Embedded or resting: only the decay runs.
    fn wait(&mut self, h: f32, events: &mut Vec<Event>) {
        self.tick_decay(h, events);
    }

    /// Hanging on a bone: the attachment expires.
    fn hang(&mut self, h: f32, events: &mut Vec<Event>) {
        if let Phase::Attached { left_ms, .. } = &mut self.phase {
            let spent = (h * 1000.0).ceil() as u32;
            if *left_ms <= spent {
                self.phase = Phase::Gone;
                events.push(Event::Ended);
            } else {
                *left_ms -= spent;
            }
        }
    }

    /// `OnProcess`: count the decay down; at zero the entity is removed.
    fn tick_decay(&mut self, h: f32, events: &mut Vec<Event>) {
        if let Some(left) = &mut self.decay_left {
            *left -= h;
            if *left < 0.0 {
                self.decay_left = None;
                self.phase = Phase::Gone;
                events.push(Event::Ended);
            }
        }
    }
}

/// Reflect `v` about the plane with unit `normal`, keeping `BOUNCE_RESTITUTION` of the normal
/// component.
fn reflect(v: Vec3, normal: Vec3) -> Vec3 {
    let along = v.dot(normal);
    if along >= 0.0 {
        return v;
    }
    v - normal * (along * (1.0 + BOUNCE_RESTITUTION))
}

/// The cosine ease of the engine (`FUN_10039c90`): `(1 - cos(pi t)) / 2` on `0..=1`.
fn ease(t: f32) -> f32 {
    let t = t.clamp(0.0, 1.0);
    (1.0 - (std::f32::consts::PI * t).cos()) * 0.5
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::params::PLAYER_ARROW_KMH_PER_DRAW;
    use crate::shape::{self, Capsule};

    /// A flat ground at `y = 0` of one material, a list of characters, and nothing else.
    #[derive(Default)]
    struct Field {
        ground: ShapeMaterial,
        actors: Vec<(u64, Capsule, bool, usize)>,
        wall_x: Option<(f32, ShapeMaterial)>,
    }

    impl ProjectileWorld for Field {
        fn surface(&self, from: Vec3, to: Vec3, _r: f32) -> Option<SurfaceHit> {
            let mut best: Option<SurfaceHit> = None;
            if from.y > 0.0 && to.y <= 0.0 {
                let t = from.y / (from.y - to.y);
                best = Some(SurfaceHit {
                    t,
                    point: from + (to - from) * t,
                    normal: Vec3::Y,
                    material: self.ground,
                });
            }
            if let Some((x, material)) = self.wall_x
                && from.x < x
                && to.x >= x
            {
                let t = (x - from.x) / (to.x - from.x);
                if best.as_ref().is_none_or(|b| t < b.t) {
                    best = Some(SurfaceHit {
                        t,
                        point: from + (to - from) * t,
                        normal: Vec3::NEG_X,
                        material,
                    });
                }
            }
            best
        }
        fn actor(&self, from: Vec3, to: Vec3, r: f32, ignore: u64) -> Option<ActorHit> {
            self.actors
                .iter()
                .filter(|(id, ..)| *id != ignore)
                .filter_map(|(id, cap, player, attachments)| {
                    let t = shape::sweep(from, to, cap, r)?;
                    let point = from + (to - from) * t;
                    let head = cap.height_fraction(point) > 0.85;
                    Some(ActorHit {
                        t,
                        point,
                        actor: *id,
                        bone: Some(
                            if head {
                                "Human_Head_Head"
                            } else {
                                "Human_Spine_Spine_1"
                            }
                            .into(),
                        ),
                        head,
                        is_player: *player,
                        attachments: *attachments,
                    })
                })
                .min_by(|a, b| a.t.total_cmp(&b.t))
        }
        fn aim_point(&self, actor: u64) -> Option<Vec3> {
            self.actors
                .iter()
                .find(|(id, ..)| *id == actor)
                .map(|(_, c, ..)| c.base + Vec3::Y * c.height * 0.7)
        }
    }

    fn man(id: u64, x: f32) -> (u64, Capsule, bool, usize) {
        (
            id,
            Capsule {
                base: Vec3::new(x, 0.0, 0.0),
                height: 180.0,
                radius: 35.0,
            },
            false,
            0,
        )
    }

    fn run(p: &mut Projectile, w: &Field, secs: f32) -> Vec<Event> {
        let mut out = Vec::new();
        let steps = (secs / 0.05).round() as usize;
        for _ in 0..steps {
            out.extend(p.step(0.05, w));
        }
        out
    }

    fn arrow(owner: u64, speed_kmh: f32, dir: Vec3) -> Projectile {
        let mut p = Projectile::new(ProjectileParams::arrow(), owner);
        p.params.shoot_velocity = speed_kmh;
        p.params.linear_damping = 0.0;
        p.set_target_direction(dir.normalize());
        p
    }

    #[test]
    fn a_fireball_flies_100_kmh_in_a_straight_line() {
        let w = Field::default();
        let mut p = Projectile::new(ProjectileParams::spell_missile(), 1);
        p.set_target_direction(Vec3::X);
        p.shoot(Vec3::new(0.0, 150.0, 0.0), &w);
        run(&mut p, &w, 1.0);
        assert!((p.position.x - 2777.78).abs() < 1.0, "x = {}", p.position.x);
        assert_eq!(p.position.y, 150.0, "gravity does not touch a line flight");
    }

    #[test]
    fn a_horizontal_arrow_drops_like_a_stone() {
        let w = Field::default();
        let mut p = arrow(1, 100.0, Vec3::X);
        p.shoot(Vec3::new(0.0, 1000.0, 0.0), &w);
        run(&mut p, &w, 0.5);
        let drop = 1000.0 - p.position.y;
        assert!((drop - 0.5 * 981.0 * 0.25).abs() < 3.0, "drop = {drop}");
        assert!((p.position.x - 0.5 * 2777.78).abs() < 2.0);
    }

    #[test]
    fn a_full_draw_at_45_degrees_flies_about_310_metres() {
        let w = Field::default();
        let mut p = arrow(1, PLAYER_ARROW_KMH_PER_DRAW, Vec3::new(1.0, 1.0, 0.0));
        p.shoot(Vec3::new(0.0, 1.0, 0.0), &w);
        let ev = run(&mut p, &w, 15.0);
        let range = ev
            .iter()
            .find_map(|e| match e {
                Event::HitLevel { point, .. } => Some(point.x),
                _ => None,
            })
            .expect("landed");
        // v^2 / g with v = 55.56 m/s.
        assert!((range - 31_464.0).abs() < 200.0, "range = {range}");
    }

    #[test]
    fn the_player_arrow_damping_costs_about_five_percent_of_the_range() {
        let w = Field::default();
        let mut p = Projectile::new(ProjectileParams::arrow(), 1);
        p.params.shoot_velocity = PLAYER_ARROW_KMH_PER_DRAW;
        p.params.linear_damping = crate::params::PLAYER_ARROW_DAMPING;
        p.set_target_direction(Vec3::new(1.0, 1.0, 0.0).normalize());
        p.shoot(Vec3::new(0.0, 1.0, 0.0), &w);
        let range = run(&mut p, &w, 15.0)
            .iter()
            .find_map(|e| match e {
                Event::HitLevel { point, .. } => Some(point.x),
                _ => None,
            })
            .expect("landed");
        assert!(range < 30_600.0 && range > 29_000.0, "{range}");
    }

    #[test]
    fn the_shot_passes_its_owner_and_hits_the_next_man() {
        let w = Field {
            actors: vec![man(1, 0.0), man(2, 1000.0)],
            ..Field::default()
        };
        let mut p = Projectile::new(ProjectileParams::spell_missile(), 1);
        p.set_target_direction(Vec3::X);
        p.shoot(Vec3::new(10.0, 120.0, 0.0), &w);
        let ev = run(&mut p, &w, 1.0);
        let hit = ev
            .iter()
            .find_map(|e| match e {
                Event::HitActor { actor, .. } => Some(*actor),
                _ => None,
            })
            .expect("hit");
        assert_eq!(hit, 2);
        assert!(p.is_gone(), "KillSelf ends the missile");
        assert_eq!(ev.last(), Some(&Event::Ended));
    }

    #[test]
    fn a_missile_with_a_target_homes_in_on_a_moving_man() {
        let mut w = Field {
            actors: vec![man(2, 1500.0)],
            ..Field::default()
        };
        let mut params = ProjectileParams::spell_missile();
        params.path = PathStyle::Missile;
        let mut p = Projectile::new(params, 1);
        p.set_target(Some(2));
        p.shoot(Vec3::new(0.0, 120.0, 0.0), &w);
        let mut hit = false;
        for i in 0..40 {
            // The man runs sideways at 5 m/s.
            w.actors[0].1.base.z = 5.0 * 50.0 * i as f32 * 0.05;
            let ev = p.step(0.05, &w);
            if ev
                .iter()
                .any(|e| matches!(e, Event::HitActor { actor: 2, .. }))
            {
                hit = true;
                break;
            }
        }
        assert!(hit, "a homing missile follows its target");
    }

    #[test]
    fn a_straight_missile_misses_a_man_who_stepped_aside() {
        let w = Field {
            actors: vec![(
                2,
                Capsule {
                    base: Vec3::new(1500.0, 0.0, 400.0),
                    height: 180.0,
                    radius: 35.0,
                },
                false,
                0,
            )],
            ..Field::default()
        };
        let mut p = Projectile::new(ProjectileParams::spell_missile(), 1);
        p.set_target_direction(Vec3::X);
        p.shoot(Vec3::new(0.0, 120.0, 0.0), &w);
        let ev = run(&mut p, &w, 2.0);
        assert!(!ev.iter().any(|e| matches!(e, Event::HitActor { .. })));
    }

    #[test]
    fn an_arrow_sticks_in_earth_35_cm_deep() {
        let w = Field {
            ground: ShapeMaterial::Earth,
            ..Field::default()
        };
        let mut p = arrow(1, 100.0, Vec3::new(1.0, -1.0, 0.0));
        p.shoot(Vec3::new(0.0, 200.0, 0.0), &w);
        let ev = run(&mut p, &w, 2.0);
        assert!(matches!(p.phase(), Phase::Embedded));
        let contact = ev
            .iter()
            .find_map(|e| match e {
                Event::HitLevel { point, .. } => Some(*point),
                _ => None,
            })
            .expect("contact");
        // The arrow was still rising/falling along the diagonal when it hit; it stands 35 cm
        // further along its direction of flight.
        let travelled = (p.position - contact).length();
        assert!((travelled - 35.0).abs() < 0.5, "{travelled}");
    }

    #[test]
    fn an_arrow_bounces_off_stone_and_lies_still() {
        let w = Field {
            ground: ShapeMaterial::Stone,
            ..Field::default()
        };
        let mut p = arrow(1, 100.0, Vec3::new(1.0, -0.5, 0.0));
        p.params.linear_damping = 0.1;
        p.shoot(Vec3::new(0.0, 300.0, 0.0), &w);
        let ev = run(&mut p, &w, 12.0);
        assert!(ev.iter().any(|e| matches!(e, Event::Bounced { .. })));
        assert!(
            !ev.iter().any(|e| matches!(e, Event::Embedded { .. })),
            "stone does not take arrows"
        );
        assert!(p.position.y < 5.0, "it ended on the ground");
    }

    #[test]
    fn a_hit_on_a_man_attaches_for_a_minute_but_not_on_the_player_the_head_or_a_full_quiver() {
        let hit = |player: bool, attachments: usize, z: f32| {
            let mut m = man(2, 600.0);
            m.2 = player;
            m.3 = attachments;
            let w = Field {
                actors: vec![m],
                ..Field::default()
            };
            let mut p = arrow(1, 150.0, Vec3::X);
            p.shoot(Vec3::new(0.0, z, 0.0), &w);
            run(&mut p, &w, 1.0)
                .into_iter()
                .find_map(|e| match e {
                    Event::Attached { lifetime_ms, .. } => Some(lifetime_ms),
                    _ => None,
                })
                .expect("attached")
        };
        assert_eq!(hit(false, 0, 100.0), 60_000);
        assert_eq!(hit(true, 0, 100.0), 1, "the player's body shows no arrows");
        assert_eq!(
            hit(false, 0, 170.0),
            1,
            "a head shot takes the arrow with it"
        );
        assert_eq!(hit(false, 8, 100.0), 1, "eight arrows are enough");
    }

    #[test]
    fn a_man_is_only_hit_once_and_damage_comes_with_the_hit_event() {
        let w = Field {
            actors: vec![man(2, 500.0)],
            ..Field::default()
        };
        let mut p = arrow(1, 150.0, Vec3::X);
        p.shoot(Vec3::new(0.0, 100.0, 0.0), &w);
        let ev = run(&mut p, &w, 3.0);
        let hits = ev
            .iter()
            .filter(|e| matches!(e, Event::HitActor { .. }))
            .count();
        assert_eq!(hits, 1);
    }

    #[test]
    fn a_wall_stops_a_fireball_and_reports_the_level_hit() {
        let w = Field {
            wall_x: Some((1000.0, ShapeMaterial::Stone)),
            ..Field::default()
        };
        let mut p = Projectile::new(ProjectileParams::spell_missile(), 1);
        p.set_target_direction(Vec3::X);
        p.shoot(Vec3::new(0.0, 150.0, 0.0), &w);
        let ev = run(&mut p, &w, 1.0);
        assert!(matches!(
            &ev[0],
            Event::HitLevel {
                material: ShapeMaterial::Stone,
                ..
            }
        ));
        assert!(p.is_gone());
    }

    #[test]
    fn a_bolt_drops_off_the_man_it_hit() {
        let w = Field {
            ground: ShapeMaterial::Earth,
            actors: vec![man(2, 500.0)],
            ..Field::default()
        };
        let mut p = Projectile::new(ProjectileParams::bolt(), 1);
        p.params.shoot_velocity = 150.0;
        p.params.linear_damping = 0.0;
        p.set_target_direction(Vec3::X);
        p.shoot(Vec3::new(0.0, 100.0, 0.0), &w);
        let ev = run(&mut p, &w, 5.0);
        assert!(ev.iter().any(|e| matches!(e, Event::HitActor { .. })));
        assert!(ev.iter().any(|e| matches!(e, Event::Bounced { .. })));
        assert!(!p.is_flying());
    }

    #[test]
    fn embedded_arrows_fade_and_vanish_after_the_decay_duration() {
        let w = Field {
            ground: ShapeMaterial::Earth,
            ..Field::default()
        };
        let mut p = arrow(1, 100.0, Vec3::new(1.0, -1.0, 0.0));
        p.shoot(Vec3::new(0.0, 100.0, 0.0), &w);
        run(&mut p, &w, 1.0);
        assert!(matches!(p.phase(), Phase::Embedded));
        assert!(p.alpha() > 0.9);
        let mut faded = false;
        let mut ended = false;
        for _ in 0..400 {
            let ev = p.step(0.05, &w);
            faded |= p.alpha() < 0.5 && !p.is_gone();
            ended |= ev.contains(&Event::Ended);
            if ended {
                break;
            }
        }
        assert!(faded && ended);
    }

    #[test]
    fn a_lost_arrow_ends_after_the_flight_limit() {
        let w = Field::default();
        let mut p = Projectile::new(ProjectileParams::spell_missile(), 1);
        p.set_target_direction(Vec3::Y);
        p.shoot(Vec3::ZERO, &w);
        let ev = run(&mut p, &w, MAX_FLIGHT_SECS + 1.0);
        assert_eq!(ev.last(), Some(&Event::Ended));
    }
}
