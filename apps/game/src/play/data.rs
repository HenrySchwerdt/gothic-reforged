//! Derived world data of the play mode: interact objects and loose items around Ardea,
//! treasure sets and recipes. All three files are exports of the inspector (`object-dump`,
//! `treasure-dump`, `recipe-dump`) under the gitignored `.local/`; nothing here is invented, the
//! values are the property streams of the original world and templates.
use gothic_sim::script::enums::UseType;
use gothic_sim::script::player::api::{Lock, LockStatus, Recipe, SkillCategory};
use serde_json::Value;
use std::collections::HashMap;
use std::path::Path;

/// One interactive world entity: a use-able object, a routine point or a loose item.
#[derive(Debug, Clone)]
pub struct WorldObjectDef {
    /// Source id of the entity (hex), the key routine points refer to.
    pub id: String,
    /// Entity (template) name.
    pub name: String,
    /// Localised focus name (`FO_<name>`).
    pub display: Option<String>,
    /// World position in the original space (cm).
    pub position: [f32; 3],
    /// Heading about the vertical axis from the world matrix (radians, original space).
    pub heading: f32,
    /// `gEUseType` name of `gCInteraction_PS` (`None` when the entity has no interaction).
    pub use_type: String,
    /// `FocusNameType` of the interaction (`Entity`, `Skeleton`, `Disable`).
    pub focus_name: String,
    /// It is a loose item (carries `gCItem_PS`).
    pub is_item: bool,
    /// Lock of a chest or door.
    pub lock: Option<LockDef>,
    /// `TreasureSet1..5` of the entity's inventory.
    pub treasure_sets: Vec<String>,
    /// Fixed stacks in the entity's inventory.
    pub stacks: Vec<(String, u32)>,
    /// Already used up (`UsedByPlayer`).
    pub used: bool,
}

/// `gCLock_PS`.
#[derive(Debug, Clone)]
pub struct LockDef {
    pub locked: bool,
    pub key: String,
    pub difficulty: i32,
}

impl LockDef {
    /// The script-side lock.
    pub fn to_lock(&self) -> Lock {
        Lock {
            status: if self.locked {
                LockStatus::Locked
            } else {
                LockStatus::Unlocked
            },
            key: self.key.clone(),
            key_amount: 1,
            difficulty: self.difficulty.max(0) as u32 + u32::from(self.difficulty < 0) * 99,
        }
    }
}

impl WorldObjectDef {
    /// The use type as the script enum (`None` for unknown names such as `Smalltalk`).
    pub fn script_use(&self) -> UseType {
        UseType::from_name(&self.use_type).unwrap_or(UseType::None)
    }

    /// Can the player use it (`PS_Interact` has a state for the use type) and is its name shown?
    pub fn player_usable(&self) -> bool {
        !self.is_item
            && self.focus_name != "Disable"
            && gothic_sim::script::player::interact::state_for_use(self.script_use()).is_some()
    }
}

fn f32s(v: &Value) -> Option<[f32; 3]> {
    let a = v.as_array()?;
    Some([
        a.first()?.as_f64()? as f32,
        a.get(1)?.as_f64()? as f32,
        a.get(2)?.as_f64()? as f32,
    ])
}

/// Entities whose use type marks equipment of a character (they live in an NPC's inventory
/// entity, not in the world).
fn is_character_equipment(use_type: &str) -> bool {
    matches!(use_type, "Body" | "Head" | "Hair" | "Beard" | "Helmet")
}

