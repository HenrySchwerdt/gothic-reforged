# Script_Game.dll engine interface: what the Rust host must provide

Source: static analysis of `scripts/Script_Game.dll` (build 1.75.20108.14, sha256 `cdd9c461...`). All 2395 functions (847 registered entry points, 466 internal helpers, 1082 runtime/CRT/RTTI support stubs) were decompiled with Ghidra 12.1.4 into the gitignored `.local/research/script_game/`; this file contains only my own prose and the *public symbol names* of the imported APIs (undecorated MSVC signatures read from the import table). No decompiled bodies are reproduced. Companion docs: `SCRIPT_PORT_PLAN.md` (port batches), `GAME_LOGIC_MAP.md` (script host), `COMBAT_AI_RULES.md`, `GAME_RULES.md`.

Evidence tags: **[V]** read from the import table or from call-site counts in the decompilation; **[I]** inferred semantics (names only). Call-site counts are *static* counts of `call [iat]` instructions plus calls through a register preloaded from the import slot, so they approximate how often an API matters, not runtime frequency.

## 1. What the interface actually is

The script DLL does not link against gameplay logic of its own. It imports **877** distinct symbols (excluding KERNEL32 CRT plumbing) from four DLLs. The decisive finding is that the surface a Rust host must implement is almost entirely `Script.dll`, the thin script API layer, not `Game.dll` or `Engine.dll` directly:

| Provider | Imported symbols | Static call sites | Role |
| --- | ---: | ---: | --- |
| Script.dll | 691 | 23534 | Script API layer: `Entity` handle, `PS*` property-set wrappers, `Template`, `GUI2`, `Music`, `EffectSystem`. **This is the real host trait.** |
| Game.dll | 28 | 2517 | Script processing unit (`Self`/`Other`/`Target` entities, AI instruction callbacks), script admin (call script by name), session, quest manager, SVM (voice sample) manager. |
| Engine.dll | 57 | 620 | Rarely touched directly: engine setup flags, animation motion control, physics ray traces, localised strings, scene lookups. |
| SharedBase.dll | 101 | 9703 | Value types and utilities (`bCString`, `bCUnicodeString`, `bCVector`, `bCQuaternion`, `bCBox`, allocator). Implement natively in Rust; no host state. |

Of the Script.dll symbols, **212 property accessors** (`PS<Class>::Property<Name>` getters/setters, section 5) are plain typed field reads/writes on the entity property sets; the rest are method calls.

## 2. Calling model the host must reproduce [V]

- **Entry point shape.** Registered script functions and states take `(unit, self_entity_ptr, other_entity_ptr, arg)`; states take `(unit, ...)` only. The first thing most functions do is ask the processing unit for its Self / Other / Target entity (`gCScriptProcessingUnit::GetSelfEntity` is called 792 times, `GetTargetEntity` 430, `GetOtherEntity` 362). The Rust host therefore needs a per-invocation *script context* with three entity slots; do not model them as globals. A `None` sentinel entity is used for "no entity".
- **Script-to-script calls** go through the script admin by *name string*: `gCScriptAdmin::RunScriptFunction(name, state_stack, unit)` (615 sites in 277 functions) and `CallScriptFromScript(name, a, b, int)` (128 sites). The host must keep a name -> function registry and a per-NPC *state stack* (`gScriptRunTimeSingleState`), because AI states (`ZS_*`) push/pop and loop via the state machine, not via ordinary Rust calls. The registry in `.local/research/script-registry.json` already lists the 874 names.
- **AI instruction queue.** Long-running actions are not performed by the script; it enqueues instructions whose handlers live in the host as static callbacks: `sAIPlayAniInstr` (56 sites), `sAIWaitInstr` (45), `sAICombatMoveInstr` (34), `sAIGotoInstr` (13), `sAIPlayAimInstr` (10), `sAIOutputInstr` (2), `sAIHUDWaitInstr`, `sAIPrepareAimInstr`. Each returns a boolean "finished" and is polled by the processing unit. This is the contract between a state function (re-evaluated each tick) and the animation/movement layer: the Rust `crates/sim` scheduler needs the same "instruction in flight" abstraction. [V names, I semantics of the bool]
- **Entity.** `Entity` is a *by-value* 0xa8-byte wrapper around an engine entity proxy; helpers copy it freely. In Rust model it as a cheap `Copy` handle (generational id) with the methods in section 4. Global entity tables used by the script: `Entity::ms_Player`, `ms_arrEntities`, `ms_arrNPCs`, `ms_u32LastFrame` (data imports). The host must expose the player, all entities and all NPCs as iterable sets and a frame counter.
- **Engine setup flags.** `eCApplication::GetEngineSetup` is read 69 times; the shipped values are documented in `COMBAT_AI_RULES.md` section 0.2 (ExtendedContent, AIMode, MinHitDuration, XPModifier, QuickLoot).
- **Localised strings** (`eCLocString` ctor/dtor/operator, 97 + 97 + 77 sites): every user-visible or dialog string is looked up through a string id. The host needs `string_id -> text`.

## 3. Suggested Rust trait split

Mirrors the call-density below; all traits are implemented by `crates/sim` over its own ECS, and the script ports depend only on the traits.

| Trait | Covers | Approx. size |
| --- | --- | ---: |
| `ScriptCtx` | `gCScriptProcessingUnit`, `gCScriptAdmin`, AI instruction queue | 13 |
| `EntityApi` | `Entity` (+ `eCEntity`, `Template`, world/scene lookups, spawning) | 135 |
| `NpcApi` | `PSNpc`, `PSRoutine`, `PSNavigation`, `PSCharacterMovement`, `PSCharacterControl`, `PSFocus`, `PSAnimation`, `PSAnchor`, `PSParty` | 98 |
| `CombatApi` | `PSDamage`, `PSDamageReceiver`, `PSTouchDamage`, `PSProjectile`, `PSMagic`, `PSEffect`, `EffectSystem` | 17 |
| `InventoryApi` | `PSInventory`, `PSItem`, `PSRecipe`, `PSLock`, `PSInteraction`, `PSDoor` | 88 |
| `QuestDialogApi` | `PSQuestManager`, `PSInfoManager`, `PSDialog`, `PSInfoScriptCommand`, `PSPlayerMemory`, `PSEnclave`, `gCQuestManager_PS`, `gCSVMManager` | 103 |
| `UiAudioApi` | `GUI2`, `Music`, `PSCamera`, `PSClock`, `eCApplication` (video, clock) | 50 |
| `PhysicsAnimApi` | `eCPhysicsScene` ray traces, `eCVisualAnimation_PS`, `eCWrapper_emfx2Actor`, `eCAnimationAdmin`, `PSCollisionShape`, `PSRigidBody` | 30 |
| value types | SharedBase `bC*` strings/vectors/etc. (native Rust types, no host state) | 101 |

Notes on the biggest wrappers. `Entity` (110 methods, 16k sites) is the hub: attach-to-engine-entity, position/pose, distance/direction, animation names, collision group, random numbers, player/world lookups. `PSInventory` (63) is the item/trade/equip surface. `PSPlayerMemory` (62 methods + 15 properties) is *not* a memory in the AI sense: it is the player-side store of string-keyed **game-event flags** (`SetGameEvent` / `ClearGameEvent` / `IsSetGameEvent`, 39 sites), tutorial flags, base-value (attribute) additions, hit-point / protection / attribute reads, and the political fame/crime counters, so the Rust world-state should expose it as a typed key-value store plus a few stat getters rather than 77 bespoke methods. Counts in the trait table exclude the typed property accessors (section 5).

## 4. API catalogue

Rows are `method(args) -> return` in Rust-like notation inferred from the undecorated MSVC signature; `sites` = static call sites, `fns` = number of script functions that reference it. Enum and class names are the original engine names; `BString` = `bCString`. `&self` is the receiver; `ctor/dtor/operator` rows are shown compactly.

### Script.dll :: Entity (110 symbols, 16387 sites)

