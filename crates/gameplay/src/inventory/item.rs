//! Item template data as the inventory rules see it (`gCItem_PS`, the use type of
//! `gCInteraction_PS` and the damage amount of `gCDamage_PS`).
use gothic_formats::props::enums;
use gothic_formats::props::records::ItemTemplate;
use gothic_formats::props::tails::ScriptLine;
use std::collections::HashMap;
use std::sync::Arc;

/// `gEItemCategory`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
#[repr(u8)]
pub enum Category {
    #[default]
    None = 0,
    Weapon = 1,
    Armor = 2,
    Artefact = 3,
    Alchemy = 4,
    EmptyA = 5,
    Misc = 6,
    Written = 7,
    EmptyB = 8,
    Spellbook = 9,
    Skill = 10,
}

impl Category {
    pub fn from_i32(v: i32) -> Category {
        match v {
            1 => Category::Weapon,
            2 => Category::Armor,
            3 => Category::Artefact,
            4 => Category::Alchemy,
            5 => Category::EmptyA,
            6 => Category::Misc,
            7 => Category::Written,
            8 => Category::EmptyB,
            9 => Category::Spellbook,
            10 => Category::Skill,
            _ => Category::None,
        }
    }
}

/// `gEItemCategory` by enumerator name (`Weapon`); unknown names give `None`.
pub fn category_from_name(name: &str) -> Category {
    Category::from_i32(enums::value("gEItemCategory", name).unwrap_or(0))
}

/// `gEUseType` value by enumerator name (`1H`); unknown names give 0 (`None`).
pub fn use_type_from_name(name: &str) -> u32 {
    enums::value("gEUseType", name).unwrap_or(0).max(0) as u32
}

/// One `ModAttribN` entry: tag, raw `EAttribModOperation` and value.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct StatMod {
    pub tag: String,
    pub op: i32,
    pub value: i32,
}

/// One `ReqAttribN` entry.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct StatReq {
    pub tag: String,
    pub op: i32,
    pub value: i32,
}

/// Property set 0x69 (`gCArmorSet_PS`): six piece templates and the bonus granted while all
/// of them are worn.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ArmorSet {
    /// Lower-case piece template names; empty entries are unused.
    pub pieces: [String; 6],
    pub mods: Vec<StatMod>,
    /// Skills activated while the set is complete.
    pub skills: Vec<String>,
}

/// Everything the inventory rules read from an item template.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct ItemInfo {
    /// Template name (`It_1H_Sword_01`).
    pub name: String,
    pub category: Category,
    /// `gEUseType` value (`gCInteraction_PS` +0x58).
    pub use_type: u32,
    /// `GoldValue`.
    pub gold_value: i32,
    /// `gCItem_PS.Quality` flag bits of the template (0x100 damaged, 0x20/0x40/0x80 bonus ...).
    pub quality: u32,
    pub mission_item: bool,
    /// `Permanent` (skills, spells and recipes: never removed from the inventory).
    pub permanent: bool,
    pub robe: bool,
    /// Amount of the item's `gCDamage_PS` (`CompareDamage` key), 0 without one.
    pub damage_amount: i32,
    /// Up to six modifiers, applied while the item is worn and on consumption.
    pub mods: Vec<StatMod>,
    /// Up to four attribute requirements.
    pub reqs: Vec<StatReq>,
    /// `RequiredSkill1/2`.
    pub required_skills: Vec<String>,
    /// `ActivateSkill1/2`: skills active while worn or after consumption.
    pub activate_skills: Vec<String>,
    /// `Skill` proxy (the skill this item teaches).
    pub skill: Option<String>,
    /// `Spell` proxy.
    pub spell: Option<String>,
    pub armor_set: Option<Arc<ArmorSet>>,
    /// `Texture` icon file name; empty means "category default".
    pub icon: String,
    /// `ItemWorld` / `ItemInventory` replacement templates.
    pub item_world: Option<String>,
    pub item_inventory: Option<String>,
    /// Use-effect script lines.
    pub script: Vec<ScriptLine>,
    /// The spell proxy carries a `gCMagic_PS` (grey icon rule).
    pub spell_is_magic: bool,
    /// The skill proxy carries a `gCSkill_PS` (grey icon rule).
    pub skill_is_skill: bool,
}

