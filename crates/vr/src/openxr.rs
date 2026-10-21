//! OpenXR backend (issue VR-07): tracking and controller input from an OpenXR runtime into
//! the [`XrFrame`](crate::XrFrame) seam.
//!
//! With the `openxr` cargo feature, [`OpenXrInput`] opens the system OpenXR loader at run
//! time (`openxr` crate, `loaded`: no link-time dependency, so builds and CI need no runtime),
//! starts a **headless** session (`XR_MND_headless`: tracking and actions without a graphics
//! binding), and every frame turns the view pose, the grip poses / velocities and the
//! controller actions into an `XrFrame` in the stage space (floor origin, -Z forward: the same
//! convention as tracking space here). Rendering into the headset swapchains is not part of
//! this backend (see `docs/vr/OPENXR.md`); the desktop window keeps mirroring the rig.
//!
//! Without the feature, or when no runtime / no headless support is found, `VrPlugin` keeps
//! the desktop fallback and says why.
//!
//! The suggested bindings ([`PROFILES`]) are plain data so they are tested in every build.

/// The actions the backend reads, one per `ControllerState` field.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ActionKind {
    GripPose,
    Trigger,
    Squeeze,
    Thumbstick,
    Primary,
    Secondary,
    StickClick,
    Menu,
}

impl ActionKind {
    pub const ALL: [ActionKind; 8] = [
        ActionKind::GripPose,
        ActionKind::Trigger,
        ActionKind::Squeeze,
        ActionKind::Thumbstick,
        ActionKind::Primary,
        ActionKind::Secondary,
        ActionKind::StickClick,
        ActionKind::Menu,
    ];

    /// OpenXR action name (lower case, no spaces).
    pub fn name(self) -> &'static str {
        match self {
            ActionKind::GripPose => "grip_pose",
            ActionKind::Trigger => "trigger",
            ActionKind::Squeeze => "squeeze",
            ActionKind::Thumbstick => "thumbstick",
            ActionKind::Primary => "primary",
            ActionKind::Secondary => "secondary",
            ActionKind::StickClick => "stick_click",
            ActionKind::Menu => "menu",
        }
    }
}

/// One suggested binding: action, hand (`left` / `right`), component path below
/// `/user/hand/<hand>/input/`.
pub type Binding = (ActionKind, &'static str, &'static str);

/// Suggested bindings per interaction profile (A/X = primary, B/Y = secondary).
pub const PROFILES: &[(&str, &[Binding])] = &[
    (
        "/interaction_profiles/oculus/touch_controller",
        &[
            (ActionKind::GripPose, "left", "grip/pose"),
            (ActionKind::GripPose, "right", "grip/pose"),
            (ActionKind::Trigger, "left", "trigger/value"),
            (ActionKind::Trigger, "right", "trigger/value"),
            (ActionKind::Squeeze, "left", "squeeze/value"),
            (ActionKind::Squeeze, "right", "squeeze/value"),
            (ActionKind::Thumbstick, "left", "thumbstick"),
            (ActionKind::Thumbstick, "right", "thumbstick"),
            (ActionKind::Primary, "left", "x/click"),
            (ActionKind::Primary, "right", "a/click"),
            (ActionKind::Secondary, "left", "y/click"),
            (ActionKind::Secondary, "right", "b/click"),
            (ActionKind::StickClick, "left", "thumbstick/click"),
            (ActionKind::StickClick, "right", "thumbstick/click"),
            (ActionKind::Menu, "left", "menu/click"),
        ],
    ),
    (
        "/interaction_profiles/valve/index_controller",
        &[
            (ActionKind::GripPose, "left", "grip/pose"),
            (ActionKind::GripPose, "right", "grip/pose"),
            (ActionKind::Trigger, "left", "trigger/value"),
            (ActionKind::Trigger, "right", "trigger/value"),
            (ActionKind::Squeeze, "left", "squeeze/value"),
            (ActionKind::Squeeze, "right", "squeeze/value"),
            (ActionKind::Thumbstick, "left", "thumbstick"),
            (ActionKind::Thumbstick, "right", "thumbstick"),
            (ActionKind::Primary, "left", "a/click"),
            (ActionKind::Primary, "right", "a/click"),
            (ActionKind::Secondary, "left", "b/click"),
            (ActionKind::Secondary, "right", "b/click"),
            (ActionKind::StickClick, "left", "thumbstick/click"),
            (ActionKind::StickClick, "right", "thumbstick/click"),
            (ActionKind::Menu, "left", "system/click"),
        ],
    ),
    (
        "/interaction_profiles/htc/vive_controller",
        &[
            (ActionKind::GripPose, "left", "grip/pose"),
            (ActionKind::GripPose, "right", "grip/pose"),
            (ActionKind::Trigger, "left", "trigger/value"),
            (ActionKind::Trigger, "right", "trigger/value"),
            (ActionKind::Squeeze, "left", "squeeze/click"),
            (ActionKind::Squeeze, "right", "squeeze/click"),
            (ActionKind::Thumbstick, "left", "trackpad"),
            (ActionKind::Thumbstick, "right", "trackpad"),
            (ActionKind::StickClick, "left", "trackpad/click"),
            (ActionKind::StickClick, "right", "trackpad/click"),
            (ActionKind::Menu, "left", "menu/click"),
            (ActionKind::Menu, "right", "menu/click"),
        ],
    ),
    (
        "/interaction_profiles/khr/simple_controller",
        &[
            (ActionKind::GripPose, "left", "grip/pose"),
            (ActionKind::GripPose, "right", "grip/pose"),
            (ActionKind::Trigger, "left", "select/click"),
            (ActionKind::Trigger, "right", "select/click"),
            (ActionKind::Menu, "left", "menu/click"),
            (ActionKind::Menu, "right", "menu/click"),
        ],
    ),
];

