//! Per-character playback: the motion-instance stack of the original layer system, an optional
//! masked overlay layer, pose evaluation and root motion. Plain data plus methods; the ECS
//! systems in `runtime.rs` drive it.
//!
//! Layer semantics (E03, ports of the EMotionFX layer system and `eCWrapper_emfx2Actor`):
//! every playing motion is an [`Instance`] with its own weight. Instances are kept in a list
//! sorted by priority (top first, newest first among equals) and combined bottom-up: each one is
//! lerped over everything below it with its weight, the bottom one over the rest pose, and
//! nodes a motion has no track for are left alone. `SwitchFadeOut` gives the old instance the
//! fading priority, so a cross-fade is the old clip fading out on top of the new clip, which
//! starts at full weight underneath.
use crate::{
    blend::{self, Instance, PlayDesc, slot},
    clip::{Clip, ClipBinding, RootMotion, RootMotionSource},
    pose::Trs,
    rig::{EvalStep, Rig},
};
use bevy::{
    ecs::component::Component,
    math::{Affine3A, Quat},
};
use std::sync::Arc;

/// Default cross-fade time in seconds (`eCAnimationAdmin` standard fade).
pub const DEFAULT_FADE: f32 = blend::STANDARD_FADE;
/// Most instances kept at once; the oldest fading ones go first.
const MAX_LAYERS: usize = 8;
/// Root travel below this (cm over the whole clip) counts as in place.
const IN_PLACE_LIMIT: f32 = 20.0;

/// A clip together with its slot mapping for one rig.
#[derive(Clone)]
pub struct Motion {
    pub clip: Arc<Clip>,
    pub binding: Arc<ClipBinding>,
}

#[derive(Clone)]
struct Layer {
    motion: Motion,
    inst: Instance,
    /// Motion slot the instance occupies; `None` once it was moved out by `SwitchFadeOut`.
    slot: Option<u8>,
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
    /// Rotation from the authored skeleton frame to the entity frame (facing fix).
    facing: Affine3A,
    /// Top layer first.
    layers: Vec<Layer>,
    pub overlay: Option<Overlay>,
    /// Global playback speed multiplier (the entity's local time scale).
    pub speed: f32,
    pub paused: bool,
    /// Extract root motion for the primary slot each frame.
    pub root_motion_enabled: bool,
    /// Root motion of the last update, in model space (see [`RootMotion`]).
    pub root_motion: RootMotion,
    pose: Vec<Trs>,
    model: Vec<Affine3A>,
    /// Model-space matrix of each joint (same order as the composition's joints).
    pub joints: Vec<Affine3A>,
    /// Set whenever `joints` changed; the joint writer clears it. Lets unchanged (distant,
    /// idle, off-screen) characters skip the per-joint copies.
    pub joints_dirty: bool,
    /// Time not yet applied (update-rate LOD), seconds.
    pub(crate) pending_dt: f32,
    pub(crate) lod_seeded: bool,
}

/// Unwrapped play position of an instance (loops included), for root motion deltas.
fn total_time(inst: &Instance) -> f32 {
    inst.loop_index.max(0) as f32 * inst.duration + inst.time
}

impl Animator {
    pub fn new(
        rig: Arc<Rig>,
        plan: Arc<[EvalStep]>,
        joint_nodes: Arc<[u16]>,
        facing: Quat,
    ) -> Self {
        let nodes = rig.names.len();
        let mut model = vec![Affine3A::IDENTITY; nodes];
        let pose = rig.rest.clone();
        crate::rig::evaluate(&plan, &pose, &mut model);
        let facing = Affine3A::from_quat(facing);
        let joints = joint_nodes
            .iter()
            .map(|&n| facing * model[n as usize])
            .collect();
        Self {
            rig,
            plan,
            joint_nodes,
            facing,
            layers: Vec::with_capacity(MAX_LAYERS),
            overlay: None,
            speed: 1.0,
            paused: false,
            root_motion_enabled: true,
            root_motion: RootMotion::default(),
            pose,
            model,
            joints,
            joints_dirty: true,
            pending_dt: 0.0,
            lod_seeded: false,
        }
    }

    // --- wrapper API (eCWrapper_emfx2Actor) -------------------------------------------------

