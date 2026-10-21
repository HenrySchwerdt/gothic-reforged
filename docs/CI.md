# CI (`.github/workflows/rust.yml`)

| Job | Runners | What |
|---|---|---|
| `fmt` | Linux | `cargo fmt --all --check`. Informational (`continue-on-error`) until #37 lands one formatting commit. |
| `check` | Windows, Linux | `clippy --all-targets --all-features -D warnings`, then `cargo test`, for the whole workspace **except** the VR crates. |
| `vr` | Windows, Linux | The same for the six VR crates, plus the headless desktop-fallback runs (`gothic-vr` greybox, `gothic-vr-sandbox` duel; exit code = result). |
| `release` | Windows, Linux | `cargo build --release -p gothic-reforged --features viewer`, uploaded as an artifact (no game data). |

All jobs use `--locked`. Tests that need the installed game skip themselves when it is
missing, so nothing here needs game data.

**Caching** (`Swatinem/rust-cache`, one key per job kind):
- Caches are **saved only from `dev/engine-continuation`**. PR runs restore the integration
  branch's cache and don't fill the 10 GB repository quota with per-branch copies.
- `cache-on-failure` is on: as long as some lint or test is red, a failing job still leaves a
  warm cache. Before this, every run started cold ("No cache found") because the failing
  `check` job never reached the save step.

**Order:** clippy runs before the tests in `check` and `vr`, so a lint failure shows after the
clippy build (minutes) instead of after the full test build.

## Measured cost (cold, no cache; run 39596802235, 2026-10-21)

| Step | Linux | Windows |
|---|---|---|
| free disk space | 3:21 | — |
| `cargo test --workspace` (VR included) | 18:10 | 40:47 |
| `cargo clippy` after the tests (stopped at the first lint error) | 2:40 | 7:21 |
| release build of the game | 12:38 | 25:52 |

Splitting VR out lets its avian3d tree build in parallel with the rest instead of inside the
40-minute Windows test step. The headless VR runs take seconds once built (fixed 90 Hz
steps; see `docs/vr/README.md`). With a warm cache, only the workspace crates rebuild.
Update these numbers after the first warm runs on the integration branch.
