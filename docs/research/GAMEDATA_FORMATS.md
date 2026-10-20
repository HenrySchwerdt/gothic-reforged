# Game-data formats: strings, quests, infos (dialogs), templates

Research date: 2026-10-20, Gothic 3 1.75 install. Parsers: `gothic-formats::{gdini, stringtable, quest, info, template}`;
inspector subcommands `strings`, `quest-survey`, `info-survey`, `template-survey` (JSON summaries written to
gitignored `.local/`). Public leads consulted for orientation only (no code copied): g3dit (georgeto, GPL),
Baltram/rmtools, auronen.cokoliv.eu/gmc. Everything below was verified against the installed files.

Quests, infos and strings are **plain text** (no binary decoding needed). Templates are Genome binary containers.
`Quests.cpt`/`Infos.cpt`/`Templates.cpt` etc. are unsupported archive variants (see `INSTALLATION.md`); only `.pak` is read.

## Coverage summary

| Format | Files | Parsed | Notes |
| --- | ---: | ---: | --- |
| `Strings.pak/stringtable.ini` | 1 (22 255 keys x 10 languages) | 1 | 6 keys irregular (see below), 0 duplicates |
| `Quests.pak/*.quest` | 570 | **570** | 0 anomalies; all log topics found in string table; all `FinishedQuests` resolve |
| `Infos.pak/*.info` | 4591 | **4591** | 7 anomaly records in 5 files (see below) |
| `Templates.pak/*.tple` | 6295 | 6294 | 1 corrupt file in the shipped data; semantic values opaque |

## String table (`Strings.pak`)

`stringtable.ini`: UTF-16LE with BOM `FF FE`, CRLF. Sections: `[LocAdmin_Languages]`
(`Languages=English;Italian;French;German;Spanish;Czech;Hungarian;Polish;Russian;TRC`, `CurrentLanguage=English`,
`CurrentRevision=0`), `[LocAdmin_Strings]`, and an empty `[LocAdmin_Revisions]`.
Each string line is `KEY=t0;;t1;;...;;t9;` (one text per language in `Languages` order, last terminated by one `;`).
Literal backslash-n (`\n`) inside text means line break; `stringtable::unescape` converts. All ten languages are
present in this install regardless of the audio/text language setting (German text is column 3).
The Chinese column is `TRC` (traditional).

Key families (counts): `INFO_*` 12 217 (dialog lines), `SVM_*` 4 791 (stock voice lines), `FO_*` 2 928 (entity
display names, e.g. items/objects), `ITEMDESC_*` 918, `QUEST_*` 680 (quest titles, plus quest log entry texts),
`HUD_*` 245, `GO_*` 80, `Info_*` 59, `HL_*`, `LETTERTEXT_*`, `BOOKTEXT*`, `ATTRIB*`, `TUT_*`, `MAP_*`.
Empty texts per language: English 996, German 0, Russian 3 ... i.e. German is the complete master column.

Irregularities (reported, not hidden): six `SVM_Orc_*` keys contain a literal `;` inside a text (e.g. `; ;` in the Italian
text) so the `;;` split yields a wrong count; they are listed in `StringTable::irregular_keys` and padded/truncated
rather than rejected. Other files in `Strings.pak`: `SVMTable.ini` (plain-text `KEY=(null);(null);0;SVM_...`, mapping,
not parsed), `SVMAdmin.dat` / `TeacherSVMs.dat` (Genome-envelope binaries with UTF-16 strings, not parsed).
`Projects_compiled.pak/G3_World_01/G3_World_01_StringTable.ini` is an editor-side per-world table (ASCII ini with
hex-encoded UTF-16 texts, German only) and is not parsed.

## Quests (`Quests.pak`, `*.quest`)

570 files at `G3_World_01/<Name>_quest_G3_World_01.quest`. Text INI, header `[Quest]`, CRLF, Latin-1/ASCII, one record per
file. A key with no value is written without `=` (`FinishedQuests`). Fields (all present in all files unless noted):
`Name`, `Type`, `FinishedQuests` (`;` list of prerequisite quest names), `LogTopic` (string-table key, resolves for
570/570), `LogText`, `ExperiencePoints` (sum 207 525), `PoliticalSuccess[Amount]`, `EnclaveSuccess[Amount]`,
`JobSuccess[Amount]`, `Folder`, `DeliveryEntities` / `DeliveryAmounts` / `DeliveryCounter` (parallel `;` lists:
item or NPC entity names, required counts, current counters), `DestinationEntity`, `RunningTimeYears/Days/Hours`,
`AttribSuccess[Amount]`; and in one file (`Lardo_SpellQuestTest`) `SpellName`, `QuestDescription`.

