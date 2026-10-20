//! Typed gameplay records assembled from decoded property sets: items and NPCs.
//!
//! These are *data extraction* views: every field is read from a named property or tail of the
//! shipped data (see `docs/research/PROPERTY_STREAMS.md` for which meanings are verified and
//! which are inferred). Nothing is simulated here.
use crate::props::{enums, tails::ScriptLine, tails::Tail};
use crate::propstream::{PropValue, PropertySet};
use crate::template::{TemplateDocument, TemplateEntity};
use serde::Serialize;
use std::collections::HashMap;

/// Name/GUID index over all templates, so entity references (`eCTemplateEntityProxy`, inventory
/// stacks) resolve to template names.
#[derive(Debug, Default, Clone)]
pub struct TemplateCatalog {
    by_guid: HashMap<String, usize>,
    names: Vec<String>,
    paths: Vec<String>,
}

impl TemplateCatalog {
    /// Register a parsed template; both header GUIDs resolve to the entity name.
    pub fn insert(&mut self, path: &str, doc: &TemplateDocument) {
        let Some(header) = doc.reference_header() else {
            return;
        };
        let id = self.names.len();
        self.names.push(header.name.clone());
        self.paths.push(path.to_owned());
        for e in &doc.entities {
            self.by_guid.entry(e.guid.clone()).or_insert(id);
        }
    }
    pub fn name_of(&self, guid: &str) -> Option<&str> {
        self.by_guid.get(guid).map(|&i| self.names[i].as_str())
    }
    pub fn path_of(&self, guid: &str) -> Option<&str> {
        self.by_guid.get(guid).map(|&i| self.paths[i].as_str())
    }
    pub fn len(&self) -> usize {
        self.names.len()
    }
    pub fn is_empty(&self) -> bool {
        self.names.is_empty()
    }
    /// Resolve to the template name, or the raw GUID when unknown.
    pub fn resolve(&self, guid: &str) -> String {
        self.name_of(guid).unwrap_or(guid).to_owned()
    }
}

fn enum_name(set: &PropertySet, prop: &str) -> Option<(String, i32)> {
    match set.get(prop)? {
        PropValue::Enum { ty, value } => Some((
            enums::name(ty, *value).map_or_else(|| format!("#{value}"), str::to_owned),
            *value,
        )),
        _ => None,
    }
}

/// Free-text fields hold UTF-8 bytes that the Latin-1 string table view turns into mojibake
/// (`Ã¤` for `ä`); re-decode when the bytes form valid UTF-8.
pub fn repair_text(s: &str) -> String {
    if s.is_ascii() || s.chars().any(|c| u32::from(c) > 0xff) {
        return s.to_owned();
    }
    let bytes: Vec<u8> = s.chars().map(|c| c as u8).collect();
    String::from_utf8(bytes).unwrap_or_else(|_| s.to_owned())
}