/// Full binding path of a component.
pub fn binding_path(hand: &str, component: &str) -> String {
    format!("/user/hand/{hand}/input/{component}")
}

#[cfg(feature = "openxr")]
pub use backend::OpenXrInput;

// The only unsafe code of the crate: loading the OpenXR loader, creating the session and the
// two clock conversions are FFI calls.
#[cfg(feature = "openxr")]
#[allow(unsafe_code)]
mod backend {
    use super::{ActionKind, PROFILES, binding_path};
    use crate::input::{ControllerState, Hand, Pose, XrFrame};
    use bevy::math::{Quat, Vec2, Vec3};
    use openxr as xr;

    fn pose(p: &xr::Posef) -> Pose {
        Pose::new(
            Vec3::new(p.position.x, p.position.y, p.position.z),
            Quat::from_xyzw(
                p.orientation.x,
                p.orientation.y,
                p.orientation.z,
                p.orientation.w,
            )
            .normalize(),
        )
    }

    fn tracked(flags: xr::SpaceLocationFlags) -> bool {
        flags.contains(xr::SpaceLocationFlags::POSITION_VALID)
            && flags.contains(xr::SpaceLocationFlags::ORIENTATION_VALID)
    }

    struct Actions {
        set: xr::ActionSet,
        grip: xr::Action<xr::Posef>,
        trigger: xr::Action<f32>,
        squeeze: xr::Action<f32>,
        stick: xr::Action<xr::Vector2f>,
        primary: xr::Action<bool>,
        secondary: xr::Action<bool>,
        stick_click: xr::Action<bool>,
        menu: xr::Action<bool>,
    }

    /// A running OpenXR headless session feeding [`XrFrame`]s.
    pub struct OpenXrInput {
        instance: xr::Instance,
        session: xr::Session<xr::Headless>,
        actions: Actions,
        hands: [xr::Path; 2],
        hand_spaces: [xr::Space; 2],
        stage: xr::Space,
        view: xr::Space,
        events: xr::EventDataBuffer,
        state: xr::SessionState,
        running: bool,
        /// Runtime and system name, for the log.
        pub description: String,
    }

    fn err(what: &str, e: impl std::fmt::Display) -> String {
        format!("{what}: {e}")
    }