    /// Port of eCWrapper_emfx2Actor::PlayMotion (Engine:0x100476f0): stops any instance in the
    /// slot without a fade, then starts the motion. The new instance sorts by priority (0), so
    /// it goes above equal-priority instances and below fading ones.
    pub fn play_slot(&mut self, slot: u8, motion: Motion, desc: &PlayDesc, start_time: f32) {
        self.layers.retain(|l| l.slot != Some(slot));
        let inst = Instance::new(motion.clip.duration, desc, start_time);
        let at = blend::insertion_index(&self.layers, inst.priority, |l| l.inst.priority);
        self.layers.insert(
            at,
            Layer {
                motion,
                inst,
                slot: Some(slot),
            },
        );
        while self.layers.len() > MAX_LAYERS {
            // Drop the lowest fading instance; if there is none, the lowest layer.
            let drop = self
                .layers
                .iter()
                .rposition(|l| l.slot.is_none())
                .unwrap_or(self.layers.len() - 1);
            self.layers.remove(drop);
        }
    }

    /// Port of eCWrapper_emfx2Actor::SwitchFadeOut (Engine:0x10049dd0). The instance leaves its
    /// slot (so a new clip can start there at once) and fades out over `seconds`.
    pub fn switch_fade_out(&mut self, slot: u8, seconds: f32) {
        let Some(i) = self.layers.iter().position(|l| l.slot == Some(slot)) else {
            return;
        };
        self.layers[i].slot = None;
        if !self.layers[i].inst.switch_fade_out(seconds) {
            self.layers.remove(i);
        }
    }

    /// Port of eCWrapper_emfx2Actor::StopMotion (Engine:0x10047910). Zero fade removes the
    /// instance, otherwise it fades out like `SwitchFadeOut`.
    pub fn stop_slot(&mut self, slot: u8, fade: f32) {
        if fade <= 0.0 {
            self.layers.retain(|l| l.slot != Some(slot));
        } else {
            self.switch_fade_out(slot, fade);
        }
    }

    /// Port of eCWrapper_emfx2Actor::StopAtLoopEnd (Engine:0x100479c0).
    pub fn stop_at_loop_end(&mut self, slot: u8) {
        if let Some(l) = self.slot_layer_mut(slot) {
            l.inst.stop_at_loop_end();
        }
    }

    /// Port of eCWrapper_emfx2Actor::SetTargetWeight (Engine:0x10049ca0).
    pub fn set_target_weight(&mut self, slot: u8, weight: f32) {
        if let Some(l) = self.slot_layer_mut(slot) {
            l.inst.set_target_weight(weight);
        }
    }

    /// Port of eCWrapper_emfx2Actor::SetBlendMode (Engine:0x10049d10).
    pub fn set_blend_mode(&mut self, slot: u8, mode: i32) {
        if let Some(l) = self.slot_layer_mut(slot) {
            l.inst.set_blend_mode(mode);
        }
    }

    /// The instance in `slot`, if any.
    pub fn slot_instance(&self, slot: u8) -> Option<&Instance> {
        self.layers
            .iter()
            .find(|l| l.slot == Some(slot))
            .map(|l| &l.inst)
    }

    /// Name of the clip in `slot`.
    pub fn slot_name(&self, slot: u8) -> Option<&str> {
        self.layers
            .iter()
            .find(|l| l.slot == Some(slot))
            .map(|l| l.motion.clip.name.as_str())
    }

    /// Port of eCWrapper_emfx2Actor::IsActorPlaying (Engine:0x1004a040).
    pub fn is_slot_playing(&self, slot: u8) -> bool {
        self.slot_instance(slot).is_some_and(Instance::is_playing)
    }

    fn slot_layer_mut(&mut self, slot: u8) -> Option<&mut Layer> {
        self.layers.iter_mut().find(|l| l.slot == Some(slot))
    }

    /// Number of live instances (fading ones included).
    pub fn instance_count(&self) -> usize {
        self.layers.len()
    }

    // --- primary-slot convenience API -------------------------------------------------------