fn proxy(set: &PropertySet, prop: &str, catalog: &TemplateCatalog) -> Option<String> {
    set.get(prop)
        .and_then(PropValue::as_ref_guid)
        .map(|g| catalog.resolve(g))
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct AttributeRequirement {
    /// Attribute tag as stored (`STR`, `DEX`, `INT`, ...).
    pub tag: String,
    /// Raw `EAttribReqOperation` value (names are not extractable statically).
    pub op: i32,
    pub value: i32,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct AttributeModifier {
    pub tag: String,
    /// Raw `EAttribModOperation` value.
    pub op: i32,
    pub value: i32,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct DamageInfo {
    pub damage_type: String,
    pub amount: i32,
    pub mana_multiplier: f32,
    pub hit_multiplier: f32,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct ItemTemplate {
    /// Entity name used by scripts, quests and infos (`It_1H_Sword_01`).
    pub name: String,
    pub file: String,
    pub path: String,
    pub guid: String,
    pub category: String,
    pub category_id: i32,
    /// `gCInteraction_PS.UseType` (`1H`, `2H`, `Bow`, `Potion`, ...), the equip/use behaviour.
    pub use_type: Option<String>,
    /// Gold value in the data.
    pub value: i32,
    pub amount: i32,
    pub quality: i32,
    pub mission_item: bool,
    pub permanent: bool,
    pub robe: bool,
    /// Weapon damage block (`gCDamage_PS`), only for templates that carry one with amount > 0.
    pub damage: Option<DamageInfo>,
    pub requirements: Vec<AttributeRequirement>,
    pub required_skills: Vec<String>,
    pub modifiers: Vec<AttributeModifier>,
    pub activate_skills: Vec<String>,
    pub spell: Option<String>,
    pub skill: Option<String>,
    pub armor_set: Option<String>,
    /// Item use script lines (`gCItem_PS` tail).
    pub use_effects: Vec<ScriptLine>,
    /// Mesh of the world/inventory visual (`eCVisualMesh*_PS.ResourceFileName`).
    pub mesh: Option<String>,
    /// Inventory icon texture.
    pub icon: Option<String>,
}

fn indexed<'a>(
    set: &'a PropertySet,
    prefix: &str,
    suffix: &str,
    n: usize,
) -> Option<&'a PropValue> {
    set.get(&format!("{prefix}{n}{suffix}"))
}

impl ItemTemplate {
    /// Build from a template document; `None` when it has no `gCItem_PS`.
    pub fn from_document(
        path: &str,
        doc: &TemplateDocument,
        catalog: &TemplateCatalog,
    ) -> Option<Self> {
        let header: &TemplateEntity = doc.entities.iter().find(|e| e.set("gCItem_PS").is_some())?;
        let item = header.set("gCItem_PS")?;
        let (category, category_id) = enum_name(item, "Category").unwrap_or_default();
        // Op properties repeat the same name in the stream, so take them positionally.
        let ops = |prefix: &str| -> Vec<i32> {
            item.properties
                .iter()
                .filter(|p| p.name.starts_with(prefix) && p.name.ends_with("Op"))
                .filter_map(|p| p.value.as_i32())
                .collect()
        };
        let req_ops = ops("ReqAttrib");
        let mod_ops = ops("ModAttrib");
        let mut requirements = Vec::new();
        for n in 1..=4 {
            let tag = indexed(item, "ReqAttrib", "Tag", n)
                .and_then(PropValue::as_str)
                .unwrap_or_default();
            if tag.is_empty() {
                continue;
            }
            requirements.push(AttributeRequirement {
                tag: tag.to_owned(),
                op: req_ops.get(n - 1).copied().unwrap_or(0),
                value: indexed(item, "ReqAttrib", "Value", n)
                    .and_then(PropValue::as_i32)
                    .unwrap_or(0),
            });
        }
        let mut modifiers = Vec::new();
        for n in 1..=6 {
            let tag = indexed(item, "ModAttrib", "Tag", n)
                .and_then(PropValue::as_str)
                .unwrap_or_default();
            if tag.is_empty() {
                continue;
            }
            modifiers.push(AttributeModifier {
                tag: tag.to_owned(),
                op: mod_ops.get(n - 1).copied().unwrap_or(0),
                value: indexed(item, "ModAttrib", "Value", n)
                    .and_then(PropValue::as_i32)
                    .unwrap_or(0),
            });
        }
        let refs = |names: &[&str]| -> Vec<String> {
            names
                .iter()
                .filter_map(|n| proxy(item, n, catalog))
                .collect()
        };
        let damage = header.set("gCDamage_PS").and_then(|d| {
            let amount = d.int("DamageAmount")?;
            (amount != 0).then(|| DamageInfo {
                damage_type: enum_name(d, "DamageType").map(|x| x.0).unwrap_or_default(),
                amount,
                mana_multiplier: d.float("DamageManaMultiplier").unwrap_or(1.0),
                hit_multiplier: d.float("DamageHitMultiplier").unwrap_or(1.0),
            })
        });
        let mesh = ["eCVisualMeshDynamic_PS", "eCVisualMeshStatic_PS"]
            .iter()
            .filter_map(|c| header.set(c))
            .find_map(|s| s.string("ResourceFileName").filter(|v| !v.is_empty()))
            .map(str::to_owned);
        let use_effects = match &item.tail {
            Tail::Item { script, .. } => script.clone(),
            _ => Vec::new(),
        };
        Some(Self {
            name: header.name.clone(),
            file: header.file_name.clone(),
            path: path.to_owned(),
            guid: header.guid.clone(),
            category,
            category_id,
            use_type: header
                .set("gCInteraction_PS")
                .and_then(|i| enum_name(i, "UseType"))
                .map(|x| x.0),
            value: item.int("GoldValue").unwrap_or(0),
            amount: item.int("Amount").unwrap_or(1),
            quality: item.int("Quality").unwrap_or(0),
            mission_item: item.boolean("MissionItem").unwrap_or(false),
            permanent: item.boolean("Permanent").unwrap_or(false),
            robe: item.boolean("Robe").unwrap_or(false),
            damage,
            requirements,
            required_skills: refs(&["RequiredSkill1", "RequiredSkill2"]),
            modifiers,
            activate_skills: refs(&["ActivateSkill1", "ActivateSkill2"]),
            spell: proxy(item, "Spell", catalog),
            skill: proxy(item, "Skill", catalog),
            armor_set: proxy(item, "ArmorSet", catalog),
            use_effects,
            mesh,
            icon: item
                .string("Texture")
                .filter(|t| !t.is_empty())
                .map(str::to_owned),
        })
    }
}

// ---------------------------------------------------------------------------------------------
// Skills (perks, crafting skills): `gCSkill_PS` templates
// ---------------------------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct SkillTemplate {
    pub name: String,
    pub path: String,
    pub guid: String,
    /// Short skill tag (`1H_1`).
    pub tag: String,
    /// `gESkillCategory` (Combat, Hunting, Magic, Smithing, Theft, Alchemy, Misc, Cooking).
    pub category: String,
    pub learn_points: i32,
    pub requirements: Vec<(String, i32)>,
    pub required_skill: Option<String>,
    pub sort_order: i32,
}

impl SkillTemplate {
    pub fn from_document(
        path: &str,
        doc: &TemplateDocument,
        catalog: &TemplateCatalog,
    ) -> Option<Self> {
        let header = doc
            .entities
            .iter()
            .find(|e| e.set("gCSkill_PS").is_some())?;
        let skill = header.set("gCSkill_PS")?;
        let requirements = (1..=4)
            .filter_map(|n| {
                let tag = indexed(skill, "ReqAttrib", "Tag", n)?.as_str()?;
                let value = indexed(skill, "ReqAttrib", "Value", n)?.as_i32()?;
                (!tag.is_empty()).then(|| (tag.to_owned(), value))
            })
            .collect();
        Some(Self {
            name: header.name.clone(),
            path: path.to_owned(),
            guid: header.guid.clone(),
            tag: skill.string("Tag").unwrap_or_default().to_owned(),
            category: enum_name(skill, "Category")
                .map(|x| x.0)
                .unwrap_or_default(),
            learn_points: skill.int("LearnPoints").unwrap_or(0),
            requirements,
            required_skill: proxy(skill, "ReqSkill1", catalog),
            sort_order: skill.int("SortOrder").unwrap_or(0),
        })
    }
}

// ---------------------------------------------------------------------------------------------
// NPCs
// ---------------------------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct InventoryEntry {
    pub item: String,
    pub amount: i32,
    pub quality: i32,
    /// `gEStackType`: Normal / Treasure / Merchandise.
    pub stack_type: String,
    pub quick_slot: i32,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct EquippedItem {
    /// `gESlot` name (`RightHand`, `Armor`, `Head`, ...).
    pub slot: String,
    pub template: String,
    /// Concrete item entity GUID if the slot holds a materialised item.
    pub item_entity: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Default)]
