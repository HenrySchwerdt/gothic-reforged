# Install Detection, Data Import, Localization & Patches

## Supported installation discovery

Support explicit directory selection first; progressively add verified Steam/GOG discovery on Windows/Linux/macOS. Scan read-only and produce a report of file formats, hashes/signatures, language resources, patch status and unsupported/ambiguous installations. Never assume an English installation or a fixed directory layout.

## Asset and gameplay import

1. Discover all archives and file relationships; compare findings against existing documented format tools, respecting source licenses.
2. Implement bounded, validated Rust parsers; use synthetic fixtures checked into Git.
3. Build a canonical stable-ID asset graph for meshes, materials, textures, skeletons, animations, collisions, terrain, world object placements, items, quests, NPCs, routines, dialogue, audio and scripts.
4. Resolve dependencies and detect missing, duplicate or inconsistent IDs.
5. Convert assets locally into versioned cache formats suited to Bevy; conversion must preserve transform semantics, handedness, coordinate scales, rigs, weights, localization and source provenance.
6. Emit coverage, warnings, errors, hash and performance reports; fail explicitly for corrupt or unsupported inputs.

Do not treat third-party reverse engineering libraries as guaranteed complete or compatible. Validate formats and licenses first.

## German-first, locale-neutral behavior

- Detect actual installed text tables and spoken audio and their supported language identifiers.
- Game scripts reference localization IDs, not source-language text values.
- Maintain independently selected text/subtitle and spoken-audio locale, limited to available resources.
- Missing strings/audio must be reported; a fallback can be used only when present and configured, not fabricated.
- Localize new UI separately and test accented letters, Unicode, wrapping and fonts.

## Community Patch baseline

- Target observed content and intended behavior of Community Patch **1.75.14**, subject to validation against installation evidence.
- An installation may already be patched; compare fingerprints/manifests before applying any change.
- Patch acquisition and execution require approved rights, trust validation and explicit user consent.
- Stage patched copies; never modify the original installation automatically.
- Make the normalized, effective dataset reproducible; include original game fingerprint, patch provenance, locale, conversion tool version and content hashes.
- Research additional Update Pack and Parallel Universe Patch changes *separately*; never stack blindly or redistribute protected patch payloads.
- Port behavioral corrections as data rules and regression tests; replace original engine-specific performance fixes with suitable Bevy implementations.

## Fix registry (proposed format)

```toml
id = "G3-FIX-EXAMPLE"
category = "quest"
source = "research"
status = "investigating" # investigating | reproduced | fixed | verified
affected_versions = ["example-only"]
evidence = ["docs/research/example.md"]
reproduction_test = "example_regression"
```

This is an **illustrative schema**, not a claim about any verified game bug.

## Reverse-engineering discipline

Document file signatures, expected values, ambiguous semantics, and observed results. Decompiled output may misrepresent compiler intent; do not translate it directly into Rust or label anomalies as bugs without reproduction. Implement safe, deterministic and well-tested equivalents. Observe legal limits for interoperability work and keep unsupported data private.
