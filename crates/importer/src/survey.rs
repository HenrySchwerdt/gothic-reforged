//! Whole-installation parse surveys (world documents and meshes). Read-only;
//! reports contain only counts, names and error categories, never payloads.
use anyhow::Result;
use gothic_formats::{
    vfs::{Slot, VirtualFileSystem},
    world_node::WorldDocument,
    xcmsh::Xcmsh,
    xlmsh::Xlmsh,
};
use serde::Serialize;
use std::collections::{BTreeMap, BTreeSet, HashMap};

#[derive(Debug, Default, Serialize)]
pub struct ErrorClass {
    pub count: usize,
    pub examples: Vec<String>,
}

fn record_error(map: &mut BTreeMap<String, ErrorClass>, path: &str, error: &anyhow::Error) {
    let mut category = format!("{error:#}");
    category.truncate(120);
    let class = map.entry(category).or_default();
    class.count += 1;
    if class.examples.len() < 40 {
        class.examples.push(path.to_owned());
    }
}

/// Lowercase basename -> slot for every visible entry whose archive name starts with `prefix`.
pub fn basename_index(vfs: &VirtualFileSystem, prefix: &str) -> HashMap<String, Slot> {
    let mut map: HashMap<String, (String, Slot)> = HashMap::new();
    for (path, slot) in vfs.iter() {
        if !vfs.archive_name(slot).starts_with(prefix) {
            continue;
        }
        let base = path.rsplit('/').next().unwrap_or(path).to_owned();
        match map.get(&base) {
            Some((existing, _)) if existing.as_str() <= path => {}
            _ => {
                map.insert(base, (path.to_owned(), slot));
            }
        }
    }
    map.into_iter().map(|(k, (_, s))| (k, s)).collect()
}

pub fn is_world_document(path: &str) -> bool {
    path.ends_with(".node") || path.ends_with(".lrentdat")
}

#[derive(Debug, Default, Serialize)]
pub struct VfsSummary {
    pub archives_in_mount_order: Vec<String>,
    pub skipped_archives: Vec<(String, String)>,
    pub visible_entries: usize,
    pub overridden_entries: usize,
    pub ignored_payloadless: Vec<gothic_formats::vfs::IgnoredRecord>,
}

pub fn vfs_summary(vfs: &VirtualFileSystem) -> VfsSummary {
    VfsSummary {
        archives_in_mount_order: vfs.archive_names().to_vec(),
        skipped_archives: vfs.skipped.clone(),
        visible_entries: vfs.len(),
        overridden_entries: vfs.overridden,
        ignored_payloadless: vfs.ignored_payloadless.clone(),
    }
}

#[derive(Debug, Default, Serialize)]
pub struct WorldSurvey {
    pub vfs: VfsSummary,
    pub documents_total: usize,
    pub documents_ok: usize,
    pub documents_failed: usize,
    pub documents_by_kind: BTreeMap<String, usize>,
    pub non_genome_stubs: usize,
    pub errors: BTreeMap<String, ErrorClass>,
    pub entities: usize,
    pub entities_with_mesh: usize,
    pub asset_reference_occurrences: usize,
    pub asset_references_by_extension: BTreeMap<String, usize>,
    pub unique_mesh_references: usize,
    pub mesh_references_resolved: usize,
    pub mesh_references_unresolved: usize,
    /// Unresolved references that exist as a basename in some other archive.
    pub unresolved_found_elsewhere: usize,
    pub unresolved_examples: Vec<String>,
    /// LOD levels named by referenced .xlmsh files, and how many lack an .xcmsh.
    pub lod_levels_total: usize,
    pub lod_levels_unresolved: usize,
    pub lod_unresolved_examples: Vec<String>,
    pub referenced_meshes_parse_ok: usize,
    pub referenced_meshes_parse_failed: usize,
    pub referenced_mesh_errors: BTreeMap<String, ErrorClass>,
}

