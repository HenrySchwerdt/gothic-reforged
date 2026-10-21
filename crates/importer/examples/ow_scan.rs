//! `ow_scan DATA_DIR`: property-set class histogram over all world documents and the
//! documents that carry trigger classes (development tool, output only counts and names).
use anyhow::Result;
use gothic_formats::vfs::VirtualFileSystem;
use gothic_formats::world_node::WorldDocument;
use std::collections::BTreeMap;
use std::path::Path;

fn main() -> Result<()> {
    let dir = std::env::args().nth(1).expect("DATA_DIR");
    let mut vfs = VirtualFileSystem::open_data_dir(Path::new(&dir))?;
    let slots: Vec<_> = vfs
        .iter()
        .filter(|(p, _)| {
            let l = p.to_ascii_lowercase();
            l.ends_with(".node") || l.ends_with(".lrentdat")
        })
        .map(|(p, s)| (p.to_owned(), s))
        .collect();
    let mut classes: BTreeMap<String, usize> = BTreeMap::new();
    let mut docs: BTreeMap<String, BTreeMap<String, usize>> = BTreeMap::new();
    for (path, slot) in slots {
        let Ok(bytes) = vfs.read(slot) else { continue };
        let Ok(doc) = WorldDocument::parse(&bytes) else {
            continue;
        };
        for e in &doc.entities {
            for c in &e.property_sets {
                *classes.entry(c.clone()).or_default() += 1;
                if c.contains("Trigger") || c.contains("EventScript") || c.contains("EventFilter") {
                    *docs
                        .entry(path.clone())
                        .or_default()
                        .entry(c.clone())
                        .or_default() += 1;
                }
            }
        }
    }
    for (c, n) in &classes {
        println!("{n:7} {c}");
    }
    println!("--- documents with trigger classes");
    for (d, m) in &docs {
        println!("{d}: {m:?}");
    }
    Ok(())
}
