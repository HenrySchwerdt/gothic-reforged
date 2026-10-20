//! Per-character playback: cross-fade stack, optional masked overlay layer, pose evaluation
//! and root motion. Plain data plus methods; the ECS systems in `systems.rs` drive it.
use crate::{
    clip::{Clip, ClipBinding, RootMotion, RootMotionSource},
    pose::Trs,
    rig::{EvalStep, Rig},
};
use bevy::{ecs::component::Component, math::Affine3A};
use std::sync::Arc;

/// Default cross-fade time in seconds.
pub const DEFAULT_FADE: f32 = 0.2;
const MAX_LAYERS: usize = 4;

/// A clip together with its slot mapping for one rig.
#[derive(Clone)]
pub struct Motion {
    pub clip: Arc<Clip>,
    pub binding: Arc<ClipBinding>,
}

#[derive(Clone)]
struct Layer {
    motion: Motion,
    time: f32,
    rate: f32,
    /// Fade-in progress 0..1 (the bottom layer is always fully in).
    fade: f32,
    fade_len: f32,
}

/// Upper-body (or any masked) layer evaluated on top of the base stack.
#[derive(Clone)]
pub struct Overlay {
    pub motion: Motion,
    pub time: f32,
    pub rate: f32,
    pub weight: f32,
    /// Weight per pose slot (see [`Rig::upper_body_mask`]).
    pub mask: Arc<[f32]>,
}

#[derive(Component)]
pub struct Animator {
    pub rig: Arc<Rig>,
    plan: Arc<[EvalStep]>,
    joint_nodes: Arc<[u16]>,
    layers: Vec<Layer>,
    pub overlay: Option<Overlay>,
    /// Global playback speed multiplier.
    pub speed: f32,
    pub paused: bool,
    /// Extract root motion for the topmost layer each frame.
    pub root_motion_enabled: bool,
    /// Root motion of the last update, in model space (see [`RootMotion`]).
    pub root_motion: RootMotion,
    pose: Vec<Trs>,
    scratch: Vec<Trs>,
    model: Vec<Affine3A>,
    /// Model-space matrix of each joint (same order as the composition's joints).
    pub joints: Vec<Affine3A>,
}

fn smooth(x: f32) -> f32 {
    let x = x.clamp(0.0, 1.0);
    x * x * (3.0 - 2.0 * x)
}

impl Animator {
    pub fn new(rig: Arc<Rig>, plan: Arc<[EvalStep]>, joint_nodes: Arc<[u16]>) -> Self {
        let slots = rig.slot_node.len();
        let nodes = rig.names.len();
        let mut model = vec![Affine3A::IDENTITY; nodes];
        let pose = rig.rest.clone();
        crate::rig::evaluate(&plan, &pose, &mut model);
        let joints = joint_nodes.iter().map(|&n| model[n as usize]).collect();
        Self {
            rig,
            plan,
            joint_nodes,
            layers: Vec::with_capacity(MAX_LAYERS),
            overlay: None,
            speed: 1.0,
            paused: false,
            root_motion_enabled: true,
            root_motion: RootMotion::default(),
            scratch: vec![Trs::IDENTITY; slots],
            pose,
            model,
            joints,
        }
    }

    /// Starts `motion`, cross-fading from whatever is playing. The first motion starts
    /// immediately. `time` is the start offset in seconds.
    pub fn play(&mut self, motion: Motion, fade: f32, rate: f32, time: f32) {
        if self.layers.is_empty() || fade <= 1e-4 {
            self.layers.clear();
            self.layers.push(Layer {
                motion,
                time,
                rate,
                fade: 1.0,
                fade_len: 0.0,
            });
            return;
        }
        if self.layers.len() >= MAX_LAYERS {
            // Drop the oldest layers; the visible blend is dominated by the newest ones.
            self.layers.remove(0);
            if let Some(first) = self.layers.first_mut() {
                first.fade = 1.0;
            }
        }
        self.layers.push(Layer {
            motion,
            time,
            rate,
            fade: 0.0,
            fade_len: fade,
        });
    }

