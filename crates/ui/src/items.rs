//! Item template records (from `gothic-inspector item-dump`, default `.local/items.json`).
use anyhow::{Context, Result};
use serde_json::Value;
use std::collections::HashMap;
use std::path::Path;

#[derive(Debug, Clone, Default)]
pub struct ItemInfo {
    pub name: String,
    /// `gEItemCategory` raw id: 1 weapon, 2 armor, 3 artefact, 4 alchemy, 6 misc, 7 written,
    /// 9 spellbook, 10 skill, 0 none.
    pub category: i32,
    pub value: u32,
    pub icon: Option<String>,
    pub description_en: Option<String>,
    pub damage: Option<(u32, String)>,
    pub requirements: Vec<(String, i32)>,
    pub modifiers: Vec<(String, i32)>,
    pub mission_item: bool,
    pub use_type: String,
}

#[derive(Debug, Clone, Default)]
pub struct ItemCatalog {
    map: HashMap<String, ItemInfo>,
}

impl ItemCatalog {
    pub fn load(path: &Path) -> Result<Self> {
        let text = std::fs::read_to_string(path).with_context(|| path.display().to_string())?;
        Self::from_json(&text)
    }

    pub fn from_json(text: &str) -> Result<Self> {
        let v: Value = serde_json::from_str(text)?;
        let mut map = HashMap::new();
        for it in v.as_array().context("item list expected")? {
            let s = |k: &str| it.get(k).and_then(Value::as_str).map(str::to_string);
            let pairs = |k: &str| -> Vec<(String, i32)> {
                it.get(k)
                    .and_then(Value::as_array)
                    .into_iter()
                    .flatten()
                    .filter_map(|m| {
                        Some((
                            m.get("tag")?.as_str()?.to_string(),
                            m.get("value")?.as_i64()? as i32,
                        ))
                    })
                    .collect()
            };
            let Some(name) = s("name") else { continue };
            let info = ItemInfo {
                category: it.get("category_id").and_then(Value::as_i64).unwrap_or(0) as i32,
                value: it.get("value").and_then(Value::as_u64).unwrap_or(0) as u32,
                icon: s("icon"),
                description_en: s("description"),
                damage: it.get("damage").and_then(|d| {
                    Some((
                        d.get("amount")?.as_u64()? as u32,
                        d.get("damage_type")?.as_str()?.to_string(),
                    ))
                }),
                requirements: pairs("requirements"),
                modifiers: pairs("modifiers"),
                mission_item: it
                    .get("mission_item")
                    .and_then(Value::as_bool)
                    .unwrap_or(false),
                use_type: s("use_type").unwrap_or_default(),
                name: name.clone(),
            };
            map.insert(name.to_ascii_lowercase(), info);
        }
        Ok(Self { map })
    }

    pub fn get(&self, name: &str) -> Option<&ItemInfo> {
        self.map.get(&name.to_ascii_lowercase())
    }

    pub fn iter(&self) -> impl Iterator<Item = (&str, &ItemInfo)> {
        self.map.values().map(|i| (i.name.as_str(), i))
    }

    pub fn len(&self) -> usize {
        self.map.len()
    }

    pub fn is_empty(&self) -> bool {
        self.map.is_empty()
    }
}
