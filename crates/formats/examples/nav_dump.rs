//! Debug: dump the navigation property sets of world documents.
//! Usage: nav_dump DATA_DIR PATH_SUBSTR CLASS_SUBSTR [SAMPLES]
use gothic_formats::propstream::DecodeStats;
use gothic_formats::vfs::VirtualFileSystem;
use gothic_formats::world_node::WorldDocument;
use std::collections::BTreeMap;
use std::path::Path;
fn main() {
    let a: Vec<String> = std::env::args().collect();
    let mut vfs = VirtualFileSystem::open_data_dir(Path::new(&a[1])).unwrap();
    let f = a[2].to_ascii_lowercase();
    let class = a[3].clone();
    let limit: usize = a.get(4).and_then(|s| s.parse().ok()).unwrap_or(2);
    let slots: Vec<_> = vfs
        .iter()
        .filter(|(p, _)| {
            let l = p.to_ascii_lowercase();
            l.contains(&f) && (l.ends_with(".node") || l.ends_with(".lrentdat"))
        })
        .map(|(p, s)| (p.to_owned(), s))
        .collect();
    let mut counts: BTreeMap<String, usize> = BTreeMap::new();
    let mut shown = 0;
    for (path, slot) in slots {
        let bytes = vfs.read(slot).unwrap();
        let mut st = DecodeStats::default();
        let c = class.clone();
        let Ok((doc, sets)) =
            WorldDocument::parse_with_properties(&bytes, &move |k| k.contains(&c), &mut st)
        else {
            continue;
        };
        for (e, ps) in doc.entities.iter().zip(&sets) {
            for p in ps {
                *counts.entry(p.class.clone()).or_default() += 1;
                if shown < limit {
                    shown += 1;
                    println!("-- {path} :: {} [{}]", e.name, e.property_sets.join(","));
                    for pr in &p.properties {
                        let s = format!("{:?}", pr.value);
                        println!(
                            "   {} ({}) = {}",
                            pr.name,
                            pr.type_name,
                            &s[..s.len().min(260)]
                        );
                    }
                }
            }
        }
    }
    println!("{counts:?}");
}
