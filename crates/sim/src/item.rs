//! Item templates as the simulation needs them, equipment slots and their effects.
//!
//! [`ItemDef`] deserialises the `.local/items.json` export of
//! `gothic_formats::props::records::ItemTemplate` (extra keys are ignored), but can also be
//! built by hand for synthetic fixtures.
use crate::types::DamageType;
use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

fn one() -> f32 {
    1.0
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct DamageDef {
    pub damage_type: String,
    pub amount: i32,
    pub hit_multiplier: f32,
}

impl Default for DamageDef {
    fn default() -> Self {
        Self {
            damage_type: "Impact".into(),
            amount: 0,
            hit_multiplier: 1.0,
        }
    }
}

impl DamageDef {
    pub fn new(ty: &str, amount: i32) -> Self {
        Self {
            damage_type: ty.into(),
            amount,
            hit_multiplier: one(),
        }
    }
    pub fn kind(&self) -> DamageType {
        DamageType::from_name(&self.damage_type)
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(default)]
pub struct Requirement {
    pub tag: String,
    /// Raw `EAttribReqOperation`; 0 is treated as ">=" [I].
    pub op: i32,
    pub value: i32,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(default)]
pub struct Modifier {
    pub tag: String,
    /// Raw `EAttribModOperation`; 0 is treated as "add" [I].
    pub op: i32,
    pub value: i32,
}

/// `gEUseType` values that the hand-slot table distinguishes [CA 2.12].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Default)]
pub enum Use {
    #[default]
    None,
    OneH,
    TwoH,
    Axe,
    Staff,
    Halberd,
    Fist,
    PhysicalFist,
    Shield,
    Torch,
    Bow,
    CrossBow,
    Arrow,
    Bolt,
    Cast,
    Hammer,
    Body,
    Other,
}

impl Use {
    pub fn from_name(s: &str) -> Self {
        match s {
            "1H" => Self::OneH,
            "2H" => Self::TwoH,
            "Axe" => Self::Axe,
            "Staff" => Self::Staff,
            "Halberd" => Self::Halberd,
            "Fist" => Self::Fist,
            "PhysicalFist" => Self::PhysicalFist,
            "Shield" => Self::Shield,
            "Torch" => Self::Torch,
            "Bow" => Self::Bow,
            "CrossBow" => Self::CrossBow,
            "Arrow" => Self::Arrow,
            "Bolt" => Self::Bolt,
            "Cast" => Self::Cast,
            "Hammer" => Self::Hammer,
            "Body" => Self::Body,
            "None" | "" => Self::None,
            _ => Self::Other,
        }
    }
    /// Melee weapon use types that fill the right-hand slot.
    pub fn is_melee_weapon(self) -> bool {
        matches!(
            self,
            Self::OneH | Self::TwoH | Self::Axe | Self::Staff | Self::Halberd
        )
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(default)]
pub struct ItemDef {
    pub name: String,
    pub category: String,
    pub use_type: Option<String>,
    pub value: i32,
    /// `PSItem.Quality` flag bits [CA 2.5, 9.7].
    pub quality: i32,
    pub robe: bool,
    pub damage: Option<DamageDef>,
    pub requirements: Vec<Requirement>,
    pub required_skills: Vec<String>,
    pub modifiers: Vec<Modifier>,
    pub spell: Option<String>,
    pub permanent: bool,
}

impl ItemDef {
    /// Convenience constructor for fixtures.
    pub fn weapon(name: &str, use_type: &str, ty: &str, amount: i32) -> Self {
        Self {
            name: name.into(),
            category: "Weapon".into(),
            use_type: Some(use_type.into()),
            damage: Some(DamageDef::new(ty, amount)),
            ..Self::default()
        }
    }

    pub fn use_kind(&self) -> Use {
        Use::from_name(self.use_type.as_deref().unwrap_or(""))
    }

    /// Sum of the `PROT_*` modifiers for a damage type (add operation) [CA 2.7].
    pub fn protection(&self, ty: DamageType) -> i32 {
        self.modifier(ty.protection_tag())
    }

    pub fn modifier(&self, tag: &str) -> i32 {
        self.modifiers
            .iter()
            .filter(|m| m.tag.eq_ignore_ascii_case(tag) && m.op == 0)
            .map(|m| m.value)
            .sum()
    }
}

/// Item database keyed by lower-case template name.
#[derive(Debug, Clone, Default)]
pub struct ItemDb {
    items: BTreeMap<String, ItemDef>,
}

impl ItemDb {
    pub fn insert(&mut self, item: ItemDef) {
        self.items.insert(item.name.to_ascii_lowercase(), item);
    }
    pub fn get(&self, name: &str) -> Option<&ItemDef> {
        self.items.get(&name.to_ascii_lowercase())
    }
    pub fn len(&self) -> usize {
        self.items.len()
    }
    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }
    pub fn iter(&self) -> impl Iterator<Item = &ItemDef> {
        self.items.values()
    }
    /// Parse the JSON array exported to `.local/items.json`.
    pub fn from_json(text: &str) -> Result<Self> {
        let v: Vec<ItemDef> = serde_json::from_str(text).context("items.json")?;
        let mut db = Self::default();
        for i in v {
            db.insert(i);
        }
        Ok(db)
    }
    pub fn load(path: &Path) -> Result<Self> {
        let text =
            std::fs::read_to_string(path).with_context(|| format!("reading {}", path.display()))?;
        Self::from_json(&text)
    }
}

/// Why an item cannot be equipped.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Unmet {
    Attribute { tag: String, need: i32, have: i32 },
    Skill(String),
}

/// Attribute and skill requirements of an item [CA: GAME_RULES 4.2 / PROPERTY_STREAMS items].
/// `attr` returns the actor's current value for a tag (`STR`, `DEX`, `INT`, ...).
pub fn check_requirements(
    item: &ItemDef,
    attr: impl Fn(&str) -> i32,
    skills: &BTreeSet<String>,
) -> Result<(), Unmet> {
    for r in &item.requirements {
        let have = attr(&r.tag);
        if have < r.value {
            return Err(Unmet::Attribute {
                tag: r.tag.clone(),
                need: r.value,
                have,
            });
        }
    }
    for s in &item.required_skills {
        if !skills.contains(&s.to_ascii_lowercase()) {
            return Err(Unmet::Skill(s.clone()));
        }
    }
    Ok(())
}

/// Hand-slot combination class [CA 2.12].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum WeaponMode {
    #[default]
    None,
    Melee,
    Ranged,
    Magic,
}