    /// Starts `motion` in the primary slot at play rate `rate`, fading the previous primary clip
    /// out over `fade` seconds (`SwitchFadeOut`). The first motion starts immediately; `time`
    /// is the start offset in seconds. Clips flagged looping loop forever, others play once and
    /// hold their last pose.
    pub fn play(&mut self, motion: Motion, fade: f32, rate: f32, time: f32) {
        if fade <= 1e-4 {
            self.layers.clear();
        } else {
            self.switch_fade_out(slot::PRIMARY, fade);
        }
        let desc = PlayDesc {
            speed: rate,
            max_loops: if motion.clip.looping { -1 } else { 1 },
            ..PlayDesc::default()
        };
        self.play_slot(slot::PRIMARY, motion, &desc, time);
    }

    fn primary(&self) -> Option<&Layer> {
        self.layers.iter().find(|l| l.slot == Some(slot::PRIMARY))
    }

    /// Name of the primary clip.
    pub fn current_name(&self) -> Option<&str> {
        self.primary().map(|l| l.motion.clip.name.as_str())
    }

    pub fn current_clip(&self) -> Option<&Arc<Clip>> {
        self.primary().map(|l| &l.motion.clip)
    }

    pub fn current_time(&self) -> f32 {
        self.primary().map_or(0.0, |l| l.inst.time)
    }

    /// Playback rate of the primary clip (speed sync). A clip held at its end stays held.
    pub fn set_rate(&mut self, rate: f32) {
        if let Some(l) = self.slot_layer_mut(slot::PRIMARY)
            && !l.inst.at_end()
        {
            l.inst.speed = rate;
        }
    }

    /// Jump the primary clip to `time` (deterministic screenshots, scrubbing).
    pub fn seek(&mut self, time: f32) {
        if let Some(l) = self.slot_layer_mut(slot::PRIMARY) {
            l.inst.time = time.clamp(0.0, l.inst.duration);
            l.inst.loop_index = 0;
            l.inst.ended = false;
        }
    }

    /// True once a one-shot primary clip has played to its end (and is fully faded in).
    pub fn finished(&self) -> bool {
        self.primary()
            .is_some_and(|l| l.inst.at_end() && l.inst.weight >= 0.999)
    }

    pub fn is_blending(&self) -> bool {
        self.layers.len() > 1 || self.primary().is_some_and(|l| l.inst.weight < 0.999)
    }

    pub fn set_overlay(&mut self, overlay: Option<Overlay>) {
        self.overlay = overlay;
    }

    // --- per frame ----------------------------------------------------------------------------

    /// Advances time, evaluates the pose and fills `joints`.
    pub fn update(&mut self, dt: f32) {
        self.root_motion = RootMotion::default();
        if self.layers.is_empty() {
            return;
        }
        if !self.paused && dt > 0.0 {
            let dt = dt * self.speed;
            let root_enabled = self.root_motion_enabled;
            for l in &mut self.layers {
                let primary = l.slot == Some(slot::PRIMARY);
                if primary {
                    l.inst.run_stop_prevention(dt);
                }
                let before = total_time(&l.inst);
                l.inst.advance(dt);
                if primary && root_enabled {
                    let after = total_time(&l.inst);
                    let clip = &l.motion.clip;
                    let mut rm = RootMotionSource::delta(&l.motion.binding, clip, before, after);
                    rm.translation = self.facing.transform_vector3(rm.translation);
                    if l.motion.binding.root_travel < IN_PLACE_LIMIT {
                        // In-place clip: ground travel and turning are implied by the name
                        // (nominal speed and direction, `Turn90`), not stored in the keys.
                        let advanced = (after - before).abs();
                        rm.translation = clip.implied_dir * clip.nominal_speed * (after - before);
                        rm.yaw = if clip.duration > 1e-4 && clip.implied_turn != 0.0 {
                            clip.implied_turn * advanced / clip.duration
                        } else {
                            0.0
                        };
                    }
                    self.root_motion = rm;
                }
            }
            if let Some(o) = &mut self.overlay {
                o.time += dt * o.rate;
            }
            // Faded-out instances and finished non-primary ones are removed.
            self.layers
                .retain(|l| !l.inst.expired && !(l.inst.ended && l.slot != Some(slot::PRIMARY)));
        }
        self.evaluate();
    }

