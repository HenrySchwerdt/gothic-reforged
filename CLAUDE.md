# Gothic Reforged continuation

Read `docs/HANDOFF_CLAUDE.md` first, then `docs/PROJECT_SPEC.md` and `docs/ARCHITECTURE.md`.

The user requested that Codex start a performant, modular Rust/Bevy Gothic 3 reimplementation, locate the installed game on D:, and leave an actionable handoff for Claude to continue from 11:00 Europe/Berlin on 2026-10-20.

Original installation: `D:\SteamLibrary\steamapps\common\Gothic 3`. Read-only source; all generated reports, samples and native analysis stay under gitignored `.local/`. Do not commit original content or decompiled bodies. Current runtime is a foundation, not a playable Gothic 3 port; distinguish tested support from plans.

Use bounded independent Rust parsers and synthetic public fixtures. Native REA findings and retained evidence locations are in `docs/research/NATIVE_FINDINGS.md`. The real process is Gothic3Final.exe; Gothic3.exe is a launcher. Never infer gameplay parity from static exports or a successful build.

VR mode (issues #2-#8): `crates/vr*` and `apps/vr-sandbox`, see `docs/vr/README.md`. Built and tested with synthetic greybox content only; headsets need an OpenXR backend that does not exist for Bevy 0.20 yet (`docs/vr/OPENXR.md`).
