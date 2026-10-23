//! Bevy integration: the XR backend seam, input resources, locomotion/body systems and the
//! tracked entities other VR plugins attach to.
//!
//! Data flow per frame (`Update`, chained [`VrSet`]s):
//!
//! ```text
//! backend (desktop / scripted / OpenXR*) -> PendingXrFrame
//!   -> VrSet::Input      VrInputRes (edges, hysteresis) + runtime CoreCommands
//!   -> VrSet::Locomotion VrPlayer (CharacterController + Locomotion) on VrWorld
//!   -> VrSet::Body       VrBody (full-body IK)
//!   -> VrSet::Sync       Transforms of XrHead / XrHand / XrEyeCamera entities
//! ```
//!
//! `*` A headset backend only has to write [`PendingXrFrame`] before [`VrSet::Input`] (and
//! drive the eye cameras' projection). See `docs/vr/OPENXR.md` for why no OpenXR backend is
//! compiled in yet (no Bevy 0.20 compatible OpenXR integration has been released).
use crate::bindings::{CommandMapper, hero_transform};
use crate::body::{BodyIk, BodyPose, Proportions};
use crate::desktop::{DesktopEmulator, DesktopInput, InputScript};
use crate::input::{Hand, Pose, VrInput, XrFrame};
use crate::locomotion::{ComfortSettings, Locomotion, LocomotionReport};
use crate::rig::{DEFAULT_IPD, Eye, StereoMode, eye_pose};
use crate::{CM_PER_M, pose_to_transform};
use bevy::prelude::*;
use gothic_physics::{CharacterController, CollisionWorld, MovementProfile};
use gothic_runtime::CoreCommand;

/// Which device produces [`XrFrame`]s.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum XrBackendKind {
    /// Keyboard and mouse emulate the headset.
    #[default]
    Desktop,
    /// A scripted [`InputScript`] (headless CI, demos).
    Scripted,
    /// An OpenXR runtime (SteamVR, Quest Link, Monado, ...): tracking and controller input
    /// with the `openxr` cargo feature (see [`crate::openxr`]); without the feature or a
    /// usable runtime it falls back to [`XrBackendKind::Desktop`] with a warning.
    OpenXr,
    /// Another plugin, a test or a recording writes [`PendingXrFrame`] itself.
    External,
}

impl XrBackendKind {
    /// `GOTHIC_VR_BACKEND=desktop|openxr` (default desktop).
    pub fn from_env() -> Self {
        match std::env::var("GOTHIC_VR_BACKEND").as_deref() {
            Ok("openxr") => XrBackendKind::OpenXr,
            _ => XrBackendKind::Desktop,
        }
    }
}

/// The active backend and why it was chosen.
#[derive(Resource, Debug, Clone)]
pub struct XrBackend {
    pub kind: XrBackendKind,
    pub note: String,
}

/// The newest frame from the backend, consumed by [`VrSet::Input`].
#[derive(Resource, Debug, Default, Clone)]
pub struct PendingXrFrame(pub Option<XrFrame>);

/// Current input with edge detection.
#[derive(Resource, Debug, Default, Clone)]
pub struct VrInputRes(pub VrInput);

/// Desktop emulator state and an optional script that replaces the keyboard.
#[derive(Resource, Debug, Default, Clone)]
pub struct DesktopXr {
    pub emulator: DesktopEmulator,
    pub script: Option<InputScript>,
    /// The script ran out.
    pub script_done: bool,
    /// Last input fed to the emulator (for HUD/debug).
    pub last: DesktopInput,
}

/// Collision geometry (centimetres) the VR character walks on.
#[derive(Resource, Default)]
pub struct VrWorld(pub CollisionWorld);

/// The VR player: the game's character controller plus the comfort rig.
#[derive(Resource, Debug, Clone)]
pub struct VrPlayer {
    pub ctrl: CharacterController,
    pub loco: Locomotion,
    pub last: LocomotionReport,
}

impl VrPlayer {
    /// A player standing at `feet` (world metres).
    pub fn new(feet: Vec3, settings: ComfortSettings, head: &Pose) -> Self {
        let ctrl = CharacterController::new(MovementProfile::hero(), feet * CM_PER_M);
        let loco = Locomotion::new(settings, &ctrl, head);
        Self {
            ctrl,
            loco,
            last: LocomotionReport::default(),
        }
    }

    /// World pose of a tracking-space pose.
    pub fn world(&self, p: &Pose) -> Pose {
        self.loco.to_world(p)
    }
}

/// Full-body avatar.
#[derive(Resource, Debug, Clone)]
pub struct VrBody {
    pub ik: BodyIk,
    pub pose: Option<BodyPose>,
}

impl Default for VrBody {
    fn default() -> Self {
        Self {
            ik: BodyIk::new(Proportions::for_eye_height(1.7)),
            pose: None,
        }
    }
}

