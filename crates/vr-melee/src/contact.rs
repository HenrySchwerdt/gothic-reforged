//! Bevy-free melee contact model.
//!
//! Each physics step [`MeleeWorld::step`] receives every weapon's pose at the start and the
//! end of the step and every body's hitzone capsules, and answers with [`MeleeEvent`]s:
//!
//! * **Swept detection**: the step is cut into sub-steps short enough that no striker can
//!   jump over a hitzone (fast swings cannot tunnel); the earliest contact counts.
//! * **Velocity-scaled damage**: the impact speed is the relative velocity of the striking
//!   point. Below a striker's minimum speed a contact is just a touch, so slow pushes and
//!   waggling do not hurt; a hit is only registered when a striker *enters* a hitzone, with a
//!   short per-victim cooldown.
//! * **Edge alignment**: a cut needs the edge to lead (`|v . edge_normal|`); a flat hit becomes
//!   blunt damage.
//! * **Stabs**: a point moving along its axis fast enough penetrates and sticks
//!   ([`MeleeEvent::Stuck`]); while stuck the weapon cannot hit, and pulling back along the
//!   wound axis (or ripping it out sideways) frees it ([`MeleeEvent::PulledOut`]).
//! * **Weapon vs weapon**: blades of different wielders collide. A nearly still weapon in the
//!   way **blocks** (the attacker recoils), a weapon swung against the attack within the
//!   timing window **parries** (longer stagger). An attack that was blocked earlier in the same
//!   step does not also hit the body behind the block.
//!
//! The output [`HitEvent`] carries the inputs of Gothic's damage formula (damage type, impact
//! speed, bone); the formula itself is applied by the maintainer's integration layer.
use crate::geom::capsule_overlap;
use glam::{Quat, Vec3};
use gothic_vr::Pose;
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};

/// Gothic's three physical damage types.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum DamageType {
    /// Cutting edge (`Edge` in Gothic 3).
    Edge,
    /// Blunt impact (`Blunt`).
    Blunt,
    /// Piercing point (`Point`).
    Point,
}

/// Generic humanoid bones carrying hitzones (mapped to Gothic skeletons later).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Bone {
    Head,
    Neck,
    Chest,
    Abdomen,
    Pelvis,
    UpperArmL,
    UpperArmR,
    ForearmL,
    ForearmR,
    HandL,
    HandR,
    ThighL,
    ThighR,
    ShinL,
    ShinR,
    FootL,
    FootR,
}

/// A body's capsule hitzone: `half_length` along the local Y of `pose`.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Hitzone {
    pub bone: Bone,
    pub pose: Pose,
    pub half_length: f32,
    pub radius: f32,
}

impl Hitzone {
    pub fn segment(&self) -> (Vec3, Vec3) {
        (
            self.pose
                .transform_point(Vec3::new(0.0, -self.half_length, 0.0)),
            self.pose
                .transform_point(Vec3::new(0.0, self.half_length, 0.0)),
        )
    }

    /// Capsule between two points.
    pub fn between(bone: Bone, a: Vec3, b: Vec3, radius: f32) -> Self {
        let d = b - a;
        let len = d.length();
        let rotation = if len > 1e-6 {
            Quat::from_rotation_arc(Vec3::Y, d / len)
        } else {
            Quat::IDENTITY
        };
        Self {
            bone,
            pose: Pose::new((a + b) / 2.0, rotation),
            half_length: len / 2.0,
            radius,
        }
    }

    fn lerp(&self, to: &Hitzone, t: f32) -> Hitzone {
        Hitzone {
            bone: self.bone,
            pose: Pose::new(
                self.pose.position.lerp(to.pose.position, t),
                self.pose.rotation.slerp(to.pose.rotation, t),
            ),
            half_length: self.half_length + (to.half_length - self.half_length) * t,
            radius: self.radius + (to.radius - self.radius) * t,
        }
    }
}

/// What part of a weapon strikes and how.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub enum StrikerKind {
    /// A cutting edge along `a..b` facing `normal` (weapon frame). `double` edges cut both ways.
    Edge { normal: Vec3, double: bool },
    /// A blunt surface along `a..b`.
    Blunt,
    /// A point at `b`, stabbing along `a -> b`.
    Point,
}

