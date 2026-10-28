//! Play-mode data dumps: `object-dump` (interact objects, loose items, chests and doors around
//! Ardea), `treasure-dump` (`TS_*` treasure set templates) and `recipe-dump` (`gCRecipe_PS`
//! templates). Outputs go to the gitignored `.local/`.
use crate::gamedata::{Opts, parse_opts};
use anyhow::{Context, Result};
use gothic_formats::{
    props::{enums, records::TemplateCatalog, tails::Tail},
    propstream::{DecodeStats, PropValue, PropertySet},
    template::TemplateDocument,
    vfs::VirtualFileSystem,
    world_node::WorldDocument,
};
use serde_json::{Map, Value, json};
use std::{ffi::OsString, path::Path};

pub fn dispatch(command: &str, args: &[OsString]) -> Result<bool> {
    let opts = parse_opts(args)?;
    match command {
        "object-dump" => object_dump(&opts)?,
        "treasure-dump" => treasure_dump(&opts)?,
        "recipe-dump" => recipe_dump(&opts)?,
        _ => return Ok(false),
    }
    Ok(true)
}

/// Compact JSON of a property set: enums by name, entity references by template name.
fn simplify(set: &PropertySet, catalog: &TemplateCatalog) -> Value {
    let mut m = Map::new();
    for p in &set.properties {
        let v = match &p.value {
            PropValue::Bool(b) => json!(b),
            PropValue::I32(i) => json!(i),
            PropValue::U32(i) => json!(i),
            PropValue::I16(i) => json!(i),
            PropValue::U16(i) => json!(i),
            PropValue::Char(i) => json!(i),
            PropValue::F32(f) => json!(f),
            PropValue::Str(s) => json!(s),
            PropValue::Enum { ty, value } => {
                json!(enums::name(ty, *value).map_or_else(|| format!("#{value}"), str::to_owned))
            }
            PropValue::Vec3(v) => json!(v),
            PropValue::EntityRef(Some(g)) => json!(catalog.resolve(g)),
            PropValue::StrArray(v) => json!(v),
            PropValue::EntityRefArray(v) => {
                json!(
                    v.iter()
                        .flatten()
                        .map(|g| catalog.resolve(g))
                        .collect::<Vec<_>>()
                )
            }
            _ => continue,
        };
        m.insert(p.name.clone(), v);
    }
    if let Tail::Inventory { stacks, .. } = &set.tail {
        let list: Vec<Value> = stacks
            .iter()
            .map(|s| {
                json!({
                    "item": s.template.as_deref().map_or_else(|| "?".to_owned(), |g| catalog.resolve(g)),
                    "amount": s.amount,
                    "quality": s.quality,
                    "stack_type": enums::name("gEStackType", s.generated),
                    "min_amount": s.set.int("MinAmount"),
                    "max_amount": s.set.int("MaxAmount"),
                    "slot": s.set.get("Slot").and_then(PropValue::as_i32)
                        .and_then(|v| enums::name("gESlot", v)),
                })
            })
            .collect();
        m.insert("stacks".into(), Value::Array(list));
    }
    Value::Object(m)
}

type Templates = Vec<(String, TemplateDocument)>;