| API | Signature | sites | fns |
| --- | --- | ---: | ---: |
| `AttachTo` | `fn AttachTo(&self, a0: *eCEntity) -> bool` | 724 | 362 |
| `GetPlayer` | `fn GetPlayer() -> Entity` | 358 | 241 |
| `GetName` | `fn GetName(&self) -> BString` | 272 | 92 |
| `GetRandomNumber` | `fn GetRandomNumber(a0: i32) -> i32` | 155 | 119 |
| `GetDistanceTo` | `fn GetDistanceTo(&self, a0: &Entity) -> f32` | 128 | 75 |
| `GetWorldEntity` | `fn GetWorldEntity() -> Entity` | 67 | 31 |
| `SetAlignmentTarget` | `fn SetAlignmentTarget(&self, a0: &Entity)` | 50 | 49 |
| `GetAni` | `fn GetAni(&self, a0: gEAction, a1: gEPhase) -> BString` | 46 | 33 |
| `SetCollisionGroup` | `fn SetCollisionGroup(&self, a0: eECollisionGroup)` | 42 | 29 |
| `GetPose` | `fn GetPose(&self) -> bCMatrix` | 39 | 21 |
| `GetPlayingTime` | `fn GetPlayingTime() -> f32` | 33 | 20 |
| `GetEntity` | `fn GetEntity(a0: BString) -> Entity` | 29 | 8 |
| `GetNPCs` | `fn GetNPCs() -> bTObjArray<class Entity>` | 27 | 27 |
| `GetPosition` | `fn GetPosition(&self) -> bCVector` | 26 | 6 |
| `GetZone` | `fn GetZone(&self) -> Entity` | 23 | 15 |
| `GetCamera` | `fn GetCamera() -> PSCamera` | 23 | 21 |
| `MoveTo` | `fn MoveTo(&self, a0: &bCMatrix)` | 21 | 13 |
| `CanEvade` | `fn CanEvade(&self, a0: &Entity, a1: gEDirection) -> bool` | 18 | 10 |
| `GetVelocity` | `fn GetVelocity(&self) -> bCVector` | 16 | 14 |
| `GetAni` | `fn GetAni(&self, a0: gEAction, a1: gEUseType, a2: gEUseType, a3: gEPhase) -> BString` | 15 | 10 |
| `SetLookAtTarget` | `fn SetLookAtTarget(&self, a0: &Entity)` | 14 | 13 |
| `Decay` | `fn Decay(&self)` | 14 | 12 |
| `SetMovementMode` | `fn SetMovementMode(&self, a0: gECharMovementMode)` | 13 | 12 |
| `GetCurrentDifficulty` | `fn GetCurrentDifficulty() -> EDifficulty` | 13 | 8 |
| `IsPlayer` | `fn IsPlayer(&self) -> bool` | 13 | 9 |
| `IsFreeLOS` | `fn IsFreeLOS(&self, a0: &Entity, a1: bool) -> bool` | 13 | 10 |
| `GetFocusName` | `fn GetFocusName(&self) -> BWString` | 12 | 12 |
| `EnableCollisionWith` | `fn EnableCollisionWith(&self, a0: &Entity, a1: bool)` | 11 | 7 |
| `IsInFOV` | `fn IsInFOV(&self, a0: &Entity) -> bool` | 11 | 11 |
| `StartPlayAni` | `fn StartPlayAni(&self, a0: BString, a1: i32, a2: bool, a3: f32, a4: bool)` | 10 | 9 |
| `SortEntityListByDistanceTo` | `fn SortEntityListByDistanceTo(a0: &bTObjArray<class Entity>, a1: &Entity)` | 9 | 8 |
| `GetAIZoneOwner` | `fn GetAIZoneOwner(&self) -> Entity` | 9 | 7 |
| `GetTemplate` | `fn GetTemplate(&self) -> Template` | 9 | 9 |
| `IsSwimming` | `fn IsSwimming(&self) -> bool` | 8 | 8 |
| `IsDiving` | `fn IsDiving(&self) -> bool` | 8 | 8 |
| `EnableAutoAiming` | `fn EnableAutoAiming(&self, a0: bool, a1: &Entity, a2: &BString)` | 8 | 7 |
| `GetOverlayAni` | `fn GetOverlayAni(&self, a0: gEAction, a1: gEUseType, a2: gEUseType, a3: gEPhase) -> BString` | 8 | 4 |
| `Spawn` | `fn Spawn(a0: Template, a1: &bCMatrix) -> Entity` | 8 | 8 |
| `HasSVMBlock` | `fn HasSVMBlock(&self, a0: &BString) -> bool` | 7 | 7 |
| `GetEntities` | `fn GetEntities() -> bTObjArray<class Entity>` | 7 | 7 |
| `GetAIZoneSecurityLevel` | `fn GetAIZoneSecurityLevel(&self) -> gESecurityLevel` | 7 | 6 |
| `SetTimeScale` | `fn SetTimeScale(&self, a0: f32)` | 7 | 7 |
| `Kill` | `fn Kill(&self)` | 7 | 6 |
| `GetSquareDistanceTo` | `fn GetSquareDistanceTo(&self, a0: &Entity) -> f32` | 7 | 7 |
| `IsCheatGodEnabled` | `fn IsCheatGodEnabled() -> bool` | 6 | 6 |
| `GetMaterialName` | `fn GetMaterialName(a0: eEShapeMaterial) -> BString` | 6 | 1 |
| `GetAIZoneType` | `fn GetAIZoneType(&self) -> gEZoneType` | 5 | 5 |
| `GetAniEx` | `fn GetAniEx(&self, a0: gEAction, a1: gEPhase, a2: &BString, a3: bool) -> BString` | 5 | 3 |
| `FindSpawnPose` | `fn FindSpawnPose(&self, a0: &bCMatrix, a1: &Entity, a2: bool, a3: u16) -> bool` | 5 | 5 |
| `IsDead` | `fn IsDead(&self) -> bool` | 5 | 5 |
| `StartGoto` | `fn StartGoto(&self, a0: &Entity, a1: gEWalkMode) -> bool` | 5 | 4 |
| `GetWorldTime` | `fn GetWorldTime() -> f32` | 5 | 3 |
| `SetCurrentAniDirection` | `fn SetCurrentAniDirection(&self, a0: gEDirection)` | 4 | 4 |
| `GetEntitiesByDistance` | `fn GetEntitiesByDistance(&self) -> bTObjArray<class Entity>` | 4 | 4 |
| `GetTimeStampInSeconds` | `fn GetTimeStampInSeconds() -> u32` | 4 | 4 |
| `GetCollisionGroup` | `fn GetCollisionGroup(&self) -> eECollisionGroup` | 4 | 2 |
| `IsSprinting` | `fn IsSprinting(&self) -> bool` | 4 | 4 |
| `GetDirectionTo` | `fn GetDirectionTo(&self, a0: &Entity) -> bCVector` | 3 | 3 |
| `GetOverlayAni` | `fn GetOverlayAni(&self, a0: gEAction, a1: gEPhase) -> BString` | 3 | 3 |
| `ResetInteractAnimation` | `fn ResetInteractAnimation(&self)` | 3 | 2 |
| `HasImportantInfo` | `fn HasImportantInfo(&self, a0: &Entity) -> bool` | 3 | 1 |
| `EnableCollision` | `fn EnableCollision(&self, a0: bool)` | 3 | 3 |
| `StartTransform` | `fn StartTransform(&self, a0: &Entity, a1: f32, a2: bool)` | 3 | 3 |
| `StartFacialAni` | `fn StartFacialAni(&self, a0: BString)` | 2 | 2 |
| `IsStanding` | `fn IsStanding(&self) -> bool` | 2 | 2 |
| `RevertPlayingAni` | `fn RevertPlayingAni(&self, a0: f32)` | 2 | 2 |
| `HasInfo` | `fn HasInfo(&self, a0: &Entity) -> bool` | 2 | 1 |
| `GetMotionDelta` | `fn GetMotionDelta(&self) -> &bCVector` | 2 | 1 |
| `GetCurrentAniPhase` | `fn GetCurrentAniPhase(&self) -> gEPhase` | 2 | 1 |
| `IsFreeLOSKneeHeight` | `fn IsFreeLOSKneeHeight(&self, a0: &Entity) -> bool` | 2 | 2 |
| `Teleport` | `fn Teleport(&self, a0: &BString) -> bool` | 2 | 1 |
| `EndTransform` | `fn EndTransform(&self)` | 2 | 2 |
| `IsCheatInvisibleEnabled` | `fn IsCheatInvisibleEnabled() -> bool` | 1 | 1 |
| `GetIndoorOwnerZone` | `fn GetIndoorOwnerZone(&self, a0: f32) -> Entity` | 1 | 1 |
| `StartSaySVM` | `fn StartSaySVM(&self, a0: &Entity, a1: BString, a2: bool)` | 1 | 1 |
| `IsAlternateCameraEnabled` | `fn IsAlternateCameraEnabled() -> bool` | 1 | 1 |
| `StopPlayOverlayAni` | `fn StopPlayOverlayAni(&self)` | 1 | 1 |
| `GetBoundingRadius` | `fn GetBoundingRadius(&self) -> f32` | 1 | 1 |
| `StartPlayOverlayAni` | `fn StartPlayOverlayAni(&self, a0: BString, a1: i32, a2: bool, a3: f32)` | 1 | 1 |
| `HasCommentInfo` | `fn HasCommentInfo(&self, a0: &Entity) -> bool` | 1 | 1 |
| `StartComment` | `fn StartComment(&self, a0: &Entity) -> bool` | 1 | 1 |
| `IsFalling` | `fn IsFalling(&self) -> bool` | 1 | 1 |
| `StartPlayAniEx` | `fn StartPlayAniEx(&self, a0: &BString, a1: i32, a2: bool, a3: bool, a4: f32, a5: bool)` | 1 | 1 |
| `StopFacialAni` | `fn StopFacialAni(&self)` | 1 | 1 |
| `GetAlignmentTarget` | `fn GetAlignmentTarget(&self) -> Entity` | 1 | 1 |
| `CanGoto` | `fn CanGoto(&self, a0: &Entity) -> bool` | 1 | 1 |
| `SetAlpha` | `fn SetAlpha(&self, a0: f32)` | 1 | 1 |
| `DoDamage` | `fn DoDamage(&self, a0: &Entity, a1: u32, a2: gEDamageType)` | 1 | 1 |
| `IsBraking` | `fn IsBraking(&self) -> bool` | 1 | 1 |
| `SetPlayingTime` | `fn SetPlayingTime(a0: f32)` | 1 | 1 |
| `GetAziElev` | `fn GetAziElev(&self, a0: &Entity, a1: &f32, a2: &f32)` | 1 | 1 |
| `StopPlayAni` | `fn StopPlayAni(&self)` | 1 | 1 |
| `GetInstance` | `fn GetInstance(&self) -> *eCEntity` | 1 | 1 |
| `GetRandomNavigationPoint` | `fn GetRandomNavigationPoint(&self, a0: bCVector, a1: f32) -> bCVector` | 1 | 1 |
| `GetNextPatrolPoint` | `fn GetNextPatrolPoint(a0: &Entity) -> Entity` | 1 | 1 |
| `GetRoamingPointsByName` | `fn GetRoamingPointsByName(a0: BString) -> bTValArray<class eCEntity *>` | 1 | 1 |
| `IsNPC` | `fn IsNPC(&self) -> bool` | 1 | 1 |
| `GetEnclave` | `fn GetEnclave(&self) -> Entity` | 1 | 1 |
| `StartSay` | `fn StartSay(&self, a0: &Entity, a1: BString, a2: bool)` | 1 | 1 |
| `MoveTo` | `fn MoveTo(&self, a0: &Entity)` | 1 | 1 |
| `GetNearestFleePoint` | `fn GetNearestFleePoint(&self, a0: &Entity, a1: f32) -> Entity` | 1 | 1 |
| `~Entity` | `fn ~Entity(&self)` | 6768 | 999 |
| `Entity` | `fn Entity(&self, a0: &Entity)` | 2379 | 604 |
| `Entity` | `fn Entity(&self)` | 1414 | 650 |
| `operator=` | `fn operator=(&self, a0: &Entity) -> &Entity` | 1292 | 612 |
| `Entity` | `fn Entity(&self, a0: *eCEntity)` | 896 | 448 |
| `operator==` | `fn operator==(&self, a0: &Entity) -> bool` | 480 | 248 |
| `operator!=` | `fn operator!=(&self, a0: &Entity) -> bool` | 405 | 165 |
| `operator class eCEntity *` | `fn operator class eCEntity *(&self)` | 280 | 135 |
| `Entity` | `fn Entity(&self, a0: &bCPropertyID)` | 1 | 1 |

### Script.dll :: PSRoutine (15 symbols, 1157 sites)

| API | Signature | sites | fns |
| --- | --- | ---: | ---: |
| `SetState` | `fn SetState(&self, a0: BString)` | 293 | 257 |
| `SetTask` | `fn SetTask(&self, a0: BString)` | 182 | 139 |
| `FullStop` | `fn FullStop(&self)` | 160 | 82 |
| `ContinueRoutine` | `fn ContinueRoutine(&self)` | 139 | 117 |
| `SetTaskCallback` | `fn SetTaskCallback(&self, a0: BString)` | 79 | 48 |
| `GetStateTime` | `fn GetStateTime(&self) -> f32` | 72 | 60 |
| `StopAIGoto` | `fn StopAIGoto(&self)` | 60 | 19 |
| `GetCurrentTask` | `fn GetCurrentTask(&self) -> BString` | 43 | 28 |
| `SetLocalCallback` | `fn SetLocalCallback(&self, a0: BString)` | 34 | 32 |
| `SetStateTime` | `fn SetStateTime(&self, a0: f32)` | 29 | 27 |
| `GetCurrentState` | `fn GetCurrentState(&self) -> BString` | 27 | 14 |
| `GetTaskTime` | `fn GetTaskTime(&self) -> f32` | 21 | 15 |
| `SetTaskTime` | `fn SetTaskTime(&self, a0: f32)` | 9 | 9 |
| `GetLastTask` | `fn GetLastTask(&self) -> BString` | 8 | 6 |
| `StopAIPlayAnimation` | `fn StopAIPlayAnimation(&self)` | 1 | 1 |

### Script.dll :: PSInventory (63 symbols, 1034 sites)

| API | Signature | sites | fns |
| --- | --- | ---: | ---: |
| `FindStackIndex` | `fn FindStackIndex(&self, a0: gESlot) -> i32` | 165 | 67 |
| `GetItemFromSlot` | `fn GetItemFromSlot(&self, a0: gESlot) -> Entity` | 125 | 61 |
| `AssureItemsEx` | `fn AssureItemsEx(&self, a0: &Template, a1: u32, a2: i32, a3: i32, a4: bool) -> i32` | 123 | 1 |
| `GetUseType` | `fn GetUseType(&self, a0: i32) -> gEUseType` | 92 | 40 |
| `IsSkillActive` | `fn IsSkillActive(&self, a0: &Template) -> bool` | 86 | 39 |
| `AssureItems` | `fn AssureItems(&self, a0: &Template, a1: u32, a2: i32) -> i32` | 57 | 26 |
| `GetTemplateItem` | `fn GetTemplateItem(&self, a0: i32) -> Entity` | 44 | 33 |
| `UnEquipStack` | `fn UnEquipStack(&self, a0: i32) -> bool` | 31 | 11 |
| `GetAmount` | `fn GetAmount(&self, a0: i32) -> i32` | 28 | 18 |
| `EquipStackToSlot` | `fn EquipStackToSlot(&self, a0: i32, a1: gESlot) -> bool` | 26 | 8 |
| `GetStackCount` | `fn GetStackCount(&self) -> i32` | 22 | 11 |
| `FindStackIndex` | `fn FindStackIndex(&self, a0: &Template, a1: i32, a2: bool) -> i32` | 19 | 8 |
| `GetCategory` | `fn GetCategory(&self, a0: i32) -> gEItemCategory` | 19 | 6 |
| `HoldStacks` | `fn HoldStacks(&self, a0: i32, a1: i32) -> bool` | 17 | 17 |
| `GetSlotFromStack` | `fn GetSlotFromStack(&self, a0: i32) -> gESlot` | 14 | 6 |
| `DeleteItems` | `fn DeleteItems(&self, a0: i32, a1: i32) -> bool` | 13 | 10 |
| `SplitStack` | `fn SplitStack(&self, a0: i32, a1: i32) -> i32` | 12 | 6 |
| `HasItems` | `fn HasItems(&self, a0: &Template, a1: i32) -> bool` | 11 | 6 |
| `PrintMissingRequirements` | `fn PrintMissingRequirements(&self, a0: i32, a1: bool) -> bool` | 11 | 4 |
| `EquipStack` | `fn EquipStack(&self, a0: i32) -> bool` | 11 | 4 |
| `MergeStacks` | `fn MergeStacks(&self)` | 9 | 7 |
| `CreateItems` | `fn CreateItems(&self, a0: &Template, a1: u32, a2: i32) -> i32` | 7 | 4 |
| `FindStackIndexByQuickSlot` | `fn FindStackIndexByQuickSlot(&self, a0: i32) -> i32` | 6 | 3 |
| `GeneratePlunderInvFromTreasureSets` | `fn GeneratePlunderInvFromTreasureSets(&self) -> bool` | 5 | 5 |
| `GetTemplate` | `fn GetTemplate(&self, a0: i32) -> Template` | 5 | 5 |
| `IsStackMerchandise` | `fn IsStackMerchandise(&self, a0: i32) -> bool` | 5 | 2 |
| `DropItemsFromSlot` | `fn DropItemsFromSlot(&self, a0: gESlot, a1: i32) -> Entity` | 4 | 3 |
| `GetQuality` | `fn GetQuality(&self, a0: i32) -> u32` | 4 | 4 |
| `HasLearnedSpell` | `fn HasLearnedSpell(&self, a0: &Template) -> bool` | 4 | 3 |
| `GetTreasureSet1` | `fn GetTreasureSet1(&self) -> Entity` | 3 | 3 |
| `GetTreasureSet2` | `fn GetTreasureSet2(&self) -> Entity` | 3 | 3 |
| `GetTreasureSet3` | `fn GetTreasureSet3(&self) -> Entity` | 3 | 3 |
| `GetTreasureSet4` | `fn GetTreasureSet4(&self) -> Entity` | 3 | 3 |
| `GetTreasureSet5` | `fn GetTreasureSet5(&self) -> Entity` | 3 | 3 |
| `HoldRightStack` | `fn HoldRightStack(&self, a0: i32) -> bool` | 3 | 3 |
| `ArrangeStacks` | `fn ArrangeStacks(&self)` | 3 | 3 |
| `DeleteStack` | `fn DeleteStack(&self, a0: i32) -> bool` | 2 | 2 |
| `TransferAmountTo` | `fn TransferAmountTo(&self, a0: i32, a1: i32, a2: &Entity) -> bool` | 2 | 2 |
| `RefreshTradeInvFromTreasureSets` | `fn RefreshTradeInvFromTreasureSets(&self) -> bool` | 2 | 1 |
| `IsPermanent` | `fn IsPermanent(&self, a0: i32) -> bool` | 2 | 2 |
| `AssignStackToQuickSlot` | `fn AssignStackToQuickSlot(&self, a0: i32, a1: i32) -> bool` | 2 | 2 |
| `TransferAllTo` | `fn TransferAllTo(&self, a0: &Entity) -> bool` | 2 | 2 |
| `IsGenerated` | `fn IsGenerated(&self, a0: i32) -> bool` | 2 | 2 |
| `ApplyQuality` | `fn ApplyQuality(&self, a0: i32, a1: u32, a2: i32) -> i32` | 2 | 1 |
| `IsGeneratedPlunder` | `fn IsGeneratedPlunder(&self) -> bool` | 2 | 2 |
| `GetGeneratedStackCount` | `fn GetGeneratedStackCount(&self) -> i32` | 2 | 2 |
| `PurgeItems` | `fn PurgeItems(&self, a0: &Template, a1: i32)` | 2 | 1 |
| `GenerateTradeInvFromTreasureSets` | `fn GenerateTradeInvFromTreasureSets(&self) -> bool` | 1 | 1 |
| `HoldLeftStack` | `fn HoldLeftStack(&self, a0: i32) -> bool` | 1 | 1 |
| `DropItemFromSlot` | `fn DropItemFromSlot(&self, a0: gESlot) -> Entity` | 1 | 1 |
| `InsertStack` | `fn InsertStack(&self, a0: &Entity) -> i32` | 1 | 1 |
| `SwapSlots` | `fn SwapSlots(&self, a0: gESlot, a1: gESlot)` | 1 | 1 |
| `ConsumeItems` | `fn ConsumeItems(&self, a0: i32, a1: i32) -> bool` | 1 | 1 |
| `Synthesize` | `fn Synthesize(&self, a0: &Entity, a1: i32) -> i32` | 1 | 1 |
| `StackTradingGold` | `fn StackTradingGold(&self)` | 1 | 1 |
| `UseStack` | `fn UseStack(&self, a0: i32) -> bool` | 1 | 1 |
| `ExecuteItem` | `fn ExecuteItem(&self, a0: i32) -> bool` | 1 | 1 |
| `SortStacks` | `fn SortStacks(&self)` | 1 | 1 |
| `GetName` | `fn GetName(&self, a0: i32) -> BString` | 1 | 1 |
| `SetAmount` | `fn SetAmount(&self, a0: i32, a1: u32) -> bool` | 1 | 1 |
| `SetGeneratedPlunder` | `fn SetGeneratedPlunder(&self, a0: bool)` | 1 | 1 |
| `TeachSkill` | `fn TeachSkill(&self, a0: &Template) -> bool` | 1 | 1 |
| `GetLargestStackForTransfer` | `fn GetLargestStackForTransfer(&self, a0: &Template, a1: i32) -> i32` | 1 | 1 |

