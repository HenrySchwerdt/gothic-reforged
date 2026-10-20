//! Property-stream subcommands: `prop-survey`, `item-dump`, `npc-dump`, `tple-dump`.
//! Reports and exports go to `--out` paths (use the gitignored `.local/`).
use crate::gamedata::{Opts, parse_opts};
use anyhow::{Context, Result};
use gothic_formats::{
    props::records::{ItemTemplate, NpcEntity, NpcTemplate, SkillTemplate, TemplateCatalog},
    propstream::DecodeStats,
    template::TemplateDocument,
    vfs::VirtualFileSystem,
    world_node::WorldDocument,
};
use serde_json::{Value, json};
use std::{collections::BTreeMap, ffi::OsString, path::Path};

pub fn dispatch(command: &str, args: &[OsString]) -> Result<bool> {
    let opts = parse_opts(args)?;
    match command {
        "prop-survey" => prop_survey(&opts)?,
        "tple-dump" => tple_dump(&opts)?,
        "world-props" => world_props(&opts)?,
        "item-dump" => item_dump(&opts)?,
        "npc-dump" => npc_dump(&opts)?,
        "skill-dump" => skill_dump(&opts)?,
        _ => return Ok(false),
    }
    Ok(true)
}

fn emit(value: &Value, opts: &Opts) -> Result<()> {
    let text = serde_json::to_string_pretty(value)?;
    if let Some(out) = opts.flags.get("out") {
        std::fs::write(out, &text)?;
        eprintln!("wrote {out}");
    }
    if !opts.flags.contains_key("quiet") {
        println!("{text}");
    }
    Ok(())
}

fn top<K: Clone + Ord>(map: &BTreeMap<K, u64>, n: usize) -> Vec<(K, u64)> {
    let mut v: Vec<_> = map.iter().map(|(k, v)| (k.clone(), *v)).collect();
    v.sort_by_key(|(_, c)| std::cmp::Reverse(*c));
    v.truncate(n);
    v
}

fn stats_json(stats: &DecodeStats) -> Value {
    let mut types: Vec<_> = stats.by_type.iter().collect();
    types.sort_by_key(|(_, (n, _))| std::cmp::Reverse(*n));
    let untyped_types: Vec<_> = types
        .iter()
        .filter(|(_, (n, t))| n != t)
        .map(|(k, (n, t))| json!({"type": k, "instances": n, "typed": t}))
        .collect();
    let pct = |a: u64, b: u64| {
        if b == 0 {
            100.0
        } else {
            a as f64 * 100.0 / b as f64
        }
    };
    let tails: Vec<_> = stats
        .tails_by_class
        .iter()
        .filter(|(_, (n, d))| n != d)
        .map(|(k, (n, d))| json!({"class": k, "sets": n, "decoded": d}))
        .collect();
    json!({
        "property_instances": stats.properties,
        "typed_instances": stats.typed,
        "typed_percent": pct(stats.typed, stats.properties),
        "distinct_types": stats.by_type.len(),
        "types_not_fully_typed": untyped_types,
        "opaque_shapes_top": top(&stats.opaque_shapes, 60),
        "tails_total": stats.tails_total,
        "tails_with_decoder_or_empty": stats.tails_decoded,
        "tails_not_decoded_by_class": tails,
        "tail_errors": top(&stats.tail_errors, 40),
        "by_type": stats.by_type.iter().map(|(k, (n, t))| (k.clone(), json!([n, t]))).collect::<BTreeMap<_, _>>(),
    })
}

