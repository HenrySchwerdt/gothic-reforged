# Gothic Reforged

Open-source Rust/Bevy reimplementation of Gothic 3, using players' locally installed game data.

**Status:** M0 implementation started. Unofficial fan project, not endorsed by the rights holders. This is not yet a playable Gothic 3 port.

The goal is to first reproduce the complete Community-Patched Gothic 3 experience, including original German localization, in a portable Rust/Bevy runtime. Later, replaceable plugins may introduce modern combat, UI, movement and graphical remastering.

The project must not distribute proprietary Gothic assets, dialogue, voice-over or patch payloads. The legality of a game-specific release and reverse engineering must be assessed before distribution.

The workspace contains a bounded G3V0 archive reader, read-only installation scanner, CLI inspector, and a Bevy streaming laboratory. The graphical viewer uses a synthetic debug grid; original world geometry, gameplay and localization are not implemented.

Requires Rust 1.98+. Bevy is pinned to 0.20.0; graphical compilation is opt-in.

On this Windows workstation use `./scripts/dev.ps1 test`, `./scripts/dev.ps1 headless`, or `./scripts/dev.ps1 viewer`. The wrapper selects the already installed GNU Rust toolchain and the project-local LLVM-MinGW tools; the system's default MSVC toolchain lacks `link.exe`. On a machine with a complete native Rust toolchain, the standard commands below apply.

```powershell
cargo test --workspace --locked
cargo run -p gothic-inspector -- scan "D:\SteamLibrary\steamapps\common\Gothic 3"
cargo run -p gothic-inspector -- list "D:\SteamLibrary\steamapps\common\Gothic 3\Data\Quests.pak"
cargo run -p gothic-inspector -- ximg-dds "D:\SteamLibrary\steamapps\common\Gothic 3\Data\_compiledImage.pak" "_Intern/blue.ximg" ".local/blue.dds"
cargo run -p gothic-reforged
cargo run -p gothic-reforged --features viewer
```

Viewer controls: WASD movement, hold right mouse to look, Space/Ctrl vertical, Shift faster. Close the window to exit. `./scripts/dev.ps1 smoke` captures `.local/viewer-smoke.png` and exits after 240 frames. No original files are modified. Reports and extracted samples belong in ignored `.local/`; CLI extraction creates only a new caller-selected file. `ximg-dds` handles the observed v1 DXT1/3/5 texture subset; other resource variants fail explicitly.

Continue with [Claude handoff](docs/HANDOFF_CLAUDE.md), [native findings](docs/research/NATIVE_FINDINGS.md), [archive format](docs/research/G3V0_FORMAT.md), and the existing product and architecture specifications in `docs/`.
