# Research attribution

The G3V0 reader was informed by the file-layout documentation expressed in [G3Archive](https://github.com/gekonnn/G3Archive), consulted 2026-10-20, based on NicoDE's G3Pak specification. The implementation in this project is Rust. Preserve this notice for the consulted MIT-licensed work:

MIT License

Copyright (c) 2024 gekoN

Permission is hereby granted, free of charge, to any person obtaining a copy
of this software and associated documentation files (the "Software"), to deal
in the Software without restriction, including without limitation the rights
to use, copy, modify, merge, publish, distribute, sublicense, and/or sell
copies of the Software, and to permit persons to whom the Software is
furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in all
copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR
IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY,
FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE
AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER
LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM,
OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE
SOFTWARE.

# Souls-combat reference

`crates/combat-souls` (the opt-in Souls-style combat mode) was adapted from the structure of [ELDEN-RING-Playermodel-Rust](https://github.com/Funny-Bones/ELDEN-RING-Playermodel-Rust) at commit `177a3b5ec14a9bde39b9ab458a3400b4038224e3` (consulted 2026-10-21): the engine-free 60 Hz player state machine with frame-based actions, per-input cancel windows, one-slot input buffer, roll-on-release / hold-to-sprint, guard-counter window and hit-stop. The Rust code, action table, tuning values and enemy model in this repository are our own; no Elden Ring game data, animations or extracted numbers are used. Files derived from the reference (`crates/combat-souls/src/hero.rs`) keep a pointer to this notice. Preserve this notice for the MIT-licensed work:

MIT License

Copyright (c) 2026 Funny-Bones and contributors

Permission is hereby granted, free of charge, to any person obtaining a copy
of this software and associated documentation files (the "Software"), to deal
in the Software without restriction, including without limitation the rights
to use, copy, modify, merge, publish, distribute, sublicense, and/or sell
copies of the Software, and to permit persons to whom the Software is
furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in all
copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR
IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY,
FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE
AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER
LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM,
OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE
SOFTWARE.

Rust dependencies retain their respective licenses. No original Gothic 3 data is included in this repository. Local analysis output and extracted samples belong under ignored `.local/`.

# Physics dependency (VR)

`crates/vr-physics`, `crates/vr-melee`, `crates/vr-npc`, `crates/vr-archery`, `crates/vr-bridge` and `apps/vr-sandbox` use [avian3d](https://github.com/avianphysics/avian) 0.8.0 (MIT OR Apache-2.0) and, transitively, parry3d (Apache-2.0). Used as a library only; no source is vendored.

# Added crate dependencies and external tools (cloud integration)

New crates.io dependencies, all under permissive licences (MIT and/or Apache-2.0); no copyleft code is linked: `toml`, `ron`, `semver` (`crates/mods`), `rhai` and its transitive crates (`crates/mods-script`), `naga` (dev-dependency of `crates/env-synth`). Their licence texts ship with the crates and can be listed with `cargo license` / `cargo about`.

`ffmpeg` / `ffprobe` (`crates/video`) are **not linked or bundled**: they run as separate processes and are located at runtime (`GOTHIC_FFMPEG`, next to the executable, or `PATH`). If a release package ships ffmpeg binaries, it must include that build's own licence text and source offer (prefer an LGPL build; GPL builds additionally require offering the corresponding source of ffmpeg itself, not of this project). See `docs/VIDEO.md`.

# Added crate dependencies (integrate/cloud-2)

Pulled in via Cargo, not vendored; listed here for licence tracking. Full licence texts ship with each crate.

- `apps/modtool`: `toml` (MIT OR Apache-2.0), `semver` (MIT OR Apache-2.0), `zip` (MIT), `ed25519-dalek` (BSD-3-Clause), `getrandom` (MIT OR Apache-2.0).
- `crates/formats` (dev-dependency only): `proptest` (MIT OR Apache-2.0).
- `crates/formats/fuzz` (separate nightly workspace, not part of the default build): `libfuzzer-sys` (MIT OR Apache-2.0 OR NCSA).

# VR feel references (principles only)

The held-weapon, two-handed grip, bow and haptics behaviour of `crates/vr`, `crates/vr-physics`, `crates/vr-melee` and `crates/vr-archery` was designed after reading these open-source VR interaction systems (consulted 2026-10-22). No source code was copied; the Rust implementation, structure and most tuning values are our own. Values taken over as starting points are named in `docs/vr/PHYSICS.md` and the code comments.

- [SteamVR Unity Plugin](https://github.com/ValveSoftware/steamvr_unity_plugin) Interaction System (BSD-3-Clause, Copyright (c) Valve Corporation): longbow nock lead-in and hand-to-rest aiming, release and string haptics sequence, velocity-driven held objects.
- [Godot XR Tools](https://github.com/GodotVR/godot-xr-tools) (MIT, Copyright (c) 2018-2023 Bastiaan Olij and Contributors): two-handed grab driver (primary hand leads, secondary aims), rumble manager (strongest event wins).
- [VR Expansion Plugin](https://github.com/mordentral/VRExpansionPlugin) (MIT, Copyright Joshua Statzer): virtual hand inertia and per-grip constraint settings.
- [NewtonVR](https://github.com/TomorrowTodayLabs/NewtonVR) (MIT, Copyright (c) 2015 TomorrowTodayLabs): velocity-based physical held objects.

Physics-melee behaviour (blades stop at bodies, stabs pierce along the blade, recoil weakens the arm) follows what Blade & Sorcery and Boneworks are publicly known for; nothing of those games was decompiled or used.

# OpenXR backend (VR-07, optional feature)

`crates/vr` with the cargo feature `openxr` (off by default) uses [`openxr`](https://github.com/Ralith/openxrs) 0.22.0 and `openxr-sys` 0.14.0 (MIT OR Apache-2.0), `libloading` (ISC) and `libc` (MIT OR Apache-2.0). The Khronos OpenXR loader (`libopenxr_loader.so.1` / `openxr_loader.dll`, Apache-2.0) and the runtime (SteamVR, Meta, Monado, ...) are **not linked or bundled**: the loader is opened at run time from the system. CI installs Ubuntu's `libopenxr-loader1` and Monado (`monado-service`, `libopenxr1-monado`, Boost Software License 1.0) only to test against a simulated headset; nothing of them is redistributed.