fn prop_survey(opts: &Opts) -> Result<()> {
    let usage = "prop-survey DATA_DIR [--full] [--out F] [--quiet]";
    let dir = opts.positional.first().context(usage)?;
    let mut vfs = VirtualFileSystem::open_data_dir(Path::new(dir))?;
    let mut tstats = DecodeStats::default();
    let mut ok = 0usize;
    let mut total = 0usize;
    let mut errors: BTreeMap<String, u64> = BTreeMap::new();
    let mut error_examples: BTreeMap<String, Vec<String>> = BTreeMap::new();
    let mut tail_examples: Vec<String> = Vec::new();
    let mut parsed_docs: Vec<TemplateDocument> = Vec::new();
    let mut catalog = TemplateCatalog::default();
    let slots: Vec<_> = vfs
        .iter()
        .filter(|(p, _)| p.to_ascii_lowercase().ends_with(".tple"))
        .map(|(p, s)| (p.to_owned(), s))
        .collect();
    for (path, slot) in slots {
        total += 1;
        let bytes = vfs.read(slot)?;
        let before: u64 = tstats.tail_errors.values().sum();
        let result = TemplateDocument::parse(&bytes, &mut tstats);
        if tstats.tail_errors.values().sum::<u64>() > before && tail_examples.len() < 40 {
            tail_examples.push(path.clone());
        }
        match result {
            Ok(doc) => {
                ok += 1;
                catalog.insert(&path, &doc);
                parsed_docs.push(doc);
            }
            Err(e) => {
                let key = format!("{e:#}");
                let key = key.chars().take(100).collect::<String>();
                *errors.entry(key.clone()).or_default() += 1;
                let ex = error_examples.entry(key).or_default();
                if ex.len() < 5 {
                    ex.push(path);
                }
            }
        }
    }
    // Template references (`eCTemplateEntityProxy`, inventory stacks) that resolve in the catalog.
    let (mut refs_total, mut refs_resolved) = (0u64, 0u64);
    let mut unresolved: BTreeMap<String, u64> = BTreeMap::new();
    for doc in &parsed_docs {
        for e in &doc.entities {
            for set in &e.property_sets {
                for p in &set.properties {
                    if p.type_name == "eCTemplateEntityProxy"
                        && let Some(g) = p.value.as_ref_guid()
                    {
                        refs_total += 1;
                        if catalog.name_of(g).is_some() {
                            refs_resolved += 1;
                        } else {
                            *unresolved
                                .entry(format!("{}.{}", set.class, p.name))
                                .or_default() += 1;
                        }
                    }
                }
                if let gothic_formats::props::tails::Tail::Inventory { stacks, .. } = &set.tail {
                    for st in stacks {
                        if let Some(g) = &st.template {
                            refs_total += 1;
                            if catalog.name_of(g).is_some() {
                                refs_resolved += 1;
                            } else {
                                *unresolved.entry("inventory stack".into()).or_default() += 1;
                            }
                        }
                    }
                }
            }
        }
    }
    let mut report = json!({
        "template_reference_resolution": {
            "references": refs_total, "resolved": refs_resolved, "unresolved_by_source": unresolved,
        },
        "templates": {
            "files": total, "parsed": ok,
            "errors": errors, "error_examples": error_examples,
            "tail_error_examples": tail_examples,
            "stats": stats_json(&tstats),
        }
    });
    if opts.flags.contains_key("full") {
        let mut wstats = DecodeStats::default();
        let (mut docs, mut parsed, mut ents, mut sets) = (0usize, 0usize, 0usize, 0usize);
        let mut werrors: BTreeMap<String, u64> = BTreeMap::new();
        let slots: Vec<_> = vfs
            .iter()
            .filter(|(p, _)| {
                let l = p.to_ascii_lowercase();
                l.ends_with(".node") || l.ends_with(".lrentdat")
            })
            .map(|(p, s)| (p.to_owned(), s))
            .collect();
        for (_, slot) in slots {
            docs += 1;
            let bytes = vfs.read(slot)?;
            match WorldDocument::parse_with_properties(&bytes, &|_| true, &mut wstats) {
                Ok((doc, s)) => {
                    parsed += 1;
                    ents += doc.entities.len();
                    sets += s.iter().map(Vec::len).sum::<usize>();
                }
                Err(e) => {
                    let key: String = format!("{e:#}").chars().take(100).collect();
                    *werrors.entry(key).or_default() += 1;
                }
            }
        }
        report["worlds"] = json!({
            "documents": docs, "parsed": parsed, "entities": ents, "property_sets": sets,
            "errors": werrors, "stats": stats_json(&wstats),
        });
    }
    emit(&report, opts)
}

