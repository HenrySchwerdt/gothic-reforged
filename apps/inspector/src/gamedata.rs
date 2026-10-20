//! Game-data survey subcommands (`strings`, `quest-survey`, `info-survey`, ...).
//! Each parses every member of an archive and reports exact coverage; summaries
//! go to stdout and optionally `--out FILE` (use a path under the gitignored `.local/`).
use anyhow::{Context, Result};
use gothic_formats::{PakArchive, info::Info, quest::Quest, stringtable::StringTable};
use serde_json::{Value, json};
use std::{
    collections::{BTreeMap, BTreeSet},
    ffi::OsString,
    path::PathBuf,
};

pub struct Opts {
    pub positional: Vec<String>,
    pub flags: BTreeMap<String, String>,
}

pub fn parse_opts(args: &[OsString]) -> Result<Opts> {
    let mut positional = Vec::new();
    let mut flags = BTreeMap::new();
    let mut it = args.iter().skip(1);
    while let Some(a) = it.next() {
        let a = a.to_str().context("arguments must be Unicode")?;
        if let Some(name) = a.strip_prefix("--") {
            if matches!(name, "quiet" | "full") {
                flags.insert(name.to_string(), String::new());
                continue;
            }
            let v = it
                .next()
                .with_context(|| format!("--{name} needs a value"))?;
            flags.insert(name.to_string(), v.to_str().context("Unicode")?.to_string());
        } else {
            positional.push(a.to_string());
        }
    }
    Ok(Opts { positional, flags })
}

fn emit(value: &Value, opts: &Opts) -> Result<()> {
    let text = serde_json::to_string_pretty(value)?;
    if let Some(out) = opts.flags.get("out") {
        std::fs::write(PathBuf::from(out), &text)?;
        eprintln!("wrote {out}");
    }
    if opts.flags.contains_key("quiet") {
        return Ok(());
    }
    println!("{text}");
    Ok(())
}

fn histogram<S: ToString>(items: impl IntoIterator<Item = S>) -> BTreeMap<String, usize> {
    let mut m = BTreeMap::new();
    for i in items {
        *m.entry(i.to_string()).or_insert(0) += 1;
    }
    m
}

/// Parse every non-payloadless member passing `filter`; returns (path, bytes).
fn members(archive: &str, ext: &str) -> Result<Vec<(String, Vec<u8>)>> {
    let mut pak = PakArchive::open(archive)?;
    let mut out = Vec::new();
    for i in 0..pak.entries.len() {
        let e = &pak.entries[i];
        if !e.path.to_ascii_lowercase().ends_with(ext) || e.payloadless() {
            continue;
        }
        let path = e.path.clone();
        out.push((path, pak.read_payload(i)?));
    }
    Ok(out)
}

fn load_strings(opts: &Opts) -> Result<Option<StringTable>> {
    let Some(path) = opts.flags.get("strings") else {
        return Ok(None);
    };
    let mut pak = PakArchive::open(path)?;
    let i = pak
        .entries
        .iter()
        .position(|e| e.path.eq_ignore_ascii_case("stringtable.ini"))
        .context("stringtable.ini not in archive")?;
    Ok(Some(StringTable::parse(&pak.read_payload(i)?)?))
}

pub fn dispatch(command: &str, args: &[OsString]) -> Result<bool> {
    let opts = parse_opts(args)?;
    match command {
        "strings" => strings(&opts)?,
        "quest-survey" => quest_survey(&opts)?,
        "info-survey" => info_survey(&opts)?,
        "terrain-survey" => terrain_survey(&opts)?,
        "template-survey" => template_survey(&opts)?,
        _ => return Ok(false),
    }
    Ok(true)
}

