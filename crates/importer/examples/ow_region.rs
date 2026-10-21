//! `ow_region DATA_DIR`: per-region entity / NPC-property-set counts of every `.lrentdat`
//! that parses, plus the table-less layers' size (development tool, prints counts only).
use anyhow::Result;
use gothic_formats::vfs::VirtualFileSystem;
use gothic_formats::world_node::WorldDocument;
use std::collections::BTreeMap;
use std::path::Path;

fn main() -> Result<()> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let mut vfs = VirtualFileSystem::open_data_dir(Path::new(&args[0]))?;
    let slots: Vec<_> = vfs
        .iter()
        .filter(|(p, _)| p.to_ascii_lowercase().ends_with(".lrentdat"))
        .map(|(p, s)| (p.to_owned(), s))
        .collect();
    // region = first two path components below G3_World_01
    let mut regions: BTreeMap<String, (usize, usize, usize, usize)> = BTreeMap::new();
    for (path, slot) in slots {
        let bytes = vfs.read(slot)?;
        let parts: Vec<&str> = path.split('/').collect();
        let region = parts.iter().skip(1).take(2).copied().collect::<Vec<_>>().join("/");
        let r = regions.entry(region).or_default();
        match WorldDocument::parse(&bytes) {
            Ok(doc) => {
                r.0 += 1;
                r.1 += doc.entities.len();
                r.2 += doc
                    .entities
                    .iter()
                    .filter(|e| e.property_sets.iter().any(|c| c == "gCNPC_PS"))
                    .count();
            }
            Err(_) => {
                r.3 += 1;
                println!("UNPARSED {} {} B", path, bytes.len());
            }
        }
    }
    println!("region: docs entities npc_ps failed");
    for (k, v) in regions {
        println!("{k}: {} {} {} {}", v.0, v.1, v.2, v.3);
    }
    Ok(())
}