impl ItemInfo {
    /// Bare info for fixtures: name, category and use type.
    pub fn new(name: &str, category: Category, use_type: u32) -> Self {
        Self {
            name: name.to_string(),
            category,
            use_type,
            ..Self::default()
        }
    }

    /// Build from the decoded template record.
    pub fn from_template(t: &ItemTemplate) -> Self {
        let use_type = t
            .use_type
            .as_deref()
            .and_then(|n| enums::value("gEUseType", n))
            .unwrap_or(0)
            .max(0) as u32;
        Self {
            name: t.name.clone(),
            category: Category::from_i32(t.category_id),
            use_type,
            gold_value: t.value,
            quality: t.quality.max(0) as u32,
            mission_item: t.mission_item,
            permanent: t.permanent,
            robe: t.robe,
            damage_amount: t.damage.as_ref().map_or(0, |d| d.amount),
            mods: t
                .modifiers
                .iter()
                .map(|m| StatMod {
                    tag: m.tag.clone(),
                    op: m.op,
                    value: m.value,
                })
                .collect(),
            reqs: t
                .requirements
                .iter()
                .map(|r| StatReq {
                    tag: r.tag.clone(),
                    op: r.op,
                    value: r.value,
                })
                .collect(),
            required_skills: t.required_skills.clone(),
            activate_skills: t.activate_skills.clone(),
            skill: t.skill.clone(),
            spell: t.spell.clone(),
            armor_set: None,
            icon: t.icon.clone().unwrap_or_default(),
            item_world: None,
            item_inventory: None,
            script: t.use_effects.clone(),
            spell_is_magic: t.spell.is_some(),
            skill_is_skill: t.skill.is_some(),
        }
    }

    /// Port of gCItem_PS::GetModValue (Game:0x100b9d20): value of the first modifier whose
    /// tag equals `tag`, 0 when there is none.
    pub fn mod_value(&self, tag: &str) -> i32 {
        gothic_portcov::hit!("mod_value");
        self.mods
            .iter()
            .find(|m| m.tag == tag)
            .map_or(0, |m| m.value)
    }

    /// Does the template carry no inventory-visible data at all (unknown template)?
    pub fn is_unknown(&self) -> bool {
        self.name.is_empty()
    }
}

/// Source of item templates for operations that create stacks.
pub trait ItemCatalog {
    /// Look a template up by (case-insensitive) name.
    fn item(&self, name: &str) -> Option<Arc<ItemInfo>>;
}

/// Hash-map catalog keyed by lower-case name; also used for synthetic fixtures.
#[derive(Debug, Clone, Default)]
pub struct MapCatalog {
    items: HashMap<String, Arc<ItemInfo>>,
}

impl MapCatalog {
    pub fn insert(&mut self, info: ItemInfo) -> Arc<ItemInfo> {
        let a = Arc::new(info);
        self.items.insert(a.name.to_ascii_lowercase(), a.clone());
        a
    }

    pub fn len(&self) -> usize {
        self.items.len()
    }

    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }

    /// Build from decoded templates (`ItemTemplate`).
    pub fn from_templates<'a>(it: impl IntoIterator<Item = &'a ItemTemplate>) -> Self {
        let mut c = Self::default();
        for t in it {
            c.insert(ItemInfo::from_template(t));
        }
        c
    }
}

impl ItemCatalog for MapCatalog {
    fn item(&self, name: &str) -> Option<Arc<ItemInfo>> {
        self.items.get(&name.to_ascii_lowercase()).cloned()
    }
}

/// A catalog that knows nothing: stacks get a bare placeholder (used by tests and by hosts
/// that only track counts).
#[derive(Debug, Clone, Copy, Default)]
pub struct NoCatalog;

impl ItemCatalog for NoCatalog {
    fn item(&self, _name: &str) -> Option<Arc<ItemInfo>> {
        None
    }
}

/// A catalog with exactly one answer, whatever the name: lets a host that already resolved a
/// template pass it to operations that ask a catalog.
#[derive(Debug, Clone, Default)]
pub struct OneItem(pub Option<Arc<ItemInfo>>);

impl ItemCatalog for OneItem {
    fn item(&self, _name: &str) -> Option<Arc<ItemInfo>> {
        self.0.clone()
    }
}
