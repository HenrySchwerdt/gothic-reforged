# Engine.dll / Game.dll port plan

Companion to `ENGINE_MAP.md` (what the code does) and `SCRIPT_PORT_PLAN.md` (the same batching method for `Script_Game.dll`). Written 2026-10-20 from the Ghidra 12.1.4 index of the installed build 1.75.20108.14 (`.local/research/engine/<Dll>/index.json`, `classes.jsonl`). This file contains only my own prose, names, addresses, sizes and counts; decompiled bodies stay in the gitignored `.local/research/engine/<Dll>/src/`. Full per-batch function lists (address, class, name, size, substantive flag, documented flag) are in the gitignored `.local/research/engine/port_plan/E<nn>.tsv`; the generator scripts are `.local/plan/{rules,stats,gen,build_doc}.py` (re-run to refresh after changing a rule).

Evidence: class membership is **[V]** from RTTI/namespace of the decompile. Functions without an RTTI class (`FUN_*`, 6537 in Engine and 5134 in Game) were attributed **[I]** to the class that surrounds them in the address space (same class on both sides), else to the single class that calls them; see the attribution table. Category is a decision rule on the class, **not** a verification that behaviour parity exists: *covered* means a Rust module was written from the same source, not that it is differentially tested.

## 1. Classification rules

| Class | Meaning | Rule (summary) |
| --- | --- | --- |
| **a port** | gameplay-visible behaviour that our Rust code must reproduce (values, thresholds, state machines, order of operations) | camera, character movement, animation transitions/actor assembly/look-at, facial+lip-sync, inventory/trade/items/stats/damage, focus/interaction, entity processing range and streaming, game zones/triggers, session/save/load, GUI pages and HUD logic, audio semantics, navigation, effects/flocks, and the *parameter rules* of LOD, fade, shadow selection, lights, sky, weather, materials, vegetation, particles |
| **b replaced** | the original engine mechanism has no counterpart in our design because Bevy/wgpu, the OS or the Rust std provide it | D3D9 device and gfx admins, shader/effect compile, vertex pools, render ops, mesh/lightmap offline tools, PhysX wrapper, resource managers and caches, VFS/archives, config files, input devices, Win32-style widgets and editor tools, FMOD wrapper internals, reflection/property boilerplate (`bT*`), CRT, SEH funclets, debug overlays and console commands, cheats/editor sessions |
| **c covered** | an existing crate already implements it from the same analysis | Script/AI host (`crates/script`, `crates/sim`), dialog/info/quest/clock (`crates/gameplay`), localisation and `g3.igl` (`crates/formats`, `crates/ui`), EMotionFX/mesh/lightmap/image readers (`crates/formats`, `crates/cook`), project/database/layer loading and dummy collectors (`crates/importer`, `crates/formats`) |
| **d unknown** | no class, no unique caller class, not a boilerplate thunk | listed in section 6 |

Override rules applied per function inside a *port* class: names containing `Debug`, `Draw*`, `ShowDebug`, console commands go to *replaced*; `Read`/`Write`/`ReadVn`/`OnPostRead` go to the **save batches** (E-save: they define the savegame property schema, not runtime behaviour); renderer-side buffer/skinning/import code inside the animation wrappers goes to *replaced*. A function is **substantive** if it is at least 64 bytes and not a constructor, destructor, property accessor, `GetPropertySetType`/`GetVersion`/`Invalidate`-style stub or vtable stub. Batch sizes below count substantive functions; the remaining small functions of the same classes ride along (they are accessors and wrappers the substantive code calls).

## 2. Counts

### 2.1 Totals

| DLL | functions | a port | b replaced | c covered | d unknown |
| --- | ---: | ---: | ---: | ---: | ---: |
| Engine.dll | 22536 | 4241 | 16624 | 1005 | 666 |
| Game.dll | 14292 | 4749 | 7626 | 1439 | 478 |
| **both** | **36828** | **8990** | **24250** | **2444** | **1144** |

Of the 8990 *port* functions, 3115 are substantive and drive the batch plan; 511 are already cited with address and name in a research doc (the spec exists, mostly `ENGINE_MAP.md`). Script_Game.dll (847 registered entry points, 716 ported, 99 not needed, 32 open per the last commit) is a separate binary and is not part of these counts.

### 2.2 Attribution method (how much is [V] and how much [I])

| DLL | RTTI class [V] | surrounding-class [I] | single-caller-class [I] | CRT name / tail region / SEH | 
| --- | ---: | ---: | ---: | ---: |
| Engine | 16327 | 429 | 2617 | 3163 |
| Game | 10322 | 457 | 1782 | 1731 |

### 2.3 By RTTI class prefix

| Prefix | Engine fns | a | b | c | d | Game fns | a | b | c | d |
| --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: |
| eC* (Engine) | 12223 | 4168 | 7310 | 745 | 0 | 23 | 4 | 7 | 12 | 0 |
| bT* (templates) | 4134 | 0 | 4134 | 0 | 0 | 3985 | 0 | 3985 | 0 | 0 |
| gC* (Game) | 13 | 0 | 0 | 13 | 0 | 6296 | 4738 | 131 | 1427 | 0 |
| (CRT / tail region) | 2277 | 0 | 2277 | 0 | 0 | 1731 | 0 | 1731 | 0 | 0 |
| (unattributed FUN_) | 1738 | 0 | 1072 | 0 | 666 | 1484 | 0 | 1006 | 0 | 478 |
| (SEH funclets) | 886 | 0 | 886 | 0 | 0 | 0 | 0 | 0 | 0 | 0 |
| thunks to other DLL (ENGINE.DLL) | 0 | 0 | 0 | 0 | 0 | 525 | 0 | 525 | 0 | 0 |
| other (CRT/MSVC/middleware) | 325 | 8 | 317 | 0 | 0 | 103 | 0 | 103 | 0 | 0 |
| EMotionFX::* | 320 | 63 | 10 | 247 | 0 | 0 | 0 | 0 | 0 | 0 |
| thunks to other DLL (GFC.DLL) | 70 | 0 | 70 | 0 | 0 | 74 | 0 | 74 | 0 | 0 |
| std::* | 114 | 0 | 114 | 0 | 0 | 7 | 0 | 7 | 0 | 0 |
| thunks to other DLL (SHAREDBASE.DLL) | 66 | 0 | 66 | 0 | 0 | 50 | 0 | 50 | 0 | 0 |
| eLoDMesh::* | 104 | 0 | 104 | 0 | 0 | 0 | 0 | 0 | 0 | 0 |
| thunks to other DLL (FMODEX.DLL) | 99 | 0 | 99 | 0 | 0 | 0 | 0 | 0 | 0 | 0 |
| MCore::* | 78 | 0 | 78 | 0 | 0 | 0 | 0 | 0 | 0 | 0 |
| eT* (templates) | 65 | 0 | 65 | 0 | 0 | 0 | 0 | 0 | 0 | 0 |
| Nx* (PhysX) | 18 | 0 | 18 | 0 | 0 | 0 | 0 | 0 | 0 | 0 |
| bC* (SharedBase) | 0 | 0 | 0 | 0 | 0 | 7 | 0 | 7 | 0 | 0 |
| gS* (Game structs) | 0 | 0 | 0 | 0 | 0 | 7 | 7 | 0 | 0 | 0 |
| eS* | 4 | 2 | 2 | 0 | 0 | 0 | 0 | 0 | 0 | 0 |
| FMOD::* | 2 | 0 | 2 | 0 | 0 | 0 | 0 | 0 | 0 | 0 |

### 2.4 By subsystem

Subsystem = the label the rule table gives the class (finer than the prefix). Engine and Game columns are `a/b/c/d`.

| Subsystem | Engine a/b/c/d | Game a/b/c/d | total |
| --- | ---: | ---: | ---: |
| reflection/property boilerplate, CRT, containers, memory | 0/5465/0/0 | 0/5078/0/0 | 10543 |
| compiler/CRT support (SEH funclets, CRT, tail region) | 0/3163/0/0 | 0/1731/0/0 | 4894 |
| renderer internals (D3D9 gfx admin, shaders, vertex pools, render ops, mesh tooling, offline AO/lightmap tools) | 0/3419/0/0 | 0/0/0/0 | 3419 |
| LOD / fade / shadow-selection / material / light / sky parameter rules | 1995/0/0/0 | 0/0/0/0 | 1995 |
| application host, file system, caches, config, input devices, console, resource managers | 0/1441/0/0 | 0/1/0/0 | 1442 |
| Win32-style widget toolkit and editor tools | 0/1341/0/0 | 0/5/0/0 | 1346 |
| GUI pages, HUD, option menus, input keys | 0/0/0/0 | 1189/0/0/0 | 1189 |
| unattributed FUN_ (no class by RTTI, neighbours or callers) | 0/0/0/666 | 0/0/0/478 | 1144 |
| import thunks / foreign DLL stubs | 0/279/0/0 | 0/651/0/0 | 930 |
| world entity model, processing range, streaming, triggers | 925/0/0/0 | 3/0/0/0 | 928 |
| PhysX wrapper internals and collision queries | 0/827/0/0 | 0/1/0/0 | 828 |
| inventory, items, stats, damage | 0/0/0/0 | 800/0/0/0 | 800 |
| audio semantics (wrapper, emitters, music, ambient) | 203/0/0/0 | 485/0/0/0 | 688 |
| project/database/layer loading, authoring collectors | 0/0/35/0 | 0/0/565/0 | 600 |
| file-format readers (EMotionFX, meshes, lightmaps, images) | 0/0/554/0 | 0/0/0/0 | 554 |
| particles, strips, decals, precipitation | 509/0/0/0 | 0/0/0/0 | 509 |
| D3D9 device wrappers | 0/498/0/0 | 0/0/0/0 | 498 |
| navigation | 0/0/0/0 | 495/0/0/0 | 495 |
| script host and NPC brain | 0/0/2/0 | 0/0/483/0 | 485 |
| animation: actor assembly, blending, look-at | 467/0/0/0 | 0/0/0/0 | 467 |
| localisation, image groups (g3.igl), UI theme | 0/0/414/0 | 0/0/0/0 | 414 |
| dialog, info scripts, quests, clock | 0/0/0/0 | 0/0/391/0 | 391 |
| game zones, triggers, sector enable | 0/0/0/0 | 383/0/0/0 | 383 |
| flocks, effects, enclaves, statistics | 0/0/0/0 | 347/0/0/0 | 347 |
| character movement | 0/0/0/0 | 286/0/0/0 | 286 |
| focus and interaction (doors, locks, movers) | 0/0/0/0 | 248/0/0/0 | 248 |
| session, world load/save | 0/0/0/0 | 240/0/0/0 | 240 |
| save/load schema (Read/Write/ReadVn) of port-class property sets | 117/0/0/0 | 111/0/0/0 | 228 |
| camera | 0/0/0/0 | 162/0/0/0 | 162 |
| editor, cheats, application shell | 0/0/0/0 | 0/110/0/0 | 110 |
| FMOD / shader templates | 0/67/0/0 | 0/0/0/0 | 67 |
| trivial thunks / stubs (<=24 bytes) | 0/37/0/0 | 0/28/0/0 | 65 |
| debug overlays / console commands | 0/23/0/0 | 0/21/0/0 | 44 |
| render/scene context shells | 0/34/0/0 | 0/0/0/0 | 34 |
| renderer-side actor buffers, skinning upload, file import inside animation wrappers | 0/30/0/0 | 0/0/0/0 | 30 |
| facial animation and lip-sync | 25/0/0/0 | 0/0/0/0 | 25 |

### 2.5 Covered classes (c) and where they live

| Subsystem | Functions | Existing code |
| --- | ---: | --- |
| project/database/layer loading, authoring collectors | 600 | crates/importer (world_index.rs, VFS), crates/formats (template.rs, world_node.rs, genome.rs) |
| file-format readers (EMotionFX, meshes, lightmaps, images) | 554 | crates/formats (emfx.rs, xmot.rs, xcmsh.rs, xlmsh.rs, xlmp.rs, ximg.rs, xact.rs, terrain), crates/cook |
| script host and NPC brain | 485 | crates/script + crates/sim (Script_Game port, SCRIPT_PORT_PLAN) |
| localisation, image groups (g3.igl), UI theme | 414 | crates/formats (stringtable.rs, igl.rs), crates/ui (style.rs, assets.rs) |
| dialog, info scripts, quests, clock | 391 | crates/gameplay (dialog.rs, questmgr.rs, conditions.rs), crates/runtime |

Not counted as covered although code exists: `crates/audio`, `crates/ui`, `crates/animation`, `crates/physics`, `crates/nav`, `crates/world`, `apps/game/src/play/*`. They implement parts of *port* subsystems and are listed as "replaces / extends" in the batches. Their parity with the original is only claimed where `ENGINE_MAP.md` or `PHYSICS_MOVEMENT.md` says so.

## 3. Method for every batch