### Script.dll :: Template (10 symbols, 865 sites)

| API | Signature | sites | fns |
| --- | --- | ---: | ---: |
| `IsValid` | `fn IsValid(&self) -> bool` | 4 | 4 |
| `Template` | `fn Template(&self, a0: *i8)` | 419 | 121 |
| `~Template` | `fn ~Template(&self)` | 347 | 94 |
| `Template` | `fn Template(&self)` | 70 | 35 |
| `Template` | `fn Template(&self, a0: &Template)` | 11 | 6 |
| `operator=` | `fn operator=(&self, a0: &Template) -> &Template` | 8 | 5 |
| `operator class bCPropertyID &` | `fn operator class bCPropertyID &(&self)` | 2 | 2 |
| `operator=` | `fn operator=(&self, a0: *i8) -> &Template` | 2 | 2 |
| `operator==` | `fn operator==(&self, a0: &bCPropertyID) -> bool` | 1 | 1 |
| `Template` | `fn Template(&self, a0: &bCPropertyID)` | 1 | 1 |

### Script.dll :: PSNpc (34 symbols, 539 sites)

| API | Signature | sites | fns |
| --- | --- | ---: | ---: |
| `GetCurrentTarget` | `fn GetCurrentTarget(&self) -> Entity` | 152 | 110 |
| `SetCurrentTarget` | `fn SetCurrentTarget(&self, a0: &Entity)` | 68 | 48 |
| `GetCurrentAttacker` | `fn GetCurrentAttacker(&self) -> Entity` | 46 | 33 |
| `IsTransformed` | `fn IsTransformed(&self) -> bool` | 36 | 31 |
| `HasStatusEffects` | `fn HasStatusEffects(&self, a0: i32) -> bool` | 31 | 18 |
| `GetEnclave` | `fn GetEnclave(&self) -> Entity` | 30 | 22 |
| `EnableStatusEffects` | `fn EnableStatusEffects(&self, a0: i32, a1: bool)` | 25 | 16 |
| `GetAlternativeTarget` | `fn GetAlternativeTarget(&self) -> Entity` | 19 | 14 |
| `ForceNextPose` | `fn ForceNextPose(&self, a0: gEPose) -> bool` | 14 | 14 |
| `SetAlternativeTarget` | `fn SetAlternativeTarget(&self, a0: &Entity)` | 14 | 13 |
| `GetPrimaryPoseExt` | `fn GetPrimaryPoseExt(&self, a0: gEAction, a1: gEPhase) -> gEPose` | 12 | 10 |
| `SetLastHitTimestamp` | `fn SetLastHitTimestamp(&self, a0: u32)` | 10 | 4 |
| `GetLastHitTimestamp` | `fn GetLastHitTimestamp(&self) -> u32` | 10 | 4 |
| `SetLastHit` | `fn SetLastHit(&self)` | 9 | 9 |
| `SetCurrentAttacker` | `fn SetCurrentAttacker(&self, a0: &Entity)` | 8 | 8 |
| `CanHit` | `fn CanHit(&self, a0: u32) -> bool` | 6 | 4 |
| `GetPrimaryPose` | `fn GetPrimaryPose(&self) -> gEPose` | 6 | 5 |
| `GetCurrentMovementAni` | `fn GetCurrentMovementAni(&self) -> BString` | 5 | 5 |
| `EnableFrozen` | `fn EnableFrozen(&self, a0: bool)` | 5 | 5 |
| `SetLastAttacker` | `fn SetLastAttacker(&self, a0: &Entity)` | 5 | 5 |
| `SetSpellTarget` | `fn SetSpellTarget(&self, a0: &Entity)` | 5 | 1 |
| `IsFrozen` | `fn IsFrozen(&self) -> bool` | 4 | 4 |
| `IsDiseased` | `fn IsDiseased(&self) -> bool` | 3 | 3 |
| `GetGuardPoint` | `fn GetGuardPoint(&self) -> Entity` | 3 | 2 |
| `IsPoisoned` | `fn IsPoisoned(&self) -> bool` | 2 | 2 |
| `IsBurning` | `fn IsBurning(&self) -> bool` | 2 | 2 |
| `EnableBurning` | `fn EnableBurning(&self, a0: bool)` | 2 | 2 |
| `SetLastCast` | `fn SetLastCast(&self)` | 1 | 1 |
| `GetLastAttacker` | `fn GetLastAttacker(&self) -> Entity` | 1 | 1 |
| `CanCast` | `fn CanCast(&self, a0: u32) -> bool` | 1 | 1 |
| `EnablePoisoned` | `fn EnablePoisoned(&self, a0: bool)` | 1 | 1 |
| `EnableDiseased` | `fn EnableDiseased(&self, a0: bool)` | 1 | 1 |
| `IsInMagicBarrier` | `fn IsInMagicBarrier(&self) -> bool` | 1 | 1 |
| `SetEnclave` | `fn SetEnclave(&self, a0: &Entity) -> bool` | 1 | 1 |

### Script.dll :: PSInteraction (8 symbols, 203 sites)

| API | Signature | sites | fns |
| --- | --- | ---: | ---: |
| `GetUseType` | `fn GetUseType(&self) -> gEUseType` | 77 | 50 |
| `GetSpell` | `fn GetSpell(&self) -> Entity` | 40 | 28 |
| `GetOwner` | `fn GetOwner(&self) -> Entity` | 30 | 19 |
| `GetAnchorPoint` | `fn GetAnchorPoint(&self) -> Entity` | 18 | 10 |
| `GetUser` | `fn GetUser(&self) -> Entity` | 18 | 12 |
| `SetSpell` | `fn SetSpell(&self, a0: &Entity)` | 8 | 4 |
| `SetUser` | `fn SetUser(&self, a0: &Entity) -> bool` | 7 | 6 |
| `SetOwner` | `fn SetOwner(&self, a0: &Entity) -> bool` | 5 | 5 |

### Script.dll :: PSInfoScriptCommand (7 symbols, 174 sites)

| API | Signature | sites | fns |
| --- | --- | ---: | ---: |
| `GetID1` | `fn GetID1(&self) -> BString` | 99 | 1 |
| `GetEntity2` | `fn GetEntity2(&self) -> Entity` | 38 | 1 |
| `GetID2` | `fn GetID2(&self) -> BString` | 18 | 1 |
| `GetEntity1` | `fn GetEntity1(&self) -> Entity` | 7 | 1 |
| `GetTextID` | `fn GetTextID(&self) -> BString` | 5 | 1 |
| `GetText` | `fn GetText(&self) -> BWString` | 4 | 1 |
| `GetCommand` | `fn GetCommand(&self) -> BString` | 3 | 1 |

### Script.dll :: PSPlayerMemory (62 symbols, 159 sites)