/// Read `objects.json`.
pub fn load_objects(path: &Path) -> Vec<WorldObjectDef> {
    let Ok(text) = std::fs::read_to_string(path) else {
        return Vec::new();
    };
    let Ok(root) = serde_json::from_str::<Value>(&text) else {
        return Vec::new();
    };
    let mut out = Vec::new();
    for o in root["objects"].as_array().into_iter().flatten() {
        let sets = &o["sets"];
        let inter = &sets["gCInteraction_PS"];
        let use_type = inter["UseType"].as_str().unwrap_or("None").to_string();
        if is_character_equipment(&use_type) {
            continue;
        }
        let Some(position) = f32s(&o["position"]) else {
            continue;
        };
        let m = &o["matrix"];
        let heading = {
            let x = m[2][0].as_f64().unwrap_or(0.0);
            let z = m[2][2].as_f64().unwrap_or(1.0);
            x.atan2(z) as f32
        };
        let lock = sets.get("gCLock_PS").map(|l| LockDef {
            locked: l["Status"].as_str() == Some("Locked"),
            key: l["Key"].as_str().unwrap_or("").to_string(),
            difficulty: l["Difficulty"].as_i64().unwrap_or(-1) as i32,
        });
        let inv = &sets["gCInventory_PS"];
        let treasure_sets = (1..=5)
            .filter_map(|n| inv[format!("TreasureSet{n}")].as_str())
            .filter(|s| !s.is_empty())
            .map(str::to_string)
            .collect();
        let stacks = inv["stacks"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|s| {
                let item = s["item"].as_str()?;
                (!item.starts_with("deleted_") && item != "?")
                    .then(|| (item.to_string(), s["amount"].as_u64().unwrap_or(1) as u32))
            })
            .collect();
        out.push(WorldObjectDef {
            id: o["id"].as_str().unwrap_or("").to_string(),
            name: o["name"].as_str().unwrap_or("").to_string(),
            display: o["display_name"].as_str().map(str::to_string),
            position,
            heading,
            use_type,
            focus_name: inter["FocusNameType"].as_str().unwrap_or("").to_string(),
            is_item: sets.get("gCItem_PS").is_some(),
            lock,
            treasure_sets,
            stacks,
            used: inter["UsedByPlayer"].as_bool().unwrap_or(false),
        });
    }
    out
}

/// A `TS_*` treasure set template.
#[derive(Debug, Clone, Default)]
pub struct TreasureSet {
    /// `gETreasureDistribution` (`Plunder`, `Weaponry`, `Trader`, `Treasure`, ...).
    pub distribution: String,
    pub min_stacks: i32,
    pub max_stacks: i32,
    pub stacks: Vec<(String, u32)>,
}

/// All treasure sets by lowercase name.
#[derive(Debug, Default)]
pub struct Treasure {
    sets: HashMap<String, TreasureSet>,
}

/// Small deterministic generator for loot rolls (seeded per object / character).
pub struct Roll(pub u64);

impl Roll {
    pub fn next(&mut self, n: u32) -> u32 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        if n == 0 {
            0
        } else {
            ((self.0 >> 11) % u64::from(n)) as u32
        }
    }
}

/// FNV-1a hash of a string (seeds the loot rolls).
pub fn seed_of(s: &str) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for b in s.bytes() {
        h ^= u64::from(b);
        h = h.wrapping_mul(0x0000_0100_0000_01b3);
    }
    h | 1
}

impl Treasure {
    pub fn load(path: &Path) -> Self {
        let mut t = Self::default();
        let Ok(text) = std::fs::read_to_string(path) else {
            return t;
        };
        let Ok(root) = serde_json::from_str::<Value>(&text) else {
            return t;
        };
        for s in root["sets"].as_array().into_iter().flatten() {
            let name = s["name"].as_str().unwrap_or("").to_ascii_lowercase();
            let stacks = s["inventory"]["stacks"]
                .as_array()
                .into_iter()
                .flatten()
                .filter_map(|x| {
                    let item = x["item"].as_str()?;
                    (!item.starts_with("deleted_") && item != "?")
                        .then(|| (item.to_string(), x["amount"].as_u64().unwrap_or(1) as u32))
                })
                .collect();
            t.sets.insert(
                name,
                TreasureSet {
                    distribution: s["treasure"]["TreasureDistribution"]
                        .as_str()
                        .unwrap_or("")
                        .to_string(),
                    min_stacks: s["treasure"]["MinTransferStacks"].as_i64().unwrap_or(0) as i32,
                    max_stacks: s["treasure"]["MaxTransferStacks"].as_i64().unwrap_or(0) as i32,
                    stacks,
                },
            );
        }
        t
    }

    pub fn get(&self, name: &str) -> Option<&TreasureSet> {
        self.sets.get(&name.to_ascii_lowercase())
    }

    /// Weapons of the `Weaponry` sets of a character: every stack of the set.
    pub fn weaponry(&self, sets: &[String]) -> Vec<(String, u32)> {
        sets.iter()
            .filter_map(|n| self.get(n))
            .filter(|s| s.distribution == "Weaponry")
            .flat_map(|s| s.stacks.iter().cloned())
            .collect()
    }

