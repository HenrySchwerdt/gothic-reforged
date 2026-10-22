# Quest 3 feasibility (native Bevy 0.20 + OpenXR)

Status 2026-10-21: greybox runs on a Meta Quest 3 (Horizon OS, Android 14) with head and controller tracking at a steady 72 FPS. Ardea content is NOT yet measured (see "Open").

## Stack (all verified on device)
* `crates/xr-quest`: OpenXR (`openxr` 0.22, Khronos loader 1.0.34 from the official AAR, sha1 checked) -> Vulkan instance/device created by the runtime through `XR_KHR_vulkan_enable2` -> adopted by wgpu 30 via `wgpu::hal::vulkan::{Instance::from_raw, Adapter::device_from_raw}` -> Bevy `RenderCreation::manual`. Per-eye swapchains (sRGB RGBA8) wrapped as `ManualTextureView`s, two eye cameras with asymmetric reverse-Z projection from the runtime FOV. Custom frame loop (`xrWaitFrame -> Begin -> locate views/sync actions -> App::update -> release -> xrEndFrame`). No winit/window.
* `apps/quest`: NativeActivity cdylib (`android-activity`), greybox scene, smooth locomotion (left stick, head-relative) + snap turn (right stick), controller aim rays.
* Packaging: `scripts/quest/build-apk.sh` (cargo + NDK r28c, aapt2, zipalign -P 16, apksigner with a local debug key; no gradle). `scripts/quest/run.sh` builds, installs and launches. Toolchains live under `.local/tools` (NDK sha1 verified against Google's repository manifest).
* Manifest per Meta docs: `com.oculus.intent.category.VR` + `org.khronos.openxr.intent.category.IMMERSIVE_HMD`, `android.hardware.vr.headtracking` v1 required, `com.oculus.supportedDevices`, landscape, black fullscreen theme (docs: mobile-native-manifest, mobile-openxr; loader >= 1.0.34 required on Quest).
* Bevy 0.20 has no multiview in its pipelines (`multiview_mask: None` everywhere): the scene is submitted once per eye (2x draw calls). A multiview path needs a custom renderer or Bevy patches.

## Black flicker (user report) - root cause and fix
Symptom: black frames on both eyes, constantly. Not visible in OVR/compositor stale-frame metrics because the app delivered a frame every cycle; the frames were just empty (clear colour only).
Measurement (headset screenshots, `scripts/quest/shots.py`, a frame counts as black when max luminance < 20):
| Config | black frames |
|---|---|
| Bevy default (GPU indirect drawing + GPU culling) | 4/8 and 2/3 (about 50 %) |
| `NoIndirectDrawing` on the eye cameras | 0/8, then 0/16 on the final build |
Perfetto (`--xr-runtime`): compositor ran at 72 Hz in both traces; stale frames for the app: 9 in 6 s before (startup hitches only), 0 in 8 s after. xrWaitFrame/xrEndFrame cadence was correct all along, so the OpenXR layer submission, swapchain ordering and refresh rate were ruled out (checked against Meta docs: swapchains `mobile-openxr-swapchains`, missed frames `os-missed-frames`).
Root cause: Bevy's GPU culling/indirect-draw path (compute-generated indirect draw buffers) produces empty draw lists on roughly half the frames on this Adreno driver when rendering two views per frame. Fix: `NoIndirectDrawing` on both eye cameras (default in `XrPlugin`; `setprop debug.gr.noindirect 0` re-enables it for experiments). GPU mesh preprocessing stays on.
CPU update ~6.3 ms, 72.0 FPS locked (refresh rate requested explicitly: the runtime defaulted to 90 Hz and fell back to 72).

## Update 2026-10-21 (later)
* Ardea loader written (`apps/quest/src/ardea.rs`, `examples/pack_ardea.rs`): cooked quest cache subset (36 cells, 5255 instances, 444 meshes, 151 ASTC textures = 117 MB, pushed to `/sdcard/Android/data/dev.gothicreforged.questtest/files/ardea` with `adb push` of one tar; use `MSYS_NO_PATHCONV=1` in Git Bash). The app builds and installs, but with the world data present the OpenXR session ends right after start (no scene log); root cause not yet found, so Ardea numbers are still missing. Greybox is forced with `setprop debug.gr.greybox 1`.
* Locomotion now uses `gothic-physics` (`MovementProfile::hero()`, jump 600 cm/s, gravity 981) with A = jump. Meta's locomotion guidance (design/locomotion-input-maps) only specifies left stick = move and right stick = turn; A for jump is the common convention, not a Meta rule.
* Cloud VR crates (`vr`, `vr-physics`, `vr-melee`, `vr-archery`, `vr-npc`) extracted to `.local/vendor` (not integrated yet). Integration plan: backend `External`, write `PendingXrFrame` from `XrFrameData`, drive eye cameras from `VrPlayer::world`, attach cooked weapon/hero meshes (`g3_weapon_1h_sword_orcslayer_01`, actor `it_weapon_ranged_bow_hunting_01`, `g3_hero_body_player` + `g3_head_hero_hero_01`) to the physics props; hero skeleton node names are known (`Hero_Spine_Spine_1..5`, `Hero_Left_Arm_Arm_1/2`, `Hero_Left_Hand_Hand_1`, ...).

## Ardea measurements (Quest 3, 72 Hz requested, full recommended eye resolution, `scripts/quest/bench.py`)
Scene: 32 cells around the plaza, 3161 mesh entities (2.2 M triangles spawned in total, about 440-880 k visible depending on view, 530-1180 visible meshes), terrain + statics, no vegetation, no NPCs, no shadows, MSAA off.
| Config | FPS | App CPU (`App::update`) | Note |
|---|---|---|---|
| Bevy `StandardMaterial` (PBR) | 24.5 | 6.1 ms | GPU-bound (xrWaitFrame 34 ms) |
| same, render scale 0.7 / 0.5 | 41.8 / 62.8 | 6.0 / 7.2 ms | fragment-bound |
| `StandardMaterial` unlit | 50.4 | 6.9 ms | texture fetch + bandwidth still heavy |
| unlit, scale 0.7 | 72.0 | 7.0 ms | |
| Custom cheap shader (`quest.wesl`, original gamma-space lighting, 2-layer terrain blend, fog) | 70.9-72.0 | 7.2-9.2 ms | full resolution |
Findings: the frame is fragment-bound, not triangle- or draw-call-bound at this scale; replacing PBR by the ported original lighting model (SH ambient + one sun, no clustering/shadows/prepass) is the largest single win (about 3x). App CPU 7-9 ms of the 13.9 ms budget on the main thread (draw submission is on the same thread because pipelined rendering is off); this is the next limit when vegetation and NPCs are added.
Bugs found on the way: (1) wgpu rejects ASTC textures whose base size is not a multiple of the block size; the cooked 512/1024 textures are padded to 6-multiples and the UVs rescaled; the default Bevy render-error handler ends the session on that validation error (that was the 'session exit'). (2) C++ (meshopt) needs libc++_shared.so in the APK. (3) STAGE space never recenters; LOCAL_FLOOR is used (Meta migration guide: local space is redefined on system recenter), with a fallback to LOCAL + 1.6 m.

## Update (LOD, textures, collision)
* Blurry near textures: the cache contains 28 whole `*_lowpoly.xcmsh` proxy meshes whose material is the `g3_world_lowpoly__static_objects_*` atlas; they were drawn on top of the real houses. They are now skipped (same rule as the desktop loader). The ground additionally lacked the original detail (x2 modulated second texture) maps and the second layer of the terrain blend: both are in the Quest shader now. LOD stages and small-object culling use `gothic-render-rules` (`lod::stage_distances`, `cull::distance_band`) with 5 % hysteresis, evaluated per frame against the eye position.
* Collision: terrain collision triangles of each cell plus the original per-prop collision meshes (`collision.gcol`, xnvmsh) are inserted into the `gothic-physics` world used by the hero `CharacterController` (cm). Walk-into-wall test on device (autowalk 15 s from the plaza): blocked after 7 m / 10 m in two directions, 20 m and 57 m in the others, versus 110 m with no prop collision. Edge of the streamed area acts as an invisible wall instead of letting the capsule fall; a head inside geometry fades the view to black (Meta, design/locomotion-virtual-environments: disable rendering in invalid spaces).
* Cost: the controller step against prop collision costs 1-4 ms while moving (up to about 6 ms when pushing against complex walls); it runs at 40 Hz with interpolation. Needs a BVH/coarser collision for the final plan.
* Not tested on device: barrels, NPC capsules (no NPCs yet), stairs.

## Open
Ardea from the cooked `quest` cache, draw-call/triangle/GPU numbers, thermals, and the plan to reach 72 FPS there: not done yet.