/// `tple-dump DATA_DIR SUBSTRING [--max N]`: full decoded JSON of templates whose path or entity
/// name contains SUBSTRING (case-insensitive).
fn tple_dump(opts: &Opts) -> Result<()> {
    let usage = "tple-dump DATA_DIR SUBSTRING [--max N] [--out F]";
    let dir = opts.positional.first().context(usage)?;
    let needle = opts.positional.get(1).context(usage)?.to_ascii_lowercase();
    let max: usize = opts.flags.get("max").map_or(Ok(3), |v| v.parse())?;
    let mut vfs = VirtualFileSystem::open_data_dir(Path::new(dir))?;
    let slots: Vec<_> = vfs
        .iter()
        .filter(|(p, _)| p.to_ascii_lowercase().ends_with(".tple"))
        .map(|(p, s)| (p.to_owned(), s))
        .collect();
    let mut out = Vec::new();
    for (path, slot) in slots {
        if out.len() >= max {
            break;
        }
        let bytes = vfs.read(slot)?;
        let mut stats = DecodeStats::default();
        let Ok(doc) = TemplateDocument::parse(&bytes, &mut stats) else {
            continue;
        };
        let hit = path.to_ascii_lowercase().contains(&needle)
            || doc
                .entities
                .iter()
                .any(|e| e.name.to_ascii_lowercase() == needle);
        if hit {
            out.push(json!({"path": path, "document": doc}));
        }
    }
    emit(&json!(out), opts)
}

/// `world-props DATA_DIR PATH_SUBSTRING [--class C] [--max N] [--out F]`: decoded property sets of
/// entities in matching world documents (optionally only entities having class C).
fn world_props(opts: &Opts) -> Result<()> {
    let usage = "world-props DATA_DIR PATH_SUBSTRING [--class C] [--max N] [--out F]";
    let dir = opts.positional.first().context(usage)?;
    let needle = opts.positional.get(1).context(usage)?.to_ascii_lowercase();
    let class = opts.flags.get("class");
    let max: usize = opts.flags.get("max").map_or(Ok(5), |v| v.parse())?;
    let mut vfs = VirtualFileSystem::open_data_dir(Path::new(dir))?;
    let slots: Vec<_> = vfs
        .iter()
        .filter(|(p, _)| {
            let l = p.to_ascii_lowercase();
            (l.ends_with(".node") || l.ends_with(".lrentdat")) && l.contains(&needle)
        })
        .map(|(p, s)| (p.to_owned(), s))
        .collect();
    let mut out = Vec::new();
    'outer: for (path, slot) in slots {
        let bytes = vfs.read(slot)?;
        let mut stats = DecodeStats::default();
        let Ok((doc, sets)) = WorldDocument::parse_with_properties(&bytes, &|_| true, &mut stats)
        else {
            continue;
        };
        for (e, s) in doc.entities.iter().zip(sets) {
            if class.is_some_and(|c| !s.iter().any(|x| &x.class == c)) {
                continue;
            }
            out.push(
                json!({"path": path, "name": e.name, "world_matrix": e.world_matrix, "sets": s}),
            );
            if out.len() >= max {
                break 'outer;
            }
        }
    }
    emit(&json!(out), opts)
}

/// Parse every visible template; returns (path, document) pairs plus the GUID catalog.
fn load_templates(
    vfs: &mut VirtualFileSystem,
) -> Result<(Vec<(String, TemplateDocument)>, TemplateCatalog)> {
    let slots: Vec<_> = vfs
        .iter()
        .filter(|(p, _)| p.to_ascii_lowercase().ends_with(".tple"))
        .map(|(p, s)| (p.to_owned(), s))
        .collect();
    let mut docs = Vec::new();
    let mut catalog = TemplateCatalog::default();
    let mut stats = DecodeStats::default();
    for (path, slot) in slots {
        let bytes = vfs.read(slot)?;
        if let Ok(doc) = TemplateDocument::parse(&bytes, &mut stats) {
            catalog.insert(&path, &doc);
            docs.push((path, doc));
        }
    }
    Ok((docs, catalog))
}

