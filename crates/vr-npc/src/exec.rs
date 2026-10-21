//! Execution layer between the original game AI and the VR body (issue VR-12).
//!
//! The original AI (`gothic-sim` / `gothic-script`) decides **when** and **whom** to attack and
//! with which move ([`AiAction`], same numbering as `gothic_sim::types::Action`). The VR layer
//! decides **how**: every attack becomes a telegraphed physical move ([`VrMove`]), attacks
//! respect the global [`AttackSlots`], a blocked or parried attack recoils, and a landed
//! attack becomes an [`AiHitEvent`] for the original damage formula, which needs the move
//! (`move_strength`, quick attacks halved and so on).
//!
//! Two bodies:
//! * **humanoids** (humans, orcs): the [`CombatAi`] in commanded mode drives the active
//!   ragdoll and its physical weapon; contacts come from the melee model (`vr-melee`), and
//!   [`Executor::on_melee_hit`] / [`Executor::on_clash`] turn them into events;
//! * **quadrupeds** (wolves, boars, ...): a small kinematic body ([`QuadBody`]) whose mouth or
//!   paw sweeps a capsule; [`Executor::step`] resolves it against the player's hitzones and
//!   weapon itself (block = the weapon crosses the sweep first).
//!
//! Everything here is Bevy-free and deterministic.
use crate::ai::{AttackDir, AttackSlots, CombatAi, NpcIntent, Order, Perception, Phase};
use glam::{Quat, Vec3};
use gothic_vr_melee::geom::segment_segment;
use gothic_vr_melee::{Bone, DamageType, Hitzone};
use serde::{Deserialize, Serialize};
use std::f32::consts::PI;

/// The original AI's combat actions (values of `gothic_sim::types::Action`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum AiAction {
    None,
    Attack,
    PowerAttack,
    QuickAttack,
    QuickAttackR,
    QuickAttackL,
    SimpleWhirl,
    SprintAttack,
    WhirlAttack,
    PierceAttack,
    JumpAttack,
    RamAttack,
    HackAttack,
    FinishingAttack,
    Parade,
    GetUpAttack,
    /// Anything else (movement, waiting, ranged, magic): not executed here.
    Other(u8),
}

impl AiAction {
    /// From the numeric `gothic_sim::types::Action` value.
    pub fn from_sim(v: u8) -> Self {
        match v {
            0 => Self::None,
            1 => Self::Attack,
            2 => Self::PowerAttack,
            3 => Self::QuickAttack,
            4 => Self::QuickAttackR,
            5 => Self::QuickAttackL,
            6 => Self::SimpleWhirl,
            9 => Self::SprintAttack,
            10 => Self::WhirlAttack,
            11 => Self::PierceAttack,
            12 => Self::JumpAttack,
            13 => Self::RamAttack,
            14 => Self::HackAttack,
            15 => Self::FinishingAttack,
            16 => Self::Parade,
            30 => Self::GetUpAttack,
            other => Self::Other(other),
        }
    }

    pub fn is_attack(self) -> bool {
        !matches!(self, Self::None | Self::Parade | Self::Other(_))
    }
}

/// Body plan, from `Species::is_humanoid()`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum BodyKind {
    Humanoid,
    Quadruped,
}

/// The physical move that executes an action.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum VrMove {
    /// Humanoid weapon strike from a side (telegraphed on that side).
    Swing(AttackDir),
    /// Quadruped: quick snap at the target's body.
    Bite,
    /// Quadruped: crouch, then leap at the target's chest.
    Lunge,
    /// Quadruped: sideways paw / tusk sweep (true = from its right).
    Swipe(bool),
}