    /// Re-evaluates the pose from the current times without advancing them.
    pub fn evaluate(&mut self) {
        if self.layers.is_empty() {
            return;
        }
        let rest = &self.rig.rest;
        let slots = rest.len();
        // The topmost layer that fully replaces everything below it is where evaluation starts.
        let start = self.layers.iter().position(|l| {
            l.inst.weight >= 0.999 && !l.inst.additive && l.motion.binding.bound == slots
        });
        let from = match start {
            Some(i) => {
                let l = &self.layers[i];
                let t = l.inst.time.min(l.motion.clip.duration);
                l.motion
                    .binding
                    .sample(&l.motion.clip, rest, t, &mut self.pose);
                i
            }
            None => {
                self.pose.copy_from_slice(rest);
                self.layers.len()
            }
        };
        for l in self.layers[..from].iter().rev() {
            let w = l.inst.weight;
            if w <= 1e-4 {
                continue;
            }
            let t = l.inst.time.min(l.motion.clip.duration);
            if l.inst.additive {
                l.motion
                    .binding
                    .add_into(&l.motion.clip, rest, t, w, &mut self.pose);
            } else {
                l.motion
                    .binding
                    .blend_into(&l.motion.clip, t, w, &mut self.pose);
            }
        }
        if let Some(o) = &self.overlay
            && o.weight > 1e-3
        {
            let t = o.motion.clip.wrap(o.time);
            for ((slot, p), m) in self.pose.iter_mut().enumerate().zip(o.mask.iter()) {
                let w = o.weight * m;
                let n = o.motion.binding.node_of_slot[slot];
                if w > 1e-3 && n != crate::rig::NONE {
                    *p = p.blend(o.motion.clip.nodes[n as usize].sample(t), w);
                }
            }
        }
        crate::rig::evaluate(&self.plan, &self.pose, &mut self.model);
        for (j, &n) in self.joint_nodes.iter().enumerate() {
            self.joints[j] = self.facing * self.model[n as usize];
        }
        self.joints_dirty = true;
    }

    /// Model-space matrix of a node of the rig (valid for planned nodes only).
    pub fn node_model(&self, node: u16) -> Affine3A {
        self.model[node as usize]
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::blend::PlayDesc;
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
        (
            Animator::new(rig.clone(), plan, joints, Quat::IDENTITY),
            rig,
        )
    }

    #[test]
    fn crossfade_reaches_target_and_drops_old_layers() {
        let (mut a, rig) = animator();
        let m1 = make(
            &rig,
            clip_with_track(
                vec![(0.0, Vec3::new(0.0, 0.0, 0.0)), (10.0, Vec3::ZERO)],
                "a",
                "Test_Hip_1",
            ),
        );
        let m2 = make(
            &rig,
            clip_with_track(
                vec![
                    (0.0, Vec3::new(0.0, 0.0, 100.0)),
                    (10.0, Vec3::new(0.0, 0.0, 100.0)),
                ],
                "b",
                "Test_Hip_1",
            ),
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
            clip_with_track(
                vec![(0.0, Vec3::ZERO), (0.5, Vec3::X * 5.0)],
                "s",
                "Test_Hip_1",
            ),
        );
        a.play(m, 0.2, 1.0, 0.0);
        assert!(!a.finished());
        a.update(1.0);
        assert!(a.finished());
        assert!((a.joints[0].translation.x - 5.0).abs() < 2e-3);
    }

    #[test]
    fn in_place_clips_report_implied_root_motion() {
        let (mut a, rig) = animator();
        let mut clip = clip_with_track(
            vec![(0.0, Vec3::ZERO), (1.0, Vec3::ZERO)],
            "w",
            "Test_Hip_1",
        );
        clip.looping = true;
        clip.nominal_speed = 100.0;
        clip.implied_dir = crate::locomotion::FORWARD;
        let motion = make(&rig, clip);
        a.play(motion, 0.2, 1.0, 0.0);
        a.update(0.5);
        assert!((a.root_motion.translation - Vec3::new(0.0, 0.0, -50.0)).length() < 1e-3);
        a.set_rate(2.0);
        a.update(0.5);
        assert!((a.root_motion.translation.z + 100.0).abs() < 1e-3);
    }

