# Gothic Reforged continuation

Read `docs/HANDOFF_CODEX_20261022.md` first for the current continuation, then the historical
`docs/HANDOFF_CLAUDE.md`, `docs/PROJECT_SPEC.md` and `docs/ARCHITECTURE.md`.

The user explicitly authorized Codex to manage scoped parallel coding with cheaper models
(prefer `gpt-6-luna` for bounded coder tasks), conserve manager tokens, preserve Claude's work,
fix renderer/water/leaf and VR issues, and deliver an optimized local PC EXE with screenshots.
Use the isolated `codex/renderer-stability` checkout; do not reset the primary checkout or
Claude's integration worktree. Coordinate build caches and never benchmark under compiler load.

The user requested that Codex start a performant, modular Rust/Bevy Gothic 3 reimplementation, locate the installed game on D:, and leave an actionable handoff for Codex to continue from 11:00 Europe/Berlin on 2026-10-20.

Original installation: `D:\SteamLibrary\steamapps\common\Gothic 3`. Read-only source; all generated reports, samples and native analysis stay under gitignored `.local/`. Do not commit original content or decompiled bodies. The current runtime is a playable integration slice, not a complete Gothic 3 port; distinguish tested support from plans.

Use bounded independent Rust parsers and synthetic public fixtures. Native REA findings and retained evidence locations are in `docs/research/NATIVE_FINDINGS.md`. The real process is Gothic3Final.exe; Gothic3.exe is a launcher. Never infer gameplay parity from static exports or a successful build.
