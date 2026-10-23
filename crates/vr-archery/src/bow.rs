//! Bevy-free two-handed archery: take an arrow from the quiver over the shoulder, nock it on
//! the string, draw (draw length = power), release.
//!
//! Bow frame (see `gothic_vr_physics::props::bow`): stave along Y, shoots toward -Z, the
//! string's nocking point rests at [`BowSpec::string_rest`] behind the grip.
use crate::ballistics::ArrowSpec;
use glam::{Quat, Vec3};
use gothic_vr::{Hand, Pose};
use serde::{Deserialize, Serialize};

/// Bow parameters.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct BowSpec {
    /// Nocking point at rest (bow frame).
    pub string_rest: Vec3,
    /// Full draw, m.
    pub max_draw: f32,
    /// Below this draw a release just drops the arrow, m.
    pub min_draw: f32,
    /// Arrow speed at full draw, m/s.
    pub max_speed: f32,
    /// Arrow speed at the smallest draw that still shoots, m/s (a weak shot still flies).
    pub min_speed: f32,
    /// The bow's own damage value (Gothic item data, supplied locally).
    pub damage: f32,
    /// A drawing hand further than this from the bow's shooting axis loses the arrow, m. The
    /// arrow itself always lies on the line from the drawing hand through the arrow rest
    /// (Valve's Longbow aims the same way), so this is only a safety net for a hand that left
    /// the bow entirely.
    pub max_off_axis: f32,
}

impl Default for BowSpec {
    fn default() -> Self {
        Self {
            string_rest: gothic_vr_physics::props::bow_string_rest(),
            max_draw: 0.6,
            min_draw: 0.08,
            max_speed: 58.0,
            min_speed: 5.0,
            damage: 20.0,
            max_off_axis: 0.35,
        }
    }
}

impl BowSpec {
    /// 0..1 power from a draw length (linear: a bow stores energy ~ draw^2, so the arrow
    /// speed grows ~ draw).
    pub fn draw_factor(&self, draw: f32) -> f32 {
        if !draw.is_finite() {
            return 0.0;
        }
        (draw / self.max_draw).clamp(0.0, 1.0)
    }

    /// Arrow speed for a draw factor: linear from [`BowSpec::min_speed`] to
    /// [`BowSpec::max_speed`].
    pub fn speed(&self, draw_factor: f32) -> f32 {
        self.min_speed + (self.max_speed - self.min_speed) * draw_factor.clamp(0.0, 1.0)
    }
}

/// Damage of an arrow hit from ammunition damage, bow damage and draw factor. The default is
/// the hook formula of VR-06; the maintainer can plug in Gothic's original computation.
pub trait ArrowDamage {
    fn damage(&self, ammo: f32, bow: f32, draw_factor: f32) -> f32;
}

/// `(ammo_damage + bow_damage) * draw_factor`.
#[derive(Debug, Clone, Copy, Default)]
pub struct LinearDamage;

impl ArrowDamage for LinearDamage {
    fn damage(&self, ammo: f32, bow: f32, draw_factor: f32) -> f32 {
        (ammo + bow) * draw_factor.clamp(0.0, 1.0)
    }
}

/// Where the quiver sits: relative to the head (yaw only), over the shoulder of the drawing
/// hand's side, behind the head.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Quiver {
    pub offset: Vec3,
    pub radius: f32,
}

impl Default for Quiver {
    fn default() -> Self {
        Self {
            offset: Vec3::new(0.12, -0.05, 0.18),
            radius: 0.2,
        }
    }
}

