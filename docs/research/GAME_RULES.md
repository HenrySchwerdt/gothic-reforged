# Gothic 3 gameplay rules: info availability, quest state machine, info-script executor

Recovered 2026-10-20 from the installed 1.75.20108.14 build by targeted capstone disassembly of specific functions (REA/Ghidra was deliberately not run on Game.dll; see GAME_LOGIC_MAP.md). Only prose, addresses and pseudo-rules appear here; no decompiled bodies. Raw per-function listings live in gitignored `.local/research/rules/` (`g3dis.py`, `xref.py`, `ac.txt`, `ss.txt`, `oe.txt`, `ex.txt`, `od.txt`, `pi.txt`, `cases.json`).

Evidence legend: **V** = read from the instructions (address given: `G:` = Game.dll RVA, `S:` = scripts/Script_Game.dll RVA); **I** = inferred (naming, partially read, or consistent with data); **U** = not recovered.

Field notation: `info.X` and `quest.X` are the property names from `class-schemas.json`. Object offsets used by the binary (info: ConditionType +0x28, Type +0x30, InfoGiven +0x18, Owner +0x14, Quest +0x20, Parent +0x1c, SortID +0x10; quest: Type +0x14, Status +0xac, Destination +0x78, DeliveryEntities +0x54, counters/amounts parallel arrays) were verified against the property-registration offsets. Time unit used everywhere below: `hours = (year*365 + day)*24 + hour`.

Constants read from the data section: near-distance 500 (owner/delivery proximity), BringNpc distance 2000, guard-point margin 80, "Animal" cooldown 48 hours, political/enclave fame range 0..100, "friend" fame threshold 75.

---------------------------------------------------------------------------

## 1. Info availability

### 1.1 IsAvailable [V G:1c5f90]

```
IsAvailable(info) =
  if info.InfoGiven and info.ConditionType != End(51): false
  else AreConditionsFulfilled(info)
```
Permanent is NOT consulted here; it only matters when an info is consumed (see 1.5).

### 1.2 AreConditionsFulfilled [V G:1c3370]

Evaluated with the manager's pair: `InfoManager.player` (+0x44) and `InfoManager.npc` (+0x48, the dialog partner, "self" of the dialog). Steps, in order, any failure returns false:

1. Owner entity (`info.Owner`) must exist in the scene, else error + false.
2. If `info.Quest` is non-empty the quest must exist in the quest manager, else error + false. `quest` below is that object (may be null for quest-less infos; quest-bound conditions then error and fail).
3. If `info.ConditionOwnerNearEntity` is non-empty: distance(owner, that entity) must be <= 500.
4. If ConditionType > 53: false.
5. **Per-condition-type predicate** (table 1.3). Types that pass fall into the common checks.
6. **Common checks** (all must hold; empty lists are skipped):
   a. *Item container*: if `ConditionItemContainer` resolves to an entity, it needs an inventory (else false). For each `CondItems[i]` with `CondItemAmounts[i]`: if amount == 0, the container must NOT hold that template; if amount > 0, it must hold a stack with count >= amount. If the container entity does not exist the whole check is skipped (passes).
   b. *Wears item* (`CondWearsItem`): each named item must be present and equipped in the container inventory (the slot is derived from the item's use type; alternative slot tried for rings/weapons) [I for slot mapping].
   c. *Player knows* (`ConditionPlayerKnows`): every named info must be present in `PlayerMemory.PlayerKnows` (player memory propertyset type 60); missing player or memory = error + false. *Player knows not* (`ConditionPlayerKnowsNot`): none may be present.
   d. *Has skill* (`CondHasSkill`): for each template, skill active (`IsSkillActive`) or spell learned, else false.
   e. *Secondary NPCs* (`CondSecondaryNPC[i]` + `CondSecondaryNPCStatus[i]`, a `gEInfoNPCStatus`): entity must exist and have an NPC propertyset. Status values [V G:1c3370 @55a3..55f2]:
      0 = alive; 1 = alive and not defeated; 2 = defeated-by-player flag set (NPC_PS+0xa4); 3 = dead (`IsDead`: AI mode == Dead(9)); 4 = dialog-propertyset TalkedToPlayer true; 5 = TalkedToPlayer false.
      Missing entity: logged, then states 0,1,5 fail and 2,3,4 pass [I].
   f. *Reputation* (`CondReputGroup[i]`, `CondReputAmount[i]`, `CondReputRelation[i]`): group is a political alignment tag (Orc, Reb, Ass, Nom, Nrd, Pir, Mid) or an enclave/guild name. Value = player memory political fame (for alignments) or the enclave's `PlayerFame`. If relation string == "MAX" the value must be <= amount, otherwise value >= amount.
   g. *CondPAL*: the dialog partner's political alignment (Orc, Reb, Ass, Nom, Nrd, Pir, Mid) must be in the list.
   h. *Party members* (`CondPlayerPartyMember`): number of current party members of the player whose names are listed must be >= list length.
   i. If `info.Type == QuestRunning(7)`: quest must be Running, and `now_hours - max(quest.activation, info.lastRun) >= 2` hours where `info.lastRun` (+0x94, written in OnEndInfo) overrides the quest activation time when non-zero. [V 1c5cb4..1c5d6b]
   j. If `info.Type == Parent(4)`: result = `AreChildInfosAvailable(info)`.
   k. otherwise true.

### 1.3 Predicate per ConditionType (step 5) [V G:1c3370 switch at 1c34ee]

| cond (id) | predicate |
| --- | --- |
| Crime(0) | `InfoManager.npc.field(+0x70) == 0` [I meaning: guard/crime flag] |
| Fight(1) | always false (fight infos are only started by script) |
| Hello(2) | owner's dialog PS exists and `TalkedToPlayer != true` |
| General(3), EnclaveFriend(24), PoliticalFriend(25), NothingToSay(50), End(51), Back(52), StepBack(53) | no extra predicate (common checks only). Friend/reputation gating for 24/25 happens in Execute (1.6) |
| Overtime(4) | quest required; status == Running and `IsOvertime()` |
| Open(5), Activator(6) | quest required; status == Open and all `quest.FinishedQuests` finished (1.4). If quest.Type == Arena(5): the destination arena PS must exist and not be Running |
| Reactivator(21) | quest required; status == **Lost(6)**; same Arena check as Open |
| Lost(20) | quest status == Lost(6) |
| Running(7), DoCancel(11) | quest status == Running(1) |
| Delivery(8) | quest status Running, then by quest.Type: HasItems(0): `quest.Destination == info.Owner` and the player holds all required items (`HaveNPCsAllRequiredItems`); Defeat(3): destination NPC missing -> true, else its defeated flag (NPC_PS+0xa4) set; BringNpc(6): needs delivery entities, destination exists, every delivery NPC follows the player (party leader == player) and is within 2000 of the destination; EnterArea(7 in this table slot = FollowNpc, see note): every delivery entity within 500 of destination; all other types: no extra predicate |
| PartDelivery(9) | status Running, Type == HasItems, player has some but not all required items (`HasNPCpartOfRequiredItems` true and `HaveNPCsAllRequiredItems` false), destination exists |
| Success(10) | quest status == Success(2) |
| Won(22) | quest status == Won(7) |
| Failed(12) | quest status == Failed(3) |
| Cancelled(13) | quest status == Cancelled(5) |
| Ready(19) | quest status is Open(0) or Running(1) |
| Join(14) | partner dialog `PartyEnabled`; party PS exists; (party `Waiting` or party leader != player) |
| Dismiss(15) | partner dialog `PartyEnabled` (or, failing that, nav PS present and party member type == Controlled(4)); and party leader == player |
| MobJoin(28) | dialog `MobEnabled`; party PS; (Waiting or leader != player); special case: quest "Reto_FreeMine", if it exists, must be Running |
| SlaveJoin(29) | dialog `SlaveryEnabled`; party PS; (Waiting or leader != player) |
| MobDismiss(33) | dialog `MobEnabled`; party leader == player |
| Wait(34) | party PS; not Waiting; member type != Controlled; leader == player |
| Companion(37) | party member type in {Party,Mob,Slave} (or >5) and leader == player |
| Teach(16) | dialog `TeachEnabled`; if `info.TeachAttrib` is non-empty the NPC's TeachAttribs list must contain it; otherwise the NPC's TeachSkills must contain `info.TeachSkill` and the player must not have learned that skill/spell yet |
| Trade(17) | dialog `TradeEnabled` |
| PickPocket(18) | player has skill `Perk_PickPocket_1` active; partner dialog `PickedPocket` false; partner inventory has a treasure set whose distribution == Pickpocket(7) |
| MasterThief(23) | script function `HasMasterThiefInfo(npc)` true [I], then PickPocket conditions |
| Heal(35) | script function `HasHealInfo(npc)` returns nonzero |
| EnclaveCrime(31) | script `HasGuardNews(npc)` nonzero [I]; PoliticalCrime(32): script `HasBossNews(npc)` nonzero [I] |
| FirstWarn(26) | NPC has a guard-point entity and GuardStatus == Active(0) |
| SecondWarn(27) | GuardStatus == FirstWarnGiven(1), guard point exists (else error), distance(guard point, player) < lastDistToGuardPoint-field(+0x140) - 80 |
| Animal(36) | script `CanAttack(npc)` is false; and `dialog.EndDialogTimestamp` is 0 or at least 48 game hours old (cooldown for cows etc.; data: the only shipped info of this type is the cow info) |
| NotFriend(38), NotFriendOrThief(39) | NPC type != Friend(3); NPC attitude field in {None,Predator}; not already a party member of the player; world-clock cooldown of 1 hour since NPC field(+0x19c); 39 additionally requires dialog flag(+0x24) != 1 [I] |
| LongTimeNoSee(30), 40..49 | always false (script/engine fired only) |

Notes: the enum label order in GAME_LOGIC_MAP section 4.2 is confirmed by this switch (gaps 40-49, the 50-53 group, Animal info data). In the Delivery row the quest-type dispatch is a jump table indexed by quest type 0..7; the only non-trivial cases are types 0, 3, 6 and 7 (FollowNpc is 7, EnterArea is 8; the table maps type 7 to the proximity check). [V for dispatch, I for the type-7 naming]

### 1.4 Quest helpers [V]

- `AreFinishedQuestsFinished` [G:157220]: for every name in `quest.FinishedQuests`: unknown quest = logged, ignored (counts as satisfied); status Open, Running or Lost = not finished (false). Success, Failed, Obsolete, Cancelled, Won count as finished.
- `IsOvertime` [G:1570b0]: false if status == Open; limit = hours(RunningTime) * 3600; limit 0 = never overtime; overtime = (now - activationTime) in seconds > limit. Nothing in the engine changes status on overtime (the only caller is the `Overtime` info condition); the "Overtime" info script is expected to call FailQuest.
- `CheckDeliveryEntitiesStatus` [G:156bb0]: runs only for status Open/Running/Lost (SpellCast only when Running). If **every** `DeliveryCounter[i] >= DeliveryAmounts[i]` (vacuously true with zero entities) then SetStatus(Won) for Sparring(12), else SetStatus(Success). Called by every counter-changing event hook.

### 1.5 Info lists and dialog flow [V G:1ce2f0, 1cde00, 1ce1f0, 1cd5a0, 1cd830]

- Infos are indexed by Owner. All infos whose owner is `PC_Hero` are appended to the list of every other owner. An NPC's list is its own infos plus the PC_Hero infos.
- `GetInfoList(root infos = Parent empty)` for the dialog start collects, restricted to infos that are `IsAvailable`:
  1. first available **Refuse(0)** info only; 2. first available **Important(1)** only; 3. all **QuestRunning(7)**; 4. all **News(2)**; 5. all **Info(3)/Parent(4)** sorted by (ConditionType ascending, NPC-owned before PC_Hero-owned, SortID ascending). Then keep only the first of each of End(51), Trade(17), MasterThief(23), NothingToSay(50); drop NothingToSay unless the list contains nothing but End/NothingToSay.
  Child lists (`GetChildInfoList`) contain infos with Parent == chosen info, Type Info/Parent, matching owner (npc or player), available; sorted the same way.
- StartInfoManager: empty list = end; list of only End infos = end; if the first entry is Info/Parent and there is more than one entry = show the choice menu; otherwise execute the first entry immediately (this is how Refuse/Important/News/QuestRunning auto-run).
- After an info script finishes (OnInfoScriptFinished): run OnEndInfo; if the info is End/NothingToSay/Trade/MasterThief or Refuse, the dialog ends; if `ClearChildren` on a child info, all sibling children get InfoGiven=true; if the finished info has available children, switch to child mode and show them; in child mode and not Back, show the parent's children again (StepBack shows the grandparent's); else rebuild the root list with **only** Info/Parent entries; empty = end; only End = run it; one entry = run it; else menu.
- `HasInfos(npc)`: true if any root info of that owner list with Type != Comment, cond not Back/End/StepBack, no Parent, is available. `HasImportantInfo(npc)`: any available Important, or an available Refuse info (cond != NothingToSay) that is not PC_Hero-owned. These drive NPC approach/greeting behaviour [I for the caller].
- Consumption (`IsPermanent` G:1c2ed0): an info stays available (not marked given) when Type is Refuse(0) or Parent(4), cond in {PartDelivery(9), Back(52), End(51), StepBack(53)}, `Permanent` true, or the owner is the player. Otherwise `InfoGiven := true` after Execute starts. Combined with 1.1 a given info is hidden except End.

