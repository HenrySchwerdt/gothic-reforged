# Genome property streams: typed values, tails, gameplay records

Recorded 2026-10-20 against Gothic 3 1.75.20108.14 (read-only install). Code: `gothic_formats::{propstream, props::{tails, enums, records}}`,
`template::TemplateDocument`, `world_node::WorldDocument::parse_with_properties`. Inspector: `prop-survey`, `tple-dump`, `world-props`,
`item-dump`, `npc-dump`, `skill-dump` (all write under the gitignored `.local/`). Leads consulted for orientation only, no code copied:
georgeto/g3dit (GPL) property/class sources, `class-schemas.json` (static extraction, see `GAME_LOGIC_MAP.md`).

Evidence legend: **[V]** verified by byte-exact decoding of the whole installation (a decode is accepted only if it consumes its length frame
exactly, otherwise the payload stays `Opaque` and is counted); **[I]** inferred (naming, correlation, one external lead) and not proven.

## 1. Stream grammar [V]

A property set (class instance) is `u16 object_version (0x53; older 0x52/0x51/1 occur)`, `u32 size`, `u16 list_version (30)`, `u32 count`,
`count` properties, then the class **tail** up to `size`: `u16 class_version` followed by class-specific data.
Property = `u16 name_idx`, `u16 type_idx`, `u16 magic (30)`, `u32 length`, payload. Strings are `u16` indices into the document string table.
The payload layout depends **only on the type string**, and `length` frames it, so the stream can never desynchronise. Object versions below
0x53: 0x52 has the same layout; 0x51 and 1 carry an extra legacy 20-byte object id (and, for 1, a name entry) before the list (108 templates).

Wrappers: world entity property set = `u16 wrapper_version`, accessor `01 00 01 01 00 01`, class name idx, filler `01 00 00 vv 00`, object, `0xDEADC0DE`.
Template class entry = `u16 version`, `u32 pre-size`, `name idx`, then the same accessor/name/filler/object, `0xDEADC0DE`. Nested objects
(`bTPropertyObject<class X,...>`, smart-pointer arrays) start directly at the object version. `bTArray` payloads carry one unused prefix byte, a
`u32` count, then elements.

Template container [V]: `GENOMFLE` envelope whose body is `GENOMETP`, `u16 version (58..62)`, `u16 1`, `u8 indexed`, `u32 n`, `n x u16`
string-table indices (identity in all but remapped in principle), `u32 code size`, `u32 header count`, entity headers (versions 40..62, per-version
fields as in `TemplateEntity`), `(parent, child)` pairs, `ffffffff ffffffff`. Header 0 is the management/"item" header (`helper_parent`), header 1
the reference header carrying the property sets. Both header GUIDs resolve to the template entity name (1974 of 1974 `eCTemplateEntityProxy`
and inventory-stack references in `Templates.pak` resolve; 0 dangling).

## 2. Value layouts by type string [V]

| Type string | Payload |
| --- | --- |
| `bool` | `u8` 0/1 (strict) |
| `char`, `unsigned char` | `u8` |
| `short`, `unsigned short` | 2 bytes |
| `int`, `long` / `unsigned long` | 4 bytes (`long` is 32-bit) |
| `float` | f32 |
| `bCString`, `bCScriptString`, `eCLocString`, `bC{Mesh,Animation,Image,ImageOrMaterial,SpeedTreeResource}String` | `u16` string-table index |
| `bTPropertyContainer<enum T>` | `u16 version (1)` + `i32` enumerator value |
| `bCVector`, `bCEulerAngles` / `bCVector2` | 3 / 2 x f32 |
| `bCFloatColor` / `bCFloatAlphaColor` | 4-byte vtable slot + 3 / 4 x f32 |
| `bCRange1`, `bCRange3`, `bCRect`, `bCBox`, `bCMatrix` | 2, 6, 4, 6, 16 x f32 |
| `bCGuid`, `bCPropertyID` | 20 bytes (16-byte GUID + validity word) |
| `eCEntityProxy`, `eCTemplateEntityProxy` | `u16 version`, `bool`, 20-byte GUID if set |
| `eCPropertySetProxy` | `u16`, `bool` + name idx, `u16`, `bool` + GUID |
| `bTObjArray<class bCString>`, `bTValArray<float / long / unsigned long / unsigned short / bool / class bCVector / class bCPropertyID / enum gEDirection>` | array |
| `bTObjArray<class eCEntityProxy / eCTemplateEntityProxy>` | array of proxies |
| `bTValArray<class bCMotion>` | array of 7 x f32 (position, quaternion) |
| `bTObjArray<struct gCQuest_PS::SLogEntry>` | array of `u16`, idx, idx (heading, text) |
| `bTPropertyObject<class X, ...>` | nested object |
| `bTObjArray<class bTAutoPOSmartPtr<class X> >` | array of (`u16`, `bool`, object) |
| `bTPtrArray<class eCEntityProxy *>` | empty payload (15 instances, always empty) |

