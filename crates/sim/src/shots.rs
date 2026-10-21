//! Arrows, bolts and spell missiles in flight.
//!
//! The rules live in `gothic-projectile` (port of `gCProjectile_PS`); this module gives them the
//! simulation's world: the characters as vertical capsules, the level through
//! [`WorldQuery::surface_hit`] and the hero's aim point set by the camera
//! ([`Sim::set_hero_aim`]). A hit calls the damage pipeline once; the shot then sticks, bounces,
//! hangs on a bone or ends as its template says.
use gothic_projectile::ProjectileParams;
use gothic_projectile::flight::{
    ActorHit, Event as ShotEvent, Phase, Projectile, ProjectileWorld, SurfaceHit,
};
use gothic_projectile::launch::{self, Launch};
use gothic_projectile::params::{ShapeMaterial, speed_cm_s};
use gothic_projectile::shape::{self, Capsule};
use gothic_script::magic::api::ProjectileSpec;

use crate::script_host::actor_of;
use crate::sim::{EventKind, Sim};
use crate::types::{ActorId, Species};
use crate::world::{Pos, WorldQuery};

pub use glam::Vec3;

/// Where the hero's arrow starts and which point it flies at; set by the camera each frame while
/// the hero aims (`Shoot`, S:86450: the pose of the arrow in the right hand and
/// `PSCamera::GetScreenCenterPickPoint`).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct HeroAim {
    /// World position of the arrow in the hand.
    pub origin: Vec3,
    /// The point under the crosshair.
    pub pick_point: Vec3,
}

/// What the presentation needs of one projectile in flight.
#[derive(Debug, Clone, PartialEq)]
pub struct ShotView {
    /// Stable id for the lifetime of the shot.
    pub id: u64,
    /// The template (`It_Arrow`, `Mis_Fireball`).
    pub template: String,
    /// A spell missile.
    pub spell: bool,
    /// The shooter.
    pub owner: ActorId,
    /// Current position.
    pub position: Vec3,
    /// Position one step ago, for interpolation.
    pub previous: Vec3,
    /// Unit direction the model points along.
    pub direction: Vec3,
    /// Render alpha (the fade of a decaying arrow).
    pub alpha: f32,
    /// The shot is still in the air.
    pub flying: bool,
    /// Hanging on a bone of this actor, with the bone name.
    pub attached: Option<(ActorId, Option<String>)>,
}

/// Collision body of a species `(radius, height)` in cm [I]: the hero's movement cylinder is
/// 33 by 134; the body shapes the engine hits are not decoded.
pub fn body_of(species: Species) -> (f32, f32) {
    use Species::*;
    match species {
        Human | Skeleton | Zombie | Goblin | Demon => (35.0, 180.0),
        Orc => (45.0, 200.0),
        Troll | Ogre | Golem | FireGolem | IceGolem => (80.0, 320.0),
        Dragon => (200.0, 400.0),
        Wolf | Boar | Sabertooth | Shadowbeast | Ripper | Lurker | Varan | Snapper | Stalker
        | Bison | Rhino | Alligator | FireVaran | Cow | Deer | Molerat | Pig | SwampLurker => {
            (45.0, 110.0)
        }
        Trex | ScorpionKing | Minecrawler => (90.0, 200.0),
        _ => (30.0, 60.0),
    }
}

fn vec(p: Pos) -> Vec3 {
    Vec3::new(p.x, p.y, p.z)
}

fn pos(v: Vec3) -> Pos {
    Pos::new(v.x, v.y, v.z)
}

/// The simulation as seen by a projectile.
struct ShotField<'a, W: WorldQuery> {
    sim: &'a Sim<W>,
    /// Shots hanging on each actor, counted before the step.
    attached: std::collections::BTreeMap<u64, usize>,
}

impl<W: WorldQuery> ShotField<'_, W> {
    fn capsule(&self, id: ActorId) -> Option<Capsule> {
        let a = self.sim.actors.get(id.0 as usize)?;
        if !a.is_alive() {
            return None;
        }
        let at = self.sim.world.position(id)?;
        let (radius, height) = body_of(a.species);
        Some(Capsule {
            base: vec(at),
            height,
            radius,
        })
    }
}

