//! Mod discovery and load order.
//!
//! A mods directory holds one folder per mod: `mod.toml` plus `patches/*.toml|*.ron`
//! (applied in file-name order). The load order is a topological order of the dependency and
//! `load.after` / `load.before` edges, ties broken by mod id, so it is deterministic.
use crate::manifest::Manifest;
use crate::patch::PatchFile;
use anyhow::{Context, Result};
use semver::Version;
use serde::Serialize;
use std::collections::btree_map::Entry;
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

/// A mod read from disk (or built in memory).
#[derive(Debug, Clone)]
pub struct LoadedMod {
    pub manifest: Manifest,
    pub dir: PathBuf,
    /// (file name, patch) in application order.
    pub patches: Vec<(String, PatchFile)>,
}

impl LoadedMod {
    pub fn id(&self) -> &str {
        self.manifest.id()
    }

    /// Read `dir/mod.toml` and `dir/patches/*`.
    pub fn load(dir: &Path) -> Result<Self> {
        let text = std::fs::read_to_string(dir.join("mod.toml"))
            .with_context(|| format!("reading {}", dir.join("mod.toml").display()))?;
        let manifest =
            Manifest::parse(&text).with_context(|| format!("mod at {}", dir.display()))?;
        let mut files: Vec<PathBuf> = match std::fs::read_dir(dir.join("patches")) {
            Ok(rd) => rd
                .filter_map(|e| e.ok().map(|e| e.path()))
                .filter(|p| {
                    p.is_file()
                        && matches!(p.extension().and_then(|e| e.to_str()), Some("toml" | "ron"))
                })
                .collect(),
            Err(_) => Vec::new(),
        };
        files.sort();
        let patches = files
            .iter()
            .map(|p| {
                let name = p
                    .file_name()
                    .map_or_else(String::new, |n| n.to_string_lossy().into_owned());
                PatchFile::load(p).map(|f| (name, f))
            })
            .collect::<Result<Vec<_>>>()?;
        Ok(Self {
            manifest,
            dir: dir.to_path_buf(),
            patches,
        })
    }
}

/// Every mod folder (a directory with `mod.toml`) under `mods_dir`, sorted by folder name.
pub fn discover(mods_dir: &Path) -> Result<Vec<LoadedMod>> {
    let mut dirs: Vec<PathBuf> = std::fs::read_dir(mods_dir)
        .with_context(|| format!("reading {}", mods_dir.display()))?
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| p.join("mod.toml").is_file())
        .collect();
    dirs.sort();
    dirs.iter().map(|d| LoadedMod::load(d)).collect()
}

/// Problems that keep a mod from loading.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "problem", rename_all = "snake_case")]
pub enum OrderProblem {
    DuplicateId {
        id: String,
    },
    MissingDependency {
        id: String,
        needs: String,
    },
    VersionMismatch {
        id: String,
        needs: String,
        requirement: String,
        found: String,
    },
    Cycle {
        mods: Vec<String>,
    },
}

/// The resolved order and the mods left out (with their problems).
#[derive(Debug, Clone, Default)]
pub struct LoadOrder {
    pub order: Vec<LoadedMod>,
    pub problems: Vec<OrderProblem>,
}

