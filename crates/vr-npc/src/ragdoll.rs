//! Bevy-free active ragdoll for humanoids.
//!
//! The body is a point-mass skeleton on the generic humanoid joints of
//! [`gothic_vr::body::Joint`]: bones are distance constraints, the torso is braced by cross
//! constraints, joint limits are minimum-distance and hinge-side constraints (knees bend
//! forward, elbows do not fold through the upper arm, the head stays above the chest).
//! Integration is position-based (Verlet + Gauss-Seidel projection, sub-stepped).
//!
//! **Muscles** pull each joint toward the animation's target pose with a per-joint gain
//! scaled by the current muscle strength, so the body plays its animation while reacting
//! physically: hits are impulses on the nearest joints, hands can push joints and grab them.
//!
//! **Balance**: the actual centre of mass is compared with the animated one (what the body
//! would have if it followed its animation, walking included). A moderate disturbance or
//! error makes the body *stagger* (weaker muscles and a recovery step that
//! moves the target pose under the centre of mass), a large one or lost balance makes it
//! *fall* (muscles off, pure ragdoll); after lying still it *gets up* (muscles fade back in
//! while the target blends from the lying pose to standing).
//!
//! Units: metres, kilograms, seconds; Y up.
use glam::Vec3;
use gothic_vr::body::{BONES, BodyPose, JOINT_COUNT, Joint};
use serde::{Deserialize, Serialize};

/// Balance state.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Stance {
    Standing,
    /// Knocked off balance; recovering for the stored seconds.
    Stagger,
    Fallen,
    GettingUp,
}

/// Tunables.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct RagdollSettings {
    pub gravity: f32,
    pub substeps: u32,
    pub iterations: u32,
    /// Velocity kept per substep (air drag / joint damping).
    pub damping: f32,
    /// Fraction of the remaining distance to the target pose corrected per substep at full
    /// strength.
    pub muscle_gain: f32,
    /// Muscle strength while staggering.
    pub stagger_strength: f32,
    pub stagger_time: f32,
    /// Seconds lying still before getting up.
    pub lie_time: f32,
    pub get_up_time: f32,
    /// A single impulse above this (N*s) knocks the body down.
    pub fall_impulse: f32,
    /// An impulse above this (N*s) makes it stagger.
    pub stagger_impulse: f32,
    /// Centre of mass further than this (m) from the animated one staggers ...
    pub stagger_offset: f32,
    /// ... and further than this falls.
    pub fall_offset: f32,
}

impl Default for RagdollSettings {
    fn default() -> Self {
        Self {
            gravity: 9.81,
            substeps: 4,
            iterations: 6,
            damping: 0.995,
            muscle_gain: 0.22,
            stagger_strength: 0.3,
            stagger_time: 0.7,
            lie_time: 2.0,
            get_up_time: 1.5,
            fall_impulse: 60.0,
            stagger_impulse: 12.0,
            stagger_offset: 0.18,
            fall_offset: 0.45,
        }
    }
}

const MASS: [f32; JOINT_COUNT] = [
    12.0, // Pelvis
    9.0,  // Spine
    13.0, // Chest
    3.0,  // Neck
    5.0,  // Head
    2.5,  // ShoulderL
    2.5,  // ShoulderR
    1.8,  // ElbowL
    1.8,  // ElbowR
    1.0,  // HandL
    1.0,  // HandR
    4.0,  // HipL
    4.0,  // HipR
    4.0,  // KneeL
    4.0,  // KneeR
    1.5,  // AnkleL
    1.5,  // AnkleR
];

/// Collision radius of each joint against the ground.
const RADIUS: [f32; JOINT_COUNT] = [
    0.12, 0.11, 0.12, 0.06, 0.1, 0.06, 0.06, 0.05, 0.05, 0.04, 0.04, 0.08, 0.08, 0.06, 0.06, 0.05,
    0.05,
];

/// Per-joint muscle gain multipliers (core strong, extremities lighter).
const GAIN: [f32; JOINT_COUNT] = [
    1.0, 1.0, 1.0, 0.9, 0.9, 0.9, 0.9, 0.7, 0.7, 0.6, 0.6, 1.0, 1.0, 0.9, 0.9, 1.0, 1.0,
];

