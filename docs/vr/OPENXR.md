# OpenXR for Bevy 0.20 — evaluation (VR-01)

Date: 2026-10-21. Checked on crates.io and the upstream repository.

| Crate | Latest | Bevy | Usable with our Bevy 0.20.0 |
| --- | --- | --- | --- |
| `bevy_mod_openxr` (bevy_oxr workspace) | 0.6.0 | `^0.19` | no |
| `bevy_mod_xr` | 0.6.0 | `^0.19` | no |
| upstream `main` (awtterpip/bevy_oxr) | 0.6.0 | `0.19` | no |
| `openxr` (raw bindings) | 0.22.0 | — | yes, but needs our own Vulkan/D3D swapchain interop with Bevy's wgpu renderer |

Bevy minor versions are not semver compatible, so the published OpenXR integration cannot be
pinned with Bevy 0.20 without forking it. Writing the graphics interop directly on `openxr`
(session with a Vulkan graphics binding sharing wgpu-hal's device, multiview swapchain images
as render targets, frame timing against `xrWaitFrame`) is the larger part of such a plugin
and cannot be verified without a headset, so it was **not** guessed at here.

## What is in place

* `gothic_vr::XrBackendKind::{Desktop, Scripted, OpenXr, External}` and
  `GOTHIC_VR_BACKEND=openxr`: recognised, logs a warning and uses the desktop fallback.
* The backend seam: anything that writes `PendingXrFrame(Some(XrFrame))` before
  `VrSet::Input` drives input, locomotion, IK, physics hands, melee and archery. Poses are in
  tracking space (metres, floor origin, -Z forward); the grip pose follows the OpenXR
  `grip` convention (-Z through the fist from little finger to thumb).
* Stereo: `XrEyeCamera(Eye)` entities with `rig::eye_pose`; the desktop preview renders them
  side by side (`--stereo`).

## Enabling a headset once Bevy 0.20 support exists

1. Add `bevy_mod_openxr` behind an `openxr` cargo feature of `gothic-vr` (pin exact versions,
   keep the default build headset-free).
2. In `VrPlugin::build`, for `XrBackendKind::OpenXr`, add its plugin group instead of
   `bevy_winit`'s window and map its action set to an `XrFrame`: head = view pose, hands =
   grip poses + linear velocity, trigger/grip/thumbstick/A/B/stick click/menu.
3. Let it own the eye cameras (projection, swapchain targets) and keep `XrEyeCamera` for
   other systems to find them.
4. SteamVR / Quest Link: start the runtime, then `GOTHIC_VR_BACKEND=openxr cargo run -p
   gothic-vr-sandbox --features gothic-vr/openxr`.
