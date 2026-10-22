# VR renderer audit and input gating

Stand: 2026-10-22. Scope: isolated Quest renderer copy in `D:/gr-wt/codex-renderer`, focused on OpenXR tracking/input validity and user-visible stale poses. Static source review only. No cargo build, test execution, headset access, deployment, or runtime behavior claim was made.

## Verified sources

- Meta Horizon OS Developers, [Input API](https://developers.meta.com/horizon/documentation/native/android/mobile-openxr-input): pose actions expose an `isActive` state separately from located-space `valid`/`tracked` flags; applications should check pose-action activation before locating/using controller poses. Action states can become inactive when the app loses focus, and inactive inputs should be ignored.
- Meta Horizon OS Developers, [Synchronizing and Submitting Frames](https://developers.meta.com/horizon/documentation/native/android/mobile-openxr-frames): frame work uses `xrWaitFrame`'s predicted display time, then begin/render/end; `shouldRender` controls whether a projection layer is submitted. This confirms the existing loop's broad ordering but does not certify its latency or frame pacing.
- These primary Meta pages were retrieved via `vr_docs_search` / `vr_docs_get_page` on 2026-10-22. The repository's separate `docs/research/VR_RENDERER_AUDIT.md` was not present when the isolated checkout was first inspected; this report records the source evidence used for the fix.

## Changes in this checkout

`crates/xr-quest/src/xr.rs` now records the current OpenXR session state and accepts gameplay input only while the session is `FOCUSED`. It checks `Action::is_active` for grip and aim pose actions before locating them, then requires both position and orientation validity for each pose. Grip and aim have separate active flags so presentation cannot treat a valid grip as proof of a valid aim pose. The last valid transforms remain cached for continuity, while the corresponding active flag stays false whenever the current sample is not usable.

Scalar and button actions pass through the same focus/action-active gate and become neutral when inactive. The thumbstick follows that gate as well. Queued haptics are consumed only while focused. `apps/quest/src/greybox.rs` now shows the pointer only when the aim pose is active; locomotion still consumes the grip pose only when the grip pose is active.

Pure unit tests exercise the actual pose-cache update helper across focus, pose-action activation, and both validity flags; verify invalid samples preserve the cached transform without activating it; and cover neutral action values during inactive actions or session focus loss. The tests were added but **not run** because the task prohibited Cargo builds/test runs.

## Remaining checks and limits

- Validate on a runtime/simulator that the pose action is inactive or flags are incomplete during controller tracking loss and focus transitions; source-level helpers cannot establish runtime behavior.
- Validate on device that the app receives FOCUSED events as expected and that hands disappear/inputs neutralize when a system overlay takes focus.
- The `D:/gr-wt/codex-renderer` tracked VR crate is the desktop mono/side-by-side emulator baseline, not an OpenXR headset backend, so it cannot serve as visual headset parity evidence.
- Static inspection found explicit metre/centimetre conversions and runtime-provided asymmetric eye poses/FOV in the reviewed path; this audit did not establish a defect in those areas.
- Missing static mesh/material loads still have quiet failure paths in `apps/quest/src/ardea.rs`; a diagnostic improvement was optional and not included in this narrow patch.

## Follow-up verification: hero host check and Quest read-only snapshot

On 2026-10-22, the freshly built `C:/Users/PowerTower/dev/gothic-reforged/.local/target-hostq/x86_64-pc-windows-gnu/release/examples/hero_check.exe` was run from the root repository working directory. Captured output: `D:/gr-wt/codex-renderer/.local/vr-hero-baseline.log`. The process returned exit code 0, but the example's own checks reported one failure: `HOLD finger axis vs grip frame (-Y): worst 24.50 deg (must be < 15): FAIL`. The torso-to-eye clearance was 0.110 m against a 0.080 m minimum; blade/fist alignment, palm normal, palm-to-grip position, and rotation stability checks passed. Treat this as a failed host assertion despite the zero process exit status. This executable uses Claude's existing build and does **not** contain the OpenXR input-gating changes from this isolated worktree.

The Quest 3 (serial `2G0YC1ZG1003WN`) was listed as connected. Read-only app info confirmed `dev.gothicreforged.questtest` is installed (version `0.1`, code `1`, last updated 2026-10-22 07:24:16). A tag-filtered `GothicQuest` logcat query returned no lines; a package/regex query failed inside logcat with exit 134, and a subsequent unfiltered 100-line read contained only system diagnostics rather than app-specific XR output. The read-only screenshot was black. No install, launch, proximity change, or other device mutation was performed. These observations do not establish that the current Quest app is running or crashing.

## Follow-up: 24.5° hand-axis result

The failure was in the diagnostic's measurement window, not evidence of a bad bind-pose grip frame. The old check measured the wrist-to-`Finger3_2` vector throughout the curl sweep, including fully curled fingers. This vector is expected to rotate as the proximal finger segment curls; the log's maximum 24.50° therefore mixed finger articulation with the hand-to-grip frame alignment. The mesh-derived sword frame independently reports blade/fist 0.00°, edge/finger 0.00°, and flat/palm-normal alignment 1.000. Retargeting derives its hand basis from the actual wrist, middle-finger, thumb, and little-finger bind positions, so the meaningful frame check is at zero curl.

`apps/quest/examples/hero_check.rs` now keeps the original strict `< 15°` limit for the open/bind finger axis, reports the maximum curled-axis angle as diagnostic output, and still checks palm placement and rotation continuity across the full 0..1 curl sweep. Every explicit geometric `FAIL` condition now contributes to a final nonzero process exit; `expect` failures already panic nonzero. No threshold was relaxed and no check was removed. The 24.5° host log predates this diagnostic correction; this source edit is not built or run here, so the revised result remains for the manager's host build to verify.

Suggested host build from the root project directory, targeting the isolated stand-alone Quest workspace (the manager's prepared `.local/qmine` copy):

```powershell
cargo +stable-x86_64-pc-windows-gnu build --manifest-path .local/qmine/Cargo.toml --target x86_64-pc-windows-gnu --target-dir .local/target-hostq --release --example hero_check
```

Then run the produced `.local/target-hostq/x86_64-pc-windows-gnu/release/examples/hero_check.exe` from the root project directory so it reads the expected `.local/quest/ardea-pack` assets. The example's summary must exit nonzero if any unchanged threshold fails. This verifies only host-side rig geometry and synthetic pose checks, not Quest runtime behavior.

For the pure Gating tests in this checkout, run from `D:/gr-wt/codex-renderer`:

```powershell
cargo +stable-x86_64-pc-windows-gnu test --locked -p gothic-xr-quest --target x86_64-pc-windows-gnu --target-dir .local/target-hostq
```

The isolated checkout keeps `crates/xr-quest` as a workspace member and excludes `apps/quest`, so this command runs the helper tests without pulling the vendored VR app crates into the same Cargo workspace. The command was **not run** during this verification.

## Quest combat API migration

The later Quest build log `.local/quest-host-build.log` failed with `E0432: unresolved import gothic_sim::combat::resolve_hit` at `apps/quest/src/combat.rs:10`. The old root checkout still has `combat::resolve_hit(cfg, rng, attacker, victim, attack)`, but the isolated/newer Sim integration no longer exports or implements that resolver; its remaining `combat.rs` entry-point comment is stale. The current integration exposes `Sim::resolve_ported(attacker_id, victim_id, &attack)` in `crates/sim/src/sim/hit_ported.rs`. That method delegates assessment to `gothic_script::damage::do_logical_damage` and applies the Script `DamageAmount` to the Sim victim's HP. It does not perform the VR entity's ragdoll/presentation aftermath, which remains in the Quest adapter.

`apps/quest/src/combat.rs` now owns a `Sim<Grid2dWorld>` and routes melee and arrow damage through `Sim::resolve_ported`. It synchronizes the existing Quest actor copies into Sim before resolution and copies the resulting victim state back, retaining the Quest-side death/ragdoll presentation. The initial synthetic fixture omitted `sim.hero` and used an unarmed actor; the Script host therefore did not recognize its attacker as the player, and its neutral-hit gate correctly dropped the hit. This was a fixture error, not evidence that `GothicCombat::load` omitted player registration: that loader does set `sim.hero = Some(hero_id)`. The tests now follow `crates/sim/tests/common.rs`'s established setup, including player registration, progression/perk, and synthetic sword equipment.

The runtime bridge review found a separate NPC-hit context gap: `new_orc` creates Sim actors without a Sim target, while `do_logical_damage` reads the attacker's current target. A physical `HitMessage` already identifies the actual victim, so the bridge now supplies that victim as the target for non-player hits before calling the Script damage pipeline. This adapts collision context; it does not change combat rules. Synthetic tests cover hero-to-orc, orc-to-hero with no preloaded target, and repeated hits. They verify each `HitOutcome::hp_lost` equals the visible and Sim-side HP delta, preventing missing synchronization or double subtraction.

The Quest source also uses `self.sim.cfg` for NPC construction after moving config ownership into Sim. The targeted host test command completed successfully (2 passed, 0 failed): `cargo +stable-x86_64-pc-windows-gnu test --manifest-path .local/qmine-codex/Cargo.toml --release --target x86_64-pc-windows-gnu -j2 --lib --locked`, using the prepared `quest-env.sh` and host target directory; full output is in `.local/quest-host-tests.log`. This is a host lib-test result, not an Android Quest build. The manager must still build the actual stand-alone Quest workspace after staging this change.

## Quest Android release lib build (verified 2026-10-22)

Canonical apps/quest/src and examples were hash-synchronized into standalone .local/qmine-codex.
Source integration base was 36fef36; the actual migrated source is preserved in commit434ee3a.
Exact source inventories: .local/quest-android-build.meta.txt. Standalone manifest/lock and
isolated vendor-codex copy were retained.

Command: cargo +stable-x86_64-pc-windows-gnu build --manifest-path .local/qmine-codex/Cargo.toml
--release --target aarch64-linux-android -j2 --lib --locked, after sourcing primary
.local/quest-env.sh; CARGO_TARGET_DIR points to primary .local/target-quest and
RUST_MIN_STACK=67108864. Build succeeded in4m08s, seven warnings/zero errors.
Log: .local/quest-android-build.log. Output:
C:/Users/PowerTower/dev/gothic-reforged/.local/target-quest/aarch64-linux-android/release/libgothic_quest.so
(104687840 bytes), SHA256 3b720252047aa76d1b319ce02b4fabb467caa5c96e60fd5204eedc0576a58968.
NDK llvm-readelf confirmed ELF64/AArch64 shared object and libc++_shared.so dependency.

No APK packaged, no deployment/device operation: expected SDK path and bounded additional
searches of primary .local/tools, .local/tools2, Program Files/Android, Meta Spatial Editor,
and AppData/Local/meta-xr-operator found no aapt2, zipalign, apksigner or android.jar. NDK alone
does not supply APK packaging tools. The previous installed Quest app remains untouched.
Actual worn-headset validation remains open.

Fresh host hero_check exited0 with grip/palm/blade/hand-rotation sweep checks PASS; full log
.local/vr-hero-final.log. Host example build finished in13m12s; host_shots then exited0 and
saved five actual PNGs under own .local/quest/host_*.png. These validate the host fixture,
not live controller input or the headset compositor. Manager separately ran XR helper tests:
3passed; earlier statements that these tests were not run are superseded by this result.
## Host avatar screenshots: missing head

The headless appearance in `host_third_front.png` is explained by render-layer setup. the original `host_shots` version assigned its screenshot camera only layer 0 (initialization remains at `apps/quest/examples/host_shots.rs:65`), while `apps/quest/src/actor.rs:182` puts any `hide_from_eyes` part on layer 1 only; the host fixture spawns the hero head with that option at `host_shots.rs:221`. The head asset itself was present in the same Ardea pack: the successful `hero_check` path calls `spawn_part(...).expect("hero head")` at `hero_check.rs:50`.

This explains the original headless screenshot as a fixture camera-mask omission, not evidence of a missing head asset or a Quest runtime failure. The hide-from-eyes behavior is intended for the player's eye cameras. The runtime Quest eye cameras are spawned without a `RenderLayers` component in `crates/xr-quest/src/lib.rs:265-274` (default layer 0), while the optional mirror camera explicitly includes layers 0 and 1 in `apps/quest/src/hero.rs:90-97`. This is consistent with hiding the player head from their own eye view while allowing it in the mirror. The host screenshot fixture mask has since been corrected below. Actual on-headset visibility remains unverified.

### Host screenshot mask follow-up

The fixture-only visibility mismatch is fixed in `apps/quest/examples/host_shots.rs`: third-person front/back cameras now include layers 0 and 1, while FPV, FPV-grip, and orc shots remain on layer 0. The canonical `examples` tree was copied to `.local/qmine-codex/examples` and its SHA-256 path/hash inventory matches exactly; `host_shots.rs` SHA-256 is `82C97BE5B35712D0B93A386EF6B0A20BDE9ECFBEA0D02C91CFB89C9162E064C4`. This only changes the host screenshot harness. No build or recapture was run here. The manager should rebuild/run `host_shots`, confirm the hero head appears in both third-person images, and confirm FPV remains head-free and the orc shot remains unchanged. This does not verify headset rendering.

### Host screenshot mask build and visual check

The isolated host example was rebuilt after fixing a RenderLayers move error by cloning the small layer-mask value inside the camera query. The retry succeeded in 7m23s with the requested stable GNU host target, --release -j2 --locked. The build log includes the first failed attempt and successful retry: .local/quest-host-shots-layer-build.log. The resulting executable is $exe (SHA-256 $exeHash). Running it from D:/gr-wt/codex-renderer exited 0 and saved all five screenshots; run log: .local/quest-host-shots-layer-run.log.

Visual inspection confirms the hero head appears in both third-person screenshots (.local/quest/host_third_front.png, .local/quest/host_third_back.png), remains excluded in the first-person view (.local/quest/host_fpv.png), and the Orc head remains visible (.local/quest/host_orc.png). The curled grip image was regenerated at .local/quest/host_fpv_grip.png. SHA-256 values:

host_third_front.png SHA-256=C08FF0CE54A450F61DC30055A8EE8FE22863AD11C458C6B8EBCDC9C5D5ECAF6A
host_third_back.png SHA-256=AFB95DA602546060A6A0D2BBA48E04F9FF6B7A082D5DE31FDEE966C82B8A5C46
host_fpv.png SHA-256=16EDB2994F9D092DE186F0D4BCF6B09EC26FBDE38DCA71A4AC0DEDFDE37E95A3
host_orc.png SHA-256=07831D0E2088FC3618FBB1423FEB7F017D7A435F342D9252834295A950F622D0
host_fpv_grip.png SHA-256=64D2EB67D36B63462102E5A6BA5C36D130333BAF733AE8FA49BB33265B426310

This verifies the host fixture's layer selection only. It supplies no new headset-runtime evidence.
