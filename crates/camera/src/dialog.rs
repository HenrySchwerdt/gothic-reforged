//! Dialog camera: shot geometry, shot selection and validity (behavioural port).
//!
//! Port of the geometry in `gCCameraAI_PS::GetCurrentWishedTranslation` (GameDll:0x10023190,
//! dialog branch), `FindPreset` (0x1002d1a0), `DialogPresetIsValid` (0x1002dca0) and
//! `StartDialogCamera` (0x1002cfc0). Which actor anchors which preset group is reconstructed from
//! control flow, not recovered verbatim.
use crate::math::{ease, lerp2};
use crate::preset::{CameraPreset, PresetKey, PresetTable};
use crate::world::CameraWorld;
use glam::{Quat, Vec3};

/// Seconds of the slow drift between the two framings of a "Lerp" preset.
pub const LERP_SECONDS: f32 = 18.0;
/// Local node box volume above which an actor counts as a large creature (about 64^3).
pub const LARGE_VOLUME: f32 = 262_144.0;
/// Below this speaker/listener distance the nearer shoulder shots are allowed.
pub const NEAR_SHOT_DISTANCE: f32 = 2000.0;
/// Probability thresholds used by the shot draw.
pub const SHOULDER_CHANCE: f32 = 0.3;
pub const FACE2_CHANCE: f32 = 0.25;
/// Listener closer than this (horizontally) to the speaker: the camera sits at the speaker's head.
pub const HEAD_FALLBACK_DISTANCE: f32 = 15.0;

/// One conversation partner as seen by the camera.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct DialogActor {
    /// Stable id of the entity (used to tell whether the same partner repeats).
    pub id: u64,
    /// World position of the entity origin.
    pub origin: Vec3,
    /// World position of the head.
    pub head: Vec3,
    /// Volume of the local node box (cm^3), for the large-creature rule.
    pub volume: f32,
    /// Height of the local node box (cm).
    pub box_height: f32,
}

impl DialogActor {
    /// Audio speaker point: origin plus the box height, capped at 350 cm
    /// (`UpdateSpeakerPosition`, GameDll:0x10025ff0).
    pub fn speaker_point(&self) -> Vec3 {
        self.origin + Vec3::Y * self.box_height.min(350.0)
    }
}

/// Camera pose of a dialog shot.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ShotPose {
    pub position: Vec3,
    pub look_at: Vec3,
}

fn rot_y(v: Vec3, deg: f32) -> Vec3 {
    Quat::from_rotation_y(deg.to_radians()) * v
}

/// Whether the shot is a close-up on the speaker (camera in front of the speaker's face).
fn is_face(key: PresetKey) -> bool {
    matches!(key, PresetKey::DialogFaceL | PresetKey::DialogFaceR)
}

/// Whether the look target is the midpoint of the pair (wide two-shots).
fn is_two_shot(key: PresetKey) -> bool {
    matches!(
        key,
        PresetKey::DialogFace2L
            | PresetKey::DialogFace2R
            | PresetKey::DialogSideL
            | PresetKey::DialogSideR
    )
}

/// The partner preset a Lerp preset drifts toward (1<->2, 3<->4).
pub fn lerp_partner(key: PresetKey) -> Option<PresetKey> {
    match key {
        PresetKey::DialogLerp1 => Some(PresetKey::DialogLerp2),
        PresetKey::DialogLerp3 => Some(PresetKey::DialogLerp4),
        _ => None,
    }
}