    impl OpenXrInput {
        /// Load the loader, create the instance, a headless session and the actions.
        pub fn new(app_name: &str) -> Result<Self, String> {
            // SAFETY: the loader found by the dynamic linker is the system OpenXR loader.
            let entry = unsafe { xr::Entry::load(&()) }
                .map_err(|e| err("no OpenXR loader (libopenxr_loader)", e))?;
            let available = entry
                .enumerate_extensions()
                .map_err(|e| err("no OpenXR runtime", e))?;
            if !available.mnd_headless {
                return Err(
                    "the OpenXR runtime has no XR_MND_headless (tracking-only sessions); \
                            headset rendering is not implemented yet"
                        .into(),
                );
            }
            let mut want = xr::ExtensionSet::default();
            want.mnd_headless = true;
            #[cfg(unix)]
            {
                if !available.khr_convert_timespec_time {
                    return Err("the OpenXR runtime has no XR_KHR_convert_timespec_time".into());
                }
                want.khr_convert_timespec_time = true;
            }
            #[cfg(windows)]
            {
                if !available.khr_win32_convert_performance_counter_time {
                    return Err(
                        "the OpenXR runtime has no XR_KHR_win32_convert_performance_counter_time"
                            .into(),
                    );
                }
                want.khr_win32_convert_performance_counter_time = true;
            }
            let instance = entry
                .create_instance(
                    &xr::ApplicationInfo {
                        application_name: app_name,
                        application_version: 0,
                        engine_name: "Gothic Reforged",
                        engine_version: 0,
                        api_version: xr::Version::new(1, 0, 0),
                    },
                    &want,
                    &[],
                    &(),
                )
                .map_err(|e| err("xrCreateInstance", e))?;
            let props = instance
                .properties()
                .map_err(|e| err("xrGetInstanceProperties", e))?;
            let system = instance
                .system(xr::FormFactor::HEAD_MOUNTED_DISPLAY)
                .map_err(|e| err("no head-mounted display", e))?;
            let sys_props = instance
                .system_properties(system)
                .map_err(|e| err("xrGetSystemProperties", e))?;
            let description = format!(
                "{} {} on {}",
                props.runtime_name, props.runtime_version, sys_props.system_name
            );
            // SAFETY: headless sessions take no graphics handles.
            let (session, _waiter, _stream) = unsafe {
                instance.create_session::<xr::Headless>(system, &xr::headless::SessionCreateInfo {})
            }
            .map_err(|e| err("xrCreateSession (headless)", e))?;

            let path = |s: &str| instance.string_to_path(s).map_err(|e| err(s, e));
            let hands = [path("/user/hand/left")?, path("/user/hand/right")?];
            let set = instance
                .create_action_set("gameplay", "Gameplay", 0)
                .map_err(|e| err("xrCreateActionSet", e))?;
            macro_rules! action {
                ($ty:ty, $kind:expr) => {
                    set.create_action::<$ty>($kind.name(), $kind.name(), &hands)
                        .map_err(|e| err($kind.name(), e))?
                };
            }
            let actions = Actions {
                grip: action!(xr::Posef, ActionKind::GripPose),
                trigger: action!(f32, ActionKind::Trigger),
                squeeze: action!(f32, ActionKind::Squeeze),
                stick: action!(xr::Vector2f, ActionKind::Thumbstick),
                primary: action!(bool, ActionKind::Primary),
                secondary: action!(bool, ActionKind::Secondary),
                stick_click: action!(bool, ActionKind::StickClick),
                menu: action!(bool, ActionKind::Menu),
                set,
            };
            for (profile, bindings) in PROFILES {
                let Ok(profile) = instance.string_to_path(profile) else {
                    continue;
                };
                let mut list = Vec::new();
                for (kind, hand, component) in *bindings {
                    let Ok(p) = instance.string_to_path(&binding_path(hand, component)) else {
                        continue;
                    };
                    list.push(match kind {
                        ActionKind::GripPose => xr::Binding::new(&actions.grip, p),
                        ActionKind::Trigger => xr::Binding::new(&actions.trigger, p),
                        ActionKind::Squeeze => xr::Binding::new(&actions.squeeze, p),
                        ActionKind::Thumbstick => xr::Binding::new(&actions.stick, p),
                        ActionKind::Primary => xr::Binding::new(&actions.primary, p),
                        ActionKind::Secondary => xr::Binding::new(&actions.secondary, p),
                        ActionKind::StickClick => xr::Binding::new(&actions.stick_click, p),
                        ActionKind::Menu => xr::Binding::new(&actions.menu, p),
                    });
                }
                // A runtime that does not know a profile rejects it; the others still apply.
                let _ = instance.suggest_interaction_profile_bindings(profile, &list);
            }
            session
                .attach_action_sets(&[&actions.set])
                .map_err(|e| err("xrAttachSessionActionSets", e))?;
            let hand_spaces = [
                actions
                    .grip
                    .create_space(&session, hands[0], xr::Posef::IDENTITY)
                    .map_err(|e| err("left grip space", e))?,
                actions
                    .grip
                    .create_space(&session, hands[1], xr::Posef::IDENTITY)
                    .map_err(|e| err("right grip space", e))?,
            ];
            let stage = session
                .create_reference_space(xr::ReferenceSpaceType::STAGE, xr::Posef::IDENTITY)
                .or_else(|_| {
                    session.create_reference_space(
                        xr::ReferenceSpaceType::LOCAL,
                        xr::Posef {
                            orientation: xr::Quaternionf::IDENTITY,
                            // LOCAL sits at the eyes: put the floor ~1.7 m below.
                            position: xr::Vector3f {
                                x: 0.0,
                                y: -1.7,
                                z: 0.0,
                            },
                        },
                    )
                })
                .map_err(|e| err("stage space", e))?;
            let view = session
                .create_reference_space(xr::ReferenceSpaceType::VIEW, xr::Posef::IDENTITY)
                .map_err(|e| err("view space", e))?;
            Ok(Self {
                instance,
                session,
                actions,
                hands,
                hand_spaces,
                stage,
                view,
                events: xr::EventDataBuffer::new(),
                state: xr::SessionState::UNKNOWN,
                running: false,
                description,
            })
        }