fn strings(opts: &Opts) -> Result<()> {
    let archive = opts
        .positional
        .first()
        .context("strings ARCHIVE [--key K] [--lang L] [--prefix P] [--out F]")?;
    let mut pak = PakArchive::open(archive)?;
    let i = pak
        .entries
        .iter()
        .position(|e| e.path.eq_ignore_ascii_case("stringtable.ini"))
        .context("stringtable.ini not in archive")?;
    let table = StringTable::parse(&pak.read_payload(i)?)?;
    let lang = opts.flags.get("lang").map_or("English", String::as_str);
    let li = table
        .language_index(lang)
        .with_context(|| format!("unknown language {lang}"))?;
    if let Some(key) = opts.flags.get("key") {
        let e = table.get_loose(key).context("key not found")?;
        return emit(
            &json!({"key": e.key, "language": lang, "text": e.texts[li], "all": e.texts}),
            opts,
        );
    }
    if let Some(prefix) = opts.flags.get("prefix") {
        let list: Vec<_> = table
            .entries
            .iter()
            .filter(|e| e.key.starts_with(prefix.as_str()))
            .take(200)
            .map(|e| json!({"key": e.key, "text": e.texts[li]}))
            .collect();
        return emit(&json!(list), opts);
    }
    let prefixes = histogram(
        table
            .entries
            .iter()
            .map(|e| e.key.split('_').next().unwrap_or("")),
    );
    let empty: Vec<usize> = (0..table.languages.len())
        .map(|l| {
            table
                .entries
                .iter()
                .filter(|e| e.texts[l].is_empty())
                .count()
        })
        .collect();
    emit(
        &json!({
            "entries": table.entries.len(),
            "languages": table.languages,
            "current_language": table.current_language,
            "current_revision": table.current_revision,
            "irregular_keys": table.irregular_keys,
            "duplicate_keys": table.duplicate_keys,
            "empty_text_per_language": empty,
            "key_prefix_histogram": prefixes,
        }),
        opts,
    )
}

fn quest_survey(opts: &Opts) -> Result<()> {
    let archive = opts
        .positional
        .first()
        .context("quest-survey QUESTS_ARCHIVE [--strings STRINGS_ARCHIVE] [--out F]")?;
    let strings = load_strings(opts)?;
    let files = members(archive, ".quest")?;
    let mut failures: Vec<Value> = Vec::new();
    let mut quests = Vec::new();
    for (path, bytes) in &files {
        match Quest::parse(bytes) {
            Ok(q) => quests.push((path.clone(), q)),
            Err(e) => failures.push(json!({"path": path, "error": format!("{e:#}")})),
        }
    }
    let mut anomalies: BTreeMap<String, Vec<String>> = BTreeMap::new();
    for (path, q) in &quests {
        for a in &q.anomalies {
            anomalies
                .entry(a.kind.to_string())
                .or_default()
                .push(format!("{path}: {}", a.detail));
        }
    }
    let names: BTreeSet<_> = quests
        .iter()
        .map(|(_, q)| q.name.to_ascii_lowercase())
        .collect();
    let dup_names = quests.len() - names.len();
    let mut missing_topics = Vec::new();
    let mut missing_desc = Vec::new();
    let mut dangling_prereqs = Vec::new();
    for (_, q) in &quests {
        if let Some(s) = &strings {
            if s.get(&q.log_topic).is_none() {
                missing_topics.push(q.log_topic.clone());
            }
            if !q.quest_description.is_empty() && s.get(&q.quest_description).is_none() {
                missing_desc.push(q.quest_description.clone());
            }
        }
        for f in &q.finished_quests {
            if !names.contains(&f.to_ascii_lowercase()) {
                dangling_prereqs.push(format!("{} -> {f}", q.name));
            }
        }
    }
    let extra_keys = histogram(
        quests
            .iter()
            .flat_map(|(_, q)| q.extra.keys().map(String::as_str)),
    );
    let summary = json!({
        "archive": archive,
        "quest_files": files.len(),
        "parsed": quests.len(),
        "failed": failures.len(),
        "failures": failures,
        "duplicate_names_case_insensitive": dup_names,
        "anomaly_counts": anomalies.iter().map(|(k, v)| (k.clone(), v.len())).collect::<BTreeMap<_, _>>(),
        "anomaly_examples": anomalies.iter().map(|(k, v)| (k.clone(), v.iter().take(5).collect::<Vec<_>>())).collect::<BTreeMap<_, _>>(),
        "type_histogram": histogram(quests.iter().map(|(_, q)| q.quest_type)),
        "unknown_field_histogram": extra_keys,
        "total_experience_points": quests.iter().map(|(_, q)| u64::from(q.experience_points)).sum::<u64>(),
        "string_table_checked": strings.is_some(),
        "log_topics_missing_in_string_table": missing_topics,
        "quest_descriptions_missing_in_string_table": missing_desc,
        "finished_quests_not_defined": dangling_prereqs,
        "quests": quests.iter().map(|(p, q)| json!({"path": p, "quest": q})).collect::<Vec<_>>(),
    });
    if opts.flags.contains_key("full") || opts.flags.contains_key("out") {
        emit(&summary, opts)?;
    } else {
        let mut brief = summary.clone();
        brief.as_object_mut().map(|o| o.remove("quests"));
        emit(&brief, opts)?;
    }
    Ok(())
}