    /// Plunder / treasure of the non-weaponry sets: `Min..=MaxTransferStacks` stacks picked at
    /// random per set (all stacks when both are zero) [I: the roll is the port's own].
    pub fn plunder(&self, sets: &[String], roll: &mut Roll) -> Vec<(String, u32)> {
        let mut out: Vec<(String, u32)> = Vec::new();
        for s in sets
            .iter()
            .filter_map(|n| self.get(n))
            .filter(|s| !matches!(s.distribution.as_str(), "Weaponry" | "Trader"))
        {
            let mut pool = s.stacks.clone();
            let n = if s.max_stacks <= 0 {
                pool.len()
            } else {
                let span = (s.max_stacks - s.min_stacks).max(0) as u32 + 1;
                ((s.min_stacks.max(0) as u32 + roll.next(span)) as usize).min(pool.len())
            };
            for _ in 0..n {
                let i = roll.next(pool.len() as u32) as usize;
                let (item, amount) = pool.remove(i);
                match out.iter_mut().find(|(k, _)| *k == item) {
                    Some((_, a)) => *a += amount,
                    None => out.push((item, amount)),
                }
            }
        }
        out
    }
}

/// A recipe template (`gCRecipe_PS` of a blueprint item).
#[derive(Debug, Clone)]
pub struct RecipeDef {
    /// Template of the blueprint / recipe item.
    pub item: String,
    pub category: SkillCategory,
    pub recipe: Recipe,
}

pub fn load_recipes(path: &Path) -> Vec<RecipeDef> {
    let Ok(text) = std::fs::read_to_string(path) else {
        return Vec::new();
    };
    let Ok(root) = serde_json::from_str::<Value>(&text) else {
        return Vec::new();
    };
    let mut out = Vec::new();
    for r in root["recipes"].as_array().into_iter().flatten() {
        let p = &r["sets"]["gCRecipe_PS"];
        let category = match p["Craft"].as_str() {
            Some("Alchemy") => SkillCategory::Alchemy,
            Some("Smithing") => SkillCategory::Smith,
            Some("Cooking") => SkillCategory::Cooking,
            _ => continue,
        };
        let ingredients = (1..=4)
            .filter_map(|n| {
                let item = p[format!("Ingredient{n}Item")].as_str()?;
                let amount = p[format!("Ingredient{n}Amount")].as_u64()? as u32;
                (amount > 0 && !item.is_empty()).then(|| (item.to_string(), amount))
            })
            .collect();
        let Some(result) = p["ResultItem"].as_str() else {
            continue;
        };
        out.push(RecipeDef {
            item: r["name"].as_str().unwrap_or("").to_string(),
            category,
            recipe: Recipe {
                category,
                ingredients,
                result: result.to_string(),
                result_amount: p["ResultAmount"].as_u64().unwrap_or(1) as u32,
                required_perk: p["ReqSkill1"].as_str().unwrap_or("").to_string(),
            },
        });
    }
    out
}

/// Item template -> mesh name (without extension) from the item export.
pub fn load_item_meshes(path: &Path) -> HashMap<String, String> {
    let mut out = HashMap::new();
    let Ok(text) = std::fs::read_to_string(path) else {
        return out;
    };
    let Ok(Value::Array(items)) = serde_json::from_str::<Value>(&text) else {
        return out;
    };
    for i in items {
        if let (Some(n), Some(m)) = (i["name"].as_str(), i["mesh"].as_str()) {
            out.insert(
                n.to_ascii_lowercase(),
                m.trim_end_matches(".xcmsh").to_string(),
            );
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plunder_picks_between_min_and_max_stacks() {
        let mut t = Treasure::default();
        t.sets.insert(
            "ts_x".into(),
            TreasureSet {
                distribution: "Plunder".into(),
                min_stacks: 2,
                max_stacks: 3,
                stacks: vec![
                    ("A".into(), 1),
                    ("B".into(), 1),
                    ("C".into(), 1),
                    ("D".into(), 1),
                ],
            },
        );
        for seed in 1..40u64 {
            let got = t.plunder(&["TS_X".into()], &mut Roll(seed * 7919));
            assert!((2..=3).contains(&got.len()), "{got:?}");
        }
        assert!(t.weaponry(&["TS_X".into()]).is_empty());
    }
}