/// Which physical move executes `action` for `kind`. `alternate` flips the side of
/// side-less quick attacks so combos alternate like the original animations.
pub fn plan(kind: BodyKind, action: AiAction, alternate: bool) -> Option<VrMove> {
    use AiAction::*;
    let side = if alternate {
        AttackDir::FromLeft
    } else {
        AttackDir::FromRight
    };
    Some(match (kind, action) {
        (BodyKind::Humanoid, QuickAttackR) => VrMove::Swing(AttackDir::FromRight),
        (BodyKind::Humanoid, QuickAttackL) => VrMove::Swing(AttackDir::FromLeft),
        (BodyKind::Humanoid, Attack | QuickAttack | GetUpAttack) => VrMove::Swing(side),
        (BodyKind::Humanoid, SimpleWhirl | WhirlAttack) => VrMove::Swing(side),
        (BodyKind::Humanoid, PowerAttack | HackAttack | JumpAttack | FinishingAttack) => {
            VrMove::Swing(AttackDir::Overhead)
        }
        (BodyKind::Humanoid, PierceAttack | SprintAttack | RamAttack) => {
            VrMove::Swing(AttackDir::Thrust)
        }
        (BodyKind::Quadruped, Attack | QuickAttack | QuickAttackR | QuickAttackL) => VrMove::Bite,
        (BodyKind::Quadruped, GetUpAttack | FinishingAttack | HackAttack) => VrMove::Bite,
        (BodyKind::Quadruped, PowerAttack | JumpAttack | SprintAttack | RamAttack) => VrMove::Lunge,
        (BodyKind::Quadruped, PierceAttack) => VrMove::Lunge,
        (BodyKind::Quadruped, SimpleWhirl | WhirlAttack) => VrMove::Swipe(!alternate),
        (_, None | Parade | Other(_)) => return Option::None,
    })
}

/// One decision of the original AI for one NPC.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct AiIntent {
    /// Whom (an id the host maps to an entity); `None` = no target (stand down).
    pub target: Option<u64>,
    pub action: AiAction,
}

/// What the target looks like this tick (for quadruped contacts and blocks).
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct TargetBody {
    pub hitzones: Vec<Hitzone>,
    /// The target's weapon blade (start, end), if it holds one up.
    pub weapon: Option<(Vec3, Vec3)>,
}

/// A landed attack, for the original damage pipeline.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct AiHitEvent {
    pub attacker: u64,
    pub victim: u64,
    /// The original action that was executed (`move_strength`, quick halving, ...).
    pub action: AiAction,
    pub vr_move: VrMove,
    pub bone: Bone,
    pub damage_type: DamageType,
    pub point: Vec3,
    pub direction: Vec3,
    /// Impact speed of the weapon or jaws, m/s.
    pub impact_speed: f32,
}

/// What happened this tick.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub enum ExecEvent {
    /// The wind-up started; `lead` seconds until the blow is live.
    Telegraph {
        vr_move: VrMove,
        lead: f32,
    },
    /// The attack waits for an attack slot (others are fighting the target).
    WaitingForSlot,
    Hit(AiHitEvent),
    /// The target's weapon stopped the attack; the attacker recoils.
    Blocked,
    /// Standing back up after a knockdown.
    GettingUp,
}

/// Quadruped phases.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub enum QuadPhase {
    Ready,
    Telegraph(VrMove, f32),
    Strike(VrMove, f32),
    Recover(f32),
    Recoil(f32),
    /// Knocked down by a heavy hit; gets up after the time.
    Down(f32),
}

/// Kinematic four-legged body: feet centre, facing, and the attacking point (mouth or paw).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct QuadBody {
    pub root: Vec3,
    pub yaw: f32,
    pub phase: QuadPhase,
    /// Shoulder height, m (wolf about 0.8).
    pub height: f32,
    /// Ranges it likes to fight from.
    pub min_range: f32,
    pub max_range: f32,
    pub speed: f32,
    /// Attacking point last tick (for the swept contact).
    last_point: Vec3,
}

/// Quadruped timings (telegraph, strike, recover) per move, s.
pub fn quad_timing(m: VrMove) -> (f32, f32, f32) {
    match m {
        VrMove::Bite => (0.45, 0.2, 0.45),
        VrMove::Lunge => (0.8, 0.35, 0.7),
        VrMove::Swipe(_) => (0.55, 0.25, 0.5),
        VrMove::Swing(_) => (0.55, 0.3, 0.55),
    }
}

impl QuadBody {
    pub fn new(root: Vec3, yaw: f32) -> Self {
        let mut b = Self {
            root,
            yaw,
            phase: QuadPhase::Ready,
            height: 0.8,
            min_range: 1.3,
            max_range: 1.8,
            speed: 3.0,
            last_point: Vec3::ZERO,
        };
        b.last_point = b.attack_point();
        b
    }

    fn forward(&self) -> Vec3 {
        Quat::from_rotation_y(self.yaw) * Vec3::NEG_Z
    }

    fn right(&self) -> Vec3 {
        Quat::from_rotation_y(self.yaw) * Vec3::X
    }