/// A striking feature in weapon coordinates.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Striker {
    pub kind: StrikerKind,
    pub a: Vec3,
    pub b: Vec3,
    pub radius: f32,
    /// Below this impact speed (m/s) a contact does no damage.
    pub min_speed: f32,
    /// At this speed the hit counts fully (`HitEvent::quality` = 1).
    pub full_speed: f32,
}

/// Melee description of a weapon.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MeleeProfile {
    pub strikers: Vec<Striker>,
    /// The whole weapon as a capsule (weapon-vs-weapon contacts): `a..b`, radius.
    pub guard: (Vec3, Vec3, f32),
    /// How deep a stab may go, m.
    pub max_penetration: f32,
}

/// One weapon's motion over the step.
#[derive(Debug, Clone, Copy)]
pub struct WeaponMotion<'a> {
    pub id: u64,
    pub wielder: Option<u64>,
    pub prev: Pose,
    pub cur: Pose,
    pub profile: &'a MeleeProfile,
}

/// One body's hitzones at the start and end of the step (same order and length).
#[derive(Debug, Clone, Copy)]
pub struct BodyMotion<'a> {
    pub id: u64,
    pub prev: &'a [Hitzone],
    pub cur: &'a [Hitzone],
}

/// The damage-relevant output of a contact.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct HitEvent {
    /// Wielder of the weapon (`None` for a thrown or loose weapon).
    pub attacker: Option<u64>,
    pub weapon: u64,
    pub victim: u64,
    pub bone: Bone,
    pub damage_type: DamageType,
    /// Relative speed of the striking point, m/s.
    pub impact_speed: f32,
    /// World contact point.
    pub point: Vec3,
    /// Direction of the strike (unit relative velocity).
    pub direction: Vec3,
    /// 0..1: how clean the hit was (speed between min and full, edge alignment).
    pub quality: f32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ClashKind {
    /// The defender held its weapon in the way.
    Block,
    /// The defender swung into the attack.
    Parry,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub enum MeleeEvent {
    Hit(HitEvent),
    /// Two weapons met. `attacker_impulse` is the velocity change (m/s) to apply to the
    /// attacking weapon at `point` to deflect it.
    Clash {
        kind: ClashKind,
        attacker_weapon: u64,
        defender_weapon: u64,
        attacker: Option<u64>,
        defender: Option<u64>,
        point: Vec3,
        attacker_impulse: Vec3,
        /// Seconds the attacker's weapon cannot deal damage (recoil / stagger).
        recoil: f32,
    },
    /// A stab penetrated and the weapon now sticks in the body.
    Stuck {
        weapon: u64,
        victim: u64,
        bone: Bone,
        /// Entry point and wound axis (world).
        entry: Vec3,
        axis: Vec3,
    },
    /// A stuck weapon came out.
    PulledOut {
        weapon: u64,
        victim: u64,
    },
}

/// Thresholds of the model.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct MeleeSettings {
    /// Edge cuts need `|v . edge_normal|` at least this (else they count as flat/blunt).
    pub edge_alignment: f32,
    /// A flat hit deals blunt damage scaled by this.
    pub flat_quality: f32,
    /// Stabs need `v . axis` at least this fraction of the speed.
    pub stab_alignment: f32,
    /// Same weapon cannot hit the same victim again for this long, s.
    pub rehit_cooldown: f32,
    /// Weapons meeting slower than this (m/s) just touch.
    pub clash_min_speed: f32,
    /// A defender moving into the attack at least this fast parries.
    pub parry_speed: f32,
    pub block_recoil: f32,
    pub parry_recoil: f32,
    /// Fraction of the attacker's closing speed removed (and reflected) on a clash.
    pub deflect: f32,
    /// Pulled sideways further than this (m) from the wound axis, a stuck blade rips out.
    pub rip_distance: f32,
}

impl Default for MeleeSettings {
    fn default() -> Self {
        Self {
            edge_alignment: 0.5,
            flat_quality: 0.35,
            stab_alignment: 0.75,
            rehit_cooldown: 0.25,
            clash_min_speed: 1.0,
            parry_speed: 2.0,
            block_recoil: 0.35,
            parry_recoil: 0.8,
            deflect: 1.3,
            rip_distance: 0.12,
        }
    }
}

