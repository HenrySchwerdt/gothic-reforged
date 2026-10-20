# Gothic 3 game-logic map (quests, infos, script registry)

Recorded 2026-10-20 against the installed 1.75.20108.14 build (read-only source `D:\SteamLibrary\steamapps\common\Gothic 3`). Scope: static analysis only; nothing was executed. No original content or decompiled bodies are reproduced here; this file holds names, addresses, layouts and prose. Machine-readable output lives in gitignored `.local/research/`:

- `class-schemas.json` - 258 property-bearing classes (113 Game.dll, 145 Engine.dll), 1551 property members, 120 enums with 1124 enumerators.
- `script-registry.json` - 874 registered script entries from `scripts/Script_Game.dll`, the 68-entry info-script command table, and observed command usage in `Infos.pak`.
- `gl/` - helper scripts (`tools/*.py`), extracted string tables, survey JSON (`Infos-values.json`, `Quests-values.json`, ...). Re-runnable; they need `capstone` (installed only into the session scratchpad).
- `scriptdll-evidence.json` - REA evidence bundle (3 records) for Script.dll.

## Evidence legend

- **[V]** verified: deterministic byte-level extraction from the binaries (PE export tables, static-initialiser stubs disassembled with capstone, string tables) and, where possible, cross-checked against the shipped data files (`Quests.pak`, `Infos.pak`, `Workspace.pak`).
- **[R:ev_...]** REA/Ghidra evidence record (Script.dll only).
- **[I]** inferred from naming, call structure or log/error strings; not proven by reading control flow.
- **[U]** unknown.

REA status: Game.dll (4.6 MB, 8.5k exports) again could not be served by the Ghidra provider (first query timed out at provider start and the session dropped); it was not retried. Script.dll (459 KB) analysed fine. Ghidra evidence: `ev_fbb30790eb75f449d2ccbc6f6eae2354662ad51c79749efc8ac7a12a198b35cb` (`PSQuestManager::RunQuest`, 0x10011e30), `ev_6d489bf7c29e04d6ea3c97a3461b779c9003a98096cd06f52592bdfa58e3b69a` (`PSInfoManager::Give`, 0x10005a90), `ev_70652499227746c3428dff30edd72284c0a2f5f68c9895f2d113582327efbd81` (`PSQuestManager::OnNPCKilled`, 0x10012110). All three show the same fact: the Script.dll `PS*` classes are thin null-checking forwarders to the Game.dll property sets (`gCQuestManager_PS`, `gCInfo_PS`, ...). Everything else below comes from the non-REA static pipeline (scripts in `.local/research/gl/tools`); no Game.dll control flow was decompiled.

## 1. DLL roles

| File | Bytes | Exports | Role |
| --- | ---: | ---: | --- |
| `Gothic3Final.exe` | 196608 | 0 | Process entry; imports SharedBase, Importer, Game, Engine, fmodex. |
| `SharedBase.dll` | 937984 | 4765 | `bC*` base library: strings, streams, property-object framework (`bCPropertyObjectBase`, `bTPropertyContainer`, `bTObjArray`). |
| `Engine.dll` | 6901760 | 16224 | `eC*` engine: entities, property sets (visual, collision, particles, triggers, audio), resources, renderer, physics glue. 145 property classes, 717 properties. |
| `Game.dll` | 4640768 | 8569 | `gC*` game logic: quests, infos, NPC, inventory, session, world, navigation, script admin, HUD. **All quest/info/inventory/NPC rules live here.** 113 property classes, 834 properties. |
| `Script.dll` | 458752 | 2141 | C++ **script API** (`Entity`, `Template`, `World`, `PS*` wrappers, `Info`, `GUI`, `Music`, `EffectSystem`). Thin forwarders into Game/Engine for use by the script DLL [R:ev_fbb3...]. |
| `scripts/Script_Game.dll` | 1265664 | 1 (`_ScriptInit@0`) | The **compiled game script**: AI states `ZS_*`, player states `PS_*`, routines `Rtn_*`, combat/magic/ambient logic, info-script command dispatch, perks. Loaded by `gCScriptAdmin::LoadScriptDLL` from `.\scripts`. Not in the earlier PE inventory; sha256 `cdd9c46120abac49ebb6d53e5bed79512a528e5940238d6d88d150fe9aef3cdc`. |
| `FileSystem.dll` | 176128 | 41 | PAK/volume mounting (see NATIVE_FINDINGS). |
| `Importer.dll` | 667648 | 241 | Asset importers. |
| `GUI.dll` / `GFC.dll` | 471040 / 155648 | 73 / 549 | Widget layer (`CFFGUIModule`, `CFFGFC*` window classes). |
| `SCM.dll` | 454656 | 33 | `GECSCMModule` source-control module (WSOCK32); tooling, not gameplay. |
| `Dialogs.dll` | 278528 | 12 | Error/memory dialogs; tooling. |