/// English string table from `<data>/Strings.pak`, if present.
fn load_names(dir: &str) -> Option<gothic_formats::stringtable::StringTable> {
    let mut pak = gothic_formats::PakArchive::open(Path::new(dir).join("Strings.pak")).ok()?;
    let i = pak
        .entries
        .iter()
        .position(|e| e.path.eq_ignore_ascii_case("stringtable.ini"))?;
    gothic_formats::stringtable::StringTable::parse(&pak.read_payload(i).ok()?).ok()
}

/// Add `display_name` / `description` (English) from `FO_<name>` / `ITEMDESC_<name>`.
fn add_display(
    values: &mut [Value],
    table: &gothic_formats::stringtable::StringTable,
    with_description: bool,
) {
    for v in values {
        let Some(name) = v["name"].as_str().map(str::to_owned) else {
            continue;
        };
        if let Some(t) = table
            .text(&format!("FO_{name}"), "English")
            .filter(|t| !t.is_empty())
        {
            v["display_name"] = json!(gothic_formats::stringtable::unescape(t));
        }
        if with_description
            && let Some(t) = table
                .text(&format!("ITEMDESC_{name}"), "English")
                .filter(|t| !t.is_empty() && *t != "-")
        {
            v["description"] = json!(gothic_formats::stringtable::unescape(t));
        }
    }
}

fn is_live(path: &str) -> bool {
    !path.starts_with("_deleted/")
}

fn write_json(value: &Value, opts: &Opts, default: &str) -> Result<()> {
    let out = opts.flags.get("out").map_or(default, String::as_str);
    std::fs::write(out, serde_json::to_vec_pretty(value)?)?;
    eprintln!("wrote {out}");
    Ok(())
}

/// `item-dump DATA_DIR [--name SUBSTR] [--out F]` (default `.local/items.json`).
fn item_dump(opts: &Opts) -> Result<()> {
    let usage = "item-dump DATA_DIR [--name SUBSTR] [--out F]";
    let dir = opts.positional.first().context(usage)?;
    let mut vfs = VirtualFileSystem::open_data_dir(Path::new(dir))?;
    let (docs, catalog) = load_templates(&mut vfs)?;
    let mut items: Vec<ItemTemplate> = docs
        .iter()
        .filter(|(p, _)| is_live(p))
        .filter_map(|(p, d)| ItemTemplate::from_document(p, d, &catalog))
        .collect();
    items.sort_by(|a, b| {
        a.name
            .to_ascii_lowercase()
            .cmp(&b.name.to_ascii_lowercase())
    });
    let mut by_category: BTreeMap<String, usize> = BTreeMap::new();
    for i in &items {
        *by_category.entry(i.category.clone()).or_default() += 1;
    }
    eprintln!(
        "{} item templates; by category: {by_category:?}",
        items.len()
    );
    if let Some(name) = opts.flags.get("name") {
        let needle = name.to_ascii_lowercase();
        let hits: Vec<_> = items
            .iter()
            .filter(|i| i.name.to_ascii_lowercase().contains(&needle))
            .take(20)
            .collect();
        println!("{}", serde_json::to_string_pretty(&hits)?);
        return Ok(());
    }
    let mut values = serde_json::to_value(&items)?;
    if let (Some(table), Some(list)) = (load_names(dir), values.as_array_mut()) {
        add_display(list, &table, true);
    }
    write_json(&values, opts, ".local/items.json")
}