/// Torso braces (keep the trunk rigid).
const BRACES: [(Joint, Joint); 7] = [
    (Joint::ShoulderL, Joint::ShoulderR),
    (Joint::HipL, Joint::HipR),
    (Joint::ShoulderL, Joint::Pelvis),
    (Joint::ShoulderR, Joint::Pelvis),
    (Joint::HipL, Joint::Chest),
    (Joint::HipR, Joint::Chest),
    (Joint::Neck, Joint::Spine),
];

/// Minimum distances (fraction of the rest distance) acting as joint limits.
const LIMITS: [(Joint, Joint, f32); 7] = [
    (Joint::Head, Joint::Chest, 0.85),
    (Joint::ShoulderL, Joint::HandL, 0.25),
    (Joint::ShoulderR, Joint::HandR, 0.25),
    (Joint::HipL, Joint::AnkleL, 0.35),
    (Joint::HipR, Joint::AnkleR, 0.35),
    (Joint::Pelvis, Joint::Head, 0.7),
    (Joint::AnkleL, Joint::AnkleR, 0.5),
];

#[derive(Debug, Clone, Copy, PartialEq)]
struct Link {
    a: usize,
    b: usize,
    rest: f32,
    /// Only push apart (limit), never pull together.
    min_only: bool,
}

/// An external hand holding a joint.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct GrabHold {
    pub joint: Joint,
    pub target: Vec3,
    /// 0..1 per substep.
    pub stiffness: f32,
}

/// The active ragdoll.
#[derive(Debug, Clone)]
pub struct ActiveRagdoll {
    pub settings: RagdollSettings,
    pos: [Vec3; JOINT_COUNT],
    prev: [Vec3; JOINT_COUNT],
    links: Vec<Link>,
    stance: Stance,
    stance_time: f32,
    /// Muscle strength 0..1 (from the stance).
    strength: f32,
    /// Accumulated impulse this step (N*s) for the balance check.
    impulse: f32,
    /// Pose captured when the get-up starts.
    lying: Option<[Vec3; JOINT_COUNT]>,
    pub holds: Vec<GrabHold>,
    standing_pelvis_height: f32,
    /// Horizontal offset of the actual from the animated centre of mass (last step).
    balance_error: Vec3,
    /// Recovery step taken when the stagger started (added to the target while staggering).
    recovery: Vec3,
    /// Length of the last substep, s.
    sub_dt: f32,
}

fn com_of(joints: &[Vec3; JOINT_COUNT]) -> Vec3 {
    joints.iter().zip(MASS).map(|(p, m)| *p * m).sum::<Vec3>() / MASS.iter().sum::<f32>()
}

impl ActiveRagdoll {
    /// A ragdoll in `pose` (also its rest proportions).
    pub fn new(pose: &BodyPose) -> Self {
        let pos = pose.joints;
        let mut links = Vec::new();
        let d = |a: Joint, b: Joint| pos[a.index()].distance(pos[b.index()]);
        for (a, b) in BONES.iter().chain(BRACES.iter()) {
            links.push(Link {
                a: a.index(),
                b: b.index(),
                rest: d(*a, *b),
                min_only: false,
            });
        }
        for (a, b, f) in LIMITS {
            links.push(Link {
                a: a.index(),
                b: b.index(),
                rest: d(a, b) * f,
                min_only: true,
            });
        }
        let floor = pos[Joint::AnkleL.index()]
            .y
            .min(pos[Joint::AnkleR.index()].y)
            - 0.08;
        Self {
            settings: RagdollSettings::default(),
            pos,
            prev: pos,
            links,
            stance: Stance::Standing,
            stance_time: 0.0,
            strength: 1.0,
            impulse: 0.0,
            lying: None,
            holds: Vec::new(),
            standing_pelvis_height: pos[Joint::Pelvis.index()].y - floor,
            balance_error: Vec3::ZERO,
            recovery: Vec3::ZERO,
            sub_dt: 1.0 / 360.0,
        }
    }

