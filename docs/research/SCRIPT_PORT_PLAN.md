# Script_Game.dll port plan

Companion to `SCRIPT_ENGINE_INTERFACE.md` (the host API) and the rule docs (`GAME_RULES.md`, `COMBAT_AI_RULES.md`, `GAME_LOGIC_MAP.md`). Written 2026-10-20 from a full Ghidra 12.1.4 decompilation of `scripts/Script_Game.dll` (build 1.75.20108.14). The decompiled bodies live only in the gitignored `.local/research/script_game/` (one `.c` file per function under `<category>/<name>.c`, plus `index.json`); this document contains my own prose, names and numbers only.

Evidence tags follow the rule docs: **[V]** read from code, **[I]** inferred, **[U]** unknown. Coverage means "an existing rule doc cites this function by `S:` address (`address`) or mentions its registry name (`name`, weaker: the doc may only list it)". It does not mean a Rust port exists or is verified; `crates/sim` currently implements a subset (see `crates/sim` and `SCRIPT_ENGINE_INTERFACE.md` section 3).

## 1. Inventory

| Item | Count |
| --- | ---: |
| Functions found by Ghidra (incl. 19 recovered by splitting merged bodies at registered entry addresses) | 2395 |
| Registered script entry points (unique addresses; the registry has 874 names, 27 are aliases of the same address) | 847 |
| Internal script helpers (called from registered code, no registry name) | 228 |
| Compiler/runtime support (CRT, RTTI, container instantiations, SEH/cleanup thunks, static-init stubs; port: none) | 1320 |

The 1320 support functions are **not** gameplay. Only one of them (an event handler at `S:1020` that toggles the game clock factor and prints a message, i.e. a developer feature) touches game state; skip it or reimplement as a debug command.

### Script ABI observed in the decompilation [V unless marked]

- **Resumable states.** A `ZS_*`/`PS_*` state body is called repeatedly with `(state stack, processing unit)`. Each blocking step has a lazily allocated static slot id; the function compares it with the progress counter stored in the top stack frame (frames are 0x18 bytes, `gScriptRunTimeSingleState`) and either skips the step as already done or queues one AI command and records progress [I: reading of the compare-and-store pattern; seen in `ZS_Smith` and all routine tasks]. Routine tasks additionally keep a `TaskPosition` counter in `PSRoutine`, incremented per pass and used as a `switch`. In Rust model every state as `fn step(&mut self, ctx) -> StepResult { Yield(cmd), Done, Goto(state) }` over an explicit step enum rather than trying to re-create the slot trick.
- **State transitions** are explicit host calls: `PSRoutine::SetState` (256 call sites) and `SetTask` (138), `ContinueRoutine` (116). Script-to-script calls go through `RunScriptFunction` by name (615 sites). A port needs a name-to-function table; `.local/research/script-registry.json` already enumerates it.
- **Callbacks** (`On*`) are plain functions invoked by the engine on animation events, damage, enter/leave areas and ticks; their signatures are the `ScriptCtx` surface (batch B14).
- **Use-type object table.** `S:8190` returns, for a `gEUseType` value, the interaction-object entity currently found for the NPC from a global table of `Entity` slots (filled by the scanner `S:7c90`). Routine tasks use it to get their work object: `ZS_Smith` asks for use types 0x39 Anvil and 0x3a Forge, `ZS_Digger` for 0x56 WaterBarrel [V id mapping via `gEUseType`, I table fill]; a `None` result sends the NPC to an idle fallback task.

## 2. Coverage by existing rule docs

| Category | Registered | covered by address | covered by name only | uncovered |
| --- | ---: | ---: | ---: | ---: |
| player_state | 151 | 7 | 16 | 128 |
| routine | 129 | 11 | 45 | 73 |
| ai_state | 123 | 16 | 35 | 72 |
| magic | 101 | 8 | 12 | 81 |
| ai_command | 89 | 12 | 13 | 64 |
| callback | 87 | 13 | 13 | 61 |
| memory | 64 | 18 | 5 | 41 |
| perception | 39 | 12 | 15 | 12 |
| decision | 38 | 17 | 7 | 14 |
| info_command | 14 | 2 | 12 | 0 |
| player_interface | 12 | 0 | 0 | 12 |
| **total** | **847** | **116** | **173** | **558** |

So **116 of 847 registered functions (14%) have an address-level citation**, a further 173 are only named in a doc, and **558 (66%) are uncovered**. Helpers: 14 of 228 cited by address. By code size the uncovered share is larger because the documented functions are the big ones (AssessHit, routine selection, attitude).

By machine-code bytes of registered functions: 416663 of 666536 bytes (63%) are uncovered.

## 3. Port order and batches

Order: Phase 0 foundation (leaf predicates and stat accessors every other batch calls), Phase 1 routines and Phase 2 AI states/combat (the NPC behaviour that makes the world playable), Phase 3 the engine-callback contract, Phase 4 player states, Phase 5 magic and dialog engine. Within a phase the order is by gameplay value and by dependency, but the call graph between batches is **not** acyclic (for example routines call party code in B12, work-station tasks call the idle tasks in B05, decision predicates call a few magic predicates in B20), so port against the host traits and stub the not-yet-ported callee behind a trait method; the `Calls into` line of each batch lists the actual edges. Batch sizes are 14-61 registered functions; helper counts are the private helpers that belong to the batch and should be ported with it.

| Batch | Phase | Title | Registered | Private helpers | Addr cov | Name cov | Uncovered | Code bytes |
| --- | --- | --- | ---: | ---: | ---: | ---: | ---: | ---: |
| B00 | 0 Foundation | Shared helper library | 0 | 123 | 0 | 0 | 0 | 29799 |
| B01 | 0 Foundation | NPC stat accessors (attribute get/set/add) | 61 | 0 | 17 | 5 | 39 | 14380 |
| B02 | 0 Foundation | Decision predicates (attitude, species, guard, parade, upset) | 43 | 0 | 20 | 7 | 16 | 18834 |
| B03 | 1 Routines | Routine core, party join/leave, ambient and animal routines | 37 | 8 | 7 | 20 | 10 | 28751 |
| B04 | 1 Routines | Routine tasks: crafting, farming, trading work stations | 34 | 0 | 0 | 12 | 22 | 45562 |
| B05 | 1 Routines | Routine tasks: idle, social, guard, sleep, ceremonial | 58 | 10 | 1 | 14 | 43 | 70091 |
| B06 | 2 AI states and combat | Perception, crime memory and enclave/revolution | 41 | 2 | 10 | 14 | 17 | 34863 |
| B07 | 2 AI states and combat | Damage pipeline: AssessHit, hit/damage/touch callbacks, XP | 22 | 9 | 12 | 2 | 8 | 26575 |
| B08 | 2 AI states and combat | Combat AI: ZS_Attack, combat-move selection, evade | 51 | 17 | 10 | 7 | 34 | 49126 |
| B09 | 2 AI states and combat | Combat AI commands: attack/parade/stumble/shoot/cast primitives | 38 | 1 | 1 | 2 | 35 | 30365 |
| B10 | 2 AI states and combat | Reaction states: damage, weapon, zone, suspect, fight watching, threaten | 50 | 4 | 0 | 13 | 37 | 35596 |
| B11 | 2 AI states and combat | Down states and movement modes: dead, unconscious, stumble, fall, slide, swim | 40 | 2 | 10 | 20 | 10 | 30732 |
| B12 | 2 AI states and combat | Party tasks, goto, speech and dialog commands | 44 | 9 | 4 | 10 | 30 | 38653 |
| B13 | 2 AI states and combat | Item, action and misc AI commands; nav test state | 23 | 15 | 0 | 4 | 19 | 34518 |
| B14 | 3 Engine contract | Engine lifecycle callbacks and engine queries | 26 | 13 | 6 | 2 | 18 | 36312 |
| B15 | 4 Player states | Player core: normal state, input interface, sleep, ghost, inventory setup | 28 | 3 | 1 | 5 | 22 | 33042 |
| B16 | 4 Player states | Player combat: melee and ranged states | 29 | 1 | 2 | 4 | 23 | 24194 |
| B17 | 4 Player states | Player magic casting and book stands | 23 | 1 | 0 | 3 | 20 | 26934 |
| B18 | 4 Player states | Player world interaction (chests, doors, workstations, mobs) | 53 | 1 | 1 | 3 | 49 | 45707 |
| B19 | 4 Player states | Player HUD, inventory use and crafting UI states | 36 | 5 | 4 | 2 | 30 | 20113 |
| B20 | 5 Magic and dialog engine | Magic spells (cast effects per school) | 51 | 3 | 7 | 8 | 36 | 25256 |
| B21 | 5 Magic and dialog engine | Magic states: summoned/area effects, freeze, sleep, hypnosis | 45 | 0 | 1 | 4 | 40 | 22144 |
| B22 | 5 Magic and dialog engine | Info-script engine states (dialog gating and command executor) | 14 | 1 | 2 | 12 | 0 | 9567 |