/// `npc-dump DATA_DIR [--name SUBSTR] [--doc SUBSTR] [--out F]` (default `.local/npcs.json`):
/// NPC templates from `Templates.pak` and NPC entities from all world documents.
fn npc_dump(opts: &Opts) -> Result<()> {
    let usage = "npc-dump DATA_DIR [--name SUBSTR] [--doc SUBSTR] [--out F]";
    let dir = opts.positional.first().context(usage)?;
    let mut vfs = VirtualFileSystem::open_data_dir(Path::new(dir))?;
    let (docs, catalog) = load_templates(&mut vfs)?;
    let mut templates: Vec<NpcTemplate> = docs
        .iter()
        .filter(|(p, _)| is_live(p))
        .filter_map(|(p, d)| NpcTemplate::from_document(p, d, &catalog))
        .collect();
    templates.sort_by(|a, b| {
        a.name
            .to_ascii_lowercase()
            .cmp(&b.name.to_ascii_lowercase())
    });
    const NPC_CLASSES: &[&str] = &[
        "gCNPC_PS",
        "gCInventory_PS",
        "gCNavigation_PS",
        "gCScriptRoutine_PS",
        "eCVisualAnimation_PS",
        "gCDamage_PS",
        "gCDamageReceiver_PS",
        "gCDialog_PS",
    ];
    let doc_filter = opts.flags.get("doc").map(|s| s.to_ascii_lowercase());
    let slots: Vec<_> = vfs
        .iter()
        .filter(|(p, _)| {
            let l = p.to_ascii_lowercase();
            (l.ends_with(".node") || l.ends_with(".lrentdat"))
                && doc_filter.as_ref().is_none_or(|f| l.contains(f))
        })
        .map(|(p, s)| (p.to_owned(), s))
        .collect();
    let mut entities: Vec<NpcEntity> = Vec::new();
    let mut stats = DecodeStats::default();
    for (path, slot) in slots {
        let bytes = vfs.read(slot)?;
        let Ok((doc, sets)) =
            WorldDocument::parse_with_properties(&bytes, &|c| NPC_CLASSES.contains(&c), &mut stats)
        else {
            continue;
        };
        for (i, (e, s)) in doc.entities.iter().zip(sets).enumerate() {
            if let Some(n) = NpcEntity::from_world(&path, i, e, &s, &catalog) {
                entities.push(n);
            }
        }
    }
    eprintln!(
        "{} NPC templates, {} world NPC entities in {} documents",
        templates.len(),
        entities.len(),
        entities
            .iter()
            .map(|e| &e.document)
            .collect::<std::collections::BTreeSet<_>>()
            .len()
    );
    if let Some(name) = opts.flags.get("name") {
        let needle = name.to_ascii_lowercase();
        let t: Vec<_> = templates
            .iter()
            .filter(|n| n.name.to_ascii_lowercase().contains(&needle))
            .take(5)
            .collect();
        let e: Vec<_> = entities
            .iter()
            .filter(|n| n.name.to_ascii_lowercase().contains(&needle))
            .take(5)
            .collect();
        println!(
            "{}",
            serde_json::to_string_pretty(&json!({"templates": t, "entities": e}))?
        );
        return Ok(());
    }
    let mut tv = serde_json::to_value(&templates)?;
    let mut ev = serde_json::to_value(&entities)?;
    if let Some(table) = load_names(dir) {
        if let Some(l) = tv.as_array_mut() {
            add_display(l, &table, false);
        }
        if let Some(l) = ev.as_array_mut() {
            add_display(l, &table, false);
        }
    }
    write_json(
        &json!({"templates": tv, "entities": ev}),
        opts,
        ".local/npcs.json",
    )
}

/// `skill-dump DATA_DIR [--out F]` (default `.local/skills.json`).
fn skill_dump(opts: &Opts) -> Result<()> {
    let usage = "skill-dump DATA_DIR [--out F]";
    let dir = opts.positional.first().context(usage)?;
    let mut vfs = VirtualFileSystem::open_data_dir(Path::new(dir))?;
    let (docs, catalog) = load_templates(&mut vfs)?;
    let mut skills: Vec<SkillTemplate> = docs
        .iter()
        .filter(|(p, _)| is_live(p))
        .filter_map(|(p, d)| SkillTemplate::from_document(p, d, &catalog))
        .collect();
    skills.sort_by(|a, b| {
        a.name
            .to_ascii_lowercase()
            .cmp(&b.name.to_ascii_lowercase())
    });
    eprintln!("{} skill templates", skills.len());
    write_json(&serde_json::to_value(&skills)?, opts, ".local/skills.json")
}