### 1.6 Execute and OnEndInfo [V G:1c0a30, 1c1270, 1d1460, 1c2620]

Execute(info):
- Teach(16): cost = `TeachAttribValue` if TeachAttrib set (doubled when engine setting byte +0x1ea is on, the attrib is STR/DEX/INT and the player's current value >= 250), else the skill's LP cost (SkillPS +0x18) or spell cost (MagicPS +0x8c). Failing `CanLearnSkill` = state `IS_Running_NoLearnRequirements`; cost > player LP (PlayerMemory +0x58) = `IS_Running_NotEnoughLP`.
- Trade(17) and MasterThief(23): append a `DoTrade` / `DoLoot` command (E1 npc, E2 player) if the script's last line is not already one.
- GoldCost > 0: player inventory needs a gold stack >= cost (doubled under the same STR/DEX/INT rule for Teach), else `IS_Running_NotEnoughGold`; otherwise gold is transferred player -> partner via `Give`.
- Join(14): script `CanJoinPlayerParty` must be nonzero else `IS_Running_NoParty`.
- EnclaveFriend(24): enclave `PlayerFame` of the NPC's enclave must be >= 75 else `IS_Running_NoEnclaveReputation`; PoliticalFriend(25): player's political fame for the NPC's alignment >= 75 else `IS_Running_NoPoliticalReputation`.
- Then run the info script (`IS_Running_ProcessInfo`) and set InfoGiven unless IsPermanent.

Script progress: when `CurrentScriptLine` first advances to index 1 (after skipping leading `Description` pseudo-lines) the engine calls `OnDelivery` (G:1c2620).

OnDelivery (only cond Delivery(8)/PartDelivery(9), quest named by info.Quest):
- HasItems quest: for each required item give `amount[i]-counter[i]` of it from player to the partner (`gCInfo_PS::Give`), printing `HUD_MissingItem` for shortfalls.
- Delivery(8) only, by quest.Type: Report(1)/DriveAway(4): the delivery entity equal to the info owner gets counter+1, then CheckDeliveryEntitiesStatus. Defeat(3), BringNpc(6), SpellCast(13): SetStatus(Success) directly; BringNpc also runs script `StopBringNPC` on each delivery NPC and clears its Party/Slavery/Mob enabled flags. FollowNpc(7): SetStatus(Success) and run script `StopGuidePlayer`.

OnEndInfo (side effects after the script, by ConditionType) [V G:1c1270]:
- Activator(6) and Reactivator(21): `quest.SetStatus(Running)`. FollowNpc quests additionally put any other Running FollowNpc quest into Lost, then run script `StartGuidePlayer`. BringNpc: script `StartBringNPC` per delivery NPC. Arena/Sparring: per delivery NPC script `SendToArena`, AI FullStop then ContinueRoutine.
- DoCancel(11): `quest.SetCancelled` (Open -> Obsolete, otherwise Cancelled).
- Join(14)/MobJoin(28)/SlaveJoin(29)/Dismiss(15)/MobDismiss(33)/Wait(34): scripts `JoinPlayerParty`, `JoinPlayerMob`, `JoinPlayerSlavery`, `DismissPlayerParty`, `DismissPlayerMob`, `WaitPlayerParty`.
- Teach(16): learn skill/spell (`LearnSkill` / `LearnSpell`) and subtract LP; for attributes: new Maximum and BaseValue += `TeachAttribValue * m` where m = 4 for HP/MP/SP and 1 otherwise; LP -= TeachAttribValue (and a second subtraction when the 250 rule above applies); prints `GO_Learned`, `GO_LearningPointsLeft`.
- FirstWarn(26)/SecondWarn(27): NPC GuardStatus := FirstWarnGiven(1) / SecondWarnGiven(2), store distance to guard point.
- Animal(36): `dialog.EndDialogTimestamp := now (hours)`.
- Generic: for ConditionTypes **not** in {General 3, Overtime 4, Open 5, Activator 6, Running 7, Delivery 8, Success 10, DoCancel 11, Ready 19}, `SuppressLog := true`. For the listed ones, each `Say`/`SaySVM`... line of the script is appended to the bound quest's log; `AddQuestLog` (ID1 = quest, optional `Quest#...` form) also appends. `Type == QuestRunning` stores the current world hours in the info (+0x94).

Quest log appends only happen when `info.Quest` resolves to a quest.

---------------------------------------------------------------------------

## 2. Quest manager

### 2.1 Manager entry points [V G:15e170..15e250]

| call | rule |
| --- | --- |
| RunQuest(name) | quest exists and status == Open -> SetStatus(Running) |
| CloseQuest(name) | status == Open -> SetStatus(Obsolete); any other status -> SetStatus(Cancelled) (which itself only succeeds from Running) |
| SucceedQuest(name) | status Open or Running -> SetStatus(Success) |
| FailQuest(name) | unconditional SetStatus(Failed) |
| gCQuest_PS::SetFailed | Open -> Obsolete, else Failed |
| gCQuest_PS::SetCancelled | Open -> Obsolete, else Cancelled |

### 2.2 SetStatus transition table [V G:157840]

| requested | allowed from | effects |
| --- | --- | --- |
| Open(0) | none | error "reopen" and refused |
| Running(1) | Open, Lost | `activationTime := world clock (Y,D,H)`; log; HUD `GO_QuestRunning: <title>` (msg type 7); if RunningTimeDays > 0 also `GO_QuestTimeLimit` with `$(day)` replaced |
| Success(2) and Won(7) (shared path) | Open, Running | HUD `GO_QuestSuccess`; rewards (2.3); then status stored as requested (Success or Won) |
| Failed(3) | Open, Running | HUD `GO_QuestFailure` only if it was Running; status stored |
| Obsolete(4) | Open | silent log, status stored |
| Cancelled(5) | Running | HUD `GO_QuestCancel`, status stored |
| Lost(6) | Running | HUD `GO_QuestCancel`, status stored |

Other transitions return false without change. Failed/Cancelled/Lost/Obsolete grant no rewards. After storing the status, for quest types Arena(5) and Sparring(12) the destination arena PS gets `Status := (new status == Running)` (arena entity must have a gCArena_PS, else error).

### 2.3 Rewards on Success/Won [V G:157840]

In this order:
1. `PoliticalSuccess` + `PoliticalSuccessAmount` > 0: player memory political fame[alignment] += amount, clamped to 100 (HUD `GO_PoliticalFame_<tag>`).
2. `PoliticalFailureAmount` > 0 (not present in shipped data): fame[`PoliticalFailure`] -= amount, floored at 0.
3. `AttribSuccess` (tag such as STR) with `AttribSuccessAmount` != 0: attribute BaseValue += amount (tag unknown = "<invalid attrib>" message). HUD message type 0xf.
4. `EnclaveSuccess` + `EnclaveSuccessAmount` > 0: enclave `PlayerFame` += amount (cap 100); `EnclaveFailure(+Amount)` decreases (floor 0).
5. `ExperiencePoints` > 0: script function `GiveXP` is run with source = world, receiver = player, amount = quest XP. **GiveXP multiplies world-sourced amounts by 5** (2.5), so a quest value of 300 grants 1500 XP.
6. `InfoManager.StartComment` and, for quest "Ardea_Revolution", player-memory tutorial flag 8 plus tutorial popup `TUT_AfterFight` [V, trivia].

### 2.4 Completion hooks per quest type [V G:158c70..15acc0, 156bb0]

All hooks act only while status is Open, Running or Lost unless noted; the manager forwards each event to every quest in manager order (OnReceiveItem only to the HasItems and Plunder lists). "Counter" = `DeliveryCounter[i]`, success check = 1.4 `CheckDeliveryEntitiesStatus`.

- **OnReceiveItem(from, to, item, n)** (types HasItems(0), Plunder(11)): HasItems: if `to` is the destination inventory entity delta = +n; if `from` is the destination delta = -n; otherwise ignored. Plunder: reversed (taking from the destination counts +n). Matching delivery entity (item template) counter += delta. Only a positive delta triggers the success check, so losing items later never un-succeeds.
- **OnNPCKilled(npc)** (npc named `PC_Hero` ignored):
  - Kill(2), Defeat(3), DriveAway(4): delivery entity equal to the npc name -> counter+1, success check.
  - If npc is the destination: HasItems or SpellCast -> `SetFailed`; Kill/Defeat/DriveAway -> `SetCancelled` only if the engine option byte (+0x1ea) is on [I meaning]; other types nothing.
  - Report(1): delivery entity died before counter reached amount -> SetFailed.
  - SpellCast(13): npc gets considered only when its NPC PS value(+0x34) is 0 or 5; delivery npc with counter < amount died -> SetFailed.
  - Arena(5)/Sparring(12): a delivery entity (opponent) killed -> SetFailed.
  - BringNpc(6): delivery[0] died with counter < amount, or the destination died -> SetFailed. FollowNpc(7): delivery[0] died with counter < amount -> SetFailed.
- **OnNPCDefeated(npc)** (arrays must have equal length, else error): Defeat(3): delivery entity match -> counter+1, success check. Arena(5)/Sparring(12) while Running: if the defeated npc is the player and the player's current attacker name matches a delivery entity -> SetStatus(Lost); else delivery entity match -> counter+1, success check (Success for Arena, Won for Sparring).
- **OnEnter(who, area)** (EnterArea 8... note the quest Type value used is 8): area == destination and `who` == delivery[0] -> counter[0]+1 and success check (needs >= 1 delivery entity, else error).
- **OnLeave(who, area)** (Arena/Sparring): if who is the player and area == destination and the arena is Running -> SetStatus(Lost). If area == destination and the arena is Running, any delivery entity equal to `who` gets counter+1 and success check.
- **OnEnclaveStateChanged(enclave)** (FreeEnclave 10): delivery entity equal to the enclave name -> counter+1, success check.
- **OnSpellCast(caster, target, spell)** (SpellCast 13): both entities required; `quest.SpellName` must equal the spell; delivery entity equal to the target name -> counter+1, success check.
- **Info-driven**: Report/DriveAway/Defeat/BringNpc/FollowNpc/SpellCast complete through `OnDelivery` (1.6). HasItems completes through item counters or delivery. FollowNpc/BringNpc have no automatic hook other than OnNPCKilled failure.

Remaining statuses: nothing sets Obsolete automatically except Close/SetFailed/SetCancelled while Open; nothing times quests out.

---------------------------------------------------------------------------

## 3. Info-script command executor [V S:b7f10 (IS_Running_ProcessInfo), command id = switch index, 69-entry jump table at S:bcd90]

Each line has Command, Entity1 (E1), Entity2 (E2), ID1, ID2, Text. `player`/`npc`/`self`/`other` are aliases resolved by `FindEntityFromString`. `ID1`/`ID2` booleans parse as integer or the substring "true". After every command the executor calls `OnCommandCompleted` (asynchronous for speech/movement/wait).

Not implemented in the executor (unknown-command warning, then completes): **SetEnclave(20), PlayMusic(47), Back(50)** and unnamed ids. `Description` has no id (choice label only).

| id | command | behaviour |
| --- | --- | --- |
| 0 | Say | E1 speaks text id T (sound file `<voice>.ogg` looked up via the SVM manager; falls back to script `_AI_Say_Ex`), ID2 = stage-direction animation. Comment infos play without stopping routines [I] |
| 1 | SaySVM | script `_AI_SaySVM`: E1 speaks SVM line ID1 |
| 2 | SaySound | play the animation then output the sound named ID1 |
| 3 | SayKeepItUp | random `KeepItUp_00..04` SVM |
| 4/5 | SayBossNews / SayGuardNews | speak news SVM (scripted) |
| 6 | SayConfirm | SVM `Confirm_02` |
| 7 | SayHoldingPosition | SVM line |
| 8/9 | Excuse / Bribe | scripts `_AI_Excuse`, `_AI_Bribe` on E2 |
| 10 | PickPocket | starts the pickpocket mini-game on E2 |
| 11 | Attack | E1 attacks E2; ID1 = AttackReason (Duel, Enemy, Guard, ReactToWeapon, Revolution); the NPC "Xardas" has a special NPC-type assignment |
| 12 | GiveXP | `GiveXP(world, player, int(ID1))` => player gains 5 x ID1 XP (see 4.1) |
| 13 | Give | E1 giver inventory, E2 receiver, ID1 item, ID2 amount. If E1 lacks the stack, the missing amount is created in E1's inventory first (template default stack size used), then `PSInfoManager::Give` transfers; when items had to be created and the giver is not the player, the leftover is purged [I on the purge] |
| 14 | Wear | find or create the item in E1's inventory and `EquipStack` |
| 15 | Use | ensure the item (`AssureItems`) then script `_AI_UseInventoryItem` |
| 16 | Heal | plays the heal animation |
| 17 | OpenDoor | `PSDoor::Open` |
| 18 | SetClass | ID1 in {Mage, Paladin, Warrior, Ranger} -> NPC Class |
| 19 | SetPAL | ID1 alignment (Orc, Nrd, Pir, Reb, Mid, Ass, Nom, Out, Slave, None) -> NPC PoliticalAlignment; specials: Wenzel/Zuben/KapDun get an NPC type and enclave raid/setup side effects |
| 21 | Sleep | ID1 hours, advances time |
| 22-26 | Set{Teach,Trade,Party,Mob,Slavery}Enabled | dialog propertyset flag := ID1 bool |
| 27/28 | SetRevolutionEnabled / SetRaidEnabled | enclave flag |
| 29-31 | Set{Sleeping,Working,Relaxing}Point | navigation point := E2 |
| 32 | FullStop | routine FullStop |
| 33 | SetRoutine | `SetRoutine(ID1)`; if AI mode is Routine: FullStop then ContinueRoutine |
| 34 | SetGuardStatus | ID1 "Active" -> Active(0), otherwise Inactive(3) [I for the else branch] |
| 35/36 | SetGameEvent / ClearGameEvent | `PlayerMemory.SetGameEvent(ID1)` / `ClearGameEvent`; the event set is the player-memory `PlayerKnows` list, so GameEvents are tested with `ConditionPlayerKnows(Not)` [I link] |
| 37 | MoveTo | E1 teleports to E2 unless dead |
| 38 | SetSectorStatus | world sector ID1 enable/disable; hard-coded extras for the Gotha and Ardea sectors (enclave alignment, Switched, crime resets) |
| 39 | RunQuest | `QuestManager.RunQuest(ID1)` |
| 40 | CloseQuest | `QuestManager.CloseQuest(ID1)` |
| 41 | SucceedQuest | `QuestManager.SucceedQuest(ID1)`; if the quest is FollowNpc, additionally script call with the dialog NPC (stop guiding) |
| 66 | FailQuest | `QuestManager.FailQuest(ID1)` |
| 42 | StartEffect | effect ID1 on E1 |
| 43/44 | DoTrade / DoLoot | opens the trade/loot UI on E2: sets the NPC routine state `IS_Idle_Trade` |
| 45/46 | ShowPicture / ShowSubtitle | GUI image ID1 / subtitle text T |
| 48 | InfoWait | wait ID1 ms (AI wait instruction) |
| 49 | EndGame | plays `G3_Credits2.bik` or `G3_Credits.bik` (by condition) then terminates |
| 51 | End | `PSInfoManager::End` |
| 52 | BoostAttribs | ID1 in {STR, DEX, INT, XAR}: adds script-defined boost to that attribute (XAR = all three; engine option byte alters amount) |
| 53 | SetChapter | `PlayerMemory.Chapter := int(ID1)` |
| 54 | Erase | remove ID2... `PurgeItems(ID1 template, amount)` from E1 inventory |
| 55 | StartRevolution | enclave Raid := true, Status := Attack |
| 56 | Spawn | `SpawnEntity(template ID1)` at position encoded in ID2 words: `wpos x y z`, `lpos`, `wposg`, `lposg` (ground-snapped via ray trace), rotation `wrot`/`lrot` angles |
| 57 | ShowGameMessage | text T printed as game message (one of two types depending on text) |
| 58 | AddQuestLog | no executor case; applied in OnEndInfo log pass (1.6) |
| 59 | ModReputPol | ID1 alignment tag, ID2 amount > 0, Text "DEC" = decrease else increase; fame clamped to 0..100; HUD `GO_PoliticalFame_<tag>` |
| 60 | ModReputEnc | E2 enclave entity, same rule on enclave `PlayerFame`; HUD `GO_EnclaveFame` |
| 61 | Grant | create items ID1 x ID2 in the entity's inventory |
| 62 | Humanize | NPC Immortal := false (when E1 != player) |
| 63 | JoinNPC | adds NPC E2 to the player's party (existing member of that kind is dismissed via `DismissPlayerParty`), sets PartyMemberType and flags |
| 64 | GiveAll | `GeneratePlunderInvFromTreasureSets` then `TransferAllTo` between E1 and E2 |
| 65 | JoinPlayer | switches a party/mob/slave NPC to following the player (task `ZS_FollowPlayer`, stops BringNPC) |
| 67 | ResetCrime | clears enclave crime state: KnownPlayerCrime, suspect comments, status, per-member LastPlayerAR/Crime/Fight, ContinueRoutine |
| 68 | DecreaseRevolCount | political crime count for Orc/Reb/Ass reduced by ID2 and suspect comment reset |

`Pickpocket` (data spelling) is matched case-insensitively by the same id 10. Arguments not decoded in full are I.

---------------------------------------------------------------------------

## 4. RPG formulas

### 4.1 XP and levels [V S:4e800, S:4e160]

- `XPNext(L) = floor((L+2) * 0.5 * 500 * (L+1)) = 250*(L+1)*(L+2)`; the player levels up when cumulative `XP >= XPNext(Level)`.
- Level up: `Level += 1`, `LPAttribs += 10`, and `+1` more LP if the player has skill `Perk_Learn` active. The check runs twice per GiveXP call, so at most two levels per call.
- GiveXP(self, other, amount): receiver must be the player or a party member of the player (party type Party/Slave; Summoned excluded); `self == player` returns without effect. If `self` is the world/None entity (quests, info scripts) XP = `amount * 5`. If the source is an NPC kill with amount -1: base = `level*10` with a minimum of 25 or 50 depending on a creature check, scaled by an engine setup factor (`+0x1f0`): `xp = 5 * round(factor * base/500 + 0.5)` [I on the scaling]. Message `GO_XP + n`.
- Learning points: taught attribute raises cost LP 1:1 (HP/MP/SP give 4 units of attribute per LP point), doubled above value 250 under the engine option (1.6).

### 4.2 Protection [V S:2de50 partially]

`GetProtection(damageType)` reads the matching `PROT_*` attribute (Blade, Impact, Missile, Fire, Ice, Lightning); with the engine option byte (+0x1ea) off: robe + `Perk_LightArmor` doubles it, non-robe + `Perk_HeavyArmor` adds 25 percent, hard cap 90. With the option on the protection comes from NPC-template protection fields divided by 5. The full hit/damage routine `AssessHit` (S:2b580, about 2000 instructions) was surveyed but not decoded: it multiplies damage amount by the hit multiplier, applies perks (`Perk_Assassin`, `Perk_KnockOut`, `Perk_Shield_2`), triggers states `ZS_Unconscious`, `ZS_SitKnockDown`, `ZS_LieKnockDown`, `ZS_PiercedDead/KO`, `ZS_RagDollDead`, `ZS_ParadeStumble`, `ZS_HeavyParadeStumble` and scales by difficulty. [U for exact formula]

---------------------------------------------------------------------------

## 5. Remaining unknowns

- Exact semantics of Crime(0), EnclaveCrime(31), PoliticalCrime(32) and the NotFriend fields (offsets identified, field names not).
- `AssessHit` damage formula, hit chance, magic damage, attribute-to-damage scaling, GiveXP kill scaling constants.
- Body of `gCInfoManager_PS::StartComment`, `OnProcess` (who triggers Important infos and the NPC walk-to-player), `SetPartyLeader`.
- `gEInfoNPCStatus` names (values 0-5 behaviour known).
- Which game-event names are read by conditions; GameEvent storage appears to share `PlayerKnows`.
- Spawn/SetSectorStatus hard-coded per-map branches beyond Gotha/Ardea.
- REA was not opened; all evidence is from address-scoped capstone disassembly (reproducible with `.local/research/rules/g3dis.py <dll> 0x<rva>`).
