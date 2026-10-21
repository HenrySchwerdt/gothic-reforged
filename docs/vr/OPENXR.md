# OpenXR backend (VR-01 evaluation, VR-07 backend)

## Evaluation

Date: 2026-10-21. Checked on crates.io and the upstream repository.

| Crate | Latest | Bevy | Usable with our Bevy 0.20.0 |
| --- | --- | --- | --- |
| `bevy_mod_openxr` (bevy_oxr workspace) | 0.6.0 | `^0.19` | no |
| `bevy_mod_xr` | 0.6.0 | `^0.19` | no |
| upstream `main` (awtterpip/bevy_oxr) | 0.6.0 | `0.19` | no |
| `openxr` (raw bindings) | 0.22.0 | — | yes |

Bevy minor versions are not semver compatible, so `bevy_mod_openxr` cannot be used with Bevy
0.20 without a fork that ports its render-graph and wgpu-hal interop to 0.20 — a large patch
to carry and re-port on every Bevy update, and the part we could least verify without a
headset. The raw `openxr` crate has no Bevy dependency, so we build on it:

* **Input and tracking** (done, this backend): an OpenXR session without graphics
  (`XR_MND_headless`), an action set for both controllers and the view / grip spaces in the
  stage space, polled once per frame into the existing `XrFrame` seam. Verified against
  Monado (simulated HMD) locally and in CI.
* **Rendering into the headset** (not done): a session with `XR_KHR_vulkan_enable2` sharing
  wgpu-hal's Vulkan instance/device, swapchain images wrapped as wgpu textures for the two
  `XrEyeCamera`s, `xrWaitFrame` / `xrBeginFrame` / `xrEndFrame` around Bevy's render. This
  needs Bevy's `RenderCreation::Manual` path and cannot be judged without a real headset; the
  Quest build renders through its own app (`apps/quest`, local).

## Using it

```sh
# any OpenXR runtime (SteamVR, Meta Quest Link, WMR, Monado)
GOTHIC_VR_BACKEND=openxr cargo run -p gothic-vr-sandbox --features gothic-vr/openxr
cargo run -p gothic-vr --features openxr --example openxr_probe   # print tracking, exit code
```

* Feature `openxr` of `gothic-vr` (off by default): `openxr` 0.22 with the `loaded` feature —
  the system loader (`libopenxr_loader.so.1` / `openxr_loader.dll`) is opened at run time,
  so building needs no OpenXR SDK and the default build is unchanged.
* `VrPlugin { backend: XrBackendKind::OpenXr }` (or `GOTHIC_VR_BACKEND=openxr`) starts
  `openxr::OpenXrInput`; on success `XrBackend { kind: OpenXr, note }` names the runtime and
  system, otherwise the desktop fallback runs and the note says why (no loader, no runtime,
  no headless support, ...).
* Mapping (`crates/vr/src/openxr.rs`): head = `VIEW` located in `STAGE` (fallback: `LOCAL`
  shifted 1.7 m down); hands = `grip/pose` + linear velocity; trigger, squeeze, thumbstick
  (Vive: trackpad), A/X = primary, B/Y = secondary, stick click, menu. Suggested bindings for
  Oculus Touch, Valve Index, HTC Vive and the KHR simple controller (`PROFILES`, tested in
  every build). Time comes from `XR_KHR_convert_timespec_time` (Linux) or
  `XR_KHR_win32_convert_performance_counter_time` (Windows).
* The headset itself shows the runtime's idle scene while the desktop window mirrors the rig;
  everything downstream (locomotion, IK, physics hands, melee, archery, HUD) runs on the real
  tracking.

## Testing without a headset (Monado)

```sh
sudo apt-get install libopenxr-loader1 libopenxr1-monado monado-service mesa-vulkan-drivers xvfb
mkdir -p /tmp/xdg && chmod 700 /tmp/xdg
# monado-service polls stdin: give it a pipe
sleep 600 | XDG_RUNTIME_DIR=/tmp/xdg VK_ICD_FILENAMES=/usr/share/vulkan/icd.d/lvp_icd.json \
  xvfb-run -a monado-service &
XDG_RUNTIME_DIR=/tmp/xdg cargo run -p gothic-vr --features openxr --example openxr_probe
XDG_RUNTIME_DIR=/tmp/xdg GOTHIC_VR_EXPECT_RUNTIME=1 cargo test -p gothic-vr --features openxr --test openxr
```

Monado's simulated HMD has no controllers (hands report no pose); head tracking, the session
lifecycle, action sync and the stage space are exercised. CI runs the same steps (`openxr`
job in `.github/workflows/rust.yml`); the `check` job's clippy builds the feature on Windows
and Linux.

## Backend seam (unchanged)

Anything that writes `PendingXrFrame(Some(XrFrame))` before `VrSet::Input` drives input,
locomotion, IK, physics hands, melee and archery (`XrBackendKind::External` for tests and
recordings). Poses are in tracking space (metres, floor origin, -Z forward); the grip pose
follows the OpenXR `grip` convention.
