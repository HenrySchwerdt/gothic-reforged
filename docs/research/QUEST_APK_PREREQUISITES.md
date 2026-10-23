# Isolated Quest APK packaging checkpoint — 2026-10-23

This is a read-only packaging audit, not an APK build, device check or Store-policy
validation. The previous Android native library and host fixture checks remain valid
within their documented scope. Do not repeat them simply because packaging is blocked.

## Current script cannot be used unchanged

`scripts/quest/build-apk.sh` first sources this checkout's `.local/quest-env.sh`,
which is absent. The primary checkout's similarly named file hardcodes its own root
and Cargo target directory. Sourcing that file is not an isolated packaging solution:
it changes `R` as well as output paths back to the primary checkout.

The script invokes Cargo with `-p gothic-quest`, without selecting the standalone
workspace. The root workspace does not contain `apps/quest`. The prepared workspace
is `.local/qmine-codex/Cargo.toml`, with its own `[workspace]` and lockfile. Its
manifest defines release, but does not define the script's advertised profiling
profile. Its staged Android manifest must be hash-checked against the canonical
`apps/quest/android/AndroidManifest.xml` before packaging.

## Exact local dependency findings

The configured SDK is `C:/Users/PowerTower/AppData/Local/Android/Sdk`; the directory
was absent at this check. Therefore the script's exact inputs below were absent:

- `build-tools/35.0.0/aapt2.exe`
- `build-tools/35.0.0/zipalign.exe`
- `build-tools/35.0.0/apksigner.bat`
- `platforms/android-34/android.jar`

These versions describe the existing script, not current Meta submission requirements.
No policy-driven manifest or target-SDK change was made.

The script also references NDK r28c, its `llvm-strip.exe` and `libc++_shared.so`,
and an OpenXR loader under this checkout's `.local/tools/`. These paths are absent
here. The corresponding NDK and loader files exist under the primary checkout and
may be treated as read-only inputs in a deliberately isolated future pipeline;
their existence does not justify sourcing an environment that redirects outputs.
The hardcoded JDK keytool exists, but the signing process was not run or validated.

The previously built primary-path native library still exists at
`C:/Users/PowerTower/dev/gothic-reforged/.local/target-quest/aarch64-linux-android/release/libgothic_quest.so`
(104,687,840 bytes). Its prior verified hash and build provenance are recorded in
`VR_RENDERER_AUDIT.md`. This audit did not rehash or rebuild it and does not establish
that it matches any newer source changes. No APK was produced.

## Requirements before a future packaging attempt

1. Supply explicit standalone manifest, NDK, SDK, OpenXR loader, JDK and D-side
   output paths. Keep all generated files and temporary signing material inside
   this checkout's ignored `.local/`; preserve the delivered packages and primary.
2. Check required files before Cargo or any mutation. Use release unless the
   standalone profiling profile is deliberately defined and validated. Do not
   infer the profiling artifact directory from a profile name without checking it.
3. Either select a source-inventoried existing `.so` with verified provenance, or
   intentionally build via the standalone manifest and lockfile into an isolated
   target directory. Native library success alone is not APK validation.
4. Package into a fresh output directory; verify native dependencies, alignment,
   signature and packaged manifest before considering device operations. Do not
   reuse the old script's destructive staging sequence against a computed path.
5. Only when device work is actually needed, inspect the current metavr-managed
   device inventory and select the intended device. No device was queried here;
   there is no new evidence about installed apps, controllers or headset rendering.

Do not repeat the filesystem search or native build while these inputs remain
unchanged. No tools were installed or configured during this audit.

## Documentation verification

The manager read `hz-quest-verify-first` and used Meta's `vr_docs_search` and
`vr_docs_get_page` on 2026-10-23. The official [Android Development Software Setup](https://developers.meta.com/horizon/documentation/native/android/mobile-studio-setup-android)
lists SDK platform/build tools and NDK as separate setup components. Its returned
page includes legacy API/version examples; those are not evidence of current Store
minimums. This checkpoint intentionally makes no new compatibility or policy claim.