    pub fn stance(&self) -> Stance {
        self.stance
    }

    pub fn strength(&self) -> f32 {
        self.strength
    }

    pub fn position(&self, j: Joint) -> Vec3 {
        self.pos[j.index()]
    }

    pub fn velocity(&self, j: Joint, dt: f32) -> Vec3 {
        if dt > 0.0 {
            (self.pos[j.index()] - self.prev[j.index()]) / (dt / self.settings.substeps as f32)
        } else {
            Vec3::ZERO
        }
    }

    pub fn total_mass(&self) -> f32 {
        MASS.iter().sum()
    }

    pub fn center_of_mass(&self) -> Vec3 {
        self.pos.iter().zip(MASS).map(|(p, m)| *p * m).sum::<Vec3>() / self.total_mass()
    }

    /// Current joints as a pose (for hitzones and rendering).
    pub fn pose(&self) -> BodyPose {
        let side = self.pos[Joint::ShoulderR.index()] - self.pos[Joint::ShoulderL.index()];
        let fwd = Vec3::Y.cross(side).normalize_or(Vec3::NEG_Z);
        BodyPose {
            joints: self.pos,
            body_yaw: (-fwd.x).atan2(-fwd.z),
            reached: [true; 2],
        }
    }

    /// Height of the pelvis above the given floor relative to standing (1 = upright).
    pub fn uprightness(&self, floor: f32) -> f32 {
        (self.pos[Joint::Pelvis.index()].y - floor) / self.standing_pelvis_height.max(0.1)
    }

    /// Apply an impulse (N*s) at a world point; it is shared by the two nearest joints.
    pub fn apply_impulse(&mut self, point: Vec3, impulse: Vec3, dt: f32) {
        if !impulse.is_finite() || !point.is_finite() || dt <= 0.0 {
            return;
        }
        let sub_dt = dt / self.settings.substeps as f32;
        let mut idx: Vec<usize> = (0..JOINT_COUNT).collect();
        idx.sort_by(|&a, &b| {
            self.pos[a]
                .distance_squared(point)
                .total_cmp(&self.pos[b].distance_squared(point))
        });
        for (k, &i) in idx.iter().take(2).enumerate() {
            let share = if k == 0 { 0.65 } else { 0.35 };
            let dv = impulse * share / MASS[i];
            // Verlet velocity lives in (pos - prev) / sub_dt.
            self.prev[i] -= dv * sub_dt;
        }
        self.impulse += impulse.length();
    }

    /// Push joints out of a moving sphere (a hand) and give them its velocity.
    pub fn push(&mut self, center: Vec3, radius: f32, velocity: Vec3, dt: f32) -> bool {
        let sub_dt = dt / self.settings.substeps as f32;
        let mut touched = false;
        for i in 0..JOINT_COUNT {
            let d = self.pos[i] - center;
            let r = radius + RADIUS[i];
            let len = d.length();
            if len < r && len > 1e-6 {
                let n = d / len;
                self.pos[i] += n * (r - len);
                let v_rel = velocity.dot(n);
                if v_rel > 0.0 {
                    self.prev[i] -= n * v_rel * sub_dt * 0.5;
                    self.impulse += v_rel * MASS[i] * 0.5;
                }
                touched = true;
            }
        }
        touched
    }

    /// Force a stance (scripted knock-down, death).
    pub fn set_stance(&mut self, s: Stance) {
        if s == Stance::Stagger && self.stance != Stance::Stagger {
            // Step toward where the body is being pushed (velocity-led), at most 40 cm.
            let v: Vec3 = (0..JOINT_COUNT)
                .map(|i| (self.pos[i] - self.prev[i]) * MASS[i])
                .sum::<Vec3>()
                / self.total_mass()
                / self.sub_dt;
            let lead = self.balance_error * 1.2 + Vec3::new(v.x, 0.0, v.z) * 0.25;
            self.recovery = lead.clamp_length_max(0.4);
        }
        self.stance = s;
        self.stance_time = 0.0;
        if s == Stance::GettingUp {
            self.lying = Some(self.pos);
        }
    }