    /// Mouth (bite, lunge) or paw (swipe) position for the current phase.
    pub fn attack_point(&self) -> Vec3 {
        let f = self.forward();
        let head = self.root + Vec3::Y * self.height + f * 0.55;
        let ease = |k: f32| {
            let k = k.clamp(0.0, 1.0);
            k * k * (3.0 - 2.0 * k)
        };
        match self.phase {
            QuadPhase::Telegraph(VrMove::Lunge, t) => {
                // Crouch: head down and back.
                let k = ease(t / quad_timing(VrMove::Lunge).0);
                head - Vec3::Y * 0.3 * k - f * 0.25 * k
            }
            QuadPhase::Telegraph(VrMove::Bite, t) => {
                head - f * 0.2 * ease(t / quad_timing(VrMove::Bite).0)
            }
            QuadPhase::Telegraph(VrMove::Swipe(r), t) => {
                let side = if r { self.right() } else { -self.right() };
                head + side * 0.45 * ease(t / quad_timing(VrMove::Swipe(r)).0)
            }
            // Strikes start where the wind-up ended (no jump in the swept contact).
            QuadPhase::Strike(VrMove::Bite, t) => {
                let k = ease(t / quad_timing(VrMove::Bite).1);
                head + f * (0.55 * k - 0.2 * (1.0 - k))
            }
            QuadPhase::Strike(VrMove::Lunge, t) => {
                // Leap: up to chest height of a standing human.
                let k = ease(t / quad_timing(VrMove::Lunge).1);
                head + f * (0.5 * k - 0.25 * (1.0 - k))
                    + Vec3::Y * (0.6 * (k * PI).sin() + 0.3 * k - 0.3 * (1.0 - k))
            }
            QuadPhase::Strike(VrMove::Swipe(r), t) => {
                let k = ease(t / quad_timing(VrMove::Swipe(r)).1);
                let side = if r { self.right() } else { -self.right() };
                head + side * 0.45 * (1.0 - 2.0 * k) + f * 0.35 * k
            }
            _ => head,
        }
    }
}

/// One NPC's executor.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum Body {
    Humanoid(CombatAi),
    Quadruped(QuadBody),
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Executor {
    pub id: u64,
    pub body: Body,
    /// The intent being executed.
    pub intent: Option<AiIntent>,
    /// The move of the attack in flight and the action it executes.
    pub current: Option<(AiAction, VrMove)>,
    /// Whom the attack in flight is aimed at.
    victim: u64,
    alternate: bool,
    /// The move in flight already landed (one hit per attack).
    landed: bool,
    waiting: bool,
}

/// Knockdown time for quadrupeds hit hard, s.
const QUAD_DOWN: f32 = 1.6;
/// Recoil after a blocked quadruped attack, s.
const QUAD_RECOIL: f32 = 0.6;

impl Executor {
    pub fn humanoid(id: u64, root: Vec3, yaw: f32) -> Self {
        let mut ai = CombatAi::new(root, yaw, id);
        ai.autonomous = false;
        Self::with(id, Body::Humanoid(ai))
    }

    pub fn quadruped(id: u64, root: Vec3, yaw: f32) -> Self {
        Self::with(id, Body::Quadruped(QuadBody::new(root, yaw)))
    }

    fn with(id: u64, body: Body) -> Self {
        Self {
            id,
            body,
            intent: None,
            current: None,
            victim: 0,
            alternate: false,
            landed: false,
            waiting: false,
        }
    }

    pub fn kind(&self) -> BodyKind {
        match self.body {
            Body::Humanoid(_) => BodyKind::Humanoid,
            Body::Quadruped(_) => BodyKind::Quadruped,
        }
    }

    /// A new decision from the original AI (replaces the previous one; an attack already
    /// swinging finishes first).
    pub fn command(&mut self, intent: AiIntent) {
        self.intent = Some(intent);
        if let Body::Humanoid(ai) = &mut self.body {
            ai.order = match plan(BodyKind::Humanoid, intent.action, self.alternate) {
                Some(VrMove::Swing(d)) if intent.target.is_some() => Some(Order::Attack(d)),
                _ if intent.action == AiAction::Parade => Some(Order::Guard),
                _ => None,
            };
        }
    }

    /// Whether an attack is between telegraph and recovery.
    pub fn attacking(&self) -> bool {
        match &self.body {
            Body::Humanoid(ai) => matches!(
                ai.phase,
                Phase::Telegraph(..) | Phase::Strike(..) | Phase::Recover(..)
            ),
            Body::Quadruped(q) => matches!(
                q.phase,
                QuadPhase::Telegraph(..) | QuadPhase::Strike(..) | QuadPhase::Recover(_)
            ),
        }
    }