/// Pose of one preset: the speaker->listener axis is turned by `BestAzi` about Y (positive puts the
/// camera to the right of the axis) and pitched down by `BestElev`, then scaled by `BestRange`.
pub fn single_pose(
    preset: &CameraPreset,
    key: PresetKey,
    listener: &DialogActor,
    speaker: &DialogActor,
) -> ShotPose {
    let axis = speaker.head - listener.head;
    let flat = Vec3::new(axis.x, 0.0, axis.z).normalize_or(Vec3::NEG_Z);
    let el = preset.best_elev.to_radians();
    let turned = |d: Vec3| {
        let r = rot_y(d, preset.best_azi);
        Vec3::new(r.x * el.cos(), -el.sin(), r.z * el.cos())
    };
    if is_face(key) {
        // In front of the speaker's face, looking at it.
        let toward_listener = turned(-flat);
        // `turned` pitches the vector down; the camera must be raised, so flip the vertical part.
        let v = Vec3::new(toward_listener.x, -toward_listener.y, toward_listener.z);
        ShotPose {
            position: speaker.head + v * preset.best_range,
            look_at: speaker.head,
        }
    } else {
        let fwd = turned(flat);
        let look_at = if is_two_shot(key) {
            (listener.head + speaker.head) * 0.5
        } else {
            speaker.head
        };
        ShotPose {
            position: listener.head - fwd * preset.best_range,
            look_at,
        }
    }
}

/// Pose including the Lerp blend over [`LERP_SECONDS`] and the head fallback for partners standing
/// on top of each other.
pub fn shot_pose(
    table: &PresetTable,
    key: PresetKey,
    listener: &DialogActor,
    speaker: &DialogActor,
    timer: f32,
) -> ShotPose {
    let flat = Vec3::new(
        speaker.origin.x - listener.origin.x,
        0.0,
        speaker.origin.z - listener.origin.z,
    );
    if flat.length() < HEAD_FALLBACK_DISTANCE {
        return ShotPose {
            position: speaker.head,
            look_at: speaker.head + Vec3::NEG_Z,
        };
    }
    let a = single_pose(table.get(key), key, listener, speaker);
    match lerp_partner(key) {
        Some(p) => {
            let b = single_pose(table.get(p), p, listener, speaker);
            let t = ease(timer / LERP_SECONDS);
            ShotPose {
                position: a.position.lerp(b.position, t),
                look_at: a.look_at.lerp(b.look_at, t),
            }
        }
        None => a,
    }
}

/// Field of view of a dialog shot: the preset minimum.
pub fn shot_fov(preset: &CameraPreset) -> f32 {
    lerp2(preset.fov, 0.0)
}

/// Whether a shot puts the camera inside geometry or behind a wall: the boom from the anchor and
/// the line of sight to the target must be free.
pub fn shot_is_valid(
    table: &PresetTable,
    key: PresetKey,
    listener: &DialogActor,
    speaker: &DialogActor,
    world: &dyn CameraWorld,
) -> bool {
    let pose = shot_pose(table, key, listener, speaker, 0.0);
    let anchor = if is_face(key) {
        speaker.head
    } else {
        listener.head
    };
    let boom = pose.position - anchor;
    let len = boom.length();
    if len > 1.0 && world.ray_nearest(anchor, boom / len, len + 20.0).is_some() {
        return false;
    }
    let sight = pose.look_at - pose.position;
    let sl = sight.length();
    sl < 1.0
        || world
            .ray_nearest(pose.position, sight / sl, (sl - 60.0).max(0.0))
            .is_none()
}

/// Deterministic xorshift generator (shot draws must be reproducible in tests).
#[derive(Debug, Clone)]
pub struct Rng(u32);

impl Rng {
    pub fn new(seed: u32) -> Self {
        Self(seed.max(1))
    }
    /// Uniform float in `[0, 1)`.
    pub fn next_f32(&mut self) -> f32 {
        let mut x = self.0;
        x ^= x << 13;
        x ^= x >> 17;
        x ^= x << 5;
        self.0 = x;
        (x >> 8) as f32 / (1u32 << 24) as f32
    }
    /// Uniform float in `[-1, 1)`.
    pub fn next_signed(&mut self) -> f32 {
        self.next_f32() * 2.0 - 1.0
    }
}

/// Inputs of a shot choice.
#[derive(Debug, Clone, Copy)]
pub struct ShotChoice {
    /// First cut of this conversation (favours the slow Lerp opening).
    pub first_time: bool,
    /// Camera side flag: `true` picks the `R` variants.
    pub right: bool,
    /// Preset used by the previous cut (never repeated).
    pub previous: Option<PresetKey>,
}

