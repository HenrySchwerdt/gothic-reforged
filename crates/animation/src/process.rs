//! Per-frame decisions of `eCVisualAnimation_PS::OnProcess` that need no clip data.
//!
//! The host asks [`process_plan`] once per frame and entity; the answer says whether the
//! actor is advanced at all, whether the pose is physics driven, and whether the ragdoll has
//! come to rest and may be switched off. [`IdleChain`] is the A/B motion hand-over that
//! `OnProcess` runs on slot 0, and [`to_local_target`] converts a look-at target to the
//! entity's local space.

/// Inputs of the process gate.
#[derive(Debug, Clone, Copy, Default)]
pub struct ProcessState {
    /// The entity was rendered (or in view) this frame (`entity.lastFrame == app.frame`).
    pub seen_this_frame: bool,
    /// The ragdoll feature is enabled for this actor (+0x12c).
    pub ragdoll_enabled: bool,
    /// The ragdoll was stopped and the pose is final (+0x179).
    pub ragdoll_stopped: bool,
    /// Once-only attachments are queued (+0xd4 > 0).
    pub has_once_attachments: bool,
    /// `ForceAlwaysVisible` (+0x17a).
    pub force_always_visible: bool,
    /// The physics controller exists and is simulating a body (`+0x10c` or `+0x120`).
    pub controller_simulating: bool,
    /// A physics controller exists at all.
    pub has_controller: bool,
    /// Aim overlay enabled (+0x17b).
    pub auto_aim: bool,
}

/// Result of [`process_plan`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ProcessPlan {
    /// Advance the animation and attachments this frame (`bStack_8c`).
    pub advance: bool,
    /// The pose comes from physics rather than from motions (`local_88`).
    pub physics_driven: bool,
    /// Run the aim overlay update before processing.
    pub update_aim: bool,
    /// Push look-at targets to the constraints.
    pub update_look_at: bool,
}

/// Port of the gating in eCVisualAnimation_PS::OnProcess (Engine:0x10032d40): an actor is
/// advanced when it was seen this frame, when a live ragdoll needs its physics stepped, when
/// once-only attachments are waiting, or when it is forced visible. The pose is physics
/// driven for a stopped ragdoll, or for an enabled ragdoll whose controller is missing or
/// not simulating.
pub fn process_plan(s: &ProcessState) -> ProcessPlan {
    let advance = s.seen_this_frame
        || (s.ragdoll_enabled && !s.ragdoll_stopped)
        || s.has_once_attachments
        || s.force_always_visible;
    let physics_driven =
        s.ragdoll_stopped || (s.ragdoll_enabled && (!s.has_controller || !s.controller_simulating));
    ProcessPlan {
        advance,
        physics_driven,
        update_aim: advance && s.auto_aim,
        update_look_at: advance,
    }
}

/// Is the ragdoll finished? `OnProcess` switches it off (`SetRagDollEnabled(false)`) once the
/// ragdoll age (+0x134) exceeds `max_age` (the constant at Engine 0x10488498, 15 s in the
/// ragdoll timer of `visual.rs`) while a controller is active and its clock reached one
/// second.
pub fn ragdoll_should_stop(age: f32, max_age: f32, controller_active: bool, clock: f32) -> bool {
    age > max_age && controller_active && clock >= 1.0
}

/// 3D vector for the small amount of math here.
pub type V3 = [f32; 3];

/// Row-major affine transform `[right, up, at, position]`, enough to invert rigid
/// transforms.
pub type Affine = [[f32; 3]; 4];

/// Convert a world position to the local space of a rigid transform (rotation and
/// translation, no scale), as `OnProcess` does with the inverse entity matrix before it
/// calls `eCLookAtConstraint::SetTargetPos`.
pub fn to_local_target(world: &Affine, p: V3) -> V3 {
    let d = [p[0] - world[3][0], p[1] - world[3][1], p[2] - world[3][2]];
    let dot = |a: [f32; 3], b: V3| a[0] * b[0] + a[1] * b[1] + a[2] * b[2];
    [dot(world[0], d), dot(world[1], d), dot(world[2], d)]
}

/// The two-motion hand-over `OnProcess` runs on slot 0: `first` plays; when it has
/// `fade` seconds or less left it is cross-faded into `second`; `second` plays to its end
/// and `first` restarts hard.
#[derive(Debug, Clone, PartialEq)]
pub struct IdleChain {
    pub first: String,
    pub second: String,
    /// Cross-fade length and trigger distance from the end of `first` (+0x14c).
    pub fade: f32,
    /// `first` is the running one (+0x150).
    pub on_first: bool,
}