impl<W: WorldQuery> ProjectileWorld for ShotField<'_, W> {
    fn surface(&self, from: Vec3, to: Vec3, _radius: f32) -> Option<SurfaceHit> {
        let mut best: Option<SurfaceHit> = None;
        if let Some(g) = self.sim.world.ground_height(from.x, from.z)
            && from.y > g
            && to.y <= g
        {
            let t = (from.y - g) / (from.y - to.y);
            best = Some(SurfaceHit {
                t,
                point: from + (to - from) * t,
                normal: Vec3::Y,
                material: ShapeMaterial::Earth,
            });
        }
        if let Some(c) = self.sim.world.surface_hit(pos(from), pos(to))
            && best.as_ref().is_none_or(|b| c.t < b.t)
        {
            best = Some(SurfaceHit {
                t: c.t,
                point: from + (to - from) * c.t,
                normal: Vec3::from(c.normal),
                material: ShapeMaterial::from_value(i64::from(c.material)),
            });
        }
        best
    }

    fn actor(&self, from: Vec3, to: Vec3, radius: f32, ignore: u64) -> Option<ActorHit> {
        let mut best: Option<ActorHit> = None;
        for a in &self.sim.actors {
            let id = a.id;
            if u64::from(id.0) == ignore {
                continue;
            }
            let Some(cap) = self.capsule(id) else {
                continue;
            };
            let Some(t) = shape::sweep(from, to, &cap, radius) else {
                continue;
            };
            if best.as_ref().is_some_and(|b| b.t <= t) {
                continue;
            }
            let point = from + (to - from) * t;
            let head = cap.height_fraction(point) > 0.85;
            best = Some(ActorHit {
                t,
                point,
                actor: u64::from(id.0),
                bone: Some(if head { "Head_Head" } else { "Spine_Spine_1" }.to_string()),
                head,
                is_player: a.is_player,
                attachments: self.attached.get(&u64::from(id.0)).copied().unwrap_or(0),
            });
        }
        best
    }

    fn aim_point(&self, actor: u64) -> Option<Vec3> {
        let cap = self.capsule(ActorId(actor as u32))?;
        Some(cap.base + Vec3::Y * cap.height * 0.7)
    }
}

/// A projectile and the spec it was launched with.
#[derive(Debug, Clone, PartialEq)]
pub struct Shot {
    /// Id of the shot.
    pub id: u64,
    /// Rules, owner and aim.
    pub projectile: Projectile,
    /// What the script asked for.
    pub spec: ProjectileSpec,
}

impl<W: WorldQuery> Sim<W> {
    /// The hero's arrow and aim point for the next shot (the camera's job, every frame while
    /// aiming). Without it the arrow starts at the chest and is lobbed at the target.
    pub fn set_hero_aim(&mut self, aim: Option<HeroAim>) {
        self.script.magic.hero_aim = aim;
    }

    /// Replace the template values of a projectile (`gCProjectile_PS` and rigid body
    /// properties of a decoded template), by template name.
    pub fn set_projectile_params(&mut self, template: &str, params: ProjectileParams) {
        self.script
            .magic
            .projectile_params
            .insert(template.into(), params);
    }

    /// Everything the presentation draws: the shots in flight, stuck or hanging.
    pub fn shot_views(&self) -> Vec<ShotView> {
        self.script
            .magic
            .flights
            .iter()
            .filter_map(|f| {
                let p = &f.shot.projectile;
                let owner = actor_of(f.shot.spec.owner)?;
                Some(ShotView {
                    id: f.shot.id,
                    template: f.shot.spec.template.clone(),
                    spell: !f.shot.spec.spell.is_empty(),
                    owner,
                    position: p.position,
                    previous: p.previous,
                    direction: p.direction,
                    alpha: p.alpha(),
                    flying: p.is_flying(),
                    attached: match p.phase() {
                        Phase::Attached { actor, bone, .. } => {
                            Some((ActorId(*actor as u32), bone.clone()))
                        }
                        _ => None,
                    },
                })
            })
            .collect()
    }

    fn params_for(&self, spec: &ProjectileSpec) -> ProjectileParams {
        if let Some(p) = self.script.magic.projectile_params.get(&spec.template) {
            return p.clone();
        }
        if spec.spell.is_empty() {
            if spec.template.to_ascii_lowercase().contains("bolt") {
                ProjectileParams::bolt()
            } else {
                ProjectileParams::arrow()
            }
        } else {
            ProjectileParams::spell_missile()
        }
    }

    /// Facing of an actor as a unit vector on the ground (yaw 0 looks along +x).
    fn forward_of(&self, id: ActorId) -> Vec3 {
        let yaw = self.world.facing(id);
        Vec3::new(yaw.cos(), 0.0, yaw.sin())
    }

