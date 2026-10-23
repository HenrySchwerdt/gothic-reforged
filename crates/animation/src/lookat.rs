//! Look-at constraints of the visual animation property set (E04): the head and eyes follow a
//! target within per-bone angle limits (`eCVisualAnimation_PS::SetLookAtConstraint`,
//! `eCLookAtConstraint::SetTargetPos`, EMotionFX `LookAtConstraint` update 0x102e2460).
//!
//! The axis conventions of the limit vectors are reconstructed ([I]): x is pitch, y is yaw and
//! z is roll, which fits the default limits of (-360, -360, 0) .. (360, 360, 0) (free pitch and
//! yaw, no roll). The interpolation law is recovered: the rotation moves towards the wanted one
//! by `clamp(speed * dt * 6, 0, 1)` per frame.
use crate::locomotion::{FORWARD, RIGHT};
use bevy::math::{Quat, Vec3};

/// Factor between the file's `InterpolationSpeed` and the per-second turn fraction (the double
/// at Engine 0x10488a08).
pub const SPEED_SCALE: f32 = 6.0;

/// Limits and speed of one constrained bone, as stored in the `.xlac` side file.
#[derive(Clone, Debug, PartialEq)]
pub struct LookAtSetup {
    /// Minimum angles in degrees (pitch, yaw, roll).
    pub min: Vec3,
    /// Maximum angles in degrees.
    pub max: Vec3,
    pub speed: f32,
}

impl Default for LookAtSetup {
    /// `ImportLookAtConstraints` defaults: (-360, -360, 0) .. (360, 360, 0).
    fn default() -> Self {
        Self {
            min: Vec3::new(-360.0, -360.0, 0.0),
            max: Vec3::new(360.0, 360.0, 0.0),
            speed: 1.0,
        }
    }
}

/// One constraint: the bone, its setup and the current smoothed rotation.
#[derive(Clone, Debug)]
pub struct LookAtConstraint {
    pub node: String,
    pub setup: LookAtSetup,
    /// Current rotation relative to the animated bone rotation.
    pub rotation: Quat,
    pub enabled: bool,
}

impl LookAtConstraint {
    /// Rotation that aims the forward axis at `target`, both in the bone's parent frame
    /// (forward = -Z, right = +X, up = +Y), with the yaw and pitch clamped to the limits.
    pub fn wanted_rotation(&self, bone_pos: Vec3, target: Vec3) -> Quat {
        let d = target - bone_pos;
        if d.length_squared() < 1e-8 {
            return Quat::IDENTITY;
        }
        let yaw = d.dot(RIGHT).atan2(d.dot(FORWARD));
        let flat = (d.dot(RIGHT).powi(2) + d.dot(FORWARD).powi(2)).sqrt();
        let pitch = d.y.atan2(flat);
        let yaw = yaw.clamp(self.setup.min.y.to_radians(), self.setup.max.y.to_radians());
        let pitch = pitch.clamp(self.setup.min.x.to_radians(), self.setup.max.x.to_radians());
        Quat::from_rotation_y(-yaw) * Quat::from_rotation_x(pitch)
    }

    /// Moves the smoothed rotation towards the wanted one and returns it. Without a target the
    /// rotation relaxes back to identity at the same speed.
    pub fn step(&mut self, dt: f32, bone_pos: Vec3, target: Option<Vec3>) -> Quat {
        let wanted = match (self.enabled, target) {
            (true, Some(t)) => self.wanted_rotation(bone_pos, t),
            _ => Quat::IDENTITY,
        };
        let f = (self.setup.speed * dt * SPEED_SCALE).clamp(0.0, 1.0);
        self.rotation = self.rotation.slerp(wanted, f);
        self.rotation
    }
}

/// All look-at constraints of a character plus the shared target.
#[derive(Clone, Debug, Default)]
pub struct LookAtSet {
    constraints: Vec<LookAtConstraint>,
    /// Target in the character's local space.
    pub target: Option<Vec3>,
}

