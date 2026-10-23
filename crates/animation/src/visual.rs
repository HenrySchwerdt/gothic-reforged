//! Remaining state machines of `eCVisualAnimation_PS` (E04) that need no clip data: the
//! per-slot motion descriptors with their owners, the aim overlay weights, the ragdoll timer
//! and the physics level of detail.
use crate::blend::{MOTION_SLOTS, owner};

// ---------------------------------------------------------------------------------------------
// Motion slots (descriptor array at this+0xe8)
// ---------------------------------------------------------------------------------------------

/// Per-slot descriptor: the motion name set by `SetMotion` and the cooperative owner.
#[derive(Clone, Debug)]
pub struct SlotDesc {
    pub name: String,
    pub owner: u8,
}

/// The eight slot descriptors of a character.
#[derive(Clone, Debug)]
pub struct MotionSlots {
    slots: [SlotDesc; MOTION_SLOTS],
}

impl Default for MotionSlots {
    fn default() -> Self {
        Self {
            slots: std::array::from_fn(|_| SlotDesc {
                name: String::new(),
                owner: owner::FREE,
            }),
        }
    }
}

impl MotionSlots {
    /// Port of eCVisualAnimation_PS::SetMotion (Engine:0x1002bee0).
    pub fn set_motion(&mut self, slot: u8, name: &str) {
        gothic_portcov::hit!("set_motion");
        if let Some(s) = self.slots.get_mut(slot as usize) {
            s.name = name.to_owned();
        }
    }

    pub fn name(&self, slot: u8) -> &str {
        self.slots.get(slot as usize).map_or("", |s| &s.name)
    }

    /// Port of eCVisualAnimation_PS::HasMotion: a motion was set for the slot.
    pub fn has_motion(&self, slot: u8) -> bool {
        gothic_portcov::hit!("has_motion");
        !self.name(slot).is_empty()
    }

    /// Port of eCVisualAnimation_PS::GetMotionOwner (Engine:0x1002bfc0).
    pub fn owner(&self, slot: u8) -> u8 {
        gothic_portcov::hit!("owner");
        self.slots
            .get(slot as usize)
            .map_or(owner::FREE, |s| s.owner)
    }

    /// Port of eCVisualAnimation_PS::SetMotionOwner (Engine:0x1002bfa0).
    pub fn set_owner(&mut self, slot: u8, value: u8) {
        gothic_portcov::hit!("set_owner");
        if let Some(s) = self.slots.get_mut(slot as usize) {
            s.owner = value;
        }
    }

    /// The gate of PlayMotion/StopMotion/StopAtLoopEnd (Engine:0x10030860, 0x10030980,
    /// 0x100309d0): slot below 8 and a non-empty motion name.
    pub fn can_command(&self, slot: u8) -> bool {
        (slot as usize) < MOTION_SLOTS && self.has_motion(slot)
    }

    /// Port of GameReset (Engine:0x1002b700) for the descriptors: names cleared, owners free.
    pub fn reset(&mut self) {
        gothic_portcov::hit!("reset");
        *self = Self::default();
    }
}

/// Port of eCVisualAnimation_PS::SetLocalTimeScale (Engine:0x100301b0) / the frame delta of
/// `eCWrapper_emfx2Actor::Process`: animation time is the application's scaled frame time
/// times the entity's local scale.
pub fn animation_delta(frame_time: f32, local_time_scale: f32) -> f32 {
    gothic_portcov::hit!("animation_delta");
    frame_time * local_time_scale
}

// ---------------------------------------------------------------------------------------------
// Aim overlay (UpdateAim, Engine:0x10030230)
// ---------------------------------------------------------------------------------------------

/// Weights of the two overlay slots while aiming.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct AimPose {
    /// Slot 4: the centre pose (`Fwd_00_`), weight `1 - f`.
    pub centre_weight: f32,
    /// Slot 5: the `Up_00_` or `Down_00_` pose at weight 1, only when `f > 0`.
    pub tilt: Option<AimTilt>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AimTilt {
    Up,
    Down,
}

/// Port of the weight computation of UpdateAim: the pitch angle (radians, clamped to
/// +-pi/2) gives `f = |pitch| / (pi/2)`; the centre pose gets `1 - f` and the tilt pose full
/// weight once `f > 0`. [I] for the exact weights.
pub fn aim_pose(pitch: f32) -> AimPose {
    gothic_portcov::hit!("aim_pose");
    let half_pi = std::f32::consts::FRAC_PI_2;
    let p = pitch.clamp(-half_pi, half_pi);
    let f = p.abs() / half_pi;
    AimPose {
        centre_weight: 1.0 - f,
        tilt: (f > 0.0).then_some(if p > 0.0 { AimTilt::Up } else { AimTilt::Down }),
    }
}

/// Aim clip names derived from the primary clip: the first eight underscore-separated words,
/// then `Fwd_00_`, `Up_00_` or `Down_00_`.
pub fn aim_clip_name(primary: &str, pose: &str) -> String {
    let head: Vec<&str> = primary.split('_').take(8).collect();
    format!("{}_{pose}_00_", head.join("_"))
}

// ---------------------------------------------------------------------------------------------
// Ragdoll and physics level of detail (ProcessPhysicsLOD, Engine:0x10031d40)
// ---------------------------------------------------------------------------------------------

