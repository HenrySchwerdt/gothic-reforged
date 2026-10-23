//! VR foundation for Gothic Reforged (issues VR-01 and VR-02).
//!
//! * [`input`]: Bevy-free model of tracked head/hands/buttons with edge detection.
//! * [`desktop`]: keyboard/mouse headset emulator (the desktop fallback) and input scripts.
//! * [`bindings`]: controller buttons -> the game core's `Command`s.
//! * [`rig`]: stereo eye poses and viewports.
//! * [`locomotion`]: comfort locomotion driving `gothic_physics::CharacterController`.
//! * [`body`]: full-body IK from head + hands on a generic humanoid skeleton.
//! * [`greybox`]: synthetic test level (no game content).
//! * [`haptics`]: controller vibration (pulses from gameplay, merged per hand).
//! * [`openxr`]: OpenXR tracking / controller input and haptics (cargo feature `openxr`).
//! * [`plugin`] / [`render`]: Bevy systems, resources and the debug presentation.
//!
//! The backend seam is [`plugin::PendingXrFrame`]: anything that writes an [`input::XrFrame`]
//! there drives the whole stack. Tracking space and Bevy world are metres; the Gothic
//! character controller and collision world are centimetres ([`CM_PER_M`]).
pub mod bindings;
pub mod body;
pub mod desktop;
pub mod greybox;
pub mod haptics;
pub mod input;
pub mod locomotion;
pub mod openxr;
pub mod plugin;
pub mod render;
pub mod rig;

pub use gothic_physics;
pub use haptics::HapticPulse;
pub use input::{ControllerState, Hand, Pose, VrButton, VrInput, XrFrame};
pub use plugin::{
    DesktopXr, HapticMessage, HapticOut, PendingXrFrame, VrBody, VrCommands, VrDisplay, VrInputRes,
    VrPlayer, VrPlugin, VrSet, VrWorld, XrBackend, XrBackendKind, XrEyeCamera, XrHand, XrHead,
};
pub use render::VrRenderPlugin;

/// Gothic world units (cm) per metre.
pub const CM_PER_M: f32 = 100.0;

/// Bevy transform of a pose.
pub fn pose_to_transform(p: &Pose) -> bevy::prelude::Transform {
    bevy::prelude::Transform::from_translation(p.position).with_rotation(p.rotation)
}

/// Pose of a Bevy transform (scale ignored).
pub fn transform_to_pose(t: &bevy::prelude::Transform) -> Pose {
    Pose::new(t.translation, t.rotation)
}