impl LookAtSet {
    /// Port of eCVisualAnimation_PS::SetLookAtConstraint (Engine:0x10032750): one constraint
    /// per node name; setting an existing one replaces limits and speed and resets its state.
    pub fn set_constraint(&mut self, node: &str, min: Vec3, max: Vec3, speed: f32) {
        gothic_portcov::hit!("set_constraint");
        let setup = LookAtSetup { min, max, speed };
        match self.constraints.iter_mut().find(|c| c.node == node) {
            Some(c) => {
                c.setup = setup;
                c.rotation = Quat::IDENTITY;
            }
            None => self.constraints.push(LookAtConstraint {
                node: node.to_owned(),
                setup,
                rotation: Quat::IDENTITY,
                enabled: true,
            }),
        }
    }

    /// Port of eCVisualAnimation_PS::RemoveLookAtConstraint (Engine:0x10032890).
    pub fn remove_constraint(&mut self, node: &str) -> bool {
        gothic_portcov::hit!("remove_constraint");
        match self.constraints.iter().position(|c| c.node == node) {
            Some(i) => {
                self.constraints.remove(i);
                true
            }
            None => false,
        }
    }

    /// Port of ClearLookAtConstraints (Engine:0x10032180).
    pub fn clear(&mut self) {
        gothic_portcov::hit!("clear");
        self.constraints.clear();
    }

    /// Port of eCLookAtConstraint::Enable (Engine:0x1002afa0) for every constraint.
    pub fn enable_all(&mut self, enabled: bool) {
        gothic_portcov::hit!("enable_all");
        for c in &mut self.constraints {
            c.enabled = enabled;
        }
    }

    pub fn constraints(&self) -> &[LookAtConstraint] {
        &self.constraints
    }

    pub fn get(&self, node: &str) -> Option<&LookAtConstraint> {
        self.constraints.iter().find(|c| c.node == node)
    }

    /// Steps every constraint towards the current target. `bone_pos` returns the bone position
    /// in the character's frame; the result is the (node, rotation) list to apply on top of
    /// the animated pose.
    pub fn step(
        &mut self,
        dt: f32,
        bone_pos: impl Fn(&str) -> Option<Vec3>,
    ) -> Vec<(String, Quat)> {
        let target = self.target;
        self.constraints
            .iter_mut()
            .filter_map(|c| {
                let pos = bone_pos(&c.node)?;
                Some((c.node.clone(), c.step(dt, pos, target)))
            })
            .collect()
    }
}