| API | Signature | sites | fns |
| --- | --- | ---: | ---: |
| `SetGameEvent` | `fn SetGameEvent(&self, a0: &BString) -> bool` | 27 | 25 |
| `PrintAddValue` | `fn PrintAddValue(&self, a0: &BString, a1: i32)` | 9 | 9 |
| `EnableTutorial` | `fn EnableTutorial(&self, a0: gETutorialFlag, a1: bool)` | 8 | 5 |
| `ClearGameEvent` | `fn ClearGameEvent(&self, a0: &BString) -> bool` | 7 | 5 |
| `AddBaseValue` | `fn AddBaseValue(&self, a0: &BString, a1: i32)` | 6 | 6 |
| `GetHitPointsMax` | `fn GetHitPointsMax(&self) -> i32` | 6 | 5 |
| `GetPoliticalCrimeCount` | `fn GetPoliticalCrimeCount(&self, a0: gEPoliticalAlignment) -> i32` | 6 | 4 |
| `GetPoliticalFame` | `fn GetPoliticalFame(&self, a0: gEPoliticalAlignment) -> i32` | 5 | 5 |
| `IsSetGameEvent` | `fn IsSetGameEvent(&self, a0: &BString) -> bool` | 5 | 4 |
| `IsTutorialEnabled` | `fn IsTutorialEnabled(&self, a0: gETutorialFlag) -> bool` | 5 | 4 |
| `GetHitPoints` | `fn GetHitPoints(&self) -> i32` | 4 | 4 |
| `GetProtectionImpact` | `fn GetProtectionImpact(&self) -> i32` | 3 | 2 |
| `GetProtectionBlades` | `fn GetProtectionBlades(&self) -> i32` | 3 | 2 |
| `GetIntelligence` | `fn GetIntelligence(&self) -> i32` | 3 | 2 |
| `GetStaminaPointsMax` | `fn GetStaminaPointsMax(&self) -> i32` | 3 | 3 |
| `GetPoliticalPlayerCrime` | `fn GetPoliticalPlayerCrime(&self, a0: gEPoliticalAlignment) -> bool` | 2 | 2 |
| `GetProtectionMissile` | `fn GetProtectionMissile(&self) -> i32` | 2 | 2 |
| `GetProtectionFire` | `fn GetProtectionFire(&self) -> i32` | 2 | 2 |
| `GetProtectionIce` | `fn GetProtectionIce(&self) -> i32` | 2 | 2 |
| `GetProtectionLightning` | `fn GetProtectionLightning(&self) -> i32` | 2 | 2 |
| `GetManaPointsMax` | `fn GetManaPointsMax(&self) -> i32` | 2 | 2 |
| `GetStaminaPoints` | `fn GetStaminaPoints(&self) -> i32` | 2 | 2 |
| `SetHitPoints` | `fn SetHitPoints(&self, a0: i32)` | 2 | 2 |
| `SetStaminaPoints` | `fn SetStaminaPoints(&self, a0: i32)` | 2 | 2 |
| `SetPoliticalSuspectComment` | `fn SetPoliticalSuspectComment(&self, a0: gEPoliticalAlignment, a1: i32)` | 2 | 2 |
| `SetPoliticalPlayerCrime` | `fn SetPoliticalPlayerCrime(&self, a0: gEPoliticalAlignment, a1: bool)` | 2 | 2 |
| `SetPoliticalCrimeCount` | `fn SetPoliticalCrimeCount(&self, a0: gEPoliticalAlignment, a1: i32)` | 2 | 2 |
| `SetDexterity` | `fn SetDexterity(&self, a0: i32)` | 1 | 1 |
| `SetProtectionImpact` | `fn SetProtectionImpact(&self, a0: i32)` | 1 | 1 |
| `SetHitPointsMax` | `fn SetHitPointsMax(&self, a0: i32)` | 1 | 1 |
| `SetStaminaPointsMax` | `fn SetStaminaPointsMax(&self, a0: i32)` | 1 | 1 |
| `SetIntelligence` | `fn SetIntelligence(&self, a0: i32)` | 1 | 1 |
| `SetProtectionMissile` | `fn SetProtectionMissile(&self, a0: i32)` | 1 | 1 |
| `SetAlchemy` | `fn SetAlchemy(&self, a0: i32)` | 1 | 1 |
| `SetProtectionFire` | `fn SetProtectionFire(&self, a0: i32)` | 1 | 1 |
| `SetSmithing` | `fn SetSmithing(&self, a0: i32)` | 1 | 1 |
| `SetProtectionIce` | `fn SetProtectionIce(&self, a0: i32)` | 1 | 1 |
| `GetManaPoints` | `fn GetManaPoints(&self) -> i32` | 1 | 1 |
| `SetTheft` | `fn SetTheft(&self, a0: i32)` | 1 | 1 |
| `SetProtectionLightning` | `fn SetProtectionLightning(&self, a0: i32)` | 1 | 1 |
| `AddProtectionBlades` | `fn AddProtectionBlades(&self, a0: i32)` | 1 | 1 |
| `GetManaPointsPercent` | `fn GetManaPointsPercent(&self) -> f32` | 1 | 1 |
| `AddProtectionImpact` | `fn AddProtectionImpact(&self, a0: i32)` | 1 | 1 |
| `SetManaPoints` | `fn SetManaPoints(&self, a0: i32)` | 1 | 1 |
| `GetStrength` | `fn GetStrength(&self) -> i32` | 1 | 1 |
| `AddProtectionMissile` | `fn AddProtectionMissile(&self, a0: i32)` | 1 | 1 |
| `SetManaPointsMax` | `fn SetManaPointsMax(&self, a0: i32)` | 1 | 1 |
| `AddProtectionFire` | `fn AddProtectionFire(&self, a0: i32)` | 1 | 1 |
| `AddProtectionIce` | `fn AddProtectionIce(&self, a0: i32)` | 1 | 1 |
| `AddManaPoints` | `fn AddManaPoints(&self, a0: i32)` | 1 | 1 |
| `GetDexterity` | `fn GetDexterity(&self) -> i32` | 1 | 1 |
| `AddProtectionLightning` | `fn AddProtectionLightning(&self, a0: i32)` | 1 | 1 |
| `AddManaPointsMax` | `fn AddManaPointsMax(&self, a0: i32)` | 1 | 1 |
| `GetAlchemy` | `fn GetAlchemy(&self) -> i32` | 1 | 1 |
| `GetSmithing` | `fn GetSmithing(&self) -> i32` | 1 | 1 |
| `GetHitPointsPercent` | `fn GetHitPointsPercent(&self) -> f32` | 1 | 1 |
| `GetStaminaPointsPercent` | `fn GetStaminaPointsPercent(&self) -> f32` | 1 | 1 |
| `GetTheft` | `fn GetTheft(&self) -> i32` | 1 | 1 |
| `SetStrength` | `fn SetStrength(&self, a0: i32)` | 1 | 1 |
| `SetProtectionBlades` | `fn SetProtectionBlades(&self, a0: i32)` | 1 | 1 |
| `GetPoliticalSuspectComment` | `fn GetPoliticalSuspectComment(&self, a0: gEPoliticalAlignment) -> i32` | 1 | 1 |
| `SetPoliticalFame` | `fn SetPoliticalFame(&self, a0: gEPoliticalAlignment, a1: i32)` | 1 | 1 |

### Script.dll :: PSNavigation (16 symbols, 151 sites)

| API | Signature | sites | fns |
| --- | --- | ---: | ---: |
| `GetCurrentDestinationPoint` | `fn GetCurrentDestinationPoint(&self) -> Entity` | 79 | 65 |
| `SetRoutine` | `fn SetRoutine(&self, a0: &BString) -> bool` | 22 | 4 |
| `IsInProcessingRange` | `fn IsInProcessingRange(&self) -> bool` | 13 | 11 |
| `GetWorkingPoint` | `fn GetWorkingPoint(&self) -> Entity` | 11 | 4 |
| `GetCurrentZone` | `fn GetCurrentZone(&self) -> Entity` | 5 | 4 |
| `SetDCCEnabled` | `fn SetDCCEnabled(&self, a0: bool)` | 4 | 4 |
| `SetSleepingPoint` | `fn SetSleepingPoint(&self, a0: &Entity) -> bool` | 3 | 3 |
| `SetWorkingPoint` | `fn SetWorkingPoint(&self, a0: &Entity) -> bool` | 3 | 3 |
| `SetRelaxingPoint` | `fn SetRelaxingPoint(&self, a0: &Entity) -> bool` | 3 | 3 |
| `GetSleepingPoint` | `fn GetSleepingPoint(&self) -> Entity` | 2 | 2 |
| `GetLastZone` | `fn GetLastZone(&self) -> Entity` | 1 | 1 |
| `GetRelaxingPoint` | `fn GetRelaxingPoint(&self) -> Entity` | 1 | 1 |
| `GetGotoStatus` | `fn GetGotoStatus(&self) -> gEGotoStatus` | 1 | 1 |
| `SetCurrentDestinationPoint` | `fn SetCurrentDestinationPoint(&self, a0: &Entity) -> bool` | 1 | 1 |
| `IsTravelling` | `fn IsTravelling(&self) -> bool` | 1 | 1 |
| `HasAnyDestination` | `fn HasAnyDestination(&self) -> bool` | 1 | 1 |

### Script.dll :: PSParty (4 symbols, 129 sites)

| API | Signature | sites | fns |
| --- | --- | ---: | ---: |
| `GetPartyLeader` | `fn GetPartyLeader(&self) -> Entity` | 80 | 39 |
| `SetPartyLeader` | `fn SetPartyLeader(&self, a0: Entity) -> bool` | 29 | 20 |
| `GetMembers` | `fn GetMembers(&self, a0: bool) -> bTValArray<class eCEntity *>` | 17 | 17 |
| `Add` | `fn Add(&self, a0: Entity) -> bool` | 3 | 3 |

### Script.dll :: GUI2 (31 symbols, 127 sites)

| API | Signature | sites | fns |
| --- | --- | ---: | ---: |
| `PrintGameMessage` | `fn PrintGameMessage(&self, a0: &BWString, a1: gEGameMessageType)` | 62 | 32 |
| `GetActivePage` | `fn GetActivePage(&self) -> gEGamePage` | 14 | 12 |
| `ShowCrossHair` | `fn ShowCrossHair(&self, a0: bool)` | 6 | 6 |
| `IsAnyPageOpen` | `fn IsAnyPageOpen(&self) -> bool` | 6 | 5 |
| `ClosePage` | `fn ClosePage(&self) -> bool` | 5 | 4 |
| `OpenCooking` | `fn OpenCooking(&self, a0: &Entity) -> bool` | 2 | 2 |
| `OpenSmithing` | `fn OpenSmithing(&self, a0: &Entity) -> bool` | 2 | 2 |
| `OpenBless` | `fn OpenBless(&self, a0: &Entity) -> bool` | 2 | 2 |
| `OpenPoison` | `fn OpenPoison(&self, a0: &Entity) -> bool` | 2 | 2 |
| `OpenSharpen` | `fn OpenSharpen(&self, a0: &Entity) -> bool` | 2 | 2 |
| `OpenFry` | `fn OpenFry(&self, a0: &Entity) -> bool` | 2 | 2 |
| `OpenLoot` | `fn OpenLoot(&self, a0: &Entity) -> bool` | 2 | 2 |
| `OpenAlchemy` | `fn OpenAlchemy(&self, a0: &Entity) -> bool` | 2 | 2 |
| `OpenTutorial` | `fn OpenTutorial(&self, a0: &BWString)` | 1 | 1 |
| `StopSay` | `fn StopSay(&self)` | 1 | 1 |
| `StartSay` | `fn StartSay(&self, a0: &Entity, a1: &BWString, a2: bool)` | 1 | 1 |
| `UpdatePlayer` | `fn UpdatePlayer(&self)` | 1 | 1 |
| `OpenDialog` | `fn OpenDialog(&self) -> bool` | 1 | 1 |
| `OpenInventory` | `fn OpenInventory(&self) -> bool` | 1 | 1 |
| `OpenSpells` | `fn OpenSpells(&self) -> bool` | 1 | 1 |
| `ToggleQuickSlots` | `fn ToggleQuickSlots(&self)` | 1 | 1 |
| `OpenSkills` | `fn OpenSkills(&self) -> bool` | 1 | 1 |
| `OpenQuests` | `fn OpenQuests(&self) -> bool` | 1 | 1 |
| `OpenMap` | `fn OpenMap(&self, a0: &Entity) -> bool` | 1 | 1 |
| `OpenBook` | `fn OpenBook(&self, a0: &Entity) -> bool` | 1 | 1 |
| `OpenLetter` | `fn OpenLetter(&self, a0: &Entity) -> bool` | 1 | 1 |
| `OpenTrade` | `fn OpenTrade(&self, a0: &Entity) -> bool` | 1 | 1 |
| `SetCrossHairSize` | `fn SetCrossHairSize(&self, a0: f32)` | 1 | 1 |
| `IsMenuOpen` | `fn IsMenuOpen(&self) -> bool` | 1 | 1 |
| `ShowSubtitle` | `fn ShowSubtitle(&self, a0: &BWString)` | 1 | 1 |
| `ShowPicture` | `fn ShowPicture(&self, a0: &BString)` | 1 | 1 |

### Script.dll :: PSCharacterMovement (18 symbols, 69 sites)

| API | Signature | sites | fns |
| --- | --- | ---: | ---: |
| `StopMovement` | `fn StopMovement(&self)` | 12 | 12 |
| `SetGroundBias` | `fn SetGroundBias(&self, a0: &Entity)` | 10 | 8 |
| `GetMovementMode` | `fn GetMovementMode(&self) -> gECharMovementMode` | 9 | 2 |
| `GetFloorMaterial` | `fn GetFloorMaterial(&self) -> eEShapeMaterial` | 8 | 7 |
| `SetMovementMode` | `fn SetMovementMode(&self, a0: gECharMovementMode)` | 4 | 4 |
| `GetFloorY` | `fn GetFloorY(&self) -> f32` | 4 | 3 |
| `PutToGround` | `fn PutToGround(&self)` | 4 | 1 |
| `IsJumping` | `fn IsJumping(&self) -> bool` | 3 | 2 |
| `SetTreatWaterAsSolid` | `fn SetTreatWaterAsSolid(&self, a0: bool)` | 3 | 3 |
| `GetLastMovementMode` | `fn GetLastMovementMode(&self) -> gECharMovementMode` | 2 | 2 |
| `CanJump` | `fn CanJump(&self) -> bool` | 2 | 2 |
| `GetWaterY` | `fn GetWaterY(&self) -> f32` | 2 | 2 |
| `GetCeilingY` | `fn GetCeilingY(&self) -> f32` | 1 | 1 |
| `IsFalling` | `fn IsFalling(&self) -> bool` | 1 | 1 |
| `IsSwimming` | `fn IsSwimming(&self) -> bool` | 1 | 1 |
| `IsDiving` | `fn IsDiving(&self) -> bool` | 1 | 1 |
| `GetTreatWaterAsSolid` | `fn GetTreatWaterAsSolid(&self) -> bool` | 1 | 1 |
| `GetSlideStartedFrontally` | `fn GetSlideStartedFrontally(&self) -> bool` | 1 | 1 |

### Script.dll :: PSFocus (3 symbols, 63 sites)

| API | Signature | sites | fns |
| --- | --- | ---: | ---: |
| `SetFocusEntity` | `fn SetFocusEntity(&self, a0: &Entity)` | 25 | 22 |
| `GetFocusEntity` | `fn GetFocusEntity(&self) -> Entity` | 21 | 15 |
| `GetFocusEntity` | `fn GetFocusEntity(&self, a0: gECombatMode, a1: gEDirection) -> Entity` | 17 | 17 |

### Script.dll :: PSEnclave (7 symbols, 61 sites)

| API | Signature | sites | fns |
| --- | --- | ---: | ---: |
| `GetEnclave` | `fn GetEnclave(a0: &BString) -> Entity` | 27 | 4 |
| `GetCrimeCount` | `fn GetCrimeCount(&self, a0: gEPlayerCrime) -> i32` | 9 | 6 |
| `SetCrimeCount` | `fn SetCrimeCount(&self, a0: gEPlayerCrime, a1: i32)` | 7 | 5 |
| `SetSuspectComment` | `fn SetSuspectComment(&self, a0: gEPlayerCrime, a1: i32)` | 6 | 4 |
| `GetSuspectComment` | `fn GetSuspectComment(&self, a0: gEPlayerCrime) -> i32` | 5 | 2 |
| `GetMembers` | `fn GetMembers(&self) -> bTValArray<class eCEntity *>` | 4 | 4 |
| `GetDisplayName` | `fn GetDisplayName(&self) -> BWString` | 3 | 2 |

