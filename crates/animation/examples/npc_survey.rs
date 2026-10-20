//! Headless survey of the NPC appearance chain: every world NPC entity (and optionally only a
//! document filter) is resolved through `appearance::resolve`; unresolved reasons and texture
//! problems are counted. Usage: npc_survey [DATA_DIR] [--doc SUBSTR] [--verbose]
use gothic_animation::{ActorLibrary, appearance};
use gothic_formats::{
    props::records::{NpcEntity, NpcLook, TemplateCatalog},
    propstream::DecodeStats,
    template::TemplateDocument,
    vfs::VirtualFileSystem,
    world_node::WorldDocument,
};
use std::{collections::BTreeMap, path::Path};

fn main() -> anyhow::Result<()> {
    let mut dir = r"D:\SteamLibrary\steamapps\common\Gothic 3\Data".to_string();
    let mut doc_filter = None;
    let mut verbose = false;
    let mut args = std::env::args().skip(1);
    while let Some(a) = args.next() {
        match a.as_str() {
            "--doc" => doc_filter = args.next().map(|s| s.to_ascii_lowercase()),
            "--verbose" => verbose = true,
            _ => dir = a,
        }
    }
    let mut vfs = VirtualFileSystem::open_data_dir(Path::new(&dir))?;
    let mut stats = DecodeStats::default();
    let mut catalog = TemplateCatalog::default();
    let tples: Vec<_> = vfs
        .iter()
        .filter(|(p, _)| p.to_ascii_lowercase().ends_with(".tple"))
        .map(|(p, s)| (p.to_owned(), s))
        .collect();
    for (path, slot) in tples {
        if let Ok(doc) = TemplateDocument::parse(&vfs.read(slot)?, &mut stats) {
            catalog.insert(&path, &doc);
        }
    }
    let worlds: Vec<_> = vfs
        .iter()
        .filter(|(p, _)| {
            let l = p.to_ascii_lowercase();
            (l.ends_with(".node") || l.ends_with(".lrentdat"))
                && doc_filter.as_ref().is_none_or(|f| l.contains(f))
        })
        .map(|(p, s)| (p.to_owned(), s))
        .collect();
    const CLASSES: &[&str] = &["gCNPC_PS", "gCInventory_PS", "eCVisualAnimation_PS"];
    let mut looks: Vec<(NpcLook, String)> = Vec::new();
    for (path, slot) in worlds {
        let Ok((doc, sets)) = WorldDocument::parse_with_properties(
            &vfs.read(slot)?,
            &|c| CLASSES.contains(&c),
            &mut stats,
        ) else {
            continue;
        };
        for (i, (e, s)) in doc.entities.iter().zip(sets).enumerate() {
            if let Some(n) = NpcEntity::from_world(&path, i, e, &s, &catalog) {
                looks.push((
                    NpcLook {
                        name: n.name,
                        document: n.document,
                        position: n.position,
                        species: n.data.species.clone(),
                        visual: n.data.visual,
                        inventory: gothic_formats::props::records::LookInventory {
                            equipped: n.data.inventory.equipped,
                        },
                    },
                    n.data.species,
                ));
            }
        }
    }
    let mut library = ActorLibrary::open(Path::new(&dir))?;
    let (mut ok, mut bad, mut tex_bad) = (0usize, 0usize, 0usize);
    let mut reasons: BTreeMap<String, usize> = BTreeMap::new();
    let mut tex: BTreeMap<String, usize> = BTreeMap::new();
    let mut missing: BTreeMap<String, usize> = BTreeMap::new();
    let mut seen = std::collections::HashSet::new();
    for (look, _) in &looks {
        // Items without any visual data are scripted objects, not characters.
        if look.visual.skeleton.is_none() && look.visual.parts.is_empty() {
            continue;
        }
        for m in appearance::missing_meshes(&library, look) {
            *missing.entry(m).or_default() += 1;
        }
        match appearance::resolve(&library, look) {
            Ok(spec) => {
                ok += 1;
                let mut problems = Vec::new();
                for part in &spec.parts {
                    if seen.insert(part.clone()) {
                        problems.extend(library.texture_problems(part));
                    }
                }
                if !problems.is_empty() {
                    tex_bad += 1;
                }
                for p in problems {
                    *tex.entry(p).or_default() += 1;
                }
            }
            Err(why) => {
                bad += 1;
                if verbose {
                    eprintln!("{} [{}]: {why}", look.name, look.document);
                }
                *reasons.entry(why).or_default() += 1;
            }
        }
    }
    println!(
        "NPC entities with visuals: {} resolved, {} unresolved",
        ok, bad
    );
    for (r, n) in &reasons {
        println!("  unresolved x{n}: {r}");
    }
    println!(
        "not-shipped equipment meshes (skipped): {}",
        missing.values().sum::<usize>()
    );
    for (m, n) in &missing {
        println!("  x{n} {m}");
    }
    println!("distinct texture problems (first sight, {tex_bad} NPCs affected):");
    for (p, n) in &tex {
        println!("  x{n} {p}");
    }
    Ok(())
}