    /// One tick. `target_pos` is the target's feet, `body` its hitzones and guard (used for
    /// quadruped contacts; humanoid contacts come from the melee model). Returns the humanoid
    /// body intent (for the ragdoll) and the events.
    pub fn step(
        &mut self,
        per: &Perception,
        target: &TargetBody,
        slots: &mut AttackSlots,
        dt: f32,
    ) -> (Option<NpcIntent>, Vec<ExecEvent>) {
        let mut events = Vec::new();
        let dt = if dt.is_finite() {
            dt.clamp(0.0, 0.1)
        } else {
            0.0
        };
        match &mut self.body {
            Body::Humanoid(ai) => {
                let before = ai.phase;
                let ordered = ai.order;
                let out = ai.update(self.id, per, slots, dt);
                if let (Phase::Telegraph(d, t), false) =
                    (out.phase, matches!(before, Phase::Telegraph(..)))
                {
                    let action = self.intent.map_or(AiAction::Attack, |i| i.action);
                    self.current = Some((action, VrMove::Swing(d)));
                    self.landed = false;
                    self.alternate = !self.alternate;
                    events.push(ExecEvent::Telegraph {
                        vr_move: VrMove::Swing(d),
                        lead: ai.settings.telegraph_time - t,
                    });
                } else if matches!(ordered, Some(Order::Attack(_)))
                    && ai.order.is_some()
                    && out.phase == Phase::Ready
                    && !slots.holders().contains(&self.id)
                    && slots.holders().len() >= slots.max
                {
                    if !self.waiting {
                        events.push(ExecEvent::WaitingForSlot);
                    }
                    self.waiting = true;
                } else {
                    self.waiting = false;
                }
                if out.phase == Phase::Ready && !matches!(before, Phase::Ready) {
                    self.current = None;
                }
                (Some(out), events)
            }
            Body::Quadruped(_) => {
                self.step_quad_inner(per, target, slots, dt, &mut events);
                (None, events)
            }
        }
    }