        pub fn state(&self) -> xr::SessionState {
            self.state
        }

        pub fn running(&self) -> bool {
            self.running
        }

        /// The runtime's current time.
        fn now(&self) -> Result<xr::Time, String> {
            let mut t = xr::Time::from_nanos(0);
            #[cfg(unix)]
            {
                let mut ts = libc::timespec {
                    tv_sec: 0,
                    tv_nsec: 0,
                };
                // SAFETY: valid out pointer; CLOCK_MONOTONIC always exists on Linux.
                unsafe { libc::clock_gettime(libc::CLOCK_MONOTONIC, &mut ts) };
                let f = self
                    .instance
                    .exts()
                    .khr_convert_timespec_time
                    .ok_or("XR_KHR_convert_timespec_time not loaded")?;
                // SAFETY: the extension was enabled at instance creation.
                let r = unsafe {
                    (f.convert_timespec_time_to_time)(self.instance.as_raw(), &ts, &mut t)
                };
                if r.into_raw() < 0 {
                    return Err(format!("xrConvertTimespecTimeToTimeKHR: {r}"));
                }
            }
            #[cfg(windows)]
            {
                unsafe extern "system" {
                    fn QueryPerformanceCounter(count: *mut i64) -> i32;
                }
                let mut pc = 0i64;
                // SAFETY: valid out pointer.
                unsafe { QueryPerformanceCounter(&mut pc) };
                let f = self
                    .instance
                    .exts()
                    .khr_win32_convert_performance_counter_time
                    .ok_or("XR_KHR_win32_convert_performance_counter_time not loaded")?;
                // SAFETY: the extension was enabled at instance creation.
                let r = unsafe {
                    (f.convert_win32_performance_counter_to_time)(
                        self.instance.as_raw(),
                        &pc,
                        &mut t,
                    )
                };
                if r.into_raw() < 0 {
                    return Err(format!("xrConvertWin32PerformanceCounterToTimeKHR: {r}"));
                }
            }
            Ok(t)
        }

        /// Handle runtime events (session lifecycle). Returns false once the session ended.
        pub fn pump_events(&mut self) -> Result<bool, String> {
            while let Some(event) = self
                .instance
                .poll_event(&mut self.events)
                .map_err(|e| err("xrPollEvent", e))?
            {
                match event {
                    xr::Event::SessionStateChanged(e) => {
                        self.state = e.state();
                        match e.state() {
                            xr::SessionState::READY => {
                                self.session
                                    .begin(xr::ViewConfigurationType::PRIMARY_STEREO)
                                    .map_err(|e| err("xrBeginSession", e))?;
                                self.running = true;
                            }
                            xr::SessionState::STOPPING => {
                                self.session.end().map_err(|e| err("xrEndSession", e))?;
                                self.running = false;
                            }
                            xr::SessionState::EXITING | xr::SessionState::LOSS_PENDING => {
                                self.running = false;
                                return Ok(false);
                            }
                            _ => {}
                        }
                    }
                    xr::Event::InstanceLossPending(_) => return Ok(false),
                    _ => {}
                }
            }
            Ok(true)
        }