/// A weapon stuck in a body, in the hitzone's frame.
#[derive(Debug, Clone, Copy, PartialEq)]
struct Embed {
    victim: u64,
    zone: usize,
    entry: Vec3,
    axis: Vec3,
    tip: Vec3,
}

/// Persistent contact state across steps.
#[derive(Debug, Default, Clone)]
pub struct MeleeWorld {
    pub settings: MeleeSettings,
    /// (weapon, victim, striker, zone) pairs overlapping at the end of the last step.
    touching: HashSet<(u64, u64, usize, usize)>,
    /// (weapon, weapon) pairs overlapping at the end of the last step.
    clashing: HashSet<(u64, u64)>,
    /// (weapon, victim) -> seconds until it may hit again.
    cooldown: HashMap<(u64, u64), f32>,
    /// weapon -> seconds of recoil left.
    recoil: HashMap<u64, f32>,
    embeds: HashMap<u64, Embed>,
}

const MAX_SUBSTEPS: usize = 48;

fn lerp_pose(a: &Pose, b: &Pose, t: f32) -> Pose {
    Pose::new(
        a.position.lerp(b.position, t),
        a.rotation.slerp(b.rotation, t),
    )
}

impl MeleeWorld {
    pub fn new(settings: MeleeSettings) -> Self {
        Self {
            settings,
            ..Default::default()
        }
    }

    /// Is this weapon stuck in a body?
    pub fn stuck_in(&self, weapon: u64) -> Option<u64> {
        self.embeds.get(&weapon).map(|e| e.victim)
    }

    /// Seconds of recoil left for a weapon.
    pub fn recoil_left(&self, weapon: u64) -> f32 {
        self.recoil.get(&weapon).copied().unwrap_or(0.0)
    }

    /// Forget a weapon (despawned, or its stuck state was resolved externally).
    pub fn forget(&mut self, weapon: u64) {
        self.embeds.remove(&weapon);
        self.recoil.remove(&weapon);
        self.touching.retain(|k| k.0 != weapon);
        self.clashing.retain(|k| k.0 != weapon && k.1 != weapon);
        self.cooldown.retain(|k, _| k.0 != weapon);
    }

    /// Advance one physics step of `dt` seconds.
    pub fn step(
        &mut self,
        weapons: &[WeaponMotion],
        bodies: &[BodyMotion],
        dt: f32,
    ) -> Vec<MeleeEvent> {
        let mut events = Vec::new();
        if !(dt > 0.0 && dt.is_finite()) {
            return events;
        }
        for v in self.cooldown.values_mut().chain(self.recoil.values_mut()) {
            *v -= dt;
        }
        self.cooldown.retain(|_, v| *v > 0.0);
        self.recoil.retain(|_, v| *v > 0.0);

        // Sub-step count from the fastest point of any weapon relative to its thinnest feature.
        let mut n = 1usize;
        for w in weapons {
            let thin = w
                .profile
                .strikers
                .iter()
                .map(|s| s.radius)
                .fold(w.profile.guard.2, f32::min)
                .max(0.005);
            let (ga, gb, _) = w.profile.guard;
            let travel = [ga, gb]
                .iter()
                .chain(w.profile.strikers.iter().flat_map(|s| [&s.a, &s.b]))
                .map(|p| {
                    w.prev
                        .transform_point(*p)
                        .distance(w.cur.transform_point(*p))
                })
                .fold(0.0, f32::max);
            if travel.is_finite() {
                n = n.max((travel / thin).ceil() as usize);
            }
        }
        let n = n.clamp(1, MAX_SUBSTEPS);

        let clash_time = self.weapon_clashes(weapons, n, dt, &mut events);
        self.stuck_weapons(weapons, bodies, &mut events);
        self.body_hits(weapons, bodies, n, dt, &clash_time, &mut events);
        events
    }

