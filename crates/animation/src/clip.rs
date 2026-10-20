//! Compact, fast-to-sample motion clips converted from `.xmot` data.
//!
//! Keys are stored in structure-of-arrays form. Almost every installed motion has keys on a
//! uniform 25 Hz grid; such tracks are indexed in O(1) (`floor((t - t0) / dt)`), the rest use
//! a binary search over the (short) time array. Quaternion keys are normalised and made
//! sign-continuous at load time so playback only needs a plain nlerp.
use crate::{
    pose::{Trs, nlerp, quat_from_file, yaw_of},
    rig::{NONE, Rig},
};
use bevy::math::{Quat, Vec3};
use gothic_formats::xmot::Motion;
use std::collections::HashMap;

#[derive(Debug, Clone)]
pub struct Track<T> {
    times: Box<[f32]>,
    values: Box<[T]>,
    t0: f32,
    /// Key spacing when the keys are on a uniform grid, otherwise 0.
    dt: f32,
}

impl<T: Copy> Track<T> {
    fn new(keys: Vec<(f32, T)>) -> Self {
        let times: Vec<f32> = keys.iter().map(|k| k.0).collect();
        let values: Vec<T> = keys.iter().map(|k| k.1).collect();
        let t0 = times.first().copied().unwrap_or(0.0);
        let mut dt = 0.0;
        if times.len() >= 2 {
            let candidate = (times[times.len() - 1] - t0) / (times.len() - 1) as f32;
            if candidate > 1e-5
                && times
                    .iter()
                    .enumerate()
                    .all(|(i, &t)| (t - (t0 + i as f32 * candidate)).abs() < candidate * 0.02)
            {
                dt = candidate;
            }
        }
        Track {
            times: times.into_boxed_slice(),
            values: values.into_boxed_slice(),
            t0,
            dt,
        }
    }

    pub fn len(&self) -> usize {
        self.times.len()
    }

    pub fn is_empty(&self) -> bool {
        self.times.is_empty()
    }

    /// Key pair and blend factor for time `t` (clamped to the key range).
    #[inline]
    fn locate(&self, t: f32) -> Option<(T, T, f32)> {
        let n = self.times.len();
        match n {
            0 => None,
            1 => Some((self.values[0], self.values[0], 0.0)),
            _ => {
                if t <= self.t0 {
                    return Some((self.values[0], self.values[0], 0.0));
                }
                if t >= self.times[n - 1] {
                    let v = self.values[n - 1];
                    return Some((v, v, 0.0));
                }
                let i = if self.dt > 0.0 {
                    (((t - self.t0) / self.dt) as usize).min(n - 2)
                } else {
                    self.times.partition_point(|&k| k <= t).saturating_sub(1).min(n - 2)
                };
                let span = (self.times[i + 1] - self.times[i]).max(1e-6);
                let f = ((t - self.times[i]) / span).clamp(0.0, 1.0);
                Some((self.values[i], self.values[i + 1], f))
            }
        }
    }
}

impl Track<Vec3> {
    #[inline]
    pub fn sample(&self, t: f32) -> Option<Vec3> {
        self.locate(t).map(|(a, b, f)| a.lerp(b, f))
    }
}

impl Track<Quat> {
    #[inline]
    pub fn sample(&self, t: f32) -> Option<Quat> {
        self.locate(t).map(|(a, b, f)| nlerp(a, b, f))
    }
}

#[derive(Debug, Clone)]
pub struct ClipNode {
    pub name: String,
    pub pos: Track<Vec3>,
    pub rot: Track<Quat>,
    pub scale: Track<Vec3>,
    /// Static pose of the node record, used for channels without keys.
    pub rest: Trs,
}

impl ClipNode {
    #[inline]
    pub fn sample(&self, t: f32) -> Trs {
        Trs {
            t: self.pos.sample(t).unwrap_or(self.rest.t),
            r: self.rot.sample(t).unwrap_or(self.rest.r),
            s: self.scale.sample(t).unwrap_or(self.rest.s),
        }
    }
}

#[derive(Debug, Clone)]
pub struct Clip {
    pub name: String,
    pub duration: f32,
    pub nodes: Vec<ClipNode>,
    /// Plays cyclically (set by the library from the motion name grammar).
    pub looping: bool,
    /// Nominal ground speed in cm/s from the trailing number of the motion name (0 if none).
    pub nominal_speed: f32,
    pub by_name: HashMap<String, u16>,
}