fn info_survey(opts: &Opts) -> Result<()> {
    let archive = opts.positional.first().context(
        "info-survey INFOS_ARCHIVE [--strings STRINGS_ARCHIVE] [--quests QUESTS_ARCHIVE] [--out F]",
    )?;
    let strings = load_strings(opts)?;
    let known_quests: Option<BTreeSet<String>> = match opts.flags.get("quests") {
        Some(q) => Some(
            members(q, ".quest")?
                .iter()
                .filter_map(|(_, b)| Quest::parse(b).ok())
                .map(|q| q.name.to_ascii_lowercase())
                .collect(),
        ),
        None => None,
    };
    let files = members(archive, ".info")?;
    let mut failures = Vec::new();
    let mut infos = Vec::new();
    for (path, bytes) in &files {
        match Info::parse(bytes) {
            Ok(i) => infos.push((path.clone(), i)),
            Err(e) => failures.push(json!({"path": path, "error": format!("{e:#}")})),
        }
    }
    let mut anomalies: BTreeMap<String, Vec<String>> = BTreeMap::new();
    for (path, i) in &infos {
        for a in &i.anomalies {
            anomalies
                .entry(a.kind.to_string())
                .or_default()
                .push(format!("{path}: {}", a.detail));
        }
    }
    let names: BTreeSet<_> = infos
        .iter()
        .map(|(_, i)| i.name.to_ascii_lowercase())
        .collect();
    let dup_names = infos.len() - names.len();
    let commands = histogram(
        infos
            .iter()
            .flat_map(|(_, i)| i.script.iter().map(|s| s.command.as_str())),
    );
    // Event graph: events set vs required.
    let mut set_events = BTreeSet::new();
    for (_, i) in &infos {
        for s in &i.script {
            if s.command == "SetGameEvent" {
                set_events.insert(s.id1.to_ascii_lowercase());
            }
        }
    }
    let mut required = BTreeSet::new();
    for (_, i) in &infos {
        for e in i.cond_player_knows.iter().chain(&i.cond_player_knows_not) {
            required.insert(e.to_ascii_lowercase());
        }
    }
    let never_set: Vec<_> = required.difference(&set_events).take(50).cloned().collect();
    let mut text_refs = 0usize;
    let mut text_missing: Vec<String> = Vec::new();
    if let Some(s) = &strings {
        for (_, i) in &infos {
            for step in &i.script {
                if !step.text.is_empty() {
                    text_refs += 1;
                    if s.get_loose(&step.text).is_none() {
                        text_missing.push(format!("{}: {}", i.name, step.text));
                    }
                }
            }
        }
    }
    let mut quest_missing = BTreeSet::new();
    let mut quest_refs = 0usize;
    if let Some(known) = &known_quests {
        for (_, i) in &infos {
            if !i.quest.is_empty() {
                quest_refs += 1;
                if !known.contains(&i.quest.to_ascii_lowercase()) {
                    quest_missing.insert(i.quest.clone());
                }
            }
        }
    }
    let parents_missing: Vec<_> = infos
        .iter()
        .filter(|(_, i)| !i.parent.is_empty() && !names.contains(&i.parent.to_ascii_lowercase()))
        .map(|(_, i)| format!("{} -> {}", i.name, i.parent))
        .take(50)
        .collect();
    let owners: BTreeSet<_> = infos.iter().map(|(_, i)| i.owner.as_str()).collect();
    let summary = json!({
        "archive": archive,
        "info_files": files.len(),
        "parsed": infos.len(),
        "failed": failures.len(),
        "failures": failures,
        "duplicate_names_case_insensitive": dup_names,
        "anomaly_counts": anomalies.iter().map(|(k, v)| (k.clone(), v.len())).collect::<BTreeMap<_, _>>(),
        "anomaly_examples": anomalies.iter().map(|(k, v)| (k.clone(), v.iter().take(5).collect::<Vec<_>>())).collect::<BTreeMap<_, _>>(),
        "distinct_owners": owners.len(),
        "condition_type_histogram": histogram(infos.iter().map(|(_, i)| i.condition_type)),
        "info_type_histogram": histogram(infos.iter().map(|(_, i)| i.info_type)),
        "script_command_histogram": commands,
        "unknown_field_histogram": histogram(infos.iter().flat_map(|(_, i)| i.extra.keys().map(String::as_str))),
        "game_events_set": set_events.len(),
        "game_events_required": required.len(),
        "required_events_never_set_sample": never_set,
        "string_table_checked": strings.is_some(),
        "text_references": text_refs,
        "text_references_missing_in_string_table": text_missing.len(),
        "text_missing_examples": text_missing.iter().take(20).collect::<Vec<_>>(),
        "quest_table_checked": known_quests.is_some(),
        "quest_references": quest_refs,
        "quests_referenced_but_undefined": quest_missing,
        "parent_references_unresolved_sample": parents_missing,
    });
    emit(&summary, opts)
}