impl Quiver {
    /// World centre of the quiver opening for a drawing hand.
    pub fn center(&self, head: &Pose, hand: Hand) -> Vec3 {
        let yaw = Quat::from_rotation_y(head.yaw());
        head.position + yaw * Vec3::new(self.offset.x * hand.sign(), self.offset.y, self.offset.z)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub enum ArcherState {
    Idle,
    /// An arrow is in this hand.
    Holding(Hand),
    /// Nocked on the string, drawn `draw` m by this hand.
    Nocked {
        hand: Hand,
        draw: f32,
    },
}

/// One frame of input.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ArcherInput {
    pub head: Pose,
    /// World grip poses.
    pub hands: [Option<Pose>; 2],
    /// Grip held per hand.
    pub grip: [bool; 2],
    /// The bow's world pose and the hand holding it.
    pub bow: Option<(Pose, Hand)>,
    /// The hand already holds another object (cannot take an arrow).
    pub busy: [bool; 2],
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum ArcherEvent {
    TookArrow(Hand),
    Nocked,
    /// The arrow left the bow: centre position, velocity, draw factor.
    Fired {
        origin: Vec3,
        velocity: Vec3,
        draw_factor: f32,
    },
    /// The arrow was let go without firing (falls).
    Dropped {
        at: Pose,
    },
}

/// Per-player archery state.
#[derive(Debug, Clone, PartialEq)]
pub struct Archer {
    pub bow: BowSpec,
    pub arrow: ArrowSpec,
    pub quiver: Quiver,
    pub state: ArcherState,
    /// Nocking distance, m.
    pub nock_radius: f32,
    /// From this distance on a held arrow glides toward the string (visual lead-in), m.
    pub nock_lead_radius: f32,
    was_grip: [bool; 2],
}

impl Default for Archer {
    fn default() -> Self {
        Self {
            bow: BowSpec::default(),
            arrow: ArrowSpec::default(),
            quiver: Quiver::default(),
            state: ArcherState::Idle,
            nock_radius: 0.1,
            nock_lead_radius: 0.15,
            was_grip: [false; 2],
        }
    }
}

impl Archer {
    /// World pose of the arrow (centre, -Z toward the tip) in the current state.
    pub fn arrow_pose(&self, input: &ArcherInput) -> Option<Pose> {
        match self.state {
            ArcherState::Idle => None,
            ArcherState::Holding(h) => input.hands[h.index()].map(|p| {
                // Held at the nock end, pointing along the hand's forward ...
                let held = Pose::new(
                    p.position + p.forward() * self.arrow.half_length,
                    p.rotation,
                );
                // ... gliding onto the string near the bow (Valve's ArrowHand lead-in).
                match input.bow {
                    Some((bow, bow_hand)) if bow_hand != h => {
                        let rest = bow.transform_point(self.bow.string_rest);
                        let d = p.position.distance(rest);
                        let t = ((self.nock_lead_radius - d)
                            / (self.nock_lead_radius - self.nock_radius).max(1e-3))
                        .clamp(0.0, 1.0);
                        if t <= 0.0 {
                            return held;
                        }
                        let (_, nocked) = self.aim(&bow, rest);
                        Pose::new(
                            held.position.lerp(nocked.position, t),
                            held.rotation.slerp(nocked.rotation, t),
                        )
                    }
                    _ => held,
                }
            }),
            ArcherState::Nocked { hand, .. } => {
                let (bow, _) = input.bow?;
                Some(self.aim(&bow, input.hands[hand.index()]?.position).1)
            }
        }
    }

    /// Draw length and arrow pose for a drawing hand at `hand`: the arrow lies on the line from
    /// the hand through the arrow rest, its nock at the hand (clamped to the full draw).
    pub fn aim(&self, bow: &Pose, hand: Vec3) -> (f32, Pose) {
        let rest = bow.transform_point(self.bow.string_rest);
        let back = bow.rotation * Vec3::Z;
        let rel = hand - rest;
        let draw = if rel.dot(back) > 0.0 {
            rel.length().min(self.bow.max_draw)
        } else {
            0.0
        };
        let dir = if rel.length() > 1e-3 && rel.dot(back) > 0.0 {
            -rel.normalize()
        } else {
            -back
        };
        let nock = rest - dir * draw;
        (
            draw,
            Pose::new(
                nock + dir * self.arrow.half_length,
                Quat::from_rotation_arc(Vec3::NEG_Z, dir),
            ),
        )
    }

    /// Where the nock is for a drawing hand at `hand`.
    pub fn nock_point(&self, bow: &Pose, hand: Vec3) -> Vec3 {
        let (_, p) = self.aim(bow, hand);
        p.position - p.forward() * self.arrow.half_length
    }

    /// Advance one frame.
    pub fn update(&mut self, input: &ArcherInput) -> Vec<ArcherEvent> {
        let mut out = Vec::new();
        let pressed = |i: usize| input.grip[i] && !self.was_grip[i];
        let released = |i: usize| !input.grip[i] && self.was_grip[i];
        match self.state {
            ArcherState::Idle => {
                for hand in Hand::BOTH {
                    let i = hand.index();
                    let holds_bow = input.bow.is_some_and(|(_, h)| h == hand);
                    if !pressed(i) || holds_bow || input.busy[i] {
                        continue;
                    }
                    if let Some(p) = input.hands[i]
                        && p.position.distance(self.quiver.center(&input.head, hand))
                            <= self.quiver.radius
                    {
                        self.state = ArcherState::Holding(hand);
                        out.push(ArcherEvent::TookArrow(hand));
                        break;
                    }
                }
            }
            ArcherState::Holding(hand) => {
                let i = hand.index();
                if !input.grip[i] {
                    if let Some(at) = self.arrow_pose(input) {
                        out.push(ArcherEvent::Dropped { at });
                    }
                    self.state = ArcherState::Idle;
                } else if let (Some((bow, bow_hand)), Some(p)) = (input.bow, input.hands[i])
                    && bow_hand != hand
                    && p.position
                        .distance(bow.transform_point(self.bow.string_rest))
                        <= self.nock_radius
                {
                    self.state = ArcherState::Nocked { hand, draw: 0.0 };
                    out.push(ArcherEvent::Nocked);
                }
            }
            ArcherState::Nocked { hand, .. } => {
                let i = hand.index();
                let (Some((bow, _)), Some(p)) = (input.bow, input.hands[i]) else {
                    // Bow dropped or hand lost: the arrow falls.
                    if let Some(at) = self.arrow_pose(input) {
                        out.push(ArcherEvent::Dropped { at });
                    }
                    self.state = ArcherState::Idle;
                    self.was_grip = input.grip;
                    return out;
                };
                let rest = bow.transform_point(self.bow.string_rest);
                let back = bow.rotation * Vec3::Z;
                let rel = p.position - rest;
                let along = rel.dot(back);
                let off = (rel - back * along).length();
                let (draw, aimed) = self.aim(&bow, p.position);
                let pose = Some(aimed);
                if released(i) || !input.grip[i] {
                    if draw >= self.bow.min_draw {
                        let k = self.bow.draw_factor(draw);
                        out.push(ArcherEvent::Fired {
                            origin: aimed.position,
                            velocity: aimed.forward() * self.bow.speed(k),
                            draw_factor: k,
                        });
                    } else if let Some(at) = pose {
                        out.push(ArcherEvent::Dropped { at });
                    }
                    self.state = ArcherState::Idle;
                } else if off > self.bow.max_off_axis || along < -0.1 {
                    if let Some(at) = pose {
                        out.push(ArcherEvent::Dropped { at });
                    }
                    self.state = ArcherState::Idle;
                } else {
                    self.state = ArcherState::Nocked { hand, draw };
                }
            }
        }
        self.was_grip = input.grip;
        out
    }
}

/// Bend an aim `velocity` toward the best target centre inside `cone_deg` (ballistically
/// corrected), by `strength` 0..1. Targets are (id, centre).
pub fn aim_assist(
    origin: Vec3,
    velocity: Vec3,
    targets: &[(u64, Vec3)],
    cone_deg: f32,
    strength: f32,
) -> (Vec3, Option<u64>) {
    let speed = velocity.length();
    if speed < 1.0 || strength <= 0.0 {
        return (velocity, None);
    }
    let dir = velocity / speed;
    let cos = cone_deg.to_radians().cos();
    let mut best: Option<(f32, u64, Vec3)> = None;
    for (id, c) in targets {
        let Some(ideal) = crate::ballistics::ballistic_direction(origin, *c, speed) else {
            continue;
        };
        let d = ideal.dot(dir);
        if d >= cos && best.is_none_or(|b| d > b.0) {
            best = Some((d, *id, ideal));
        }
    }
    match best {
        Some((_, id, ideal)) => (
            dir.lerp(ideal, strength.clamp(0.0, 1.0)).normalize() * speed,
            Some(id),
        ),
        None => (velocity, None),
    }
}