/// Button -> runtime command mapping.
#[derive(Resource, Debug, Default, Clone)]
pub struct VrCommands {
    pub mapper: CommandMapper,
    /// Also send `Command::HeroTransform` every frame.
    pub send_hero_transform: bool,
}

/// Rig display settings.
#[derive(Resource, Debug, Clone, Copy)]
pub struct VrDisplay {
    pub stereo: StereoMode,
    pub ipd: f32,
}

impl Default for VrDisplay {
    fn default() -> Self {
        Self {
            stereo: StereoMode::Mono,
            ipd: DEFAULT_IPD,
        }
    }
}

/// The tracked head (world transform, metres).
#[derive(Component, Debug, Default, Clone, Copy)]
pub struct XrHead;

/// A tracked hand (world transform of the grip pose, metres). `tracked` is false while the
/// runtime lost it (the transform then keeps its last value).
#[derive(Component, Debug, Clone, Copy)]
pub struct XrHand {
    pub hand: Hand,
    pub tracked: bool,
    /// World-space velocity, m/s.
    pub velocity: Vec3,
    /// World-space angular velocity (scaled axis), rad/s.
    pub angular_velocity: Vec3,
}

/// An eye camera of the rig.
#[derive(Component, Debug, Clone, Copy)]
pub struct XrEyeCamera(pub Eye);

/// Ask for a controller vibration (see [`crate::haptics`]).
#[derive(Message, Debug, Clone, Copy)]
pub struct HapticMessage(pub crate::haptics::HapticPulse);

/// Merged haptics: the mixer and, for tests and the desktop fallback, the pulses played.
#[derive(Resource, Default)]
pub struct HapticOut {
    pub mixer: crate::haptics::HapticMixer,
    /// Every pulse started so far (kept short: the last 64).
    pub played: Vec<crate::haptics::HapticPulse>,
}

#[derive(SystemSet, Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum VrSet {
    Backend,
    Input,
    Locomotion,
    Body,
    Sync,
}

/// Core VR plugin (works headless with `MinimalPlugins`).
#[derive(Default)]
pub struct VrPlugin {
    pub backend: XrBackendKind,
}

impl Plugin for VrPlugin {
    fn build(&self, app: &mut App) {
        let backend = match self.backend {
            XrBackendKind::OpenXr => openxr_backend(app),
            kind => XrBackend {
                kind,
                note: format!("{kind:?} backend"),
            },
        };
        app.insert_resource(backend)
            .init_resource::<PendingXrFrame>()
            .init_resource::<VrInputRes>()
            .init_resource::<DesktopXr>()
            .init_resource::<VrBody>()
            .init_resource::<VrCommands>()
            .init_resource::<VrDisplay>()
            .add_message::<CoreCommand>()
            .add_message::<HapticMessage>()
            .init_resource::<HapticOut>()
            .add_systems(Last, play_haptics)
            .configure_sets(
                Update,
                (
                    VrSet::Backend,
                    VrSet::Input,
                    VrSet::Locomotion,
                    VrSet::Body,
                    VrSet::Sync,
                )
                    .chain(),
            )
            .add_systems(Startup, spawn_tracked_entities)
            .add_systems(Update, desktop_backend.in_set(VrSet::Backend))
            .add_systems(
                Update,
                (consume_frame, runtime_commands)
                    .chain()
                    .in_set(VrSet::Input),
            )
            .add_systems(
                Update,
                locomotion
                    .in_set(VrSet::Locomotion)
                    .run_if(resource_exists::<VrPlayer>)
                    .run_if(resource_exists::<VrWorld>),
            )
            .add_systems(Update, solve_body.in_set(VrSet::Body))
            .add_systems(Update, sync_tracked.in_set(VrSet::Sync));
    }
}

/// Start the OpenXR input backend, or explain the desktop fallback.
fn openxr_backend(app: &mut App) -> XrBackend {
    #[cfg(feature = "openxr")]
    let backend = match crate::openxr::OpenXrInput::new("Gothic Reforged") {
        Ok(xr) => {
            let note = format!(
                "OpenXR input from {} (headless session: tracking and controllers; the window \
                 mirrors the rig)",
                xr.description
            );
            info!("{note}");
            app.insert_non_send(xr)
                .add_systems(Update, openxr_frame.in_set(VrSet::Backend));
            XrBackend {
                kind: XrBackendKind::OpenXr,
                note,
            }
        }
        Err(e) => {
            let note = format!("OpenXR unavailable ({e}); using the desktop fallback");
            warn!("{note}");
            XrBackend {
                kind: XrBackendKind::Desktop,
                note,
            }
        }
    };
    #[cfg(not(feature = "openxr"))]
    let backend = {
        let _ = app;
        let note = "OpenXR requested, but this build has no OpenXR backend (build with \
                    `--features gothic-vr/openxr`, see docs/vr/OPENXR.md); using the desktop \
                    fallback"
            .to_string();
        warn!("{note}");
        XrBackend {
            kind: XrBackendKind::Desktop,
            note,
        }
    };
    backend
}