pub struct NpcInventory {
    pub treasure_sets: Vec<String>,
    pub generated_plunder: bool,
    pub generated_trade: bool,
    pub stacks: Vec<InventoryEntry>,
    pub equipped: Vec<EquippedItem>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Default)]
pub struct NpcRoutine {
    /// Active routine name (`gCNavigation_PS.Routine`).
    pub routine: String,
    pub routine_names: Vec<String>,
    pub start_position: Option<[f32; 3]>,
    /// Ids of the work / relax / sleep helper points (resolve through the world).
    pub working_points: Vec<String>,
    pub relaxing_points: Vec<String>,
    pub sleeping_points: Vec<String>,
    /// Script routine function name (`gCScriptRoutine_PS.Routine`), if set.
    pub script_routine: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Default)]
pub struct ActorVisual {
    /// Skeleton resource (`eCVisualAnimation_PS.ResourceFilePath`, an `.fxa`).
    pub skeleton: Option<String>,
    pub facial_animation: Option<String>,
    /// (slot name, first resource, optional animated variant) of the body parts.
    pub body_parts: Vec<(String, String, Option<String>)>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct NpcData {
    pub level: i32,
    pub level_max: i32,
    pub class: String,
    pub species: String,
    pub gender: String,
    pub npc_type: String,
    /// `gEPoliticalAlignment` (the faction used as guild: Orc, Nrd, Reb, Mid, Ass, Nom, Out, Slave, Pirate).
    pub guild: String,
    /// Enclave property id (hex), empty when unset.
    pub enclave: Option<String>,
    pub voice: String,
    pub role_description: String,
    pub attitude_to_player: String,
    pub guard_status: String,
    pub immortal: bool,
    /// Points stored in `gCDamageReceiver_PS` (these are runtime values; templates carry 1/1).
    pub hit_points: Option<(i32, i32)>,
    pub mana_points: Option<(i32, i32)>,
    pub stamina_points: Option<(i32, i32)>,
    /// `gCDamage_PS` (unarmed / natural attack block).
    pub damage: Option<DamageInfo>,
    pub teach_attribs: Vec<String>,
    pub teach_skills: Vec<String>,
    pub trade_enabled: bool,
    pub teach_enabled: bool,
    pub party_enabled: bool,
    pub inventory: NpcInventory,
    pub routine: NpcRoutine,
    pub visual: ActorVisual,
}

impl NpcData {
    /// Build from an entity's property sets; `None` when there is no `gCNPC_PS`.
    pub fn from_sets(sets: &[PropertySet], catalog: &TemplateCatalog) -> Option<Self> {
        let find = |c: &str| sets.iter().find(|s| s.class == c);
        let npc = find("gCNPC_PS")?;
        let en = |s: &PropertySet, p: &str| enum_name(s, p).map(|x| x.0).unwrap_or_default();
        let id = |s: &PropertySet, p: &str| match s.get(p) {
            Some(PropValue::Id(v)) if !v.bytes().all(|b| b == b'0') => Some(v.clone()),
            _ => None,
        };
        let strs = |s: &PropertySet, p: &str| match s.get(p) {
            Some(PropValue::StrArray(v)) => v.clone(),
            _ => Vec::new(),
        };
        let pair = |s: Option<&PropertySet>, a: &str, b: &str| {
            let s = s?;
            Some((s.int(a)?, s.int(b)?))
        };
        let dmg = find("gCDamage_PS");
        let recv = find("gCDamageReceiver_PS");
        let dialog = find("gCDialog_PS");
        let teach_skills = match npc.get("TeachSkills") {
            Some(PropValue::EntityRefArray(v)) => {
                v.iter().flatten().map(|g| catalog.resolve(g)).collect()
            }
            _ => Vec::new(),
        };
        let mut inventory = NpcInventory::default();
        if let Some(inv) = find("gCInventory_PS") {
            for n in 1..=5 {
                if let Some(ts) = inv
                    .string(&format!("TreasureSet{n}"))
                    .filter(|t| !t.is_empty())
                {
                    inventory.treasure_sets.push(ts.to_owned());
                }
            }
            inventory.generated_plunder = inv.boolean("GeneratedPlunder").unwrap_or(false);
            inventory.generated_trade = inv.boolean("GeneratedTrade").unwrap_or(false);
            if let Tail::Inventory { stacks, slots } = &inv.tail {
                for s in stacks {
                    inventory.stacks.push(InventoryEntry {
                        item: s
                            .template
                            .as_deref()
                            .map_or_else(|| "?".to_owned(), |g| catalog.resolve(g)),
                        amount: s.amount,
                        quality: s.quality,
                        stack_type: enums::name("gEStackType", s.generated)
                            .unwrap_or("?")
                            .to_owned(),
                        quick_slot: s.quick_slot,
                    });
                }
                for (slot, template, item) in slots {
                    if let Some(t) = template {
                        inventory.equipped.push(EquippedItem {
                            slot: enums::name("gESlot", *slot as i32)
                                .unwrap_or("?")
                                .to_owned(),
                            template: catalog.resolve(t),
                            item_entity: item.clone(),
                        });
                    }
                }
            }
        }
        let mut routine = NpcRoutine::default();
        if let Some(nav) = find("gCNavigation_PS") {
            routine.routine = nav.string("Routine").unwrap_or_default().to_owned();
            routine.routine_names = strs(nav, "RoutineNames");
            if let Some(PropValue::Vec3(v)) = nav.get("StartPosition") {
                routine.start_position = Some(*v);
            }
            let ids = |p: &str| match nav.get(p) {
                Some(PropValue::IdArray(v)) => v.clone(),
                _ => Vec::new(),
            };
            routine.working_points = ids("WorkingPoints");
            routine.relaxing_points = ids("RelaxingPoints");
            routine.sleeping_points = ids("SleepingPoints");
        }
        routine.script_routine = find("gCScriptRoutine_PS")
            .and_then(|s| s.string("Routine"))
            .filter(|s| !s.is_empty())
            .map(str::to_owned);
        let mut visual = ActorVisual::default();
        if let Some(anim) = find("eCVisualAnimation_PS") {
            visual.skeleton = anim
                .string("ResourceFilePath")
                .filter(|s| !s.is_empty())
                .map(str::to_owned);
            visual.facial_animation = anim
                .string("FacialAnimFilePath")
                .filter(|s| !s.is_empty())
                .map(str::to_owned);
            if let Tail::Animation { body_parts, .. } = &anim.tail {
                for p in body_parts {
                    visual.body_parts.push((
                        p.name.clone().unwrap_or_default(),
                        p.file.clone(),
                        p.file2.clone(),
                    ));
                }
            }
        }
        Some(Self {
            level: npc.int("Level").unwrap_or(0),
            level_max: npc.int("LevelMax").unwrap_or(0),
            class: en(npc, "Class"),
            species: en(npc, "Species"),
            gender: en(npc, "Gender"),
            npc_type: en(npc, "Type"),
            guild: en(npc, "PoliticalAlignment"),
            enclave: id(npc, "Enclave"),
            voice: npc.string("Voice").unwrap_or_default().to_owned(),
            role_description: repair_text(npc.string("RoleDescription").unwrap_or_default()),
            attitude_to_player: en(npc, "AttitudeToPlayer2"),
            guard_status: en(npc, "GuardStatus"),
            immortal: npc.boolean("Immortal").unwrap_or(false),
            hit_points: pair(recv, "HitPoints", "HitPointsMax"),
            mana_points: pair(recv, "ManaPoints", "ManaPointsMax"),
            stamina_points: pair(recv, "StaminaPoints", "StaminaPointsMax"),
            damage: dmg
                .filter(|d| d.int("DamageAmount").unwrap_or(0) != 0)
                .map(|d| DamageInfo {
                    damage_type: en(d, "DamageType"),
                    amount: d.int("DamageAmount").unwrap_or(0),
                    mana_multiplier: d.float("DamageManaMultiplier").unwrap_or(1.0),
                    hit_multiplier: d.float("DamageHitMultiplier").unwrap_or(1.0),
                }),
            teach_attribs: strs(npc, "TeachAttribs"),
            teach_skills,
            trade_enabled: dialog
                .and_then(|d| d.boolean("TradeEnabled"))
                .unwrap_or(false),
            teach_enabled: dialog
                .and_then(|d| d.boolean("TeachEnabled"))
                .unwrap_or(false),
            party_enabled: dialog
                .and_then(|d| d.boolean("PartyEnabled"))
                .unwrap_or(false),
            inventory,
            routine,
            visual,
        })
    }
}

/// An NPC definition from `Templates.pak`.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct NpcTemplate {
    pub name: String,
    pub file: String,
    pub path: String,
    pub guid: String,
    #[serde(flatten)]
    pub data: NpcData,
}