    #[test]
    fn overlay_respects_mask() {
        let (mut a, rig) = animator();
        let base = make(
            &rig,
            clip_with_track(
                vec![(0.0, Vec3::ZERO), (1.0, Vec3::ZERO)],
                "b",
                "Test_Hip_1",
            ),
        );
        a.play(base, 0.2, 1.0, 0.0);
        let over = make(
            &rig,
            clip_with_track(
                vec![
                    (0.0, Vec3::new(0.0, 0.0, 30.0)),
                    (1.0, Vec3::new(0.0, 0.0, 30.0)),
                ],
                "o",
                "Test_Hip_1",
            ),
        );
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

    fn const_motion(rig: &Arc<Rig>, z: f32, name: &str) -> Motion {
        make(
            rig,
            clip_with_track(
                vec![
                    (0.0, Vec3::new(0.0, 0.0, z)),
                    (10.0, Vec3::new(0.0, 0.0, z)),
                ],
                name,
                "Test_Hip_1",
            ),
        )
    }

    #[test]
    fn fading_clip_sits_on_top_of_the_new_one_and_ramps_linearly() {
        let (mut a, rig) = animator();
        a.play(const_motion(&rig, 0.0, "old"), 0.2, 1.0, 0.0);
        a.update(0.0);
        a.play(const_motion(&rig, 100.0, "new"), 0.2, 1.0, 0.0);
        assert_eq!(a.instance_count(), 2);
        assert_eq!(a.current_name(), Some("new"));
        // Linear weight: after 0.05 s the old clip still has 75 %.
        a.update(0.05);
        assert!((a.joints[0].translation.z - 25.0).abs() < 1e-2);
        a.update(0.1);
        assert!((a.joints[0].translation.z - 75.0).abs() < 1e-2);
        a.update(0.1);
        assert_eq!(a.instance_count(), 1);
    }

    #[test]
    fn newest_equal_priority_slot_is_on_top_and_weights_stack() {
        let (mut a, rig) = animator();
        let full = PlayDesc::default();
        a.play_slot(0, const_motion(&rig, 0.0, "a"), &full, 0.0);
        let half = PlayDesc {
            weight: 0.5,
            ..PlayDesc::default()
        };
        a.play_slot(1, const_motion(&rig, 100.0, "b"), &half, 0.0);
        a.evaluate();
        assert!((a.joints[0].translation.z - 50.0).abs() < 1e-3);
        // SetTargetWeight is instant.
        a.set_target_weight(1, 0.25);
        a.evaluate();
        assert!((a.joints[0].translation.z - 25.0).abs() < 1e-3);
        // Replacing a slot drops the old instance without a fade.
        a.play_slot(1, const_motion(&rig, 40.0, "c"), &full, 0.0);
        a.evaluate();
        assert_eq!(a.instance_count(), 2);
        assert!((a.joints[0].translation.z - 40.0).abs() < 1e-3);
    }

    #[test]
    fn additive_instance_adds_its_offset_from_rest() {
        let (mut a, rig) = animator();
        a.play_slot(0, const_motion(&rig, 10.0, "a"), &PlayDesc::default(), 0.0);
        let desc = PlayDesc {
            weight: 0.5,
            ..PlayDesc::default()
        };
        a.play_slot(1, const_motion(&rig, 30.0, "b"), &desc, 0.0);
        a.set_blend_mode(1, 2);
        a.evaluate();
        // rest of the hip is at the origin, so the offset is 30 and half of it is added.
        assert!((a.joints[0].translation.z - 25.0).abs() < 1e-3);
    }

    #[test]
    fn held_one_shot_is_not_restarted_by_set_rate() {
        let (mut a, rig) = animator();
        let m = make(
            &rig,
            clip_with_track(
                vec![(0.0, Vec3::ZERO), (0.5, Vec3::X * 5.0)],
                "s",
                "Test_Hip_1",
            ),
        );
        a.play(m, 0.2, 1.0, 0.0);
        a.update(1.0);
        assert!(a.finished());
        a.set_rate(2.0);
        a.update(1.0);
        assert!(a.finished());
        assert!((a.current_time() - 0.5).abs() < 1e-2);
    }
}
