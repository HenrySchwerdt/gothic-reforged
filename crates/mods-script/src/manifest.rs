//! The `[mod]` and `[script]` sections of `mod.toml` (other sections, e.g. data overlay
//! settings, are ignored here).
//!
//! ```toml
//! [mod]
//! id = "chicken_rain"
//! version = "1.0.0"
//!
//! [script]
//! entry = "scripts/main.rhai"            # default: main.rhai
//! capabilities = ["world", "spawn", "ui"]
//! ```
use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;

/// What a script may do beyond pure computation, logging and its seeded RNG.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Capability {
    /// Read the world snapshot: hero position, actors, quest states, game time.
    World,
    /// Spawn NPCs.
    Spawn,
    /// Give items to the hero or NPCs.
    Items,
    /// Show on-screen notices.
    Ui,
    /// Rewrite subtitles through `fn rewrite_line(line)`.
    Subtitles,
}

impl Capability {
    pub fn name(self) -> &'static str {
        match self {
            Self::World => "world",
            Self::Spawn => "spawn",
            Self::Items => "items",
            Self::Ui => "ui",
            Self::Subtitles => "subtitles",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ModHeader {
    pub id: String,
    #[serde(default)]
    pub name: String,
    pub version: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ScriptSection {
    #[serde(default = "default_entry")]
    pub entry: String,
    #[serde(default)]
    pub capabilities: BTreeSet<Capability>,
}

fn default_entry() -> String {
    "main.rhai".into()
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ScriptManifest {
    #[serde(rename = "mod")]
    pub info: ModHeader,
    /// `None` for a mod without scripts (a pure data overlay).
    pub script: Option<ScriptSection>,
}

impl ScriptManifest {
    pub fn parse(text: &str) -> Result<Self> {
        let m: Self = toml::from_str(text).context("mod.toml")?;
        let id = &m.info.id;
        if id.is_empty()
            || id.len() > 64
            || !id
                .chars()
                .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_' || c == '-')
        {
            bail!("mod id {id:?} must be 1-64 characters of [a-z0-9_-]");
        }
        if let Some(s) = &m.script
            && (s.entry.contains("..") || s.entry.starts_with('/') || s.entry.contains('\\'))
        {
            bail!(
                "mod {id}: script entry {:?} must stay inside the mod folder",
                s.entry
            );
        }
        Ok(m)
    }

    /// An in-memory manifest (tests, tools).
    pub fn new(id: &str, capabilities: &[Capability]) -> Self {
        Self {
            info: ModHeader {
                id: id.into(),
                name: id.into(),
                version: "0.0.0".into(),
            },
            script: Some(ScriptSection {
                entry: default_entry(),
                capabilities: capabilities.iter().copied().collect(),
            }),
        }
    }

    pub fn id(&self) -> &str {
        &self.info.id
    }

    pub fn capabilities(&self) -> BTreeSet<Capability> {
        self.script
            .as_ref()
            .map(|s| s.capabilities.clone())
            .unwrap_or_default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_script_section() {
        let m = ScriptManifest::parse(
            "[mod]\nid = \"x\"\nversion = \"1.0.0\"\n[script]\ncapabilities = [\"spawn\", \"ui\"]\n[dependencies]\n",
        )
        .unwrap();
        assert_eq!(m.script.as_ref().unwrap().entry, "main.rhai");
        assert!(m.capabilities().contains(&Capability::Spawn));
        assert!(
            ScriptManifest::parse(
                "[mod]\nid = \"x\"\nversion = \"1\"\n[script]\ncapabilities = [\"fs\"]\n"
            )
            .is_err()
        );
        assert!(
            ScriptManifest::parse(
                "[mod]\nid = \"x\"\nversion = \"1\"\n[script]\nentry = \"../evil.rhai\"\n"
            )
            .is_err()
        );
        assert!(
            ScriptManifest::parse("[mod]\nid = \"x\"\nversion = \"1\"\n")
                .unwrap()
                .script
                .is_none()
        );
    }
}
