//! Original animation clips on the Quest avatars, reusing `gothic-animation` (mirrored clip packs, `Rig`,
//! `ClipBinding`, pose blending). The packs (`clips_hero.grcp`, `clips_orc.grcp`) are written by
//! `examples/pack_clips.rs` from the user's own install into the local data pack.
//!
//! A [`ClipSet`] samples a weighted set of clips into model-space bone rotations (in the avatar's facing frame) that
//! `Retarget::apply` blends over the IK/ragdoll pose per bone ([`PoseOverride`]).
//!
//! [`Gait`] picks the locomotion clips from the avatar's body-local velocity: Walk/Run x Fwd/Back/Left/Right,
//! blended by the velocity components, played at `speed / nominal speed` (the trailing number of the original clip
//! name, cm/s) so the feet match the ground speed.
use std::collections::HashMap;
use std::path::Path;
use std::sync::Arc;

use bevy::math::Affine3A;
use bevy::prelude::*;
use gothic_animation::clip::{Clip, ClipBinding};
use gothic_animation::clip_pack::ClipPack;
use gothic_animation::names::trailing_speed;
use gothic_animation::pose::Trs;
use gothic_animation::rig::{self, EvalStep, Rig};

use crate::actor::ActorRig;

/// Per-node world rotation overrides blended over the avatar pose.
pub struct PoseOverride {
    /// World rotation per quest-rig node (None: untouched).
    pub rot: Vec<Option<Quat>>,
    /// Blend weight per node.
    pub weight: Vec<f32>,
}

pub struct ClipSet {
    rig: Arc<Rig>,
    plan: Vec<EvalStep>,
    clips: HashMap<String, (Arc<Clip>, Arc<ClipBinding>)>,
    /// Quest-rig node per animation-rig node.
    qnode: Vec<Option<usize>>,
    /// Rotation turning the clips' model-space facing to world -Z.
    pub fix: Quat,
    n_quest: usize,
}

impl ClipSet {
    pub fn load(path: &Path, qrig: &ActorRig, family: &str) -> Option<Self> {
        let pack = ClipPack::load(path).ok()?;
        let names = qrig.names.clone();
        let parent: Vec<u16> = qrig
            .parent
            .iter()
            .map(|&p| if p < 0 { rig::NONE } else { p as u16 })
            .collect();
        let bind: Vec<Trs> = qrig
            .bind_local
            .iter()
            .map(|t| Trs {
                t: t.translation,
                r: t.rotation,
                s: t.scale,
            })
            .collect();
        let mut rig = Rig::from_parts(family, names, parent, bind);
        rig.family = family.to_string();
        let rig = Arc::new(rig);
        let plan = rig.plan(&(0..rig.names.len() as u16).collect::<Vec<_>>());
        let mut clips = HashMap::new();
        for pc in pack.clips {
            let binding = Arc::new(ClipBinding::new(&rig, &pc.clip));
            clips.insert(pc.clip.name.to_lowercase(), (Arc::new(pc.clip), binding));
        }
        let qnode = rig
            .names
            .iter()
            .map(|n| qrig.by_name.get(n).copied())
            .collect();
        let mut set = Self {
            rig,
            plan,
            clips,
            qnode,
            fix: Quat::IDENTITY,
            n_quest: qrig.names.len(),
        };
        // Facing of the clips' model space, measured on the family's idle clip like the desktop animator does.
        if let Some(idle) = set.find("ambient_loop") {
            let mut model = vec![Affine3A::IDENTITY; set.rig.names.len()];
            let pose = set.pose(&[(idle.as_str(), 0.0, 1.0)]);
            rig::evaluate(&set.plan, &pose, &mut model);
            if let Some(f) = set.rig.facing_in_pose(&model) {
                set.fix = Quat::from_rotation_arc(f, Vec3::NEG_Z);
            }
        }
        Some(set)
    }

    /// First clip whose (lower-case) name contains `needle`.
    pub fn find(&self, needle: &str) -> Option<String> {
        let n = needle.to_lowercase();
        let mut found: Vec<&String> = self.clips.keys().filter(|k| k.contains(&n)).collect();
        found.sort();
        found.first().map(|s| (*s).clone())
    }

    pub fn duration(&self, key: &str) -> f32 {
        self.clips.get(key).map_or(1.0, |c| c.0.duration.max(0.05))
    }

    /// Nominal ground speed (m/s) of a locomotion clip, from the trailing number of its name.
    pub fn nominal_speed(&self, key: &str) -> f32 {
        self.clips
            .get(key)
            .map_or(0.0, |c| trailing_speed(&c.0.name))
            / 100.0
    }