/// Parses the `.xlac` side file ([I], tolerant): sections `[Node]` with `InterpolationSpeed`,
/// `MinConstraints` and `MaxConstraints` (three numbers in any separator style). Missing keys
/// keep the defaults.
pub fn parse_xlac(text: &str) -> Vec<(String, LookAtSetup)> {
    fn numbers(s: &str) -> Vec<f32> {
        s.split(|c: char| !(c.is_ascii_digit() || matches!(c, '-' | '+' | '.' | 'e' | 'E')))
            .filter_map(|t| t.parse().ok())
            .collect()
    }
    let mut out: Vec<(String, LookAtSetup)> = Vec::new();
    for line in text.lines().map(str::trim) {
        if let Some(name) = line.strip_prefix('[').and_then(|l| l.strip_suffix(']')) {
            out.push((name.trim().to_owned(), LookAtSetup::default()));
        } else if let (Some((_, setup)), Some((key, value))) =
            (out.last_mut(), line.split_once('='))
        {
            let n = numbers(value);
            match key.trim() {
                "InterpolationSpeed" => setup.speed = n.first().copied().unwrap_or(setup.speed),
                "MinConstraints" if n.len() >= 3 => setup.min = Vec3::new(n[0], n[1], n[2]),
                "MaxConstraints" if n.len() >= 3 => setup.max = Vec3::new(n[0], n[1], n[2]),
                _ => {}
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn constraint(min_yaw: f32, max_yaw: f32) -> LookAtConstraint {
        LookAtConstraint {
            node: "Head".into(),
            setup: LookAtSetup {
                min: Vec3::new(-20.0, min_yaw, 0.0),
                max: Vec3::new(20.0, max_yaw, 0.0),
                speed: 1.0,
            },
            rotation: Quat::IDENTITY,
            enabled: true,
        }
    }

    fn yaw_deg(q: Quat) -> f32 {
        let f = q * FORWARD;
        -f.dot(RIGHT).atan2(f.dot(FORWARD)).to_degrees() * -1.0
    }

    #[test]
    fn target_ahead_needs_no_rotation_and_side_targets_are_clamped() {
        let c = constraint(-60.0, 60.0);
        assert!(
            c.wanted_rotation(Vec3::ZERO, FORWARD * 100.0)
                .angle_between(Quat::IDENTITY)
                < 1e-4
        );
        // Target 45 degrees to the right is within the limits.
        let q = c.wanted_rotation(Vec3::ZERO, (FORWARD + RIGHT) * 100.0);
        assert!((yaw_deg(q).abs() - 45.0).abs() < 0.1, "{}", yaw_deg(q));
        // Target directly behind is clamped to the limit.
        let q = c.wanted_rotation(Vec3::ZERO, -FORWARD * 100.0 + RIGHT * 1.0);
        assert!((yaw_deg(q).abs() - 60.0).abs() < 0.1);
        // Pitch is limited too: straight up becomes 20 degrees.
        let q = c.wanted_rotation(Vec3::ZERO, Vec3::Y * 100.0);
        let up = (q * FORWARD).y.asin().to_degrees();
        assert!((up - 20.0).abs() < 0.1, "{up}");
    }

    #[test]
    fn smoothing_follows_the_original_factor() {
        let mut c = constraint(-90.0, 90.0);
        c.setup.speed = 1.0;
        let target = RIGHT * 100.0; // 90 degrees
        c.step(1.0 / 60.0, Vec3::ZERO, Some(target));
        // fraction = 1 * (1/60) * 6 = 0.1 of the way.
        assert!(
            (yaw_deg(c.rotation).abs() - 9.0).abs() < 0.2,
            "{}",
            yaw_deg(c.rotation)
        );
        // Large dt saturates at the target.
        c.step(10.0, Vec3::ZERO, Some(target));
        assert!((yaw_deg(c.rotation).abs() - 90.0).abs() < 0.1);
        // Without a target (or disabled) it relaxes back.
        c.enabled = false;
        c.step(10.0, Vec3::ZERO, Some(target));
        assert!(c.rotation.angle_between(Quat::IDENTITY) < 1e-4);
    }

    #[test]
    fn constraint_set_upserts_by_node_and_resets() {
        let mut set = LookAtSet::default();
        set.set_constraint("Head", Vec3::splat(-30.0), Vec3::splat(30.0), 2.0);
        set.set_constraint("Eye", Vec3::splat(-10.0), Vec3::splat(10.0), 3.0);
        set.target = Some(RIGHT * 100.0);
        let out = set.step(0.1, |_| Some(Vec3::ZERO));
        assert_eq!(out.len(), 2);
        assert!(
            set.get("Head")
                .unwrap()
                .rotation
                .angle_between(Quat::IDENTITY)
                > 0.01
        );
        set.set_constraint("Head", Vec3::splat(-5.0), Vec3::splat(5.0), 1.0);
        assert_eq!(set.constraints().len(), 2);
        assert_eq!(set.get("Head").unwrap().rotation, Quat::IDENTITY);
        assert!(set.remove_constraint("Eye"));
        assert!(!set.remove_constraint("Eye"));
    }

    #[test]
    fn xlac_defaults_and_overrides() {
        let text = "[Bip01 Head]\nInterpolationSpeed = 4.5\nMinConstraints = -40 -70 0\n\
                    MaxConstraints=(40, 70, 0)\n[Eye]\n";
        let list = parse_xlac(text);
        assert_eq!(list.len(), 2);
        assert_eq!(list[0].1.speed, 4.5);
        assert_eq!(list[0].1.min, Vec3::new(-40.0, -70.0, 0.0));
        assert_eq!(list[0].1.max, Vec3::new(40.0, 70.0, 0.0));
        assert_eq!(list[1].1, LookAtSetup::default());
    }
}