[V] Quest/info/dialog logic is in **Game.dll** (`gCQuest_PS`, `gCQuestManager_PS`, `gCInfo_PS`, `gCInfoManager_PS`, `gCInfoScript_PS`, `gCInfoScriptLine_PS`, `gCDialog_PS`, `gCDialogManagerCtrl`, `gCQuestLogCtrl`). Quests and infos are **data**, not code. Script_Game.dll contributes the info-state machine (`IS_*`), the info-script command executor and gameplay reactions.

### Script host architecture [V names, I semantics]

`gCScriptAdmin` (Game.dll) has five registries and entry points `RunScriptState` (RVA 0x1603d0), `RunScriptFunction` (0x1604e0), `RunScriptCallback` (0x160610), `RunScriptRoutine` (0x160720), `RunScript` (0x160830), `LoadScriptDLL` (0x160c30). Error strings name them "state", "ai function", "callback", "routine function", "script". `_ScriptInit` returns a 15-dword registry struct (5 x pointer/count/capacity) at VA 0x1011a1b0 filled by static initialisers. Each of the 874 registrations is one static-init stub that calls one of five helper functions; the helper-to-category mapping is [I] from naming:

| Helper VA (Script_Game) | Entries | Category (inferred) | Examples |
| --- | ---: | --- | --- |
| 0x10012880 | 384 | AI/player/info **states** | `ZS_Attack`, `ZS_Attack_Loop`, `ZS_Sleep_Bed`, `PS_Interact_Chest`, `PS_Melee_Parade`, `PS_Magic_Cast` |
| 0x1000dfd0 | 387 | **script functions** (queries/helpers/event handlers) | `CanParade`, `GetAttitude`, `GiveXP`, `Perk_Learn`, `IsGuard`, `OnHit`, `OnDamage`, `OnInit` |
| 0x100127f0 | 45 | state **callbacks** | `OnAttack_Goto`, `OnFlee_GoBack`, `OnInteract_Loop`, `OnGoto`, `OnWork`, `OnSleep` |
| 0x10037d90 | 55 | **AI commands** (queued actions) | `_AI_Say`, `_AI_SaySVM`, `_AI_Attack`, `_AI_Cast`, `_AI_TakeItem`, `_AI_TransferItem` |
| 0x10086b10 | 3 | **routines** | `Rtn_Monster`, `Rtn_Dead`, `Rtn_Player` |

Each stub also carries the original source path (e.g. `.\Script\AI\AI_Commands\GiveXP.cpp`), so the module layout is recoverable from `script-registry.json` (`sourceFile`): AI 380 entries, Player 169, Routine 135, Magic 114, Engine 66, Enclave 8, Test 2. Ambient, trade, crime, guard and combat behaviour is **compiled C++ that must be re-authored in Rust**; it is not interpretable data.

## 2. Property-object / schema model [V]

Game classes derive from `bCObjectRefBase` (plain property objects such as quests and infos) or `eCEntityPropertySet` (components attached to entities, `*_PS`). Each class exports `ms_PropertyObjectInstance_<Class>` (its template argument gives the base), `GetVersion` (a constant: gCQuest_PS = 3, gCInfo_PS = 1, gCInfoScript_PS = 2, gCNPC_PS = 79) and, for entity property sets, `GetPropertySetType` returning an `eEPropertySetType` value (NPC=30, Inventory=31, QuestManager=53, Quest=54, InfoManager=55, Info=56, InfoScript=57, InfoScriptLine=58, ... 108 enumerators in `class-schemas.json`; the values returned by the `GetPropertySetType` functions match the enum order, which independently validates the enum extraction).

Every property is a static `ms_PropertyMember_m_<field>` whose export name encodes the C++ type (`bCString`, `u32`, `bTPropertyContainer<enum gEQuestType>`, `bTObjArray<bCString>`, `bTValArray<u32>`, `eCEntityProxy`, `eCLocString`, ...). One static-initialiser stub per property registers the **property name** (the string used in GENOMFLE property streams). Three registration shapes were decoded:

1. Name plus immediate (`push name; ...; push N; mov esi, PROP`): N is the member offset inside the C++ object (monotone and field-size consistent, e.g. gCQuest_PS RunningTimeYears/Days/Hours = 124/128/132) [V extraction, I meaning "offset"].
2. Name via a shim function (`push name; lea eax; call shim`, shim does `push kind; mov ecx, PROP`): kind 1/2/3 observed (plain/array, enum container, object) [I].
3. Description/category/name triple plus two flags (`push desc; push category; push name`): categories seen include `Internal`, `Status`, `Script`, `Common`, `Readonly`, `Precipitation`, `Rendering`, `Audio` [V].

Coverage: names resolved for 743 of 834 Game properties and 501 of 717 Engine properties; the rest keep only the member-derived name (`name: null` in JSON). Registered property names can differ from INI keys (the INI files are written by hand-coded `Read/Write(bCString)`: property `ConditionOwnerNearEntity` is INI key `CondOwnerNearEntity`; `ConditionPlayerKnows` is `CondPlayerKnows`; `CondSecondaryNPCStatus` is `CondSecondaryNPCstates`) [V].