/// Choose a preset for a cut (`FindPreset`).
///
/// Large creatures and far partners force the side shots; otherwise a draw below
/// [`SHOULDER_CHANCE`] prefers over-the-shoulder, below [`FACE2_CHANCE`] a two-shot, else a face
/// close-up. Invalid candidates fall through the list; `None` when nothing is valid.
pub fn find_preset(
    table: &PresetTable,
    choice: ShotChoice,
    listener: &DialogActor,
    speaker: &DialogActor,
    world: &dyn CameraWorld,
    rng: &mut Rng,
) -> Option<PresetKey> {
    use PresetKey::*;
    let big = listener.volume > LARGE_VOLUME || speaker.volume > LARGE_VOLUME;
    let near = (listener.origin - speaker.origin).length() < NEAR_SHOT_DISTANCE;
    let pick = |l: PresetKey, r: PresetKey| if choice.right { r } else { l };
    let mut order: Vec<PresetKey> = Vec::with_capacity(8);
    let draw = rng.next_f32();
    if choice.first_time && !big && near && draw < SHOULDER_CHANCE {
        order.push(if rng.next_f32() < 0.5 {
            DialogLerp1
        } else {
            DialogLerp3
        });
    }
    if big || !near {
        order.extend([
            pick(DialogSideL, DialogSideR),
            pick(DialogShoulder2L, DialogShoulder2R),
        ]);
    } else if draw < SHOULDER_CHANCE {
        order.extend([
            pick(DialogShoulder1L, DialogShoulder1R),
            pick(DialogShoulder2L, DialogShoulder2R),
            pick(DialogFace2L, DialogFace2R),
        ]);
    } else if rng.next_f32() < FACE2_CHANCE {
        order.extend([
            pick(DialogFace2L, DialogFace2R),
            pick(DialogFaceL, DialogFaceR),
            pick(DialogShoulder1L, DialogShoulder1R),
        ]);
    } else {
        order.extend([
            pick(DialogFaceL, DialogFaceR),
            pick(DialogShoulder1L, DialogShoulder1R),
            pick(DialogFace2L, DialogFace2R),
        ]);
    }
    order.extend([
        pick(DialogSideL, DialogSideR),
        pick(DialogShoulder2L, DialogShoulder2R),
    ]);
    order
        .into_iter()
        .filter(|k| Some(*k) != choice.previous)
        .find(|k| shot_is_valid(table, *k, listener, speaker, world))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::world::NoWorld;

    fn pair() -> (DialogActor, DialogActor) {
        let l = DialogActor {
            id: 1,
            origin: Vec3::ZERO,
            head: Vec3::new(0.0, 170.0, 0.0),
            volume: 100_000.0,
            box_height: 200.0,
        };
        let s = DialogActor {
            id: 2,
            origin: Vec3::new(0.0, 0.0, -300.0),
            head: Vec3::new(0.0, 170.0, -300.0),
            ..l
        };
        (l, s)
    }

    #[test]
    fn shoulder_camera_sits_behind_the_listener_at_the_preset_range() {
        let t = PresetTable::default();
        let (l, s) = pair();
        let p = single_pose(
            t.get(PresetKey::DialogShoulder1R),
            PresetKey::DialogShoulder1R,
            &l,
            &s,
        );
        // Range 200, azimuth +40, elevation 25: camera to the right (+x) and behind (+z) and above.
        assert!((p.position - l.head).length() - 200.0 < 0.01);
        assert!(p.position.x > 0.0 && p.position.z > 0.0 && p.position.y > l.head.y);
        assert!((p.position.y - l.head.y - 200.0 * 25.0_f32.to_radians().sin()).abs() < 0.01);
        assert_eq!(p.look_at, s.head);
    }

    #[test]
    fn mirrored_presets_mirror_the_camera() {
        let t = PresetTable::default();
        let (l, s) = pair();
        let a = single_pose(
            t.get(PresetKey::DialogShoulder1L),
            PresetKey::DialogShoulder1L,
            &l,
            &s,
        );
        let b = single_pose(
            t.get(PresetKey::DialogShoulder1R),
            PresetKey::DialogShoulder1R,
            &l,
            &s,
        );
        assert!((a.position.x + b.position.x).abs() < 1e-3);
        assert!((a.position.z - b.position.z).abs() < 1e-3);
    }

    #[test]
    fn face_close_up_is_in_front_of_the_speaker() {
        let t = PresetTable::default();
        let (l, s) = pair();
        let p = single_pose(
            t.get(PresetKey::DialogFaceL),
            PresetKey::DialogFaceL,
            &l,
            &s,
        );
        assert!((p.position - Vec3::new(0.0, 170.0, -100.0)).length() < 0.01);
        assert_eq!(p.look_at, s.head);
    }

    #[test]
    fn lerp_blends_over_eighteen_seconds() {
        let t = PresetTable::default();
        let (l, s) = pair();
        let start = shot_pose(&t, PresetKey::DialogLerp1, &l, &s, 0.0);
        let mid = shot_pose(&t, PresetKey::DialogLerp1, &l, &s, 9.0);
        let end = shot_pose(&t, PresetKey::DialogLerp1, &l, &s, 30.0);
        let a = single_pose(
            t.get(PresetKey::DialogLerp1),
            PresetKey::DialogLerp1,
            &l,
            &s,
        );
        let b = single_pose(
            t.get(PresetKey::DialogLerp2),
            PresetKey::DialogLerp2,
            &l,
            &s,
        );
        assert!((start.position - a.position).length() < 1e-3);
        assert!((end.position - b.position).length() < 1e-3);
        assert!((mid.position - a.position.lerp(b.position, 0.5)).length() < 1e-3);
    }

    #[test]
    fn partners_on_top_of_each_other_use_the_speaker_head() {
        let t = PresetTable::default();
        let (l, mut s) = pair();
        s.origin = l.origin + Vec3::new(5.0, 0.0, 0.0);
        let p = shot_pose(&t, PresetKey::DialogFaceL, &l, &s, 0.0);
        assert_eq!(p.position, s.head);
    }

    #[test]
    fn large_creature_forces_side_shots_and_never_repeats() {
        let t = PresetTable::default();
        let (mut l, s) = pair();
        l.volume = LARGE_VOLUME * 2.0;
        let mut rng = Rng::new(7);
        let choice = ShotChoice {
            first_time: false,
            right: false,
            previous: None,
        };
        let k = find_preset(&t, choice, &l, &s, &NoWorld, &mut rng).unwrap();
        assert_eq!(k, PresetKey::DialogSideL);
        let again = find_preset(
            &t,
            ShotChoice {
                previous: Some(k),
                ..choice
            },
            &l,
            &s,
            &NoWorld,
            &mut rng,
        )
        .unwrap();
        assert_ne!(again, k);
    }

    #[test]
    fn draw_frequencies_follow_the_thresholds() {
        let t = PresetTable::default();
        let (l, s) = pair();
        let mut rng = Rng::new(12345);
        let (mut shoulder, mut face2, mut face) = (0, 0, 0);
        for _ in 0..2000 {
            let k = find_preset(
                &t,
                ShotChoice {
                    first_time: false,
                    right: true,
                    previous: None,
                },
                &l,
                &s,
                &NoWorld,
                &mut rng,
            )
            .unwrap();
            match k {
                PresetKey::DialogShoulder1R => shoulder += 1,
                PresetKey::DialogFace2R => face2 += 1,
                PresetKey::DialogFaceR => face += 1,
                other => panic!("{other:?}"),
            }
        }
        // 0.3 shoulder; of the remaining 0.7, a quarter two-shots.
        assert!((500..700).contains(&shoulder), "{shoulder}");
        assert!((250..450).contains(&face2), "{face2}");
        assert!(face > 800, "{face}");
    }
}
