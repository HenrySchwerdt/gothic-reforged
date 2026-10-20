# Gothic Reforged — Product & Compatibility Specification

**Status:** design proposal; no implementation or compatibility is claimed.  
**Primary target:** Gothic 3 (PC), installed by the user, with the Community Patch 1.75.14 behavior/content baseline (subject to verification).  
**Technology:** Rust workspace and Bevy. Target Windows, Linux, and macOS, with platform support gated by automated and manual testing.

## Vision

Build an independent, native Rust/Bevy reimplementation of **the entire original Gothic 3**, using local legally acquired game data. The first shipping objective is a complete, stable, playable and faithful reconstruction of the game including the world, main and side quests, NPC routines, combat, magic, inventory, dialogue, factions, audio and save/load. Improvements must avoid confirmed engine defects. **A visual overhaul and modernized mechanics are later phases**, not prerequisites for faithful playability.

## Priorities and non-goals

1. Analyze supported installations, game formats and known patches; establish reproducible baseline.
2. Import the full game and its available localization; implement original game systems and test them.
3. Match intentional Community-Patched gameplay while addressing verified defects, preserving all important quest and script behavior.
4. Achieve a verified end-to-end playthrough, not merely a visually accurate demo.
5. Only then remaster world/characters/animation/rendering and offer alternative combat, control, magic and HUD modes.

Initially **do not** ship redesigned magic, Souls-style combat, AI-generated art, Gothic 1 Remake meshes, or a new HUD in lieu of finishing the original game. Never label speculative or stubbed mechanics as implemented.

## Definition of fidelity

- Geography, placements, navigation, interactions, items, factions, skills, NPCs, schedules, quests, dialogue, reputation, trading and crafting derive from original or permitted patched game data.
- Retain stable source IDs and references independent of Bevy entity IDs.
- Preserve intended game mechanics and quest outcomes; do not canonize engine crashes, leaks, invalid data, broken quests or accidental exploits as specification.
- Treat ambiguous behavior as a documented research question. A decompiled fragment is neither definitive intended behavior nor proof of a bug.
- All story paths and meaningful state transitions require evidence-backed compatibility tests.
- Performance, platform support and graphics quality are measured targets, never guaranteed by the choice of Rust/Bevy.

## Required modularity

The engine must support changing **combat, magic, movement, controls, input mappings, cameras, AI, HUD/UI, animations and rendering** without rewriting world/quest/state/inventory infrastructure. Support *logical action* abstraction (e.g. Attack, Cast, Interact) separate from physical keys/gamepad controls. Mechanic implementations use documented components, commands and events. The presentation layer must not own authoritative progression state.

## Localization

The development installation is **German**. Detect *actual* text, subtitle and voice-over packages instead of assuming English files. Preserve German text and recordings, keep language independent of IDs/logic, support selection of text vs speech language when installed assets permit. New UI strings must be localized independently (e.g. Fluent catalogs), with validated fallback rules and missing-resource reports.

## Patching

Detect installation version, patch state and languages using verifiable signatures. Treat **Community Patch 1.75.14** as the *initial research baseline*, not as a license to redistribute its files. Import already patched data; when a supported source needs patching, obtain a permitted official patch artifact with user approval, verify it and apply in a separate staging area. **Never mutate the original installation by default**. Record provenance and do not blindly combine Update Pack / Parallel Universe Patch versions. Engine-performance fixes should be implemented appropriately for Bevy rather than porting binary patches.

## Rights and distribution

Distribute only original project-owned code and content for which redistribution rights are secured. **Do not commit, bundle, host or distribute** original meshes, music, voices, dialogues, scripts, archives, extracted data, patch executables or derived asset packs without authorization. A user's possession of Gothic 3 does **not** automatically authorize game-specific reimplementation, extraction, conversion, derivative works or redistribution. Perform a release-specific rights review and seek permission where appropriate. No DRM bypass, anti-cheat bypass or piracy-enabling tooling. Prefer local, offline installation discovery and asset conversion; do not upload user assets. File-presence checks do not verify ownership.

## Acceptance criteria for full compatibility

- Complete asset-reference and localization validation reports for each supported installation configuration.
- Data coverage measured for all known world regions, templates, quest/dialogue nodes, NPC routines and items, with explicit exceptions.
- Verified multi-path playthrough of main story and a documented side-quest regression corpus.
- Save/reload stability, reproducible import signatures, validated error handling and crash-free representative long play sessions.
- Controller and remastered visuals remain separate, later milestones and may be unavailable during fidelity phase.

See [architecture](ARCHITECTURE.md), [import and patches](IMPORT_AND_PATCHES.md), [quality and milestones](QUALITY_AND_MILESTONES.md), and [remaster roadmap](REMASTER_ROADMAP.md).