/// What the idle chain wants done.
#[derive(Debug, Clone, PartialEq)]
pub enum IdleAction {
    Nothing,
    /// Start this motion on slot 0 without a fade.
    Start(String),
    /// Fade the running motion out over `fade` seconds and start this one.
    CrossFadeTo {
        motion: String,
        fade: f32,
    },
}

impl IdleChain {
    /// Port of the chain step in eCVisualAnimation_PS::OnProcess. `remaining` is the time
    /// left of slot 0's motion (length minus position), 0 when nothing plays.
    pub fn step(&mut self, remaining: f32) -> IdleAction {
        if self.fade < 0.0 {
            return IdleAction::Nothing;
        }
        if self.on_first && remaining <= self.fade {
            self.on_first = false;
            return if self.second.is_empty() {
                IdleAction::Nothing
            } else {
                IdleAction::CrossFadeTo {
                    motion: self.second.clone(),
                    fade: self.fade,
                }
            };
        }
        if !self.on_first && remaining == 0.0 {
            self.on_first = true;
            return if self.first.is_empty() {
                IdleAction::Nothing
            } else {
                IdleAction::Start(self.first.clone())
            };
        }
        IdleAction::Nothing
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn gate_follows_visibility_ragdoll_and_forced_flags() {
        let mut s = ProcessState::default();
        assert!(!process_plan(&s).advance);
        s.seen_this_frame = true;
        assert!(process_plan(&s).advance);
        s.seen_this_frame = false;
        s.ragdoll_enabled = true;
        assert!(process_plan(&s).advance);
        s.ragdoll_stopped = true;
        assert!(!process_plan(&s).advance);
        s.force_always_visible = true;
        assert!(process_plan(&s).advance);
    }

    #[test]
    fn physics_driven_pose_rules() {
        let s = ProcessState {
            ragdoll_enabled: true,
            ..Default::default()
        };
        assert!(process_plan(&s).physics_driven);
        let s = ProcessState {
            ragdoll_enabled: true,
            has_controller: true,
            controller_simulating: true,
            ..Default::default()
        };
        assert!(!process_plan(&s).physics_driven);
        let s = ProcessState {
            ragdoll_stopped: true,
            ..Default::default()
        };
        assert!(process_plan(&s).physics_driven);
    }

    #[test]
    fn aim_runs_only_when_advancing() {
        let s = ProcessState {
            seen_this_frame: true,
            auto_aim: true,
            ..Default::default()
        };
        assert!(process_plan(&s).update_aim);
        let s = ProcessState {
            auto_aim: true,
            ..Default::default()
        };
        assert!(!process_plan(&s).update_aim);
    }

    #[test]
    fn local_target_inverts_a_rigid_transform() {
        // Rotated 90 degrees about Y: right=(0,0,-1), up=(0,1,0), at=(1,0,0); origin (10,0,0).
        let m: Affine = [
            [0.0, 0.0, -1.0],
            [0.0, 1.0, 0.0],
            [1.0, 0.0, 0.0],
            [10.0, 0.0, 0.0],
        ];
        let l = to_local_target(&m, [10.0, 2.0, -5.0]);
        assert_eq!(l, [5.0, 2.0, 0.0]);
        let id: Affine = [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0], [0.0; 3]];
        assert_eq!(to_local_target(&id, [1.0, 2.0, 3.0]), [1.0, 2.0, 3.0]);
    }

    #[test]
    fn idle_chain_alternates_and_fades_before_the_end() {
        let mut c = IdleChain {
            first: "A".into(),
            second: "B".into(),
            fade: 0.4,
            on_first: false,
        };
        // Nothing plays: start the first motion.
        assert_eq!(c.step(0.0), IdleAction::Start("A".into()));
        assert!(c.on_first);
        // Plenty of time left: nothing to do.
        assert_eq!(c.step(2.0), IdleAction::Nothing);
        // Close to the end: cross-fade over `fade` seconds.
        assert_eq!(
            c.step(0.3),
            IdleAction::CrossFadeTo {
                motion: "B".into(),
                fade: 0.4
            }
        );
        assert!(!c.on_first);
        // The second motion runs; at its end the first restarts.
        assert_eq!(c.step(1.0), IdleAction::Nothing);
        assert_eq!(c.step(0.0), IdleAction::Start("A".into()));
        // A negative fade disables the chain.
        c.fade = -1.0;
        assert_eq!(c.step(0.0), IdleAction::Nothing);
    }

    #[test]
    fn ragdoll_stop_condition() {
        assert!(ragdoll_should_stop(16.0, 15.0, true, 1.5));
        assert!(!ragdoll_should_stop(14.0, 15.0, true, 1.5));
        assert!(!ragdoll_should_stop(16.0, 15.0, false, 1.5));
        assert!(!ragdoll_should_stop(16.0, 15.0, true, 0.5));
    }
}