### Script.dll :: PSItem (11 symbols, 58 sites)

| API | Signature | sites | fns |
| --- | --- | ---: | ---: |
| `GetSpell` | `fn GetSpell(&self) -> Entity` | 30 | 22 |
| `GetQuality` | `fn GetQuality(&self) -> u32` | 8 | 6 |
| `GetCategory` | `fn GetCategory(&self) -> gEItemCategory` | 4 | 4 |
| `IsRobe` | `fn IsRobe(&self) -> bool` | 4 | 2 |
| `IsMissionItem` | `fn IsMissionItem(&self) -> bool` | 3 | 2 |
| `IsDropped` | `fn IsDropped(&self) -> bool` | 2 | 2 |
| `GetGoldValue` | `fn GetGoldValue(&self) -> i32` | 2 | 2 |
| `IsFullBody` | `fn IsFullBody(&self) -> bool` | 2 | 1 |
| `IsEquipped` | `fn IsEquipped(&self) -> bool` | 1 | 1 |
| `ExecuteScript` | `fn ExecuteScript(&self) -> bool` | 1 | 1 |
| `ModifyQuality` | `fn ModifyQuality(&self, a0: u32, a1: u32)` | 1 | 1 |

### Script.dll :: EffectSystem (4 symbols, 53 sites)

| API | Signature | sites | fns |
| --- | --- | ---: | ---: |
| `StartEffect` | `fn StartEffect(a0: &BString, a1: &Entity) -> Effect` | 46 | 27 |
| `StartEffect` | `fn StartEffect(a0: &BString, a1: &bCMatrix, a2: &Entity) -> Effect` | 3 | 2 |
| `ExistsEffect` | `fn ExistsEffect(a0: &BString) -> bool` | 3 | 1 |
| `StartEffect` | `fn StartEffect(a0: &BString, a1: &bCMatrix) -> Effect` | 1 | 1 |

### Script.dll :: PSEffect (4 symbols, 52 sites)

| API | Signature | sites | fns |
| --- | --- | ---: | ---: |
| `StopEffect` | `fn StopEffect(&self, a0: bool)` | 26 | 20 |
| `StartEffect` | `fn StartEffect(&self, a0: &BString, a1: bool) -> bool` | 10 | 7 |
| `StartRuntimeEffect` | `fn StartRuntimeEffect(&self, a0: &BString) -> bool` | 8 | 6 |
| `StopRuntimeEffect` | `fn StopRuntimeEffect(&self, a0: &BString, a1: bool) -> bool` | 8 | 7 |

### Script.dll :: PSInfoManager (8 symbols, 45 sites)

| API | Signature | sites | fns |
| --- | --- | ---: | ---: |
| `GetNPC` | `fn GetNPC(&self) -> Entity` | 19 | 7 |
| `OnCommandCompleted` | `fn OnCommandCompleted(&self, a0: bool) -> bool` | 8 | 8 |
| `IsRunning` | `fn IsRunning(&self) -> bool` | 6 | 5 |
| `End` | `fn End(&self) -> bool` | 5 | 5 |
| `Start` | `fn Start(&self, a0: &Entity, a1: &Entity) -> bool` | 2 | 1 |
| `Give` | `fn Give(&self, a0: &Entity, a1: &Entity, a2: i32, a3: u32) -> bool` | 2 | 2 |
| `GetCurrentInfo` | `fn GetCurrentInfo(&self) -> Info` | 2 | 1 |
| `GetCurrentCommand` | `fn GetCurrentCommand(&self) -> PSInfoScriptCommand` | 1 | 1 |

### Script.dll :: World (1 symbols, 35 sites)

| API | Signature | sites | fns |
| --- | --- | ---: | ---: |
| `SetSectorStatus` | `fn SetSectorStatus(&self, a0: &BString, a1: bool) -> bool` | 35 | 4 |

### Script.dll :: PSCamera (5 symbols, 23 sites)

| API | Signature | sites | fns |
| --- | --- | ---: | ---: |
| `IsInFirstPerson` | `fn IsInFirstPerson(&self) -> bool` | 11 | 11 |
| `SetMode` | `fn SetMode(&self, a0: gECameraMode, a1: bool, a2: bool)` | 8 | 8 |
| `GetScreenCenterPickPoint` | `fn GetScreenCenterPickPoint(&self) -> bCVector` | 2 | 2 |
| `EnableTempFirstPerson` | `fn EnableTempFirstPerson(&self, a0: bool)` | 1 | 1 |
| `EnableSpecialBowMode` | `fn EnableSpecialBowMode(&self, a0: bool)` | 1 | 1 |

### Script.dll :: PSQuestManager (13 symbols, 23 sites)

| API | Signature | sites | fns |
| --- | --- | ---: | ---: |
| `IsOpen` | `fn IsOpen(&self, a0: &BString) -> bool` | 4 | 2 |
| `RunQuest` | `fn RunQuest(&self, a0: &BString) -> bool` | 3 | 3 |
| `OnSpellCast` | `fn OnSpellCast(&self, a0: &Entity, a1: &Entity, a2: BString) -> bool` | 2 | 2 |
| `IsSucceeded` | `fn IsSucceeded(&self, a0: &BString) -> bool` | 2 | 2 |
| `SucceedQuest` | `fn SucceedQuest(&self, a0: &BString) -> bool` | 2 | 2 |
| `OnLeave` | `fn OnLeave(&self, a0: &Entity, a1: &BString) -> bool` | 2 | 2 |
| `CloseQuest` | `fn CloseQuest(&self, a0: &BString) -> bool` | 2 | 2 |
| `OnNPCKilled` | `fn OnNPCKilled(&self, a0: &Entity) -> bool` | 1 | 1 |
| `OnNPCDefeated` | `fn OnNPCDefeated(&self, a0: &Entity) -> bool` | 1 | 1 |
| `OnEnclaveStateChanged` | `fn OnEnclaveStateChanged(&self, a0: &Entity) -> bool` | 1 | 1 |
| `OnEnter` | `fn OnEnter(&self, a0: &Entity, a1: &BString) -> bool` | 1 | 1 |
| `IsRunning` | `fn IsRunning(&self, a0: &BString) -> bool` | 1 | 1 |
| `FailQuest` | `fn FailQuest(&self, a0: &BString) -> bool` | 1 | 1 |

### Script.dll :: PSCharacterControl (1 symbols, 18 sites)

| API | Signature | sites | fns |
| --- | --- | ---: | ---: |
| `SetMovementConstraints` | `fn SetMovementConstraints(&self, a0: f32)` | 18 | 18 |

### Script.dll :: (data/other) (8 symbols, 13 sites)

| API | Signature | sites | fns |
| --- | --- | ---: | ---: |
| `None` | data/import | 12 | 4 |
| `?ms_u32LastFrame@Entity@@2KA` | data/import | 1 | 1 |
| `?ms_Player@Entity@@2V1@A` | data/import | 0 | 0 |
| `world` | data/import | 0 | 0 |
| `?ms_arrEntities@Entity@@2V?$bTObjArray@VEntity@@@@A` | data/import | 0 | 0 |
| `?ms_arrNPCs@Entity@@2V?$bTObjArray@VEntity@@@@A` | data/import | 0 | 0 |
| `music` | data/import | 0 | 0 |
| `gui2` | data/import | 0 | 0 |

### Script.dll :: PSCollisionShape (6 symbols, 13 sites)

| API | Signature | sites | fns |
| --- | --- | ---: | ---: |
| `GetPhysicMaterial` | `fn GetPhysicMaterial(&self) -> eEShapeMaterial` | 5 | 3 |
| `GetType` | `fn GetType(&self) -> eEPropertySetType` | 2 | 2 |
| `CreateShape` | `fn CreateShape(&self, a0: eECollisionShapeType, a1: eEShapeGroup, a2: bCVector, a3: bCVector) -> bool` | 2 | 2 |
| `GetNumTouchingShapes` | `fn GetNumTouchingShapes(&self) -> i32` | 2 | 1 |
| `GetTouchingBone` | `fn GetTouchingBone(&self) -> BString` | 1 | 1 |
| `GetTouchingShapeGroup` | `fn GetTouchingShapeGroup(&self, a0: u32) -> eEShapeGroup` | 1 | 1 |

### Script.dll :: PSDamageReceiver (2 symbols, 12 sites)

| API | Signature | sites | fns |
| --- | --- | ---: | ---: |
| `GetLastInflictor` | `fn GetLastInflictor(&self) -> Entity` | 11 | 1 |
| `SetLastInflictor` | `fn SetLastInflictor(&self, a0: &Entity)` | 1 | 1 |

### Script.dll :: PSTouchDamage (2 symbols, 12 sites)

| API | Signature | sites | fns |
| --- | --- | ---: | ---: |
| `ClearTriggeredList` | `fn ClearTriggeredList(&self)` | 11 | 9 |
| `GetInflictor` | `fn GetInflictor(&self) -> Entity` | 1 | 1 |

### Script.dll :: PSAnimation (3 symbols, 10 sites)

| API | Signature | sites | fns |
| --- | --- | ---: | ---: |
| `GetResourceName` | `fn GetResourceName(&self) -> BString` | 4 | 3 |
| `AddForceAtPos` | `fn AddForceAtPos(&self, a0: bCVector, a1: bCVector)` | 4 | 1 |
| `GetSkeletonName` | `fn GetSkeletonName(&self, a0: &BString) -> bool` | 2 | 2 |

### Script.dll :: PSClock (2 symbols, 8 sites)

| API | Signature | sites | fns |
| --- | --- | ---: | ---: |
| `GetHour` | `fn GetHour(&self) -> i32` | 4 | 2 |
| `SetFactor` | `fn SetFactor(&self, a0: f32)` | 4 | 3 |

### Script.dll :: PSMagic (3 symbols, 8 sites)

| API | Signature | sites | fns |
| --- | --- | ---: | ---: |
| `GetSpawn` | `fn GetSpawn(&self) -> Entity` | 6 | 5 |
| `IsAutoReload` | `fn IsAutoReload(&self) -> bool` | 1 | 1 |
| `GetReloadDelay` | `fn GetReloadDelay(&self) -> u32` | 1 | 1 |

### Script.dll :: PSAnchor (1 symbols, 7 sites)

| API | Signature | sites | fns |
| --- | --- | ---: | ---: |
| `GetInteractPoints` | `fn GetInteractPoints(&self, a0: bool) -> bTObjArray<class Entity>` | 7 | 4 |

### Script.dll :: Music (6 symbols, 6 sites)

| API | Signature | sites | fns |
| --- | --- | ---: | ---: |
| `TriggerSituation` | `fn TriggerSituation(&self, a0: &BString) -> bool` | 1 | 1 |
| `TriggerRevolution` | `fn TriggerRevolution(&self) -> bool` | 1 | 1 |
| `TriggerVictory` | `fn TriggerVictory(&self) -> bool` | 1 | 1 |
| `TriggerShowdown` | `fn TriggerShowdown(&self) -> bool` | 1 | 1 |
| `TriggerFight` | `fn TriggerFight(&self) -> bool` | 1 | 1 |
| `TriggerExplore` | `fn TriggerExplore(&self) -> bool` | 1 | 1 |

### Script.dll :: PSProjectile (2 symbols, 6 sites)

| API | Signature | sites | fns |
| --- | --- | ---: | ---: |
| `Shoot` | `fn Shoot(&self)` | 3 | 3 |
| `SetTarget` | `fn SetTarget(&self, a0: &Entity)` | 3 | 2 |

### Script.dll :: PSDoor (3 symbols, 5 sites)

| API | Signature | sites | fns |
| --- | --- | ---: | ---: |
| `IsOpen` | `fn IsOpen(&self) -> bool` | 2 | 2 |
| `Open` | `fn Open(&self, a0: bool)` | 2 | 2 |
| `Close` | `fn Close(&self, a0: bool)` | 1 | 1 |

### Script.dll :: PSLock (1 symbols, 4 sites)

| API | Signature | sites | fns |
| --- | --- | ---: | ---: |
| `GetKey` | `fn GetKey(&self) -> Template` | 4 | 4 |

### Script.dll :: Info (1 symbols, 2 sites)

| API | Signature | sites | fns |
| --- | --- | ---: | ---: |
| `IsComment` | `fn IsComment(&self) -> bool` | 2 | 1 |

### Script.dll :: PSRecipe (2 symbols, 2 sites)

| API | Signature | sites | fns |
| --- | --- | ---: | ---: |
| `GetReqSkill1` | `fn GetReqSkill1(&self) -> Entity` | 1 | 1 |
| `GetResultItem` | `fn GetResultItem(&self) -> Entity` | 1 | 1 |

### Script.dll :: PSMover (1 symbols, 1 sites)

| API | Signature | sites | fns |
| --- | --- | ---: | ---: |
| `Toggle` | `fn Toggle(&self)` | 1 | 1 |

### Script.dll :: PSRigidBody (1 symbols, 1 sites)