    fn weapon_clashes(
        &mut self,
        weapons: &[WeaponMotion],
        n: usize,
        dt: f32,
        events: &mut Vec<MeleeEvent>,
    ) -> HashMap<u64, f32> {
        let s = self.settings;
        let mut first: HashMap<u64, f32> = HashMap::new();
        let mut touching = HashSet::new();
        for (i, a) in weapons.iter().enumerate() {
            for b in &weapons[i + 1..] {
                if a.wielder.is_some() && a.wielder == b.wielder {
                    continue;
                }
                let key = (a.id.min(b.id), a.id.max(b.id));
                let mut hit = None;
                let mut end_overlap = false;
                for k in 1..=n {
                    let t = k as f32 / n as f32;
                    let pa = lerp_pose(&a.prev, &a.cur, t);
                    let pb = lerp_pose(&b.prev, &b.cur, t);
                    let (a0, a1, ra) = a.profile.guard;
                    let (b0, b1, rb) = b.profile.guard;
                    if let Some((point, normal, _)) = capsule_overlap(
                        pa.transform_point(a0),
                        pa.transform_point(a1),
                        ra,
                        pb.transform_point(b0),
                        pb.transform_point(b1),
                        rb,
                    ) {
                        if hit.is_none() {
                            hit = Some((t, point, normal));
                        }
                        if k == n {
                            end_overlap = true;
                        }
                    }
                }
                if end_overlap {
                    touching.insert(key);
                }
                let Some((t, point, normal)) = hit else {
                    continue;
                };
                if self.clashing.contains(&key) {
                    continue;
                }
                let vel = |w: &WeaponMotion| {
                    let at = lerp_pose(&w.prev, &w.cur, t);
                    let local = at.inverse().transform_point(point);
                    (w.cur.transform_point(local) - w.prev.transform_point(local)) / dt
                };
                let (va, vb) = (vel(a), vel(b));
                if va.length().max(vb.length()) < s.clash_min_speed {
                    continue;
                }
                // `normal` points from b toward a.
                let (att, def, v_att, v_def, n_att) = if va.length() >= vb.length() {
                    (a, b, va, vb, normal)
                } else {
                    (b, a, vb, va, -normal)
                };
                // The defender moves into the attacker (against n_att, i.e. toward it).
                let into = v_def.dot(n_att);
                let kind = if into >= s.parry_speed {
                    ClashKind::Parry
                } else {
                    ClashKind::Block
                };
                let closing = (-v_att.dot(n_att)).max(0.0);
                let recoil = match kind {
                    ClashKind::Block => s.block_recoil,
                    ClashKind::Parry => s.parry_recoil,
                };
                self.recoil.insert(att.id, recoil);
                first
                    .entry(att.id)
                    .and_modify(|x| *x = x.min(t))
                    .or_insert(t);
                events.push(MeleeEvent::Clash {
                    kind,
                    attacker_weapon: att.id,
                    defender_weapon: def.id,
                    attacker: att.wielder,
                    defender: def.wielder,
                    point,
                    attacker_impulse: n_att * (closing * s.deflect + into.max(0.0)),
                    recoil,
                });
            }
        }
        self.clashing = touching;
        first
    }

    fn stuck_weapons(
        &mut self,
        weapons: &[WeaponMotion],
        bodies: &[BodyMotion],
        events: &mut Vec<MeleeEvent>,
    ) {
        let s = self.settings;
        let mut freed = Vec::new();
        for w in weapons {
            let Some(e) = self.embeds.get(&w.id) else {
                continue;
            };
            let zone = bodies
                .iter()
                .find(|b| b.id == e.victim)
                .and_then(|b| b.cur.get(e.zone));
            let Some(zone) = zone else {
                freed.push((w.id, e.victim));
                continue;
            };
            let entry = zone.pose.transform_point(e.entry);
            let axis = zone.pose.rotation * e.axis;
            let tip = w.cur.transform_point(e.tip);
            let depth = (tip - entry).dot(axis);
            let lateral = ((tip - entry) - axis * depth).length();
            if depth < 0.005 || lateral > s.rip_distance {
                freed.push((w.id, e.victim));
            }
        }
        for (weapon, victim) in freed {
            self.embeds.remove(&weapon);
            self.cooldown
                .insert((weapon, victim), self.settings.rehit_cooldown);
            events.push(MeleeEvent::PulledOut { weapon, victim });
        }
    }

