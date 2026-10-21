//! The moddable data set: quests, infos and strings (the [`GameDatabase`] parts) plus item
//! templates and NPC records.
//!
//! Records are held in their serde JSON form while patches are applied and converted back to
//! the typed structs ([`Quest`], [`Info`], [`ItemDef`], [`NpcRecord`]) for validation and
//! output, so every patched record is checked against the real type.
use crate::patch::RecordKind;
use anyhow::{Context, Result};
use gothic_formats::info::Info;
use gothic_formats::quest::Quest;
use gothic_formats::stringtable::{StringEntry, StringTable};
use gothic_gameplay::GameDatabase;
use gothic_sim::data::NpcRecord;
use gothic_sim::item::ItemDef;
use serde_json::{Map, Value};

/// All patchable data.
#[derive(Debug, Clone)]
pub struct ModData {
    pub quests: Vec<Value>,
    pub infos: Vec<Value>,
    pub items: Vec<Value>,
    /// NPC templates/entities in their `npcs.json` form.
    pub npcs: Vec<Value>,
    pub strings: StringTable,
}

fn to_values<T: serde::Serialize>(v: &[T]) -> Vec<Value> {
    v.iter()
        .map(|r| serde_json::to_value(r).expect("record serialises"))
        .collect()
}

impl PartialEq for ModData {
    /// Same records and the same string table contents (languages and entries).
    fn eq(&self, other: &Self) -> bool {
        let strings = |t: &StringTable| {
            (
                t.languages.clone(),
                t.entries
                    .iter()
                    .map(|e| (e.key.clone(), e.texts.clone()))
                    .collect::<Vec<_>>(),
            )
        };
        self.quests == other.quests
            && self.infos == other.infos
            && self.items == other.items
            && self.npcs == other.npcs
            && strings(&self.strings) == strings(&other.strings)
    }
}

/// A record's identifying name (`name` field).
pub fn record_name(v: &Value) -> Option<&str> {
    v.get("name").and_then(Value::as_str)
}

impl ModData {
    pub fn new(
        quests: &[Quest],
        infos: &[Info],
        strings: StringTable,
        items: &[ItemDef],
        npcs: Vec<Value>,
    ) -> Self {
        Self {
            quests: to_values(quests),
            infos: to_values(infos),
            items: to_values(items),
            npcs,
            strings,
        }
    }

    /// The quest/info/string parts of a database (no items/NPCs).
    pub fn from_database(db: &GameDatabase) -> Self {
        Self::new(&db.quests, &db.infos, db.strings.clone(), &[], Vec::new())
    }

    pub fn records(&self, kind: RecordKind) -> &Vec<Value> {
        match kind {
            RecordKind::Quest => &self.quests,
            RecordKind::Info => &self.infos,
            RecordKind::Item => &self.items,
            RecordKind::Npc | RecordKind::String => &self.npcs,
        }
    }

    pub fn records_mut(&mut self, kind: RecordKind) -> &mut Vec<Value> {
        match kind {
            RecordKind::Quest => &mut self.quests,
            RecordKind::Info => &mut self.infos,
            RecordKind::Item => &mut self.items,
            RecordKind::Npc | RecordKind::String => &mut self.npcs,
        }
    }

    /// Index of a record by name (case-insensitive).
    pub fn find(&self, kind: RecordKind, name: &str) -> Option<usize> {
        self.records(kind)
            .iter()
            .position(|r| record_name(r).is_some_and(|n| n.eq_ignore_ascii_case(name)))
    }

    pub fn quests_typed(&self) -> Result<Vec<Quest>> {
        typed(&self.quests, "quest")
    }

    pub fn infos_typed(&self) -> Result<Vec<Info>> {
        typed(&self.infos, "info")
    }

    pub fn items_typed(&self) -> Result<Vec<ItemDef>> {
        typed(&self.items, "item")
    }

    pub fn npcs_typed(&self) -> Result<Vec<NpcRecord>> {
        typed(&self.npcs, "npc")
    }

    /// Build the indexed [`GameDatabase`] for a language.
    pub fn to_database(&self, language: &str) -> Result<GameDatabase> {
        let mut strings = self.strings.clone();
        strings.rebuild_index();
        GameDatabase::from_parts(self.quests_typed()?, self.infos_typed()?, strings, language)
    }

    /// Set (add or replace) a string's texts by language name.
    pub fn set_string(&mut self, key: &str, texts: &std::collections::BTreeMap<String, String>) {
        let langs = self.strings.languages.clone();
        let pos = self
            .strings
            .entries
            .iter()
            .position(|e| e.key.eq_ignore_ascii_case(key));
        let entry = match pos {
            Some(i) => &mut self.strings.entries[i],
            None => {
                self.strings.entries.push(StringEntry {
                    key: key.to_string(),
                    texts: vec![String::new(); langs.len()],
                });
                self.strings.entries.last_mut().expect("just pushed")
            }
        };
        entry.texts.resize(langs.len(), String::new());
        for (lang, text) in texts {
            if let Some(i) = langs.iter().position(|l| l.eq_ignore_ascii_case(lang)) {
                entry.texts[i] = text.clone();
            }
        }
        self.strings.rebuild_index();
    }

    pub fn delete_string(&mut self, key: &str) -> bool {
        let before = self.strings.entries.len();
        self.strings
            .entries
            .retain(|e| !e.key.eq_ignore_ascii_case(key));
        self.strings.rebuild_index();
        before != self.strings.entries.len()
    }
}

fn typed<T: serde::de::DeserializeOwned>(values: &[Value], what: &str) -> Result<Vec<T>> {
    values
        .iter()
        .map(|v| {
            serde_json::from_value(v.clone())
                .with_context(|| format!("{what} {:?}", record_name(v).unwrap_or("<unnamed>")))
        })
        .collect()
}

/// The JSON form of an empty record of `kind` named `name` (the base an `add` fills in).
pub fn empty_record(kind: RecordKind, name: &str) -> Value {
    let v = match kind {
        RecordKind::Quest => serde_json::to_value(
            Quest::parse(format!("[Quest]\r\nName={name}\r\n").as_bytes()).expect("empty quest"),
        ),
        RecordKind::Info => serde_json::to_value(
            Info::parse(format!("[Info]\r\nName={name}\r\n").as_bytes()).expect("empty info"),
        ),
        RecordKind::Item => serde_json::to_value(ItemDef {
            name: name.to_string(),
            ..ItemDef::default()
        }),
        RecordKind::Npc | RecordKind::String => {
            let mut m = Map::new();
            m.insert("name".into(), Value::String(name.to_string()));
            Ok(Value::Object(m))
        }
    };
    v.expect("empty record serialises")
}

/// Check that a JSON record converts to its typed form.
pub fn validate(kind: RecordKind, v: &Value) -> Result<()> {
    match kind {
        RecordKind::Quest => serde_json::from_value::<Quest>(v.clone()).map(drop),
        RecordKind::Info => serde_json::from_value::<Info>(v.clone()).map(drop),
        RecordKind::Item => serde_json::from_value::<ItemDef>(v.clone()).map(drop),
        RecordKind::Npc => serde_json::from_value::<NpcRecord>(v.clone()).map(drop),
        RecordKind::String => Ok(()),
    }
    .map_err(anyhow::Error::from)
}