| API | Signature | sites | fns |
| --- | --- | ---: | ---: |
| `HoverTo` | `fn HoverTo(&self, a0: &bCVector, a1: f32)` | 1 | 1 |

### Game.dll :: gCScriptProcessingUnit (11 symbols, 1746 sites)

| API | Signature | sites | fns |
| --- | --- | ---: | ---: |
| `GetSelfEntity` | `fn GetSelfEntity(&self) -> *eCEntity` | 792 | 792 |
| `GetTargetEntity` | `fn GetTargetEntity(&self) -> *eCEntity` | 430 | 430 |
| `GetOtherEntity` | `fn GetOtherEntity(&self) -> *eCEntity` | 362 | 362 |
| `sAIPlayAniInstr` | `fn sAIPlayAniInstr(a0: *(), a1: *gCScriptProcessingUnit, a2: bool) -> bool` | 56 | 41 |
| `sAIWaitInstr` | `fn sAIWaitInstr(a0: *(), a1: *gCScriptProcessingUnit, a2: bool) -> bool` | 45 | 42 |
| `sAICombatMoveInstr` | `fn sAICombatMoveInstr(a0: *(), a1: *gCScriptProcessingUnit, a2: bool) -> bool` | 34 | 26 |
| `sAIGotoInstr` | `fn sAIGotoInstr(a0: *(), a1: *gCScriptProcessingUnit, a2: bool) -> bool` | 13 | 13 |
| `sAIPlayAimInstr` | `fn sAIPlayAimInstr(a0: *(), a1: *gCScriptProcessingUnit, a2: bool) -> bool` | 10 | 5 |
| `sAIOutputInstr` | `fn sAIOutputInstr(a0: *(), a1: *gCScriptProcessingUnit, a2: bool) -> bool` | 2 | 2 |
| `sAIPrepareAimInstr` | `fn sAIPrepareAimInstr(a0: *(), a1: *gCScriptProcessingUnit, a2: bool) -> bool` | 1 | 1 |
| `sAIHUDWaitInstr` | `fn sAIHUDWaitInstr(a0: *(), a1: *gCScriptProcessingUnit, a2: bool) -> bool` | 1 | 1 |

### Game.dll :: gCScriptAdmin (2 symbols, 743 sites)

| API | Signature | sites | fns |
| --- | --- | ---: | ---: |
| `RunScriptFunction` | `fn RunScriptFunction(&self, a0: &BString, a1: &bTObjStack<struct gScriptRunTimeSingleState>, a2: *gCScriptProcessingUnit) -> bool` | 615 | 277 |
| `CallScriptFromScript` | `fn CallScriptFromScript(&self, a0: &BString, a1: *(), a2: *(), a3: i32) -> i32` | 128 | 63 |

### Game.dll :: gCSVMManager (4 symbols, 10 sites)

| API | Signature | sites | fns |
| --- | --- | ---: | ---: |
| `GetInstance` | `fn GetInstance() -> &gCSVMManager` | 5 | 4 |
| `FormatSampleName` | `fn FormatSampleName(a0: *gCNPC_PS, a1: &BString, a2: bool) -> BString` | 2 | 2 |
| `FormatSampleName` | `fn FormatSampleName(a0: &BString, a1: &BString, a2: bool) -> BString` | 2 | 2 |
| `GetStringID` | `fn GetStringID(a0: &BString, a1: &BString) -> BString` | 1 | 1 |

### Game.dll :: gCSession (3 symbols, 8 sites)

| API | Signature | sites | fns |
| --- | --- | ---: | ---: |
| `GetInstance` | `fn GetInstance() -> &gCSession` | 4 | 2 |
| `SpawnEntity` | `fn SpawnEntity(&self, a0: &BString, a1: bCVector, a2: bool) -> *eCEntity` | 3 | 2 |
| `GetPlayer` | `fn GetPlayer(&self) -> *eCDynamicEntity` | 1 | 1 |

### Game.dll :: gCCharacterMovement_PS (3 symbols, 5 sites)

| API | Signature | sites | fns |
| --- | --- | ---: | ---: |
| `EnableMovementFromSPU` | `fn EnableMovementFromSPU(&self, a0: bool)` | 3 | 3 |
| `GetDistToGround` | `fn GetDistToGround(&self, a0: &bCVector, a1: bool) -> f32` | 1 | 1 |
| `CanFallDown` | `fn CanFallDown(&self) -> bool` | 1 | 1 |

### Game.dll :: gCQuestManager_PS (2 symbols, 4 sites)

| API | Signature | sites | fns |
| --- | --- | ---: | ---: |
| `GetInstance` | `fn GetInstance() -> *gCQuestManager_PS` | 3 | 3 |
| `GetQuest` | `fn GetQuest(&self, a0: &BString) -> *gCQuest_PS` | 1 | 1 |

### Game.dll :: gCQuest_PS (1 symbols, 1 sites)

| API | Signature | sites | fns |
| --- | --- | ---: | ---: |
| `SetStatus` | `fn SetStatus(&self, a0: &gEQuestStatus) -> bool` | 1 | 1 |

### Game.dll :: (data/other) (2 symbols, 0 sites)

| API | Signature | sites | fns |
| --- | --- | ---: | ---: |
| `?g_strStringIDPoliticalAlignment@@3PAVbCString@@A` | data/import | 0 | 0 |
| `?g_pstrPhaseString@@3QBVbCString@@B` | data/import | 0 | 0 |

### Engine.dll :: eCLocString (4 symbols, 291 sites)

| API | Signature | sites | fns |
| --- | --- | ---: | ---: |
| `GetString` | `fn GetString(&self) -> BWString` | 20 | 12 |
| `~eCLocString` | `fn ~eCLocString(&self)` | 97 | 36 |
| `eCLocString` | `fn eCLocString(&self, a0: &BString)` | 97 | 36 |
| `operator class bCUnicodeString const ` | `fn operator class bCUnicodeString const (&self)` | 77 | 29 |

### Engine.dll :: eCApplication (5 symbols, 155 sites)

| API | Signature | sites | fns |
| --- | --- | ---: | ---: |
| `GetInstance` | `fn GetInstance() -> &eCApplication` | 77 | 48 |
| `GetEngineSetup` | `fn GetEngineSetup(&self) -> &eSSetupEngine` | 69 | 46 |
| `IsInitialised` | `fn IsInitialised() -> bool` | 6 | 6 |
| `PlayVideo` | `fn PlayVideo(&self, a0: &BString)` | 2 | 1 |
| `GetScaledFrameTimeInSeconds` | `fn GetScaledFrameTimeInSeconds(&self) -> f32` | 1 | 1 |

### Engine.dll :: eCVisualAnimation_PS (8 symbols, 56 sites)

| API | Signature | sites | fns |
| --- | --- | ---: | ---: |
| `GetActor` | `fn GetActor(&self) -> *eCWrapper_emfx2Actor` | 20 | 6 |
| `GetMotionDesc` | `fn GetMotionDesc(&self, a0: eCWrapper_emfx2Actor::eEMotionType) -> &eCVisualAnimation_PS::eSMotionDesc` | 9 | 5 |
| `eCVisualAnimation_PS::eSMotionDesc::GetMotionFilename` | `fn GetMotionFilename(&self) -> &BString` | 9 | 5 |
| `SetMotionOwner` | `fn SetMotionOwner(&self, a0: eCWrapper_emfx2Actor::eEMotionType, a1: eCVisualAnimation_PS::eSMotionDesc::eEMotionOwner)` | 6 | 2 |
| `HasMotion` | `fn HasMotion(&self, a0: eCWrapper_emfx2Actor::eEMotionType) -> bool` | 5 | 1 |
| `StopMotion` | `fn StopMotion(&self, a0: eCWrapper_emfx2Actor::eEMotionType, a1: f32)` | 4 | 1 |
| `SetMotion` | `fn SetMotion(&self, a0: eCWrapper_emfx2Actor::eEMotionType, a1: *eCResourceDataEntity)` | 2 | 1 |
| `PlayMotion` | `fn PlayMotion(&self, a0: eCWrapper_emfx2Actor::eEMotionType, a1: *eCWrapper_emfx2Motion::eSMotionDesc)` | 1 | 1 |

### Engine.dll :: eCPhysicsScene (4 symbols, 30 sites)

| API | Signature | sites | fns |
| --- | --- | ---: | ---: |
| `GetInstance` | `fn GetInstance() -> &eCPhysicsScene` | 24 | 2 |
| `TraceRayFirstHit` | `fn TraceRayFirstHit(&self, a0: &bCVector, a1: &bCVector, a2: *eCEntity, a3: u32, a4: *eCEntity, a5: eEPropertySetType, a6: eEShapeGroup) -> bool` | 4 | 1 |
| `TraceRayNearestHit` | `fn TraceRayNearestHit(&self, a0: &bCVector, a1: &bCVector, a2: *eCEntity, a3: u32, a4: *eCEntity, a5: eEPropertySetType, a6: eEShapeGroup) -> bool` | 1 | 1 |
| `GetQueryReport` | `fn GetQueryReport(&self) -> &eCPhysicQueryReport` | 1 | 1 |

### Engine.dll :: eCEntity (8 symbols, 20 sites)

| API | Signature | sites | fns |
| --- | --- | ---: | ---: |
| `GetWorldNodeBoundary` | `fn GetWorldNodeBoundary(&self) -> &bCBox` | 8 | 1 |
| `GetWorldPosition` | `fn GetWorldPosition(&self) -> &bCVector` | 3 | 1 |
| `GetWorldPosition` | `fn GetWorldPosition(&self, a0: &bCVector)` | 3 | 1 |
| `GetLocalRotation` | `fn GetLocalRotation(&self, a0: &bCQuaternion)` | 2 | 1 |
| `IsRenderingEnabled` | `fn IsRenderingEnabled(&self) -> bool` | 1 | 1 |
| `GetPropertySet` | `fn GetPropertySet(&self, a0: eEPropertySetType) -> *eCEntityPropertySet` | 1 | 1 |
| `GetLocalNodeBoundary` | `fn GetLocalNodeBoundary(&self) -> &bCBox` | 1 | 1 |
| `SetLocalRotation` | `fn SetLocalRotation(&self, a0: &bCQuaternion)` | 1 | 1 |

### Engine.dll :: eCWrapper_emfx2Actor (8 symbols, 20 sites)

| API | Signature | sites | fns |
| --- | --- | ---: | ---: |
| `GetMaxTime` | `fn GetMaxTime(&self, a0: eCWrapper_emfx2Actor::eEMotionType) -> f64` | 5 | 5 |
| `GetPlayTime` | `fn GetPlayTime(&self, a0: eCWrapper_emfx2Actor::eEMotionType) -> f64` | 5 | 5 |
| `SetTargetWeight` | `fn SetTargetWeight(&self, a0: eCWrapper_emfx2Actor::eEMotionType, a1: f32)` | 4 | 1 |
| `GetIntroBlending` | `fn GetIntroBlending(&self, a0: eCWrapper_emfx2Actor::eEMotionType) -> f32` | 2 | 1 |
| `SwitchFadeOut` | `fn SwitchFadeOut(&self, a0: eCWrapper_emfx2Actor::eEMotionType, a1: bool, a2: f32)` | 1 | 1 |
| `IsMotionRunning` | `fn IsMotionRunning(&self, a0: eCWrapper_emfx2Actor::eEMotionType) -> bool` | 1 | 1 |
| `SetIntroExtroBlending` | `fn SetIntroExtroBlending(&self, a0: eCWrapper_emfx2Actor::eEMotionType, a1: &BString)` | 1 | 1 |
| `GetExtroBlending` | `fn GetExtroBlending(&self, a0: eCWrapper_emfx2Actor::eEMotionType) -> f32` | 1 | 1 |

### Engine.dll :: eCActionMapper (3 symbols, 12 sites)

| API | Signature | sites | fns |
| --- | --- | ---: | ---: |
| `GetInstance` | `fn GetInstance() -> &eCActionMapper` | 6 | 6 |
| `RegisterScriptEvent` | `fn RegisterScriptEvent(&self, a0: &eSScriptEvent)` | 5 | 5 |
| `ParseScriptEvents` | `fn ParseScriptEvents(&self)` | 1 | 1 |

### Engine.dll :: eCModuleAdmin (2 symbols, 12 sites)

| API | Signature | sites | fns |
| --- | --- | ---: | ---: |
| `GetInstance` | `fn GetInstance() -> &eCModuleAdmin` | 6 | 6 |
| `FindModule` | `fn FindModule(&self, a0: &BString) -> *eCEngineComponentBase` | 6 | 6 |

### Engine.dll :: eCSceneAdmin (2 symbols, 7 sites)

| API | Signature | sites | fns |
| --- | --- | ---: | ---: |
| `GetTemplateEntityByName` | `fn GetTemplateEntityByName(&self, a0: &BString) -> *eCTemplateEntity` | 4 | 2 |
| `GetEntityByName` | `fn GetEntityByName(&self, a0: &BString) -> *eCEntity` | 3 | 1 |

### Engine.dll :: eCConsole (1 symbols, 4 sites)

| API | Signature | sites | fns |
| --- | --- | ---: | ---: |
| `AccessConsole` | `fn AccessConsole() -> &eCConsole` | 4 | 3 |

### Engine.dll :: eCAnimationAdmin (3 symbols, 3 sites)