/// Equipped items. `left`/`right` are the hand slots of the table in [CA 2.12]: shield,
/// torch, bow and crossbow sit in the left hand; one-handers, two-handers, axes, staffs,
/// halberds, fists, ammunition and `Cast` in the right hand.
#[derive(Debug, Clone, Default)]
pub struct Equipment {
    pub left: Option<ItemDef>,
    pub right: Option<ItemDef>,
    pub body: Option<ItemDef>,
    /// Helmet, amulet, rings, belt ... (only modifiers matter).
    pub accessories: Vec<ItemDef>,
}

impl Equipment {
    /// Put `item` into the slot its use type implies. Dual one-handers go left when the
    /// right hand is taken [CA 2.3 "one-hander in each hand"].
    pub fn equip(&mut self, item: ItemDef) {
        match item.use_kind() {
            Use::Shield | Use::Torch | Use::Bow | Use::CrossBow => self.left = Some(item),
            Use::OneH if self.right.as_ref().is_some_and(|r| r.use_kind() == Use::OneH) => {
                self.left = Some(item)
            }
            Use::OneH
            | Use::TwoH
            | Use::Axe
            | Use::Staff
            | Use::Halberd
            | Use::Fist
            | Use::PhysicalFist
            | Use::Arrow
            | Use::Bolt
            | Use::Cast => self.right = Some(item),
            Use::Body => self.body = Some(item),
            _ => self.accessories.push(item),
        }
    }

    pub fn left_use(&self) -> Use {
        self.left.as_ref().map_or(Use::None, ItemDef::use_kind)
    }
    pub fn right_use(&self) -> Use {
        self.right.as_ref().map_or(Use::None, ItemDef::use_kind)
    }

    /// Class of the hand combination mapped to a weapon mode (rows of [CA 2.12]).
    pub fn weapon_mode(&self) -> WeaponMode {
        use Use::*;
        match (self.left_use(), self.right_use()) {
            (None, OneH | TwoH | Axe | Staff | Halberd | Fist)
            | (OneH | Shield | Torch, OneH)
            | (PhysicalFist, PhysicalFist) => WeaponMode::Melee,
            (Bow, None | Arrow) | (CrossBow, None | Bolt) => WeaponMode::Ranged,
            (None | Staff, Cast) => WeaponMode::Magic,
            _ => WeaponMode::None,
        }
    }

    /// One-hander in each hand [CA 2.3].
    pub fn dual_one_hand(&self) -> bool {
        self.left_use() == Use::OneH && self.right_use() == Use::OneH
    }

    pub fn has_shield(&self) -> bool {
        self.left_use() == Use::Shield
    }

    /// Shield together with a one-hander [CA 2.9 missile parry].
    pub fn shield_and_one_hander(&self) -> bool {
        self.has_shield() && self.right_use() == Use::OneH
    }

