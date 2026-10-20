//! Loading the extracted game data (`.local/npcs.json`, `.local/items.json`) into simulation
//! actors. The original install stays read-only; these JSON files are derived exports under the
//! gitignored `.local/` (see `docs/research/PROPERTY_STREAMS.md`).
use crate::actor::Actor;
use crate::config::SimConfig;
use crate::item::{DamageDef, ItemDb, ItemDef, Use};
use crate::types::*;
use anyhow::{Context, Result};
use serde::Deserialize;
use std::path::Path;

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
pub struct EquippedRec {
    pub slot: String,
    pub template: String,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
pub struct StackRec {
    pub item: String,
    pub amount: i32,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
pub struct InventoryRec {
    pub equipped: Vec<EquippedRec>,
    pub stacks: Vec<StackRec>,
    pub treasure_sets: Vec<String>,
}

/// One NPC entity or template record of `npcs.json`.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
pub struct NpcRecord {
    pub name: String,
    pub display_name: Option<String>,
    pub species: String,
    pub class: String,
    pub guild: String,
    pub npc_type: String,
    pub level: i32,
    pub level_max: i32,
    pub immortal: bool,
    pub enclave: Option<String>,
    pub attitude_to_player: String,
    pub damage: Option<DamageDef>,
    pub inventory: InventoryRec,
    pub position: [f32; 3],
    pub document: String,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
pub struct NpcFile {
    pub entities: Vec<NpcRecord>,
    pub templates: Vec<NpcRecord>,
}

impl NpcFile {
    pub fn from_json(text: &str) -> Result<Self> {
        serde_json::from_str(text).context("npcs.json")
    }
    pub fn load(path: &Path) -> Result<Self> {
        let text =
            std::fs::read_to_string(path).with_context(|| format!("reading {}", path.display()))?;
        Self::from_json(&text)
    }
    pub fn entity(&self, name: &str) -> Option<&NpcRecord> {
        self.entities
            .iter()
            .find(|e| e.name.eq_ignore_ascii_case(name))
    }
}

impl NpcRecord {
    /// Build a ready-to-fight actor: stats derive from level, equipped items found in the
    /// item database are worn. NPC weapons come from treasure sets in the real game (not
    /// extracted), so the caller may arm the actor with [`default_weapon`].
    pub fn to_actor(&self, id: ActorId, cfg: &SimConfig, items: &ItemDb) -> Actor {
        let mut a = Actor::npc(
            id,
            &self.name,
            Species::from_name(&self.species),
            self.level,
            self.level_max,
        );
        a.class = NpcClass::from_name(&self.class);
        a.alignment = Alignment::from_name(&self.guild);
        a.npc_type = NpcType::from_name(&self.npc_type);
        a.enclave = self.enclave.clone();
        a.immortal = self.immortal;
        a.natural_damage = self.damage.clone();
        a.base_attitude_to_player = self.base_attitude();
        for e in &self.inventory.equipped {
            if let Some(i) = items.get(&e.template) {
                a.equipment.equip(i.clone());
            }
        }
        for s in &self.inventory.stacks {
            if s.amount > 0 {
                *a.inventory.entry(s.item.to_ascii_lowercase()).or_insert(0) += s.amount as u32;
            }
        }
        a.refresh(cfg);
        a
    }

    /// Initial attitude to the player from the property (`Neutral` / `None`).
    pub fn base_attitude(&self) -> Attitude {
        match self.attitude_to_player.to_ascii_lowercase().as_str() {
            "friendly" => Attitude::Friendly,
            "neutral" => Attitude::Neutral,
            "angry" => Attitude::Angry,
            "hostile" => Attitude::Hostile,
            _ => Attitude::None,
        }
    }
}

/// A plausible starter weapon for an NPC without extracted loot: orcs carry the orc halberd,
/// humans a one-hand sword; falls back to the strongest weapon of the matching use type.
/// Returns `None` for species that fight with natural attacks.
pub fn default_weapon(items: &ItemDb, species: Species) -> Option<ItemDef> {
    let pick = |name: &str| items.get(name).cloned();
    match species {
        Species::Orc => pick("It_Halberd_Orc_01").or_else(|| strongest(items, Use::Halberd)),
        Species::Human => pick("It_1H_Sword_01").or_else(|| strongest(items, Use::OneH)),
        _ => None,
    }
}

fn strongest(items: &ItemDb, use_kind: Use) -> Option<ItemDef> {
    items
        .iter()
        .filter(|i| i.use_kind() == use_kind && i.damage.is_some())
        .max_by_key(|i| (i.damage.as_ref().map_or(0, |d| d.amount), i.name.clone()))
        .cloned()
}

#[cfg(test)]
mod tests {
    use super::*;

    const NPCS: &str = r#"{"entities":[{"name":"Orc_Test","species":"Orc","class":"Warrior","guild":"Orc",
        "npc_type":"Personal","level":10,"level_max":30,"immortal":false,"attitude_to_player":"Neutral",
        "damage":null,"position":[1.0,2.0,3.0],"document":"x.lrentdat",
        "inventory":{"equipped":[{"slot":"Body","template":"Orc_Armor"}],"stacks":[{"item":"It_Potion_Health","amount":2}],"treasure_sets":[]}}],
        "templates":[]}"#;

    #[test]
    fn loads_and_builds_an_actor() {
        let mut items = ItemDb::default();
        items.insert(ItemDef {
            name: "Orc_Armor".into(),
            use_type: Some("Body".into()),
            ..ItemDef::default()
        });
        items.insert(ItemDef::weapon(
            "It_Halberd_Orc_01",
            "Halberd",
            "Blade",
            200,
        ));
        let f = NpcFile::from_json(NPCS).unwrap();
        let cfg = SimConfig::default();
        let a = f
            .entity("orc_test")
            .unwrap()
            .to_actor(ActorId(3), &cfg, &items);
        assert_eq!(a.species, Species::Orc);
        assert_eq!(a.alignment, Alignment::Orc);
        assert_eq!((a.level, a.level_max), (10, 30));
        assert_eq!(a.hp, 600);
        assert!(a.equipment.body.is_some());
        assert_eq!(a.inventory["it_potion_health"], 2);
        assert_eq!(
            f.entity("orc_test").unwrap().base_attitude(),
            Attitude::Neutral
        );
        let w = default_weapon(&items, Species::Orc).unwrap();
        assert_eq!(w.name, "It_Halberd_Orc_01");
        assert!(default_weapon(&items, Species::Wolf).is_none());
    }
}