| API | Signature | sites | fns |
| --- | --- | ---: | ---: |
| `IsAnimationMissed` | `fn IsAnimationMissed(&self, a0: &BString) -> bool` | 1 | 1 |
| `AddMissingAnimation` | `fn AddMissingAnimation(&self, a0: &BString)` | 1 | 1 |
| `QueryMotionDataEntity` | `fn QueryMotionDataEntity(&self, a0: &BString, a1: eEResourceCacheBehavior) -> *eCResourceDataEntity` | 1 | 1 |

### Engine.dll :: eCEntityProxy (2 symbols, 3 sites)

| API | Signature | sites | fns |
| --- | --- | ---: | ---: |
| `~eCEntityProxy` | `fn ~eCEntityProxy(&self)` | 2 | 1 |
| `eCEntityProxy` | `fn eCEntityProxy(&self)` | 1 | 1 |

### Engine.dll :: eCArchiveFile (2 symbols, 2 sites)

| API | Signature | sites | fns |
| --- | --- | ---: | ---: |
| `IsValid` | `fn IsValid(&self) -> bool` | 1 | 1 |
| `~eCArchiveFile` | `fn ~eCArchiveFile(&self)` | 1 | 1 |

### Engine.dll :: eCArchiveManager (2 symbols, 2 sites)

| API | Signature | sites | fns |
| --- | --- | ---: | ---: |
| `GetFile` | `fn GetFile(&self, a0: &BString) -> eCArchiveFile` | 1 | 1 |
| `GetInstance` | `fn GetInstance() -> &eCArchiveManager` | 1 | 1 |

### Engine.dll :: eCMessageHandler (1 symbols, 1 sites)

| API | Signature | sites | fns |
| --- | --- | ---: | ---: |
| `Terminate` | `fn Terminate(&self, a0: bool)` | 1 | 1 |

### Engine.dll :: eCNode (1 symbols, 1 sites)

| API | Signature | sites | fns |
| --- | --- | ---: | ---: |
| `GetID` | `fn GetID(&self) -> &bCPropertyID` | 1 | 1 |

### Engine.dll :: eCResourceDataEntity (1 symbols, 1 sites)

| API | Signature | sites | fns |
| --- | --- | ---: | ---: |
| `GetFileName` | `fn GetFileName(&self) -> BString` | 1 | 1 |

### SharedBase.dll :: bCString (25 symbols, 8114 sites)

| API | Signature | sites | fns |
| --- | --- | ---: | ---: |
| `Contains` | `fn Contains(&self, a0: *i8, a1: i32) -> bool` | 57 | 30 |
| `GetInteger` | `fn GetInteger(&self, a0: i32, a1: bool) -> i32` | 16 | 1 |
| `GetWord` | `fn GetWord(&self, a0: i32, a1: &BString, a2: &BString, a3: bool, a4: bool) -> i32` | 13 | 2 |
| `CompareNoCase` | `fn CompareNoCase(&self, a0: *i8) -> i32` | 11 | 2 |
| `IsEmpty` | `fn IsEmpty(&self) -> bool` | 7 | 6 |
| `GetFloat` | `fn GetFloat(&self, a0: i32) -> f32` | 6 | 1 |
| `Format` | `fn Format(&self, a0: *i8, a1: ...) -> bool` | 6 | 3 |
| `CountWords` | `fn CountWords(&self, a0: &BString) -> i32` | 2 | 1 |
| `Clear` | `fn Clear(&self)` | 2 | 1 |
| `GetFormattedString` | `fn GetFormattedString(a0: *i8, a1: ...) -> BString` | 2 | 2 |
| `CompareNoCase` | `fn CompareNoCase(&self, a0: &BString) -> i32` | 1 | 1 |
| `Delete` | `fn Delete(&self, a0: i32, a1: i32) -> i32` | 1 | 1 |
| `~bCString` | `fn ~bCString(&self)` | 3227 | 1222 |
| `bCString` | `fn bCString(&self, a0: *i8)` | 3074 | 1393 |
| `operator=` | `fn operator=(&self, a0: &BString) -> &BString` | 752 | 341 |
| `operator=` | `fn operator=(&self, a0: *i8) -> &BString` | 638 | 286 |
| `bCString` | `fn bCString(&self)` | 117 | 85 |
| `operator==` | `fn operator==(&self, a0: *i8) -> bool` | 107 | 41 |
| `operator==` | `fn operator==(&self, a0: &BString) -> bool` | 27 | 4 |
| `operator+` | `fn operator+(&self, a0: *i8) -> BString` | 14 | 7 |
| `bCString` | `fn bCString(&self, a0: &BString)` | 13 | 1 |
| `operator char const *` | `fn operator char const *(&self)` | 11 | 4 |
| `operator!=` | `fn operator!=(&self, a0: *i8) -> bool` | 7 | 6 |
| `operator+` | `fn operator+(&self, a0: &BString) -> BString` | 2 | 1 |
| `operator!=` | `fn operator!=(&self, a0: &BString) -> bool` | 1 | 1 |

### SharedBase.dll :: (data/other) (7 symbols, 635 sites)

| API | Signature | sites | fns |
| --- | --- | ---: | ---: |
| `_new@8` | data/import | 630 | 283 |
| `_GE_MESSAGE_INFO@8` | data/import | 2 | 1 |
| `_GetDLLVersion@20` | data/import | 1 | 1 |
| `GE_MESSAGEF_INFO` | data/import | 1 | 1 |
| `GE_MESSAGEF_WARN` | data/import | 1 | 1 |
| `g_iDefaultMessagePriority` | data/import | 0 | 0 |
| `g_fPosInfinity` | data/import | 0 | 0 |

### SharedBase.dll :: bCUnicodeString (13 symbols, 443 sites)

| API | Signature | sites | fns |
| --- | --- | ---: | ---: |
| `Replace` | `fn Replace(&self, a0: *u16, a1: *u16) -> i32` | 33 | 20 |
| `Format` | `fn Format(&self, a0: *u16, a1: ...)` | 7 | 5 |
| `~bCUnicodeString` | `fn ~bCUnicodeString(&self)` | 237 | 35 |
| `bCUnicodeString` | `fn bCUnicodeString(&self, a0: &BWString)` | 66 | 23 |
| `operator wchar_t *` | `fn operator wchar_t *(&self)` | 31 | 20 |
| `operator+` | `fn operator+(&self, a0: &BWString) -> BWString` | 27 | 5 |
| `operator+` | `fn operator+(&self, a0: *u16) -> BWString` | 10 | 1 |
| `bCUnicodeString` | `fn bCUnicodeString(&self, a0: i32)` | 10 | 3 |
| `bCUnicodeString` | `fn bCUnicodeString(&self)` | 9 | 8 |
| `operator=` | `fn operator=(&self, a0: &BWString) -> &BWString` | 7 | 6 |
| `bCUnicodeString` | `fn bCUnicodeString(&self, a0: *i8)` | 3 | 3 |
| `operator wchar_t const *` | `fn operator wchar_t const *(&self)` | 2 | 2 |
| `operator!=` | `fn operator!=(&self, a0: *u16) -> bool` | 1 | 1 |

### SharedBase.dll :: (free) (3 symbols, 186 sites)

| API | Signature | sites | fns |
| --- | --- | ---: | ---: |
| `::operator==` | `fn operator==(&self, a0: *i8, a1: &BString) -> bool` | 141 | 39 |
| `::operator!=` | `fn operator!=(&self, a0: *i8, a1: &BString) -> bool` | 28 | 25 |
| `::operator+` | `fn operator+(&self, a0: *i8, a1: &BString) -> BString` | 17 | 10 |

### SharedBase.dll :: bCVector (14 symbols, 171 sites)

| API | Signature | sites | fns |
| --- | --- | ---: | ---: |
| `SetVector` | `fn SetVector(&self, a0: &bCVector)` | 53 | 8 |
| `Translate` | `fn Translate(&self, a0: &bCVector)` | 23 | 9 |
| `Scale` | `fn Scale(&self, a0: f32)` | 22 | 8 |
| `GetSquareMagnitude` | `fn GetSquareMagnitude(&self) -> f32` | 20 | 18 |
| `GetTranslated` | `fn GetTranslated(&self, a0: &bCVector) -> bCVector` | 12 | 4 |
| `InvTranslate` | `fn InvTranslate(&self, a0: &bCVector)` | 11 | 6 |
| `Normalize` | `fn Normalize(&self) -> &bCVector` | 10 | 4 |
| `GetScaled` | `fn GetScaled(&self, a0: f32) -> bCVector` | 6 | 3 |
| `GetMagnitude` | `fn GetMagnitude(&self) -> f32` | 4 | 3 |
| `GetInvTranslated` | `fn GetInvTranslated(&self, a0: &bCVector) -> bCVector` | 3 | 2 |
| `GetUp` | `fn GetUp() -> &bCVector` | 2 | 1 |
| `GetDotProduct` | `fn GetDotProduct(&self, a0: &bCVector) -> f32` | 2 | 1 |
| `Invert` | `fn Invert(&self)` | 2 | 1 |
| `GetInverted` | `fn GetInverted(&self) -> bCVector` | 1 | 1 |

### SharedBase.dll :: bCMemoryAdmin (4 symbols, 102 sites)

| API | Signature | sites | fns |
| --- | --- | ---: | ---: |
| `GetInstance` | `fn GetInstance() -> &bCMemoryAdmin` | 51 | 32 |
| `Free` | `fn Free(&self, a0: *())` | 42 | 24 |
| `Realloc` | `fn Realloc(&self, a0: *(), a1: u32) -> *()` | 8 | 8 |
| `DeleteObject` | `fn DeleteObject(&self, a0: *())` | 1 | 1 |

### SharedBase.dll :: bCObjectRefBase (19 symbols, 20 sites)

| API | Signature | sites | fns |
| --- | --- | ---: | ---: |
| `NotifyPropertyValueChangedEnterEx` | `fn NotifyPropertyValueChangedEnterEx(&self, a0: *i8, a1: bool) -> bool` | 1 | 1 |
| `NotifyPropertyDeprecated` | `fn NotifyPropertyDeprecated(&self, a0: &BString, a1: &BString, a2: &bCMemoryStream, a3: u16)` | 1 | 1 |
| `OnNotifyPropertyValueChangedExitEx` | `fn OnNotifyPropertyValueChangedExitEx(&self, a0: *i8, a1: bool) -> bool` | 1 | 1 |
| `OnNotifyPropertyValueChangedEnterEx` | `fn OnNotifyPropertyValueChangedEnterEx(&self, a0: *i8, a1: bool) -> bool` | 1 | 1 |
| `NotifyPropertyValueChangedExit` | `fn NotifyPropertyValueChangedExit(&self, a0: *i8) -> u32` | 1 | 1 |
| `NotifyPropertyValueChangedEnter` | `fn NotifyPropertyValueChangedEnter(&self, a0: *i8) -> u32` | 1 | 1 |
| `PostInitializeProperties` | `fn PostInitializeProperties(&self) -> bEResult` | 1 | 1 |
| `IsVirtualReferencingEnabled` | `fn IsVirtualReferencingEnabled(&self) -> bool` | 1 | 1 |
| `ReleaseReference` | `fn ReleaseReference(&self) -> u32` | 1 | 1 |
| `AddReference` | `fn AddReference(&self) -> u32` | 1 | 1 |
| `Create` | `fn Create(&self) -> bEResult` | 1 | 1 |
| `Destroy` | `fn Destroy(&self)` | 1 | 1 |
| `Read` | `fn Read(&self, a0: &bCIStream) -> bEResult` | 1 | 1 |
| `Write` | `fn Write(&self, a0: &bCOStream) -> bEResult` | 1 | 1 |
| `IsValid` | `fn IsValid(&self) -> bool` | 1 | 1 |
| `GetVersion` | `fn GetVersion(&self) -> u16` | 1 | 1 |
| `NotifyPropertyValueChangedExitEx` | `fn NotifyPropertyValueChangedExitEx(&self, a0: *i8, a1: bool) -> bool` | 1 | 1 |
| `~bCObjectRefBase` | `fn ~bCObjectRefBase(&self)` | 2 | 2 |
| `bCObjectRefBase` | `fn bCObjectRefBase(&self)` | 1 | 1 |

### SharedBase.dll :: bCMessageAdmin (2 symbols, 8 sites)

| API | Signature | sites | fns |
| --- | --- | ---: | ---: |
| `GetInstance` | `fn GetInstance() -> &bCMessageAdmin` | 4 | 4 |
| `OnMessage` | `fn OnMessage(&self, a0: bEMessageTypes, a1: *i8, a2: *i8, a3: *i8, a4: i32, a5: i32) -> bool` | 4 | 4 |

### SharedBase.dll :: bCQuaternion (4 symbols, 8 sites)

| API | Signature | sites | fns |
| --- | --- | ---: | ---: |
| `RotateAxisX` | `fn RotateAxisX(&self, a0: f32)` | 2 | 1 |
| `Normalize` | `fn Normalize(&self)` | 2 | 1 |
| `RotateAxisZ` | `fn RotateAxisZ(&self, a0: f32)` | 2 | 1 |
| `RotateAxisY` | `fn RotateAxisY(&self, a0: f32)` | 2 | 1 |

### SharedBase.dll :: bCObjectBase (6 symbols, 6 sites)