    /// Weighted pose (`(clip key, time, weight)` layers) in slot space.
    fn pose(&self, layers: &[(&str, f32, f32)]) -> Vec<Trs> {
        let mut pose = self.rig.rest.clone();
        let mut acc = 0.0f32;
        for &(key, t, w) in layers {
            let Some((clip, binding)) = self.clips.get(key) else {
                continue;
            };
            if w <= 1e-4 {
                continue;
            }
            let t = if clip.looping {
                clip.wrap(t)
            } else {
                t.clamp(0.0, clip.duration)
            };
            if acc <= 0.0 {
                binding.sample(clip, &self.rig.rest, t, &mut pose);
            } else {
                binding.blend_into(clip, t, w / (acc + w), &mut pose);
            }
            acc += w;
        }
        pose
    }

    /// Samples the layers and returns the world rotation of every quest-rig node (`yaw` = body yaw about Y).
    pub fn sample_world(&self, layers: &[(&str, f32, f32)], yaw: Quat) -> Vec<Option<Quat>> {
        let pose = self.pose(layers);
        let mut model = vec![Affine3A::IDENTITY; self.rig.names.len()];
        rig::evaluate(&self.plan, &pose, &mut model);
        let mut out = vec![None; self.n_quest];
        for (j, m) in model.iter().enumerate() {
            if let Some(q) = self.qnode[j] {
                let (_, r, _) = m.to_scale_rotation_translation();
                out[q] = Some(yaw * self.fix * r.normalize());
            }
        }
        out
    }
}

/// Locomotion state: smoothed body-local velocity, gait phase and the clip blend.
#[derive(Default)]
pub struct Gait {
    prev: Option<Vec3>,
    vel: Vec3,
    phase: f32,
    /// 0 = IK legs (standing), 1 = clip legs.
    active: f32,
    pub speed: f32,
}

impl Gait {
    /// `pos` metres (world), `yaw` the body yaw. Returns the leg-override weight and layer list.
    pub fn update(
        &mut self,
        set: &ClipSet,
        pos: Vec3,
        yaw: f32,
        dt: f32,
    ) -> (f32, Vec<(String, f32, f32)>) {
        let dt = dt.max(1e-3);
        if let Some(p) = self.prev {
            let v = (pos - p) / dt;
            let v = Vec3::new(v.x, 0.0, v.z);
            let v = if v.length() > 12.0 { Vec3::ZERO } else { v }; // teleports / recenter jumps
            self.vel = self.vel.lerp(v, (dt * 10.0).min(1.0));
        }
        self.prev = Some(pos);
        let local = Quat::from_rotation_y(-yaw) * self.vel;
        self.speed = local.length();
        let target = ((self.speed - 0.2) / 0.4).clamp(0.0, 1.0);
        self.active += (target - self.active) * (dt * 8.0).min(1.0);
        if self.active < 0.01 {
            return (0.0, vec![]);
        }
        // Direction weights (forward is -Z in the body frame).
        let d = if self.speed > 1e-3 {
            local / self.speed
        } else {
            Vec3::NEG_Z
        };
        let (wf, wb, wl, wr) = ((-d.z).max(0.0), d.z.max(0.0), (-d.x).max(0.0), d.x.max(0.0));
        let s = (wf + wb + wl + wr).max(1e-3);
        // Walk to run between 1.4 and 2.8 m/s.
        let run = ((self.speed - 1.4) / 1.4).clamp(0.0, 1.0);
        let mut layers = Vec::new();
        let mut cycle = 0.0f32;
        for (gait, gw) in [("move_walk", 1.0 - run), ("move_run", run)] {
            if gw <= 0.0 {
                continue;
            }
            for (dir, dw) in [
                ("n_fwd", wf),
                ("n_back", wb),
                ("n_left", wl),
                ("n_right", wr),
            ] {
                let w = gw * dw / s;
                if w <= 0.0 {
                    continue;
                }
                if let Some(key) = set.find(&format!("{gait}_{dir}")) {
                    let rate = (self.speed / set.nominal_speed(&key).max(0.3)).clamp(0.4, 2.2);
                    cycle += w * rate / set.duration(&key);
                    layers.push((key, w));
                }
            }
        }
        self.phase = (self.phase + dt * cycle).fract();
        // One shared normalised phase for all layers (the cycles are aligned in the original data).
        let out = layers
            .into_iter()
            .map(|(k, w)| {
                let t = self.phase * set.duration(&k);
                (k, t, w)
            })
            .collect();
        (self.active, out)
    }
}

/// A one-shot full-body clip (hit reaction) with fade in and out.
#[derive(Default)]
pub struct OneShot {
    key: Option<String>,
    t: f32,
    len: f32,
}

impl OneShot {
    pub fn start(&mut self, set: &ClipSet, needle: &str) {
        if let Some(k) = set.find(needle) {
            self.len = set.duration(&k);
            self.key = Some(k);
            self.t = 0.0;
        }
    }
    /// Layer and weight while playing.
    pub fn update(&mut self, dt: f32) -> Option<(String, f32, f32)> {
        let key = self.key.clone()?;
        self.t += dt;
        if self.t >= self.len {
            self.key = None;
            return None;
        }
        let fade = 0.08f32;
        let w = (self.t / fade)
            .min(1.0)
            .min(((self.len - self.t) / fade).clamp(0.0, 1.0));
        Some((key, self.t, w))
    }
}