## 3. Coverage [V]

| | Templates (`Templates.pak`) | World (`Projects_compiled` `.node` + `.lrentdat`) |
| --- | ---: | ---: |
| Files | 6295 | 2536 |
| Decoded | **6244** (50 legacy raw `02 00 00 00` + 1 corrupt unsupported) | **2512** (24 failures are pre-existing, see `WORLD_SURVEY.md`) |
| Entities | (1 to 2 headers per template) | 231 126 |
| Property sets | 42 459 | 764 191 |
| Property instances | 719 363 | 8 443 780 |
| **Typed** (exact length match) | **100.000 %** | **100.000 %** |
| Distinct type strings | 132 | 118 |

The first pass left only `bCFloatColor`/`bCFloatAlphaColor` (vtable word), `bTValArray<enum gEDirection|class bCMotion>`, `SLogEntry` arrays and the
empty `bTPtrArray` untyped (99.81 % templates, 99.993 % world); all are now decoded. Typed means the layout is consistent with the length frame for every
instance in the install, not that the field's meaning is understood. The 50 legacy raw templates (all in `_deleted/`, `Test/Particles`, `Navigation`,
plus old duplicates such as `Artefacts_It_FiremageCup`, `Myrtana_Nomad`) use a pre-Genome layout and are not decoded; all gameplay entities are in the
supported variant.

Tails: 5 classes have dedicated decoders (below). Of the property-set tails, render/physics classes (`eCIlluminated_PS`, `eCCollisionShape_PS`,
`eCRigidBody_PS`, `eCVisualMesh*_PS`, `eCSpringAndDamperEffector`, `eCParticle_PS`, `eCVegetation_PS`, `gCProjectile_PS`, ...) are skipped as raw bytes
by design (geometry/physics work owns them). Dedicated decoders succeed on: `gCInventory_PS` 1571/1583 templates, `eCVisualAnimation_PS` 1990/2007 templates (the failing templates are 14 `_deleted/` entries, two
`Testzeug_*` door testers and `Monster_Master_Master_ bison`), 100 % in the world data
(all NPC inventories and visuals decode), `gCItem_PS` 100 %, `gCParty_PS`/`gCEnclave_PS`/`gCAnchor_PS` 100 %. Every other class in the NPC/item
data (`gCNPC_PS`, `gCNavigation_PS`, `gCDamage_PS`, `gCDamageReceiver_PS`, `gCDialog_PS`, `gCSkill_PS`, ...) has **no tail beyond the class
version**, so its full state is in the properties.

## 4. Class tails

| Class | Tail layout | Status |
| --- | --- | --- |
| `gCInventory_PS` | `u16`, `u32 n` stacks (`gCInventoryStack` sub-objects: Amount, Quality, QuickSlot, Generated (gEStackType), Template, Item), `u16`, `u32 19` slots (empty `01 00 00` or `gCInventorySlot`: Template, Item, Slot), 5 cached `eCEntityProxy` | [V] 100 % in world data |
| `gCItem_PS` | 24 constant bytes, `i32 slot`, `u8 visible`, `bTArray` of script lines (`u16`, command, entity1, entity2, id1, id2), `u8 executed` | [V] frame, [I] meaning of the 24-byte block and `slot` |
| `eCVisualAnimation_PS` | `u16 5`, skeleton slot, `u32` named body-part slots (head/body/...: `.fxa` + optional animated variant + material switch), marker, `u32` string entries, `u32` attachments (GUID, bone name, 72 bytes), bounding box | [V] |
| `gCParty_PS`, `gCEnclave_PS`, `gCAnchor_PS` | entity-proxy array (members / interact points) | [V] |
| any other `eCEntityPropertySet`-derived class with a 3-byte tail | `u16 version`, `bool` | [I] |

## 5. Enum names [V extraction, I where flagged]

`props::enums` embeds the 120 enums (1124 enumerators) from `class-schemas.json` (names are registration-table data, not code). 35 enumerators were
inferred by sequence and are listed in `enums::INFERRED` (e.g. `gEGender::Female`, `gEDamageType::Impact`). Not extractable statically:
`EAttribModOperation`, `EAttribReqOperation`, `eERigidbody_Flag`, `gEDirection`, `gESession_State`, `bENoiseTurbulence`; values stay numeric.
Cross-check that the extraction is right: every sword has `gEDamageType::Blade` and `gEItemCategory::Weapon`; the Ardea slaves are
`gEPoliticalAlignment::Slave` and the rebels `Reb`, orcs `Orc`.

## 6. Typed records (`props::records`)

* `ItemTemplate` (from `gCItem_PS` + `gCDamage_PS` + `gCInteraction_PS.UseType` + visual): name (`It_*`), category, use type, gold **value**, amount, quality,
  flags (mission/permanent/robe), **damage** (type + amount, weapons), **requirements** (`STR`/`DEX`/`INT` tag, op, value), required skills, **modifiers** (tag/op/value,
  armour `PROT_*`, consumable `HP`/`MP`/`SP`, attribute boosts `STR`/`DEX`/...), activate skills (perks), spell/skill/armor-set links, use-effect script lines, mesh and icon.