| API | Signature | sites | fns |
| --- | --- | ---: | ---: |
| `GetProfileLastTicksThird` | `fn GetProfileLastTicksThird(&self) -> u32` | 1 | 1 |
| `GetProfileLastTicksSecond` | `fn GetProfileLastTicksSecond(&self) -> u32` | 1 | 1 |
| `GetProfileLastTicksFirst` | `fn GetProfileLastTicksFirst(&self) -> u32` | 1 | 1 |
| `GetReferenceCount` | `fn GetReferenceCount(&self) -> u32` | 1 | 1 |
| `CopyFrom` | `fn CopyFrom(&self, a0: &bCObjectBase) -> bEResult` | 1 | 1 |
| `Write` | `fn Write(&self, a0: &bCOStream) -> bool` | 1 | 1 |

### SharedBase.dll :: bCClassNameBase (1 symbols, 4 sites)

| API | Signature | sites | fns |
| --- | --- | ---: | ---: |
| `UnMangle` | `fn UnMangle(a0: *i8) -> BString` | 4 | 4 |

### SharedBase.dll :: bCMatrix (1 symbols, 4 sites)

| API | Signature | sites | fns |
| --- | --- | ---: | ---: |
| `operator=` | `fn operator=(&self, a0: &bCMatrix) -> &bCMatrix` | 4 | 2 |

### SharedBase.dll :: bCBox (1 symbols, 1 sites)

| API | Signature | sites | fns |
| --- | --- | ---: | ---: |
| `IsValid` | `fn IsValid(&self) -> bool` | 1 | 1 |

### SharedBase.dll :: bCPropertyID (1 symbols, 1 sites)

| API | Signature | sites | fns |
| --- | --- | ---: | ---: |
| `~bCPropertyID` | `fn ~bCPropertyID(&self)` | 1 | 1 |

## 5. Typed property accessors (`PS<Class>::Property<Name>`)

Reads (`operator T`) and writes (`operator=`) on the property sets of an entity. Each is a field of the original engine property sets; the Rust ECS components should expose these as plain typed fields (schemas also in `.local/research/class-schemas.json`). Columns are static read / write call-site counts.

Total: 141 properties across 24 property-set classes.

### PSRoutine (8 properties)

| Property | Type | reads | writes |
| --- | --- | ---: | ---: |
| AIMode | `&gEAIMode` | 198 | 69 |
| Action | `gEAction` | 64 | 72 |
| AniState | `&gEAniState` | 46 | 75 |
| StatePosition | `u32` | 35 | 37 |
| TaskPosition | `u32` | 18 | 12 |
| HitDirection | `&gEHitDirection` | 1 | 6 |
| AmbientAction | `&gEAmbientAction` | 0 | 4 |
| CommandTime | `&i32` | 2 | 2 |

### PSNpc (31 properties)

| Property | Type | reads | writes |
| --- | --- | ---: | ---: |
| Species | `gESpecies` | 158 | 6 |
| AttackReason | `gEAttackReason` | 66 | 10 |
| PoliticalAlignment | `&gEPoliticalAlignment` | 37 | 9 |
| CombatState | `u32` | 18 | 14 |
| LastFightAgainstPlayer | `gELastFightAgainstPlayer` | 10 | 12 |
| Level | `u32` | 18 | 3 |
| Type | `gENPCType` | 16 | 4 |
| Class | `&gEClass` | 17 | 1 |
| AttitudeToPlayer2 | `&gEAttitude` | 12 | 5 |
| ManaUsed | `i32` | 5 | 12 |
| LastFightTimestamp | `&f32` | 7 | 9 |
| TimeStampTransformed | `&u32` | 1 | 11 |
| LastPlayerCrime | `gEPlayerCrime` | 6 | 5 |
| GuardStatus | `gEGuardStatus` | 6 | 5 |
| LastDistToTarget | `f32` | 5 | 5 |
| LastPlayerAR | `gEAttackReason` | 4 | 5 |
| DefeatedByPlayer | `bool` | 3 | 3 |
| LastSpell | `&BString` | 3 | 3 |
| LevelMax | `u32` | 5 | 0 |
| Ransacked | `bool` | 1 | 3 |
| LastDistToGuardPoint | `f32` | 2 | 2 |
| Voice | `BString` | 3 | 0 |
| Discovered | `bool` | 1 | 2 |
| LastFightComment | `&bool` | 0 | 3 |
| Gender | `gEGender` | 3 | 0 |
| Immortal | `bool` | 1 | 1 |
| TimeStampFrozen | `u32` | 1 | 0 |
| LastHitTimestamp | `&u32` | 0 | 1 |
| LastCastTimestamp | `&u32` | 0 | 1 |
| TimeStampBurning | `u32` | 1 | 0 |
| TimeStampPoisoned | `u32` | 1 | 0 |

### PSEnclave (8 properties)

| Property | Type | reads | writes |
| --- | --- | ---: | ---: |
| Status | `&gEEnclaveStatus` | 45 | 3 |
| Switched | `bool` | 25 | 22 |
| PoliticalAlignment | `gEPoliticalAlignment` | 14 | 19 |
| KnownPlayerCrime | `gEPlayerCrime` | 7 | 5 |
| Revolution | `bool` | 6 | 4 |
| Raid | `bool` | 3 | 5 |
| PlayerFame | `i32` | 3 | 2 |
| MusicPlayed | `bool` | 1 | 1 |

### PSPlayerMemory (15 properties)

| Property | Type | reads | writes |
| --- | --- | ---: | ---: |
| LastWeaponConfig | `&gEWeaponConfig` | 4 | 11 |
| LPAttribs | `i32` | 6 | 7 |
| IsConsumingItem | `&bool` | 2 | 6 |
| BookOfZuben | `&i32` | 6 | 2 |
| BookOfFlood | `i32` | 6 | 2 |
| BookOfRhobar | `i32` | 6 | 2 |
| XP | `&i32` | 5 | 2 |
| SecondsTransformRemain | `&f32` | 3 | 4 |
| Chapter | `i32` | 2 | 2 |
| DuskToDawnStartHour | `i32` | 1 | 2 |
| TalkedToDiego | `&bool` | 1 | 1 |
| TalkedToMilten | `&bool` | 1 | 1 |
| TalkedToGorn | `&bool` | 1 | 1 |
| TalkedToLester | `&bool` | 1 | 1 |
| LastSpell | `bCPropertyID` | 1 | 1 |

### PSInteraction (4 properties)

| Property | Type | reads | writes |
| --- | --- | ---: | ---: |
| UseFunc | `&BString` | 46 | 1 |
| UsedByPlayer | `&bool` | 5 | 10 |
| UseType | `gEUseType` | 13 | 0 |
| ExitROIScript | `&BString` | 0 | 1 |

### PSParty (2 properties)

| Property | Type | reads | writes |
| --- | --- | ---: | ---: |
| PartyMemberType | `gEPartyMemberType` | 28 | 17 |
| Waiting | `bool` | 8 | 21 |

### PSCharacterControl (4 properties)

| Property | Type | reads | writes |
| --- | --- | ---: | ---: |
| IsPressed | `bool` | 25 | 0 |
| IsPressedBefore | `bool` | 15 | 0 |
| PressedKey | `gESessionKey` | 8 | 0 |
| DurationPressedMSecs | `u32` | 7 | 0 |

### PSMagic (20 properties)

| Property | Type | reads | writes |
| --- | --- | ---: | ---: |
| AnimActionCast | `BString` | 8 | 0 |
| SpellTarget | `gESpellTarget` | 6 | 0 |
| MinManaCost | `i32` | 4 | 0 |
| CastType | `gECastType` | 4 | 0 |
| EffectTargetCast | `BString` | 4 | 0 |
| SpellDeity | `gESpellDeity` | 3 | 0 |
| AnimActionPowerCast | `BString` | 3 | 0 |
| MaxManaCost | `i32` | 3 | 0 |
| SpellType | `gESpellType` | 2 | 0 |
| FuncOnTargetHit | `BString` | 2 | 0 |
| EffectCasterHold | `BString` | 1 | 0 |
| EffectItemHold | `BString` | 1 | 0 |
| EffectCasterLoad | `BString` | 1 | 0 |
| EffectItemLoad | `BString` | 1 | 0 |
| EffectCasterPower | `BString` | 1 | 0 |
| EffectTargetPower | `BString` | 1 | 0 |
| EffectItemPower | `BString` | 1 | 0 |
| EffectCasterCast | `BString` | 1 | 0 |
| EffectItemCast | `BString` | 1 | 0 |
| FuncOnCast | `BString` | 1 | 0 |

### PSFocus (7 properties)

| Property | Type | reads | writes |
| --- | --- | ---: | ---: |
| OverrideRange | `&f32` | 0 | 21 |
| FocusLookAtMode | `&gEFocusLookAt` | 0 | 8 |
| EnableRangeRating | `&bool` | 0 | 5 |
| OverrideCharacterPriority | `&f32` | 0 | 3 |
| OverrideInteractPriority | `&f32` | 0 | 3 |
| OverrideItemPriority | `&f32` | 0 | 2 |
| CurrentMode | `&gEFocus` | 0 | 2 |

### PSDamage (4 properties)

| Property | Type | reads | writes |
| --- | --- | ---: | ---: |
| DamageType | `gEDamageType` | 19 | 0 |
| DamageHitMultiplier | `f32` | 7 | 8 |
| DamageAmount | `i32` | 4 | 1 |
| ManaUsed | `i32` | 1 | 3 |

### PSNavigation (3 properties)

| Property | Type | reads | writes |
| --- | --- | ---: | ---: |
| IsOnDestination | `&bool` | 9 | 29 |
| StartPosition | `bCVector` | 2 | 1 |
| IsOnDestinationTimeStamp | `u32` | 1 | 0 |

### PSStrip (1 properties)

| Property | Type | reads | writes |
| --- | --- | ---: | ---: |
| Enabled | `&bool` | 0 | 36 |

### PSDialog (8 properties)

| Property | Type | reads | writes |
| --- | --- | ---: | ---: |
| MobEnabled | `bool` | 1 | 7 |
| PartyEnabled | `&bool` | 0 | 5 |
| EndDialogTimestamp | `f32` | 2 | 2 |
| SlaveryEnabled | `&bool` | 0 | 4 |
| TradeCategory | `gETradeCategory` | 2 | 0 |
| TeachEnabled | `&bool` | 0 | 2 |
| PickedPocket | `&bool` | 0 | 1 |
| TradeEnabled | `&bool` | 0 | 1 |

### PSAnchor (3 properties)

| Property | Type | reads | writes |
| --- | --- | ---: | ---: |
| UserCount | `&u32` | 8 | 5 |
| MaxUsers | `u32` | 6 | 3 |
| AnchorType | `gEAnchorType` | 3 | 0 |

### PSDamageReceiver (5 properties)

| Property | Type | reads | writes |
| --- | --- | ---: | ---: |
| HitPoints | `i32` | 4 | 2 |
| StaminaPoints | `&i32` | 3 | 2 |
| DamageAmount | `&i32` | 0 | 4 |
| HitPointsMax | `&i32` | 3 | 1 |
| StaminaPointsMax | `&i32` | 3 | 1 |

### PSLock (3 properties)

| Property | Type | reads | writes |
| --- | --- | ---: | ---: |
| Difficulty | `i32` | 9 | 0 |
| Status | `&gELockStatus` | 5 | 2 |
| KeyAmount | `i32` | 4 | 0 |

### PSProjectile (5 properties)

| Property | Type | reads | writes |
| --- | --- | ---: | ---: |
| PathStyle | `gEProjectilePath` | 3 | 4 |
| TargetDirection | `&bCVector` | 0 | 3 |
| TouchBehavior | `&gEProjectileTouchBehavior` | 0 | 1 |
| ShootVelocity | `&f32` | 0 | 1 |
| DecayDuration | `&f32` | 0 | 1 |

### PSCharacterMovement (2 properties)

| Property | Type | reads | writes |
| --- | --- | ---: | ---: |
| CanBePushedWhileIdle | `&bool` | 0 | 5 |
| ForceGroundAlignment | `&bool` | 0 | 2 |

### PSArena (1 properties)

| Property | Type | reads | writes |
| --- | --- | ---: | ---: |
| Status | `gEArenaStatus` | 6 | 0 |

### PSRecipe (2 properties)

| Property | Type | reads | writes |
| --- | --- | ---: | ---: |
| ResultAmount | `u32` | 4 | 0 |
| Craft | `gESkillCategory` | 1 | 0 |

### PSTouchDamage (1 properties)

| Property | Type | reads | writes |
| --- | --- | ---: | ---: |
| ResetOnUntouch | `&bool` | 0 | 4 |

### PSTreasureSet (1 properties)

| Property | Type | reads | writes |
| --- | --- | ---: | ---: |
| TreasureDistribution | `gETreasureDistribution` | 3 | 0 |

### PSInfoManager (1 properties)

| Property | Type | reads | writes |
| --- | --- | ---: | ---: |
| ChildMode | `&bool` | 0 | 2 |

### PSRigidBody (2 properties)

| Property | Type | reads | writes |
| --- | --- | ---: | ---: |
| LinearDamping | `&f32` | 0 | 1 |
| AngularDamping | `&f32` | 0 | 1 |