pub fn world_survey(vfs: &mut VirtualFileSystem) -> Result<WorldSurvey> {
    let mut docs: Vec<(String, Slot)> = vfs
        .iter()
        .filter(|(p, _)| is_world_document(p))
        .map(|(p, s)| (p.to_owned(), s))
        .collect();
    docs.sort();
    let mut report = WorldSurvey {
        vfs: vfs_summary(vfs),
        documents_total: docs.len(),
        ..Default::default()
    };
    let mut mesh_refs: BTreeSet<String> = BTreeSet::new();
    for (path, slot) in docs {
        let result = vfs.read(slot).and_then(|bytes| {
            if bytes.len() < 64 && !bytes.starts_with(b"GENOMFLE") {
                report.non_genome_stubs += 1;
            }
            WorldDocument::parse(&bytes)
        });
        match result {
            Ok(doc) => {
                report.documents_ok += 1;
                *report
                    .documents_by_kind
                    .entry(format!("{:?}", doc.kind))
                    .or_default() += 1;
                report.entities += doc.entities.len();
                for entity in &doc.entities {
                    let mut has_mesh = false;
                    for asset in &entity.asset_references {
                        report.asset_reference_occurrences += 1;
                        let ext = asset.rsplit('.').next().unwrap_or("").to_ascii_lowercase();
                        *report
                            .asset_references_by_extension
                            .entry(ext.clone())
                            .or_default() += 1;
                        if ext == "xcmsh" || ext == "xlmsh" {
                            has_mesh = true;
                            mesh_refs.insert(asset.to_ascii_lowercase());
                        }
                    }
                    report.entities_with_mesh += usize::from(has_mesh);
                }
            }
            Err(error) => {
                report.documents_failed += 1;
                record_error(&mut report.errors, &path, &error);
            }
        }
    }
    let meshes = basename_index(vfs, "_compiledmesh");
    report.unique_mesh_references = mesh_refs.len();
    for name in &mesh_refs {
        let base = name.rsplit(['/', '\\']).next().unwrap_or(name);
        match meshes.get(base) {
            Some(&slot) => {
                report.mesh_references_resolved += 1;
                let path = vfs.entry_path(slot).to_owned();
                match parse_mesh(vfs, slot, &path) {
                    Ok(()) => {
                        report.referenced_meshes_parse_ok += 1;
                        if path.to_ascii_lowercase().ends_with(".xlmsh") {
                            let lod = vfs.read(slot).and_then(|b| Xlmsh::parse(&b));
                            for level in lod.map(|x| x.levels).unwrap_or_default() {
                                report.lod_levels_total += 1;
                                let key = level.mesh.to_ascii_lowercase();
                                let base = key.rsplit(['/', '\\']).next().unwrap_or(&key);
                                if !meshes.contains_key(base) {
                                    report.lod_levels_unresolved += 1;
                                    if report.lod_unresolved_examples.len() < 25 {
                                        report.lod_unresolved_examples.push(level.mesh);
                                    }
                                }
                            }
                        }
                    }
                    Err(error) => {
                        report.referenced_meshes_parse_failed += 1;
                        record_error(&mut report.referenced_mesh_errors, &path, &error);
                    }
                }
            }
            None => {
                report.mesh_references_unresolved += 1;
                report.unresolved_found_elsewhere +=
                    usize::from(!vfs.paths_for_basename(base).is_empty());
                if report.unresolved_examples.len() < 50 {
                    report.unresolved_examples.push(name.clone());
                }
            }
        }
    }
    Ok(report)
}

fn parse_mesh(vfs: &mut VirtualFileSystem, slot: Slot, path: &str) -> Result<()> {
    let bytes = vfs.read(slot)?;
    if path.to_ascii_lowercase().ends_with(".xlmsh") {
        Xlmsh::parse(&bytes)?;
    } else {
        Xcmsh::parse(&bytes)?;
    }
    Ok(())
}

#[derive(Debug, Default, Serialize)]
pub struct MeshSurvey {
    pub total: usize,
    pub ok: usize,
    pub failed: usize,
    pub by_extension: BTreeMap<String, (usize, usize)>,
    pub errors: BTreeMap<String, ErrorClass>,
}

pub fn mesh_survey(vfs: &mut VirtualFileSystem) -> Result<MeshSurvey> {
    let mut items: Vec<(String, Slot)> = vfs
        .iter()
        .filter(|(p, s)| {
            (p.ends_with(".xcmsh") || p.ends_with(".xlmsh"))
                && vfs.archive_name(*s).starts_with("_compiledmesh")
        })
        .map(|(p, s)| (p.to_owned(), s))
        .collect();
    items.sort();
    let mut report = MeshSurvey {
        total: items.len(),
        ..Default::default()
    };
    for (path, slot) in items {
        let ext = path.rsplit('.').next().unwrap_or("").to_owned();
        let counts = report.by_extension.entry(ext).or_default();
        match parse_mesh(vfs, slot, &path) {
            Ok(()) => {
                report.ok += 1;
                counts.0 += 1;
            }
            Err(error) => {
                report.failed += 1;
                counts.1 += 1;
                record_error(&mut report.errors, &path, &error);
            }
        }
    }
    Ok(report)
}
