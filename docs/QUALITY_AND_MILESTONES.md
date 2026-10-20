# Quality Strategy & Milestones

## Baseline

Before coding the entire game, produce an inventory of installation formats, CP patch versions, locales, relevant license constraints and known community bug reports. Record confidence, evidence and ownership for every claim. The German installation is the first supported test fixture source; proprietary files remain local.

## Milestones

### M0 — Foundation and research
- Rust workspace with CI, lint/format/test/build, license policy, design docs.
- Installation scanner, reliable fingerprinting, German localization/voice inventory.
- Source-format and patch-compatibility risk register.
**Exit:** actionable import roadmap and repeatable scanner report.

### M1 — Ardea vertical slice (faithful)
- Real original Ardea terrain/world geometry/materials placed from original records.
- Nameless hero, skeleton, animations, camera, locomotion and collisions.
- Real NPC placements and routines; at least one imported interaction/dialogue.
- A minimal save/reload scenario and an asset-coverage report.
**Exit:** interactive slice running entirely in Bevy, not screenshots or placeholder geometry. Explicitly list any unfinished systems.

### M2 — Systems parity
- Quests/conditions/actions, dialogue, inventory, equipment, vendors, skills, reputation, magic, combat, AI, audio, save/load and world streaming.
- CP baseline fixes mapped to normalized data and runtime behavior.
**Exit:** subsystem contracts and automated regression coverage; no dummy features represented as finished.

### M3 — World and narrative completeness
- Myrtana, Varant and Nordmar with all required locations and content.
- End-to-end playthrough across main story branches; tested side quests and NPC schedules.
**Exit:** auditable compatibility matrix and complete-playthrough evidence.

### M4 — Stabilization
- Correct and document verified original/CP defects in new implementation.
- Performance, stability, platform validation, save migration and robust errors.
**Exit:** release candidate subject to rights review; performance targets established through profiling, not assumptions.

### M5 — Remaster (future)
- Material/mesh/rig/animation upgrade pipeline, lighting/vegetation/LOD improvements, modern HUD/input/combat/magic as opt-in or configurable implementations.
**Exit:** measurable visual quality and gameplay improvements without losing faithful mode.

## CI and tests

- Unit tests on parsers, state machines, math, quest predicates, localization and serializations.
- Integration tests on read-only import output and cross-system state transitions.
- Golden fixture checks and deterministic asset graph hashes; licensed-free synthetic fixtures for public CI.
- Regression registry for verified CP fixes and independently found bugs, with steps, source evidence, root cause, verification and impacted versions.
- Representative save/load loops, malformed-input fuzzing where feasible, large-world streaming/memory profiling.
- Manual and automated smoke tests on claimed OS/device/GPU combinations.
- Generate browsable HTML reports for import coverage, localization missing keys, regression status and benchmark trends.

## Quality gates

Do not merge nontrivial feature work without tests and documentation, do not classify unsupported formats as supported, do not silently fall back on invented content, do not mark fixes resolved without reproducer and verification. Every milestone requires a demonstration and issue list with known deficiencies.