    /// Build the launch of a spec: port of the aim logic of `Shoot` and `MagicProjectile`.
    pub(crate) fn make_launch(&self, spec: &ProjectileSpec) -> Option<Launch> {
        let owner = actor_of(spec.owner)?;
        let target = actor_of(spec.target);
        let params = self.params_for(spec);
        let (_, height) = body_of(self.actor(owner).species);
        let at = vec(self.world.position(owner)?);
        let forward = self.forward_of(owner);
        let chest = at + Vec3::Y * height * 0.8 + forward * 40.0;
        let owner_id = u64::from(owner.0);
        let target_id = target.map(|t| u64::from(t.0));
        if !spec.spell.is_empty() {
            let power = spec.path == gothic_script::magic::api::PathStyle::Missile;
            let mut l = launch::spell_missile(&params, owner_id, chest, forward, target_id, power);
            l.projectile.params.shoot_velocity = if spec.velocity > 0.0 {
                spec.velocity
            } else {
                self.script
                    .magic
                    .spells
                    .get(&spec.spell)
                    .map_or(params.shoot_velocity, |s| s.velocity)
            };
            return Some(l);
        }
        let hero_shot = spec.path == gothic_script::magic::api::PathStyle::Physics;
        if !hero_shot {
            return Some(launch::npc_arrow(
                &params, owner_id, chest, forward, target_id,
            ));
        }
        let draw = spec.hit_mult.max(0.0);
        let aim = self.script.magic.hero_aim;
        let (origin, pick) = match aim {
            Some(a) => (a.origin, a.pick_point),
            None => {
                // No camera: aim at the target's chest, with the arc a player would give.
                let goal = target
                    .and_then(|t| {
                        let (_, h) = body_of(self.actor(t).species);
                        self.world.position(t).map(|p| vec(p) + Vec3::Y * h * 0.7)
                    })
                    .unwrap_or(chest + forward * 3000.0);
                let kmh = draw * gothic_projectile::params::PLAYER_ARROW_KMH_PER_DRAW;
                let dir = launch::lob_direction(chest, goal, kmh).unwrap_or_else(|| {
                    (Vec3::new(goal.x - chest.x, 0.0, goal.z - chest.z).normalize_or_zero()
                        + Vec3::Y)
                        .normalize()
                });
                (chest, chest + dir * 1000.0)
            }
        };
        Some(launch::hero_arrow(&params, owner_id, origin, pick, draw))
    }

    /// Step every shot by one simulation tick and apply the hits.
    pub(crate) fn step_shots(&mut self) {
        let dt = self.cfg.tick_ms as f32 / 1000.0;
        let mut flights = std::mem::take(&mut self.script.magic.flights);
        let mut results: Vec<(usize, Vec<ShotEvent>)> = Vec::new();
        {
            let mut attached = std::collections::BTreeMap::new();
            for f in &flights {
                if let Phase::Attached { actor, .. } = f.shot.projectile.phase() {
                    *attached.entry(*actor).or_insert(0usize) += 1;
                }
            }
            let field = ShotField {
                sim: &*self,
                attached,
            };
            for (i, f) in flights.iter_mut().enumerate() {
                let events = f.shot.projectile.step(dt, &field);
                if !events.is_empty() {
                    results.push((i, events));
                }
            }
        }
        for (i, events) in results {
            let spec = flights[i].shot.spec.clone();
            let shot = flights[i].shot.id;
            for e in events {
                self.apply_shot_event(shot, &spec, &e);
            }
        }
        flights.retain(|f| !f.shot.projectile.is_gone());
        // Shots launched while the events were applied (none today) keep their place.
        flights.append(&mut self.script.magic.flights);
        self.script.magic.flights = flights;
    }

    fn apply_shot_event(&mut self, shot: u64, spec: &ProjectileSpec, e: &ShotEvent) {
        match e {
            ShotEvent::HitActor {
                actor, point, head, ..
            } => {
                let (Some(att), vic) = (actor_of(spec.owner), ActorId(*actor as u32)) else {
                    return;
                };
                self.push(EventKind::ProjectileImpact {
                    shot,
                    at: point.to_array(),
                    on_actor: Some(vic),
                    spell: !spec.spell.is_empty(),
                });
                self.land_projectile(spec.clone(), att, vic, *head);
            }
            ShotEvent::HitLevel { point, .. } => self.push(EventKind::ProjectileImpact {
                shot,
                at: point.to_array(),
                on_actor: None,
                spell: !spec.spell.is_empty(),
            }),
            ShotEvent::Ended => {
                self.push(EventKind::ProjectileEnded { shot });
            }
            ShotEvent::Embedded { .. } | ShotEvent::Bounced { .. } | ShotEvent::Attached { .. } => {
            }
        }
    }

    /// Spawn the shot of a spec at the shooter. Returns its id.
    pub(crate) fn launch_shot(&mut self, spec: ProjectileSpec) -> Option<u64> {
        let launch = self.make_launch(&spec)?;
        let id = self.script.magic.next_shot;
        self.script.magic.next_shot += 1;
        let mut projectile = launch.projectile;
        {
            let field = ShotField {
                sim: &*self,
                attached: std::collections::BTreeMap::new(),
            };
            projectile.shoot(launch.origin, &field);
        }
        let speed = speed_cm_s(projectile.params.shoot_velocity).max(1.0);
        let goal = actor_of(spec.target)
            .and_then(|t| self.world.position(t))
            .map_or(launch.origin, vec);
        let eta_ms = (launch.origin.distance(goal) / speed * 1000.0) as u64;
        let now = self.now_ms();
        self.script.magic.flights.push(crate::script_magic::Flight {
            spec: spec.clone(),
            arrives_ms: now + eta_ms,
            shot: Shot {
                id,
                projectile,
                spec,
            },
        });
        Some(id)
    }
}