    /// Name of the topmost (target) clip.
    pub fn current_name(&self) -> Option<&str> {
        self.layers.last().map(|l| l.motion.clip.name.as_str())
    }

    pub fn current_clip(&self) -> Option<&Arc<Clip>> {
        self.layers.last().map(|l| &l.motion.clip)
    }

    pub fn current_time(&self) -> f32 {
        self.layers.last().map_or(0.0, |l| l.time)
    }

    /// Playback rate of the topmost layer (speed sync).
    pub fn set_rate(&mut self, rate: f32) {
        if let Some(l) = self.layers.last_mut() {
            l.rate = rate;
        }
    }

    /// Jump the topmost layer to `time` (deterministic screenshots, scrubbing).
    pub fn seek(&mut self, time: f32) {
        if let Some(l) = self.layers.last_mut() {
            l.time = time;
        }
    }

    /// True once a non-looping topmost clip has reached its end (and is fully faded in).
    pub fn finished(&self) -> bool {
        self.layers.last().is_some_and(|l| {
            !l.motion.clip.looping && l.time >= l.motion.clip.duration && l.fade >= 1.0
        })
    }

    pub fn is_blending(&self) -> bool {
        self.layers.len() > 1
    }

    pub fn set_overlay(&mut self, overlay: Option<Overlay>) {
        self.overlay = overlay;
    }

    /// Advances time, evaluates the pose and fills `joints`.
    pub fn update(&mut self, dt: f32) {
        self.root_motion = RootMotion::default();
        if self.layers.is_empty() {
            return;
        }
        if !self.paused && dt > 0.0 {
            let dt = dt * self.speed;
            let top = self.layers.len() - 1;
            for (i, l) in self.layers.iter_mut().enumerate() {
                let before = l.time;
                l.time += dt * l.rate;
                if !l.motion.clip.looping {
                    l.time = l.time.min(l.motion.clip.duration.max(0.0));
                }
                if i == top && self.root_motion_enabled {
                    self.root_motion = RootMotionSource::delta(
                        &self.rig,
                        &l.motion.binding,
                        &l.motion.clip,
                        before,
                        l.time,
                    );
                }
                if i > 0 {
                    l.fade = if l.fade_len > 1e-4 {
                        (l.fade + dt / l.fade_len).min(1.0)
                    } else {
                        1.0
                    };
                }
            }
            if let Some(o) = &mut self.overlay {
                o.time += dt * o.rate;
            }
            // A completed fade makes everything below it irrelevant.
            if let Some(full) = self.layers.iter().rposition(|l| l.fade >= 1.0) {
                if full > 0 {
                    self.layers.drain(..full);
                    self.layers[0].fade = 1.0;
                }
            }
        }
        self.evaluate();
    }

    /// Re-evaluates the pose from the current times without advancing them.
    pub fn evaluate(&mut self) {
        if self.layers.is_empty() {
            return;
        }
        let rest = &self.rig.rest;
        {
            let l = &self.layers[0];
            let t = l.motion.clip.wrap(l.time);
            l.motion.binding.sample(&l.motion.clip, rest, t, &mut self.pose);
        }
        for l in &self.layers[1..] {
            let t = l.motion.clip.wrap(l.time);
            l.motion
                .binding
                .sample(&l.motion.clip, rest, t, &mut self.scratch);
            let w = smooth(l.fade);
            for (p, s) in self.pose.iter_mut().zip(&self.scratch) {
                *p = p.blend(*s, w);
            }
        }
        if let Some(o) = &self.overlay {
            if o.weight > 1e-3 {
                let t = o.motion.clip.wrap(o.time);
                o.motion
                    .binding
                    .sample(&o.motion.clip, rest, t, &mut self.scratch);
                for ((p, s), m) in self.pose.iter_mut().zip(&self.scratch).zip(o.mask.iter()) {
                    let w = o.weight * m;
                    if w > 1e-3 {
                        *p = p.blend(*s, w);
                    }
                }
            }
        }
        crate::rig::evaluate(&self.plan, &self.pose, &mut self.model);
        for (j, &n) in self.joint_nodes.iter().enumerate() {
            self.joints[j] = self.model[n as usize];
        }
    }