fn load_catalog(vfs: &mut VirtualFileSystem) -> Result<(Templates, TemplateCatalog)> {
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

fn names_table(dir: &str) -> Option<gothic_formats::stringtable::StringTable> {
    let mut pak = gothic_formats::PakArchive::open(Path::new(dir).join("Strings.pak")).ok()?;
    let i = pak
        .entries
        .iter()
        .position(|e| e.path.eq_ignore_ascii_case("stringtable.ini"))?;
    gothic_formats::stringtable::StringTable::parse(&pak.read_payload(i).ok()?).ok()
}

fn write(value: &Value, opts: &Opts, default: &str) -> Result<()> {
    let out = opts.flags.get("out").map_or(default, String::as_str);
    std::fs::write(out, serde_json::to_vec_pretty(value)?)?;
    eprintln!("wrote {out}");
    Ok(())
}

/// The identity (first 16 bytes, 32 hex digits) of every entity a character of the world refers to
/// as a routine point: `gCNavigation_PS.WorkingPoints / RelaxingPoints / SleepingPoints`. The engine
/// resolves such a reference by the 16-byte GUID only (`bCPropertyID::operator==` 0x1000ea00, see
/// `gothic_session::guid`); the entities can be of any class (waypoints, patrol and guard spots,
/// free points), so the interactive-class filter of `object-dump` would drop most of them.
fn routine_point_ids(
    vfs: &mut VirtualFileSystem,
    slots: &[(String, gothic_formats::vfs::Slot)],
) -> Result<std::collections::HashMap<String, String>> {
    let mut ids = std::collections::HashMap::new();
    for (_, slot) in slots {
        let bytes = vfs.read(*slot)?;
        let mut stats = DecodeStats::default();
        let Ok((_, sets)) = WorldDocument::parse_with_properties(
            &bytes,
            &|c| c == "gCNavigation_PS",
            &mut stats,
        ) else {
            continue;
        };
        for set in sets.iter().flatten() {
            for p in &set.properties {
                if !matches!(
                    p.name.as_str(),
                    "WorkingPoints"
                        | "RelaxingPoints"
                        | "SleepingPoints"
                        | "WorkingPoint"
                        | "RelaxingPoint"
                        | "SleepingPoint"
                ) {
                    continue;
                }
                match &p.value {
                    PropValue::IdArray(v) => {
                        for g in v {
                            ids.entry(g.chars().take(32).collect::<String>())
                                .or_insert_with(|| g.clone());
                        }
                    }
                    PropValue::Id(g) => {
                        ids.entry(g.chars().take(32).collect::<String>())
                            .or_insert_with(|| g.clone());
                    }
                    _ => {}
                }
            }
        }
    }
    Ok(ids)
}

/// `object-dump DATA_DIR [--cx X --cz Z --radius R] [--out F]` (default `.local/objects.json`).
fn object_dump(opts: &Opts) -> Result<()> {
    let usage = "object-dump DATA_DIR [--cx X] [--cz Z] [--radius R] [--out F]";
    let dir = opts.positional.first().context(usage)?;
    let num =
        |k: &str, d: f32| -> f32 { opts.flags.get(k).and_then(|v| v.parse().ok()).unwrap_or(d) };
    let (cx, cz, radius) = (
        num("cx", 91_000.0),
        num("cz", -11_000.0),
        num("radius", 7_000.0),
    );
    let mut vfs = VirtualFileSystem::open_data_dir(Path::new(dir))?;
    let (_, catalog) = load_catalog(&mut vfs)?;
    const CLASSES: &[&str] = &[
        "gCInteraction_PS",
        "gCLock_PS",
        "gCDoor_PS",
        "gCItem_PS",
        "gCInventory_PS",
        "gCNPC_PS",
        "gCMover_PS",
        "gCAIHelper_FreePoint_PS",
        "gCAIHelper_Label_PS",
        "gCBook_PS",
        "gCLetter_PS",
        "gCRecipe_PS",
    ];
    let slots: Vec<_> = vfs
        .iter()
        .filter(|(p, _)| {
            let l = p.to_ascii_lowercase();
            l.ends_with(".node") || l.ends_with(".lrentdat")
        })
        .map(|(p, s)| (p.to_owned(), s))
        .collect();
    let names = names_table(dir);
    let routine_ids = routine_point_ids(&mut vfs, &slots)?;
    eprintln!("{} routine point identities referenced by characters", routine_ids.len());
    let mut routine_only = 0usize;
    let mut out = Vec::new();
    for (path, slot) in slots {
        let bytes = vfs.read(slot)?;
        let mut stats = DecodeStats::default();
        let Ok((doc, sets)) =
            WorldDocument::parse_with_properties(&bytes, &|c| CLASSES.contains(&c), &mut stats)
        else {
            continue;
        };
        for (i, (e, s)) in doc.entities.iter().zip(sets).enumerate() {
            let m = e.world_matrix;
            let (x, y, z) = (m[3][0], m[3][1], m[3][2]);
            if (x - cx).hypot(z - cz) > radius {
                continue;
            }
            // NPCs are handled by npc-dump; keep everything else that is interactive, and every
            // entity a character uses as a routine point whatever its class.
            let id_hex = gothic_formats::propstream::hex(&e.source_id);
            let routine_point =
                routine_ids.contains_key(&id_hex.chars().take(32).collect::<String>());
            if s.iter().any(|p| p.class == "gCNPC_PS") || (s.is_empty() && !routine_point) {
                continue;
            }
            if s.is_empty() {
                routine_only += 1;
            }
            let mut sets_json = Map::new();
            for p in &s {
                sets_json.insert(p.class.clone(), simplify(p, &catalog));
            }
            let display = |lang: &str| {
                names.as_ref().and_then(|t| {
                    t.text(&format!("FO_{}", e.name), lang)
                        .filter(|x| !x.is_empty())
                        .map(gothic_formats::stringtable::unescape)
                })
            };
            out.push(json!({
                "name": e.name,
                "id": id_hex,
                "routine_point": routine_point,
                "classes": e.property_sets,
                "display_name": display("German").or_else(|| display("English")),
                "document": path,
                "index": i,
                "position": [x, y, z],
                "matrix": m,
                "assets": e.asset_references,
                "sets": sets_json,
            }));
        }
    }
    eprintln!(
        "{} interactive world entities ({routine_only} of them only routine points)",
        out.len()
    );
    // References that no world document answers: templates of the game data (the default points
    // every character without a point of its own refers to).
    let found: std::collections::HashSet<String> = out
        .iter()
        .filter_map(|o| o["id"].as_str().map(|i| i.chars().take(32).collect()))
        .collect();
    let mut unresolved: Vec<Value> = routine_ids
        .iter()
        .filter(|(k, _)| !found.contains(*k))
        .map(|(k, full)| {
            json!({ "id": full, "key": k, "template": catalog.name_of(full).or_else(|| catalog.name_of(k)), "path": catalog.path_of(full).or_else(|| catalog.path_of(k)) })
        })
        .collect();
    unresolved.sort_by_key(|v| v["id"].as_str().unwrap_or("").to_owned());
    eprintln!("{} referenced routine points are in no world document", unresolved.len());
    write(
        &json!({ "objects": out, "unresolved_routine_points": unresolved }),
        opts,
        ".local/objects.json",
    )
}

/// `treasure-dump DATA_DIR [--out F]` (default `.local/treasure.json`): every `TS_*` template
/// with its distribution and stacks.
fn treasure_dump(opts: &Opts) -> Result<()> {
    let dir = opts
        .positional
        .first()
        .context("treasure-dump DATA_DIR [--out F]")?;
    let mut vfs = VirtualFileSystem::open_data_dir(Path::new(dir))?;
    let (docs, catalog) = load_catalog(&mut vfs)?;
    let mut sets = Vec::new();
    for (path, doc) in &docs {
        for e in &doc.entities {
            let Some(ts) = e.set("gCTreasureSet_PS") else {
                continue;
            };
            if !e.name.to_ascii_lowercase().starts_with("ts_") {
                continue;
            }
            let inv = e.set("gCInventory_PS").map(|s| simplify(s, &catalog));
            sets.push(json!({
                "name": e.name,
                "path": path,
                "treasure": simplify(ts, &catalog),
                "inventory": inv,
            }));
        }
    }
    eprintln!("{} treasure sets", sets.len());
    write(&json!({ "sets": sets }), opts, ".local/treasure.json")
}

/// `recipe-dump DATA_DIR [--out F]` (default `.local/recipes.json`): templates with `gCRecipe_PS`.
fn recipe_dump(opts: &Opts) -> Result<()> {
    let dir = opts
        .positional
        .first()
        .context("recipe-dump DATA_DIR [--out F]")?;
    let mut vfs = VirtualFileSystem::open_data_dir(Path::new(dir))?;
    let (docs, catalog) = load_catalog(&mut vfs)?;
    let mut out = Vec::new();
    for (path, doc) in &docs {
        for e in &doc.entities {
            if e.set("gCRecipe_PS").is_none() {
                continue;
            }
            let mut sets = Map::new();
            for p in &e.property_sets {
                if matches!(
                    p.class.as_str(),
                    "gCRecipe_PS" | "gCItem_PS" | "gCInventory_PS" | "gCInteraction_PS"
                ) {
                    sets.insert(p.class.clone(), simplify(p, &catalog));
                }
            }
            out.push(json!({ "name": e.name, "path": path, "sets": sets }));
        }
    }
    eprintln!("{} recipe templates", out.len());
    write(&json!({ "recipes": out }), opts, ".local/recipes.json")
}