    fn step_quad_inner(
        &mut self,
        per: &Perception,
        target: &TargetBody,
        slots: &mut AttackSlots,
        dt: f32,
        events: &mut Vec<ExecEvent>,
    ) {
        let id = self.id;
        let intent = self.intent;
        let Body::Quadruped(q) = &mut self.body else {
            return;
        };
        q.last_point = q.attack_point();
        // Face the target.
        let to = per.player - q.root;
        let flat = Vec3::new(to.x, 0.0, to.z);
        let dist = flat.length();
        if dist > 1e-3 && !matches!(q.phase, QuadPhase::Strike(..) | QuadPhase::Down(_)) {
            q.yaw = (-flat.x).atan2(-flat.z);
        }
        let dir = if dist > 1e-3 {
            flat / dist
        } else {
            Vec3::NEG_Z
        };
        q.phase = match q.phase {
            QuadPhase::Down(t) if t - dt > 0.0 => QuadPhase::Down(t - dt),
            QuadPhase::Down(_) => {
                events.push(ExecEvent::GettingUp);
                QuadPhase::Recover(0.3)
            }
            QuadPhase::Recoil(t) if t - dt > 0.0 => QuadPhase::Recoil(t - dt),
            QuadPhase::Recoil(_) => {
                slots.release(id);
                self.current = None;
                QuadPhase::Ready
            }
            QuadPhase::Telegraph(m, t) if t + dt < quad_timing(m).0 => {
                QuadPhase::Telegraph(m, t + dt)
            }
            QuadPhase::Telegraph(m, _) => QuadPhase::Strike(m, 0.0),
            QuadPhase::Strike(m, t) if t + dt < quad_timing(m).1 => QuadPhase::Strike(m, t + dt),
            QuadPhase::Strike(m, _) => QuadPhase::Recover(quad_timing(m).2),
            QuadPhase::Recover(t) if t - dt > 0.0 => QuadPhase::Recover(t - dt),
            QuadPhase::Recover(_) => {
                slots.release(id);
                self.current = None;
                QuadPhase::Ready
            }
            QuadPhase::Ready => {
                let wanted = intent.filter(|i| i.target.is_some()).and_then(|i| {
                    plan(BodyKind::Quadruped, i.action, self.alternate).map(|m| (i.action, m))
                });
                match wanted {
                    Some((action, m)) if dist <= q.max_range && dist >= q.min_range - 0.4 => {
                        if slots.request(id) {
                            self.waiting = false;
                            self.current = Some((action, m));
                            self.victim = intent.and_then(|i| i.target).unwrap_or(0);
                            self.landed = false;
                            self.alternate = !self.alternate;
                            self.intent = None;
                            events.push(ExecEvent::Telegraph {
                                vr_move: m,
                                lead: quad_timing(m).0,
                            });
                            QuadPhase::Telegraph(m, 0.0)
                        } else {
                            if !self.waiting {
                                events.push(ExecEvent::WaitingForSlot);
                            }
                            self.waiting = true;
                            QuadPhase::Ready
                        }
                    }
                    _ => QuadPhase::Ready,
                }
            }
        };
        // Footwork: close in or keep distance while ready; strikes carry the body.
        let step = match q.phase {
            QuadPhase::Ready if dist > q.max_range => dir * q.speed,
            QuadPhase::Ready if dist < q.min_range => -dir * q.speed * 0.5,
            QuadPhase::Strike(VrMove::Lunge, _) => dir * q.speed * 1.2,
            // Bites and swipes snap forward from the fighting distance.
            QuadPhase::Strike(VrMove::Bite | VrMove::Swipe(_), _) if dist > 0.6 => {
                dir * q.speed * 0.9
            }
            QuadPhase::Recoil(_) => -dir * q.speed * 0.4,
            _ => Vec3::ZERO,
        };
        if step.is_finite() {
            q.root += step * dt;
        }
        // Contact while striking: the sweep from last to this tick's attack point.
        if let (QuadPhase::Strike(m, _), Some((action, _)), false) =
            (q.phase, self.current, self.landed)
        {
            let (a, b) = (q.last_point, q.attack_point());
            let jaw = 0.08;
            let blocked = target.weapon.is_some_and(|(w0, w1)| {
                let (p, r, _, _) = segment_segment(a, b, w0, w1);
                p.distance(r) < jaw + 0.03
            });
            if blocked {
                self.landed = true;
                q.phase = QuadPhase::Recoil(QUAD_RECOIL);
                events.push(ExecEvent::Blocked);
            } else if let Some(z) = target.hitzones.iter().find(|z| {
                let (s0, s1) = z.segment();
                let (p, r, _, _) = segment_segment(a, b, s0, s1);
                p.distance(r) < z.radius + jaw
            }) {
                self.landed = true;
                let speed = if dt > 0.0 { a.distance(b) / dt } else { 0.0 };
                events.push(ExecEvent::Hit(AiHitEvent {
                    attacker: id,
                    victim: self.victim,
                    action,
                    vr_move: m,
                    bone: z.bone,
                    damage_type: match m {
                        VrMove::Bite | VrMove::Lunge => DamageType::Point,
                        _ => DamageType::Edge,
                    },
                    point: b,
                    direction: (b - a).normalize_or_zero(),
                    impact_speed: speed,
                }));
            }
        }
    }

    /// The melee model reported a hit by this humanoid's weapon on `victim`.
    pub fn on_melee_hit(
        &mut self,
        victim: u64,
        bone: Bone,
        damage_type: DamageType,
        point: Vec3,
        direction: Vec3,
        impact_speed: f32,
    ) -> Option<AiHitEvent> {
        let (action, vr_move) = self.current?;
        if self.landed {
            return None;
        }
        self.landed = true;
        Some(AiHitEvent {
            attacker: self.id,
            victim,
            action,
            vr_move,
            bone,
            damage_type,
            point,
            direction,
            impact_speed,
        })
    }

    /// The melee model reported this NPC's attack hit a guard (`recoil` seconds).
    pub fn on_clash(&mut self, recoil: f32) -> Option<ExecEvent> {
        // Follow-through after a landed hit is not a block.
        if !self.attacking() || self.landed {
            return None;
        }
        self.landed = true;
        match &mut self.body {
            Body::Humanoid(ai) => ai.recoil(recoil),
            Body::Quadruped(q) => q.phase = QuadPhase::Recoil(recoil.max(QUAD_RECOIL)),
        }
        Some(ExecEvent::Blocked)
    }

    /// The NPC was hit hard (`knockdown`) or lightly. Humanoids react through the active
    /// ragdoll; quadrupeds fall down and get up.
    pub fn on_hit_taken(&mut self, knockdown: bool, slots: &mut AttackSlots) {
        if let Body::Quadruped(q) = &mut self.body
            && knockdown
        {
            q.phase = QuadPhase::Down(QUAD_DOWN);
            self.current = None;
            slots.release(self.id);
        }
    }
}
