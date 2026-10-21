//! Patch files: typed add / patch / delete operations per record kind, in TOML or RON.
//!
//! ```toml
//! [[items.add]]                      # a whole new record (unset fields take defaults)
//! name = "It_HermitTea"
//! category = "Potion"
//! value = 25
//!
//! [[quests.patch]]                   # change fields of an existing record
//! name = "Q_Letter"
//! set = { experience_points = 250 }
//!
//! [[infos.delete]]
//! name = "B_Smalltalk"
//!
//! [[strings.set]]                    # add or replace a localized string
//! key = "MOD_HERMIT_HELLO"
//! texts = { English = "Who goes there?", German = "Wer da?" }
//!
//! [[strings.delete]]
//! key = "OLD_KEY"
//! ```
//!
//! Record kinds: `quests`, `infos` (dialog nodes with their script lines), `items`, `npcs`
//! (NPC templates and entities), plus `strings`. Records are identified by their `name`
//! (case-insensitive), strings by `key`. `set` replaces top-level fields (lists are replaced as
//! a whole); the result is type-checked against the record type, so a typo or a wrong type is
//! reported instead of silently ignored.
use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use std::collections::BTreeMap;
use std::path::Path;

/// Kinds of patchable records.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RecordKind {
    Quest,
    Info,
    Item,
    Npc,
    String,
}

impl RecordKind {
    pub fn name(self) -> &'static str {
        match self {
            Self::Quest => "quest",
            Self::Info => "info",
            Self::Item => "item",
            Self::Npc => "npc",
            Self::String => "string",
        }
    }
}

/// Change fields of an existing record.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FieldPatch {
    pub name: String,
    pub set: Map<String, Value>,
}

/// Remove a record.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Delete {
    pub name: String,
}

/// Operations on one record kind.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RecordOps {
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub add: Vec<Map<String, Value>>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub patch: Vec<FieldPatch>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub delete: Vec<Delete>,
}

impl RecordOps {
    pub fn is_empty(&self) -> bool {
        self.add.is_empty() && self.patch.is_empty() && self.delete.is_empty()
    }
}

/// Add or replace a localized string; languages not given keep their text (or stay empty).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct StringSet {
    pub key: String,
    pub texts: BTreeMap<String, String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct StringDelete {
    pub key: String,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct StringOps {
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub set: Vec<StringSet>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub delete: Vec<StringDelete>,
}

impl StringOps {
    pub fn is_empty(&self) -> bool {
        self.set.is_empty() && self.delete.is_empty()
    }
}

/// One patch file.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PatchFile {
    #[serde(default, skip_serializing_if = "RecordOps::is_empty")]
    pub quests: RecordOps,
    #[serde(default, skip_serializing_if = "RecordOps::is_empty")]
    pub infos: RecordOps,
    #[serde(default, skip_serializing_if = "RecordOps::is_empty")]
    pub items: RecordOps,
    #[serde(default, skip_serializing_if = "RecordOps::is_empty")]
    pub npcs: RecordOps,
    #[serde(default, skip_serializing_if = "StringOps::is_empty")]
    pub strings: StringOps,
}

impl PatchFile {
    pub fn ops(&self, kind: RecordKind) -> Option<&RecordOps> {
        match kind {
            RecordKind::Quest => Some(&self.quests),
            RecordKind::Info => Some(&self.infos),
            RecordKind::Item => Some(&self.items),
            RecordKind::Npc => Some(&self.npcs),
            RecordKind::String => None,
        }
    }

    pub fn ops_mut(&mut self, kind: RecordKind) -> Option<&mut RecordOps> {
        match kind {
            RecordKind::Quest => Some(&mut self.quests),
            RecordKind::Info => Some(&mut self.infos),
            RecordKind::Item => Some(&mut self.items),
            RecordKind::Npc => Some(&mut self.npcs),
            RecordKind::String => None,
        }
    }

    pub fn from_toml(text: &str) -> Result<Self> {
        Ok(toml::from_str(text)?)
    }

    pub fn from_ron(text: &str) -> Result<Self> {
        Ok(ron::from_str(text)?)
    }

    pub fn to_toml(&self) -> Result<String> {
        Ok(toml::to_string_pretty(self)?)
    }

    pub fn to_ron(&self) -> Result<String> {
        Ok(ron::ser::to_string_pretty(
            self,
            ron::ser::PrettyConfig::default(),
        )?)
    }

    /// Load a `.toml` or `.ron` patch file.
    pub fn load(path: &Path) -> Result<Self> {
        let text =
            std::fs::read_to_string(path).with_context(|| format!("reading {}", path.display()))?;
        let r = match path.extension().and_then(|e| e.to_str()) {
            Some("toml") => Self::from_toml(&text),
            Some("ron") => Self::from_ron(&text),
            other => bail!("{}: unsupported patch extension {other:?}", path.display()),
        };
        r.with_context(|| format!("parsing {}", path.display()))
    }

    pub fn is_empty(&self) -> bool {
        self.quests.is_empty()
            && self.infos.is_empty()
            && self.items.is_empty()
            && self.npcs.is_empty()
            && self.strings.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const TOML: &str = r#"
[[items.add]]
name = "It_Tea"
value = 25

[[quests.patch]]
name = "Q_Letter"
set = { experience_points = 250 }

[[infos.delete]]
name = "B_Smalltalk"

[[strings.set]]
key = "MOD_HELLO"
texts = { English = "Hello", German = "Hallo" }
"#;

    #[test]
    fn toml_and_ron_round_trip() {
        let p = PatchFile::from_toml(TOML).unwrap();
        assert_eq!(p.items.add[0]["name"], "It_Tea");
        assert_eq!(p.quests.patch[0].set["experience_points"], 250);
        assert_eq!(p.strings.set[0].texts["German"], "Hallo");
        let again = PatchFile::from_toml(&p.to_toml().unwrap()).unwrap();
        assert_eq!(p, again);
        let ron = p.to_ron().unwrap();
        assert_eq!(PatchFile::from_ron(&ron).unwrap(), p);
    }

    #[test]
    fn unknown_sections_and_fields_are_errors() {
        assert!(PatchFile::from_toml("[[weapons.add]]\nname = \"x\"\n").is_err());
        assert!(PatchFile::from_toml("[[quests.patch]]\nname = \"x\"\nsett = {}\n").is_err());
    }
}
