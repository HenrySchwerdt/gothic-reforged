//! `ow_props DATA_DIR CLASS [MAX]`: entities carrying a property-set class, with all decoded
//! properties of that class and of `eCCollisionShape_PS` (development tool).
use anyhow::Result;
use gothic_formats::propstream::DecodeStats;
use gothic_formats::vfs::VirtualFileSystem;
use gothic_formats::world_node::WorldDocument;
use std::collections::BTreeMap;
use std::path::Path;

fn main() -> Result<()> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let class = args[1].clone();
    let max: usize = args.get(2).and_then(|s| s.parse().ok()).unwrap_or(5);
    let mut vfs = VirtualFileSystem::open_data_dir(Path::new(&args[0]))?;
    let slots: Vec<_> = vfs
        .iter()
        .filter(|(p, _)| {
            let l = p.to_ascii_lowercase();
            l.ends_with(".node") || l.ends_with(".lrentdat")
        })
        .map(|(p, s)| (p.to_owned(), s))
        .collect();
    let mut per_doc: BTreeMap<String, usize> = BTreeMap::new();
    let mut shown = 0;
    let mut names: BTreeMap<String, usize> = BTreeMap::new();
    for (path, slot) in slots {
        let Ok(bytes) = vfs.read(slot) else { continue };
        let mut stats = DecodeStats::default();
        let want = class.clone();
        let Ok((doc, sets)) = WorldDocument::parse_with_properties(
            &bytes,
            &move |c| c == want || c == "eCCollisionShape_PS",
            &mut stats,
        ) else {
            continue;
        };
        for (i, e) in doc.entities.iter().enumerate() {
            let Some(set) = sets
                .get(i)
                .and_then(|s| s.iter().find(|s| s.class == class))
            else {
                continue;
            };
            *per_doc.entry(path.clone()).or_default() += 1;
            *names.entry(e.name.clone()).or_default() += 1;
            if shown < max {
                shown += 1;
                println!("== {} / {} at {:?}", path, e.name, e.world_matrix[3]);
                for s in &sets[i] {
                    println!("  [{}]", s.class);
                    for p in &s.properties {
                        println!("    {} = {:?}", p.name, p.value);
                    }
                }
            }
        }
    }
    println!("--- per document");
    for (d, n) in &per_doc {
        println!("{n:5} {d}");
    }
    println!("--- names");
    for (d, n) in names.iter().take(60) {
        println!("{n:5} {d}");
    }
    Ok(())
}
