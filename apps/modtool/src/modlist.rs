//! The mod list: installed mods, enabled state and user order, persisted in
//! `<mods>/modlist.toml`. This is the data model of the in-game mod menu: it lists every
//! installed mod with its status and lets the player enable, disable and reorder. The load
//! order respects dependencies and `load.after` / `load.before` hints; among mods free to go
//! either way, the user's order decides.
use anyhow::{Context, Result};
use gothic_mods::{LoadedMod, OrderProblem};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

pub const LIST_FILE: &str = "modlist.toml";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct Item {
    id: String,
    enabled: bool,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
struct ListFile {
    #[serde(default)]
    mods: Vec<Item>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ModStatus {
    /// Enabled and in the load order.
    Active,
    Disabled,
    /// Enabled but not loadable (bad manifest, missing dependency, cycle, ...).
    Problem,
}

/// One row of the mod menu.
#[derive(Debug, Clone, Serialize)]
pub struct ModEntry {
    pub id: String,
    pub name: String,
    pub version: String,
    pub description: String,
    pub authors: Vec<String>,
    pub has_script: bool,
    pub patch_files: usize,
    pub enabled: bool,
    /// Position in the user's list (0 = top).
    pub position: usize,
    /// Index in the final load order (active mods only).
    pub load_index: Option<usize>,
    pub status: ModStatus,
    pub problems: Vec<String>,
}

pub struct ModList {
    dir: PathBuf,
    items: Vec<Item>,
    mods: BTreeMap<String, Result<LoadedMod, String>>,
}

fn has_script(dir: &Path) -> bool {
    std::fs::read_to_string(dir.join("mod.toml"))
        .ok()
        .and_then(|t| gothic_mods_script::ScriptManifest::parse(&t).ok())
        .is_some_and(|m| m.script.is_some())
}

fn problem_text(p: &OrderProblem) -> (String, String) {
    match p {
        OrderProblem::DuplicateId { id } => (id.clone(), "duplicate mod id".into()),
        OrderProblem::MissingDependency { id, needs } => (
            id.clone(),
            format!("needs {needs}, which is missing or disabled"),
        ),
        OrderProblem::VersionMismatch {
            id,
            needs,
            requirement,
            found,
        } => (
            id.clone(),
            format!("needs {needs} {requirement}, found {found}"),
        ),
        OrderProblem::Cycle { mods } => (String::new(), format!("dependency cycle: {mods:?}")),
    }
}

impl ModList {
    /// Scan `mods_dir` and merge with the saved list (new mods are appended, enabled;
    /// uninstalled ones are dropped).
    pub fn load(mods_dir: &Path) -> Result<Self> {
        let mut saved: ListFile = match std::fs::read_to_string(mods_dir.join(LIST_FILE)) {
            Ok(t) => toml::from_str(&t).context(LIST_FILE)?,
            Err(_) => ListFile::default(),
        };
        let mut mods = BTreeMap::new();
        if mods_dir.is_dir() {
            for e in std::fs::read_dir(mods_dir)? {
                let dir = e?.path();
                let name = dir
                    .file_name()
                    .map(|n| n.to_string_lossy().into_owned())
                    .unwrap_or_default();
                if name.starts_with('.') || !dir.join("mod.toml").is_file() {
                    continue;
                }
                let loaded = LoadedMod::load(&dir).map_err(|e| format!("{e:#}"));
                let id = loaded.as_ref().map(|m| m.id().to_string()).unwrap_or(name);
                mods.insert(id, loaded);
            }
        }
        saved.mods.retain(|i| mods.contains_key(&i.id));
        let known: BTreeSet<String> = saved.mods.iter().map(|i| i.id.clone()).collect();
        for id in mods.keys() {
            if !known.contains(id) {
                saved.mods.push(Item {
                    id: id.clone(),
                    enabled: true,
                });
            }
        }
        Ok(Self {
            dir: mods_dir.to_path_buf(),
            items: saved.mods,
            mods,
        })
    }

    pub fn save(&self) -> Result<()> {
        std::fs::create_dir_all(&self.dir)?;
        let text = toml::to_string_pretty(&ListFile {
            mods: self.items.clone(),
        })?;
        std::fs::write(self.dir.join(LIST_FILE), text)?;
        Ok(())
    }

    pub fn set_enabled(&mut self, id: &str, enabled: bool) -> bool {
        match self.items.iter_mut().find(|i| i.id == id) {
            Some(i) => {
                i.enabled = enabled;
                true
            }
            None => false,
        }
    }

    /// Move a mod to `position` in the user's list.
    pub fn move_to(&mut self, id: &str, position: usize) -> bool {
        let Some(from) = self.items.iter().position(|i| i.id == id) else {
            return false;
        };
        let item = self.items.remove(from);
        self.items.insert(position.min(self.items.len()), item);
        true
    }

    pub fn forget(&mut self, id: &str) {
        self.items.retain(|i| i.id != id);
        self.mods.remove(id);
    }

    /// Final load order of the enabled mods and the problems that kept mods out.
    pub fn load_order(&self) -> (Vec<String>, Vec<OrderProblem>) {
        let enabled: Vec<LoadedMod> = self
            .items
            .iter()
            .filter(|i| i.enabled)
            .filter_map(|i| self.mods.get(&i.id)?.as_ref().ok().cloned())
            .collect();
        let resolved = gothic_mods::resolve(enabled);
        let ok: BTreeMap<String, &LoadedMod> = resolved
            .order
            .iter()
            .map(|m| (m.id().to_string(), m))
            .collect();
        let pos = |id: &str| {
            self.items
                .iter()
                .position(|i| i.id == id)
                .unwrap_or(usize::MAX)
        };
        // Kahn's algorithm over the same edges, ready set ordered by user position.
        let mut indegree: BTreeMap<&str, usize> = ok.keys().map(|k| (k.as_str(), 0)).collect();
        let mut edges: BTreeMap<&str, BTreeSet<&str>> = BTreeMap::new();
        for (id, m) in &ok {
            let mut add = |a: &str, b: &str| {
                if let (Some((a, _)), Some((b, _))) = (ok.get_key_value(a), ok.get_key_value(b))
                    && a != b
                    && edges.entry(a.as_str()).or_default().insert(b.as_str())
                {
                    *indegree.get_mut(b.as_str()).expect("known") += 1;
                }
            };
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
        let mut ready: BTreeSet<(usize, &str)> = indegree
            .iter()
            .filter(|(_, d)| **d == 0)
            .map(|(id, _)| (pos(id), *id))
            .collect();
        let mut order = Vec::new();
        while let Some(first) = ready.iter().next().copied() {
            ready.remove(&first);
            let id = first.1;
            order.push(id.to_string());
            if let Some(ts) = edges.get(id) {
                for t in ts {
                    let d = indegree.get_mut(t).expect("known");
                    *d -= 1;
                    if *d == 0 {
                        ready.insert((pos(t), *t));
                    }
                }
            }
        }
        (order, resolved.problems)
    }

    /// The loaded mods of the final order (for `gothic_mods::apply` and the script host).
    pub fn active_mods(&self) -> Vec<LoadedMod> {
        self.load_order()
            .0
            .iter()
            .filter_map(|id| self.mods.get(id)?.as_ref().ok().cloned())
            .collect()
    }

    pub fn entries(&self) -> Vec<ModEntry> {
        let (order, problems) = self.load_order();
        let mut by_mod: BTreeMap<String, Vec<String>> = BTreeMap::new();
        let mut global = Vec::new();
        for p in &problems {
            let (id, text) = problem_text(p);
            if let OrderProblem::Cycle { mods } = p {
                for m in mods {
                    by_mod.entry(m.clone()).or_default().push(text.clone());
                }
            } else if id.is_empty() {
                global.push(text);
            } else {
                by_mod.entry(id).or_default().push(text);
            }
        }
        self.items
            .iter()
            .enumerate()
            .map(|(position, item)| {
                let dir = self.dir.join(&item.id);
                let load_index = order.iter().position(|o| *o == item.id);
                let mut problems = by_mod.get(&item.id).cloned().unwrap_or_default();
                let (name, version, description, authors, patch_files) =
                    match self.mods.get(&item.id) {
                        Some(Ok(m)) => (
                            m.manifest.info.name.clone(),
                            m.manifest.info.version.clone(),
                            m.manifest.info.description.clone(),
                            m.manifest.info.authors.clone(),
                            m.patches.len(),
                        ),
                        Some(Err(e)) => {
                            problems.push(e.clone());
                            Default::default()
                        }
                        None => Default::default(),
                    };
                let status = if !item.enabled {
                    ModStatus::Disabled
                } else if load_index.is_some() {
                    ModStatus::Active
                } else {
                    ModStatus::Problem
                };
                ModEntry {
                    id: item.id.clone(),
                    name,
                    version,
                    description,
                    authors,
                    has_script: has_script(&dir),
                    patch_files,
                    enabled: item.enabled,
                    position,
                    load_index,
                    status,
                    problems,
                }
            })
            .collect()
    }
}
