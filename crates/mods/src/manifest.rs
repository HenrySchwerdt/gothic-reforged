//! `mod.toml`: identity, version, dependencies and load-order hints of a mod.
//!
//! ```toml
//! [mod]
//! id = "forest_hermit"          # [a-z0-9_-], unique
//! name = "The Forest Hermit"
//! version = "1.2.0"             # semver
//! description = "..."
//! authors = ["..."]
//!
//! [dependencies]                # id = semver requirement
//! base_fixes = ">=1.0"
//!
//! [load]
//! after = ["other_mod"]         # optional ordering hints (no dependency)
//! before = ["late_mod"]
//! ```
use anyhow::{Context, Result, bail};
use semver::{Version, VersionReq};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ModInfo {
    pub id: String,
    #[serde(default)]
    pub name: String,
    pub version: String,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub authors: Vec<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct LoadHints {
    #[serde(default)]
    pub after: Vec<String>,
    #[serde(default)]
    pub before: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Manifest {
    #[serde(rename = "mod")]
    pub info: ModInfo,
    #[serde(default)]
    pub dependencies: BTreeMap<String, String>,
    #[serde(default)]
    pub load: LoadHints,
}

impl Manifest {
    pub fn parse(text: &str) -> Result<Self> {
        let m: Manifest = toml::from_str(text).context("mod.toml")?;
        m.validate()?;
        Ok(m)
    }

    pub fn id(&self) -> &str {
        &self.info.id
    }

    pub fn version(&self) -> Result<Version> {
        Version::parse(&self.info.version)
            .with_context(|| format!("mod {}: version {:?}", self.info.id, self.info.version))
    }

    /// Parsed dependency requirements.
    pub fn requirements(&self) -> Result<Vec<(String, VersionReq)>> {
        self.dependencies
            .iter()
            .map(|(id, req)| {
                VersionReq::parse(req)
                    .map(|r| (id.clone(), r))
                    .with_context(|| format!("mod {}: dependency {id} = {req:?}", self.info.id))
            })
            .collect()
    }

    fn validate(&self) -> Result<()> {
        let id = &self.info.id;
        if id.is_empty()
            || id.len() > 64
            || !id
                .chars()
                .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_' || c == '-')
        {
            bail!("mod id {id:?} must be 1-64 characters of [a-z0-9_-]");
        }
        self.version()?;
        self.requirements()?;
        if self.dependencies.contains_key(id) {
            bail!("mod {id} depends on itself");
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_and_validates() {
        let m = Manifest::parse(
            "[mod]\nid = \"a_mod\"\nversion = \"1.2.3\"\n[dependencies]\nbase = \">=1.0\"\n[load]\nafter = [\"x\"]\n",
        )
        .unwrap();
        assert_eq!(m.id(), "a_mod");
        assert_eq!(m.requirements().unwrap()[0].0, "base");
        assert_eq!(m.load.after, vec!["x"]);
        assert!(Manifest::parse("[mod]\nid = \"Bad Id\"\nversion = \"1.0.0\"\n").is_err());
        assert!(Manifest::parse("[mod]\nid = \"ok\"\nversion = \"one\"\n").is_err());
        assert!(
            Manifest::parse(
                "[mod]\nid = \"ok\"\nversion = \"1.0.0\"\n[dependencies]\nok = \"*\"\n"
            )
            .is_err()
        );
    }
}