        /// One frame of input: `None` until the session runs. Untracked parts keep `fallback`'s
        /// values (head) or report no pose (hands).
        pub fn frame(&mut self, fallback: &XrFrame) -> Result<Option<XrFrame>, String> {
            if !self.pump_events()? || !self.running {
                return Ok(None);
            }
            self.session
                .sync_actions(&[xr::ActiveActionSet::new(&self.actions.set)])
                .map_err(|e| err("xrSyncActions", e))?;
            let time = self.now()?;
            let mut out = *fallback;
            let head = self
                .view
                .locate(&self.stage, time)
                .map_err(|e| err("locate view", e))?;
            if tracked(head.location_flags) {
                out.head = pose(&head.pose);
            }
            for hand in Hand::BOTH {
                let i = hand.index();
                let sub = self.hands[i];
                let a = &self.actions;
                let (loc, vel) = self.hand_spaces[i]
                    .relate(&self.stage, time)
                    .map_err(|e| err("locate hand", e))?;
                let active = a
                    .grip
                    .is_active(&self.session, sub)
                    .map_err(|e| err("grip pose state", e))?;
                let f =
                    |r: xr::Result<xr::ActionState<f32>>| r.map(|s| s.current_state).unwrap_or(0.0);
                let b = |r: xr::Result<xr::ActionState<bool>>| {
                    r.map(|s| s.current_state).unwrap_or(false)
                };
                let stick = a
                    .stick
                    .state(&self.session, sub)
                    .map(|s| Vec2::new(s.current_state.x, s.current_state.y))
                    .unwrap_or(Vec2::ZERO);
                out.hands[i] = ControllerState {
                    pose: (active && tracked(loc.location_flags)).then(|| pose(&loc.pose)),
                    linear_velocity: vel
                        .velocity_flags
                        .contains(xr::SpaceVelocityFlags::LINEAR_VALID)
                        .then(|| {
                            Vec3::new(
                                vel.linear_velocity.x,
                                vel.linear_velocity.y,
                                vel.linear_velocity.z,
                            )
                        }),
                    trigger: f(a.trigger.state(&self.session, sub)),
                    grip: f(a.squeeze.state(&self.session, sub)),
                    thumbstick: stick,
                    primary: b(a.primary.state(&self.session, sub)),
                    secondary: b(a.secondary.state(&self.session, sub)),
                    stick_click: b(a.stick_click.state(&self.session, sub)),
                    menu: b(a.menu.state(&self.session, sub)),
                };
            }
            Ok(Some(out.sanitized(fallback)))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    #[test]
    fn every_profile_binds_poses_and_trigger_for_both_hands() {
        for (profile, bindings) in PROFILES {
            assert!(profile.starts_with("/interaction_profiles/"), "{profile}");
            for hand in ["left", "right"] {
                for need in [ActionKind::GripPose, ActionKind::Trigger] {
                    assert!(
                        bindings.iter().any(|(k, h, _)| *k == need && *h == hand),
                        "{profile} lacks {need:?} for {hand}"
                    );
                }
            }
            let mut seen = HashSet::new();
            for (k, h, c) in *bindings {
                assert!(seen.insert((*k, *h)), "{profile}: {k:?} bound twice on {h}");
                assert!(!c.starts_with('/') && !c.ends_with('/'), "{c}");
                assert!(binding_path(h, c).starts_with("/user/hand/"));
            }
        }
        let touch = PROFILES[0].1;
        assert_eq!(
            touch.len(),
            15,
            "Touch binds every action (menu only on the left)"
        );
        let names: HashSet<_> = ActionKind::ALL.iter().map(|k| k.name()).collect();
        assert_eq!(names.len(), ActionKind::ALL.len());
        assert!(
            names
                .iter()
                .all(|n| n.chars().all(|c| c.is_ascii_lowercase() || c == '_'))
        );
    }
}