/// Distance within which characters keep a physics controller (`PhysicControllerDistance`).
pub const PHYSICS_DISTANCE: f32 = 2000.0;
/// Width of the blend band below the distance limit.
pub const PHYSICS_BAND: f32 = 250.0;
/// A ragdoll lives this long, seconds.
pub const RAGDOLL_LIFETIME: f32 = 15.0;
/// After this many seconds the physics time slows down.
pub const RAGDOLL_SLOWDOWN_START: f32 = 10.0;
/// Duration of the slow-down to a standstill.
pub const RAGDOLL_SLOWDOWN: f32 = 5.0;

/// How strongly the physics controller drives the skeleton at a given camera distance.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum PhysicsLod {
    /// Too far: no controller.
    Off,
    /// Full physics.
    Full,
    /// Inside the band below the limit: blend factor 1 (near) to 0 (limit).
    Blend(f32),
}

/// Port of the distance test of ProcessPhysicsLOD. The decompiled band expression is garbled,
/// so the linear ramp over the last 250 units before the limit is [I].
pub fn physics_lod(distance: f32) -> PhysicsLod {
    gothic_portcov::hit!("physics_lod");
    if distance > PHYSICS_DISTANCE {
        PhysicsLod::Off
    } else if distance < PHYSICS_DISTANCE - PHYSICS_BAND {
        PhysicsLod::Full
    } else {
        PhysicsLod::Blend(
            (1.0 - (distance - (PHYSICS_DISTANCE - PHYSICS_BAND)) / PHYSICS_BAND).clamp(0.0, 1.0),
        )
    }
}

/// Result of one ragdoll timer step.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct RagdollStep {
    /// Factor applied to the physics controllers' time scale (1 = unchanged).
    pub physics_time_scale: f32,
    /// `StopRagDoll` is due.
    pub stop: bool,
}

/// Ragdoll life timer (this+0x130 with the slow-down timer at +0x1a4).
#[derive(Clone, Copy, Debug, Default)]
pub struct RagdollTimer {
    age: f32,
    slowdown: f32,
}

impl RagdollTimer {
    /// Advances by the scaled frame time `scaled_dt` (the age) and the unscaled `frame_dt`
    /// (the slow-down clock). The first two seconds do nothing, from 10 s the controllers'
    /// time scale falls linearly to zero over 5 s, and after 15 s the ragdoll stops.
    pub fn step(&mut self, scaled_dt: f32, frame_dt: f32) -> RagdollStep {
        self.age += scaled_dt;
        if self.age > RAGDOLL_LIFETIME {
            return RagdollStep {
                physics_time_scale: 0.0,
                stop: true,
            };
        }
        let mut scale = 1.0;
        if self.age > RAGDOLL_SLOWDOWN_START {
            self.slowdown += frame_dt;
            scale = (1.0 - self.slowdown / RAGDOLL_SLOWDOWN).max(0.0);
        }
        RagdollStep {
            physics_time_scale: scale,
            stop: false,
        }
    }

    pub fn age(&self) -> f32 {
        self.age
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn slot_gate_needs_a_name_and_a_valid_index() {
        let mut s = MotionSlots::default();
        assert!(!s.can_command(0));
        s.set_motion(0, "Hero_Stand_");
        assert!(s.can_command(0));
        assert!(!s.can_command(8));
        assert_eq!(s.owner(0), owner::FREE);
        s.set_owner(0, owner::IDLE);
        assert_eq!(s.owner(0), 9);
        s.reset();
        assert!(!s.has_motion(0));
    }

    #[test]
    fn aim_weights_follow_the_pitch() {
        assert_eq!(
            aim_pose(0.0),
            AimPose {
                centre_weight: 1.0,
                tilt: None
            }
        );
        let up = aim_pose(std::f32::consts::FRAC_PI_4);
        assert!((up.centre_weight - 0.5).abs() < 1e-6);
        assert_eq!(up.tilt, Some(AimTilt::Up));
        let down = aim_pose(-10.0);
        assert_eq!(down.centre_weight, 0.0);
        assert_eq!(down.tilt, Some(AimTilt::Down));
    }

    #[test]
    fn aim_names_keep_the_first_eight_words() {
        let name = "Hero_Stand_Bow_None_P0_Aim_Loop_N_Fwd_00_";
        assert_eq!(
            aim_clip_name(name, "Up"),
            "Hero_Stand_Bow_None_P0_Aim_Loop_N_Up_00_"
        );
    }

    #[test]
    fn physics_lod_bands() {
        assert_eq!(physics_lod(2500.0), PhysicsLod::Off);
        assert_eq!(physics_lod(500.0), PhysicsLod::Full);
        assert_eq!(physics_lod(1750.0), PhysicsLod::Blend(1.0));
        match physics_lod(1875.0) {
            PhysicsLod::Blend(f) => assert!((f - 0.5).abs() < 1e-6),
            other => panic!("{other:?}"),
        }
        assert_eq!(physics_lod(2000.0), PhysicsLod::Blend(0.0));
    }

    #[test]
    fn ragdoll_lives_fifteen_seconds_and_slows_after_ten() {
        let mut t = RagdollTimer::default();
        let s = t.step(9.0, 9.0);
        assert_eq!(
            s,
            RagdollStep {
                physics_time_scale: 1.0,
                stop: false
            }
        );
        let s = t.step(3.0, 3.0); // age 12, slow-down clock 3 s
        assert!((s.physics_time_scale - 0.4).abs() < 1e-6, "{s:?}");
        let s = t.step(2.0, 2.0); // age 14, clock 5 s
        assert_eq!(s.physics_time_scale, 0.0);
        assert!(!s.stop);
        assert!(t.step(1.5, 1.5).stop);
    }

    #[test]
    fn local_time_scale_multiplies_the_frame() {
        assert!((animation_delta(0.02, 0.5) - 0.01).abs() < 1e-9);
    }
}