type Slot = (usize, usize);

/// Case-insensitive basename -> slot over several archives; later archives override earlier ones.
fn basename_index(paks: &[PakArchive]) -> BTreeMap<String, Slot> {
    let mut m = BTreeMap::new();
    for (a, pak) in paks.iter().enumerate() {
        for (i, e) in pak.entries.iter().enumerate() {
            if e.payloadless() {
                continue;
            }
            let base = e
                .path
                .rsplit('/')
                .next()
                .unwrap_or(&e.path)
                .to_ascii_lowercase();
            m.insert(base, (a, i));
        }
    }
    m
}

fn terrain_survey(opts: &Opts) -> Result<()> {
    use gothic_formats::{
        terrain::{TerrainCell, TerrainPatch, collect_cells},
        world_node::WorldDocument,
        xcmsh::Xcmsh,
    };
    let usage = "terrain-survey PROJECTS_COMPILED.pak MESH_ARCHIVE[,MESH_OVERLAY...] [--box X0,X1,Z0,Z1] [--obj F] [--json F] [--bounds yes] [--out F]";
    let projects = opts.positional.first().context(usage)?;
    let mesh_list = opts.positional.get(1).context(usage)?;
    let mut world = PakArchive::open(projects)?;
    let mut meshes = Vec::new();
    for m in mesh_list.split(',') {
        meshes.push(PakArchive::open(m)?);
    }
    let mesh_index = basename_index(&meshes);
    let bbox: Option<[f32; 4]> = match opts.flags.get("box") {
        Some(b) => {
            let v: Vec<f32> = b
                .split(',')
                .map(str::parse)
                .collect::<std::result::Result<_, _>>()?;
            Some(
                v.try_into()
                    .map_err(|_| anyhow::anyhow!("--box needs 4 numbers"))?,
            )
        }
        None => None,
    };
    let want_bounds = opts.flags.contains_key("bounds");
    let mut docs = 0usize;
    let mut doc_failures = Vec::new();
    let mut cells: Vec<(String, TerrainCell)> = Vec::new();
    for i in 0..world.entries.len() {
        let e = &world.entries[i];
        if !e.path.ends_with("_CStat.node") || e.payloadless() {
            continue;
        }
        let path = e.path.clone();
        let bytes = world.read_payload(i)?;
        docs += 1;
        match WorldDocument::parse(&bytes) {
            Ok(d) => cells.extend(collect_cells(&d).into_iter().map(|c| (path.clone(), c))),
            Err(err) => doc_failures.push(json!({"path": path, "error": format!("{err:#}")})),
        }
    }
    let mut by_region: BTreeMap<String, usize> = BTreeMap::new();
    let mut seen: BTreeMap<(String, u32), usize> = BTreeMap::new();
    let mut missing_mesh = Vec::new();
    let mut missing_nav = 0usize;
    let mut with_nav = 0usize;
    let mut rotated = 0usize;
    for (_, c) in &cells {
        *by_region.entry(c.region.clone()).or_default() += 1;
        *seen.entry((c.region.clone(), c.cell)).or_default() += 1;
        if !mesh_index.contains_key(&c.mesh.to_ascii_lowercase()) {
            missing_mesh.push(c.mesh.clone());
        }
        if let Some(n) = &c.nav_mesh {
            with_nav += 1;
            if !mesh_index.contains_key(&n.to_ascii_lowercase()) {
                missing_nav += 1;
            }
        }
        let m = c.world_matrix;
        let ident =
            (0..3).all(|r| (0..3).all(|k| (m[r][k] - if r == k { 1.0 } else { 0.0 }).abs() < 1e-4));
        if !ident {
            rotated += 1;
        }
    }
    let duplicates: Vec<_> = seen
        .iter()
        .filter(|(_, n)| **n > 1)
        .map(|(k, n)| format!("{}:{} x{n}", k.0, k.1))
        .collect();
    // Parse meshes (all with --bounds, otherwise only those whose translation is in the box).
    let mut patch = TerrainPatch::default();
    let mut parse_fail = Vec::new();
    let mut parsed = 0usize;
    let mut tri_total = 0usize;
    let mut cell_bounds: Vec<Value> = Vec::new();
    let (mut gmin, mut gmax) = ([f32::MAX; 3], [f32::MIN; 3]);
    for (_, c) in &cells {
        let t = c.world_matrix[3];
        let in_box = bbox.is_some();
        if !want_bounds && !in_box {
            continue;
        }
        let Some(&(a, i)) = mesh_index.get(&c.mesh.to_ascii_lowercase()) else {
            continue;
        };
        let bytes = meshes[a].read_payload(i)?;
        match Xcmsh::parse(&bytes) {
            Ok(mesh) => {
                parsed += 1;
                let mut p = TerrainPatch::default();
                p.append(c, &mesh)?;
                tri_total += p.indices.len() / 3;
                if let Some((lo, hi)) = p.bounds() {
                    for k in 0..3 {
                        gmin[k] = gmin[k].min(lo[k]);
                        gmax[k] = gmax[k].max(hi[k]);
                    }
                    cell_bounds.push(json!({"region": c.region, "cell": c.cell, "translation": [t[0], t[1], t[2]], "min": lo, "max": hi, "vertices": p.positions.len(), "submeshes": mesh.submeshes.len(), "name": c.entity_name}));
                    if let Some(b) = bbox
                        && hi[0] >= b[0]
                        && lo[0] <= b[1]
                        && hi[2] >= b[2]
                        && lo[2] <= b[3]
                    {
                        patch.append_in_box(c, &mesh, b)?;
                    }
                }
            }
            Err(e) => parse_fail.push(json!({"mesh": c.mesh, "error": format!("{e:#}")})),
        }
    }
    if let Some(obj) = opts.flags.get("obj") {
        std::fs::write(obj, patch.to_obj())?;
        eprintln!(
            "wrote {obj}: {} vertices, {} triangles",
            patch.positions.len(),
            patch.indices.len() / 3
        );
    }
    if let Some(f) = opts.flags.get("json") {
        std::fs::write(f, serde_json::to_string(&patch)?)?;
        eprintln!("wrote {f}");
    }
    let summary = json!({
        "world_documents": docs,
        "world_document_failures": doc_failures,
        "landscape_cell_placements": cells.len(),
        "by_region": by_region,
        "duplicate_placements": duplicates,
        "placements_with_nav_mesh": with_nav,
        "nav_meshes_missing_in_archives": missing_nav,
        "render_meshes_missing_in_archives": missing_mesh,
        "placements_with_non_identity_rotation_or_scale": rotated,
        "meshes_parsed": parsed,
        "mesh_parse_failures": parse_fail,
        "triangles_parsed": tri_total,
        "world_bounds_of_parsed": if parsed > 0 { json!({"min": gmin, "max": gmax}) } else { Value::Null },
        "export": { "box_xz": bbox, "cells": patch.cells, "vertices": patch.positions.len(), "triangles": patch.indices.len() / 3, "bounds": patch.bounds().map(|(a, b)| json!({"min": a, "max": b})) },
        "cell_bounds": cell_bounds,
    });
    emit(&summary, opts)
}