Six enums referenced by properties are not enumerable statically (`EAttribModOperation`, `EAttribReqOperation`, `bENoiseTurbulence`, `eERigidbody_Flag`, `gEDirection`, `gESession_State`).

## 3. Quests

### 3.1 Data and loading [V]

- Quest files: `Data/Quests/<Folder>/<Name>_quest_<World>.quest` (`Quests.pak`: 570 INI files, all `[Quest]`). Format string in `gCQuestManager_PS::LoadQuestsFromInis` (RVA 0x15e610): `%s/%s_quest_%s.quest`; duplicate names are logged and rejected; `-noquests` skips loading. Runtime state is stored separately (`ReadRuntimeData` / `WriteRuntimeData`; savegame packets are matched to quests by name; unknown names stream into a temporary quest).
- Observed quest INI keys: on all 570 files `Name, Type, FinishedQuests, LogTopic, ExperiencePoints, PoliticalSuccess, PoliticalSuccessAmount, EnclaveSuccess, EnclaveSuccessAmount, Folder, DeliveryEntities, DeliveryAmounts, DeliveryCounter, DestinationEntity, RunningTimeYears/Days/Hours, AttribSuccess, AttribSuccessAmount`; `LogText` 261; `JobSuccess` and `JobSuccessAmount` 541 (legacy keys without a 1.75 property); `SpellName` and `QuestDescription` once each. Registered but not in shipped files: `Status`, `ActivationTime_Year/Day/Hour`, `EnclaveFailure(+Amount)`, `PoliticalFailure(+Amount)`.
- Arrays are `;`-separated; `DeliveryEntities`, `DeliveryAmounts` (required counts) and `DeliveryCounter` (runtime progress) must have equal length (error strings check this).
- Rewards: `ExperiencePoints` (e.g. 300); `PoliticalSuccess` (`gEPoliticalAlignment`, observed None 391, Reb 64, Orc 30, Nrd 30, Ass 25, Nom 22, Mid 8) with signed amount; `EnclaveSuccess` is an enclave name with amount; `AttribSuccess` is a short attribute tag (STR, DEX, INT, ALC, SMT, THF, HP, MP, SP) with amount.

### 3.2 State machine

`gEQuestStatus`: Open=0, Running=1, Success=2, Failed=3, Obsolete=4, Cancelled=5, Lost=6, Won=7 [V]. `gCQuest_PS::SetStatus(gEQuestStatus)` (RVA 0x157840) logs `Quest %s (%s) changed to status = <X>` for Running/Success/Failed/Obsolete/Cancelled/Lost and triggers game-output text keys `GO_QuestRunning`, `GO_QuestTimeLimit`, `GO_QuestSuccess`, `GO_QuestFailure`, `GO_QuestCancel` (string-table keys; HUD keys `HUD_Quest_*`, icons for Won/Lost/Cancelled/Obsolete/Failed/Success/Running/Open) [V names]. Manager controls: `RunQuest`, `SucceedQuest`, `FailQuest`, `CloseQuest` (plus `PSQuestManager::CancelQuest`); predicates `IsOpen/IsRunning/IsSucceeded/IsFailed/IsFinished/IsObsolete/IsCancelled`. `gCQuest_PS` also has `IsInFinalState`, `IsFinished`, `IsOvertime` (time limit from running time Years/Days/Hours), `AreFinishedQuestsFinished` (the `FinishedQuests` prerequisite list) and `AreInfosForFinishedQuestsAvailable` [V names]. Exact transition rules (legal source states, whether `CloseQuest` maps to Obsolete or Cancelled, overtime to Failed) are **[U]**: decompile `SetStatus` and `OnProcess` next.

### 3.3 Types and completion events

`gEQuestType` [V]: HasItems=0, Report=1, Kill=2, Defeat=3, DriveAway=4, Arena=5, BringNpc=6, FollowNpc=7, EnterArea=8, FreeEnclave=10, Plunder=11, Sparring=12, SpellCast=13 (**9 is unused**). INI `Type=` uses these numbers; observed counts: HasItems 157, Report 130, Kill 114, FollowNpc 57, Arena 35, FreeEnclave 31, BringNpc 20, Defeat 10, EnterArea 8, Plunder 4, DriveAway 3, SpellCast 1.

Validation strings [V text, I rule]: HasItems/Plunder need `DestinationEntity` with a `gCInventory_PS`; Arena/Sparring need `DestinationEntity` with `gCArena_PS`; BringNpc needs `DestinationEntity` with `gCNPC_PS`; delivery entities must be `gCNPC_PS` for NPC-based types; EnterArea needs delivery entities (areas; `OnEnter`/`OnLeave`); FreeEnclave needs exactly 1 delivery entity (`OnEnclaveStateChanged`); SpellCast uses `SpellName` (`OnSpellCast`).

Engine-to-quest event hooks (Game.dll RVAs) [V]: `OnReceiveItem` 0x158c70 (called from `gCInventory_PS::NotifyQuestManager`, 0xb4880), `OnNPCKilled` 0x159110, `OnNPCDefeated` 0x159490, `OnEnter` 0x158e20 / `OnLeave`, `OnSpellCast` 0x15acc0, `OnEnclaveStateChanged` 0x159710, `CheckDeliveryEntitiesStatus` 0x156bb0. Script.dll exposes the same set as `PSQuestManager::On*` forwarders [R:ev_7065...].