impl NpcTemplate {
    pub fn from_document(
        path: &str,
        doc: &TemplateDocument,
        catalog: &TemplateCatalog,
    ) -> Option<Self> {
        let header = doc.entities.iter().find(|e| e.set("gCNPC_PS").is_some())?;
        Some(Self {
            name: header.name.clone(),
            file: header.file_name.clone(),
            path: path.to_owned(),
            guid: header.guid.clone(),
            data: NpcData::from_sets(&header.property_sets, catalog)?,
        })
    }
}

/// An NPC placed in the world (a node/lrentdat entity carrying `gCNPC_PS`).
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct NpcEntity {
    pub name: String,
    /// World document the entity lives in.
    pub document: String,
    /// Index of the entity in its document.
    pub index: usize,
    pub source_id: String,
    /// World-space position (translation of the world matrix, source units).
    pub position: [f32; 3],
    #[serde(flatten)]
    pub data: NpcData,
}

impl NpcEntity {
    pub fn from_world(
        document: &str,
        index: usize,
        entity: &crate::world_node::WorldEntity,
        sets: &[PropertySet],
        catalog: &TemplateCatalog,
    ) -> Option<Self> {
        Some(Self {
            name: entity.name.clone(),
            document: document.to_owned(),
            index,
            source_id: crate::propstream::hex(&entity.source_id),
            position: [
                entity.world_matrix[3][0],
                entity.world_matrix[3][1],
                entity.world_matrix[3][2],
            ],
            data: NpcData::from_sets(sets, catalog)?,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::propstream::{
        DecodeStats,
        testutil::{object, prop, template_class, template_file, template_header},
    };

    // String table shared by the fixtures below.
    const STRINGS: &[&str] = &[
        "It_Test.tple",                                   // 0
        "It_Test",                                        // 1
        "gCItem_PS",                                      // 2
        "GoldValue",                                      // 3
        "long",                                           // 4
        "Category",                                       // 5
        "bTPropertyContainer<enum gEItemCategory>",       // 6
        "ReqAttrib1Tag",                                  // 7
        "bCString",                                       // 8
        "STR",                                            // 9
        "ReqAttrib1Value",                                // 10
        "int",                                            // 11
        "ModAttrib1Tag",                                  // 12
        "PROT_BLADE",                                     // 13
        "ModAttrib1Value",                                // 14
        "gCDamage_PS",                                    // 15
        "DamageAmount",                                   // 16
        "DamageType",                                     // 17
        "bTPropertyContainer<enum gEDamageType>",         // 18
        "eCVisualMeshDynamic_PS",                         // 19
        "ResourceFileName",                               // 20
        "bCMeshResourceString",                           // 21
        "sword.xcmsh",                                    // 22
        "RequiredSkill1",                                 // 23
        "eCTemplateEntityProxy",                          // 24
        "Skill_Test.tple",                                // 25
        "Perk_Test",                                      // 26
        "gCNPC_PS",                                       // 27
        "Level",                                          // 28
        "PoliticalAlignment",                             // 29
        "bTPropertyContainer<enum gEPoliticalAlignment>", // 30
        "RoleDescription",                                // 31
        "Stadtchef",                                      // 32
    ];

    fn enum_payload(v: i32) -> Vec<u8> {
        let mut b = vec![1, 0];
        b.extend(v.to_le_bytes());
        b
    }

    fn str_payload(i: u16) -> Vec<u8> {
        i.to_le_bytes().to_vec()
    }

    fn proxy_payload(g: u8) -> Vec<u8> {
        let mut b = vec![1, 0, 1];
        b.extend([g; 20]);
        b
    }

    fn item_tail() -> Vec<u8> {
        let mut t = vec![0u8; 24];
        t.extend(0i32.to_le_bytes());
        t.extend([0, 0]);
        t.extend(0u32.to_le_bytes());
        t.push(0);
        t
    }

    fn parse(bytes: &[u8]) -> TemplateDocument {
        TemplateDocument::parse(bytes, &mut DecodeStats::default()).unwrap()
    }

    fn catalog_and_docs() -> (TemplateCatalog, TemplateDocument, TemplateDocument) {
        let skill = parse(&template_file(
            &["Skill_Test.tple", "Perk_Test"],
            &[
                template_header(0, 1, 0x51, true, &[]),
                template_header(0, 1, 0x52, false, &[]),
            ],
        ));
        let item = object(
            &[
                prop(3, 4, &77i32.to_le_bytes()),
                prop(5, 6, &enum_payload(1)),
                prop(7, 8, &str_payload(9)),
                prop(10, 11, &40i32.to_le_bytes()),
                prop(12, 8, &str_payload(13)),
                prop(14, 11, &5i32.to_le_bytes()),
                prop(23, 24, &proxy_payload(0x52)),
            ],
            54,
            &item_tail(),
        );
        let damage = object(
            &[
                prop(16, 4, &33i32.to_le_bytes()),
                prop(17, 18, &enum_payload(2)),
            ],
            76,
            &[],
        );
        let mesh = object(&[prop(20, 21, &str_payload(22))], 50, &[]);
        let doc = parse(&template_file(
            STRINGS,
            &[
                template_header(0, 1, 0x61, true, &[]),
                template_header(
                    0,
                    1,
                    0x62,
                    false,
                    &[
                        template_class(19, &mesh),
                        template_class(2, &item),
                        template_class(15, &damage),
                    ],
                ),
            ],
        ));
        let mut catalog = TemplateCatalog::default();
        catalog.insert("skill.tple", &skill);
        catalog.insert("item.tple", &doc);
        (catalog, skill, doc)
    }

    #[test]
    fn template_document_resolves_graph_names_and_guids() {
        let (catalog, skill, item) = catalog_and_docs();
        assert_eq!(item.entities.len(), 2);
        assert_eq!(item.entities[1].name, "It_Test");
        assert_eq!(item.entities[1].world_matrix[3], [5.0, 6.0, 7.0, 1.0]);
        assert_eq!(item.reference_header().unwrap().guid, "62".repeat(20));
        assert_eq!(skill.reference_header().unwrap().name, "Perk_Test");
        assert_eq!(catalog.name_of(&"52".repeat(20)), Some("Perk_Test"));
        assert_eq!(catalog.name_of(&"61".repeat(20)), Some("It_Test"));
        assert_eq!(catalog.resolve("zz"), "zz");
    }

    #[test]
    fn item_template_extracts_stats_requirements_and_damage() {
        let (catalog, _, doc) = catalog_and_docs();
        let item = ItemTemplate::from_document("items/it_test.tple", &doc, &catalog).unwrap();
        assert_eq!(item.name, "It_Test");
        assert_eq!((item.category.as_str(), item.category_id), ("Weapon", 1));
        assert_eq!(item.value, 77);
        assert_eq!(
            item.requirements,
            vec![AttributeRequirement {
                tag: "STR".into(),
                op: 0,
                value: 40
            }]
        );
        assert_eq!(
            item.modifiers,
            vec![AttributeModifier {
                tag: "PROT_BLADE".into(),
                op: 0,
                value: 5
            }]
        );
        let d = item.damage.unwrap();
        assert_eq!((d.damage_type.as_str(), d.amount), ("Blade", 33));
        assert_eq!(item.required_skills, vec!["Perk_Test".to_owned()]);
        assert_eq!(item.mesh.as_deref(), Some("sword.xcmsh"));
        assert!(item.use_effects.is_empty());
    }

    #[test]
    fn npc_data_extracts_identity_and_repairs_text() {
        let (catalog, _, _) = catalog_and_docs();
        let npc = object(
            &[
                prop(28, 11, &12i32.to_le_bytes()),
                prop(29, 30, &enum_payload(3)),
                prop(31, 8, &str_payload(32)),
            ],
            79,
            &[],
        );
        let doc = parse(&template_file(
            STRINGS,
            &[
                template_header(0, 1, 0x71, true, &[]),
                template_header(0, 1, 0x72, false, &[template_class(27, &npc)]),
            ],
        ));
        let t = NpcTemplate::from_document("npc/x.tple", &doc, &catalog).unwrap();
        assert_eq!(t.name, "It_Test");
        assert_eq!(t.data.level, 12);
        assert_eq!(t.data.guild, "Reb");
        assert_eq!(t.data.role_description, "Stadtchef");
        // UTF-8 bytes seen through the Latin-1 string view are repaired
        assert_eq!(repair_text("caf\u{c3}\u{a9}"), "caf\u{e9}");
        assert!(ItemTemplate::from_document("p", &doc, &catalog).is_none());
        assert_eq!(repair_text("plain"), "plain");
        assert_eq!(repair_text("\u{ff}\u{fe}"), "\u{ff}\u{fe}");
    }

    #[test]
    fn skill_template_extracts_cost_and_requirements() {
        let (catalog, _, _) = catalog_and_docs();
        let strings = [
            "S.tple",
            "Perk_X",
            "gCSkill_PS",
            "LearnPoints",
            "int",
            "ReqAttrib1Tag",
            "bCString",
            "DEX",
            "ReqAttrib1Value",
            "Category",
            "bTPropertyContainer<enum gESkillCategory>",
            "Tag",
            "X_1",
        ];
        let skill = object(
            &[
                prop(3, 4, &5i32.to_le_bytes()),
                prop(5, 6, &str_payload(7)),
                prop(8, 4, &80i32.to_le_bytes()),
                prop(9, 10, &enum_payload(1)),
                prop(11, 6, &str_payload(12)),
            ],
            1,
            &[],
        );
        let doc = parse(&template_file(
            &strings,
            &[
                template_header(0, 1, 0x81, true, &[]),
                template_header(0, 1, 0x82, false, &[template_class(2, &skill)]),
            ],
        ));
        let s = SkillTemplate::from_document("skills/x.tple", &doc, &catalog).unwrap();
        assert_eq!((s.name.as_str(), s.tag.as_str()), ("Perk_X", "X_1"));
        assert_eq!((s.category.as_str(), s.learn_points), ("Hunting", 5));
        assert_eq!(s.requirements, vec![("DEX".to_owned(), 80)]);
        assert!(s.required_skill.is_none());
    }

    #[test]
    fn template_parser_rejects_truncation() {
        let doc = template_file(STRINGS, &[template_header(0, 1, 0x61, true, &[])]);
        let mut stats = DecodeStats::default();
        // Truncating breaks either the envelope or a field; none may panic or succeed.
        for n in 0..doc.len() - 1 {
            assert!(
                TemplateDocument::parse(&doc[..n], &mut stats).is_err(),
                "{n}"
            );
        }
        assert!(TemplateDocument::parse(b"garbage garbage", &mut stats).is_err());
    }
}