#[cfg(feature = "openxr")]
fn openxr_frame(
    xr: Option<NonSendMut<crate::openxr::OpenXrInput>>,
    input: Res<VrInputRes>,
    mut pending: ResMut<PendingXrFrame>,
) {
    let Some(mut xr) = xr else {
        return;
    };
    match xr.frame(&input.0.current) {
        Ok(Some(frame)) => pending.0 = Some(frame),
        Ok(None) => {}
        Err(e) => warn!("OpenXR frame: {e}"),
    }
}

/// Merge the frame's haptic requests and hand them to the backend.
fn play_haptics(
    time: Res<Time<Real>>,
    mut reader: MessageReader<HapticMessage>,
    mut out: ResMut<HapticOut>,
    #[cfg(feature = "openxr")] mut xr: Option<NonSendMut<crate::openxr::OpenXrInput>>,
) {
    let now = time.elapsed_secs();
    let out = &mut *out;
    for HapticMessage(p) in reader.read() {
        out.mixer.push(*p, now);
    }
    let started = out.mixer.take(now);
    if started.is_empty() {
        return;
    }
    #[cfg(feature = "openxr")]
    if let Some(xr) = xr.as_mut() {
        for p in &started {
            if let Err(e) = xr.vibrate(p) {
                warn!("OpenXR haptics: {e}");
            }
        }
    }
    out.played.extend(started);
    let excess = out.played.len().saturating_sub(64);
    out.played.drain(..excess);
}

fn spawn_tracked_entities(mut commands: Commands) {
    commands.spawn((
        XrHead,
        Transform::from_xyz(0.0, 1.7, 0.0),
        Name::new("XrHead"),
    ));
    for hand in Hand::BOTH {
        commands.spawn((
            XrHand {
                hand,
                tracked: false,
                velocity: Vec3::ZERO,
                angular_velocity: Vec3::ZERO,
            },
            Transform::default(),
            Name::new(format!("XrHand{hand:?}")),
        ));
    }
}

/// Snapshot of keyboard/mouse for the emulator (`None` resources = headless: no input).
fn read_desktop(
    keys: Option<&ButtonInput<KeyCode>>,
    buttons: Option<&ButtonInput<MouseButton>>,
    motion: Vec2,
    wheel: f32,
) -> DesktopInput {
    let (Some(keys), Some(buttons)) = (keys, buttons) else {
        return DesktopInput::default();
    };
    let axis =
        |neg: KeyCode, pos: KeyCode| (keys.pressed(pos) as i32 - keys.pressed(neg) as i32) as f32;
    let look = buttons.pressed(MouseButton::Right);
    DesktopInput {
        move_axes: Vec2::new(
            axis(KeyCode::KeyA, KeyCode::KeyD),
            axis(KeyCode::KeyS, KeyCode::KeyW),
        ),
        turn_axis: axis(KeyCode::KeyQ, KeyCode::KeyE),
        teleport: keys.pressed(KeyCode::KeyT),
        look_delta: if look { motion } else { Vec2::ZERO },
        hand_delta: if look { Vec2::ZERO } else { motion },
        wheel,
        trigger: buttons.pressed(MouseButton::Left),
        toggle_grip: keys.just_pressed(KeyCode::KeyG),
        toggle_other_grip: keys.just_pressed(KeyCode::KeyF),
        switch_hand: keys.just_pressed(KeyCode::Tab),
        primary: keys.pressed(KeyCode::Space),
        secondary: keys.pressed(KeyCode::KeyR),
        sprint: keys.pressed(KeyCode::ShiftLeft),
        crouch: keys.pressed(KeyCode::KeyC),
        menu: keys.pressed(KeyCode::Escape),
    }
}

#[allow(clippy::too_many_arguments)]
fn desktop_backend(
    backend: Res<XrBackend>,
    time: Res<Time>,
    keys: Option<Res<ButtonInput<KeyCode>>>,
    buttons: Option<Res<ButtonInput<MouseButton>>>,
    motion: Option<Res<bevy::input::mouse::AccumulatedMouseMotion>>,
    scroll: Option<Res<bevy::input::mouse::AccumulatedMouseScroll>>,
    mut desktop: ResMut<DesktopXr>,
    mut pending: ResMut<PendingXrFrame>,
) {
    if !matches!(
        backend.kind,
        XrBackendKind::Desktop | XrBackendKind::Scripted
    ) {
        return;
    }
    let input = if let Some(script) = desktop.script.as_mut() {
        match script.next_input() {
            Some(i) => i,
            None => {
                desktop.script_done = true;
                DesktopInput::default()
            }
        }
    } else {
        read_desktop(
            keys.as_deref(),
            buttons.as_deref(),
            motion.map_or(Vec2::ZERO, |m| m.delta),
            scroll.map_or(0.0, |s| s.delta.y),
        )
    };
    desktop.last = input;
    let frame = desktop.emulator.frame(&input, time.delta_secs());
    pending.0 = Some(frame);
}