Decoded: structure, types, lists, cross references. **Opaque:** meaning of `Type` (0..13; histogram 0:157, 1:130, 2:114,
7:57, 5:35, 10:31, 6:20, 3:10, 8:8, 11:4, 4:3, 13:1), `PoliticalSuccess` (0..6, probably a faction id) and
`JobSuccess` (0/2/3/6): preserved as raw integers. Quest *state changes are not in these files*: they are executed by
`.info` scripts (`RunQuest`, `SucceedQuest`, `CloseQuest`, `AddQuestLog`, `Description`). Quest log text lives in the string
table (`QUEST_*`, plus `Description` command texts `INFO_*`).

## Infos / dialog (`Infos.pak`, `*.info`)

4591 files at `G3_World_01/<NAME>_info_G3_World_01.info` (names like `SKALVERAM3575`, authors' tool prefixes).
Text INI, header `[Info]`. One info = one dialog node owned by an NPC (`Owner`; 414 distinct owners; `sh` is a story-helper
pseudo-owner), with a tree link (`Parent`), a quest tag (`Quest`, 2846 references, all but one resolve:
`Innos_Power` is undefined), a condition, and a script.

Condition fields: `ConditionType` (36 distinct raw values, most frequent 3 = 1723, 6 = 539, 10 = 505, 7 = 398, 8 = 349, 19 = 302),
`InfoType` (0..5, 7; 3 = 3808), `CondPlayerKnows` / `CondPlayerKnowsNot` (game events, see below), `CondItemContainer` +
`CondItems` + `CondItemAmounts` (parallel lists), `CondSecondaryNPC` + `CondSecondaryNPCstates`,
`CondOwnerNearEntity`, `CondPlayerPartyMember`, `GoldCost`, `Permanent`, `ClearChildren`, `SuppressLog`, `InfoGiven`
(always `false` in shipped data: it is runtime state), `TeachAttrib/TeachAttribValue/TeachSkill*` (teacher dialogs;
5 files carry extra `CondPlayerSkills`/`PlayerSkillsIndex`/... keys preserved in `extra`).

Script: parallel `;` arrays `InfoScript_Commands`, `_Entities1`, `_Entities2`, `_IDs1`, `_IDs2`, `_Texts`, joined by index
into `info::ScriptStep`. Trailing separators are inconsistent (commands have none, others have one), so lists are positional
with a single trailing empty element dropped. `Say` texts are `INFO_*` string-table keys: 13 404 references, 13 222 (98.6 %)
resolve; the 182 misses are mostly placeholder values (`INC`/`DEC` used as a parameter, `INFO_QUESTLOG_` with an empty suffix)
and stale keys from editing. Entities1 is `player`, `npc`, `owner` or a named entity.

Command vocabulary (count): Say 12 547, SetGameEvent 1354, Description 623 (quest log text), Give 584, SaySVM 467,
GiveXP 346, End 271, SetRoutine 247, ClearGameEvent 147, SetTeachEnabled 143, SetTradeEnabled 139, ModReputEnc 138,
Attack 83, CloseQuest 76, RunQuest 57, ShowSubtitle 56, InfoWait 51, ShowPicture 49, SetSectorStatus 45,
SetSlaveryEnabled 44, SetPartyEnabled 41, SetPAL 38, ModReputPol 36, Sleep 24, Use 23, StartEffect 23, SetGuardStatus 21,
ResetCrime 21, SucceedQuest 20, SayKeepItUp 18, plus ~30 rarer commands (full histogram in `.local/info-survey.json`).
Argument conventions seen in samples: `RunQuest`/`SucceedQuest`/`CloseQuest` id1 = quest name; `SetGameEvent`/`ClearGameEvent`
id1 = event name; `SetRoutine` id1 = routine name; `SetSectorStatus` id1 = sector (group) name, id2 = `true|false`;
`Give` id1 = item template name, id2 = amount. These are observed conventions, not verified runtime semantics.

**Game events** are the global flag system that links dialog and quests: 1155 distinct events are set by `SetGameEvent`, 1141 are
required by `CondPlayerKnows[Not]`; 35 required events are never set by any shipped info (most are `*_test`/`bug` leftovers;
presumably set from script code or runtime), listed in the survey.

Anomalies (7 records in 5 files, all parsed, reported with exact reasons): `FILLER47` has a stray `y` before `[Info]`; `SKALVERAM3879` has a
duplicate `Permanent` key; `NEWDM0009` and `SKALVERAM3575` have keys with embedded `;` (`InfoScript_IDs2;;;;;;;;`, `TeachSkill;`);
`SKALVERAM31087` has script arrays longer than its command list.

Opaque: numeric `ConditionType`/`InfoType` meanings; per-command argument semantics beyond the conventions above; runtime evaluation
order and dialog UI behaviour.

## Templates (`Templates.pak`, `*.tple`)

6295 `.tple` plus 325 each of `.lrtpl`/`.lrtpldat` (layer templates) and 328 `.lrtpldatasc` (text lists
`{guid}:0=Name.tple`). `.tple` variants (all validated by the survey): 6129 `GENOMFLE` envelope with a `GENOMETP` body and trailing
string table, 115 bare `GENOMETP` files with a leading string table (`GENOMETP`, u16 version 58..62, `01 00 01`, u32 count, u16-prefixed
strings), 50 legacy raw files (`02 00 00 00`, ..., entity name at fixed offset 42). One file
(`SFX/.../sfx_env_water_waterfall_03.tple`) is corrupt in the shipped data (first string length wrong, trailer overruns the file).
The string table begins with the template file name and the **entity name** (e.g. `It_FiremageCup`) followed by class names
(68 distinct; most common `eCEntityProxy` 4532, `eCIlluminated_PS` 4284, `eCTemplateEntityProxy` 4245, `gCInteraction_PS` 3827,
`eCCollisionShape_PS`, `gCItem_PS` 2345, `gCInventory_PS` 1583, `gCNPC_PS` 1280, `gCDamage_PS`, `gCScriptRoutine_PS`) and property/type names.
5608 templates are outside `_deleted/`; 25 entity names occur twice. Of 165 item names referenced by quests/infos
(`DeliveryEntities` `It_*`, `CondItems`, `Give`) 162 resolve to a template entity name; 3 do not (`It_Abbas_Artefakt`,
`It_Chalice_Adanos`, `It_Chalice_Innos`).

**Opaque:** all property values, i.e. item stats, NPC attributes, inventories (`gCInventory_PS` stacks), treasure sets,
script routines, damage tables. The property stream is the same persistence format as world nodes, with per-class
schemas; decoding it is the next big task (shared with `world_node.rs` owners). Do not invent stats.

## Reproduce

```
I=target/debug/gothic-inspector; D="D:/SteamLibrary/steamapps/common/Gothic 3/Data"
$I strings $D/Strings.pak [--key K] [--prefix P] [--lang L] [--out F] [--quiet]
$I quest-survey $D/Quests.pak --strings $D/Strings.pak --out .local/quest-survey.json
$I info-survey  $D/Infos.pak  --strings $D/Strings.pak --quests $D/Quests.pak --out .local/info-survey.json
$I template-survey $D/Templates.pak --quests $D/Quests.pak --infos $D/Infos.pak --out .local/template-survey.json
```

## What is needed next for a quest-capable runtime

1. A game-event store plus an info evaluator (conditions -> available dialog options -> execute script steps). All inputs are now
   structured data; the numeric `ConditionType`/`InfoType` meanings must be recovered (from `Game.dll` via REA, or by correlating
   conditions with field combinations: e.g. type 3 correlates with plain `CondPlayerKnows`-gated dialog).
2. NPC/world entity data (`.lrentdat` sector files) to know *where* `Owner` NPCs are; `SetSectorStatus` ties scripts to the
   sector list in `G3_World_01.wrldatasc`.
3. Item/NPC template property decoding (item stats, inventories) for `Give`, `CondItems`, trading and teaching.
4. Speech: `.xlip` lip-sync/`.ogg` voice files in `Speech_*.pak` are probably keyed by the same `INFO_*` names (unverified; not examined).