    /// All equipped items.
    pub fn all(&self) -> impl Iterator<Item = &ItemDef> {
        self.left
            .iter()
            .chain(self.right.iter())
            .chain(self.body.iter())
            .chain(self.accessories.iter())
    }

    /// Sum of an attribute modifier over all equipment (`STR`, `PROT_BLADE`, ...).
    pub fn modifier_sum(&self, tag: &str) -> i32 {
        self.all().map(|i| i.modifier(tag)).sum()
    }

    /// `PlayerMemory.Protection<Type>`: the equipment-fed value before the /5 scaling [CA 2.7].
    pub fn protection_sum(&self, ty: DamageType) -> i32 {
        self.modifier_sum(ty.protection_tag())
    }

    /// Damage block of the weapon in the hand that holds the damage source: the right-hand
    /// weapon, or the bow/crossbow's own block for ranged use.
    pub fn weapon_damage(&self) -> Option<&DamageDef> {
        self.right
            .as_ref()
            .and_then(|i| i.damage.as_ref())
            .filter(|_| self.right_use().is_melee_weapon() || self.right_use() == Use::Fist)
    }

    pub fn weapon_name(&self) -> Option<&str> {
        self.right.as_ref().map(|i| i.name.as_str())
    }

    pub fn weapon_quality(&self) -> i32 {
        self.right.as_ref().map_or(0, |i| i.quality)
    }

    /// Chest item robe flag [CA 2.7].
    pub fn wears_robe(&self) -> bool {
        self.body.as_ref().is_some_and(|b| b.robe)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sword() -> ItemDef {
        let mut s = ItemDef::weapon("It_1H_Sword_01", "1H", "Blade", 50);
        s.requirements.push(Requirement {
            tag: "STR".into(),
            op: 0,
            value: 100,
        });
        s.required_skills.push("Perk_1H_1".into());
        s
    }

    fn armor(blade: i32) -> ItemDef {
        ItemDef {
            name: "Body_Test".into(),
            use_type: Some("Body".into()),
            modifiers: vec![Modifier {
                tag: "PROT_BLADE".into(),
                op: 0,
                value: blade,
            }],
            ..ItemDef::default()
        }
    }

    #[test]
    fn json_round_trip_ignores_extra_keys() {
        let j = r#"[{"name":"A","display_name":"x","category":"Weapon","use_type":"1H","value":5,
            "quality":0,"damage":{"amount":50,"damage_type":"Blade","hit_multiplier":1.0,"mana_multiplier":1.0},
            "requirements":[{"op":0,"tag":"STR","value":100}],"required_skills":["Perk_1H_1"],"modifiers":[]}]"#;
        let db = ItemDb::from_json(j).unwrap();
        let i = db.get("a").unwrap();
        assert_eq!(i.damage.as_ref().unwrap().amount, 50);
        assert_eq!(i.use_kind(), Use::OneH);
    }

    #[test]
    fn requirements_are_checked() {
        let skills: BTreeSet<String> = ["perk_1h_1".to_string()].into();
        assert!(check_requirements(&sword(), |_| 100, &skills).is_ok());
        assert_eq!(
            check_requirements(&sword(), |_| 99, &skills),
            Err(Unmet::Attribute {
                tag: "STR".into(),
                need: 100,
                have: 99
            })
        );
        assert_eq!(
            check_requirements(&sword(), |_| 100, &BTreeSet::new()),
            Err(Unmet::Skill("Perk_1H_1".into()))
        );
    }

    #[test]
    fn hand_table_and_protection() {
        let mut e = Equipment::default();
        assert_eq!(e.weapon_mode(), WeaponMode::None);
        e.equip(sword());
        assert_eq!(e.weapon_mode(), WeaponMode::Melee);
        e.equip(ItemDef {
            use_type: Some("Shield".into()),
            ..ItemDef::default()
        });
        assert!(e.shield_and_one_hander());
        e.equip(armor(40));
        e.equip(ItemDef {
            modifiers: vec![Modifier {
                tag: "PROT_BLADE".into(),
                op: 0,
                value: 5,
            }],
            use_type: Some("Amulet".into()),
            ..ItemDef::default()
        });
        assert_eq!(e.protection_sum(DamageType::Blade), 45);
        assert_eq!(e.protection_sum(DamageType::Physics), 0);
        let mut bow = Equipment::default();
        bow.equip(ItemDef {
            use_type: Some("Bow".into()),
            ..ItemDef::default()
        });
        assert_eq!(bow.weapon_mode(), WeaponMode::Ranged);
    }

    #[test]
    fn dual_wield_goes_to_left_hand() {
        let mut e = Equipment::default();
        e.equip(sword());
        e.equip(sword());
        assert!(e.dual_one_hand());
    }
}