fn consume_frame(mut pending: ResMut<PendingXrFrame>, mut input: ResMut<VrInputRes>) {
    if let Some(frame) = pending.0.take() {
        input.0.push(frame);
    }
}

fn runtime_commands(
    input: Res<VrInputRes>,
    mut map: ResMut<VrCommands>,
    mut out: MessageWriter<CoreCommand>,
) {
    for c in map.mapper.commands(&input.0) {
        out.write(CoreCommand(c));
    }
}

fn locomotion(
    time: Res<Time>,
    input: Res<VrInputRes>,
    world: Res<VrWorld>,
    mut player: ResMut<VrPlayer>,
    commands: Res<VrCommands>,
    mut out: MessageWriter<CoreCommand>,
) {
    let p = &mut *player;
    p.last = p
        .loco
        .update(&input.0, &mut p.ctrl, &world.0, time.delta_secs());
    if commands.send_hero_transform {
        out.write(CoreCommand(hero_transform(p.ctrl.position, p.ctrl.heading)));
    }
}

fn solve_body(
    time: Res<Time>,
    input: Res<VrInputRes>,
    player: Option<Res<VrPlayer>>,
    world: Option<Res<VrWorld>>,
    mut body: ResMut<VrBody>,
) {
    let to_world = |p: &Pose| player.as_ref().map_or(*p, |pl| pl.world(p));
    let head = to_world(&input.0.head());
    let hands = Hand::BOTH.map(|h| input.0.hand_pose(h).map(|p| to_world(&p)));
    let floor = player
        .as_ref()
        .map_or(0.0, |p| p.ctrl.position.y / CM_PER_M);
    let ground = |x: f32, z: f32| -> f32 {
        world
            .as_ref()
            .and_then(|w| {
                w.0.ground_below(Vec3::new(x, floor + 0.7, z) * CM_PER_M, 150.0)
                    .map(|y| y / CM_PER_M)
            })
            .unwrap_or(floor)
    };
    let pose = body.ik.solve(&head, hands, ground, time.delta_secs());
    body.pose = Some(pose);
}

#[allow(clippy::type_complexity)]
fn sync_tracked(
    time: Res<Time>,
    input: Res<VrInputRes>,
    player: Option<Res<VrPlayer>>,
    display: Res<VrDisplay>,
    mut heads: Query<&mut Transform, (With<XrHead>, Without<XrHand>, Without<XrEyeCamera>)>,
    mut hands: Query<(&mut Transform, &mut XrHand), (Without<XrHead>, Without<XrEyeCamera>)>,
    mut eyes: Query<(&mut Transform, &XrEyeCamera), (Without<XrHead>, Without<XrHand>)>,
) {
    let to_world = |p: &Pose| player.as_ref().map_or(*p, |pl| pl.world(p));
    let head = to_world(&input.0.head());
    for mut t in &mut heads {
        *t = pose_to_transform(&head);
    }
    let dt = time.delta_secs();
    // Runtime velocities are in tracking space; the play space only turns them.
    let space = to_world(&Pose::IDENTITY).rotation;
    for (mut t, mut h) in &mut hands {
        match input.0.hand_pose(h.hand) {
            Some(p) => {
                let w = to_world(&p);
                let c = input.0.current.hand(h.hand);
                h.velocity = match c.linear_velocity {
                    Some(v) => space * v,
                    None if h.tracked && dt > 0.0 => (w.position - t.translation) / dt,
                    None => Vec3::ZERO,
                };
                h.angular_velocity = match c.angular_velocity {
                    Some(v) => space * v,
                    None if h.tracked && dt > 0.0 => {
                        crate::input::rotation_between(t.rotation, w.rotation) / dt
                    }
                    None => Vec3::ZERO,
                };
                h.tracked = true;
                *t = pose_to_transform(&w);
            }
            None => {
                h.tracked = false;
                h.velocity = Vec3::ZERO;
                h.angular_velocity = Vec3::ZERO;
            }
        }
    }
    for (mut t, eye) in &mut eyes {
        let p = match display.stereo {
            StereoMode::Mono => head,
            StereoMode::SideBySide => eye_pose(&head, eye.0, display.ipd),
        };
        *t = pose_to_transform(&p);
    }
}