/// Resolve the load order. Mods with a missing or mismatching dependency are left out (and so
/// are mods depending on them); a dependency cycle leaves out its members.
pub fn resolve(mods: Vec<LoadedMod>) -> LoadOrder {
    let mut problems = Vec::new();
    let mut by_id: BTreeMap<String, LoadedMod> = BTreeMap::new();
    for m in mods {
        let id = m.id().to_string();
        match by_id.entry(id) {
            Entry::Occupied(e) => problems.push(OrderProblem::DuplicateId {
                id: e.key().clone(),
            }),
            Entry::Vacant(v) => {
                v.insert(m);
            }
        }
    }
    // Drop mods whose dependencies are missing or mismatching, until stable.
    loop {
        let mut drop: Vec<String> = Vec::new();
        for (id, m) in &by_id {
            for (dep, req) in m.manifest.requirements().unwrap_or_default() {
                match by_id.get(&dep) {
                    None => {
                        problems.push(OrderProblem::MissingDependency {
                            id: id.clone(),
                            needs: dep.clone(),
                        });
                        drop.push(id.clone());
                    }
                    Some(d) => {
                        let found = d.manifest.version().unwrap_or(Version::new(0, 0, 0));
                        if !req.matches(&found) {
                            problems.push(OrderProblem::VersionMismatch {
                                id: id.clone(),
                                needs: dep.clone(),
                                requirement: req.to_string(),
                                found: found.to_string(),
                            });
                            drop.push(id.clone());
                        }
                    }
                }
            }
        }
        if drop.is_empty() {
            break;
        }
        for id in drop {
            by_id.remove(&id);
        }
    }
    // Edges a -> b: a loads before b.
    let ids: BTreeSet<String> = by_id.keys().cloned().collect();
    let mut edges: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
    let mut add = |a: &str, b: &str| {
        if ids.contains(a) && ids.contains(b) && a != b {
            edges
                .entry(a.to_string())
                .or_default()
                .insert(b.to_string());
        }
    };
    for (id, m) in &by_id {
        for dep in m.manifest.dependencies.keys() {
            add(dep, id);
        }
        for a in &m.manifest.load.after {
            add(a, id);
        }
        for b in &m.manifest.load.before {
            add(id, b);
        }
    }
    // Kahn's algorithm, smallest id first.
    let mut indegree: BTreeMap<String, usize> = ids.iter().map(|i| (i.clone(), 0)).collect();
    for targets in edges.values() {
        for t in targets {
            *indegree.get_mut(t).expect("known id") += 1;
        }
    }
    let mut ready: BTreeSet<String> = indegree
        .iter()
        .filter(|(_, d)| **d == 0)
        .map(|(i, _)| i.clone())
        .collect();
    let mut order = Vec::new();
    while let Some(id) = ready.iter().next().cloned() {
        ready.remove(&id);
        if let Some(ts) = edges.get(&id) {
            for t in ts {
                let d = indegree.get_mut(t).expect("known id");
                *d -= 1;
                if *d == 0 {
                    ready.insert(t.clone());
                }
            }
        }
        order.push(id);
    }
    if order.len() < ids.len() {
        let cyc: Vec<String> = ids.iter().filter(|i| !order.contains(i)).cloned().collect();
        problems.push(OrderProblem::Cycle { mods: cyc });
    }
    LoadOrder {
        order: order
            .into_iter()
            .filter_map(|id| by_id.remove(&id))
            .collect(),
        problems,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn m(id: &str, version: &str, deps: &[(&str, &str)], after: &[&str]) -> LoadedMod {
        let mut text = format!("[mod]\nid = \"{id}\"\nversion = \"{version}\"\n[dependencies]\n");
        for (d, r) in deps {
            text.push_str(&format!("{d} = \"{r}\"\n"));
        }
        text.push_str("[load]\nafter = [");
        text.push_str(
            &after
                .iter()
                .map(|a| format!("\"{a}\""))
                .collect::<Vec<_>>()
                .join(","),
        );
        text.push_str("]\n");
        LoadedMod {
            manifest: Manifest::parse(&text).unwrap(),
            dir: PathBuf::new(),
            patches: Vec::new(),
        }
    }

    fn ids(o: &LoadOrder) -> Vec<&str> {
        o.order.iter().map(|m| m.id()).collect()
    }

    #[test]
    fn dependencies_and_hints_order_mods() {
        let o = resolve(vec![
            m("c", "1.0.0", &[("a", ">=1")], &[]),
            m("b", "1.0.0", &[], &["c"]),
            m("a", "1.5.0", &[], &[]),
        ]);
        assert_eq!(ids(&o), ["a", "c", "b"]);
        assert!(o.problems.is_empty());
    }

    #[test]
    fn missing_mismatching_and_cyclic_mods_are_left_out() {
        let o = resolve(vec![
            m("a", "1.0.0", &[("ghost", "*")], &[]),
            m("b", "1.0.0", &[("c", ">=2")], &[]),
            m("c", "1.0.0", &[], &[]),
            m("d", "1.0.0", &[("e", "*")], &[]),
            m("e", "1.0.0", &[("d", "*")], &[]),
            m("f", "1.0.0", &[("a", "*")], &[]),
        ]);
        assert_eq!(ids(&o), ["c"]);
        assert!(o.problems.contains(&OrderProblem::MissingDependency {
            id: "a".into(),
            needs: "ghost".into()
        }));
        assert!(
            o.problems
                .iter()
                .any(|p| matches!(p, OrderProblem::VersionMismatch { id, .. } if id == "b"))
        );
        assert!(
            o.problems
                .iter()
                .any(|p| matches!(p, OrderProblem::Cycle { mods } if mods == &["d", "e"]))
        );
        // f depended on a, which was dropped.
        assert!(
            o.problems
                .iter()
                .any(|p| matches!(p, OrderProblem::MissingDependency { id, .. } if id == "f"))
        );
    }
}