    fn body_hits(
        &mut self,
        weapons: &[WeaponMotion],
        bodies: &[BodyMotion],
        n: usize,
        dt: f32,
        clash_time: &HashMap<u64, f32>,
        events: &mut Vec<MeleeEvent>,
    ) {
        let s = self.settings;
        let mut touching = HashSet::new();
        for w in weapons {
            let stuck = self.embeds.contains_key(&w.id);
            for body in bodies {
                if Some(body.id) == w.wielder || body.prev.len() != body.cur.len() {
                    continue;
                }
                // Earliest entering contact of any striker with any zone of this body.
                let mut best: Option<(f32, usize, usize, Vec3, Vec3)> = None;
                for (si, st) in w.profile.strikers.iter().enumerate() {
                    for zi in 0..body.cur.len() {
                        let mut first = None;
                        for k in 1..=n {
                            let t = k as f32 / n as f32;
                            let p = lerp_pose(&w.prev, &w.cur, t);
                            let z = body.prev[zi].lerp(&body.cur[zi], t);
                            let (z0, z1) = z.segment();
                            if let Some((point, normal, _)) = capsule_overlap(
                                p.transform_point(st.a),
                                p.transform_point(st.b),
                                st.radius,
                                z0,
                                z1,
                                z.radius,
                            ) {
                                if first.is_none() {
                                    first = Some((t, point, normal));
                                }
                                if k == n {
                                    touching.insert((w.id, body.id, si, zi));
                                }
                            }
                        }
                        let Some((t, point, normal)) = first else {
                            continue;
                        };
                        if self.touching.contains(&(w.id, body.id, si, zi)) {
                            continue;
                        }
                        if best.as_ref().is_none_or(|b| t < b.0) {
                            best = Some((t, si, zi, point, normal));
                        }
                    }
                }
                let Some((t, si, zi, point, _normal)) = best else {
                    continue;
                };
                if stuck
                    || self.cooldown.contains_key(&(w.id, body.id))
                    || self.recoil.contains_key(&w.id)
                    || clash_time.get(&w.id).is_some_and(|ct| *ct <= t)
                {
                    continue;
                }
                let st = &w.profile.strikers[si];
                let at = lerp_pose(&w.prev, &w.cur, t);
                let local = at.inverse().transform_point(point);
                let v_weapon = (w.cur.transform_point(local) - w.prev.transform_point(local)) / dt;
                let zp = &body.prev[zi];
                let zc = &body.cur[zi];
                let v_zone = (zc.pose.position - zp.pose.position) / dt;
                let v = v_weapon - v_zone;
                let speed = v.length();
                if speed.is_nan() || speed < st.min_speed {
                    continue;
                }
                let dir = v / speed;
                let speed_q = ((speed - st.min_speed) / (st.full_speed - st.min_speed).max(1e-3))
                    .clamp(0.0, 1.0);
                let rot = at.rotation;
                let (damage_type, quality, stab) = match st.kind {
                    StrikerKind::Edge { normal, double } => {
                        let a = dir.dot(rot * normal);
                        let a = if double { a.abs() } else { a };
                        if a >= s.edge_alignment {
                            (DamageType::Edge, speed_q * a, false)
                        } else {
                            (DamageType::Blunt, speed_q * s.flat_quality, false)
                        }
                    }
                    StrikerKind::Blunt => (DamageType::Blunt, speed_q, false),
                    StrikerKind::Point => {
                        let axis = (rot * (st.b - st.a)).normalize_or_zero();
                        let along = dir.dot(axis);
                        let tip = at.transform_point(st.b);
                        // Only the tip stabs; a point striker touched elsewhere is ignored.
                        if along >= s.stab_alignment && tip.distance(point) <= st.radius + zc.radius
                        {
                            (DamageType::Point, speed_q * along, true)
                        } else {
                            continue;
                        }
                    }
                };
                self.cooldown.insert((w.id, body.id), s.rehit_cooldown);
                events.push(MeleeEvent::Hit(HitEvent {
                    attacker: w.wielder,
                    weapon: w.id,
                    victim: body.id,
                    bone: zc.bone,
                    damage_type,
                    impact_speed: speed,
                    point,
                    direction: dir,
                    quality,
                }));
                if stab {
                    let axis = (w.cur.rotation * (st.b - st.a)).normalize_or_zero();
                    let inv = zc.pose.inverse();
                    self.embeds.insert(
                        w.id,
                        Embed {
                            victim: body.id,
                            zone: zi,
                            entry: inv.transform_point(point),
                            axis: inv.rotation * axis,
                            tip: st.b,
                        },
                    );
                    events.push(MeleeEvent::Stuck {
                        weapon: w.id,
                        victim: body.id,
                        bone: zc.bone,
                        entry: point,
                        axis,
                    });
                }
            }
        }
        self.touching = touching;
    }
}