    /// Advance one step toward the animation `target` pose (world) with ground height
    /// `ground(x, z)`. Returns the stance after the step.
    #[allow(clippy::needless_range_loop)] // parallel per-joint arrays
    pub fn step(&mut self, target: &BodyPose, ground: impl Fn(f32, f32) -> f32, dt: f32) -> Stance {
        let s = self.settings;
        if !(dt > 0.0 && dt.is_finite()) {
            return self.stance;
        }
        let shift = if self.stance == Stance::Stagger {
            self.recovery
        } else {
            Vec3::ZERO
        };
        let err = self.center_of_mass() - (com_of(&target.joints) + shift);
        self.balance_error = Vec3::new(err.x, 0.0, err.z);
        self.update_stance(&ground, dt);
        let target = self.effective_target(target, &ground);
        let sub = s.substeps.max(1);
        let h = dt / sub as f32;
        self.sub_dt = h;
        let gain = (s.muscle_gain * self.strength).clamp(0.0, 1.0);
        for _ in 0..sub {
            for i in 0..JOINT_COUNT {
                let v = (self.pos[i] - self.prev[i]) * s.damping;
                self.prev[i] = self.pos[i];
                self.pos[i] += v + Vec3::new(0.0, -s.gravity, 0.0) * h * h;
            }
            // Muscles (pose matching) and gravity compensation in proportion to strength.
            for i in 0..JOINT_COUNT {
                let k = gain * GAIN[i];
                let to = target.joints[i] - self.pos[i];
                if to.is_finite() {
                    self.pos[i] += to * k;
                    self.prev[i] += to * k * 0.85; // muscles also damp the joint velocity
                    self.pos[i].y += s.gravity * h * h * self.strength.min(1.0);
                }
            }
            for hold in &self.holds {
                let i = hold.joint.index();
                let to = hold.target - self.pos[i];
                if to.is_finite() {
                    self.pos[i] += to * hold.stiffness.clamp(0.0, 1.0);
                }
            }
            for _ in 0..s.iterations {
                for l in &self.links {
                    let (pa, pb) = (self.pos[l.a], self.pos[l.b]);
                    let d = pb - pa;
                    let len = d.length();
                    if len < 1e-6 || (l.min_only && len >= l.rest) {
                        continue;
                    }
                    let (wa, wb) = (1.0 / MASS[l.a], 1.0 / MASS[l.b]);
                    let corr = d * ((len - l.rest) / (len * (wa + wb)));
                    self.pos[l.a] += corr * wa;
                    self.pos[l.b] -= corr * wb;
                }
                self.hinges();
                for i in 0..JOINT_COUNT {
                    let floor = ground(self.pos[i].x, self.pos[i].z) + RADIUS[i];
                    if self.pos[i].y < floor {
                        self.pos[i].y = floor;
                        // Friction: kill most sliding.
                        let slide = self.pos[i] - self.prev[i];
                        self.prev[i].x += slide.x * 0.6;
                        self.prev[i].z += slide.z * 0.6;
                        self.prev[i].y = self.prev[i].y.max(floor - 1e-4);
                    }
                }
            }
        }
        for p in self.pos.iter_mut().chain(self.prev.iter_mut()) {
            if !p.is_finite() {
                *p = Vec3::ZERO;
            }
        }
        self.impulse = 0.0;
        self.stance
    }

    /// Knees bend forward, elbows bend backward/outward (hinge side constraints).
    fn hinges(&mut self) {
        let side = self.pos[Joint::HipR.index()] - self.pos[Joint::HipL.index()];
        let fwd = Vec3::Y.cross(side).normalize_or_zero();
        if fwd == Vec3::ZERO {
            return;
        }
        for (hip, knee, ankle) in [
            (Joint::HipL, Joint::KneeL, Joint::AnkleL),
            (Joint::HipR, Joint::KneeR, Joint::AnkleR),
        ] {
            let (h, k, a) = (
                self.pos[hip.index()],
                self.pos[knee.index()],
                self.pos[ankle.index()],
            );
            let mid = (h + a) / 2.0;
            let bend = (k - mid).dot(fwd);
            if bend < 0.0 {
                self.pos[knee.index()] -= fwd * bend * 0.5;
            }
        }
    }

