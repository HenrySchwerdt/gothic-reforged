//! Sector layers of the world documents that hold characters: the initial `Enabled` flag of each
//! layer's root entity (what `gCSector::OnEnable` flips) and the entities that carry their own
//! `Enabled = false`.
//!
//! The flags are read once from the installation (a few seconds for the 80 MB main dynamic
//! document) and cached as JSON under the gitignored `.local/`, keyed by document path.
use gothic_formats::vfs::VirtualFileSystem;
use gothic_formats::world_node::WorldDocument;
use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
use std::path::Path;

const CACHE: &str = ".local/layer_flags.json";
const CACHE_VERSION: u32 = 1;

/// Flags of one document (one layer of one sector).
#[derive(Debug, Clone, Default)]
pub struct DocFlags {
    /// `Enabled` of the layer's root entity (any parentless entity enabled).
    pub root_enabled: bool,
    /// Entities of an enabled layer that are stored with `Enabled = false` (not instantiated).
    pub disabled: Vec<String>,
    pub entities: u32,
}

#[derive(Debug, Default)]
struct Cache {
    docs: BTreeMap<String, DocFlags>,
}

impl Cache {
    fn read() -> Self {
        let Some(v) = std::fs::read(CACHE)
            .ok()
            .and_then(|b| serde_json::from_slice::<serde_json::Value>(&b).ok())
        else {
            return Self::default();
        };
        if v["version"].as_u64() != Some(u64::from(CACHE_VERSION)) {
            return Self::default();
        }
        let mut docs = BTreeMap::new();
        for (path, d) in v["docs"].as_object().into_iter().flatten() {
            docs.insert(
                path.clone(),
                DocFlags {
                    root_enabled: d["root_enabled"].as_bool().unwrap_or(true),
                    disabled: d["disabled"]
                        .as_array()
                        .into_iter()
                        .flatten()
                        .filter_map(|n| n.as_str().map(str::to_owned))
                        .collect(),
                    entities: d["entities"].as_u64().unwrap_or(0) as u32,
                },
            );
        }
        Self { docs }
    }

    fn write(&self) {
        let docs: serde_json::Map<String, serde_json::Value> = self
            .docs
            .iter()
            .map(|(p, d)| {
                (
                    p.clone(),
                    serde_json::json!({
                        "root_enabled": d.root_enabled,
                        "disabled": d.disabled,
                        "entities": d.entities,
                    }),
                )
            })
            .collect();
        let v = serde_json::json!({ "version": CACHE_VERSION, "docs": docs });
        if let Ok(json) = serde_json::to_vec(&v) {
            let _ = std::fs::write(CACHE, json);
        }
    }
}

/// Flags per lower-case document path.
#[derive(Default)]
pub struct LayerFlags {
    pub docs: HashMap<String, DocFlags>,
    disabled: HashSet<(String, String)>,
}

impl LayerFlags {
    /// Is the entity stored with its own `Enabled = false`?
    pub fn entity_disabled(&self, doc: &str, name: &str) -> bool {
        self.disabled
            .contains(&(doc.to_ascii_lowercase(), name.to_owned()))
    }

    /// Initial flag of a document's layer root (unknown documents count as enabled).
    pub fn root_enabled(&self, doc: &str) -> bool {
        self.docs
            .get(&doc.to_ascii_lowercase())
            .is_none_or(|d| d.root_enabled)
    }

    fn index(&mut self) {
        self.disabled = self
            .docs
            .iter()
            .flat_map(|(p, d)| d.disabled.iter().map(|n| (p.clone(), n.clone())))
            .collect();
    }
}

/// The sector a document belongs to: its directory (`<Region>/<Sector>/<Sector>.lrentdat`).
pub fn sector_name(doc: &str) -> String {
    let norm = doc.replace('\\', "/");
    let mut parts: Vec<&str> = norm.split('/').collect();
    parts.pop();
    parts.last().map(|s| (*s).to_owned()).unwrap_or_default()
}

fn read_flags(
    vfs: &mut VirtualFileSystem,
    path: &str,
    slot: gothic_formats::vfs::Slot,
) -> DocFlags {
    let Ok(bytes) = vfs.read(slot) else {
        return DocFlags {
            root_enabled: true,
            ..Default::default()
        };
    };
    let Ok(doc) = WorldDocument::parse(&bytes) else {
        eprintln!("play: {path}: layer flags unreadable (document not parsed)");
        return DocFlags {
            root_enabled: true,
            ..Default::default()
        };
    };
    let mut is_child = vec![false; doc.entities.len()];
    for (_, c) in &doc.edges {
        if let Some(f) = is_child.get_mut(*c as usize) {
            *f = true;
        }
    }
    let roots: Vec<usize> = (0..doc.entities.len()).filter(|i| !is_child[*i]).collect();
    let root_enabled = roots.is_empty() || roots.iter().any(|i| doc.entities[*i].enabled);
    let disabled = if root_enabled {
        doc.entities
            .iter()
            .filter(|e| !e.enabled)
            .map(|e| e.name.clone())
            .collect()
    } else {
        Vec::new()
    };
    DocFlags {
        root_enabled,
        disabled,
        entities: doc.entities.len() as u32,
    }
}

/// Flags of `docs` (lower-case paths), from the cache or the installation.
pub fn load(data_dir: &Path, docs: &BTreeSet<String>) -> LayerFlags {
    let mut cache = Cache::read();
    let missing: BTreeSet<&String> = docs
        .iter()
        .filter(|d| !cache.docs.contains_key(*d))
        .collect();
    if !missing.is_empty() {
        match VirtualFileSystem::open_data_dir(data_dir) {
            Ok(mut vfs) => {
                let t = std::time::Instant::now();
                let slots: Vec<_> = vfs
                    .iter()
                    .filter(|(p, _)| missing.contains(&p.to_ascii_lowercase()))
                    .map(|(p, s)| (p.to_owned(), s))
                    .collect();
                for (path, slot) in slots {
                    let flags = read_flags(&mut vfs, &path, slot);
                    cache.docs.insert(path.to_ascii_lowercase(), flags);
                }
                eprintln!(
                    "play: layer flags of {} documents read in {} ms",
                    missing.len(),
                    t.elapsed().as_millis()
                );
                cache.write();
            }
            Err(e) => eprintln!("play: cannot open the archives for layer flags: {e:#}"),
        }
    }
    let mut flags = LayerFlags {
        docs: cache.docs.into_iter().collect(),
        disabled: HashSet::new(),
    };
    flags.index();
    flags
}