* `NpcTemplate` / `NpcEntity` (shared `NpcData`): name, level/level max, class, species, gender, type, **guild** (= `PoliticalAlignment` [I]), enclave id, voice,
  role description (UTF-8 repaired), attitude, guard status, inventory (treasure sets, stacks, equipped slots incl. body parts), routine (`gCNavigation_PS` routine
  names, start position, work/relax/sleep point ids; `gCScriptRoutine_PS` routine), actor visual (skeleton `.fxa`, head/body `.fxa`), dialog flags (trade/teach/party),
  and for world entities the document, index, source id and world position.
* `SkillTemplate` (perks): tag, category, learn points, requirements, required skill.
* `TemplateCatalog`: GUID/name index used for every reference.

Exports (reproduce: `gothic-inspector item-dump|npc-dump|skill-dump "<G3>/Data"`; display names come from `Strings.pak` `FO_<name>` / `ITEMDESC_<name>`):
2121 item templates (126 weapons, 89 armours, 107 alchemy, 88 artefacts, 80 written, 39 spellbooks, 71 skill items, 160 misc, 1361 body parts/hair),
1192 NPC templates, 10 279 world NPC entities in 68 documents, 72 skill templates.

## 7. Sanity checks against known game facts

* Item names, categories and display names line up with the string table (`It_1H_Sword_01` is "Sword", 50 damage Blade, value 50; `Body_AssMage` is the "Elite
  Hashishin armor", blade/impact 60, missile 40, fire/ice/lightning 20 plus the two perks `Perk_ResistHeat`, `Perk_ImmuneToPoison`) [V data, plausible vs. game].
* Weapon strength requirement is exactly twice the damage for 47 of 72 weapons with both fields (e.g. 70/140, 85/170, 120/240); the other 25 are unique or story
  weapons (e.g. `It_2H_DemonSword_01` requires STR 666). That regularity would not survive a mis-decoded field [I as a design rule].
* Ardea (`G3_Myrtana_01_Ardea_NPC_01/02.lrentdat`, 61 NPC entities) reproduces the occupation scene: town leader `Hamlar` (slave, level 15/50), `Ardea_Orkboss`, the
  15 `Orc_GameStartRaider_*` orcs (10 scouts level 5, 5 warriors level 10; templates `NPC/Myrtana/Ardea_City_Orc_GameStartRaider_Scout_01..10`, `Warrior_01..05`),
  `Slave_01..08`, `FreeSlave_*`, and rebel novices/warriors/elites. This uses the same names as the templates in `Templates.pak`.
* Armour/weapon values are monotone with tier (novice, warrior, mage armours 20/40/60 blade) as expected.

## 8. Open issues

1. **NPC combat attributes are not data.** Templates and world instances only carry `Level`/`LevelMax` (0..50), `Class`, `Species` and the runtime
   `gCDamageReceiver_PS` points (1/1 placeholders in 10 254 of 10 279 entities). `gCStatistics_PS` occurs in a single deleted template. STR/DEX/HP/MP/SP of
   an NPC must be derived at spawn by the compiled script (`Script_Game.dll`, class/species/level tables); recovering that is the next REA task.
2. `EAttribModOperation` / `EAttribReqOperation` values are unresolved. Observed: requirement op is always 0 (112 of 112); modifier op 0 (318) covers all `PROT_*` armour values, attribute boosts
   (`STR`/`DEX`/`INT`/`THF`/`ALC`) and a few `HP`/`SP`/`MP`; ops 1 (39), 2 (63) and 4 (1, `It_FakeJoint`) occur only on `HP`/`MP`/`SP`. Probably 0 = permanent add, 2 = restore
   current points (apple +5 HP, booze +50 SP), 1 = a second restore/regen mode [I].
3. `gCItem_PS` tail: the constant 24-byte block and `slot`/`visible` semantics are unproven; use-effect script lines exist on only 6 templates, so most consumable effects
   are `ModAttrib` entries plus `UseType`, with behaviour in compiled script.
4. `gCMagic_PS` (spells), `gCRecipe_PS`, `gCTreasureSet_PS` (loot tables, referenced by name from `TreasureSet1..5`) are decoded as generic property sets but have no typed
   record yet. Treasure set names (e.g. `TS_Weaponry_Reb_1H_Bow`) are strings; their content templates are `gCTreasureSet_PS` templates ready for a record.
5. Routine point ids (`WorkingPoints` etc.) are `bCPropertyID`s of helper entities in the world; resolving them to positions needs the world's entity id index.
6. 50 legacy raw templates, 1 corrupt template and the 24 pre-existing world failures remain undecoded; none of them is an NPC or item used by quests.
7. Spawn linkage: world NPC entities do not expose their creator template GUID yet (`world_node` skips it); NPC to template matching is by entity name.