impl Clip {
    pub fn from_motion(name: &str, motion: &Motion) -> Self {
        let nodes: Vec<ClipNode> = motion
            .nodes
            .iter()
            .map(|n| {
                let mut rot: Vec<(f32, Quat)> = n
                    .rotations
                    .keys
                    .iter()
                    .map(|&(t, q)| (t, quat_from_file(q)))
                    .collect();
                for i in 1..rot.len() {
                    if rot[i - 1].1.dot(rot[i].1) < 0.0 {
                        rot[i].1 = -rot[i].1;
                    }
                }
                ClipNode {
                    name: n.name.clone(),
                    pos: Track::new(
                        n.positions
                            .keys
                            .iter()
                            .map(|&(t, p)| (t, Vec3::from(p)))
                            .collect(),
                    ),
                    rot: Track::new(rot),
                    scale: Track::new(
                        n.scales
                            .keys
                            .iter()
                            .map(|&(t, s)| (t, Vec3::from(s)))
                            .collect(),
                    ),
                    rest: Trs {
                        t: Vec3::from(n.position),
                        r: quat_from_file(n.rotation),
                        s: Vec3::from(n.scale),
                    },
                }
            })
            .collect();
        let mut by_name = HashMap::new();
        for (i, n) in nodes.iter().enumerate().rev() {
            by_name.insert(n.name.clone(), i as u16);
        }
        Clip {
            name: name.to_string(),
            duration: motion.duration,
            nodes,
            looping: false,
            nominal_speed: crate::names::trailing_speed(name),
            by_name,
        }
    }

    /// Wraps `t` into the clip range for looping clips, clamps otherwise.
    #[inline]
    pub fn wrap(&self, t: f32) -> f32 {
        if self.duration <= 1e-4 {
            0.0
        } else if self.looping {
            t.rem_euclid(self.duration)
        } else {
            t.clamp(0.0, self.duration)
        }
    }

    pub fn is_pose_only(&self) -> bool {
        self.duration <= 1e-4
    }
}

/// Mapping from a rig's pose slots to a clip's nodes.
#[derive(Debug, Clone)]
pub struct ClipBinding {
    /// Clip node index per rig slot (`NONE` if the clip does not animate the slot).
    pub node_of_slot: Box<[u16]>,
    /// Slot of the root bone (animated node without animated ancestor) if the clip drives it.
    pub root_slot: u16,
    pub bound: usize,
}

impl ClipBinding {
    pub fn new(rig: &Rig, clip: &Clip) -> Self {
        let node_of_slot: Vec<u16> = rig
            .slot_node
            .iter()
            .map(|&n| {
                clip.by_name
                    .get(&rig.names[n as usize])
                    .copied()
                    .unwrap_or(NONE)
            })
            .collect();
        let bound = node_of_slot.iter().filter(|&&n| n != NONE).count();
        let root_slot = rig
            .slot_node
            .iter()
            .enumerate()
            .find(|&(slot, &n)| rig.eff_parent[n as usize] == NONE && node_of_slot[slot] != NONE)
            .map_or(NONE, |(slot, _)| slot as u16);
        ClipBinding {
            node_of_slot: node_of_slot.into_boxed_slice(),
            root_slot,
            bound,
        }
    }

    /// Samples the clip at (already wrapped) time `t` into `out` (one entry per slot);
    /// slots the clip does not animate get the rig's rest pose.
    #[inline]
    pub fn sample(&self, clip: &Clip, rest: &[Trs], t: f32, out: &mut [Trs]) {
        for (slot, o) in out.iter_mut().enumerate() {
            let n = self.node_of_slot[slot];
            *o = if n == NONE {
                rest[slot]
            } else {
                clip.nodes[n as usize].sample(t)
            };
        }
    }

    /// Root bone pose (model space) at time `t`.
    pub fn root_pose(&self, clip: &Clip, t: f32) -> Option<Trs> {
        (self.root_slot != NONE).then(|| {
            clip.nodes[self.node_of_slot[self.root_slot as usize] as usize].sample(t)
        })
    }

    /// Root motion between two times of the same pass over the clip (`t1 >= t0`, wrapped
    /// passes are handled by [`RootMotionSource::delta`]).
    fn root_delta_linear(&self, clip: &Clip, t0: f32, t1: f32) -> (Vec3, f32) {
        match (self.root_pose(clip, t0), self.root_pose(clip, t1)) {
            (Some(a), Some(b)) => (b.t - a.t, yaw_of(b.r * a.r.inverse())),
            _ => (Vec3::ZERO, 0.0),
        }
    }
}

/// Root motion produced during one frame, in the character's model space.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct RootMotion {
    /// Horizontal translation (y is zero), centimetres.
    pub translation: Vec3,
    /// Rotation about the vertical axis, radians (positive is counter-clockwise from above).
    pub yaw: f32,
}

pub struct RootMotionSource;