fn template_survey(opts: &Opts) -> Result<()> {
    use gothic_formats::template::TemplateSummary;
    let usage = "template-survey TEMPLATES_ARCHIVE [--quests Q.pak] [--infos I.pak] [--out F]";
    let archive = opts.positional.first().context(usage)?;
    let files = members(archive, ".tple")?;
    let mut failures = Vec::new();
    let mut parsed = Vec::new();
    for (path, bytes) in &files {
        match TemplateSummary::parse(bytes) {
            Ok(t) => parsed.push((path.clone(), t)),
            Err(e) => failures.push(json!({"path": path, "error": format!("{e:#}")})),
        }
    }
    let variants = histogram(parsed.iter().map(|(_, t)| format!("{:?}", t.variant)));
    let classes = histogram(
        parsed
            .iter()
            .flat_map(|(_, t)| t.classes.iter().map(String::as_str)),
    );
    let mut top_classes: Vec<_> = classes.iter().collect();
    top_classes.sort_by_key(|(_, n)| std::cmp::Reverse(**n));
    let names: BTreeSet<String> = parsed
        .iter()
        .map(|(_, t)| t.entity_name.to_ascii_lowercase())
        .collect();
    let mut by_name: BTreeMap<String, Vec<&str>> = BTreeMap::new();
    for (p, t) in &parsed {
        by_name
            .entry(t.entity_name.to_ascii_lowercase())
            .or_default()
            .push(p);
    }
    let dup_names: Vec<_> = by_name
        .iter()
        .filter(|(_, v)| v.len() > 1)
        .map(|(k, v)| format!("{k} x{}", v.len()))
        .collect();
    let live: Vec<_> = parsed
        .iter()
        .filter(|(p, _)| !p.starts_with("_deleted/"))
        .collect();
    let mismatched_file_name = parsed
        .iter()
        .filter(|(p, t)| {
            t.recorded_file_name
                .as_deref()
                .is_some_and(|f| !p.to_ascii_lowercase().ends_with(&f.to_ascii_lowercase()))
        })
        .count();
    // Cross references from game logic.
    let mut referenced: BTreeSet<String> = BTreeSet::new();
    if let Some(q) = opts.flags.get("quests") {
        for (_, b) in members(q, ".quest")? {
            if let Ok(q) = Quest::parse(&b) {
                referenced.extend(
                    q.delivery_entities
                        .iter()
                        .filter(|e| e.starts_with("It_"))
                        .cloned(),
                );
            }
        }
    }
    if let Some(i) = opts.flags.get("infos") {
        for (_, b) in members(i, ".info")? {
            if let Ok(i) = Info::parse(&b) {
                referenced.extend(i.cond_items.iter().cloned());
                for s in &i.script {
                    if matches!(s.command.as_str(), "Give" | "give" | "GiveAll")
                        && !s.id1.is_empty()
                    {
                        referenced.insert(s.id1.clone());
                    }
                }
            }
        }
    }
    let unresolved: Vec<_> = referenced
        .iter()
        .filter(|r| !names.contains(&r.to_ascii_lowercase()))
        .take(40)
        .cloned()
        .collect();
    let unresolved_count = referenced
        .iter()
        .filter(|r| !names.contains(&r.to_ascii_lowercase()))
        .count();
    let summary = json!({
        "archive": archive,
        "tple_files": files.len(),
        "parsed": parsed.len(),
        "failed": failures.len(),
        "failures": failures,
        "variants": variants,
        "recorded_file_name_mismatches": mismatched_file_name,
        "distinct_entity_names": names.len(),
        "duplicate_entity_names_sample": dup_names.iter().take(20).collect::<Vec<_>>(),
        "duplicate_entity_name_count": dup_names.len(),
        "non_deleted_templates": live.len(),
        "distinct_classes": classes.len(),
        "top_classes": top_classes.iter().take(40).collect::<Vec<_>>(),
        "item_references_from_quests_and_infos": referenced.len(),
        "item_references_unresolved": unresolved_count,
        "item_references_unresolved_sample": unresolved,
    });
    emit(&summary, opts)
}