1. Open `E<nn>.tsv`, read the spec sections named under "Spec", then the decompiled bodies of the substantive functions only (`src/<Class>/<name>.c`). Do not read the small ones unless a substantive function calls them.
2. Write the Rust in the **Bevy-free core** (`crates/gameplay`, `crates/sim`, `crates/animation` logic modules, `crates/world`) with the engine-facing data passed in through a small trait or struct, keep constants in a named `const`/ron table with the original address in a comment (`// G:0x100215c0`), never copy decompiled code.
3. Unit tests with synthetic data first (public fixtures only). Then a differential test where the batch says so: run `Gothic3Final.exe` under the existing debugger-style tracer (the INT3/ctypes approach of `.local/research/audio/fmod_dbg.py`, used for the audio mix; REA's Frida provider is not installed), log the inputs and outputs of the named function, replay the inputs through the Rust function and compare with the stated tolerance. Store traces under `.local/traces/`.
4. Replace the stand-in in the app in the same batch, so the Ardea slice improves with every batch; update `HANDOFF_CLAUDE.md` and tick the batch here.

Size estimate = decompiled machine bytes / 20 as Rust lines, with each function capped at 3000 bytes (giant initialisers and lookup tables shrink to data), tests excluded. S: under 500 lines, M: 500-1500, L: over 1500.

## 4. Batches

63 batches, E00..E62, ordered camera, movement and animation, inventory/trade/items and interaction, world entity processing and streaming, save/load, GUI, audio, facial/lip-sync, then the rest (navigation, effects/flocks, render parameter rules). Target size is 20-60 substantive functions; tail merging allows up to 72, and the facial batch has only the 16 substantive facial/lip-sync functions that exist. `[E]`/`[G]` after an address = Engine.dll / Game.dll (both are based at 0x10000000). Cross-batch calls are not acyclic (the animation wrapper calls movement, the camera calls the focus code); port against the host trait and stub the callee.

| Batch | Phase | Title (largest classes) | Fns | Subst. | Code bytes | Documented | Est. lines | Size |
| --- | --- | --- | ---: | ---: | ---: | ---: | ---: | --- |
| E00 | P1 camera | gCCameraAI_PS, gCCameraModeParams | 162 | 39 | 84863 | 25 | 2306 | L |
| E01 | P2 movement | gCCharacterMovement_PS, gCCharacterSensor_PS, gCCharacterControl_PS +1 | 154 | 60 | 42214 | 15 | 2059 | L |
| E02 | P2 movement | gCCharacterMovement_PS, gCCharacterAnimation, gCCharacterEffect | 132 | 59 | 33646 | 20 | 1682 | L |
| E03 | P2 animation | eCWrapper_emfx2Actor, eCWrapper_emfx2Motion, eCAnimationAdmin +7 | 147 | 51 | 19445 | 28 | 972 | M |
| E04 | P2 animation | eCVisualAnimation_PS | 105 | 40 | 21586 | 24 | 1002 | M |
| E05 | P2 animation | eCVisualAnimation_PS | 56 | 39 | 22659 | 8 | 863 | M |
| E06 | P2 animation | eCVisualAnimationFactory, eCVisualAnimation_PS::eCLookAtConstraint, eCAnimation_HardwareSkinner +10 | 151 | 46 | 19873 | 12 | 993 | M |
| E07 | P3 inventory/trade/items | gCInventorySlot, gCItem_PS, gCItemStats | 132 | 35 | 19678 | 5 | 983 | M |
| E08 | P3 inventory/trade/items | gCInventoryStack | 67 | 29 | 6599 | 6 | 329 | S |
| E09 | P3 inventory/trade/items | gCInventory_PS | 115 | 54 | 13132 | 4 | 656 | M |
| E10 | P3 inventory/trade/items | gCInventory_PS | 58 | 53 | 9995 | 0 | 499 | S |
| E11 | P3 inventory/trade/items | gCHUDPageTrade2, gCHUDViewLootPool2, gCHUDViewLootInventory2 +10 | 155 | 43 | 17908 | 2 | 895 | M |
| E12 | P3 inventory/trade/items | gCStats, gCAttrib, gCStatsAdmin +18 | 367 | 70 | 31158 | 0 | 1557 | L |
| E13 | P3 focus/interaction | gCFocus_PS, gCMover_PS, gCInteraction_PS +4 | 248 | 39 | 26975 | 0 | 1348 | M |
| E14 | P4 world/streaming | eCSceneAdmin, eCEntityAdmin, eCPVSPrefetcher3 | 118 | 60 | 22826 | 1 | 1114 | M |
| E15 | P4 world/streaming | eCPVSCellItem, eCProcessibleElement, eCGeometrySpatialContext +1 | 124 | 37 | 17432 | 9 | 871 | M |
| E16 | P4 world/streaming | eCEntity | 96 | 39 | 8137 | 0 | 406 | S |
| E17 | P4 world/streaming | eCEntity | 68 | 39 | 8065 | 0 | 403 | S |
| E18 | P4 world/streaming | eCDynamicEntity, eCSpatialEntity, eCNode +5 | 154 | 56 | 13207 | 0 | 660 | M |
| E19 | P4 world/streaming | eCTemplateEntity, eCTrigger_PS, eCPVSPrefetcherJob +9 | 288 | 56 | 33239 | 0 | 1661 | L |
| E20 | P4 world/streaming | gCSector, gCEntity, gCEntityDummyVisual_PS | 117 | 48 | 14341 | 9 | 717 | M |
| E21 | P4 world/streaming | gCNegZone_PSObj, gCCollisionCircle_PSObj, gCDynamicCollisionCircle_PS +3 | 151 | 59 | 50120 | 0 | 2349 | L |
| E22 | P4 world/streaming | eCPropertyProxy, eCPropertyProxyCollection, eCPropertySetProxy +12 | 195 | 39 | 17268 | 1 | 863 | M |
| E23 | P5 save/load | gCWorld | 74 | 40 | 15495 | 10 | 774 | M |
| E24 | P5 save/load | gCWorld | 60 | 40 | 20065 | 7 | 857 | M |
| E25 | P5 save/load | gCSession, eCDynamicEntity, gCSectorPersistence_PS +5 | 118 | 58 | 30678 | 15 | 1374 | M |
| E26 | P5 save/load | eCEntityDynamicContext, eCEntity, eCTrigger_PS +41 | 89 | 60 | 24268 | 2 | 1213 | M |
| E27 | P5 save/load | gCInventory_PS, gCNavigation_PS, gCStatsAdmin +56 | 127 | 71 | 23615 | 1 | 1180 | M |
| E28 | P6 GUI | gCGUIManager, gCHUDDialogMain2, gCGUIModule | 97 | 56 | 17163 | 31 | 858 | M |
| E29 | P6 GUI | gCHUDDialogMenu2, gCHUDViewQuests2, gCHUDViewSkills2 +15 | 161 | 57 | 21279 | 1 | 1063 | M |
| E30 | P6 GUI | gCHUDViewRecipe2, gCHUDViewDocument2, gCHUDViewCharacter2 +26 | 146 | 60 | 17335 | 0 | 866 | M |
| E31 | P6 GUI | gCHUDMenuFiles2, gCHUDMenuOptionsVideo2, gCHUDMenuMain2 +12 | 154 | 58 | 51932 | 3 | 1779 | L |
| E32 | P6 GUI | gCHUDMenuOptionsControls2, gCHUDMenuOptionsControlsCustom2, gCHUDMenuConfirm2 +36 | 265 | 51 | 53141 | 2 | 2025 | L |
| E33 | P6 GUI | gCHUDTableItemStats, gCHUDLog, gCHUDTextEffect +25 | 121 | 52 | 18741 | 9 | 937 | M |
| E34 | P6 GUI | gCScreenOutputCtrl, gCMap_PS, gCSessionKeys +6 | 151 | 49 | 35666 | 5 | 1339 | M |
| E35 | P7 audio | gCMusicSequencer, eCAudioModule, eCAudioEmitter_PS +7 | 246 | 60 | 21883 | 84 | 1094 | M |
| E36 | P7 audio | gCMusicFragment, gCMusicChannel, gCMusicLinkSequence +7 | 182 | 60 | 17185 | 14 | 859 | M |
| E37 | P7 audio | gCEffectCommandPlaySound, gCAmbientModule, gCAmbientSequencer +16 | 260 | 56 | 20993 | 16 | 1049 | M |
| E38 | P8 facial/lipsync | FacialLayerPass, eCVisualAnimationFactory, eCVisualAnimation_PS +3 | 33 | 16 | 9037 | 7 | 451 | S |
| E39 | P9 navigation | gCNavigationMap | 75 | 55 | 8051 | 0 | 402 | S |
| E40 | P9 navigation | gCNavigationMap | 60 | 55 | 87608 | 5 | 2913 | L |
| E41 | P9 navigation | gCNavigationMap | 61 | 55 | 7439 | 0 | 371 | S |
| E42 | P9 navigation | gCNavZone_PS, gCNavigationAdmin | 116 | 60 | 32204 | 2 | 1470 | M |
| E43 | P9 navigation | gCNavigation_PS, gCNavPath_PS, gCNavOffset_PS +1 | 183 | 40 | 24259 | 0 | 1212 | M |
| E44 | P9 effects/misc | gCEffectCommandSequence, gCBoid, gCEffectMap +1 | 98 | 56 | 16037 | 0 | 801 | M |
| E45 | P9 effects/misc | gCFlock_PS, gCEffectSystem, gCEnclave_PS +11 | 249 | 50 | 22752 | 0 | 1137 | M |
| E46 | P9 render rules | eCShaderMaterialAdmin | 75 | 37 | 11556 | 8 | 577 | M |
| E47 | P9 render rules | eCShaderMaterialAdmin | 45 | 36 | 12715 | 10 | 635 | M |
| E48 | P9 render rules | eCParticle_PS | 212 | 31 | 14617 | 0 | 730 | M |
| E49 | P9 render rules | eCParticle_PS | 43 | 31 | 26288 | 0 | 1144 | M |
| E50 | P9 render rules | eCMeshElement | 63 | 44 | 20025 | 0 | 836 | M |
| E51 | P9 render rules | eCMaterialAdmin | 81 | 40 | 27167 | 3 | 1060 | M |
| E52 | P9 render rules | eCIlluminationAdmin, eCFrustumDatabase | 128 | 58 | 30245 | 13 | 1293 | M |
| E53 | P9 render rules | eCVegetation_GridNode, eCSkydome_PS | 104 | 49 | 31628 | 12 | 1533 | L |
| E54 | P9 render rules | eCVegetation_Grid, eCStrip_PS | 93 | 43 | 14540 | 1 | 727 | M |
| E55 | P9 render rules | eCWeatherZone_PS, eCSpeedTreeAdmin, eCParticleModule | 137 | 55 | 20891 | 5 | 1044 | M |
| E56 | P9 render rules | eCResourceMaterialObject_PS, eCVegetationBrush_PS, eCWeatherAdmin +1 | 211 | 60 | 33096 | 10 | 1494 | M |
| E57 | P9 render rules | eCVegetation_PS, eCShadowMapDirectionalLightLsPSM, eCVisualMeshBase_PS +1 | 128 | 52 | 26112 | 18 | 1305 | M |
| E58 | P9 render rules | eCSpeedTreeWind_PS, eCStaticPointLight_PS, eCResourceMeshLoD_PS +2 | 149 | 56 | 21711 | 1 | 1085 | M |
| E59 | P9 render rules | eCWeatherStates, eCDecalAdmin, eCVegetationAdmin +3 | 164 | 58 | 31132 | 6 | 1556 | L |
| E60 | P9 render rules | eCSpeedTree_PS, eCFrustumElementItem, eCRenderContextMesh +5 | 204 | 57 | 28243 | 15 | 1341 | M |
| E61 | P9 render rules | eCResourceShaderMaterial_PS, eCWeatherEnvironment, eCSkyObject +10 | 268 | 58 | 39797 | 16 | 1920 | L |
| E62 | P9 render rules | eCShadowMapDirectionalLightBase, eCVegetationRubber_PS, eCIPImageFilterMaskHightLights +44 | 399 | 45 | 46037 | 10 | 1861 | L |
| **total** | | | **8990** | **3115** | **1586992** | **511** | **71353** | |

Phase summary (substantive functions): P1 camera 39; P2 movement 119; P2 animation 176; P3 inventory/trade/items 284; P3 focus/interaction 39; P4 world/streaming 433; P5 save/load 269; P6 GUI 383; P7 audio 176; P8 facial/lipsync 16; P9 navigation 265; P9 effects/misc 106; P9 render rules 810.

### 4.1 Batch details

### E00 P1 camera: gCCameraAI_PS, gCCameraModeParams

- **Scope:** 39 substantive / 162 functions, 84863 code bytes, 25 already documented. Classes (substantive/all, address range): `gCCameraAI_PS` [G] 38/158 0x10021130-0x10037e80; `gCCameraModeParams` [G] 1/4 0x10021550-0x10021550.
- **Key functions:** `CorrectCameraPosition` 0x10026b80[G] 15998B, `GetCurrentWishedTranslation` 0x10023190[G] 5756B, `DialogPresetIsValid` 0x1002dca0[G] 3810B, `FindPreset` 0x1002d1a0[G] 2665B, `OnProcess` 0x100215c0[G] 2527B, `OnAction` 0x1002b6c0[G] 2317B, `UpdatePlayerPosition` 0x10025410[G] 2175B, `GetCurrentWishedRotation` 0x10024810[G] 1825B.
- **Spec:** ENGINE_MAP: Solver (ProcessCurrentMode, wished translation/rot (9); ENGINE_MAP: D.4 Per-frame update order (gCCameraAI_PS::OnProce (5); ENGINE_MAP: D.6 Dialog camera [V unless marked] (5).
- **Target:** new `crates/gameplay/src/camera.rs` (Bevy-free: `CameraState`, mode enum 0..13, 27 presets from `Camera_PC_Camera.tple` via `gothic-formats::template`, relax filter, dialog director) driven through a `CameraWorld` trait (ray/sphere cast, pivot entity); `apps/game/src/play/cam.rs` shrinks to the Bevy transform sink.
- **Replaces/extends:** `play_camera` in `apps/game/src/play/cam.rs` (78 lines: fixed 280-unit boom, simple dialog lerp) and `update_camera` in `apps/game/src/walk.rs`, plus the fixed FOV/near-plane values.
- **Depends on:** formats template (done), `gothic-physics` ray/sphere query, runtime `Command`/`Event` for dialog roles; spec ENGINE_MAP Part D.
- **Tests:** unit: relax filter, pitch clamp, distance rules, mode transitions, seeded preset choice; differential: trace pivot, azimuth/elevation/distance, camera position/rotation at the end of `OnProcess` 0x100215c0 during a recorded walk, run and talk script, replay the recorded inputs (pivot, mouse deltas, dt, mode calls) through the Rust solver, tolerance 0.5 units / 0.2 degrees except `CorrectCameraPosition` (behavioural spec only, see Open items).
- **Size:** 2306 lines (L).

### E01 P2 movement: gCCharacterMovement_PS, gCCharacterSensor_PS, gCCharacterControl_PS +1

- **Scope:** 60 substantive / 154 functions, 42214 code bytes, 15 already documented. Classes (substantive/all, address range): `gCCharacterSensor_PS` [G] 8/25 0x100014b0-0x101dae50; `gCCharacterMovement_PS` [G] 47/92 0x10015740-0x100e4de0; `gCCharacterControl_PS` [G] 3/25 0x100ddcc0-0x100de480; `gCMovementBase_PS` [G] 2/12 0x100f0bb0-0x100f0c30.
- **Key functions:** `ProcessControledRotation` 0x100e3000[G] 4005B, `ProcessControledTranslation` 0x100e2410[G] 3027B, `FUN_100db690` 0x100db690[G] 2872B, `ProcessPlayerMovements` 0x100eec80[G] 2108B, `ProcessMovements` 0x100ddd40[G] 1854B, `FUN_100da480` 0x100da480[G] 1804B, `TestSoftCollisions` 0x100ef4c0[G] 1789B, `ProcessControledRotationLookAt` 0x100e4060[G] 1253B.
- **Spec:** ENGINE_MAP: E.3 Speeds, acceleration, turn rate, gravity, step (6); ENGINE_MAP: E.1 Class map (3); ENGINE_MAP: B1. Object model and who calls whom (3).
- **Target:** `crates/sim` new `movement.rs` (mode machine 1..8,10,0xc..0xe, speeds, turn model, slide/swim thresholds) on top of `crates/physics` controller; locomotion mixer stays in `crates/animation/locomotion.rs`.
- **Replaces/extends:** `crates/physics/src/controller.rs` stand-ins (`MoveMode` has 4 of the original ~12 modes, `ground_follow_speed`, fixed turn rate) and the walk code in `apps/game/src/walk.rs`.
- **Depends on:** E00 (camera supplies the wish direction), `PHYSICS_MOVEMENT.md` numbers, gothic-nav for goal poses; spec ENGINE_MAP Part E and B6.
- **Tests:** unit: each mode transition with the threshold numbers of `PHYSICS_MOVEMENT.md`; differential: trace hero position, rotation, `SetMovementMode` calls (0x100e7630) and animation velocity under a recorded input script (walk, run, sprint, strafe, jump, fall, swim, slope), replay inputs, compare mode sequence exactly and position within 2 units after 10 s.
- **Size:** 2059 lines (L).

### E02 P2 movement: gCCharacterMovement_PS, gCCharacterAnimation, gCCharacterEffect

- **Scope:** 59 substantive / 132 functions, 33646 code bytes, 20 already documented. Classes (substantive/all, address range): `gCCharacterAnimation` [G] 0/1; `gCCharacterEffect` [G] 0/1; `gCCharacterMovement_PS` [G] 59/130 0x100e4f90-0x100ee7d0.
- **Key functions:** `CheckChangedMovementStates` 0x100e5cd0[G] 2702B, `PutToGround` 0x100e94f0[G] 2388B, `CreateDependantShapes` 0x100e8370[G] 2299B, `ProcessPhysicsMovements` 0x100e5110[G] 2252B, `SetShapeByMode` 0x100e8ca0[G] 1759B, `IsFreeLOSAndSurrounding` 0x100ea8f0[G] 1725B, `FindFloorWaterCeiling` 0x100eb030[G] 1572B, `GetCurrentMaxSpeedOfDir` 0x100eb800[G] 1140B.
- **Spec:** ENGINE_MAP: E.3 Speeds, acceleration, turn rate, gravity, step (14); ENGINE_MAP: E.2 Movement modes and what changes them [V] (5); ENGINE_MAP: B1. Object model and who calls whom (1).
- **Target:** `crates/sim` new `movement.rs` (mode machine 1..8,10,0xc..0xe, speeds, turn model, slide/swim thresholds) on top of `crates/physics` controller; locomotion mixer stays in `crates/animation/locomotion.rs`.
- **Replaces/extends:** `crates/physics/src/controller.rs` stand-ins (`MoveMode` has 4 of the original ~12 modes, `ground_follow_speed`, fixed turn rate) and the walk code in `apps/game/src/walk.rs`.
- **Depends on:** E01 (same subsystem); E00 (camera supplies the wish direction), `PHYSICS_MOVEMENT.md` numbers, gothic-nav for goal poses; spec ENGINE_MAP Part E and B6.
- **Tests:** unit: each mode transition with the threshold numbers of `PHYSICS_MOVEMENT.md`; differential: trace hero position, rotation, `SetMovementMode` calls (0x100e7630) and animation velocity under a recorded input script (walk, run, sprint, strafe, jump, fall, swim, slope), replay inputs, compare mode sequence exactly and position within 2 units after 10 s.
- **Size:** 1682 lines (L).

### E03 P2 animation: eCWrapper_emfx2Actor, eCWrapper_emfx2Motion, eCAnimationAdmin +7

- **Scope:** 51 substantive / 147 functions, 19445 code bytes, 28 already documented. Classes (substantive/all, address range): `eCAnimationAdmin` [E] 9/25 0x10020a20-0x10021520; `eCWrapper_emfx2Actor` [E] 21/64 0x10046620-0x10050670; `eCWrapper_emfx2Motion` [E] 10/19 0x10053320-0x102eac20; `EMotionFX::PlayModePingPong` [E] 0/4; `EMotionFX::MotionInstance` [E] 0/1; `EMotionFX::PlayModeForward` [E] 0/3; +4 more classes.
- **Key functions:** `FUN_102e7b20` 0x102e7b20[E] 2117B, `Initialize` 0x10020a20[E] 1222B, `PlayMotion` 0x100476f0[E] 500B, `FUN_102ea580` 0x102ea580[E] 497B, `FUN_102ea3d0` 0x102ea3d0[E] 419B, `FUN_10053320` 0x10053320[E] 398B, `FUN_102e7380` 0x102e7380[E] 373B, `PreShutdown` 0x10020ef0[E] 369B.
- **Spec:** ENGINE_MAP: B4. Fade lengths, G3_Ani_Blendings.txt and variati (13); ENGINE_MAP: B3.2 Instance fields the wrapper manipulates [V of (13); ENGINE_MAP: B3.1 Playing a clip (1).
- **Target:** `crates/animation` (`runtime.rs` blend/slot logic, `locomotion.rs`, `appearance.rs`, new `attach.rs`/`lookat.rs`/`ragdoll.rs`); pure logic, Bevy only applies poses.
- **Replaces/extends:** the clip-switch logic in `crates/animation/src/runtime.rs` / `animator.rs` and the NPC attachment code in `apps/game/src/actor_viewer.rs`.
- **Depends on:** E01/E02 (movement state selects motions), formats `xmot`/`emfx`, `G3_Ani_Blendings.txt` parsing; spec ENGINE_MAP Part B.
- **Tests:** unit: slot ownership, fade-in/out lengths from `G3_Ani_Blendings.txt`, variation selection with seeded RNG, stop-prevention; differential: trace `eCWrapper_emfx2Actor::PlayMotion` 0x100476f0 / `SwitchFadeOut` 0x10049dd0 arguments (motion name, fade, weight) for a scripted movement session and compare the sequence with the Rust transition planner.
- **Size:** 972 lines (M).

### E04 P2 animation: eCVisualAnimation_PS

- **Scope:** 40 substantive / 105 functions, 21586 code bytes, 24 already documented. Classes (substantive/all, address range): `eCVisualAnimation_PS` [E] 40/105 0x1002b700-0x10032cd0.
- **Key functions:** `UpdateFrameEffects` 0x1002eff0[E] 4538B, `CreatePhysicsController` 0x10031170[E] 2074B, `UpdateAim` 0x10030230[E] 1557B, `GameReset` 0x1002b700[E] 974B, `ProcessPhysicsLOD` 0x10031d40[E] 934B, `IntersectPhysicsRayNearestHit` 0x10032a00[E] 714B, `SetRagDollEnabled` 0x100323f0[E] 596B, `SetLookAtTarget` 0x10032250[E] 414B.
- **Spec:** ENGINE_MAP: B9. Properties and physics (summary) (8); ENGINE_MAP: B2. The eight motion slots and slot ownership [V u (5); ENGINE_MAP: B10. Attachments and look-at constraints (5).
- **Target:** `crates/animation` (`runtime.rs` blend/slot logic, `locomotion.rs`, `appearance.rs`, new `attach.rs`/`lookat.rs`/`ragdoll.rs`); pure logic, Bevy only applies poses.
- **Replaces/extends:** the clip-switch logic in `crates/animation/src/runtime.rs` / `animator.rs` and the NPC attachment code in `apps/game/src/actor_viewer.rs`.
- **Depends on:** E03 (same subsystem); E01/E02 (movement state selects motions), formats `xmot`/`emfx`, `G3_Ani_Blendings.txt` parsing; spec ENGINE_MAP Part B.
- **Tests:** unit: slot ownership, fade-in/out lengths from `G3_Ani_Blendings.txt`, variation selection with seeded RNG, stop-prevention; differential: trace `eCWrapper_emfx2Actor::PlayMotion` 0x100476f0 / `SwitchFadeOut` 0x10049dd0 arguments (motion name, fade, weight) for a scripted movement session and compare the sequence with the Rust transition planner.
- **Size:** 1002 lines (M).

### E05 P2 animation: eCVisualAnimation_PS

- **Scope:** 39 substantive / 56 functions, 22659 code bytes, 8 already documented. Classes (substantive/all, address range): `eCVisualAnimation_PS` [E] 39/56 0x10032d40-0x10041e00.
- **Key functions:** `FUN_1003e1d0` 0x1003e1d0[E] 5736B, `FUN_1003fc80` 0x1003fc80[E] 5648B, `OnProcess` 0x10032d40[E] 2518B, `GetStatus` 0x10033e10[E] 1595B, `OnCacheOut` 0x10033790[E] 900B, `FUN_1003d120` 0x1003d120[E] 830B, `OnNotifyPropertyValueChangedExitEx` 0x10034640[E] 417B, `FUN_1003f8b0` 0x1003f8b0[E] 381B.
- **Spec:** ENGINE_MAP: B15. Cache and misc (5); ENGINE_MAP: B9. Properties and physics (summary) (2); ENGINE_MAP: B1. Object model and who calls whom (1).
- **Target:** `crates/animation` (`runtime.rs` blend/slot logic, `locomotion.rs`, `appearance.rs`, new `attach.rs`/`lookat.rs`/`ragdoll.rs`); pure logic, Bevy only applies poses.
- **Replaces/extends:** the clip-switch logic in `crates/animation/src/runtime.rs` / `animator.rs` and the NPC attachment code in `apps/game/src/actor_viewer.rs`.
- **Depends on:** E04 (same subsystem); E01/E02 (movement state selects motions), formats `xmot`/`emfx`, `G3_Ani_Blendings.txt` parsing; spec ENGINE_MAP Part B.
- **Tests:** unit: slot ownership, fade-in/out lengths from `G3_Ani_Blendings.txt`, variation selection with seeded RNG, stop-prevention; differential: trace `eCWrapper_emfx2Actor::PlayMotion` 0x100476f0 / `SwitchFadeOut` 0x10049dd0 arguments (motion name, fade, weight) for a scripted movement session and compare the sequence with the Rust transition planner.
- **Size:** 863 lines (M).

### E06 P2 animation: eCVisualAnimationFactory, eCVisualAnimation_PS::eCLookAtConstraint, eCAnimation_HardwareSkinner +10

- **Scope:** 46 substantive / 151 functions, 19873 code bytes, 12 already documented. Classes (substantive/all, address range): `eCVisualAnimation_PS::eCActorAttachment` [E] 2/12 0x1002a5b0-0x1002a820; `eCVisualAnimation_PS::eCLookAtConstraint` [E] 6/14 0x1002acd0-0x102e2f40; `eCVisualAnimationFactory` [E] 25/36 0x1001a7a0-0x102e4c70; `EMotionFX::RepositioningLayerPass` [E] 0/2; `eCVisualAnimation_PS::eSMotionDesc` [E] 3/16 0x1002a0c0-0x1002a180; `LookAtConstraintEx` [E] 0/8; +7 more classes.
- **Key functions:** `FUN_102e0510` 0x102e0510[E] 764B, `CreateLoD` 0x10043300[E] 690B, `RemoveNotUsedBones` 0x1003a4d0[E] 683B, `Process` 0x1002a5b0[E] 614B, `FUN_102dffa0` 0x102dffa0[E] 575B, `FUN_102e4c70` 0x102e4c70[E] 393B, `SetTargetPos` 0x1002acd0[E] 391B, `CreateActorInstance` 0x10039b60[E] 376B.
- **Spec:** ENGINE_MAP: B10. Attachments and look-at constraints (6); ENGINE_MAP: B1. Object model and who calls whom (5); ENGINE_MAP: B11. Actor assembly and LoD (3).
- **Target:** `crates/animation` (`runtime.rs` blend/slot logic, `locomotion.rs`, `appearance.rs`, new `attach.rs`/`lookat.rs`/`ragdoll.rs`); pure logic, Bevy only applies poses.
- **Replaces/extends:** the clip-switch logic in `crates/animation/src/runtime.rs` / `animator.rs` and the NPC attachment code in `apps/game/src/actor_viewer.rs`.
- **Depends on:** E05 (same subsystem); E01/E02 (movement state selects motions), formats `xmot`/`emfx`, `G3_Ani_Blendings.txt` parsing; spec ENGINE_MAP Part B.
- **Tests:** unit: slot ownership, fade-in/out lengths from `G3_Ani_Blendings.txt`, variation selection with seeded RNG, stop-prevention; differential: trace `eCWrapper_emfx2Actor::PlayMotion` 0x100476f0 / `SwitchFadeOut` 0x10049dd0 arguments (motion name, fade, weight) for a scripted movement session and compare the sequence with the Rust transition planner.
- **Size:** 993 lines (M).

### E07 P3 inventory/trade/items: gCInventorySlot, gCItem_PS, gCItemStats

- **Scope:** 35 substantive / 132 functions, 19678 code bytes, 5 already documented. Classes (substantive/all, address range): `gCItem_PS` [G] 10/84 0x100b9100-0x100bc260; `gCInventorySlot` [G] 21/39 0x100bcf70-0x100bec50; `gCItemStats` [G] 4/9 0x10187c80-0x10187e00.
- **Key functions:** `ExecuteScript` 0x100b9df0[G] 2576B, `OnCustomPatch` 0x100b9100[G] 2152B, `OnApplyStats` 0x100be5f0[G] 812B, `OnUnApplyStats` 0x100be920[G] 812B, `AttachItemToSekelton` 0x100bdb70[G] 737B, `DetachItemFromSkeleton` 0x100bde60[G] 571B, `EnableItemRendering` 0x100be0a0[G] 542B, `SpawnItem` 0x100be3f0[G] 509B.
- **Spec:** ENGINE_MAP: Q2. NPC appearance chain (head / body / hair / bea (5).
- **Target:** `crates/sim` (`item.rs`, `economy.rs`, `vitals.rs`, `combat.rs`) plus a `crates/sim/src/inventory.rs` for stacks/slots/equip/transaction; trade/loot view-models in `crates/ui/src/logic.rs`.
- **Replaces/extends:** the simplified item and trade handling in `crates/sim/src/item.rs`/`economy.rs` and the category-glyph fallbacks in `crates/ui`.
- **Depends on:** `crates/formats` property streams for items (done), E00..E06 not required; spec ENGINE_MAP Q1/Q2, section 8, GAME_RULES.md.
- **Tests:** unit: stack merge/split invariants, equip rules (`CanEquip`), skill learning gates (`CanLearnSkill`), transaction totals; property tests on random transfer sequences (item count conserved); differential: trace `TransferItemsTo`/`ConsumeItems`/`CanEquip` results for scripted trades with a copy of the hero inventory.
- **Size:** 983 lines (M).

### E08 P3 inventory/trade/items: gCInventoryStack

- **Scope:** 29 substantive / 67 functions, 6599 code bytes, 6 already documented. Classes (substantive/all, address range): `gCInventoryStack` [G] 29/67 0x100c0330-0x100c3050.
- **Key functions:** `Drop` 0x100c06a0[G] 678B, `GetIconFileNames` 0x100c0ae0[G] 506B, `CompareFocusName` 0x100c2c00[G] 495B, `Compare` 0x100c2460[G] 299B, `CompareProtection` 0x100c2f10[G] 286B, `CompareDamage` 0x100c2df0[G] 273B, `CompareType` 0x100c3050[G] 267B, `CompareTemplate` 0x100c2a10[G] 226B.
- **Spec:** ENGINE_MAP: 8.1 Stack data model used by the controls [V] (3); ENGINE_MAP: Q1. Item icon atlas cell order (`gCInventoryStack: (3); ENGINE_MAP: 8.2 Icon resolution [V] (3).
- **Target:** `crates/sim` (`item.rs`, `economy.rs`, `vitals.rs`, `combat.rs`) plus a `crates/sim/src/inventory.rs` for stacks/slots/equip/transaction; trade/loot view-models in `crates/ui/src/logic.rs`.
- **Replaces/extends:** the simplified item and trade handling in `crates/sim/src/item.rs`/`economy.rs` and the category-glyph fallbacks in `crates/ui`.
- **Depends on:** E07 (same subsystem); `crates/formats` property streams for items (done), E00..E06 not required; spec ENGINE_MAP Q1/Q2, section 8, GAME_RULES.md.
- **Tests:** unit: stack merge/split invariants, equip rules (`CanEquip`), skill learning gates (`CanLearnSkill`), transaction totals; property tests on random transfer sequences (item count conserved); differential: trace `TransferItemsTo`/`ConsumeItems`/`CanEquip` results for scripted trades with a copy of the hero inventory.
- **Size:** 329 lines (S).

### E09 P3 inventory/trade/items: gCInventory_PS

- **Scope:** 54 substantive / 115 functions, 13132 code bytes, 4 already documented. Classes (substantive/all, address range): `gCInventory_PS` [G] 54/115 0x10018450-0x100b7ca0.
- **Key functions:** `CanLearnSkill` 0x100b5ff0[G] 1903B, `ConsumeItems` 0x100b7970[G] 808B, `TransferItemsTo` 0x100b6e20[G] 633B, `AssureItems` 0x100b5cc0[G] 458B, `GeneratePlunderInvFromTreasureSets` 0x100b71f0[G] 329B, `PatchTreasureSets` 0x100b7700[G] 259B, `UpdateVisibility` 0x100b5330[G] 207B, `PatchDefaultItems` 0x100b56d0[G] 199B.
- **Spec:** ENGINE_MAP: Q2. NPC appearance chain (head / body / hair / bea (2); ENGINE_MAP: 8.1 Stack data model used by the controls [V] (2).
- **Target:** `crates/sim` (`item.rs`, `economy.rs`, `vitals.rs`, `combat.rs`) plus a `crates/sim/src/inventory.rs` for stacks/slots/equip/transaction; trade/loot view-models in `crates/ui/src/logic.rs`.
- **Replaces/extends:** the simplified item and trade handling in `crates/sim/src/item.rs`/`economy.rs` and the category-glyph fallbacks in `crates/ui`.
- **Depends on:** E08 (same subsystem); `crates/formats` property streams for items (done), E00..E06 not required; spec ENGINE_MAP Q1/Q2, section 8, GAME_RULES.md.
- **Tests:** unit: stack merge/split invariants, equip rules (`CanEquip`), skill learning gates (`CanLearnSkill`), transaction totals; property tests on random transfer sequences (item count conserved); differential: trace `TransferItemsTo`/`ConsumeItems`/`CanEquip` results for scripted trades with a copy of the hero inventory.
- **Size:** 656 lines (M).

### E10 P3 inventory/trade/items: gCInventory_PS

- **Scope:** 53 substantive / 58 functions, 9995 code bytes, 0 already documented. Classes (substantive/all, address range): `gCInventory_PS` [G] 53/58 0x100b7d40-0x100c5360.
- **Key functions:** `CanEquip` 0x100b7dc0[G] 796B, `FUN_100c49a0` 0x100c49a0[G] 456B, `FUN_100c4d20` 0x100c4d20[G] 456B, `FUN_100c4b70` 0x100c4b70[G] 421B, `EquipStack` 0x100b8160[G] 360B, `FUN_100c4fa0` 0x100c4fa0[G] 307B, `FUN_100c50e0` 0x100c50e0[G] 307B, `FUN_100c3e50` 0x100c3e50[G] 302B.
- **Spec:** none yet (new analysis needed; start with the class method list in the tsv).
- **Target:** `crates/sim` (`item.rs`, `economy.rs`, `vitals.rs`, `combat.rs`) plus a `crates/sim/src/inventory.rs` for stacks/slots/equip/transaction; trade/loot view-models in `crates/ui/src/logic.rs`.
- **Replaces/extends:** the simplified item and trade handling in `crates/sim/src/item.rs`/`economy.rs` and the category-glyph fallbacks in `crates/ui`.
- **Depends on:** E09 (same subsystem); `crates/formats` property streams for items (done), E00..E06 not required; spec ENGINE_MAP Q1/Q2, section 8, GAME_RULES.md.
- **Tests:** unit: stack merge/split invariants, equip rules (`CanEquip`), skill learning gates (`CanLearnSkill`), transaction totals; property tests on random transfer sequences (item count conserved); differential: trace `TransferItemsTo`/`ConsumeItems`/`CanEquip` results for scripted trades with a copy of the hero inventory.
- **Size:** 499 lines (S).

### E11 P3 inventory/trade/items: gCHUDPageTrade2, gCHUDViewLootPool2, gCHUDViewLootInventory2 +10

- **Scope:** 43 substantive / 155 functions, 17908 code bytes, 2 already documented. Classes (substantive/all, address range): `gCArmorSet_PS` [G] 0/37; `gCInventoryListener` [G] 3/7 0x101dc637-0x101dd3d4; `gCTreasureSet_PS` [G] 3/11 0x101b4d90-0x101b53d0; `gCHUDViewTradeInventory2` [G] 3/8 0x10080960-0x10080c20; `gCHUDViewTradeTransaction2` [G] 2/6 0x10080c90-0x10080d40; `gCHUDPageTrade2` [G] 9/17 0x100811a0-0x1008a0b0; +7 more classes.
- **Key functions:** `GeneratePlunderInventory` 0x101b4d90[G] 1080B, `GenerateTradeInventory` 0x101b51f0[G] 474B, `FUN_101dd3d4` 0x101dd3d4[G] 399B, `FUN_100811a0` 0x100811a0[G] 397B, `RefreshTradeInventory` 0x101b53d0[G] 343B, `FUN_100a1dc0` 0x100a1dc0[G] 341B, `FUN_100834a0` 0x100834a0[G] 275B, `GetTreasureSet` 0x101b6ad0[G] 267B.
- **Spec:** ENGINE_MAP: 8.3 List controls [V for call structure] (2).
- **Target:** `crates/sim` (`item.rs`, `economy.rs`, `vitals.rs`, `combat.rs`) plus a `crates/sim/src/inventory.rs` for stacks/slots/equip/transaction; trade/loot view-models in `crates/ui/src/logic.rs`.
- **Replaces/extends:** the simplified item and trade handling in `crates/sim/src/item.rs`/`economy.rs` and the category-glyph fallbacks in `crates/ui`.
- **Depends on:** E10 (same subsystem); `crates/formats` property streams for items (done), E00..E06 not required; spec ENGINE_MAP Q1/Q2, section 8, GAME_RULES.md.
- **Tests:** unit: stack merge/split invariants, equip rules (`CanEquip`), skill learning gates (`CanLearnSkill`), transaction totals; property tests on random transfer sequences (item count conserved); differential: trace `TransferItemsTo`/`ConsumeItems`/`CanEquip` results for scripted trades with a copy of the hero inventory.
- **Size:** 895 lines (M).

### E12 P3 inventory/trade/items: gCStats, gCAttrib, gCStatsAdmin +18

- **Scope:** 70 substantive / 367 functions, 31158 code bytes, 0 already documented. Classes (substantive/all, address range): `gCProjectile_PS` [G] 7/36 0x10151e30-0x10153050; `gCTouchDamage_PS` [G] 2/13 0x100692f0-0x100693b0; `gCDamage_PS` [G] 1/13 0x10039e00-0x10039e00; `gCDamageReceiver_PS` [G] 0/17; `gCAttrib` [G] 8/23 0x10186c70-0x10187130; `gCAttribCondDesc` [G] 2/13 0x101872d0-0x10187420; +15 more classes.
- **Key functions:** `OnTouch` 0x10152650[G] 2505B, `OnProcess` 0x100d3080[G] 978B, `GetDirection` 0x10151e30[G] 706B, `ShootIteration` 0x101523a0[G] 581B, `GetHeaderContent` 0x1018ad60[G] 540B, `ApplyPerm` 0x10184670[G] 413B, `GetDesc` 0x101879e0[G] 404B, `GetDesc` 0x101872d0[G] 332B.
- **Spec:** none yet (new analysis needed; start with the class method list in the tsv).
- **Target:** `crates/sim` (`item.rs`, `economy.rs`, `vitals.rs`, `combat.rs`) plus a `crates/sim/src/inventory.rs` for stacks/slots/equip/transaction; trade/loot view-models in `crates/ui/src/logic.rs`.
- **Replaces/extends:** the simplified item and trade handling in `crates/sim/src/item.rs`/`economy.rs` and the category-glyph fallbacks in `crates/ui`.
- **Depends on:** E11 (same subsystem); `crates/formats` property streams for items (done), E00..E06 not required; spec ENGINE_MAP Q1/Q2, section 8, GAME_RULES.md.
- **Tests:** unit: stack merge/split invariants, equip rules (`CanEquip`), skill learning gates (`CanLearnSkill`), transaction totals; property tests on random transfer sequences (item count conserved); differential: trace `TransferItemsTo`/`ConsumeItems`/`CanEquip` results for scripted trades with a copy of the hero inventory.
- **Size:** 1557 lines (L).

### E13 P3 focus/interaction: gCFocus_PS, gCMover_PS, gCInteraction_PS +4

- **Scope:** 39 substantive / 248 functions, 26975 code bytes, 0 already documented. Classes (substantive/all, address range): `gCFocus_PS` [G] 18/137 0x10020b70-0x1006ffd0; `gCInteraction_PS` [G] 7/36 0x10003360-0x101b1310; `gCMover_PS` [G] 8/31 0x101a6ff0-0x101a7a70; `gCDoor_PS` [G] 2/15 0x100cbf80-0x100cc020; `gCInteractionAdmin` [G] 2/8 0x101b4160-0x101b41e0; `gCLock_PS` [G] 0/12; +1 more classes.
- **Key functions:** `FindFocusEntity` 0x1006da20[G] 1808B, `GetCurrentPriority` 0x1006c910[G] 936B, `GetAngles` 0x1006c530[G] 932B, `GetFocusEntity` 0x1006e140[G] 912B, `IsEntityInAngles` 0x1006ccd0[G] 788B, `Close` 0x101a7a70[G] 782B, `Open` 0x101a7780[G] 752B, `IsValidEntity` 0x1006d3f0[G] 742B.
- **Spec:** none yet (new analysis needed; start with the class method list in the tsv).
- **Target:** `crates/sim` interaction module + `apps/game/src/play` focus picking; core keeps the candidate scoring, the host supplies visibility.
- **Replaces/extends:** the nearest-object pickup in `apps/game/src/play/events.rs`.
- **Depends on:** E00 (camera direction), inventory batches for pickup/loot; spec ENGINE_MAP section 7.5 and `SCRIPT_ENGINE_INTERFACE.md`.
- **Tests:** unit: candidate scoring and cone/range tests with synthetic entities; differential: trace the focus entity id chosen by `gCFocus_PS` per frame for a recorded walk past doors, chests, NPCs.
- **Size:** 1348 lines (M).

### E14 P4 world/streaming: eCSceneAdmin, eCEntityAdmin, eCPVSPrefetcher3

- **Scope:** 60 substantive / 118 functions, 22826 code bytes, 1 already documented. Classes (substantive/all, address range): `eCPVSPrefetcher3` [E] 16/30 0x101a8d70-0x101ab950; `eCEntityAdmin` [E] 20/37 0x10230720-0x10231e20; `eCSceneAdmin` [E] 24/51 0x10232540-0x10233e20.
- **Key functions:** `Execute` 0x101a98b0[E] 3537B, `Process` 0x10230a70[E] 2700B, `RecalculateGrid` 0x101aa750[E] 1161B, `Shutdown` 0x10232540[E] 1022B, `GetEntityByPartName` 0x10232c20[E] 871B, `Trigger` 0x10233ba0[E] 611B, `GetEntityCountByPartName` 0x10232f90[E] 583B, `Recalibrate` 0x101a9470[E] 556B.
- **Spec:** ENGINE_MAP: F.5 Streaming around the player (1).
- **Target:** `crates/world` (residency planner gains the original rules: cell grid, rectangle, hysteresis, ROI), `crates/runtime/src/world.rs` for processing-range enter/exit events, `apps/game/src/world_loader.rs` as the asset side.
- **Replaces/extends:** the synthetic residency laboratory in `crates/world` and the all-at-once spawn in `world_loader.rs`.
- **Depends on:** importer world index (done); spec ENGINE_MAP Part F (F.1, F.5); `WORLD_SURVEY.md`.
- **Tests:** unit: rectangle/hysteresis/cell-grid arithmetic, ROI enter/exit ordering; differential: trace the entity ids passing `OnEnterProcessingRange`/`OnExitProcessingRange` (eCEntityAdmin::Process 0x10230a70) along a scripted path and compare sets per 100 ms bucket.
- **Size:** 1114 lines (M).

### E15 P4 world/streaming: eCPVSCellItem, eCProcessibleElement, eCGeometrySpatialContext +1

- **Scope:** 37 substantive / 124 functions, 17432 code bytes, 9 already documented. Classes (substantive/all, address range): `eCProcessibleElement` [E] 11/40 0x1005c9b0-0x1005db50; `eCEntityDynamicContext` [E] 1/10 0x1021d410-0x1021d410; `eCPVSCellItem` [E] 16/41 0x101a7230-0x101a8970; `eCGeometrySpatialContext` [E] 9/33 0x1021b610-0x1021c880.
- **Key functions:** `BuildStaticHierachy` 0x101a7520[E] 2246B, `SaveData` 0x1005d600[E] 986B, `ReadNodes` 0x1021c080[E] 942B, `WriteNodes` 0x1021c4e0[E] 837B, `LoadData` 0x1005d2f0[E] 760B, `Load` 0x1005ce00[E] 732B, `ReadNodesOld` 0x1021b9c0[E] 620B, `BuildStaticHierachy` 0x101a7280[E] 601B.
- **Spec:** ENGINE_MAP: F.5 Streaming around the player (9).
- **Target:** `crates/world` (residency planner gains the original rules: cell grid, rectangle, hysteresis, ROI), `crates/runtime/src/world.rs` for processing-range enter/exit events, `apps/game/src/world_loader.rs` as the asset side.
- **Replaces/extends:** the synthetic residency laboratory in `crates/world` and the all-at-once spawn in `world_loader.rs`.
- **Depends on:** E14 (same subsystem); importer world index (done); spec ENGINE_MAP Part F (F.1, F.5); `WORLD_SURVEY.md`.
- **Tests:** unit: rectangle/hysteresis/cell-grid arithmetic, ROI enter/exit ordering; differential: trace the entity ids passing `OnEnterProcessingRange`/`OnExitProcessingRange` (eCEntityAdmin::Process 0x10230a70) along a scripted path and compare sets per 100 ms bucket.
- **Size:** 871 lines (M).

### E16 P4 world/streaming: eCEntity

- **Scope:** 39 substantive / 96 functions, 8137 code bytes, 0 already documented. Classes (substantive/all, address range): `eCEntity` [E] 39/96 0x10223370-0x10225360.
- **Key functions:** `CopyEntityPrivateData` 0x10223370[E] 577B, `OnUpdateInternals` 0x10223bf0[E] 290B, `OnChildrenAvailable` 0x102238b0[E] 279B, `OnEnterProcessingRange` 0x102239d0[E] 277B, `EnableDeactivation` 0x10224c50[E] 257B, `OnExitProcessingRange` 0x10223af0[E] 245B, `OnCreateFromTemplate` 0x10224450[E] 195B, `OnUpdatedWorldMatrix` 0x102237a0[E] 173B.
- **Spec:** none yet (new analysis needed; start with the class method list in the tsv).
- **Target:** `crates/world` (residency planner gains the original rules: cell grid, rectangle, hysteresis, ROI), `crates/runtime/src/world.rs` for processing-range enter/exit events, `apps/game/src/world_loader.rs` as the asset side.
- **Replaces/extends:** the synthetic residency laboratory in `crates/world` and the all-at-once spawn in `world_loader.rs`.
- **Depends on:** E15 (same subsystem); importer world index (done); spec ENGINE_MAP Part F (F.1, F.5); `WORLD_SURVEY.md`.
- **Tests:** unit: rectangle/hysteresis/cell-grid arithmetic, ROI enter/exit ordering; differential: trace the entity ids passing `OnEnterProcessingRange`/`OnExitProcessingRange` (eCEntityAdmin::Process 0x10230a70) along a scripted path and compare sets per 100 ms bucket.
- **Size:** 406 lines (S).

### E17 P4 world/streaming: eCEntity

- **Scope:** 39 substantive / 68 functions, 8065 code bytes, 0 already documented. Classes (substantive/all, address range): `eCEntity` [E] 39/68 0x102253c0-0x10228520.
- **Key functions:** `IsFreeLOS` 0x10228000[E] 649B, `UpdateParentDependencies` 0x10227860[E] 570B, `AddPropertySet` 0x10226820[E] 519B, `UpdateChildDependencies` 0x10227aa0[E] 485B, `GetAziElev` 0x10227e00[E] 409B, `RemovePropertySetAt` 0x10226aa0[E] 326B, `SetWorldTreeBoundary` 0x102273e0[E] 288B, `SetWorldNodeBoundary` 0x10227510[E] 283B.
- **Spec:** none yet (new analysis needed; start with the class method list in the tsv).
- **Target:** `crates/world` (residency planner gains the original rules: cell grid, rectangle, hysteresis, ROI), `crates/runtime/src/world.rs` for processing-range enter/exit events, `apps/game/src/world_loader.rs` as the asset side.
- **Replaces/extends:** the synthetic residency laboratory in `crates/world` and the all-at-once spawn in `world_loader.rs`.
- **Depends on:** E16 (same subsystem); importer world index (done); spec ENGINE_MAP Part F (F.1, F.5); `WORLD_SURVEY.md`.
- **Tests:** unit: rectangle/hysteresis/cell-grid arithmetic, ROI enter/exit ordering; differential: trace the entity ids passing `OnEnterProcessingRange`/`OnExitProcessingRange` (eCEntityAdmin::Process 0x10230a70) along a scripted path and compare sets per 100 ms bucket.
- **Size:** 403 lines (S).

### E18 P4 world/streaming: eCDynamicEntity, eCSpatialEntity, eCNode +5

- **Scope:** 56 substantive / 154 functions, 13207 code bytes, 0 already documented. Classes (substantive/all, address range): `eCEntityProxy` [E] 5/19 0x1022e340-0x1022e500; `eCNode` [E] 10/28 0x1020d960-0x1020df40; `eCEntityProxyInternal` [E] 0/5; `eCEntityPropertySet` [E] 0/19; `eCEntityTemplateContext` [E] 6/13 0x1021ee20-0x102202e0; `eCSpatialEntity` [E] 11/19 0x10228740-0x10229100; +2 more classes.
- **Key functions:** `SaveTemplateEntity` 0x102202e0[E] 784B, `SetMeshVisual` 0x102292c0[E] 573B, `SaveAllTemplateEntities` 0x1021f1d0[E] 505B, `SetResourceFile` 0x102287a0[E] 429B, `ReadHeaders` 0x1021f020[E] 421B, `OnIntersect` 0x1022a650[E] 359B, `SaveHeaders` 0x1021eec0[E] 344B, `UpdateLocalBoundary` 0x10228fb0[E] 334B.
- **Spec:** none yet (new analysis needed; start with the class method list in the tsv).
- **Target:** `crates/world` (residency planner gains the original rules: cell grid, rectangle, hysteresis, ROI), `crates/runtime/src/world.rs` for processing-range enter/exit events, `apps/game/src/world_loader.rs` as the asset side.
- **Replaces/extends:** the synthetic residency laboratory in `crates/world` and the all-at-once spawn in `world_loader.rs`.
- **Depends on:** E17 (same subsystem); importer world index (done); spec ENGINE_MAP Part F (F.1, F.5); `WORLD_SURVEY.md`.
- **Tests:** unit: rectangle/hysteresis/cell-grid arithmetic, ROI enter/exit ordering; differential: trace the entity ids passing `OnEnterProcessingRange`/`OnExitProcessingRange` (eCEntityAdmin::Process 0x10230a70) along a scripted path and compare sets per 100 ms bucket.
- **Size:** 660 lines (M).

### E19 P4 world/streaming: eCTemplateEntity, eCTrigger_PS, eCPVSPrefetcherJob +9

- **Scope:** 56 substantive / 288 functions, 33239 code bytes, 0 already documented. Classes (substantive/all, address range): `eCTriggerBase_PS` [E] 5/16 0x10080cf0-0x10080f90; `eCEventFilter_PS` [E] 4/20 0x10077740-0x10077e70; `eCTrigger_PS` [E] 8/65 0x1007c350-0x1007ddf0; `eCEventLogic_PS` [E] 1/9 0x1007a480-0x1007a480; `eCTriggerCombiner_PS` [E] 5/32 0x100821f0-0x10082820; `eCTriggerList_PS` [E] 7/45 0x10083cb0-0x100843d0; +6 more classes.
- **Key functions:** `ActivateTrigger` 0x1007d0f0[E] 1514B, `ReadAll` 0x1022cc80[E] 1513B, `ReadHeader` 0x1022d2d0[E] 1306B, `PatchEntity` 0x1022bf30[E] 852B, `CacheInStaticEntities` 0x101a6820[E] 845B, `DeactivateTrigger` 0x1007d6e0[E] 788B, `ReadContent` 0x1022d7f0[E] 780B, `OnTrigger` 0x100824c0[E] 765B.
- **Spec:** none yet (new analysis needed; start with the class method list in the tsv).
- **Target:** `crates/world` (residency planner gains the original rules: cell grid, rectangle, hysteresis, ROI), `crates/runtime/src/world.rs` for processing-range enter/exit events, `apps/game/src/world_loader.rs` as the asset side.
- **Replaces/extends:** the synthetic residency laboratory in `crates/world` and the all-at-once spawn in `world_loader.rs`.
- **Depends on:** E18 (same subsystem); importer world index (done); spec ENGINE_MAP Part F (F.1, F.5); `WORLD_SURVEY.md`.
- **Tests:** unit: rectangle/hysteresis/cell-grid arithmetic, ROI enter/exit ordering; differential: trace the entity ids passing `OnEnterProcessingRange`/`OnExitProcessingRange` (eCEntityAdmin::Process 0x10230a70) along a scripted path and compare sets per 100 ms bucket.
- **Size:** 1661 lines (L).

### E20 P4 world/streaming: gCSector, gCEntity, gCEntityDummyVisual_PS

- **Scope:** 48 substantive / 117 functions, 14341 code bytes, 9 already documented. Classes (substantive/all, address range): `gCSector` [G] 35/76 0x100ca570-0x10195b30; `gCEntityDummyVisual_PS` [G] 1/6 0x100425b0-0x100425b0; `gCEntity` [G] 12/35 0x100664e0-0x10067690.
- **Key functions:** `BackupFiles` 0x101923b0[G] 1519B, `DoLoadData` 0x10193c40[G] 870B, `DoDamage` 0x100666d0[G] 512B, `AddPropertySetToProcessingEntityThisIsAnE3HackDoNotUseLetsTalkAboutItLater` 0x10066d50[G] 441B, `OnNotifyPropertyValueChangedExitEx` 0x10193a90[G] 417B, `RestoreFiles` 0x101929b0[G] 409B, `CreateFromTemplate` 0x100664e0[G] 398B, `OnActivate` 0x10194190[G] 358B.
- **Spec:** ENGINE_MAP: F.1 Object model: workspace, project, database, wo (10).
- **Target:** `crates/world` (residency planner gains the original rules: cell grid, rectangle, hysteresis, ROI), `crates/runtime/src/world.rs` for processing-range enter/exit events, `apps/game/src/world_loader.rs` as the asset side.
- **Replaces/extends:** the synthetic residency laboratory in `crates/world` and the all-at-once spawn in `world_loader.rs`.
- **Depends on:** E19 (same subsystem); importer world index (done); spec ENGINE_MAP Part F (F.1, F.5); `WORLD_SURVEY.md`.
- **Tests:** unit: rectangle/hysteresis/cell-grid arithmetic, ROI enter/exit ordering; differential: trace the entity ids passing `OnEnterProcessingRange`/`OnExitProcessingRange` (eCEntityAdmin::Process 0x10230a70) along a scripted path and compare sets per 100 ms bucket.
- **Size:** 717 lines (M).

### E21 P4 world/streaming: gCNegZone_PSObj, gCCollisionCircle_PSObj, gCDynamicCollisionCircle_PS +3

- **Scope:** 59 substantive / 151 functions, 50120 code bytes, 0 already documented. Classes (substantive/all, address range): `gCDynamicCollisionCircle_PS` [G] 11/30 0x10106a30-0x10108c20; `gCWaterZone_PS` [G] 9/28 0x1011c5c0-0x1011d740; `gCNegZone_PS` [G] 5/12 0x10119470-0x1011a290; `gCCollisionCircle_PSObj` [G] 13/34 0x10105ca0-0x1013c110; `gCPrefPath_PSObj` [G] 6/17 0x10140bb0-0x10141d20; `gCNegZone_PSObj` [G] 15/30 0x1013c430-0x10140900.
- **Key functions:** `RunTargetInZoneNavigation` 0x1013e9b0[G] 4774B, `TestPreferredPathUse` 0x10140bb0[G] 3648B, `TestRealIntersection` 0x1013c8b0[G] 3557B, `CalcPreferedStandInterPose` 0x10107200[G] 3156B, `ConeTriangleIntersects` 0x10141d20[G] 2827B, `RunNPCInZoneNavigation` 0x1013fc60[G] 2394B, `AddCollisionCircleOffset` 0x1013b580[G] 1590B, `CalcAngleSumStakeA` 0x1013da90[G] 1392B.
- **Spec:** none yet (new analysis needed; start with the class method list in the tsv).
- **Target:** `crates/world` (residency planner gains the original rules: cell grid, rectangle, hysteresis, ROI), `crates/runtime/src/world.rs` for processing-range enter/exit events, `apps/game/src/world_loader.rs` as the asset side.
- **Replaces/extends:** the synthetic residency laboratory in `crates/world` and the all-at-once spawn in `world_loader.rs`.
- **Depends on:** E20 (same subsystem); importer world index (done); spec ENGINE_MAP Part F (F.1, F.5); `WORLD_SURVEY.md`.
- **Tests:** unit: rectangle/hysteresis/cell-grid arithmetic, ROI enter/exit ordering; differential: trace the entity ids passing `OnEnterProcessingRange`/`OnExitProcessingRange` (eCEntityAdmin::Process 0x10230a70) along a scripted path and compare sets per 100 ms bucket.
- **Size:** 2349 lines (L).

### E22 P4 world/streaming: eCPropertyProxy, eCPropertyProxyCollection, eCPropertySetProxy +12

- **Scope:** 39 substantive / 195 functions, 17268 code bytes, 1 already documented. Classes (substantive/all, address range): `eCEngineComponentBase` [E] 0/11; `eCPropertyProxy` [E] 9/19 0x1022e800-0x1022eea0; `eCStaticLight_PS` [E] 0/10; `eCPropertyProxyCollection` [E] 9/19 0x1022f140-0x1022f5b0; `eCPropertySetProxy` [E] 7/20 0x1022f760-0x1022fd60; `gCPrefPath_PS` [G] 2/15 0x1011b2b0-0x1011b560; +9 more classes.
- **Key functions:** `OnProcess` 0x100d5120[G] 1153B, `TriggerDistance` 0x100d5620[G] 847B, `OnNotifyPropertyValueChangedExitEx` 0x10104040[G] 549B, `CreateShape` 0x1006a490[G] 442B, `CreatePSObject` 0x10103ea0[G] 385B, `CreatePSObject` 0x1011b2b0[G] 385B, `Execute` 0x1022e800[E] 326B, `RebuildInteractPoints` 0x1001c4e0[G] 309B.
- **Spec:** ENGINE_MAP: 2.1 Key functions (1).
- **Target:** `crates/world` (residency planner gains the original rules: cell grid, rectangle, hysteresis, ROI), `crates/runtime/src/world.rs` for processing-range enter/exit events, `apps/game/src/world_loader.rs` as the asset side.
- **Replaces/extends:** the synthetic residency laboratory in `crates/world` and the all-at-once spawn in `world_loader.rs`.
- **Depends on:** E21 (same subsystem); importer world index (done); spec ENGINE_MAP Part F (F.1, F.5); `WORLD_SURVEY.md`.
- **Tests:** unit: rectangle/hysteresis/cell-grid arithmetic, ROI enter/exit ordering; differential: trace the entity ids passing `OnEnterProcessingRange`/`OnExitProcessingRange` (eCEntityAdmin::Process 0x10230a70) along a scripted path and compare sets per 100 ms bucket.
- **Size:** 863 lines (M).

### E23 P5 save/load: gCWorld

- **Scope:** 40 substantive / 74 functions, 15495 code bytes, 10 already documented. Classes (substantive/all, address range): `gCWorld` [G] 40/74 0x10017aa0-0x1019bc20.
- **Key functions:** `RestartWorld` 0x1019a300[G] 1460B, `RestoreFiles` 0x1019af70[G] 1314B, `BackupFiles` 0x1019aa50[G] 1308B, `CreateSystemLayer` 0x10199c50[G] 1030B, `GetSaveGameHeader` 0x10198c40[G] 1003B, `FUN_10197530` 0x10197530[G] 954B, `GetSaveGameList` 0x10199030[G] 647B, `FUN_101978f0` 0x101978f0[G] 566B.
- **Spec:** ENGINE_MAP: F.3 Save game read/write and version handling [V] (7); ENGINE_MAP: F.1 Object model: workspace, project, database, wo (4).
- **Target:** new `crates/gameplay/src/save.rs` (versioned core state) and `crates/runtime` load/save wiring; original-format reader only as an optional importer (`crates/formats/src/save.rs`) for the header and the per-layer blobs.
- **Replaces/extends:** the JSON save/load in `crates/runtime/src/core.rs`.
- **Depends on:** world batches (layer and sector state), inventory batches (stack state), `PROPERTY_STREAMS.md`; spec ENGINE_MAP F.3.
- **Tests:** unit: round trip of a populated core, version migration, corrupted header; for the optional reader: parse the header of real saves in the user folder (`Documents\\gothic3`) and check magic `GENO`/`MESG`, version 0xaf/0xb0, four world versions; no original bytes in fixtures.
- **Size:** 774 lines (M).

### E24 P5 save/load: gCWorld

- **Scope:** 40 substantive / 60 functions, 20065 code bytes, 7 already documented. Classes (substantive/all, address range): `gCWorld` [G] 40/60 0x1019bca0-0x101dc8bf.
- **Key functions:** `LoadGameWorld` 0x1019d170[G] 5922B, `SaveGameWorld` 0x1019c6b0[G] 2597B, `DoAfterWorldActivate` 0x1019f610[G] 1261B, `DoLoadData` 0x1019e8b0[G] 1041B, `DoSaveData` 0x1019ecd0[G] 807B, `DoLoadLocalConfiguration` 0x1019f1f0[G] 798B, `OnActivate` 0x1019fb00[G] 666B, `ReleaseData` 0x1019fde0[G] 502B.
- **Spec:** ENGINE_MAP: F.1 Object model: workspace, project, database, wo (7); ENGINE_MAP: F.3 Save game read/write and version handling [V] (3).
- **Target:** new `crates/gameplay/src/save.rs` (versioned core state) and `crates/runtime` load/save wiring; original-format reader only as an optional importer (`crates/formats/src/save.rs`) for the header and the per-layer blobs.
- **Replaces/extends:** the JSON save/load in `crates/runtime/src/core.rs`.
- **Depends on:** E23 (same subsystem); world batches (layer and sector state), inventory batches (stack state), `PROPERTY_STREAMS.md`; spec ENGINE_MAP F.3.
- **Tests:** unit: round trip of a populated core, version migration, corrupted header; for the optional reader: parse the header of real saves in the user folder (`Documents\\gothic3`) and check magic `GENO`/`MESG`, version 0xaf/0xb0, four world versions; no original bytes in fixtures.
- **Size:** 857 lines (M).

### E25 P5 save/load: gCSession, eCDynamicEntity, gCSectorPersistence_PS +5

- **Scope:** 58 substantive / 118 functions, 30678 code bytes, 15 already documented. Classes (substantive/all, address range): `eCAudioEmitter_PS` [E] 1/2 0x10057d60-0x10057d60; `eCDirectionalLight_PS` [E] 0/1; `eCEffector` [E] 0/2; `eCDynamicEntity` [E] 5/5 0x102295c0-0x1022ac70; `gCSessionInfo` [G] 0/14; `gCSession` [G] 49/81 0x10170e60-0x101775b0; +2 more classes.
- **Key functions:** `RegisterEvents` 0x10174500[G] 6189B, `OnCheat` 0x10171430[G] 1414B, `OnGotoEntity` 0x101719c0[G] 1226B, `Start` 0x10173d80[G] 1080B, `ReadSaveGame` 0x10229700[E] 872B, `OnKeyboard` 0x10176bb0[G] 847B, `WriteSaveGame` 0x10229a70[E] 826B, `PostInitialize` 0x101775b0[G] 796B.
- **Spec:** ENGINE_MAP: Startup order (as far as visible) [V unless noted] (6); ENGINE_MAP: Session API (4); ENGINE_MAP: F.1 Object model: workspace, project, database, wo (3).
- **Target:** new `crates/gameplay/src/save.rs` (versioned core state) and `crates/runtime` load/save wiring; original-format reader only as an optional importer (`crates/formats/src/save.rs`) for the header and the per-layer blobs.
- **Replaces/extends:** the JSON save/load in `crates/runtime/src/core.rs`.
- **Depends on:** E24 (same subsystem); world batches (layer and sector state), inventory batches (stack state), `PROPERTY_STREAMS.md`; spec ENGINE_MAP F.3.
- **Tests:** unit: round trip of a populated core, version migration, corrupted header; for the optional reader: parse the header of real saves in the user folder (`Documents\\gothic3`) and check magic `GENO`/`MESG`, version 0xaf/0xb0, four world versions; no original bytes in fixtures.
- **Size:** 1374 lines (M).

### E26 P5 save/load: eCEntityDynamicContext, eCEntity, eCTrigger_PS +41

- **Scope:** 60 substantive / 89 functions, 24268 code bytes, 2 already documented. Classes (substantive/all, address range): `eCVisualAnimationLipSync` [E] 2/2 0x10027690-0x10027710; `eCVisualAnimationFactory` [E] 2/2 0x1003aca0-0x1003ae80; `eCVisualAnimationLoD` [E] 1/2 0x1003bbb0-0x1003bbb0; `eCTrigger_PS` [E] 3/3 0x1007dec0-0x1007e1a0; `eCTriggerBase_PS` [E] 0/1; `eCTriggerCombiner_PS` [E] 0/2; +38 more classes.
- **Key functions:** `Read` 0x1021f4d0[E] 2536B, `ReadV63` 0x10225840[E] 1790B, `ReadSaveGame` 0x1021d850[E] 1522B, `Read` 0x101cf980[E] 1299B, `ReadV83` 0x10225f40[E] 1210B, `Write` 0x1021fec0[E] 1042B, `WriteSaveGame` 0x1021d460[E] 988B, `Write` 0x10226400[E] 982B.
- **Spec:** ENGINE_MAP: 6.4 Mesh/animation LOD resources [V names, U detai (1); ENGINE_MAP: B14. Facial animation and lip-sync [V unless marke (1).
- **Target:** new `crates/gameplay/src/save.rs` (versioned core state) and `crates/runtime` load/save wiring; original-format reader only as an optional importer (`crates/formats/src/save.rs`) for the header and the per-layer blobs.
- **Replaces/extends:** the JSON save/load in `crates/runtime/src/core.rs`.
- **Depends on:** E25 (same subsystem); world batches (layer and sector state), inventory batches (stack state), `PROPERTY_STREAMS.md`; spec ENGINE_MAP F.3.
- **Tests:** unit: round trip of a populated core, version migration, corrupted header; for the optional reader: parse the header of real saves in the user folder (`Documents\\gothic3`) and check magic `GENO`/`MESG`, version 0xaf/0xb0, four world versions; no original bytes in fixtures.
- **Size:** 1213 lines (M).

### E27 P5 save/load: gCInventory_PS, gCNavigation_PS, gCStatsAdmin +56

- **Scope:** 71 substantive / 127 functions, 23615 code bytes, 1 already documented. Classes (substantive/all, address range): `eCVisualAnimation_PS::eCActorAttachment` [E] 2/2 0x1002a880-0x1002a950; `eCVisualAnimation_PS::eCLookAtConstraint` [E] 1/2 0x1002ae60-0x1002ae60; `eCVisualAnimation_PS` [E] 2/2 0x1002c000-0x1002c250; `eCWrapper_emfx2Actor` [E] 1/1 0x1004b810-0x1004b810; `eCWrapper_emfx2Motion` [E] 2/2 0x10050d20-0x10052740; `eCVisualMeshDynamic_PS` [E] 2/3 0x10212d20-0x10212e80; +53 more classes.
- **Key functions:** `Read` 0x10189ef0[G] 2097B, `Write` 0x1018a730[G] 1564B, `Read` 0x1004b810[E] 1486B, `Write` 0x10052740[E] 1293B, `Read` 0x100ed2c0[G] 1064B, `Write` 0x10037470[G] 960B, `Read` 0x10037100[G] 877B, `Read` 0x1002c000[E] 589B.
- **Spec:** ENGINE_MAP: D.1 Class map (1).
- **Target:** new `crates/gameplay/src/save.rs` (versioned core state) and `crates/runtime` load/save wiring; original-format reader only as an optional importer (`crates/formats/src/save.rs`) for the header and the per-layer blobs.
- **Replaces/extends:** the JSON save/load in `crates/runtime/src/core.rs`.
- **Depends on:** E26 (same subsystem); world batches (layer and sector state), inventory batches (stack state), `PROPERTY_STREAMS.md`; spec ENGINE_MAP F.3.
- **Tests:** unit: round trip of a populated core, version migration, corrupted header; for the optional reader: parse the header of real saves in the user folder (`Documents\\gothic3`) and check magic `GENO`/`MESG`, version 0xaf/0xb0, four world versions; no original bytes in fixtures.
- **Size:** 1180 lines (M).

### E28 P6 GUI: gCGUIManager, gCHUDDialogMain2, gCGUIModule

- **Scope:** 56 substantive / 97 functions, 17163 code bytes, 31 already documented. Classes (substantive/all, address range): `gCGUIManager` [G] 38/63 0x10020fb0-0x100a6bf0; `gCGUIModule` [G] 3/10 0x1007a420-0x1007acc0; `gCHUDDialogMain2` [G] 15/24 0x1008a900-0x100a6b30.
- **Key functions:** `PrintGameMessage` 0x1007c5c0[G] 2095B, `ImportLogSettings` 0x1007cf10[G] 1615B, `FUN_1008ac00` 0x1008ac00[G] 1302B, `HandleMessage` 0x1007d9e0[G] 1186B, `ExportLogSettings` 0x1007d570[G] 899B, `FUN_1008aa40` 0x1008aa40[G] 442B, `FUN_1008b340` 0x1008b340[G] 394B, `CloseMenu` 0x1007c380[G] 291B.
- **Spec:** ENGINE_MAP: 5.1 gCGUIManager (the single entry point for gamep (25); ENGINE_MAP: 2.1 Files involved (3); ENGINE_MAP: 2.4 Bootstrapping order in the Game [V] (2).
- **Target:** `crates/ui` (`logic.rs` page/state logic, `screens.rs`, `hud.rs`); input-key handling in `apps/game`.
- **Replaces/extends:** generic placeholders in `crates/ui/src/screens.rs` where the original page behaviour differs.
- **Depends on:** `UI_ORIGINAL.md` (themes, igl, coordinates done); inventory batches for item pages; spec ENGINE_MAP Part "original UI stack".
- **Tests:** unit: page open/close input-mode switch, list filtering/sorting, tab state, option-to-setting mapping (ge3.ini keys); snapshot of view-model output; visual compare against `.local` reference screenshots of the original.
- **Size:** 858 lines (M).

### E29 P6 GUI: gCHUDDialogMenu2, gCHUDViewQuests2, gCHUDViewSkills2 +15

- **Scope:** 57 substantive / 161 functions, 21279 code bytes, 1 already documented. Classes (substantive/all, address range): `gCHUDDialog` [G] 0/6; `gCHUDPage` [G] 0/13; `gCHUDViewQuests2` [G] 9/16 0x10084080-0x100a04d0; `gCHUDViewSkills2` [G] 9/13 0x100857d0-0x100a4f30; `gCHUDViewSpells2` [G] 8/13 0x10086d70-0x100a5280; `gCHUDPageDialog2` [G] 2/7 0x10087ab0-0x10087b10; +12 more classes.
- **Key functions:** `FUN_1008bca0` 0x1008bca0[G] 827B, `FUN_10084690` 0x10084690[G] 817B, `FUN_10085da0` 0x10085da0[G] 577B, `FUN_10088670` 0x10088670[G] 542B, `FUN_100884b0` 0x100884b0[G] 439B, `FUN_10085ae0` 0x10085ae0[G] 360B, `FUN_100841e0` 0x100841e0[G] 354B, `FUN_10084080` 0x10084080[G] 348B.
- **Spec:** ENGINE_MAP: 5.4 HUD dialog (`gCHUDDialogMain2`) [V unless note (1).
- **Target:** `crates/ui` (`logic.rs` page/state logic, `screens.rs`, `hud.rs`); input-key handling in `apps/game`.
- **Replaces/extends:** generic placeholders in `crates/ui/src/screens.rs` where the original page behaviour differs.
- **Depends on:** E28 (same subsystem); `UI_ORIGINAL.md` (themes, igl, coordinates done); inventory batches for item pages; spec ENGINE_MAP Part "original UI stack".
- **Tests:** unit: page open/close input-mode switch, list filtering/sorting, tab state, option-to-setting mapping (ge3.ini keys); snapshot of view-model output; visual compare against `.local` reference screenshots of the original.
- **Size:** 1063 lines (M).

### E30 P6 GUI: gCHUDViewRecipe2, gCHUDViewDocument2, gCHUDViewCharacter2 +26

- **Scope:** 60 substantive / 146 functions, 17335 code bytes, 0 already documented. Classes (substantive/all, address range): `gCHUDViewAmount2` [G] 3/8 0x1007e060-0x1007e240; `gCHUDViewCharacter2` [G] 5/8 0x1007e490-0x100a5e00; `gCHUDViewItem2` [G] 5/8 0x1007eca0-0x100a5540; `gCHUDViewRecipe2` [G] 7/10 0x1007f2d0-0x100a4a50; `gCHUDViewTutorial2` [G] 3/9 0x10080240-0x10080380; `gCHUDViewDialog2` [G] 1/5 0x1009ee40-0x1009ee40; +23 more classes.
- **Key functions:** `FUN_100a5e00` 0x100a5e00[G] 1159B, `FUN_100826c0` 0x100826c0[G] 985B, `FUN_10081e20` 0x10081e20[G] 646B, `FUN_1007f500` 0x1007f500[G] 590B, `FUN_1007edb0` 0x1007edb0[G] 346B, `FUN_10083e00` 0x10083e00[G] 279B, `FUN_1007e240` 0x1007e240[G] 275B, `FUN_1007e910` 0x1007e910[G] 265B.
- **Spec:** none yet (new analysis needed; start with the class method list in the tsv).
- **Target:** `crates/ui` (`logic.rs` page/state logic, `screens.rs`, `hud.rs`); input-key handling in `apps/game`.
- **Replaces/extends:** generic placeholders in `crates/ui/src/screens.rs` where the original page behaviour differs.
- **Depends on:** E29 (same subsystem); `UI_ORIGINAL.md` (themes, igl, coordinates done); inventory batches for item pages; spec ENGINE_MAP Part "original UI stack".
- **Tests:** unit: page open/close input-mode switch, list filtering/sorting, tab state, option-to-setting mapping (ge3.ini keys); snapshot of view-model output; visual compare against `.local` reference screenshots of the original.
- **Size:** 866 lines (M).

### E31 P6 GUI: gCHUDMenuFiles2, gCHUDMenuOptionsVideo2, gCHUDMenuMain2 +12

- **Scope:** 58 substantive / 154 functions, 51932 code bytes, 3 already documented. Classes (substantive/all, address range): `gCHUDMenuFiles2` [G] 16/25 0x1008d830-0x100aa040; `gCHUDMenuMain2` [G] 7/15 0x1008f1d0-0x100909e0; `gCHUDMenuOptionsAudio2` [G] 6/17 0x10091690-0x100aaa10; `gCHUDMenuOptionsPerformanceCustom2` [G] 7/19 0x10097e60-0x100ad670; `gCHUDMenuOptionsVideo2` [G] 8/19 0x1009daf0-0x100b0df0; `gCHUDListCtrlDialog` [G] 2/6 0x1009ec90-0x1009ed20; +9 more classes.
- **Key functions:** `FUN_100988f0` 0x100988f0[G] 19351B, `FUN_1008ddf0` 0x1008ddf0[G] 1872B, `FUN_1008f850` 0x1008f850[G] 994B, `FUN_100a2d40` 0x100a2d40[G] 986B, `FUN_1008f1d0` 0x1008f1d0[G] 613B, `FUN_100a0fd0` 0x100a0fd0[G] 599B, `FUN_1008dbe0` 0x1008dbe0[G] 521B, `FUN_1008f440` 0x1008f440[G] 502B.
- **Spec:** ENGINE_MAP: 8.3 List controls [V for call structure] (2); ENGINE_MAP: 7.2 Quickslots [V] (1).
- **Target:** `crates/ui` (`logic.rs` page/state logic, `screens.rs`, `hud.rs`); input-key handling in `apps/game`.
- **Replaces/extends:** generic placeholders in `crates/ui/src/screens.rs` where the original page behaviour differs.
- **Depends on:** E30 (same subsystem); `UI_ORIGINAL.md` (themes, igl, coordinates done); inventory batches for item pages; spec ENGINE_MAP Part "original UI stack".
- **Tests:** unit: page open/close input-mode switch, list filtering/sorting, tab state, option-to-setting mapping (ge3.ini keys); snapshot of view-model output; visual compare against `.local` reference screenshots of the original.
- **Size:** 1779 lines (L).

### E32 P6 GUI: gCHUDMenuOptionsControls2, gCHUDMenuOptionsControlsCustom2, gCHUDMenuConfirm2 +36

- **Scope:** 51 substantive / 265 functions, 53141 code bytes, 2 already documented. Classes (substantive/all, address range): `gCHUDOptionPerformanceShadowQuality` [G] 3/10 0x10001390-0x100af2d0; `gCHUDMenu` [G] 1/14 0x1009ea50-0x1009ea50; `gCHUDMenuOptions2` [G] 3/10 0x10090c90-0x10090f50; `gCHUDMenuConfirm2` [G] 4/9 0x1008c800-0x1008ca00; `gCHUDMenuCredits2` [G] 3/9 0x1008cd20-0x1008ce10; `gCHUDMenuDifficulty2` [G] 1/9 0x1008d110-0x1008d110; +33 more classes.
- **Key functions:** `FUN_10097230` 0x10097230[G] 850B, `FUN_10090f50` 0x10090f50[G] 532B, `FUN_100ad750` 0x100ad750[G] 526B, `FUN_10097770` 0x10097770[G] 492B, `FUN_10092870` 0x10092870[G] 485B, `FUN_1008d110` 0x1008d110[G] 413B, `FUN_1008ca00` 0x1008ca00[G] 387B, `FUN_100b09a0` 0x100b09a0[G] 369B.
- **Spec:** ENGINE_MAP: 11.1 Mechanics [V] (1); ENGINE_MAP: 11.3 Slider semantics (from ge3.INI comments and t (1).
- **Target:** `crates/ui` (`logic.rs` page/state logic, `screens.rs`, `hud.rs`); input-key handling in `apps/game`.
- **Replaces/extends:** generic placeholders in `crates/ui/src/screens.rs` where the original page behaviour differs.
- **Depends on:** E31 (same subsystem); `UI_ORIGINAL.md` (themes, igl, coordinates done); inventory batches for item pages; spec ENGINE_MAP Part "original UI stack".
- **Tests:** unit: page open/close input-mode switch, list filtering/sorting, tab state, option-to-setting mapping (ge3.ini keys); snapshot of view-model output; visual compare against `.local` reference screenshots of the original.
- **Size:** 2025 lines (L).

### E33 P6 GUI: gCHUDTableItemStats, gCHUDLog, gCHUDTextEffect +25

- **Scope:** 52 substantive / 121 functions, 18741 code bytes, 9 already documented. Classes (substantive/all, address range): `gCHUDStaticTalk` [G] 0/1; `gCHUDStaticQuest` [G] 5/8 0x1009f500-0x1009fbc0; `gCHUDStaticQuestLocation` [G] 0/1; `gCHUDTabCtrlQuests` [G] 0/3; `gCHUDStaticInvCash` [G] 3/5 0x100a3280-0x100a6dc0; `gCHUDStaticInvCategory` [G] 0/1; +22 more classes.
- **Key functions:** `FUN_1009fbc0` 0x1009fbc0[G] 1447B, `FUN_100a5a50` 0x100a5a50[G] 858B, `FUN_1009f500` 0x1009f500[G] 764B, `FUN_100a7880` 0x100a7880[G] 635B, `FUN_100a3df0` 0x100a3df0[G] 619B, `FUN_100a3bb0` 0x100a3bb0[G] 566B, `FUN_100a8a60` 0x100a8a60[G] 563B, `FUN_100a87f0` 0x100a87f0[G] 558B.
- **Spec:** ENGINE_MAP: 7.1 Bars (health/mana/endurance/focus) [V] (4); ENGINE_MAP: 7.3 Compass [V] (3); ENGINE_MAP: 7.4 Minimap / world map [V for structure] (2).
- **Target:** `crates/ui` (`logic.rs` page/state logic, `screens.rs`, `hud.rs`); input-key handling in `apps/game`.
- **Replaces/extends:** generic placeholders in `crates/ui/src/screens.rs` where the original page behaviour differs.
- **Depends on:** E32 (same subsystem); `UI_ORIGINAL.md` (themes, igl, coordinates done); inventory batches for item pages; spec ENGINE_MAP Part "original UI stack".
- **Tests:** unit: page open/close input-mode switch, list filtering/sorting, tab state, option-to-setting mapping (ge3.ini keys); snapshot of view-model output; visual compare against `.local` reference screenshots of the original.
- **Size:** 937 lines (M).

### E34 P6 GUI: gCScreenOutputCtrl, gCMap_PS, gCSessionKeys +6

- **Scope:** 49 substantive / 151 functions, 35666 code bytes, 5 already documented. Classes (substantive/all, address range): `gSMapMarker` [G] 0/4; `gSMapGenerate` [G] 0/3; `gCDialogManagerCtrl` [G] 4/25 0x100729b0-0x100735f0; `gCNpcInfoCtrl` [G] 5/17 0x10074340-0x10074950; `gCQuestLogCtrl` [G] 4/19 0x10075e30-0x10077190; `gCScreenOutputCtrl` [G] 15/27 0x10077630-0x100792c0; +3 more classes.
- **Key functions:** `CreateDefaultKeys` 0x1017f690[G] 11873B, `CreateINIKeys` 0x10182500[G] 1230B, `CalcTextRect` 0x10079870[G] 1083B, `PrintScreen_InfoScript_Say` 0x10077880[G] 920B, `Init` 0x100743e0[G] 852B, `FillTextureWithMarkers` 0x101a47f0[G] 758B, `GenerateMarkers` 0x101a43d0[G] 742B, `Init` 0x10075e30[G] 698B.
- **Spec:** ENGINE_MAP: 7.5 Floating focus names [V] (4); ENGINE_MAP: 9.1 Dialog UI [V for structure] (1).
- **Target:** `crates/ui` (`logic.rs` page/state logic, `screens.rs`, `hud.rs`); input-key handling in `apps/game`.
- **Replaces/extends:** generic placeholders in `crates/ui/src/screens.rs` where the original page behaviour differs.
- **Depends on:** E33 (same subsystem); `UI_ORIGINAL.md` (themes, igl, coordinates done); inventory batches for item pages; spec ENGINE_MAP Part "original UI stack".
- **Tests:** unit: page open/close input-mode switch, list filtering/sorting, tab state, option-to-setting mapping (ge3.ini keys); snapshot of view-model output; visual compare against `.local` reference screenshots of the original.
- **Size:** 1339 lines (M).

### E35 P7 audio: gCMusicSequencer, eCAudioModule, eCAudioEmitter_PS +7

- **Scope:** 60 substantive / 246 functions, 21883 code bytes, 84 already documented. Classes (substantive/all, address range): `eCAudioModule` [E] 11/28 0x10054490-0x10055440; `eCAudioChannelGroup` [E] 1/11 0x100541f0-0x100541f0; `eCAudioChannel` [E] 5/52 0x100539e0-0x10053e80; `eCAudioSystem` [E] 0/17; `eCAudioSound` [E] 8/31 0x100560e0-0x100567d0; `eCAudioEmitter_PS` [E] 9/40 0x10057c70-0x10058600; +4 more classes.
- **Key functions:** `UpdateOcclusion` 0x100548e0[E] 676B, `ExecuteTrigger` 0x100fec60[G] 610B, `PlaySample3D` 0x10054f80[E] 596B, `OnProcess` 0x10057ef0[E] 583B, `FUN_100ff1f0` 0x100ff1f0[G] 567B, `PlaySample2D` 0x10054d50[E] 550B, `FindFallback` 0x100fe290[G] 514B, `CreateSound` 0x10058330[E] 418B.
- **Spec:** ENGINE_MAP: C1. Classes and addresses (65); ENGINE_MAP: C10.4 Transition choice and timing (10); ENGINE_MAP: C4. FMOD calls and flag sets [V unless marked] (8).
- **Target:** `crates/audio` (`mix.rs`, `music.rs`, `sfx.rs`, `speech.rs`).
- **Replaces/extends:** the generic emitter/music code where it differs from the original numbers (ENGINE_MAP Part C cheat sheet).
- **Depends on:** `AUDIO_FORMATS.md`, formats `xact`; spec ENGINE_MAP Part C.
- **Tests:** unit: rolloff, priority/steal, music fragment/transition selection with seeded RNG; differential: FMOD call trace (already available from the earlier mix analysis) vs Rust channel plan for a scripted walk through a music trigger.
- **Size:** 1094 lines (M).

### E36 P7 audio: gCMusicFragment, gCMusicChannel, gCMusicLinkSequence +7

- **Scope:** 60 substantive / 182 functions, 17185 code bytes, 14 already documented. Classes (substantive/all, address range): `gCMusicModule` [G] 4/16 0x10100e30-0x10100fc0; `gCMusicChannel` [G] 9/33 0x100fc030-0x100fc5f0; `gCMusicFragment` [G] 10/31 0x100f1680-0x100f26a0; `gCMusicFragmentList` [G] 5/12 0x100f3740-0x100f98d0; `gCMusicLibrary` [G] 2/9 0x100f3e70-0x100f3f40; `gCMusicLinkSequence` [G] 9/19 0x100f5ab0-0x100f6100; +4 more classes.
- **Key functions:** `ImportLinks` 0x100f26a0[G] 807B, `SelectFragment` 0x100f98d0[G] 718B, `FUN_100f1d60` 0x100f1d60[G] 714B, `FUN_100f7400` 0x100f7400[G] 604B, `ImportOffset` 0x100f79e0[G] 520B, `SetDescriptor` 0x100fa410[G] 284B, `StartNextStream` 0x100fc3c0[G] 255B, `CompareItems` 0x100f37b0[G] 228B.
- **Spec:** ENGINE_MAP: C10.5 Channels, fades and volume (6); ENGINE_MAP: C10.2 State and triggers (4); ENGINE_MAP: C10.4 Transition choice and timing (2).
- **Target:** `crates/audio` (`mix.rs`, `music.rs`, `sfx.rs`, `speech.rs`).
- **Replaces/extends:** the generic emitter/music code where it differs from the original numbers (ENGINE_MAP Part C cheat sheet).
- **Depends on:** E35 (same subsystem); `AUDIO_FORMATS.md`, formats `xact`; spec ENGINE_MAP Part C.
- **Tests:** unit: rolloff, priority/steal, music fragment/transition selection with seeded RNG; differential: FMOD call trace (already available from the earlier mix analysis) vs Rust channel plan for a scripted walk through a music trigger.
- **Size:** 859 lines (M).

### E37 P7 audio: gCEffectCommandPlaySound, gCAmbientModule, gCAmbientSequencer +16

- **Scope:** 56 substantive / 260 functions, 20993 code bytes, 16 already documented. Classes (substantive/all, address range): `eCAudioStreamer` [E] 4/10 0x100569e0-0x10056ee0; `eSParticleSound` [E] 0/2; `gCMusicEventFragment` [G] 1/8 0x10100c30-0x10100c30; `gCMusicLink` [G] 2/16 0x100f42c0-0x100f4330; `gCEffectCommandPlaySound` [G] 8/19 0x10052380-0x10058390; `gCEffectCommandPlayVoice` [G] 2/11 0x10058f10-0x10059030; +13 more classes.
- **Key functions:** `UpdateWaterSounds` 0x101bab90[G] 1020B, `OpenFile` 0x10056bf0[E] 676B, `FUN_100569e0` 0x100569e0[E] 526B, `SelectSample` 0x10057f40[G] 410B, `Process` 0x101ba9a0[G] 287B, `Clone` 0x10058f10[G] 280B, `SetDescriptor` 0x1005c0d0[G] 278B, `Execute` 0x10059030[G] 262B.
- **Spec:** ENGINE_MAP: C11. Ambient system [V] (7); ENGINE_MAP: C1. Classes and addresses (4); ENGINE_MAP: C9. Effect commands PlaySound / MusicTrigger (3).
- **Target:** `crates/audio` (`mix.rs`, `music.rs`, `sfx.rs`, `speech.rs`).
- **Replaces/extends:** the generic emitter/music code where it differs from the original numbers (ENGINE_MAP Part C cheat sheet).
- **Depends on:** E36 (same subsystem); `AUDIO_FORMATS.md`, formats `xact`; spec ENGINE_MAP Part C.
- **Tests:** unit: rolloff, priority/steal, music fragment/transition selection with seeded RNG; differential: FMOD call trace (already available from the earlier mix analysis) vs Rust channel plan for a scripted walk through a music trigger.
- **Size:** 1049 lines (M).

### E38 P8 facial/lipsync: FacialLayerPass, eCVisualAnimationFactory, eCVisualAnimation_PS +3

- **Scope:** 16 substantive / 33 functions, 9037 code bytes, 7 already documented. Classes (substantive/all, address range): `eCVisualAnimation_PS` [E] 2/3 0x10030a20-0x10030b60; `eCVisualAnimationLipSync` [E] 1/8 0x10027650-0x10027650; `eCVisualAnimationFactory` [E] 3/4 0x1003a780-0x1003aa80; `eCWrapper_emfx2Motion` [E] 1/1 0x10052cb0-0x10052cb0; `EMotionFX::FacialLayerPass` [E] 9/15 0x102fafe0-0x102fbce0; `EMotionFX::SmartMeshMorphDeformer` [E] 0/2.
- **Key functions:** `FUN_102fb5b0` 0x102fb5b0[E] 1609B, `CreateLipSyncMotion` 0x10052cb0[E] 1529B, `FUN_102fb150` 0x102fb150[E] 528B, `StopFacialAnimation` 0x1003a8d0[E] 364B, `StartFacialAnimation` 0x1003a780[E] 326B, `PlayFacialMotion` 0x10030a20[E] 306B, `FUN_102fb3d0` 0x102fb3d0[E] 273B, `FUN_102fbce0` 0x102fbce0[E] 243B.
- **Spec:** ENGINE_MAP: B14. Facial animation and lip-sync [V unless marke (6); ENGINE_MAP: B1. Object model and who calls whom (2); ENGINE_MAP: B2. The eight motion slots and slot ownership [V u (2).
- **Target:** `crates/animation/src/face.rs` (phoneme timeline to morph weights, expression layers) fed by the speech start event from `crates/audio/src/speech.rs`.
- **Replaces/extends:** nothing (no facial animation exists yet).
- **Depends on:** E03..E06 (motion slots), formats `emfx` expression parts, phoneme chunks (`PhonemeMotionData`); spec ENGINE_MAP B14.
- **Tests:** unit: viseme timeline sampling, blend of expression layers; differential: trace the morph weights of one NPC during one voice line and compare per frame within 0.02.
- **Size:** 451 lines (S).

### E39 P9 navigation: gCNavigationMap

- **Scope:** 55 substantive / 75 functions, 8051 code bytes, 0 already documented. Classes (substantive/all, address range): `gCNavigationMap` [G] 55/75 0x10015e60-0x10122fd0.
- **Key functions:** `FUN_10015f00` 0x10015f00[G] 210B, `FUN_10122740` 0x10122740[G] 176B, `FUN_10016160` 0x10016160[G] 166B, `FUN_10015fe0` 0x10015fe0[G] 163B, `FUN_10122880` 0x10122880[G] 156B, `FUN_100187f0` 0x100187f0[G] 150B, `FUN_10015e60` 0x10015e60[G] 148B, `FUN_10018100` 0x10018100[G] 134B.
- **Spec:** none yet (new analysis needed; start with the class method list in the tsv).
- **Target:** `crates/nav` (zone graph, A*, zone/path tests) and `crates/sim` navigation component.
- **Replaces/extends:** the stand-in navmesh queries in `crates/nav`.
- **Depends on:** importer nav data (`NavigationMap.xnav` is authored data in sectors); spec ENGINE_MAP F.4.
- **Tests:** unit: zone point tests, cost function, deadlock prevention; differential: `FindWay` results (0x1012efc0) for 50 start/goal pairs in Ardea vs Rust path length within 2%.
- **Size:** 402 lines (S).

### E40 P9 navigation: gCNavigationMap

- **Scope:** 55 substantive / 60 functions, 87608 code bytes, 5 already documented. Classes (substantive/all, address range): `gCNavigationMap` [G] 55/60 0x10123040-0x101388b0.
- **Key functions:** `CompileStaticNavigationScene` 0x101294d0[G] 11120B, `BuildLists` 0x101236d0[G] 10317B, `FindWay` 0x1012efc0[G] 8383B, `TestEntityObstacles` 0x10135080[G] 6474B, `WriteLists` 0x10127630[G] 4894B, `FindWay` 0x10131090[G] 4520B, `GetZone` 0x1012d520[G] 4055B, `ReadLists` 0x10126810[G] 3580B.
- **Spec:** ENGINE_MAP: F.4 Navigation (summary) (5).
- **Target:** `crates/nav` (zone graph, A*, zone/path tests) and `crates/sim` navigation component.
- **Replaces/extends:** the stand-in navmesh queries in `crates/nav`.
- **Depends on:** E39 (same subsystem); importer nav data (`NavigationMap.xnav` is authored data in sectors); spec ENGINE_MAP F.4.
- **Tests:** unit: zone point tests, cost function, deadlock prevention; differential: `FindWay` results (0x1012efc0) for 50 start/goal pairs in Ardea vs Rust path length within 2%.
- **Size:** 2913 lines (L).

### E41 P9 navigation: gCNavigationMap

- **Scope:** 55 substantive / 61 functions, 7439 code bytes, 0 already documented. Classes (substantive/all, address range): `gCNavigationMap` [G] 55/61 0x101389a0-0x101836f0.
- **Key functions:** `FUN_101393d0` 0x101393d0[G] 285B, `FUN_10139260` 0x10139260[G] 273B, `FUN_10139910` 0x10139910[G] 267B, `FUN_10139540` 0x10139540[G] 262B, `FUN_10139100` 0x10139100[G] 261B, `FUN_10138aa0` 0x10138aa0[G] 232B, `FUN_10139730` 0x10139730[G] 230B, `FUN_10139ab0` 0x10139ab0[G] 222B.
- **Spec:** none yet (new analysis needed; start with the class method list in the tsv).
- **Target:** `crates/nav` (zone graph, A*, zone/path tests) and `crates/sim` navigation component.
- **Replaces/extends:** the stand-in navmesh queries in `crates/nav`.
- **Depends on:** E40 (same subsystem); importer nav data (`NavigationMap.xnav` is authored data in sectors); spec ENGINE_MAP F.4.
- **Tests:** unit: zone point tests, cost function, deadlock prevention; differential: `FindWay` results (0x1012efc0) for 50 start/goal pairs in Ardea vs Rust path length within 2%.
- **Size:** 371 lines (S).

### E42 P9 navigation: gCNavZone_PS, gCNavigationAdmin

- **Scope:** 60 substantive / 116 functions, 32204 code bytes, 2 already documented. Classes (substantive/all, address range): `gCNavZone_PS` [G] 35/57 0x101143d0-0x1011a230; `gCNavigationAdmin` [G] 25/59 0x100655e0-0x10121a50.
- **Key functions:** `GetEvade` 0x10115c30[G] 5171B, `IntersectsStretch` 0x10114da0[G] 3625B, `CompileNavigationScene` 0x1011ed90[G] 2677B, `Process` 0x10120720[G] 2661B, `Test3DPointInZone` 0x10117180[G] 933B, `CheckAllControlledDCCs` 0x10121190[G] 908B, `GetRandomNavigationPoint` 0x1011ff90[G] 874B, `RunDynamicNavigation` 0x10121530[G] 759B.
- **Spec:** ENGINE_MAP: F.4 Navigation (summary) (2).
- **Target:** `crates/nav` (zone graph, A*, zone/path tests) and `crates/sim` navigation component.
- **Replaces/extends:** the stand-in navmesh queries in `crates/nav`.
- **Depends on:** E41 (same subsystem); importer nav data (`NavigationMap.xnav` is authored data in sectors); spec ENGINE_MAP F.4.
- **Tests:** unit: zone point tests, cost function, deadlock prevention; differential: `FindWay` results (0x1012efc0) for 50 start/goal pairs in Ardea vs Rust path length within 2%.
- **Size:** 1470 lines (M).

### E43 P9 navigation: gCNavigation_PS, gCNavPath_PS, gCNavOffset_PS +1

- **Scope:** 40 substantive / 183 functions, 24259 code bytes, 0 already documented. Classes (substantive/all, address range): `gCNavPath_PS` [G] 18/55 0x10002270-0x1011b4e0; `gCNavigation_PS` [G] 21/109 0x1010acb0-0x1010e5c0; `gCNavOffset_PS` [G] 1/9 0x101102a0-0x101102a0; `gCNavHelper_PS` [G] 0/10.
- **Key functions:** `OnNotifyPropertyValueChangedExitEx` 0x1010adc0[G] 1890B, `FillNextForNavigationPath` 0x1010c440[G] 1836B, `FillNextForPreferredPath` 0x1010bdc0[G] 1617B, `OnPreProcess` 0x1010dfe0[G] 1396B, `OnEnterProcessingRange` 0x1010d630[G] 1392B, `Test3DPointOnPath` 0x10111ad0[G] 873B, `CalcNearestPathPoint` 0x10111f80[G] 872B, `GetCosts` 0x10112380[G] 766B.
- **Spec:** none yet (new analysis needed; start with the class method list in the tsv).
- **Target:** `crates/nav` (zone graph, A*, zone/path tests) and `crates/sim` navigation component.
- **Replaces/extends:** the stand-in navmesh queries in `crates/nav`.
- **Depends on:** E42 (same subsystem); importer nav data (`NavigationMap.xnav` is authored data in sectors); spec ENGINE_MAP F.4.
- **Tests:** unit: zone point tests, cost function, deadlock prevention; differential: `FindWay` results (0x1012efc0) for 50 start/goal pairs in Ardea vs Rust path length within 2%.
- **Size:** 1212 lines (M).

### E44 P9 effects/misc: gCEffectCommandSequence, gCBoid, gCEffectMap +1

- **Scope:** 56 substantive / 98 functions, 16037 code bytes, 0 already documented. Classes (substantive/all, address range): `gCEffectCommandSequence` [G] 23/36 0x1004fdf0-0x10051fe0; `gCEffect` [G] 9/22 0x1005e800-0x1005ec80; `gCEffectMap` [G] 11/20 0x1005eee0-0x1005f7b0; `gCBoid` [G] 13/20 0x101ab660-0x101ad390.
- **Key functions:** `FindTravelPoint` 0x101abe90[G] 2487B, `Process` 0x101ab660[G] 1666B, `InsertItem` 0x10051250[G] 734B, `GetPerceivedCenterVec` 0x101acb60[G] 570B, `CheckStateChanges` 0x101ad390[G] 475B, `AddFlock` 0x101ac900[G] 446B, `Travel` 0x101abcf0[G] 412B, `GetNeighbourVelocity` 0x101acf30[G] 409B.
- **Spec:** none yet (new analysis needed; start with the class method list in the tsv).
- **Target:** `crates/sim` / `crates/runtime` (effects as command sequences, flocks, enclaves) and `apps/game/src/play`.
- **Replaces/extends:** ad-hoc effect spawning in `apps/game/src/play/events.rs`.
- **Depends on:** audio and world batches; spec: none beyond the class names (new analysis needed).
- **Tests:** unit on effect command sequencing; visual compare for spawned effects.
- **Size:** 801 lines (M).

### E45 P9 effects/misc: gCFlock_PS, gCEffectSystem, gCEnclave_PS +11

- **Scope:** 50 substantive / 249 functions, 22752 code bytes, 0 already documented. Classes (substantive/all, address range): `gCEffectCommandEarthquake` [G] 2/14 0x1005b170-0x1005b2b0; `gCEffectCommandSpawnEntity` [G] 4/17 0x10054dd0-0x10054f40; `gCFlock_PS` [G] 9/54 0x10170780-0x101afb40; `gCEffectCommandCreateDecal` [G] 3/22 0x10059d70-0x10059e70; `gCEnclave_PS` [G] 5/27 0x10062750-0x10062bf0; `gCBoidsAdmin` [G] 5/13 0x101ad9c0-0x101adb80; +8 more classes.
- **Key functions:** `Process` 0x101af1f0[G] 2384B, `Execute` 0x10054f40[G] 1154B, `OnBoidStateChanged` 0x101aeea0[G] 780B, `OnTouch` 0x101aebe0[G] 693B, `BuildMemberList` 0x10062a00[G] 485B, `FindAssembly` 0x101adb80[G] 343B, `PlayAni` 0x101afb40[G] 330B, `AddMember` 0x10062860[G] 277B.
- **Spec:** none yet (new analysis needed; start with the class method list in the tsv).
- **Target:** `crates/sim` / `crates/runtime` (effects as command sequences, flocks, enclaves) and `apps/game/src/play`.
- **Replaces/extends:** ad-hoc effect spawning in `apps/game/src/play/events.rs`.
- **Depends on:** E44 (same subsystem); audio and world batches; spec: none beyond the class names (new analysis needed).
- **Tests:** unit on effect command sequencing; visual compare for spawned effects.
- **Size:** 1137 lines (M).

### E46 P9 render rules: eCShaderMaterialAdmin

- **Scope:** 37 substantive / 75 functions, 11556 code bytes, 8 already documented. Classes (substantive/all, address range): `eCShaderMaterialAdmin` [E] 37/75 0x10008d80-0x102378e0.
- **Key functions:** `CompileShaderPoolEllement` 0x102366b0[E] 1144B, `SetCurrentRenderStates` 0x10237660[E] 637B, `SetShaderInstanceStaticLightData` 0x10236fb0[E] 383B, `BeginMaterial` 0x102378e0[E] 374B, `GetCurrentRenderStates` 0x102374f0[E] 358B, `OnCameraChanged` 0x10237360[E] 337B, `SetRefractmap` 0x10236c40[E] 268B, `SetupShaderMacros` 0x102364d0[E] 239B.
- **Spec:** ENGINE_MAP: 1.2 Shader versions and the Shader.Cache [V] (2); ENGINE_MAP: 1.4 Global shader constants owned by eCShaderMater (2); ENGINE_MAP: 1.1 Class map [V unless noted] (1).
- **Target:** Bevy render plugins in `apps/game` (`world_loader`, materials, lights, sky, vegetation) with the rules as plain functions in a Bevy-free `crates/world` or `crates/animation` module so they can be unit tested.
- **Replaces/extends:** hand-tuned LOD distances, light/fog/sky constants and shadow settings in `apps/game/src/viewer.rs`/`walk.rs`.
- **Depends on:** ENGINE_MAP rendering reference (sections 1-8); `ORIGINAL_SHADING.md`; do not port the D3D9 internals.
- **Tests:** unit: threshold functions (area factor, stage thresholds, fade distance, shadow caster/receiver selection) with the numbers of ENGINE_MAP; visual: screenshot compare of fixed viewpoints against original captures in `.local/compare`.
- **Size:** 577 lines (M).

### E47 P9 render rules: eCShaderMaterialAdmin

- **Scope:** 36 substantive / 45 functions, 12715 code bytes, 10 already documented. Classes (substantive/all, address range): `eCShaderMaterialAdmin` [E] 36/45 0x10237a60-0x1025a120.
- **Key functions:** `SetCurrentShaderVersion` 0x10238090[E] 1438B, `PostInitialize` 0x10239340[E] 1273B, `ReadShaderPool` 0x10238930[E] 946B, `BeginZPass` 0x10237ab0[E] 717B, `WriteShaderPool` 0x10238cf0[E] 669B, `FUN_1023b190` 0x1023b190[E] 473B, `PreShutdown` 0x10239160[E] 466B, `SaveShaderPool` 0x10238f90[E] 464B.
- **Spec:** ENGINE_MAP: 1.1 Class map [V unless noted] (4); ENGINE_MAP: 1.2 Shader versions and the Shader.Cache [V] (3); ENGINE_MAP: 5.6 Z-pass types [V] (2).
- **Target:** Bevy render plugins in `apps/game` (`world_loader`, materials, lights, sky, vegetation) with the rules as plain functions in a Bevy-free `crates/world` or `crates/animation` module so they can be unit tested.
- **Replaces/extends:** hand-tuned LOD distances, light/fog/sky constants and shadow settings in `apps/game/src/viewer.rs`/`walk.rs`.
- **Depends on:** E46 (same subsystem); ENGINE_MAP rendering reference (sections 1-8); `ORIGINAL_SHADING.md`; do not port the D3D9 internals.
- **Tests:** unit: threshold functions (area factor, stage thresholds, fade distance, shadow caster/receiver selection) with the numbers of ENGINE_MAP; visual: screenshot compare of fixed viewpoints against original captures in `.local/compare`.
- **Size:** 635 lines (M).

### E48 P9 render rules: eCParticle_PS

- **Scope:** 31 substantive / 212 functions, 14617 code bytes, 0 already documented. Classes (substantive/all, address range): `eCParticle_PS` [E] 31/212 0x1014bc50-0x10150330.
- **Key functions:** `ProcessParticles` 0x1014ffd0[E] 853B, `DoProcessV1` 0x1014fcc0[E] 781B, `CalcColorFactor` 0x1014f790[E] 552B, `CalcExtents` 0x1014fae0[E] 465B, `OnNotifyPropertyValueChangedExitEx` 0x1014eba0[E] 359B, `FUN_10150330` 0x10150330[E] 329B, `Dump` 0x1014f1b0[E] 278B, `CalcFrame` 0x1014f4e0[E] 236B.
- **Spec:** none yet (new analysis needed; start with the class method list in the tsv).
- **Target:** Bevy render plugins in `apps/game` (`world_loader`, materials, lights, sky, vegetation) with the rules as plain functions in a Bevy-free `crates/world` or `crates/animation` module so they can be unit tested.
- **Replaces/extends:** hand-tuned LOD distances, light/fog/sky constants and shadow settings in `apps/game/src/viewer.rs`/`walk.rs`.
- **Depends on:** E47 (same subsystem); ENGINE_MAP rendering reference (sections 1-8); `ORIGINAL_SHADING.md`; do not port the D3D9 internals.
- **Tests:** unit: threshold functions (area factor, stage thresholds, fade distance, shadow caster/receiver selection) with the numbers of ENGINE_MAP; visual: screenshot compare of fixed viewpoints against original captures in `.local/compare`.
- **Size:** 730 lines (M).

### E49 P9 render rules: eCParticle_PS

- **Scope:** 31 substantive / 43 functions, 26288 code bytes, 0 already documented. Classes (substantive/all, address range): `eCParticle_PS` [E] 31/43 0x10150480-0x1015b8a0.
- **Key functions:** `UpdateVertices` 0x10153630[E] 5664B, `ApplyDefaults` 0x101514b0[E] 3383B, `TickParticles` 0x10155800[E] 3348B, `Update` 0x101509f0[E] 2631B, `EmitParticles` 0x10156520[E] 1439B, `Spawn` 0x10150530[E] 1206B, `CreateVisualFF` 0x101523f0[E] 918B, `ApplyRenderStatesFF` 0x10153000[E] 900B.
- **Spec:** none yet (new analysis needed; start with the class method list in the tsv).
- **Target:** Bevy render plugins in `apps/game` (`world_loader`, materials, lights, sky, vegetation) with the rules as plain functions in a Bevy-free `crates/world` or `crates/animation` module so they can be unit tested.
- **Replaces/extends:** hand-tuned LOD distances, light/fog/sky constants and shadow settings in `apps/game/src/viewer.rs`/`walk.rs`.
- **Depends on:** E48 (same subsystem); ENGINE_MAP rendering reference (sections 1-8); `ORIGINAL_SHADING.md`; do not port the D3D9 internals.
- **Tests:** unit: threshold functions (area factor, stage thresholds, fade distance, shadow caster/receiver selection) with the numbers of ENGINE_MAP; visual: screenshot compare of fixed viewpoints against original captures in `.local/compare`.
- **Size:** 1144 lines (M).

### E50 P9 render rules: eCMeshElement

- **Scope:** 44 substantive / 63 functions, 20025 code bytes, 0 already documented. Classes (substantive/all, address range): `eCMeshElement` [E] 44/63 0x1006cf40-0x101d20a0.
- **Key functions:** `CreateLightmapUVGroups` 0x101bee60[E] 6264B, `FUN_101d0f30` 0x101d0f30[E] 3025B, `FindProjectionPlane` 0x101be750[E] 748B, `FUN_101d0ca0` 0x101d0ca0[E] 655B, `FUN_101d1ce0` 0x101d1ce0[E] 630B, `FUN_101c1aa0` 0x101c1aa0[E] 616B, `SetMaterialFile` 0x101c1290[E] 593B, `IntersectsRay` 0x101c0c90[E] 569B.
- **Spec:** none yet (new analysis needed; start with the class method list in the tsv).
- **Target:** Bevy render plugins in `apps/game` (`world_loader`, materials, lights, sky, vegetation) with the rules as plain functions in a Bevy-free `crates/world` or `crates/animation` module so they can be unit tested.
- **Replaces/extends:** hand-tuned LOD distances, light/fog/sky constants and shadow settings in `apps/game/src/viewer.rs`/`walk.rs`.
- **Depends on:** E49 (same subsystem); ENGINE_MAP rendering reference (sections 1-8); `ORIGINAL_SHADING.md`; do not port the D3D9 internals.
- **Tests:** unit: threshold functions (area factor, stage thresholds, fade distance, shadow caster/receiver selection) with the numbers of ENGINE_MAP; visual: screenshot compare of fixed viewpoints against original captures in `.local/compare`.
- **Size:** 836 lines (M).

### E51 P9 render rules: eCMaterialAdmin

- **Scope:** 40 substantive / 81 functions, 27167 code bytes, 3 already documented. Classes (substantive/all, address range): `eCMaterialAdmin` [E] 40/81 0x1012ed90-0x10135450.
- **Key functions:** `PostInitialize` 0x10132d70[E] 8951B, `ResetResources` 0x10131d00[E] 2224B, `PreShutdown` 0x10131540[E] 1879B, `RestoreResources` 0x10132630[E] 1744B, `ExecuteDefaultFX` 0x10130d50[E] 989B, `ExecutePoolFX` 0x10130a60[E] 729B, `RunLODIndicator` 0x10130400[E] 633B, `RunPerformanceIndicator` 0x101306a0[E] 583B.
- **Spec:** ENGINE_MAP: 1.1 Class map [V unless noted] (3).
- **Target:** Bevy render plugins in `apps/game` (`world_loader`, materials, lights, sky, vegetation) with the rules as plain functions in a Bevy-free `crates/world` or `crates/animation` module so they can be unit tested.
- **Replaces/extends:** hand-tuned LOD distances, light/fog/sky constants and shadow settings in `apps/game/src/viewer.rs`/`walk.rs`.
- **Depends on:** E50 (same subsystem); ENGINE_MAP rendering reference (sections 1-8); `ORIGINAL_SHADING.md`; do not port the D3D9 internals.
- **Tests:** unit: threshold functions (area factor, stage thresholds, fade distance, shadow caster/receiver selection) with the numbers of ENGINE_MAP; visual: screenshot compare of fixed viewpoints against original captures in `.local/compare`.
- **Size:** 1060 lines (M).

### E52 P9 render rules: eCIlluminationAdmin, eCFrustumDatabase

- **Scope:** 58 substantive / 128 functions, 30245 code bytes, 13 already documented. Classes (substantive/all, address range): `eCIlluminationAdmin` [E] 32/65 0x10018770-0x101d5360; `eCFrustumDatabase` [E] 26/63 0x100bcab0-0x101a29e0.
- **Key functions:** `Execute` 0x101a0530[E] 7385B, `ExtractFromFrustumOC` 0x1019e590[E] 2237B, `ExecuteShadowMap` 0x101a2220[E] 1607B, `ExtractFromFrustum` 0x1019ee70[E] 1332B, `GetIlluminationAt` 0x100e4920[E] 1087B, `ExtractFromShadowMap` 0x1019f460[E] 1062B, `ResetResources` 0x100e5ef0[E] 596B, `GetDynamicStaticLightData` 0x100e5620[E] 544B.
- **Spec:** ENGINE_MAP: 2.1 Key functions (4); ENGINE_MAP: 5.1 Classes [V] (3); ENGINE_MAP: 7.3 `eCFrustumDatabase::Execute` order [V sequence (2).
- **Target:** Bevy render plugins in `apps/game` (`world_loader`, materials, lights, sky, vegetation) with the rules as plain functions in a Bevy-free `crates/world` or `crates/animation` module so they can be unit tested.
- **Replaces/extends:** hand-tuned LOD distances, light/fog/sky constants and shadow settings in `apps/game/src/viewer.rs`/`walk.rs`.
- **Depends on:** E51 (same subsystem); ENGINE_MAP rendering reference (sections 1-8); `ORIGINAL_SHADING.md`; do not port the D3D9 internals.
- **Tests:** unit: threshold functions (area factor, stage thresholds, fade distance, shadow caster/receiver selection) with the numbers of ENGINE_MAP; visual: screenshot compare of fixed viewpoints against original captures in `.local/compare`.
- **Size:** 1293 lines (M).

### E53 P9 render rules: eCVegetation_GridNode, eCSkydome_PS

- **Scope:** 49 substantive / 104 functions, 31628 code bytes, 12 already documented. Classes (substantive/all, address range): `eCVegetation_GridNode` [E] 26/43 0x100869b0-0x1027e630; `eCSkydome_PS` [E] 23/61 0x1008d8f0-0x10275500.
- **Key functions:** `UpdateSkydomeEnvCubemap` 0x10273700[E] 3967B, `CreateAbsorbationImage` 0x10272880[E] 2791B, `CreateShader` 0x1026fdd0[E] 2627B, `CreateBuffers` 0x1026f440[E] 2280B, `CacheInGeometry` 0x1027d830[E] 1889B, `CreateLensFlare` 0x10272160[E] 1817B, `OnRenderAlpha` 0x10270fa0[E] 1790B, `OnRender` 0x102709c0[E] 1493B.
- **Spec:** ENGINE_MAP: 4.1 Classes and addresses (12).
- **Target:** Bevy render plugins in `apps/game` (`world_loader`, materials, lights, sky, vegetation) with the rules as plain functions in a Bevy-free `crates/world` or `crates/animation` module so they can be unit tested.
- **Replaces/extends:** hand-tuned LOD distances, light/fog/sky constants and shadow settings in `apps/game/src/viewer.rs`/`walk.rs`.
- **Depends on:** E52 (same subsystem); ENGINE_MAP rendering reference (sections 1-8); `ORIGINAL_SHADING.md`; do not port the D3D9 internals.
- **Tests:** unit: threshold functions (area factor, stage thresholds, fade distance, shadow caster/receiver selection) with the numbers of ENGINE_MAP; visual: screenshot compare of fixed viewpoints against original captures in `.local/compare`.
- **Size:** 1533 lines (L).

### E54 P9 render rules: eCVegetation_Grid, eCStrip_PS

- **Scope:** 43 substantive / 93 functions, 14540 code bytes, 1 already documented. Classes (substantive/all, address range): `eCStrip_PS` [E] 20/51 0x102759c0-0x102777c0; `eCVegetation_Grid` [E] 23/42 0x1027b380-0x1027cef0.
- **Key functions:** `InsertEntryCheckSpacing` 0x1027bfd0[E] 1421B, `UpdateVertices` 0x10277210[E] 1164B, `Render` 0x1027c9a0[E] 739B, `CreateVisual` 0x10276da0[E] 680B, `ResizeGrid` 0x1027ba20[E] 607B, `RenderInternal` 0x10276720[E] 557B, `FUN_10275b70` 0x10275b70[E] 535B, `CacheIn` 0x1027c5f0[E] 474B.
- **Spec:** ENGINE_MAP: 6.5 Vegetation grid [V] (1).
- **Target:** Bevy render plugins in `apps/game` (`world_loader`, materials, lights, sky, vegetation) with the rules as plain functions in a Bevy-free `crates/world` or `crates/animation` module so they can be unit tested.
- **Replaces/extends:** hand-tuned LOD distances, light/fog/sky constants and shadow settings in `apps/game/src/viewer.rs`/`walk.rs`.
- **Depends on:** E53 (same subsystem); ENGINE_MAP rendering reference (sections 1-8); `ORIGINAL_SHADING.md`; do not port the D3D9 internals.
- **Tests:** unit: threshold functions (area factor, stage thresholds, fade distance, shadow caster/receiver selection) with the numbers of ENGINE_MAP; visual: screenshot compare of fixed viewpoints against original captures in `.local/compare`.
- **Size:** 727 lines (M).

### E55 P9 render rules: eCWeatherZone_PS, eCSpeedTreeAdmin, eCParticleModule

- **Scope:** 55 substantive / 137 functions, 20891 code bytes, 5 already documented. Classes (substantive/all, address range): `eCParticleModule` [E] 17/29 0x10020220-0x10161bf0; `eCWeatherZone_PS` [E] 19/68 0x1028bb50-0x102970b0; `eCSpeedTreeAdmin` [E] 19/40 0x101c8210-0x102bbf10.
- **Key functions:** `FUN_102af9a0` 0x102af9a0[E] 929B, `FUN_102bbf10` 0x102bbf10[E] 904B, `OnEndRenderSolid` 0x10209200[E] 748B, `OnBeginRenderSolid` 0x10208f60[E] 656B, `IntersectsRay` 0x10296400[E] 650B, `EndRenderTerrainShadows` 0x10209590[E] 580B, `GetBoxBlendFactor` 0x10296b60[E] 580B, `CreateParticleMaterial` 0x101610c0[E] 580B.
- **Spec:** ENGINE_MAP: 6.6 SpeedTree [V names, I behaviour] (3); ENGINE_MAP: 3.2 Weather classes (2); ENGINE_MAP: 5.5 Terrain shadow map and tree shadows [V] (1).
- **Target:** Bevy render plugins in `apps/game` (`world_loader`, materials, lights, sky, vegetation) with the rules as plain functions in a Bevy-free `crates/world` or `crates/animation` module so they can be unit tested.
- **Replaces/extends:** hand-tuned LOD distances, light/fog/sky constants and shadow settings in `apps/game/src/viewer.rs`/`walk.rs`.
- **Depends on:** E54 (same subsystem); ENGINE_MAP rendering reference (sections 1-8); `ORIGINAL_SHADING.md`; do not port the D3D9 internals.
- **Tests:** unit: threshold functions (area factor, stage thresholds, fade distance, shadow caster/receiver selection) with the numbers of ENGINE_MAP; visual: screenshot compare of fixed viewpoints against original captures in `.local/compare`.
- **Size:** 1044 lines (M).

### E56 P9 render rules: eCResourceMaterialObject_PS, eCVegetationBrush_PS, eCWeatherAdmin +1

- **Scope:** 60 substantive / 211 functions, 33096 code bytes, 10 already documented. Classes (substantive/all, address range): `eCShadowMapDirectionalLight` [E] 14/33 0x10104090-0x10104e00; `eCVegetationBrush_PS` [E] 15/56 0x10280680-0x1028add0; `eCResourceMaterialObject_PS` [E] 16/89 0x101dc090-0x101dd530; `eCWeatherAdmin` [E] 15/33 0x1028c3f0-0x1028f200.
- **Key functions:** `Process` 0x1028d070[E] 6208B, `RecreateCompleteMaterial` 0x101dc090[E] 2097B, `CalcDistribution` 0x10280de0[E] 1616B, `UpdateInternals` 0x10104610[E] 1466B, `LoadWeatherStates` 0x1028c870[E] 1073B, `InsertEntries` 0x10281a70[E] 995B, `FindWorldPosAndNormal` 0x10281760[E] 766B, `DoLoad` 0x101dd1b0[E] 617B.
- **Spec:** ENGINE_MAP: 5.1 Classes [V] (6); ENGINE_MAP: 3.2 Weather classes (4).
- **Target:** Bevy render plugins in `apps/game` (`world_loader`, materials, lights, sky, vegetation) with the rules as plain functions in a Bevy-free `crates/world` or `crates/animation` module so they can be unit tested.
- **Replaces/extends:** hand-tuned LOD distances, light/fog/sky constants and shadow settings in `apps/game/src/viewer.rs`/`walk.rs`.
- **Depends on:** E55 (same subsystem); ENGINE_MAP rendering reference (sections 1-8); `ORIGINAL_SHADING.md`; do not port the D3D9 internals.
- **Tests:** unit: threshold functions (area factor, stage thresholds, fade distance, shadow caster/receiver selection) with the numbers of ENGINE_MAP; visual: screenshot compare of fixed viewpoints against original captures in `.local/compare`.
- **Size:** 1494 lines (M).

### E57 P9 render rules: eCVegetation_PS, eCShadowMapDirectionalLightLsPSM, eCVisualMeshBase_PS +1

- **Scope:** 52 substantive / 128 functions, 26112 code bytes, 18 already documented. Classes (substantive/all, address range): `eCShadowMapDirectionalLightLsPSM` [E] 13/23 0x101055f0-0x10107300; `eCVisualMeshBase_PS` [E] 13/36 0x1020e4c0-0x1020f7e0; `eCOcclusionProcessor` [E] 12/24 0x101a3d10-0x101a5260; `eCVegetation_PS` [E] 14/45 0x10214b80-0x102801e0.
- **Key functions:** `CreateLightmapResource` 0x1020ea30[E] 1601B, `FUN_10105ca0` 0x10105ca0[E] 1581B, `RenderDynamicOccludees` 0x101a4830[E] 1411B, `UpdateShadowMatrix` 0x101066f0[E] 1394B, `RenderShadowMapOccludees` 0x101a4dc0[E] 1177B, `GetLightmapResourceDataEntity` 0x1020f7e0[E] 1053B, `CopyLightmapsFrom` 0x1020f080[E] 877B, `CreateOccludeeGfxResources` 0x101a3d70[E] 839B.
- **Spec:** ENGINE_MAP: 7.5 Occlusion processor [V usage, U numbers] (8); ENGINE_MAP: 5.1 Classes [V] (7); ENGINE_MAP: 6.5 Vegetation grid [V] (2).
- **Target:** Bevy render plugins in `apps/game` (`world_loader`, materials, lights, sky, vegetation) with the rules as plain functions in a Bevy-free `crates/world` or `crates/animation` module so they can be unit tested.
- **Replaces/extends:** hand-tuned LOD distances, light/fog/sky constants and shadow settings in `apps/game/src/viewer.rs`/`walk.rs`.
- **Depends on:** E56 (same subsystem); ENGINE_MAP rendering reference (sections 1-8); `ORIGINAL_SHADING.md`; do not port the D3D9 internals.
- **Tests:** unit: threshold functions (area factor, stage thresholds, fade distance, shadow caster/receiver selection) with the numbers of ENGINE_MAP; visual: screenshot compare of fixed viewpoints against original captures in `.local/compare`.
- **Size:** 1305 lines (M).

### E58 P9 render rules: eCSpeedTreeWind_PS, eCStaticPointLight_PS, eCResourceMeshLoD_PS +2

- **Scope:** 56 substantive / 149 functions, 21711 code bytes, 1 already documented. Classes (substantive/all, address range): `eCStaticPointLight_PS` [E] 11/23 0x100f3e50-0x100f4a90; `eCSpeedTreeWind_PS` [E] 12/47 0x10208200-0x102b1450; `eCResourceMeshLoD_PS` [E] 11/31 0x101ced10-0x101cf760; `eCLightSet` [E] 11/25 0x10071a00-0x100e8270; `eCStaticSpotLight_PS` [E] 11/23 0x100f5bd0-0x100f6bc0.
- **Key functions:** `UpdateIllumination` 0x100e7c70[E] 1221B, `CalculateStaticSHLighting` 0x100f65a0[E] 1196B, `CalculateStaticSHLighting` 0x100f4590[E] 1060B, `CalculateLight` 0x100f62c0[E] 723B, `FUN_102b0fd0` 0x102b0fd0[E] 633B, `FUN_102b1450` 0x102b1450[E] 618B, `FUN_102b0d80` 0x102b0d80[E] 589B, `CalculateLight` 0x100f4360[E] 552B.
- **Spec:** ENGINE_MAP: 2.1 Key functions (1).
- **Target:** Bevy render plugins in `apps/game` (`world_loader`, materials, lights, sky, vegetation) with the rules as plain functions in a Bevy-free `crates/world` or `crates/animation` module so they can be unit tested.
- **Replaces/extends:** hand-tuned LOD distances, light/fog/sky constants and shadow settings in `apps/game/src/viewer.rs`/`walk.rs`.
- **Depends on:** E57 (same subsystem); ENGINE_MAP rendering reference (sections 1-8); `ORIGINAL_SHADING.md`; do not port the D3D9 internals.
- **Tests:** unit: threshold functions (area factor, stage thresholds, fade distance, shadow caster/receiver selection) with the numbers of ENGINE_MAP; visual: screenshot compare of fixed viewpoints against original captures in `.local/compare`.
- **Size:** 1085 lines (M).

### E59 P9 render rules: eCWeatherStates, eCDecalAdmin, eCVegetationAdmin +3

- **Scope:** 58 substantive / 164 functions, 31132 code bytes, 6 already documented. Classes (substantive/all, address range): `eCDecal` [E] 9/23 0x1006d310-0x1008d890; `eCDecalAdmin` [E] 10/26 0x1006f6a0-0x10070d10; `eCVegetationAdmin` [E] 10/24 0x10279ea0-0x1027ad70; `eCPointLight_PS` [E] 9/36 0x100f10e0-0x100f1ab0; `eCFrustumFilter` [E] 9/18 0x101ac3c0-0x101acc00; `eCWeatherStates` [E] 11/37 0x10290180-0x10291cc0.
- **Key functions:** `RenderDecals` 0x1006fd80[E] 2945B, `ClipPolygonAgainstPlane` 0x1006d600[E] 1421B, `ClipMesh` 0x1006ddc0[E] 1035B, `CreateDecals` 0x1006f7a0[E] 773B, `AddPolygon` 0x1006d310[E] 747B, `CreateBuffers` 0x100709f0[E] 711B, `GetMaterial` 0x1027a480[E] 701B, `CullSmallObjects` 0x101ac6d0[E] 619B.
- **Spec:** ENGINE_MAP: 3.2 Weather classes (2); ENGINE_MAP: 6.5 Vegetation grid [V] (2); ENGINE_MAP: 6.2 Area factor and mesh stage thresholds [V] (1).
- **Target:** Bevy render plugins in `apps/game` (`world_loader`, materials, lights, sky, vegetation) with the rules as plain functions in a Bevy-free `crates/world` or `crates/animation` module so they can be unit tested.
- **Replaces/extends:** hand-tuned LOD distances, light/fog/sky constants and shadow settings in `apps/game/src/viewer.rs`/`walk.rs`.
- **Depends on:** E58 (same subsystem); ENGINE_MAP rendering reference (sections 1-8); `ORIGINAL_SHADING.md`; do not port the D3D9 internals.
- **Tests:** unit: threshold functions (area factor, stage thresholds, fade distance, shadow caster/receiver selection) with the numbers of ENGINE_MAP; visual: screenshot compare of fixed viewpoints against original captures in `.local/compare`.
- **Size:** 1556 lines (L).

### E60 P9 render rules: eCSpeedTree_PS, eCFrustumElementItem, eCRenderContextMesh +5

- **Scope:** 57 substantive / 204 functions, 28243 code bytes, 15 already documented. Classes (substantive/all, address range): `eCFrustumElementItem` [E] 8/32 0x101a2d20-0x101a35c0; `eCRenderMeshElement` [E] 6/10 0x101d98b0-0x101da070; `eCIlluminated_PS` [E] 6/37 0x100ee350-0x100ee900; `eCDecal_PS` [E] 6/31 0x100722a0-0x10072b60; `eCDynamicLight_PS` [E] 7/21 0x100eb6e0-0x100ec070; `eCDirectionalLight_PS` [E] 7/27 0x100e8630-0x100ea620; +2 more classes.
- **Key functions:** `OnRender` 0x100e90a0[E] 4421B, `UpdateShadowMaps` 0x100e8630[E] 1941B, `UpdateEffectedObjects` 0x100eb8d0[E] 1533B, `FillDescriptor` 0x101d9bc0[E] 915B, `IntersectsRay` 0x10072890[E] 710B, `OnShadowSettingsChanged` 0x100e8de0[E] 566B, `PrefetchBegin` 0x101d98b0[E] 553B, `Prefetch` 0x101d9340[E] 552B.
- **Spec:** ENGINE_MAP: 6.1 Pipeline per visible element [V] (5); ENGINE_MAP: 7.1 Contexts and ops [V] (5); ENGINE_MAP: 5.2 Which shadow maps exist [V values, I naming] (2).
- **Target:** Bevy render plugins in `apps/game` (`world_loader`, materials, lights, sky, vegetation) with the rules as plain functions in a Bevy-free `crates/world` or `crates/animation` module so they can be unit tested.
- **Replaces/extends:** hand-tuned LOD distances, light/fog/sky constants and shadow settings in `apps/game/src/viewer.rs`/`walk.rs`.
- **Depends on:** E59 (same subsystem); ENGINE_MAP rendering reference (sections 1-8); `ORIGINAL_SHADING.md`; do not port the D3D9 internals.
- **Tests:** unit: threshold functions (area factor, stage thresholds, fade distance, shadow caster/receiver selection) with the numbers of ENGINE_MAP; visual: screenshot compare of fixed viewpoints against original captures in `.local/compare`.
- **Size:** 1341 lines (M).

### E61 P9 render rules: eCResourceShaderMaterial_PS, eCWeatherEnvironment, eCSkyObject +10

- **Scope:** 58 substantive / 268 functions, 39797 code bytes, 16 already documented. Classes (substantive/all, address range): `eCSkyObject` [E] 5/21 0x1026e4c0-0x1026eea0; `eCVisualAnimationLoD` [E] 5/18 0x1003b5a0-0x1003bad0; `eCRenderContextAnimation` [E] 4/13 0x10044680-0x100448f0; `eCResourceShaderMaterial_PS` [E] 6/31 0x101df990-0x101e09e0; `eCRenderContextLegacy` [E] 4/14 0x101ad660-0x101ad8b0; `eCTerrainShadowMap` [E] 4/14 0x101082e0-0x101084d0; +7 more classes.
- **Key functions:** `Update` 0x10293190[E] 4315B, `ConvertFrom` 0x101df990[E] 2638B, `SetResourceMesh` 0x10288d00[E] 2124B, `Update` 0x101084d0[E] 1409B, `Process` 0x1028f780[E] 922B, `FillVertexBuffer` 0x1026e7d0[E] 862B, `OnNotifyPropertyValueChangedExitEx` 0x10294280[E] 702B, `CreateBuffers` 0x1026e4c0[E] 699B.
- **Spec:** ENGINE_MAP: 1.1 Class map [V unless noted] (3); ENGINE_MAP: 4.1 Classes and addresses (3); ENGINE_MAP: B1. Object model and who calls whom (3).
- **Target:** Bevy render plugins in `apps/game` (`world_loader`, materials, lights, sky, vegetation) with the rules as plain functions in a Bevy-free `crates/world` or `crates/animation` module so they can be unit tested.
- **Replaces/extends:** hand-tuned LOD distances, light/fog/sky constants and shadow settings in `apps/game/src/viewer.rs`/`walk.rs`.
- **Depends on:** E60 (same subsystem); ENGINE_MAP rendering reference (sections 1-8); `ORIGINAL_SHADING.md`; do not port the D3D9 internals.
- **Tests:** unit: threshold functions (area factor, stage thresholds, fade distance, shadow caster/receiver selection) with the numbers of ENGINE_MAP; visual: screenshot compare of fixed viewpoints against original captures in `.local/compare`.
- **Size:** 1920 lines (L).

### E62 P9 render rules: eCShadowMapDirectionalLightBase, eCVegetationRubber_PS, eCIPImageFilterMaskHightLights +44

- **Scope:** 45 substantive / 399 functions, 46037 code bytes, 10 already documented. Classes (substantive/all, address range): `eCGfxFog` [E] 1/17 0x1008f420-0x1008f420; `eCGfxLight` [E] 1/21 0x10094570-0x10094570; `eCShadowMapBase` [E] 2/14 0x10103cb0-0x10103d50; `eCShadowMapDirectionalLightBase` [E] 3/15 0x10105180-0x10105260; `eCShadowMapPointLight` [E] 1/12 0x10107590-0x10107590; `eCMaterialEffect` [E] 1/14 0x10147ea0-0x10147ea0; +41 more classes.
- **Key functions:** `BeginEffectApplication` 0x10147ea0[E] 10819B, `UpdateCamera` 0x10107590[E] 1358B, `Render` 0x101ad040[E] 1159B, `UpdateSHHemisphere` 0x100ecf50[E] 870B, `UpdateVertexData` 0x10116f10[E] 691B, `DoLoad` 0x101dea70[E] 610B, `PreExecuteFilterShader` 0x1011a3b0[E] 539B, `PreExecuteFilterShader` 0x101173f0[E] 537B.
- **Spec:** ENGINE_MAP: 5.1 Classes [V] (5); ENGINE_MAP: 3.1 Fog [V] (2); ENGINE_MAP: 2.1 Key functions (2).
- **Target:** Bevy render plugins in `apps/game` (`world_loader`, materials, lights, sky, vegetation) with the rules as plain functions in a Bevy-free `crates/world` or `crates/animation` module so they can be unit tested.
- **Replaces/extends:** hand-tuned LOD distances, light/fog/sky constants and shadow settings in `apps/game/src/viewer.rs`/`walk.rs`.
- **Depends on:** E61 (same subsystem); ENGINE_MAP rendering reference (sections 1-8); `ORIGINAL_SHADING.md`; do not port the D3D9 internals.
- **Tests:** unit: threshold functions (area factor, stage thresholds, fade distance, shadow caster/receiver selection) with the numbers of ENGINE_MAP; visual: screenshot compare of fixed viewpoints against original captures in `.local/compare`.
- **Size:** 1861 lines (L).

## 5. First five batches (start here)

Common rules: work in a fresh module, keep the Bevy-free part testable with `cargo test -p <crate>`, cite the original address in a comment, read only the files listed in the tsv as substantive, and never commit decompiled code. All addresses below are `[G]` Game.dll unless marked `[E]`. Read ENGINE_MAP.md Part D / E / B first (sections named per batch). Each batch ends with the app using the new module and the stand-in deleted.

### E00 Gameplay camera including dialog camera

- **Goal:** replace the 78-line `play_camera` (fixed 280-unit boom, simple dialog lerp) and `update_camera` in `apps/game/src/walk.rs:1229` with the original solver.
- **Files:** create `crates/gameplay/src/camera.rs` (add `pub mod camera;` to `crates/gameplay/src/lib.rs`); keep `apps/game/src/play/cam.rs` as the thin driver that builds `CameraInput` from Bevy resources and writes the `Transform` (world units are centimetres, `CM = 0.01` to Bevy meters as today).
- **Types:** `CameraMode` (enum 0..13, ENGINE_MAP D.3 "Modes"), `CameraPreset` (the 33-field `gCCameraModeParams` object, parsed from `Camera_PC_Camera.tple` through `gothic_formats::template`; keep field order, do not hardcode the 27 presets), `CameraState` (azimuth, elevation, distance, relaxed pivot, relaxed camera position, FOV, tremor, dialog roles, 18 s dialog timer), `CameraInput` (dt, hero matrix, head position, movement mode, in-combat flag, mouse deltas, wheel, mode requests), `trait CameraWorld { fn ray_nearest(&self, from: Vec3, dir: Vec3, len: f32) -> Option<f32>; fn sphere_blocked(...) }` implemented by `gothic_physics::CollisionWorld`.
- **Order of work (substantive functions in `E00.tsv`):** (1) relax filter + `UpdateDeltaRotX/Y` 0x10026690/0x10026750 + `UpdateDistance` 0x10026860 + `GetCurrentWishedDistance` 0x10026ae0 + `UpdateFOV` 0x10026a40; (2) `UpdatePlayerPosition` 0x10025410 (pivot, head matrix 0x1002b2e0, species head bones), `GetCurrentWishedTranslation` 0x10023190 and `GetCurrentWishedRotation` 0x10024810, `ProcessCurrentMode` 0x10022d10, `UpdateCameraPosition` 0x10026350, `GetFirstPersonMat` 0x1002b100; (3) mode switching `SetMode` 0x1002c4b0, `OnAction` 0x1002b6c0 (zoom wheel, first-person switch at 200 units past minimum, look-back), `SetTarget` 0x1002c040, hero alpha fade and `EnableItemRendering` 0x1002b490; (4) dialog camera `SetDialogRoles` 0x1002ce40, `StartDialogCamera` 0x1002cfc0, `FindPreset` 0x1002d1a0, `DialogPresetIsValid` 0x1002dca0, `StopDialogCamera` 0x1002dc10, the two "Lerp" presets with the 18 s progress, 0.3/0.25 weights, mirrored side flag; (5) collision as behavioural spec: boom ray from pivot, snap closer instantly, relax outward, first-person push-out 17.5 in a 35 box (`CorrectCameraPosition` 0x10026b80 body is not recoverable, `MaintainWallDistance` 0x1002aa10 is); (6) `AddTremor` 0x10024f40 / `ApplyTremor` 0x10025260 and `UpdateListenerPosition` 0x10025c90 / `UpdateSpeakerPosition` 0x10025ff0 (audio listener/speaker point for `crates/audio`).
- **Wiring:** `Command::CameraDialogRoles{speaker, listener}` / `StopDialogCamera` in `crates/runtime` (the dialog session already knows both actors), `Event`-free otherwise. Expose near plane 25 (13 in first person) and the hero fade to the app.
- **Tests:** unit as in the batch table; golden values: pitch clamp +-89, rest distance 150, moving 300, cap 800, tau 0.5 s camera / 0.05 s pivot / 0.625 s FOV. Differential: record 60 s of `OnProcess` 0x100215c0 inputs and outputs with the tracer (walk, run, wheel zoom to first person, one dialog), replay in a Rust test reading `.local/traces/camera_*.json` (skipped when absent).
- **Done when:** `cargo test -p gothic-gameplay` passes, `--play` third-person and dialog use the new solver, the old fixed boom code is gone, and `HANDOFF_CLAUDE.md` lists the open `CorrectCameraPosition` approximation.
- **Size:** 84 kB of original machine code, of which 25 kB is `PostInitializeProperties` (preset defaults, replaced by the .tple) and 16 kB `CorrectCameraPosition` (not recoverable); estimator 2300 lines, expect 1500-2000 (L); can be split at step (4) into two sessions.

### E01 Controlled movement pipeline and locomotion mixer

- **Goal:** input to goal pose to velocity to turn, plus the idle/directional/turn-in-place mixer that picks the motions (ENGINE_MAP E.4, E.5, B6).
- **Files:** create `crates/sim/src/movement.rs` (pure functions + `MovementState`), extend `crates/animation/src/locomotion.rs` where its constants differ from B6, and make `crates/physics/src/controller.rs` consume `MovementState` (it keeps the collision resolution).
- **Functions (Game):** `gCCharacterControl_PS::ProcessMovements` 0x100ddd40 (wish direction: flatten camera forward/right, sum axes, normalise, camera-relative vs character-relative flag), `OnAction` 0x100de480, `gCCharacterSensor_PS::ProcessPlayerMovements` 0x100eec80, `SmoothGoal` 0x100eeb40, `TestSoftCollisions` 0x100ef4c0; `gCCharacterMovement_PS::ProcessControledTranslation` 0x100e2410, `ProcessControledRotation` 0x100e3000, `...LookAt` 0x100e4060, `...ToTarget` 0x100e3fb0, `ProcessQuadrupedXRotation` 0x100e4550, `CalcNewRotationAngleAndVelocity` 0x100e49f0, `CorrectGoalMiss` 0x100e4de0, `CalcAniVelocity` 0x100e1cd0, `ProcessMovementAnimation` 0x100e1a50, `AddToCurrentVelocity` 0x100e1ee0; the unnamed mixer routines 0x100d94b0, 0x100d9940 (idle/stand mixer), 0x100d9d80 (turn in place), 0x100da480 (directional locomotion core), 0x100db690, 0x100dc1d0, 0x100dae20 and neighbours up to 0x100dd9d0 (documented in B6.1..B6.4).
- **Rules to reproduce:** acceleration/deceleration (999 999 means instant), turn rate 200 deg/s (100 sprinting) clamped to the remaining angle, animation-velocity filter 0.125 s feeding a speed/direction blend, walk/run/sprint/sneak selection, root-motion vs speed-driven locomotion split (E.5).
- **Tests:** unit with `PHYSICS_MOVEMENT.md` numbers; differential: trace `ProcessControledTranslation` inputs/outputs and the motion names chosen by the mixer for a scripted input sequence; compare chosen motion names exactly and velocities within 1 %.
- **Done when:** the hero's motion choice in `--play` comes from `movement.rs` + `locomotion.rs`, and no gait/turn constants remain duplicated in `walk.rs`.
- **Size:** about 42 kB of original code, roughly 2000 lines (L).

### E02 Movement mode machine, ground following, sliding, swimming

- **Goal:** `SetMovementMode` as the only writer of the mode (modes 1..8, 10, 0xc, 0xd, 0xe, ENGINE_MAP E.2), ground/water/ceiling probing, slide, stuck detection, shape per mode.
- **Files:** extend `crates/sim/src/movement.rs`; `crates/physics/src/controller.rs` `MoveMode` grows from 4 to the real set (today only Ground, Slide 0xc, Jump 6, Fall 0xd).
- **Functions:** `SetMovementMode` 0x100e7630, `CheckChangedMovementStates` 0x100e5cd0, `SetFlagsByMode` 0x100e79c0, `SetEffectsByMode` 0x100e7c50, `SetSpeedByMode`/`GetSpeedByMode` 0x100e9390/0x100e9450, `SetShapeByMode` 0x100e8ca0, `CreateDependantShapes` 0x100e8370, `PutToGround` 0x100e94f0, `FindFloorWaterCeiling` 0x100eb030, `CanSlide` 0x100ea700, `CanWalkOnFloor` 0x100ea420, `CanStandAtGoal` 0x100ea4e0, `CanFallDown` 0x100ea650, `IsFreeLOSAndSurrounding` 0x100ea8f0, `CheckCharacterGotStuck` 0x100e6760, `GetCurrentMaxSpeedOfDir` 0x100eb800, `CalcGroundSlopeTransScale` 0x100ebc80, `ProcessPhysicsMovements` 0x100e5110, `OnTouch/OnUntouch/OnIntersect` 0x100ebf10/0x100ec150/0x100ec210, `GetGoalReached` 0x100e70b0, goal setters 0x100e6a50..0x100e7000, `GetMovementModeAtPosition` 0x100e7fc0.
- **Host events:** after a mode change fire `OnMovementModeChanged(entity, previous)` into the existing script host (`crates/script` callbacks) so `GetLastMovementMode` works; swim depth 155 for the hero with 10-unit hysteresis; landing grace 0.3 s; fall start height and `LastFallVelocity`.
- **Tests:** unit state-transition table (mode x condition); differential: trace `SetMovementMode` call sequence (mode, previous) while walking into water, off a ledge, up a steep slope and into a wall; compare the sequence exactly.
- **Open:** modes 9 and 0xb (not produced anywhere we found), swim vs dive margins.
- **Size:** about 34 kB, roughly 1600 lines (L).

### E03 Animation transitions: admin, wrapper blend primitives, motion instances

- **Goal:** the cross-fade rules that make transitions look like the original (ENGINE_MAP B2..B4).
- **Files:** `crates/animation/src/runtime.rs` (slots, fade, stop prevention) and a new `blend.rs` for the pure planner `plan_transition(slot, current, requested, intro, extro, variation) -> Vec<MotionOp>`; parse `G3_Ani_Blendings.txt` in `crates/animation/src/library.rs` if not yet done.
- **Functions [E]:** `eCAnimationAdmin::GetAniIntroBlending` 0x100213a0, `GetAniExtroBlending` 0x10021450, `GetAniVariation` 0x10021290, `SetAniVariationMax` 0x100211c0, `QueryMotionDataEntity` 0x10021070, `Initialize` 0x10020a20; `eCWrapper_emfx2Actor::PlayMotion` 0x100476f0, `StopMotion` 0x10047910, `RunStopPrevention` 0x10047600, per-frame `Process` 0x10047a00, `SetTargetWeight` 0x10049ca0, `SwitchFadeOut` 0x10049dd0, `IsActorPlaying` 0x1004a040, `HasMotionInstance` 0x1004a0d0; `EMotionFX::SkeletalMotionInstance` and `MotionLayerSystem` unnamed routines 0x102e9e20..0x102eaad0, 0x102ed1c0..0x102ed620 (layer weights and ramps; these sit in the statically linked EMotionFX library and are attributed here by address).
- **Rules:** eight motion slots and ownership (B2), fade lengths from `G3_Ani_Blendings.txt` with the `%` variation suffix (B4), `SwitchFadeOut` as the only cross-fade primitive, stop prevention for non-interruptible motions, per-frame instance update.
- **Tests:** unit: planner output for stand to walk, walk to run, run to stop, attack to idle with the shipped blending table; differential: trace `PlayMotion`/`SwitchFadeOut` arguments for the scripted session of E01 and compare the op sequence.
- **Done when:** `Animator` transitions in `--play` come from the planner and the hero no longer pops between locomotion clips.
- **Size:** about 19 kB, roughly 970 lines (M).

### E04 Visual animation property set: motion play API, attachments, look-at, ragdoll, aim

- **Goal:** the layer above the wrapper that scripts and gameplay call (`PlayMotion`/`StopMotion`/`StopAtLoopEnd`, body parts, weapon/item attachments, look-at constraints, aim overlay, ragdoll switch, frame effects such as footsteps).
- **Files:** `crates/animation/src/runtime.rs` (call API), new `attach.rs` (attachment bookkeeping incl. "once" attachments), `lookat.rs` (constraint set with targets), `ragdoll.rs` hooks into `crates/physics/src/ragdoll.rs`, aim overlay in `locomotion.rs`; frame effects (`UpdateFrameEffects` 0x1002eff0, 4.5 kB: sound/effect events on motion frames) emit `AnimEvent` for `crates/audio` and effects.
- **Functions [E]:** `eCVisualAnimation_PS::PlayMotion` 0x10030860, `StopMotion` 0x10030980, `StopAtLoopEnd` 0x100309d0, `SetMotion` 0x1002bee0/0x1002bf40, `AddBodyPart` 0x1002c3c0, `RemoveBodyPart` 0x1002c4e0, `GetBoneMatrix` 0x1002c5c0/0x1002c6a0, `UpdateLoD` 0x1002c780, `SetLocalTimeScale` 0x100301b0, `UpdateAim` 0x10030230, attachments 0x10030bc0..0x10031110, `SetLookAtTarget` 0x10032250, `SetLookAtConstraint` 0x10032750, `RemoveLookAtConstraint` 0x10032890, `SetRagDollEnabled` 0x100323f0, `StopRagDoll` 0x10034550, `ProcessPhysicsLOD` 0x10031d40, `OnProcess` 0x10032d40, `OnCacheIn/Out` 0x10033730/0x10033790, `GetStatus` 0x10033e10, `OnEnterProcessingRange` 0x10034450, `GameReset` 0x1002b700. `CreatePhysicsController` 0x10031170 and the collision queries are the PhysX side (skip; map to `crates/physics/src/ragdoll.rs`).
- **Tests:** unit: attachment add/remove/relink idempotence, look-at constraint priority, aim overlay weights (B8), time-scale; differential: trace `PlayMotion` names for an NPC routine and a combat sequence.
- **Done when:** NPC and hero attachments and head look-at in `--play` are driven through this API (the weapon-draw and dialog look-at are the visible effects).
- **Size:** about 22 kB, roughly 1000 lines (M).

After E04 continue with E05..E06 (rest of the visual animation property set and factory), then E07 (inventory items and slots), E08..E12 (stacks, inventory transactions/equip, trade and loot lists, stats/magic/recipes), E13 (focus and interaction).

## 6. Unknown (d) and caveats

1144 functions (151424 bytes) could not be attributed: no RTTI class, no single calling class, not a boilerplate thunk. 103 are 200 bytes or larger; none is on a path the plan depends on that we know of. Largest ones by address (candidates to check when a batch misses a callee):

- Game 0x101d68d0 5532B, callers 1, callees 7
- Engine 0x102e5920 1787B, callers 3, callees 5
- Engine 0x102451b0 1565B, callers 1, callees 8
- Engine 0x1012d430 1284B, callers 0, callees 0
- Game 0x101d8420 1215B, callers 1, callees 1
- Game 0x101d7f50 1211B, callers 1, callees 0
- Engine 0x102bcf10 1170B, callers 1, callees 5
- Game 0x100a62f0 1137B, callers 3, callees 2
- Game 0x100a9a10 1058B, callers 2, callees 21
- Engine 0x102b8ee0 979B, callers 1, callees 7
- Engine 0x102e5550 975B, callers 1, callees 2
- Engine 0x102b92c0 974B, callers 1, callees 7

Caveats:

- Class-level rules put whole classes into one category; a few functions inside a *replaced* class (for example camera helpers in `eCCameraBase`) or a *port* class (loader code inside `gCSector`) belong to the other side. Fix by editing `rules.py` and regenerating.
- `FUN_*` attribution is heuristic ([I]). Inside the animation, camera and movement classes the unnamed functions are mostly the large locomotion/mixer routines, which ENGINE_MAP already describes by address.
- *Covered* is not *verified*. Where a batch overlaps a covered subsystem it is because the *port* class also drives that subsystem at run time.
- Open items inherited from `ENGINE_MAP.md`: body of `CorrectCameraPosition` (not recoverable, treat as behavioural spec), movement modes 9 and 0xb, swim vs dive margins, savegame MD5 failure behaviour.