impl RootMotionSource {
    /// Root motion of `clip` between playback times `from` and `to` (unwrapped, may cross the
    /// loop point). Clips whose root bone does not travel (all walk/run cycles are in place)
    /// report zero; the nominal speed in [`Clip::nominal_speed`] covers those.
    pub fn delta(binding: &ClipBinding, clip: &Clip, from: f32, to: f32) -> RootMotion {
        if clip.duration <= 1e-4 || (to - from).abs() < 1e-7 {
            return RootMotion::default();
        }
        let (a, b) = if to >= from { (from, to) } else { (to, from) };
        let sign = if to >= from { 1.0 } else { -1.0 };
        let (trans, yaw);
        if clip.looping {
            let ta = a.rem_euclid(clip.duration);
            let tb = ta + (b - a);
            if tb <= clip.duration {
                (trans, yaw) = binding.root_delta_linear(clip, ta, tb);
            } else {
                let (t1, y1) = binding.root_delta_linear(clip, ta, clip.duration);
                let (t2, y2) = binding.root_delta_linear(clip, 0.0, tb - clip.duration);
                (trans, yaw) = (t1 + t2, y1 + y2);
            }
        } else {
            let (ta, tb) = (a.clamp(0.0, clip.duration), b.clamp(0.0, clip.duration));
            (trans, yaw) = binding.root_delta_linear(clip, ta, tb);
        }
        // Model-space horizontal translation; the root bone's own up axis is world up.
        RootMotion {
            translation: Vec3::new(trans.x, 0.0, trans.z) * sign,
            yaw: yaw * sign,
        }
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    pub fn clip_with_track(keys: Vec<(f32, Vec3)>, name: &str, node: &str) -> Clip {
        let n = ClipNode {
            name: node.to_string(),
            pos: Track::new(keys),
            rot: Track::new(vec![]),
            scale: Track::new(vec![]),
            rest: Trs::IDENTITY,
        };
        let duration = n.pos.times.last().copied().unwrap_or(0.0);
        let mut by_name = HashMap::new();
        by_name.insert(node.to_string(), 0);
        Clip {
            name: name.into(),
            duration,
            nodes: vec![n],
            looping: false,
            nominal_speed: 0.0,
            by_name,
        }
    }

    #[test]
    fn uniform_track_indexes_directly_and_interpolates() {
        let keys: Vec<(f32, Vec3)> = (0..11).map(|i| (i as f32 * 0.04, Vec3::splat(i as f32))).collect();
        let track = Track::new(keys);
        assert!(track.dt > 0.0);
        let v = track.sample(0.10).unwrap();
        assert!((v.x - 2.5).abs() < 1e-3);
        assert_eq!(track.sample(-1.0).unwrap().x, 0.0);
        assert_eq!(track.sample(9.0).unwrap().x, 10.0);
        // exactly on a key
        assert!((track.sample(0.2).unwrap().x - 5.0).abs() < 1e-3);
    }

    #[test]
    fn irregular_track_uses_binary_search() {
        let keys = vec![
            (0.0, Vec3::ZERO),
            (0.1, Vec3::X),
            (0.5, Vec3::X * 5.0),
            (2.0, Vec3::X * 20.0),
        ];
        let track = Track::new(keys);
        assert_eq!(track.dt, 0.0);
        assert!((track.sample(0.3).unwrap().x - 3.0).abs() < 1e-4);
        assert!((track.sample(1.25).unwrap().x - 12.5).abs() < 1e-4);
    }

    #[test]
    fn single_key_and_empty_tracks() {
        let one = Track::new(vec![(0.0, Vec3::Y)]);
        assert_eq!(one.sample(5.0).unwrap(), Vec3::Y);
        let none: Track<Vec3> = Track::new(vec![]);
        assert!(none.sample(0.0).is_none());
    }

    #[test]
    fn wrap_loops_and_clamps() {
        let mut clip = clip_with_track(vec![(0.0, Vec3::ZERO), (1.0, Vec3::X)], "c", "n");
        assert_eq!(clip.wrap(1.5), 1.0);
        clip.looping = true;
        assert!((clip.wrap(1.5) - 0.5).abs() < 1e-6);
    }

    #[test]
    fn root_motion_spans_the_loop_point() {
        use crate::rig::tests::synthetic;
        let rig = synthetic();
        // Root bone is slot 0 ("Test_Hip_1") moving 10 cm/s along X.
        let mut clip = clip_with_track(vec![(0.0, Vec3::ZERO), (1.0, Vec3::X * 10.0)], "c", "Test_Hip_1");
        clip.looping = true;
        let binding = ClipBinding::new(&rig, &clip);
        assert_eq!(binding.root_slot, 0);
        let d = RootMotionSource::delta(&binding, &clip, 0.9, 1.2);
        // 0.1 s before the loop point and 0.2 s after: 3 cm.
        assert!((d.translation.x - 3.0).abs() < 1e-3, "{d:?}");
        let back = RootMotionSource::delta(&binding, &clip, 1.2, 0.9);
        assert!((back.translation.x + 3.0).abs() < 1e-3);
    }
}