    /// Animation target adjusted by the stance (recovery step, get-up blend).
    fn effective_target(&self, target: &BodyPose, ground: &impl Fn(f32, f32) -> f32) -> BodyPose {
        let mut t = target.clone();
        match self.stance {
            Stance::Stagger => {
                // Recovery step: the whole target moves under the displaced centre of mass.
                for j in t.joints.iter_mut() {
                    *j += self.recovery;
                }
            }
            Stance::GettingUp => {
                if let Some(lying) = self.lying {
                    let k = (self.stance_time / self.settings.get_up_time).clamp(0.0, 1.0);
                    let e = k * k * (3.0 - 2.0 * k);
                    // Stand up where the pelvis lies.
                    let anchor = lying[Joint::Pelvis.index()];
                    let off = Vec3::new(
                        anchor.x - target.get(Joint::Pelvis).x,
                        0.0,
                        anchor.z - target.get(Joint::Pelvis).z,
                    );
                    for (i, j) in t.joints.iter_mut().enumerate() {
                        let up = *j + off;
                        let up = Vec3::new(up.x, up.y.max(ground(up.x, up.z)), up.z);
                        *j = lying[i].lerp(up, e);
                    }
                }
            }
            Stance::Standing | Stance::Fallen => {}
        }
        t
    }

    /// Horizontal distance of the actual from the animated centre of mass (last step).
    pub fn balance_offset(&self) -> f32 {
        self.balance_error.length()
    }

    fn update_stance(&mut self, ground: &impl Fn(f32, f32) -> f32, dt: f32) {
        let s = self.settings;
        self.stance_time += dt;
        let offset = self.balance_offset();
        let pelvis = self.pos[Joint::Pelvis.index()];
        let upright = self.uprightness(ground(pelvis.x, pelvis.z));
        match self.stance {
            Stance::Standing => {
                if self.impulse >= s.fall_impulse || offset > s.fall_offset || upright < 0.45 {
                    self.set_stance(Stance::Fallen);
                } else if self.impulse >= s.stagger_impulse || offset > s.stagger_offset {
                    self.set_stance(Stance::Stagger);
                }
            }
            Stance::Stagger => {
                if self.impulse >= s.fall_impulse || offset > s.fall_offset || upright < 0.45 {
                    self.set_stance(Stance::Fallen);
                } else if self.stance_time > s.stagger_time && offset < s.stagger_offset * 0.7 {
                    self.set_stance(Stance::Standing);
                }
            }
            Stance::Fallen => {
                let speed: f32 = (0..JOINT_COUNT)
                    .map(|i| (self.pos[i] - self.prev[i]).length())
                    .fold(0.0, f32::max)
                    / (dt / s.substeps as f32);
                if speed > 0.3 {
                    self.stance_time = 0.0;
                }
                if self.stance_time > s.lie_time {
                    self.set_stance(Stance::GettingUp);
                }
            }
            Stance::GettingUp => {
                if self.impulse >= s.fall_impulse {
                    self.set_stance(Stance::Fallen);
                } else if self.stance_time > s.get_up_time + 0.3 {
                    self.lying = None;
                    self.set_stance(Stance::Standing);
                }
            }
        }
        let want = match self.stance {
            Stance::Standing => 1.0,
            Stance::Stagger => s.stagger_strength,
            Stance::Fallen => 0.0,
            Stance::GettingUp => (self.stance_time / s.get_up_time).clamp(0.0, 1.0),
        };
        // Strength changes quickly but not instantly.
        let rate = 4.0 * dt;
        self.strength += (want - self.strength).clamp(-rate * 3.0, rate);
    }

    /// Move the whole body (spawn / teleport).
    pub fn translate(&mut self, by: Vec3) {
        for p in self.pos.iter_mut().chain(self.prev.iter_mut()) {
            *p += by;
        }
    }
}