### 3.4 Quest log [V names]

`gCQuestLogCtrl` renders `ListQuestNames/States/Types/LogTopics`; `LogTopic` is a localised string key (e.g. `QUEST_FILLER60`), `LogText` entries are `SLogEntry` structs; `AddQuestLog` is an info-script command (quest name plus text id); `Folder` groups quests (`Varant/Braga/`, `Hauptstory/`).

## 4. Infos (dialogues)

### 4.1 Data [V]

- Files: `Data/Infos/<Folder>/<Name>_info_<World>.info` (`Infos.pak`: 4590 `[Info]` files). The manager can also read/produce `compiledinfos_%s.bin`; `-noinfos` skips loading.
- INI keys on all files: `Name, SortID, Owner, InfoGiven, Parent, Quest, ConditionType, InfoType, GoldCost, CondOwnerNearEntity, Folder, CondPlayerKnows, CondItemContainer, CondItems, CondItemAmounts, CondSecondaryNPC, CondSecondaryNPCstates, ClearChildren, InfoScript_Commands, InfoScript_Entities1, InfoScript_Entities2, InfoScript_IDs1, InfoScript_IDs2, InfoScript_Texts`; `Permanent` 4497; teaching `TeachSkill, TeachAttrib, TeachAttribValue` (178 each; `TeachSkillIsPerc/Index/Value` rare); `CondPlayerSkills` + `CondPlayerSkillValues` + `PlayerSkills*` (5), `CondPlayerKnowsNot` (3), `CondPlayerPartyMember` (1), `SuppressLog` (1). Registered but never seen in files: `CondWearsItem, CondHasSkill, CondReputGroup, CondReputRelation, CondReputAmount, CondPAL`.
- `Owner` is the NPC/template name or a special value (`PC_Hero`, `owner`, interactable statues); `Parent` names the parent info (child infos are the player's dialogue choices); `Quest` binds an info to a quest (required by quest-bound condition types); `InfoGiven` is runtime state; `Permanent` infos are not consumed; `ClearChildren` clears sibling choices; `SortID` orders the list. All shipped `InfoGiven` values are false.
- The info script is a table of parallel `;`-lists: `InfoScript_Commands`, `Entities1`, `Entities2`, `IDs1`, `IDs2`, `Texts`; one slot per command (17 of 4590 files have inconsistent column counts; tolerate this). `Texts` hold string-table ids (e.g. `BPANKRATZ31`) resolved from `Strings.pak`; missing ids are logged as errors.

### 4.2 Info types and condition types

`gEInfoType` [V]: Refuse=0, Important=1, News=2, Info=3\*, Parent=4, Comment=5, QuestRunning=7 (observed: Info 3808, News 538, Important 148, Parent 47, Refuse 27, Comment 22, QuestRunning 1).

`gEInfoCondType` (INI `ConditionType`) [V]; \* marks values inferred by sequence (the init stub used a different encoding); cross-check: General=3 is the most common value in the data and `End`=51 matches the dialog-closing infos:

`gEInfoCondType`: Crime=0, Fight=1, Hello=2, General=3\*, Overtime=4, Open=5, Activator=6, Running=7, Delivery=8, PartDelivery=9, Success=10, DoCancel=11, Failed=12, Cancelled=13, Join=14, Dismiss=15, Teach=16, Trade=17, PickPocket=18, Ready=19, Lost=20, Reactivator=21, Won=22, MasterThief=23, EnclaveFriend=24, PoliticalFriend=25, FirstWarn=26, SecondWarn=27, MobJoin=28, SlaveJoin=29, LongTimeNoSee=30, EnclaveCrime=31, PoliticalCrime=32, MobDismiss=33, Wait=34, Heal=35, Animal=36, Companion=37, NotFriend=38, NotFriendOrThief=39, NothingToSay=50, End=51, Back=52, StepBack=53

Observed usage (infos): General 1723, Activator 539, Success 505, Running 398, Delivery 349, Ready 302, Open 249, Teach 146, Back 90, Reactivator 90, PartDelivery 26, Hello 24, NothingToSay 23, DoCancel 21, EnclaveFriend 20, Trade 11, Cancelled 11, FirstWarn 11, Failed 10, Overtime 9, Dismiss 9, Lost 5; others at most 3.

Semantics are [I] from names and strings; **exact predicates are [U]**: decompile `gCInfo_PS::AreConditionsFulfilled` (RVA 0x1c3370) and `IsAvailable` (0x1c5f90). Working model: quest-bound conditions (Open, Running, Delivery, PartDelivery, Success, DoCancel, Failed, Cancelled, Lost, Won, Overtime, Ready, Activator, Reactivator) are evaluated against the quest named in `Quest` (error text: "A valid quest is required for the selected condition type"); item/NPC conditions use `CondItemContainer` + `CondItems/CondItemAmounts`, `CondOwnerNearEntity`, `CondSecondaryNPC` + states, `CondPlayerKnows(Not)` (infos that must / must not already have been given) and PAL/reputation lists; crime/guard conditions (Crime, FirstWarn, SecondWarn, EnclaveCrime, PoliticalCrime); social ones (Join, Dismiss, MobJoin, SlaveJoin, MobDismiss, Teach, Trade, PickPocket, Heal, LongTimeNoSee, NotFriend, NotFriendOrThief); structural ones (Hello, Back, StepBack, End, NothingToSay, Wait). `CondSecondaryNPCstates` values 0-5 are `gEInfoNPCStatus` [U enumerators; `StringToNPCstatus` / `NPCstatusToString` exist].

`gCInfoManager_PS` (102 exports) drives the dialogue: `StartInfoManager` 0x1cd830, `ExecuteDialogInfo` 0x1cce40, `OnInfoScriptFinished` 0x1cd5a0, `LoadInfosFromInis` 0x1cf160. Script-side manager states [V names, I flow]: `IS_Idle_Closed`, `IS_Running_SelectInfo`, `IS_Running_ProcessInfo`, `IS_Idle_Trade`, plus refusal states `IS_Running_NoEnclaveReputation`, `NoLearnRequirements`, `NoParty`, `NoPoliticalReputation`, `NotEnoughGold`, `NotEnoughLP` (paired with `GO_*` messages). `gCInfo_PS::Execute` 0x1c0a30 runs the info script (`gCInfoScript_PS::Execute` 0x1d10e0); `OnDelivery` 0x1c2620 consumes delivery items; `Give` transfers items via `gCInventory_PS::TransferItems` (error strings for missing inventories/templates/stacks).

### 4.3 Info-script command table [V]

The command executor is in Script_Game.dll (error: `IS_Running_ProcessInfo -> Unrecognized info command: %s.`). It maps the command name to an id with a static table of 68 entries (`SaySVM`=1 is [I]: its init stub has a different shape and was not decoded, 1 is the only gap; `Description` is a pseudo-command with no id used to label the player's choice):

`Say`=0, `SaySVM`=1\*, `SaySound`=2, `SayKeepItUp`=3, `SayBossNews`=4, `SayGuardNews`=5, `SayConfirm`=6, `SayHoldingPosition`=7, `Excuse`=8, `Bribe`=9, `PickPocket`=10 (data also contains `Pickpocket`), `Attack`=11, `GiveXP`=12, `Give`=13, `Wear`=14, `Use`=15, `Heal`=16, `OpenDoor`=17, `SetClass`=18, `SetPAL`=19, `SetEnclave`=20, `Sleep`=21, `SetTeachEnabled`=22, `SetTradeEnabled`=23, `SetPartyEnabled`=24, `SetMobEnabled`=25, `SetSlaveryEnabled`=26, `SetRevolutionEnabled`=27, `SetRaidEnabled`=28, `SetSleepingPoint`=29, `SetWorkingPoint`=30, `SetRelaxingPoint`=31, `FullStop`=32, `SetRoutine`=33, `SetGuardStatus`=34, `SetGameEvent`=35, `ClearGameEvent`=36, `MoveTo`=37, `SetSectorStatus`=38, `RunQuest`=39, `CloseQuest`=40, `SucceedQuest`=41, `StartEffect`=42, `DoTrade`=43, `DoLoot`=44, `ShowPicture`=45, `ShowSubtitle`=46, `PlayMusic`=47, `InfoWait`=48, `EndGame`=49, `Back`=50, `End`=51, `BoostAttribs`=52, `SetChapter`=53, `Erase`=54, `StartRevolution`=55, `Spawn`=56, `ShowGameMessage`=57, `AddQuestLog`=58, `ModReputPol`=59, `ModReputEnc`=60, `Grant`=61, `Humanize`=62, `JoinNPC`=63, `GiveAll`=64, `JoinPlayer`=65, `FailQuest`=66, `ResetCrime`=67, `DecreaseRevolCount`=68.

Argument conventions (derived from `Infos.pak` usage counts plus Game.dll validation strings [V data, I intent]). E1/E2 are entity names (`player`, `npc`, `owner` are aliases), I1/I2 string ids, T a text id:

| Command | Lines | Arguments |
| --- | ---: | --- |
| Say | 12547 | E1 speaker, T text id, I2 optional stage direction (`SD_Angry`, `SD_Point`, ...) |
| SetGameEvent / ClearGameEvent | 1354 / 147 | I1 event name (global named boolean flags read by infos/routines) |
| Description | 623 | T choice label, E1 optional |
| Give | 584 | E1 giver, E2 receiver, I1 item template, I2 amount (> 0 required; both inventories checked) |
| SaySVM | 467 | E1 speaker, I1 SVM id (`Data/Strings/SVMAdmin.dat`; must exist) |
| GiveXP | 346 | I1 amount (> 0 required) |
| End | 271 | none; ends the dialogue |
| SetRoutine | 247 | E1 npc, I1 routine name (required) |
| Set{Teach,Trade,Party,Mob,Slavery,Revolution,Raid}Enabled | 143, 139, 41, 5, 44, 12, 6 | E1 npc/enclave, I1 true/false |
| ModReputEnc / ModReputPol | 138 / 36 | E2 enclave or I1 PAL, I2 signed amount, T message |
| Attack | 83 | E1 attacker, E2 target, I1 reason (`Duel`, `Enemy`, `Revolution`, `Guard`) |
| RunQuest / CloseQuest / SucceedQuest | 57 / 76 / 20 | I1 quest name (must exist) |
| ShowSubtitle / ShowPicture / InfoWait / PlayMusic | 56 / 49 / 51 / 5 | T subtitle, I1 image (`G3_Epilog_*.dds`), I1 milliseconds, I1 music id (cutscenes/epilogue) |
| SetSectorStatus | 45 | I1 sector name, I2 true/false |
| SetPAL | 38 | E1 npc, I1 PAL (`Reb`, `Slave`, `Orc`, `None`) |
| Sleep / Use / Erase / Wear | 24 / 23 / 4 / 2 | E1, I1 hour or item, I2 amount |
| StartEffect | 23 | E1 entity, I1 effect name (`eff_event_*`) |
| SetGuardStatus | 21 | E1, I1 security level (e.g. `Inactive`) |
| ResetCrime | 21 | I1 enclave |
| remaining (SetClass, BoostAttribs, SetChapter, AddQuestLog, JoinPlayer, GiveAll, Humanize, StartRevolution, DecreaseRevolCount, OpenDoor, FullStop, Heal, Excuse, Bribe, Say* variants) | 1-18 each | see `script-registry.json` -> `infoScriptCommandUsage` |

`gCInfoScript_PS` holds `InfoScriptLines` (`gCInfoScriptLine_PS`: `Text, Command, Entity1, Entity2, ID1, ID2`), `CurrentScriptLine`, `ScriptIsRunnig`; commands complete asynchronously (`OnCommandCompleted`, `WaitForScript`), so a Rust implementation needs a coroutine-style line executor with wait states (speech playback, `InfoWait`, movement). Failed `FailQuest` (66), `Grant`, `Humanize`, `Spawn`, `EndGame` argument semantics are [U].

## 5. Other gameplay property sets (full schemas in `class-schemas.json`)

- **gCNPC_PS** (version 79, set type 30) [V]: Description, Voice, RoleDescription, Level/LevelMax, Class (`gEClass`), Species (`gESpecies`, 53 values), Gender, Type (`gENPCType`: Personal, Enclave, Political, Friend), PoliticalAlignment, Enclave (property id), AttitudeToPlayer2 (`gEAttitude`), GuardStatus, AttackReason (21 values), LastFightAgainstPlayer, LastPlayerCrime, Bearing, Immortal, DefeatedByPlayer, Ransacked, Discovered, current target/attacker/guard-point entity proxies, status-effect timestamps (Burning, Diseased, Frozen, Poisoned, Transformed), TeachAttribs/TeachSkills.
- **gCInventory_PS** (type 31): stacks and slots; `TransferItemsTo/StackTo/AllTo`, `PerformTransaction` (trade), `EquipStack/UnEquipStack`, `LearnSkill/LearnSpell`, treasure sets (`GeneratePlunderInvFromTreasureSets`, `GenerateTradeInvFromTreasureSets`; `gETreasureDistribution`: Plunder, Unique, Trophy, Weaponry, Trade_Generate, Trade_Refresh, Mining, Pickpocket, Trade_NotRandom) and the `NotifyQuestManager` item hook.
- Schemas without behaviour analysis: gCStatistics_PS, gCSkill_PS, gCPlayerMemory_PS, gCParty_PS, gCEnclave_PS, gCArena_PS, gCDialog_PS, gCMagic_PS, gCItem_PS, gCRecipe_PS, gCLock_PS, gCDoor_PS, gCTreasureSet_PS, gCDamage_PS, gCDamageReceiver_PS, gCFocus_PS, gCInteraction_PS, gCNavigation_PS, gCCharacterMovement_PS, gCClock_PS, gCSession, gCSector, gCWorld.
- **Attributes and traits are data-driven**: `Workspace.pak:skilladmin.txt` (INI; also emitted as `data/ge_script_stats.h`) [V file]. Attribute ids (index = enum value): 0 Hitpoints, 1 Mana, 2 Stamina, 3 Strength, 4 Dexterity, 5 Intelligence, 6 Endurance, 7 Prot_Blade, 8 Prot_Impact, 9-14 Reputation_ORC/REB/ASS/NRD/MID/NOM, 15 Reputation_Gladiator, 16 Reputation_Thief, 17 Reputation_Hunter, 18 Reputation_Smith, 19 Reputation_Alchemist, 20 Reputation_Mage, 21 Prot_Missile, 22 Prot_Ice, 23 Prot_Fire, 24 Prot_Lightning (25 total). Traits 0-5: Lockpick1-3, Pickpocket1-3 (no modifiers/deps/conditions/costs in the shipped file; other perks are script-side `Perk_*` and `It_Perk_*` items) [I index to enum mapping]. Short tags used in INI/scripts: HP, MP, SP, STR, DEX, INT, ALC, SMT, THF (script functions `AddStrength`, `AddSmithing`, ...).

## 6. Enumerations [V]

\* = value inferred from sequence (the init stub shape differed). Game logic enums below; all 120 (Game 71, Engine 49 renderer/physics/audio) are in `class-schemas.json -> enums`. `gESession` merges the `gESession_State_*` / `gESession_StartMode_*` families because they share a prefix; split before use. `gEAction` has 145 entries, `gEUseType` 117 (see JSON).

- `gEQuestStatus` (8): Open=0, Running=1, Success=2, Failed=3, Obsolete=4, Cancelled=5, Lost=6, Won=7
- `gEQuestType` (13): HasItems=0, Report=1, Kill=2, Defeat=3, DriveAway=4, Arena=5, BringNpc=6, FollowNpc=7, EnterArea=8, FreeEnclave=10, Plunder=11, Sparring=12, SpellCast=13
- `gEInfoType` (7): Refuse=0, Important=1, News=2, Info=3\*, Parent=4, Comment=5, QuestRunning=7
- `gEPoliticalAlignment` (10): None=0, Orc=1, Nrd=2, Reb=3, Mid=4, Ass=5, Nom=6, Out=7, Slave=8, Pirate=9
- `gEAttitude` (7): None=0, Friendly=1, Neutral=2, Angry=3, Hostile=4, Predator=5, Panic=6
- `gEClass` (12): None=0, Mage=1, Paladin=2, Warrior=3, Ranger=4, EMPTY_A=5, EMPTY_B=6, EMPTY_C=7, EMPTY_D=8, EMPTY_E=9, EMPTY_F=10, EMPTY_G=11
- `gENPCType` (4): Personal=0, Enclave=1, Political=2, Friend=3
- `gEPlayerCrime` (6): None=0, Livestock=1, Attack=2, Theft=3, Murder=4, Count=5
- `gESecurityLevel` (4): None=0, Public=1, Enclave=2, Political=3
- `gEGuardStatus` (5): Active=0, FirstWarnGiven=1, SecondWarnGiven=2, Inactive=3, Behind=4
- `gEPartyMemberType` (6): None=0, Party=1, Mob=2, Slave=3, Controlled=4, Summoned=5
- `gEAIMode` (10): Routine=0, Reaction=1, Combat=3, Flee=4, Arena=5, Talk=6, Down=8, Dead=9, Abandoned=10, PlayerParty=11
- `gEEnclaveStatus` (3): Routine=0, Attack=1, Flee=2
- `gELastFightAgainstPlayer` (6): None=0, Lost=1, Won=2, Cancel=3, Running=4, Friendly=5
- `gEGender` (2): Male=0, Female=1\*
- `gEBearing` (3): None=0, Babe=1, Zombie=2
- `gEItemCategory` (12): None=0, Weapon=1, Armor=2, Artefact=3, Alchemy=4, Empty_A=5, Misc=6, Written=7, Empty_B=8, Spellbook=9, Skill=10, Count=11
- `gESkillCategory` (8): Combat=0, Hunting=1, Magic=2, Smithing=3, Theft=4, Alchemy=5, Misc=6, Cooking=7
- `gESlot` (19): None=0, RightHand=1, LeftHand=2, EquipLeftHand=3, Beard=4, BackLeft=5, BackRight=6, Bow=7, Crossbow=8, Armor=9, Belt=10, Amulet=11, Ring1=12, Ring2=13, Ammo=14, Hair=15, Head=16, Body=17, Helmet=18
- `gEStackType` (3): Normal=0, Treasure=1, Merchandise=2
- `gETradeCategory` (5): None=0, Valuable=1, Trophy=2, Empty_A=3, Empty_B=4
- `gETreasureDistribution` (9): Plunder=0, Unique=1, Trophy=2, Weaponry=3, Trade_Generate=4, Trade_Refresh=5, Mining=6, Pickpocket=7, Trade_NotRandom=8
- `gEDamageType` (8): None=0, Impact=1\*, Blade=2, Missile=3, Fire=4, Ice=5, Lightning=6, Physics=7
- `gEWeaponConfig` (3): Melee=0, Ranged=1, Magic=2
- `gESpellDeity` (4): Adanos=0, Beliar=1, Innos=2, None=3
- `gESpellType` (3): Projectile=0, Target=1, Summon=2
- `gECastType` (3): Immediate=0, Power=1, Regular=2
- `gEAttackReason` (21): None=0, LeaveZone=1, Livestock=2, StopThief=3, Theft=4, UseInteractObject=5, ReactToWeapon=6, Angry=7, StopFight=8, ReactToDamage=9, GateGuard=10, Intruder=11, Enemy=12, Murder=13, Prey=14, DriveAway=15, PlayerCommand=16, Duel=17, Arena=18, Revolution=19, Kill=20
- `gESession` (12): State_None=0, State_Movement=1, State_Fight=2, State_Ride_Movement=3, State_Ride_Fight=4, State_ItemUse=5, State_Inventory=6, State_Dialog=7, State_Trade=8, State_InteractObj=9, State_Journal=10, State_Editor=11
- `gEMoverState` (4): OPEN=0, OPENING=1, CLOSED=2\*, CLOSING=3
- `gEDoorStatus` (2): Open=0, Closed=1\*
- `gELockStatus` (2): Locked=0, Unlocked=1\*
- `gEAnchorType` (4): Local=0, Roam=1, Patrol=2, Event=3
- `gEArenaStatus` (2): None=0, Running=1\*
- `gESpecies` (53): Human=0, Skeleton=1, Zombie=2, Golem=3, Demon=4, Orc=5, Goblin=6, Troll=7, Minecrawler=8, Scavenger=9, Wolf=10, Boar=11, Sabertooth=12, Shadowbeast=13, Bison=14, Rhino=15, Ripper=16, Lurker=17, Varan=18, Snapper=19, Alligator=20, Trex=21, FireVaran=22, Bloodfly=23, Lizard=24, Fish=25, Meatbug=26, Vulture=27, Rabbit=28, Deer=29, Chicken=30, Molerat=31, Snake=32, Pig=33, Cow=34, Spider=35, Turtle=36, Bird=37, Gargoyle=38, SwampLurker=39, EMPTY_B=40, Ogre=41, Ray=42, Scorpion=43, Seagull=44, Walrus=45, Whale=46, Eagle=47, FireGolem=48, IceGolem=49, ScorpionKing=50, Stalker=51, Dragon=52

Enumerators not statically discoverable (no `Enum_Name` strings; defined elsewhere or numeric only) [U]: `gEInfoNPCStatus`, `gEPose`, `gEPhase`, `gEFaceAni`, `gEStatusEffect`, `gEGotoStatus`, `gEWalkMode`, `gEDirection`, `gEGamePage`, `gEGameMessageType`, `gEHUD*`, `gECombatMode`, `gECameraMode`, `gECharMovementMode`, `gESoftCollisionState`, `gETutorialFlag`, `gEInventoryFindQuality`, `EDifficulty`, `EAttrib`/`ETrait` (derive from skilladmin), `EAttribModOperation`, `EAttribReqOperation`.

## 7. Reimplementation priorities

1. **GENOMFLE property-stream decoder**: use `class-schemas.json` (names, types, class versions) to decode templates, entities, quest/info runtime packets and saves. Unknown properties must be skipped by length, not by schema.
2. **Quest and info data model and loaders** (pure Rust data): parse the INI formats above (570 quests, 4590 infos) with the enum tables; implement the quest state machine and event hooks (OnReceiveItem, OnNPCKilled/Defeated, OnEnter/Leave, OnSpellCast, OnEnclaveStateChanged); implement the info manager (Hello/General/Back/End structure, parent/child choices, Permanent/ClearChildren, SortID, condition evaluation). Highest unknown: exact condition predicates.
3. **Info-script executor** for the 68-command table with Strings.pak text and SVM resolution; named game-event flags (`SetGameEvent`/`ClearGameEvent`) as the cross-system blackboard.
4. **Game state tables**: inventory/stack/trade transfer rules, attributes via `skilladmin.txt`, reputation (PAL and enclave), crime/guard escalation (`gEPlayerCrime`, `gEGuardStatus`, `IS_*` refusal states), party/mob/slave membership, enclave status.
5. **Script-layer behaviours** (largest effort, no data fallback): the 874 registered AI states/functions/callbacks/commands/routines. Organise by source module (AI, Routine, Player, Magic, Engine, Enclave). Start with `Rtn_*`, `ZS_Ambient/Sleep/Work`, `ZS_Attack*`, `PS_Interact_*`, `OnPlayer*`, perks. Behavioural parity requires decompiling Script_Game.dll function by function; `script-registry.json` gives each function's entry VA (`fnVA`).
6. Quest log UI, HUD message keys (`GO_*`, `HUD_Quest_*`) and localisation tables (Strings.pak).

## 8. Open questions

- Condition predicate semantics per `gEInfoCondType`; legal quest status transitions; overtime handling; what sets Won/Lost (arena/sparring) versus Success/Failed.
- Meaning of the second registration flag and the shim-kind codes in property stubs [I].
- `SaySVM`=1 and any other table entry decoded by inference; argument semantics of `Grant`, `Humanize`, `Spawn`, `EndGame`, `SetChapter`, `BoostAttribs`, `FailQuest`.
- `gEInfoNPCStatus`, `gEAction` semantics, pose/phase/face animation enums.
- Whether the compiled info cache (`compiledinfos_*.bin`) matters for parity (the INI files are authoritative).
- Game.dll deep analysis under REA (needs a longer provider timeout or a reduced profile). Every control-flow claim above other than the Script.dll forwarders is unconfirmed by decompilation.