    /// Model-space matrix of a node of the rig (valid for planned nodes only).
    pub fn node_model(&self, node: u16) -> Affine3A {
        self.model[node as usize]
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{clip::tests::clip_with_track, rig::tests::synthetic};
    use bevy::math::Vec3;

    fn make(rig: &Arc<Rig>, clip: Clip) -> Motion {
        let binding = Arc::new(ClipBinding::new(rig, &clip));
        Motion {
            clip: Arc::new(clip),
            binding,
        }
    }

    fn animator() -> (Animator, Arc<Rig>) {
        let rig = Arc::new(synthetic());
        let plan: Arc<[EvalStep]> = rig.plan(&[1, 3, 4]).into();
        let joints: Arc<[u16]> = vec![1u16, 3, 4].into();
        (Animator::new(rig.clone(), plan, joints), rig)
    }

    #[test]
    fn crossfade_reaches_target_and_drops_old_layers() {
        let (mut a, rig) = animator();
        let m1 = make(
            &rig,
            clip_with_track(vec![(0.0, Vec3::new(0.0, 0.0, 0.0)), (10.0, Vec3::ZERO)], "a", "Test_Hip_1"),
        );
        let m2 = make(
            &rig,
            clip_with_track(vec![(0.0, Vec3::new(0.0, 0.0, 100.0)), (10.0, Vec3::new(0.0, 0.0, 100.0))], "b", "Test_Hip_1"),
        );
        a.play(m1, 0.2, 1.0, 0.0);
        a.update(0.0);
        a.play(m2, 0.2, 1.0, 0.0);
        assert!(a.is_blending());
        a.update(0.1); // halfway: smoothstep(0.5) = 0.5
        let z = a.joints[0].translation.z;
        assert!((z - 50.0).abs() < 1.0, "z={z}");
        a.update(0.2);
        assert!(!a.is_blending());
        assert!((a.joints[0].translation.z - 100.0).abs() < 1e-3);
    }

    #[test]
    fn one_shot_clip_finishes_and_holds() {
        let (mut a, rig) = animator();
        let m = make(
            &rig,
            clip_with_track(vec![(0.0, Vec3::ZERO), (0.5, Vec3::X * 5.0)], "s", "Test_Hip_1"),
        );
        a.play(m, 0.2, 1.0, 0.0);
        assert!(!a.finished());
        a.update(1.0);
        assert!(a.finished());
        assert!((a.joints[0].translation.x - 5.0).abs() < 1e-4);
    }

    #[test]
    fn overlay_respects_mask() {
        let (mut a, rig) = animator();
        let base = make(&rig, clip_with_track(vec![(0.0, Vec3::ZERO), (1.0, Vec3::ZERO)], "b", "Test_Hip_1"));
        a.play(base, 0.2, 1.0, 0.0);
        let over = make(&rig, clip_with_track(vec![(0.0, Vec3::new(0.0, 0.0, 30.0)), (1.0, Vec3::new(0.0, 0.0, 30.0))], "o", "Test_Hip_1"));
        a.set_overlay(Some(Overlay {
            motion: over,
            time: 0.0,
            rate: 1.0,
            weight: 1.0,
            mask: vec![0.0, 0.0].into(),
        }));
        a.update(0.01);
        assert!(a.joints[0].translation.z.abs() < 1e-4);
        if let Some(o) = &mut a.overlay {
            o.mask = vec![1.0, 0.0].into();
        }
        a.update(0.01);
        assert!((a.joints[0].translation.z - 30.0).abs() < 1e-3);
    }
}
