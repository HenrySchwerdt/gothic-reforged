# Gothic WESL checker

This standalone target validates Gothic shader edits without rebuilding the game. Build it once
after coordinated game builds are idle, then rerun it directly after WESL-only changes:

```powershell
cargo run --manifest-path tools/shader-check/Cargo.toml -- --repo .
```

On this Windows machine, use the prepared GNU environment from Git Bash:

```bash
source /c/Users/PowerTower/dev/gothic-reforged/.local/env.sh
cd /d/gr-wt/codex-renderer
export CARGO_TARGET_DIR=D:/gr-build/shader-check RUST_MIN_STACK=67108864
cargo +stable-x86_64-pc-windows-gnu run --manifest-path tools/shader-check/Cargo.toml --release --locked -- --repo .
```

After that first build, rerun `D:/gr-build/shader-check/release/gothic-shader-check.exe --repo D:/gr-wt/codex-renderer` directly. It reads the shader sources at runtime.

It registers the Gothic vertex and fragment shaders, the custom prepass shader and shared wind
module, plus Bevy 0.20 registry shader modules in Bevy's `ShaderCache`. It compiles the forward
vertex variant and forward fragment variants with and without visibility-range dithering, along
with prepass variants through Bevy's WESL compiler, then parses/validates the resulting WGSL with
the locked Naga version. Its `GR_NOWIND` cases keep `VERTEX_NORMALS` while omitting normal-prepass
output definitions, matching Bevy's mesh-layout specialization; the otherwise-unused vertex entry
is still type-checked. It also checks wind depth/shadow, compressed normals, missing mesh normals,
motion vectors, and a deliberate unbraced-guard mutation that must fail with
`invalid field accessor world_normal`.

The variants set Bevy's `MATERIAL_BIND_GROUP=3` and use representative desktop limits:
`AVAILABLE_STORAGE_BUFFER_BINDINGS=8` with matching `__GE_3`/`__GE_6` flags, plus Bevy's
`MAX_DIRECTIONAL_LIGHTS=10`, `MAX_CASCADES_PER_LIGHT=4`, and `MAX_RECT_LIGHTS=8`. The actual
storage-buffer limit is adapter-dependent and is not queried by this standalone tool.

The checker does not create a GPU device or render pipeline. It catches import, conditional-code,
WGSL type/validation, and field-access errors, but cannot validate resource binding layouts,
vertex-buffer layouts, or runtime silhouette parity. Keep periodic app/runtime smoke validation for
those checks. Registry sources are discovered under Cargo's default registry path; pass
`--registry-src PATH` when using a nonstandard `CARGO_HOME`.
