//! Serialisation of [`Inventory`] for the save game.
//!
//! The current format lists the stacks with their links; the legacy format (a plain map of
//! template name to count) is still accepted when loading. Template data is not stored: call
//! [`Inventory::rebind`] after loading.
use super::list::{DefaultItem, Inventory};
use super::slot::{SLOT_COUNT, Slot};
use super::stack::{Stack, StackType};
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use std::collections::BTreeMap;

#[derive(Serialize, Deserialize)]
struct StackSave {
    template: String,
    amount: i32,
    #[serde(default)]
    quality: u32,
    #[serde(default)]
    slot: u8,
    #[serde(default = "no_quick_slot")]
    quick_slot: i32,
    #[serde(default)]
    sort_index: i32,
    #[serde(default)]
    kind: u8,
    #[serde(default)]
    learned: bool,
    #[serde(default)]
    activation_count: i32,
    #[serde(default)]
    stats_applied: bool,
}

fn no_quick_slot() -> i32 {
    -1
}

#[derive(Serialize, Deserialize)]
struct InventorySave {
    stacks: Vec<StackSave>,
    /// Default item template per slot (empty = none) and whether it is spawned.
    #[serde(default)]
    defaults: Vec<(String, bool)>,
    #[serde(default)]
    treasure_sets: Vec<String>,
    #[serde(default)]
    generated_plunder: bool,
    #[serde(default)]
    generated_trade: bool,
}

#[derive(Deserialize)]
#[serde(untagged)]
enum AnyInventory {
    Full(InventorySave),
    Legacy(BTreeMap<String, u32>),
}

impl Serialize for Inventory {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        let stacks = self
            .stacks()
            .iter()
            .map(|st| StackSave {
                template: st.template.clone(),
                amount: st.amount,
                quality: st.quality,
                slot: st.slot as u8,
                quick_slot: st.quick_slot,
                sort_index: st.sort_index,
                kind: st.kind as u8,
                learned: st.learned,
                activation_count: st.activation_count,
                stats_applied: st.stats_applied,
            })
            .collect();
        let defaults = Slot::ALL
            .iter()
            .map(|sl| {
                self.default_slot(*sl)
                    .map_or((String::new(), false), |d| (d.template.clone(), d.linked))
            })
            .collect();
        InventorySave {
            stacks,
            defaults,
            treasure_sets: self.treasure_sets.to_vec(),
            generated_plunder: self.generated_plunder,
            generated_trade: self.generated_trade,
        }
        .serialize(s)
    }
}

impl<'de> Deserialize<'de> for Inventory {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let mut inv = Inventory::new();
        match AnyInventory::deserialize(d)? {
            AnyInventory::Legacy(map) => {
                for (k, n) in map {
                    inv.insert(&k, n);
                }
            }
            AnyInventory::Full(f) => {
                let mut stacks = Vec::with_capacity(f.stacks.len());
                for s in f.stacks {
                    let mut st = Stack::new(&s.template, None);
                    st.amount = s.amount;
                    st.quality = s.quality;
                    st.slot = Slot::from_index(s.slot as usize).unwrap_or(Slot::None);
                    st.quick_slot = s.quick_slot;
                    st.sort_index = s.sort_index;
                    st.kind = StackType::from_u8(s.kind);
                    st.learned = s.learned;
                    st.activation_count = s.activation_count;
                    st.stats_applied = s.stats_applied;
                    stacks.push(st);
                }
                let mut defaults: Vec<Option<DefaultItem>> = f
                    .defaults
                    .into_iter()
                    .map(|(t, linked)| {
                        (!t.is_empty()).then_some(DefaultItem {
                            template: t,
                            info: None,
                            linked,
                        })
                    })
                    .collect();
                defaults.resize(SLOT_COUNT, None);
                inv.restore(
                    stacks,
                    defaults,
                    f.treasure_sets,
                    f.generated_plunder,
                    f.generated_trade,
                );
            }
        }
        Ok(inv)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::inventory::item::{Category, ItemCatalog, ItemInfo, MapCatalog};
    use crate::inventory::slot::use_type;

    #[test]
    fn round_trip_keeps_links_and_defaults() {
        let mut c = MapCatalog::default();
        c.insert(ItemInfo::new("It_Sword", Category::Weapon, use_type::ONE_H));
        c.insert(ItemInfo::new("It_Hair", Category::Misc, use_type::HAIR));
        let mut inv = Inventory::new();
        let s = inv.create_items(&c, "It_Sword", 0x20, 1) as usize;
        inv.equip_stack(s);
        inv.set_default_item(Slot::Hair, "It_Hair", c.item("It_Hair"));
        inv.treasure_sets[1] = "TS".into();
        inv.generated_plunder = true;
        let json = serde_json::to_string(&inv).unwrap();
        let mut back: Inventory = serde_json::from_str(&json).unwrap();
        back.rebind(&c);
        assert_eq!(back.stack(0).unwrap().slot, Slot::BackRight);
        assert_eq!(back.stack(0).unwrap().quality, 0x20);
        assert!(back.stack(0).unwrap().is_equipped());
        assert_eq!(back.default_item(Slot::Hair), "It_Hair");
        assert!(back.default_slot(Slot::Hair).unwrap().linked);
        assert_eq!(back.treasure_sets[1], "TS");
        assert!(back.generated_plunder);
        assert_eq!(back.count("it_sword"), 1);
    }

    #[test]
    fn legacy_count_maps_still_load() {
        let inv: Inventory = serde_json::from_str(r#"{"it_gold": 120, "it_apple": 3}"#).unwrap();
        assert_eq!(inv.count("It_Gold"), 120);
        assert_eq!(inv.count("it_apple"), 3);
    }
}