## 4. Batch details

### B00: Shared helper library

- **Scope:** 123 shared helpers, 29799 bytes. Cited by address in docs: 5.
- **Rule-doc sections citing these by address:** COMBAT_AI_RULES §2.12 (2), COMBAT_AI_RULES §2.10 (1), COMBAT_AI_RULES §9.5 (1), COMBAT_AI_RULES §9.3 (1), COMBAT_AI_RULES §10.5 (1).
- **Host API used (Script/Game/Engine dll classes by call sites):** Entity 813, PSInventory 118, PSNpc 70, Template 50, PSRoutine 35, eCApplication 25, PSInteraction 15.
- **Behaviour:** Not a gameplay module: the 123 non-registered helper subroutines that are reached from more than one batch. They fall into four groups: (1) typed wrappers over script-global lookups: the indexed global-entity table, class-name unmangling and module-admin lookups, used by almost every state; (2) growable-array and string plumbing instantiated for `Entity` and `bCString` (about 20 tiny functions: grow, copy, free); (3) gameplay helpers shared across modules: the hand-item-combination to weapon-mode lookup (`S:2dd0`, `S:3240` family, documented in COMBAT_AI_RULES 2.12), the animation-speed multiplier (`S:42a0`, the largest helper at about 2.9 KB, COMBAT_AI_RULES 9.5), weapon-slot and use-type probes over `PSInventory`/`PSInteraction`, SVM sample-name formatting (`S:baf0`, `S:bc20`), and the player-is-owner/target predicates (`S:79a0`..`S:7c90`); (4) AI-state bookkeeping used by routines (`S:1d90`/`S:1e20` write the routine's AniState property). Port these first as a `helpers` module behind the host traits; most are 10-150 bytes and become one-line Rust functions. The array/string plumbing disappears in Rust (`Vec<Entity>`, `String`).

### B01: NPC stat accessors (attribute get/set/add)

- **Scope:** 61 registered functions, 0 private helpers, 14380 bytes of machine code. Existing docs: 17 by address, 5 by name, 39 uncovered.
- **Rule-doc sections citing these by address:** COMBAT_AI_RULES §2.13 (17), COMBAT_AI_RULES §2.7 (2).
- **Calls into:** B02 (2 call edges).
- **Host API used (Script/Game/Engine dll classes by call sites):** Entity 561, gCScriptProcessingUnit 122, PSPlayerMemory 73, PSNpc 23, PSDamageReceiver 19, eCApplication 4, Template 4.
- **Behaviour:** Mechanical accessors on a character's stat block, all in the (self, other, value) script-function shape: attributes (Strength, Dexterity, Intelligence, Alchemy, Smithing, Theft), six protection channels (Blades, Impact, Missile, Fire, Ice, Lightning), hit points / mana / stamina with maxima and percentage getters, and level queries (`GetCurrentLevel`, `GetLevel`, `GetLevelMax`, `GetDiffToPlayerLevel`). Storage rule [V from the pseudo-code]: attributes and protections of the **player** live in the player-memory property set (a transformed player, and every NPC, instead derive the attribute from level through one shared helper, 10 minus a level-based term, per COMBAT_AI_RULES 2.13) and the `Set*`/`Add*` forms are no-ops for NPCs; hit points, mana and stamina live in `PSDamageReceiver` for NPCs and are mirrored into player memory for the player. `AddHitPoints`/`AddStaminaPoints` carry the only non-trivial rules (god mode, free-action timer, COMBAT_AI_RULES 9.5). The cheapest batch and it unblocks every combat and magic function; write it as a `Stats` trait with a macro-generated body.
- **Functions:** GetStrength*, GetDexterity*, GetIntelligence*, GetAlchemy*, GetSmithing*, GetTheft*, SetStrength, SetDexterity, SetIntelligence, SetAlchemy, SetSmithing, SetTheft, AddStrength*, AddDexterity, AddIntelligence, AddAlchemy, AddSmithing*, AddTheft, GetProtectionBlade*, GetProtectionImpact, GetProtectionMissile, GetProtectionFire, GetProtectionIce, GetProtectionLightning*, SetProtectionBlades, SetProtectionImpact, SetProtectionMissile, SetProtectionFire, SetProtectionIce, SetProtectionLightning, AddProtectionBlades, AddProtectionImpact, AddProtectionMissile, AddProtectionFire, AddProtectionIce, AddProtectionLightning, GetHitPoints*, GetHitPointsMax*, GetHitPointsPercent, SetHitPoints, SetHitPointsMax, AddHitPoints*, AddHitPointsMax, GetManaPoints*, GetManaPointsMax*, GetManaPointsPercent, SetManaPoints, SetManaPointsMax, AddManaPoints*, AddManaPointsMax, GetStaminaPoints, GetStaminaPointsMax*, GetStaminaPointsPercent, SetStaminaPoints, SetStaminaPointsMax, AddStaminaPoints*, AddStaminaPointsMax, GetCurrentLevel*, GetLevel*, GetLevelMax*, GetDiffToPlayerLevel*. (`*` = cited in a rule doc.)

### B02: Decision predicates (attitude, species, guard, parade, upset)

- **Scope:** 43 registered functions, 0 private helpers, 18834 bytes of machine code. Existing docs: 20 by address, 7 by name, 16 uncovered.
- **Rule-doc sections citing these by address:** COMBAT_AI_RULES §2.9 (5), COMBAT_AI_RULES §10.4 (5), COMBAT_AI_RULES §2.6 (4), COMBAT_AI_RULES §9.1 (2), COMBAT_AI_RULES §11.3 (2), COMBAT_AI_RULES §2.11 (1).
- **Calls into:** B00 (8 call edges), B20 (2 call edges), B01 (1 call edges).
- **Host API used (Script/Game/Engine dll classes by call sites):** Entity 757, gCScriptProcessingUnit 86, PSNpc 71, PSEnclave 25, PSParty 15, PSRoutine 10, PSInteraction 7.
- **Behaviour:** Pure decision predicates with no side effects: `GetAttitude` and its helpers (outlaw, revolution, player, political matrix, species relations), species/role classification (`IsHumanoid`, `IsUndead`, `IsLivestock`, `IsEvil`, `IsSlave`, `IsGuard`, `IsBoss`, `IsGateGuard`), the parade/guard-mode predicates (`CanParade*`, `IsInParadeMode`, `IsInFistMode`), `CanSleep`/`CanThreaten`/`CanEatGround`/`CanSwim` (the last three are stubs returning false), attack-reason checks, `GetFollowTime`, `IsUpset`, `WantToRansack`, plus the routine-time and walk-mode predicates and the two magic gates `CanTameAnimal`/`CanTransformInto`. Most are already specified in COMBAT_AI_RULES 10.4 and 2.12; the port is a table-driven function per predicate with unit tests lifted from the doc (political matrix, species table).
- **Functions:** CanParade*, CanParadeMoveOf*, IsInParadeMode*, IsInParadePose, IsInFistMode, HasStaffMagicParade, CanParadeMagic*, CanParadeMissile*, CanSleep, CanThreaten, CanEatGround, GetAttitude*, GetOutlawAttitude*, GetRevolutionAttitude*, GetAttitudeToPlayer*, GetFocusNameAttitude, GetZoneAttitudeToPlayer*, GetPoliticalAttitude*, GetSpeciesAttitude*, IsAmbientCreature*, GetFollowTime*, HasAttackReasonToKill*, HasAttackReasonToWatch*, HasDangerousWeapon, IsDroppableWeapon, IsEvil*, IsBoss*, IsGuard*, IsGateGuard, IsHumanoid*, IsLivestock, IsSlave, IsUndead*, SetLastFightTimestamp, IsTemporaryFriendlyToPlayer*, IsUpset*, WantToRansack, CanTameAnimal, CanTransformInto, GetRoutineTime*, GetTravellingWalkMode*, IsFreeDestination*, CanSwim*. (`*` = cited in a rule doc.)

### B03: Routine core, party join/leave, ambient and animal routines

- **Scope:** 37 registered functions, 8 private helpers, 28751 bytes of machine code. Existing docs: 7 by address, 20 by name, 10 uncovered.
- **Rule-doc sections citing these by address:** COMBAT_AI_RULES §11.1 (3), COMBAT_AI_RULES §11.3 (3), COMBAT_AI_RULES §11.4 (1).
- **Calls into:** B00 (57 call edges), B02 (13 call edges), B12 (4 call edges), B06 (3 call edges), B11 (2 call edges), B01 (1 call edges), B20 (1 call edges), B14 (1 call edges).
- **Host API used (Script/Game/Engine dll classes by call sites):** Entity 672, gCScriptProcessingUnit 76, PSRoutine 73, PSParty 50, PSNpc 37, gCScriptAdmin 31, PSNavigation 23.
- **Behaviour:** The routine engine proper. `ContinueRoutine` resumes the NPC's daily schedule, `SelectRoutineTask`/`SelectRoutineDestination` choose the next anchor and task by anchor type and use type (the table in COMBAT_AI_RULES 11.3), `SendToArena` and the party calls (`CanJoinPlayerParty`, `JoinPlayerParty`, `DismissPlayerParty`, mob and slavery variants, `WaitPlayerParty`, guide start/stop) manage the player's companion state. The `Rtn_*` entries are the top-level routine entry points for monsters, the dead and the player [I: from names]. Also here: the animal and ambient fillers (`ZS_Ambient`, `ZS_AnimalSleep`, `ZS_Herd`, `ZS_Monster`) and the self-heal pair. Highest gameplay value because it is what makes the world look alive; depends on B01, B02 and the anchor/navigation host API.
- **Functions:** Rtn_Monster*, Rtn_Dead*, Rtn_Player*, OnGateGuard, OnSleep*, GetAmbientAction*, ZS_Ambient*, ZS_Ambient_Loop, ZS_Ambient_Sleep, ZS_Ambient_Sleep_Loop*, ZS_AnimalSleep*, OnAnimalSleep, ZS_AnimalSleep_SleepGround, ZS_Herd*, ZS_Herd_EatGround, ZS_Herd_DrinkWater, ZS_Monster*, ZS_Monster_Roam, ContinueRoutine*, ZS_StandUp*, CanHealSelf*, ZS_HealSelf*, ZS_HealSelf_Cast, SelectRoutineDestination*, SetCurrentDestinationPoint, SendToArena*, CanJoinPlayerParty*, JoinPlayerParty*, DismissPlayerParty*, JoinPlayerMob*, DismissPlayerMob*, JoinPlayerSlavery*, WaitPlayerParty*, StartGuidePlayer*, StopGuidePlayer*, StopBringNPC*, SelectRoutineTask*. (`*` = cited in a rule doc.)

### B04: Routine tasks: crafting, farming, trading work stations

- **Scope:** 34 registered functions, 0 private helpers, 45562 bytes of machine code. Existing docs: 0 by address, 12 by name, 22 uncovered.
- **Calls into:** B00 (135 call edges), B12 (15 call edges), B05 (9 call edges), B11 (1 call edges).
- **Host API used (Script/Game/Engine dll classes by call sites):** Entity 781, gCScriptAdmin 110, gCScriptProcessingUnit 68, PSRoutine 66, Template 65, PSNavigation 15, PSInventory 9.
- **Behaviour:** Work-station routine tasks. Each `ZS_*` is a short script that walks to an interaction point, plays a use animation loop and cycles through sub-steps: Smith (forge, anvil, cool, grindstone), Alchemy and Cook/Barbecue, Study (bookstand, bookshelf), Digger (ore, ground, water barrel), Farmer (rake, field work), Lumberjack, Woodwork (saw, repair), CarryStones and Trader (stand, bookstand, chest, sweep). The shared shape is a step counter in the routine property set that advances one animation command per pass and falls back to an idle task when no free interaction object is found. The task *selection* is documented (COMBAT_AI_RULES 11.3) but the sub-step sequences are not; they need a read of the animation names and anchor probes per state. Uncovered, large and repetitive, so suit a table-driven port (task -> list of (use type, animation, duration)).
- **Functions:** ZS_Alchemy*, ZS_Alchemy_Alchemy, ZS_Alchemy_Bookstand, ZS_Alchemy_Cauldron, ZS_Barbecue*, ZS_CarryStones*, ZS_Cook*, ZS_Digger*, ZS_Digger_PickOre, ZS_Digger_PickGround, ZS_Digger_DigGround, ZS_Digger_WaterBarrel, ZS_Farmer*, ZS_Farmer_RakeField, ZS_Farmer_FieldWork, ZS_Grind*, ZS_Lumberjack*, ZS_Smith*, ZS_Smith_Forge, ZS_Smith_Anvil, ZS_Smith_Cool, ZS_Smith_GrindStone, ZS_Study*, ZS_Study_Bookstand, ZS_Study_Bookshelf, ZS_Trader*, ZS_Trader_Stand, ZS_Trader_Bookstand, ZS_Trader_Bookshelf, ZS_Trader_Chest, ZS_Trader_SweepFloor, ZS_Woodwork*, ZS_Woodwork_SawLog, ZS_Woodwork_Repair. (`*` = cited in a rule doc.)

### B05: Routine tasks: idle, social, guard, sleep, ceremonial

- **Scope:** 58 registered functions, 10 private helpers, 70091 bytes of machine code. Existing docs: 1 by address, 14 by name, 43 uncovered.
- **Rule-doc sections citing these by address:** COMBAT_AI_RULES §11.3 (1).
- **Calls into:** B00 (167 call edges), B12 (15 call edges), B03 (3 call edges), B02 (2 call edges), B11 (1 call edges), B06 (1 call edges).
- **Host API used (Script/Game/Engine dll classes by call sites):** Entity 1257, gCScriptAdmin 152, PSRoutine 138, gCScriptProcessingUnit 121, Template 62, PSNavigation 32, PSNpc 30.
- **Behaviour:** The remaining routine tasks: social and idle behaviour (`ZS_HangAround` with its stool/bench/ground/waterpipe/campfire variants, `ZS_Break` including sitting, stretching, relieving oneself and chest/water-barrel pauses, `ZS_Smalltalk` leader/partner pairs, `ZS_Listen`), guard duty (`ZS_Guard`), the boss and slave-babe tasks (`ZS_Boss`, `ZS_SlaveBabe`), religion (`ZS_Pray`, `ZS_Preach` with auto-preach and SVM lines), bard, goblin and orc-party tasks (drum, dance, boulder, spectator), sleeping (`ZS_Sleep` bed/ground) and `ZS_RunToAttackPoint`. These choose animations from small random tables and pair NPCs through the `PSParty`/`Entity` partner search, which is the part that needs host support (finding a free partner within range and syncing the two states). Sleep timing is tied to `GetRoutineTime` (B02).
- **Functions:** ZS_Bard*, ZS_Boss*, ZS_Boss_Stand, ZS_Boss_Throne, ZS_Boss_Pace, HaveABreak*, ZS_Break*, ZS_Break_Stool, ZS_Break_Bench, ZS_Break_SitGround, ZS_Break_Stand, ZS_Break_Pee, ZS_Break_Chest, ZS_Break_WaterBarrel, ZS_Goblin*, ZS_Goblin_FryMeat, ZS_Goblin_SitGround, ZS_Goblin_Smalltalk, ZS_Guard*, ZS_Guard_Loop, ZS_HangAround*, ZS_HangAround_Stool, ZS_HangAround_Bench, ZS_HangAround_SitGround, ZS_HangAround_FryMeat, ZS_HangAround_Waterpipe, ZS_HangAround_Stand, ZS_HangAround_Pee, ZS_Idiot, ZS_Deadlock, ZS_HangAround_DoNothing, ZS_Listen, ZS_Listen_Loop, ZS_OrcParty*, ZS_OrcParty_OrcDrum, ZS_OrcParty_OrcDance, ZS_OrcParty_OrcBoulder, ZS_OrcParty_Spectator, ZS_Pray*, ZS_Pray_Shrine, ZS_Preach*, ZS_Preach_AutoPreach, ZS_Preach_SVM, ZS_RunToAttackPoint*, ZS_SlaveBabe*, ZS_SlaveBabe_CleanFloor, ZS_SlaveBabe_SweepFloor, ZS_SlaveBabe_FanBoss, ZS_SlaveBabe_Dance, ZS_Sleep*, ZS_Sleep_Bed*, ZS_Sleep_Ground, ZS_Sleep_Loop*, ZS_Smalltalk, ZS_Smalltalk_Partner, ZS_Smalltalk_Partner_Loop, ZS_Smalltalk_Leader, ZS_Smalltalk_Leader_Loop. (`*` = cited in a rule doc.)

### B06: Perception, crime memory and enclave/revolution

- **Scope:** 41 registered functions, 2 private helpers, 34863 bytes of machine code. Existing docs: 10 by address, 14 by name, 17 uncovered.
- **Rule-doc sections citing these by address:** COMBAT_AI_RULES §10.5 (5), COMBAT_AI_RULES §10.1 (3), COMBAT_AI_RULES §9.1 (1), COMBAT_AI_RULES §10.2 (1).
- **Calls into:** B02 (51 call edges), B00 (37 call edges), B12 (5 call edges), B11 (4 call edges), B22 (2 call edges), B20 (1 call edges), B10 (1 call edges), B03 (1 call edges).
- **Host API used (Script/Game/Engine dll classes by call sites):** Entity 1016, PSRoutine 128, PSNpc 121, gCScriptProcessingUnit 82, PSEnclave 33, PSParty 28, gCScriptAdmin 10.
- **Behaviour:** The perception and crime layer, already well documented (COMBAT_AI_RULES 10): the `SendPerception*` family broadcasts events to all NPCs, `CanReceivePerceptionFrom` filters receivers, and each `Assess*` handler (target, hit-independent events, takeItem/useInteract, player sneaking/weapon/zone, fight sound, murder, defeat, talk, command attack/follow, active perception poll) applies its own distance, field-of-view and line-of-sight tests and then switches the NPC into a reaction state. Crime bookkeeping (`MemorizePlayerCrime`, `GetPlayerCrime`, `SetAttackReason`) and the enclave/revolution functions (`NotifyEnclave`, `ChangeEnclaveStatus`, `MemorizeEnclaveCrime`, `AddEnclaveTheftCount`, `ZS_RevolutionAttack`, `ZS_RevolutionSuccess`) are in the same batch. About two thirds are covered by address; port against that doc and add the missing handlers (Talk, CmdAttack/CmdFollow, Warn) from the pseudo-code.
- **Functions:** AssessActivePerception*, AssessCmdAttack, AssessDefeat*, AssessFightSound*, AssessMurder*, AssessPlayerSneaking*, AssessPlayerWeapon*, AssessPlayerZone*, AssessPlayerEnterZone, ZS_GotoPlayerZone, AssessTakeItem*, IsBotheredByPlayerTheftOf*, AssessTalk, AssessTarget*, AssessUseInteractObject*, AssessWarn, AssessAttack, CanReceivePerceptionFrom*, SendPerceptionCmdAttack*, SendPerceptionCmdFollow, SendPerceptionFightSound*, SendPerceptionDefeat*, SendPerceptionMurder*, SendPerceptionTakeItem, SendPerceptionUseInteractObject, SendPerceptionWarn, SendPerceptionPreach, SendPerceptionEnterZone, CallHelp*, GetPlayerCrime, MemorizePlayerCrime*, SetAttackReason, AssessCmdFollow*, NotifyEnclave*, ChangeEnclaveStatus*, MemorizeEnclaveCrime*, AddEnclaveTheftCount*, ZS_RevolutionAttack*, OnRevolutionAttack, ZS_RevolutionAttack_Loop, ZS_RevolutionSuccess. (`*` = cited in a rule doc.)

### B07: Damage pipeline: AssessHit, hit/damage/touch callbacks, XP

- **Scope:** 22 registered functions, 9 private helpers, 26575 bytes of machine code. Existing docs: 12 by address, 2 by name, 8 uncovered.
- **Rule-doc sections citing these by address:** COMBAT_AI_RULES §1 (4), COMBAT_AI_RULES §2.3 (4), COMBAT_AI_RULES §2.10 (3), COMBAT_AI_RULES §2.5 (3), COMBAT_AI_RULES §2.7 (2), GAME_RULES §4.2 (2).
- **Calls into:** B01 (18 call edges), B02 (15 call edges), B00 (6 call edges), B20 (4 call edges), B06 (3 call edges), B11 (1 call edges), B19 (1 call edges).
- **Host API used (Script/Game/Engine dll classes by call sites):** Entity 658, Template 91, PSRoutine 74, PSNpc 73, PSInventory 61, gCScriptProcessingUnit 45, PSDamage 19.
- **Behaviour:** The combat resolution path: `OnHit`/`OnDamage`/`OnTouchDamage` callbacks feed `DoLogicalDamage` and `AssessHit` (the big resolver, S:2b580) with its helper predicates (`CanBePoisoned`, `CanBeDiseased`, `CanBurn`, `CanFreeze`, `CanBeKilled`, `IsDeadlyDamage`, `GetProtection`, `GetPlayerSkillDamageBonus`), plus `GiveXP`/`GetXPNext` and the cheat kill/defeat hooks and item/projectile touch callbacks. Almost fully specified in COMBAT_AI_RULES 1-3 and GAME_RULES 4 and already ported in part by `crates/sim`; the port task is verification against the pseudo-code and filling the few uncovered callbacks (character-touch, projectile-touches-level, item-received).
- **Functions:** AssessHit*, CanBePoisoned*, CanBeDiseased*, CanBurn*, CanFreeze*, CanBeKilled*, IsDeadlyDamage*, GetProtection*, GetPlayerSkillDamageBonus*, GetProtectionForHUD, GiveXP*, GetXPNext*, OnCheatKill, OnCheatDefeat, OnDamage*, OnHit*, OnCharacterTouchesCharacter, OnTouchDamage, DoLogicalDamage*, OnProjectileTouchsLevel, OnItemTouchesLevel, OnItemReceived. (`*` = cited in a rule doc.)

### B08: Combat AI: ZS_Attack, combat-move selection, evade

- **Scope:** 51 registered functions, 17 private helpers, 49126 bytes of machine code. Existing docs: 10 by address, 7 by name, 34 uncovered.
- **Rule-doc sections citing these by address:** COMBAT_AI_RULES §9.1 (5), COMBAT_AI_RULES §9.3 (4), COMBAT_AI_RULES §9.2 (1).
- **Calls into:** B00 (70 call edges), B02 (12 call edges), B06 (4 call edges), B12 (2 call edges), B14 (2 call edges), B01 (1 call edges), B16 (1 call edges), B20 (1 call edges), B11 (1 call edges), B07 (1 call edges).
- **Host API used (Script/Game/Engine dll classes by call sites):** Entity 1119, PSRoutine 199, gCScriptProcessingUnit 124, PSNpc 123, gCScriptAdmin 59, eCVisualAnimation_PS 56, eCApplication 21.
- **Behaviour:** The NPC combat brain. `ZS_Attack` and its sub-states (loop, wait, goto, sprint, change position, go back, strafe left/right, turn, wait-for-flee, end) plus the `OnAttack_*` callbacks implement the decision loop in COMBAT_AI_RULES 9.1: pick weapon, call `SelectCombatMove` (which dispatches by AI archetype to `FAI_Active`, `FAI_Passive`, `FAI_Ranged`, `FAI_Magic`, `FAI_Misted`), map the returned action to a state, apply the give-up and retreat rules. `GetAttackRange`/`GetCloseRange` give species ranges and the evade states (`ZS_EvadeLoop/Left/Right/Forward/Backward`) are the dodge manoeuvres. The tactic scoring and ranges are documented; the per-state movement sequencing (positioning, strafing, sprint) is only partly covered.
- **Functions:** ZS_Attack*, OnAttack, ZS_Attack_Loop*, ZS_Attack_Wait, ZS_Attack_Goto*, OnAttack_Goto*, ZS_Attack_Sprint*, ZS_Attack_ChangePosition, OnAttack_Positioning, ZS_Attack_GoBack*, OnAttack_GoBack, ZS_Attack_StrafeLeft, ZS_Attack_StrafeRight, OnAttack_Strafe, ZS_Attack_TurnLeft, ZS_Attack_TurnRight, OnAttack_Turn, ZS_Attack_Attack, ZS_Attack_SimpleWhirl, ZS_Attack_PowerAttack, ZS_Attack_WhirlAttack, ZS_Attack_QuickAttack, ZS_Attack_Parade, ZS_Attack_JumpBack, ZS_Attack_PierceAttack, ZS_Attack_HackAttack, ZS_Attack_FinishingAttack, ZS_Attack_DrinkPotion, ZS_Attack_Shoot, ZS_Attack_Cast, ZS_Attack_WaitForDialog, ZS_Attack_WaitOnShore, ZS_Attack_WaitForFlee*, ZS_Attack_WaitForFlee_Loop, CancelAttack, ZS_Attack_End*, ZS_Attack_Arena_End, GetCombatWalkMode*, ZS_EvadeLoop, ZS_EvadeLeft, ZS_EvadeRight, ZS_EvadeForward, ZS_EvadeBackward, GetAttackRange*, GetCloseRange*, SelectCombatMove*, FAI_Active*, FAI_Passive*, FAI_Ranged*, FAI_Magic*, FAI_Misted*. (`*` = cited in a rule doc.)

### B09: Combat AI commands: attack/parade/stumble/shoot/cast primitives

- **Scope:** 38 registered functions, 1 private helpers, 30365 bytes of machine code. Existing docs: 1 by address, 2 by name, 35 uncovered.
- **Rule-doc sections citing these by address:** COMBAT_AI_RULES §9.7 (1).
- **Calls into:** B00 (99 call edges), B01 (2 call edges), B12 (2 call edges), B11 (2 call edges), B06 (1 call edges), B02 (1 call edges).
- **Host API used (Script/Game/Engine dll classes by call sites):** Entity 483, PSRoutine 131, PSInventory 61, gCScriptProcessingUnit 55, PSNpc 40, gCScriptAdmin 36, PSStrip 29.
- **Behaviour:** The AI command primitives for fighting, each paired with an `OnAI_*` callback that fires on the animation event: attack variants (quick, power, pierce, hack, simple whirl, whirl, finishing, get-up attack), defence (`_AI_Parade`, parade/heavy/pierce/quick stumbles, get-up parade, knock-down lie), movement (`_AI_Jump`, `_AI_JumpBack`, `_AI_StandUp`), ranged (`_AI_Shoot`, `_AI_Reload`), casting (`_AI_Cast`, begin/execute, `_AI_StartCastPhase`) and `_AI_Die`. They enqueue the animation through the AI instruction queue, charge stamina and set the hit-window timers documented in COMBAT_AI_RULES 9.5. The player melee states (B16) call the same commands, so this batch is shared by NPC and player combat.
- **Functions:** _AI_Attack*, OnAI_Attack, _AI_Cast*, _AI_Cast_Begin, _AI_Cast_Execute, _AI_StartCastPhase*, _AI_Die, _AI_FinishingAttack, OnAI_FinishingAttack, _AI_GetUpAttack, OnAI_GetUpAttack, _AI_GetUpParade, OnAI_GetUpParade, _AI_HackAttack, OnAI_HackAttack, _AI_HeavyParadeStumble, _AI_Jump, _AI_JumpBack, _AI_LieKnockDown, _AI_Parade, OnAI_Parade, _AI_ParadeStumble, _AI_PierceAttack, OnAI_PierceAttack, _AI_PierceStumble, _AI_PowerAttack, OnAI_PowerAttack, _AI_QuickAttack, OnAI_QuickAttack, _AI_QuickStumble, _AI_Reload, _AI_Shoot, _AI_StandUp, _AI_Stumble, _AI_SimpleWhirl, OnAI_SimpleWhirl, _AI_WhirlAttack, OnAI_WhirlAttack. (`*` = cited in a rule doc.)

### B10: Reaction states: damage, weapon, zone, suspect, fight watching, threaten

- **Scope:** 50 registered functions, 4 private helpers, 35596 bytes of machine code. Existing docs: 0 by address, 13 by name, 37 uncovered.
- **Calls into:** B00 (96 call edges), B06 (11 call edges), B02 (11 call edges), B20 (2 call edges), B11 (1 call edges), B08 (1 call edges), B03 (1 call edges).
- **Host API used (Script/Game/Engine dll classes by call sites):** Entity 803, PSRoutine 172, gCScriptProcessingUnit 106, PSNpc 68, gCScriptAdmin 39, PSNavigation 14, PSInventory 11.
- **Behaviour:** Reaction states that sit between routine and combat: `ZS_ReactToDamage`, `ZS_ReactToWeapon` (two warnings then attack), `ZS_ReactToZone`, `ZS_ObserveSuspect` family (sneak, footsteps, applaud, observe-weapon, zone), `ZS_WatchFight` (spectator, cheer, go-back), `ZS_Threaten`, `ZS_Flee` (flee point, observe follower, go back), `ZS_EatBody`, `ZS_RansackBody`/`ZS_FetchWeapon` and `ZS_Talk`. The timers and outcomes are in COMBAT_AI_RULES 10.3; the remaining steps are animation/line choreography with `AI_Say`-style commands and perception polls.
- **Functions:** ZS_EatBody*, OnEatBody_Perception, ZS_EatBody_Loop, ZS_Flee*, ZS_Flee_GotoFleePoint, OnFlee_GotoFleePoint, ZS_Flee_ObserveFollower, ZS_Flee_GoBack, OnFlee_GoBack*, ZS_ObserveSuspect*, ZS_ObserveSuspect_Loop, ZS_ReactToSneak*, ZS_ObserveWeapon, ZS_Applaud, ZS_ObserveZonePlayer, ZS_ReactToFootsteps*, ZS_RansackBody*, OnRansackBody, ZS_FetchWeapon*, ZS_ReactToDamage*, OnReactToDamage, ZS_ReactToDamage_Loop, ZS_ReactToDamage_GoBack, OnReactToDamage_GoBack, ZS_ReactToDamage_End, ZS_ReactToWeapon*, OnReactToWeapon, ZS_ReactToWeapon_WaitForSecondWarn, ZS_ReactToWeapon_WaitForAttack, ZS_ReactToWeapon_End, ZS_ReactToZone*, OnReactToZone, ZS_ReactToZone_Loop, ZS_Talk, ZS_Talk_Loop, ZS_Lee_Teleport, ZS_Threaten*, ZS_Threaten_Loop, ZS_WatchFight*, OnWatchFight, ZS_WatchFight_Loop, ZS_WatchFight_GoBack, OnWatchFight_GoBack, ZS_WatchFight_Goto, OnWatchFight_Goto, CheerFight, ZS_WatchFight_Spectator, ZS_WatchFight_Spectator_Loop, ZS_WatchFight_WaitForFlee, ZS_WatchFight_WaitForFlee_Loop. (`*` = cited in a rule doc.)

### B11: Down states and movement modes: dead, unconscious, stumble, fall, slide, swim

- **Scope:** 40 registered functions, 2 private helpers, 30732 bytes of machine code. Existing docs: 10 by address, 20 by name, 10 uncovered.
- **Rule-doc sections citing these by address:** COMBAT_AI_RULES §4.2 (3), COMBAT_AI_RULES §5.1 (3), COMBAT_AI_RULES §9.4 (2), COMBAT_AI_RULES §3 (2), COMBAT_AI_RULES §4.1 (1), COMBAT_AI_RULES §2.10 (1).
- **Calls into:** B00 (55 call edges), B02 (10 call edges), B01 (6 call edges), B07 (3 call edges), B06 (3 call edges), B20 (3 call edges), B03 (2 call edges), B12 (2 call edges).
- **Host API used (Script/Game/Engine dll classes by call sites):** Entity 695, PSRoutine 103, gCScriptProcessingUnit 94, PSNpc 79, PSInventory 27, PSCharacterMovement 18, gCScriptAdmin 15.
- **Behaviour:** Down and movement-mode states: death (`ZS_Dead`, pierced and ragdoll dead, `Kill`, plunder cleanup), unconscious (`ZS_Unconscious` loop with regeneration and wake-up timers, `Defeat`), the stumble and knock-down states (`ZS_Stumble` family: parade, heavy, quick, pierce, sit knock-down and its get-up attack/parade), and the engine's `OnMovementModeChanged` with fall, land, heavy land, acrobat, slide and swim/dive states (fall damage rule in COMBAT_AI_RULES 5.1, swimming in 11.5). Many entries are covered by name only; the numerical rules (regen, wake timers) are documented, the transitions between the states are not.
- **Functions:** ZS_PiercedDead*, ZS_Dead*, ZS_Dead_Loop, Kill*, CleanUpPlunderInv*, ZS_RagDollDead*, ZS_RagDollDead_Loop, ZS_Stumble*, ZS_HeavyParadeStumble*, ZS_QuickStumble*, PipiStumble*, ZS_ParadeStumble*, ZS_SitKnockDown*, ZS_SitKnockDown_Loop, ZS_SitKnockDown_GetUpAttack, ZS_SitKnockDown_GetUpParade, ZS_PierceStumble*, AssessAgressor, ResetAll*, ResetInteraction*, DropHandItems, RemoveNonCombatItems*, ZS_PiercedKO*, ZS_LieKnockDown*, ZS_Unconscious*, ZS_Unconscious_Loop*, ZS_Unconscious_End*, Defeat*, OnMovementModeChanged*, ZS_Fall*, ZS_Land*, ZS_HeavyLand*, ZS_Acrobat*, ZS_Slide*, ZS_Slide_Loop, OnSlide, PS_Slide_Jump*, ZS_DiveIn*, PS_Swim*, PS_Swim_Loop. (`*` = cited in a rule doc.)

### B12: Party tasks, goto, speech and dialog commands

- **Scope:** 44 registered functions, 9 private helpers, 38653 bytes of machine code. Existing docs: 4 by address, 10 by name, 30 uncovered.
- **Rule-doc sections citing these by address:** COMBAT_AI_RULES §8.2 (2), COMBAT_AI_RULES §11.4 (1), COMBAT_AI_RULES §10.5 (1), COMBAT_AI_RULES §3 (1).
- **Calls into:** B00 (69 call edges), B06 (13 call edges), B01 (8 call edges), B02 (5 call edges), B14 (3 call edges), B20 (2 call edges), B07 (1 call edges), B03 (1 call edges).
- **Host API used (Script/Game/Engine dll classes by call sites):** Entity 720, gCScriptProcessingUnit 74, gCScriptAdmin 63, PSRoutine 52, PSNpc 38, PSNavigation 23, PSParty 15.
- **Behaviour:** Companion and dialog behaviour: party tasks (`ZS_FollowPlayer`/`ZS_FollowNPC`, `ZS_GuidePlayer`, `ZS_WaitForPlayer`, `ZS_Arena`, `ZS_TravelToDestination`), goto extensions (`_AI_Goto_Ext`, `EndGoto`, wait-for-free-destination, controlled return home), and the speech command layer (`_AI_Say`, `_AI_SaySVM`, `_AI_Say_Ex`, guard/boss/crime news, all `StartSay*` lines) together with the social crime tools `_AI_Excuse`, `_AI_Bribe` and `PickPocket`/`GeneratePickpocketInventory` (COMBAT_AI_RULES 8.2, 10.5). Follow distance and tick rate are documented; the SVM line selection is a lookup into the voice-sample tables and needs the SVM manager host API.
- **Functions:** ZS_Arena*, OnArena, ZS_Arena_Loop, ZS_TravelToDestination, ZS_TravelToDestination_Loop, ZS_FollowPlayer*, OnFollowPlayer, ZS_FollowPlayer_Loop, ZS_FollowNPC*, OnFollowNPC, ZS_FollowNPC_Loop, ZS_GuidePlayer*, OnGuidePlayer, ZS_GuidePlayer_Loop, ZS_GuidePlayer_Wait, ZS_WaitForPlayer*, ZS_WaitForPlayer_Loop, _AI_SayGuardNews, _AI_SayCrimeInvestigation, _AI_SayBossNews, GetGuardAttackReason, _AI_Excuse*, _AI_Bribe*, PickPocket*, GeneratePickpocketInventory*, _AI_Goto_Ext, EndGoto, HandleControlledReturnHome, ZS_WaitForFreeDestination, ZS_WaitForFreeDestination_Loop, _AI_SaySVM*, _AI_Say*, _AI_Say_Ex*, StartSayAttackEnd, StartSayDefeatComment*, StartSayAttackReason, StartSayConfirm, StartSayHoldingPosition, StartSayFollow, StartSayRefuseTalk, StartSayCrimeRefuse, StartSayZoneRefuse, StartSayAargh, OnWaitForPlayer*. (`*` = cited in a rule doc.)

### B13: Item, action and misc AI commands; nav test state

- **Scope:** 23 registered functions, 15 private helpers, 34518 bytes of machine code. Existing docs: 0 by address, 4 by name, 19 uncovered.
- **Calls into:** B00 (73 call edges), B11 (3 call edges), B14 (2 call edges), B01 (1 call edges).
- **Host API used (Script/Game/Engine dll classes by call sites):** Entity 508, PSInventory 229, Template 66, PSRoutine 43, PSInteraction 39, gCScriptAdmin 35, PSNpc 32.
- **Behaviour:** The remaining AI command primitives: generic action/animation commands (`_AI_Action`, loop and change variants, `_AI_AniStateAction`), item commands (hold, hold-inventory, enjoy, equip-inventory-weapons, select weapon, take, transfer, use, use-inventory, unlock), `_AI_HUD_OpenTutorial`, and the `ZS_NavEval` navigation test state (a developer-only state, can be skipped). Thin wrappers over `PSInventory`/`PSInteraction`/animation; the weapon selection logic ties into the hand-item table (COMBAT_AI_RULES 2.12).
- **Functions:** _AI_Action, _AI_LoopAction, _AI_ChangeAction, IsInteractAction, _AI_AniStateAction, IsInteractAniStateAction, _AI_EnjoyItems, _AI_Enjoy, _AI_EquipInventoryWeapons, _AI_HoldInventoryItems, _AI_HoldInventoryItems_Base, _AI_HoldLeftInventoryItem_Base, _AI_HoldRightInventoryItem_Base, _AI_HoldItems, _AI_HUD_OpenTutorial, _AI_SelectWeapon*, _AI_TakeItem*, _AI_TransferItem*, _AI_Unlock, _AI_UseInventoryItem*, _AI_UseItem, ZS_NavEval, ZS_NavEval_Loop. (`*` = cited in a rule doc.)

### B14: Engine lifecycle callbacks and engine queries

- **Scope:** 26 registered functions, 13 private helpers, 36312 bytes of machine code. Existing docs: 6 by address, 2 by name, 18 uncovered.
- **Rule-doc sections citing these by address:** COMBAT_AI_RULES §5.2 (4), COMBAT_AI_RULES §2.13 (3), COMBAT_AI_RULES §9.2 (1), COMBAT_AI_RULES §11.1 (1), COMBAT_AI_RULES §2.12 (1), COMBAT_AI_RULES §99 (1).
- **Calls into:** B01 (42 call edges), B00 (17 call edges), B02 (7 call edges), B06 (2 call edges), B03 (2 call edges), B20 (1 call edges), B08 (1 call edges), B11 (1 call edges), B15 (1 call edges), B07 (1 call edges).
- **Host API used (Script/Game/Engine dll classes by call sites):** Entity 717, PSEnclave 113, PSNpc 110, PSRoutine 85, gCScriptProcessingUnit 52, Template 51, PSInventory 48.
- **Behaviour:** Engine-facing lifecycle and query callbacks the host calls into: `OnInit`, `OnGameStartUp`, `OnTick` (regeneration per COMBAT_AI_RULES 5.2, `OnExit_Gorn`, enclave sector refresh and music situation handling), `OnEnterProcessingRange` with `Respawn`/`RefreshHitPoints`/`RefreshWeaponry`/`EquipWeaponry`, `OnExitProcessingRange`, `OnEnterArea`/`OnLeaveArea`, `OnProcessRoutine`, `OnResetPlayer`, `OnReturnFromMenu`, `OnPlayerScriptFinished`, plus engine queries (`GetAniStateByAction`, `GetCombatMoveLength`, `GetDCCRadius`, `GetFocusEntityPriority`, `IsFirstPersonCameraAllowed`) and the magic-barrier enter/leave/touch callbacks. This is the contract surface of `ScriptCtx`: the Rust host must call these at the same events, so port the signatures first and the bodies incrementally.
- **Functions:** OnEnterMagicBarrier, OnLeaveMagicBarrier, OnToucheBarrier, GetAniStateByAction, GetCombatMoveLength*, GetDCCRadius, GetFocusEntityPriority, IsFirstPersonCameraAllowed, OnEnterArea, OnEnterProcessingRange*, RefreshHitPoints*, RefreshWeaponry, EquipWeaponry, Respawn*, OnExitProcessingRange, OnGameStartUp*, OnInit*, OnLeaveArea, OnPlayerScriptFinished, OnProcessRoutine*, OnResetPlayer, OnReturnFromMenu, OnTick*, OnExit_Gorn, OnEnclaveSectorRefresh, HandleMusicSituation. (`*` = cited in a rule doc.)

### B15: Player core: normal state, input interface, sleep, ghost, inventory setup

- **Scope:** 28 registered functions, 3 private helpers, 33042 bytes of machine code. Existing docs: 1 by address, 5 by name, 22 uncovered.
- **Rule-doc sections citing these by address:** COMBAT_AI_RULES §11.5 (1).
- **Calls into:** B00 (72 call edges), B01 (6 call edges), B06 (5 call edges), B19 (3 call edges), B20 (2 call edges), B03 (2 call edges), B18 (2 call edges), B17 (1 call edges), B07 (1 call edges), B02 (1 call edges), B11 (1 call edges).
- **Host API used (Script/Game/Engine dll classes by call sites):** Entity 690, Template 257, PSInventory 188, PSRoutine 82, gCScriptProcessingUnit 59, PSCharacterControl 45, PSFocus 21.
- **Behaviour:** Player baseline: `PS_Normal` (and its take, jump, sneak sub-states, quick use, loot approach), the input interface (`OnPlayerGameKeyPressed` and ten action/movement/selection handlers that translate key events into state changes), the dusk-to-dawn sleep time-skip state, the ghost and ghost-summoner states, and startup inventory setup (`InventoryAssureStartupItems`, `InventoryPopulate`). Sneak and sleep rules are in COMBAT_AI_RULES 11.5; the key handlers decide which `PS_*` state to enter per held weapon mode.
- **Functions:** OnPlayerChanged, OnPlayerGameKeyPressed, OnPlayerMovement, OnPlayerAction, OnPlayerSecondaryAction, OnPlayerWalk, OnPlayerSneak, OnPlayerJump, OnPlayerSelectWeapon, OnPlayerConfirm, OnPlayerUseQuickSlot, OnPlayerPotionQuickUse, PS_DuskToDawn*, PS_DuskToDawn_Loop, PS_Ghost*, PS_Ghost_Loop, PS_Ghost_Jump, PS_GhostSummoner*, PS_GhostSummoner_Loop, InventoryAssureStartupItems, InventoryPopulate, PS_Normal*, PS_Normal_Loop, PS_Normal_Take, PS_Normal_Jump, PS_Normal_Sneak, PS_QuickUse*, PS_GonnaLoot*. (`*` = cited in a rule doc.)

### B16: Player combat: melee and ranged states

- **Scope:** 29 registered functions, 1 private helpers, 24194 bytes of machine code. Existing docs: 2 by address, 4 by name, 23 uncovered.
- **Rule-doc sections citing these by address:** COMBAT_AI_RULES §9.6 (2).
- **Calls into:** B00 (77 call edges), B15 (9 call edges), B02 (1 call edges).
- **Host API used (Script/Game/Engine dll classes by call sites):** Entity 564, gCScriptProcessingUnit 65, PSRoutine 62, PSNpc 31, PSInventory 30, PSFocus 26, gCScriptAdmin 18.
- **Behaviour:** Player melee and ranged states. `PS_Melee*` map input to the `_AI_*` attack commands (B09) and handle parade, jump-back, finishing and sneak; `PS_Ranged*` cover walking while aiming, draw and power aim (draw-time damage multiplier is documented in COMBAT_AI_RULES 9.6), shoot, reload, crossbow and sneak, plus `OnGetAimAngle` and `Shoot`. Aim-angle and camera interplay need the camera host API.
- **Functions:** PS_Melee*, PS_Melee_Loop, PS_Melee_Attack, PS_Melee_SimpleWhirl, PS_Melee_PowerAttack, PS_Melee_QuickAttack, PS_Melee_Parade*, PS_Melee_JumpBack, PS_Melee_WhirlAttack, PS_Melee_PierceAttack, PS_Melee_HackAttack, PS_Melee_FinishingAttack, PS_Melee_Jump, PS_Melee_Sneak, OnGetAimAngle, PS_Ranged*, PS_Ranged_Walk, PS_Ranged_Walk_Loop, PS_Ranged_Aim, PS_Ranged_PowerAim*, OnPowerAim_Loop*, PS_Ranged_Aim_Loop, PS_Ranged_Shoot, PS_Ranged_Aim_Reload, PS_Ranged_Reload, PS_Ranged_Jump, PS_Ranged_Sneak, PS_Ranged_CrossBowSneak_Loop, Shoot*. (`*` = cited in a rule doc.)

### B17: Player magic casting and book stands

- **Scope:** 23 registered functions, 1 private helpers, 26934 bytes of machine code. Existing docs: 0 by address, 3 by name, 20 uncovered.
- **Calls into:** B00 (94 call edges), B01 (5 call edges), B15 (3 call edges), B20 (1 call edges).
- **Host API used (Script/Game/Engine dll classes by call sites):** Entity 503, gCScriptProcessingUnit 59, PSRoutine 59, gCScriptAdmin 39, PSFocus 38, PSPlayerMemory 32, PSInventory 31.
- **Behaviour:** Player magic: `PS_Magic` states (quick cast, cast, power cast, flame sword, summon with abort, staff parade, reload) and `PS_MagicPotion`, `OnMagicHandleInput`, plus the eight book-stand states (`BS_*`: the ancient wisdom, flood, Rhobar, Zuben, Chronicles, alchemy and smith books) that grant permanent bonuses when a book is read. Cast-phase mana accounting is documented (COMBAT_AI_RULES 9.7); the book effects are not and should be read per book.
- **Functions:** BS_NothingNew, BS_AncientWisdom, BS_BookOfFlood, BS_BookOfRhobar, BS_BookOfZuben, BS_Chronicles, BS_Alchemy, BS_Smith, PS_Magic*, PS_Magic_Loop, PS_Magic_QuickCast, PS_Magic_Cast*, PS_Magic_PowerCast, PS_Magic_FlameSword, PS_Magic_Summon, PS_Magic_Summon_Abort, PS_Magic_StaffParade, PS_Magic_StaffParade_Loop, PS_Magic_Reload, PS_Magic_Jump, PS_Magic_Sneak, OnMagicHandleInput, PS_MagicPotion*. (`*` = cited in a rule doc.)

### B18: Player world interaction (chests, doors, workstations, mobs)

- **Scope:** 53 registered functions, 1 private helpers, 45707 bytes of machine code. Existing docs: 1 by address, 3 by name, 49 uncovered.
- **Rule-doc sections citing these by address:** COMBAT_AI_RULES §11.5 (1).
- **Calls into:** B00 (116 call edges), B01 (12 call edges), B06 (6 call edges), B19 (3 call edges), B13 (1 call edges).
- **Host API used (Script/Game/Engine dll classes by call sites):** Entity 849, eCLocString 114, gCScriptProcessingUnit 114, PSRoutine 75, Template 72, gCScriptAdmin 55, PSInteraction 53.
- **Behaviour:** Player world interaction: the `PS_Interact*` family for chests, doors, anvil/forge/grindstone/cauldron/barbecue/alchemy tables, shrine, ore, bookstand, stone plate, saw log, orc drum, water barrel, pirate treasure, sitting (throne, stool, bench), waterpipe, campfire, sleeping (`PlayerSleep`) and the orc-boulder tilt/hold/lift trio, stone carrying and talk. Requirements per object are in COMBAT_AI_RULES 11.5; the actual loops (animation selection, loot generation like `GeneratePickOreInventory`, boulder weight classes `OB_Light/Medium/Heavy`) are mostly uncovered and mechanical.
- **Functions:** CanOpen, PS_Interact, StartSayNothingToGet, StartSayDoesntWork, PS_Interact_WaitForExit, PS_Interact_End, PS_Interact_Loop, OnInteract_Loop*, OnInteract_HUD_Loop, PS_Interact_Chest*, PS_Interact_Chest_Loop, PS_Interact_Door, PS_Interact_Anvil, PS_Interact_Forge, PS_Interact_GrindStone, PS_Interact_Cauldron, PS_Interact_Barbecue, PS_Interact_Alchemy, PS_Interact_Shrine, PS_Interact_PickOre, GeneratePickOreInventory, PS_Interact_Bookstand, PS_Interact_Stoneplate, PS_Interact_SawLog, PS_Interact_OrcDrum, PS_Interact_DrinkWaterBarrel, PS_Interact_PirateTreasure, PS_Interact_SitThrone, PS_Interact_SitStool, PS_Interact_SitBench, PS_Interact_UseWaterpipe, PS_Interact_Campfire, PS_Interact_SleepBed*, PS_Interact_SleepGround, PlayerSleep*, PS_Interact_TiltOrcBoulder, PS_Interact_TiltOrcBoulder_Loop, OnInteract_TiltOrcBoulder_Loop, PS_Interact_HoldOrcBoulder, PS_Interact_HoldOrcBoulder_Loop, OnInteract_HoldOrcBoulder_Loop, PS_Interact_LiftOrcBoulder, PS_Interact_LiftOrcBoulder_Loop, OnInteract_LiftOrcBoulder_Loop, OB_Light, OB_Medium, OB_Heavy, OB_Faring_Boulder, PS_Interact_CarryStone, PS_Interact_CarryStone_Loop, PS_Interact_CarryStone_End, PS_Interact_Talk, IsBasket. (`*` = cited in a rule doc.)

### B19: Player HUD, inventory use and crafting UI states

- **Scope:** 36 registered functions, 5 private helpers, 20113 bytes of machine code. Existing docs: 4 by address, 2 by name, 30 uncovered.
- **Rule-doc sections citing these by address:** COMBAT_AI_RULES §2.5 (1), COMBAT_AI_RULES §8.3 (1), COMBAT_AI_RULES §8.4 (1), COMBAT_AI_RULES §8.1 (1).
- **Calls into:** B00 (44 call edges), B02 (1 call edges).
- **Host API used (Script/Game/Engine dll classes by call sites):** Entity 500, PSInventory 90, gCScriptProcessingUnit 73, GUI2 48, eCLocString 48, PSRoutine 32, Template 18.
- **Behaviour:** Player HUD and inventory-use states: `PS_HUD` loop with equip/unequip/use, stack operations (use, fry, transform, synthesise recipe, assign quick key), `PS_StopForMenu`, the `PlayerOpen*` window openers (dialog, inventory, trade, map, quests, skills, spells, alchemy, smithing, poison, bless, fry, book, letter, loot), and the economy hooks `OnPlayerGetGoldCost`, `OnWantsToSell`, `OnPlayerCanTransfer`, `GetQualityBonus` which are documented (COMBAT_AI_RULES 8). The openers are thin UI calls; the economy hooks are the important part.
- **Functions:** PS_HUD*, PS_HUD_Loop, PS_HUD_Equip, PS_HUD_UnEquip, PS_HUD_Use, OnHUDHandleInput, OnPlayerUseStack, OnPlayerFryStack, OnPlayerTransformStack, OnPlayerSynthRecipe, OnPlayerGetDamage, GetQualityBonus*, OnWantsToSell*, OnPlayerCanTransfer*, OnPlayerGetGoldCost*, OnPlayerGetEquipBonus, OnPlayerAssignStackKey, PlayerOpenBook, PlayerOpenDialog, PlayerOpenInventory, PlayerOpenLetter, PlayerOpenLoot, PlayerOpenMap, PlayerOpenQuests, PlayerOpenSkills, PlayerOpenSpells, PlayerOpenTrade, PlayerOpenAlchemy, PlayerOpenSmithing, PlayerOpenPoison, PlayerOpenBless, PlayerOpenFry, OnToggleQuickSlots, HasRecipes, HasBladeWeapon, PS_StopForMenu*. (`*` = cited in a rule doc.)

### B20: Magic spells (cast effects per school)

- **Scope:** 51 registered functions, 3 private helpers, 25256 bytes of machine code. Existing docs: 7 by address, 8 by name, 36 uncovered.
- **Rule-doc sections citing these by address:** COMBAT_AI_RULES §9.7 (3), COMBAT_AI_RULES §2.5 (2), COMBAT_AI_RULES §1.1 (1), COMBAT_AI_RULES §9.5 (1).
- **Calls into:** B00 (26 call edges), B01 (18 call edges), B02 (10 call edges), B06 (10 call edges), B18 (3 call edges), B07 (3 call edges), B12 (1 call edges).
- **Host API used (Script/Game/Engine dll classes by call sites):** Entity 850, gCScriptProcessingUnit 102, PSNpc 56, Template 39, PSRoutine 38, PSInteraction 25, PSParty 25.
- **Behaviour:** Spell effect functions by school: generic `GetCastAction`, `MagicInstant`/`MagicProjectile`/`MagicSummon`, Adanos (dispel, cures, animal form, call animals, hypnosis, sleep), Beliar (control, tame, summon demon/skeleton/golem/goblin/army/monster/companion, bloodlust, fear, telekinesis, soul travel, time bubble, charm), Innos (heal other, open locks) and misc (burn, freeze, poison, disease, refresh, teleport, transform, speed up, stun, keep-stamina) with their duration/bonus rules (COMBAT_AI_RULES 9.7 for the documented subset). Spell numbers are template data; this code is the glue that applies status bits and spawns summons.
- **Functions:** GetCastAction*, MagicInstant*, MagicProjectile*, MagicSummon*, MagicDispel, MagicCure, MagicCurePoison, MagicCureDisease, MagicAnimalForm, SendPerceptionTransform, MagicCallAnimals*, MagicHypnosis, MagicSleep, CanControl, MagicWordOfControl, MagicTameAnimal, MagicSummonDemon, MagicSummonSkeleton, MagicSummonGolem, MagicSummonGoblin, MagicSummonArmyOfDarkness, MagicSummonMonster, MagicSummonCompanion, MagicBloodlust, MagicFear*, MagicTelekinesis, MagicSoulTravel, MagicTimeBubble, MagicCharm, MagicSummonArmyOfCuteness, MagicHealOther, MagicOpenLocks, MagicBurn, MagicFreeze, MagicPoison, MagicDisease, MagicRefresh, MagicTeleport, MagicTransform, IsAnimal*, CanAttack*, IsBig, IsLarge, IsGame*, GetPlayerSpellDurationBonus*, GetTransformDuration*, ReturnToBody*, TransformBack, MagicSpeedUp*, MagicKeepStamina*, MagicStun*. (`*` = cited in a rule doc.)

### B21: Magic states: summoned/area effects, freeze, sleep, hypnosis

- **Scope:** 45 registered functions, 0 private helpers, 22144 bytes of machine code. Existing docs: 1 by address, 4 by name, 40 uncovered.
- **Rule-doc sections citing these by address:** COMBAT_AI_RULES §9.7 (1).
- **Calls into:** B00 (24 call edges), B11 (13 call edges), B20 (8 call edges), B02 (7 call edges), B06 (3 call edges), B03 (2 call edges), B12 (1 call edges).
- **Host API used (Script/Game/Engine dll classes by call sites):** Entity 485, PSRoutine 114, gCScriptProcessingUnit 108, PSInteraction 24, PSNpc 21, gCScriptAdmin 8, PSEffect 6.
- **Behaviour:** Area-of-effect and status states: summoned effect states (hailstorm, firerain, heatwave, frostwave, meteor, lightning, icebomb, light, time bubble, decay) with the shared `MagicAreaDamageTick` (documented tick/hit chance, COMBAT_AI_RULES 9.7), mist states (`ZS_Mist`, `ZS_Misted`, no-aggro variants, `AssessMist`), freeze, magic sleep, hypnosis, stun, wait-for-meteor and wait-for-dawn. All run on top of `AssessHit` (B07) and the status-effect bit set.
- **Functions:** ZS_Freeze*, ZS_Freeze_Loop, ZS_MagicSleep, ZS_MagicSleep_Loop, ZS_MagicHypnosis, ZS_MagicHypnosis_Loop, ZS_WaitForMeteor, MagicAreaDamageTick*, ZS_Decay, ZS_Hailstorm, ZS_Hailstorm_FadeIn, ZS_Hailstorm_FadeIn_Loop, ZS_Hailstorm_Loop, ZS_Hailstorm_FadeOut_Loop, ZS_Firerain, ZS_Firerain_Loop, ZS_Heatwave, ZS_HeatWave_Loop, ZS_Frostwave, ZS_FrostWave_Loop, ZS_Meteor, ZS_Meteor_Loop, ZS_Lightning, ZS_Lightning_Loop, ZS_IceBomb, ZS_IceBomb_Loop, ZS_Light, ZS_Light_Loop, ZS_Timebubble, ZS_Timebubble_Loop, MagicMistTick, ZS_Mist, ZS_Mist_Loop, ZS_Mist_End, AssessMist*, ZS_Misted*, ZS_Misted_Loop, MagicMistTickNoAggro, ZS_Mist_NoAggro, ZS_Mist_NoAggro_Loop, AssessMistNoAggro, ZS_Stunned*, ZS_Stunned_Loop, ZS_Stunned_End, ZS_WaitForDawn. (`*` = cited in a rule doc.)

### B22: Info-script engine states (dialog gating and command executor)

- **Scope:** 14 registered functions, 1 private helpers, 9567 bytes of machine code. Existing docs: 2 by address, 12 by name, 0 uncovered.
- **Rule-doc sections citing these by address:** GAME_RULES §3 (1), COMBAT_AI_RULES §10.5 (1).
- **Calls into:** B00 (27 call edges), B02 (3 call edges).
- **Host API used (Script/Game/Engine dll classes by call sites):** Entity 351, PSInfoScriptCommand 174, eCLocString 39, PSInfoManager 32, gCScriptProcessingUnit 31, PSEnclave 30, PSNpc 28.
- **Behaviour:** The info-script (dialog) engine states: gating states that refuse a dialog (no party, not enough gold, not enough learning points, no learn requirements, no political or enclave reputation, closed/trade idle), `IS_Running_SelectInfo` and the command executor `IS_Running_ProcessInfo` (a 69-case jump table, specified in GAME_RULES 3), with `HasGuardNews`/`HasBossNews`/`HasCrimeRefuse`/`HasHealInfo`. Small but central to quests and dialog; port with `crates/dialog` and the quest engine.
- **Functions:** IS_Idle_Closed*, IS_Idle_Trade*, IS_Running_NoEnclaveReputation*, IS_Running_NoLearnRequirements*, IS_Running_NoParty*, IS_Running_NoPoliticalReputation*, IS_Running_NotEnoughGold*, IS_Running_NotEnoughLP*, IS_Running_ProcessInfo*, HasGuardNews*, HasBossNews*, HasCrimeRefuse*, HasHealInfo*, IS_Running_SelectInfo*. (`*` = cited in a rule doc.)

## 5. How to use the local evidence

- `.local/research/script_game/index.json`: one record per function with `name`, `category`, `module`, `size`, `callees`, `calledBy`, `importsCalled` (mangled import -> static call sites), `rulesDoc` (doc sections that cite it) and `coverage`. `imports.json` / `imports_parsed.json` hold the import catalogue behind `SCRIPT_ENGINE_INTERFACE.md`.
- `.local/research/script_game/<category>/<name>.c`: untouched Ghidra pseudo-code with a header (address, size, source file). Categories: `routine`, `ai_state`, `ai_command`, `callback`, `decision`, `memory`, `perception`, `magic`, `player_state`, `player_interface`, `info_command`, `helper`, `runtime_support`. Parameter and local names are auto-generated; the *names of called APIs* are accurate (they come from the import table).
- Regenerate: `build_index.py`, `batches.py`, `iface.py`, `gen_iface_doc.py`, `gen_plan.py` in the same directory (Ghidra dump script in `ghidra_scripts/`). Reading tip: start from `index.json` and open the function plus its `callees`; the existing `.local/research/rules/` disassembly tools are still the way to resolve *which* entity a comparison uses when the decompiler merges stack slots.

## 6. Where the largest knowledge gaps are

| Batch | Uncovered functions | Uncovered bytes |
| --- | ---: | ---: |
| B05 | 43 | 51475 |
| B18 | 49 | 42615 |
| B04 | 22 | 30809 |
| B15 | 22 | 25589 |
| B09 | 35 | 25410 |
| B08 | 34 | 24569 |
| B12 | 30 | 24410 |
| B14 | 18 | 22735 |
| B10 | 37 | 21114 |
| B13 | 19 | 20535 |

The big undocumented chunks are the routine work-station tasks (B04, B05), the player interaction/HUD states (B18, B19) and the spell glue (B20, B21); all are repetitive, so a per-batch table extracted from the pseudo-code (task -> steps -> animation/use-type) should cover them cheaply. The rule-heavy batches (B01, B02, B06, B07, B08) are the opposite: well documented, so the work is verification and tests rather than discovery.
